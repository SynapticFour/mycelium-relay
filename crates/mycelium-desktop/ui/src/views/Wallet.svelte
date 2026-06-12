// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
<script>
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  let address = $state("");
  let confirmed = $state(0);
  let pending = $state(0);
  let toAddress = $state("");
  let amount = $state(0);
  let memo = $state("");

  async function refresh() {
    address = await invoke("wallet_address");
    const bal = await invoke("wallet_balance");
    confirmed = bal.confirmed_muon ?? 0;
    pending = bal.pending_muon ?? 0;
  }

  async function send() {
    if (!toAddress || !amount) return;
    await invoke("wallet_send", {
      toAddress,
      amountMuon: Number(amount),
      feeMuon: 1000,
      memo: memo || null
    });
    toAddress = "";
    memo = "";
    amount = 0;
    await refresh();
  }

  $effect(() => {
    refresh();
    const unsubs = [];
    (async () => {
      unsubs.push(await listen("metrics-updated", refresh));
    })();
    return () => unsubs.forEach((u) => u?.());
  });
</script>

<div class="wallet">
  <section>
    <h3>Address</h3>
    <p class="mono">{address}</p>
    <p>Confirmed: {confirmed} muon</p>
    <p>Pending: {pending} muon</p>
  </section>
  <section>
    <h3>Send</h3>
    <input bind:value={toAddress} placeholder="mxc1..." />
    <input bind:value={amount} type="number" min="1" placeholder="Amount (muon)" />
    <input bind:value={memo} placeholder="Memo (optional)" />
    <button onclick={send}>Send Payment</button>
  </section>
</div>

<style>
  .wallet { display: grid; grid-template-columns: 1fr 1fr; gap: 16px; padding: 16px; }
  section { border: 1px solid var(--border); border-radius: 10px; padding: 12px; display: flex; flex-direction: column; gap: 8px; }
  .mono { font-family: ui-monospace, monospace; overflow-wrap: anywhere; }
  input { border: 1px solid var(--border); border-radius: 6px; padding: 8px; background: none; color: inherit; }
  button { padding: 8px 12px; border: none; border-radius: 6px; background: var(--accent); color: #fff; }
</style>
