//! The GUI's steps and state, kept free of Tauri so tests can drive them:
//! inspect a download, convert it, install it (H2, H3, H5).

use std::path::{Path, PathBuf};

use libremodele::{convert, donor, mdl, pipeline};
use serde::{Deserialize, Serialize};

/// The window's form: the input plus the overrides (H3). Empty text is unset.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Form {
    pub input: String,
    pub name: Option<String>,
    pub donor: Option<String>,
    pub avatar: Option<String>,
    pub package: Option<String>,
    pub licence: Option<String>,
    pub variant: Option<String>,
    /// `[FBX material, .mat or image]` pairs (G12).
    pub materials: Vec<(String, String)>,
    pub height: Option<f32>,
    pub max_tris: Option<usize>,
    pub keep_polygons: bool,
    pub sway: bool,
}

impl Form {
    fn request(&self) -> pipeline::Request {
        let opt = |s: &Option<String>| {
            s.as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        };
        pipeline::Request {
            input: PathBuf::from(self.input.trim()),
            name: opt(&self.name),
            donor: opt(&self.donor).map(PathBuf::from),
            avatar: opt(&self.avatar),
            package: opt(&self.package),
            licence: opt(&self.licence),
            variant: opt(&self.variant),
            materials: self
                .materials
                .iter()
                .filter(|(k, v)| !k.trim().is_empty() && !v.trim().is_empty())
                .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
                .collect(),
            height: self.height,
            max_tris: self.max_tris,
            keep_polygons: self.keep_polygons,
            sway: self.sway,
            keep: Default::default(),
        }
    }

    /// The fields that change what [`pipeline::inspect`] chooses.
    fn choices(&self) -> Form {
        Form {
            height: None,
            max_tris: None,
            keep_polygons: false,
            sway: false,
            ..self.clone()
        }
    }
}

/// What the app chose for a download (H2).
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub avatar_file: String,
    pub title: String,
    pub licence: String,
    /// The licence doesn't clearly allow redistribution (A13).
    pub restricted: bool,
    pub name: String,
    pub donor: String,
    /// Why this donor (G22); absent when the user chose it.
    pub donor_why: Option<String>,
    pub height_m: f32,
    pub parts: usize,
    /// Reader notes and warnings.
    pub notes: Vec<String>,
    /// Sven Co-op's addon player-model folder, when installed (H5).
    pub sven_addon: Option<String>,
}

/// A finished conversion.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub name: String,
    pub report: Vec<String>,
    pub game: String,
    pub review: String,
    pub triangles: usize,
    pub bytes: usize,
    pub restricted: bool,
}

struct Done {
    game: PathBuf,
    restricted: bool,
    /// Kept for the 3D viewer (H4).
    model: mdl::Model,
}

/// One avatar at a time (H2): the last inspection and conversion.
#[derive(Default)]
pub struct Session {
    inspected: Option<(Form, pipeline::Inspected)>,
    done: Option<Done>,
    /// A Sven weapon model held in the viewer (H4).
    weapon: Option<mdl::Model>,
    /// Bumped whenever the model (`[0]`) or the weapon (`[1]`) changes, so
    /// the viewer can't mix one model's geometry with another's frames or
    /// textures.
    generation: [u64; 2],
}

fn text(e: impl std::fmt::Display) -> String {
    format!("{e:#}")
}

impl Session {
    /// Open the download and make every choice (A15); cached for [`Self::convert`].
    pub fn inspect(&mut self, f: &Form) -> Result<Summary, String> {
        self.done = None;
        let insp = pipeline::inspect(&f.request()).map_err(text)?;
        let av = &insp.chosen.avatar;
        let s = Summary {
            avatar_file: insp.chosen.path.clone(),
            title: av.title.clone(),
            licence: av.licence.clone(),
            restricted: convert::licence_restricted(&av.licence),
            name: insp.name.clone(),
            donor: insp.donor.display().to_string(),
            donor_why: insp.donor_why.clone(),
            height_m: donor::height(av),
            parts: av.parts.len(),
            notes: av.notes.clone(),
            sven_addon: donor::addon_players().map(|p| p.display().to_string()),
        };
        self.inspected = Some((f.choices(), insp));
        Ok(s)
    }

    /// Convert into `out` (re-inspecting only when the choices changed),
    /// reporting each stage to `progress`.
    pub fn convert(
        &mut self,
        f: &Form,
        out: &Path,
        progress: &mut dyn FnMut(&str),
    ) -> Result<Outcome, String> {
        self.done = None;
        if self
            .inspected
            .as_ref()
            .is_none_or(|(c, _)| *c != f.choices())
        {
            progress("reading the download");
            self.inspect(f)?;
        }
        let (_, insp) = self.inspected.as_ref().expect("inspected above");
        let req = f.request();
        let mut built = pipeline::build(insp, &req, progress).map_err(text)?;
        progress("saving");
        let (game, review) = pipeline::save(&mut built, out).map_err(text)?;
        let restricted = convert::licence_restricted(&insp.chosen.avatar.licence);
        let o = Outcome {
            name: built.name.clone(),
            report: built.report.clone(),
            game: game.display().to_string(),
            review: review.display().to_string(),
            triangles: built.model.tri_count(),
            bytes: built.bytes.len(),
            restricted,
        };
        self.generation[0] += 1;
        self.done = Some(Done {
            game,
            restricted,
            model: built.model,
        });
        Ok(o)
    }

