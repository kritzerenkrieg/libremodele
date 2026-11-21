//! Automatic material pairing (G14, A15): an FBX material with no texture
//! gets the texture whose painted texels best cover the part's UV triangles,
//! with name similarity as a tie-break. Nobody picks a texture by eye.

use crate::vrm::Part;

/// Coverage grids are this many cells on a side.
const GRID: usize = 64;
/// Side of the copy that seams are sampled from.
const SMALL: u32 = 256;

/// A texture the download offers, reduced to what pairing needs.
pub struct Candidate {
    /// Where it came from (`.mat` name or file path), for the report.
    pub name: String,
    /// Texture file name, for `find_image`.
    pub file: String,
    pub png: Vec<u8>,
    /// From a `.mat`: its cutout flag; from a bare image: `None`.
    pub masked: Option<bool>,
    /// Painted (mostly opaque) cells. Opaque counts as paint: a flat
    /// border colour is often the hair or skin itself (Blindy's `hairdo.png`).
    painted: Vec<bool>,
    /// Cells with transparent texels (alpha < 128).
    clear: Vec<bool>,
    /// A small copy for colour sampling.
    small: image::RgbaImage,
}

/// File names that are not colour textures (normal maps, masks, UV guides).
fn not_colour(name: &str) -> bool {
    let n = name.to_lowercase();
    let toks: Vec<&str> = n
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .collect();
    [
        "normal",
        "normals",
        "nrm",
        "mask",
        "eyemask",
        "ao",
        "uv",
        "uvs",
        "emission",
        "emissive",
        "matcap",
        "cubemap",
        "datatex",
        "rough",
        "roughness",
        "metal",
        "metallic",
        "spec",
        "specular",
        "height",
        "outline",
        "noise",
    ]
    .iter()
    .any(|w| {
        toks.iter()
            .any(|t| t == w || (t.len() > w.len() + 2 && t.ends_with(w)))
    })
}

impl Candidate {
    /// `None` when the bytes aren't a PNG or the name says it isn't colour.
    pub fn new(name: String, file: String, png: Vec<u8>, masked: Option<bool>) -> Option<Self> {
        if !crate::texture::is_image(&png) || not_colour(&file) {
            return None;
        }
        let img = image::load_from_memory(&png).ok()?.to_rgba8();
        let (w, h) = img.dimensions();
        if w < 16 || h < 16 {
            return None;
        }
        // An image smaller than the grid is stretched to it (nearest), so
        // every cell gets texels.
        let img = if (w as usize) < GRID || (h as usize) < GRID {
            let (gw, gh) = (w.max(GRID as u32), h.max(GRID as u32));
            image::imageops::resize(&img, gw, gh, image::imageops::FilterType::Nearest)
        } else {
            img
        };
        let (w, h) = img.dimensions();
        let mut painted = vec![0u32; GRID * GRID];
        let mut clear = vec![0u32; GRID * GRID];
        let mut count = vec![0u32; GRID * GRID];
        for (x, y, p) in img.enumerate_pixels() {
            let cell = (y as usize * GRID / h as usize) * GRID + x as usize * GRID / w as usize;
            count[cell] += 1;
            if p.0[3] < 128 {
                clear[cell] += 1;
            } else {
                painted[cell] += 1;
            }
        }
        Some(Candidate {
            name,
            file,
            png,
            masked,
            painted: (0..GRID * GRID)
                .map(|i| painted[i] * 2 > count[i])
                .collect(),
            clear: (0..GRID * GRID).map(|i| clear[i] * 20 > count[i]).collect(),
            small: image::imageops::resize(
                &img,
                SMALL,
                SMALL,
                image::imageops::FilterType::Triangle,
            ),
        })
    }
}

