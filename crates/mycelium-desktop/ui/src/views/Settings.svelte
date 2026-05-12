<script>
  let { peerId } = $props();
</script>

<div class="settings">
  <h3>Settings</h3>
  <p>Peer ID: <code>{peerId}</code></p>
  <p>Further desktop preferences can be added here (theme, notifications, startup behavior).</p>
</div>

<style>
  .settings { padding: 16px; }
  code { font-family: ui-monospace, monospace; }
</style>
