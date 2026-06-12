// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
<script>
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import Chat from "./views/Chat.svelte";
  import Groups from "./views/Groups.svelte";
  import Mail from "./views/Mail.svelte";
  import Bulletin from "./views/Bulletin.svelte";
  import Peers from "./views/Peers.svelte";
  import Wallet from "./views/Wallet.svelte";
  import Settings from "./views/Settings.svelte";
  import Setup from "./views/Setup.svelte";
  import MiniApp from "./views/MiniApp.svelte";
  import MiniAppStore from "./views/MiniAppStore.svelte";
  import ConnectionStatus from "./components/ConnectionStatus.svelte";

  let nodeStarted = $state(false);
  let peerId = $state("");
  let activeView = $state("chat");
  let selectedMiniAppId = $state("");
  let peers = $state(0);
  let forwarded = $state(0);
  let queue = $state(0);
  const views = [
    { id: "chat", label: "Chat" },
    { id: "groups", label: "Groups" },
    { id: "mail", label: "Mail" },
    { id: "bulletin", label: "Bulletin" },
    { id: "peers", label: "Connect" },
    { id: "wallet", label: "Wallet" },
    { id: "miniapp", label: "Mini apps" },
    { id: "settings", label: "Settings" },
  ];

  async function startNode(config) {
    peerId = await invoke("start_node", {
      dbPath: config.dbPath,
      displayName: config.displayName,
      bootstrapPeers: config.bootstrapPeers ?? [],
    });
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
  <Setup onstart={startNode} />
{:else}
  <div class="app">
    <nav>
      <div class="logo">Mycelium</div>
      <div class="status">
        <span class="dot" class:green={peers > 0}></span>
        {peers} peer{peers !== 1 ? "s" : ""}
      </div>
      {#each views as view}
        <button class:active={activeView === view.id} onclick={() => (activeView = view.id)}>
          {view.label}
        </button>
      {/each}
      <div class="metrics-mini">
        <span>{forwarded} fwd</span>
        <span>{queue} q</span>
      </div>
    </nav>
    <main>
      <ConnectionStatus peerCount={peers} />
      {#if activeView === "chat"}
        <Chat {peerId} />
      {:else if activeView === "groups"}
        <Groups />
      {:else if activeView === "mail"}
        <Mail />
      {:else if activeView === "bulletin"}
        <Bulletin />
      {:else if activeView === "peers"}
        <Peers {peerId} />
      {:else if activeView === "wallet"}
        <Wallet />
      {:else if activeView === "miniapp"}
        <MiniAppStore
          onOpenApp={(id) => {
            selectedMiniAppId = id;
            activeView = "miniapp_run";
          }}
        />
      {:else if activeView === "miniapp_run"}
        <div class="miniapp-run">
          <button type="button" class="back" onclick={() => (activeView = "miniapp")}>← Mini apps</button>
          <MiniApp appId={selectedMiniAppId} />
        </div>
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
  main {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
  }
  .miniapp-run {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  .miniapp-run .back {
    align-self: flex-start;
    margin: 8px 12px;
    padding: 6px 10px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--sidebar-bg);
    cursor: pointer;
    font-size: 13px;
  }
  :global(html, body, #app) { height: 100%; }
  :global(body) { overflow: hidden; }
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
