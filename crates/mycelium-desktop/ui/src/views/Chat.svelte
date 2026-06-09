<script>
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";

  let { peerId } = $props();
  let selectedPeer = $state("");
  let acceptedContacts = $state([]);
  let pendingContacts = $state([]);
  let messages = $state([]);
  let input = $state("");
  let isEncrypted = $state(false);
  let errorMsg = $state("");
  let locationLoading = $state(false);
  let showLocationPrivacy = $state(false);
  let locationPrivacyAck = $state(
    typeof localStorage !== "undefined" &&
      localStorage.getItem("location_privacy_acknowledged") === "1",
  );

  function formatLocation(lat, lon, accuracyM) {
    const latAbs = Math.abs(lat).toFixed(4);
    const lonAbs = Math.abs(lon).toFixed(4);
    const latDir = lat >= 0 ? "N" : "S";
    const lonDir = lon >= 0 ? "E" : "W";
    const acc =
      accuracyM != null && !Number.isNaN(accuracyM)
        ? ` (±${Math.round(accuracyM)}m)`
        : "";
    return `📍 ${latAbs}° ${latDir}, ${lonAbs}° ${lonDir}${acc}`;
  }

  function extractCoordinates(body) {
    const m = body.match(/(-?\d+\.\d+)° ([NS]), (-?\d+\.\d+)° ([EW])/);
    if (!m) return null;
    const lat = parseFloat(m[1]) * (m[2] === "S" ? -1 : 1);
    const lon = parseFloat(m[3]) * (m[4] === "W" ? -1 : 1);
    return { lat, lon };
  }

  async function refreshContacts() {
    acceptedContacts = await invoke("list_accepted_contacts");
    pendingContacts = await invoke("list_pending_contacts");
  }

  async function refresh() {
    await refreshContacts();
    if (selectedPeer) {
      messages = await invoke("chat_history", { peerId: selectedPeer, limit: 100 });
      isEncrypted = await invoke("peer_has_enc_key", { peerId: selectedPeer });
    } else {
      isEncrypted = false;
    }
  }

  async function acceptContact(peerId) {
    await invoke("accept_contact", { peerId });
    selectedPeer = peerId;
    await refresh();
  }

  async function rejectContact(peerId) {
    await invoke("reject_contact", { peerId });
    if (selectedPeer === peerId) selectedPeer = "";
    await refresh();
  }

  async function sendBody(body) {
    if (!selectedPeer || !body.trim()) return;
    errorMsg = "";
    try {
      await invoke("send_chat_encrypted", { toPeer: selectedPeer, body });
      await refresh();
    } catch (e) {
      const msg = String(e);
      if (msg.includes("enc_key_not_yet_exchanged")) {
        errorMsg =
          "Key exchange pending — message not sent. Try again shortly.";
      } else {
        errorMsg = msg;
      }
    }
  }

  async function send() {
    if (!input.trim() || !selectedPeer) return;
    const body = input;
    input = "";
    await sendBody(body);
  }

  function shareLocationOnce() {
    if (!selectedPeer || !navigator.geolocation) {
      errorMsg = "Location is not available on this device.";
      return;
    }
    locationLoading = true;
    navigator.geolocation.getCurrentPosition(
      async (pos) => {
        locationLoading = false;
        const body = formatLocation(
          pos.coords.latitude,
          pos.coords.longitude,
          pos.coords.accuracy,
        );
        await sendBody(body);
      },
      () => {
        locationLoading = false;
        errorMsg = "Could not get location. Check system location permissions.";
      },
      { enableHighAccuracy: true, timeout: 15_000, maximumAge: 60_000 },
    );
  }

  function onLocationClick() {
    if (!locationPrivacyAck) {
      showLocationPrivacy = true;
      return;
    }
    shareLocationOnce();
  }

  function confirmLocationPrivacy() {
    localStorage.setItem("location_privacy_acknowledged", "1");
    locationPrivacyAck = true;
    showLocationPrivacy = false;
    shareLocationOnce();
  }

  async function openMaps(body) {
    const coords = extractCoordinates(body);
    if (!coords) return;
    const url = `https://www.google.com/maps?q=${coords.lat},${coords.lon}`;
    window.open(url, "_blank");
  }

  $effect(() => {
    refresh();
    const unsubs = [];
    (async () => {
      unsubs.push(await listen("chat-updated", refresh));
      unsubs.push(await listen("contacts-updated", refresh));
      unsubs.push(await listen("metrics-updated", refreshContacts));
    })();
    return () => unsubs.forEach((u) => u?.());
  });
</script>

