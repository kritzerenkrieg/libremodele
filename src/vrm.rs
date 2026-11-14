//! VRM 0.x / glb input (A5, B1): node tree, humanoid map, spring-bone chains,
//! and every mesh primitive skinned into its bind pose. Blendshapes are left
//! at 0, which is the neutral face (B7).

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;

use crate::math::{V3, Xf, norm};

pub struct Node {
    pub name: String,
    pub parent: Option<usize>,
    pub world: Xf,
}

/// One primitive, in glTF world space at the bind pose.
pub struct Part {
    /// `node/mesh#primitive`, used in the clean manifest (B4).
    pub id: String,
    pub node_name: String,
    pub positions: Vec<V3>,
    pub normals: Vec<V3>,
    pub uvs: Vec<[f32; 2]>,
    /// The node with the largest skin weight, per vertex (D3).
    pub dominant: Vec<usize>,
    pub triangles: Vec<[u32; 3]>,
    /// Image index of the base colour texture.
    pub image: Option<usize>,
    pub unlit: bool,
    pub first_person_only: bool,
}

/// A sphere collider: `offset` in the node's local space (metres).
#[derive(Clone, Debug)]
pub struct Collider {
    pub node: usize,
    pub offset: V3,
    pub radius: f32,
}

/// One VRM 0.x spring-bone group, as UniVRM uses it.
#[derive(Clone, Debug)]
pub struct SpringGroup {
    pub name: String,
    pub stiffness: f32,
    pub gravity_power: f32,
    /// glTF space.
    pub gravity_dir: V3,
    pub drag: f32,
    pub hit_radius: f32,
    pub roots: Vec<usize>,
    pub colliders: Vec<Collider>,
}

pub struct Avatar {
    pub title: String,
    pub licence: String,
    pub nodes: Vec<Node>,
    /// VRM humanoid bone name (`leftUpperArm`) → node.
    pub humanoid: HashMap<String, usize>,
    /// Nodes in spring-bone chains (roots and their descendants).
    pub spring: HashSet<usize>,
    /// VRM 0.x spring-bone groups with their settings (D15).
    pub spring_groups: Vec<SpringGroup>,
    pub parts: Vec<Part>,
    /// Encoded image files (PNG) by image index.
    pub images: Vec<Vec<u8>>,
}

struct Glb {
    j: Value,
    bin: Vec<u8>,
}

