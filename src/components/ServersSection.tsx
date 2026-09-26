import { useEffect, useMemo, useState } from "react";
import { Button, Input, Spinner, Tooltip } from "@fluentui/react-components";
import { FlashRegular, SearchRegular, WarningRegular } from "@fluentui/react-icons";
import { Mihomo, RuleInfo } from "../api";
import { cleanName, Flag } from "../flags";
import { useNotify } from "../toast";
import { Delay, NOT_TESTABLE, Servers } from "./useServers";
import { ConnectionsList } from "./ConnectionsList";
import { AppRules } from "./AppRules";

export const TYPE_LABEL: Record<string, string> = {
  Vless: "VLESS",
  Vmess: "VMess",
  Trojan: "Trojan",
  Shadowsocks: "SS",
  ShadowsocksR: "SSR",
  Hysteria2: "Hysteria2",
  Hysteria: "Hysteria",
  Tuic: "TUIC",
  WireGuard: "WireGuard",
  Socks5: "SOCKS5",
  Http: "HTTP",
  AnyTLS: "AnyTLS",
  URLTest: "авто",
  Fallback: "резерв",
  LoadBalance: "баланс",
  Selector: "группа",
  Direct: "напрямую",
  Reject: "блок",
  RejectDrop: "блок",
  Pass: "пропуск",
};

export function ServersSection({
  servers,
  connected,
  mihomo,
}: {
  servers: Servers;
  connected: boolean;
  mihomo: Mihomo | null;
}) {
  // «Программы» доступны и без подключения: правила — это настройка,
  // их удобно задать заранее.
  const [view, setView] = useState<"servers" | "conn" | "apps" | "rules">("servers");
  const tabs: { id: typeof view; label: string; needsConnection: boolean }[] = [
    { id: "servers", label: "Серверы", needsConnection: true },
    { id: "conn", label: "Соединения", needsConnection: true },
    { id: "apps", label: "Программы", needsConnection: false },
    { id: "rules", label: "Правила подписки", needsConnection: true },
  ];
  const active = tabs.find((t) => t.id === view)!;

  return (
    <section className="pane pane-list">
      <div className="chips" style={{ padding: "12px 12px 6px" }}>
        {tabs.map((t) => (
          <button key={t.id} className={`chip${view === t.id ? " on" : ""}`} onClick={() => setView(t.id)}>
            {t.label}
          </button>
        ))}
      </div>

      {active.needsConnection && !connected ? (
        <div className="empty">
          <span className="empty-title">{active.label}</span>
          <span className="hint">Появятся после подключения</span>
        </div>
      ) : view === "servers" ? (
        <ServerList servers={servers} />
      ) : view === "conn" ? (
        <ConnectionsList mihomo={mihomo} />
      ) : view === "apps" ? (
        <AppRules connected={connected} />
      ) : (
        <RuleList mihomo={mihomo} />
      )}
    </section>
  );
}

function ServerList({ servers }: { servers: Servers }) {
  const notify = useNotify();
  const [query, setQuery] = useState("");
  const [proto, setProto] = useState("all");
  const [testing, setTesting] = useState(false);
  const { groups, group, setGroup, current, members, delays } = servers;

  const protocols = useMemo(() => [...new Set(members.map((m) => m.type))], [members]);
  const visible = members.filter(
    (m) => (proto === "all" || m.type === proto) && (!query || m.name.toLowerCase().includes(query.toLowerCase())),
  );

  const testAll = async () => {
    setTesting(true);
    await servers.testMany(members.filter((m) => !NOT_TESTABLE.has(m.type)).map((m) => m.name));
    setTesting(false);
  };

  return (
    <>
      <div className="list-head">
        <Input
          appearance="filled-lighter"
          contentBefore={<SearchRegular />}
          placeholder="Поиск сервера"
          value={query}
          onChange={(_, d) => setQuery(d.value)}
          style={{ flex: 1, minWidth: 0 }}
        />
        <Tooltip content="Проверить задержку всех" relationship="label">
          <Button
            appearance="subtle"
            icon={testing ? <Spinner size="tiny" /> : <FlashRegular />}
            onClick={testAll}
            disabled={testing}
          />
        </Tooltip>
      </div>

      {/* Группы и протоколы показываем всегда: по ним сразу видно,
          что вообще есть в подписке. */}
      {groups.length > 0 && (
        <>
          <div className="caps" style={{ margin: "2px 12px 4px" }}>
            Группы
          </div>
          <div className="chips">
            {groups.map((g) => (
              <button key={g.name} className={`chip${g.name === group ? " on" : ""}`} onClick={() => setGroup(g.name)}>
                {g.name === "GLOBAL" ? "Глобальный" : cleanName(g.name)}
              </button>
            ))}
          </div>
        </>
      )}
      {protocols.length > 0 && (
        <>
          <div className="caps" style={{ margin: "6px 12px 4px" }}>
            Протоколы
          </div>
          <div className="chips">
            {["all", ...protocols].map((p) => (
              <button key={p} className={`chip ghost${p === proto ? " on" : ""}`} onClick={() => setProto(p)}>
                {p === "all" ? `Все (${members.length})` : `${TYPE_LABEL[p] ?? p} (${members.filter((m) => m.type === p).length})`}
              </button>
            ))}
          </div>
        </>
      )}

      <div className="list">
        {visible.map((m) => (
          <button
            key={m.name}
            className={`srv${current?.now === m.name ? " on" : ""}`}
            onClick={() => servers.select(m.name).catch((e) => notify.error("Не удалось выбрать сервер", e))}
            title={m.name}
          >
            <Flag name={m.name} size={22} />
            <span className="srv-name ellipsis">{cleanName(m.name)}</span>
            <span className="srv-type">
              {TYPE_LABEL[m.type] ?? m.type}
              {m.now ? ` · ${cleanName(m.now)}` : ""}
            </span>
            {!NOT_TESTABLE.has(m.type) ? (
              <DelayBadge d={delays[m.name]} onClick={(e) => (e.stopPropagation(), servers.testOne(m.name))} />
            ) : (
              <span className="delay" />
            )}
          </button>
        ))}
        {!visible.length && <div className="hint" style={{ padding: 16 }}>Ничего не найдено</div>}
      </div>
    </>
  );
}

