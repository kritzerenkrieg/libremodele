//! Chain sway (D13, D15, D16): UniVRM 0.x spring bones, simulated offline for
//! every sequence and blend, then stored as rotation channels on the avatar's
//! chain bones.
//!
//! The step is UniVRM's `VRMSpringBoneLogic.Update`: Verlet tail with drag,
//! stiffness pulling toward the rest direction under the current parent,
//! gravity, a length constraint and sphere colliders. Forces are scaled from
//! metres to game units. Each sequence's animation data has to fit in 64 KB
//! (u16 offsets), so chain bones are added root-first until that budget is
//! used up. A bone that doesn't fit stays static, and so do its descendants.

use anyhow::Result;

use crate::math::{M3, V3, Xf, add, dot, len, mul, norm, sub};
use crate::mdl::{self, Model};

/// UniVRM's tail length for a chain end with no child (metres).
const LEAF_TAIL_M: f32 = 0.07;
/// Simulation rate (Hz), close to VRChat's frame rates.
const SIM_HZ: f32 = 60.0;
/// Looping sequences: cycles to settle before recording one.
const WARMUP_CYCLES: usize = 3;
/// Non-looping sequences: seconds held on frame 0 before recording.
const SETTLE_S: f32 = 1.0;
/// Per-sequence animation span limit (u16 offsets), minus a margin for alignment.
const SPAN_LIMIT: usize = 65535 - 64;

/// The three rotation channels of one bone in one blend.
type Rotations = [Option<Vec<i16>>; 3];

pub struct Group {
    pub name: String,
    pub stiffness: f32,
    pub gravity_power: f32,
    /// Unit vector in model space.
    pub gravity_dir: V3,
    pub drag: f32,
    /// Game units.
    pub hit_radius: f32,
    /// (bone, bone-local centre, radius), in game units.
    pub colliders: Vec<(usize, V3, f32)>,
}

pub struct Rig {
    pub units_per_metre: f32,
    /// Chain bones in parent-first order, with their group.
    pub chains: Vec<(usize, usize)>,
    pub groups: Vec<Group>,
    /// Bone-local tail for chain bones whose child links were merged away
    /// (D2 bone budget): the first merged child's joint, so the spring's
    /// lever matches the avatar's instead of a short virtual tail.
    pub tails: std::collections::HashMap<usize, V3>,
}

pub struct Summary {
    pub full: usize,
    pub partial: usize,
    pub none: usize,
}

