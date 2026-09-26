import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";

export type VpnState = "stopped" | "starting" | "running" | "stopping";
export type Mode = "rule" | "global" | "direct";
export type Connection = "sysproxy" | "tun";

export interface VpnStatus {
  state: VpnState;
  controller?: string;
  secret?: string;
  mixedPort?: number;
  startedAt?: number;
  connection?: Connection;
  profileId?: string;
  error?: string;
  stage?: string;
}

export interface TgStatus {
  state: VpnState;
  host?: string;
  port?: number;
  secret?: string;
  link?: string;
  startedAt?: number;
  error?: string;
}

export interface ZapretStatus {
  state: VpnState;
  startedAt?: number;
  error?: string;
  strategy?: string;
}

export interface ProbeResult {
  id: string;
  title: string;
  ok: number;
  total: number;
  failed: string[];
  error?: string;
}

export interface AutotuneProgress {
  step: number;
  total: number;
  title: string;
}

export interface HostSetInfo {
  id: string;
  title: string;
  about: string;
  count: number;
}

export interface HostsState {
  sets: string[];
  custom: string;
  total: number;
  available: HostSetInfo[];
}

export interface Startup {
  withWindows: boolean;
  minimized: boolean;
  vpn: boolean;
  tg: boolean;
  zapret: boolean;
  tray: boolean;
}

export interface StrategyInfo {
  id: string;
  title: string;
  about: string;
}

export interface Settings {
  activeProfile?: string;
  mode: Mode;
  connection: Connection;
  mixedPort: number;
  tgHost: string;
  tgPort: number;
  tgSecret?: string;
  zapretStrategy: string;
  zapretCustom: string;
  zapretHostlistOn: boolean;
}

export interface UserInfo {
  upload: number;
  download: number;
  total: number;
  expire?: number;
}

export interface Profile {
  id: string;
  name: string;
  url?: string;
  kind: "yaml" | "links";
  updatedAt: number;
  userinfo?: UserInfo;
  proxyCount: number;
}

export interface LogLine {
  id: number;
  ts: number;
  source: string;
  level: string;
  msg: string;
}

export interface PlatformInfo {
  mica: boolean;
  elevated: boolean;
  version: string;
}

export interface CoreInfo {
  id: "mihomo" | "tgws" | "zapret";
  title: string;
  repo: string;
  installed: boolean;
  version?: string;
  latest?: string;
  updateAvailable: boolean;
  bundled: boolean;
}

export const api = {
  vpnStatus: () => invoke<VpnStatus>("vpn_status"),
  vpnStart: () => invoke<void>("vpn_start"),
  vpnStop: () => invoke<void>("vpn_stop"),
  getSettings: () => invoke<Settings>("get_settings"),
  setMode: (mode: Mode) => invoke<void>("set_mode", { mode }),
  setConnection: (connection: Connection) => invoke<void>("set_connection", { connection }),
  listProfiles: () => invoke<Profile[]>("list_profiles"),
  addProfileUrl: (url: string) => invoke<Profile>("add_profile_url", { url }),
  addProfileContent: (name: string | null, content: string) =>
    invoke<Profile>("add_profile_content", { name, content }),
  refreshProfile: (id: string) => invoke<Profile>("refresh_profile", { id }),
  deleteProfile: (id: string) => invoke<void>("delete_profile", { id }),
  renameProfile: (id: string, name: string) => invoke<void>("rename_profile", { id, name }),
  setActiveProfile: (id: string) => invoke<void>("set_active_profile", { id }),
  getLogs: (source?: string) => invoke<LogLine[]>("get_logs", { source }),
  clearLogs: (source?: string) => invoke<void>("clear_logs", { source }),
  platformInfo: () => invoke<PlatformInfo>("platform_info"),
  restartAsAdmin: (connect: boolean) => invoke<void>("restart_as_admin", { connect }),
  tgStatus: () => invoke<TgStatus>("tg_status"),
  tgStart: () => invoke<void>("tg_start"),
  tgStop: () => invoke<void>("tg_stop"),
  tgConnections: () => invoke<number>("tg_connections"),
  setTgParams: (host: string, port: number, secret: string) =>
    invoke<void>("set_tg_params", { host, port, secret }),
  regenerateTgSecret: () => invoke<string>("regenerate_tg_secret"),
  zapretStatus: () => invoke<ZapretStatus>("zapret_status"),
  zapretStart: () => invoke<void>("zapret_start"),
  zapretStop: () => invoke<void>("zapret_stop"),
  zapretStrategies: () => invoke<StrategyInfo[]>("zapret_strategies"),
  setZapretStrategy: (id: string) => invoke<void>("set_zapret_strategy", { id }),
  zapretHosts: () => invoke<HostsState>("zapret_hosts"),
  setZapretHosts: (sets: string[], custom: string) =>
    invoke<number>("set_zapret_hosts", { sets, custom }),
  getStartup: () => invoke<Startup>("get_startup"),
  setStartup: (value: Startup) => invoke<void>("set_startup", { value }),
  setZapretCustom: (args: string) => invoke<void>("set_zapret_custom", { args }),
  zapretAutotune: (domains: string[]) => invoke<ProbeResult[]>("zapret_autotune", { domains }),
  restartAll: () => invoke<string[]>("restart_all"),
  dpiConflict: () => invoke<DpiConflict | null>("dpi_conflict"),
  openExternal: (url: string) => openUrl(url),
  coreInfo: () => invoke<CoreInfo[]>("core_info"),
  checkCoreUpdates: () => invoke<CoreInfo[]>("check_core_updates"),
  updateCore: (id: string) => invoke<string>("update_core", { id }),
};

