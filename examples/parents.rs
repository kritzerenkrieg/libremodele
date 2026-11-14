//! Dev aid: each bone's parent name, and its rotation change from rest relative to the root
//! at one sequence frame (degrees), to see what a chain hangs from.
use libremodele::{mdl, preview};
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let m = mdl::read(&std::fs::read(&a[0]).unwrap()).unwrap();
    let si = m.sequences.iter().position(|s| s.label == a[1]).unwrap();
    let f: usize = a[2].parse().unwrap();
    let rest = preview::bone_worlds(&m, None);
    let pose = preview::bone_worlds(&m, Some((si, f)));
    for (i, b) in m.bones.iter().enumerate() {
        // Turn relative to the root, so the whole-body facing doesn't count.
        let rl = rest[0].r.transpose().mul(&rest[i].r);
        let pl = pose[0].r.transpose().mul(&pose[i].r);
        let d = rl.transpose().mul(&pl);
        let ang = ((d.0[0][0] + d.0[1][1] + d.0[2][2] - 1.0) / 2.0)
            .clamp(-1.0, 1.0)
            .acos()
            .to_degrees();
        let p = usize::try_from(b.parent)
            .ok()
            .map_or("-", |p| m.bones[p].name.as_str());
        if a.get(3)
            .is_none_or(|filt| b.name.contains(filt.as_str()) || p.contains(filt.as_str()))
        {
            println!("{i:3} {:24} parent {:24} turn {ang:6.1}", b.name, p);
        }
    }
}
