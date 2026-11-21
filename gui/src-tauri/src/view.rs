//! Data for the 3D viewer (H4): geometry in each vertex's bone space, so the
//! front end poses it with per-frame bone transforms, as the game does.

use libremodele::math::Xf;
use libremodele::mdl::Model;
use libremodele::preview;
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelView {
    /// The session generation this view belongs to; pass it back with
    /// every texture and frame request.
    pub generation: u64,
    pub name: String,
    pub bones: Vec<BoneView>,
    pub textures: Vec<TextureView>,
    /// One mesh per texture.
    pub meshes: Vec<MeshView>,
    pub sequences: Vec<SequenceView>,
    pub hitboxes: Vec<HitboxView>,
    /// Bones at and above this index follow the upper-body sequence; the
    /// rest follow the gait (legs) sequence, as Sven's player renderer does.
    pub gait_split: usize,
    pub triangles: usize,
}

#[derive(Serialize)]
pub struct BoneView {
    pub name: String,
    pub parent: i32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextureView {
    pub name: String,
    pub width: i32,
    pub height: i32,
    pub masked: bool,
}

#[derive(Serialize)]
pub struct MeshView {
    pub texture: usize,
    /// xyz per corner, in the corner's bone space.
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    /// uv per corner, 0..1.
    pub uvs: Vec<f32>,
    pub bones: Vec<u16>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SequenceView {
    pub label: String,
    pub fps: f32,
    pub frames: usize,
    pub blends: usize,
    pub activity: i32,
}

#[derive(Serialize)]
pub struct HitboxView {
    pub bone: i32,
    pub group: i32,
    pub min: [f32; 3],
    pub max: [f32; 3],
}

/// Whether every index [`model_view`] follows is in range: bone parents
/// come first, and each command's vertex and normal, and their bones, exist.
/// Models from outside the app (weapons) are checked before viewing.
pub fn check(m: &Model) -> Result<(), String> {
    let bones = m.bones.len();
    if let Some((i, b)) = m
        .bones
        .iter()
        .enumerate()
        .find(|(i, b)| b.parent >= *i as i32)
    {
        return Err(format!(
            "bone {i} ({}) has parent {}, not an earlier bone",
            b.name, b.parent
        ));
    }
    for (bi, bp) in m.bodyparts.iter().enumerate() {
        for sm in &bp.models {
            if let Some(b) = sm
                .vert_bones
                .iter()
                .chain(&sm.norm_bones)
                .find(|&&b| b as usize >= bones)
            {
                return Err(format!("body part {bi}: bone {b} of {bones}"));
            }
            if sm.vert_bones.len() < sm.verts.len() || sm.norm_bones.len() < sm.norms.len() {
                return Err(format!("body part {bi}: vertices without bones"));
            }
            for me in &sm.meshes {
                for c in &me.commands {
                    for &[vi, ni, _, _] in &c.verts {
                        if vi < 0
                            || vi as usize >= sm.verts.len()
                            || ni < 0
                            || ni as usize >= sm.norms.len()
                        {
                            return Err(format!(
                                "body part {bi}: vertex {vi} / normal {ni} out of range"
                            ));
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// Everything but the texture pixels and animation frames.
pub fn model_view(m: &Model, generation: u64) -> ModelView {
    let rest = preview::bone_worlds(m, None);
    // Front faces as in game: wind every triangle counter-clockwise.
    let flip = preview::winding_sign(m) < 0.0;
    let mut meshes: Vec<MeshView> = Vec::new();
    for bp in &m.bodyparts {
        let Some(sm) = bp.models.first() else {
            continue;
        };
        for me in &sm.meshes {
            let tex = m
                .skin_families
                .first()
                .and_then(|f| f.get(me.skinref as usize))
                .map_or(0, |&t| t as usize);
            let (w, h) = m.textures.get(tex).map_or((1.0, 1.0), |t| {
                (t.width.max(1) as f32, t.height.max(1) as f32)
            });
            let mv = match meshes.iter().position(|x| x.texture == tex) {
                Some(i) => &mut meshes[i],
                None => {
                    meshes.push(MeshView {
                        texture: tex,
                        positions: Vec::new(),
                        normals: Vec::new(),
                        uvs: Vec::new(),
                        bones: Vec::new(),
                    });
                    meshes.last_mut().expect("just pushed")
                }
            };
            for cmd in &me.commands {
                let v = &cmd.verts;
                for i in 0..v.len().saturating_sub(2) {
                    let mut idx = if cmd.fan {
                        [0, i + 1, i + 2]
                    } else if i % 2 == 0 {
                        [i, i + 1, i + 2]
                    } else {
                        [i + 1, i, i + 2]
                    };
                    if flip {
                        idx.swap(1, 2);
                    }
                    for k in idx {
                        let [vi, ni, s, t] = v[k];
                        let vb = sm.vert_bones[vi as usize] as usize;
                        let nb = sm.norm_bones[ni as usize] as usize;
                        // The normal, moved from its bone into the vertex's.
                        let n = rest[vb]
                            .r
                            .transpose()
                            .apply(rest[nb].r.apply(sm.norms[ni as usize]));
                        mv.positions.extend(sm.verts[vi as usize]);
                        mv.normals.extend(n);
                        mv.uvs.extend([s as f32 / w, t as f32 / h]);
                        mv.bones.push(vb as u16);
                    }
                }
            }
        }
    }
    ModelView {
        generation,
        name: m.name.clone(),
        bones: m
            .bones
            .iter()
            .map(|b| BoneView {
                name: b.name.clone(),
                parent: b.parent,
            })
            .collect(),
        textures: m
            .textures
            .iter()
            .map(|t| TextureView {
                name: t.name.clone(),
                width: t.width,
                height: t.height,
                masked: t.flags & preview::MASKED != 0,
            })
            .collect(),
        meshes,
        sequences: m
            .sequences
            .iter()
            .map(|s| SequenceView {
                label: s.label.clone(),
                fps: s.fps,
                frames: s.num_frames.max(1) as usize,
                blends: s.blends.len().max(1),
                activity: s.activity,
            })
            .collect(),
        hitboxes: m
            .hitboxes
            .iter()
            .map(|h| HitboxView {
                bone: h.bone,
                group: h.group,
                min: h.bbmin,
                max: h.bbmax,
            })
            .collect(),
        gait_split: m
            .bones
            .iter()
            .position(|b| b.name == "Bip01 Spine")
            .unwrap_or(m.bones.len()),
        triangles: m.tri_count(),
    }
}

/// Texture `i` as RGBA; palette index 255 is transparent on masked textures.
pub fn texture_rgba(m: &Model, i: usize) -> Option<Vec<u8>> {
    let t = m.textures.get(i)?;
    let masked = t.flags & preview::MASKED != 0;
    let mut out = Vec::with_capacity(t.pixels.len() * 4);
    for &p in &t.pixels {
        let c = &t.palette[p as usize * 3..p as usize * 3 + 3];
        out.extend([c[0], c[1], c[2], if masked && p == 255 { 0 } else { 255 }]);
    }
    Some(out)
}

/// Every frame's bone transforms relative to their parents for one blend of
/// a sequence (`None`: the rest pose, one frame): 12 floats per bone, the
/// rotation's rows then the translation.
pub fn locals(m: &Model, seq: Option<(usize, usize)>) -> Vec<f32> {
    let frames = seq
        .and_then(|(s, _)| m.sequences.get(s))
        .map_or(1, |s| s.num_frames.max(1) as usize);
    let mut out = Vec::with_capacity(frames * m.bones.len() * 12);
    for f in 0..frames {
        for x in preview::bone_locals(m, seq.map(|(s, b)| (s, b, f))) {
            push(&mut out, &x);
        }
    }
    out
}

fn push(out: &mut Vec<f32>, x: &Xf) {
    for row in x.r.0 {
        out.extend(row);
    }
    out.extend(x.t);
}

/// Sven's third-person weapon models (`p_*.mdl`) to hold in the viewer.
pub fn weapons() -> Vec<String> {
    let Some(root) = libremodele::donor::sven_root() else {
        return Vec::new();
    };
    let mut out: Vec<String> = std::fs::read_dir(root.join("svencoop/models"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.starts_with("p_") && n.ends_with(".mdl"))
        .collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unrolled corners keep every triangle, and rest locals chain to the
    /// preview's rest pose.
    #[test]
    fn check_refuses_bad_indices() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../donors/yuki_muumuyu.mdl");
        let mut m = libremodele::pipeline::read_mdl(&p).unwrap();
        assert_eq!(check(&m), Ok(()));
        let c = &mut m.bodyparts[0].models[0].meshes[0].commands[0];
        c.verts[0][0] = i16::MAX;
        assert!(check(&m).unwrap_err().contains("out of range"));
    }

    #[test]
    fn view_matches_preview() {
        let p =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../donors/yuki_muumuyu.mdl");
        let m = libremodele::pipeline::read_mdl(&p).unwrap();
        let v = model_view(&m, 0);
        let corners: usize = v.meshes.iter().map(|x| x.bones.len()).sum();
        assert_eq!(
            corners,
            3 * preview::triangles(&m, &preview::bone_worlds(&m, None)).len()
        );
        assert_eq!(locals(&m, None).len(), m.bones.len() * 12);
        let s = m.sequences.iter().position(|s| s.label == "idle").unwrap();
        let f = m.sequences[s].num_frames as usize;
        assert_eq!(locals(&m, Some((s, 0))).len(), f * m.bones.len() * 12);
        assert!(
            texture_rgba(&m, 0).unwrap().len()
                == (m.textures[0].width * m.textures[0].height * 4) as usize
        );
        assert!(v.gait_split > 0 && v.gait_split < m.bones.len());
    }
}