pub fn load(bytes: &[u8]) -> Result<Avatar> {
    let g = parse_glb(bytes)?;
    let j = &g.j;
    let vrm = j
        .pointer("/extensions/VRM")
        .ok_or_else(|| anyhow!("no VRM 0.x extension (VRM 1.0 is not supported yet)"))?;

    let jnodes = arr(j, "nodes")?;
    let mut parent = vec![None; jnodes.len()];
    for (i, n) in jnodes.iter().enumerate() {
        for c in n
            .get("children")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            parent[c.as_u64().ok_or_else(|| anyhow!("bad child"))? as usize] = Some(i);
        }
    }
    let local: Vec<Xf> = jnodes.iter().map(node_local).collect::<Result<_>>()?;
    let mut world = vec![None; jnodes.len()];
    for i in 0..jnodes.len() {
        world_of(i, &parent, &local, &mut world);
    }
    let nodes: Vec<Node> = jnodes
        .iter()
        .enumerate()
        .map(|(i, n)| Node {
            name: n
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            parent: parent[i],
            world: world[i].unwrap(),
        })
        .collect();

    let mut humanoid = HashMap::new();
    for b in vrm
        .pointer("/humanoid/humanBones")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let (Some(name), Some(node)) = (
            b.get("bone").and_then(Value::as_str),
            b.get("node").and_then(Value::as_u64),
        ) {
            humanoid.insert(name.to_string(), node as usize);
        }
    }

    let mut spring = HashSet::new();
    let mut spring_groups = Vec::new();
    let colliders: Vec<Vec<Collider>> = vrm
        .pointer("/secondaryAnimation/colliderGroups")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|cg| {
            let node = cg.get("node").and_then(Value::as_u64).unwrap_or(0) as usize;
            cg.get("colliders")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|c| Collider {
                    node,
                    offset: xyz(c.get("offset")),
                    radius: c.get("radius").and_then(Value::as_f64).unwrap_or(0.0) as f32,
                })
                .collect()
        })
        .collect();
    for g in vrm
        .pointer("/secondaryAnimation/boneGroups")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let num = |k: &str, d: f64| g.get(k).and_then(Value::as_f64).unwrap_or(d) as f32;
        let mut group = SpringGroup {
            name: g
                .get("comment")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            // UniVRM 0.x spells it "stiffiness".
            stiffness: num("stiffiness", 1.0),
            gravity_power: num("gravityPower", 0.0),
            gravity_dir: xyz(g.get("gravityDir")),
            drag: num("dragForce", 0.4),
            hit_radius: num("hitRadius", 0.02),
            roots: Vec::new(),
            colliders: Vec::new(),
        };
        for ci in g
            .get("colliderGroups")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(cs) = ci.as_u64().and_then(|i| colliders.get(i as usize)) {
                group.colliders.extend(cs.iter().cloned());
            }
        }
        for b in g
            .get("bones")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(b) = b.as_u64() {
                let b = b as usize;
                group.roots.push(b);
                for (i, _) in nodes.iter().enumerate() {
                    if is_descendant(i, b, &parent) {
                        spring.insert(i);
                    }
                }
            }
        }
        spring_groups.push(group);
    }

    let mut first_person_only = HashSet::new();
    for a in vrm
        .pointer("/firstPerson/meshAnnotations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if a.get("firstPersonFlag").and_then(Value::as_str) == Some("FirstPersonOnly")
            && let Some(m) = a.get("mesh").and_then(Value::as_u64)
        {
            first_person_only.insert(m as usize);
        }
    }
    // The neutral face is "all blendshapes at 0" unless the neutral group binds some.
    for grp in vrm
        .pointer("/blendShapeMaster/blendShapeGroups")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if grp.get("presetName").and_then(Value::as_str) == Some("neutral")
            && grp
                .get("binds")
                .and_then(Value::as_array)
                .is_some_and(|b| !b.is_empty())
        {
            bail!(
                "the VRM's neutral expression binds blendshapes; applying them is not implemented (B7)"
            );
        }
    }

    let mut parts = Vec::new();
    for (ni, n) in jnodes.iter().enumerate() {
        let Some(mesh) = n.get("mesh").and_then(Value::as_u64) else {
            continue;
        };
        let mesh = mesh as usize;
        let skin = n.get("skin").and_then(Value::as_u64).map(|s| s as usize);
        let jm = j
            .pointer(&format!("/meshes/{mesh}"))
            .ok_or_else(|| anyhow!("mesh {mesh} missing"))?;
        let mesh_name = jm.get("name").and_then(Value::as_str).unwrap_or("");
        for (pi, p) in jm
            .get("primitives")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            let id = format!("{}/{}#{pi}", nodes[ni].name, mesh_name);
            let part = primitive(&g, p, ni, skin, &nodes).with_context(|| id.clone())?;
            let (image, unlit) = material(j, p)?;
            parts.push(Part {
                id,
                node_name: nodes[ni].name.clone(),
                image,
                unlit,
                first_person_only: first_person_only.contains(&mesh),
                ..part
            });
        }
    }

    let mut images = Vec::new();
    for im in arr(j, "images")? {
        let bv = im
            .get("bufferView")
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                anyhow!("image without a bufferView (external images are not supported)")
            })?;
        images.push(g.view(bv as usize)?.to_vec());
    }

    let meta = vrm.get("meta");
    let meta_str = |k: &str| {
        meta.and_then(|m| m.get(k))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    Ok(Avatar {
        title: meta_str("title"),
        licence: meta_str("licenseName"),
        nodes,
        humanoid,
        spring,
        spring_groups,
        parts,
        images,
    })
}

fn xyz(v: Option<&Value>) -> V3 {
    let f = |k: &str| {
        v.and_then(|v| v.get(k))
            .and_then(Value::as_f64)
            .unwrap_or(0.0) as f32
    };
    [f("x"), f("y"), f("z")]
}

