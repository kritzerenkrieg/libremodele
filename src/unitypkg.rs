//! `.unitypackage` input (B9, phase G): a gzip'd tar of `<guid>/pathname`,
//! `<guid>/asset` and `<guid>/asset.meta`. From it we read what the FBX
//! doesn't hold: `.mat` colours and textures a prefab puts on each renderer
//! (G2), the prefab variant (G4), Unity's humanoid map (G10) and VRChat
//! PhysBones (G5). Objects inside the FBX, which scenes name only by Unity's
//! hashed fileIDs, are found by reproducing the hash (G18; was skipped, G9).

use std::collections::HashMap;
use std::io::Read;

use anyhow::{Context, Result, bail};

pub struct Asset {
    pub guid: String,
    pub path: String,
    pub data: Option<Vec<u8>>,
    pub meta: Option<Vec<u8>>,
}

/// More material slots than any renderer has; a larger prefab override
/// index is malformed.
const MAX_SLOTS: usize = 256;

/// Largest expanded `.unitypackage` (the tar inside), as the download scan
/// budget (G16).
const MAX_TAR: u64 = 2 << 30;

pub struct Package {
    /// Sorted by path.
    pub assets: Vec<Asset>,
}

impl Package {
    pub fn read(gz: &[u8]) -> Result<Package> {
        // Expand at most MAX_TAR bytes: a small package can't inflate into
        // an unbounded buffer.
        let mut tar = Vec::new();
        flate2::read::GzDecoder::new(gz)
            .take(MAX_TAR + 1)
            .read_to_end(&mut tar)
            .context("unitypackage is not gzip")?;
        if tar.len() as u64 > MAX_TAR {
            bail!("unitypackage expands past {} GB", MAX_TAR >> 30);
        }
        let mut by_guid: HashMap<String, Asset> = HashMap::new();
        let mut o = 0;
        let mut long_name: Option<String> = None;
        while o + 512 <= tar.len() {
            let h = &tar[o..o + 512];
            if h.iter().all(|&b| b == 0) {
                break;
            }
            let cstr = |b: &[u8]| {
                let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
                String::from_utf8_lossy(&b[..end]).into_owned()
            };
            let size_s = cstr(&h[124..136]);
            let size = usize::from_str_radix(size_s.trim(), 8)
                .with_context(|| format!("bad tar size {size_s:?}"))?;
            let kind = h[156];
            let mut name = cstr(&h[0..100]);
            if &h[257..262] == b"ustar" {
                let prefix = cstr(&h[345..500]);
                if !prefix.is_empty() {
                    name = format!("{prefix}/{name}");
                }
            }
            let body_at = o + 512;
            let body = tar
                .get(body_at..body_at + size)
                .context("unitypackage tar truncated")?;
            o = body_at + size.div_ceil(512) * 512;
            match kind {
                b'L' => {
                    long_name = Some(cstr(body));
                    continue;
                }
                b'0' | 0 => {}
                _ => {
                    long_name = None;
                    continue;
                }
            }
            if let Some(n) = long_name.take() {
                name = n;
            }
            let name = name.trim_start_matches("./");
            let Some((guid, file)) = name.split_once('/') else {
                continue;
            };
            let a = by_guid.entry(guid.to_string()).or_insert_with(|| Asset {
                guid: guid.to_string(),
                path: String::new(),
                data: None,
                meta: None,
            });
            match file {
                "pathname" => {
                    a.path = String::from_utf8_lossy(body)
                        .lines()
                        .next()
                        .unwrap_or("")
                        .to_string()
                }
                "asset" => a.data = Some(body.to_vec()),
                "asset.meta" => a.meta = Some(body.to_vec()),
                _ => {}
            }
        }
        let mut assets: Vec<Asset> = by_guid
            .into_values()
            .filter(|a| !a.path.is_empty())
            .collect();
        assets.sort_by(|a, b| a.path.cmp(&b.path));
        if assets.is_empty() {
            bail!("the unitypackage holds no assets");
        }
        Ok(Package { assets })
    }

