//! VRM avatar → GoldSrc playermodel, with animations from a donor MDL (D7).
//!
//! Retarget (D4): the new skeleton keeps the donor's bone rotations, so the
//! donor's rotation channels play unchanged, but takes its bone offsets from
//! the avatar. Each avatar bone gets a frame `Lfr` in avatar space: origin at
//! its joint, rotated like the donor bone after aligning the donor's bone
//! direction with the avatar's. A vertex rigidly bound to that bone (D3) is
//! stored as `Lfr⁻¹ · p`, so it lands where it was relative to its bone.
//! Child offsets are the child joint seen from the parent's `Lfr`, which keeps
//! every joint attached.

use std::collections::{HashMap, HashSet};

use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};

use crate::math::{M3, V3, Xf, cross, dot, len, mul, norm, sub};
use crate::mdl::{Bodypart, Bone, Hitbox, Mesh, Model, SubModel, Texture};
use crate::preview::{bone_worlds, winding_sign};
use crate::texture;
use crate::vrm::Avatar;

/// Limits (A11): HL values, which Sven keeps except for textures.
pub const MAX_BONES: usize = 128;
/// Vertices, and separately normals, per mesh.
pub const MAX_VERTS: usize = 2048;
pub const MAX_TRIS_PER_MESH: usize = 4080;
pub const MAX_TRIS: usize = 20000;
/// Sven Co-op playermodel guidance (A11): warn above this.
pub const SVEN_POLY_GUIDE: usize = 9000;
/// D6: textures at most 512².
pub const MAX_TEXTURE: u32 = 512;
const FLATSHADE: i32 = 0x1;

/// VRM humanoid bone → donor Bip01 bone (D1).
const HUMANOID: &[(&str, &str)] = &[
    ("hips", "Bip01 Pelvis"),
    ("spine", "Bip01 Spine"),
    ("chest", "Bip01 Spine1"),
    ("upperChest", "Bip01 Spine2"),
    ("neck", "Bip01 Neck"),
    ("head", "Bip01 Head"),
    ("leftShoulder", "Bip01 L Arm"),
    ("leftUpperArm", "Bip01 L Arm1"),
    ("leftLowerArm", "Bip01 L Arm2"),
    ("leftHand", "Bip01 L Hand"),
    ("rightShoulder", "Bip01 R Arm"),
    ("rightUpperArm", "Bip01 R Arm1"),
    ("rightLowerArm", "Bip01 R Arm2"),
    ("rightHand", "Bip01 R Hand"),
    ("leftUpperLeg", "Bip01 L Leg"),
    ("leftLowerLeg", "Bip01 L Leg1"),
    ("leftFoot", "Bip01 L Foot"),
    ("rightUpperLeg", "Bip01 R Leg"),
    ("rightLowerLeg", "Bip01 R Leg1"),
    ("rightFoot", "Bip01 R Foot"),
];

/// Bone chains, root to tip; a bone aims at the next mapped bone in its chain.
const CHAINS: &[&[&str]] = &[
    &[
        "Bip01 Pelvis",
        "Bip01 Spine",
        "Bip01 Spine1",
        "Bip01 Spine2",
        "Bip01 Spine3",
        "Bip01 Neck",
        "Bip01 Head",
    ],
    &[
        "Bip01 L Arm",
        "Bip01 L Arm1",
        "Bip01 L Arm2",
        "Bip01 L Hand",
    ],
    &[
        "Bip01 R Arm",
        "Bip01 R Arm1",
        "Bip01 R Arm2",
        "Bip01 R Hand",
    ],
    &["Bip01 L Leg", "Bip01 L Leg1", "Bip01 L Foot"],
    &["Bip01 R Leg", "Bip01 R Leg1", "Bip01 R Foot"],
];

pub struct Options {
    pub name: String,
    /// Model height in game units; default: the donor's (B8).
    pub height: Option<f32>,
    /// Clean-manifest overrides: part id → keep (B4).
    pub keep: HashMap<String, bool>,
}

pub struct Converted {
    pub model: Model,
    pub manifest: Value,
    pub report: Vec<String>,
}

fn is_standard(name: &str) -> bool {
    name == "Bip01" || CHAINS.iter().any(|c| c.contains(&name))
}

