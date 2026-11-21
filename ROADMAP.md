# Roadmap

Planned work, in order. `DECISIONS.md` overrules this file wherever they disagree. Phases are lettered and match decision codes. A phase starts only when its decisions are Decided.

```mermaid
flowchart LR
    A["Phase A<br/>Decisions and spikes"] --> B["Phase B<br/>Ingest and clean"] --> C["Phase C<br/>Reduce, atlas, bake"] --> D["Phase D<br/>GoldSrc back end"] --> E["Phase E<br/>Source back end"] --> F["Phase F<br/>Batch and review"] --> G["Phase G<br/>FBX + Unity package"] --> R
    subgraph R["Cycle"]
        direction LR
        FT["Feature"] --> M["Maintenance"] --> FT
    end
```

**Task format:** `- [ ] <task> (<codes>). Done when: <check>`

## Phase A: Decisions and spikes

**Goal:** settle what blocks Phase B, and the target numbers that B–D depend on.
**Exit:** every A and B row in `DECISIONS.md` is Decided. A11 limits are recorded.

- [x] Triage all Question rows with the user (A1–F3). Done when: no row is Question. Passed 2026-11-14 (C8 Decided; E1–E4 Decided; F1–F3 TBD until Phase F)
- [x] Test fixtures: Lucian (VRM/FBX/blend/unitypackage) and yuki_ref (Sven MDL) in `samples/`, local only (A5, A6). Recorded in A5 and A6
- [x] ~~Spike: studiomdl under Wine~~, replaced by A9 (own writer): MDL v10 round trip on yuki_ref (A9, A10). Done when: `cargo test --test roundtrip` passes. Passed 2026-11-14
- [ ] [P] Spike: confirm the GoldSrc + Source limits for the A3/A4 targets (A11). Done when: §I table in `ARCHITECTURE.md` shows confirmed numbers
- [ ] [P] ~~Spike: AssetRipper export~~: pending B1. Under A1, v1 reads VRM directly
- [ ] [P] Spike: interior-face detection, ray-cast vs naming (B5). Done when: B5 has before/after renders of both
- [x] Fill in `AGENTS.md` Commands and scaffold the repo (A7). Done when: the CLI prints usage and `cargo test` passes. Passed 2026-11-14 (A12, the repo licence, still open)

## Phase B: Ingest and clean

**Goal:** any supported source becomes a clean, normalised `clean.glb` + manifest.
**Exit:** fixture avatar passes gate G1 with no manual edits, or with only manifest edits.
**Out of scope:** decimation, rigging.

- [x] Stage 1: read VRM directly (B1, A5). B2's intermediate files are replaced by in-memory stages inside one binary (A1). Done when: `tests/convert.rs` loads Lucian. Passed 2026-11-14
- [x] Stage 2 rules: first-person-only meshes and eye-highlight shells (B3, B4). Done when: the manifest lists each part with its rule. Passed 2026-11-14 (Lucian has nothing to drop)
- [x] ~~Stage 2 interior removal~~: not in v1 (B5)
- [x] Weld + degenerate removal + neutral face + scale (B3, B7, B8, C2). Done when: height = target ±1 unit (`tests/convert.rs`). Passed 2026-11-14
- [x] Manifest override via `--manifest` (B4). Done when: flipping one entry changes the output. Passed 2026-11-14 (dropping EAR_RINGS: 6,356 → 6,068 tris)
- [ ] Update `ARCHITECTURE.md` (B codes). Done when: sections B to J match the code

## Phase C: Reduce, atlas, bake

**Goal:** per-target reduced meshes with one or two baked atlases.
**Exit:** fixture passes G2 and G3 for the GoldSrc budget.

- [ ] Spike: QEC vs meshoptimizer on the fixture, error + seam integrity (C1). Done when: C1 has a comparison table
- [ ] Stage 3 error-capped decimation with seams, borders and weight boundaries locked (C2, C3, C4). Done when: test asserts error ≤ cap and seam edge count unchanged
- [x] Single texture: downscale to 512² + 256-colour median cut (C5, C6, D6). Done when: `tests/convert.rs` checks 512×512, flatshade. Passed 2026-11-14. Multi-texture atlasing stays open for the next avatar that needs it
- [ ] Report renders for G2/G3 (F1). Done when: `report.html` shows before/after + quantised preview
- [ ] Update `ARCHITECTURE.md` (C codes). Done when: sections B to J match the code