    pub fn by_guid(&self, guid: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.guid == guid)
    }

    /// Asset data whose file name matches, ignoring case.
    pub fn file_by_name(&self, name: &str) -> Option<Vec<u8>> {
        self.assets
            .iter()
            .find(|a| base(&a.path).eq_ignore_ascii_case(name))
            .and_then(|a| a.data.clone())
    }
}

fn base(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// A `.mat` as GoldSrc needs it (G2, C7).
#[derive(Clone, Debug)]
pub struct UnityMaterial {
    pub name: String,
    /// sRGB.
    pub colour: [f64; 3],
    pub masked: bool,
    /// (file name, PNG/JPG bytes).
    pub texture: Option<(String, Vec<u8>)>,
}

#[derive(Clone, Debug)]
pub struct PhysCollider {
    pub bone: String,
    pub position: [f32; 3],
    pub radius: f32,
}

#[derive(Clone, Debug)]
pub struct PhysBone {
    /// GameObject (= FBX bone) name of the root transform.
    pub root: String,
    pub ignore: Vec<String>,
    pub pull: f32,
    pub spring: f32,
    pub gravity: f32,
    pub radius: f32,
    pub colliders: Vec<PhysCollider>,
}

pub struct Settings {
    pub notes: Vec<String>,
    /// Renderer GameObject name → material per slot.
    pub renderer_materials: HashMap<String, Vec<Option<UnityMaterial>>>,
    /// Unity humanoid map from the FBX `.meta`: (bone name, human name).
    pub human: Vec<(String, String)>,
    pub physbones: Vec<PhysBone>,
    /// The FBX `.meta` import scale (`globalScale`), 1 when absent.
    pub global_scale: f64,
    /// FBX material name → `.mat`, from the FBX `.meta` `externalObjects`:
    /// Unity's own remap, used where no prefab override applies (G2).
    pub remap: HashMap<String, UnityMaterial>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            notes: Vec::new(),
            renderer_materials: HashMap::new(),
            human: Vec::new(),
            physbones: Vec::new(),
            global_scale: 1.0,
            remap: HashMap::new(),
        }
    }
}

/// A `.mat` by name (with or without `.mat`), for `--material` (G12).
pub fn material_named(p: &Package, name: &str) -> Option<UnityMaterial> {
    let want = name.strip_suffix(".mat").unwrap_or(name);
    p.assets
        .iter()
        .find(|a| {
            base(&a.path)
                .strip_suffix(".mat")
                .is_some_and(|b| b.eq_ignore_ascii_case(want))
        })
        .and_then(|a| material(p, a))
}

/// Every `.mat` in the package, for automatic pairing (G14).
pub fn materials(p: &Package) -> Vec<UnityMaterial> {
    p.assets.iter().filter_map(|a| material(p, a)).collect()
}

