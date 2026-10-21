<script lang="ts">
  // Download (filled) and upload (line) speed over the last minute.
  let { points }: { points: { down: number; up: number }[] } = $props();

  const W = 300;
  const H = 64;
  const N = 60;

  const max = $derived(Math.max(1, ...points.map((p) => Math.max(p.down, p.up))));

  function path(key: "down" | "up", close: boolean): string {
    if (points.length < 2) return "";
    const offset = N - points.length;
    const xy = points.map((p, i) => [((offset + i) / (N - 1)) * W, H - 2 - (p[key] / max) * (H - 6)]);
    let d = `M${xy[0][0].toFixed(1)},${xy[0][1].toFixed(1)}`;
    for (const [x, y] of xy.slice(1)) d += `L${x.toFixed(1)},${y.toFixed(1)}`;
    if (close) d += `L${W},${H}L${xy[0][0].toFixed(1)},${H}Z`;
    return d;
  }
</script>

<svg viewBox="0 0 {W} {H}" preserveAspectRatio="none" class="spark">
  <defs>
    <linearGradient id="spark-fill" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="var(--accent)" stop-opacity="0.35" />
      <stop offset="1" stop-color="var(--accent)" stop-opacity="0" />
    </linearGradient>
  </defs>
  <path d={path("down", true)} fill="url(#spark-fill)" />
  <path d={path("down", false)} fill="none" stroke="var(--accent)" stroke-width="1.6" vector-effect="non-scaling-stroke" />
  <path d={path("up", false)} fill="none" stroke="var(--accent-2)" stroke-width="1.2" stroke-dasharray="3 3" vector-effect="non-scaling-stroke" />
</svg>

<style>
  .spark {
    width: 100%;
    height: 64px;
    display: block;
  }
</style>