/// Cells the part's UV triangles touch (UVs wrap; top-left origin).
/// Grid cells under the part's UVs, or `None` when a triangle spans more
/// texture repeats than the scan covers, so its coverage can't be judged.
fn uv_cells(part: &Part) -> Option<Vec<bool>> {
    let mut on = vec![false; GRID * GRID];
    let g = GRID as f32;
    for t in &part.triangles {
        let uv = t.map(|i| part.uvs[i as usize]);
        // Wrap the triangle as a whole by its first corner's tile.
        let (fu, fv) = (uv[0][0].floor(), uv[0][1].floor());
        let p = uv.map(|[u, v]| [(u - fu) * g, (v - fv) * g]);
        let (x0, x1) = (
            p.iter().map(|q| q[0]).fold(f32::MAX, f32::min),
            p.iter().map(|q| q[0]).fold(f32::MIN, f32::max),
        );
        let (y0, y1) = (
            p.iter().map(|q| q[1]).fold(f32::MAX, f32::min),
            p.iter().map(|q| q[1]).fold(f32::MIN, f32::max),
        );
        let area =
            (p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[2][0] - p[0][0]) * (p[1][1] - p[0][1]);
        // Scanning is capped at two texture repeats per axis; a triangle
        // wider than that would be clipped, and a clipped mask would claim
        // less coverage than the triangle has.
        let span = 2 * GRID as i64;
        let (cx0, cy0) = (x0.floor() as i64, y0.floor() as i64);
        let (cx1, cy1) = (x1.floor() as i64, y1.floor() as i64);
        // Extreme coordinates saturate the casts; checked subtraction keeps
        // them from wrapping into a "small" span.
        let fits = |a: i64, b: i64| b.checked_sub(a).is_some_and(|d| d <= span);
        if !(fits(cx0, cx1) && fits(cy0, cy1)) {
            return None;
        }
        for cy in cy0..=cy1 {
            for cx in cx0..=cx1 {
                let (c, r) = (
                    cx.rem_euclid(GRID as i64) as usize,
                    cy.rem_euclid(GRID as i64) as usize,
                );
                // Cell centre inside the triangle, or the triangle smaller
                // than a cell (it still marks the cell it sits in).
                let q = [cx as f32 + 0.5, cy as f32 + 0.5];
                let tiny = (x1 - x0) < 1.0 && (y1 - y0) < 1.0;
                if tiny || inside(&p, q, area) {
                    on[r * GRID + c] = true;
                }
            }
        }
    }
    Some(on)
}

fn inside(p: &[[f32; 2]; 3], q: [f32; 2], area: f32) -> bool {
    if area.abs() < 1e-9 {
        return false;
    }
    let e =
        |a: [f32; 2], b: [f32; 2]| (b[0] - a[0]) * (q[1] - a[1]) - (q[0] - a[0]) * (b[1] - a[1]);
    let s = area.signum();
    e(p[0], p[1]) * s >= 0.0 && e(p[1], p[2]) * s >= 0.0 && e(p[2], p[0]) * s >= 0.0
}

/// Name similarity in 0..=1: shared word tokens (`body_blindy` ↔ `Blindy_Body`).
pub fn name_similarity(a: &str, b: &str) -> f32 {
    let toks = |s: &str| -> Vec<String> {
        let s = s
            .rsplit_once('.')
            .filter(|(_, e)| e.chars().all(|c| c.is_ascii_alphabetic()) && e.len() <= 4)
            .map_or(s, |(s, _)| s);
        s.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|t| t.len() > 1 && !t.chars().all(|c| c.is_ascii_digit()))
            .map(str::to_string)
            .collect()
    };
    let (x, y) = (toks(a), toks(b));
    if x.is_empty() || y.is_empty() {
        return 0.0;
    }
    let hit = |t: &String, o: &[String]| {
        o.iter().any(|u| {
            u == t || (t.len() >= 4 && (u.starts_with(t.as_str()) || t.starts_with(u.as_str())))
        })
    };
    let n = x.iter().filter(|t| hit(t, &y)).count() + y.iter().filter(|t| hit(t, &x)).count();
    n as f32 / (x.len() + y.len()) as f32
}

/// One candidate's fit to a part.
#[derive(Clone, Copy, Debug)]
pub struct Fit {
    /// Share of the part's UV cells on painted texels.
    pub precision: f32,
    /// F1 of UV cells against painted cells.
    pub f1: f32,
    pub name: f32,
    /// Share of the part's UV cells with transparent texels.
    pub clear: f32,
    /// Colour agreement across UV seams, 1 = seamless, 0 = like random
    /// texel pairs; `None` without seams.
    pub seam: Option<f32>,
    /// The candidate is an authored `.mat`.
    pub mat: bool,
    /// Mean texture colour (sRGB, 0..1) under the part's triangles.
    pub mean: [f32; 3],
    /// The candidate is marked alternate while the avatar isn't.
    pub alt: bool,
}

/// Weights: seam continuity leads; it is clamped at 0 because stylised
/// textures (hair strands) put colour changes on seams on purpose, which
/// makes it say nothing rather than "wrong".
const NAME_WEIGHT: f32 = 0.4;
const MAT_BONUS: f32 = 0.1;
const ALT_PENALTY: f32 = 0.15;

