<script>
  import { invoke } from "@tauri-apps/api/core";
  let scope = $state("mycelium/general");
  let title = $state("");
  let body = $state("");
  let posts = $state([]);

  async function refresh() {
    posts = await invoke("bulletins_for_scope", { scope });
  }

  async function post() {
    if (!title || !body) return;
    await invoke("post_bulletin", { scope, title, body, ttlSecs: 86400 });
    title = "";
    body = "";
    await refresh();
  }

  $effect(() => {
    refresh();
    const t = setInterval(refresh, 4000);
    return () => clearInterval(t);
  });
</script>

<div class="bulletin">
  <section>
    <h3>Post</h3>
    <input bind:value={scope} placeholder="Scope" />
    <input bind:value={title} placeholder="Title" />
    <textarea bind:value={body} placeholder="Body"></textarea>
    <button onclick={post}>Post Bulletin</button>
  </section>
  <section>
    <h3>Board</h3>
    {#each posts as p}
      <article>
        <strong>{p.title}</strong>
        <div>{p.from_display_name} - {p.scope}</div>
        <p>{p.body}</p>
      </article>
    {/each}
  </section>
</div>

<style>
  .bulletin { display: grid; grid-template-columns: 1fr 1fr; gap: 16px; padding: 16px; height: 100%; overflow: auto; }
  section { border: 1px solid var(--border); border-radius: 10px; padding: 12px; display: flex; flex-direction: column; gap: 8px; }
  input, textarea { border: 1px solid var(--border); border-radius: 6px; padding: 8px; background: none; color: inherit; }
  textarea { min-height: 120px; }
  button { padding: 8px 12px; border: none; border-radius: 6px; background: var(--accent); color: #fff; }
  article { border-top: 1px solid var(--border); padding-top: 8px; }
</style>