/// Add sway channels to `m`'s chain bones in place.
pub fn apply(m: &mut Model, rig: &Rig) -> Result<Summary> {
    let chain_bones: Vec<usize> = rig.chains.iter().map(|c| c.0).collect();
    let usable = vec![true; chain_bones.len()];

    // Simulate: per sequence, per blend, per chain bone, per frame, the
    // rotation delta from rest.
    let mut deltas: Vec<Vec<Vec<Vec<V3>>>> = Vec::with_capacity(m.sequences.len());
    for si in 0..m.sequences.len() {
        let blends = m.sequences[si].blends.len();
        deltas.push((0..blends).map(|b| simulate(m, rig, si, b)).collect());
    }

    // One scale per chain bone and rotation channel, over every sequence.
    let mut scale = vec![[1e-7f32; 3]; chain_bones.len()];
    for seq in &deltas {
        for blend in seq {
            for (ci, frames) in blend.iter().enumerate() {
                for d in frames {
                    for k in 0..3 {
                        scale[ci][k] = scale[ci][k].max(d[k].abs() / 32000.0);
                    }
                }
            }
        }
    }
    for (ci, &b) in chain_bones.iter().enumerate() {
        m.bones[b].scale[3..6].copy_from_slice(&scale[ci]);
    }

    // Encode within each sequence's budget, root-first.
    let parent_of = |b: usize| usize::try_from(m.bones[b].parent).ok();
    let (mut full, mut partial, mut none) = (0, 0, 0);
    for (si, seq_deltas) in deltas.iter().enumerate() {
        let seq = &m.sequences[si];
        let mut span = seq.blends.len() * m.bones.len() * 12
            + seq
                .blends
                .iter()
                .flatten()
                .flat_map(|a| a.channels.iter().flatten())
                .map(|v| mdl::channel_bytes(v))
                .sum::<usize>();
        let mut added = vec![false; m.bones.len()];
        let mut blocked = vec![false; m.bones.len()];
        let mut planned: Vec<(usize, usize, Rotations)> = Vec::new();
        let mut count = 0;
        for (ci, &b) in chain_bones.iter().enumerate() {
            if !usable[ci] || parent_of(b).is_some_and(|p| blocked[p]) {
                blocked[b] = true;
                continue;
            }
            let mut cost = 0;
            let mut per_blend = Vec::new();
            for (bi, blend) in seq_deltas.iter().enumerate() {
                let raw: [Option<Vec<i16>>; 3] = std::array::from_fn(|k| {
                    let v: Vec<i16> = blend[ci]
                        .iter()
                        .map(|d| (d[k] / scale[ci][k]).round().clamp(-32767.0, 32767.0) as i16)
                        .collect();
                    v.iter().any(|&x| x != 0).then_some(v)
                });
                cost += raw
                    .iter()
                    .flatten()
                    .map(|v| mdl::channel_bytes(v))
                    .sum::<usize>();
                per_blend.push((bi, raw));
            }
            if span + cost > SPAN_LIMIT {
                blocked[b] = true;
                continue;
            }
            span += cost;
            added[b] = true;
            count += 1;
            for (bi, raw) in per_blend {
                planned.push((bi, b, raw));
            }
        }
        let seq = &mut m.sequences[si];
        for (bi, b, raw) in planned {
            let [x, y, z] = raw;
            seq.blends[bi][b].channels[3] = x;
            seq.blends[bi][b].channels[4] = y;
            seq.blends[bi][b].channels[5] = z;
        }
        match count {
            0 => none += 1,
            c if c == chain_bones.iter().zip(&usable).filter(|(_, u)| **u).count() => full += 1,
            _ => partial += 1,
        }
    }
    Ok(Summary {
        full,
        partial,
        none,
    })
}

/// Bone worlds at a fractional frame of blend `b`, lerping raw channel values
/// (chain bones are overwritten by the simulation).
fn pose(m: &Model, si: usize, b: usize, f: f32) -> Vec<Xf> {
    let seq = &m.sequences[si];
    let n = seq.num_frames.max(1) as usize;
    let f0 = (f.floor() as usize).min(n - 1);
    let f1 = (f0 + 1).min(n - 1);
    let t = (f - f0 as f32).clamp(0.0, 1.0);
    let mut out: Vec<Xf> = Vec::with_capacity(m.bones.len());
    for (i, bone) in m.bones.iter().enumerate() {
        let mut v = bone.value;
        for (c, ch) in seq.blends[b][i].channels.iter().enumerate() {
            if let Some(vals) = ch {
                let raw = vals[f0] as f32 * (1.0 - t) + vals[f1] as f32 * t;
                v[c] += raw * bone.scale[c];
            }
        }
        let local = Xf::from_gs(&v);
        out.push(match usize::try_from(bone.parent).ok() {
            Some(p) => out[p].then(&local),
            None => local,
        });
    }
    out
}

struct State {
    bone: usize,
    group: usize,
    parent: usize,
    rest_pos: V3,
    rest_rot: M3,
    rest_angles: V3,
    /// Tail direction in bone-local space (unit) and its length.
    axis: V3,
    length: f32,
    cur: V3,
    prev: V3,
}