/// Settings for `fbx_name` (the FBX file name) from the package. `variant`
/// picks a prefab or scene by name (G4); the default is the first prefab by
/// path, else the first scene.
///
/// `paths` are the converted FBX's object paths (`Armature/hips`): Unity names
/// objects inside an FBX by a hash of them (G18).
pub fn settings(
    p: &Package,
    fbx_name: &str,
    bones: &[&str],
    paths: &[String],
    variant: Option<&str>,
) -> Result<Settings> {
    let mut s = Settings::default();

    // The package's copy of this FBX: by file name, else (G11) the FBX whose
    // `.meta` skeleton shares the most bone names with ours.
    let by_name = p
        .assets
        .iter()
        .find(|a| base(&a.path).eq_ignore_ascii_case(fbx_name));
    let fbx = by_name.or_else(|| {
            let ours: std::collections::HashSet<&str> = bones.iter().copied().collect();
            p.assets
                .iter()
                .filter(|a| a.path.to_lowercase().ends_with(".fbx"))
                .filter_map(|a| {
                    let meta = String::from_utf8_lossy(a.meta.as_deref()?).into_owned();
                    let n = meta
                        .lines()
                        .filter_map(|l| l.trim().strip_prefix("- name: "))
                        .filter(|n| ours.contains(n.trim()))
                        .count();
                    (n > 0).then_some((n, a))
                })
                // Most shared names; ties to the first by path.
                .max_by(|x, y| x.0.cmp(&y.0).then(y.1.path.cmp(&x.1.path)))
                .map(|(n, a)| {
                    s.notes.push(format!(
                        "unitypackage: {fbx_name} is not in the package; using {} ({n} shared bone names, G11)",
                        a.path
                    ));
                    a
                })
        });
    if let Some(meta) = fbx.and_then(|a| a.meta.as_deref()) {
        let text = String::from_utf8_lossy(meta);
        s.global_scale = text
            .lines()
            .find_map(|l| l.trim().strip_prefix("globalScale: "))
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(1.0);
        // Unity's material remap; only for the package's own FBX (as G18):
        // through G11 the names belong to another file.
        if by_name.is_some() {
            let lines: Vec<&str> = text.lines().map(str::trim).collect();
            for (i, l) in lines.iter().enumerate() {
                if *l == "type: UnityEngine:Material"
                    && let Some(name) = lines.get(i + 2).and_then(|n| n.strip_prefix("name: "))
                    && let Some(r) = lines.get(i + 3).and_then(|n| n.strip_prefix("second: "))
                    && let Some(m) = guid(r)
                        .and_then(|g| p.by_guid(g))
                        .and_then(|a| material(p, a))
                {
                    s.remap.insert(name.to_string(), m);
                }
            }
            if !s.remap.is_empty() {
                s.notes.push(format!(
                    "unitypackage: {} FBX materials remapped to .mat files by the FBX .meta (G2)",
                    s.remap.len()
                ));
            }
        }
        let text = String::from_utf8_lossy(meta);
        let mut lines = text.lines().peekable();
        while let Some(l) = lines.next() {
            if let Some(b) = l.trim().strip_prefix("- boneName: ")
                && let Some(h) = lines
                    .peek()
                    .and_then(|n| n.trim().strip_prefix("humanName: "))
            {
                s.human.push((b.trim().to_string(), h.trim().to_string()));
            }
        }
    }

    // G4: the variant.
    let is = |a: &&Asset, ext: &str| a.path.to_lowercase().ends_with(ext) && a.data.is_some();
    let stem = |a: &Asset| {
        let b = base(&a.path);
        b.rsplit_once('.').map_or(b, |(s, _)| s).to_string()
    };
    let by_stem = |ext: &str| {
        let mut v: Vec<&Asset> = p.assets.iter().filter(|a| is(a, ext)).collect();
        v.sort_by_key(|a| stem(a));
        v
    };
    // By name, so `Gothic Doll` comes before `Gothic Doll 1` (G4).
    let mut candidates: Vec<&Asset> = by_stem(".prefab")
        .into_iter()
        .chain(by_stem(".unity"))
        .collect();
    // G11: prefabs and scenes that use this FBX come first.
    if let Some(g) = fbx.map(|a| format!("guid: {}", a.guid)) {
        candidates.sort_by_key(|a| {
            !String::from_utf8_lossy(a.data.as_deref().unwrap_or_default()).contains(&g)
        });
    }
    let chosen = match variant {
        Some(v) => Some(
            *candidates
                .iter()
                .find(|a| stem(a) == v || base(&a.path) == v)
                .with_context(|| {
                    format!(
                        "no prefab or scene named {v:?}; the package has {:?}",
                        candidates.iter().map(|a| stem(a)).collect::<Vec<_>>()
                    )
                })?,
        ),
        None => candidates.first().copied(),
    };
    let Some(chosen) = chosen else {
        s.notes
            .push("unitypackage: no prefab or scene; FBX materials only (G2)".into());
        return Ok(s);
    };
    s.notes.push(format!(
        "unitypackage variant (G4, G11): {} (of {:?})",
        chosen.path,
        candidates.iter().map(|a| stem(a)).collect::<Vec<_>>()
    ));
    let text = String::from_utf8_lossy(chosen.data.as_deref().unwrap()).into_owned();
    let docs = split_docs(&text);
    let by_id: HashMap<i64, &Doc> = docs.iter().map(|d| (d.id, d)).collect();
    // G18: objects inside the FBX, by Unity's hashed fileID → (class, name).
    let fbx_guid = fbx.map(|a| a.guid.as_str()).unwrap_or("");
    let hashed = hashed_ids(paths);
    // A stripped object stands for one inside the FBX (or a nested prefab).
    let source = |d: &Doc, class: i32| -> Option<String> {
        let r = d.get("m_CorrespondingSourceObject")?;
        if guid(r)? != fbx_guid {
            return None;
        }
        hashed
            .get(&file_id(r)?)
            .filter(|(c, _)| *c == class)
            .map(|(_, n)| n.clone())
    };
    let go_name = |id: i64| -> Option<String> {
        let d = by_id.get(&id)?;
        match (d.class, d.stripped) {
            (1, false) => d.get("m_Name").map(str::to_string),
            (1, true) => source(d, 1),
            _ => None,
        }
    };
    let tf_name = |id: i64| -> Option<String> {
        let d = by_id.get(&id)?;
        match (d.class, d.stripped) {
            (4 | 224, false) => go_name(file_id(d.get("m_GameObject")?)?),
            (4 | 224, true) => source(d, 4),
            _ => None,
        }
    };
    let mut unresolved = 0usize;

    // G2: renderer materials.
    for d in docs.iter().filter(|d| matches!(d.class, 137 | 23)) {
        let Some(name) = (!d.stripped)
            .then(|| d.get("m_GameObject").and_then(file_id).and_then(go_name))
            .flatten()
        else {
            unresolved += 1;
            continue;
        };
        let mats = d
            .list("m_Materials")
            .iter()
            .map(|r| {
                guid(r)
                    .and_then(|g| p.by_guid(g))
                    .and_then(|a| material(p, a))
            })
            .collect();
        s.renderer_materials.insert(name, mats);
    }
    // G18: material overrides on the FBX's renderers, from prefab instances.
    let (mut applied, mut skipped) = (0usize, 0usize);
    for o in material_overrides(&text) {
        let target = (o.guid == fbx_guid)
            .then(|| hashed.get(&o.target))
            .flatten()
            .filter(|(c, _)| matches!(c, 137 | 23));
        let mat = p.by_guid(&o.material).and_then(|a| material(p, a));
        match (target, mat) {
            // Only for the package's own FBX: through G11's bone match the
            // objects belong to another file (Blindy's would get the
            // Combined FBX's atlas).
            // A slot index past any real renderer would only grow the array.
            (Some((_, name)), Some(m)) if by_name.is_some() && o.slot < MAX_SLOTS => {
                let slots = s.renderer_materials.entry(name.clone()).or_default();
                if slots.len() <= o.slot {
                    slots.resize(o.slot + 1, None);
                }
                slots[o.slot] = Some(m);
                applied += 1;
            }
            _ => skipped += 1,
        }
    }
    if applied + skipped > 0 {
        s.notes.push(format!(
            "unitypackage: {applied} material overrides applied by Unity's hashed fileIDs (G18){}",
            if skipped > 0 {
                format!(
                    "; {skipped} not ({})",
                    if by_name.is_some() {
                        "their objects aren't in this FBX"
                    } else {
                        "the converted FBX is not the package's own, G11"
                    }
                )
            } else {
                String::new()
            }
        ));
    }

    // G5: PhysBones (duck-typed: they have `pull` and `rootTransform`).
    for d in docs
        .iter()
        .filter(|d| d.class == 114 && d.get("pull").is_some())
    {
        let own = d.get("m_GameObject").and_then(file_id);
        let root_tf = d.get("rootTransform").and_then(file_id).unwrap_or(0);
        let root = if root_tf == 0 {
            own.and_then(go_name)
        } else {
            tf_name(root_tf)
        };
        let Some(root) = root else {
            unresolved += 1;
            continue;
        };
        let num = |k: &str| d.get(k).and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
        let ignore = d
            .list("ignoreTransforms")
            .iter()
            .filter_map(|r| file_id(r))
            .filter_map(tf_name)
            .collect();
        let colliders = d
            .list("colliders")
            .iter()
            .filter_map(|r| file_id(r).and_then(|id| by_id.get(&id)))
            .filter(|c| !c.stripped)
            .filter_map(|c| {
                let tf = c.get("rootTransform").and_then(file_id).unwrap_or(0);
                let bone = if tf == 0 {
                    c.get("m_GameObject").and_then(file_id).and_then(go_name)
                } else {
                    tf_name(tf)
                };
                Some(PhysCollider {
                    bone: bone?,
                    position: vec3(c.get("position")?),
                    radius: c.get("radius")?.parse().ok()?,
                })
            })
            .collect();
        s.physbones.push(PhysBone {
            root,
            ignore,
            pull: num("pull"),
            spring: num("spring"),
            gravity: num("gravity"),
            radius: num("radius"),
            colliders,
        });
    }
    if unresolved > 0 {
        s.notes.push(format!(
            "unitypackage: {unresolved} renderers/PhysBones sit on objects not found in this FBX, even by hashed fileID (G18); skipped"
        ));
    }
    Ok(s)
}

