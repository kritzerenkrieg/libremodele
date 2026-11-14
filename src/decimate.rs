//! Lossy reduction to a triangle budget (C4, C8): quadric edge collapses
//! (Garland–Heckbert), cheapest by area-weighted plane distance first.
//! When both ends of an edge are plain interior vertices they merge at the
//! quadric's best point, with UV and normal interpolated along the edge, so
//! curved shells keep their bulge instead of sinking into what lies beneath
//! (hair over a scalp). Anything on a seam, border or locked bone keeps the
//! half-edge rule: it moves onto a neighbour and takes that corner's UVs.
//!
//! Locks relax in stages, each tried only when the budget isn't met yet (C3):
//! 0. `u` is interior to one bone, one texture and one UV chart, off borders.
//! 1. As 0, but bone boundaries may move (rigid binding, D3).
//! 2. Vertices on a UV seam or an open border may slide along it; seam and
//!    border corners (three or more such edges) stay.
//!
//!
//! Then, still at stage 2, the clearance caps below loosen step by step
//! (a quarter of the gap, half, three quarters, none) until the budget is met.
//!
//! Layers: surfaces that sit close over others (face skin over the eyes,
//! hair over the scalp) clip through them when moved by even a fraction of a
//! unit, which plane distance alone can't see. Each vertex's **clearance** is
//! its distance to the nearest triangle of a different connected piece; a
//! collapse may move it by at most a share of that, a quarter at first.
//!
//! Every stage keeps the mesh manifold (link condition), refuses folds, and
//! adds penalty planes along borders and seams so their outlines hold.

use std::cmp::Ordering;
use std::collections::{BTreeSet, BinaryHeap, HashMap};

use crate::math::{V3, cross, dot, len, norm, sub};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vtx {
    pub bone: usize,
    pub tex: usize,
    /// Model-space position.
    pub p: V3,
    pub n: V3,
    pub st: [i16; 2],
}

#[derive(Debug, Default)]
pub struct Stats {
    /// Highest lock stage used (see the module comment).
    pub stage: u8,
    pub reached: bool,
    /// Largest RMS distance (game units) from a moved vertex's new position
    /// to the original triangle planes it had absorbed.
    pub max_error: f32,
    pub collapses: usize,
    /// Vertices whose movement was capped by a nearby other layer.
    pub near_layers: usize,
    /// Share of the clearance a vertex could move in the last step used
    /// (infinite: caps lifted).
    pub clearance_share: f64,
}

/// (lock stage, clearance share) steps, tried in order until the budget is met.
const STEPS: [(u8, f64); 6] = [
    (0, 0.25),
    (1, 0.25),
    (2, 0.25),
    (2, 0.5),
    (2, 0.75),
    (2, f64::INFINITY),
];
/// Clearances are measured up to this far (game units); beyond, no cap.
const CLEARANCE_RANGE: f32 = 1.5;

