//! Binary FBX 7.x input (B9, phase G): the node tree, skinned meshes in their
//! bind pose, materials and texture file names, read into the same
//! [`Avatar`] the VRM reader gives. Settings that live in a Unity package
//! (materials, humanoid map, PhysBones) are merged in by [`load`] (G2, G5, G10).
//! Shape keys are left at 0, the neutral face (B7).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read;

use anyhow::{Context, Result, anyhow, bail};

/// Largest decoded FBX array (bytes): inputs are capped at 200 MB (G16), and a
/// 65k-triangle avatar's biggest array is a few MB.
const MAX_ARRAY_BYTES: usize = 512 << 20;
/// Largest total of decoded arrays kept for one file (bytes, as stored:
/// every element widens to 8 bytes), so many large arrays can't exhaust
/// memory either.
const MAX_DECODED_BYTES: usize = 1 << 30;

use crate::math::{M3, V3, Xf, cross, dot, len, norm, sub};
use crate::unitypkg::{self, Package};
use crate::vrm::{Avatar, Node, Part, SpringGroup};

// ---------------------------------------------------------------- parser

#[derive(Clone, Debug, PartialEq)]
pub enum Prop {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Raw(Vec<u8>),
    Ints(Vec<i64>),
    Floats(Vec<f64>),
}

#[derive(Clone, Debug)]
pub struct FNode {
    pub name: String,
    pub props: Vec<Prop>,
    pub children: Vec<FNode>,
}

impl FNode {
    pub fn child(&self, name: &str) -> Option<&FNode> {
        self.children.iter().find(|c| c.name == name)
    }
    fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a FNode> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }
    fn int(&self, i: usize) -> Option<i64> {
        match self.props.get(i)? {
            Prop::Int(v) => Some(*v),
            _ => None,
        }
    }
    fn str(&self, i: usize) -> Option<&str> {
        match self.props.get(i)? {
            Prop::Str(s) => Some(s),
            _ => None,
        }
    }
    fn floats(&self) -> Option<&[f64]> {
        match self.props.first()? {
            Prop::Floats(v) => Some(v),
            _ => None,
        }
    }
    fn ints(&self) -> Option<&[i64]> {
        match self.props.first()? {
            Prop::Ints(v) => Some(v),
            _ => None,
        }
    }
    /// `Properties70` entry `P: name, type, label, flags, values...`.
    fn p70(&self, name: &str) -> Option<&[Prop]> {
        self.child("Properties70")?
            .all("P")
            .find(|p| p.str(0) == Some(name))
            .map(|p| &p.props[4.min(p.props.len())..])
    }
    fn p70_v3(&self, name: &str) -> Option<[f64; 3]> {
        let v = self.p70(name)?;
        let f = |i: usize| match v.get(i) {
            Some(Prop::Float(x)) => Some(*x),
            Some(Prop::Int(x)) => Some(*x as f64),
            _ => None,
        };
        Some([f(0)?, f(1)?, f(2)?])
    }
    fn p70_num(&self, name: &str) -> Option<f64> {
        match self.p70(name)?.first()? {
            Prop::Float(x) => Some(*x),
            Prop::Int(x) => Some(*x as f64),
            _ => None,
        }
    }
}

/// Top-level nodes of a binary FBX file.
pub fn parse(d: &[u8]) -> Result<Vec<FNode>> {
    if d.len() < 27 || &d[..21] != b"Kaydara FBX Binary  \0" {
        bail!("not a binary FBX file (ASCII FBX is not supported)");
    }
    let ver = u32::from_le_bytes(d[23..27].try_into().unwrap());
    if !(7000..8000).contains(&ver) {
        bail!("FBX version {ver} is not supported (7.x only)");
    }
    let mut r = Reader {
        d,
        o: 27,
        wide: ver >= 7500,
        budget: MAX_DECODED_BYTES,
    };
    let mut top = Vec::new();
    while let Some(n) = r.node()? {
        top.push(n);
    }
    Ok(top)
}

struct Reader<'a> {
    d: &'a [u8],
    o: usize,
    wide: bool,
    /// Decoded-array bytes still allowed (see [`MAX_DECODED_BYTES`]).
    budget: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        let s = self
            .d
            .get(self.o..self.o + n)
            .ok_or_else(|| anyhow!("FBX truncated at byte {}", self.o))?;
        self.o += n;
        Ok(s)
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn word(&mut self) -> Result<u64> {
        if self.wide {
            Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
        } else {
            Ok(self.u32()? as u64)
        }
    }

    fn node(&mut self) -> Result<Option<FNode>> {
        let end = self.word()? as usize;
        let nprops = self.word()?;
        let _plen = self.word()?;
        let nlen = self.take(1)?[0] as usize;
        let name = String::from_utf8_lossy(self.take(nlen)?).into_owned();
        if end == 0 {
            return Ok(None);
        }
        if end > self.d.len() {
            bail!("FBX node `{name}` ends past the file");
        }
        // The end must not move the reader backwards (re-parsing the same bytes).
        if end < self.o {
            bail!("FBX node `{name}` ends before its own header");
        }
        // Every property takes at least one byte, so the count can't exceed them.
        if nprops > (end - self.o) as u64 {
            bail!(
                "FBX node `{name}` claims {nprops} properties in {} bytes",
                end - self.o
            );
        }
        let mut props = Vec::with_capacity(nprops as usize);
        for _ in 0..nprops {
            props.push(self.prop()?);
        }
        let mut children = Vec::new();
        while self.o < end {
            match self.node()? {
                Some(c) => children.push(c),
                None => break,
            }
        }
        self.o = end;
        Ok(Some(FNode {
            name,
            props,
            children,
        }))
    }

    fn prop(&mut self) -> Result<Prop> {
        let t = self.take(1)?[0];
        let le = |b: &[u8]| -> [u8; 8] {
            let mut a = [0; 8];
            a[..b.len()].copy_from_slice(b);
            a
        };
        Ok(match t {
            b'C' => Prop::Bool(self.take(1)?[0] != 0),
            b'Y' => Prop::Int(i16::from_le_bytes(self.take(2)?.try_into().unwrap()) as i64),
            b'I' => Prop::Int(i32::from_le_bytes(self.take(4)?.try_into().unwrap()) as i64),
            b'L' => Prop::Int(i64::from_le_bytes(le(self.take(8)?))),
            b'F' => Prop::Float(f32::from_le_bytes(self.take(4)?.try_into().unwrap()) as f64),
            b'D' => Prop::Float(f64::from_le_bytes(le(self.take(8)?))),
            b'S' | b'R' => {
                let n = self.u32()? as usize;
                let b = self.take(n)?.to_vec();
                if t == b'S' {
                    Prop::Str(String::from_utf8_lossy(&b).into_owned())
                } else {
                    Prop::Raw(b)
                }
            }
            b'f' | b'd' | b'l' | b'i' | b'b' => {
                let n = self.u32()? as usize;
                let enc = self.u32()?;
                let clen = self.u32()? as usize;
                let size = match t {
                    b'f' | b'i' => 4,
                    b'd' | b'l' => 8,
                    _ => 1,
                };
                let want = n
                    .checked_mul(size)
                    .filter(|&b| b <= MAX_ARRAY_BYTES)
                    .with_context(|| format!("FBX array of {n} elements is too large"))?;
                // Charged as stored (8 bytes per element) before decoding.
                self.budget = self
                    .budget
                    .checked_sub(n * 8)
                    .context("FBX arrays exceed the total decode limit")?;
                let raw = self.take(clen)?;
                let data = match enc {
                    0 => raw.to_vec(),
                    1 => {
                        // Inflate at most one byte past the declared size, so a
                        // bomb can't grow the buffer before the length check.
                        let mut out = Vec::with_capacity(want);
                        flate2::read::ZlibDecoder::new(raw)
                            .take(want as u64 + 1)
                            .read_to_end(&mut out)
                            .context("FBX array inflate")?;
                        out
                    }
                    e => bail!("FBX array encoding {e}"),
                };
                if data.len() != want {
                    bail!("FBX array has {} bytes, expected {want}", data.len());
                }
                let ch = data.chunks_exact(size);
                match t {
                    b'f' => Prop::Floats(
                        ch.map(|c| f32::from_le_bytes(c.try_into().unwrap()) as f64)
                            .collect(),
                    ),
                    b'd' => Prop::Floats(
                        ch.map(|c| f64::from_le_bytes(c.try_into().unwrap()))
                            .collect(),
                    ),
                    b'l' => Prop::Ints(
                        ch.map(|c| i64::from_le_bytes(c.try_into().unwrap()))
                            .collect(),
                    ),
                    b'i' => Prop::Ints(
                        ch.map(|c| i32::from_le_bytes(c.try_into().unwrap()) as i64)
                            .collect(),
                    ),
                    _ => Prop::Ints(ch.map(|c| c[0] as i64).collect()),
                }
            }
            t => bail!("unknown FBX property type {:?}", t as char),
        })
    }
}