fn material(p: &Package, a: &Asset) -> Option<UnityMaterial> {
    if !a.path.to_lowercase().ends_with(".mat") {
        return None;
    }
    let text = String::from_utf8_lossy(a.data.as_deref()?).into_owned();
    let lines: Vec<&str> = text.lines().collect();
    let mut colour = [1.0; 3];
    let mut texture = None;
    let mut masked = false;
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim();
        if let Some(c) = t.strip_prefix("- _Color: ") {
            let f = |k: &str| field(c, k).and_then(|v| v.parse::<f64>().ok());
            if let (Some(r), Some(g), Some(b)) = (f("r"), f("g"), f("b")) {
                colour = [r, g, b];
            }
        } else if t == "- _MainTex:"
            && let Some(tex) = lines
                .get(i + 1)
                .and_then(|n| n.trim().strip_prefix("m_Texture: "))
            && let Some(ta) = guid(tex).and_then(|g| p.by_guid(g))
            && let Some(d) = &ta.data
        {
            texture = Some((base(&ta.path).to_string(), d.clone()));
        } else if let Some(rt) = t.strip_prefix("RenderType: ") {
            masked = matches!(rt.trim(), "TransparentCutout" | "Transparent");
        }
    }
    Some(UnityMaterial {
        name: base(&a.path).to_string(),
        colour,
        masked,
        texture,
    })
}

