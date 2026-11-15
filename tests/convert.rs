//! Lucian (VRM) → GoldSrc with yuki_ref as the animation donor. Both samples
//! are licensed local files (A6) kept out of git, so the test skips without them.

use std::collections::HashMap;
use std::path::PathBuf;

use libremodele::{convert, fbx, mdl, preview, source, sway, unitypkg, vrm};

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

    // Lossless (C2): reduction only removes triangles, never adds; textures
    // fit D6. Back faces for open sheets come on top (D17).
    let backs: usize = c
        .report
        .iter()
        .find_map(|l| l.strip_prefix("two-sided (D17): "))
        .and_then(|l| l.split(' ').next())
        .and_then(|n| n.parse().ok())
        .unwrap();
    assert!(backs > 0);
    let front = m.tri_count() - backs;
    assert!(front <= 6356 && front > 6000, "{}", front);
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
            files: None,
            path: String::new(),
            materials: Vec::new(),
        },
    )
    .unwrap();
    assert_eq!(av.humanoid.len(), 51);
    assert_eq!(av.parts.len(), 23);
    // G18: the scene's overrides on FBX objects resolve by hashed fileID:
    // all 23 material slots and both PhysBones (sway, D15).
    assert!(
        av.notes
            .iter()
            .any(|n| n
                == "unitypackage: 23 material overrides applied by Unity's hashed fileIDs (G18)"),
        "{:?}",
        av.notes
    );
    assert!(
        av.notes
            .iter()
            .any(|n| n.contains("FBX material Material.005 → Hair 1.mat (prefab, G2)"))
    );
    assert_eq!(av.spring_groups.len(), 2);
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
    let s = unitypkg::settings(&pkg, "Gothic Doll.fbx", &[], &[], None).unwrap();
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
            files: None,
            path: String::new(),
            materials: Vec::new(),
        },
    )
    .unwrap();
    assert_eq!(av.parts.len(), 14);
    assert_eq!(av.humanoid.len(), 19);
    assert_eq!(av.spring_groups.len(), 16);
    assert!(av.parts.iter().all(|p| p.image.is_some()));
}

/// Poka Blondy and Blindy: loose `*_NoStencilMesh.fbx` files that aren't in
/// their packages, so the package FBX is matched on bone names and the
/// variant on that FBX (G11); Blindy's materials come from `--material`
/// (G12). Local fixtures (A6).
fn poka(fbx_rel: &str, pkg_rel: &str, materials: &[(&str, &str)]) -> Option<convert::Converted> {
    let (Some(f), Some(pk), Some(d)) = (
        sample(fbx_rel),
        sample(pkg_rel),
        sample("yuki_ref/models/player/yuki_muumuyu/yuki_muumuyu.mdl"),
    ) else {
        return None;
    };
    let pkg = unitypkg::Package::read(&pk).unwrap();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("samples")
        .join(fbx_rel);
    let (files, rel) = source::Files::open(&path).unwrap();
    let av = fbx::load(
        &f,
        &fbx::Load {
            file_name: path.file_name().unwrap().to_string_lossy().into(),
            title: "poka".into(),
            licence: "local fixture".into(),
            package: Some(&pkg),
            variant: None,
            files: Some(&files),
            path: rel.unwrap(),
            materials: materials
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        },
    )
    .unwrap();
    // Unity's map from the matched package FBX (G10, G11): `root` is Hips.
    assert_eq!(av.nodes[av.humanoid["hips"]].name, "root");
    // G18: the prefab's material overrides belong to the package's FBX, not
    // this loose one (G11), so they are held back; PhysBones still apply.
    assert!(
        av.notes
            .iter()
            .any(|n| n.ends_with("3 not (the converted FBX is not the package's own, G11)")),
        "{:?}",
        av.notes
    );
    assert!(!av.spring_groups.is_empty());
    // Every part textured (no flat colours, no swatch).
    assert!(av.parts.iter().all(|p| p.image.is_some()));
    assert_eq!(av.images.len(), 3);
    let donor = mdl::read(&d).unwrap();
    let c = convert::convert(
        &av,
        &donor,
        &convert::Options {
            name: "poka".into(),
            height: None,
            keep: HashMap::new(),
            max_tris: None,
        },
    )
    .unwrap();
    assert_eq!(c.model.textures.len(), 3);
    assert_eq!(mdl::read(&mdl::write(&c.model).unwrap()).unwrap(), c.model);
    Some(c)
}

