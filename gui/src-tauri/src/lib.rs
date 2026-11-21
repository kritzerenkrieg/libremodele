//! libremodele GUI backend (H6): Tauri commands over [`session::Session`],
//! with the slow steps on a worker thread so the window stays responsive.

pub mod session;
pub mod view;

use std::sync::{Arc, Mutex};

use session::{Form, Outcome, Session, Summary};
use tauri::ipc::Response;
use tauri::{Emitter, Manager};

type Shared = Arc<Mutex<Session>>;

/// Run `f` on the session off the main thread.
async fn on_worker<T: Send + 'static>(
    s: &Shared,
    f: impl FnOnce(&mut Session) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let s = s.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = s.lock().map_err(|e| e.to_string())?;
        f(&mut guard)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The app version, shown in the window.
#[tauri::command]
fn version() -> String {
    format!("libremodele {}", env!("CARGO_PKG_VERSION"))
}

/// Open a download and show what was chosen (H2).
#[tauri::command]
async fn inspect(form: Form, state: tauri::State<'_, Shared>) -> Result<Summary, String> {
    on_worker(&state, move |s| s.inspect(&form)).await
}

/// Convert into the app's data folder, emitting `progress` per stage.
#[tauri::command]
async fn convert(
    form: Form,
    app: tauri::AppHandle,
    state: tauri::State<'_, Shared>,
) -> Result<Outcome, String> {
    let out = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("out");
    on_worker(&state, move |s| {
        s.convert(&form, &out, &mut |stage| {
            let _ = app.emit("progress", stage);
        })
    })
    .await
}

/// Install the last conversion into Sven Co-op's addon folder (H2, H5).
#[tauri::command]
async fn install(keep_local: bool, state: tauri::State<'_, Shared>) -> Result<String, String> {
    let addon = libremodele::donor::addon_players()
        .ok_or("Sven Co-op wasn't found in the Steam libraries")?;
    on_worker(&state, move |s| {
        s.install(&addon, keep_local)
            .map(|p| p.display().to_string())
    })
    .await
}

/// The converted model (or the loaded weapon) for the 3D viewer (H4).
#[tauri::command]
async fn model_view(
    weapon: bool,
    state: tauri::State<'_, Shared>,
) -> Result<view::ModelView, String> {
    on_worker(&state, move |s| {
        let g = s.generation(weapon);
        s.model(weapon).map(|m| view::model_view(m, g))
    })
    .await
}

/// Texture `index` as raw RGBA bytes.
#[tauri::command]
async fn texture(
    weapon: bool,
    generation: u64,
    index: usize,
    state: tauri::State<'_, Shared>,
) -> Result<Response, String> {
    on_worker(&state, move |s| {
        view::texture_rgba(s.model_at(weapon, generation)?, index)
            .map(Response::new)
            .ok_or_else(|| format!("no texture {index}"))
    })
    .await
}

/// Bone transforms per frame (little-endian f32, 12 per bone) for one blend
/// of a sequence, or the rest pose when `sequence` is absent.
#[tauri::command]
async fn bone_frames(
    weapon: bool,
    generation: u64,
    sequence: Option<usize>,
    blend: usize,
    state: tauri::State<'_, Shared>,
) -> Result<Response, String> {
    on_worker(&state, move |s| {
        let m = s.model_at(weapon, generation)?;
        let v = view::locals(m, sequence.map(|q| (q, blend)));
        Ok(Response::new(
            v.iter().flat_map(|f| f.to_le_bytes()).collect::<Vec<u8>>(),
        ))
    })
    .await
}

/// Sven's `p_*.mdl` weapon models.
#[tauri::command]
fn weapons() -> Vec<String> {
    view::weapons()
}

/// Hold a weapon in the viewer (or none).
#[tauri::command]
async fn load_weapon(file: Option<String>, state: tauri::State<'_, Shared>) -> Result<(), String> {
    on_worker(&state, move |s| s.load_weapon(file.as_deref())).await
}

/// Start the window.
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Shared::default())
        .invoke_handler(tauri::generate_handler![
            version,
            inspect,
            convert,
            install,
            model_view,
            texture,
            bone_frames,
            weapons,
            load_weapon
        ])
        .run(tauri::generate_context!())
        .expect("error while running the libremodele window");
}