// ---------- контроллер mihomo ----------

export interface RuleInfo {
  type: string;
  payload: string;
  proxy: string;
}

export interface DpiConflict {
  pid: number;
  path: string;
}

export interface ProxyInfo {
  name: string;
  type: string;
  now?: string;
  all?: string[];
  hidden?: boolean;
  history?: { delay: number }[];
}

export const DELAY_URL = "https://www.gstatic.com/generate_204";

export class Mihomo {
  constructor(
    private host: string,
    private secret: string,
  ) {}

  private async req<T>(method: string, path: string, body?: unknown): Promise<T> {
    const r = await fetch(`http://${this.host}${path}`, {
      method,
      headers: { Authorization: `Bearer ${this.secret}`, "Content-Type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    if (!r.ok) {
      const text = await r.text().catch(() => "");
      throw new Error(text || `HTTP ${r.status}`);
    }
    return r.status === 204 ? (undefined as T) : r.json();
  }

  proxies() {
    return this.req<{ proxies: Record<string, ProxyInfo> }>("GET", "/proxies");
  }

  rules() {
    return this.req<{ rules: RuleInfo[] }>("GET", "/rules");
  }

  select(group: string, name: string) {
    return this.req<void>("PUT", `/proxies/${encodeURIComponent(group)}`, { name });
  }

  async delay(name: string, timeout = 5000): Promise<number> {
    const q = `url=${encodeURIComponent(DELAY_URL)}&timeout=${timeout}`;
    const r = await this.req<{ delay: number }>("GET", `/proxies/${encodeURIComponent(name)}/delay?${q}`);
    return r.delay;
  }

  closeAllConnections() {
    return this.req<void>("DELETE", "/connections");
  }

  ws(path: string) {
    return new WebSocket(`ws://${this.host}${path}?token=${encodeURIComponent(this.secret)}`);
  }
}

// ---------- форматирование ----------

export function formatBytes(n: number): string {
  if (!n || n < 0) return "0 Б";
  const units = ["Б", "КБ", "МБ", "ГБ", "ТБ"];
  let i = 0;
  while (n >= 1024 && i < units.length - 1) {
    n /= 1024;
    i++;
  }
  return `${n >= 100 || i === 0 ? Math.round(n) : n.toFixed(1)} ${units[i]}`;
}

export function formatDuration(ms: number): string {
  const s = Math.floor(ms / 1000);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const pad = (x: number) => String(x).padStart(2, "0");
  return h > 0 ? `${h}:${pad(m)}:${pad(sec)}` : `${pad(m)}:${pad(sec)}`;
}

export function formatAgo(ts: number): string {
  const d = Date.now() - ts;
  if (d < 60_000) return "только что";
  if (d < 3_600_000) return `${Math.floor(d / 60_000)} мин назад`;
  if (d < 86_400_000) return `${Math.floor(d / 3_600_000)} ч назад`;
  return new Date(ts).toLocaleDateString("ru-RU");
}

export function errorText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
}