// ---------------------------------------------------------------- 4×4 maths

/// Row-major 4×4, acting on column vectors.
type M4 = [[f64; 4]; 4];
const I4: M4 = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

fn mm(a: &M4, b: &M4) -> M4 {
    let mut m = [[0.0; 4]; 4];
    for (r, row) in m.iter_mut().enumerate() {
        for (c, v) in row.iter_mut().enumerate() {
            *v = (0..4).map(|k| a[r][k] * b[k][c]).sum();
        }
    }
    m
}
fn tr(t: [f64; 3]) -> M4 {
    let mut m = I4;
    (0..3).for_each(|i| m[i][3] = t[i]);
    m
}
fn sc(s: [f64; 3]) -> M4 {
    let mut m = I4;
    (0..3).for_each(|i| m[i][i] = s[i]);
    m
}
fn neg(v: [f64; 3]) -> [f64; 3] {
    [-v[0], -v[1], -v[2]]
}
fn axis_rot(axis: usize, deg: f64) -> M4 {
    let (s, c) = deg.to_radians().sin_cos();
    let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
    let mut m = I4;
    m[a][a] = c;
    m[a][b] = -s;
    m[b][a] = s;
    m[b][b] = c;
    m
}
/// FBX Euler angles (degrees) in the node's rotation order; `0` = XYZ,
/// meaning X is applied first: R = Rz · Ry · Rx.
fn euler(r: [f64; 3], order: i64) -> Result<M4> {
    let seq: [usize; 3] = match order {
        0 => [0, 1, 2],
        1 => [0, 2, 1],
        2 => [1, 2, 0],
        3 => [1, 0, 2],
        4 => [2, 0, 1],
        5 => [2, 1, 0],
        o => bail!("FBX rotation order {o} is not supported"),
    };
    let mut m = I4;
    for a in seq {
        m = mm(&axis_rot(a, r[a]), &m);
    }
    Ok(m)
}
/// Inverse of a rotation (orthonormal) 4×4.
fn rot_inv(m: &M4) -> M4 {
    let mut t = I4;
    for r in 0..3 {
        for c in 0..3 {
            t[r][c] = m[c][r];
        }
    }
    t
}
/// FBX matrices are stored column-major.
fn from_fbx(a: &[f64]) -> Result<M4> {
    if a.len() != 16 {
        bail!("FBX matrix with {} values", a.len());
    }
    let mut m = [[0.0; 4]; 4];
    for (r, row) in m.iter_mut().enumerate() {
        for (c, v) in row.iter_mut().enumerate() {
            *v = a[c * 4 + r];
        }
    }
    Ok(m)
}
fn apply(m: &M4, p: [f64; 3]) -> [f64; 3] {
    let mut o = [0.0; 3];
    for (r, v) in o.iter_mut().enumerate() {
        *v = m[r][0] * p[0] + m[r][1] * p[1] + m[r][2] * p[2] + m[r][3];
    }
    o
}
fn apply_dir(m: &M4, p: [f64; 3]) -> [f64; 3] {
    let mut o = [0.0; 3];
    for (r, v) in o.iter_mut().enumerate() {
        *v = m[r][0] * p[0] + m[r][1] * p[1] + m[r][2] * p[2];
    }
    o
}
/// `m`'s linear part for normals: the inverse transpose, up to scale (the
/// cofactor matrix, signed by the determinant), so non-uniform scale keeps
/// normals perpendicular to their faces. Normalise the result.
fn normal_matrix(m: &M4) -> M4 {
    let c = |r: usize, k: usize| {
        let (r1, r2) = ((r + 1) % 3, (r + 2) % 3);
        let (k1, k2) = ((k + 1) % 3, (k + 2) % 3);
        m[r1][k1] * m[r2][k2] - m[r1][k2] * m[r2][k1]
    };
    let det = m[0][0] * c(0, 0) + m[0][1] * c(0, 1) + m[0][2] * c(0, 2);
    // Scaled so the largest cofactor is 1: tiny scales don't underflow.
    let big = (0..9).map(|i| c(i / 3, i % 3).abs()).fold(0.0f64, f64::max);
    let s = if big == 0.0 || !big.is_finite() {
        0.0
    } else if det < 0.0 {
        -1.0 / big
    } else {
        1.0 / big
    };
    let mut o = I4;
    for (r, row) in o.iter_mut().take(3).enumerate() {
        for (k, v) in row.iter_mut().take(3).enumerate() {
            *v = s * c(r, k);
        }
    }
    o
}
fn f3(v: [f64; 3]) -> V3 {
    [v[0] as f32, v[1] as f32, v[2] as f32]
}

/// Local transform of a Model node:
/// T · Roff · Rp · Rpre · R · Rpost⁻¹ · Rp⁻¹ · Soff · Sp · S · Sp⁻¹.
fn local_of(m: &FNode) -> Result<M4> {
    let v = |k: &str, d: f64| m.p70_v3(k).unwrap_or([d; 3]);
    let order = m.p70_num("RotationOrder").unwrap_or(0.0) as i64;
    let r = euler(v("Lcl Rotation", 0.0), order)?;
    let pre = euler(v("PreRotation", 0.0), 0)?;
    let post = euler(v("PostRotation", 0.0), 0)?;
    let (rp, sp) = (v("RotationPivot", 0.0), v("ScalingPivot", 0.0));
    let chain = [
        tr(v("Lcl Translation", 0.0)),
        tr(v("RotationOffset", 0.0)),
        tr(rp),
        pre,
        r,
        rot_inv(&post),
        tr(neg(rp)),
        tr(v("ScalingOffset", 0.0)),
        tr(sp),
        sc(v("Lcl Scaling", 1.0)),
        tr(neg(sp)),
    ];
    Ok(chain.iter().fold(I4, |a, b| mm(&a, b)))
}

/// The mesh-only geometric transform (not inherited by children).
fn geometric_of(m: &FNode) -> Result<M4> {
    let v = |k: &str, d: f64| m.p70_v3(k).unwrap_or([d; 3]);
    Ok(mm(
        &mm(
            &tr(v("GeometricTranslation", 0.0)),
            &euler(v("GeometricRotation", 0.0), 0)?,
        ),
        &sc(v("GeometricScaling", 1.0)),
    ))
}

// ---------------------------------------------------------------- scene

