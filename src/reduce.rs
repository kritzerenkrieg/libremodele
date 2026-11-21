//! Lossless reduction (C1, C2, C3): half-edge collapses that change nothing
//! visible. A vertex `v` may be merged into a neighbour `u` only when:
//! - its triangles form a closed fan (not on an open border, C3),
//! - every corner in the fan is on the same bone (bone boundaries locked, C3),
//! - each fan vertex has one texture coordinate and one normal in the fan
//!   (UV seams and normal creases locked, C3),
//! - the fan is planar, and the texture coordinates are one affine map over it,
//! - and the re-triangulated fan keeps its facing with no zero-area triangles.
//!
//! Under those conditions the surface, its texture mapping and its binding are
//! unchanged (C2).

use std::collections::HashMap;

use crate::math::{V3, cross, dot, len, norm, sub};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vtx {
    pub bone: usize,
    /// Bone-local position.
    pub p: V3,
    pub n: V3,
    pub st: [i16; 2],
}

type Key = (usize, [i32; 3]);

fn pkey(v: &Vtx) -> Key {
    (v.bone, v.p.map(|x| (x * 1e4).round() as i32))
}

fn nkey(n: V3) -> [i32; 3] {
    n.map(|x| (x * 1e3).round() as i32)
}

/// Why a vertex was kept, for the report.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Block {
    Border,
    Bone,
    SeamOrCrease,
    NonManifold,
    NotPlanar,
    Degenerate,
    UvNotAffine,
    NoTarget,
}

/// Same triangles minus every vertex that can go without visible change.
pub fn reduce(input: &[[Vtx; 3]]) -> Vec<[Vtx; 3]> {
    reduce_with_stats(input).0
}

/// `reduce`, plus how many vertices each rule kept in the final pass.
pub fn reduce_with_stats(input: &[[Vtx; 3]]) -> (Vec<[Vtx; 3]>, Vec<(Block, usize)>) {
    // Vertex ids by (bone, position); triangles as corner ids plus attributes.
    let mut ids: HashMap<Key, usize> = HashMap::new();
    let mut pos: Vec<V3> = Vec::new();
    let mut tris: Vec<Option<[(usize, Vtx); 3]>> = Vec::with_capacity(input.len());
    for t in input {
        tris.push(Some(t.map(|v| {
            let next = pos.len();
            let id = *ids.entry(pkey(&v)).or_insert(next);
            if id == next {
                pos.push(v.p);
            }
            (id, v)
        })));
    }
    let mut incident: Vec<Vec<usize>> = vec![Vec::new(); pos.len()];
    for (ti, t) in tris.iter().enumerate() {
        for (id, _) in t.as_ref().unwrap() {
            incident[*id].push(ti);
        }
    }

    let mut blocked: HashMap<Block, usize> = HashMap::new();
    loop {
        let mut changed = false;
        blocked.clear();
        for v in 0..pos.len() {
            if incident[v].is_empty() {
                continue;
            }
            match try_collapse(v, &tris, &incident, &pos) {
                Ok((u, plan)) => {
                    apply(v, u, plan, &mut tris, &mut incident);
                    changed = true;
                }
                Err(b) => *blocked.entry(b).or_default() += 1,
            }
        }
        if !changed {
            break;
        }
    }
    let mut stats: Vec<(Block, usize)> = blocked.into_iter().collect();
    stats.sort();
    let out = tris
        .into_iter()
        .flatten()
        .map(|t| t.map(|(_, v)| v))
        .collect();
    (out, stats)
}

/// For collapsing `v` into `u`: triangles to delete, and triangles to rewrite with `u`'s corner.
struct Plan {
    delete: Vec<usize>,
    rewrite: Vec<(usize, [(usize, Vtx); 3])>,
}

