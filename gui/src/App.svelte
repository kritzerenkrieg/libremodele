<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { open } from "@tauri-apps/plugin-dialog";
  import * as api from "./api";
  import Viewer from "./Viewer.svelte";
  import type { Outcome, Summary } from "./api";

  let form = $state(api.emptyForm());
  let materialsText = $state("");
  let summary = $state<Summary | null>(null);
  let outcome = $state<Outcome | null>(null);
  let busy = $state<"" | "inspect" | "convert" | "install">("");
  let stage = $state("");
  let error = $state("");
  let keepLocal = $state(false);
  let installed = $state("");
  let dragging = $state(false);
  let version = $state("");
  let generation = $state(0);

  const isWarning = (l: string) => l.startsWith("warning");
  const fileName = (p: string) => p.split(/[\\/]/).pop() ?? p;

  onMount(() => {
    api.version().then((v) => (version = v));
    const unlisten = [
      listen<string>("progress", (e) => (stage = e.payload)),
      // H2: drop a download anywhere on the window.
      getCurrentWebview().onDragDropEvent((e) => {
        const p = e.payload;
        if (p.type === "enter" || p.type === "over") dragging = true;
        else if (p.type === "leave") dragging = false;
        else if (p.type === "drop") {
          dragging = false;
          if (p.paths.length) choose(p.paths[0]);
        }
      }),
    ];
    return () => unlisten.forEach((u) => u.then((f) => f()));
  });

  async function pick(directory: boolean) {
    const p = await open({
      directory,
      multiple: false,
      filters: directory
        ? undefined
        : [{ name: "Avatar download", extensions: ["zip", "rar", "vrm", "fbx"] }],
    });
    if (typeof p === "string") choose(p);
  }

  async function pickDonor() {
    const p = await open({ multiple: false, filters: [{ name: "GoldSrc model", extensions: ["mdl"] }] });
    if (typeof p === "string") form.donor = p;
  }

  function choose(path: string) {
    // A drop while reading or converting would swap the form mid-run.
    if (busy) return;
    // A new download starts clean: its own name, licence and overrides.
    form = { ...api.emptyForm(), input: path, sway: form.sway, keepPolygons: form.keepPolygons };
    materialsText = "";
    runInspect();
  }

  function materials(): [string, string][] {
    return materialsText
      .split("\n")
      .map((l) => l.split("="))
      .filter((p) => p.length === 2 && p[0].trim() && p[1].trim())
      .map(([k, v]) => [k.trim(), v.trim()]);
  }

  async function run<T>(what: typeof busy, f: () => Promise<T>): Promise<T | undefined> {
    busy = what;
    error = "";
    try {
      return await f();
    } catch (e) {
      error = String(e);
    } finally {
      busy = "";
      stage = "";
    }
  }

  async function runInspect() {
    summary = null;
    outcome = null;
    installed = "";
    const s = await run("inspect", () => api.inspect({ ...form, materials: materials() }));
    if (s) {
      summary = s;
      if (!form.name) form.name = s.name;
    }
  }

  async function runConvert() {
    outcome = null;
    installed = "";
    keepLocal = false;
    const o = await run("convert", () => api.convert({ ...form, materials: materials() }));
    if (o) {
      outcome = o;
      generation++;
    }
  }

  async function runInstall() {
    const p = await run("install", () => api.install(keepLocal));
    if (p) installed = p;
  }

  // H5: a restricted licence needs the tick before installing.
  let canInstall = $derived(
    !!outcome && !!summary?.svenAddon && (!outcome.restricted || keepLocal) && !busy,
  );
</script>

