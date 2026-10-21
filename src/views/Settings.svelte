<script lang="ts">
  import { api, type CoreUpdateInfo, type Settings } from "../lib/api";
  import { app } from "../lib/app.svelte";
  import { formatDate } from "../lib/format";
  import Icon from "../components/Icon.svelte";
  import Switch from "../components/Switch.svelte";

  const snap = $derived(app.snapshot!);
  const s = $derived(snap.store.settings);

  let port = $state(0);
  let subHours = $state(0);
  let geoRepo = $state("");
  $effect(() => {
    port = s.mixedPort;
    subHours = s.subUpdateHours;
    geoRepo = s.geoRepo;
  });

  let update = $state<CoreUpdateInfo | null>(null);
  let checking = $state(false);
  let installing = $state(false);
  let geoBusy = $state(false);
  let logs = $state<string[] | null>(null);

  async function save(patch: Partial<Settings>) {
    await app.run(() => api.setSettings({ ...s, ...patch }));
  }

  async function check() {
    checking = true;
    update = (await app.run(api.checkCoreUpdate)) ?? null;
    checking = false;
    if (update && !update.newer) app.notify(app.t("settings.upToDate"));
  }

  async function install() {
    installing = true;
    const tag = await app.run(api.installCoreUpdate);
    installing = false;
    if (tag) {
      update = null;
      app.notify(`Xray ${tag} ✓`);
      await app.reload();
    }
  }

  async function updateGeo() {
    geoBusy = true;
    const ok = (await app.run(api.updateGeo)) !== undefined;
    geoBusy = false;
    if (ok) app.notify(app.t("settings.geoUpdated"));
  }

  async function toggleLogs() {
    logs = logs ? null : ((await app.run(api.logs)) ?? []);
  }
</script>

<div class="section-title">{app.t("settings.connection")}</div>
<section class="card">
  <div class="list-row column">
    <span>{app.t("settings.mode")}</span>
    <label class="radio-row">
      <input type="radio" name="mode" checked={s.mode === "tun"} onchange={() => save({ mode: "tun" })} />
      {app.t("settings.mode.tun")}
    </label>
    <label class="radio-row">
      <input type="radio" name="mode" checked={s.mode === "systemProxy"} onchange={() => save({ mode: "systemProxy" })} />
      {app.t("settings.mode.proxy")}
    </label>
  </div>
  <div class="list-row">
    <span class="spacer">{app.t("settings.port")}</span>
    <input class="num" type="number" min="1024" max="65535" bind:value={port} onchange={() => save({ mixedPort: Number(port) })} />
  </div>
  <div class="list-row">
    <span class="spacer">{app.t("settings.ipv6")}</span>
    <Switch checked={s.ipv6} onchange={(v) => save({ ipv6: v })} />
  </div>
</section>

<div class="section-title">{app.t("settings.startup")}</div>
<section class="card">
  <div class="list-row">
    <span class="spacer">{app.t("settings.autostart")}</span>
    <Switch checked={s.autostart} onchange={(v) => save({ autostart: v })} />
  </div>
  <div class="list-row">
    <span class="spacer">{app.t("settings.connectOnStart")}</span>
    <Switch checked={s.connectOnStart} onchange={(v) => save({ connectOnStart: v })} />
  </div>
  <div class="list-row">
    <span class="spacer">{app.t("settings.startMinimized")}</span>
    <Switch checked={s.startMinimized} onchange={(v) => save({ startMinimized: v })} />
  </div>
</section>

<div class="section-title">{app.t("settings.interface")}</div>
<section class="card">
  <div class="list-row">
    <span class="spacer">{app.t("settings.language")}</span>
    <select class="select" value={s.language} onchange={(e) => save({ language: (e.target as HTMLSelectElement).value as Settings["language"] })}>
      <option value="auto">{app.t("settings.language.auto")}</option>
      <option value="ru">Русский</option>
      <option value="en">English</option>
    </select>
  </div>
</section>

<div class="section-title">{app.t("settings.subs")}</div>
<section class="card">
  <div class="list-row">
    <span class="spacer">{app.t("settings.subHours")}</span>
    <input class="num" type="number" min="1" max="168" bind:value={subHours} onchange={() => save({ subUpdateHours: Math.max(1, Number(subHours)) })} />
  </div>
</section>

<div class="section-title">{app.t("settings.core")}</div>
<section class="card">
  <div class="list-row">
    <span class="spacer">{app.t("settings.coreVersion")}</span>
    <span class="mono">{snap.coreVersion ?? "—"}</span>
  </div>
  <div class="list-row">
    <span class="spacer">{app.t("settings.prerelease")}</span>
    <Switch checked={s.corePrerelease} onchange={(v) => save({ corePrerelease: v })} />
  </div>
  <div class="list-row">
    <button class="btn small" onclick={check} disabled={checking || installing}><Icon name="refresh" size={14} />{app.t("settings.checkUpdate")}</button>
    <span class="spacer"></span>
    {#if update?.newer}
      <button class="btn small primary" onclick={install} disabled={installing}>
        {app.t("settings.install", { v: update.latest.tag })}
      </button>
    {/if}
  </div>
</section>

<div class="section-title">{app.t("settings.geo")}</div>
<section class="card">
  <div class="list-row column">
    <span class="hint">{app.t("settings.geoRepo")}</span>
    <input type="text" class="mono" bind:value={geoRepo} onchange={() => save({ geoRepo: geoRepo.trim() })} spellcheck="false" />
  </div>
  <div class="list-row">
    <button class="btn small" onclick={updateGeo} disabled={geoBusy}><Icon name="refresh" size={14} />{app.t("settings.geoUpdate")}</button>
    <span class="spacer"></span>
    {#if s.lastGeoUpdate}<span class="hint">{formatDate(s.lastGeoUpdate, app.lang)}</span>{/if}
  </div>
</section>

<div class="section-title">{app.t("settings.about")}</div>
<section class="card">
  <div class="list-row">
    <span class="spacer">LiteRay {snap.appVersion}</span>
    <span class="hint">{app.t(snap.portable ? "settings.portable" : "settings.installed")}</span>
  </div>
  <div class="list-row">
    <button class="btn small" onclick={toggleLogs}><Icon name="terminal" size={14} />{app.t("settings.logs")}</button>
    <button class="btn small" onclick={() => app.run(api.openDataDir)}><Icon name="folder" size={14} />{app.t("settings.openData")}</button>
  </div>
  {#if logs}
    <pre class="logs mono scroll">{logs.length ? logs.join("\n") : "—"}</pre>
  {/if}
</section>

<style>
  .column {
    flex-direction: column;
    align-items: stretch;
    gap: 6px;
  }

  .radio-row {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-2);
    cursor: pointer;
  }

  .radio-row input {
    accent-color: var(--accent);
    margin: 0;
  }

  .num {
    width: 92px;
    text-align: right;
  }

  .select {
    width: auto;
    min-width: 150px;
  }

  .logs {
    margin: 0;
    padding: 10px 14px;
    max-height: 240px;
    border-top: 1px solid var(--border);
    background: var(--surface-2);
    white-space: pre-wrap;
    word-break: break-all;
    user-select: text;
  }
</style>
