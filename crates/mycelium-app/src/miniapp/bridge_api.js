// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Mycelium Project
/**
 * Mycelium Mini-App Bridge API — v1 (injected before app HTML runs).
 * Native host implements `window.__mycelium_native_call(json)` and resolves via
 * `window.__mycelium_resolve(id, resultJson, errorMessage)`.
 */
window.mycelium = (() => {
  let callId = 0;
  const pending = {};

  window.__mycelium_resolve = (id, result, error) => {
    const p = pending[id];
    if (!p) return;
    delete pending[id];
    if (error) p.reject(new Error(error));
    else p.resolve(result);
  };

  const CAP_PERMS = {
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

  function call(method, args = {}) {
    return new Promise((resolve, reject) => {
      const id = ++callId;
      pending[id] = { resolve, reject };
      const payload = { ...args };
      if (window.__mycelium_session) {
        payload._session = window.__mycelium_session;
      }
      const capPerm = CAP_PERMS[method];
      if (capPerm && window.__mycelium_caps && window.__mycelium_caps[capPerm]) {
        payload._cap = window.__mycelium_caps[capPerm];
      }
      window.__mycelium_native_call(JSON.stringify({ id, method, args: payload }));
    });
  }

  return {
    getIdentity: () => call("identity.get"),
    sendMessage: (to_peer, payload) => call("messaging.send", { to_peer, payload }),
    onMessage: (handler) => {
      window.__mycelium_msg_handler = handler;
      return call("messaging.subscribe");
    },
    broadcast: (payload, scope) => call("messaging.broadcast", { payload, scope }),
    requestPayment: (amount_muon, memo) =>
      call("payment.request", { amount_muon, memo }),
    createPaymentQr: (amount_muon, memo) =>
      call("payment.create_qr", { amount_muon, memo }),
    getBalance: () => call("payment.get_balance"),
    storage: {
      get: (key) => call("storage.get", { key }),
      set: (key, value) => call("storage.set", { key, value }),
      delete: (key) => call("storage.delete", { key }),
      list: (prefix) => call("storage.list", { prefix }),
    },
    getBulletins: (scope) => call("bulletin.get", { scope }),
    postBulletin: (scope, title, body, ttl_secs) =>
      call("bulletin.post", { scope, title, body, ttl_secs }),
    getNearbyPeers: () => call("peers.nearby"),
    now: () => call("util.now"),
    scanQr: () => call("util.scan_qr"),
    alert: (message) => call("util.alert", { message }),
    confirm: (message) => call("util.confirm", { message }),
    getAppId: () => call("app.get_id"),
    getVersion: () => call("app.get_version"),
  };
})();

window.__mycelium_dispatch_message = (msg_json) => {
  if (window.__mycelium_msg_handler) {
    try {
      window.__mycelium_msg_handler(JSON.parse(msg_json));
    } catch (e) {
      console.error("[mycelium] message handler error:", e);
    }
  }
};
