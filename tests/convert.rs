//! Lucian (VRM) → GoldSrc with yuki_ref as the animation donor. Both samples
//! are licensed local files (A6) kept out of git, so the test skips without them.

use std::collections::HashMap;
use std::path::PathBuf;

use libremodele::{convert, fbx, mdl, preview, sway, unitypkg, vrm};

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
            max_tris: None,
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

    // Lossless (C2): reduction only removes triangles, never adds; textures fit D6.
    assert!(
        m.tri_count() <= 6356 && m.tri_count() > 6000,
        "{}",
        m.tri_count()
    );
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

    // Sway (D15, D16): every chain bone in a spring group, every sequence
    // written within its 64 KB budget, and the result reads back unchanged.
    // Strands sharing triangles (dress panels, back hair) stay static; separate ones sway.
    let swaying: Vec<&str> = c
        .rig
        .chains
        .iter()
        .map(|ch| m.bones[ch.0].name.as_str())
        .collect();
    assert!(
        swaying.iter().all(|n| !n.starts_with("Dress")),
        "{swaying:?}"
    );
    assert!(
        swaying.contains(&"FHair1") && swaying.contains(&"Ear1.L"),
        "{swaying:?}"
    );
    let mut swayed = c.model.clone();
    let sum = sway::apply(&mut swayed, &c.rig).unwrap();
    assert_eq!(sum.full + sum.partial + sum.none, swayed.sequences.len());
    assert!(sum.full > 150, "{} fully swaying", sum.full);
    let bytes = mdl::write(&swayed).unwrap();
    assert_eq!(mdl::read(&bytes).unwrap(), swayed);
    // Body bones are untouched.
    for (a, b) in m.sequences.iter().zip(&swayed.sequences) {
        for (bi, bone) in m.bones.iter().enumerate() {
            if !c.rig.chains.iter().any(|ch| ch.0 == bi) {
                assert_eq!(a.blends[0][bi], b.blends[0][bi], "{}", bone.name);
            }
        }
    }
}

/// Miku (VRM, 65,737 tris, 191 chain-heavy bones, 4 images with alpha):
/// the bone budget (D2), lossy decimation (C4, C8) and one texture per
/// image with masks (C5, D6). Local fixture only (A6).
#[test]
fn miku_converts() {
    let (Some(v), Some(d)) = (
        sample("miku/HatsuneMikuNT.vrm"),
        sample("yuki_ref/models/player/yuki_muumuyu/yuki_muumuyu.mdl"),
    ) else {
        return;
    };
    let av = vrm::load(&v).unwrap();
    let donor = mdl::read(&d).unwrap();
    let opts = convert::Options {
        name: "miku".into(),
        height: None,
        keep: HashMap::new(),
        max_tris: None,
    };
    let c = convert::convert(&av, &donor, &opts).unwrap();
    let m = &c.model;
    assert_eq!(m.bones.len(), convert::MAX_BONES);
    // Default budget: the 20k engine maximum (C4).
    assert!(
        m.tri_count() <= 20000 && m.tri_count() > 19500,
        "{}",
        m.tri_count()
    );
    for sm in m.bodyparts.iter().flat_map(|b| &b.models) {
        assert!(sm.verts.len() <= convert::MAX_VERTS && sm.norms.len() <= convert::MAX_VERTS);
        // Normals run mesh by mesh, in order (GoldSrc lights them that way).
        let mut next = 0;
        for me in &sm.meshes {
            assert_eq!(me.norm_index, next);
            next += me.num_norms;
        }
        assert_eq!(next as usize, sm.norms.len());
    }
    // Body, costume, hair, hair cutout, transparent cutout.
    let masked = m.textures.iter().filter(|t| t.flags & 0x40 != 0).count();
    assert_eq!((m.textures.len(), masked), (5, 2));
    assert_eq!(m.skin_families, vec![(0..5).collect::<Vec<i16>>()]);
    assert_eq!(mdl::read(&mdl::write(m).unwrap()).unwrap(), *m);
    // Deterministic: the same input gives the same model.
    let again = convert::convert(&av, &donor, &opts).unwrap();
    assert_eq!(again.model, *m);
}

