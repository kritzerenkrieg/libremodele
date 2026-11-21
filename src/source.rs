//! What to convert, chosen from a download with no hand choices (A15, G16):
//! a zip, a folder, or a single `.vrm`/`.fbx` (whose own folder is searched
//! too); `.vrm`/`.fbx` files inside Unity packages count as well. Avatar files are ranked, the licence text is found next to
//! them (G15), outfit-only inputs are refused (G17), and every choice, with
//! the runners-up, goes to the report.

use std::collections::{HashMap, HashSet};
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
/// A folder scan reads at most this much in all.
const MAX_SCAN: u64 = 2 << 30;

impl Files {
    /// A zip, a folder (recursive), or one `.vrm`/`.fbx` plus its own folder
    /// (textures and licences sit beside the model); then the file's path
    /// inside the result is returned too, as the avatar. Other archives are
    /// refused with a clear message (RAR, 7z), and a folder scan stops at
    /// [`MAX_SCAN`] bytes so a stray path can't read a home directory.
    pub fn open(path: &Path) -> Result<(Files, Option<String>)> {
        let mut file = None;
        let lower = path.to_string_lossy().to_lowercase();
        let ext = lower.rsplit_once('.').map_or("", |(_, e)| e);
        let mut budget = MAX_SCAN;
        let mut entries = if path.is_dir() {
            scan(path, path, 6, &mut budget)?
        } else if ext == "zip" {
            unzip(&std::fs::read(path).with_context(|| format!("reading {}", path.display()))?)?
        } else if ext == "rar" {
            unrar(&std::fs::read(path).with_context(|| format!("reading {}", path.display()))?)?
        } else if ext == "vrm" || ext == "fbx" {
            let root = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            let mut e = scan(root, root, 3, &mut budget)?;
            let rel = rel_path(root, path);
            if !e.iter().any(|(p, _)| *p == rel) {
                e.push((rel.clone(), std::fs::read(path)?));
            }
            file = Some(rel);
            e
        } else if !path.exists() {
            bail!("{} does not exist", path.display());
        } else {
            bail!(
                "{}: unsupported input (.{ext}); give a .zip, a .rar, a folder, a .vrm or a .fbx (extract other archives first, G16)",
                path.display()
            );
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

fn scan(root: &Path, dir: &Path, depth: usize, budget: &mut u64) -> Result<Vec<(String, Vec<u8>)>> {
    let mut out = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .collect::<std::io::Result<_>>()
        .with_context(|| format!("listing {}", dir.display()))?;
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let p = e.path();
        // Not followed: symlinks (they could point outside the download) and
        // special files such as pipes (reading one can block).
        let kind = e
            .file_type()
            .with_context(|| format!("reading {}", p.display()))?;
        if kind.is_dir() {
            if depth > 0 {
                out.extend(scan(root, &p, depth - 1, budget)?);
            }
            continue;
        }
        if !kind.is_file() {
            continue;
        }
        let meta = e
            .metadata()
            .with_context(|| format!("reading {}", p.display()))?;
        if meta.len() <= MAX_FILE {
            *budget = budget.checked_sub(meta.len()).with_context(|| {
                format!(
                    "{} holds more than {} GB of files; give the avatar's own download folder",
                    root.display(),
                    MAX_SCAN >> 30
                )
            })?;
            out.push((
                rel_path(root, &p),
                std::fs::read(&p).with_context(|| format!("reading {}", p.display()))?,
            ));
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
    let mut budget = MAX_SCAN;
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
        // Limits as for folders: files over the per-file cap are skipped,
        // and the archive as a whole has a budget.
        if usize_ as u64 > MAX_FILE {
            continue;
        }
        budget = budget
            .checked_sub(usize_ as u64)
            .with_context(|| format!("the zip holds more than {} GB of files", MAX_SCAN >> 30))?;
        let data = match method {
            0 => raw.to_vec(),
            8 => {
                // Inflate at most one byte past the declared size.
                let mut v = Vec::with_capacity(usize_);
                flate2::read::DeflateDecoder::new(raw)
                    .take(usize_ as u64 + 1)
                    .read_to_end(&mut v)
                    .with_context(|| format!("inflating {name}"))?;
                v
            }
            m => bail!("zip entry {name}: compression method {m} is not supported"),
        };
        if data.len() != usize_ {
            bail!(
                "zip entry {name} has {} bytes, its header says {usize_}",
                data.len()
            );
        }
        out.push((name, data));
    }
    Ok(out)
}

/// RAR (all versions, G21) through the pure-Rust `rars` crate, into memory;
/// at most [`MAX_SCAN`] bytes in all.
pub fn unrar(d: &[u8]) -> Result<Vec<(String, Vec<u8>)>> {
    use std::sync::{Arc, Mutex};
    struct Sink(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for Sink {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let archive =
        rars::ArchiveReader::read(d).map_err(|e| anyhow!("not a readable RAR archive: {e}"))?;
    let mut out: Vec<(String, Arc<Mutex<Vec<u8>>>)> = Vec::new();
    archive
        .extract_to_with_options(
            rars::ArchiveReadOptions::new().with_max_total_output_bytes(MAX_SCAN),
            |m| {
                let buf = Arc::new(Mutex::new(Vec::new()));
                if !m.is_directory {
                    let name = String::from_utf8_lossy(&m.name).replace('\\', "/");
                    out.push((name, buf.clone()));
                }
                Ok(Box::new(Sink(buf)))
            },
        )
        .map_err(|e| anyhow!("extracting the RAR archive: {e}"))?;
    Ok(out
        .into_iter()
        .map(|(n, b)| (n, std::mem::take(&mut *b.lock().unwrap())))
        .collect())
}

fn base(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}
fn dir_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(d, _)| d)
}
/// The Unity package a `<package>:<asset path>` entry came from; `None` for
/// loose files.
fn container(path: &str) -> Option<&str> {
    path.split_once(':').map(|(c, _)| c)
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
///
/// `entries` are the download's files plus the text files inside its Unity
/// packages (`<package>:<asset path>`).
pub fn find_licence(
    entries: &[(String, &[u8])],
    near: &str,
) -> (Option<(String, String)>, Vec<String>) {
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
    let best = entries
        .iter()
        .filter_map(|(p, d)| rank(p).map(|r| (r, p, d)))
        // Same container first (the same Unity package, or both loose), then
        // the nearest folder, then the shortest path.
        .min_by_key(|(r, p, _)| {
            let other = container(p) != container(near);
            (*r, other, std::cmp::Reverse(shared(near, p)), p.len())
        })
        .map(|(_, p, d)| {
            let text = String::from_utf8_lossy(d)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let short: String = text.chars().take(600).collect();
            (p.clone(), short)
        });
    let pdfs: Vec<String> = entries
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

/// An avatar file to try: (path shown, bytes, the package it lives in, the
/// loose copy it duplicates, tried only if that one fails to load).
type Candidate<'a> = (String, &'a [u8], Option<String>, Option<String>);

/// Rank the avatar files in `files` and load the best (G16).
pub fn choose(files: &Files, o: &Overrides) -> Result<Chosen> {
    let is = |p: &str, ext: &str| p.to_lowercase().ends_with(ext);
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
    // Read every package once. An unreadable one the user named is an
    // error; others are skipped with a warning, never silently.
    let mut cache: HashMap<String, Option<Package>> = HashMap::new();
    let mut pkg_notes = Vec::new();
    for p in &pkgs {
        let read = files.get(p).map(Package::read).transpose();
        match read {
            Ok(k) => {
                cache.insert((*p).clone(), k);
            }
            Err(e) if o.package.is_some() => {
                return Err(e.context(format!("reading the package {p}")));
            }
            Err(e) => {
                pkg_notes.push(format!(
                    "warning: package {p} can't be read ({e:#}); skipped"
                ));
                cache.insert((*p).clone(), None);
            }
        }
    }
    pkgs.retain(|p| cache.get(*p).is_some_and(Option::is_some));

    // Candidates: loose files, then avatar files inside packages (a download
    // may be only a package, like Saji's) unless a loose copy exists.
    // (path shown, bytes, the package it lives in)
    let mut candidates: Vec<Candidate> = files
        .entries
        .iter()
        .filter(|(p, _)| is(p, ".vrm") || is(p, ".fbx"))
        .map(|(p, d)| (p.clone(), d.as_slice(), None, None))
        .collect();
    for p in &pkgs {
        let Some(Some(k)) = cache.get(*p) else {
            continue;
        };
        for a in &k.assets {
            let Some(d) = &a.data else { continue };
            if !(is(&a.path, ".vrm") || is(&a.path, ".fbx")) {
                continue;
            }
            // A loose copy wins; the packaged one stays as its fallback,
            // unless the user named the packaged one.
            let path = format!("{p}:{}", a.path);
            let named = o.avatar.as_deref() == Some(path.as_str());
            let loose = candidates
                .iter()
                .find(|(c, _, inside, _)| {
                    inside.is_none() && base(c).eq_ignore_ascii_case(base(&a.path))
                })
                .map(|(c, ..)| c.clone());
            let fallback = if named { None } else { loose };
            candidates.push((path, d.as_slice(), Some((*p).clone()), fallback));
        }
    }
    if let Some(a) = &o.avatar {
        candidates.retain(|(p, ..)| p == a || base(p) == a.as_str());
        if candidates.is_empty() {
            bail!("no avatar file {a:?} in the input");
        }
    }
    if candidates.is_empty() {
        bail!("no .vrm or .fbx avatar file in the input, loose or in a Unity package (G16)");
    }
    let mut notes = pkg_notes;
    let mut ranked: Vec<(i64, String, Avatar)> = Vec::new();
    let mut rejected: Vec<String> = Vec::new();
    let mut loaded_ok: HashSet<String> = HashSet::new();
    for (path, bytes, inside, fallback) in &candidates {
        // A packaged duplicate is only needed when its loose copy failed.
        if let Some(loose) = fallback {
            if loaded_ok.contains(loose) {
                continue;
            }
            notes.push(format!(
                "avatar (G16): {loose} can't be read; trying the packaged copy {path}"
            ));
        }
        let path = path.clone();
        let mut own = false;
        let loaded = if is(&path, ".vrm") {
            vrm::load(bytes)
        } else {
            // The package it came from; else the one holding this FBX by
            // name; else the first (PC first).
            let by_name = inside.clone().or_else(|| {
                pkgs.iter()
                    .find(|p| {
                        cache
                            .get(**p)
                            .and_then(Option::as_ref)
                            .is_some_and(|k| k.file_by_name(base(&path)).is_some())
                    })
                    .map(|p| (*p).clone())
            });
            own = by_name.is_some();
            let pick = by_name.or(pkgs.first().map(|p| (*p).clone()));
            let package = pick
                .as_ref()
                .and_then(|p| cache.get(p))
                .and_then(Option::as_ref);
            fbx::load(
                bytes,
                &fbx::Load {
                    file_name: base(&path).to_string(),
                    title: stem(&path),
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
            Ok(av) => {
                loaded_ok.insert(path.clone());
                av
            }
            Err(e) => {
                rejected.push(format!("{path}: {e:#}"));
                continue;
            }
        };
        if let Some(why) = crate::convert::incomplete(&av) {
            rejected.push(format!("{path}: {why} (G17)"));
            continue;
        }
        ranked.push((score(&path, &av, own), path, av));
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
    let mut texts: Vec<(String, &[u8])> = files
        .entries
        .iter()
        .map(|(p, d)| (p.clone(), d.as_slice()))
        .collect();
    for p in &pkgs {
        if let Some(Some(k)) = cache.get(*p) {
            for a in &k.assets {
                let l = a.path.to_lowercase();
                if let Some(d) = &a.data
                    && [".txt", ".md", ".pdf"].iter().any(|e| l.ends_with(e))
                {
                    texts.push((format!("{p}:{}", a.path), d.as_slice()));
                }
            }
        }
    }
    let (found, pdfs) = find_licence(&texts, &path);
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

fn stem(path: &str) -> String {
    let b = base(path);
    b.rsplit_once('.').map_or(b, |(s, _)| s).to_string()
}

/// Higher is better: VRM first (it carries everything); FBX without rim
/// shells or outline-only materials, the default outfit before alternates,
/// then more detail.
/// `own`: the package holding this FBX is its own (by name or inside it),
/// so the author's whole Unity setup applies (G18).
fn score(path: &str, av: &Avatar, own: bool) -> i64 {
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
    // Alternate outfits, by file name; mobile builds anywhere in the path
    // (`Quest/…/Blondy_QuestVer.fbx` inside `…_QUEST.unitypackage`).
    let file = base(&lower);
    if file
        .split(|c: char| !c.is_alphanumeric())
        .any(|t| t == "alt" || t.starts_with("alternate"))
    {
        s -= 100;
    }
    if lower.contains("quest") || lower.contains("android") {
        s -= 100;
    }
    if own {
        s += 5;
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
    let lower = input.to_string_lossy().to_lowercase();
    let from = if input.is_file() && ![".zip", ".rar"].iter().any(|e| lower.ends_with(e)) {
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
    // A name with no ASCII letters or digits at all (`アバター`): a neutral
    // default the user can rename, not an error.
    if out.is_empty() {
        out = "avatar".into();
    }
    out
}
