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
| A2 | Targets | Decided | GoldSrc first; **Source (Phase E) starts once GoldSrc is hardened**: a second avatar converts cleanly and the open C rows are settled (user, 2026-11-14). |
| A3 | Which Source game | Decided | **GMod first; other Source games later**, each as its own Question row (user, 2026-11-14). |
| A4 | Which GoldSrc game | Decided | **Sven Co-op** first (user, 2026-11-14). Evidence: yuki_ref has the 192-sequence Sven player set. |
| A5 | Input formats | Decided | v1 reads **VRM** only (user accepted the proposal, 2026-11-14). Earlier note: **Evidence (lucian):** the download ships `.blend`, `.fbx`, `.unitypackage` and `.vrm` together. The VRM is 0.x, with a full humanoid map (53 bones), 3 spring-bone groups and 14 blendshape groups. Proposal: the first run uses the VRM, so AssetRipper isn't needed yet. VRChat avatars arrive as (a) a Unity package / project with FBX + prefab, (b) an AssetBundle, which needs AssetRipper, or (c) VRM (glTF 2.0, humanoid map built in, no ripping needed). Support all three, with everything normalised to one glTF + humanoid-map file at the end of stage 1? |
| A6 | Source rights | Decided | Lucian (`Redistribution_Prohibited`, no commercial use) is a **local test fixture only**: its output stays in `out/` and never goes to a server or FastDL (user, 2026-11-14). The same applies to yuki_ref (model by Yuki, anims by TapChan). Whether to make the `licence` field mandatory for all avatars is still open: see A13. |
| A7 | Language and app form | Decided | **Rust**, one binary with its libraries compiled in. **CLI first**; a Tauri GUI comes in a later phase (user, 2026-11-14). |
| A8 | Compute location | Decided | **Local is fine**: Lucian converts in 0.2 s. The global rule still applies, so any heavy job goes remote (user, 2026-11-14). |
| A9 | Compiling the MDL | Decided | libremodele **writes MDL v10 itself**, with no studiomdl (user, 2026-11-14). |
| A10 | Spike: MDL writer round-trip | Decided | **Passed** (user, 2026-11-14): yuki_ref reads, writes and re-reads identically. |
| A11 | Limits | Decided | **Sven's raised limits where documented** (user, 2026-11-14). Research (2026-11-14): Sven raises the texture limit to **1024²** (HL is 512²). Every other documented limit is HL's and stays: per mesh **2,048 verts/normals and 4,080 triangles**, **128 bones**, and Sven guidance of **≤ ~9k polys** per playermodel. Sven playermodel rules: extra bones only as children of existing bones; same sequence order, frame counts and FPS; the hitbox set cannot change; no skins or swappable bodies; portrait is a 164×200 BMP with a full 256-colour palette (fewer than 224 colours triggers remapping). Sources: developer.valvesoftware.com/wiki/HLMDL, the303.org/tutorials/gold_player_sven_comp.htm. |
| A13 | Licence gate | Decided | **Warn, don't block** (user, 2026-11-14): the report prints the VRM licence and flags redistribution limits; output is still written. |
| A14 | Verifying without the game | Decided | **Own checks only**: libremodele re-reads its output and renders previews (rest pose and frames from the donor animations). There is no in-game or HLAM step. Gate G5 becomes "re-read and preview pass" (user, 2026-11-14). |
| D10 | Texture render flag | Decided | **Flatshade** (0x1) for unlit/toon materials, the same as yuki_ref (user, 2026-11-14). |
| D11 | Triangle strips | Decided | **Build real strips and fans** (user, 2026-11-14). Lossless (C2), and gives a smaller download. **Result:** Lucian is 6,356 tris in 1,774 strips; the file shrank from 1.40 to 1.31 MB. Most of the rest is animation: each of the 56 bones has a 12-byte header in each of the 308 sequence blends. |
| D12 | Normal welding precision | Decided | **Weld normals coarser** (~1°) for flatshade textures, to fit fewer bodyparts (user, 2026-11-14). **Result:** Lucian went from 3 bodyparts to 2 (2,018 + 1,439 verts). |
| D13 | Chain animation | Decided | **Procedural sway** for avatar chains (user, 2026-11-14): generated per sequence from the root and parent motion. This needs a design spike before building (see D15). |
| C8 | Near-lossless option | Decided | **Add later** (user, 2026-11-14): build it when an avatar goes over Sven's ~9k guidance, not before. Then: an optional `--max-error <units>`, off by default so C2 stays the rule, same C3 locks, allowing collapses whose geometric error is under the cap. **Revised 2026-11-14 (user): build now, budget-driven.** Quadric (QEM) edge-collapse decimation down to the C4 budget, after the lossless pass. UV seams and bone boundaries stay locked where possible (C3), relaxed only if the budget can't be met otherwise. The report gives the max geometric error. |
| D16 | Sway on or off | Decided | **Optional**: a `--sway` flag, off by default (user, 2026-11-14). The report gives the added size. |
| D14 | Hitbox set | Decided | **Copy the donor's hitbox set** for the bones we keep, with bounds rescaled to the avatar (user, 2026-11-14). Overrides the hitbox part of D8. |
| D15 | Procedural sway | Decided | **Option A: UniVRM spring port** (user, 2026-11-14), within the per-sequence 64 KB budget (roots first; long sequences fall back to static chains). **Strands that share triangles with another strand stay static** (user, 2026-11-14: "hair + ears only"): under rigid binding (D3) swinging them apart tears the mesh, as Lucian's dress panels did. For Lucian, ears, FHair and SHairA sway (10 bones, +0.51 MB, all 192 sequences within budget), while the dress and the joined back hair stay static. **Findings 2026-11-14.** (1) Lucian's VRM has its own spring settings: stiffness 1.0, gravity 0, drag 0.1 for EAR, HAIR and DRESS, plus sphere colliders on both upper legs for the DRESS group. A UniVRM-style spring simulation could reuse them as the author tuned them. (2) Size: 34 chain bones (17 chain roots). Worst case with no run-length savings: all 34 bones animated add 1.92 MB, roots only add 0.96 MB, 8 bones add 0.45 MB. The model is 1.31 MB today. (3) Hard limit: each sequence's animation data must fit 64 KB (u16 offsets). `ref_reload_saw` (211 frames, 2 blends) already uses 60.7 KB with no sway, so even one swaying bone overflows it, and with all 34 bones 4 sequences overflow. Any design must choose bones per sequence within the budget and fall back to static chains for long sequences. (4) 102 of 192 sequences loop: their sway has to be simulated over several cycles and taken from a settled cycle, so the loop seam doesn't pop. **Options (not chosen):** A) UniVRM spring port: Verlet simulation with the VRM's own stiffness, drag, gravity and colliders, at each sequence's fps with substeps; within budget, roots get sway first. Most faithful; most code. B) Lag filter: each chain bone trails its parent's rotation with damping. No physics and no colliders, so the dress can clip the legs. Simple. C) Roots only, with A or B: 17 bones, about +0.96 MB worst case, and the rest of each chain stays rigid. Smallest change to file size and download. |
| A12 | Licence of libremodele | Decided | **MIT** (user, 2026-11-14). |

