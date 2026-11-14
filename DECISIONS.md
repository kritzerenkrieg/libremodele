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
| A5 | Input formats | Decided | v1 reads **VRM** only (user accepted the proposal, 2026-11-14). Earlier note: **Evidence (lucian):** the download ships `.blend`, `.fbx`, `.unitypackage` and `.vrm` together. The VRM is 0.x, with a full humanoid map (53 bones), 3 spring-bone groups and 14 blendshape groups. Proposal: the first run uses the VRM, so AssetRipper isn't needed yet. VRChat avatars arrive as (a) a Unity package / project with FBX + prefab, (b) an AssetBundle, which needs AssetRipper, or (c) VRM (glTF 2.0, humanoid map built in, no ripping needed). Support all three, with everything normalised to one glTF + humanoid-map file at the end of stage 1? |
| A6 | Source rights | Decided | Lucian (`Redistribution_Prohibited`, no commercial use) is a **local test fixture only**: its output stays in `out/` and never goes to a server or FastDL (user, 2026-11-14). The same applies to yuki_ref (model by Yuki, anims by TapChan). Whether to make the `licence` field mandatory for all avatars is still open: see A13. |
| A7 | Language and app form | Decided | **Rust**, one binary with its libraries compiled in. **CLI first**; a Tauri GUI comes in a later phase (user, 2026-11-14). |
| A8 | Compute location | Question | Every stage is expected to be light (one avatar, ≤100k tris in, textures ≤2k). Per the global rule, nothing heavy runs on the laptop: if a batch (F) or the bake (C) turns out heavy, it moves to Kaggle or the GPU server. Confirm the threshold, e.g. "anything over 2 min or 4 GB RAM goes remote". |
| A9 | Compiling the MDL | Decided | libremodele **writes MDL v10 itself**, with no studiomdl (user, 2026-11-14). |
| A10 | Spike: MDL writer round-trip | Question | Inspect: read yuki_ref, write it back with our writer, re-read it, and compare field by field: bones, sequences, meshes, textures, hitboxes. Sven Co-op is not installed (user, 2026-11-14), so loading it in game is the last check, not the first; see A14. **Result 2026-11-14:** passes on yuki_ref. All 38 bones, 192 sequences (decoded frame by frame), 3 bodyparts, the texture, skins, hitboxes and attachments come back identical; writing twice gives the same bytes; texture pixels end the file. Our file is 1,104,120 bytes vs 1,055,424 for the original: the animation runs are packed less tightly than studiomdl does, with the same values. |
| A11 | Spike: limits table | Question | **Evidence (yuki_ref, a working Sven model):** 38 bones, 3 bodyparts with 1 mesh each, 775 verts / 1464 tris total, one 256×256 8-bit texture (flags 0x1 = flatshade), 31 hitboxes, 4 attachments, 5 bone controllers, thumbnail BMP 164×200 8-bit. Inspect: confirm the real limits for the A4 target (bones, verts per submodel, tris, texture size and divisibility, skins), from `studio.h` and a test compile. The numbers in `ARCHITECTURE.md` §I are working assumptions until then. |
| A13 | Mandatory licence field | Question | Should every avatar need a `licence` entry in `avatar.toml` before output, with the VRM meta read automatically where present? |
| A14 | Verifying without the game | Decided | **Own checks only**: libremodele re-reads its output and renders previews (rest pose and frames from the donor animations). There is no in-game or HLAM step. Gate G5 becomes "re-read and preview pass" (user, 2026-11-14). |
| D10 | Texture render flag | Decided | **Flatshade** (0x1) for unlit/toon materials, the same as yuki_ref (user, 2026-11-14). |
| D11 | Triangle strips | Decided | **Build real strips and fans** (user, 2026-11-14). Lossless (C2), and gives a smaller download. **Result:** Lucian is 6,356 tris in 1,774 strips; the file shrank from 1.40 to 1.31 MB. Most of the rest is animation: each of the 56 bones has a 12-byte header in each of the 308 sequence blends. |
| D12 | Normal welding precision | Decided | **Weld normals coarser** (~1°) for flatshade textures, to fit fewer bodyparts (user, 2026-11-14). **Result:** Lucian went from 3 bodyparts to 2 (2,018 + 1,439 verts). |
| D13 | Chain animation | Question | Lucian's hair, ear and dress chains are static bones (D2). The donor's own hair bones were animated (`Bip01 Hair1-3` in 134 sequences). Copy the donor's hair motion onto the avatar's hair chains later? |
| A12 | Licence of libremodele | Question | Licence of this repo (e.g. MIT, as FastDLX uses). Tools it calls (AssetRipper GPL-3, Blender GPL, meshoptimizer MIT, MeshLab GPL) are invoked as separate programs and not bundled. Is that OK? |

## B: Ingest and clean

