<script lang="ts">
  // Live 3D preview of the converted model playing its (donor's) sequences (H4).
  import { onMount, untrack } from "svelte";
  import * as THREE from "three";
  import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
  import * as api from "./api";
  import { build, setLocal, type Rig } from "./rig";

  /** Bumped by the parent after each conversion to reload. */
  let { generation }: { generation: number } = $props();

  let canvas: HTMLCanvasElement;
  let box: HTMLDivElement;
  let rig = $state.raw<Rig | null>(null);
  let gun: Rig | null = null;
  let error = $state("");
  let loading = $state(false);

  // Playback state.
  let seq = $state(0);
  let gait = $state(-1); // -1: legs follow the main sequence
  let blend = $state(0);
  let playing = $state(true);
  let speed = $state(1);
  let frame = $state(0);
  let showMesh = $state(true);
  let showBones = $state(false);
  let showHitboxes = $state(false);
  let turntable = $state(false);
  let weaponList = $state<string[]>([]);
  let weapon = $state("");

  let upper: Float32Array | null = null;
  let legs: Float32Array | null = null;
  let legsFrames = 1;
  let legsFps = 30;
  let clock = 0;
  let legsClock = 0;

  const root = new THREE.Group();
  root.rotation.x = -Math.PI / 2; // GoldSrc is z-up; three.js is y-up
  const scene = new THREE.Scene();
  let skeletonHelper: THREE.SkeletonHelper | null = null;

  let sequences = $derived(rig?.view.sequences ?? []);
  let current = $derived(sequences[seq]);

  async function loadModel() {
    loading = true;
    error = "";
    try {
      dropWeapon();
      rig?.dispose();
      skeletonHelper?.removeFromParent();
      rig = await build(root, false);
      skeletonHelper = new THREE.SkeletonHelper(root);
      scene.add(skeletonHelper);
      const idle = rig.view.sequences.findIndex((s) => s.label === "idle");
      seq = idle >= 0 ? idle : 0;
      gait = -1;
      await loadSequence();
      if (weapon) await setWeapon(weapon);
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  async function loadSequence() {
    if (!rig) return;
    const s = rig.view.sequences[seq];
    blend = Math.min(blend, s.blends - 1);
    upper = await api.boneFrames(false, seq, blend);
    clock = 0;
    await loadGait();
  }

  async function loadGait() {
    if (!rig || gait < 0) {
      legs = null;
      return;
    }
    const g = rig.view.sequences[gait];
    legs = await api.boneFrames(false, gait, 0);
    legsFrames = g.frames;
    legsFps = Math.max(g.fps, 1);
    legsClock = 0;
  }

  function dropWeapon() {
    gun?.dispose();
    gun = null;
  }

  async function setWeapon(file: string) {
    dropWeapon();
    if (!rig || !file) {
      await api.loadWeapon(null);
      return;
    }
    try {
      await api.loadWeapon(file);
      gun = await build(root, true, rig);
      gun.hitboxes.forEach((h) => (h.visible = false));
    } catch (e) {
      error = String(e);
    }
  }

  function pose() {
    if (!rig || !upper) return;
    const n = rig.bones.length;
    const s = rig.view.sequences[seq];
    const f = Math.min(frame, s.frames - 1);
    for (let i = 0; i < n; i++) {
      // Sven's player renderer: bones before "Bip01 Spine" follow the gait.
      if (legs && i < rig.view.gaitSplit) {
        const lf = Math.floor(legsClock * legsFps) % legsFrames;
        setLocal(rig.bones[i], legs, i, lf * n * 12);
      } else {
        setLocal(rig.bones[i], upper, i, f * n * 12);
      }
    }
  }

  onMount(() => {
    const renderer = new THREE.WebGLRenderer({ canvas, antialias: true });
    renderer.setPixelRatio(window.devicePixelRatio);
    const dark = window.matchMedia("(prefers-color-scheme: dark)").matches;
    scene.background = new THREE.Color(dark ? 0x1b1e23 : 0xe9ecf0);
    scene.add(new THREE.HemisphereLight(0xffffff, 0x555566, 1.6));
    const sun = new THREE.DirectionalLight(0xffffff, 1.6);
    sun.position.set(120, 200, 160);
    scene.add(sun);
    scene.add(root);
    // Player origin is the hull centre; feet are at z = -36.
    const grid = new THREE.GridHelper(256, 16, 0x7a8290, dark ? 0x30343b : 0xc8cdd4);
    grid.position.y = -36;
    scene.add(grid);

    const camera = new THREE.PerspectiveCamera(40, 1, 1, 4000);
    camera.position.set(110, 20, 60);
    const controls = new OrbitControls(camera, canvas);
    controls.target.set(0, 0, 0);
    controls.enableDamping = true;
    controls.update();

    const resize = () => {
      const w = box.clientWidth;
      const h = box.clientHeight;
      renderer.setSize(w, h, false);
      camera.aspect = w / Math.max(h, 1);
      camera.updateProjectionMatrix();
    };
    const ro = new ResizeObserver(resize);
    ro.observe(box);
    resize();

    let last = performance.now();
    let raf = 0;
    const tick = (now: number) => {
      const dt = Math.min((now - last) / 1000, 0.1);
      last = now;
      if (rig && current) {
        if (playing) {
          clock += dt * speed;
          legsClock += dt * speed;
          const fps = Math.max(current.fps, 1);
          frame = Math.floor(clock * fps) % current.frames;
        }
        pose();
        rig.meshes.forEach((m) => (m.visible = showMesh));
        rig.hitboxes.forEach((h) => (h.visible = showHitboxes));
        if (skeletonHelper) skeletonHelper.visible = showBones;
      }
      controls.autoRotate = turntable;
      controls.update();
      renderer.render(scene, camera);
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    api.weapons().then((w) => (weaponList = w));

    return () => {
      cancelAnimationFrame(raf);
      ro.disconnect();
      controls.dispose();
      dropWeapon();
      rig?.dispose();
      renderer.dispose();
    };
  });

  // Reload after each conversion.
  $effect(() => {
    if (generation > 0) untrack(() => loadModel());
  });

  function scrub(e: Event) {
    playing = false;
    frame = Number((e.target as HTMLInputElement).value);
    clock = frame / Math.max(current?.fps ?? 30, 1);
  }
</script>

<div class="viewer">
  <div class="stage" bind:this={box}>
    <canvas bind:this={canvas}></canvas>
    {#if loading}<div class="overlay">Loading model…</div>{/if}
    {#if error}<div class="overlay err">{error}</div>{/if}
    <div class="hint">Drag to orbit · scroll to zoom · right-drag to pan</div>
  </div>

  {#if rig}
    <div class="controls">
      <label>
        Sequence
        <select bind:value={seq} onchange={loadSequence}>
          {#each sequences as s, i}
            <option value={i}>{s.label} ({s.frames} f{s.blends > 1 ? `, ${s.blends} blends` : ""})</option>
          {/each}
        </select>
      </label>
      <label>
        Legs (gait)
        <select bind:value={gait} onchange={loadGait}>
          <option value={-1}>same as sequence</option>
          {#each sequences as s, i}
            <option value={i}>{s.label}</option>
          {/each}
        </select>
      </label>
      {#if current && current.blends > 1}
        <label>
          Blend (aim pitch) {blend + 1}/{current.blends}
          <input type="range" min="0" max={current.blends - 1} bind:value={blend} onchange={loadSequence} />
        </label>
      {/if}
      <label>
        Weapon
        <select bind:value={weapon} onchange={() => setWeapon(weapon)}>
          <option value="">none</option>
          {#each weaponList as w}
            <option value={w}>{w.replace(/^p_|\.mdl$/g, "")}</option>
          {/each}
        </select>
      </label>

      <div class="transport">
        <button onclick={() => (playing = !playing)}>{playing ? "Pause" : "Play"}</button>
        <input
          type="range"
          min="0"
          max={(current?.frames ?? 1) - 1}
          value={frame}
          oninput={scrub}
        />
        <span class="muted">{frame + 1}/{current?.frames ?? 1}</span>
        <select bind:value={speed}>
          <option value={0.25}>¼×</option>
          <option value={0.5}>½×</option>
          <option value={1}>1×</option>
          <option value={2}>2×</option>
        </select>
      </div>

      <div class="toggles">
        <label class="check"><input type="checkbox" bind:checked={showMesh} /> Mesh</label>
        <label class="check"><input type="checkbox" bind:checked={showBones} /> Bones</label>
        <label class="check"><input type="checkbox" bind:checked={showHitboxes} /> Hitboxes</label>
        <label class="check"><input type="checkbox" bind:checked={turntable} /> Turntable</label>
      </div>
    </div>
  {/if}
</div>

<style>
  .viewer {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .stage {
    position: relative;
    height: 62vh;
    min-height: 360px;
    border-radius: 8px;
    overflow: hidden;
    border: 1px solid var(--line);
  }
  canvas {
    display: block;
    width: 100%;
    height: 100%;
  }
  .overlay {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    background: color-mix(in srgb, var(--bg) 70%, transparent);
  }
  .overlay.err {
    color: var(--error);
    padding: 16px;
    white-space: pre-wrap;
  }
  .hint {
    position: absolute;
    left: 10px;
    bottom: 8px;
    font-size: 12px;
    color: var(--muted);
    pointer-events: none;
  }
  .controls {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
    gap: 10px;
    align-items: end;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    color: var(--muted);
    font-size: 13px;
  }
  label.check {
    flex-direction: row;
    align-items: center;
    color: var(--text);
  }
  select {
    font: inherit;
    color: var(--text);
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: 6px;
    padding: 5px 6px;
  }
  .transport {
    grid-column: 1 / -1;
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .transport input {
    flex: 1;
  }
  .transport button {
    font: inherit;
    color: var(--text);
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: 6px;
    padding: 5px 12px;
    cursor: pointer;
    min-width: 70px;
  }
  .toggles {
    grid-column: 1 / -1;
    display: flex;
    gap: 16px;
    flex-wrap: wrap;
  }
  .muted {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
</style>