struct Doc<'a> {
    class: u32,
    id: i64,
    stripped: bool,
    lines: Vec<&'a str>,
}

impl<'a> Doc<'a> {
    /// Value of a top-level field (indented two spaces under the class key).
    fn get(&self, key: &str) -> Option<&'a str> {
        self.lines.iter().find_map(|l| {
            l.strip_prefix("  ")
                .filter(|r| !r.starts_with(' '))
                .and_then(|r| r.strip_prefix(key))
                .and_then(|r| r.strip_prefix(':'))
                .map(str::trim)
        })
    }
    /// Items of a top-level block list field.
    fn list(&self, key: &str) -> Vec<&'a str> {
        let head = format!("  {key}:");
        let Some(i) = self.lines.iter().position(|l| l.trim_end() == head) else {
            return Vec::new();
        };
        self.lines[i + 1..]
            .iter()
            .map_while(|l| l.strip_prefix("  - "))
            .collect()
    }
}

fn split_docs(text: &str) -> Vec<Doc<'_>> {
    let mut out: Vec<Doc> = Vec::new();
    for l in text.lines() {
        if let Some(h) = l.strip_prefix("--- !u!") {
            let mut it = h.split_whitespace();
            let class = it.next().and_then(|c| c.parse().ok()).unwrap_or(0);
            let id = it
                .next()
                .and_then(|i| i.strip_prefix('&'))
                .and_then(|i| i.parse().ok())
                .unwrap_or(0);
            out.push(Doc {
                class,
                id,
                stripped: it.any(|w| w == "stripped"),
                lines: Vec::new(),
            });
        } else if let Some(d) = out.last_mut() {
            d.lines.push(l);
        }
    }
    out
}