<div class="app" class:dragging>
  <aside>
    <header>
      <h1>libremodele</h1>
      <span class="muted">{version}</span>
    </header>

    <section class="drop">
      {#if form.input}
        <div class="input" title={form.input}>{fileName(form.input)}</div>
      {:else}
        <p class="muted">Drop a download here (zip, rar, folder, .vrm, .fbx)</p>
      {/if}
      <div class="row">
        <button onclick={() => pick(false)} disabled={!!busy}>Choose file…</button>
        <button onclick={() => pick(true)} disabled={!!busy}>Choose folder…</button>
      </div>
    </section>

    <section>
      <label>Model name <input bind:value={form.name} placeholder="from the download" /></label>
      <label>
        Donor (skeleton, animations, hitboxes)
        <div class="row">
          <input bind:value={form.donor} placeholder="automatic (G22)" />
          <button onclick={pickDonor} disabled={!!busy}>…</button>
        </div>
      </label>
      <label class="check"><input type="checkbox" bind:checked={form.sway} /> Hair and skirt sway</label>
      <label class="check">
        <input type="checkbox" bind:checked={form.keepPolygons} /> Keep all polygons (may exceed 20k)
      </label>

      <details>
        <summary>Advanced</summary>
        <label>Height (units) <input type="number" bind:value={form.height} placeholder="donor's" /></label>
        <label>Max triangles <input type="number" bind:value={form.maxTris} placeholder="20000" /></label>
        <label>Avatar file in the download <input bind:value={form.avatar} placeholder="automatic" /></label>
        <label>Unity package <input bind:value={form.package} placeholder="automatic" /></label>
        <label>Prefab variant <input bind:value={form.variant} placeholder="automatic" /></label>
        <label>Licence text <input bind:value={form.licence} placeholder="found in the download" /></label>
        <label>
          Materials, one per line: <code>FBX material = .mat or image</code>
          <textarea rows="3" bind:value={materialsText}></textarea>
        </label>
        <button onclick={runInspect} disabled={!form.input || !!busy}>Re-read the download</button>
      </details>
    </section>

    <section class="actions">
      <button class="primary" onclick={runConvert} disabled={!summary || !!busy}>
        {busy === "convert" ? "Converting…" : "Convert"}
      </button>
      {#if busy}
        <div class="progress"><span class="spinner"></span>{busy === "inspect" ? "reading the download" : stage || busy}</div>
      {/if}
    </section>
  </aside>

  <main>
    {#if error}
      <div class="error">{error}</div>
    {/if}

    {#if summary}
      <section class="card">
        <h2>{summary.title || summary.name}</h2>
        <dl>
          <dt>Avatar file</dt><dd>{summary.avatarFile}</dd>
          <dt>Height</dt><dd>{summary.heightM.toFixed(2)} m, {summary.parts} parts</dd>
          <dt>Donor</dt><dd>{fileName(summary.donor)}{summary.donorWhy ? ` (${summary.donorWhy})` : ""}</dd>
          <dt>Licence</dt>
          <dd class:warn={summary.restricted}>{summary.licence || "none found"}</dd>
        </dl>
        {#each summary.notes as n}
          <p class:warn={isWarning(n)} class="note">{n}</p>
        {/each}
      </section>
    {:else if !busy}
      <section class="empty muted">Pick or drop an avatar download to begin.</section>
    {/if}

    {#if outcome}
      <section class="card">
        <h2>{outcome.name}</h2>
        <Viewer {generation} />
        <p>{outcome.triangles.toLocaleString()} triangles, {(outcome.bytes / 1e6).toFixed(2)} MB</p>

        <div class="install">
          {#if outcome.restricted}
            <p class="warn">
              This licence doesn't clearly allow redistribution. A server playermodel is downloaded by every
              player, so keep this model on your own machine (A6).
            </p>
            <label class="check">
              <input type="checkbox" bind:checked={keepLocal} /> I'll keep this model local
            </label>
          {/if}
          {#if summary?.svenAddon}
            <button class="primary" onclick={runInstall} disabled={!canInstall}>Install into Sven Co-op</button>
            <span class="muted path">{summary.svenAddon}</span>
          {:else}
            <p class="muted">Sven Co-op wasn't found in the Steam libraries; copy from {outcome.game}.</p>
          {/if}
          {#if installed}
            <p class="ok">Installed to {installed}. In game: <code>model {outcome.name}</code></p>
          {/if}
        </div>

        <details open>
          <summary>Report</summary>
          <pre>{#each outcome.report as l}<span class:warn={isWarning(l)}>{l}</span>{"\n"}{/each}</pre>
        </details>
        <p class="muted path">Files: {outcome.game}</p>
      </section>
    {/if}
  </main>
</div>

<style>
  :global(:root) {
    --bg: #f4f5f7;
    --panel: #ffffff;
    --text: #1d2127;
    --muted: #68707c;
    --line: #d9dde3;
    --accent: #2f6fdf;
    --warn: #b25b00;
    --error: #c0392b;
    --ok: #1e8a4c;
    color-scheme: light dark;
  }
  @media (prefers-color-scheme: dark) {
    :global(:root) {
      --bg: #17191d;
      --panel: #202328;
      --text: #e6e8eb;
      --muted: #9aa1ab;
      --line: #343840;
      --accent: #5b93f5;
      --warn: #f0a24b;
      --error: #ef6f61;
      --ok: #4fc483;
    }
  }
  :global(body) {
    margin: 0;
    background: var(--bg);
    color: var(--text);
    font: 14px/1.45 system-ui, sans-serif;
  }
  .app {
    display: grid;
    grid-template-columns: 320px 1fr;
    height: 100vh;
  }
  .app.dragging {
    outline: 3px dashed var(--accent);
    outline-offset: -6px;
  }
  aside {
    background: var(--panel);
    border-right: 1px solid var(--line);
    padding: 16px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  main {
    padding: 16px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  header {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
  }
  h1 {
    font-size: 18px;
    margin: 0;
  }
  h2 {
    font-size: 16px;
    margin: 0 0 8px;
  }
  section {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .drop {
    border: 1px dashed var(--line);
    border-radius: 8px;
    padding: 12px;
  }
  .input {
    font-weight: 600;
    word-break: break-all;
  }
  .row {
    display: flex;
    gap: 6px;
  }
  .row input {
    flex: 1;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    color: var(--muted);
  }
  label.check {
    flex-direction: row;
    align-items: center;
    color: var(--text);
  }
  input:not([type="checkbox"]),
  textarea {
    font: inherit;
    color: var(--text);
    background: var(--bg);
    border: 1px solid var(--line);
    border-radius: 6px;
    padding: 6px 8px;
  }
  button {
    font: inherit;
    color: var(--text);
    background: var(--bg);
    border: 1px solid var(--line);
    border-radius: 6px;
    padding: 6px 12px;
    cursor: pointer;
  }
  button.primary {
    background: var(--accent);
    border-color: var(--accent);
    color: #fff;
    font-weight: 600;
  }
  button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  details {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  details > :global(*:not(summary)) {
    margin-top: 10px;
  }
  summary {
    cursor: pointer;
    font-weight: 600;
  }
  .card {
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: 8px;
    padding: 16px;
    gap: 6px;
  }
  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 4px 12px;
    margin: 0;
  }
  dt {
    color: var(--muted);
  }
  dd {
    margin: 0;
    word-break: break-word;
  }
  .note {
    margin: 0;
    font-size: 13px;
  }
  .install {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 8px;
    padding: 8px 0;
  }
  pre {
    font: 12px/1.5 ui-monospace, monospace;
    white-space: pre-wrap;
    background: var(--bg);
    border-radius: 6px;
    padding: 10px;
    max-height: 360px;
    overflow: auto;
  }
  .muted {
    color: var(--muted);
  }
  .path {
    font-size: 12px;
    word-break: break-all;
  }
  .warn {
    color: var(--warn);
  }
  .ok {
    color: var(--ok);
  }
  .error {
    color: var(--error);
    border: 1px solid var(--error);
    border-radius: 8px;
    padding: 10px 12px;
    white-space: pre-wrap;
  }
  .empty {
    margin: auto;
  }
  .progress {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--muted);
  }
  .spinner {
    width: 12px;
    height: 12px;
    border: 2px solid var(--line);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
