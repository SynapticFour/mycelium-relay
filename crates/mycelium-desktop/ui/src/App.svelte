<script>
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import Chat from "./views/Chat.svelte";
  import Mail from "./views/Mail.svelte";
  import Bulletin from "./views/Bulletin.svelte";
  import Peers from "./views/Peers.svelte";
  import Wallet from "./views/Wallet.svelte";
  import Settings from "./views/Settings.svelte";
  import Setup from "./views/Setup.svelte";

  let nodeStarted = $state(false);
  let peerId = $state("");
  let activeView = $state("chat");
  let peers = $state(0);
  let forwarded = $state(0);
  let queue = $state(0);
  const views = ["chat", "mail", "bulletin", "peers", "wallet", "settings"];

  async function startNode(config) {
    peerId = await invoke("start_node", config);
    nodeStarted = true;
  }

  onMount(async () => {
    const unlisten = await listen("metrics-updated", (event) => {
      peers = event.payload.peers ?? 0;
      forwarded = event.payload.forwarded ?? 0;
      queue = event.payload.queue ?? 0;
    });
    return () => unlisten();
  });
</script>

{#if !nodeStarted}
  <Setup on:start={(e) => startNode(e.detail)} />
{:else}
  <div class="app">
    <nav>
      <div class="logo">Mycelium</div>
      <div class="status">
        <span class="dot" class:green={peers > 0}></span>
        {peers} peer{peers !== 1 ? "s" : ""}
      </div>
      {#each views as view}
        <button class:active={activeView === view} onclick={() => (activeView = view)}>
          {view}
        </button>
      {/each}
      <div class="metrics-mini">
        <span>{forwarded} fwd</span>
        <span>{queue} q</span>
      </div>
    </nav>
    <main>
      {#if activeView === "chat"}
        <Chat {peerId} />
      {:else if activeView === "mail"}
        <Mail />
      {:else if activeView === "bulletin"}
        <Bulletin />
      {:else if activeView === "peers"}
        <Peers {peerId} />
      {:else if activeView === "wallet"}
        <Wallet />
      {:else if activeView === "settings"}
        <Settings {peerId} />
      {/if}
    </main>
  </div>
{/if}

<style>
  .app { display: flex; height: 100vh; font-family: system-ui, sans-serif; }
  nav { width: 180px; min-width: 180px; display: flex; flex-direction: column; gap: 2px; padding: 16px 8px; border-right: 1px solid var(--border); background: var(--sidebar-bg); }
  .logo { font-weight: 600; font-size: 18px; padding: 8px; margin-bottom: 12px; }
  .status { font-size: 12px; color: var(--text-muted); padding: 4px 8px; display: flex; align-items: center; gap: 6px; margin-bottom: 8px; }
  .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--muted); }
  .dot.green { background: #1d9e75; }
  nav button { text-align: left; padding: 8px 12px; border-radius: 6px; border: none; background: none; cursor: pointer; font-size: 14px; color: var(--text-secondary); text-transform: capitalize; }
  nav button.active { background: var(--accent-subtle); color: var(--accent); font-weight: 500; }
  .metrics-mini { margin-top: auto; font-size: 11px; color: var(--text-muted); padding: 4px 8px; display: flex; justify-content: space-between; }
  main { flex: 1; overflow: hidden; }
  :global(*) { box-sizing: border-box; margin: 0; padding: 0; }
  :global(:root) {
    --border: #e5e3da;
    --sidebar-bg: #f8f7f4;
    --accent: #1d9e75;
    --accent-subtle: #e1f5ee;
    --text-secondary: #444;
    --text-muted: #888;
    --muted: #ccc;
  }
</style>
