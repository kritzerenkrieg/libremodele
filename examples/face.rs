//! Dev aid: face close-up at frame 0 of a sequence (the donor's animations
//! turn the model to face the front camera), framed on the head bone.
use libremodele::{
    mdl,
    preview::{self, View},
};
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let m = mdl::read(&std::fs::read(&a[0]).unwrap()).unwrap();
    let seq = a.get(2).map_or("idle", |s| s.as_str());
    let si = m.sequences.iter().position(|s| s.label == seq).unwrap();
    let w = preview::bone_worlds(&m, Some((si, 0)));
    let head = m.bones.iter().position(|b| b.name == "Bip01 Head").unwrap();
    let t = preview::triangles(&m, &w);
    let h = w[head].t;
    // Front view: screen x = y, screen y = -z.
    let half: f32 = a.get(3).map_or(6.0, |s| s.parse().unwrap());
    let img = preview::render(
        &m,
        &t,
        preview::winding_sign(&m),
        View::Front,
        (h[1], -h[2] - 2.0, half),
        600,
        600,
        [40, 42, 48],
    );
    img.save(&a[1]).unwrap();
}
