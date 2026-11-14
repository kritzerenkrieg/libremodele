//! Dev aid: a row of frames of one sequence, side by side, for both views.
use image::{GenericImage, RgbImage};
use libremodele::{
    mdl,
    preview::{self, View},
};
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let m = mdl::read(&std::fs::read(&a[0]).unwrap()).unwrap();
    let si = m.sequences.iter().position(|s| s.label == a[1]).unwrap();
    let n = m.sequences[si].num_frames as usize;
    let count = 8.min(n);
    let wind = preview::winding_sign(&m);
    let rest = preview::triangles(&m, &preview::bone_worlds(&m, None));
    let (w, h) = (180, 240);
    let mut out = RgbImage::new(w * count as u32, h * 2);
    for (row, view) in [View::Front, View::Side].into_iter().enumerate() {
        let (cx, cy, half) = preview::framing(&rest, view);
        for i in 0..count {
            let f = i * (n - 1) / (count - 1).max(1);
            let t = preview::triangles(&m, &preview::bone_worlds(&m, Some((si, f))));
            let img = preview::render(
                &m,
                &t,
                wind,
                view,
                (cx, cy + half * 0.3, half * 1.4),
                w,
                h,
                [40, 42, 48],
            );
            out.copy_from(&img, i as u32 * w, row as u32 * h).unwrap();
        }
    }
    out.save(&a[2]).unwrap();
    println!("{} frames", n);
}
