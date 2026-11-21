//! Donor choice when `--donor` isn't given (G22): avatars are authored at
//! real scale, so a short one is chibi and gets a chibi donor; others get
//! Sven Co-op's stock `gina`, read from the local Steam install. The chibi
//! donor ships inside the app (A6, D7 amended 2026-11-21).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::mdl::{self, Model};
use crate::vrm::Avatar;

/// Real-scale height (metres) below which an avatar is chibi (G22).
pub const CHIBI_HEIGHT: f32 = 1.4;

/// The built-in chibi donor's name: Yuki's `yuki_muumuyu` with TapChan's
/// Sven Co-op animations, shipped with permission (A6, D7 amended 2026-11-21).
pub const CHIBI_DONOR: &str = "<built-in>/yuki_muumuyu.mdl";

static CHIBI_BYTES: &[u8] = include_bytes!("../donors/yuki_muumuyu.mdl");

/// Read a donor: the built-in chibi donor by its name, else a file.
pub fn read(path: &Path) -> Result<Model> {
    if path == Path::new(CHIBI_DONOR) {
        return mdl::read(CHIBI_BYTES).context("parsing the built-in chibi donor");
    }
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    mdl::read(&bytes).with_context(|| format!("parsing {}", path.display()))
}

/// The stock donor inside a Sven Co-op install.
const STOCK_DONOR: &str = "svencoop/models/player/gina/gina.mdl";

/// The avatar's height in metres, over all its parts (y up).
pub fn height(av: &Avatar) -> f32 {
    let (lo, hi) = av
        .parts
        .iter()
        .flat_map(|p| &p.positions)
        .fold((f32::MAX, f32::MIN), |(l, h), p| (l.min(p[1]), h.max(p[1])));
    if hi > lo { hi - lo } else { 0.0 }
}

/// Steam's install folder from the registry (`HKCU\Software\Valve\Steam`,
/// `SteamPath`), read with the system's `reg` tool.
#[cfg(windows)]
fn windows_steam_path() -> Option<PathBuf> {
    let out = std::process::Command::new("reg")
        .args(["query", r"HKCU\Software\Valve\Steam", "/v", "SteamPath"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text
        .lines()
        .find(|l| l.trim_start().starts_with("SteamPath"))?;
    // `SteamPath    REG_SZ    <path>`: split at the first marker only, so a
    // path that itself contains "REG_SZ" stays whole.
    let value = line.split_once("REG_SZ")?.1.trim();
    (!value.is_empty()).then(|| PathBuf::from(value.replace('/', "\\")))
}

/// Steam library roots that may hold Sven Co-op.
fn steam_libraries() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        for r in [
            ".local/share/Steam",
            ".steam/steam",
            ".var/app/com.valvesoftware.Steam/.local/share/Steam",
            "Library/Application Support/Steam",
        ] {
            roots.push(home.join(r));
        }
    }
    // Windows: Steam records where it is installed (often not on C:).
    #[cfg(windows)]
    if let Some(p) = windows_steam_path() {
        roots.push(p);
    }
    for r in [r"C:\Program Files (x86)\Steam", r"C:\Program Files\Steam"] {
        roots.push(PathBuf::from(r));
    }
    let mut libs = roots.clone();
    // Other libraries are listed as `"path" "<dir>"` in libraryfolders.vdf.
    for r in &roots {
        let Ok(t) = std::fs::read_to_string(r.join("steamapps/libraryfolders.vdf")) else {
            continue;
        };
        for line in t.lines() {
            let f: Vec<&str> = line.split('"').collect();
            if f.len() >= 4 && f[1] == "path" {
                libs.push(PathBuf::from(f[3].replace(r"\\", r"\")));
            }
        }
    }
    libs
}

/// The first Sven Co-op install found in the Steam libraries.
pub fn sven_root() -> Option<PathBuf> {
    steam_libraries()
        .into_iter()
        .map(|l| l.join("steamapps/common/Sven Co-op"))
        .find(|p| p.join(STOCK_DONOR).is_file())
}

/// The stock donor in the first Steam library that has Sven Co-op.
pub fn stock() -> Option<PathBuf> {
    sven_root().map(|r| r.join(STOCK_DONOR))
}

/// Sven Co-op's addon player-model folder, where installed models go (H5).
pub fn addon_players() -> Option<PathBuf> {
    sven_root().map(|r| r.join("svencoop_addon/models/player"))
}

/// The donor path for `av`, and why (G22).
pub fn choose(av: &Avatar) -> Result<(PathBuf, String)> {
    let h = height(av);
    if h < CHIBI_HEIGHT {
        Ok((
            PathBuf::from(CHIBI_DONOR),
            format!("chibi: {h:.2} m is under {CHIBI_HEIGHT} m"),
        ))
    } else {
        match stock() {
            Some(p) => Ok((
                p,
                format!("normal proportions: {h:.2} m is {CHIBI_HEIGHT} m or more"),
            )),
            None => bail!(
                "the avatar is {h:.2} m tall, so normal proportions, but no Sven Co-op install with {STOCK_DONOR} was found; give --donor <donor.mdl> (G22)"
            ),
        }
    }
}
