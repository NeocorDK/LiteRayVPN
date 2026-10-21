<script lang="ts">
  import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { ask } from "@tauri-apps/plugin-dialog";
  import { api, type Profile, type Subscription } from "../lib/api";
  import { app } from "../lib/app.svelte";
  import { formatBytes, formatDate, pingClass } from "../lib/format";
  import Icon from "../components/Icon.svelte";

  let input = $state("");
  let adding = $state(false);
  let refreshing = $state<Record<string, boolean>>({});
  let editing = $state<string | null>(null);
  let editName = $state("");

  const store = $derived(app.store!);
  const groups = $derived.by(() => {
    const result: { sub: Subscription | null; profiles: Profile[] }[] = store.subscriptions.map((sub) => ({
      sub,
      profiles: store.profiles.filter((p) => p.subscriptionId === sub.id),
    }));
    const manual = store.profiles.filter((p) => !p.subscriptionId || !store.subscriptions.some((s) => s.id === p.subscriptionId));
    if (manual.length) result.push({ sub: null, profiles: manual });
    return result;
  });

  async function add(text: string) {
    if (!text.trim()) return;
    adding = true;
    const res = await app.run(() => api.importText(text));
    adding = false;
    if (!res) return;
    input = "";
    if (res.subscription) {
      app.notify(app.t("servers.subAdded"));
    } else {
      let msg = app.t("servers.imported", { n: res.added });
      if (res.skipped) msg += `, ${app.t("servers.skipped", { n: res.skipped })}`;
      app.notify(msg);
    }
    app.pingAll();
  }

  async function paste() {
    const text = await app.run(readText);
    if (text) await add(text);
  }

  async function refresh(id: string) {
    refreshing = { ...refreshing, [id]: true };
    await app.run(() => api.refreshSubscription(id));
    refreshing = { ...refreshing, [id]: false };
  }

  async function removeSub(sub: Subscription) {
    if (await ask(app.t("servers.confirmDeleteSub"), { title: sub.name || sub.url, kind: "warning" })) {
      await app.run(() => api.deleteSubscription(sub.id));
    }
  }

  async function saveName(sub: Subscription) {
    editing = null;
    if (editName.trim() && editName !== sub.name) await app.run(() => api.renameSubscription(sub.id, editName));
  }

  async function copyLink(p: Profile) {
    await app.run(() => writeText(p.link));
    app.notify(app.t("servers.copyLink") + " ✓");
  }

  function usage(sub: Subscription): string | null {
    const u = sub.userinfo;
    if (!u) return null;
    const used = formatBytes(u.upload + u.download, app.lang);
    const total = u.total > 0 ? formatBytes(u.total, app.lang) : app.t("servers.unlimited");
    let s = `${app.t("servers.used")} ${used} ${app.t("servers.of")} ${total}`;
    if (u.expire) s += ` · ${app.t("servers.expires")} ${formatDate(u.expire, app.lang)}`;
    return s;
  }

  function usagePercent(sub: Subscription): number | null {
    const u = sub.userinfo;
    if (!u || u.total <= 0) return null;
    return Math.min(100, ((u.upload + u.download) / u.total) * 100);
  }

  function hostName(url: string): string {
    try {
      return new URL(url).host;
    } catch {
      return url;
    }
  }
</script>

<section class="add card">
  <textarea bind:value={input} placeholder={app.t("servers.addHint")} rows="2" spellcheck="false"></textarea>
  <div class="row">
    <button class="btn" onclick={paste} disabled={adding}><Icon name="clipboard" size={16} />{app.t("servers.paste")}</button>
    <span class="spacer"></span>
    <button class="btn primary" onclick={() => add(input)} disabled={adding || !input.trim()}>
      <Icon name="plus" size={16} />{app.t("servers.add")}
    </button>
  </div>
</section>

