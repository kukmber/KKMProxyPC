import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Mihomo, Mode, ProxyInfo } from "../api";

export const NOT_TESTABLE = new Set(["Direct", "Reject", "RejectDrop", "Pass", "Compatible"]);
const ATTEMPTS = 3;
const RETRY_EVERY_MS = 30_000;

export interface Delay {
  ms?: number;
  failed?: boolean;
  testing?: boolean;
}

/** Группы и серверы из контроллера mihomo, выбор сервера и замер задержки. */
export function useServers(mihomo: Mihomo | null, mode: Mode) {
  const [proxies, setProxies] = useState<Record<string, ProxyInfo>>({});
  const [group, setGroup] = useState<string | null>(null);
  const [delays, setDelays] = useState<Record<string, Delay>>({});
  const delaysRef = useRef(delays);
  delaysRef.current = delays;

  const load = useCallback(async () => {
    if (!mihomo) return;
    try {
      const d = await mihomo.proxies();
      setProxies(d.proxies);
      // Задержки, которые ядро уже намеряло само (группы url-test).
      setDelays((prev) => {
        const next = { ...prev };
        for (const p of Object.values(d.proxies)) {
          const last = p.history?.[p.history.length - 1]?.delay;
          if (last && !next[p.name]?.ms) next[p.name] = { ms: last };
        }
        return next;
      });
    } catch {
      /* ядро перезапускается */
    }
  }, [mihomo]);

  useEffect(() => {
    setProxies({});
    setDelays({});
    if (!mihomo) return;
    load();
    const id = setInterval(load, 10_000);
    return () => clearInterval(id);
  }, [mihomo, load]);

  const groups = useMemo(() => {
    const order = proxies.GLOBAL?.all ?? Object.keys(proxies);
    const list = order.map((n) => proxies[n]).filter((p) => p && p.type === "Selector" && p.all?.length && !p.hidden);
    if (mode === "global" && proxies.GLOBAL) list.unshift(proxies.GLOBAL);
    return list;
  }, [proxies, mode]);

  useEffect(() => {
    if (groups.length && !groups.some((g) => g.name === group)) setGroup(groups[0].name);
  }, [groups, group]);

  const current = groups.find((g) => g.name === group) ?? null;
  const members = useMemo(
    () => (current?.all ?? []).map((n) => proxies[n] ?? { name: n, type: "?" }),
    [current, proxies],
  );

  /** Сервер, через который реально идёт трафик: раскрываем вложенные группы («Авто» → узел). */
  const active = useMemo(() => {
    let name = groups[0]?.now;
    for (let i = 0; name && proxies[name]?.now && i < 5; i++) name = proxies[name].now;
    return name ? { name, type: proxies[name]?.type ?? "?" } : null;
  }, [groups, proxies]);

  const testOne = useCallback(
    async (name: string) => {
      if (!mihomo) return;
      setDelays((d) => ({ ...d, [name]: { ...d[name], testing: true } }));
      // Первое соединение через CDN бывает дольше таймаута — одна неудача ещё не значит «не работает».
      for (let i = 0; i < ATTEMPTS; i++) {
        try {
          const ms = await mihomo.delay(name);
          setDelays((d) => ({ ...d, [name]: { ms } }));
          return;
        } catch {
          /* следующая попытка */
        }
      }
      setDelays((d) => ({ ...d, [name]: { failed: true } }));
    },
    [mihomo],
  );

  const testMany = useCallback(
    async (names: string[]) => {
      const queue = [...names];
      const worker = async () => {
        while (queue.length) await testOne(queue.shift()!);
      };
      await Promise.all(Array.from({ length: Math.min(8, queue.length) }, worker));
    },
    [testOne],
  );

  // Неответившие узлы перепроверяются раз в 30 секунд; предупреждение снимается при первом успехе.
  useEffect(() => {
    if (!mihomo) return;
    const id = setInterval(() => {
      const failed = Object.entries(delaysRef.current)
        .filter(([, d]) => d.failed && !d.testing)
        .map(([n]) => n);
      if (failed.length) testMany(failed);
    }, RETRY_EVERY_MS);
    return () => clearInterval(id);
  }, [mihomo, testMany]);

  // Сразу после подключения меряем всё один раз — чтобы список не был пустым на цифры.
  const measuredFor = useRef<Mihomo | null>(null);
  useEffect(() => {
    if (!mihomo || !members.length || measuredFor.current === mihomo) return;
    measuredFor.current = mihomo;
    testMany(members.filter((m) => !NOT_TESTABLE.has(m.type)).map((m) => m.name));
  }, [mihomo, members, testMany]);

  const select = async (name: string) => {
    if (!mihomo || !current) return;
    setProxies((p) => ({ ...p, [current.name]: { ...p[current.name], now: name } }));
    try {
      await mihomo.select(current.name, name);
      // Старые соединения остались бы на прежнем сервере.
      await mihomo.closeAllConnections().catch(() => {});
    } catch (e) {
      load();
      throw e;
    }
  };

  return { groups, group, setGroup, current, members, active, delays, testOne, testMany, select };
}

export type Servers = ReturnType<typeof useServers>;
