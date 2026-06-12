// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
<script>
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";

  let { onOpenApp = () => {} } = $props();

  let installed = $state([]);
  let listings = $state([]);
  let message = $state("");
  let pendingInstall = $state(null);
  let allowDowngrade = $state(false);
  let revokeAppId = $state("");
  let revokeReason = $state("malware");

  const trustLabels = {
    verified_listing: "Verified store listing",
    matching_listing_hash: "Listing hash match (signature invalid)",
    hash_mismatch: "Bundle hash mismatch",
    sideload_only: "Sideload only",
  };

  async function refresh() {
    try {
      installed = await invoke("miniapp_list_installed");
      listings = await invoke("miniapp_browse_store");
    } catch (e) {
      message = String(e);
    }
  }

  async function installFromFile(ev) {
    const file = ev.target?.files?.[0];
    if (!file) return;
    const buf = await file.arrayBuffer();
    const bytes = new Uint8Array(buf);
    let binary = "";
    for (let i = 0; i < bytes.length; i++) binary += String.fromCharCode(bytes[i]);
    const bundleBase64 = btoa(binary);
    try {
      const preview = await invoke("miniapp_preview_install", { bundleBase64 });
      allowDowngrade = false;
      pendingInstall = { bundleBase64, preview };
    } catch (e) {
      message = String(e);
    }
    ev.target.value = "";
  }

  function listingAppIdForPending() {
    if (!pendingInstall) return null;
    if (pendingInstall.preview.trust_level === "verified_listing") {
      return pendingInstall.preview.manifest.id;
    }
    return null;
  }

  async function confirmPendingInstall() {
    if (!pendingInstall) return;
    try {
      await invoke("miniapp_install", {
        bundleBase64: pendingInstall.bundleBase64,
        listingAppId: listingAppIdForPending(),
        allowSideload: listingAppIdForPending() == null,
        allowDowngrade,
      });
      message = `Installed ${pendingInstall.preview.manifest.name}`;
      pendingInstall = null;
      await refresh();
    } catch (e) {
      message = String(e);
    }
  }

  async function publishRevocation() {
    const id = revokeAppId.trim();
    if (!id) {
      message = "Enter an app id to revoke";
      return;
    }
    try {
      await invoke("miniapp_publish_revocation", {
        appId: id,
        reason: revokeReason.trim() || "curator",
      });
      message = `Published revocation for ${id} on gossip`;
      revokeAppId = "";
      await refresh();
    } catch (e) {
      message = String(e);
    }
  }

  async function uninstall(appId) {
    try {
      await invoke("miniapp_uninstall", { appId });
      await refresh();
    } catch (e) {
      message = String(e);
    }
  }

  $effect(() => {
    refresh();
    const unsubs = [];
    (async () => {
      unsubs.push(await listen("appstore-updated", refresh));
    })();
    return () => unsubs.forEach((u) => u?.());
  });
</script>

