//! What to convert, chosen from a download with no hand choices (A15, G16):
//! a zip, a folder, or a single `.vrm`/`.fbx` (whose surrounding folder is
//! searched too). Avatar files are ranked, the licence text is found next to
//! them (G15), outfit-only inputs are refused (G17), and every choice, with
//! the runners-up, goes to the report.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};

use crate::unitypkg::Package;
use crate::vrm::Avatar;
use crate::{fbx, vrm};

/// Files of a download: (path with `/` separators, bytes), sorted by path.
pub struct Files {
    pub entries: Vec<(String, Vec<u8>)>,
}

/// Files larger than this are skipped when scanning folders (videos, .blend
/// backups); avatar inputs are far smaller.
const MAX_FILE: u64 = 200 << 20;

impl Files {
    /// A zip, a folder (recursive), or one file plus the folder two levels
    /// up, where downloads keep textures and licences beside the model; then
    /// the file's path inside the result is returned too, as the avatar.
    pub fn open(path: &Path) -> Result<(Files, Option<String>)> {
        let mut file = None;
        let lower = path.to_string_lossy().to_lowercase();
        let mut entries = if path.is_dir() {
            scan(path, path, 6)?
        } else if lower.ends_with(".zip") {
            unzip(&std::fs::read(path).with_context(|| format!("reading {}", path.display()))?)?
        } else {
            let root = path
                .parent()
                .and_then(Path::parent)
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            let mut e = scan(root, root, 3)?;
            let rel = rel_path(root, path);
            if !e.iter().any(|(p, _)| *p == rel) {
                e.push((rel.clone(), std::fs::read(path)?));
            }
            file = Some(rel);
            e
        };
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        Ok((Files { entries }, file))
    }

    pub fn get(&self, path: &str) -> Option<&[u8]> {
        self.entries
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, d)| d.as_slice())
    }

    /// A file by name (ignoring case), nearest to `from` first: the one
    /// sharing the longest folder prefix.
    pub fn near(&self, from: &str, name: &str) -> Option<&[u8]> {
        self.entries
            .iter()
            .filter(|(p, _)| base(p).eq_ignore_ascii_case(name))
            .max_by_key(|(p, _)| (shared(from, p), std::cmp::Reverse(p.len())))
            .map(|(_, d)| d.as_slice())
    }
}

/// The path of `path` relative to the scan root, as the zip reader names it.
pub fn rel_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn scan(root: &Path, dir: &Path, depth: usize) -> Result<Vec<(String, Vec<u8>)>> {
    let mut out = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .flatten()
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let p = e.path();
        let Ok(meta) = e.metadata() else { continue };
        if meta.is_dir() {
            if depth > 0 {
                out.extend(scan(root, &p, depth - 1)?);
            }
        } else if meta.len() <= MAX_FILE {
            out.push((rel_path(root, &p), std::fs::read(&p)?));
        }
    }
    Ok(out)
}

/// Own zip reader (A1): central directory, stored or deflated entries.
pub fn unzip(d: &[u8]) -> Result<Vec<(String, Vec<u8>)>> {
    let u16_at = |o: usize| -> Result<usize> {
        Ok(u16::from_le_bytes(d.get(o..o + 2).context("zip truncated")?.try_into()?) as usize)
    };
    let u32_at = |o: usize| -> Result<usize> {
        Ok(u32::from_le_bytes(d.get(o..o + 4).context("zip truncated")?.try_into()?) as usize)
    };
    let eocd = (0..d.len().saturating_sub(21))
        .rev()
        .take(65_557)
        .find(|&o| d[o..].starts_with(b"PK\x05\x06"))
        .ok_or_else(|| anyhow!("not a zip file (no end of central directory)"))?;
    let count = u16_at(eocd + 10)?;
    let mut o = u32_at(eocd + 16)?;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if !d.get(o..).is_some_and(|r| r.starts_with(b"PK\x01\x02")) {
            bail!("zip central directory is damaged");
        }
        let method = u16_at(o + 10)?;
        let csize = u32_at(o + 20)?;
        let usize_ = u32_at(o + 24)?;
        let (nlen, xlen, clen) = (u16_at(o + 28)?, u16_at(o + 30)?, u16_at(o + 32)?);
        let local = u32_at(o + 42)?;
        let name = String::from_utf8_lossy(d.get(o + 46..o + 46 + nlen).context("zip truncated")?)
            .replace('\\', "/");
        o += 46 + nlen + xlen + clen;
        if name.ends_with('/') {
            continue;
        }
        if csize == 0xFFFF_FFFF || usize_ == 0xFFFF_FFFF || local == 0xFFFF_FFFF {
            bail!("zip64 archives are not supported ({name})");
        }
        if !d.get(local..).is_some_and(|r| r.starts_with(b"PK\x03\x04")) {
            bail!("zip entry {name} has no local header");
        }
        let start = local + 30 + u16_at(local + 26)? + u16_at(local + 28)?;
        let raw = d.get(start..start + csize).context("zip entry truncated")?;
        let data = match method {
            0 => raw.to_vec(),
            8 => {
                let mut v = Vec::with_capacity(usize_);
                flate2::read::DeflateDecoder::new(raw)
                    .read_to_end(&mut v)
                    .with_context(|| format!("inflating {name}"))?;
                v
            }
            m => bail!("zip entry {name}: compression method {m} is not supported"),
        };
        out.push((name, data));
    }
    Ok(out)
}

