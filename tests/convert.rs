//! Lucian (VRM) → GoldSrc with yuki_ref as the animation donor. Both samples
//! are licensed local files (A6) kept out of git, so the test skips without them.

use std::collections::HashMap;
use std::path::PathBuf;

use libremodele::{convert, mdl, preview, vrm};

fn sample(rel: &str) -> Option<Vec<u8>> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("samples")
        .join(rel);
    std::fs::read(&p)
        .map_err(|_| eprintln!("skipped: no sample at {}", p.display()))
        .ok()
}

#[test]
fn lucian_converts() {
    let (Some(v), Some(d)) = (
        sample("lucian/DrHeart_Lucian_SF/VRM_Lucian_SF.vrm"),
        sample("yuki_ref/models/player/yuki_muumuyu/yuki_muumuyu.mdl"),
    ) else {
        return;
    };
    let av = vrm::load(&v).unwrap();
    let donor = mdl::read(&d).unwrap();
    let c = convert::convert(
        &av,
        &donor,
        &convert::Options {
            name: "lucian".into(),
            height: None,
            keep: HashMap::new(),
        },
    )
    .unwrap();
    let m = &c.model;

    // Limits (A11 working values) and the re-read check (A14).
    assert!(m.bones.len() <= convert::MAX_BONES);
    for sm in m.bodyparts.iter().flat_map(|b| &b.models) {
        assert!(sm.verts.len() <= convert::MAX_VERTS && sm.norms.len() <= convert::MAX_VERTS);
    }
    assert_eq!(mdl::read(&mdl::write(m).unwrap()).unwrap(), *m);

    // Lossless (C2): every input triangle survives; textures fit D6.
    assert_eq!(m.tri_count(), 6356);
    assert_eq!(
        (
            m.textures[0].width,
            m.textures[0].height,
            m.textures[0].flags
        ),
        (512, 512, 1)
    );

    // Donor animations and extras carried over (D7, D8); chains kept (D2).
    assert_eq!(m.sequences.len(), donor.sequences.len());
    assert_eq!((m.bone_controllers.len(), m.attachments.len()), (5, 4));
    assert!(
        m.bones.iter().any(|b| b.name == "FHair1") && m.bones.iter().any(|b| b.name == "DressA1.L")
    );
    assert!(
        !m.bones
            .iter()
            .any(|b| b.name.starts_with("Index") || b.name.starts_with("Bip01 Tail"))
    );

    // Hitboxes are the donor's set on kept bones (D14), same groups and order.
    let kept: Vec<_> = donor
        .hitboxes
        .iter()
        .filter(|h| {
            m.bones
                .iter()
                .any(|b| b.name == donor.bones[h.bone as usize].name)
        })
        .collect();
    assert_eq!(m.hitboxes.len(), kept.len());
    for (ours, theirs) in m.hitboxes.iter().zip(&kept) {
        assert_eq!(
            m.bones[ours.bone as usize].name,
            donor.bones[theirs.bone as usize].name
        );
        assert_eq!(ours.group, theirs.group);
    }
    assert!(m.textures.iter().all(|t| t.width <= 512 && t.height <= 512));

    // Faces wind the same way as the donor's, so the engine culls the right side.
    assert_eq!(preview::winding_sign(m), preview::winding_sign(&donor));

    // Height matches the donor (B8) within a unit.
    let w = preview::bone_worlds(m, None);
    let zs: Vec<f32> = m
        .bodyparts
        .iter()
        .flat_map(|b| &b.models)
        .flat_map(|sm| {
            sm.verts
                .iter()
                .zip(&sm.vert_bones)
                .map(|(v, &b)| w[b as usize].apply(*v)[2])
        })
        .collect();
    let h =
        zs.iter().cloned().fold(f32::MIN, f32::max) - zs.iter().cloned().fold(f32::MAX, f32::min);
    assert!((h - 54.65).abs() < 1.0, "height {h}");
}