/// `k: v` inside a `{...}` flow map.
fn field<'a>(s: &'a str, k: &str) -> Option<&'a str> {
    let s = s.trim().trim_start_matches('{').trim_end_matches('}');
    s.split(',').find_map(|kv| {
        let (a, b) = kv.split_once(':')?;
        (a.trim() == k).then(|| b.trim())
    })
}
fn file_id(s: &str) -> Option<i64> {
    field(s, "fileID")?.parse().ok()
}
fn guid(s: &str) -> Option<&str> {
    field(s, "guid")
}
fn vec3(s: &str) -> [f32; 3] {
    let f = |k: &str| field(s, k).and_then(|v| v.parse().ok()).unwrap_or(0.0);
    [f("x"), f("y"), f("z")]
}

/// Unity's fileIDs for objects inside an FBX (`fileIdsGeneration: 2`, G18):
/// XXH64 of `Type:<Class>->//RootNode/root/<path>[/<Class>]0`. Unofficial;
/// checked against Neru's and the Poka packages.
fn hashed_ids(paths: &[String]) -> HashMap<i64, (i32, String)> {
    let mut out = HashMap::new();
    let id = |s: String| xxh64(s.as_bytes()) as i64;
    for p in paths {
        let name = p.rsplit('/').next().unwrap_or(p).to_string();
        let at = format!("//RootNode/root/{p}");
        out.insert(id(format!("Type:GameObject->{at}0")), (1, name.clone()));
        for (class, c) in [
            (4, "Transform"),
            (137, "SkinnedMeshRenderer"),
            (23, "MeshRenderer"),
        ] {
            out.insert(id(format!("Type:{c}->{at}/{c}0")), (class, name.clone()));
        }
    }
    out
}

/// XXH64 with seed 0.
pub fn xxh64(d: &[u8]) -> u64 {
    const P1: u64 = 11400714785074694791;
    const P2: u64 = 14029467366897019727;
    const P3: u64 = 1609587929392839161;
    const P4: u64 = 9650029242287828579;
    const P5: u64 = 2870177450012600261;
    let u64_at = |i: usize| u64::from_le_bytes(d[i..i + 8].try_into().unwrap());
    let round = |acc: u64, v: u64| {
        acc.wrapping_add(v.wrapping_mul(P2))
            .rotate_left(31)
            .wrapping_mul(P1)
    };
    let mut i = 0;
    let mut h = if d.len() >= 32 {
        let mut v = [P1.wrapping_add(P2), P2, 0, 0u64.wrapping_sub(P1)];
        while i + 32 <= d.len() {
            for (k, vk) in v.iter_mut().enumerate() {
                *vk = round(*vk, u64_at(i + 8 * k));
            }
            i += 32;
        }
        let mut h = v[0]
            .rotate_left(1)
            .wrapping_add(v[1].rotate_left(7))
            .wrapping_add(v[2].rotate_left(12))
            .wrapping_add(v[3].rotate_left(18));
        for vk in v {
            h = (h ^ round(0, vk)).wrapping_mul(P1).wrapping_add(P4);
        }
        h
    } else {
        P5
    };
    h = h.wrapping_add(d.len() as u64);
    while i + 8 <= d.len() {
        h = (h ^ round(0, u64_at(i)))
            .rotate_left(27)
            .wrapping_mul(P1)
            .wrapping_add(P4);
        i += 8;
    }
    if i + 4 <= d.len() {
        let k = u32::from_le_bytes(d[i..i + 4].try_into().unwrap()) as u64;
        h = (h ^ k.wrapping_mul(P1))
            .rotate_left(23)
            .wrapping_mul(P2)
            .wrapping_add(P3);
        i += 4;
    }
    for &b in &d[i..] {
        h = (h ^ (b as u64).wrapping_mul(P5))
            .rotate_left(11)
            .wrapping_mul(P1);
    }
    h ^= h >> 33;
    h = h.wrapping_mul(P2);
    h ^= h >> 29;
    h = h.wrapping_mul(P3);
    h ^ (h >> 32)
}

struct MaterialOverride {
    target: i64,
    guid: String,
    slot: usize,
    material: String,
}