fn arr<'a>(j: &'a Value, k: &str) -> Result<&'a Vec<Value>> {
    j.get(k)
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("glTF has no `{k}`"))
}

fn floats(v: Option<&Value>) -> Option<Vec<f32>> {
    v?.as_array()?
        .iter()
        .map(|x| x.as_f64().map(|f| f as f32))
        .collect()
}

fn node_local(n: &Value) -> Result<Xf> {
    if let Some(m) = floats(n.get("matrix")) {
        return Ok(Xf::from_gltf_matrix(&m));
    }
    let t = floats(n.get("translation")).unwrap_or(vec![0.0; 3]);
    let q = floats(n.get("rotation")).unwrap_or(vec![0.0, 0.0, 0.0, 1.0]);
    let s = floats(n.get("scale")).unwrap_or(vec![1.0; 3]);
    if t.len() != 3 || q.len() != 4 || s.len() != 3 {
        bail!("bad node transform");
    }
    Ok(Xf::from_trs(
        [t[0], t[1], t[2]],
        [q[0], q[1], q[2], q[3]],
        [s[0], s[1], s[2]],
    ))
}

fn world_of(i: usize, parent: &[Option<usize>], local: &[Xf], world: &mut Vec<Option<Xf>>) -> Xf {
    if let Some(w) = world[i] {
        return w;
    }
    let w = match parent[i] {
        Some(p) => world_of(p, parent, local, world).then(&local[i]),
        None => local[i],
    };
    world[i] = Some(w);
    w
}

fn is_descendant(i: usize, root: usize, parent: &[Option<usize>]) -> bool {
    let mut n = Some(i);
    while let Some(k) = n {
        if k == root {
            return true;
        }
        n = parent[k];
    }
    false
}

fn material(j: &Value, p: &Value) -> Result<(Option<usize>, bool)> {
    let Some(mi) = p.get("material").and_then(Value::as_u64) else {
        return Ok((None, false));
    };
    let m = j
        .pointer(&format!("/materials/{mi}"))
        .ok_or_else(|| anyhow!("material {mi} missing"))?;
    let unlit = m.pointer("/extensions/KHR_materials_unlit").is_some();
    let Some(tex) = m.pointer("/pbrMetallicRoughness/baseColorTexture") else {
        return Ok((None, unlit));
    };
    if let Some(tt) = tex.pointer("/extensions/KHR_texture_transform") {
        let off = floats(tt.get("offset")).unwrap_or(vec![0.0, 0.0]);
        let sc = floats(tt.get("scale")).unwrap_or(vec![1.0, 1.0]);
        let rot = tt.get("rotation").and_then(Value::as_f64).unwrap_or(0.0);
        if off != [0.0, 0.0] || sc != [1.0, 1.0] || rot != 0.0 {
            bail!("material {mi}: texture transforms other than identity are not supported yet");
        }
    }
    let ti = tex
        .get("index")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("texture without index"))?;
    let src = j
        .pointer(&format!("/textures/{ti}/source"))
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("texture {ti} has no source"))?;
    Ok((Some(src as usize), unlit))
}