    /// The current viewer generation of the model or the weapon (see
    /// [`Self::model_at`]).
    pub fn generation(&self, weapon: bool) -> u64 {
        self.generation[weapon as usize]
    }

    /// As [`Self::model`], refused when the model changed since `generation`.
    pub fn model_at(&self, weapon: bool, generation: u64) -> Result<&mdl::Model, String> {
        if generation != self.generation[weapon as usize] {
            return Err("the model changed; reloading".into());
        }
        self.model(weapon)
    }

    /// The converted model, or the loaded weapon, for the viewer (H4).
    pub fn model(&self, weapon: bool) -> Result<&mdl::Model, String> {
        if weapon {
            self.weapon
                .as_ref()
                .ok_or_else(|| "no weapon loaded".into())
        } else {
            self.done
                .as_ref()
                .map(|d| &d.model)
                .ok_or_else(|| "convert an avatar first".into())
        }
    }

    /// Load one of Sven's `p_*.mdl` weapon models (or none) into the viewer.
    pub fn load_weapon(&mut self, file: Option<&str>) -> Result<(), String> {
        self.weapon = None;
        self.generation[1] += 1;
        let Some(file) = file else { return Ok(()) };
        if file.contains(['/', '\\']) || !file.ends_with(".mdl") {
            return Err(format!("not a weapon model: {file}"));
        }
        let root = donor::sven_root().ok_or("Sven Co-op wasn't found")?;
        let m = pipeline::read_mdl(&root.join("svencoop/models").join(file)).map_err(text)?;
        // The viewer chains bones parent-first; any other order is malformed.
        if let Some((i, b)) = m
            .bones
            .iter()
            .enumerate()
            .find(|(i, b)| b.parent >= *i as i32)
        {
            return Err(format!(
                "{file}: bone {i} ({}) has parent {}, not an earlier bone",
                b.name, b.parent
            ));
        }
        self.weapon = Some(m);
        Ok(())
    }

    /// Copy the last conversion into `addon` (H2). A model whose licence
    /// doesn't clearly allow redistribution needs `keep_local` ticked (H5).
    pub fn install(&self, addon: &Path, keep_local: bool) -> Result<PathBuf, String> {
        let d = self.done.as_ref().ok_or("convert an avatar first")?;
        if d.restricted && !keep_local {
            return Err("this licence doesn't clearly allow redistribution: tick \
                 \"I'll keep this model local\" to install (A6, A13, H5)"
                .into());
        }
        pipeline::install(&d.game, addon).map_err(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Inspect → convert → install into a temporary addon folder (Phase H
    /// done-check). Skips without the git-ignored samples (A6).
    #[test]
    fn inspect_convert_install() {
        let input = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/neru");
        if !input.is_dir() {
            eprintln!("skipping: samples/neru missing");
            return;
        }
        let tmp = std::env::temp_dir().join(format!("libremodele-gui-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let form = Form {
            input: input.display().to_string(),
            name: Some("neru_gui".into()),
            ..Default::default()
        };
        let mut s = Session::default();
        assert!(s.install(&tmp, true).is_err(), "nothing converted yet");

        let sum = s.inspect(&form).unwrap();
        assert_eq!(sum.name, "neru_gui");
        assert!(sum.donor_why.as_deref().unwrap().starts_with("chibi"));

        let mut stages = Vec::new();
        let o = s
            .convert(&form, &tmp.join("out"), &mut |st| {
                stages.push(st.to_string())
            })
            .unwrap();
        assert_eq!(
            stages.first().map(String::as_str),
            Some("reading the donor")
        );
        assert!(stages.iter().any(|s| s == "saving"));
        assert!(o.triangles > 0 && o.bytes > 0);

        let addon = tmp.join("addon/models/player");
        if o.restricted {
            assert!(s.install(&addon, false).is_err(), "H5 tick-box");
        }
        let target = s.install(&addon, true).unwrap();
        let mdl = std::fs::read(target.join("neru_gui.mdl")).unwrap();
        let built = std::fs::read(Path::new(&o.game).join("neru_gui.mdl")).unwrap();
        assert_eq!(mdl, built);
        assert!(target.join("neru_gui.bmp").is_file());

        // Changing only a conversion option reuses the inspection.
        let mut stages = Vec::new();
        let full = Form {
            keep_polygons: true,
            ..form
        };
        s.convert(&full, &tmp.join("out"), &mut |st| {
            stages.push(st.to_string())
        })
        .unwrap();
        assert!(!stages.iter().any(|s| s == "reading the download"));
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
