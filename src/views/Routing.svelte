<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { api, type ProcessInfo, type RoutingSettings, type RuleKind } from "../lib/api";
  import { app } from "../lib/app.svelte";
  import { translateError } from "../lib/i18n";
  import Icon from "../components/Icon.svelte";
  import Switch from "../components/Switch.svelte";
  import ProcessPicker from "../components/ProcessPicker.svelte";

  const routing = $derived(app.store!.routing);
  let inputs = $state<Record<RuleKind, string>>({ app: "", domain: "", ip: "" });
  let errors = $state<Record<RuleKind, string>>({ app: "", domain: "", ip: "" });
  let picker = $state(false);

  const listKey = { app: "apps", domain: "domains", ip: "ips" } as const;

  async function save(next: RoutingSettings) {
    const saved = await app.run(() => api.setRouting(next));
    if (saved && app.status.state === "connected") app.notify(app.t("routing.saved"));
  }

  async function addValues(kind: RuleKind, raw: string[]) {
    const key = listKey[kind];
    const list = [...routing[key]];
    for (const value of raw.map((v) => v.trim()).filter(Boolean)) {
      try {
        const norm = await api.normalizeRule(kind, value);
        if (!list.includes(norm)) list.push(norm);
      } catch (e) {
        errors = { ...errors, [kind]: `${value}: ${translateError(app.lang, e)}` };
        return false;
      }
    }
    errors = { ...errors, [kind]: "" };
    await save({ ...routing, [key]: list });
    return true;
  }

  async function submit(kind: RuleKind) {
    // Apps may contain spaces in paths, so only domains and IPs are split on whitespace.
    const parts = kind === "app" ? inputs[kind].split(/[,;\n]/) : inputs[kind].split(/[\s,;]+/);
    if (await addValues(kind, parts)) inputs = { ...inputs, [kind]: "" };
  }

  async function remove(kind: RuleKind, value: string) {
    const key = listKey[kind];
    await save({ ...routing, [key]: routing[key].filter((v) => v !== value) });
  }

  async function pickExe() {
    const files = await open({ multiple: true, filters: [{ name: "Programs", extensions: ["exe"] }] });
    if (files) await addValues("app", Array.isArray(files) ? files : [files]);
  }

  async function pickFolder() {
    const dir = await open({ directory: true });
    if (typeof dir === "string") await addValues("app", [dir.replace(/[\\/]*$/, "/")]);
  }

  async function pickRunning(list: ProcessInfo[]) {
    picker = false;
    await addValues("app", list.map((p) => p.path));
  }

  function appLabel(v: string): { name: string; detail: string } {
    if (v.endsWith("/")) return { name: v.slice(0, -1).split("/").pop() + "/", detail: v };
    if (v.includes("/")) return { name: v.split("/").pop()!, detail: v.slice(0, v.lastIndexOf("/")) };
    return { name: v, detail: "" };
  }
</script>

<div class="section-title">{app.t("routing.mode")}</div>
<div class="segmented card">
  <button class:active={routing.listMode === "exclude"} onclick={() => save({ ...routing, listMode: "exclude" })}>
    {app.t("routing.exclude")}
  </button>
  <button class:active={routing.listMode === "include"} onclick={() => save({ ...routing, listMode: "include" })}>
    {app.t("routing.include")}
  </button>
</div>
<p class="hint mode-hint">{app.t(routing.listMode === "exclude" ? "routing.excludeHint" : "routing.includeHint")}</p>

<div class="section-title">{app.t("routing.presets")}</div>
<section class="card">
  <div class="list-row">
    <span class="flag">RU</span>
    <span class="spacer">{app.t("routing.ruDirect")}</span>
    <Switch checked={routing.ruDirect} onchange={(v) => save({ ...routing, ruDirect: v })} />
  </div>
  <div class="list-row">
    <Icon name="home" size={16} />
    <span class="spacer">{app.t("routing.lanDirect")}</span>
    <Switch checked={routing.lanDirect} onchange={(v) => save({ ...routing, lanDirect: v })} />
  </div>
  <div class="list-row">
    <Icon name="shield" size={16} />
    <span class="spacer">{app.t("routing.blockAds")}</span>
    <Switch checked={routing.blockAds} onchange={(v) => save({ ...routing, blockAds: v })} />
  </div>
