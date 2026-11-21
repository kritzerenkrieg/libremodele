//! Dev aid: how far a bone tips away from its rest "down" (degrees), over a
//! sequence, in the root's frame. Long rigid chains swing by this much.
use libremodele::{mdl, preview};
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let m = mdl::read(&std::fs::read(&a[0]).unwrap()).unwrap();
    let bi = m.bones.iter().position(|b| b.name == a[2]).unwrap();
    let rest = preview::bone_worlds(&m, None);
    for (si, s) in m.sequences.iter().enumerate() {
        if !s.label.contains(a[1].as_str()) {
            continue;
        }
        let mut worst: f32 = 0.0;
        for f in 0..s.num_frames as usize {
            let pose = preview::bone_worlds(&m, Some((si, f)));
            // The root's down, seen from the bone, at rest vs posed: rotations
            // relative to the root, so turning the whole body doesn't count.
            let down = [0.0, 0.0, -1.0];
            let rel = |w: &[libremodele::math::Xf]| w[0].r.transpose().mul(&w[bi].r);
            let r = rel(&rest).transpose().apply(down);
            let p = rel(&pose).transpose().apply(down);
            let d = (r[0] * p[0] + r[1] * p[1] + r[2] * p[2])
                .clamp(-1.0, 1.0)
                .acos()
                .to_degrees();
            worst = worst.max(d);
        }
        println!(
            "{:24} {} frames: {} tips up to {worst:.0}°",
            s.label, s.num_frames, a[2]
        );
    }
}
