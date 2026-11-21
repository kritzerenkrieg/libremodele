// Typed calls to the Rust backend (gui/src-tauri/src/lib.rs).
import { invoke } from "@tauri-apps/api/core";

/** The form: the input plus overrides (H3). Empty text means "choose for me". */
export interface Form {
  input: string;
  name: string;
  donor: string;
  avatar: string;
  package: string;
  licence: string;
  variant: string;
  materials: [string, string][];
  height: number | null;
  maxTris: number | null;
  keepPolygons: boolean;
  sway: boolean;
}

/** What the app chose for a download (H2). */
export interface Summary {
  avatarFile: string;
  title: string;
  licence: string;
  restricted: boolean;
  name: string;
  donor: string;
  donorWhy: string | null;
  heightM: number;
  parts: number;
  notes: string[];
  svenAddon: string | null;
}

export interface Outcome {
  name: string;
  report: string[];
  game: string;
  review: string;
  triangles: number;
  bytes: number;
  restricted: boolean;
}

export const emptyForm = (): Form => ({
  input: "",
  name: "",
  donor: "",
  avatar: "",
  package: "",
  licence: "",
  variant: "",
  materials: [],
  height: null,
  maxTris: null,
  keepPolygons: false,
  sway: false,
});

// Blank number inputs can arrive as undefined or NaN; send null.
const clean = (f: Form): Form => ({
  ...f,
  height: Number.isFinite(f.height) ? f.height : null,
  maxTris: Number.isFinite(f.maxTris) ? f.maxTris : null,
});

export const version = () => invoke<string>("version");
export const inspect = (form: Form) => invoke<Summary>("inspect", { form: clean(form) });
export const convert = (form: Form) => invoke<Outcome>("convert", { form: clean(form) });
export const install = (keepLocal: boolean) => invoke<string>("install", { keepLocal });

// 3D viewer (H4): see gui/src-tauri/src/view.rs.
export interface ModelView {
  name: string;
  bones: { name: string; parent: number }[];
  textures: { name: string; width: number; height: number; masked: boolean }[];
  meshes: { texture: number; positions: number[]; normals: number[]; uvs: number[]; bones: number[] }[];
  sequences: { label: string; fps: number; frames: number; blends: number; activity: number }[];
  hitboxes: { bone: number; group: number; min: number[]; max: number[] }[];
  gaitSplit: number;
  triangles: number;
}

export const modelView = (weapon: boolean) => invoke<ModelView>("model_view", { weapon });
export const texture = async (weapon: boolean, index: number) =>
  new Uint8Array(await invoke<ArrayBuffer>("texture", { weapon, index }));
/** 12 floats per bone per frame: rotation rows, then translation. */
export const boneFrames = async (weapon: boolean, sequence: number | null, blend: number) =>
  new Float32Array(await invoke<ArrayBuffer>("bone_frames", { weapon, sequence, blend }));
export const weapons = () => invoke<string[]>("weapons");
export const loadWeapon = (file: string | null) => invoke<void>("load_weapon", { file });