impl Fit {
    pub fn score(&self) -> f32 {
        self.seam.unwrap_or(0.0).max(0.0)
            + NAME_WEIGHT * self.name
            + if self.mat { MAT_BONUS } else { 0.0 }
            - if self.alt { ALT_PENALTY } else { 0.0 }
    }
}

fn is_alt(name: &str) -> bool {
    name.to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|t| t == "alt" || t.starts_with("alternate") || (t.len() > 3 && t.ends_with("alt")))
}

fn stem(file: &str) -> &str {
    let b = file.rsplit('/').next().unwrap_or(file);
    b.rsplit_once('.').map_or(b, |(s, _)| s)
}

/// `context` is the avatar file's path (for the alternate check).
pub fn fit(part: &Part, material: &str, context: &str, c: &Candidate) -> Option<Fit> {
    fit_shape(&Shape::of(part), material, context, c)
}

/// What scoring needs from a part, independent of the candidate: built once
/// per part, then reused for every candidate (#11).
pub struct Shape<'p> {
    part: &'p Part,
    /// `None`: the coverage can't be judged (see [`uv_cells`]).
    uv: Option<Vec<bool>>,
    n_uv: usize,
    seams: Vec<[[f32; 2]; 4]>,
}

impl<'p> Shape<'p> {
    pub fn of(part: &'p Part) -> Self {
        let uv = uv_cells(part);
        let n_uv = uv.iter().flatten().filter(|&&b| b).count();
        Shape {
            part,
            uv,
            n_uv,
            seams: seams(part),
        }
    }
}

fn fit_shape(sh: &Shape, material: &str, context: &str, c: &Candidate) -> Option<Fit> {
    let (part, n_uv) = (sh.part, sh.n_uv);
    let uv = sh.uv.as_ref()?;
    if n_uv == 0 {
        return None;
    }
    let n_p = c.painted.iter().filter(|&&b| b).count();
    let both = (0..uv.len()).filter(|&i| uv[i] && c.painted[i]).count();
    let clear = (0..uv.len()).filter(|&i| uv[i] && c.clear[i]).count();
    Some(Fit {
        precision: both as f32 / n_uv as f32,
        f1: 2.0 * both as f32 / (n_uv + n_p).max(1) as f32,
        name: name_similarity(material, stem(&c.file)).max(if c.masked.is_some() {
            name_similarity(material, &c.name)
        } else {
            0.0
        }),
        clear: clear as f32 / n_uv as f32,
        seam: seam_fit(&sh.seams, c),
        mat: c.masked.is_some(),
        mean: mean_colour(part, c),
        alt: (is_alt(stem(&c.file)) || (c.masked.is_some() && is_alt(&c.name)))
            && !is_alt(material)
            && !is_alt(stem(context)),
    })
}

/// Mean colour under the part's triangle centres (at most ~2000 sampled).
fn mean_colour(part: &Part, c: &Candidate) -> [f32; 3] {
    let step = (part.triangles.len() / 2000).max(1);
    let (mut sum, mut n) = ([0f32; 3], 0f32);
    for t in part.triangles.iter().step_by(step) {
        let uv = t.map(|i| part.uvs[i as usize]);
        let (u, v) = (
            (uv[0][0] + uv[1][0] + uv[2][0]) / 3.0,
            (uv[0][1] + uv[1][1] + uv[2][1]) / 3.0,
        );
        let x = ((u.rem_euclid(1.0) * SMALL as f32) as u32).min(SMALL - 1);
        let y = ((v.rem_euclid(1.0) * SMALL as f32) as u32).min(SMALL - 1);
        let p = c.small.get_pixel(x, y).0;
        for k in 0..3 {
            sum[k] += p[k] as f32 / 255.0;
        }
        n += 1.0;
    }
    sum.map(|x| x / n.max(1.0))
}

/// One side of a mesh edge in UV space.
type UvEdge = ([f32; 2], [f32; 2]);