#[test]
fn poka_blondy_converts() {
    let Some(c) = poka(
        "poka_blondy/Blondy_Assets/Blondy_NoStencilMesh.fbx",
        "poka_blondy/KB_Poka_Blondy_PC.unitypackage",
        &[],
    ) else {
        return;
    };
    // Low-poly: nothing to decimate (C2); open sheets get back faces from
    // the spare budget (D17).
    assert_eq!(c.model.tri_count(), 1923 + 1763);
    assert!(
        c.report
            .iter()
            .any(|l| l.starts_with("two-sided (D17): 1763 of 1763"))
    );
}

#[test]
fn poka_blindy_converts() {
    let Some(c) = poka(
        "poka_blind/Blindy_Assets/BlindGirl_PrimaryOutfitOnly_NoStencilMesh.fbx",
        "poka_blind/KB_Poka_BlindGirl_PC.unitypackage",
        &[
            ("body_blindy.001", "bodyo.png"),
            ("hair_blindy.001", "Blindy_Hair"),
            ("face eyes", "Blindy_Face"),
        ],
    ) else {
        return;
    };
    assert_eq!(c.model.tri_count(), 1569 + 1487);
    assert!(
        c.report
            .iter()
            .any(|l| l.contains("→ bodyo.png (--material, G12)"))
    );
}

/// A15, G15, G16, G17: each sample folder (an unpacked download) picks its
/// avatar file, package and licence with no overrides, and the outfit-only
/// Gothic Doll is refused.
#[test]
fn downloads_choose() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("samples");
    let cases = [
        (
            "lucian",
            "DrHeart_Lucian_SF/VRM_Lucian_SF.vrm",
            "from the VRM meta",
        ),
        ("miku", "HatsuneMikuNT.vrm", "from the VRM meta"),
        (
            "neru",
            "Zako Neru 2.0/FBX/zako.fbx",
            "warning: no licence text",
        ),
        (
            "poka_blondy",
            "Blondy_Assets/Blondy_NoStencilMesh.fbx",
            "from LICENSE.txt",
        ),
        (
            "poka_blind",
            "Blindy_Assets/BlindGirl_PrimaryOutfitOnly_NoStencilMesh.fbx",
            "from LICENSE.txt",
        ),
    ];
    for (dir, want, licence) in cases {
        let Ok((files, None)) = source::Files::open(&root.join(dir)) else {
            eprintln!("skipped: no sample folder {dir}");
            continue;
        };
        let c = source::choose(&files, &source::Overrides::default()).unwrap();
        assert_eq!(c.path, want, "{dir}");
        assert!(
            c.avatar.notes.iter().any(|n| n.contains(licence)),
            "{dir}: {:?}",
            c.avatar.notes
        );
        if dir == "poka_blind" {
            // G14: paired with no `--material`; `bodyo.png`, not the
            // alternate outfit's or `Blindy_Body.mat`'s mismatched atlas.
            let paired: Vec<_> = c
                .avatar
                .notes
                .iter()
                .filter(|n| n.contains(": texture paired (G14)"))
                .collect();
            assert_eq!(paired.len(), 3, "{:?}", c.avatar.notes);
            assert!(
                paired[0].contains("Textures/bodyo.png (score"),
                "{}",
                paired[0]
            );
            assert!(
                paired[1].contains(": Blindy_Hair.mat (score"),
                "{}",
                paired[1]
            );
            assert!(
                paired[2].contains(": Blindy_Face.mat (score"),
                "{}",
                paired[2]
            );
            assert!(c.avatar.notes.iter().all(|n| !n.starts_with("swatch")));
        } else {
            // Blondy and Neru unchanged: nothing paired (G14).
            assert!(
                c.avatar
                    .notes
                    .iter()
                    .all(|n| !n.contains(": texture paired (G14)")),
                "{dir}: {:?}",
                c.avatar.notes
            );
        }
        let cover = convert::head_cover(&c.avatar).unwrap();
        eprintln!("{dir}: head cover {cover:.2}");
        assert!(cover >= convert::MIN_HEAD_COVER + 0.2, "{dir}: {cover}");
    }
    if let Ok((files, _)) = source::Files::open(&root.join("gothic")) {
        let e = source::choose(&files, &source::Overrides::default())
            .err()
            .unwrap();
        assert!(format!("{e:#}").contains("looks like an outfit"), "{e:#}");
    }
}

