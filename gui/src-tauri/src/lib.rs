//! libremodele GUI backend (H6): Tauri commands over the library pipeline.

/// The app and library version, shown in the window.
#[tauri::command]
fn version() -> String {
    format!("libremodele {}", env!("CARGO_PKG_VERSION"))
}

/// Start the window.
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![version])
        .run(tauri::generate_context!())
        .expect("error while running the libremodele window");
}