fn base(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}
fn dir_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(d, _)| d)
}
/// Number of leading folders two paths share.
fn shared(a: &str, b: &str) -> usize {
    dir_of(a)
        .split('/')
        .zip(dir_of(b).split('/'))
        .take_while(|(x, y)| x == y && !x.is_empty())
        .count()
}

/// Licence text for the avatar at `near` (G15): `licen[cs]e`/terms/`規約`
/// text files before readmes, nearest folder first. PDFs can't be read, so
/// they are only named.
pub fn find_licence(files: &Files, near: &str) -> (Option<(String, String)>, Vec<String>) {
    let rank = |p: &str| -> Option<u8> {
        let b = base(p).to_lowercase();
        let text = [".txt", ".md", ".text"].iter().any(|e| b.ends_with(e)) || !b.contains('.');
        if !text {
            return None;
        }
        if b.contains("licen") || b.contains("terms") || b.contains("規約") || b.contains("利用")
        {
            Some(0)
        } else if b.contains("readme") || b.contains("read_me") || b.contains("read me") {
            Some(1)
        } else {
            None
        }
    };
    let best = files
        .entries
        .iter()
        .filter_map(|(p, d)| rank(p).map(|r| (r, p, d)))
        .min_by_key(|(r, p, _)| (*r, std::cmp::Reverse(shared(near, p)), p.len()))
        .map(|(_, p, d)| {
            let text = String::from_utf8_lossy(d)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let short: String = text.chars().take(600).collect();
            (p.clone(), short)
        });
    let pdfs: Vec<String> = files
        .entries
        .iter()
        .map(|(p, _)| p)
        .filter(|p| p.to_lowercase().ends_with(".pdf"))
        .cloned()
        .collect();
    (best, pdfs)
}

/// Overrides from the command line; all optional (A15).
#[derive(Default)]
pub struct Overrides {
    /// Path (inside the download) of the avatar file to use instead of the ranked best.
    pub avatar: Option<String>,
    pub package: Option<String>,
    pub licence: Option<String>,
    pub variant: Option<String>,
    pub materials: Vec<(String, String)>,
}

pub struct Chosen {
    pub avatar: Avatar,
    /// The chosen avatar file's path inside the download.
    pub path: String,
}

/// Rank the avatar files in `files` and load the best (G16).
pub fn choose(files: &Files, o: &Overrides) -> Result<Chosen> {
    let is = |p: &str, ext: &str| p.to_lowercase().ends_with(ext);
    let mut candidates: Vec<&String> = files
        .entries
        .iter()
        .map(|(p, _)| p)
        .filter(|p| is(p, ".vrm") || is(p, ".fbx"))
        .collect();
    if let Some(a) = &o.avatar {
        candidates.retain(|p| *p == a || base(p) == a.as_str());
        if candidates.is_empty() {
            bail!("no avatar file {a:?} in the input");
        }
    }
    if candidates.is_empty() {
        bail!("no .vrm or .fbx avatar file in the input (G16)");
    }
    // Packages: PC before Quest/Android, then by path (G16).
    let mut pkgs: Vec<&String> = files
        .entries
        .iter()
        .map(|(p, _)| p)
        .filter(|p| is(p, ".unitypackage"))
        .filter(|p| {
            o.package
                .as_ref()
                .is_none_or(|w| *p == w || base(p) == w.as_str())
        })
        .collect();
    let mobile = |p: &str| {
        let l = p.to_lowercase();
        l.contains("quest") || l.contains("android")
    };
    pkgs.sort_by_key(|p| (mobile(p), (*p).clone()));
    let mut cache: HashMap<String, Option<Package>> = HashMap::new();
    let mut notes = Vec::new();
    let mut ranked: Vec<(i64, String, Avatar)> = Vec::new();
    let mut rejected: Vec<String> = Vec::new();
    for path in candidates {
        let bytes = files.get(path).unwrap();
        let loaded = if is(path, ".vrm") {
            vrm::load(bytes)
        } else {
            // The package holding this FBX by name, else the first (PC first).
            let pick = pkgs
                .iter()
                .find(|p| {
                    cache_pkg(&mut cache, files, p)
                        .is_some_and(|k| k.file_by_name(base(path)).is_some())
                })
                .or(pkgs.first())
                .map(|p| (*p).clone());
            let package = pick
                .as_ref()
                .and_then(|p| cache.get(p))
                .and_then(Option::as_ref);
            fbx::load(
                bytes,
                &fbx::Load {
                    file_name: base(path).to_string(),
                    title: stem(path),
                    licence: String::new(),
                    package,
                    variant: o.variant.as_deref(),
                    materials: o.materials.clone(),
                    files: Some(files),
                    path: path.clone(),
                },
            )
            .map(|mut av| {
                if let Some(p) = &pick {
                    av.notes.insert(0, format!("package (G16): {p}"));
                }
                av
            })
        };
        let av = match loaded {
            Ok(av) => av,
            Err(e) => {
                rejected.push(format!("{path}: {e:#}"));
                continue;
            }
        };
        if let Some(why) = crate::convert::incomplete(&av) {
            rejected.push(format!("{path}: {why} (G17)"));
            continue;
        }
        ranked.push((score(path, &av), path.clone(), av));
    }
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut it = ranked.into_iter();
    let Some((s, path, mut av)) = it.next() else {
        bail!(
            "no usable avatar in the input (G16, G17):\n  {}",
            rejected.join("\n  ")
        );
    };
    notes.push(format!("input (G16): {path} (score {s})"));
    for (s2, p2, _) in it {
        notes.push(format!("  runner-up: {p2} (score {s2})"));
    }
    for r in &rejected {
        notes.push(format!("  not an avatar: {r}"));
    }

    // Licence (G15): the override, else the VRM's own, else a text file.
    let (found, pdfs) = find_licence(files, &path);
    if let Some(l) = &o.licence {
        av.licence = l.clone();
        notes.push("licence (G15): from --licence".into());
    } else if !av.licence.is_empty() {
        notes.push("licence (G15): from the VRM meta".into());
        if let Some((p, _)) = &found {
            notes.push(format!("  the download also has {p}"));
        }
    } else if let Some((p, text)) = found {
        notes.push(format!("licence (G15): from {p}"));
        av.licence = text;
    } else if !pdfs.is_empty() {
        notes.push(format!(
            "warning: licence only in PDF files {pdfs:?}, which can't be read; read them before sharing (G15, A13)"
        ));
    } else {
        notes.push("warning: no licence text found in the input (G15, A13)".into());
    }
    let mut all = notes;
    all.append(&mut av.notes);
    av.notes = all;
    Ok(Chosen { avatar: av, path })
}

