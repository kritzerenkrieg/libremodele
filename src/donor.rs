//! Donor choice when `--donor` isn't given (G22): avatars are authored at
//! real scale, so a short one is chibi and gets a chibi donor; others get
//! Sven Co-op's stock `gina`, read from the local Steam install.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

use crate::vrm::Avatar;

/// Real-scale height (metres) below which an avatar is chibi (G22).
pub const CHIBI_HEIGHT: f32 = 1.4;

/// The chibi donor, relative to the working directory (A6: a local file).
pub const CHIBI_DONOR: &str = "samples/yuki_ref/models/player/yuki_muumuyu/yuki_muumuyu.mdl";

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

/// The stock donor in the first Steam library that has Sven Co-op.
pub fn stock() -> Option<PathBuf> {
    steam_libraries()
        .into_iter()
        .map(|l| l.join("steamapps/common/Sven Co-op").join(STOCK_DONOR))
        .find(|p| p.is_file())
}

/// The donor path for `av`, and why (G22).
pub fn choose(av: &Avatar) -> Result<(PathBuf, String)> {
    let h = height(av);
    if h < CHIBI_HEIGHT {
        let p = Path::new(CHIBI_DONOR);
        if !p.is_file() {
            bail!(
                "the avatar is {h:.2} m tall, so chibi (under {CHIBI_HEIGHT} m), but the chibi donor {CHIBI_DONOR} is missing; give --donor <donor.mdl> (G22)"
            );
        }
        Ok((
            p.into(),
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
