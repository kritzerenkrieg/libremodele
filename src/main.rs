use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use libremodele::{convert, mdl, preview, sway, texture, vrm};

const USAGE: &str = "usage:
  libremodele convert <avatar.vrm> --donor <donor.mdl> --name <name> [--height <units>] [--max-tris <n>] [--manifest <clean_manifest.json>] [--out <dir>] [--sway]
  libremodele preview <file.mdl> <out.png>
  libremodele mdl-info <file.mdl>
  libremodele mdl-roundtrip <in.mdl> <out.mdl>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<()> {
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["convert", vrm_path, rest @ ..] => convert_cmd(vrm_path, rest),
        ["preview", path, out] => {
            let m = load(path)?;
            let (img, labels) = preview::sheet(&m, (240, 300));
            img.save(out).with_context(|| format!("writing {out}"))?;
            labels.iter().for_each(|l| println!("{l}"));
            Ok(())
        }
        ["mdl-info", path] => info(&load(path)?),
        ["mdl-roundtrip", input, output] => roundtrip(input, output),
        _ => bail!("{USAGE}"),
    }
}

fn load(path: &str) -> Result<mdl::Model> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {path}"))?;
    mdl::read(&bytes).with_context(|| format!("parsing {path}"))
}

fn flag<'a>(rest: &[&'a str], name: &str) -> Option<&'a str> {
    rest.iter()
        .position(|a| *a == name)
        .and_then(|i| rest.get(i + 1))
        .copied()
}

fn convert_cmd(vrm_path: &str, rest: &[&str]) -> Result<()> {
    let donor_path = flag(rest, "--donor").context("--donor <donor.mdl> is required (D7)")?;
    let name = flag(rest, "--name").context("--name <name> is required")?;
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        bail!("--name must be ASCII letters, digits, _ or -");
    }
    let height = flag(rest, "--height")
        .map(str::parse::<f32>)
        .transpose()
        .context("--height must be a number")?;
    let max_tris = flag(rest, "--max-tris")
        .map(str::parse::<usize>)
        .transpose()
        .context("--max-tris must be a whole number")?;
    let out = PathBuf::from(flag(rest, "--out").unwrap_or("out"));
    let mut keep = HashMap::new();
    if let Some(m) = flag(rest, "--manifest") {
        let v: serde_json::Value =
            serde_json::from_slice(&std::fs::read(m).with_context(|| format!("reading {m}"))?)?;
        for e in v.as_array().into_iter().flatten() {
            if let (Some(p), Some(k)) = (
                e.get("part").and_then(|x| x.as_str()),
                e.get("keep").and_then(|x| x.as_bool()),
            ) {
                keep.insert(p.to_string(), k);
            }
        }
    }

    let av = vrm::load(&std::fs::read(vrm_path).with_context(|| format!("reading {vrm_path}"))?)
        .with_context(|| format!("parsing {vrm_path}"))?;
    let donor = load(donor_path)?;
    let c = convert::convert(
        &av,
        &donor,
        &convert::Options {
            name: name.to_string(),
            height,
            keep,
            max_tris,
        },
    )?;
    let mut c = c;
    let mut size_note = None;
    if rest.contains(&"--sway") {
        // D16: off by default; D15: UniVRM spring port within each sequence's budget.
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

    let game = out.join("goldsrc/models/player").join(name);
    let review = out.join("review").join(name);
    std::fs::create_dir_all(&game)?;
    std::fs::create_dir_all(&review)?;
    write(&game.join(format!("{name}.mdl")), &bytes)?;

    let (sheet, labels) = preview::sheet(&c.model, (240, 300));
    sheet.save(review.join("preview.png"))?;
    let thumb = thumbnail(&c.model);
    write(&game.join(format!("{name}.bmp")), &texture::bmp8(&thumb))?;
    write(
        &review.join("clean_manifest.json"),
        serde_json::to_string_pretty(&c.manifest)?.as_bytes(),
    )?;

    let mut report = c.report;
    report.push(format!(
        "model: {} bytes, re-read identical (A14)",
        bytes.len()
    ));
    report.extend(labels);
    write(
        &review.join("report.txt"),
        (report.join("\n") + "\n").as_bytes(),
    )?;
    report.iter().for_each(|l| println!("{l}"));
    println!("wrote {} and {}", game.display(), review.display());
    Ok(())
}

/// The 164×200 player-menu picture (D8): idle pose from the front.
fn thumbnail(m: &mdl::Model) -> texture::Indexed {
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

fn info(m: &mdl::Model) -> Result<()> {
    println!("{}", m.name);
    println!(
        "  bones {}, controllers {}, hitboxes {}, attachments {}",
        m.bones.len(),
        m.bone_controllers.len(),
        m.hitboxes.len(),
        m.attachments.len()
    );
    println!(
        "  sequences {}, triangles {}",
        m.sequences.len(),
        m.tri_count()
    );
    for b in &m.bodyparts {
        for sm in &b.models {
            println!(
                "  bodypart {:?} model {:?}: {} verts, {} meshes",
                b.name,
                sm.name,
                sm.verts.len(),
                sm.meshes.len()
            );
        }
    }
    for t in &m.textures {
        println!(
            "  texture {:?} {}x{} flags {:#x}",
            t.name, t.width, t.height, t.flags
        );
    }
    Ok(())
}

/// A10: read, write with our writer, read again, and require an identical model.
fn roundtrip(input: &str, output: &str) -> Result<()> {
    let original = load(input)?;
    let bytes = mdl::write(&original)?;
    std::fs::write(output, &bytes).with_context(|| format!("writing {output}"))?;
    let again = mdl::read(&bytes).context("re-reading our own output")?;
    if again != original {
        bail!("round trip changed the model");
    }
    println!(
        "round trip OK: {} bytes in, {} bytes out",
        std::fs::metadata(input)?.len(),
        bytes.len()
    );
    Ok(())
}
