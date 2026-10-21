<script lang="ts">
  import { onMount } from "svelte";
  import { app } from "./lib/app.svelte";
  import Icon from "./components/Icon.svelte";
  import AdminDialog from "./components/AdminDialog.svelte";
  import Home from "./views/Home.svelte";
  import Servers from "./views/Servers.svelte";
  import Routing from "./views/Routing.svelte";
  import Settings from "./views/Settings.svelte";

  type Tab = "home" | "servers" | "routing" | "settings";
  let tab = $state<Tab>("home");

  const tabs: { id: Tab; icon: string; key: "tab.home" | "tab.servers" | "tab.routing" | "tab.settings" }[] = [
    { id: "home", icon: "home", key: "tab.home" },
    { id: "servers", icon: "server", key: "tab.servers" },
    { id: "routing", icon: "route", key: "tab.routing" },
    { id: "settings", icon: "settings", key: "tab.settings" },
  ];

  onMount(() => {
    app.init().catch((e) => app.showError(e));
  });

  $effect(() => {
    document.documentElement.lang = app.lang;
  });
</script>

<div class="shell">
  <main class="scroll">
    {#if app.snapshot}
      {#if tab === "home"}
        <Home onopenservers={() => (tab = "servers")} />
      {:else if tab === "servers"}
        <Servers />
      {:else if tab === "routing"}
        <Routing />
      {:else}
        <Settings />
      {/if}
    {/if}
  </main>

  <nav>
    {#each tabs as t (t.id)}
      <button class:active={tab === t.id} onclick={() => (tab = t.id)}>
        <Icon name={t.icon} size={20} />
        <span>{app.t(t.key)}</span>
      </button>
    {/each}
  </nav>

  {#if app.toast}
    <div class="toast" class:error={app.toast.error} role="status">{app.toast.text}</div>
  {/if}

  {#if app.needAdmin}
    <AdminDialog />
  {/if}
</div>

<style>
  .shell {
    height: 100%;
    display: flex;
    flex-direction: column;
    position: relative;
  }

  main {
    flex: 1;
    padding: 16px 16px 24px;
  }

  nav {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    background: var(--surface);
    border-top: 1px solid var(--border);
    padding: 6px 6px 8px;
  }

  nav button {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    padding: 6px 0;
    border-radius: 10px;
    color: var(--text-3);
    font-size: 11.5px;
    font-weight: 500;
  }

  nav button:hover {
    color: var(--text-2);
  }

  nav button.active {
    color: var(--accent);
    background: var(--accent-soft);
  }

  .toast {
    position: absolute;
    left: 16px;
    right: 16px;
    bottom: 76px;
    padding: 10px 14px;
    border-radius: var(--radius-sm);
    background: var(--text);
    color: var(--bg);
    font-size: 13px;
    box-shadow: 0 6px 24px rgba(0, 0, 0, 0.2);
    animation: rise 0.18s ease-out;
    user-select: text;
  }

  .toast.error {
    background: var(--bad);
    color: #fff;
  }

  @keyframes rise {
    from {
      transform: translateY(8px);
      opacity: 0;
    }
  }
</style>