## Phase D: GoldSrc back end

**Goal:** fixture becomes a working HL1 (or A4 target) server playermodel.
**Exit:** model loads in HLAM and on a local listen server, playing the stock animations. **Met 2026-11-14** in Sven Co-op (A4) on a local listen server, via `svencoop_addon/` (HLAM not used, A14): menu and portrait, idle/run/crouch/jump/aim/death, height and facing, and `--sway` (ears and front hair swing, dress solid) all confirmed by the user, no issues.

- [x] Bone map humanoid → Bip01; fingers/toes/eyes merged; hair/ear/dress chains kept (D1, D2). Done when: test checks chains present, fingers and donor extras absent. Passed 2026-11-14
- [x] Retarget to the donor skeleton with Lucian's proportions + dominant-bone binding (D3, D4). Done when: preview sheet shows no torn joints. Passed 2026-11-14 (`out/review/lucian/preview.png`)
- [x] 8-bit palette quantise (D6). Masked materials are not handled yet: Lucian is opaque. Passed 2026-11-14
- [x] MDL written directly with donor sequences, hitboxes, controllers, attachments, thumbnail (A9, D5, D7, D8). Done when: re-read is identical. Passed 2026-11-14
- [x] Limit checks + output tree (D9, A11): bones, verts and normals per mesh, 4,080 tris per mesh, 20k tris, Sven 9k warning, D14 hitboxes, full portrait palette. Done when: `tests/convert.rs` passes. Passed 2026-11-14
- [x] Zero-error edge collapse with C3 locks (C1, C2, C3). Done when: unit tests show a flat grid reduced to its border with the same area and UVs, bone halves never merged, and folds and seams blocked. Passed 2026-11-14. Lucian: 0 of 3,340 vertices removable (already minimal); the report lists which rule kept each vertex
- [x] Spike: procedural sway for chain bones (D13, D15). Done when: D15 has a design with evidence. Passed 2026-11-14 (Option A chosen)
- [x] Optional chain sway, UniVRM spring port (D13, D15, D16). Done when: `--sway` adds sway within every sequence's 64 KB budget, body channels unchanged, re-read identical (`tests/convert.rs`), and frame strips show no tearing. Passed 2026-11-14: Lucian 10 bones, 192/192 sequences, +0.51 MB
- [x] G5 under A14: user reviewed `out/review/lucian/preview.png` and the thumbnail. Done when: user confirms. Confirmed 2026-11-14; in-game check passed the same day
- [x] Update `ARCHITECTURE.md` (D codes). Done when: sections B to J match the code. Passed 2026-11-14 (full rewrite to the standalone Rust design; Source back end marked planned)

### Hardening before "stable" (user, 2026-11-14): three more avatars

- [x] Miku (VRM, 65,737 tris, 191 bones, 4 images with alpha) converts: bone budget merges the deepest chain links (D2), one texture per (image, masked) with per-mesh normal runs (C5, D6), lossy QEM to the 20k default budget (C4, revised after the first in-game test showed a bald, broken face at 9k) with optimal placement, staged locks and layer clearance caps so skin can't move through the hair or over the eyes (C2, C8), deterministic output. Done when: `tests/convert.rs` `miku_converts` passes. Passed 2026-11-14
- [x] Miku in game (Sven, local listen server, `miku` and `miku_sway`). Done when: user confirms. Confirmed 2026-11-15 after the C4/C8 fixes (first try at 9k failed: bald, broken face)
- [x] Neru: FBX only, needs Phase G (B9, G1–G10 Decided) Converted and confirmed in game 2026-11-15
- [x] Third avatar: Gothic Doll turned out to be an outfit without its base doll (2026-11-15). Replaced by Kilerbomb's Poka Blondy and Blindy (FBX + package, G11–G13): both convert (`poka_blondy_converts`, `poka_blindy_converts`). Done when: user confirms both in game (confirmed 2026-11-15)

## Phase E: Source back end

**Goal:** fixture becomes a GMod (or A3 target) server playermodel addon.
**Exit:** addon loads on a local server and the model is selectable and animates.

- [ ] ValveBiped mapping + smooth-weight cap (E1, E2). Done when: max weights/vertex ≤ A11 value (test)
- [ ] Own DXT1/DXT5 VTF + VMT writer (E3). Done when: textures open in a VTF viewer / load in game
- [ ] Own Source MDL/VVD/VTX writer with `$includemodel` anims, addon layout + Lua registration (A1, E1, E4). Done when: our re-read matches and the addon folder matches the spec
- [ ] Manual G5 in game. Done when: user confirms
- [ ] Update `ARCHITECTURE.md` (E codes). Done when: sections B to J match the code