/// Linear → sRGB, for FBX colours (Blender writes linear base colours).
fn srgb(c: f64) -> f64 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// The name part of an FBX object name (`Name\0\x01Class`).
fn obj_name(n: &FNode) -> String {
    n.str(1)
        .unwrap_or("")
        .split('\0')
        .next()
        .unwrap_or("")
        .to_string()
}

fn base_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

struct Material {
    name: String,
    colour: [f64; 3],
    texture: Option<String>,
}

struct Cluster {
    bone: i64,
    indexes: Vec<i64>,
    weights: Vec<f64>,
    /// The mesh's bind world: FBX files store `Transform` in the bone's
    /// space (the SDK hides this), so it is `TransformLink · Transform`.
    mesh_bind: Option<M4>,
}

/// Inputs for an FBX conversion besides the file itself.
pub struct Load<'a> {
    /// The FBX file name, to find its `.meta` in the package.
    pub file_name: String,
    pub title: String,
    pub licence: String,
    pub package: Option<&'a Package>,
    /// Prefab (or scene) name to take settings from (G4).
    pub variant: Option<&'a str>,
    /// The download's files, for textures the package doesn't hold (G16).
    pub files: Option<&'a crate::source::Files>,
    /// The FBX's path inside `files`: lookups prefer files nearest to it.
    pub path: String,
    /// `--material` overrides (G12): FBX material name → `.mat` name in the
    /// package, or an image file name (package, download) or path.
    pub materials: Vec<(String, String)>,
}

/// Blender's default material colour (linear 0.8) in sRGB: a colour nobody
/// set (G14).
const DEFAULT_GREY: f64 = 0.906;