fn try_collapse(
    v: usize,
    tris: &[Option<[(usize, Vtx); 3]>],
    incident: &[Vec<usize>],
    pos: &[V3],
) -> Result<(usize, Plan), Block> {
    let fan: Vec<usize> = incident[v]
        .iter()
        .copied()
        .filter(|&t| tris[t].is_some())
        .collect();
    if fan.len() < 3 {
        return Err(Block::Border);
    }
    let corners = |t: usize| tris[t].as_ref().unwrap();

    // One bone, and one attribute set per vertex across the fan.
    let bone = corners(fan[0])[0].1.bone;
    let mut attr: HashMap<usize, ([i16; 2], [i32; 3])> = HashMap::new();
    for &t in &fan {
        for (id, c) in corners(t) {
            if c.bone != bone {
                return Err(Block::Bone);
            }
            let a = (c.st, nkey(c.n));
            if *attr.entry(*id).or_insert(a) != a {
                return Err(Block::SeamOrCrease);
            }
        }
    }

    // Normals are interpolated across a face: removing v keeps the shading
    // only when v's normal equals its neighbours' (a planar fan can still
    // carry a tilted normal at its centre).
    if attr.values().any(|a| a.1 != attr[&v].1) {
        return Err(Block::SeamOrCrease);
    }

    // Closed fan: every ring edge (v, w) is shared by exactly two fan triangles,
    // once in each direction.
    let mut out_edges: HashMap<usize, usize> = HashMap::new();
    for &t in &fan {
        let c = corners(t);
        let k = c.iter().position(|(id, _)| *id == v).ok_or(Block::Border)?;
        let (a, b) = (c[(k + 1) % 3].0, c[(k + 2) % 3].0);
        if a == v || b == v || out_edges.insert(a, b).is_some() {
            return Err(Block::NonManifold);
        }
    }
    let ring: Vec<usize> = out_edges.keys().copied().collect();
    if ring.len() != fan.len() || out_edges.values().any(|b| !out_edges.contains_key(b)) {
        return Err(Block::Border);
    }

    // Planar fan.
    let face = |t: usize| {
        let c = corners(t);
        cross(sub(pos[c[1].0], pos[c[0].0]), sub(pos[c[2].0], pos[c[0].0]))
    };
    let n0 = norm(face(fan[0]));
    if len(n0) < 0.5 {
        return Err(Block::Degenerate);
    }
    for &t in &fan {
        let f = face(t);
        if len(f) < 1e-10 || dot(norm(f), n0) < 1.0 - 1e-6 {
            return Err(Block::NotPlanar);
        }
    }
    // Plane offset too, so a fan of parallel but offset triangles cannot pass.
    let d0 = dot(n0, pos[v]);
    if fan
        .iter()
        .flat_map(|&t| corners(t).iter())
        .any(|(id, _)| (dot(n0, pos[*id]) - d0).abs() > 1e-4)
    {
        return Err(Block::NotPlanar);
    }

    // One affine texture map over the fan: fit it on the first triangle,
    // then every corner must agree within a twentieth of a texel.
    let e1 = norm(sub(pos[corners(fan[0])[1].0], pos[corners(fan[0])[0].0]));
    let e2 = cross(n0, e1);
    let plane = |p: V3| [dot(p, e1), dot(p, e2)];
    let c0 = corners(fan[0]);
    let q: Vec<[f32; 2]> = c0.iter().map(|(id, _)| plane(pos[*id])).collect();
    let det = (q[1][0] - q[0][0]) * (q[2][1] - q[0][1]) - (q[2][0] - q[0][0]) * (q[1][1] - q[0][1]);
    if det.abs() < 1e-12 {
        return Err(Block::Degenerate);
    }
    let map = |k: usize| {
        let s: Vec<f32> = c0.iter().map(|(_, c)| c.st[k] as f32).collect();
        let a = ((s[1] - s[0]) * (q[2][1] - q[0][1]) - (s[2] - s[0]) * (q[1][1] - q[0][1])) / det;
        let b = ((s[2] - s[0]) * (q[1][0] - q[0][0]) - (s[1] - s[0]) * (q[2][0] - q[0][0])) / det;
        (a, b, s[0] - a * q[0][0] - b * q[0][1])
    };
    let (ms, mt) = (map(0), map(1));
    for (id, (st, _)) in &attr {
        let p = plane(pos[*id]);
        let ps = ms.0 * p[0] + ms.1 * p[1] + ms.2;
        let pt = mt.0 * p[0] + mt.1 * p[1] + mt.2;
        if (ps - st[0] as f32).abs() > 0.05 || (pt - st[1] as f32).abs() > 0.05 {
            return Err(Block::UvNotAffine);
        }
    }

    // Try each ring vertex as the target; the new fan must keep its facing,
    // have no slivers, and not duplicate a triangle that already exists.
    let mut ring_sorted = ring.clone();
    ring_sorted.sort();
    'target: for &u in &ring_sorted {
        let mut plan = Plan {
            delete: Vec::new(),
            rewrite: Vec::new(),
        };
        let u_corner = fan
            .iter()
            .flat_map(|&t| corners(t).iter())
            .find(|(id, _)| *id == u)
            .map(|c| c.1)
            .ok_or(Block::NoTarget)?;
        for &t in &fan {
            let c = *corners(t);
            if c.iter().any(|(id, _)| *id == u) {
                plan.delete.push(t);
                continue;
            }
            let nc = c.map(|(id, x)| if id == v { (u, u_corner) } else { (id, x) });
            let f = cross(
                sub(pos[nc[1].0], pos[nc[0].0]),
                sub(pos[nc[2].0], pos[nc[0].0]),
            );
            if len(f) < 1e-8 || dot(norm(f), n0) < 1.0 - 1e-6 {
                continue 'target;
            }
            let mut set = [nc[0].0, nc[1].0, nc[2].0];
            set.sort();
            for &ot in &incident[u] {
                if let Some(o) = &tris[ot] {
                    let mut os = [o[0].0, o[1].0, o[2].0];
                    os.sort();
                    if os == set {
                        continue 'target;
                    }
                }
            }
            plan.rewrite.push((t, nc));
        }
        if plan.delete.len() == 2 {
            return Ok((u, plan));
        }
    }
    Err(Block::NoTarget)
}