/// Rotation deltas from rest, per chain bone, per frame.
fn simulate(m: &Model, rig: &Rig, si: usize, b: usize) -> Vec<Vec<V3>> {
    let seq = &m.sequences[si];
    let frames = seq.num_frames.max(1) as usize;
    // A broken rate (zero, NaN, or tiny enough to make billions of
    // substeps) falls back to 30 or is clamped to a sane range.
    let fps = if seq.fps > 0.0 && seq.fps.is_finite() {
        seq.fps.clamp(1.0, 1000.0)
    } else {
        30.0
    };
    let looping = seq.flags & 1 != 0 && frames > 1;
    let substeps = (SIM_HZ / fps).ceil().max(1.0) as usize;
    let dt = 1.0 / (fps * substeps as f32);
    let s = rig.units_per_metre;

    // Rest tails from the bone hierarchy: the first chain child, else a short
    // virtual tail continuing the parent → bone direction.
    let start = pose(m, si, b, 0.0);
    let mut states: Vec<State> = rig
        .chains
        .iter()
        .map(|&(bone, group)| {
            let bn = &m.bones[bone];
            let parent = bn.parent as usize;
            let rest_pos = [bn.value[0], bn.value[1], bn.value[2]];
            let rest_angles = [bn.value[3], bn.value[4], bn.value[5]];
            let child = m.bones.iter().position(|c| c.parent == bone as i32);
            let local_tail = match (child, rig.tails.get(&bone)) {
                (Some(c), _) => [
                    m.bones[c].value[0],
                    m.bones[c].value[1],
                    m.bones[c].value[2],
                ],
                (None, Some(&t)) => t,
                (None, None) => {
                    // Parent → bone direction, expressed in the bone's frame.
                    let rot = M3::from_gs_angles(rest_angles);
                    let d = norm(rest_pos);
                    let d = if len(d) < 0.5 { [1.0, 0.0, 0.0] } else { d };
                    mul(rot.transpose().apply(d), LEAF_TAIL_M * s)
                }
            };
            let world = start[parent].then(&Xf::new(M3::from_gs_angles(rest_angles), rest_pos));
            let tail = world.apply(local_tail);
            State {
                bone,
                group,
                parent,
                rest_pos,
                rest_rot: M3::from_gs_angles(rest_angles),
                rest_angles,
                axis: norm(local_tail),
                length: len(local_tail).max(1e-4),
                cur: tail,
                prev: tail,
            }
        })
        .collect();

    // Every bone below each chain bone: when the spring turns a bone, its
    // subtree (colliders on it, and the parents of chains further down)
    // turns with it within the same substep (#54).
    let under: Vec<Vec<usize>> = states.iter().map(|st| descendants(m, st.bone)).collect();

    let mut out = vec![Vec::with_capacity(frames); states.len()];
    let step = |f: f32, record: bool, states: &mut Vec<State>, out: &mut Vec<Vec<V3>>| {
        let mut w = pose(m, si, b, f);
        for (i, st) in states.iter_mut().enumerate() {
            let g = &rig.groups[st.group];
            let pw = w[st.parent];
            let pos = pw.apply(st.rest_pos);
            let rest_world = pw.r.mul(&st.rest_rot);
            let mut next = add(
                add(st.cur, mul(sub(st.cur, st.prev), 1.0 - g.drag)),
                add(
                    mul(rest_world.apply(st.axis), g.stiffness * dt * s),
                    mul(g.gravity_dir, g.gravity_power * dt * s),
                ),
            );
            next = add(pos, mul(norm(sub(next, pos)), st.length));
            for &(cb, off, r) in &g.colliders {
                let c = w[cb].apply(off);
                let rr = g.hit_radius + r;
                let d = sub(next, c);
                if dot(d, d) < rr * rr {
                    let pushed = add(c, mul(norm(d), rr));
                    next = add(pos, mul(norm(sub(pushed, pos)), st.length));
                }
            }
            st.prev = st.cur;
            st.cur = next;
            let rot =
                M3::align(norm(rest_world.apply(st.axis)), norm(sub(next, pos))).mul(&rest_world);
            carry(&mut w, st.bone, Xf::new(rot, pos), &under[i]);
            if record {
                let a = pw.r.transpose().mul(&rot).to_gs_angles();
                let prev = out[i].last().copied().unwrap_or([0.0; 3]);
                out[i].push(nearest(a, st.rest_angles, prev));
            }
        }
    };

    let n = substeps as f32;
    if looping {
        // The engine loops over frames 0 .. frames - 1, the last frame being
        // the first again.
        let cycle = frames - 1;
        for c in 0..=WARMUP_CYCLES {
            for k in 0..cycle {
                for j in 1..=substeps {
                    let rec = c == WARMUP_CYCLES && j == substeps;
                    step(k as f32 + j as f32 / n, rec, &mut states, &mut out);
                }
            }
        }
        // Recorded frames 1 ..= cycle; frame 0 is the same pose as the last.
        for o in &mut out {
            let last = *o.last().expect("at least one recorded frame");
            o.insert(0, last);
        }
    } else {
        for _ in 0..(SETTLE_S / dt) as usize {
            step(0.0, false, &mut states, &mut out);
        }
        step(0.0, true, &mut states, &mut out);
        for k in 1..frames {
            for j in 1..=substeps {
                step(
                    (k - 1) as f32 + j as f32 / n,
                    j == substeps,
                    &mut states,
                    &mut out,
                );
            }
        }
    }
    out
}

