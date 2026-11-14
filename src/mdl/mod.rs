//! GoldSrc studio model (MDL v10) in memory, with a reader and a writer (A9).
//! Field layout follows the HL SDK `studio.h`. Animations are held decoded,
//! frame by frame, so they can be compared and copied between models (D7).
//! Every file offset is recomputed on write.

mod anim;
mod bytes;
mod read;
mod write;

pub use bytes::Vec3;
pub use read::read;
pub use write::write;

#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    pub name: String,
    pub eye_position: Vec3,
    pub min: Vec3,
    pub max: Vec3,
    pub bbmin: Vec3,
    pub bbmax: Vec3,
    pub flags: i32,
    pub bones: Vec<Bone>,
    pub bone_controllers: Vec<BoneController>,
    pub hitboxes: Vec<Hitbox>,
    pub sequences: Vec<Sequence>,
    pub seq_groups: Vec<SeqGroup>,
    pub textures: Vec<Texture>,
    /// Skin reference count (columns of `skin_families`).
    pub num_skinref: i32,
    /// `numskinfamilies` rows of `num_skinref` texture indices.
    pub skin_families: Vec<Vec<i16>>,
    pub bodyparts: Vec<Bodypart>,
    pub attachments: Vec<Attachment>,
    pub num_transitions: i32,
    /// `num_transitions`² node transition bytes.
    pub transitions: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Bone {
    pub name: String,
    pub parent: i32,
    pub flags: i32,
    pub controllers: [i32; 6],
    /// Rest position (x, y, z) and rotation (x, y, z, Euler radians).
    pub value: [f32; 6],
    /// Animation value scale per channel.
    pub scale: [f32; 6],
}

#[derive(Debug, Clone, PartialEq)]
pub struct BoneController {
    pub bone: i32,
    pub kind: i32,
    pub start: f32,
    pub end: f32,
    pub rest: i32,
    pub index: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hitbox {
    pub bone: i32,
    pub group: i32,
    pub bbmin: Vec3,
    pub bbmax: Vec3,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SeqGroup {
    pub label: String,
    pub name: String,
    pub unused1: i32,
    pub unused2: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sequence {
    pub label: String,
    pub fps: f32,
    pub flags: i32,
    pub activity: i32,
    pub act_weight: i32,
    pub events: Vec<Event>,
    pub num_frames: i32,
    pub pivots: Vec<Pivot>,
    pub motion_type: i32,
    pub motion_bone: i32,
    pub linear_movement: Vec3,
    pub automove_pos_index: i32,
    pub automove_angle_index: i32,
    pub bbmin: Vec3,
    pub bbmax: Vec3,
    /// One entry per blend, each with one `BoneAnim` per bone.
    pub blends: Vec<Vec<BoneAnim>>,
    pub blend_type: [i32; 2],
    pub blend_start: [f32; 2],
    pub blend_end: [f32; 2],
    pub blend_parent: i32,
    pub seq_group: i32,
    pub entry_node: i32,
    pub exit_node: i32,
    pub node_flags: i32,
    pub next_seq: i32,
}

/// Six channels (position x, y, z, rotation x, y, z) of raw per-frame values.
/// `None`: the channel stays at the bone's rest value.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BoneAnim {
    pub channels: [Option<Vec<i16>>; 6],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub frame: i32,
    pub event: i32,
    pub kind: i32,
    pub options: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pivot {
    pub org: Vec3,
    pub start: i32,
    pub end: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Attachment {
    pub name: String,
    pub kind: i32,
    pub bone: i32,
    pub org: Vec3,
    pub vectors: [Vec3; 3],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Texture {
    pub name: String,
    pub flags: i32,
    pub width: i32,
    pub height: i32,
    /// `width * height` palette indices.
    pub pixels: Vec<u8>,
    /// 256 RGB triples.
    pub palette: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Bodypart {
    pub name: String,
    pub base: i32,
    pub models: Vec<SubModel>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SubModel {
    pub name: String,
    pub kind: i32,
    pub bounding_radius: f32,
    /// The single bone each vertex follows (rigid skinning).
    pub vert_bones: Vec<u8>,
    pub verts: Vec<Vec3>,
    pub norm_bones: Vec<u8>,
    pub norms: Vec<Vec3>,
    pub meshes: Vec<Mesh>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Mesh {
    pub num_tris: i32,
    pub skinref: i32,
    pub num_norms: i32,
    /// Raw `normindex` (0 in studiomdl output).
    pub norm_index: i32,
    pub commands: Vec<TriCmd>,
}

/// A triangle strip or fan. Each vertex is (vertex index, normal index, s, t).
#[derive(Debug, Clone, PartialEq)]
pub struct TriCmd {
    pub fan: bool,
    pub verts: Vec<[i16; 4]>,
}

impl Model {
    pub fn tri_count(&self) -> usize {
        self.bodyparts
            .iter()
            .flat_map(|b| &b.models)
            .flat_map(|m| &m.meshes)
            .flat_map(|me| &me.commands)
            .map(|c| c.verts.len().saturating_sub(2))
            .sum()
    }
}