fn apply(
    v: usize,
    u: usize,
    plan: Plan,
    tris: &mut [Option<[(usize, Vtx); 3]>],
    incident: &mut [Vec<usize>],
) {
    // Only the deleted triangles' corners have stale incidence entries.
    let mut touched = Vec::new();
    for t in plan.delete {
        if let Some(c) = tris[t] {
            touched.extend(c.map(|(id, _)| id));
        }
        tris[t] = None;
    }
    for (t, nc) in plan.rewrite {
        tris[t] = Some(nc);
        incident[u].push(t);
    }
    incident[v].clear();
    touched.sort_unstable();
    touched.dedup();
    for id in touched {
        incident[id].retain(|&t| tris[t].is_some());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vtx(x: f32, y: f32, bone: usize) -> Vtx {
        // Texture coordinates are an affine map of position: 8 texels per unit.
        Vtx {
            bone,
            p: [x, y, 0.0],
            n: [0.0, 0.0, 1.0],
            st: [(x * 8.0) as i16, (y * 8.0) as i16],
        }
    }

    fn grid(n: usize, bone_of: impl Fn(usize, usize) -> usize) -> Vec<[Vtx; 3]> {
        let mut t = Vec::new();
        for y in 0..n {
            for x in 0..n {
                let b = bone_of(x, y);
                let (x0, y0, x1, y1) = (x as f32, y as f32, x as f32 + 1.0, y as f32 + 1.0);
                t.push([vtx(x0, y0, b), vtx(x1, y0, b), vtx(x0, y1, b)]);
                t.push([vtx(x1, y0, b), vtx(x1, y1, b), vtx(x0, y1, b)]);
            }
        }
        t
    }

    fn area(t: &[[Vtx; 3]]) -> f32 {
        t.iter()
            .map(|t| len(cross(sub(t[1].p, t[0].p), sub(t[2].p, t[0].p))) / 2.0)
            .sum()
    }

    #[test]
    fn flat_grid_keeps_only_its_border() {
        let g = grid(4, |_, _| 0);
        let r = reduce(&g);
        // 16 border vertices stay (open border, C3); a 16-gon needs 14 triangles.
        assert_eq!(r.len(), 14);
        assert!((area(&r) - 16.0).abs() < 1e-4);
        for t in &r {
            for v in t {
                assert_eq!(v.st, [(v.p[0] * 8.0) as i16, (v.p[1] * 8.0) as i16]);
            }
            let f = cross(sub(t[1].p, t[0].p), sub(t[2].p, t[0].p));
            assert!(f[2] > 0.0, "a triangle flipped");
        }
    }

    #[test]
    fn bone_boundary_is_kept() {
        // Left half on bone 0, right half on bone 1: the halves are reduced
        // separately, and nothing crosses the boundary.
        let g = grid(4, |x, _| usize::from(x >= 2));
        let r = reduce(&g);
        assert!(r.len() < g.len());
        assert!(r.iter().all(|t| t.iter().all(|v| v.bone == t[0].bone)));
        assert!((area(&r) - 16.0).abs() < 1e-4);
    }

    #[test]
    fn uv_seam_and_curvature_block_collapse() {
        // A bent strip (two planes) keeps its fold vertices.
        let mut g = grid(2, |_, _| 0);
        for t in &mut g {
            for v in t.iter_mut() {
                if v.p[0] > 1.5 {
                    v.p[2] = 0.5;
                }
            }
        }
        let bent = reduce(&g);
        assert!((area(&bent) - area(&g)).abs() < 1e-4);
        // A UV seam through the middle vertex: same position, different st.
        let mut s = grid(2, |_, _| 0);
        for (i, t) in s.iter_mut().enumerate() {
            for v in t.iter_mut() {
                if v.p == [1.0, 1.0, 0.0] && i % 2 == 0 {
                    v.st = [99, 99];
                }
            }
        }
        assert_eq!(reduce(&s).len(), s.len());
    }
}
