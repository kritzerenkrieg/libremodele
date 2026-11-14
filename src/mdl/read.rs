use anyhow::{Context, Result, bail};

use super::anim;
use super::bytes::{Cur, idx};
use super::*;

const HEADER: usize = 244;

/// Parse an MDL v10 file with its textures inline (no `T.mdl`, no `NN.mdl` sequence groups).
pub fn read(d: &[u8]) -> Result<Model> {
    let mut c = Cur::new(d, 0);
    if c.take(4)? != b"IDST" {
        bail!("not a studio model (no IDST)");
    }
    let version = c.i32()?;
    if version != 10 {
        bail!("MDL version {version}; only GoldSrc v10 is supported");
    }
    let name = c.name(64)?;
    let _length = c.i32()?;
    let [eye_position, min, max, bbmin, bbmax] =
        [c.vec3()?, c.vec3()?, c.vec3()?, c.vec3()?, c.vec3()?];
    let flags = c.i32()?;
    let [
        numbones,
        boneindex,
        numbonecontrollers,
        bonecontrollerindex,
        numhitboxes,
        hitboxindex,
        numseq,
        seqindex,
        numseqgroups,
        seqgroupindex,
        numtextures,
        textureindex,
        _texturedataindex,
        numskinref,
        numskinfamilies,
        skinindex,
        numbodyparts,
        bodypartindex,
        numattachments,
        attachmentindex,
        _soundtable,
        _soundindex,
        soundgroups,
        _soundgroupindex,
        numtransitions,
        transitionindex,
    ] = c.ints::<26>()?;
    debug_assert_eq!(c.pos, HEADER);
    if soundgroups != 0 {
        bail!("sound groups are not supported");
    }

    let num_bones = idx(numbones, "bone count")?;
    let mut bones = Vec::with_capacity(num_bones);
    let mut c = Cur::new(d, idx(boneindex, "bone index")?);
    for _ in 0..num_bones {
        bones.push(Bone {
            name: c.name(32)?,
            parent: c.i32()?,
            flags: c.i32()?,
            controllers: c.ints()?,
            value: c.floats()?,
            scale: c.floats()?,
        });
    }

    let mut bone_controllers = Vec::new();
    let mut c = Cur::new(d, idx(bonecontrollerindex, "bone controller index")?);
    for _ in 0..idx(numbonecontrollers, "bone controller count")? {
        bone_controllers.push(BoneController {
            bone: c.i32()?,
            kind: c.i32()?,
            start: c.f32()?,
            end: c.f32()?,
            rest: c.i32()?,
            index: c.i32()?,
        });
    }

    let mut hitboxes = Vec::new();
    let mut c = Cur::new(d, idx(hitboxindex, "hitbox index")?);
    for _ in 0..idx(numhitboxes, "hitbox count")? {
        hitboxes.push(Hitbox {
            bone: c.i32()?,
            group: c.i32()?,
            bbmin: c.vec3()?,
            bbmax: c.vec3()?,
        });
    }

    let mut seq_groups = Vec::new();
    let mut c = Cur::new(d, idx(seqgroupindex, "sequence group index")?);
    for _ in 0..idx(numseqgroups, "sequence group count")? {
        seq_groups.push(SeqGroup {
            label: c.name(32)?,
            name: c.name(64)?,
            unused1: c.i32()?,
            unused2: c.i32()?,
        });
    }

    let mut sequences = Vec::new();
    let mut c = Cur::new(d, idx(seqindex, "sequence index")?);
    for i in 0..idx(numseq, "sequence count")? {
        let s = sequence(d, &mut c, num_bones).with_context(|| format!("sequence {i}"))?;
        sequences.push(s);
    }

    let mut textures = Vec::new();
    let mut c = Cur::new(d, idx(textureindex, "texture index")?);
    for _ in 0..idx(numtextures, "texture count")? {
        let name = c.name(64)?;
        let [flags, width, height, index] = c.ints()?;
        let size = idx(width, "texture width")? * idx(height, "texture height")?;
        let mut p = Cur::new(d, idx(index, "texture data index")?);
        let pixels = p.take(size)?.to_vec();
        let palette = p.take(768)?.to_vec();
        textures.push(Texture {
            name,
            flags,
            width,
            height,
            pixels,
            palette,
        });
    }

    let mut skin_families = Vec::new();
    let mut c = Cur::new(d, idx(skinindex, "skin index")?);
    for _ in 0..idx(numskinfamilies, "skin family count")? {
        let mut row = Vec::new();
        for _ in 0..idx(numskinref, "skin reference count")? {
            row.push(c.i16()?);
        }
        skin_families.push(row);
    }

    let mut bodyparts = Vec::new();
    let mut c = Cur::new(d, idx(bodypartindex, "bodypart index")?);
    for i in 0..idx(numbodyparts, "bodypart count")? {
        let name = c.name(64)?;
        let [nummodels, base, modelindex] = c.ints()?;
        let mut models = Vec::new();
        let mut mc = Cur::new(d, idx(modelindex, "model index")?);
        for j in 0..idx(nummodels, "model count")? {
            models.push(submodel(d, &mut mc).with_context(|| format!("bodypart {i} model {j}"))?);
        }
        bodyparts.push(Bodypart { name, base, models });
    }

    let mut attachments = Vec::new();
    let mut c = Cur::new(d, idx(attachmentindex, "attachment index")?);
    for _ in 0..idx(numattachments, "attachment count")? {
        attachments.push(Attachment {
            name: c.name(32)?,
            kind: c.i32()?,
            bone: c.i32()?,
            org: c.vec3()?,
            vectors: [c.vec3()?, c.vec3()?, c.vec3()?],
        });
    }

    let n = idx(numtransitions, "transition count")?;
    let transitions = Cur::new(d, idx(transitionindex, "transition index")?)
        .take(n * n)?
        .to_vec();

    Ok(Model {
        name,
        eye_position,
        min,
        max,
        bbmin,
        bbmax,
        flags,
        bones,
        bone_controllers,
        hitboxes,
        sequences,
        seq_groups,
        textures,
        num_skinref: numskinref,
        skin_families,
        bodyparts,
        attachments,
        num_transitions: numtransitions,
        transitions,
    })
}