## B: Ingest and clean

| Code | Topic | Status | Decision / Question |
|---|---|---|---|
| B1 | Extract tool | Decided | v1 reads **VRM / glb**; no AssetRipper (user accepted the proposal, 2026-11-14). Earlier note: AssetRipper is a separate app, so it conflicts with A1. Proposal: v1 reads **VRM / glb** (and FBX later). Unity bundles/packages stay a manual step outside the app for now. |
| B2 | Interchange format | Decided | Superseded by A1: one binary, stages pass data in memory; review files (`clean_manifest.json`, `preview.png`, `report.txt`) are the only stage outputs (user, 2026-11-14). |
| B3 | What "clean" deletes | Decided | **Rules only** for v1 (user, 2026-11-14): weld, drop degenerate tris, drop meshes VRM marks first-person-only, and drop eye-highlight shells by name. |
| B4 | Detection: rules + manifest | Decided | Rule results go to an editable `clean_manifest.json` (user, 2026-11-14). |
| B5 | Interior geometry method | Decided | **Not in v1** (user, 2026-11-14). No hidden-face removal. |
| B6 | Outfit / toggles | Decided | **Default outfit only**: parts that are on by default (user, 2026-11-14). Sven forbids swappable bodies anyway (A11). |
| B7 | Blendshapes | Decided | Bake the **neutral** face, with all blendshapes at 0, for GoldSrc (user, 2026-11-14). Source flexes are still open for Phase E. |
| B8 | Units and scale | Decided | Height is a **setting** (user, 2026-11-14). The default **matches the donor skeleton** (yuki_ref), so animations, hitboxes and eye height line up. |
| B9 | FBX input | Question | Raised 2026-11-14 by the user's Neru and Gothic Doll samples, which ship only FBX + Unity package (+ .blend), no VRM. Most VRChat avatars are distributed this way. Proposal for its own phase: an own binary FBX reader (A1), humanoid mapping by bone name, spring settings from the Unity package's PhysBone components. Would revise A5/B1. User: "do Miku first, FBX later". |