/// Closest distance from `p` to triangle `t` (Ericson, Real-Time Collision
/// Detection, 5.1.5).
fn point_tri(p: V3, t: [V3; 3]) -> f32 {
    let [a, b, c] = t;
    let (ab, ac, ap) = (sub(b, a), sub(c, a), sub(p, a));
    let (d1, d2) = (dot(ab, ap), dot(ac, ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return len(ap);
    }
    let bp = sub(p, b);
    let (d3, d4) = (dot(ab, bp), dot(ac, bp));
    if d3 >= 0.0 && d4 <= d3 {
        return len(bp);
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return len(sub(p, [0, 1, 2].map(|k| a[k] + ab[k] * v)));
    }
    let cp = sub(p, c);
    let (d5, d6) = (dot(ab, cp), dot(ac, cp));
    if d6 >= 0.0 && d5 <= d6 {
        return len(cp);
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return len(sub(p, [0, 1, 2].map(|k| a[k] + ac[k] * w)));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        let bc = sub(c, b);
        return len(sub(p, [0, 1, 2].map(|k| b[k] + bc[k] * w)));
    }
    let n = cross(ab, ac);
    let nl = len(n);
    if nl < 1e-12 {
        return len(ap);
    }
    (dot(ap, n) / nl).abs()
}

/// The original surface, bucketed for "how far is the nearest other piece?"
/// queries. Pieces are connected by shared vertices (eyes, scalp and hair
/// are separate pieces).
struct Layers {
    piece: Vec<usize>,
    tris: Vec<([V3; 3], usize)>,
    grid: HashMap<[i32; 3], Vec<usize>>,
}

impl Layers {
    fn new(pos: &[V3], tris: &[Option<Tri>]) -> Layers {
        let mut root: Vec<usize> = (0..pos.len()).collect();
        fn find(r: &mut [usize], mut x: usize) -> usize {
            while r[x] != x {
                r[x] = r[r[x]];
                x = r[x];
            }
            x
        }
        for t in tris.iter().flatten() {
            let a = find(&mut root, t[0].0);
            for x in &t[1..] {
                let b = find(&mut root, x.0);
                root[b] = a;
            }
        }
        let piece: Vec<usize> = (0..pos.len()).map(|x| find(&mut root, x)).collect();
        let mut grid: HashMap<[i32; 3], Vec<usize>> = HashMap::new();
        let mut list = Vec::new();
        for t in tris.iter().flatten() {
            let ps = t.map(|x| pos[x.0]);
            let lo =
                Self::cell([0, 1, 2].map(|k| ps.iter().map(|p| p[k]).fold(f32::MAX, f32::min)));
            let hi =
                Self::cell([0, 1, 2].map(|k| ps.iter().map(|p| p[k]).fold(f32::MIN, f32::max)));
            for x in lo[0]..=hi[0] {
                for y in lo[1]..=hi[1] {
                    for z in lo[2]..=hi[2] {
                        grid.entry([x, y, z]).or_default().push(list.len());
                    }
                }
            }
            list.push((ps, piece[t[0].0]));
        }
        Layers {
            piece,
            tris: list,
            grid,
        }
    }

    fn cell(p: V3) -> [i32; 3] {
        p.map(|x| (x / CLEARANCE_RANGE).floor() as i32)
    }

    /// Distance from `p` to the nearest original triangle of a piece other
    /// than `own`, or infinity beyond `CLEARANCE_RANGE`.
    fn gap(&self, p: V3, own: usize) -> f32 {
        let c = Self::cell(p);
        let mut best = f32::INFINITY;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let Some(list) = self.grid.get(&[c[0] + dx, c[1] + dy, c[2] + dz]) else {
                        continue;
                    };
                    for &ti in list {
                        let (t, pc) = self.tris[ti];
                        if pc != own {
                            best = best.min(point_tri(p, t));
                        }
                    }
                }
            }
        }
        if best <= CLEARANCE_RANGE {
            best
        } else {
            f32::INFINITY
        }
    }
}

/// Symmetric 4×4 plane quadric: aa ab ac ad bb bc bd cc cd dd.
type Quad = [f64; 10];

fn plane(n: [f64; 3], d: f64, w: f64) -> Quad {
    let [a, b, c] = n;
    [
        a * a * w,
        a * b * w,
        a * c * w,
        a * d * w,
        b * b * w,
        b * c * w,
        b * d * w,
        c * c * w,
        c * d * w,
        d * d * w,
    ]
}

fn add_q(a: &mut Quad, b: &Quad) {
    for k in 0..10 {
        a[k] += b[k];
    }
}

fn eval(q: &Quad, p: V3) -> f64 {
    let [x, y, z] = p.map(f64::from);
    q[0] * x * x
        + 2.0 * q[1] * x * y
        + 2.0 * q[2] * x * z
        + 2.0 * q[3] * x
        + q[4] * y * y
        + 2.0 * q[5] * y * z
        + 2.0 * q[6] * y
        + q[7] * z * z
        + 2.0 * q[8] * z
        + q[9]
}

/// Corner attributes that must match for two corners to be one "wedge".
fn key(c: &Vtx, stage: u8) -> (usize, [i16; 2], usize) {
    (c.tex, c.st, if stage == 0 { c.bone } else { 0 })
}

type Tri = [(usize, Vtx); 3];

struct Mesh {
    pos: Vec<V3>,
    tris: Vec<Option<Tri>>,
    inc: Vec<Vec<usize>>,
    /// Each triangle slot's original unit normal, so small turns can't add up to a fold.
    orig: Vec<V3>,
}

