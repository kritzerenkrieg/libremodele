//! The whole conversion as library calls, shared by the CLI and the GUI (H6):
//! [`inspect`] opens a download and makes every choice (A15), [`build`] turns
//! it into a checked model, [`save`] writes the game and review files.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::{convert, donor, mdl, preview, source, sway, texture};

/// What the user asked for; every field but `input` is an override (A15).
#[derive(Clone, Debug, Default)]
pub struct Request {
    pub input: PathBuf,
    pub name: Option<String>,
    /// `--donor`; chosen by height when absent (G22).
    pub donor: Option<PathBuf>,
    pub avatar: Option<String>,
    pub package: Option<String>,
    pub licence: Option<String>,
    pub variant: Option<String>,
    /// `--material "<FBX material>=<.mat or image>"` pairs (G12).
    pub materials: Vec<(String, String)>,
    pub height: Option<f32>,
    pub max_tris: Option<usize>,
    pub keep_polygons: bool,
    pub sway: bool,
    /// Clean-manifest overrides: part id → keep (B4).
    pub keep: HashMap<String, bool>,
}

/// A download opened and every choice made, before the heavy work.
pub struct Inspected {
    pub chosen: source::Chosen,
    pub name: String,
    pub donor: PathBuf,
    /// Why this donor (G22); `None` when given by the user.
    pub donor_why: Option<String>,
}

/// A finished, re-read-checked model and its review data.
pub struct Built {
    pub name: String,
    pub model: mdl::Model,
    pub bytes: Vec<u8>,
    pub manifest: Value,
    pub report: Vec<String>,
}

/// Open the input and choose the avatar file, package, licence, name and donor.
pub fn inspect(req: &Request) -> Result<Inspected> {
    let (mut files, file) = source::Files::open(&req.input)?;
    let overrides = source::Overrides {
        avatar: req.avatar.clone().or(file),
        package: req.package.clone(),
        licence: req.licence.clone(),
        variant: req.variant.clone(),
        materials: req.materials.clone(),
    };
    // A package path outside the input is read as is.
    if let Some(p) = &overrides.package
        && files.get(p).is_none()
        && !files
            .entries
            .iter()
            .any(|(e, _)| e.ends_with(&format!("/{p}")))
    {
        let data = std::fs::read(p).with_context(|| format!("reading {p}"))?;
        files.entries.push((p.clone(), data));
    }
    let chosen = source::choose(&files, &overrides)
        .with_context(|| format!("reading {}", req.input.display()))?;
    let name = match &req.name {
        Some(n) => n.clone(),
        None => source::default_name(&req.input, &chosen.path),
    };
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        bail!("the model name must be ASCII letters, digits, _ or -");
    }
    let (donor, donor_why) = match &req.donor {
        Some(d) => (d.clone(), None),
        None => {
            let (p, why) = donor::choose(&chosen.avatar)?;
            (p, Some(why))
        }
    };
    Ok(Inspected {
        chosen,
        name,
        donor,
        donor_why,
    })
}