/// Bones below `bone` (bones are stored parent-first), nearest first.
fn descendants(m: &Model, bone: usize) -> Vec<usize> {
    let mut inside = vec![false; m.bones.len()];
    inside[bone] = true;
    let mut out = Vec::new();
    for (i, b) in m.bones.iter().enumerate().skip(bone + 1) {
        if b.parent >= 0 && inside.get(b.parent as usize) == Some(&true) {
            inside[i] = true;
            out.push(i);
        }
    }
    out
}

/// Set `bone`'s world transform and move the bones `under` it rigidly with
/// it, so their worlds stay consistent with the new pose.
fn carry(w: &mut [Xf], bone: usize, new: Xf, under: &[usize]) {
    let delta = new.then(&w[bone].inverse_rigid());
    for &d in under {
        w[d] = delta.then(&w[d]);
    }
    w[bone] = new;
}

/// The delta from `rest` of whichever Euler triple equal to `a` lies closest
/// to `prev`. Near gimbal lock x and z swing far for a small turn; the engine
/// slerps the quaternions it builds from them, so any equal triple plays the
/// same, and the closest one keeps channels smooth.
fn nearest(a: V3, rest: V3, prev: V3) -> V3 {
    let pi = std::f32::consts::PI;
    let alt = [a[0] + pi, pi - a[1], a[2] + pi];
    let near = |t: V3| -> V3 { std::array::from_fn(|k| prev[k] + wrap(t[k] - rest[k] - prev[k])) };
    let (x, y) = (near(a), near(alt));
    let d = |v: V3| (0..3).map(|k| (v[k] - prev[k]).abs()).sum::<f32>();
    if d(x) <= d(y) { x } else { y }
}

fn wrap(a: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let mut a = a % tau;
    if a > std::f32::consts::PI {
        a -= tau;
    } else if a < -std::f32::consts::PI {
        a += tau;
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turning_a_bone_carries_its_subtree() {
        // Bone 0 at the origin, a collider bone 1 one unit along x below it.
        let mut w = vec![
            Xf::new(M3::IDENTITY, [0.0; 3]),
            Xf::new(M3::IDENTITY, [1.0, 0.0, 0.0]),
        ];
        // Turn bone 0 a quarter turn about z: x goes to y.
        let quarter = M3::align([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        carry(&mut w, 0, Xf::new(quarter, [0.0; 3]), &[1]);
        let p = w[1].apply([0.0; 3]);
        assert!((p[0]).abs() < 1e-5 && (p[1] - 1.0).abs() < 1e-5, "{p:?}");
        let x = w[1].r.apply([1.0, 0.0, 0.0]);
        assert!((x[1] - 1.0).abs() < 1e-5, "{x:?}");
    }

    #[test]
    fn nearest_keeps_the_rotation_and_stays_close() {
        let rest = [0.3, 1.5, -2.0];
        let prev = [0.1, 0.05, -0.1];
        for a in [[0.4, 1.55, -2.1], [3.0, 1.4, 1.0], [-2.9, 0.2, 3.1]] {
            let d = nearest(a, rest, prev);
            let got = M3::from_gs_angles(std::array::from_fn(|k| rest[k] + d[k]));
            let want = M3::from_gs_angles(a);
            for i in 0..3 {
                for j in 0..3 {
                    assert!((got.0[i][j] - want.0[i][j]).abs() < 1e-4);
                }
            }
        }
        // Already near prev: unchanged up to wrapping.
        let d = nearest([0.42, 1.52, -2.05], rest, prev);
        assert!((0..3).all(|k| (d[k] - prev[k]).abs() < 0.2), "{d:?}");
    }
}