pub fn load(bytes: &[u8], l: &Load) -> Result<Avatar> {
    let top = parse(bytes)?;
    let find = |n: &str| top.iter().find(|t| t.name == n);
    let objects = find("Objects").ok_or_else(|| anyhow!("FBX has no Objects"))?;
    let conns = find("Connections").ok_or_else(|| anyhow!("FBX has no Connections"))?;
    let mut notes = Vec::new();

    // Unity package settings (G2, G4, G5, G10, G11), matched on bone names.
    let bone_names: Vec<String> = objects
        .all("Model")
        .filter(|m| m.str(2) != Some("Mesh"))
        .map(obj_name)
        .collect();
    let bone_refs: Vec<&str> = bone_names.iter().map(String::as_str).collect();
    // Hierarchy paths (`Armature/hips/spine`), for Unity's hashed fileIDs (G18).
    let model_name: HashMap<i64, String> = objects
        .all("Model")
        .filter_map(|m| Some((m.int(0)?, obj_name(m))))
        .collect();
    let mut model_parent: HashMap<i64, i64> = HashMap::new();
    for c in conns.all("C") {
        if c.str(0) == Some("OO")
            && let (Some(ch), Some(pa)) = (c.int(1), c.int(2))
            && model_name.contains_key(&ch)
            && model_name.contains_key(&pa)
        {
            model_parent.insert(ch, pa);
        }
    }
    let mut model_paths: Vec<String> = model_name
        .keys()
        .map(|&id| {
            let mut v = vec![model_name[&id].as_str()];
            let mut at = id;
            while let Some(&pa) = model_parent.get(&at) {
                v.push(&model_name[&pa]);
                at = pa;
                if v.len() > model_name.len() {
                    break; // a cycle; FBX files shouldn't have one
                }
            }
            v.reverse();
            v.join("/")
        })
        .collect();
    model_paths.sort();
    let unity = match l.package {
        Some(p) => Some(unitypkg::settings(
            p,
            &l.file_name,
            // A packaged FBX's path is "<package>:<asset path>".
            l.path
                .to_ascii_lowercase()
                .find(".unitypackage:")
                .map(|i| &l.path[i + ".unitypackage:".len()..]),
            &bone_refs,
            &model_paths,
            l.variant,
        )?),
        None => None,
    };
    if let Some(u) = &unity {
        notes.extend(u.notes.iter().cloned());
    }

    // G6: axes and units → glTF-like Y-up, metres.
    let gs = find("GlobalSettings");
    let gi = |k: &str, d: f64| gs.and_then(|g| g.p70_num(k)).unwrap_or(d);
    let (up, up_s) = (gi("UpAxis", 1.0) as usize, gi("UpAxisSign", 1.0));
    let (front, front_s) = (gi("FrontAxis", 2.0) as usize, gi("FrontAxisSign", 1.0));
    let (coord, coord_s) = (gi("CoordAxis", 0.0) as usize, gi("CoordAxisSign", 1.0));
    // Unity's import scale applies on top of the file's units (G11).
    let unit = gi("UnitScaleFactor", 1.0) / 100.0 * unity.as_ref().map_or(1.0, |u| u.global_scale);
    let mut axes = [[0.0f64; 4]; 4];
    axes[3][3] = 1.0;
    if up > 2 || front > 2 || coord > 2 || up == front || up == coord || front == coord {
        bail!("FBX axis settings are inconsistent");
    }
    axes[0][coord] = coord_s * unit;
    axes[1][up] = up_s * unit;
    axes[2][front] = front_s * unit;
    notes.push(format!(
        "FBX axes (G6): up {}{}, front {}{}, coord {}{}, {} m per unit",
        if up_s < 0.0 { "-" } else { "+" },
        ["X", "Y", "Z"][up],
        if front_s < 0.0 { "-" } else { "+" },
        ["X", "Y", "Z"][front],
        if coord_s < 0.0 { "-" } else { "+" },
        ["X", "Y", "Z"][coord],
        unit
    ));

    let by_id: HashMap<i64, &FNode> = objects
        .children
        .iter()
        .filter_map(|o| o.int(0).map(|id| (id, o)))
        .collect();
    // Every object is looked up by its ID below; one without is malformed.
    if let Some(o) = objects.children.iter().find(|o| o.int(0).is_none()) {
        bail!("FBX object `{}` has no ID", o.name);
    }
    // Connections: (child, parent) for OO, plus property links.
    let mut parent_of: HashMap<i64, i64> = HashMap::new();
    let mut children_of: HashMap<i64, Vec<i64>> = HashMap::new();
    for c in conns.all("C") {
        let (Some(a), Some(b)) = (c.int(1), c.int(2)) else {
            continue;
        };
        children_of.entry(b).or_default().push(a);
        let (Some(ca), Some(cb)) = (by_id.get(&a), by_id.get(&b)) else {
            continue;
        };
        if c.str(0) == Some("OO") && ca.name == "Model" && cb.name == "Model" {
            parent_of.insert(a, b);
        }
    }
    let kids = |id: i64, kind: &str| -> Vec<&FNode> {
        children_of
            .get(&id)
            .into_iter()
            .flatten()
            .filter_map(|c| by_id.get(c).copied())
            .filter(|n| n.name == kind)
            .collect()
    };

    // Nodes: every Model, in file order.
    let models: Vec<&FNode> = objects.all("Model").collect();
    let index: HashMap<i64, usize> = models
        .iter()
        .enumerate()
        .map(|(i, m)| (m.int(0).unwrap(), i))
        .collect();
    let parent: Vec<Option<usize>> = models
        .iter()
        .map(|m| {
            parent_of
                .get(&m.int(0).unwrap())
                .and_then(|p| index.get(p))
                .copied()
        })
        .collect();
    // A parent loop would recurse forever in the world and ancestry walks.
    for (start, model) in models.iter().enumerate() {
        let (mut at, mut steps) = (start, 0);
        while let Some(p) = parent[at] {
            steps += 1;
            if steps > parent.len() {
                bail!("FBX model `{}` is its own ancestor", obj_name(model));
            }
            at = p;
        }
    }
    let local: Vec<M4> = models.iter().map(|m| local_of(m)).collect::<Result<_>>()?;

    // Bind pose: cluster TransformLink, else BindPose matrix, else parent · local.
    let mut bind: HashMap<usize, M4> = HashMap::new();
    for p in objects.all("Pose") {
        if p.str(2) != Some("BindPose") {
            continue;
        }
        for pn in p.all("PoseNode") {
            if let (Some(id), Some(m)) = (
                pn.child("Node").and_then(|n| n.int(0)),
                pn.child("Matrix").and_then(FNode::floats),
            ) && let Some(&i) = index.get(&id)
            {
                bind.insert(i, from_fbx(m)?);
            }
        }
    }
    // Nodes skin clusters point at: bones, even when typed Null.
    let mut skinned: HashSet<usize> = HashSet::new();
    let mut clusters_of: HashMap<i64, Vec<Cluster>> = HashMap::new(); // geometry id →
    for skin in objects.all("Deformer").filter(|d| d.str(2) == Some("Skin")) {
        let sid = skin.int(0).unwrap();
        let Some(&geo) = parent_of_any(conns, sid).first() else {
            continue;
        };
        for cl in kids(sid, "Deformer") {
            let cid = cl.int(0).unwrap();
            let Some(bone) = kids(cid, "Model").first().and_then(|b| b.int(0)) else {
                continue;
            };
            let link = cl
                .child("TransformLink")
                .and_then(FNode::floats)
                .map(from_fbx)
                .transpose()?;
            if let Some(&bi) = index.get(&bone) {
                skinned.insert(bi);
                if let Some(link) = link {
                    bind.insert(bi, link);
                }
            }
            let transform = cl
                .child("Transform")
                .and_then(FNode::floats)
                .map(from_fbx)
                .transpose()?;
            clusters_of.entry(geo).or_default().push(Cluster {
                bone,
                indexes: cl
                    .child("Indexes")
                    .and_then(FNode::ints)
                    .unwrap_or(&[])
                    .to_vec(),
                weights: cl
                    .child("Weights")
                    .and_then(FNode::floats)
                    .unwrap_or(&[])
                    .to_vec(),
                mesh_bind: link.zip(transform).map(|(l, t)| mm(&l, &t)),
            });
        }
    }
    let mut world: Vec<Option<M4>> = vec![None; models.len()];
    fn world_of(
        i: usize,
        parent: &[Option<usize>],
        local: &[M4],
        bind: &HashMap<usize, M4>,
        world: &mut Vec<Option<M4>>,
    ) -> M4 {
        if let Some(w) = world[i] {
            return w;
        }
        let w = match (bind.get(&i), parent[i]) {
            (Some(b), _) => *b,
            (None, Some(p)) => mm(&world_of(p, parent, local, bind, world), &local[i]),
            (None, None) => local[i],
        };
        world[i] = Some(w);
        w
    }
    for i in 0..models.len() {
        world_of(i, &parent, &local, &bind, &mut world);
    }
    let world: Vec<M4> = world.into_iter().map(|w| mm(&axes, &w.unwrap())).collect();
    let nodes: Vec<Node> = models
        .iter()
        .enumerate()
        .map(|(i, m)| {
            // Rigid frame: the scale is dropped, so local offsets are metres.
            let w = &world[i];
            let col = |c: usize| norm([w[0][c] as f32, w[1][c] as f32, w[2][c] as f32]);
            let x = col(0);
            let y = norm(sub(col(1), crate::math::mul(x, dot(x, col(1)))));
            let z = cross(x, y);
            Node {
                name: obj_name(m),
                parent: parent[i],
                world: Xf::new(
                    M3([[x[0], y[0], z[0]], [x[1], y[1], z[1]], [x[2], y[2], z[2]]]),
                    [w[0][3] as f32, w[1][3] as f32, w[2][3] as f32],
                ),
            }
        })
        .collect();

    // Materials and their textures.
    let mut materials: HashMap<i64, Material> = HashMap::new();
    for m in objects.all("Material") {
        let id = m.int(0).unwrap();
        let c = m
            .p70_v3("DiffuseColor")
            .or_else(|| m.p70_v3("Diffuse"))
            .unwrap_or([1.0; 3]);
        let mut texture = None;
        for c in conns.all("C") {
            if c.str(0) == Some("OP")
                && c.int(2) == Some(id)
                && matches!(c.str(3), Some("DiffuseColor" | "Diffuse" | "BaseColor"))
                && let Some(t) = c.int(1).and_then(|t| by_id.get(&t))
            {
                texture = t
                    .child("RelativeFilename")
                    .and_then(|f| f.str(0))
                    .or_else(|| t.child("FileName").and_then(|f| f.str(0)))
                    .map(|f| base_name(f).to_string());
            }
        }
        materials.insert(
            id,
            Material {
                name: obj_name(m),
                colour: [srgb(c[0]), srgb(c[1]), srgb(c[2])],
                texture,
            },
        );
    }

    // Images: package files by name first, then the download's, nearest first.
    let mut images: Vec<Vec<u8>> = Vec::new();
    let mut image_of: HashMap<String, Option<usize>> = HashMap::new();
    let mut find_image = |name: &str, data: Option<Vec<u8>>, notes: &mut Vec<String>| {
        // Explicit bytes (a `.mat`'s texture) are cached by content, so two
        // different images that share a file name stay apart, and an earlier
        // miss by name can't hide them.
        let key = match &data {
            Some(b) => format!("bytes:{:016x}", crate::unitypkg::xxh64(b)),
            None => format!("name:{}", name.to_lowercase()),
        };
        *image_of.entry(key).or_insert_with(|| {
            let bytes = data
                .or_else(|| l.package.and_then(|p| p.file_by_name(name)))
                .or_else(|| {
                    l.files
                        .and_then(|f| f.near(&l.path, name))
                        .map(<[u8]>::to_vec)
                });
            match bytes {
                Some(b) if crate::texture::is_image(&b) => {
                    images.push(b);
                    Some(images.len() - 1)
                }
                Some(_) => {
                    notes.push(format!(
                        "warning: texture {name} is not a PNG or JPEG; drawn as a flat colour"
                    ));
                    None
                }
                None => {
                    notes.push(format!(
                        "warning: texture {name} not found; drawn as a flat colour"
                    ));
                    None
                }
            }
        })
    };

    // Meshes → parts, one per (mesh, material slot).
    struct Pending {
        part: Part,
        colour: [f64; 3],
    }
    let mut parts: Vec<Pending> = Vec::new();
    let mut ngons = 0usize;
    for (mi, m) in models.iter().enumerate() {
        if m.str(2) != Some("Mesh") {
            continue;
        }
        let mid = m.int(0).unwrap();
        let Some(geo) = kids(mid, "Geometry")
            .into_iter()
            .find(|g| g.str(2) == Some("Mesh"))
        else {
            continue;
        };
        let gid = geo.int(0).unwrap();
        let name = obj_name(m);
        let slots: Vec<i64> = children_of
            .get(&mid)
            .into_iter()
            .flatten()
            .filter(|c| by_id.get(c).is_some_and(|n| n.name == "Material"))
            .copied()
            .collect();
        let verts = geo
            .child("Vertices")
            .and_then(FNode::floats)
            .ok_or_else(|| anyhow!("mesh {name}: no Vertices"))?;
        let pvi = geo
            .child("PolygonVertexIndex")
            .and_then(FNode::ints)
            .ok_or_else(|| anyhow!("mesh {name}: no PolygonVertexIndex"))?;
        let ncp = verts.len() / 3;

        // Skin: dominant bone per control point (D3), and the bind transform.
        let clusters = clusters_of.get(&gid);
        let mut best = vec![(0.0f64, None::<usize>); ncp];
        let mut all: Vec<Vec<(usize, f32)>> = vec![Vec::new(); ncp];
        let mut mesh_bind = None;
        for c in clusters.into_iter().flatten() {
            if mesh_bind.is_none() && !c.indexes.is_empty() {
                mesh_bind = c.mesh_bind;
            }
            let Some(&bi) = index.get(&c.bone) else {
                continue;
            };
            for (k, &v) in c.indexes.iter().enumerate() {
                let w = c.weights.get(k).copied().unwrap_or(0.0);
                if let Some(a) = all.get_mut(v as usize)
                    && w > 0.0
                {
                    match a.iter_mut().find(|x| x.0 == bi) {
                        Some(x) => x.1 += w as f32,
                        None => a.push((bi, w as f32)),
                    }
                }
                if let Some(b) = best.get_mut(v as usize)
                    && w > b.0
                {
                    *b = (w, Some(bi));
                }
            }
        }
        let fallback = parent[mi].unwrap_or(mi);
        let to_world = mm(
            &axes,
            &mm(
                &mesh_bind.unwrap_or_else(|| {
                    // Unskinned: the mesh node's own bind world (without axes).
                    world_raw(mi, &parent, &local, &bind)
                }),
                &geometric_of(m)?,
            ),
        );

        let to_world_n = normal_matrix(&to_world);
        // A mirrored mesh (negative determinant) has its faces turned inside
        // out; reverse their corners so every part keeps one winding.
        let mirrored = {
            let m = &to_world;
            m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
                - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
                + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
                < 0.0
        };

        // Per-corner attributes.
        let layer = |kind: &str| geo.child(kind);
        let normals = layer("LayerElementNormal");
        let uvs = layer("LayerElementUV");
        let mats = layer("LayerElementMaterial");
        let attr = |le: Option<&FNode>, data: &str, idx: &str, comps: usize| -> Result<Attr> {
            let Some(le) = le else {
                return Ok(Attr::None);
            };
            let mapping = le
                .child("MappingInformationType")
                .and_then(|n| n.str(0))
                .unwrap_or("");
            let reference = le
                .child("ReferenceInformationType")
                .and_then(|n| n.str(0))
                .unwrap_or("Direct");
            let values = le
                .child(data)
                .and_then(FNode::floats)
                .ok_or_else(|| anyhow!("mesh {name}: {data} missing"))?
                .to_vec();
            let index = if reference == "IndexToDirect" || reference == "Index" {
                Some(
                    le.child(idx)
                        .and_then(FNode::ints)
                        .ok_or_else(|| anyhow!("mesh {name}: {idx} missing"))?
                        .to_vec(),
                )
            } else {
                None
            };
            Ok(Attr::Some {
                mapping: match mapping {
                    "ByVertice" | "ByVertex" | "ByControlPoint" => Mapping::Vertex,
                    "AllSame" => Mapping::AllSame,
                    "ByPolygon" => Mapping::Polygon,
                    _ => Mapping::Corner,
                },
                values,
                index,
                comps,
            })
        };
        let nattr = attr(normals, "Normals", "NormalsIndex", 3)?;
        let uattr = attr(uvs, "UV", "UVIndex", 2)?;
        let mat_of_poly: Box<dyn Fn(usize) -> usize> = match mats {
            Some(le) => {
                let mapping = le
                    .child("MappingInformationType")
                    .and_then(|n| n.str(0))
                    .unwrap_or("");
                let v = le
                    .child("Materials")
                    .and_then(FNode::ints)
                    .unwrap_or(&[])
                    .to_vec();
                if mapping == "AllSame" {
                    let s = v.first().copied().unwrap_or(0).max(0) as usize;
                    Box::new(move |_| s)
                } else {
                    Box::new(move |p| v.get(p).copied().unwrap_or(0).max(0) as usize)
                }
            }
            None => Box::new(|_| 0),
        };

        // Corner → (slot part, vertex index) with exact-match welding.
        let mut slot_parts: BTreeMap<usize, (Part, HashMap<WeldKey, u32>)> = BTreeMap::new();
        // Unity numbers a renderer's materials by first use in the polygons,
        // not by FBX connection order (Saji's `Body`); prefab overrides by
        // slot index need that number (G18).
        let mut first_use: Vec<usize> = Vec::new();
        let mut poly = Vec::new();
        let mut pi = 0usize;
        for (corner, &raw) in pvi.iter().enumerate() {
            let (cp, last) = if raw < 0 {
                ((-raw - 1) as usize, true)
            } else {
                (raw as usize, false)
            };
            if cp >= ncp {
                bail!("mesh {name}: polygon index {cp} out of range");
            }
            poly.push((cp, corner));
            if !last {
                continue;
            }
            let slot = mat_of_poly(pi);
            if !first_use.contains(&slot) {
                first_use.push(slot);
            }
            let (part, weld) = slot_parts.entry(slot).or_insert_with(|| {
                (
                    Part {
                        id: format!("{name}#{slot}"),
                        node_name: name.clone(),
                        material: String::new(),
                        positions: Vec::new(),
                        normals: Vec::new(),
                        uvs: Vec::new(),
                        dominant: Vec::new(),
                        weights: Vec::new(),
                        triangles: Vec::new(),
                        image: None,
                        unlit: true,
                        masked: false,
                        first_person_only: false,
                    },
                    HashMap::new(),
                )
            });
            let cp_pos = |cp: usize| {
                f3(apply(
                    &to_world,
                    [verts[cp * 3], verts[cp * 3 + 1], verts[cp * 3 + 2]],
                ))
            };
            let pts: Vec<V3> = poly.iter().map(|&(cp, _)| cp_pos(cp)).collect();
            let mut ids = Vec::with_capacity(poly.len());
            for &(cp, k) in &poly {
                let n = nattr
                    .get(cp, k, pi)
                    .map_or([0.0; 3], |n| f3(apply_dir(&to_world_n, [n[0], n[1], n[2]])));
                let n = norm(n);
                // FBX UV origin is bottom-left; glTF (and our parts) top-left.
                let uv = uattr
                    .get(cp, k, pi)
                    .map_or([0.0; 2], |t| [t[0] as f32, 1.0 - t[1] as f32]);
                let key = (
                    cp as u32,
                    n.map(f32::to_bits),
                    [uv[0].to_bits(), uv[1].to_bits()],
                );
                let id = *weld.entry(key).or_insert_with(|| {
                    part.positions.push(cp_pos(cp));
                    part.normals.push(n);
                    part.uvs.push(uv);
                    let d = best[cp].1.unwrap_or(fallback);
                    part.dominant.push(d);
                    part.weights
                        .push(crate::vrm::normalised(all[cp].clone(), d));
                    (part.positions.len() - 1) as u32
                });
                ids.push(id);
            }
            if ids.len() > 4 {
                ngons += 1;
            }
            for [a, b, c] in triangulate(&pts) {
                part.triangles.push(if mirrored {
                    [ids[a], ids[c], ids[b]]
                } else {
                    [ids[a], ids[b], ids[c]]
                });
            }
            poly.clear();
            pi += 1;
        }

        for (slot, (mut part, _)) in slot_parts {
            let unity_slot = first_use.iter().position(|&x| x == slot).unwrap_or(slot);
            let mat = slots.get(slot).and_then(|id| materials.get(id));
            let fbx_mat = mat.map_or("?".to_string(), |m| m.name.clone());
            part.material = fbx_mat.clone();
            // G2: a prefab's `.mat` wins over the FBX material.
            // G12: a `--material` override wins over everything.
            let forced: Option<unitypkg::UnityMaterial> = match l
                .materials
                .iter()
                .find(|(k, _)| mat.is_some_and(|m| &m.name == k))
            {
                Some((k, v)) => Some(
                    l.package
                        .and_then(|p| unitypkg::material_named(p, v))
                        .or_else(|| {
                            let data = l
                                .package
                                .and_then(|p| p.file_by_name(v))
                                .or_else(|| {
                                    l.files.and_then(|f| f.near(&l.path, v)).map(<[u8]>::to_vec)
                                })
                                .or_else(|| std::fs::read(v).ok())?;
                            Some(unitypkg::UnityMaterial {
                                name: v.clone(),
                                colour: [1.0; 3],
                                masked: false,
                                texture: Some((base_name(v).to_string(), data)),
                            })
                        })
                        .with_context(|| {
                            format!("--material {k}={v}: no such .mat or image (G12)")
                        })?,
                ),
                None => None,
            };
            let unity_mat = forced.as_ref().or_else(|| {
                unity.as_ref().and_then(|u| {
                    u.renderer_materials
                        .get(&name)
                        .and_then(|v| v.get(unity_slot))
                        .and_then(|m| m.as_ref())
                        .or_else(|| mat.and_then(|m| u.remap.get(&m.name)))
                })
            });
            let colour;
            if let Some(um) = unity_mat {
                colour = um.colour;
                part.masked = um.masked;
                if let Some((tname, data)) = &um.texture {
                    part.image = find_image(tname, Some(data.clone()), &mut notes);
                }
                notes.push(format!(
                    "part {}: FBX material {fbx_mat} → {} {}{}",
                    part.id,
                    um.name,
                    if forced.is_some() {
                        "(--material, G12)"
                    } else if unity.as_ref().is_some_and(|u| {
                        u.renderer_materials
                            .get(&name)
                            .and_then(|v| v.get(unity_slot))
                            .is_some_and(Option::is_some)
                    }) {
                        "(prefab, G2)"
                    } else {
                        "(FBX .meta remap, G2)"
                    },
                    if part.image.is_some() {
                        ""
                    } else {
                        ", flat colour"
                    }
                ));
            } else {
                colour = mat.map_or([1.0; 3], |m| m.colour);
                if let Some(t) = mat.and_then(|m| m.texture.clone()) {
                    part.image = find_image(&t, None, &mut notes);
                }
                notes.push(format!(
                    "part {}: FBX material {fbx_mat} (G2){}",
                    part.id,
                    if part.image.is_some() {
                        ""
                    } else {
                        ", flat colour"
                    }
                ));
            }
            if part.image.is_some() && colour.iter().any(|&c| c < 0.99) {
                notes.push(format!(
                    "warning: part {}: colour tint {colour:.2?} on a texture is not applied",
                    part.id
                ));
            }
            parts.push(Pending { part, colour });
        }
    }
    if ngons > 0 {
        notes.push(format!(
            "{ngons} polygons with more than 4 sides ear-clipped (G6)"
        ));
    }

    // G14 (A15): a material with no texture gets the one whose paint best
    // covers its UVs; flat colour (G3) only when none clearly fits.
    if parts.iter().any(|p| p.part.image.is_none()) {
        let cands = crate::pair::candidates(l.package, l.files);
        for p in parts
            .iter_mut()
            .filter(|p| p.part.image.is_none() && !p.part.triangles.is_empty())
        {
            // An authored colour (not Blender's default 0.8 grey) marks a
            // material meant to be flat.
            if p.colour.iter().any(|&c| (c - DEFAULT_GREY).abs() > 0.01) {
                notes.push(format!(
                    "part {}: colour {:.2?} set in the FBX, kept flat (G14)",
                    p.part.id, p.colour
                ));
                continue;
            }
            match crate::pair::choose(&p.part, &p.part.material, &l.path, &cands) {
                Ok((c, f, line)) => {
                    p.part.image = find_image(&c.file, Some(c.png.clone()), &mut notes);
                    p.part.masked = c.masked.unwrap_or(f.clear > 0.05); // C5
                    notes.push(format!("part {}: texture paired (G14): {line}", p.part.id));
                }
                Err(why) => notes.push(format!(
                    "part {}: no texture paired (G14): {why}; flat colour",
                    p.part.id
                )),
            }
        }
    }

    // G3: one swatch texture for every flat-colour part.
    let flat: Vec<[u8; 3]> = {
        let mut set: Vec<[u8; 3]> = parts
            .iter()
            .filter(|p| p.part.image.is_none())
            .map(|p| p.colour.map(|c| (c * 255.0).round() as u8))
            .collect();
        set.sort();
        set.dedup();
        set
    };
    if !flat.is_empty() {
        const CELL: usize = 8;
        let cols = (flat.len() as f64).sqrt().ceil() as usize;
        let rows = flat.len().div_ceil(cols);
        let (w, h) = (cols * CELL, rows * CELL);
        let mut img = image::RgbImage::new(w as u32, h as u32);
        for (k, c) in flat.iter().enumerate() {
            let (cx, cy) = ((k % cols) * CELL, (k / cols) * CELL);
            for y in cy..cy + CELL {
                for x in cx..cx + CELL {
                    img.put_pixel(x as u32, y as u32, image::Rgb(*c));
                }
            }
        }
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .context("encoding the swatch texture")?;
        images.push(png);
        let swatch = images.len() - 1;
        for p in parts.iter_mut().filter(|p| p.part.image.is_none()) {
            let c = p.colour.map(|c| (c * 255.0).round() as u8);
            let k = flat.binary_search(&c).unwrap();
            let uv = [
                ((k % cols) as f32 + 0.5) / cols as f32,
                ((k / cols) as f32 + 0.5) / rows as f32,
            ];
            p.part.uvs.iter_mut().for_each(|t| *t = uv);
            p.part.image = Some(swatch);
        }
        notes.push(format!(
            "swatch texture (G3): {} colours in {w}x{h}",
            flat.len()
        ));
    }
    let parts: Vec<Part> = parts.into_iter().map(|p| p.part).collect();

    // Humanoid: Unity's map first, then bone names (G10, G1). A mesh object
    // can share a bone's name (Lilyanne's `Head`): skeleton nodes win.
    let skeletal: Vec<bool> = models
        .iter()
        .enumerate()
        .map(|(i, m)| m.str(2) == Some("LimbNode") || skinned.contains(&i))
        .collect();
    let by_name: HashMap<&str, usize> = {
        let mut m = HashMap::new();
        for (i, n) in nodes.iter().enumerate().rev() {
            match m.get(n.name.as_str()) {
                Some(&j) if skeletal[j] && !skeletal[i] => {}
                _ => {
                    m.insert(n.name.as_str(), i);
                }
            }
        }
        m
    };
    let mut humanoid = HashMap::new();
    let unity_human = unity.as_ref().map(|u| &u.human).filter(|h| !h.is_empty());
    if let Some(h) = unity_human {
        for (bone, human) in h {
            if let Some(&i) = by_name.get(bone.as_str()) {
                let mut c = human.chars();
                let vrm = c
                    .next()
                    .map(|f| f.to_lowercase().chain(c).collect::<String>())
                    .unwrap_or_default();
                humanoid.insert(vrm, i);
            }
        }
        notes.push(format!(
            "humanoid (G10): Unity map, {} bones",
            humanoid.len()
        ));
    } else {
        humanoid = map_by_name(&nodes, &skeletal);
        notes.push(format!(
            "humanoid (G1): by bone name, {} bones: {}",
            humanoid.len(),
            {
                let mut v: Vec<String> = humanoid
                    .iter()
                    .map(|(k, &i)| format!("{k}={}", nodes[i].name))
                    .collect();
                v.sort();
                v.join(", ")
            }
        ));
    }
    let required = [
        "hips",
        "spine",
        "head",
        "leftUpperArm",
        "rightUpperArm",
        "leftLowerArm",
        "rightLowerArm",
        "leftUpperLeg",
        "rightUpperLeg",
        "leftLowerLeg",
        "rightLowerLeg",
    ];
    let missing: Vec<&str> = required
        .iter()
        .filter(|b| !humanoid.contains_key(**b))
        .copied()
        .collect();
    if !missing.is_empty() {
        bail!("no bone found for the humanoid {missing:?} (G1)");
    }

    // Sway (G5 → D15): PhysBones become spring groups.
    let mut spring = HashSet::new();
    let mut spring_groups = Vec::new();
    let human_nodes: HashSet<usize> = humanoid.values().copied().collect();
    for pb in unity.as_ref().map(|u| &u.physbones[..]).unwrap_or(&[]) {
        let Some(&root) = by_name.get(pb.root.as_str()) else {
            notes.push(format!(
                "PhysBone at {}: no such FBX bone, skipped",
                pb.root
            ));
            continue;
        };
        let ignored: HashSet<usize> = pb
            .ignore
            .iter()
            .filter_map(|n| by_name.get(n.as_str()).copied())
            .collect();
        let under = |i: usize| {
            let mut k = Some(i);
            while let Some(n) = k {
                if ignored.contains(&n) {
                    return false;
                }
                if n == root {
                    return true;
                }
                k = nodes[n].parent;
            }
            false
        };
        // The root itself stays put when it fans out (multiChildType Ignore).
        let root_kids: Vec<usize> = (0..nodes.len())
            .filter(|&c| nodes[c].parent == Some(root) && !ignored.contains(&c))
            .collect();
        let roots = if root_kids.len() > 1 || human_nodes.contains(&root) {
            root_kids
        } else {
            vec![root]
        };
        for i in 0..nodes.len() {
            if under(i) && !human_nodes.contains(&i) && (i != root || roots == [root]) {
                spring.insert(i);
            }
        }
        let colliders = pb
            .colliders
            .iter()
            .filter_map(|c| {
                by_name
                    .get(c.bone.as_str())
                    .map(|&node| crate::vrm::Collider {
                        node,
                        // Unity mirrors X when importing FBX.
                        offset: [-c.position[0], c.position[1], c.position[2]],
                        radius: c.radius,
                    })
            })
            .collect();
        spring_groups.push(SpringGroup {
            name: pb.root.clone(),
            // PhysBone pull ≈ UniVRM stiffness / 4; spring (momentum) ≈ 1 − drag.
            stiffness: pb.pull * 4.0,
            gravity_power: pb.gravity,
            gravity_dir: [0.0, -1.0, 0.0],
            drag: (1.0 - pb.spring).clamp(0.0, 1.0),
            hit_radius: pb.radius,
            roots,
            colliders,
        });
    }
    if !spring_groups.is_empty() {
        notes.push(format!(
            "PhysBones (G5): {} groups, {} chain bones",
            spring_groups.len(),
            spring.len()
        ));
    }

    Ok(Avatar {
        title: l.title.clone(),
        licence: l.licence.clone(),
        nodes,
        humanoid,
        spring,
        spring_groups,
        parts,
        images,
        notes,
    })
}