<div class="toolbar">
  <span class="section-title">{app.t("tab.servers")} · {store.profiles.length}</span>
  <span class="spacer"></span>
  {#if store.profiles.length}
    <button class="btn small" onclick={() => app.pingAll()}><Icon name="zap" size={14} />{app.t("servers.pingAll")}</button>
  {/if}
</div>

{#if !groups.length}
  <p class="empty muted">{app.t("servers.empty")}</p>
{/if}

{#each groups as group (group.sub?.id ?? "manual")}
  <section class="group card">
    <header>
      {#if group.sub}
        {@const sub = group.sub}
        <div class="sub-head">
          {#if editing === sub.id}
            <!-- svelte-ignore a11y_autofocus -->
            <input type="text" bind:value={editName} autofocus onblur={() => saveName(sub)} onkeydown={(e) => e.key === "Enter" && saveName(sub)} />
          {:else}
            <div class="sub-name ellipsis">{sub.name || hostName(sub.url)}</div>
          {/if}
          <div class="hint ellipsis">
            {#if sub.lastError}
              <span class="bad">{sub.lastError}</span>
            {:else if sub.lastUpdate}
              {app.t("servers.updated")} {formatDate(sub.lastUpdate, app.lang, true)}
              {#if sub.skipped}· {app.t("servers.skipped", { n: sub.skipped })}{/if}
            {:else}
              {app.t("servers.never")}
            {/if}
          </div>
        </div>
        <button class="icon-btn" title={app.t("servers.refresh")} onclick={() => refresh(sub.id)} disabled={refreshing[sub.id]}>
          <span class:spin={refreshing[sub.id]}><Icon name="refresh" size={16} /></span>
        </button>
        <button class="icon-btn" title={app.t("servers.rename")} onclick={() => { editing = sub.id; editName = sub.name; }}>
          <Icon name="edit" size={16} />
        </button>
        <button class="icon-btn" title={app.t("servers.delete")} onclick={() => removeSub(sub)}><Icon name="trash" size={16} /></button>
      {:else}
        <div class="sub-head"><div class="sub-name">{app.t("servers.manual")}</div></div>
      {/if}
    </header>

    {#if group.sub && usage(group.sub)}
      {@const pct = usagePercent(group.sub)}
      <div class="usage">
        <div class="hint">{usage(group.sub)}</div>
        {#if pct != null}
          <div class="bar"><div class="fill" class:warn={pct > 85} style="width: {pct}%"></div></div>
        {/if}
      </div>
    {/if}

    {#each group.profiles as p (p.id)}
      {@const ping = app.pings[p.id]}
      <div class="list-row profile" class:selected={p.id === store.selected}>
        <button class="pick" onclick={() => app.run(() => api.selectProfile(p.id))}>
          <span class="radio"></span>
          <span class="info">
            <span class="name ellipsis">{p.name}</span>
            <span class="meta">{p.server.protocol?.toUpperCase()} · {p.server.network}{p.server.security !== "none" ? ` · ${p.server.security}` : ""}</span>
          </span>
        </button>
        <span class="ping {ping === 'pending' ? '' : pingClass(ping as number | null)}">
          {#if ping === "pending"}…{:else if typeof ping === "number"}{ping} ms{:else if ping === null}{app.t("servers.timeout")}{/if}
        </span>
        <button class="icon-btn" title={app.t("servers.copyLink")} onclick={() => copyLink(p)}><Icon name="copy" size={15} /></button>
        {#if !p.subscriptionId}
          <button class="icon-btn" title={app.t("servers.delete")} onclick={() => app.run(() => api.deleteProfile(p.id))}>
            <Icon name="trash" size={15} />
          </button>
        {/if}
      </div>
    {/each}
  </section>
{/each}

<style>
  .add {
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .toolbar {
    display: flex;
    align-items: center;
    margin-top: 6px;
  }

  .toolbar .section-title {
    margin: 12px 4px 8px;
  }

  .empty {
    text-align: center;
    margin-top: 32px;
  }

  .group {
    margin-bottom: 12px;
    overflow: hidden;
  }

  header {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 10px 8px 8px 14px;
    background: var(--surface-2);
    border-bottom: 1px solid var(--border);
  }

  .sub-head {
    flex: 1;
    min-width: 0;
  }

  .sub-name {
    font-weight: 600;
  }

  .usage {
    padding: 8px 14px;
    border-bottom: 1px solid var(--border);
  }

  .bar {
    margin-top: 5px;
    height: 4px;
    border-radius: 4px;
    background: var(--surface-2);
    overflow: hidden;
  }

  .fill {
    height: 100%;
    background: linear-gradient(90deg, var(--accent), var(--accent-2));
  }

  .fill.warn {
    background: var(--bad);
  }

  .profile {
    padding: 6px 8px 6px 6px;
    gap: 4px;
  }

  .profile.selected {
    background: var(--accent-soft);
  }

  .pick {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 4px 6px;
    text-align: left;
  }

  .radio {
    width: 16px;
    height: 16px;
    border-radius: 50%;
    border: 2px solid var(--border);
    flex-shrink: 0;
  }

  .selected .radio {
    border: 5px solid var(--accent);
  }

  .info {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .name {
    font-weight: 500;
  }

  .meta {
    font-size: 11.5px;
    color: var(--text-3);
  }

  .ping {
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    min-width: 44px;
    text-align: right;
    color: var(--text-3);
  }

  .ping.good {
    color: var(--good);
  }

  .ping.ok {
    color: var(--ok);
  }

  .ping.bad {
    color: var(--bad);
  }

  .spin {
    display: inline-flex;
    animation: spin 0.9s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