/// Convert, add sway when asked (D15, D16), write and re-read (A14).
/// `progress` hears each stage as it starts.
pub fn build(insp: &Inspected, req: &Request, progress: &mut dyn FnMut(&str)) -> Result<Built> {
    progress("reading the donor");
    let donor = donor::read(&insp.donor)?;
    progress("converting");
    let mut c = convert::convert(
        &insp.chosen.avatar,
        &donor,
        &convert::Options {
            name: insp.name.clone(),
            height: req.height,
            keep: req.keep.clone(),
            max_tris: req.max_tris,
            keep_polygons: req.keep_polygons,
        },
    )?;
    if let Some(why) = &insp.donor_why {
        c.report
            .insert(0, format!("donor (G22): {} ({why})", insp.donor.display()));
    }
    let mut size_note = None;
    if req.sway {
        progress("simulating sway");
        let before = mdl::write(&c.model)?.len();
        let t = std::time::Instant::now();
        let sum = sway::apply(&mut c.model, &c.rig)?;
        c.report.push(format!(
            "sway (D15): {} chain bones in {} groups; {} sequences fully swaying, {} partly (budget), {} static; {:.1} s",
            c.rig.chains.len(),
            c.rig.groups.len(),
            sum.full,
            sum.partial,
            sum.none,
            t.elapsed().as_secs_f32()
        ));
        size_note = Some(before);
    }
    progress("writing");
    let bytes = mdl::write(&c.model)?;
    if let Some(before) = size_note {
        c.report.push(format!(
            "sway: +{} bytes ({:.2} MB)",
            bytes.len() - before,
            (bytes.len() - before) as f64 / 1e6
        ));
    }
    // G5 under A14: our own re-read must give back exactly what we built.
    let again = mdl::read(&bytes).context("re-reading the written model")?;
    if again != c.model {
        bail!("the written model does not read back identically");
    }
    c.report.push(format!(
        "model: {} bytes, re-read identical (A14)",
        bytes.len()
    ));
    Ok(Built {
        name: insp.name.clone(),
        model: c.model,
        bytes,
        manifest: c.manifest,
        report: c.report,
    })
}

/// Write `<out>/goldsrc/models/player/<name>/` (model + portrait) and
/// `<out>/review/<name>/` (preview sheet, manifest, report). Returns both
/// folders; the report gains the preview tile labels.
pub fn save(b: &mut Built, out: &Path) -> Result<(PathBuf, PathBuf)> {
    let game = out.join("goldsrc/models/player").join(&b.name);
    let review = out.join("review").join(&b.name);
    std::fs::create_dir_all(&game)?;
    std::fs::create_dir_all(&review)?;
    write(&game.join(format!("{}.mdl", b.name)), &b.bytes)?;
    let (sheet, labels) = preview::sheet(&b.model, (240, 300));
    sheet.save(review.join("preview.png"))?;
    write(
        &game.join(format!("{}.bmp", b.name)),
        &texture::bmp8(&thumbnail(&b.model)),
    )?;
    write(
        &review.join("clean_manifest.json"),
        serde_json::to_string_pretty(&b.manifest)?.as_bytes(),
    )?;
    b.report.extend(labels);
    write(
        &review.join("report.txt"),
        (b.report.join("\n") + "\n").as_bytes(),
    )?;
    Ok((game, review))
}

/// Copy a saved model folder into Sven Co-op's addon player models (H2),
/// replacing an older copy of the same name. Returns the target folder.
pub fn install(game: &Path, addon_player: &Path) -> Result<PathBuf> {
    let name = game.file_name().context("the model folder has no name")?;
    let target = addon_player.join(name);
    std::fs::create_dir_all(&target).with_context(|| format!("creating {}", target.display()))?;
    for e in std::fs::read_dir(game)? {
        let e = e?;
        if e.file_type()?.is_file() {
            let to = target.join(e.file_name());
            std::fs::copy(e.path(), &to).with_context(|| format!("copying to {}", to.display()))?;
        }
    }
    Ok(target)
}

/// Read an MDL file.
pub fn read_mdl(path: &Path) -> Result<mdl::Model> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    mdl::read(&bytes).with_context(|| format!("parsing {}", path.display()))
}

/// The 164×200 player-menu picture (D8): idle pose from the front.
pub fn thumbnail(m: &mdl::Model) -> texture::Indexed {
    let pose = m
        .sequences
        .iter()
        .position(|s| s.label == "idle")
        .map(|i| (i, 0));
    let tris = preview::triangles(m, &preview::bone_worlds(m, pose));
    let frame = preview::framing(&tris, preview::View::Front);
    let img = preview::render(
        m,
        &tris,
        preview::winding_sign(m),
        preview::View::Front,
        frame,
        164,
        200,
        [24, 24, 28],
    );
    let px: Vec<[u8; 3]> = img.pixels().map(|p| p.0).collect();
    texture::quantise(&px, 164, 200)
}

fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
}
