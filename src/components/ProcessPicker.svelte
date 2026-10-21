<script lang="ts">
  import { onMount } from "svelte";
  import { api, type ProcessInfo } from "../lib/api";
  import { app } from "../lib/app.svelte";
  import Icon from "./Icon.svelte";

  let { onpick, onclose }: { onpick: (list: ProcessInfo[]) => void; onclose: () => void } = $props();

  let processes = $state<ProcessInfo[] | null>(null);
  let query = $state("");
  let chosen = $state<Set<string>>(new Set());

  const filtered = $derived(
    (processes ?? []).filter((p) => {
      const q = query.trim().toLowerCase();
      return !q || p.name.toLowerCase().includes(q) || p.path.toLowerCase().includes(q);
    }),
  );

  onMount(async () => {
    processes = (await app.run(api.listProcesses)) ?? [];
  });

  function toggle(path: string) {
    const next = new Set(chosen);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    chosen = next;
  }
</script>

<div class="backdrop" role="presentation" onclick={onclose}>
  <div class="sheet card" role="dialog" aria-modal="true" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.key === "Escape" && onclose()}>
    <div class="head">
      <strong>{app.t("routing.running")}</strong>
      <span class="spacer"></span>
      <button class="icon-btn" onclick={onclose} aria-label={app.t("routing.close")}><Icon name="x" size={16} /></button>
    </div>
    <!-- svelte-ignore a11y_autofocus -->
    <input type="text" bind:value={query} placeholder={app.t("routing.search")} autofocus />
    <div class="items scroll">
      {#if processes === null}
        <p class="hint">…</p>
      {/if}
      {#each filtered as p (p.path)}
        <label class="item" title={p.path}>
          <input type="checkbox" checked={chosen.has(p.path)} onchange={() => toggle(p.path)} />
          <span class="text">
            <span class="ellipsis">{p.name}</span>
            <span class="hint ellipsis">{p.path}</span>
          </span>
        </label>
      {/each}
    </div>
    <button class="btn primary" disabled={!chosen.size} onclick={() => onpick((processes ?? []).filter((p) => chosen.has(p.path)))}>
      {app.t("routing.add")}{chosen.size ? ` (${chosen.size})` : ""}
    </button>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(10, 12, 20, 0.45);
    display: flex;
    align-items: flex-end;
    justify-content: center;
    padding: 24px 12px 12px;
    z-index: 20;
  }

  .sheet {
    width: 100%;
    max-width: 520px;
    max-height: 100%;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px;
  }

  .head {
    display: flex;
    align-items: center;
  }

  .items {
    flex: 1;
    min-height: 120px;
    margin: 0 -4px;
  }

  .item {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 8px;
    border-radius: 8px;
    cursor: pointer;
  }

  .item:hover {
    background: var(--surface-2);
  }

  .item input {
    accent-color: var(--accent);
  }

  .text {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
</style>
