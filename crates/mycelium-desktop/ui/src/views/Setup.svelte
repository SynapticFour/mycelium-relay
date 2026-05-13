<script>
  import { createEventDispatcher } from "svelte";
  import { invoke } from "@tauri-apps/api/core";

  const dispatch = createEventDispatcher();

  let displayName = $state("");
  let customBootstrap = $state("");

  async function submit() {
    if (!displayName.trim()) return;
    const dbPath = await invoke("get_default_db_path");
    const bootstrapPeers = customBootstrap
      .split("\n")
      .map((s) => s.trim())
      .filter(Boolean);
    dispatch("start", { dbPath, displayName, bootstrapPeers });
  }
</script>

<div class="setup">
  <h1>Mycelium</h1>
  <p class="tagline">Offline mesh network – works without internet</p>

  <label>
    Display name
    <input bind:value={displayName} placeholder="Your name in the network" />
  </label>

  <details>
    <summary>Advanced: custom bootstrap peers</summary>
    <textarea
      bind:value={customBootstrap}
      placeholder="/dns4/mycelium-relay.fly.dev/tcp/4001/p2p/12D3KooW..."
      rows="3"
    ></textarea>
    <small>Leave empty to use the default relay (when deployed) or MYCELIUM_BOOTSTRAP_PEERS / bootstrap.txt.</small>
  </details>

  <button onclick={submit} disabled={!displayName.trim()}>Start node</button>
</div>

<style>
  .setup {
    max-width: 640px;
    margin: 40px auto;
    border: 1px solid var(--border);
    border-radius: 12px;
    padding: 20px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .tagline {
    color: var(--text-muted);
    font-size: 14px;
    margin: -4px 0 8px;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 14px;
  }
  input,
  textarea {
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 8px;
    background: none;
    color: inherit;
  }
  textarea {
    min-height: 72px;
    font-family: ui-monospace, monospace;
    font-size: 12px;
  }
  details {
    font-size: 13px;
    border: 1px dashed var(--border);
    border-radius: 8px;
    padding: 8px 10px;
  }
  summary {
    cursor: pointer;
    font-weight: 500;
  }
  small {
    display: block;
    color: var(--text-muted);
    margin-top: 6px;
    line-height: 1.4;
  }
  button {
    margin-top: 8px;
    padding: 10px 14px;
    border: none;
    border-radius: 8px;
    background: var(--accent);
    color: #fff;
    cursor: pointer;
    font-weight: 600;
  }
  button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