impl Mesh {
    fn live(&self, u: usize) -> Vec<usize> {
        self.inc[u]
            .iter()
            .copied()
            .filter(|&t| self.tris[t].is_some_and(|c| c.iter().any(|x| x.0 == u)))
            .collect()
    }

    fn ring(&self, u: usize) -> BTreeSet<usize> {
        self.live(u)
            .iter()
            .flat_map(|&t| self.tris[t].unwrap().map(|x| x.0))
            .filter(|&x| x != u)
            .collect()
    }

    fn corner(&self, t: usize, u: usize) -> Vtx {
        self.tris[t].unwrap().iter().find(|x| x.0 == u).unwrap().1
    }

    /// Edge (u, w) is a border, or the wedges on either side differ.
    fn feature(&self, u: usize, w: usize, stage: u8) -> bool {
        let et: Vec<usize> = self
            .live(u)
            .into_iter()
            .filter(|&t| self.tris[t].unwrap().iter().any(|x| x.0 == w))
            .collect();
        match et.as_slice() {
            [a, b] => {
                key(&self.corner(*a, u), stage) != key(&self.corner(*b, u), stage)
                    || key(&self.corner(*a, w), stage) != key(&self.corner(*b, w), stage)
            }
            _ => true,
        }
    }

    fn face(&self, c: &[V3; 3]) -> V3 {
        cross(sub(c[1], c[0]), sub(c[2], c[0]))
    }
}

struct Plan {
    delete: Vec<usize>,
    rewrite: Vec<(usize, Tri)>,
    /// Full collapse: the merged vertex's new position (else v stays put).
    moved: Option<V3>,
}

/// `x` is a plain interior vertex at this stage: one fan, off borders and
/// seams, and (stage 0) on one bone.
fn free(m: &Mesh, x: usize, stage: u8) -> bool {
    let around = m.live(x);
    if around.is_empty() || !one_fan(m, x, &around) {
        return false;
    }
    if m.ring(x).iter().any(|&w| m.feature(x, w, stage)) {
        return false;
    }
    if stage == 0 {
        let bone = m.corner(around[0], x).bone;
        if around
            .iter()
            .any(|&t| m.tris[t].unwrap().iter().any(|c| c.1.bone != bone))
        {
            return false;
        }
    }
    true
}

/// The point minimising quadric `qs` near edge (a, b): the quadric's own
/// minimum when it is well defined and close, else the best of a, b and the
/// midpoint.
fn optimal(qs: &Quad, a: V3, b: V3) -> V3 {
    let mid = [0, 1, 2].map(|k| (a[k] + b[k]) / 2.0);
    let mut best = [a, b, mid]
        .into_iter()
        .min_by(|x, y| eval(qs, *x).total_cmp(&eval(qs, *y)))
        .unwrap();
    let mm = [
        [qs[0], qs[1], qs[2]],
        [qs[1], qs[4], qs[5]],
        [qs[2], qs[5], qs[7]],
    ];
    let r = [-qs[3], -qs[6], -qs[8]];
    let det3 = |c: [[f64; 3]; 3]| {
        c[0][0] * (c[1][1] * c[2][2] - c[1][2] * c[2][1])
            - c[0][1] * (c[1][0] * c[2][2] - c[1][2] * c[2][0])
            + c[0][2] * (c[1][0] * c[2][1] - c[1][1] * c[2][0])
    };
    let det = det3(mm);
    let tr = qs[0] + qs[4] + qs[7];
    if det.abs() > 1e-9 * tr.powi(3).max(1e-30) {
        let col = |k: usize| {
            let mut c = mm;
            for i in 0..3 {
                c[i][k] = r[i];
            }
            (det3(c) / det) as f32
        };
        let x = [col(0), col(1), col(2)];
        if len(sub(x, mid)) <= len(sub(b, a)) && eval(qs, x) < eval(qs, best) {
            best = x;
        }
    }
    best
}

fn sum_q(a: &Quad, b: &Quad) -> Quad {
    let mut s = *a;
    add_q(&mut s, b);
    s
}

