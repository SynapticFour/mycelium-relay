// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
<script>
  import { invoke } from "@tauri-apps/api/core";

  let { appId } = $props();

  let iframeEl;
  let html = $state("");
  let scriptNonce = crypto.randomUUID().replace(/-/g, "");
  let appDisplayName = $state("");

  const GRANTS_KEY = "mycelium_miniapp_grants";
  const MAX_SINGLE_PAYMENT = 100_000_000;
  const DEFAULT_FEE_MUON = 1000;

  const CAP_METHOD_PERM = {
    "identity.get": "Identity",
    "messaging.send": "Messaging",
    "messaging.broadcast": "MessagingBroadcast",
    "payment.request": "Payments",
    "payment.create_qr": "Payments",
    "payment.get_balance": "Payments",
    "util.scan_qr": "Camera",
    "bulletin.post": "BulletinWrite",
    "peers.nearby": "PeerDiscovery",
  };

  async function attachCapability(method, args) {
    const perm = CAP_METHOD_PERM[method];
    if (!perm || !args?._session) return args;
    try {
      const cap = await invoke("miniapp_issue_capability", {
        appId,
        permission: perm,
        sessionToken: args._session,
      });
      return { ...args, _cap: cap };
    } catch {
      return args;
    }
  }

  function grantKey(permission) {
    return `${appId}:${permission}`;
  }

  function hasGrant(permission) {
    try {
      const raw = localStorage.getItem(GRANTS_KEY);
      if (!raw) return false;
      const map = JSON.parse(raw);
      return !!map[grantKey(permission)];
    } catch {
      return false;
    }
  }

  function setGrant(permission) {
    const raw = localStorage.getItem(GRANTS_KEY);
    const map = raw ? JSON.parse(raw) : {};
    map[grantKey(permission)] = true;
    localStorage.setItem(GRANTS_KEY, JSON.stringify(map));
  }

  async function ensureGrant(permission, title, message) {
    if (hasGrant(permission)) return true;
    return window.confirm(`${title}\n\n${message}`);
  }

  function buildCsp(nonce) {
    return `<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'nonce-${nonce}'; style-src 'unsafe-inline'; img-src data:; connect-src 'none'; form-action 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'; wasm-unsafe-eval 'none';">`;
  }

  function addNonceToScripts(html, nonce) {
    return html.replace(/<script(\s[^>]*)?>/gi, (match, attrs = "") => {
      if (attrs.includes("nonce=")) return match;
      return `<script nonce="${nonce}"${attrs}>`;
    });
  }

  let bridgeTargetOrigin = $state("*");
  let policy = $state(null);

  async function loadPolicy() {
    try {
      policy = await invoke("miniapp_get_policy", { appId });
    } catch {
      policy = null;
    }
  }

  async function toggleSafeMode(ev) {
    if (!policy) return;
    const next = ev.target.checked;
    await invoke("miniapp_set_safe_mode", { appId, enabled: next });
    await loadPolicy();
  }

  async function reportApp() {
    await invoke("miniapp_report_app", { appId });
    await loadPolicy();
    window.alert("Report recorded locally. Reputation was adjusted.");
  }

  function postToIframe(msg) {
    iframeEl?.contentWindow?.postMessage(msg, bridgeTargetOrigin);
  }

  async function loadApp() {
    const id = appId;
    appDisplayName = id;
    const rawHtml = await invoke("miniapp_get_entry_html", {
      appId: id,
      scriptNonce,
    });
    const installed = await invoke("miniapp_list_installed");
    const info = installed.find((a) => a.id === id);
    if (info?.name) appDisplayName = info.name;

    const csp = buildCsp(scriptNonce);
    const withNonces = addNonceToScripts(rawHtml, scriptNonce);
    if (withNonces.includes("<head>")) {
      html = withNonces.replace("<head>", `<head>${csp}`);
    } else {
      html = `${csp}${withNonces}`;
    }
  }

  async function handlePaymentConfirmation(id, result) {
    const amountMuon = Number(result.amount_muon ?? 0);
    const memo = result.memo ?? "";

    const installed = await invoke("miniapp_list_installed");
    const appInfo = installed.find((a) => a.id === appId);
    const paymentAddress = appInfo?.payment_address;

    if (!paymentAddress) {
      postToIframe({
        __mycelium_resolve: {
          id,
          result: null,
          error: "app has no payment address configured",
        },
      });
      return;
    }
    if (amountMuon <= 0) {
      postToIframe({
        __mycelium_resolve: { id, result: null, error: "invalid payment amount" },
      });
      return;
    }
    if (amountMuon > MAX_SINGLE_PAYMENT) {
      postToIframe({
        __mycelium_resolve: {
          id,
          result: null,
          error: "payment amount exceeds maximum allowed per transaction (100 MXC)",
        },
      });
      return;
    }

    const bal = await invoke("wallet_balance");
    const confirmed = Number(bal.confirmed_muon ?? 0);
    if (confirmed < amountMuon) {
      postToIframe({
        __mycelium_resolve: { id, result: null, error: "insufficient balance" },
      });
      return;
    }

    const amountMxc = `${(amountMuon / 1_000_000).toFixed(4)} MXC`;
    const addressPreview =
      paymentAddress.length > 16
        ? `${paymentAddress.slice(0, 16)}…`
        : paymentAddress;
    const memoSuffix = memo ? `\n\nMemo: ${memo}` : "";

    const ok = window.confirm(
      `${appDisplayName} wants to charge:\n\n${amountMxc}\n\nTo: ${addressPreview}${memoSuffix}\n\nConfirm payment?`
    );
    if (!ok) {
      postToIframe({
        __mycelium_resolve: { id, result: null, error: "user cancelled payment" },
      });
      return;
    }

    try {
      await invoke("wallet_send", {
        toAddress: paymentAddress,
        amountMuon,
        feeMuon: DEFAULT_FEE_MUON,
        memo: memo || null,
      });
      postToIframe({
        __mycelium_resolve: {
          id,
          result: {
            status: "submitted",
            amount_muon: amountMuon,
            to: paymentAddress,
            memo,
          },
          error: null,
        },
      });
    } catch (err) {
      const msg = err?.message ?? String(err);
      postToIframe({
        __mycelium_resolve: {
          id,
          result: null,
          error: `payment failed: ${msg}`,
        },
      });
    }
  }

  function handleBridgeAction(id, result) {
    const action = result?._bridge_action;
    if (!action) {
      postToIframe({ __mycelium_resolve: { id, result, error: null } });
      return;
    }
    switch (action) {
      case "show_alert":
        window.alert(
          `[Mini-app: ${appId}]\n(This message is from the app, not Mycelium.)\n\n${result.message ?? ""}`
        );
        postToIframe({ __mycelium_resolve: { id, result: "ok", error: null } });
        break;
      case "show_confirm": {
        const ok = window.confirm(
          `[Mini-app: ${appId}]\n(This message is from the app, not Mycelium.)\n\n${result.message ?? "Confirm?"}`
        );
        postToIframe({
          __mycelium_resolve: {
            id,
            result: ok,
            error: ok ? null : "cancelled",
          },
        });
        break;
      }
      case "payment_confirmation_required":
        handlePaymentConfirmation(id, result);
        break;
      case "render_qr":
        window.alert(`Payment QR (mxcpay):\n${result.uri ?? result._qr_content ?? ""}`);
        postToIframe({
          __mycelium_resolve: { id, result: { uri: result.uri }, error: null },
        });
        break;
      case "open_qr_scanner": {
        const raw = window.prompt("Paste QR content (desktop scanner stub):");
        if (raw == null || raw === "") {
          postToIframe({
            __mycelium_resolve: { id, result: null, error: "scan cancelled" },
          });
        } else {
          postToIframe({ __mycelium_resolve: { id, result: raw, error: null } });
        }
        break;
      }
      default:
        postToIframe({ __mycelium_resolve: { id, result, error: null } });
    }
  }

  async function dispatchBridge(id, method, args) {
    if (method === "identity.get") {
      const ok = await ensureGrant(
        "Identity",
        "Share identity?",
        `${appId} wants your Peer ID and encryption public key.`
      );
      if (!ok) {
        postToIframe({
          __mycelium_resolve: { id, result: null, error: "permission denied" },
        });
        return;
      }
      setGrant("Identity");
    }
    if (method === "util.scan_qr") {
      const ok = await ensureGrant(
        "Camera",
        "Use camera?",
        "This app wants to scan QR codes."
      );
      if (!ok) {
        postToIframe({
          __mycelium_resolve: { id, result: null, error: "permission denied" },
        });
        return;
      }
      setGrant("Camera");
    }
    if (method === "messaging.broadcast") {
      const ok = await ensureGrant(
        "MessagingBroadcast",
        "Broadcast messages?",
        "This app can send mesh-wide chat broadcasts (rate limited)."
      );
      if (!ok) {
        postToIframe({
          __mycelium_resolve: { id, result: null, error: "permission denied" },
        });
        return;
      }
      setGrant("MessagingBroadcast");
    }
    if (
      method === "payment.request" ||
      method === "payment.create_qr" ||
      method === "payment.get_balance"
    ) {
      const ok = await ensureGrant(
        "Payments",
        "Wallet access?",
        "This app can read balance and show payment QR codes."
      );
      if (!ok) {
        postToIframe({
          __mycelium_resolve: { id, result: null, error: "permission denied" },
        });
        return;
      }
      setGrant("Payments");
    }

    try {
      const bridgedArgs = await attachCapability(method, args ?? {});
      const result = await invoke("miniapp_bridge_call", {
        appId,
        method,
        args: bridgedArgs,
      });
      handleBridgeAction(id, result);
    } catch (err) {
      const msg = err?.message ?? String(err);
      postToIframe({ __mycelium_resolve: { id, result: null, error: msg } });
    }
  }

  function handleMessage(e) {
    if (e.source !== iframeEl?.contentWindow) return;
    bridgeTargetOrigin = e.origin;
    const raw = e.data && e.data.__mycelium_call;
    if (raw == null) return;
    let payload;
    try {
      payload = typeof raw === "string" ? JSON.parse(raw) : raw;
    } catch {
      return;
    }
    const { id, method, args } = payload;
    dispatchBridge(id, method, args);
  }

  $effect(() => {
    const id = appId;
    loadApp();
    loadPolicy();
    window.addEventListener("message", handleMessage);
    return () => {
      window.removeEventListener("message", handleMessage);
      invoke("miniapp_revoke_bridge_session", { appId: id }).catch(() => {});
    };
  });