## C: Reduce, atlas, bake

| Code | Topic | Status | Decision / Question |
|---|---|---|---|
| C1 | Decimator | Decided | **Our own zero-error edge collapse** in Rust (user, 2026-11-14): collapse only where the shape, bone and UVs are unchanged (C2), with the C3 locks. **Result 2026-11-14:** built (`src/reduce.rs`). Lucian loses 0 triangles: 1,154 vertices are on curved fans, 1,154 on seams or creases, 802 on bone boundaries, 219 on open borders and 11 have non-affine UVs. |
| C2 | Stop rule | Decided | **Lossless reduction** (user, 2026-11-14): weld duplicates, drop degenerate and zero-area tris, and collapse only edges whose geometric error is ~0 (coplanar, same UVs, same bone). UV seams, open borders and bone boundaries are locked. The shape and texture mapping never change. **Revised 2026-11-14 (user, Miku: 65,737 tris):** lossless stays the first pass and the only pass when the avatar already fits the budget (C4). Above it, lossy QEM runs (C8). |
| C3 | Locked features | Decided | The conservative set, all four (user, 2026-11-14): **UV seams, open borders, bone boundaries, normal creases**. |
| C4 | Per-target budgets | Decided | **No triangle budget** (user, 2026-11-14). Reduction is **lossless only**: see C2. Hard engine limits still apply; if a submodel goes past the verts-per-submodel limit, it is split into submodels (D5). **Revised 2026-11-14 (user):** there is now a budget: default Sven's ~9k triangle guidance (A11), overridable with `--max-tris`; never above the 20k hard limit. Lossless avatars under budget are untouched. |
| C5 | Atlas tool | Decided | Single-texture avatars (Lucian) need **no atlas** (user accepted the proposal, 2026-11-14). Earlier note: Blender UV pack (or xatlas) into one atlas per material class (opaque / masked). The number of atlases sets the number of GoldSrc meshes and draw calls (D5). **Revised 2026-11-14 (user, Miku: 4 images): one GoldSrc texture per image**, no atlas. Each image becomes its own 8-bit texture (≤ 512², D6) and its own mesh (D5); materials with alpha cutout or blend get the masked flag, index 255 transparent (D6). |
| C6 | Bake | Decided | Single texture: **downscale only**; re-projection bake comes when an avatar needs an atlas (user accepted the proposal, 2026-11-14). Earlier note: No Cycles (A1). For unlit/toon avatars the "bake" is a texture re-projection: rasterise each atlas triangle and sample the source texture at the old UV (CPU, light). Lucian is `KHR_materials_unlit` with one texture, so this is enough for it. |
| C7 | Toon / MToon shading | Decided | **Base texture × colour tint only**; rim, matcap and emission are dropped (user, 2026-11-14). |

## D: GoldSrc back end