fn plan(m: &Mesh, q: &[Quad], u: usize, v: usize, stage: u8) -> Option<Plan> {
    let around = m.live(u);
    let et: Vec<usize> = around
        .iter()
        .copied()
        .filter(|&t| m.tris[t].unwrap().iter().any(|x| x.0 == v))
        .collect();
    if et.is_empty() || et.len() > 2 {
        return None;
    }
    // One fan: u's triangles must connect through edges at u. Surfaces that
    // only touch at u (a pinch) would be dragged across the gap.
    if !one_fan(m, u, &around) {
        return None;
    }
    // Link condition: the only shared neighbours are the edge's opposite corners.
    let (ru, rv) = (m.ring(u), m.ring(v));
    if ru.intersection(&rv).count() != et.len() {
        return None;
    }
    // Locks by stage.
    let features = ru.iter().filter(|&&w| m.feature(u, w, stage)).count();
    match stage {
        0 | 1 => {
            if features > 0 {
                return None;
            }
            if stage == 0 {
                let bone = m.corner(around[0], u).bone;
                if around
                    .iter()
                    .any(|&t| m.tris[t].unwrap().iter().any(|x| x.1.bone != bone))
                {
                    return None;
                }
            }
        }
        _ => {
            if features > 2 || (features > 0 && !m.feature(u, v, stage)) {
                return None;
            }
        }
    }
    let fold = |t: usize, nc: &Tri| {
        let old = m.face(&m.tris[t].unwrap().map(|x| m.pos[x.0]));
        let new = m.face(&nc.map(|x| x.1.p));
        len(new) < 1e-9 || dot(norm(new), norm(old)) < 0.3 || dot(norm(new), m.orig[t]) < 0.2
    };
    let mut rewrite = Vec::new();

    // Both ends plain: merge them at the quadric's best point, with the
    // texture coordinate and normal interpolated along the edge.
    if features == 0 && free(m, v, stage) && (stage > 0 || free(m, u, stage)) {
        let (pu, pv) = (m.pos[u], m.pos[v]);
        let p = optimal(&sum_q(&q[u], &q[v]), pu, pv);
        let e = sub(pv, pu);
        let t = (dot(sub(p, pu), e) / dot(e, e).max(1e-12)).clamp(0.0, 1.0);
        let (cu, cv) = (m.corner(around[0], u), m.corner(et[0], v));
        let lerp = |a: f32, b: f32| a + (b - a) * t;
        let nc = Vtx {
            bone: if t < 0.5 { cu.bone } else { cv.bone },
            tex: cu.tex,
            p,
            n: norm([0, 1, 2].map(|k| lerp(cu.n[k], cv.n[k]))),
            st: [0, 1].map(|k| lerp(cu.st[k] as f32, cv.st[k] as f32).round() as i16),
        };
        for &t in around.iter().chain(m.live(v).iter()) {
            if et.contains(&t) || rewrite.iter().any(|(r, _)| *r == t) {
                continue;
            }
            let c = m.tris[t].unwrap();
            let c = c.map(|(id, x)| (id, Vtx { p: m.pos[id], ..x }));
            let new = c.map(|(id, x)| if id == u || id == v { (v, nc) } else { (id, x) });
            if fold(t, &new) {
                return None;
            }
            rewrite.push((t, new));
        }
        return Some(Plan {
            delete: et,
            rewrite,
            moved: Some(p),
        });
    }

    // Half-edge: each surviving triangle takes v's corner from a deleted
    // triangle whose u corner is the same wedge, so seams keep their UVs on
    // both sides.
    for &t in &around {
        if et.contains(&t) {
            continue;
        }
        let c = m.tris[t].unwrap();
        let w = key(&m.corner(t, u), stage);
        let vc = et
            .iter()
            .find(|&&e| key(&m.corner(e, u), stage) == w)
            .map(|&e| m.corner(e, v))?;
        let nc = c.map(|(id, x)| {
            if id == u {
                (v, Vtx { p: m.pos[v], ..vc })
            } else {
                (id, x)
            }
        });
        if fold(t, &nc) {
            return None;
        }
        rewrite.push((t, nc));
    }
    Some(Plan {
        delete: et,
        rewrite,
        moved: None,
    })
}