fn cache_pkg<'c>(
    cache: &'c mut HashMap<String, Option<Package>>,
    files: &Files,
    p: &str,
) -> Option<&'c Package> {
    cache
        .entry(p.to_string())
        .or_insert_with(|| files.get(p).and_then(|d| Package::read(d).ok()))
        .as_ref()
}

fn stem(path: &str) -> String {
    let b = base(path);
    b.rsplit_once('.').map_or(b, |(s, _)| s).to_string()
}

/// Higher is better: VRM first (it carries everything); FBX without rim
/// shells or outline-only materials, the default outfit before alternates,
/// then more detail.
fn score(path: &str, av: &Avatar) -> i64 {
    let mut s = 0i64;
    let lower = path.to_lowercase();
    if lower.ends_with(".vrm") {
        s += 1000;
    }
    if av.parts.iter().any(|p| {
        let m = p.material.to_lowercase();
        m.contains("stencil") || m.contains("outline")
    }) {
        s -= 300;
    }
    let file = base(&lower);
    if ["alt", "alternate", "quest", "android"].iter().any(|w| {
        file.split(|c: char| !c.is_alphanumeric())
            .any(|t| t == *w || t.starts_with("alternate"))
    }) {
        s -= 100;
    }
    let all: usize = av.parts.iter().map(|p| p.triangles.len()).sum();
    s + (all / 1000).min(99) as i64
}

/// GoldSrc names `models/player/<n>/<n>.mdl` in 64 bytes, so `n` ≤ 22.
pub const MAX_NAME: usize = 22;

/// Output name from the download's name (or the avatar file's, for a single
/// file): lower case, letters, digits and `_`, without a trailing version
/// (`_v1_6`), at most [`MAX_NAME`] bytes.
pub fn default_name(input: &Path, avatar: &str) -> String {
    let from = if input.is_file() && !input.to_string_lossy().to_lowercase().ends_with(".zip") {
        stem(avatar)
    } else {
        input
            .file_name()
            .map(|f| stem(&f.to_string_lossy()))
            .unwrap_or_else(|| stem(avatar))
    };
    let mut n: String = from
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    while n.contains("__") {
        n = n.replace("__", "_");
    }
    let mut parts: Vec<&str> = n.split('_').filter(|t| !t.is_empty()).collect();
    // Drop a version tail: `v1`, `v1_6`, `1_6`.
    if let Some(v) = parts.iter().rposition(|t| {
        t.strip_prefix('v')
            .is_some_and(|r| !r.is_empty() && r.chars().all(|c| c.is_ascii_digit()))
    }) && parts[v + 1..]
        .iter()
        .all(|t| t.chars().all(|c| c.is_ascii_digit()))
        && v > 0
    {
        parts.truncate(v);
    }
    let mut out = String::new();
    for t in parts {
        let add = if out.is_empty() { t.len() } else { t.len() + 1 };
        if out.len() + add > MAX_NAME {
            break;
        }
        if !out.is_empty() {
            out.push('_');
        }
        out.push_str(t);
    }
    if out.is_empty() {
        out = n
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .take(MAX_NAME)
            .collect();
    }
    out
}