fn primitive(g: &Glb, p: &Value, node: usize, skin: Option<usize>, nodes: &[Node]) -> Result<Part> {
    if p.get("mode").and_then(Value::as_u64).unwrap_or(4) != 4 {
        bail!("only triangle lists are supported");
    }
    let attr = |k: &str| {
        p.pointer(&format!("/attributes/{k}"))
            .and_then(Value::as_u64)
            .map(|a| a as usize)
    };
    let pos = g.accessor_f32(attr("POSITION").ok_or_else(|| anyhow!("no POSITION"))?, 3)?;
    let n = pos.len() / 3;
    let nor = match attr("NORMAL") {
        Some(a) => g.accessor_f32(a, 3)?,
        None => vec![0.0; n * 3],
    };
    let uv = match attr("TEXCOORD_0") {
        Some(a) => g.accessor_f32(a, 2)?,
        None => vec![0.0; n * 2],
    };
    let idx = match p.get("indices").and_then(Value::as_u64) {
        Some(a) => g.accessor_u32(a as usize)?,
        None => (0..n as u32).collect(),
    };

    let mut positions = Vec::with_capacity(n);
    let mut normals = Vec::with_capacity(n);
    let mut dominant = Vec::with_capacity(n);
    match (skin, attr("JOINTS_0"), attr("WEIGHTS_0")) {
        (Some(s), Some(ja), Some(wa)) => {
            let js =
                g.j.pointer(&format!("/skins/{s}"))
                    .ok_or_else(|| anyhow!("skin {s} missing"))?;
            let joints: Vec<usize> = js
                .get("joints")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_u64)
                .map(|v| v as usize)
                .collect();
            let ibm = match js.get("inverseBindMatrices").and_then(Value::as_u64) {
                Some(a) => g.accessor_f32(a as usize, 16)?,
                None => [
                    1.0, 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
                ]
                .repeat(joints.len()),
            };
            let skin_m: Vec<Xf> = joints
                .iter()
                .enumerate()
                .map(|(k, &jn)| {
                    nodes[jn]
                        .world
                        .then(&Xf::from_gltf_matrix(&ibm[k * 16..k * 16 + 16]))
                })
                .collect();
            let jv = g.accessor_u32(ja)?;
            let wv = g.accessor_f32(wa, 4)?;
            for v in 0..n {
                let p = [pos[v * 3], pos[v * 3 + 1], pos[v * 3 + 2]];
                let nn = [nor[v * 3], nor[v * 3 + 1], nor[v * 3 + 2]];
                let (mut acc_p, mut acc_n, mut best) =
                    ([0.0f32; 3], [0.0f32; 3], (f32::MIN, 0usize));
                for k in 0..4 {
                    let w = wv[v * 4 + k];
                    let jk = jv[v * 4 + k] as usize;
                    if w <= 0.0 {
                        continue;
                    }
                    let m = skin_m
                        .get(jk)
                        .ok_or_else(|| anyhow!("joint index {jk} out of range"))?;
                    acc_p = crate::math::add(acc_p, crate::math::mul(m.apply(p), w));
                    acc_n = crate::math::add(acc_n, crate::math::mul(m.r.apply(nn), w));
                    if w > best.0 {
                        best = (w, joints[jk]);
                    }
                }
                positions.push(acc_p);
                normals.push(norm(acc_n));
                dominant.push(best.1);
            }
        }
        _ => {
            let w = nodes[node].world;
            for v in 0..n {
                positions.push(w.apply([pos[v * 3], pos[v * 3 + 1], pos[v * 3 + 2]]));
                normals.push(norm(w.r.apply([
                    nor[v * 3],
                    nor[v * 3 + 1],
                    nor[v * 3 + 2],
                ])));
                dominant.push(nodes[node].parent.unwrap_or(node));
            }
        }
    }
    let uvs = (0..n).map(|v| [uv[v * 2], uv[v * 2 + 1]]).collect();
    let triangles = idx
        .as_chunks::<3>()
        .0
        .iter()
        .map(|t| [t[0], t[1], t[2]])
        .collect();
    Ok(Part {
        id: String::new(),
        node_name: String::new(),
        positions,
        normals,
        uvs,
        dominant,
        triangles,
        image: None,
        unlit: false,
        first_person_only: false,
    })
}

fn parse_glb(d: &[u8]) -> Result<Glb> {
    if d.len() < 20 || &d[0..4] != b"glTF" {
        bail!("not a glb/VRM file");
    }
    let u32_at = |o: usize| u32::from_le_bytes(d[o..o + 4].try_into().unwrap()) as usize;
    let jlen = u32_at(12);
    if &d[16..20] != b"JSON" || 20 + jlen > d.len() {
        bail!("glb has no JSON chunk");
    }
    let j: Value = serde_json::from_slice(&d[20..20 + jlen]).context("glb JSON")?;
    let mut bin = Vec::new();
    let o = 20 + jlen;
    if o + 8 <= d.len() && &d[o + 4..o + 8] == b"BIN\0" {
        let blen = u32_at(o);
        bin = d
            .get(o + 8..o + 8 + blen)
            .ok_or_else(|| anyhow!("BIN chunk truncated"))?
            .to_vec();
    }
    Ok(Glb { j, bin })
}