/// Whether the triangles around `u` form one edge-connected fan.
fn one_fan(m: &Mesh, u: usize, around: &[usize]) -> bool {
    let others = |t: usize| -> Vec<usize> {
        m.tris[t]
            .unwrap()
            .iter()
            .map(|x| x.0)
            .filter(|&x| x != u)
            .collect()
    };
    let mut seen = vec![false; around.len()];
    let mut stack = vec![0];
    seen[0] = true;
    while let Some(i) = stack.pop() {
        let oi = others(around[i]);
        for (j, &t) in around.iter().enumerate() {
            if !seen[j] && others(t).iter().any(|x| oi.contains(x)) {
                seen[j] = true;
                stack.push(j);
            }
        }
    }
    seen.into_iter().all(|x| x)
}

#[derive(PartialEq)]
struct Entry(f64, usize, usize, u32);
impl Eq for Entry {}
impl Ord for Entry {
    fn cmp(&self, o: &Self) -> Ordering {
        o.0.total_cmp(&self.0)
    }
}
impl PartialOrd for Entry {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

/// Collapse until at most `target` triangles remain, or no stage can go on.
pub fn decimate(input: &[[Vtx; 3]], target: usize) -> (Vec<[Vtx; 3]>, Stats) {
    let mut stats = Stats::default();
    if input.len() <= target {
        stats.reached = true;
        return (input.to_vec(), stats);
    }
    let mut ids: HashMap<[i32; 3], usize> = HashMap::new();
    let mut pos: Vec<V3> = Vec::new();
    let tris: Vec<Option<Tri>> = input
        .iter()
        .map(|t| {
            Some(t.map(|c| {
                let next = pos.len();
                let id = *ids
                    .entry(c.p.map(|x| (x * 1e4).round() as i32))
                    .or_insert(next);
                if id == next {
                    pos.push(c.p);
                }
                (id, c)
            }))
        })
        .collect();
    let mut inc = vec![Vec::new(); pos.len()];
    for (ti, t) in tris.iter().enumerate() {
        for x in t.unwrap() {
            if !inc[x.0].contains(&ti) {
                inc[x.0].push(ti);
            }
        }
    }
    let orig = tris
        .iter()
        .map(|t| {
            let p = t.unwrap().map(|x| pos[x.0]);
            norm(cross(sub(p[1], p[0]), sub(p[2], p[0])))
        })
        .collect();
    let mut m = Mesh {
        pos,
        tris,
        inc,
        orig,
    };

    let layers = Layers::new(&m.pos, &m.tris);
    let mut cap: Vec<f64> = (0..m.pos.len())
        .map(|x| f64::from(layers.gap(m.pos[x], layers.piece[x])))
        .collect();
    stats.near_layers = cap.iter().filter(|c| c.is_finite()).count();

    // Quadrics: area-weighted for ordering, unweighted for the error report.
    let n = m.pos.len();
    let mut q = vec![[0.0f64; 10]; n];
    let mut qe = vec![[0.0f64; 10]; n];
    let mut planes = vec![0u32; n];
    let mut scale = 0.0f64;
    for t in m.tris.iter().flatten() {
        let p = t.map(|x| m.pos[x.0]);
        let f = m.face(&p);
        let a = len(f) as f64 / 2.0;
        if a <= 0.0 {
            continue;
        }
        scale += a;
        let nn = norm(f).map(f64::from);
        let d = -(nn[0] * p[0][0] as f64 + nn[1] * p[0][1] as f64 + nn[2] * p[0][2] as f64);
        for x in t {
            add_q(&mut q[x.0], &plane(nn, d, a));
            add_q(&mut qe[x.0], &plane(nn, d, 1.0));
            planes[x.0] += 1;
        }
    }
    // Penalty planes along borders and seams (stage 2 key), through the edge
    // and perpendicular to its face.
    let mean_area = scale / m.tris.len() as f64;
    for ti in 0..m.tris.len() {
        let t = m.tris[ti].unwrap();
        let p = t.map(|x| m.pos[x.0]);
        let f = norm(m.face(&p));
        for k in 0..3 {
            let (a, b) = (t[k].0, t[(k + 1) % 3].0);
            if !m.feature(a, b, 2) {
                continue;
            }
            let e = sub(m.pos[b], m.pos[a]);
            let pn = norm(cross(e, f));
            if len(pn) < 0.5 {
                continue;
            }
            let nn = pn.map(f64::from);
            let d = -(nn[0] * m.pos[a][0] as f64
                + nn[1] * m.pos[a][1] as f64
                + nn[2] * m.pos[a][2] as f64);
            let w = 10.0 * mean_area.max(dot(e, e) as f64);
            add_q(&mut q[a], &plane(nn, d, w));
            add_q(&mut q[b], &plane(nn, d, w));
        }
    }

    let mut live = m.tris.len();
    let mut stamp = vec![0u32; n];
    for (stage, share) in STEPS {
        stats.stage = stage;
        stats.clearance_share = share;
        let mut heap = BinaryHeap::new();
        let push = |heap: &mut BinaryHeap<Entry>, m: &Mesh, q: &[Quad], stamp: &[u32], x: usize| {
            let fx = free(m, x, stage);
            for y in m.ring(x) {
                let cost = if fx && free(m, y, stage) {
                    let qs = sum_q(&q[x], &q[y]);
                    eval(&qs, optimal(&qs, m.pos[x], m.pos[y]))
                } else {
                    eval(&q[x], m.pos[y])
                };
                heap.push(Entry(cost, x, y, stamp[x]));
            }
        };
        for x in 0..n {
            push(&mut heap, &m, &q, &stamp, x);
        }
        while live > target {
            let Some(Entry(_, u, v, s)) = heap.pop() else {
                break;
            };
            if s != stamp[u] || m.live(u).is_empty() || m.live(v).is_empty() {
                continue;
            }
            let Some(p) = plan(&m, &q, u, v, stage) else {
                continue;
            };
            // Clearance: no vertex moves further from its original surface
            // than its share of the gap to the next layer. sqrt of the summed
            // squares bounds the largest plane distance.
            if share.is_finite() {
                let far = |x: usize, to: V3| eval(&qe[x], to).max(0.0).sqrt() > cap[x] * share;
                let mut blocked = match p.moved {
                    Some(to) => far(u, to) || far(v, to),
                    None => far(u, m.pos[v]),
                };
                // The new triangles' insides too: a chord between two corners
                // still on the surface dips inward, and through a close layer.
                let after = |id: usize| if id == u { v } else { id };
                for (_, nc) in &p.rewrite {
                    if blocked {
                        break;
                    }
                    let gap = nc
                        .iter()
                        .map(|x| cap[x.0].min(cap[after(x.0)]))
                        .fold(cap[u].min(cap[v]), f64::min);
                    if !gap.is_finite() {
                        continue;
                    }
                    let ps = nc.map(|x| x.1.p);
                    let mid = |a: V3, b: V3| [0, 1, 2].map(|k| (a[k] + b[k]) / 2.0);
                    let samples = [
                        [0, 1, 2].map(|k| (ps[0][k] + ps[1][k] + ps[2][k]) / 3.0),
                        mid(ps[0], ps[1]),
                        mid(ps[1], ps[2]),
                        mid(ps[2], ps[0]),
                    ];
                    let own = layers.piece[nc[0].0];
                    blocked = samples
                        .iter()
                        .any(|&sp| f64::from(layers.gap(sp, own)) < gap * (1.0 - share));
                }
                if blocked {
                    continue;
                }
            }
            let err = match p.moved {
                Some(to) => {
                    eval(&sum_q(&qe[u], &qe[v]), to).max(0.0)
                        / f64::from((planes[u] + planes[v]).max(1))
                }
                None => eval(&qe[u], m.pos[v]).max(0.0) / f64::from(planes[u].max(1)),
            }
            .sqrt() as f32;
            stats.max_error = stats.max_error.max(err);
            stats.collapses += 1;
            live -= p.delete.len();
            for t in p.delete {
                m.tris[t] = None;
            }
            for (t, nc) in p.rewrite {
                m.tris[t] = Some(nc);
                if !m.inc[v].contains(&t) {
                    m.inc[v].push(t);
                }
            }
            if let Some(to) = p.moved {
                m.pos[v] = to;
            }
            m.inc[u].clear();
            let (qu, qeu) = (q[u], qe[u]);
            add_q(&mut q[v], &qu);
            add_q(&mut qe[v], &qeu);
            planes[v] += planes[u];
            cap[v] = cap[v].min(cap[u]);
            stamp[v] += 1;
            push(&mut heap, &m, &q, &stamp, v);
            for x in m.ring(v) {
                m.inc[x].retain(|&t| m.tris[t].is_some());
                stamp[x] += 1;
                push(&mut heap, &m, &q, &stamp, x);
            }
        }
        if live <= target {
            stats.reached = true;
            break;
        }
    }
    let out = m
        .tris
        .into_iter()
        .flatten()
        .map(|t| t.map(|x| x.1))
        .collect();
    (out, stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// A closed UV sphere: rings × segments, one bone, one texture, with a
    /// UV seam at segment 0 (u = 0 and u = 1 are different texels).
    fn sphere(rings: usize, segs: usize) -> Vec<[Vtx; 3]> {
        let at = |r: usize, s: usize| {
            let th = std::f32::consts::PI * r as f32 / rings as f32;
            let ph = std::f32::consts::TAU * s as f32 / segs as f32;
            let p = [th.sin() * ph.cos(), th.sin() * ph.sin(), th.cos()];
            Vtx {
                bone: 0,
                tex: 0,
                p: p.map(|x| x * 10.0),
                n: p,
                st: [(s * 512 / segs) as i16, (r * 512 / rings) as i16],
            }
        };
        let mut t = Vec::new();
        for r in 0..rings {
            for s in 0..segs {
                let (a, b, c, d) = (at(r, s), at(r, s + 1), at(r + 1, s), at(r + 1, s + 1));
                if r > 0 {
                    t.push([a, c, b]);
                }
                if r + 1 < rings {
                    t.push([b, c, d]);
                }
            }
        }
        t
    }

    fn outward(t: &[Vtx; 3]) -> f32 {
        let f = cross(sub(t[1].p, t[0].p), sub(t[2].p, t[0].p));
        let c = [0, 1, 2].map(|k| (t[0].p[k] + t[1].p[k] + t[2].p[k]) / 3.0);
        dot(f, c)
    }

    #[test]
    fn sphere_reaches_budget_without_folding() {
        let s = sphere(24, 32);
        assert!(s.iter().all(|t| outward(t) > 0.0));
        let (r, st) = decimate(&s, 300);
        assert!(st.reached, "{st:?}");
        assert!(r.len() <= 300 && r.len() > 200, "{}", r.len());
        assert!(
            r.iter().all(|t| outward(t) > 0.0),
            "a triangle folded inward"
        );
        // Corners stay close to the sphere (optimal placement keeps the bulge,
        // so nothing sinks far inside), and UVs stay within the texture.
        for v in r.iter().flatten() {
            assert!((len(v.p) - 10.0).abs() < 0.5, "{:?} at {}", v.p, len(v.p));
            assert!((0..=512).contains(&v.st[0]) && (0..=512).contains(&v.st[1]));
        }
        // Mean radius stays near 10: half-edge chords alone would sink it.
        let mean = r.iter().flatten().map(|v| len(v.p)).sum::<f32>() / (3 * r.len()) as f32;
        assert!(mean > 9.9, "mean radius {mean}");
        assert!(st.max_error < 1.0, "{st:?}");
    }

    #[test]
    fn surfaces_touching_at_a_point_are_not_dragged_together() {
        // Two flat fans that share only their centre vertex, in different
        // planes: no triangle may end up spanning both.
        let v = |p: V3, b: usize| Vtx {
            bone: b,
            tex: 0,
            p,
            n: [0.0, 0.0, 1.0],
            st: [(p[0] * 8.0) as i16, (p[1] * 8.0 + p[2] * 8.0) as i16],
        };
        let mut t = Vec::new();
        let ring = |k: usize, z: bool| {
            let a = std::f32::consts::TAU * k as f32 / 8.0;
            if z {
                [a.cos() * 5.0, 0.0, 5.0 + a.sin() * 5.0]
            } else {
                [a.cos() * 5.0, a.sin() * 5.0, 0.0]
            }
        };
        for k in 0..8 {
            t.push([
                v([0.0; 3], 0),
                v(ring(k, false), 0),
                v(ring(k + 1, false), 0),
            ]);
        }
        let c2 = [0.0, 0.0, 0.0];
        for k in 0..8 {
            let (a, b) = (ring(k, true), ring(k + 1, true));
            t.push([
                v(c2, 1),
                v([a[0], a[1], a[2] - 5.0], 1),
                v([b[0], b[1], b[2] - 5.0], 1),
            ]);
        }
        let (r, _) = decimate(&t, 1);
        for tri in &r {
            let flat = tri
                .iter()
                .filter(|c| c.p[2].abs() < 1e-4 && c.p[1].abs() > 1e-4)
                .count();
            let up = tri
                .iter()
                .filter(|c| c.p[1].abs() < 1e-4 && c.p[2].abs() > 1e-4)
                .count();
            assert!(
                flat == 0 || up == 0,
                "a triangle spans both surfaces: {tri:?}"
            );
        }
    }

    #[test]
    fn outer_shell_does_not_sink_through_inner_one() {
        // Hair over a scalp: a closed sphere of radius 10 over one of 9.7
        // (separate pieces). Decimating hard must not push the outer shell's
        // surface through the inner one. (A sphere is kind to quadrics, so
        // this guards against regressions; Miku's head is the real case.)
        let outer = sphere(24, 32);
        let inner: Vec<[Vtx; 3]> = sphere(12, 16)
            .into_iter()
            .map(|t| {
                t.map(|v| Vtx {
                    p: v.p.map(|x| x * 0.97),
                    tex: 1,
                    ..v
                })
            })
            .collect();
        let all: Vec<[Vtx; 3]> = outer.iter().chain(&inner).copied().collect();
        let (r, st) = decimate(&all, inner.len() + 900);
        assert!(st.reached && st.clearance_share.is_finite(), "{st:?}");
        // Every outer triangle's centroid stays outside the inner sphere.
        for t in r.iter().filter(|t| t[0].tex == 0) {
            let c = [0, 1, 2].map(|k| (t[0].p[k] + t[1].p[k] + t[2].p[k]) / 3.0);
            assert!(len(c) > 9.70, "outer surface sank to {}", len(c));
        }
    }

    #[test]
    fn point_triangle_distance() {
        let t = [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 2.0, 0.0]];
        assert!((point_tri([0.5, 0.5, 3.0], t) - 3.0).abs() < 1e-6);
        assert!((point_tri([-1.0, 0.0, 0.0], t) - 1.0).abs() < 1e-6);
        assert!((point_tri([2.0, 2.0, 0.0], t) - 2f32.sqrt()).abs() < 1e-5);
    }

    #[test]
    fn rim_vertices_stay_on_the_rim() {
        // Open hemisphere on two bones, decimated as far as it goes: rim
        // vertices may slide along the rim (stage 2) but never leave it.
        let mut s: Vec<[Vtx; 3]> = sphere(16, 24)
            .into_iter()
            .filter(|t| t.iter().all(|v| v.p[2] >= -1e-3))
            .collect();
        for t in &mut s {
            let b = usize::from(t.iter().map(|v| v.p[0]).sum::<f32>() > 0.0);
            for v in t.iter_mut() {
                v.bone = b;
            }
        }
        let rim: HashSet<[i32; 3]> = s
            .iter()
            .flatten()
            .filter(|v| v.p[2].abs() < 1e-3)
            .map(|v| v.p.map(|x| (x * 1e3) as i32))
            .collect();
        let (r, st) = decimate(&s, 1);
        assert!(!st.reached && r.len() < s.len());
        let left: HashSet<[i32; 3]> = r
            .iter()
            .flatten()
            .filter(|v| v.p[2].abs() < 1e-3)
            .map(|v| v.p.map(|x| (x * 1e3) as i32))
            .collect();
        assert!(left.is_subset(&rim));
        // Triangles lying in the rim plane read as ~0 here, so allow rounding.
        for t in &r {
            assert!(
                outward(t) > -1e-2,
                "folded: {:?} outward {} stats {st:?} tris {}",
                t.map(|v| v.p),
                outward(t),
                r.len()
            );
        }
    }
}
