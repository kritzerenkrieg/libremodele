//! Dev aid: surface area per texture (through the skin family) at rest, for comparing
//! two conversions of the same avatar.
use libremodele::math::{cross, len, sub};
use libremodele::{mdl, preview};
fn main() {
    for path in std::env::args().skip(1) {
        let m = mdl::read(&std::fs::read(&path).unwrap()).unwrap();
        let w = preview::bone_worlds(&m, None);
        let mut area = vec![0f32; m.textures.len()];
        let mut tris = vec![0usize; m.textures.len()];
        for bp in &m.bodyparts {
            let sm = &bp.models[0];
            let p = |i: i16| w[sm.vert_bones[i as usize] as usize].apply(sm.verts[i as usize]);
            for me in &sm.meshes {
                // skinref is a skin-family column; resolve it to a texture.
                let tex = m
                    .skin_families
                    .first()
                    .and_then(|f| f.get(me.skinref as usize))
                    .map_or(me.skinref as usize, |&t| t as usize);
                for c in &me.commands {
                    let v = &c.verts;
                    for i in 0..v.len().saturating_sub(2) {
                        let t = if c.fan {
                            [v[0], v[i + 1], v[i + 2]]
                        } else {
                            [v[i], v[i + 1], v[i + 2]]
                        };
                        let (a, b, cc) = (p(t[0][0]), p(t[1][0]), p(t[2][0]));
                        area[tex] += len(cross(sub(b, a), sub(cc, a))) / 2.0;
                        tris[tex] += 1;
                    }
                }
            }
        }
        println!("{path}");
        for (i, t) in m.textures.iter().enumerate() {
            println!(
                "  tex {i} flags {:#x}: {:6} tris, area {:8.1}",
                t.flags, tris[i], area[i]
            );
        }
    }
}
