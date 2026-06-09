<script>
  import { invoke } from '@tauri-apps/api/core';
  import { getVersion } from '@tauri-apps/api/app';
  import { confirm } from '@tauri-apps/plugin-dialog';

  let { peerId } = $props();

  let displayName = $state('');
  let energyState = $state('Active');
  let bootstrapPeers = $state('');
  let storeStats = $state({ count: 0, oldest_ms: 0 });
  let gcResult = $state(null);
  let saved = $state(false);
  let appVersion = $state('');

  const energyOptions = ['Active', 'Intermittent', 'Passive'];

  async function load() {
    const settings = await invoke('get_settings');
    displayName = settings.display_name ?? '';
    energyState = settings.energy_state ?? 'Active';
    storeStats = await invoke('get_store_stats');
    const custom = await invoke('get_custom_bootstrap_peers');
    bootstrapPeers = custom.join('\n');
  }

  async function save() {
    await invoke('set_display_name', { name: displayName });
    await invoke('set_energy_state', { energyState });
    await invoke('set_custom_bootstrap_peers', {
      peers: bootstrapPeers
        .split('\n')
        .map(s => s.trim())
        .filter(Boolean)
    });
    saved = true;
    setTimeout(() => saved = false, 2000);
  }

  async function runGc() {
    gcResult = await invoke('run_gc');
  }

  async function panicWipe() {
    const confirmed = await confirm(
      'This will immediately delete ALL local messages, keys, and data. ' +
      'The app will restart. This cannot be undone.\n\nAre you sure?',
      { title: 'PANIC WIPE', kind: 'warning' }
    );
    if (confirmed) {
      await invoke('panic_wipe');
    }
  }

  $effect(() => {
    load();
    getVersion().then((v) => (appVersion = v));
  });
</script>

<div class="settings">
  <h3>Settings</h3>

  <section>
    <h4>Identity</h4>
    <p class="field-label">Peer ID</p>
    <code class="peer-id">{peerId}</code>
    <label for="settings-display-name">Display name</label>
    <input
      id="settings-display-name"
      bind:value={displayName}
      placeholder="Your name in the network"
    />
  </section>

  <section>
    <h4>Network</h4>
    <label>
      Energy mode
      <select bind:value={energyState}>
        {#each energyOptions as opt}
          <option value={opt}>{opt}</option>
        {/each}
      </select>
    </label>
    <label>
      Custom bootstrap peers
      <textarea
        bind:value={bootstrapPeers}
        placeholder="/dns4/mycelium-relay.fly.dev/tcp/4001/p2p/12D3KooW..."
        rows="3"
      ></textarea>
      <small>One per line. Leave empty to use default relay.</small>
    </label>
  </section>

  <section>
    <h4>Storage</h4>
    <p>Messages stored: <strong>{storeStats.count}</strong></p>
    {#if storeStats.oldest_ms > 0}
      <p>Oldest: <strong>{new Date(storeStats.oldest_ms).toLocaleDateString()}</strong></p>
    {/if}
    <button onclick={runGc} class="secondary">
      Clean up expired messages
    </button>
    {#if gcResult !== null}
      <p class="success">Deleted {gcResult} expired messages</p>
    {/if}
  </section>

  <button onclick={save} class="primary">
    {saved ? '✓ Saved' : 'Save settings'}
  </button>

  {#if appVersion}
    <p class="version">Version {appVersion}</p>
  {/if}

  <section class="danger-zone">
    <h4>⚠ Danger zone</h4>
    <p>
      <strong>Panic wipe</strong> — immediately deletes all local messages,
      keys, and stored data. Use if you need to quickly clear sensitive data.
    </p>
    <button onclick={panicWipe} class="danger">Panic wipe</button>
  </section>
</div>

<style>
  .settings { padding: 16px; max-width: 600px; display: flex; flex-direction: column; gap: 16px; }
  section { display: flex; flex-direction: column; gap: 10px; padding: 16px;
            border: 1px solid var(--border); border-radius: 8px; }
  h3 { font-size: 18px; font-weight: 600; margin: 0; }
  h4 { font-size: 14px; font-weight: 600; margin: 0; color: var(--text-secondary); text-transform: uppercase; letter-spacing: 0.05em; }
  label, .field-label { display: flex; flex-direction: column; gap: 4px; font-size: 13px; }
  .field-label { margin: 0; }
  input, select, textarea {
    padding: 7px 10px; border-radius: 6px; border: 1px solid var(--border);
    background: none; color: inherit; font-size: 13px; font-family: inherit;
  }
  textarea { resize: vertical; font-family: ui-monospace, monospace; font-size: 12px; }
  code.peer-id { font-family: ui-monospace, monospace; font-size: 11px;
                 background: var(--border); padding: 4px 6px; border-radius: 4px;
                 word-break: break-all; }
  small { color: var(--text-muted); font-size: 11px; }
  button { padding: 8px 16px; border-radius: 6px; border: none;
           cursor: pointer; font-size: 13px; font-weight: 500; }
  button.primary { background: var(--accent); color: white; }
  button.secondary { background: var(--border); color: var(--text-secondary); }
  button.danger { background: #dc2626; color: white; }
  .success { color: var(--accent); font-size: 12px; }
  .danger-zone { border-color: #dc2626; }
  .danger-zone h4 { color: #dc2626; }
  .version { font-size: 12px; color: var(--text-muted); margin: 0; }
</style>
