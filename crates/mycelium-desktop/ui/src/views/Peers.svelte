// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
<script>
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import QRCode from "qrcode";

  let { peerId } = $props();
  let peers = $state([]);
  let multiaddr = $state("");
  let connectPeerId = $state("");
  let qr = $state("");
  let shareAddrs = $state([]);
  let relayStatus = $state(null);

  async function refresh() {
    peers = await invoke("get_peers");
    shareAddrs = await invoke("get_shareable_multiaddrs");
  }

  async function add() {
    if (!multiaddr.trim()) return;
    await invoke("add_peer", { multiaddr: multiaddr.trim() });
    multiaddr = "";
    await refresh();
  }

  async function connectViaRelay() {
    if (!connectPeerId.trim()) return;
    await invoke("connect_peer_id", { peerId: connectPeerId.trim() });
    connectPeerId = "";
    await refresh();
  }

  async function fetchRelayStatus() {
    try {
      relayStatus = await invoke("get_relay_status");
    } catch {
      relayStatus = { online: false, status: "offline", connections: 0 };
    }
  }

  $effect(() => {
    refresh();
    const unsubs = [];
    (async () => {
      unsubs.push(await listen("metrics-updated", refresh));
    })();
    return () => unsubs.forEach((u) => u?.());
  });

  $effect(() => {
    const payload = shareAddrs[0] || peerId || "not-started";
    QRCode.toDataURL(payload).then((x) => (qr = x));
  });

  $effect(() => {
    fetchRelayStatus();
    const t = setInterval(fetchRelayStatus, 30_000);
    return () => clearInterval(t);
  });
</script>

<div class="peers">
  {#if relayStatus}
    <div class="relay-status" class:online={relayStatus.online}>
      <span class="dot"></span>
      {relayStatus.online
        ? `Relay online · ${relayStatus.connections ?? 0} connections`
        : "Relay status unavailable in UI (node may still use relay — see make desktop-dev log)"}
    </div>
  {/if}

  <p class="hint">
    <strong>Network:</strong> Devices on the same relay auto-connect every ~45s (after you restart both apps).
    Same Wi‑Fi also uses local discovery. <strong>Contacts:</strong> Chat lists people you have history with;
    scanning QR adds a transport link (like exchanging phone numbers).
  </p>

  <section>
    <h3>Connect via relay (peer ID only)</h3>
    <input bind:value={connectPeerId} placeholder="12D3KooW… (other device's peer ID)" />
    <button onclick={connectViaRelay} disabled={!connectPeerId.trim()}>Connect via relay</button>
  </section>

  <section>
    <h3>Connect peer (multiaddr)</h3>
    <input bind:value={multiaddr} placeholder="/dns4/…/p2p-circuit/p2p/… or /ip4/…/tcp/…/p2p/…" />
    <button onclick={add}>Add bootstrap peer</button>
  </section>

  <section>
    <h3>Connected peers</h3>
    {#if peers.length === 0}
      <p class="muted">No other devices connected yet.</p>
    {:else}
      {#each peers as peer}
        <p>{peer}</p>
      {/each}
    {/if}
  </section>

  <section>
    <h3>Your dial info</h3>
    <p class="mono">{peerId}</p>
    {#each shareAddrs as addr}
      <p class="mono small">{addr}</p>
    {/each}
    {#if qr}<img alt="Invite QR (scan with Android)" src={qr} />{/if}
    <p class="muted small">QR format: <code>mycelium://invite/v1#…</code></p>
  </section>
</div>

<style>
  .peers {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 16px;
    padding: 16px;
  }
  .relay-status {
    grid-column: 1 / -1;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 12px;
    border-radius: 8px;
    font-size: 13px;
    background: #fde8e8;
    color: #444;
  }
  .relay-status.online {
    background: #e1f5ee;
  }
  .relay-status .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: #c44;
  }
  .relay-status.online .dot {
    background: #1d9e75;
  }
  .hint {
    grid-column: 1 / -1;
    font-size: 13px;
    color: var(--text-muted);
    line-height: 1.45;
    margin: 0;
  }
  section {
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 12px;
  }
  h3 {
    font-size: 14px;
    margin: 0 0 8px;
  }
  input {
    width: 100%;
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 8px;
    margin: 8px 0;
    background: none;
    color: inherit;
    font-size: 12px;
  }
  button {
    padding: 8px 12px;
    border: none;
    border-radius: 6px;
    background: var(--accent);
    color: #fff;
    cursor: pointer;
  }
  .mono {
    font-family: ui-monospace, monospace;
    font-size: 11px;
    word-break: break-all;
  }
  .small {
    margin-top: 6px;
    opacity: 0.85;
  }
  .muted {
    font-size: 13px;
    color: var(--text-muted);
  }
  img {
    max-width: 200px;
    margin-top: 8px;
  }
</style>
