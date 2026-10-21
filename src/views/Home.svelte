<script lang="ts">
  import { onDestroy } from "svelte";
  import { app } from "../lib/app.svelte";
  import { formatBytes, formatDuration, formatSpeed, pingClass } from "../lib/format";
  import Icon from "../components/Icon.svelte";
  import Sparkline from "../components/Sparkline.svelte";

  let { onopenservers }: { onopenservers: () => void } = $props();

  let now = $state(Date.now() / 1000);
  const timer = setInterval(() => (now = Date.now() / 1000), 1000);
  onDestroy(() => clearInterval(timer));

  const status = $derived(app.status);
  const connected = $derived(status.state === "connected");
  const busy = $derived(status.state === "connecting");
  const selected = $derived(app.store?.profiles.find((p) => p.id === app.store?.selected));
  const mode = $derived(app.store?.settings.mode ?? "tun");
  const stats = $derived(app.stats);
  const listPing = $derived(selected ? app.pings[selected.id] : undefined);

  const statusText = $derived(
    status.state === "connected"
      ? app.t("status.connected")
      : status.state === "connecting"
        ? app.t("status.connecting")
        : status.state === "error"
          ? app.t("status.error")
          : app.t("status.disconnected"),
  );
</script>

<section class="hero card" class:on={connected}>
  <div class="status-line">
    <span class="dot" class:on={connected} class:busy></span>
    <span class="status-text">{statusText}</span>
    <span class="spacer"></span>
    <span class="mode">{app.t(mode === "tun" ? "home.mode.tun" : "home.mode.systemProxy")}</span>
  </div>

  <button
    class="power"
    class:on={connected}
    class:busy
    disabled={!selected && !connected}
    onclick={() => app.toggleConnection()}
    aria-label={connected ? app.t("home.disconnect") : app.t("home.connect")}
  >
    <Icon name="power" size={44} />
  </button>
  <div class="power-label">{connected || busy ? app.t("home.disconnect") : app.t("home.connect")}</div>

  {#if selected}
    <button class="server" onclick={onopenservers}>
      <Icon name="globe" size={16} />
      <span class="ellipsis">{selected.name}</span>
      <span class="proto">{selected.server.protocol?.toUpperCase()} · {selected.server.network}{selected.server.security !== "none" ? ` · ${selected.server.security}` : ""}</span>
    </button>
  {:else}
    <button class="server empty" onclick={onopenservers}>
      <Icon name="plus" size={16} />
      <span>{app.t("home.addFirst")}</span>
    </button>
  {/if}
</section>

<section class="metrics">
  <div class="metric card">
    <div class="label"><Icon name="activity" size={14} /> {app.t("home.ping")}</div>
    {#if connected}
      {#if stats?.delayMs != null}
        <div class="value {pingClass(stats.delayMs)}">{stats.delayMs} <small>ms</small></div>
      {:else}
        <div class="value dim">{app.t("home.measuring")}</div>
      {/if}
    {:else if typeof listPing === "number"}
      <div class="value {pingClass(listPing)}">{listPing} <small>ms</small></div>
    {:else}
      <div class="value dim">—</div>
    {/if}
  </div>
  <div class="metric card">
    <div class="label"><Icon name="clock" size={14} /> {app.t("home.uptime")}</div>
    <div class="value">{status.state === "connected" ? formatDuration(now - status.since) : "—"}</div>
  </div>
  <div class="metric card">
    <div class="label"><Icon name="down" size={14} /> {app.t("home.download")}</div>
    <div class="value">{connected && stats ? formatSpeed(stats.downSpeed, app.lang) : "—"}</div>
  </div>
  <div class="metric card">
    <div class="label"><Icon name="up" size={14} /> {app.t("home.upload")}</div>
    <div class="value">{connected && stats ? formatSpeed(stats.upSpeed, app.lang) : "—"}</div>
  </div>
</section>

{#if connected}
  <section class="chart card">
    <Sparkline points={app.history} />
    <div class="totals">
      <span>{app.t("home.session")}: ↓ {formatBytes(stats?.totalDown ?? 0, app.lang)} · ↑ {formatBytes(stats?.totalUp ?? 0, app.lang)}</span>
      {#if stats && stats.directDownSpeed + stats.directUpSpeed > 0}
        <span class="hint">{app.t("home.direct")}: ↓ {formatSpeed(stats.directDownSpeed, app.lang)}</span>
      {/if}
    </div>
  </section>
{/if}

<style>
  .hero {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 16px 16px 14px;
    position: relative;
    overflow: hidden;
  }

  .hero::before {
    content: "";
    position: absolute;
    inset: -40% -20% auto;
    height: 260px;
    background: radial-gradient(closest-side, var(--accent-soft), transparent);
    opacity: 0;
    transition: opacity 0.4s;
    pointer-events: none;
  }

  .hero.on::before {
    opacity: 1;
  }

  .status-line {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
  }

  .dot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--text-3);
  }

  .dot.on {
    background: var(--good);
    box-shadow: 0 0 0 4px var(--good-soft);
  }

  .dot.busy {
    background: var(--ok);
    animation: pulse 1s infinite;
  }

  .status-text {
    font-weight: 600;
  }

  .mode {
    color: var(--text-3);
    font-size: 12px;
  }

  .power {
    margin-top: 22px;
    width: 128px;
    height: 128px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    color: var(--text-2);
    background: var(--surface-2);
    border: 1px solid var(--border);
    transition: all 0.25s;
    position: relative;
  }

  .power:hover:not(:disabled) {
    color: var(--accent);
    border-color: var(--accent);
  }

  .power.on {
    color: #fff;
    border-color: transparent;
    background: linear-gradient(135deg, var(--accent), var(--accent-2));
    box-shadow: 0 10px 40px -8px var(--accent);
  }

  .power.busy {
    animation: pulse 1.2s infinite;
  }

  .power-label {
    margin-top: 10px;
    font-weight: 600;
    color: var(--text-2);
  }

  .server {
    margin-top: 16px;
    width: 100%;
    display: grid;
    grid-template-columns: auto 1fr;
    grid-template-rows: auto auto;
    column-gap: 8px;
    align-items: center;
    text-align: left;
    padding: 10px 12px;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    color: var(--text);
  }

  .server :global(svg) {
    grid-row: span 2;
    color: var(--accent);
  }

  .server .proto {
    font-size: 11.5px;
    color: var(--text-3);
  }

  .server.empty {
    display: flex;
    color: var(--text-2);
  }

  .metrics {
    margin-top: 12px;
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
  }

  .metric {
    padding: 10px 12px;
  }

  .label {
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: 12px;
    color: var(--text-3);
  }

  .value {
    margin-top: 2px;
    font-size: 19px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }

  .value small {
    font-size: 12px;
    font-weight: 500;
    color: var(--text-3);
  }

  .value.dim {
    font-size: 14px;
    color: var(--text-3);
    font-weight: 500;
    padding-top: 4px;
  }

  .chart {
    margin-top: 10px;
    padding: 10px 12px;
  }

  .totals {
    margin-top: 6px;
    display: flex;
    justify-content: space-between;
    gap: 8px;
    font-size: 12px;
    color: var(--text-2);
    flex-wrap: wrap;
  }

  @keyframes pulse {
    50% {
      opacity: 0.55;
    }
  }
</style>
