use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use libremodele::mdl;

const USAGE: &str = "usage:
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
        ["mdl-info", path] => info(&load(path)?),
        ["mdl-roundtrip", input, output] => roundtrip(input, output),
        _ => bail!("{USAGE}"),
    }
}

fn load(path: &str) -> Result<mdl::Model> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {path}"))?;
    mdl::read(&bytes).with_context(|| format!("parsing {path}"))
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
