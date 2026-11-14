# Decisions

Working list for deciding features, approach and technical details. This file overrules `ARCHITECTURE.md` and `ROADMAP.md` wherever they disagree.

Each item has a code: the letter is the `ROADMAP.md` phase, the number is its sequence within that phase. Codes are stable and never reused. A new feature takes the next unused letter. After `Z`, letters continue as `AA`, `AB` ... `AZ`, then `BA` and onward, like spreadsheet columns.

Status:
- **Decided**: settled and binding. State the choice and the reason in a few words.
- **Inspect**: needs a look first (spike, library, benchmark, test). Research only.
- **TBD**: read, but not discussed yet. Ask, don't guess.
- **Question**: raised (usually by the agent) and not yet read by the human. Triage it by moving it to TBD, Inspect or Decided. Treat it like TBD until then.

A phase starts only when its rows are Decided.

> Every row below was written by the agent, so every row is a **Question**. Rows marked *(task #18 scope)* restate what hobby task #18 already says. They are probably quick to move to Decided, but only the user moves them.

| Letter | Roadmap phase |
|---|---|
| A | Phase A: Decisions and spikes |
| B | Phase B: Ingest and clean (shared front end) |
| C | Phase C: Reduce, atlas and bake (shared, per-target budget) |
| D | Phase D: GoldSrc back end (MDL v10) |
| E | Phase E: Source back end (MDL v44+ server playermodel) |
| F | Phase F: Batch runs and review report |

## A: Project-wide

| Code | Topic | Status | Decision / Question |
|---|---|---|---|
| A1 | Shape of the tool | Decided | A **standalone app**: it does the whole conversion itself, with no Blender, Wine, studiomdl or other external tool to install (user, 2026-11-14). Offline, avatar in, playermodel out. |
| A2 | Targets | Question | Two back ends from one shared front end: GoldSrc (HL1 and mods, Sven Co-op?) **and** Source as a server playermodel. Should GoldSrc ship first and Source follow (as in the roadmap), or both together? |
| A3 | Which Source game | Question | "Source server playermodel" in practice means Garry's Mod (ValveBiped skeleton, `player_manager.AddValidModel`, Workshop or FastDL). CS:S, TF2 and HL2DM do not take custom player models without server plugins. GMod only, or are other games in scope? |
| A4 | Which GoldSrc game | Decided | **Sven Co-op** first (user, 2026-11-14). Evidence: yuki_ref has the 192-sequence Sven player set. |
| A5 | Input formats | Question | **Evidence (lucian):** the download ships `.blend`, `.fbx`, `.unitypackage` and `.vrm` together. The VRM is 0.x, with a full humanoid map (53 bones), 3 spring-bone groups and 14 blendshape groups. Proposal: the first run uses the VRM, so AssetRipper isn't needed yet. VRChat avatars arrive as (a) a Unity package / project with FBX + prefab, (b) an AssetBundle, which needs AssetRipper, or (c) VRM (glTF 2.0, humanoid map built in, no ripping needed). Support all three, with everything normalised to one glTF + humanoid-map file at the end of stage 1? |
| A6 | Source rights | Decided | Lucian (`Redistribution_Prohibited`, no commercial use) is a **local test fixture only**: its output stays in `out/` and never goes to a server or FastDL (user, 2026-11-14). The same applies to yuki_ref (model by Yuki, anims by TapChan). Whether to make the `licence` field mandatory for all avatars is still open: see A13. |
| A7 | Language and app form | Decided | **Rust**, one binary with its libraries compiled in. **CLI first**; a Tauri GUI comes in a later phase (user, 2026-11-14). |
| A8 | Compute location | Question | Every stage is expected to be light (one avatar, ≤100k tris in, textures ≤2k). Per the global rule, nothing heavy runs on the laptop: if a batch (F) or the bake (C) turns out heavy, it moves to Kaggle or the GPU server. Confirm the threshold, e.g. "anything over 2 min or 4 GB RAM goes remote". |
| A9 | Compiling the MDL | Decided | libremodele **writes MDL v10 itself**, with no studiomdl (user, 2026-11-14). |
| A10 | Spike: MDL writer round-trip | Question | Inspect: read yuki_ref, write it back with our writer, re-read it, and compare field by field: bones, sequences, meshes, textures, hitboxes. Sven Co-op is not installed (user, 2026-11-14), so loading it in game is the last check, not the first; see A14. **Result 2026-11-14:** passes on yuki_ref. All 38 bones, 192 sequences (decoded frame by frame), 3 bodyparts, the texture, skins, hitboxes and attachments come back identical; writing twice gives the same bytes; texture pixels end the file. Our file is 1,104,120 bytes vs 1,055,424 for the original: the animation runs are packed less tightly than studiomdl does, with the same values. |
| A11 | Spike: limits table | Question | **Evidence (yuki_ref, a working Sven model):** 38 bones, 3 bodyparts with 1 mesh each, 775 verts / 1464 tris total, one 256×256 8-bit texture (flags 0x1 = flatshade), 31 hitboxes, 4 attachments, 5 bone controllers, thumbnail BMP 164×200 8-bit. Inspect: confirm the real limits for the A4 target (bones, verts per submodel, tris, texture size and divisibility, skins), from `studio.h` and a test compile. The numbers in `ARCHITECTURE.md` §I are working assumptions until then. |
| A13 | Mandatory licence field | Question | Should every avatar need a `licence` entry in `avatar.toml` before output, with the VRM meta read automatically where present? |
| A14 | Verifying without the game | Decided | **Own checks only**: libremodele re-reads its output and renders previews (rest pose and frames from the donor animations). There is no in-game or HLAM step. Gate G5 becomes "re-read and preview pass" (user, 2026-11-14). |
| A12 | Licence of libremodele | Question | Licence of this repo (e.g. MIT, as FastDLX uses). Tools it calls (AssetRipper GPL-3, Blender GPL, meshoptimizer MIT, MeshLab GPL) are invoked as separate programs and not bundled. Is that OK? |

## B: Ingest and clean

| Code | Topic | Status | Decision / Question |
|---|---|---|---|
| B1 | Extract tool | Question | AssetRipper is a separate app, so it conflicts with A1. Proposal: v1 reads **VRM / glb** (and FBX later). Unity bundles/packages stay a manual step outside the app for now. |
| B2 | Interchange format | Question | glTF 2.0 (`.glb`) + a sidecar `avatar.json` (humanoid bone map, materials, licence, units) between every stage. Every stage is then a file-in, file-out step that can be checked and re-run. |
| B3 | What "clean" deletes | Question | *(task #18 scope)* PhysBone helper bones and colliders, eye/face shells (highlight, tear and blush overlays), toggle objects that are off by default, interior geometry (body under clothes), then weld by distance and strip degenerate/zero-area tris. |
| B4 | Detection: rules + manifest | Question | Automatic detection by name patterns (`*Highlight*`, `*_shadow*`, PhysBone components), by Unity "active=false" state, and by an occlusion test for interior faces. The result is written to `clean_manifest.json` (keep/delete per object, with the reason), which a human can edit and re-run. Is that the right control point? |
| B5 | Interior geometry method | Question | Inspect: ray-cast visibility from N directions around the avatar (delete faces never hit), against "delete the body faces under named clothing meshes". Ray-casting is general but can eat armpits and fingers, so it needs a deviation/area guard. |
| B6 | Outfit / toggles | Question | Pick one default outfit for v1. Later, toggles could become GoldSrc bodygroups / Source bodygroups (new phase). |
| B7 | Blendshapes | Question | GoldSrc has no morphs: bake the neutral face (or a chosen expression) into the mesh. For Source, keep the visemes as flexes later, or bake them too for v1? |
| B8 | Units and scale | Question | Normalise to a target height in game units (HL/Source player ≈ 72 units, eye ≈ 64), not to a 1:1 metre conversion. Chibi or very tall avatars then fit the hull. Should the target height be a per-avatar override? |

## C: Reduce, atlas, bake

| Code | Topic | Status | Decision / Question |
|---|---|---|---|
| C1 | Decimator | Question | *(task #18 scope)* Either MeshLab quadric edge collapse (pymeshlab, texture-aware variant) or meshoptimizer `simplify` / gltfpack. Inspect: compare both on 3 avatars by Hausdorff error, face quality, and how many seams and borders actually stayed locked. |
| C2 | Stop rule | Question | *(task #18 scope)* Deviation-capped: decimate toward the target tri budget but stop when the max geometric error passes a cap (e.g. 0.5% of height). If the cap is hit before the budget, report it instead of forcing it. Should the face/hands get a tighter cap (weighted regions)? |
| C3 | Locked features | Question | *(task #18 scope)* UV seams and open boundaries locked during collapse. Also lock skin-weight boundaries (where the dominant bone changes)? This keeps joints clean for D3 rigid skinning. |
| C4 | Per-target budgets | Question | Decimate from the cleaned mesh separately per target, never GoldSrc-from-Source. Starting budgets: GoldSrc ≈ 3–5k tris (vanilla) / ≈ 10k (Sven), Source ≈ 15–30k. Pending A4 and A11. |
| C5 | Atlas tool | Question | Blender UV pack (or xatlas) into one atlas per material class (opaque / masked). The number of atlases sets the number of GoldSrc meshes and draw calls (D5). |
| C6 | Bake | Question | No Cycles (A1). For unlit/toon avatars the "bake" is a texture re-projection: rasterise each atlas triangle and sample the source texture at the old UV (CPU, light). Lucian is `KHR_materials_unlit` with one texture, so this is enough for it. |
| C7 | Toon / MToon shading | Question | VRChat avatars mostly use lilToon/Poiyomi/MToon. AssetRipper gives textures, not the shader graph. Use the main texture × colour tint only, and drop rim, matcap and emission masks? Or bake emission into fullbright regions (GoldSrc `fullbright` flag)? |

## D: GoldSrc back end

| Code | Topic | Status | Decision / Question |
|---|---|---|---|
| D1 | Target skeleton | Question | *(task #18 scope)* Bind to the HL1 player skeleton (`Bip01 …`, as in the SDK `player.mdl` decompile) so the stock player animation set can be reused unchanged. |
| D2 | Bone mapping | Question | **Evidence (yuki_ref):** a working model **keeps** extra bones (`Bip01 Hair1-3`, `Tail1-5`, `Bip55 * Leg_skirt*`, `knee1-smooth`) next to the standard Bip01 set; the stock animations just don't move them. So extra bones could be kept (static) instead of merged. Humanoid map (VRM `humanoid` or Unity Avatar from AssetRipper) → Bip01 names via a fixed table. Extra bones (hair, skirt, tail, PhysBone chains) are removed and their weights merged into the nearest mapped parent. |
| D3 | Skinning | Question | GoldSrc allows **one bone per vertex**. Assign each vertex its dominant bone. Inspect: at elbows, knees and shoulders, also split the vertices along the weight boundary and add a ring of verts so the crease does not tear. |
| D4 | Rest pose | Question | Re-pose the avatar mesh (T/A-pose) onto the Bip01 reference pose before binding, so the stock animations play without offsets. Automatic per-bone rotation, then human check (gate D-G2). |
| D5 | Mesh / submodel layout | Question | One bodypart, one submodel, one mesh per texture (ideally 1 opaque + 1 masked). Split into more submodels only when the verts-per-submodel limit is hit. |
| D6 | Texture format | Question | 8-bit indexed BMP, 256-colour palette **per texture**, size from A11 (likely ≤512², multiple of 8). Quantise with libimagequant/pngquant + dithering. Masked materials (hair cards, lace) use palette index 255 as transparent with the `masked` render mode. |
| D7 | Animations | Decided | Take the sequences from **yuki_ref** (user, 2026-11-14). Standalone (A1) means no SMD decompile step: the app copies the animation data straight out of a "donor" MDL, with bones matched by name. The donor stays a local file (A6). |
| D8 | Extras | Question | Hitboxes generated from bone bounds, a `<name>.bmp` thumbnail (164×200 render), and optional top/bottom colour remap textures. Which of these are in v1? |
| D9 | Output | Question | `out/goldsrc/models/player/<name>/<name>.mdl` (+ `<name>T.mdl` if textures are external), ready to drop into a server's download/FastDL tree. |

## E: Source back end

| Code | Topic | Status | Decision / Question |
|---|---|---|---|
| E1 | Target skeleton | Question | ValveBiped (`ValveBiped.Bip01_*`) proportions-matched, with stock animations via `$includemodel` (GMod `m_anm`/`f_anm`). |
| E2 | Skinning | Question | Keep smooth weights, but cap them at the engine max (3 or 4 per vertex, see A11) and renormalise. |
| E3 | Materials | Question | VTF (DXT1/DXT5) + VMT `VertexLitGeneric`, albedo + optional normal map, phong off by default. Linux tool: VTFLib / vtfcmd (Inspect). |
| E4 | Distribution | Question | Output a GMod addon folder (`models/`, `materials/`, `lua/autorun/` playermodel registration), ready for FastDL or Workshop. Hook into FastDLX later (new phase)? |

## F: Batch and review

| Code | Topic | Status | Decision / Question |
|---|---|---|---|
| F1 | Review report | Question | Each run writes `report.html`: before/after renders (front/side/back), tri/bone/texture counts against budget, decimation error, and the clean manifest. Every human gate (see `ARCHITECTURE.md` §D) is reviewed from this one page. |
| F2 | Re-run granularity | Question | Each stage caches its output keyed by input hash + config. Editing the manifest at gate B re-runs from B only, not from extraction. |
| F3 | Config | Question | One `avatar.toml` per avatar (source path, licence, target height, budgets, overrides) + global defaults per target. |
