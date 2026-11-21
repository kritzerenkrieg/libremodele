use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use libremodele::{mdl, pipeline, preview};

const USAGE: &str = "usage:
  libremodele convert <download.zip | download.rar | folder | avatar.vrm | avatar.fbx> [--sway]
      optional overrides (A15): [--donor <donor.mdl> (G22)] [--name <name>] [--avatar <file in input>] [--package <x.unitypackage>]
      [--licence <text>] [--variant <prefab>] [--material <fbx material>=<.mat or image>]...
      [--height <units>] [--max-tris <n>] [--keep-polygons (G26)] [--manifest <clean_manifest.json>] [--out <dir>]
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
    pipeline::read_mdl(Path::new(path))
}

fn flag<'a>(rest: &[&'a str], name: &str) -> Option<&'a str> {
    rest.iter()
        .position(|a| *a == name)
        .and_then(|i| rest.get(i + 1))
        .copied()
}

fn convert_cmd(input: &str, rest: &[&str]) -> Result<()> {
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
    // A15, G16: everything is chosen from the input; flags only override.
    let req = pipeline::Request {
        input: PathBuf::from(input),
        name: flag(rest, "--name").map(str::to_string),
        donor: flag(rest, "--donor").map(PathBuf::from),
        avatar: flag(rest, "--avatar").map(str::to_string),
        package: flag(rest, "--package").map(str::to_string),
        licence: flag(rest, "--licence").map(str::to_string),
        variant: flag(rest, "--variant").map(str::to_string),
        materials: rest
            .iter()
            .zip(rest.iter().skip(1))
            .filter(|(a, _)| **a == "--material")
            .map(|(_, v)| {
                v.split_once('=')
                    .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
                    .with_context(|| {
                        format!("--material wants \"<FBX material>=<.mat or image>\", got {v:?}")
                    })
            })
            .collect::<Result<_>>()?,
        height,
        max_tris,
        keep_polygons: rest.contains(&"--keep-polygons"),
        sway: rest.contains(&"--sway"),
        keep,
    };
    let insp = pipeline::inspect(&req)?;
    let mut built = pipeline::build(&insp, &req, &mut |_| {})?;
    let (game, review) = pipeline::save(&mut built, &out)?;
    built.report.iter().for_each(|l| println!("{l}"));
    println!("wrote {} and {}", game.display(), review.display());
    Ok(())
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
