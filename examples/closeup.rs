//! Dev aid: rest-pose close-ups (front and side) of the upper part of a model.
use image::{GenericImage, RgbImage};
use libremodele::{
    mdl,
    preview::{self, View},
};
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let m = mdl::read(&std::fs::read(&a[0]).unwrap()).unwrap();
    // Fraction of the height to frame, from the top (e.g. 0.35 = head + chest).
    let frac: f32 = a.get(2).map_or(0.35, |s| s.parse().unwrap());
    let wind = preview::winding_sign(&m);
    let t = preview::triangles(&m, &preview::bone_worlds(&m, None));
    let (w, h) = (500, 500);
    let mut out = RgbImage::new(w * 2, h);
    for (i, view) in [View::Front, View::Side].into_iter().enumerate() {
        let (cx, cy, half) = preview::framing(&t, view);
        // Screen y grows downward; the top of the model is cy - half.
        let z = half * frac;
        let img = preview::render(
            &m,
            &t,
            wind,
            view,
            (cx, cy - half + z, z),
            w,
            h,
            [40, 42, 48],
        );
        out.copy_from(&img, i as u32 * w, 0).unwrap();
    }
    out.save(&a[1]).unwrap();
}
