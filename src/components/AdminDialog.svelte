<script lang="ts">
  import { api } from "../lib/api";
  import { app } from "../lib/app.svelte";
  import Icon from "./Icon.svelte";

  async function useProxy() {
    const settings = app.store?.settings;
    if (!settings) return;
    app.needAdmin = false;
    await app.run(() => api.setSettings({ ...settings, mode: "systemProxy" }));
    await app.run(api.connect);
  }
</script>

<div class="backdrop" role="presentation" onclick={() => (app.needAdmin = false)}>
  <div class="dialog card" role="dialog" aria-modal="true" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.key === "Escape" && (app.needAdmin = false)}>
    <div class="badge"><Icon name="shield" size={22} /></div>
    <h3>{app.t("admin.title")}</h3>
    <p class="muted">{app.t("admin.text")}</p>
    <button class="btn primary" onclick={() => app.run(api.restartAsAdmin)}>{app.t("admin.restart")}</button>
    <button class="btn" onclick={useProxy}>{app.t("admin.useProxy")}</button>
  </div>
</div>

<style>
  .backdrop {
    position: absolute;
    inset: 0;
    background: rgba(10, 12, 20, 0.45);
    display: grid;
    place-items: center;
    padding: 16px;
    z-index: 10;
  }

  .dialog {
    width: 100%;
    max-width: 340px;
    padding: 20px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    text-align: center;
  }

  .badge {
    width: 44px;
    height: 44px;
    margin: 0 auto;
    border-radius: 12px;
    display: grid;
    place-items: center;
    background: var(--accent-soft);
    color: var(--accent);
  }

  h3 {
    margin: 4px 0 0;
    font-size: 16px;
  }

  p {
    margin: 0 0 6px;
  }
</style>
