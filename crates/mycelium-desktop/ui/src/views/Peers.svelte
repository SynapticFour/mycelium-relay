<script>
  import { invoke } from "@tauri-apps/api/core";
  import QRCode from "qrcode";
  let { peerId } = $props();
  let peers = $state([]);
  let multiaddr = $state("");
  let qr = $state("");

  async function refresh() {
    peers = await invoke("get_peers");
  }

  async function add() {
    if (!multiaddr.trim()) return;
    await invoke("add_peer", { multiaddr });
    multiaddr = "";
    await refresh();
  }

  $effect(() => {
    refresh();
    const t = setInterval(refresh, 3000);
    return () => clearInterval(t);
  });

  $effect(() => {
    QRCode.toDataURL(peerId || "not-started").then((x) => (qr = x));
  });
</script>

<div class="peers">
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
  .peers { display: grid; grid-template-columns: repeat(3, 1fr); gap: 16px; padding: 16px; }
  section { border: 1px solid var(--border); border-radius: 10px; padding: 12px; }
  input { width: 100%; border: 1px solid var(--border); border-radius: 6px; padding: 8px; margin: 8px 0; background: none; color: inherit; }
  button { padding: 8px 12px; border: none; border-radius: 6px; background: var(--accent); color: #fff; }
  img { max-width: 200px; margin-top: 8px; }
</style>
