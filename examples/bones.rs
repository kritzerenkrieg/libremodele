// Dev aid: bone rest values and which channels any sequence animates.
fn main() {
    let path = std::env::args().nth(1).expect("mdl path");
    let m = libremodele::mdl::read(&std::fs::read(path).unwrap()).unwrap();
    for (i, b) in m.bones.iter().enumerate() {
        let mut animated = [0usize; 6];
        for s in &m.sequences {
            for blend in &s.blends {
                for (c, ch) in blend[i].channels.iter().enumerate() {
                    if ch.is_some() {
                        animated[c] += 1;
                    }
                }
            }
        }
        let v: Vec<String> = b.value.iter().map(|x| format!("{x:.3}")).collect();
        let sc: Vec<String> = b.scale.iter().map(|x| format!("{x:.5}")).collect();
        println!(
            "{i:2} {:24} p{:3} val [{}] scale [{}] anim {:?}",
            b.name,
            b.parent,
            v.join(" "),
            sc.join(" "),
            animated
        );
    }
    for c in &m.bone_controllers {
        println!("ctrl {c:?}");
    }
    for a in &m.attachments {
        println!("att {} bone {} org {:?}", a.name, a.bone, a.org);
    }
    for s in m.sequences.iter().take(3) {
        println!(
            "seq {} blends {} bb {:?} {:?} motion {} {:?}",
            s.label,
            s.blends.len(),
            s.bbmin,
            s.bbmax,
            s.motion_type,
            s.linear_movement
        );
    }
    println!("eye {:?} flags {}", m.eye_position, m.flags);
}
