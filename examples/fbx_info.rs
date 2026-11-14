//! Dev aid: humanoid joints and per-part bounds of an FBX avatar (phase G).
use libremodele::{fbx, unitypkg};
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let pkg = a
        .get(1)
        .map(|p| unitypkg::Package::read(&std::fs::read(p).unwrap()).unwrap());
    let path = std::path::Path::new(&a[0]);
    let av = fbx::load(
        &std::fs::read(path).unwrap(),
        &fbx::Load {
            file_name: path.file_name().unwrap().to_string_lossy().into(),
            title: "t".into(),
            licence: "l".into(),
            package: pkg.as_ref(),
            variant: a.get(2).map(String::as_str),
            dir: None,
        },
    )
    .unwrap();
    let mut h: Vec<_> = av.humanoid.iter().collect();
    h.sort();
    for (k, &i) in h {
        let t = av.nodes[i].world.t;
        println!(
            "{k:16} {:20} [{:7.3} {:7.3} {:7.3}]",
            av.nodes[i].name, t[0], t[1], t[2]
        );
    }
    for p in &av.parts {
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for v in &p.positions {
            for k in 0..3 {
                lo[k] = lo[k].min(v[k]);
                hi[k] = hi[k].max(v[k]);
            }
        }
        let mut doms: Vec<_> = p
            .dominant
            .iter()
            .map(|&d| av.nodes[d].name.as_str())
            .collect();
        doms.sort();
        doms.dedup();
        println!(
            "{:16} {:6} tris lo {:?} hi {:?} bones {:?}",
            p.id,
            p.triangles.len(),
            lo.map(|x| (x * 100.0).round() / 100.0),
            hi.map(|x| (x * 100.0).round() / 100.0),
            &doms[..doms.len().min(6)]
        );
    }
}