/// Objects that `id` is connected to (its parents in the connection graph).
fn parent_of_any(conns: &FNode, id: i64) -> Vec<i64> {
    conns
        .all("C")
        .filter(|c| c.int(1) == Some(id))
        .filter_map(|c| c.int(2))
        .collect()
}

/// Bind world of a node in FBX space (no axis conversion).
fn world_raw(i: usize, parent: &[Option<usize>], local: &[M4], bind: &HashMap<usize, M4>) -> M4 {
    if let Some(b) = bind.get(&i) {
        return *b;
    }
    match parent[i] {
        Some(p) => mm(&world_raw(p, parent, local, bind), &local[i]),
        None => local[i],
    }
}

/// (control point, normal bits, UV bits): corners that weld into one vertex.
type WeldKey = (u32, [u32; 3], [u32; 2]);

/// How a layer element's values map onto the mesh.
#[derive(Clone, Copy)]
enum Mapping {
    AllSame,
    /// One per control point.
    Vertex,
    /// One per polygon.
    Polygon,
    /// One per polygon vertex (corner).
    Corner,
}

enum Attr {
    None,
    Some {
        mapping: Mapping,
        values: Vec<f64>,
        index: Option<Vec<i64>>,
        comps: usize,
    },
}

impl Attr {
    /// Value for control point `cp`, polygon-vertex `k` of polygon `p`.
    fn get(&self, cp: usize, k: usize, p: usize) -> Option<&[f64]> {
        let Attr::Some {
            mapping,
            values,
            index,
            comps,
        } = self
        else {
            return None;
        };
        let i = match mapping {
            Mapping::AllSame => 0,
            Mapping::Vertex => cp,
            Mapping::Polygon => p,
            Mapping::Corner => k,
        };
        let i = match index {
            Some(ix) => usize::try_from(*ix.get(i)?).ok()?,
            None => i,
        };
        values.get(i * comps..i * comps + comps)
    }
}

