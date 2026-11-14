//! `.unitypackage` input (B9, phase G): a gzip'd tar of `<guid>/pathname`,
//! `<guid>/asset` and `<guid>/asset.meta`. From it we read what the FBX
//! doesn't hold: `.mat` colours and textures a prefab puts on each renderer
//! (G2), the prefab variant (G4), Unity's humanoid map (G10) and VRChat
//! PhysBones (G5). Objects a scene names only by Unity's hashed fileIDs
//! inside the FBX can't be resolved; they are skipped and reported (G9).

use std::collections::HashMap;
use std::io::Read;

use anyhow::{Context, Result, bail};

pub struct Asset {
    pub guid: String,
    pub path: String,
    pub data: Option<Vec<u8>>,
    pub meta: Option<Vec<u8>>,
}

pub struct Package {
    /// Sorted by path.
    pub assets: Vec<Asset>,
}

impl Package {
    pub fn read(gz: &[u8]) -> Result<Package> {
        let mut tar = Vec::new();
        flate2::read::GzDecoder::new(gz)
            .read_to_end(&mut tar)
            .context("unitypackage is not gzip")?;
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

#[derive(Default)]
pub struct Settings {
    pub notes: Vec<String>,
    /// Renderer GameObject name → material per slot.
    pub renderer_materials: HashMap<String, Vec<Option<UnityMaterial>>>,
    /// Unity humanoid map from the FBX `.meta`: (bone name, human name).
    pub human: Vec<(String, String)>,
    pub physbones: Vec<PhysBone>,
}

/// Settings for `fbx_name` (the FBX file name) from the package. `variant`
/// picks a prefab or scene by name (G4); the default is the first prefab by
/// path, else the first scene.
pub fn settings(p: &Package, fbx_name: &str, variant: Option<&str>) -> Result<Settings> {
    let mut s = Settings::default();

    // G10: Unity humanoid map.
    let fbx = p
        .assets
        .iter()
        .find(|a| base(&a.path).eq_ignore_ascii_case(fbx_name))
        .or_else(|| {
            p.assets
                .iter()
                .find(|a| a.path.to_lowercase().ends_with(".fbx"))
        });
    if let Some(meta) = fbx.and_then(|a| a.meta.as_deref()) {
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
    let candidates: Vec<&Asset> = by_stem(".prefab")
        .into_iter()
        .chain(by_stem(".unity"))
        .collect();
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
        "unitypackage variant (G4): {} (of {:?})",
        chosen.path,
        candidates.iter().map(|a| stem(a)).collect::<Vec<_>>()
    ));
    let text = String::from_utf8_lossy(chosen.data.as_deref().unwrap()).into_owned();
    let docs = split_docs(&text);
    let by_id: HashMap<i64, &Doc> = docs.iter().map(|d| (d.id, d)).collect();
    let go_name = |id: i64| -> Option<String> {
        let d = by_id.get(&id)?;
        if d.stripped || d.class != 1 {
            return None;
        }
        d.get("m_Name").map(str::to_string)
    };
    let transform_go = |id: i64| -> Option<i64> {
        let d = by_id.get(&id)?;
        if d.stripped || !matches!(d.class, 4 | 224) {
            return None;
        }
        file_id(d.get("m_GameObject")?)
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
    let overrides = text.matches("propertyPath: m_Materials.Array.data").count();
    if overrides > 0 {
        s.notes.push(format!(
            "unitypackage: {overrides} material overrides on prefab instances name FBX objects by Unity's hashed fileIDs; skipped, FBX materials used (G9)"
        ));
    }

    // G5: PhysBones (duck-typed: they have `pull` and `rootTransform`).
    for d in docs
        .iter()
        .filter(|d| d.class == 114 && d.get("pull").is_some())
    {
        let own = d.get("m_GameObject").and_then(file_id);
        let root_tf = d.get("rootTransform").and_then(file_id).unwrap_or(0);
        let root_go = if root_tf == 0 {
            own
        } else {
            transform_go(root_tf)
        };
        let Some(root) = root_go.and_then(go_name) else {
            unresolved += 1;
            continue;
        };
        let num = |k: &str| d.get(k).and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
        let ignore = d
            .list("ignoreTransforms")
            .iter()
            .filter_map(|r| file_id(r))
            .filter_map(|t| transform_go(t).and_then(go_name))
            .collect();
        let colliders = d
            .list("colliders")
            .iter()
            .filter_map(|r| file_id(r).and_then(|id| by_id.get(&id)))
            .filter(|c| !c.stripped)
            .filter_map(|c| {
                let tf = c.get("rootTransform").and_then(file_id).unwrap_or(0);
                let go = if tf == 0 {
                    c.get("m_GameObject").and_then(file_id)
                } else {
                    transform_go(tf)
                };
                Some(PhysCollider {
                    bone: go.and_then(go_name)?,
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
            "unitypackage: {unresolved} renderers/PhysBones sit on prefab-instance objects named only by Unity's hashed fileIDs; skipped (G9)"
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

#[cfg(test)]
mod tests {
    use super::*;

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