/** Правила маршрутизации из подписки: по ним ядро решает, куда слать запрос. */
function RuleList({ mihomo }: { mihomo: Mihomo | null }) {
  const [rules, setRules] = useState<RuleInfo[] | null>(null);
  const [query, setQuery] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!mihomo) return;
    mihomo
      .rules()
      .then((r) => setRules(r.rules))
      .catch(() => setError("Не удалось получить правила от ядра"));
  }, [mihomo]);

  const q = query.toLowerCase();
  const visible = (rules ?? []).filter(
    (r) => !q || r.payload.toLowerCase().includes(q) || r.proxy.toLowerCase().includes(q) || r.type.toLowerCase().includes(q),
  );
  // Правил бывают тысячи — показываем первые, остальные ищутся поиском.
  const shown = visible.slice(0, 400);

  return (
    <>
      <div className="list-head">
        <Input
          appearance="filled-lighter"
          contentBefore={<SearchRegular />}
          placeholder="Поиск по домену, типу или группе"
          value={query}
          onChange={(_, d) => setQuery(d.value)}
          style={{ flex: 1, minWidth: 0 }}
        />
      </div>
      <div className="hint" style={{ padding: "0 14px 6px" }}>
        {error
          ? error
          : rules === null
            ? "Загрузка…"
            : `Показано ${shown.length} из ${visible.length}${visible.length !== rules.length ? ` (всего ${rules.length})` : ""}`}
      </div>
      <div className="list">
        {shown.map((r, i) => (
          <div className="rule" key={`${r.type}-${r.payload}-${i}`}>
            <span className="rule-type">{r.type}</span>
            <span className="rule-payload ellipsis mono" title={r.payload}>
              {r.payload || "—"}
            </span>
            <span className="rule-proxy ellipsis">{cleanName(r.proxy)}</span>
          </div>
        ))}
        {rules !== null && !visible.length && <div className="hint" style={{ padding: 16 }}>Ничего не найдено</div>}
      </div>
    </>
  );
}

export function delayColor(ms: number) {
  return ms < 200 ? "var(--ok)" : ms < 500 ? "var(--mid)" : "var(--bad)";
}

function DelayBadge({ d, onClick }: { d?: Delay; onClick: (e: React.MouseEvent) => void }) {
  let content: React.ReactNode = <span style={{ opacity: 0.4 }}>—</span>;
  if (d?.testing) content = <Spinner size="extra-tiny" />;
  else if (d?.failed)
    content = (
      <Tooltip content="Не ответил за 3 попытки. Перепроверю через 30 секунд" relationship="label">
        <WarningRegular style={{ color: "var(--bad)" }} />
      </Tooltip>
    );
  else if (d?.ms) content = <span style={{ color: delayColor(d.ms) }}>{d.ms}</span>;
  return (
    <span className="delay" onClick={onClick} role="button" aria-label="Проверить задержку">
      {content}
    </span>
  );
}
