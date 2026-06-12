// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
<script>
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import QRCode from "qrcode";

  let { peerId } = $props();
  let peers = $state([]);
  let pasteInvite = $state("");
  let qr = $state("");
  let inviteUrl = $state("");
  let shareAddrs = $state([]);
  let relayStatus = $state(null);
  let rendezvousEnabled = $state(false);
  let connectMessage = $state("");

  async function refresh() {
    peers = await invoke("get_peers");
    shareAddrs = await invoke("get_shareable_multiaddrs");
    try {
      const settings = await invoke("get_settings");
      rendezvousEnabled = settings?.rendezvous_enabled ?? true;
    } catch {
      rendezvousEnabled = true;
    }
  }

  async function applyInvite() {
    const raw = pasteInvite.trim();
    if (!raw) return;
    connectMessage = "";
    try {
      await invoke("connect_peer_id", { peerId: raw });
      pasteInvite = "";
      connectMessage = "Connecting… watch Connected devices below.";
      await refresh();
    } catch (e) {
      connectMessage = e instanceof Error ? e.message : String(e);
    }
  }

  async function copyInvite() {
    if (!inviteUrl) return;
    await navigator.clipboard.writeText(inviteUrl);
    connectMessage = "Invite link copied.";
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
    inviteUrl =
      shareAddrs.find((a) => a.startsWith("mycelium://invite/")) ??
      shareAddrs[0] ??
      "";
    if (!inviteUrl) {
      qr = "";
      return;
    }
    QRCode.toDataURL(inviteUrl, {
      width: 320,
      margin: 2,
      errorCorrectionLevel: "M",
    }).then((x) => (qr = x));
  });

  $effect(() => {
    fetchRelayStatus();
    const t = setInterval(fetchRelayStatus, 30_000);
    return () => clearInterval(t);
  });
</script>

<div class="connect">
  {#if relayStatus}
    <div class="relay-status" class:online={relayStatus.online}>
      <span class="dot"></span>
      {relayStatus.online
        ? `Public relay online · ${relayStatus.connections ?? 0} devices connected`
        : "Public relay unreachable — same Wi‑Fi still works"}
    </div>
  {/if}

  <section class="hero">
    <h2>Connect another device</h2>
    <p class="lede">
      Put both devices on the <strong>same Wi‑Fi</strong> when you can — they connect
      automatically. Otherwise scan each other&apos;s QR once (works across the internet via
      relay).
    </p>
    <ul class="steps">
      <li><strong>Mac → phone:</strong> show the QR below; on Android open <em>Connect → Scan their QR</em>.</li>
      <li><strong>Phone → Mac:</strong> scan the phone&apos;s QR and paste it below.</li>
      <li>
        <strong>Different cities:</strong> scan QR once; <em>Relay discovery</em> is on by default
        for new installs (both devices online, ~45s).
      </li>
      <li><strong>Bluetooth:</strong> not used for pairing yet — use Wi‑Fi or QR + relay.</li>
    </ul>
    {#if !rendezvousEnabled && peers.length === 0}
      <p class="tip">
        Tip: turn on <strong>Relay discovery</strong> in Settings if you are not on the same
        Wi‑Fi.
      </p>
    {/if}
  </section>

  <section class="qr-panel">
    <h3>Show this QR on your Mac</h3>
    {#if qr}
      <img alt="Invite QR for Android to scan" src={qr} />
    {:else}
      <p class="muted">Starting node…</p>
    {/if}
    <p class="mono">{inviteUrl || peerId}</p>
    <div class="row">
      <button type="button" onclick={copyInvite} disabled={!inviteUrl}>Copy invite link</button>
    </div>
  </section>

  <section>
    <h3>Paste from the other device</h3>
    <textarea
      bind:value={pasteInvite}
      placeholder="mycelium://invite/v1#12D3Koo…"
      rows="3"
    ></textarea>
    <button type="button" onclick={applyInvite} disabled={!pasteInvite.trim()}>Connect</button>
    {#if connectMessage}
      <p class="message">{connectMessage}</p>
    {/if}
  </section>

  <section>
    <h3>Connected devices ({peers.length})</h3>
    {#if peers.length === 0}
      <p class="muted">
        None yet. After scanning, wait a few seconds. Same Wi‑Fi should appear without relay.
      </p>
    {:else}
      {#each peers as peer}
        <p class="mono peer">{peer}</p>
      {/each}
    {/if}
  </section>

  <section class="muted-block">
    <h3>Chat vs Connect</h3>
    <p>
      <strong>Connect</strong> = network link (this tab). <strong>Chat</strong> = messaging only
      with accepted contacts. Scanning a peer-ID QR adds an accepted contact automatically.
    </p>
  </section>
</div>

<style>
  .connect {
    display: flex;
    flex-direction: column;
    gap: 20px;
    padding: 16px;
    max-width: 520px;
    margin: 0 auto;
  }
  .relay-status {
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
  .hero h2 {
    margin: 0 0 8px;
    font-size: 20px;
  }
  .lede {
    margin: 0 0 12px;
    font-size: 14px;
    line-height: 1.5;
    color: var(--text-muted);
  }
  .steps {
    margin: 0;
    padding-left: 1.2rem;
    font-size: 13px;
    line-height: 1.5;
  }
  .tip {
    margin: 12px 0 0;
    padding: 10px 12px;
    border-radius: 8px;
    background: var(--surface, #f4f4f5);
    font-size: 13px;
  }
  section {
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 14px;
  }
  h3 {
    margin: 0 0 10px;
    font-size: 14px;
  }
  .qr-panel {
    text-align: center;
  }
  .qr-panel img {
    width: 280px;
    max-width: 100%;
    height: auto;
    margin: 8px auto;
    display: block;
  }
  .mono {
    font-family: ui-monospace, monospace;
    font-size: 11px;
    word-break: break-all;
  }
  .peer {
    margin: 6px 0;
  }
  textarea {
    width: 100%;
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 10px;
    font-size: 12px;
    font-family: ui-monospace, monospace;
    background: none;
    color: inherit;
    resize: vertical;
    box-sizing: border-box;
  }
  button {
    margin-top: 10px;
    padding: 10px 16px;
    border: none;
    border-radius: 8px;
    background: var(--accent);
    color: #fff;
    cursor: pointer;
    font-size: 14px;
  }
  button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .row {
    display: flex;
    justify-content: center;
    gap: 8px;
  }
  .muted,
  .muted-block p {
    font-size: 13px;
    color: var(--text-muted);
    line-height: 1.45;
  }
  .message {
    margin-top: 8px;
    font-size: 13px;
    color: #1d6b4a;
  }
</style>
