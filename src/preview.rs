//! Our own checks without the game (A14): evaluate a pose the way the engine
//! does (bone value + raw channel × scale, GoldSrc angles), and draw the
//! textured model with a small z-buffered software rasteriser.

use image::{Rgb, RgbImage};

use crate::math::{M3, V3, Xf, cross, dot, norm, sub};
use crate::mdl::Model;

/// World transform of every bone at rest (`None`) or at (sequence, frame) of blend 0.
pub fn bone_worlds(m: &Model, pose: Option<(usize, usize)>) -> Vec<Xf> {
    let mut out: Vec<Xf> = Vec::with_capacity(m.bones.len());
    for (i, b) in m.bones.iter().enumerate() {
        let mut v = b.value;
        if let Some((s, f)) = pose
            && let Some(anim) = m
                .sequences
                .get(s)
                .and_then(|s| s.blends.first())
                .and_then(|bl| bl.get(i))
        {
            for (c, ch) in anim.channels.iter().enumerate() {
                if let Some(vals) = ch {
                    v[c] += vals[f.min(vals.len() - 1)] as f32 * b.scale[c];
                }
            }
        }
        let local = Xf::from_gs(&v);
        out.push(match usize::try_from(b.parent).ok() {
            Some(p) => out[p].then(&local),
            None => local,
        });
    }
    out
}

/// A drawable triangle: world positions, world normals and texture coordinates in pixels.
pub struct Tri {
    pub p: [V3; 3],
    pub n: [V3; 3],
    pub st: [[f32; 2]; 3],
    pub texture: usize,
}

/// Every triangle of the first submodel of each bodypart, posed by `worlds`.
/// Strips and fans are unrolled as OpenGL does, keeping their winding.
pub fn triangles(m: &Model, worlds: &[Xf]) -> Vec<Tri> {
    let mut out = Vec::new();
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
            for cmd in &me.commands {
                let v = &cmd.verts;
                for i in 0..v.len().saturating_sub(2) {
                    let idx = if cmd.fan {
                        [0, i + 1, i + 2]
                    } else if i % 2 == 0 {
                        [i, i + 1, i + 2]
                    } else {
                        [i + 1, i, i + 2]
                    };
                    let corner = |k: usize| {
                        let [vi, ni, s, t] = v[idx[k]];
                        let w = &worlds[sm.vert_bones[vi as usize] as usize];
                        let wn = &worlds[sm.norm_bones[ni as usize] as usize];
                        (
                            w.apply(sm.verts[vi as usize]),
                            norm(wn.r.apply(sm.norms[ni as usize])),
                            [s as f32, t as f32],
                        )
                    };
                    let (a, b, c) = (corner(0), corner(1), corner(2));
                    out.push(Tri {
                        p: [a.0, b.0, c.0],
                        n: [a.1, b.1, c.1],
                        st: [a.2, b.2, c.2],
                        texture: tex,
                    });
                }
            }
        }
    }
    out
}

/// +1 if the model's front faces satisfy outward normal = cross(b - a, c - a), else -1.
/// Measured against the stored vertex normals at rest.
pub fn winding_sign(m: &Model) -> f32 {
    let w = bone_worlds(m, None);
    let mut score = 0.0;
    for t in triangles(m, &w) {
        let g = cross(sub(t.p[1], t.p[0]), sub(t.p[2], t.p[0]));
        score += dot(
            g,
            norm([
                t.n[0][0] + t.n[1][0] + t.n[2][0],
                t.n[0][1] + t.n[1][1] + t.n[2][1],
                t.n[0][2] + t.n[1][2] + t.n[2][2],
            ]),
        )
        .signum();
    }
    if score >= 0.0 { 1.0 } else { -1.0 }
}

#[derive(Clone, Copy)]
pub enum View {
    Front,
    Side,
    Back,
}

impl View {
    /// Screen x (right), screen y (down), depth (toward the camera) for a GoldSrc point (X forward, Y left, Z up).
    fn project(self, p: V3) -> V3 {
        match self {
            View::Front => [p[1], -p[2], p[0]],
            View::Side => [-p[0], -p[2], p[1]],
            View::Back => [-p[1], -p[2], -p[0]],
        }
    }
    fn toward_camera(self) -> V3 {
        match self {
            View::Front => [1.0, 0.0, 0.0],
            View::Side => [0.0, 1.0, 0.0],
            View::Back => [-1.0, 0.0, 0.0],
        }
    }
}

/// Screen-space framing shared by several renders so they line up: (centre x, centre y, half height).
pub fn framing(tris: &[Tri], view: View) -> (f32, f32, f32) {
    let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
    for t in tris {
        for p in t.p {
            let q = view.project(p);
            for k in 0..2 {
                lo[k] = lo[k].min(q[k]);
                hi[k] = hi[k].max(q[k]);
            }
        }
    }
    (
        ((lo[0] + hi[0]) / 2.0),
        ((lo[1] + hi[1]) / 2.0),
        ((hi[1] - lo[1]).max(hi[0] - lo[0]) / 2.0 * 1.08).max(1e-3),
    )
}

