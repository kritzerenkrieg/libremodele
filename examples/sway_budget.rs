// D15 spike: per-sequence animation bytes now, and the worst case if `n` chain
// bones get 3 animated rotation channels (no run-length savings).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let m = libremodele::mdl::read(&std::fs::read(&a[1]).unwrap()).unwrap();
    let n: usize = a[2].parse().unwrap();
    let mut worst = (0usize, String::new());
    let mut over = 0;
    let mut total_add = 0usize;
    for s in &m.sequences {
        let f = s.num_frames as usize;
        let per_blend_structs = m.bones.len() * 12;
        // Existing data, measured by re-encoding.
        let existing: usize = s
            .blends
            .iter()
            .flatten()
            .flat_map(|b| b.channels.iter().flatten())
            .map(|v| 2 * (v.len() + v.len().div_ceil(255)))
            .sum();
        let added = s.blends.len() * n * 3 * 2 * (f + f.div_ceil(255));
        total_add += added;
        let span = s.blends.len() * per_blend_structs + existing + added;
        if span > 65535 {
            over += 1;
        }
        if span > worst.0 {
            worst = (
                span,
                format!("{} ({} frames, {} blends)", s.label, f, s.blends.len()),
            );
        }
    }
    println!(
        "bones {} sequences {}: worst sequence span {} bytes in {}; {} sequences over 64 KB; total added {:.2} MB",
        m.bones.len(),
        m.sequences.len(),
        worst.0,
        worst.1,
        over,
        total_add as f64 / 1e6
    );
}