| Code | Topic | Status | Decision / Question |
|---|---|---|---|
| D1 | Target skeleton | Decided | Bip01 names and hierarchy **from the donor** (yuki_ref) (user accepted the proposal, 2026-11-14). Earlier note: *(task #18 scope)* Bind to the HL1 player skeleton (`Bip01 …`, as in the SDK `player.mdl` decompile) so the stock player animation set can be reused unchanged. |
| D2 | Bone mapping | Decided | Map the humanoid bones to Bip01. **Keep hair/accessory chains as static extra bones** (as yuki_ref does). **Merge finger weights into the hand**, since the stock animations don't move fingers (user, 2026-11-14). **Bone budget, 2026-11-14 (user, Miku: 191 bones):** when the bones would pass 128 (A11), merge the **deepest chain links first**, evenly across chains, into their parents until it fits. Roots and upper links stay, so sway (D15) still has bones to move. |
| D3 | Skinning | Decided | **Dominant bone per vertex**, with no extra joint geometry. Creases are checked on the previews (user, 2026-11-14). |
| D4 | Rest pose / retarget | Decided | **Keep Lucian's proportions** (user, 2026-11-14): keep the donor's bone orientations, so its rotation channels play unchanged; set bone offsets to Lucian's lengths along the donor's directions; scale root motion by the height ratio. Each vertex moves rigidly with its dominant bone (D3) from Lucian's pose to the new rest pose. |
| D5 | Mesh / submodel layout | Decided | **1 bodypart, 1 mesh** per texture; **split only when** the verts-per-submodel limit is passed (user, 2026-11-14). Technical note: submodels inside one bodypart are alternatives (only one is drawn), so a split adds **bodyparts**, not submodels. |
| D6 | Texture format | Decided | **512²**, 8-bit indexed, 256-colour palette per texture (user, 2026-11-14). Downscale, then quantise (pure Rust, A1). Masked materials use index 255 as transparent. |
| D7 | Animations | Decided | Take the sequences from **yuki_ref** (user, 2026-11-14). Standalone (A1) means no SMD decompile step: the app copies the animation data straight out of a "donor" MDL, with bones matched by name. The donor stays a local file (A6). |
| D8 | Extras | Decided | All four in v1 (hitboxes: see D14, which overrides this) (user, 2026-11-14): **hitboxes** from bone vertex bounds; a 164×200 **thumbnail BMP** from our own renderer (A14); the donor's **attachments** (4) and **bone controllers** (5), re-pointed to the matching bones. |
| D9 | Output | Decided | Output to `out/goldsrc/models/player/<name>/<name>.mdl` + `.bmp` (user accepted the proposal, 2026-11-14). Earlier note: `out/goldsrc/models/player/<name>/<name>.mdl` (+ `<name>T.mdl` if textures are external), ready to drop into a server's download/FastDL tree. |

## E: Source back end

| Code | Topic | Status | Decision / Question |
|---|---|---|---|
| E1 | Target skeleton | Decided | **ValveBiped + `$includemodel`** (user, 2026-11-14): map the humanoid onto `ValveBiped.Bip01_*` with the avatar's proportions (as D4), and borrow GMod's stock animations (`m_anm`/`f_anm`) through the MDL's include-model list. The app writes the MDL itself (A1, A9); no studiomdl and no embedded animations. |
| E2 | Skinning | Decided | **Top 3 weights, renormalised** (user, 2026-11-14): keep the VRM's smooth weights, take the 3 largest per vertex (the VVD limit) and rescale them to sum to 1. |
| E3 | Materials | Decided | **Own DXT1/DXT5 encoder** (user, 2026-11-14): write VTF ourselves (A1, no VTFLib), DXT5 when the texture has alpha, else DXT1, with mipmaps, plus a `VertexLitGeneric` VMT. Normal maps and phong are off for now. |
| E4 | Distribution | Decided | **Addon folder** (user, 2026-11-14): `models/`, `materials/` and `lua/autorun/` with `player_manager.AddValidModel` registration, ready for FastDL or Workshop. A6/A13 still apply. Packing a `.gma` and FastDLX hooks are later Question rows if wanted. |

## F: Batch and review

| Code | Topic | Status | Decision / Question |
|---|---|---|---|
| F1 | Review report | TBD | Read; revisit and rewrite when Phase F starts (user, 2026-11-14 re-confirmed). Each run writes `report.html`: before/after renders (front/side/back), tri/bone/texture counts against budget, decimation error, and the clean manifest. Every human gate (see `ARCHITECTURE.md` §D) is reviewed from this one page. |
| F2 | Re-run granularity | TBD | Read; revisit when Phase F starts (user, 2026-11-14 re-confirmed); likely dropped, since a run is in memory and takes ~0.2 s (B2 superseded). Each stage caches its output keyed by input hash + config. Editing the manifest at gate B re-runs from B only, not from extraction. |
| F3 | Config | TBD | Read; revisit when Phase F starts (user, 2026-11-14 re-confirmed); today settings are CLI flags plus the editable manifest (B4). One `avatar.toml` per avatar (source path, licence, target height, budgets, overrides) + global defaults per target. |