| Code | Topic | Status | Decision / Question |
|---|---|---|---|
| B1 | Extract tool | Decided | v1 reads **VRM / glb**; no AssetRipper (user accepted the proposal, 2026-11-14). Earlier note: AssetRipper is a separate app, so it conflicts with A1. Proposal: v1 reads **VRM / glb** (and FBX later). Unity bundles/packages stay a manual step outside the app for now. |
| B2 | Interchange format | Question | glTF 2.0 (`.glb`) + a sidecar `avatar.json` (humanoid bone map, materials, licence, units) between every stage. Every stage is then a file-in, file-out step that can be checked and re-run. |
| B3 | What "clean" deletes | Decided | **Rules only** for v1 (user, 2026-11-14): weld, drop degenerate tris, drop meshes VRM marks first-person-only, and drop eye-highlight shells by name. |
| B4 | Detection: rules + manifest | Decided | Rule results go to an editable `clean_manifest.json` (user, 2026-11-14). |
| B5 | Interior geometry method | Decided | **Not in v1** (user, 2026-11-14). No hidden-face removal. |
| B6 | Outfit / toggles | Question | Pick one default outfit for v1. Later, toggles could become GoldSrc bodygroups / Source bodygroups (new phase). |
| B7 | Blendshapes | Decided | Bake the **neutral** face, with all blendshapes at 0, for GoldSrc (user, 2026-11-14). Source flexes are still open for Phase E. |
| B8 | Units and scale | Decided | Height is a **setting** (user, 2026-11-14). The default **matches the donor skeleton** (yuki_ref), so animations, hitboxes and eye height line up. |

## C: Reduce, atlas, bake

| Code | Topic | Status | Decision / Question |
|---|---|---|---|
| C1 | Decimator | Question | *(task #18 scope)* Either MeshLab quadric edge collapse (pymeshlab, texture-aware variant) or meshoptimizer `simplify` / gltfpack. Inspect: compare both on 3 avatars by Hausdorff error, face quality, and how many seams and borders actually stayed locked. |
| C2 | Stop rule | Decided | **Lossless reduction** (user, 2026-11-14): weld duplicates, drop degenerate and zero-area tris, and collapse only edges whose geometric error is ~0 (coplanar, same UVs, same bone). UV seams, open borders and bone boundaries are locked. The shape and texture mapping never change. |
| C3 | Locked features | Question | *(task #18 scope)* UV seams and open boundaries locked during collapse. Also lock skin-weight boundaries (where the dominant bone changes)? This keeps joints clean for D3 rigid skinning. |
| C4 | Per-target budgets | Decided | **No triangle budget** (user, 2026-11-14). Reduction is **lossless only**: see C2. Hard engine limits still apply; if a submodel goes past the verts-per-submodel limit, it is split into submodels (D5). |
| C5 | Atlas tool | Decided | Single-texture avatars (Lucian) need **no atlas** (user accepted the proposal, 2026-11-14). Earlier note: Blender UV pack (or xatlas) into one atlas per material class (opaque / masked). The number of atlases sets the number of GoldSrc meshes and draw calls (D5). |
| C6 | Bake | Decided | Single texture: **downscale only**; re-projection bake comes when an avatar needs an atlas (user accepted the proposal, 2026-11-14). Earlier note: No Cycles (A1). For unlit/toon avatars the "bake" is a texture re-projection: rasterise each atlas triangle and sample the source texture at the old UV (CPU, light). Lucian is `KHR_materials_unlit` with one texture, so this is enough for it. |
| C7 | Toon / MToon shading | Question | VRChat avatars mostly use lilToon/Poiyomi/MToon. AssetRipper gives textures, not the shader graph. Use the main texture × colour tint only, and drop rim, matcap and emission masks? Or bake emission into fullbright regions (GoldSrc `fullbright` flag)? |

## D: GoldSrc back end

| Code | Topic | Status | Decision / Question |
|---|---|---|---|
| D1 | Target skeleton | Decided | Bip01 names and hierarchy **from the donor** (yuki_ref) (user accepted the proposal, 2026-11-14). Earlier note: *(task #18 scope)* Bind to the HL1 player skeleton (`Bip01 …`, as in the SDK `player.mdl` decompile) so the stock player animation set can be reused unchanged. |
| D2 | Bone mapping | Decided | Map the humanoid bones to Bip01. **Keep hair/accessory chains as static extra bones** (as yuki_ref does). **Merge finger weights into the hand**, since the stock animations don't move fingers (user, 2026-11-14). |
| D3 | Skinning | Decided | **Dominant bone per vertex**, with no extra joint geometry. Creases are checked on the previews (user, 2026-11-14). |
| D4 | Rest pose / retarget | Decided | **Keep Lucian's proportions** (user, 2026-11-14): keep the donor's bone orientations, so its rotation channels play unchanged; set bone offsets to Lucian's lengths along the donor's directions; scale root motion by the height ratio. Each vertex moves rigidly with its dominant bone (D3) from Lucian's pose to the new rest pose. |
| D5 | Mesh / submodel layout | Decided | **1 bodypart, 1 mesh** per texture; **split only when** the verts-per-submodel limit is passed (user, 2026-11-14). Technical note: submodels inside one bodypart are alternatives (only one is drawn), so a split adds **bodyparts**, not submodels. |
| D6 | Texture format | Decided | **512²**, 8-bit indexed, 256-colour palette per texture (user, 2026-11-14). Downscale, then quantise (pure Rust, A1). Masked materials use index 255 as transparent. |
| D7 | Animations | Decided | Take the sequences from **yuki_ref** (user, 2026-11-14). Standalone (A1) means no SMD decompile step: the app copies the animation data straight out of a "donor" MDL, with bones matched by name. The donor stays a local file (A6). |
| D8 | Extras | Decided | All four in v1 (user, 2026-11-14): **hitboxes** from bone vertex bounds; a 164×200 **thumbnail BMP** from our own renderer (A14); the donor's **attachments** (4) and **bone controllers** (5), re-pointed to the matching bones. |
| D9 | Output | Decided | Output to `out/goldsrc/models/player/<name>/<name>.mdl` + `.bmp` (user accepted the proposal, 2026-11-14). Earlier note: `out/goldsrc/models/player/<name>/<name>.mdl` (+ `<name>T.mdl` if textures are external), ready to drop into a server's download/FastDL tree. |

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
