//! Triangle strips (D11): greedy stripification of corner-indexed triangles.
//! A corner is the full (vertex, normal, s, t) the MDL stores, so two
//! triangles only chain when they share both corners of an edge exactly.
//! Strips follow OpenGL rules: triangle i is (v[i], v[i+1], v[i+2]) when i is
//! even and (v[i+1], v[i], v[i+2]) when odd, which keeps every triangle's winding.

use std::collections::HashMap;

use crate::mdl::TriCmd;

pub type Corner = [i16; 4];

/// Strips covering exactly `tris`, each triangle once, windings kept.
pub fn strips(tris: &[[Corner; 3]]) -> Vec<TriCmd> {
    // Directed edge (a → b) → triangles having it, with their third corner.
    let mut edges: HashMap<(Corner, Corner), Vec<(usize, Corner)>> = HashMap::new();
    for (i, t) in tris.iter().enumerate() {
        for k in 0..3 {
            edges
                .entry((t[k], t[(k + 1) % 3]))
                .or_default()
                .push((i, t[(k + 2) % 3]));
        }
    }
    let mut used = vec![false; tris.len()];
    // `stamp[t] == attempt` marks triangles taken by the strip being tried.
    let mut stamp = vec![0usize; tris.len()];
    let mut attempt = 0;
    let mut out = Vec::new();
    for start in 0..tris.len() {
        if used[start] {
            continue;
        }
        let mut best: Option<(Vec<Corner>, Vec<usize>)> = None;
        for rot in 0..3 {
            attempt += 1;
            let t = tris[start];
            let mut v = vec![t[rot], t[(rot + 1) % 3], t[(rot + 2) % 3]];
            let mut taken = vec![start];
            stamp[start] = attempt;
            loop {
                let n = v.len();
                let (x, y) = (v[n - 2], v[n - 1]);
                // The next triangle's index in the strip is n - 2.
                let key = if (n - 2) % 2 == 0 { (x, y) } else { (y, x) };
                let next = edges
                    .get(&key)
                    .and_then(|c| c.iter().find(|(ti, _)| !used[*ti] && stamp[*ti] != attempt));
                let Some(&(ti, d)) = next else { break };
                stamp[ti] = attempt;
                taken.push(ti);
                v.push(d);
                if v.len() >= i16::MAX as usize {
                    break;
                }
            }
            if best.as_ref().is_none_or(|(_, b)| taken.len() > b.len()) {
                best = Some((v, taken));
            }
        }
        let (verts, taken) = best.unwrap();
        for t in taken {
            used[t] = true;
        }
        out.push(TriCmd { fan: false, verts });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Triangles a command list draws, each rotated to start at its smallest corner.
    fn unroll(cmds: &[TriCmd]) -> Vec<[Corner; 3]> {
        let mut out = Vec::new();
        for c in cmds {
            for i in 0..c.verts.len() - 2 {
                let t = if i % 2 == 0 {
                    [c.verts[i], c.verts[i + 1], c.verts[i + 2]]
                } else {
                    [c.verts[i + 1], c.verts[i], c.verts[i + 2]]
                };
                out.push(canon(t));
            }
        }
        out.sort();
        out
    }

    fn canon(t: [Corner; 3]) -> [Corner; 3] {
        let k = (0..3).min_by_key(|&k| t[k]).unwrap();
        [t[k], t[(k + 1) % 3], t[(k + 2) % 3]]
    }

    fn c(v: i16) -> Corner {
        [v, v, v, v]
    }

    #[test]
    fn grid_becomes_fewer_commands_with_same_triangles() {
        // A 6×6 quad grid, two triangles per quad, consistent winding.
        let mut tris = Vec::new();
        for y in 0..6i16 {
            for x in 0..6i16 {
                let (a, b, cc, d) = (
                    y * 7 + x,
                    y * 7 + x + 1,
                    (y + 1) * 7 + x,
                    (y + 1) * 7 + x + 1,
                );
                tris.push([c(a), c(b), c(cc)]);
                tris.push([c(b), c(d), c(cc)]);
            }
        }
        let cmds = strips(&tris);
        let mut want: Vec<_> = tris.iter().map(|t| canon(*t)).collect();
        want.sort();
        assert_eq!(unroll(&cmds), want);
        assert!(
            cmds.len() < tris.len() / 3,
            "{} strips for {} triangles",
            cmds.len(),
            tris.len()
        );
    }

    #[test]
    fn mismatched_uv_breaks_the_chain() {
        // Same positions, but the shared edge has different texture coordinates.
        let t1 = [[0, 0, 0, 0], [1, 1, 0, 0], [2, 2, 0, 0]];
        let t2 = [[2, 2, 9, 9], [1, 1, 0, 0], [3, 3, 0, 0]];
        let cmds = strips(&[t1, t2]);
        assert_eq!(cmds.len(), 2);
        assert_eq!(unroll(&cmds).len(), 2);
    }
}