## Phase F: Batch and review

**Goal:** convert several avatars in one run, with gates collected into reports.
**Exit:** 3 avatars run end to end, each stopping only at the gates that need a look.

- [ ] Stage caching by hash + `--from` re-runs (F2). Done when: an unchanged re-run skips every stage
- [ ] `avatar.toml` defaults + overrides (F3). Done when: per-avatar budget override reaches stage 3
- [ ] Remote execution hook for heavy runs (A8). Done when: a batch can be sent to Kaggle/GPU server and results pulled back
- [ ] Update `ARCHITECTURE.md` (F codes). Done when: sections B to J match the code

## Phase G: FBX + Unity package input

**Goal:** Neru (and Gothic Doll's outfit, as a decimation stress case) convert from FBX plus their `.unitypackage` (B9).
**Exit:** Neru converts and passes the same checks as `miku_converts`, and the user confirms it in game; Gothic Doll's outfit, which has no base avatar, is refused with a clear message (G17), which counts as passing.

- [x] Own binary FBX reader: nodes, mesh, UVs, normals, materials, skin clusters, bind pose, axes and units (B9, G6). Done when: a test reads both samples with the expected mesh/bone/poly counts Passed 2026-11-15 (`neru_converts`, `gothic_reads`)
- [x] Unitypackage reader: GUID → path, `.mat` colour/texture, prefab renderer materials, textures by name (B9, G2, G4). Done when: a test lists Gothic's materials per mesh Passed 2026-11-15 (`gothic_reads`)
- [x] Bone-name humanoid mapping and colour swatch texture (G1, G3). Done when: both samples map every required bone (test) Passed 2026-11-15 (Neru via Unity's map, G10; Gothic by name, 19 bones)
- [x] PhysBone → sway (G5, D15). Done when: Gothic `--sway` reports swaying strands per the strand rule Passed 2026-11-15 (8 ribbon bones sway; 12 joined skirt strands static)
- [x] `convert` takes FBX + package + `--licence` (G7). Done when: `neru_converts` passes and Gothic's outfit reaches the 20k budget (C9) Passed 2026-11-15 (Gothic 184,598 → 20,000 in ~3.5 min, G8)
- [x] Neru in game. Done when: user confirms Confirmed 2026-11-15
- [x] Update `ARCHITECTURE.md` (G codes). Done when: sections B to J match the code. Passed 2026-11-15 (rewritten for Phase G and the C4/C5/C8/C9/D2 revisions)

### Procedural follow-up (A15)

- [x] `convert <download.zip | folder | .vrm | .fbx>`: rank the avatar files and packages inside, report the choice and runners-up (G16). Done when: Poka Blindy and Blondy, Neru and Miku convert from their downloads with no flags (all six zips and sample folders, 2026-11-15; `downloads_choose`)
- [x] Licence discovery (G15). Done when: Poka and Miku get their licence text with no `--licence`; Neru warns, Gothic is refused by G17 (check reworded with the user, 2026-11-15)
- [x] Outfit-only detection (G17). Done when: Gothic stops with a clear message; the five avatars pass (head cover 1.00 for all five vs 0.05 for Gothic)
- [x] Automatic material pairing (G14, method in G19). Done when: Blindy converts with no `--material` and picks `bodyo.png`; Blondy and Neru unchanged (all three byte-identical to the installed builds, 2026-11-15)
- [x] Spike: Unity fileID hash (G18). Done when: G18 has evidence either way (XXH64 of `Type:<Class>->//RootNode/root/<path>0`: Neru 36/37, Blondy 36/46, Blindy 13/16; misses explained)
- [x] Resolve hashed fileIDs (G18 option a). Done when: Neru's 23 material overrides and 2 PhysBones apply; Poka's overrides are held back (their prefabs belong to another FBX, G11) and Blindy keeps `bodyo.png` (`neru_converts`, `poka_*_converts`)
- [x] Hold-out check on three unseen downloads (Saji, Tsumire, Lilyanne; 2026-11-15). Fixed general faults it found: avatar files inside packages (G16), a mesh named like a bone taking its role, Unity's material slot order for prefab overrides (first use, G18), mobile builds by path, single-file inputs scanning two folders up, licence search inside packages (G15). All three convert with no flags. New questions: G20, G21
- [x] Donor chosen automatically when `--donor` is absent (G22): under 1.4 m real-scale height → chibi donor `samples/yuki_ref`, else Sven's stock `gina` from the Steam install. Check: Lilyanne converts at 75.72 units on gina; neru/blondy/blindy stay on yuki; `downloads_choose` asserts the split.
- [x] Small separate pieces kept whole by decimation (G23): pieces under 64 triangles, up to a quarter of the budget. Check: Lilyanne's irises survive (100 iris strip vertices, was 0); unit test `small_separate_pieces_stay_whole`.
- [x] Smooth repose to the donor's rest pose before rigid binding (G24): skin weights kept per vertex by both readers. Check: Lilyanne's arms hang beside her on gina (were a flat wedge inside the dress); Neru on yuki unchanged; `neru_converts` asserts the weights.
- [x] Torso kept as authored (G25): no alignment on pelvis→head. Check: Saji and Lilyanne stand upright like gina's own idle (were bowed and hunched).
- [x] `--keep-polygons` (G26): skips lossy decimation past the 20k cap with a warning. Check: Lilyanne keeps all 39,017 triangles in 11 bodyparts and re-reads identically; `neru_converts` asserts it.
- [x] Fingers curled into a grip during the repose (G27). Check: Sven's `p_9mmhandgun.mdl` bone-merged onto Lilyanne in `ref_aim_onehanded` sits in a fist (was at the wrist past open fingers); `neru_converts` asserts 30 curled joints.

## Phase H: Desktop GUI

Starts once H1–H6 are Decided (A7). Tasks are written when they are.

- [x] Decide H1–H6 (2026-11-20).
- [x] Pipeline in the library: move the convert steps (open, choose, donor, convert, sway, write, re-read, preview) from `main.rs` into `src/pipeline.rs`, used by the CLI and the GUI (H6). Done when: CLI output is byte-identical for neru and all tests pass. Passed 2026-11-20: neru and lilyanne_sway `.mdl`/`.bmp` identical
- [x] Workspace + Tauri 2/Svelte scaffold in `gui/` (H1, H6). Done when: `cargo build -p libremodele-gui` and the front-end build pass. Passed 2026-11-21
- [x] Backend commands: inspect a download (choices, licence, donor, warnings), convert on a worker thread with progress events, install (H2, H3, H5). Done when: a Rust test drives inspect → convert → install into a temp addon folder. Passed 2026-11-21 (`inspect_convert_install`)
- [x] Svelte UI: drop/pick, choices panel, main controls + Advanced, convert with progress, report (H2, H3). Done when: the user converts and installs an avatar from the window. Passed 2026-11-21: user converted, installed and called it ready for release
- [x] 3D viewer: meshes and textures from the converted model, bone transforms per frame, turntable and sequence picker (H4). Done when: the user sees idle and a weapon sequence play in the window. Passed 2026-11-21 (user)
- [x] Licence warning with a tick-box before install (H5, A6, A13). Done when: install is disabled until ticked for a non-redistributable licence. Passed 2026-11-21: `install` refuses without the tick (`inspect_convert_install`); button disabled in the UI
- [x] Update `ARCHITECTURE.md` (H codes). Done when: sections B to J match the code. Passed 2026-11-21

## Phase I: Release builds

Decided in I1–I4.

- [x] Decide I1–I4 (2026-11-21).
- [ ] Release workflow `.github/workflows/release.yml`: checks, then Windows and Debian builds, then a GitHub Release for tags (I1–I4). Done when: a manual run passes and produces the `.exe`, `.msi`, `.deb` and both CLI archives.
- [ ] First tagged release. Done when: pushing `v0.1.0` publishes a release with all five files, and the user installs the Windows or Debian build.

## Cycle: feature and maintenance

Candidate future features (each one a Question row first): outfit toggles as bodygroups (B6), Source flex visemes (B7), Sven Co-op high-limit profile (A4), FastDLX upload integration (E4).

**Maintenance turn** (per release)
- [ ] Tool versions reviewed: Blender, AssetRipper, meshoptimizer, studiomdl (A7, B1, C1, A9). Done when: fixture runs end to end on the new versions
- [ ] Docs match reality. Done when: no Decided row contradicts the code or `ARCHITECTURE.md`
- [ ] Stale rows closed or reopened. Done when: no TBD or Inspect row older than one cycle