impl Glb {
    fn view(&self, bv: usize) -> Result<&[u8]> {
        let v = self
            .j
            .pointer(&format!("/bufferViews/{bv}"))
            .ok_or_else(|| anyhow!("bufferView {bv} missing"))?;
        if v.get("buffer").and_then(Value::as_u64) != Some(0) {
            bail!("only the glb's own buffer is supported");
        }
        let off = v.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
        let len = v
            .get("byteLength")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow!("bufferView without length"))? as usize;
        self.bin
            .get(off..off + len)
            .ok_or_else(|| anyhow!("bufferView {bv} outside the buffer"))
    }

    /// Accessor elements as raw component values: (bytes, component type, count, components, normalized).
    fn raw(&self, a: usize) -> Result<(Vec<&[u8]>, u64, bool)> {
        let acc = self
            .j
            .pointer(&format!("/accessors/{a}"))
            .ok_or_else(|| anyhow!("accessor {a} missing"))?;
        if acc.get("sparse").is_some() {
            bail!("sparse accessors are not supported");
        }
        let ct = acc
            .get("componentType")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let count = acc.get("count").and_then(Value::as_u64).unwrap_or(0) as usize;
        let comps = match acc.get("type").and_then(Value::as_str).unwrap_or("") {
            "SCALAR" => 1,
            "VEC2" => 2,
            "VEC3" => 3,
            "VEC4" => 4,
            "MAT4" => 16,
            t => bail!("accessor type {t} not supported"),
        };
        let size = match ct {
            5120 | 5121 => 1,
            5122 | 5123 => 2,
            5125 | 5126 => 4,
            _ => bail!("component type {ct} not supported"),
        };
        let bv = acc
            .get("bufferView")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow!("accessor without bufferView"))? as usize;
        let view = self.view(bv)?;
        let stride = self
            .j
            .pointer(&format!("/bufferViews/{bv}/byteStride"))
            .and_then(Value::as_u64)
            .map_or(size * comps, |s| s as usize);
        let off = acc.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
        let mut out = Vec::with_capacity(count * comps);
        for e in 0..count {
            for c in 0..comps {
                let at = off + e * stride + c * size;
                out.push(
                    view.get(at..at + size)
                        .ok_or_else(|| anyhow!("accessor {a} reads past its view"))?,
                );
            }
        }
        Ok((
            out,
            ct,
            acc.get("normalized")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        ))
    }

    fn accessor_f32(&self, a: usize, comps: usize) -> Result<Vec<f32>> {
        let (vals, ct, normalized) = self.raw(a)?;
        if vals.len() % comps != 0 {
            bail!("accessor {a} is not {comps}-component");
        }
        vals.iter()
            .map(|b| {
                Ok(match (ct, normalized) {
                    (5126, _) => f32::from_le_bytes((*b).try_into().unwrap()),
                    (5121, true) => b[0] as f32 / 255.0,
                    (5123, true) => u16::from_le_bytes((*b).try_into().unwrap()) as f32 / 65535.0,
                    (5121, false) => b[0] as f32,
                    (5123, false) => u16::from_le_bytes((*b).try_into().unwrap()) as f32,
                    _ => bail!("accessor {a}: component type {ct} as float not supported"),
                })
            })
            .collect()
    }

    fn accessor_u32(&self, a: usize) -> Result<Vec<u32>> {
        let (vals, ct, _) = self.raw(a)?;
        vals.iter()
            .map(|b| {
                Ok(match ct {
                    5121 => b[0] as u32,
                    5123 => u16::from_le_bytes((*b).try_into().unwrap()) as u32,
                    5125 => u32::from_le_bytes((*b).try_into().unwrap()),
                    _ => bail!("accessor {a}: component type {ct} as integer not supported"),
                })
            })
            .collect()
    }
}