/// Triangles of a planar-ish polygon: quads split on the shorter diagonal
/// when convex, anything else ear-clipped (G6). Indices into `pts`.
pub fn triangulate(pts: &[V3]) -> Vec<[usize; 3]> {
    let n = pts.len();
    if n < 3 {
        return Vec::new();
    }
    if n == 3 {
        return vec![[0, 1, 2]];
    }
    // Newell normal.
    let mut nrm = [0.0f32; 3];
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        nrm[0] += (a[1] - b[1]) * (a[2] + b[2]);
        nrm[1] += (a[2] - b[2]) * (a[0] + b[0]);
        nrm[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    let nrm = norm(nrm);
    let convex_at = |a: V3, b: V3, c: V3| dot(cross(sub(b, a), sub(c, b)), nrm) > 0.0;
    if n == 4 {
        let ok02 = convex_at(pts[3], pts[0], pts[1]) && convex_at(pts[1], pts[2], pts[3]);
        let ok13 = convex_at(pts[0], pts[1], pts[2]) && convex_at(pts[2], pts[3], pts[0]);
        let d02 = len(sub(pts[0], pts[2]));
        let d13 = len(sub(pts[1], pts[3]));
        if ok13 && (!ok02 || d13 < d02) {
            return vec![[0, 1, 3], [1, 2, 3]];
        }
        if ok02 {
            return vec![[0, 1, 2], [0, 2, 3]];
        }
    }
    let mut idx: Vec<usize> = (0..n).collect();
    let mut out = Vec::with_capacity(n - 2);
    let inside = |p: V3, a: V3, b: V3, c: V3| {
        let s = |u: V3, v: V3| dot(cross(sub(v, u), sub(p, u)), nrm) >= 0.0;
        s(a, b) && s(b, c) && s(c, a)
    };
    while idx.len() > 3 {
        let m = idx.len();
        let ear = (0..m).find(|&i| {
            let (a, b, c) = (
                pts[idx[(i + m - 1) % m]],
                pts[idx[i]],
                pts[idx[(i + 1) % m]],
            );
            convex_at(a, b, c)
                && !idx.iter().enumerate().any(|(j, &q)| {
                    j != i && j != (i + m - 1) % m && j != (i + 1) % m && inside(pts[q], a, b, c)
                })
        });
        // Degenerate or self-intersecting: fall back to a fan of what's left.
        let Some(i) = ear else {
            for k in 1..idx.len() - 1 {
                out.push([idx[0], idx[k], idx[k + 1]]);
            }
            return out;
        };
        out.push([idx[(i + m - 1) % m], idx[i], idx[(i + 1) % m]]);
        idx.remove(i);
    }
    out.push([idx[0], idx[1], idx[2]]);
    out
}

/// Humanoid bones by name (G1): case, separators and side markers
/// (`.L`, `_L`, `Left`, `L_`) are normalised; the shallowest match wins.
/// `skeletal[i]`: node `i` is a bone (LimbNode or skinned to); bones win over
/// other nodes of the same role, then the shallowest.
pub fn map_by_name(nodes: &[Node], skeletal: &[bool]) -> HashMap<String, usize> {
    let depth = |mut i: usize| {
        let mut d = 0;
        while let Some(p) = nodes[i].parent {
            d += 1;
            i = p;
        }
        d
    };
    let parsed: Vec<Option<(Option<char>, String)>> =
        nodes.iter().map(|n| parse_bone(&n.name)).collect();
    let has_upleg = parsed
        .iter()
        .flatten()
        .any(|(_, b)| b == "upleg" || b == "upperleg" || b == "thigh");
    let mut out: HashMap<String, ((bool, usize), usize)> = HashMap::new();
    for (i, p) in parsed.iter().enumerate() {
        let Some((side, base)) = p else { continue };
        let human = match base.as_str() {
            "hips" | "hip" | "pelvis" => "hips",
            "spine" => "spine",
            "chest" | "spine1" => "chest",
            "upperchest" | "spine2" => "upperChest",
            "neck" => "neck",
            "head" => "head",
            "shoulder" | "clavicle" => "Shoulder",
            "upperarm" | "uparm" | "arm" => "UpperArm",
            "lowerarm" | "forearm" | "elbow" => "LowerArm",
            "hand" | "wrist" => "Hand",
            "upperleg" | "upleg" | "thigh" => "UpperLeg",
            "lowerleg" | "calf" | "shin" | "knee" => "LowerLeg",
            "leg" if has_upleg => "LowerLeg",
            "leg" => "UpperLeg",
            "foot" | "ankle" => "Foot",
            "toe" | "toes" | "toebase" => "Toes",
            _ => continue,
        };
        let key = match (side, human.chars().next().unwrap().is_uppercase()) {
            (Some('l'), true) => format!("left{human}"),
            (Some('r'), true) => format!("right{human}"),
            (None, false) => human.to_string(),
            _ => continue,
        };
        let rank = (!skeletal.get(i).copied().unwrap_or(true), depth(i));
        let e = out.entry(key).or_insert((rank, i));
        if rank < e.0 {
            *e = (rank, i);
        }
    }
    out.into_iter().map(|(k, (_, i))| (k, i)).collect()
}

/// (side, base name) of a bone name, or None for end/tip bones.
fn parse_bone(name: &str) -> Option<(Option<char>, String)> {
    let mut s = name.to_lowercase();
    for pre in ["mixamorig:", "j_bip_", "bip01", "bip001", "armature|"] {
        if let Some(r) = s.strip_prefix(pre) {
            s = r.to_string();
        }
    }
    let mut toks: Vec<String> = s
        .split(|c: char| " ._-:|".contains(c))
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect();
    if toks.last().is_some_and(|t| t == "end") {
        return None;
    }
    let mut side = None;
    let is_side = |t: &str| match t {
        "l" | "left" => Some('l'),
        "r" | "right" => Some('r'),
        _ => None,
    };
    if let Some(sd) = toks.last().and_then(|t| is_side(t)) {
        side = Some(sd);
        toks.pop();
    } else if let Some(sd) = toks.first().and_then(|t| is_side(t)) {
        side = Some(sd);
        toks.remove(0);
    }
    let mut base: String = toks.concat();
    if side.is_none() {
        for (p, sd) in [("left", 'l'), ("right", 'r')] {
            if let Some(r) = base.strip_prefix(p) {
                side = Some(sd);
                base = r.to_string();
                break;
            }
        }
    }
    if base.is_empty() {
        return None;
    }
    Some((side, base))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mesh_named_like_a_bone_does_not_take_its_role() {
        // Lilyanne: a top-level mesh object `Head` and the `Head` bone.
        let node = |name: &str, parent| Node {
            name: name.into(),
            parent,
            world: crate::math::Xf::IDENTITY,
        };
        let nodes = vec![
            node("Head", None),
            node("Hips", None),
            node("Spine", Some(1)),
            node("Head", Some(2)),
        ];
        let m = map_by_name(&nodes, &[false, true, true, true]);
        assert_eq!(m["head"], 3);
    }

    #[test]
    fn bone_names_parse_with_sides() {
        let cases = [
            ("UpperArm.L", Some(('l', "upperarm"))),
            ("upper_arm.R", Some(('r', "upperarm"))),
            ("LeftUpLeg", Some(('l', "upleg"))),
            ("J_Bip_L_LowerArm", Some(('l', "lowerarm"))),
            ("mixamorig:RightForeArm", Some(('r', "forearm"))),
        ];
        for (n, want) in cases {
            let got = parse_bone(n);
            assert_eq!(
                got.as_ref().map(|(s, b)| (s.unwrap(), b.as_str())),
                want,
                "{n}"
            );
        }
        assert_eq!(parse_bone("Hips"), Some((None, "hips".into())));
        assert_eq!(parse_bone("Head_end"), None);
    }

    #[test]
    fn concave_polygon_is_ear_clipped_without_overlap() {
        // An L shape starting at (2, 1), which can't see the whole interior:
        // a fan from vertex 0 would cover the notch with a flipped triangle.
        let pts: Vec<V3> = [
            [2.0, 1.0],
            [1.0, 1.0],
            [1.0, 2.0],
            [0.0, 2.0],
            [0.0, 0.0],
            [2.0, 0.0],
        ]
        .iter()
        .map(|p| [p[0], p[1], 0.0])
        .collect();
        let tris = triangulate(&pts);
        assert_eq!(tris.len(), 4);
        let area: f32 = tris
            .iter()
            .map(|t| {
                let c = cross(sub(pts[t[1]], pts[t[0]]), sub(pts[t[2]], pts[t[0]]));
                assert!(c[2] > 0.0, "{t:?} is flipped");
                c[2] / 2.0
            })
            .sum();
        assert!((area - 3.0).abs() < 1e-5, "{area}");
    }

    #[test]
    fn normals_use_the_inverse_transpose() {
        // x scaled by 2: the plane x + y = 0's normal (1, 1, 0) becomes (1, 2, 0).
        let mut m = I4;
        m[0][0] = 2.0;
        let n = apply_dir(&normal_matrix(&m), [1.0, 1.0, 0.0]);
        assert!((n[1] / n[0] - 2.0).abs() < 1e-9 && n[2] == 0.0, "{n:?}");
        // A mirror keeps the normal on the face's outside.
        m[0][0] = -1.0;
        let n = apply_dir(&normal_matrix(&m), [1.0, 0.0, 0.0]);
        assert!(n[0] < 0.0, "{n:?}");
    }

    #[test]
    fn quad_splits_on_short_diagonal() {
        let pts = [
            [0.0, 0.0, 0.0],
            [3.0, 0.0, 0.0],
            [3.0, 1.0, 0.0],
            [0.5, 1.0, 0.0],
        ];
        // 0–2 is longer than 1–3, so split on 1–3.
        assert_eq!(triangulate(&pts), vec![[0, 1, 3], [1, 2, 3]]);
    }

    #[test]
    fn euler_xyz_applies_x_first() {
        let m = euler([90.0, 0.0, 90.0], 0).unwrap();
        // Y → (X by 90) → Z → (Z by 90) stays Z.
        let p = apply(&m, [0.0, 1.0, 0.0]);
        assert!((p[2] - 1.0).abs() < 1e-9, "{p:?}");
    }
}