/// Neru: FBX + Unity package (phase G, B9): Unity humanoid map (G10), FBX
/// materials (G2, scene overrides skipped, G9), flat colours in one swatch
/// texture (G3). Local fixture only (A6, G7).
#[test]
fn neru_converts() {
    let (Some(f), Some(pk), Some(d)) = (
        sample("neru/Zako Neru 2.0/FBX/zako.fbx"),
        sample("neru/Zako Neru 2.0/Unitypackage/Zako Neru.unitypackage"),
        sample("yuki_ref/models/player/yuki_muumuyu/yuki_muumuyu.mdl"),
    ) else {
        return;
    };
    let pkg = unitypkg::Package::read(&pk).unwrap();
    let av = fbx::load(
        &f,
        &fbx::Load {
            file_name: "zako.fbx".into(),
            title: "zako".into(),
            licence: "local fixture".into(),
            package: Some(&pkg),
            variant: None,
            dir: None,
        },
    )
    .unwrap();
    assert_eq!(av.humanoid.len(), 51);
    assert_eq!(av.parts.len(), 23);
    // Feet on the ground, head about 1.2 m up: axes and units (G6).
    let ys = av.parts.iter().flat_map(|p| &p.positions).map(|p| p[1]);
    let (lo, hi) = ys.fold((f32::MAX, f32::MIN), |(a, b), y| (a.min(y), b.max(y)));
    assert!(lo.abs() < 0.02 && (hi - 1.2).abs() < 0.05, "{lo} {hi}");
    let donor = mdl::read(&d).unwrap();
    let opts = convert::Options {
        name: "neru".into(),
        height: None,
        keep: HashMap::new(),
        max_tris: None,
    };
    let c = convert::convert(&av, &donor, &opts).unwrap();
    let m = &c.model;
    assert!(
        m.tri_count() <= 20000 && m.tri_count() > 19500,
        "{}",
        m.tri_count()
    );
    // The sweater texture and the swatch.
    assert_eq!(m.textures.len(), 2);
    for sm in m.bodyparts.iter().flat_map(|b| &b.models) {
        assert!(sm.verts.len() <= convert::MAX_VERTS && sm.norms.len() <= convert::MAX_VERTS);
    }
    assert_eq!(mdl::read(&mdl::write(m).unwrap()).unwrap(), *m);
    let again = convert::convert(&av, &donor, &opts).unwrap();
    assert_eq!(again.model, *m);
}

/// Gothic Doll's outfit: an unpacked prefab gives `.mat` materials per
/// renderer (G2), the default variant is the first by name (G4), bones map by
/// name (G1) and PhysBones become spring groups (G5). Read only: it is an
/// outfit without its base doll, so it isn't converted here. Local fixture (A6).
#[test]
fn gothic_reads() {
    let (Some(f), Some(pk)) = (
        sample("gothic/Gothic Doll/FBX/Gothic Doll.fbx"),
        sample("gothic/Gothic Doll/Gothic Doll.unitypackage"),
    ) else {
        return;
    };
    let pkg = unitypkg::Package::read(&pk).unwrap();
    let s = unitypkg::settings(&pkg, "Gothic Doll.fbx", None).unwrap();
    assert!(s.human.is_empty());
    assert_eq!(s.physbones.len(), 16);
    let mut mats: Vec<(String, Vec<String>)> = s
        .renderer_materials
        .iter()
        .map(|(k, v)| {
            (
                k.clone(),
                v.iter().map(|m| m.as_ref().unwrap().name.clone()).collect(),
            )
        })
        .collect();
    mats.sort();
    assert_eq!(mats.len(), 6);
    // The default (black) variant has no white.mat.
    assert!(
        mats.iter().flat_map(|(_, v)| v).all(|m| m != "white.mat"),
        "{mats:?}"
    );
    let av = fbx::load(
        &f,
        &fbx::Load {
            file_name: "Gothic Doll.fbx".into(),
            title: "Gothic Doll".into(),
            licence: "local fixture".into(),
            package: Some(&pkg),
            variant: None,
            dir: None,
        },
    )
    .unwrap();
    assert_eq!(av.parts.len(), 14);
    assert_eq!(av.humanoid.len(), 19);
    assert_eq!(av.spring_groups.len(), 16);
    assert!(av.parts.iter().all(|p| p.image.is_some()));
}
