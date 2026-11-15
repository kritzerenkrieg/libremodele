//! Dev aid: raw structure of a binary FBX (models, geometry layers,
//! materials, textures, axes) via our own parser (phase G).
use libremodele::fbx::{self, FNode, Prop};
fn s(p: &[Prop], i: usize) -> String {
    match p.get(i) {
        Some(Prop::Str(x)) => x.split('\0').next().unwrap_or("").to_string(),
        Some(x) => format!("{x:?}").chars().take(60).collect(),
        None => String::new(),
    }
}
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let top = fbx::parse(&std::fs::read(&path).unwrap()).unwrap();
    let objs = top.iter().find(|n| n.name == "Objects").unwrap();
    let mut kinds = std::collections::BTreeMap::new();
    for c in &objs.children {
        *kinds
            .entry(format!("{}:{}", c.name, s(&c.props, 2)))
            .or_insert(0) += 1;
    }
    println!("objects {kinds:?}");
    let models: Vec<String> = objs
        .children
        .iter()
        .filter(|c| c.name == "Model")
        .map(|m| format!("{}({})", s(&m.props, 1), s(&m.props, 2)))
        .collect();
    println!("{} models: {}", models.len(), models.join(", "));
    for g in objs
        .children
        .iter()
        .filter(|c| c.name == "Geometry" && s(&c.props, 2) == "Mesh")
    {
        let n = |k: &str| {
            g.child(k)
                .and_then(|v| match v.props.first() {
                    Some(Prop::Floats(f)) => Some(f.len()),
                    Some(Prop::Ints(f)) => Some(f.len()),
                    _ => None,
                })
                .unwrap_or(0)
        };
        let layers: Vec<String> = g
            .children
            .iter()
            .filter(|c| c.name.starts_with("LayerElement"))
            .map(|c| {
                let m = c
                    .child("MappingInformationType")
                    .map(|x| s(&x.props, 0))
                    .unwrap_or_default();
                format!("{}[{}]", c.name.trim_start_matches("LayerElement"), m)
            })
            .collect();
        let polys = g
            .child("PolygonVertexIndex")
            .and_then(|v| match v.props.first() {
                Some(Prop::Ints(f)) => Some(f.iter().filter(|&&i| i < 0).count()),
                _ => None,
            })
            .unwrap_or(0);
        println!(
            "geometry {}: {} verts, {polys} polys, {}",
            s(&g.props, 1),
            n("Vertices") / 3,
            layers.join(" ")
        );
        if let Some(c) = g.child("LayerElementColor") {
            let v: Vec<f64> = match c.child("Colors").and_then(|x| x.props.first()) {
                Some(Prop::Floats(f)) => f.clone(),
                _ => vec![],
            };
            let mut set: Vec<[i32; 4]> = v
                .chunks(4)
                .map(|q| [0, 1, 2, 3].map(|k| (q[k] * 255.0).round() as i32))
                .collect();
            set.sort();
            set.dedup();
            println!(
                "   vertex colours: {} values, {} distinct, first {:?}",
                v.len() / 4,
                set.len(),
                &set[..set.len().min(8)]
            );
        }
    }
    for m in objs.children.iter().filter(|c| c.name == "Material") {
        println!("material {}", s(&m.props, 1));
    }
    for t in objs.children.iter().filter(|c| c.name == "Texture") {
        let f = t
            .child("RelativeFilename")
            .or(t.child("FileName"))
            .map(|x| s(&x.props, 0))
            .unwrap_or_default();
        println!("texture {} -> {f}", s(&t.props, 1));
    }
    let _ = |_: &FNode| ();
}
