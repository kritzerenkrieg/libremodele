//! Dev aid: what a VRM holds (parts, images, spring chains) before converting.
use std::collections::{HashMap, HashSet};

fn main() {
    let path = std::env::args().nth(1).expect("vrm path");
    let av = libremodele::vrm::load(&std::fs::read(path).unwrap()).unwrap();
    println!(
        "{:?} licence {:?}; {} nodes, {} humanoid, {} images",
        av.title,
        av.licence,
        av.nodes.len(),
        av.humanoid.len(),
        av.images.len()
    );
    let mut total = 0;
    for p in &av.parts {
        total += p.triangles.len();
        println!(
            "  part {:40} tris {:6} image {:?} unlit {} fp_only {}",
            p.id,
            p.triangles.len(),
            p.image,
            p.unlit,
            p.first_person_only
        );
    }
    println!("  total tris {total}");
    // Nodes carrying vertices (dominant) and their counts.
    let mut carry: HashMap<usize, usize> = HashMap::new();
    for p in &av.parts {
        for &d in &p.dominant {
            *carry.entry(d).or_default() += 1;
        }
    }
    let humanoid: HashSet<usize> = av.humanoid.values().copied().collect();
    println!(
        "nodes carrying vertices: {} ({} humanoid, {} spring, {} other)",
        carry.len(),
        carry.keys().filter(|n| humanoid.contains(n)).count(),
        carry.keys().filter(|n| av.spring.contains(n)).count(),
        carry
            .keys()
            .filter(|n| !humanoid.contains(n) && !av.spring.contains(n))
            .count()
    );
    for (n, c) in &carry {
        if !humanoid.contains(n) && !av.spring.contains(n) {
            println!("  other carrier {:30} verts {c}", av.nodes[*n].name);
        }
    }
    for g in &av.spring_groups {
        let roots: Vec<&str> = g.roots.iter().map(|&r| av.nodes[r].name.as_str()).collect();
        println!(
            "group {:12} stiff {} grav {} drag {} colliders {} roots {}: {:?}",
            g.name,
            g.stiffness,
            g.gravity_power,
            g.drag,
            g.colliders.len(),
            roots.len(),
            roots
        );
    }
    println!(
        "spring nodes: {}, carrying vertices: {}",
        av.spring.len(),
        carry.keys().filter(|n| av.spring.contains(n)).count()
    );
}
