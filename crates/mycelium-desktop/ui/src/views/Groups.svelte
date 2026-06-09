<script>
  import { invoke } from "@tauri-apps/api/core";
  let groups = $state([]);
  let selectedId = $state("");
  let messages = $state([]);
  let newName = $state("");
  let importJson = $state("");
  let exportJson = $state("");
  let status = $state("");

  async function refresh() {
    groups = await invoke("list_groups");
    if (selectedId) {
      messages = await invoke("group_chat_history", { groupId: selectedId, limit: 100 });
    }
  }

  async function createGroup() {
    if (!newName.trim()) return;
    await invoke("create_group", { name: newName.trim() });
    newName = "";
    await refresh();
  }

  async function importInvite() {
    status = "";
    try {
      await invoke("import_group_invite", { jsonStr: importJson.trim() });
      importJson = "";
      await refresh();
      status = "Imported.";
    } catch (e) {
      status = String(e);
    }
  }

  async function doExport() {
    if (!selectedId) return;
    exportJson = await invoke("export_group_invite", { groupId: selectedId });
  }

  async function send() {
    if (!input.trim() || !selectedId) return;
    await invoke("send_group_message", { groupId: selectedId, body: input });
    input = "";
    await refresh();
  }

  async function removeGroup() {
    if (!selectedId) return;
    await invoke("delete_group", { groupId: selectedId });
    selectedId = "";
    exportJson = "";
    await refresh();
  }

  let input = $state("");

  $effect(() => {
    refresh();
    const t = setInterval(refresh, 3000);
    return () => clearInterval(t);
  });
</script>

<div class="groups-layout">
  <div class="sidebar">
    <h2>Groups</h2>
    <p class="hint">Symmetric key groups; invite JSON contains the secret — share only in person.</p>
    <div class="row">
      <input bind:value={newName} placeholder="New group name" />
      <button type="button" onclick={createGroup}>Create</button>
    </div>
    <div class="import">
      <textarea bind:value={importJson} placeholder="Paste group invite JSON…" rows="4"></textarea>
      <button type="button" onclick={importInvite}>Import invite</button>
    </div>
    {#if status}
      <p class="status">{status}</p>
    {/if}
    <ul>
      {#each groups as g}
        <li>
          <button
            type="button"
            class:active={selectedId === g.id}
            onclick={() => {
              selectedId = g.id;
              refresh();
            }}
          >
            {g.name}
          </button>
        </li>
      {/each}
    </ul>
    {#if selectedId}
      <div class="actions">
        <button type="button" onclick={doExport}>Export invite</button>
        <button type="button" class="danger" onclick={removeGroup}>Delete local</button>
      </div>
    {/if}
    {#if exportJson}
      <textarea readonly rows="3" class="export">{exportJson}</textarea>
    {/if}
  </div>
  <div class="conversation">
    <div class="enc-badge encrypted">🔒 Group — end-to-end (shared key)</div>
    <div class="messages">
      {#each messages as msg}
        <div class="msg">
          <span class="name">{msg.from_display_name}</span>
          <span class="body">{msg.body}</span>
        </div>
      {/each}
    </div>
    <div class="input-row">
      <input bind:value={input} onkeydown={(e) => e.key === "Enter" && send()} placeholder="Message…" disabled={!selectedId} />
      <button onclick={send} disabled={!selectedId}>Send</button>
    </div>
  </div>
</div>

<style>
  .groups-layout { display: flex; height: 100%; }
  .sidebar { width: 280px; border-right: 1px solid var(--border); overflow-y: auto; padding: 12px; font-size: 13px; }
  .sidebar h2 { font-size: 16px; margin-bottom: 8px; }
  .hint { color: var(--text-muted); margin-bottom: 12px; line-height: 1.4; }
  .row { display: flex; gap: 6px; margin-bottom: 12px; }
  .row input { flex: 1; padding: 6px 8px; border-radius: 6px; border: 1px solid var(--border); }
  .import textarea { width: 100%; margin-bottom: 6px; font-family: monospace; font-size: 11px; }
  .status { color: #b45309; font-size: 12px; }
  ul { list-style: none; padding: 0; margin: 12px 0; }
  li button { width: 100%; text-align: left; padding: 8px; border-radius: 6px; border: none; background: none; cursor: pointer; font-size: 13px; }
  li button.active { background: var(--accent-subtle); font-weight: 600; }
  .actions { display: flex; flex-direction: column; gap: 6px; margin-top: 8px; }
  .actions button { padding: 8px; border-radius: 6px; border: 1px solid var(--border); background: var(--sidebar-bg); cursor: pointer; }
  .actions .danger { color: #b91c1c; }
  .export { width: 100%; font-size: 10px; margin-top: 8px; }
  .conversation { flex: 1; display: flex; flex-direction: column; }
  .enc-badge { font-size: 12px; padding: 6px 10px; border-bottom: 1px solid var(--border); background: var(--accent-subtle); }
  .messages { flex: 1; overflow-y: auto; padding: 16px; display: flex; flex-direction: column; gap: 8px; }
  .msg { max-width: 85%; padding: 8px 12px; border-radius: 10px; background: var(--border); }
  .name { font-size: 11px; color: var(--text-muted); display: block; margin-bottom: 2px; }
  .input-row { display: flex; gap: 8px; padding: 12px; border-top: 1px solid var(--border); }
  .input-row input { flex: 1; padding: 8px 12px; border-radius: 6px; border: 1px solid var(--border); }
  .input-row button { padding: 8px 16px; background: var(--accent); color: white; border: none; border-radius: 6px; cursor: pointer; }
</style>
