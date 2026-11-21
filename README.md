# libremodele

Convert VRChat avatars into **Sven Co-op playermodels** (GoldSrc MDL v10).

Give it a download as it came (a `.zip`, `.rar`, folder, `.vrm`, or `.fbx` with its
Unity package) and it chooses everything itself:

- the avatar file, package, prefab variant and licence text;
- textures, including ones the FBX doesn't reference;
- a donor skeleton for the animations and hitboxes: Sven's stock `gina`, or a chibi
  donor for short avatars.

It then retargets the mesh onto the donor's `Bip01` skeleton, keeping the avatar's
proportions, and reduces it to the engine's limits. It writes the `.mdl` and the
player-menu picture with its own MDL writer. It needs no Blender, studiomdl, Unity or
other outside tools.

It comes as a command-line tool and as a desktop app with a live 3D preview that plays
the model's Sven animations.

## Features

- **Input:** VRM 0.x and binary FBX 7.x. Unity packages supply materials, cutout,
  humanoid maps, prefab overrides and PhysBones. Reads zip and RAR archives directly.
- **Retargeting:** the mesh is posed smoothly into the donor's rest pose before rigid
  binding. The torso keeps its authored posture, and the fingers curl into a grip so
  weapons sit in a fist.
- **Geometry:** lossless reduction first, then quadric decimation down to the 20k
  triangle cap. Small separate pieces such as irises and earrings stay whole.
  `--keep-polygons` skips decimation altogether.
- **Textures:** downscaled to 512² (Sven allows 1024², but this converter keeps
  512², D6), with a 256-colour palette and masked transparency.
- **Optional sway:** hair and skirt bones simulated from the avatar's spring bones, baked
  into each sequence.
- **Checks:** every written model is re-read and must match exactly. Each conversion also
  writes a preview sheet and a report of every automatic choice.

## Desktop app

```
cd gui
npm install
npm run tauri dev
```

1. Drop or pick a download. The app shows what it chose: the avatar file, height, donor
   and licence.
2. Adjust the name, donor, sway or polygon options. The rest are under **Advanced**.
3. Click **Convert**, then check the model in the 3D viewer:
   - every sequence, with separate leg (gait) and upper-body sequences as in game;
   - aim blends;
   - any `p_*.mdl` weapon from your Sven install;
   - the skeleton and hitboxes.
4. Click **Install into Sven Co-op** to copy the model to
   `svencoop_addon/models/player/`. In game, type `model <name>` in the console.

On Linux, Tauri needs the WebKitGTK 4.1, GTK 3 and libsoup 3 development packages. See
[Tauri's prerequisites](https://v2.tauri.app/start/prerequisites/).

## Command line

```
cargo run --release -- convert <download.zip | .rar | folder | .vrm | .fbx> [--sway]
```

Outputs go to `out/goldsrc/models/player/<name>/` and `out/review/<name>/`. Every flag
is an optional override:

```
--name <name>  --donor <donor.mdl>  --avatar <file in input>  --package <x.unitypackage>
--variant <prefab>  --licence <text>  --material "<FBX material>=<.mat or image>"
--height <units>  --max-tris <n>  --keep-polygons  --manifest <clean_manifest.json>  --out <dir>
```

Other commands: `preview <file.mdl> <out.png>`, `mdl-info <file.mdl>`,
`mdl-roundtrip <in.mdl> <out.mdl>`.

## Requirements

- Rust 1.88 or newer (edition 2024, let-chains).
- Node.js 20 or newer, for the desktop app only.
- Sven Co-op installed through Steam, for the default donor (`gina`) and one-click
  install. Without it, give a donor: `--donor <any Sven player .mdl>` on the command
  line, or the **Donor** field in the app.
- Short (chibi) avatars, under 1.4 m, use the built-in chibi donor, so they don't need
  Sven's files.

## Avatar licences

Most avatars are sold or shared under terms that forbid redistribution. A server
playermodel is downloaded by every player who joins, so **using a converted model on a
public server counts as redistribution**.

libremodele reads the licence that ships with the avatar and warns when it doesn't
clearly allow redistribution. The desktop app also asks you to confirm before installing
such a model. You are responsible for following each avatar's terms. No avatars are
included in this repository.

## Development

```
cargo test                    # sample-based tests skip without the git-ignored samples/
cargo clippy --workspace --all-targets
cargo fmt --all
cd gui && npm run check       # type-checks the Svelte front end (svelte-check)
```

The project follows [DLAD](https://github.com/kritzerenkrieg/dlad):

- `DECISIONS.md` records every design decision by code (A1, G24, H4 and so on).
- `ARCHITECTURE.md` describes the design.
- `ROADMAP.md` tracks the plan.

## Licence

The code is MIT; see [LICENSE](LICENSE).

The built-in chibi donor, `donors/yuki_muumuyu.mdl`, is not covered by the MIT
licence. Its model is by **Yuki** and its Sven Co-op animations are by **TapChan**,
and it is included with their permission. It's there to give short avatars a
skeleton and animations that fit them. Don't redistribute it separately.