#[test]
fn unzip_reads_stored_and_deflated() {
    use std::io::Write;
    // Build a two-entry zip by hand: one stored, one deflated.
    let entries: [(&str, &[u8], u16); 2] =
        [("a/b.txt", b"hello", 0), ("c.txt", b"world world world", 8)];
    let (mut out, mut cd) = (Vec::new(), Vec::new());
    for (name, data, method) in entries {
        let body = if method == 8 {
            let mut e =
                flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
            e.write_all(data).unwrap();
            e.finish().unwrap()
        } else {
            data.to_vec()
        };
        let off = out.len() as u32;
        let head = |sig: &[u8], central: bool| {
            let mut h = sig.to_vec();
            if central {
                h.extend(20u16.to_le_bytes());
            }
            h.extend(20u16.to_le_bytes());
            h.extend(0u16.to_le_bytes());
            h.extend(method.to_le_bytes());
            h.extend([0u8; 8]); // time, date, crc
            h.extend((body.len() as u32).to_le_bytes());
            h.extend((data.len() as u32).to_le_bytes());
            h.extend((name.len() as u16).to_le_bytes());
            h.extend(0u16.to_le_bytes());
            if central {
                h.extend([0u8; 10]); // comment len, disk, attributes
                h.extend(off.to_le_bytes());
            }
            h.extend(name.as_bytes());
            h
        };
        out.extend(head(b"PK\x03\x04", false));
        out.extend(&body);
        cd.extend(head(b"PK\x01\x02", true));
    }
    let cd_off = out.len() as u32;
    out.extend(&cd);
    out.extend(b"PK\x05\x06");
    out.extend([0u8; 4]);
    out.extend(2u16.to_le_bytes());
    out.extend(2u16.to_le_bytes());
    out.extend((cd.len() as u32).to_le_bytes());
    out.extend(cd_off.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    let got = source::unzip(&out).unwrap();
    assert_eq!(got[0], ("a/b.txt".to_string(), b"hello".to_vec()));
    assert_eq!(got[1], ("c.txt".to_string(), b"world world world".to_vec()));
}

#[test]
fn other_archives_are_refused_clearly() {
    let dir = std::env::temp_dir().join(format!("libremodele-rar-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("avatar.rar");
    std::fs::write(&f, b"Rar!").unwrap();
    let e = source::Files::open(&f).err().unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(
        format!("{e:#}").contains("unsupported input (.rar)"),
        "{e:#}"
    );
}

#[test]
fn default_names_fit() {
    use std::path::Path;
    let n = |i: &str, a: &str| source::default_name(Path::new(i), a);
    assert_eq!(
        n("/x/KB_Poka_BlindGirl_v1.6.zip", "q.fbx"),
        "kb_poka_blindgirl"
    );
    assert_eq!(n("/x/Neru_2.0.zip", "q.fbx"), "neru_2_0");
    assert!(n("/x/A_Very_Long_Download_Name_Here.zip", "q").len() <= source::MAX_NAME);
}
