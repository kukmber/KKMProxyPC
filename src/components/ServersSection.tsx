import { useMemo, useState } from "react";
import { Button, Input, Spinner, Tooltip } from "@fluentui/react-components";
import { FlashRegular, SearchRegular, WarningRegular } from "@fluentui/react-icons";
import { cleanName, Flag } from "../flags";
import { useNotify } from "../toast";
import { Delay, NOT_TESTABLE, Servers } from "./useServers";

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

export function ServersSection({ servers, connected }: { servers: Servers; connected: boolean }) {
  const notify = useNotify();
  const [query, setQuery] = useState("");
  const [proto, setProto] = useState("all");
  const [testing, setTesting] = useState(false);
  const { groups, group, setGroup, current, members, delays } = servers;

  const protocols = useMemo(() => [...new Set(members.map((m) => m.type))], [members]);
  const visible = members.filter(
    (m) => (proto === "all" || m.type === proto) && (!query || m.name.toLowerCase().includes(query.toLowerCase())),
  );

  if (!connected) {
    return (
      <section className="pane pane-list">
        <div className="empty">
          <span className="empty-title">Серверы</span>
          <span className="hint">Список и задержка появятся после подключения</span>
        </div>
      </section>
    );
  }

  const testAll = async () => {
    setTesting(true);
    await servers.testMany(members.filter((m) => !NOT_TESTABLE.has(m.type)).map((m) => m.name));
    setTesting(false);
  };

  return (
    <section className="pane pane-list">
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
          <Button appearance="subtle" icon={testing ? <Spinner size="tiny" /> : <FlashRegular />} onClick={testAll} disabled={testing} />
        </Tooltip>
      </div>

      {(groups.length > 1 || protocols.length > 1) && (
        <div className="chips">
          {groups.length > 1 &&
            groups.map((g) => (
              <button key={g.name} className={`chip${g.name === group ? " on" : ""}`} onClick={() => setGroup(g.name)}>
                {g.name === "GLOBAL" ? "Глобальный" : cleanName(g.name)}
              </button>
            ))}
          {groups.length > 1 && protocols.length > 1 && <span className="chip-sep" />}
          {protocols.length > 1 &&
            ["all", ...protocols].map((p) => (
              <button key={p} className={`chip ghost${p === proto ? " on" : ""}`} onClick={() => setProto(p)}>
                {p === "all" ? "Все" : TYPE_LABEL[p] ?? p}
              </button>
            ))}
        </div>
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
    </section>
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
