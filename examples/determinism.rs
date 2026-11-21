//! Dev aid: convert twice and report which parts of the model differ.
use libremodele::{convert, mdl, vrm};
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let av = vrm::load(&std::fs::read(&a[0]).unwrap()).unwrap();
    let donor = mdl::read(&std::fs::read(&a[1]).unwrap()).unwrap();
    let o = convert::Options {
        name: "x".into(),
        height: None,
        keep: Default::default(),
        max_tris: None,
        keep_polygons: false,
    };
    let x = convert::convert(&av, &donor, &o).unwrap().model;
    let y = convert::convert(&av, &donor, &o).unwrap().model;
    println!("bones {}", x.bones == y.bones);
    println!("textures {}", x.textures == y.textures);
    println!("bodyparts {}", x.bodyparts == y.bodyparts);
    println!("hitboxes {}", x.hitboxes == y.hitboxes);
    println!("sequences {}", x.sequences == y.sequences);
    println!("tris {} {}", x.tri_count(), y.tri_count());
}
