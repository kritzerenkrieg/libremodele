use anyhow::{Result, bail};

use super::anim;
use super::bytes::Out;
use super::*;

/// Serialise `m` as an MDL v10 file. Sections follow studiomdl's order;
/// texture pixels come last, because the engine releases everything from
/// `texturedataindex` on once the textures are uploaded.
pub fn write(m: &Model) -> Result<Vec<u8>> {
    let mut o = Out::default();
    o.bytes(b"IDST");
    o.i32(10);
    o.name(&m.name, 64)?;
    let length_at = o.pos();
    o.i32(0);
    for v in [m.eye_position, m.min, m.max, m.bbmin, m.bbmax] {
        o.vec3(v);
    }
    o.i32(m.flags);
    // 26 count/offset fields, filled in below.
    let fields_at = o.pos();
    o.zeros(26 * 4);
    let mut f = [0i32; 26];

    // Bones, controllers, attachments, hitboxes.
    f[0] = count(m.bones.len())?;
    f[1] = o.here()?;
    for b in &m.bones {
        o.name(&b.name, 32)?;
        o.i32(b.parent);
        o.i32(b.flags);
        o.ints(&b.controllers);
        o.floats(&b.value);
        o.floats(&b.scale);
    }
    f[2] = count(m.bone_controllers.len())?;
    f[3] = o.here()?;
    for b in &m.bone_controllers {
        o.ints(&[b.bone, b.kind]);
        o.floats(&[b.start, b.end]);
        o.ints(&[b.rest, b.index]);
    }
    f[18] = count(m.attachments.len())?;
    f[19] = o.here()?;
    for a in &m.attachments {
        o.name(&a.name, 32)?;
        o.ints(&[a.kind, a.bone]);
        o.vec3(a.org);
        a.vectors.iter().for_each(|&v| o.vec3(v));
    }
    f[4] = count(m.hitboxes.len())?;
    f[5] = o.here()?;
    for h in &m.hitboxes {
        o.ints(&[h.bone, h.group]);
        o.vec3(h.bbmin);
        o.vec3(h.bbmax);
    }

    // Sequence descriptions, then their events and pivots.
    f[6] = count(m.sequences.len())?;
    f[7] = o.here()?;
    let seq_at = o.pos();
    o.zeros(m.sequences.len() * 176);
    let mut extra = Vec::with_capacity(m.sequences.len());
    for s in &m.sequences {
        let ev = o.here()?;
        for e in &s.events {
            o.ints(&[e.frame, e.event, e.kind]);
            o.name(&e.options, 64)?;
        }
        let pv = o.here()?;
        for p in &s.pivots {
            o.vec3(p.org);
            o.ints(&[p.start, p.end]);
        }
        extra.push((ev, pv));
    }
    o.align4();

    f[8] = count(m.seq_groups.len())?;
    f[9] = o.here()?;
    for g in &m.seq_groups {
        o.name(&g.label, 32)?;
        o.name(&g.name, 64)?;
        o.ints(&[g.unused1, g.unused2]);
    }

    f[24] = m.num_transitions;
    f[25] = o.here()?;
    o.bytes(&m.transitions);
    o.align4();

    // Animation data: per sequence, every blend's bone structs, then the runs.
    let mut anim_at = Vec::with_capacity(m.sequences.len());
    for s in &m.sequences {
        if s.blends.iter().any(|b| b.len() != m.bones.len()) {
            bail!(
                "{}: animation bone count differs from the model's {} bones",
                s.label,
                m.bones.len()
            );
        }
        anim_at.push(o.here()?);
        let structs = o.pos();
        o.zeros(s.blends.len() * m.bones.len() * 12);
        for (i, a) in s.blends.iter().flatten().enumerate() {
            let at = structs + i * 12;
            for (ch, vals) in a.channels.iter().enumerate() {
                let Some(vals) = vals else { continue };
                if vals.len() != s.num_frames as usize {
                    bail!(
                        "{}: channel has {} frames, the sequence {}",
                        s.label,
                        vals.len(),
                        s.num_frames
                    );
                }
                let Ok(off) = u16::try_from(o.pos() - at) else {
                    bail!("{}: animation data too large (offset past 64 KB)", s.label);
                };
                o.patch_u16(at + ch * 2, off);
                o.bytes(&anim::encode(vals));
            }
        }
        o.align4();
    }

    // Fill in the sequence descriptions now that offsets are known.
    let mut body = Out::default();
    for ((s, (ev, pv)), anim) in m.sequences.iter().zip(&extra).zip(&anim_at) {
        body.name(&s.label, 32)?;
        body.f32(s.fps);
        body.ints(&[
            s.flags,
            s.activity,
            s.act_weight,
            count(s.events.len())?,
            *ev,
            s.num_frames,
            count(s.pivots.len())?,
            *pv,
            s.motion_type,
            s.motion_bone,
        ]);
        body.vec3(s.linear_movement);
        body.ints(&[s.automove_pos_index, s.automove_angle_index]);
        body.vec3(s.bbmin);
        body.vec3(s.bbmax);
        body.ints(&[count(s.blends.len())?, *anim]);
        body.ints(&s.blend_type);
        body.floats(&s.blend_start);
        body.floats(&s.blend_end);
        body.ints(&[
            s.blend_parent,
            s.seq_group,
            s.entry_node,
            s.exit_node,
            s.node_flags,
            s.next_seq,
        ]);
    }
    debug_assert_eq!(body.buf.len(), m.sequences.len() * 176);
    o.buf[seq_at..seq_at + body.buf.len()].copy_from_slice(&body.buf);

    // Bodyparts, their models, then each model's arrays and meshes.
    f[16] = count(m.bodyparts.len())?;
    f[17] = o.here()?;
    let bp_at = o.pos();
    o.zeros(m.bodyparts.len() * 76);
    let models: Vec<&SubModel> = m.bodyparts.iter().flat_map(|b| &b.models).collect();
    let models_at = o.pos();
    o.zeros(models.len() * 112);
    let mut next = models_at;
    for (i, b) in m.bodyparts.iter().enumerate() {
        let at = bp_at + i * 76;
        let mut h = Out::default();
        h.name(&b.name, 64)?;
        h.ints(&[count(b.models.len())?, b.base, count(next)?]);
        o.buf[at..at + 76].copy_from_slice(&h.buf);
        next += b.models.len() * 112;
    }
    for (i, sm) in models.iter().enumerate() {
        if sm.verts.len() != sm.vert_bones.len() || sm.norms.len() != sm.norm_bones.len() {
            bail!(
                "{}: vertex or normal bone list does not match its array",
                sm.name
            );
        }
        let vertinfo = o.here()?;
        o.bytes(&sm.vert_bones);
        let norminfo = o.here()?;
        o.bytes(&sm.norm_bones);
        o.align4();
        let verts = o.here()?;
        sm.verts.iter().for_each(|&v| o.vec3(v));
        let norms = o.here()?;
        sm.norms.iter().for_each(|&v| o.vec3(v));
        let meshes = o.here()?;
        let mesh_at = o.pos();
        o.zeros(sm.meshes.len() * 20);
        for (k, me) in sm.meshes.iter().enumerate() {
            let tris = o.here()?;
            for cmd in &me.commands {
                let n = i16::try_from(cmd.verts.len()).ok().filter(|&n| n > 0);
                let Some(n) = n else {
                    bail!(
                        "{}: triangle command with {} vertices",
                        sm.name,
                        cmd.verts.len()
                    )
                };
                o.i16(if cmd.fan { -n } else { n });
                cmd.verts.iter().flatten().for_each(|&v| o.i16(v));
            }
            o.i16(0);
            o.align4();
            let at = mesh_at + k * 20;
            for (j, v) in [me.num_tris, tris, me.skinref, me.num_norms, me.norm_index]
                .into_iter()
                .enumerate()
            {
                o.patch_i32(at + j * 4, v);
            }
        }
        let mut h = Out::default();
        h.name(&sm.name, 64)?;
        h.i32(sm.kind);
        h.f32(sm.bounding_radius);
        h.ints(&[
            count(sm.meshes.len())?,
            meshes,
            count(sm.verts.len())?,
            vertinfo,
            verts,
            count(sm.norms.len())?,
            norminfo,
            norms,
            0,
            0,
        ]);
        let at = models_at + i * 112;
        o.buf[at..at + 112].copy_from_slice(&h.buf);
    }

    // Texture headers, the skin table, then the pixels.
    f[10] = count(m.textures.len())?;
    f[11] = o.here()?;
    let tex_at = o.pos();
    o.zeros(m.textures.len() * 80);
    f[13] = m.num_skinref;
    f[14] = count(m.skin_families.len())?;
    f[15] = o.here()?;
    for row in &m.skin_families {
        if row.len() != m.num_skinref as usize {
            bail!(
                "skin family has {} entries, not {}",
                row.len(),
                m.num_skinref
            );
        }
        row.iter().for_each(|&v| o.i16(v));
    }
    o.align4();
    f[12] = o.here()?;
    for (i, t) in m.textures.iter().enumerate() {
        if t.pixels.len() != (t.width as usize) * (t.height as usize) || t.palette.len() != 768 {
            bail!(
                "{}: pixel or palette size does not match {}x{}",
                t.name,
                t.width,
                t.height
            );
        }
        let data = o.here()?;
        o.bytes(&t.pixels);
        o.bytes(&t.palette);
        let mut h = Out::default();
        h.name(&t.name, 64)?;
        h.ints(&[t.flags, t.width, t.height, data]);
        let at = tex_at + i * 80;
        o.buf[at..at + 80].copy_from_slice(&h.buf);
    }

    for (i, v) in f.iter().enumerate() {
        o.patch_i32(fields_at + i * 4, *v);
    }
    let len = o.here()?;
    o.patch_i32(length_at, len);
    Ok(o.buf)
}

fn count(n: usize) -> Result<i32> {
    i32::try_from(n).map_err(|_| anyhow::anyhow!("count {n} too large"))
}
