import { invoke } from "@tauri-apps/api/core";

export type ConnMode = "tun" | "systemProxy";
export type ListMode = "exclude" | "include";
export type RuleKind = "app" | "domain" | "ip";

export interface Server {
  protocol: "vless" | "vmess" | "trojan" | "shadowsocks" | null;
  address: string;
  port: number;
  network: string;
  security: string;
  flow: string | null;
}

export interface Profile {
  id: string;
  name: string;
  subscriptionId: string | null;
  link: string;
  server: Server;
}

export interface UserInfo {
  upload: number;
  download: number;
  total: number;
  expire: number | null;
}

export interface Subscription {
  id: string;
  name: string;
  url: string;
  lastUpdate: number | null;
  updateIntervalHours: number | null;
  userinfo: UserInfo | null;
  lastError: string | null;
  skipped: number;
}

export interface RoutingSettings {
  listMode: ListMode;
  apps: string[];
  domains: string[];
  ips: string[];
  ruDirect: boolean;
  lanDirect: boolean;
  blockAds: boolean;
}

export interface Settings {
  mode: ConnMode;
  language: "auto" | "ru" | "en";
  autostart: boolean;
  connectOnStart: boolean;
  startMinimized: boolean;
  mixedPort: number;
  ipv6: boolean;
  corePrerelease: boolean;
  geoRepo: string;
  subUpdateHours: number;
  lastGeoUpdate: number | null;
}

export interface Store {
  settings: Settings;
  profiles: Profile[];
  subscriptions: Subscription[];
  routing: RoutingSettings;
  selected: string | null;
}

export type ConnStatus =
  | { state: "disconnected" }
  | { state: "connecting" }
  | { state: "connected"; profileId: string; profileName: string; mode: ConnMode; since: number }
  | { state: "error"; message: string };

export interface Snapshot {
  store: Store;
  status: ConnStatus;
  elevated: boolean;
  portable: boolean;
  coreVersion: string | null;
  systemLanguage: "ru" | "en";
  appVersion: string;
  dataDir: string;
}

export interface StatsEvent {
  upSpeed: number;
  downSpeed: number;
  directUpSpeed: number;
  directDownSpeed: number;
  totalUp: number;
  totalDown: number;
  delayMs: number | null;
}

export interface ImportResult {
  added: number;
  skipped: number;
  subscription: boolean;
}

export interface ProcessInfo {
  name: string;
  path: string;
}

export interface CoreUpdateInfo {
  current: string | null;
  latest: { tag: string; prerelease: boolean; publishedAt: string };
  newer: boolean;
}

export const api = {
  snapshot: () => invoke<Snapshot>("get_snapshot"),
  importText: (text: string) => invoke<ImportResult>("import_text", { text }),
  addSubscription: (url: string) => invoke<void>("add_subscription", { url }),
  refreshSubscription: (id: string) => invoke<void>("refresh_subscription", { id }),
  renameSubscription: (id: string, name: string) => invoke<void>("rename_subscription", { id, name }),
  deleteSubscription: (id: string) => invoke<void>("delete_subscription", { id }),
  deleteProfile: (id: string) => invoke<void>("delete_profile", { id }),
  selectProfile: (id: string) => invoke<void>("select_profile", { id }),
  pingProfiles: (ids: string[]) => invoke<void>("ping_profiles", { ids }),
  normalizeRule: (kind: RuleKind, value: string) => invoke<string>("normalize_rule", { kind, value }),
  setRouting: (routing: RoutingSettings) => invoke<RoutingSettings>("set_routing", { routing }),
  setSettings: (settings: Settings) => invoke<void>("set_settings", { settings }),
  connect: () => invoke<void>("connect"),
  disconnect: () => invoke<void>("disconnect"),
  restartAsAdmin: () => invoke<void>("restart_as_admin"),
  listProcesses: () => invoke<ProcessInfo[]>("list_processes"),
  logs: () => invoke<string[]>("get_logs"),
  checkCoreUpdate: () => invoke<CoreUpdateInfo>("check_core_update"),
  installCoreUpdate: () => invoke<string>("install_core_update"),
  updateGeo: () => invoke<void>("update_geo"),
  openDataDir: () => invoke<void>("open_data_dir"),
};