/// Mesh edges cut into two UV islands: both UV copies of each, (a0, b0, a1, b1).
fn seams(part: &Part) -> Vec<[[f32; 2]; 4]> {
    use std::collections::HashMap;
    let key = |i: u32| part.positions[i as usize].map(|x| (x * 1e4).round() as i64);
    let mut edges: HashMap<([i64; 3], [i64; 3]), Vec<UvEdge>> = HashMap::new();
    for t in &part.triangles {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            let (ka, kb) = (key(a), key(b));
            let (ua, ub) = (part.uvs[a as usize], part.uvs[b as usize]);
            let (k, v) = if ka <= kb {
                ((ka, kb), (ua, ub))
            } else {
                ((kb, ka), (ub, ua))
            };
            if ka != kb {
                edges.entry(k).or_default().push(v);
            }
        }
    }
    let far = |p: [f32; 2], q: [f32; 2]| (p[0] - q[0]).abs() + (p[1] - q[1]).abs() > 1e-4;
    // Sorted by edge, so the comparison pairs (and the score) are the same
    // every run.
    let mut e: Vec<_> = edges.into_iter().collect();
    e.sort_by_key(|(k, _)| *k);
    e.into_iter()
        .map(|(_, v)| v)
        .filter(|v| v.len() == 2 && (far(v[0].0, v[1].0) || far(v[0].1, v[1].1)))
        .map(|v| [v[0].0, v[0].1, v[1].0, v[1].1])
        .collect()
}

fn seam_fit(s: &[[[f32; 2]; 4]], c: &Candidate) -> Option<f32> {
    if s.len() < 8 {
        return None;
    }
    let at = |[u, v]: [f32; 2]| {
        let w = SMALL as f32;
        let x = ((u.rem_euclid(1.0) * w) as u32).min(SMALL - 1);
        let y = ((v.rem_euclid(1.0) * w) as u32).min(SMALL - 1);
        c.small.get_pixel(x, y).0
    };
    let diff = |p: [u8; 4], q: [u8; 4]| -> f32 {
        (0..3)
            .map(|i| (p[i] as f32 - q[i] as f32).abs())
            .sum::<f32>()
    };
    let lerp =
        |a: [f32; 2], b: [f32; 2], t: f32| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
    let (mut across, mut random, mut n) = (0.0f32, 0.0f32, 0usize);
    for (i, e) in s.iter().enumerate() {
        // A pairing to compare against: another seam's far side.
        let o = &s[(i * 7919 + 13) % s.len()];
        for t in [0.25, 0.5, 0.75] {
            let p = at(lerp(e[0], e[1], t));
            across += diff(p, at(lerp(e[2], e[3], t)));
            random += diff(p, at(lerp(o[2], o[3], 1.0 - t)));
            n += 1;
        }
    }
    if n == 0 || random <= 1e-3 {
        return None;
    }
    Some(1.0 - across / random)
}

/// The pick needs most of the part's UVs on paint ...
pub const MIN_PRECISION: f32 = 0.8;
/// ... a score this high (a clean seam fit, or a strong name match on an
/// authored `.mat`) ...
pub const MIN_SCORE: f32 = 0.25;
/// Without a name match (under `WEAK_NAME`), the seams alone must agree
/// this well: a mostly flat texture makes weak seam scores cheap.
pub const WEAK_NAME: f32 = 0.3;
pub const MIN_SEAM_ALONE: f32 = 0.5;
/// ... and to beat the runner-up by this much.
pub const MIN_MARGIN: f32 = 0.05;