{#if showLocationPrivacy}
  <div class="modal-backdrop" role="presentation">
    <div class="modal" role="dialog">
      <h3>Share your location?</h3>
      <p>
        Your current coordinates will be sent once in this chat. Mycelium does
        not track your location continuously. The message is end-to-end
        encrypted when a key is exchanged.
      </p>
      <div class="modal-actions">
        <button type="button" class="secondary" onclick={() => (showLocationPrivacy = false)}>
          Cancel
        </button>
        <button type="button" onclick={confirmLocationPrivacy}>Share once</button>
      </div>
    </div>
  </div>
{/if}

<div class="chat-layout">
  <div class="sidebar">
    {#if pendingContacts.length > 0}
      <p class="section-label">Contact requests</p>
      {#each pendingContacts as c}
        <div class="pending-row">
          <span class="pending-name">{c.display_name || c.peer_id.slice(0, 16)}</span>
          <div class="pending-actions">
            <button type="button" class="small" onclick={() => acceptContact(c.peer_id)}>
              Accept
            </button>
            <button type="button" class="small secondary" onclick={() => rejectContact(c.peer_id)}>
              Decline
            </button>
          </div>
        </div>
      {/each}
    {/if}
    <p class="section-label">Contacts</p>
    {#if acceptedContacts.length === 0}
      <p class="empty-hint">No contacts yet. Scan a QR code on the Peers tab.</p>
    {/if}
    {#each acceptedContacts as c}
      <button
        onclick={() => ((selectedPeer = c.peer_id), refresh())}
        class:active={selectedPeer === c.peer_id}
      >
        {c.display_name || c.peer_id.slice(0, 16) + "…"}
      </button>
    {/each}
  </div>
  <div class="conversation">
    <div class="enc-badge" class:encrypted={isEncrypted}>
      {isEncrypted
        ? "🔒 End-to-end encrypted"
        : "⚠ Not encrypted – peer key not yet exchanged"}
    </div>
    {#if errorMsg}
      <div class="error-banner">{errorMsg}</div>
    {/if}
    <div class="messages">
      {#each messages as msg}
        <div class="msg" class:own={msg.from_peer === peerId}>
          <span class="name">{msg.from_display_name}</span>
          <span class="body">{msg.body}</span>
          {#if msg.body.startsWith("📍")}
            {@const coords = extractCoordinates(msg.body)}
            {#if coords}
              <button type="button" class="maps-link" onclick={() => openMaps(msg.body)}>
                Open in Maps
              </button>
            {/if}
          {/if}
        </div>
      {/each}
    </div>
    <div class="input-row">
      <button
        type="button"
        class="loc-btn"
        title="Share my location"
        disabled={!selectedPeer || locationLoading}
        onclick={onLocationClick}
      >
        {locationLoading ? "…" : "📍"}
      </button>
      <input
        bind:value={input}
        onkeydown={(e) => e.key === "Enter" && send()}
        placeholder="Message..."
        disabled={!selectedPeer}
      />
      <button onclick={send} disabled={!selectedPeer}>Send</button>
    </div>
  </div>
</div>

<style>
  .chat-layout {
    display: flex;
    height: 100%;
  }
  .sidebar {
    width: 240px;
    border-right: 1px solid var(--border);
    overflow-y: auto;
    padding: 8px;
  }
  .section-label {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--text-muted);
    margin: 8px 4px 4px;
  }
  .empty-hint {
    font-size: 12px;
    color: var(--text-muted);
    padding: 8px 4px;
    margin: 0;
  }
  .pending-row {
    padding: 6px 4px 10px;
    border-bottom: 1px solid var(--border);
    margin-bottom: 4px;
  }
  .pending-name {
    display: block;
    font-size: 13px;
    margin-bottom: 4px;
  }
  .pending-actions {
    display: flex;
    gap: 6px;
  }
  .sidebar button {
    display: block;
    width: 100%;
    text-align: left;
    padding: 8px;
    border-radius: 6px;
    border: none;
    background: none;
    cursor: pointer;
    font-size: 13px;
  }
  .sidebar button.active {
    background: var(--accent-subtle);
  }
  button.small {
    padding: 4px 8px;
    font-size: 12px;
  }
  button.secondary {
    background: none;
    color: inherit;
    border: 1px solid var(--border);
  }
  .conversation {
    flex: 1;
    display: flex;
    flex-direction: column;
  }
  .enc-badge {
    font-size: 12px;
    padding: 6px 10px;
    border-bottom: 1px solid var(--border);
    background: var(--surface-variant, rgba(128, 128, 128, 0.12));
    color: var(--text-muted);
  }
  .enc-badge.encrypted {
    background: var(--accent-subtle);
    color: var(--text);
  }
  .error-banner {
    font-size: 12px;
    padding: 8px 10px;
    background: rgba(200, 60, 60, 0.15);
    color: #c0392b;
    border-bottom: 1px solid var(--border);
  }
  .messages {
    flex: 1;
    overflow-y: auto;
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .msg {
    max-width: 70%;
    padding: 8px 12px;
    border-radius: 10px;
    background: var(--border);
  }
  .msg.own {
    align-self: flex-end;
    background: var(--accent-subtle);
  }
  .name {
    font-size: 11px;
    color: var(--text-muted);
    display: block;
    margin-bottom: 2px;
  }
  .maps-link {
    margin-top: 4px;
    padding: 0;
    border: none;
    background: none;
    color: var(--accent);
    font-size: 12px;
    cursor: pointer;
    text-decoration: underline;
  }
  .input-row {
    display: flex;
    gap: 8px;
    padding: 12px;
    border-top: 1px solid var(--border);
    align-items: center;
  }
  .loc-btn {
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: none;
    cursor: pointer;
    font-size: 16px;
  }
  .loc-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  input {
    flex: 1;
    padding: 8px 12px;
    border-radius: 6px;
    border: 1px solid var(--border);
    background: none;
    color: inherit;
    font-size: 14px;
  }
  button {
    padding: 8px 16px;
    background: var(--accent);
    color: white;
    border: none;
    border-radius: 6px;
    cursor: pointer;
    font-size: 14px;
  }
  button:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .modal-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.4);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }
  .modal {
    background: var(--sidebar-bg);
    border: 1px solid var(--border);
    border-radius: 12px;
    padding: 20px;
    max-width: 400px;
    margin: 16px;
  }
  .modal h3 {
    margin: 0 0 8px;
    font-size: 16px;
  }
  .modal p {
    font-size: 13px;
    color: var(--text-muted);
    margin: 0 0 16px;
    text-align: left;
  }
  .modal-actions {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
  }
  .modal-actions .secondary {
    background: none;
    color: inherit;
    border: 1px solid var(--border);
  }
</style>