<div class="store">
  <h3>Mini apps</h3>
  <label class="file">
    Install bundle (.mxa / .zip)
    <input type="file" accept=".zip,.mxa,application/zip" onchange={installFromFile} />
  </label>
  {#if message}
    <p class="msg">{message}</p>
  {/if}

  {#if pendingInstall}
    <div class="modal" role="dialog" aria-labelledby="install-review-title">
      <div class="modal-card">
        <h4 id="install-review-title">Review before install</h4>
        <p class="title">{pendingInstall.preview.manifest.name}</p>
        <p class="meta">{pendingInstall.preview.manifest.id} · v{pendingInstall.preview.manifest.version}</p>
        <p class="meta">
          Trust: {trustLabels[pendingInstall.preview.trust_level] ?? pendingInstall.preview.trust_level}
        </p>
        {#if pendingInstall.preview.listing_signature_ok}
          <p class="ok">Signed listing</p>
        {/if}
        {#if pendingInstall.preview.has_inline_script}
          <p class="warn">This app uses inline scripts (higher XSS risk).</p>
        {/if}
        {#if pendingInstall.preview.reproducible_attested}
          <p class="ok-line">Reproducible build attested (content hash matches manifest).</p>
        {/if}
        {#if pendingInstall.preview.strict_csp_eligible}
          <p class="ok-line">Strict CSP eligible (no inline scripts).</p>
        {/if}
        {#if pendingInstall.preview.manifest.permissions?.length}
          <p class="meta">Permissions:</p>
          <ul class="perms">
            {#each pendingInstall.preview.manifest.permissions as perm}
              <li>{perm}</li>
            {/each}
          </ul>
        {/if}
        {#if pendingInstall.preview.is_downgrade && pendingInstall.preview.installed_version}
          <p class="warn">
            Downgrade: v{pendingInstall.preview.installed_version} → v{pendingInstall.preview.manifest.version}.
            Check the box below to allow.
          </p>
          <label class="row downgrade">
            <input type="checkbox" bind:checked={allowDowngrade} />
            Allow version downgrade
          </label>
        {/if}
        <p class="warn">Only install apps you trust. Malicious mini-apps can abuse granted permissions.</p>
        <div class="row">
          <button
            type="button"
            disabled={
              pendingInstall.preview.trust_level === "hash_mismatch" ||
              (pendingInstall.preview.is_downgrade && !allowDowngrade)
            }
            onclick={confirmPendingInstall}
          >
            Install
          </button>
          <button type="button" class="muted-btn" onclick={() => (pendingInstall = null)}>Cancel</button>
        </div>
      </div>
    </div>
  {/if}

  <section>
    <h4>Installed</h4>
    {#if installed.length === 0}
      <p class="muted">None yet.</p>
    {:else}
      <ul>
        {#each installed as app (app.id)}
          <li>
            <span class="title">{app.name}</span>
            <span class="meta">{app.id} · v{app.version}</span>
            <div class="row">
              <button type="button" onclick={() => onOpenApp(app.id)}>Open</button>
              <button type="button" class="danger" onclick={() => uninstall(app.id)}>Remove</button>
            </div>
          </li>
        {/each}
      </ul>
    {/if}
  </section>
  <section class="curator">
    <h4>Curator — publish revocation</h4>
    <p class="muted">
      Signs with this node&apos;s identity and gossips on <code>mycelium/appstore/revocations/v1</code>.
      Use only if you operate a store curator key.
    </p>
    <div class="row">
      <input
        type="text"
        placeholder="com.example.badapp"
        bind:value={revokeAppId}
        aria-label="App id to revoke"
      />
      <input
        type="text"
        placeholder="reason"
        bind:value={revokeReason}
        aria-label="Revocation reason"
      />
      <button type="button" class="danger" onclick={publishRevocation}>Revoke &amp; gossip</button>
    </div>
  </section>

  <section>
    <h4>Catalog (cached listings)</h4>
    {#if listings.length === 0}
      <p class="muted">No listings on this node.</p>
    {:else}
      <ul>
        {#each listings as row (row.manifest.id + row.bundle_hash)}
          <li>
            <span class="title">{row.manifest.name}</span>
            <span class="meta">{row.manifest.id}</span>
            <span class="meta hash">{row.bundle_hash.slice(0, 16)}…</span>
            {#if row.signature_valid}
              <span class="ok">Signed listing</span>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </section>
</div>

<style>
  .store {
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    height: 100%;
    overflow: auto;
  }
  h3,
  h4 {
    margin: 0;
  }
  .file {
    display: inline-block;
    padding: 8px 12px;
    border-radius: 8px;
    border: 1px solid var(--border);
    cursor: pointer;
    font-size: 14px;
  }
  .file input {
    display: none;
  }
  .msg {
    color: #b45309;
    font-size: 14px;
  }
  .modal {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.45);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 20;
  }
  .modal-card {
    background: var(--bg, #fff);
    color: var(--text, #111);
    border-radius: 12px;
    padding: 20px;
    max-width: 420px;
    width: 90%;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.2);
  }
  .warn {
    color: #b45309;
    font-size: 13px;
  }
  .ok {
    color: #15803d;
    font-size: 12px;
    display: block;
  }
  .perms {
    margin: 4px 0 0;
    padding-left: 18px;
    font-size: 12px;
  }
  section {
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 12px;
  }
  ul {
    list-style: none;
    padding: 0;
    margin: 8px 0 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  li {
    border-bottom: 1px solid var(--border);
    padding-bottom: 10px;
  }
  li:last-child {
    border-bottom: none;
    padding-bottom: 0;
  }
  .title {
    font-weight: 600;
    display: block;
  }
  .meta {
    font-size: 12px;
    color: var(--text-muted);
    display: block;
  }
  .hash {
    font-family: ui-monospace, monospace;
  }
  .ok-line {
    color: #166534;
    font-size: 13px;
  }
  .row {
    display: flex;
    gap: 8px;
    margin-top: 8px;
    flex-wrap: wrap;
    align-items: center;
  }
  .curator input {
    flex: 1;
    min-width: 140px;
    padding: 6px 8px;
    font-size: 13px;
  }
  button {
    padding: 6px 12px;
    border: none;
    border-radius: 6px;
    background: var(--accent);
    color: #fff;
    cursor: pointer;
    font-size: 13px;
  }
  button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  button.danger {
    background: #c45c5c;
  }
  button.muted-btn {
    background: transparent;
    color: var(--text-muted);
    border: 1px solid var(--border);
  }
  .muted {
    color: var(--text-muted);
    font-size: 14px;
  }
</style>