/// Draw `tris` seen from `view`; back faces (by `winding`) are culled, as in game.
#[allow(clippy::too_many_arguments)]
pub fn render(
    m: &Model,
    tris: &[Tri],
    winding: f32,
    view: View,
    frame: (f32, f32, f32),
    w: u32,
    h: u32,
    bg: [u8; 3],
) -> RgbImage {
    let mut img = RgbImage::from_pixel(w, h, Rgb(bg));
    let mut z = vec![f32::MIN; (w * h) as usize];
    let (cx, cy, half) = frame;
    let k = (h as f32 / 2.0) / half;
    let cam = view.toward_camera();
    let light = norm([0.6, 0.5, 0.8]);
    let lit = M3::IDENTITY;
    for t in tris {
        let g = cross(sub(t.p[1], t.p[0]), sub(t.p[2], t.p[0]));
        let outward = [g[0] * winding, g[1] * winding, g[2] * winding];
        if dot(outward, cam) <= 0.0 {
            continue;
        }
        let Some(tex) = m.textures.get(t.texture) else {
            continue;
        };
        let shade = 0.7 + 0.3 * dot(norm(lit.apply(outward)), light).max(0.0);
        let s: Vec<V3> =
            t.p.iter()
                .map(|&p| {
                    let q = view.project(p);
                    [
                        (q[0] - cx) * k + w as f32 / 2.0,
                        (q[1] - cy) * k + h as f32 / 2.0,
                        q[2],
                    ]
                })
                .collect();
        let area =
            (s[1][0] - s[0][0]) * (s[2][1] - s[0][1]) - (s[2][0] - s[0][0]) * (s[1][1] - s[0][1]);
        if area.abs() < 1e-9 {
            continue;
        }
        let x0 = s
            .iter()
            .map(|p| p[0])
            .fold(f32::MAX, f32::min)
            .floor()
            .max(0.0) as i32;
        let x1 = s
            .iter()
            .map(|p| p[0])
            .fold(f32::MIN, f32::max)
            .ceil()
            .min(w as f32 - 1.0) as i32;
        let y0 = s
            .iter()
            .map(|p| p[1])
            .fold(f32::MAX, f32::min)
            .floor()
            .max(0.0) as i32;
        let y1 = s
            .iter()
            .map(|p| p[1])
            .fold(f32::MIN, f32::max)
            .ceil()
            .min(h as f32 - 1.0) as i32;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let w0 = ((s[1][0] - px) * (s[2][1] - py) - (s[2][0] - px) * (s[1][1] - py)) / area;
                let w1 = ((s[2][0] - px) * (s[0][1] - py) - (s[0][0] - px) * (s[2][1] - py)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let depth = w0 * s[0][2] + w1 * s[1][2] + w2 * s[2][2];
                let zi = (y as u32 * w + x as u32) as usize;
                if depth <= z[zi] {
                    continue;
                }
                z[zi] = depth;
                let ss = w0 * t.st[0][0] + w1 * t.st[1][0] + w2 * t.st[2][0];
                let tt = w0 * t.st[0][1] + w1 * t.st[1][1] + w2 * t.st[2][1];
                let tx = (ss.floor() as i32).rem_euclid(tex.width) as usize;
                let ty = (tt.floor() as i32).rem_euclid(tex.height) as usize;
                let pi = tex.pixels[ty * tex.width as usize + tx] as usize;
                let c = &tex.palette[pi * 3..pi * 3 + 3];
                img.put_pixel(
                    x as u32,
                    y as u32,
                    Rgb([c[0], c[1], c[2]].map(|v| (v as f32 * shade).min(255.0) as u8)),
                );
            }
        }
    }
    img
}

/// A contact sheet for review (A14, F1): rest pose from three sides, then
/// one frame of a few common sequences, all at the same scale.
pub fn sheet(m: &Model, tile: (u32, u32)) -> (RgbImage, Vec<String>) {
    let winding = winding_sign(m);
    let rest = triangles(m, &bone_worlds(m, None));
    let front = framing(&rest, View::Front);
    let side = framing(&rest, View::Side);
    type Shot = (String, Vec<Tri>, View, (f32, f32, f32));
    let mut shots: Vec<Shot> = vec![
        (
            "rest front".into(),
            triangles(m, &bone_worlds(m, None)),
            View::Front,
            front,
        ),
        (
            "rest side".into(),
            triangles(m, &bone_worlds(m, None)),
            View::Side,
            side,
        ),
        ("rest back".into(), rest, View::Back, front),
    ];
    for name in [
        "idle",
        "run",
        "crouch_idle",
        "ref_aim_mp5",
        "jump",
        "die_simple",
    ] {
        if let Some(si) = m.sequences.iter().position(|s| s.label == name) {
            let f = (m.sequences[si].num_frames as usize) / 2;
            let tris = triangles(m, &bone_worlds(m, Some((si, f))));
            let fr = framing(&tris, View::Front);
            // Keep the rest scale so poses compare, but centre each pose.
            shots.push((
                format!("{name} frame {f}"),
                tris,
                View::Front,
                (fr.0, fr.1, front.2.max(fr.2)),
            ));
        }
    }
    let cols = 3u32;
    let rows = (shots.len() as u32).div_ceil(cols);
    let mut out = RgbImage::from_pixel(tile.0 * cols, tile.1 * rows, Rgb([40, 40, 46]));
    let mut labels = Vec::new();
    for (i, (label, tris, view, frame)) in shots.iter().enumerate() {
        let img = render(
            m,
            tris,
            winding,
            *view,
            *frame,
            tile.0,
            tile.1,
            [58, 58, 66],
        );
        image::imageops::replace(
            &mut out,
            &img,
            ((i as u32 % cols) * tile.0) as i64,
            ((i as u32 / cols) * tile.1) as i64,
        );
        labels.push(format!(
            "tile {} (row {}, col {}): {label}",
            i + 1,
            i as u32 / cols + 1,
            i as u32 % cols + 1
        ));
    }
    (out, labels)
}
