//! A10: the MDL writer reproduces a real Sven Co-op model: the built-in chibi
//! donor, or any MDL named by `LIBREMODELE_SAMPLE_MDL`.

use std::path::PathBuf;

use libremodele::mdl;

fn sample() -> Option<Vec<u8>> {
    let path = std::env::var_os("LIBREMODELE_SAMPLE_MDL")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("donors/yuki_muumuyu.mdl")
        });
    match std::fs::read(&path) {
        Ok(b) => Some(b),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("skipped: no sample at {}", path.display());
            None
        }
        Err(e) => panic!("reading {}: {e}", path.display()),
    }
}

#[test]
fn yuki_round_trip() {
    let Some(bytes) = sample() else { return };
    let a = mdl::read(&bytes).unwrap();
    let out = mdl::write(&a).unwrap();
    let b = mdl::read(&out).unwrap();

    // Section by section first, so a failure names the part that changed.
    assert_eq!(a.bones, b.bones);
    assert_eq!(a.bone_controllers, b.bone_controllers);
    assert_eq!(a.hitboxes, b.hitboxes);
    assert_eq!(a.attachments, b.attachments);
    assert_eq!(a.textures, b.textures);
    assert_eq!(a.skin_families, b.skin_families);
    assert_eq!(a.bodyparts, b.bodyparts);
    for (x, y) in a.sequences.iter().zip(&b.sequences) {
        assert_eq!(x, y, "sequence {}", x.label);
    }
    assert_eq!(a, b);

    // A writer that is stable: writing the re-read model gives the same bytes.
    assert_eq!(out, mdl::write(&b).unwrap());
    // The engine frees from texturedataindex on, so pixels must end the file.
    // Header: 140 bytes up to and including `flags`, then texturedataindex is field 12.
    let tex_data = i32::from_le_bytes(out[140 + 12 * 4..140 + 13 * 4].try_into().unwrap()) as usize;
    let pixels: usize = a.textures.iter().map(|t| t.pixels.len() + 768).sum();
    assert_eq!(tex_data + pixels, out.len());
}

#[test]
fn yuki_matches_known_counts() {
    let Some(bytes) = sample() else { return };
    let m = mdl::read(&bytes).unwrap();
    assert_eq!(
        (
            m.bones.len(),
            m.sequences.len(),
            m.hitboxes.len(),
            m.attachments.len()
        ),
        (38, 192, 31, 4)
    );
    assert_eq!(
        m.bodyparts
            .iter()
            .flat_map(|b| &b.models)
            .map(|s| s.verts.len())
            .sum::<usize>(),
        775
    );
    assert_eq!(
        m.bodyparts
            .iter()
            .flat_map(|b| &b.models)
            .flat_map(|s| &s.meshes)
            .map(|me| me.num_tris)
            .sum::<i32>(),
        1464
    );
}
