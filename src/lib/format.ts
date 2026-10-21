export function formatBytes(bytes: number, lang: "ru" | "en"): string {
  const units = lang === "ru" ? ["Б", "КБ", "МБ", "ГБ", "ТБ"] : ["B", "KB", "MB", "GB", "TB"];
  let v = bytes;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  const digits = v >= 100 || i === 0 ? 0 : 1;
  return `${v.toFixed(digits)} ${units[i]}`;
}

/** Speed in bits per second, as network speeds are usually quoted. */
export function formatSpeed(bytesPerSec: number, lang: "ru" | "en"): string {
  const bits = bytesPerSec * 8;
  const units = lang === "ru" ? ["бит/с", "Кбит/с", "Мбит/с", "Гбит/с"] : ["bps", "Kbps", "Mbps", "Gbps"];
  let v = bits;
  let i = 0;
  while (v >= 1000 && i < units.length - 1) {
    v /= 1000;
    i++;
  }
  const digits = v >= 100 || i === 0 ? 0 : 1;
  return `${v.toFixed(digits)} ${units[i]}`;
}

export function formatDuration(seconds: number): string {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = Math.floor(seconds % 60);
  const pad = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${pad(m)}:${pad(s)}`;
}

export function formatDate(unix: number, lang: "ru" | "en", withTime = false): string {
  const d = new Date(unix * 1000);
  return d.toLocaleString(lang === "ru" ? "ru-RU" : "en-GB", {
    day: "numeric",
    month: "short",
    year: "numeric",
    ...(withTime ? { hour: "2-digit", minute: "2-digit" } : {}),
  });
}

export function pingClass(ms: number | null | undefined): string {
  if (ms == null) return "bad";
  if (ms < 150) return "good";
  if (ms < 350) return "ok";
  return "bad";
}