</script>

<div class="wrap">
  {#if policy}
    <div
      class="policy-bar"
      class:warn={policy.safe_mode_suggested || policy.safe_mode_forced}
      class:danger={policy.revoked}
    >
      {#if policy.revoked}
        <span>Revoked app — uninstall recommended.</span>
      {:else if policy.safe_mode_forced}
        <span>Safe mode required (score {policy.reputation_score}).</span>
      {:else if policy.safe_mode_suggested}
        <span>Safe mode recommended (score {policy.reputation_score}).</span>
      {:else}
        <span>Reputation score: {policy.reputation_score}</span>
      {/if}
      <label class="safe-toggle">
        <input
          type="checkbox"
          checked={policy.user_safe_mode || policy.safe_mode_forced}
          disabled={policy.safe_mode_forced}
          onchange={toggleSafeMode}
        />
        Safe mode
      </label>
      <button type="button" class="report-btn" onclick={reportApp}>Report</button>
    </div>
  {/if}
  <iframe
    bind:this={iframeEl}
    srcdoc={html}
    sandbox="allow-scripts"
    credentialless
    referrerpolicy="no-referrer"
    style="width:100%;height:100%;border:none;"
    title="Mini-app: {appId}"
  ></iframe>
</div>

<style>
  .wrap {
    height: 100%;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .policy-bar {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
    padding: 8px 12px;
    font-size: 13px;
    background: #e5e7eb;
    color: #111;
  }
  .policy-bar.warn {
    background: #fef3c7;
    color: #92400e;
  }
  .policy-bar.danger {
    background: #fee2e2;
    color: #991b1b;
  }
  .safe-toggle {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    margin-left: auto;
  }
  .report-btn {
    font-size: 12px;
    padding: 4px 8px;
  }
</style>
