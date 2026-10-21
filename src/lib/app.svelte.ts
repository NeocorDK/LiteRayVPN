import { listen } from "@tauri-apps/api/event";
import { api, type ConnStatus, type Snapshot, type StatsEvent } from "./api";
import { translate, translateError, type Key, type Lang } from "./i18n";

const HISTORY = 60;

class AppStore {
  snapshot = $state<Snapshot | null>(null);
  status = $state<ConnStatus>({ state: "disconnected" });
  stats = $state<StatsEvent | null>(null);
  /** Last 60 s of download/upload speeds for the sparkline. */
  history = $state<{ down: number; up: number }[]>([]);
  pings = $state<Record<string, number | null | "pending">>({});
  needAdmin = $state(false);
  toast = $state<{ text: string; error: boolean } | null>(null);
  private toastTimer: ReturnType<typeof setTimeout> | undefined;

  lang = $derived<Lang>(this.resolveLang());

  private resolveLang(): Lang {
    const s = this.snapshot;
    if (!s) return navigator.language.startsWith("ru") ? "ru" : "en";
    const l = s.store.settings.language;
    return l === "auto" ? s.systemLanguage : l;
  }

  t = (key: Key, vars?: Record<string, string | number>) => translate(this.lang, key, vars);

  async init() {
    await listen<ConnStatus>("status", (e) => {
      this.status = e.payload;
      if (e.payload.state !== "connected") {
        this.stats = null;
        this.history = [];
      }
      if (e.payload.state === "error") this.showError(e.payload.message);
    });
    await listen<StatsEvent>("stats", (e) => {
      this.stats = e.payload;
      this.history = [...this.history.slice(-(HISTORY - 1)), { down: e.payload.downSpeed, up: e.payload.upSpeed }];
    });
    await listen("changed", () => this.reload());
    await listen<{ id: string; ms: number | null }>("ping", (e) => {
      this.pings = { ...this.pings, [e.payload.id]: e.payload.ms };
    });
    await this.reload();
  }

  async reload() {
    const snap = await api.snapshot();
    this.snapshot = snap;
    this.status = snap.status;
  }

  get store() {
    return this.snapshot?.store;
  }

  notify(text: string, error = false) {
    this.toast = { text, error };
    clearTimeout(this.toastTimer);
    this.toastTimer = setTimeout(() => (this.toast = null), error ? 6000 : 2500);
  }

  showError(err: unknown) {
    this.notify(translateError(this.lang, err), true);
  }

  /** Runs an action and reports its error as a toast; returns false on failure. */
  async run<T>(fn: () => Promise<T>): Promise<T | undefined> {
    try {
      return await fn();
    } catch (e) {
      if (String(e) === "NEED_ADMIN") this.needAdmin = true;
      else this.showError(e);
      return undefined;
    }
  }

  async toggleConnection() {
    if (this.status.state === "connected" || this.status.state === "connecting") {
      await this.run(api.disconnect);
    } else {
      await this.run(api.connect);
    }
  }

  async pingAll(ids: string[] = []) {
    const targets = ids.length ? ids : (this.store?.profiles.map((p) => p.id) ?? []);
    const pending: Record<string, "pending"> = {};
    for (const id of targets) pending[id] = "pending";
    this.pings = { ...this.pings, ...pending };
    await this.run(() => api.pingProfiles(ids));
  }
}

export const app = new AppStore();