/// `m_Materials.Array.data[N]` modifications in prefab instances.
/// A YAML flow map starting `first` on line `at`, joined with the lines it
/// wraps onto until its closing brace (at most a few).
fn flow(lines: &[&str], at: usize, first: &str) -> String {
    let mut out = first.to_string();
    for l in lines.iter().skip(at + 1).take(4) {
        if out.contains('}') || !out.trim_start().starts_with('{') {
            break;
        }
        out.push(' ');
        out.push_str(l);
    }
    out
}

fn material_overrides(text: &str) -> Vec<MaterialOverride> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let Some(t) = l.strip_prefix("- target: ") else {
            continue;
        };
        // Unity wraps long flow maps (`{fileID: …, guid:` / `…, type: 3}`).
        let t = flow(&lines, i, t);
        // Long `target` lines wrap, so look a few lines on.
        let next = &lines[i + 1..(i + 6).min(lines.len())];
        let Some(slot) = next
            .iter()
            .take_while(|n| !n.starts_with("- target:"))
            .find_map(|n| n.strip_prefix("propertyPath: "))
            .and_then(|n| n.strip_prefix("m_Materials.Array.data["))
            .and_then(|n| n.strip_suffix(']'))
            .and_then(|n| n.parse().ok())
        else {
            continue;
        };
        let Some(obj) = next
            .iter()
            .take_while(|n| !n.starts_with("- target:"))
            .enumerate()
            .find_map(|(k, n)| {
                n.strip_prefix("objectReference: ")
                    .map(|o| flow(&lines, i + 1 + k, o))
            })
        else {
            continue;
        };
        if let (Some(target), Some(g), Some(m)) = (file_id(&t), guid(&t), guid(&obj)) {
            out.push(MaterialOverride {
                target,
                guid: g.to_string(),
                slot,
                material: m.to_string(),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapped_override_targets_are_read() {
        let text = "    - target: {fileID: -7511558181221131132, guid: 0123456789abcdef0123456789abcdef,\n        type: 3}\n      propertyPath: m_Materials.Array.data[2]\n      value:\n      objectReference: {fileID: 2100000, guid: fedcba9876543210fedcba9876543210,\n        type: 2}\n";
        let o = material_overrides(text);
        assert_eq!(o.len(), 1);
        assert_eq!(o[0].slot, 2);
        assert_eq!(o[0].guid, "0123456789abcdef0123456789abcdef");
        assert_eq!(o[0].material, "fedcba9876543210fedcba9876543210");
    }

    #[test]
    fn fbx_file_ids_match_unity() {
        assert_eq!(super::xxh64(b""), 0xEF46DB3751D8E999);
        // Shared by every FBX prefab Unity imports (G18).
        let ids = super::hashed_ids(&[]);
        assert!(ids.is_empty());
        let id = |s: &str| super::xxh64(s.as_bytes()) as i64;
        assert_eq!(id("Type:GameObject->//RootNode/root0"), 919132149155446097);
        assert_eq!(
            id("Type:Transform->//RootNode/root/Transform0"),
            -8679921383154817045
        );
        // Longer than 32 bytes: the four-lane path.
        assert_eq!(
            id("Type:SkinnedMeshRenderer->//RootNode/root/Plane/SkinnedMeshRenderer0"),
            7205140776170859923
        );
    }

    #[test]
    fn yaml_docs_fields_and_lists() {
        let y = "%YAML 1.1\n--- !u!1 &5\nGameObject:\n  m_Name: skirt\n--- !u!137 &7 stripped\nSkinnedMeshRenderer:\n  m_GameObject: {fileID: 5}\n  m_Materials:\n  - {fileID: 2100000, guid: abc, type: 2}\n  - {fileID: 0}\n  m_Mesh: {fileID: 1}\n";
        let d = split_docs(y);
        assert_eq!(d.len(), 2);
        assert_eq!(
            (d[0].class, d[0].id, d[0].get("m_Name")),
            (1, 5, Some("skirt"))
        );
        assert!(d[1].stripped);
        assert_eq!(d[1].get("m_GameObject").and_then(file_id), Some(5));
        let l = d[1].list("m_Materials");
        assert_eq!(l.len(), 2);
        assert_eq!(guid(l[0]), Some("abc"));
        assert_eq!(vec3("{x: 1, y: -2.5, z: 0}"), [1.0, -2.5, 0.0]);
    }
}
