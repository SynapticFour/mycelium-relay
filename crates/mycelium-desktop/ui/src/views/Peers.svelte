<script>
  import { invoke } from "@tauri-apps/api/core";
  import QRCode from "qrcode";
  let { peerId } = $props();
  let peers = $state([]);
  let multiaddr = $state("");
  let qr = $state("");
  let relayStatus = $state(null);

  async function refresh() {
    peers = await invoke("get_peers");
  }

  async function add() {
    if (!multiaddr.trim()) return;
    await invoke("add_peer", { multiaddr });
    multiaddr = "";
    await refresh();
  }

  async function fetchRelayStatus() {
    try {
      const res = await fetch("https://mycelium-relay.fly.dev/");
      relayStatus = await res.json();
    } catch {
      relayStatus = { status: "offline" };
    }
  }

  $effect(() => {
    refresh();
    const t = setInterval(refresh, 3000);
    return () => clearInterval(t);
  });

  $effect(() => {
    QRCode.toDataURL(peerId || "not-started").then((x) => (qr = x));
  });

  $effect(() => {
    fetchRelayStatus();
    const t = setInterval(fetchRelayStatus, 30_000);
    return () => clearInterval(t);
  });
</script>

<div class="peers">
  {#if relayStatus}
    <div class="relay-status" class:online={relayStatus.status === "ok"}>
      <span class="dot"></span>
      {relayStatus.status === "ok"
        ? `Relay online · ${relayStatus.connections ?? 0} connections`
        : "Relay offline – mesh-only mode"}
    </div>
  {/if}
  <section>
    <h3>Connect peer</h3>
    <input bind:value={multiaddr} placeholder="/ip4/x.x.x.x/tcp/4001/p2p/..." />
    <button onclick={add}>Add bootstrap peer</button>
  </section>
  <section>
    <h3>Connected peers</h3>
    {#each peers as peer}
      <p>{peer}</p>
    {/each}
  </section>
  <section>
    <h3>Your Peer ID</h3>
    <p>{peerId}</p>
    {#if qr}<img alt="peer qr" src={qr} />{/if}
  </section>
</div>

<style>
  .peers {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
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
    background: var(--error-container, #fde8e8);
    color: var(--text-secondary, #444);
  }
  .relay-status.online {
    background: var(--accent-subtle, #e1f5ee);
  }
  .relay-status .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: #c44;
  }
  .relay-status.online .dot {
    background: var(--accent, #1d9e75);
  }
  section {
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 12px;
  }
  input {
    width: 100%;
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 8px;
    margin: 8px 0;
    background: none;
    color: inherit;
  }
  button {
    padding: 8px 12px;
    border: none;
    border-radius: 6px;
    background: var(--accent);
    color: #fff;
  }
  img {
    max-width: 200px;
    margin-top: 8px;
  }
</style>
