//! Dev aid: per chain bone, the largest sway angle in one sequence.
use libremodele::{convert, mdl, sway, vrm};
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let av = vrm::load(&std::fs::read(&a[0]).unwrap()).unwrap();
    let donor = mdl::read(&std::fs::read(&a[1]).unwrap()).unwrap();
    let mut c = convert::convert(
        &av,
        &donor,
        &convert::Options {
            name: "x".into(),
            height: None,
            keep: Default::default(),
        },
    )
    .unwrap();
    if a.get(3).is_some_and(|x| x == "nocol") {
        for g in &mut c.rig.groups {
            g.colliders.clear();
        }
    }
    for g in &c.rig.groups {
        println!(
            "{} stiff {} grav {} {:?} drag {} hit {} colliders {:?}",
            g.name, g.stiffness, g.gravity_power, g.gravity_dir, g.drag, g.hit_radius, g.colliders
        );
    }
    let si = c
        .model
        .sequences
        .iter()
        .position(|s| s.label == a[2])
        .unwrap();
    let before = c.model.clone();
    sway::apply(&mut c.model, &c.rig).unwrap();
    for &(b, _) in &c.rig.chains {
        let bone = &c.model.bones[b];
        let mut mx = [0f32; 3];
        for ch in 3..6 {
            if let Some(v) = &c.model.sequences[si].blends[0][b].channels[ch] {
                mx[ch - 3] = v
                    .iter()
                    .map(|x| (*x as f32 * bone.scale[ch]).abs())
                    .fold(0.0, f32::max)
                    .to_degrees();
            }
        }
        let _ = &before;
        println!(
            "{:12} rest {:?} max delta deg {:?}",
            bone.name,
            [
                bone.value[3].to_degrees() as i32,
                bone.value[4].to_degrees() as i32,
                bone.value[5].to_degrees() as i32
            ],
            mx.map(|x| x as i32)
        );
    }
}
