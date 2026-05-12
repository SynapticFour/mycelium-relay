<script>
  import { createEventDispatcher } from "svelte";
  const dispatch = createEventDispatcher();
  let dbPath = $state(".mycelium-desktop");
  let displayName = $state("desktop-user");
  let bootstrap = $state("");

  function submit() {
    dispatch("start", {
      dbPath,
      displayName,
      bootstrapPeers: bootstrap
        .split(",")
        .map((x) => x.trim())
        .filter(Boolean),
    });
  }
</script>

<div class="setup">
  <h2>Start Mycelium Node</h2>
  <label>Database path</label>
  <input bind:value={dbPath} />
  <label>Display name</label>
  <input bind:value={displayName} />
  <label>Bootstrap peers (comma separated)</label>
  <textarea bind:value={bootstrap}></textarea>
  <button onclick={submit}>Start Node</button>
</div>

<style>
  .setup { max-width: 640px; margin: 40px auto; border: 1px solid var(--border); border-radius: 12px; padding: 16px; display: flex; flex-direction: column; gap: 8px; }
  input, textarea { border: 1px solid var(--border); border-radius: 6px; padding: 8px; background: none; color: inherit; }
  textarea { min-height: 100px; }
  button { margin-top: 8px; padding: 10px 12px; border: none; border-radius: 6px; background: var(--accent); color: #fff; cursor: pointer; }
</style>