/// Best candidate for `part`, with the report line; `Err` is the warning.
pub fn choose<'c>(
    part: &Part,
    material: &str,
    context: &str,
    cands: &'c [Candidate],
) -> Result<(&'c Candidate, Fit, String), String> {
    let shape = Shape::of(part);
    if shape.uv.is_none() {
        return Err(
            "its UVs stretch over more than two texture repeats, so their coverage can't be checked"
                .into(),
        );
    }
    let mut fits: Vec<(usize, Fit)> = cands
        .iter()
        .enumerate()
        .filter_map(|(i, c)| fit_shape(&shape, material, context, c).map(|f| (i, f)))
        .filter(|(_, f)| f.precision >= MIN_PRECISION)
        .collect();
    fits.sort_by(|a, b| b.1.score().total_cmp(&a.1.score()).then(a.0.cmp(&b.0)));
    let Some(&(best, f)) = fits.first() else {
        return Err(format!(
            "no texture is opaque over {:.0}% of its UVs",
            MIN_PRECISION * 100.0
        ));
    };
    let describe = |i: usize, g: &Fit| {
        format!(
            "{} (score {:.2}: seam {}, name {:.2}{}{})",
            cands[i].name,
            g.score(),
            g.seam.map_or("none".into(), |x| format!("{x:.2}")),
            g.name,
            if g.mat { ", .mat" } else { "" },
            if g.alt { ", alternate" } else { "" }
        )
    };
    if f.score() < MIN_SCORE {
        return Err(format!("best is {}, under {MIN_SCORE}", describe(best, &f)));
    }
    if f.name < WEAK_NAME && f.seam.unwrap_or(0.0) < MIN_SEAM_ALONE {
        return Err(format!(
            "best is {}: without a name match the seams must agree at {MIN_SEAM_ALONE}",
            describe(best, &f)
        ));
    }
    // The runner-up is the best *other outcome*: the same pixels under another
    // `.mat` name differ only by name, which the score already weighs; the
    // same pixels with the other cutout setting are a different result.
    let outcome = |i: usize| (digest(&cands[i].png), cands[i].masked.unwrap_or(false));
    let same = outcome(best);
    let runner = fits.iter().skip(1).find(|(i, _)| outcome(*i) != same);
    if let Some((i, g)) = runner
        && f.score() - g.score() < MIN_MARGIN
    {
        return Err(format!(
            "no clear winner: {} vs {}",
            describe(best, &f),
            describe(*i, g)
        ));
    }
    let line = format!(
        "{}{}",
        describe(best, &f),
        runner.map_or(String::new(), |(i, g)| format!(
            "; next {}",
            describe(*i, g)
        ))
    );
    Ok((&cands[best], f, line))
}

/// Every colour texture on offer: package `.mat` files (which carry the
/// cutout flag), then package and download images; one per distinct image.
pub fn candidates(
    package: Option<&crate::unitypkg::Package>,
    files: Option<&crate::source::Files>,
) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
    // A `.mat` is kept per material even when another shares its image (its
    // name and cutout flag differ); a bare image only once, and not when a
    // `.mat` already offers it.
    let mut images = std::collections::HashSet::new();
    let mut mats = std::collections::HashSet::new();
    let mut add = |c: Option<Candidate>, out: &mut Vec<Candidate>| {
        if let Some(c) = c {
            let d = digest(&c.png);
            let new = match c.masked {
                Some(m) => mats.insert((d, c.name.clone(), m)),
                None => !images.contains(&d),
            };
            if new {
                images.insert(d);
                out.push(c);
            }
        }
    };
    if let Some(p) = package {
        for m in crate::unitypkg::materials(p) {
            if let Some((file, data)) = m.texture {
                add(
                    Candidate::new(m.name.clone(), file, data, Some(m.masked)),
                    &mut out,
                );
            }
        }
        for a in &p.assets {
            if let Some(d) = &a.data
                && a.path.to_lowercase().ends_with(".png")
            {
                let file = a.path.rsplit('/').next().unwrap_or(&a.path).to_string();
                add(
                    Candidate::new(a.path.clone(), file, d.clone(), None),
                    &mut out,
                );
            }
        }
    }
    if let Some(f) = files {
        for (path, d) in &f.entries {
            if path.to_lowercase().ends_with(".png") {
                let file = path.rsplit('/').next().unwrap_or(path).to_string();
                add(
                    Candidate::new(path.clone(), file, d.clone(), None),
                    &mut out,
                );
            }
        }
    }
    out
}

fn digest(d: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    d.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tri(uvs: [[f32; 2]; 3]) -> Part {
        Part {
            id: String::new(),
            node_name: String::new(),
            material: String::new(),
            positions: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            normals: vec![[0.0, 0.0, 1.0]; 3],
            uvs: uvs.to_vec(),
            dominant: vec![0; 3],
            weights: vec![vec![(0, 1.0)]; 3],
            triangles: vec![[0, 1, 2]],
            image: None,
            unlit: false,
            masked: false,
            first_person_only: false,
        }
    }

    #[test]
    fn coverage_is_refused_when_the_scan_would_clip() {
        // Within two repeats: judged.
        assert!(uv_cells(&tri([[0.0, 0.0], [0.5, 0.0], [0.0, 0.5]])).is_some());
        // Four repeats wide and thin: the rows past the cap would be lost.
        assert!(uv_cells(&tri([[0.0, 0.0], [4.0, 0.0], [4.0, 1.0 / 16.0]])).is_none());
        // Coordinates that saturate i64 don't wrap into an accepted span.
        assert!(uv_cells(&tri([[0.0, 0.0], [-1e30, 0.0], [1e30, 1.0]])).is_none());
    }
}
