<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  let version = $state("");
  invoke<string>("version").then((v) => (version = v));
</script>

<main>
  <h1>libremodele</h1>
  <p>{version}</p>
</main>