fn sequence(d: &[u8], c: &mut Cur, num_bones: usize) -> Result<Sequence> {
    let label = c.name(32)?;
    let fps = c.f32()?;
    let [
        flags,
        activity,
        act_weight,
        numevents,
        eventindex,
        num_frames,
        numpivots,
        pivotindex,
        motion_type,
        motion_bone,
    ] = c.ints()?;
    let linear_movement = c.vec3()?;
    let [automove_pos_index, automove_angle_index] = c.ints()?;
    let (bbmin, bbmax) = (c.vec3()?, c.vec3()?);
    let [numblends, animindex] = c.ints()?;
    let blend_type = c.ints()?;
    let blend_start = c.floats()?;
    let blend_end = c.floats()?;
    let [
        blend_parent,
        seq_group,
        entry_node,
        exit_node,
        node_flags,
        next_seq,
    ] = c.ints()?;
    if seq_group != 0 {
        bail!("{label}: animations in an external sequence group are not supported");
    }

    let mut events = Vec::new();
    let mut e = Cur::new(d, idx(eventindex, "event index")?);
    for _ in 0..idx(numevents, "event count")? {
        events.push(Event {
            frame: e.i32()?,
            event: e.i32()?,
            kind: e.i32()?,
            options: e.name(64)?,
        });
    }
    let mut pivots = Vec::new();
    let mut p = Cur::new(d, idx(pivotindex, "pivot index")?);
    for _ in 0..idx(numpivots, "pivot count")? {
        pivots.push(Pivot {
            org: p.vec3()?,
            start: p.i32()?,
            end: p.i32()?,
        });
    }

    let frames = idx(num_frames, "frame count")?;
    let base = idx(animindex, "animation index")?;
    let mut blends = Vec::new();
    for b in 0..idx(numblends, "blend count")? {
        let mut per_bone = Vec::with_capacity(num_bones);
        for bone in 0..num_bones {
            let at = base + (b * num_bones + bone) * 12;
            let mut a = Cur::new(d, at);
            let mut anim = BoneAnim::default();
            for ch in 0..6 {
                let off = a.u16()? as usize;
                if off == 0 {
                    continue;
                }
                let vals = anim::decode(d, at + off, frames)
                    .with_context(|| format!("{label}: blend {b} bone {bone} channel {ch}"))?;
                // An all-zero channel and an absent one play the same; keep one form.
                if vals.iter().any(|&v| v != 0) {
                    anim.channels[ch] = Some(vals);
                }
            }
            per_bone.push(anim);
        }
        blends.push(per_bone);
    }

    Ok(Sequence {
        label,
        fps,
        flags,
        activity,
        act_weight,
        events,
        num_frames,
        pivots,
        motion_type,
        motion_bone,
        linear_movement,
        automove_pos_index,
        automove_angle_index,
        bbmin,
        bbmax,
        blends,
        blend_type,
        blend_start,
        blend_end,
        blend_parent,
        seq_group,
        entry_node,
        exit_node,
        node_flags,
        next_seq,
    })
}

fn submodel(d: &[u8], c: &mut Cur) -> Result<SubModel> {
    let name = c.name(64)?;
    let kind = c.i32()?;
    let bounding_radius = c.f32()?;
    let [
        nummesh,
        meshindex,
        numverts,
        vertinfoindex,
        vertindex,
        numnorms,
        norminfoindex,
        normindex,
        numgroups,
        _groupindex,
    ] = c.ints()?;
    if numgroups != 0 {
        bail!("{name}: model groups are not supported");
    }
    let nv = idx(numverts, "vertex count")?;
    let nn = idx(numnorms, "normal count")?;
    let vert_bones = Cur::new(d, idx(vertinfoindex, "vertex info index")?)
        .take(nv)?
        .to_vec();
    let norm_bones = Cur::new(d, idx(norminfoindex, "normal info index")?)
        .take(nn)?
        .to_vec();
    let mut v = Cur::new(d, idx(vertindex, "vertex index")?);
    let verts = (0..nv).map(|_| v.vec3()).collect::<Result<_>>()?;
    let mut n = Cur::new(d, idx(normindex, "normal index")?);
    let norms = (0..nn).map(|_| n.vec3()).collect::<Result<_>>()?;

    let mut meshes = Vec::new();
    let mut m = Cur::new(d, idx(meshindex, "mesh index")?);
    for _ in 0..idx(nummesh, "mesh count")? {
        let [num_tris, triindex, skinref, num_norms, norm_index] = m.ints()?;
        let mut t = Cur::new(d, idx(triindex, "triangle index")?);
        let mut commands = Vec::new();
        loop {
            let count = t.i16()?;
            if count == 0 {
                break;
            }
            let verts = (0..count.unsigned_abs())
                .map(|_| Ok([t.i16()?, t.i16()?, t.i16()?, t.i16()?]))
                .collect::<Result<_>>()?;
            commands.push(TriCmd {
                fan: count < 0,
                verts,
            });
        }
        meshes.push(Mesh {
            num_tris,
            skinref,
            num_norms,
            norm_index,
            commands,
        });
    }
    Ok(SubModel {
        name,
        kind,
        bounding_radius,
        vert_bones,
        verts,
        norm_bones,
        norms,
        meshes,
    })
}
