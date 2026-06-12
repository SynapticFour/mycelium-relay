// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
<script>
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  let peers = $state([]);
  let toPeer = $state("");
  let subject = $state("");
  let body = $state("");
  let inbox = $state([]);

  async function refresh() {
    peers = await invoke("get_peers");
    inbox = await invoke("mail_inbox", { limit: 100 });
  }

  async function send() {
    if (!toPeer || !subject || !body) return;
    await invoke("send_mail", { toPeer, subject, body });
    subject = "";
    body = "";
    await refresh();
  }

  $effect(() => {
    refresh();
    const unsubs = [];
    (async () => {
      unsubs.push(await listen("mail-updated", refresh));
      unsubs.push(await listen("metrics-updated", refresh));
    })();
    return () => unsubs.forEach((u) => u?.());
  });
</script>

<div class="mail">
  <section class="compose">
    <h3>Compose</h3>
    <select bind:value={toPeer}>
      <option value="">Select peer...</option>
      {#each peers as peer}
        <option value={peer}>{peer}</option>
      {/each}
    </select>
    <input bind:value={subject} placeholder="Subject" />
    <textarea bind:value={body} placeholder="Message"></textarea>
    <button onclick={send}>Send Mail</button>
  </section>
  <section class="inbox">
    <h3>Inbox</h3>
    {#each inbox as m}
      <article>
        <strong>{m.subject}</strong>
        <div>{m.from_display_name}</div>
        <p>{m.body}</p>
      </article>
    {/each}
  </section>
</div>

<style>
  .mail { display: grid; grid-template-columns: 1fr 1fr; gap: 16px; padding: 16px; height: 100%; overflow: auto; }
  .compose, .inbox { border: 1px solid var(--border); border-radius: 10px; padding: 12px; display: flex; flex-direction: column; gap: 8px; }
  input, select, textarea { border: 1px solid var(--border); border-radius: 6px; padding: 8px; background: none; color: inherit; }
  textarea { min-height: 140px; }
  button { padding: 8px 12px; border: none; border-radius: 6px; background: var(--accent); color: #fff; }
  article { border-top: 1px solid var(--border); padding-top: 8px; }
</style>