fn chain_of(name: &str) -> Option<(&'static [&'static str], usize)> {
    CHAINS
        .iter()
        .find_map(|c| c.iter().position(|n| *n == name).map(|i| (*c, i)))
}

pub fn convert(av: &Avatar, donor: &Model, o: &Options) -> Result<Converted> {
    let mut report = vec![format!("avatar: {:?}, licence {:?}", av.title, av.licence)];
    // A13: warn, don't block.
    let licence = av.licence.to_lowercase();
    if licence.contains("prohibit") || licence.contains("disallow") || licence.is_empty() {
        report.push(format!(
            "warning: licence {:?} does not clearly allow redistribution; a server playermodel is downloaded by every client, so keep this output local (A6, A13)",
            av.licence
        ));
    }

    // Clean (B3, B4): rules, then manifest overrides.
    let mut manifest = Vec::new();
    let mut kept = Vec::new();
    for (i, p) in av.parts.iter().enumerate() {
        let lower = p.id.to_lowercase();
        let (keep, rule) = if p.first_person_only {
            (false, "first-person-only mesh")
        } else if lower.contains("highlight") || lower.contains("eyehi") || lower.contains("eye_hi")
        {
            (false, "eye highlight shell")
        } else {
            (true, "")
        };
        let keep = o.keep.get(&p.id).copied().unwrap_or(keep);
        manifest.push(
            json!({"part": p.id, "keep": keep, "rule": rule, "triangles": p.triangles.len()}),
        );
        if keep {
            kept.push(i);
        } else {
            report.push(format!(
                "dropped part {} ({})",
                p.id,
                if rule.is_empty() { "manifest" } else { rule }
            ));
        }
    }
    if kept.is_empty() {
        bail!("every mesh part was dropped");
    }

    // Axes: glTF is Y-up; GoldSrc is X forward, Y left, Z up.
    let joint = |b: &str| av.humanoid.get(b).map(|&n| av.nodes[n].world.t);
    let need = |b: &str| joint(b).ok_or_else(|| anyhow!("the VRM has no humanoid `{b}` bone"));
    let up: V3 = [0.0, 1.0, 0.0];
    let across = sub(need("leftUpperArm")?, need("rightUpperArm")?);
    let left = norm(sub(across, mul(up, dot(across, up))));
    let fwd = norm(cross(left, up));
    if let (Some(foot), Some(toes)) = (joint("leftFoot"), joint("leftToes"))
        && dot(sub(toes, foot), fwd) < 0.0
    {
        report.push(
            "warning: the toes point backwards relative to the arms; check the facing".into(),
        );
    }
    // Turn the avatar to face the way the donor's rest pose faces (yuki_ref's
    // root has a -90° yaw), so the bone frames' twist about the vertical
    // matches the mesh. Otherwise every pose plays turned sideways.
    let dw = bone_worlds(donor, None);
    let dname = |n: &str| {
        donor
            .bones
            .iter()
            .position(|b| b.name == n)
            .map(|i| dw[i].t)
    };
    let (dla, dra) = (
        dname("Bip01 L Arm1").ok_or_else(|| anyhow!("the donor has no Bip01 L Arm1"))?,
        dname("Bip01 R Arm1").ok_or_else(|| anyhow!("the donor has no Bip01 R Arm1"))?,
    );
    let d_across = sub(dla, dra);
    let d_left = norm([d_across[0], d_across[1], 0.0]);
    let d_fwd = cross(d_left, [0.0, 0.0, 1.0]);
    let to_donor_facing = M3([
        [d_fwd[0], d_left[0], 0.0],
        [d_fwd[1], d_left[1], 0.0],
        [d_fwd[2], d_left[2], 1.0],
    ]);
    let c = to_donor_facing.mul(&M3::rows(fwd, left, up));

    // Scale (B8): match the donor's mesh height unless a height is given.
    let zs = kept
        .iter()
        .flat_map(|&i| &av.parts[i].positions)
        .map(|p| dot(*p, up));
    let (lmin, lmax) = zs.fold((f32::MAX, f32::MIN), |(a, b), z| (a.min(z), b.max(z)));
    let (dmin, dmax) = donor
        .bodyparts
        .iter()
        .filter_map(|b| b.models.first())
        .flat_map(|sm| {
            sm.verts
                .iter()
                .zip(&sm.vert_bones)
                .map(|(v, &b)| dw[b as usize].apply(*v)[2])
        })
        .fold((f32::MAX, f32::MIN), |(a, b), z| (a.min(z), b.max(z)));
    let target = o.height.unwrap_or(dmax - dmin);
    let s = target / (lmax - lmin);
    let to_gs = |p: V3| mul(c.apply(p), s);
    let ground = lmin * s;
    report.push(format!(
        "height: avatar {:.3} m → {target:.2} units (scale {s:.3}); donor mesh {:.2} units",
        lmax - lmin,
        dmax - dmin
    ));

    // Donor bones: keep the standard Bip01 set (D2), in donor order.
    let dindex: HashMap<&str, usize> = donor
        .bones
        .iter()
        .enumerate()
        .map(|(i, b)| (b.name.as_str(), i))
        .collect();
    let missing: Vec<&str> = CHAINS
        .iter()
        .flat_map(|c| c.iter())
        .chain(std::iter::once(&"Bip01"))
        .filter(|n| !dindex.contains_key(**n) && **n != "Bip01 Spine2" && **n != "Bip01 Spine3")
        .copied()
        .collect();
    if !missing.is_empty() {
        bail!("the donor lacks standard bones: {missing:?}");
    }
    let kept_donor: Vec<usize> = (0..donor.bones.len())
        .filter(|&i| is_standard(&donor.bones[i].name))
        .collect();
    let dropped: Vec<&str> = donor
        .bones
        .iter()
        .filter(|b| !is_standard(&b.name))
        .map(|b| b.name.as_str())
        .collect();
    report.push(format!("donor bones dropped (D2): {dropped:?}"));
    let d_pos = |n: &str| dw[dindex[n]].t;

    // Mapped donor bone → avatar node.
    let mut mapped: HashMap<&str, usize> = HashMap::new();
    for (vb, db) in HUMANOID {
        if let (Some(&node), true) = (av.humanoid.get(*vb), dindex.contains_key(db)) {
            mapped.insert(db, node);
        }
    }
    if !mapped.contains_key("Bip01 Pelvis") {
        bail!("the VRM has no hips bone");
    }
    let l_pos = |n: &str| to_gs(av.nodes[mapped[n]].world.t);
    let next_mapped = |chain: &[&'static str], i: usize| {
        chain[i + 1..]
            .iter()
            .copied()
            .find(|n| mapped.contains_key(n))
    };
    let prev_mapped = |chain: &[&'static str], i: usize| {
        chain[..i]
            .iter()
            .rev()
            .copied()
            .find(|n| mapped.contains_key(n))
    };

    // Each mapped bone's frame in avatar space.
    let mut q: HashMap<&str, M3> = HashMap::new();
    for chain in CHAINS {
        for (i, &n) in chain.iter().enumerate() {
            if !mapped.contains_key(n) {
                continue;
            }
            let qn = match next_mapped(chain, i) {
                Some(a) => M3::align(norm(sub(d_pos(a), d_pos(n))), norm(sub(l_pos(a), l_pos(n)))),
                None => prev_mapped(chain, i).map_or(M3::IDENTITY, |p| q[p]),
            };
            q.insert(n, qn);
        }
    }
    let lfr_of = |n: &str| Xf::new(q[n].mul(&dw[dindex[n]].r), l_pos(n));

    // Root height ratio: the avatar's hips above its feet against the donor's.
    let dground = dmin;
    let rp = (l_pos("Bip01 Pelvis")[2] - ground) / (d_pos("Bip01 Pelvis")[2] - dground);
    report.push(format!(
        "hip height ratio {rp:.3} (root motion and pelvis offset scale by it)"
    ));
    let seg_ratio = |n: &str, parent: Option<&str>| -> f32 {
        let Some((chain, i)) = chain_of(n) else {
            return rp;
        };
        let a = prev_mapped(chain, i).or(parent.filter(|p| mapped.contains_key(p)));
        let b = if mapped.contains_key(n) {
            Some(n)
        } else {
            next_mapped(chain, i)
        };
        match (a, b) {
            (Some(a), Some(b)) => {
                let d = len(sub(d_pos(b), d_pos(a)));
                if d > 1e-4 {
                    len(sub(l_pos(b), l_pos(a))) / d
                } else {
                    1.0
                }
            }
            _ => rp,
        }
    };

    // New skeleton: kept donor bones first, then avatar extras.
    let mut bones: Vec<Bone> = Vec::new();
    let mut new_world: Vec<Xf> = Vec::new();
    let mut lfr: Vec<Option<Xf>> = Vec::new();
    let mut maps: Vec<Option<Xf>> = Vec::new();
    let mut from_donor: Vec<Option<usize>> = Vec::new();
    let mut donor_to_new: HashMap<usize, usize> = HashMap::new();
    for &di in &kept_donor {
        let b = &donor.bones[di];
        let parent_new = usize::try_from(b.parent)
            .ok()
            .map(|p| {
                donor_to_new
                    .get(&p)
                    .copied()
                    .ok_or_else(|| anyhow!("{}: parent bone was dropped", b.name))
            })
            .transpose()?;
        let donor_off: V3 = [b.value[0], b.value[1], b.value[2]];
        let pname = usize::try_from(b.parent)
            .ok()
            .map(|p| donor.bones[p].name.as_str());
        let (off, k) = match pname {
            None => (mul(donor_off, rp), rp),
            Some(p) if mapped.contains_key(b.name.as_str()) && mapped.contains_key(p) => {
                let off = lfr_of(p).inverse_rigid().apply(l_pos(&b.name));
                let dl = len(donor_off);
                (off, if dl > 1e-4 { len(off) / dl } else { 1.0 })
            }
            Some(p) => {
                let r = seg_ratio(&b.name, Some(p));
                (mul(donor_off, r), r)
            }
        };
        let mut nb = b.clone();
        nb.value[..3].copy_from_slice(&off);
        for v in &mut nb.scale[..3] {
            *v *= k;
        }
        nb.parent = parent_new.map_or(-1, |p| p as i32);
        let local = Xf::from_gs(&nb.value);
        let world = parent_new.map_or(local, |p| new_world[p].then(&local));
        let fr = mapped
            .contains_key(b.name.as_str())
            .then(|| lfr_of(&b.name));
        donor_to_new.insert(di, bones.len());
        maps.push(fr.map(|f| world.then(&f.inverse_rigid())));
        lfr.push(fr);
        new_world.push(world);
        from_donor.push(Some(di));
        bones.push(nb);
    }
    let pelvis = donor_to_new[&dindex["Bip01 Pelvis"]];
    let mut node_bone: HashMap<usize, usize> = mapped
        .iter()
        .map(|(n, &node)| (node, donor_to_new[&dindex[n]]))
        .collect();
    let resolve_in = |node: usize, nb: &HashMap<usize, usize>| {
        let mut n = Some(node);
        while let Some(k) = n {
            if let Some(&b) = nb.get(&k) {
                return b;
            }
            n = av.nodes[k].parent;
        }
        pelvis
    };

    // Extras (D2): spring chains that carry vertices, plus their chain ancestors.
    let mut used: HashSet<usize> = HashSet::new();
    for &i in &kept {
        for &d in &av.parts[i].dominant {
            let mut n = Some(d);
            while let Some(k) = n {
                if !av.spring.contains(&k) || !used.insert(k) {
                    break;
                }
                n = av.nodes[k].parent;
            }
        }
    }
    let depth = |mut n: usize| {
        let mut d = 0;
        while let Some(p) = av.nodes[n].parent {
            n = p;
            d += 1;
        }
        d
    };
    let mut extras: Vec<usize> = used
        .into_iter()
        .filter(|n| !node_bone.contains_key(n))
        .collect();
    extras.sort_by_key(|&n| (depth(n), n));
    for &e in &extras {
        let parent = av.nodes[e]
            .parent
            .map_or(pelvis, |p| resolve_in(p, &node_bone));
        let m = maps[parent]
            .ok_or_else(|| anyhow!("{}: parent bone has no avatar frame", av.nodes[e].name))?;
        let w = av.nodes[e].world;
        let fr = Xf::new(c.mul(&w.r).mul(&c.transpose()), to_gs(w.t));
        let world = m.then(&fr);
        let local = new_world[parent].inverse_rigid().then(&world);
        let a = local.r.to_gs_angles();
        let mut name: String = av.nodes[e].name.chars().take(31).collect();
        if bones.iter().any(|b| b.name == name) {
            name = format!(
                "{}_{}",
                name.chars().take(26).collect::<String>(),
                bones.len()
            );
        }
        node_bone.insert(e, bones.len());
        bones.push(Bone {
            name,
            parent: parent as i32,
            flags: 0,
            controllers: [-1; 6],
            value: [local.t[0], local.t[1], local.t[2], a[0], a[1], a[2]],
            scale: [1.0 / 256.0, 1.0 / 256.0, 1.0 / 256.0, 1e-4, 1e-4, 1e-4],
        });
        new_world.push(world);
        lfr.push(Some(fr));
        maps.push(Some(m));
        from_donor.push(None);
    }
    if bones.len() > MAX_BONES {
        bail!("{} bones, over the {MAX_BONES} limit", bones.len());
    }
    report.push(format!(
        "bones: {} ({} from the donor, {} avatar chain bones); mapped {:?}",
        bones.len(),
        kept_donor.len(),
        extras.len(),
        HUMANOID
            .iter()
            .filter(|(_, d)| mapped.contains_key(d))
            .map(|(v, d)| format!("{v}→{d}"))
            .collect::<Vec<_>>()
    ));

    // Texture (C5, C6, D6, D10): one shared base-colour image.
    let images: HashSet<Option<usize>> = kept.iter().map(|&i| av.parts[i].image).collect();
    if images.len() != 1 {
        bail!(
            "the kept parts use {} different textures; atlasing is not implemented yet (C5)",
            images.len()
        );
    }
    let image = images
        .into_iter()
        .next()
        .unwrap()
        .ok_or_else(|| anyhow!("the avatar has no base colour texture"))?;
    let tex = texture::goldsrc_texture(&av.images[image], MAX_TEXTURE)?;
    let unlit = kept.iter().all(|&i| av.parts[i].unlit);
    let (tw, th) = (tex.width as f32, tex.height as f32);
    report.push(format!(
        "texture: {}x{}, {} colours, flags {}",
        tex.width,
        tex.height,
        tex.palette.len(),
        if unlit { "flatshade (D10)" } else { "none" }
    ));

    // Triangles with rigid bone binding (D3) in bone-local space.
    let flip = winding_sign(donor) < 0.0;
    use crate::reduce::Vtx as Corner;
    let mut tris: Vec<[Corner; 3]> = Vec::new();
    let mut degenerate = 0;
    let mut bone_hits = vec![0usize; bones.len()];
    for &pi in &kept {
        let part = &av.parts[pi];
        for t in &part.triangles {
            let mut idx = t.map(|v| v as usize);
            if idx.iter().any(|&v| v >= part.positions.len()) {
                bail!("{}: index out of range", part.id);
            }
            let world: Vec<V3> = idx.iter().map(|&v| to_gs(part.positions[v])).collect();
            if len(cross(sub(world[1], world[0]), sub(world[2], world[0]))) < 1e-8 {
                degenerate += 1;
                continue;
            }
            if flip {
                idx.swap(1, 2);
            }
            let corner = |v: usize| {
                let bone = resolve_in(part.dominant[v], &node_bone);
                let fr = lfr[bone].expect("bones that carry vertices have frames");
                let st = [
                    (part.uvs[v][0] * tw).round().clamp(-32768.0, 32767.0) as i16,
                    (part.uvs[v][1] * th).round().clamp(-32768.0, 32767.0) as i16,
                ];
                Corner {
                    bone,
                    p: fr.inverse_rigid().apply(to_gs(part.positions[v])),
                    n: norm(fr.r.transpose().apply(c.apply(part.normals[v]))),
                    st,
                }
            };
            let tri = [corner(idx[0]), corner(idx[1]), corner(idx[2])];
            for k in &tri {
                bone_hits[k.bone] += 1;
            }
            tris.push(tri);
        }
    }
    // Lossless edge collapse (C1, C2, C3).
    let before_reduce = tris.len();
    let (mut tris, kept_by) = crate::reduce::reduce_with_stats(&tris);
    report.push(format!(
        "edge collapse: vertices kept by rule {kept_by:?} (C3)"
    ));
    tris.sort_by_key(|t| t[0].bone);
    if tris.len() > MAX_TRIS {
        bail!("{} triangles, over the {MAX_TRIS} limit", tris.len());
    }
    if tris.len() > SVEN_POLY_GUIDE {
        report.push(format!(
            "warning: {} triangles, over Sven's ~{SVEN_POLY_GUIDE} playermodel guidance (A11)",
            tris.len()
        ));
    }

    // Weld (C2) and fill bodyparts up to the vertex limit (D5).
    type Key = (usize, [i32; 3]);
    #[derive(Default)]
    struct Part {
        verts: Vec<V3>,
        vert_bones: Vec<u8>,
        vmap: HashMap<Key, i16>,
        norms: Vec<V3>,
        norm_bones: Vec<u8>,
        nmap: HashMap<Key, i16>,
        tris: Vec<[[i16; 4]; 3]>,
    }
    // D12: flatshade ignores most of the normal, so weld normals within ~1°.
    let nq = if unlit { 57.3 } else { 1e3 };
    let qkey = |b: usize, v: V3, s: f32| (b, v.map(|x| (x * s).round() as i32));
    let mut parts: Vec<Part> = vec![Part::default()];
    let mut welded_degenerate = 0;
    for t in &tris {
        let cur = parts.last().unwrap();
        let new_v = t
            .iter()
            .filter(|k| !cur.vmap.contains_key(&qkey(k.bone, k.p, 1e4)))
            .count();
        let new_n = t
            .iter()
            .filter(|k| !cur.nmap.contains_key(&qkey(k.bone, k.n, nq)))
            .count();
        if cur.verts.len() + new_v > MAX_VERTS
            || cur.norms.len() + new_n > MAX_VERTS
            || cur.tris.len() + 1 > MAX_TRIS_PER_MESH
        {
            parts.push(Part::default());
        }
        let cur = parts.last_mut().unwrap();
        let mut cmd = Vec::with_capacity(3);
        for k in t {
            let vk = qkey(k.bone, k.p, 1e4);
            let vi = match cur.vmap.get(&vk) {
                Some(&i) => i,
                None => {
                    let i = cur.verts.len() as i16;
                    cur.verts.push(k.p);
                    cur.vert_bones.push(k.bone as u8);
                    cur.vmap.insert(vk, i);
                    i
                }
            };
            let nk = qkey(k.bone, k.n, nq);
            let ni = match cur.nmap.get(&nk) {
                Some(&i) => i,
                None => {
                    let i = cur.norms.len() as i16;
                    cur.norms.push(k.n);
                    cur.norm_bones.push(k.bone as u8);
                    cur.nmap.insert(nk, i);
                    i
                }
            };
            cmd.push([vi, ni, k.st[0], k.st[1]]);
        }
        if cmd[0][0] == cmd[1][0] || cmd[1][0] == cmd[2][0] || cmd[0][0] == cmd[2][0] {
            welded_degenerate += 1;
            continue;
        }
        cur.tris.push([cmd[0], cmd[1], cmd[2]]);
    }
    let total_tris: usize = parts.iter().map(|p| p.tris.len()).sum();
    report.push(format!(
        "triangles: {} in, {degenerate} zero-area and {welded_degenerate} collapsed by welding dropped, {total_tris} out; zero-error edge collapse {before_reduce} → {} (C1)",
        kept.iter().map(|&i| av.parts[i].triangles.len()).sum::<usize>(),
        tris.len()
    ));

    let bodyparts: Vec<Bodypart> = parts
        .into_iter()
        .enumerate()
        .map(|(i, p)| {
            let name = if i == 0 {
                "body".to_string()
            } else {
                format!("body{}", i + 1)
            };
            let commands = crate::strip::strips(&p.tris);
            report.push(format!(
                "bodypart {name}: {} verts, {} normals, {} triangles in {} strips",
                p.verts.len(),
                p.norms.len(),
                p.tris.len(),
                commands.len()
            ));
            Bodypart {
                name: name.clone(),
                base: 1,
                models: vec![SubModel {
                    name,
                    kind: 0,
                    bounding_radius: 0.0,
                    vert_bones: p.vert_bones,
                    verts: p.verts,
                    norm_bones: p.norm_bones,
                    norms: p.norms,
                    meshes: vec![Mesh {
                        num_tris: p.tris.len() as i32,
                        skinref: 0,
                        num_norms: p.nmap.len() as i32,
                        norm_index: 0,
                        commands,
                    }],
                }],
            }
        })
        .collect();

    // Hitboxes (D14): the donor's set on the bones we keep. Bounds come from
    // our vertices on that bone, or the donor's box scaled like the bone.
    let mut bounds: Vec<Option<(V3, V3)>> = vec![None; bones.len()];
    for bp in &bodyparts {
        for sm in &bp.models {
            for (v, &b) in sm.verts.iter().zip(&sm.vert_bones) {
                let e = bounds[b as usize].get_or_insert((*v, *v));
                *e = (
                    std::array::from_fn(|k| e.0[k].min(v[k])),
                    std::array::from_fn(|k| e.1[k].max(v[k])),
                );
            }
        }
    }
    let mut hitboxes = Vec::new();
    for h in &donor.hitboxes {
        let di = h.bone as usize;
        let Some(&nb) = donor_to_new.get(&di) else {
            continue;
        };
        let (bbmin, bbmax) = bounds[nb].unwrap_or_else(|| {
            let ds = donor.bones[di].scale[0];
            let k = if ds.abs() > 0.0 {
                bones[nb].scale[0] / ds
            } else {
                1.0
            };
            (mul(h.bbmin, k), mul(h.bbmax, k))
        });
        hitboxes.push(Hitbox {
            bone: nb as i32,
            group: h.group,
            bbmin,
            bbmax,
        });
    }
    report.push(format!(
        "hitboxes: {} of the donor's {} (D14)",
        hitboxes.len(),
        donor.hitboxes.len()
    ));

    // Donor controllers and attachments, re-pointed (D8).
    let mut controller_map = HashMap::new();
    let mut bone_controllers = Vec::new();
    for (ci, bc) in donor.bone_controllers.iter().enumerate() {
        match donor_to_new.get(&(bc.bone as usize)) {
            Some(&nb) => {
                controller_map.insert(ci as i32, bone_controllers.len() as i32);
                bone_controllers.push(crate::mdl::BoneController {
                    bone: nb as i32,
                    ..bc.clone()
                });
            }
            None => report.push(format!(
                "dropped bone controller {ci} on {}",
                donor.bones[bc.bone as usize].name
            )),
        }
    }
    for b in &mut bones {
        for c in &mut b.controllers {
            *c = if *c < 0 {
                -1
            } else {
                controller_map.get(c).copied().unwrap_or(-1)
            };
        }
    }
    let attachments = donor
        .attachments
        .iter()
        .filter_map(|a| {
            donor_to_new
                .get(&(a.bone as usize))
                .map(|&nb| crate::mdl::Attachment {
                    bone: nb as i32,
                    ..a.clone()
                })
        })
        .collect::<Vec<_>>();
    report.push(format!(
        "controllers {}/{}, attachments {}/{} from the donor",
        bone_controllers.len(),
        donor.bone_controllers.len(),
        attachments.len(),
        donor.attachments.len()
    ));

    // Animations (D7): the donor's channels for its kept bones, extras static.
    let sequences = donor
        .sequences
        .iter()
        .map(|s| {
            let mut s = s.clone();
            s.blends = s
                .blends
                .iter()
                .map(|bl| {
                    from_donor
                        .iter()
                        .map(|d| d.map(|di| bl[di].clone()).unwrap_or_default())
                        .collect()
                })
                .collect();
            s
        })
        .collect();

    let path = format!("models/player/{0}/{0}.mdl", o.name);
    let mut seq_groups = donor.seq_groups.clone();
    for g in &mut seq_groups {
        g.name = path.clone();
    }
    let model = Model {
        name: path,
        eye_position: donor.eye_position,
        min: donor.min,
        max: donor.max,
        bbmin: donor.bbmin,
        bbmax: donor.bbmax,
        flags: donor.flags,
        bones,
        bone_controllers,
        hitboxes,
        sequences,
        seq_groups,
        textures: vec![Texture {
            name: format!("{}.bmp", o.name),
            flags: if unlit { FLATSHADE } else { 0 },
            width: tex.width as i32,
            height: tex.height as i32,
            palette: tex.palette_bytes(),
            pixels: tex.pixels,
        }],
        num_skinref: 1,
        skin_families: vec![vec![0]],
        bodyparts,
        attachments,
        num_transitions: donor.num_transitions,
        transitions: donor.transitions.clone(),
    };
    let unbound = bone_hits.iter().filter(|&&h| h == 0).count();
    report.push(format!(
        "{unbound} bones carry no vertices (expected for spine links, the root and chain joints)"
    ));
    Ok(Converted {
        model,
        manifest: Value::Array(manifest),
        report,
    })
}
