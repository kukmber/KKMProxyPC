import { listen } from "@tauri-apps/api/event";
import { useEffect, useMemo, useState } from "react";
import { api, CoreInfo, DpiConflict, Mihomo, TgStatus, VpnStatus, ZapretStatus } from "./api";

export function useVpnStatus(): VpnStatus {
  const [status, setStatus] = useState<VpnStatus>({ state: "stopped" });
  useEffect(() => {
    api.vpnStatus().then(setStatus);
    const un = listen<VpnStatus>("vpn-status", (e) => setStatus(e.payload));
    return () => {
      un.then((f) => f());
    };
  }, []);
  return status;
}

export function useTgStatus(): TgStatus {
  const [status, setStatus] = useState<TgStatus>({ state: "stopped" });
  useEffect(() => {
    api.tgStatus().then(setStatus);
    const un = listen<TgStatus>("tg-status", (e) => setStatus(e.payload));
    return () => {
      un.then((f) => f());
    };
  }, []);
  return status;
}

export function useZapretStatus(): ZapretStatus {
  const [status, setStatus] = useState<ZapretStatus>({ state: "stopped" });
  useEffect(() => {
    api.zapretStatus().then(setStatus);
    const un = listen<ZapretStatus>("zapret-status", (e) => setStatus(e.payload));
    return () => {
      un.then((f) => f());
    };
  }, []);
  return status;
}

/** Чужая программа обхода DPI, если она сейчас работает. */
export function useDpiConflict(): DpiConflict | null {
  const [conflict, setConflict] = useState<DpiConflict | null>(null);
  useEffect(() => {
    const check = () => api.dpiConflict().then(setConflict).catch(() => {});
    check();
    // Такая программа может появиться в любой момент, поэтому поглядываем.
    const id = setInterval(check, 5000);
    return () => clearInterval(id);
  }, []);
  return conflict;
}

/** Сведения о ядрах; обновляются по событию после проверки или обновления. */
export function useCores(): [CoreInfo[], (c: CoreInfo[]) => void] {
  const [cores, setCores] = useState<CoreInfo[]>([]);
  useEffect(() => {
    api.coreInfo().then(setCores);
    const un = listen<CoreInfo[]>("cores", (e) => setCores(e.payload));
    return () => {
      un.then((f) => f());
    };
  }, []);
  return [cores, setCores];
}

export function useMihomo(status: VpnStatus): Mihomo | null {
  return useMemo(
    () =>
      status.state === "running" && status.controller && status.secret
        ? new Mihomo(status.controller, status.secret)
        : null,
    [status.state, status.controller, status.secret],
  );
}

export interface Traffic {
  up: number;
  down: number;
  upTotal: number;
  downTotal: number;
  connections: number;
}

const EMPTY: Traffic = { up: 0, down: 0, upTotal: 0, downTotal: 0, connections: 0 };

/** Скорость из `/traffic` и итоги с числом соединений из `/connections`. */
export function useTraffic(mihomo: Mihomo | null): Traffic {
  const [t, setT] = useState<Traffic>(EMPTY);
  useEffect(() => {
    setT(EMPTY);
    if (!mihomo) return;
    const traffic = mihomo.ws("/traffic");
    traffic.onmessage = (e) => {
      const d = JSON.parse(e.data);
      setT((p) => ({ ...p, up: d.up, down: d.down }));
    };
    const conns = mihomo.ws("/connections");
    conns.onmessage = (e) => {
      const d = JSON.parse(e.data);
      setT((p) => ({
        ...p,
        upTotal: d.uploadTotal ?? 0,
        downTotal: d.downloadTotal ?? 0,
        connections: d.connections?.length ?? 0,
      }));
    };
    return () => {
      traffic.close();
      conns.close();
    };
  }, [mihomo]);
  return t;
}

export function useNow(intervalMs = 1000, enabled = true): number {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    if (!enabled) return;
    const id = setInterval(() => setNow(Date.now()), intervalMs);
    return () => clearInterval(id);
  }, [intervalMs, enabled]);
  return now;
}

export function useSystemDark(): boolean {
  const q = "(prefers-color-scheme: dark)";
  const [dark, setDark] = useState(() => window.matchMedia(q).matches);
  useEffect(() => {
    const m = window.matchMedia(q);
    const on = () => setDark(m.matches);
    m.addEventListener("change", on);
    return () => m.removeEventListener("change", on);
  }, []);
  return dark;
}