</section>

{#snippet ruleList(kind: RuleKind, title: string, placeholder: string)}
  <div class="section-title">{title} · {routing[listKey[kind]].length}</div>
  <section class="card rules">
    {#if kind === "app"}
      <div class="app-actions">
        <button class="btn small" onclick={pickExe}><Icon name="file" size={14} />{app.t("routing.pickExe")}</button>
        <button class="btn small" onclick={pickFolder}><Icon name="folder" size={14} />{app.t("routing.pickFolder")}</button>
        <button class="btn small" onclick={() => (picker = true)}><Icon name="list" size={14} />{app.t("routing.running")}</button>
      </div>
    {/if}
    <form class="add-row" onsubmit={(e) => { e.preventDefault(); submit(kind); }}>
      <input type="text" bind:value={inputs[kind]} {placeholder} spellcheck="false" />
      <button class="btn small" type="submit" disabled={!inputs[kind].trim()}><Icon name="plus" size={14} /></button>
    </form>
    {#if errors[kind]}<div class="error">{errors[kind]}</div>{/if}
    {#each routing[listKey[kind]] as value (value)}
      <div class="list-row rule">
        {#if kind === "app"}
          {@const l = appLabel(value)}
          <Icon name={value.endsWith("/") ? "folder" : "file"} size={15} />
          <span class="rule-text">
            <span class="ellipsis">{l.name}</span>
            {#if l.detail}<span class="hint ellipsis">{l.detail}</span>{/if}
          </span>
        {:else}
          <span class="rule-text mono ellipsis">{value.replace(/^domain:/, "")}</span>
        {/if}
        <button class="icon-btn" onclick={() => remove(kind, value)} aria-label="remove"><Icon name="x" size={14} /></button>
      </div>
    {/each}
  </section>
{/snippet}

{@render ruleList("app", app.t("routing.apps"), app.t("routing.appPlaceholder"))}
<p class="hint note">{app.t("routing.appsNote")}</p>
{@render ruleList("domain", app.t("routing.domains"), app.t("routing.domainPlaceholder"))}
{@render ruleList("ip", app.t("routing.ips"), app.t("routing.ipPlaceholder"))}

{#if picker}
  <ProcessPicker onpick={pickRunning} onclose={() => (picker = false)} />
{/if}

<style>
  .segmented {
    display: grid;
    grid-template-columns: 1fr 1fr;
    padding: 4px;
    gap: 4px;
  }

  .segmented button {
    padding: 8px 6px;
    border-radius: 9px;
    font-size: 13px;
    font-weight: 500;
    color: var(--text-2);
  }

  .segmented button.active {
    background: var(--accent-soft);
    color: var(--accent);
  }

  .mode-hint {
    margin: 8px 4px 0;
  }

  .flag {
    font-size: 10.5px;
    font-weight: 700;
    padding: 1px 4px;
    border-radius: 4px;
    background: var(--surface-2);
    color: var(--text-2);
  }

  .rules {
    overflow: hidden;
  }

  .app-actions {
    display: flex;
    gap: 6px;
    padding: 10px 10px 0;
    flex-wrap: wrap;
  }

  .add-row {
    display: flex;
    gap: 6px;
    padding: 10px;
  }

  .error {
    color: var(--bad);
    font-size: 12.5px;
    padding: 0 12px 8px;
    user-select: text;
  }

  .rule {
    padding: 6px 6px 6px 12px;
    color: var(--text-2);
  }

  .rule-text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    color: var(--text);
  }

  .note {
    margin: 6px 4px 0;
  }
</style>
