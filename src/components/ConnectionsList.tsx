import { useEffect, useMemo, useState } from "react";
import { Button, Input, Tooltip } from "@fluentui/react-components";
import { DismissRegular, SearchRegular } from "@fluentui/react-icons";
import { ConnectionInfo, formatBytes, formatDuration, Mihomo } from "../api";
import { cleanName } from "../flags";

/** Куда прямо сейчас идёт трафик: сайт, программа, через какой сервер. */
export function ConnectionsList({ mihomo }: { mihomo: Mihomo | null }) {
  const [items, setItems] = useState<ConnectionInfo[]>([]);
  const [query, setQuery] = useState("");
  const [now, setNow] = useState(Date.now());

  useEffect(() => {
    if (!mihomo) return;
    const ws = mihomo.ws("/connections");
    ws.onmessage = (e) => {
      const d = JSON.parse(e.data);
      setItems(d.connections ?? []);
      setNow(Date.now());
    };
    return () => ws.close();
  }, [mihomo]);

  const q = query.toLowerCase();
  const visible = useMemo(() => {
    const named = items.map((c) => ({
      c,
      host: c.metadata.host || c.metadata.destinationIP || "—",
      app: appName(c),
      via: c.chains?.[0] ?? "",
    }));
    return named
      .filter((n) => !q || n.host.toLowerCase().includes(q) || n.app.toLowerCase().includes(q))
      // Самые «тяжёлые» сверху: обычно именно их и ищут.
      .sort((a, b) => b.c.download + b.c.upload - (a.c.download + a.c.upload));
  }, [items, q]);

  const total = items.reduce((s, c) => s + c.download + c.upload, 0);

  return (
    <>
      <div className="list-head">
        <Input
          appearance="filled-lighter"
          contentBefore={<SearchRegular />}
          placeholder="Поиск по сайту или программе"
          value={query}
          onChange={(_, d) => setQuery(d.value)}
          style={{ flex: 1, minWidth: 0 }}
        />
        <Tooltip content="Закрыть все соединения" relationship="label">
          <Button
            appearance="subtle"
            icon={<DismissRegular />}
            disabled={!items.length}
            onClick={() => mihomo?.closeAllConnections().catch(() => {})}
          />
        </Tooltip>
      </div>
      <div className="hint" style={{ padding: "0 14px 6px" }}>
        {items.length ? `${items.length} соединений · ${formatBytes(total)} за сессию` : "Пока пусто"}
      </div>
      <div className="list">
        {visible.map(({ c, host, app, via }) => (
          <div className="conn" key={c.id}>
            <div className="conn-main">
              <span className="ellipsis" title={host}>
                {host}
              </span>
              <span className="hint ellipsis">
                {app}
                {via ? ` · ${cleanName(via)}` : ""}
                {c.metadata.network ? ` · ${c.metadata.network.toUpperCase()}` : ""}
              </span>
            </div>
            <span className="conn-bytes" title="передано / получено">
              ↑ {formatBytes(c.upload)}
              <br />↓ {formatBytes(c.download)}
            </span>
            <span className="hint conn-time">{formatDuration(now - Date.parse(c.start))}</span>
            <Tooltip content="Закрыть соединение" relationship="label">
              <Button
                appearance="subtle"
                size="small"
                icon={<DismissRegular />}
                onClick={() => mihomo?.closeConnection(c.id).catch(() => {})}
              />
            </Tooltip>
          </div>
        ))}
        {!visible.length && items.length > 0 && (
          <div className="hint" style={{ padding: 16 }}>
            Ничего не найдено
          </div>
        )}
      </div>
    </>
  );
}

/** Имя программы: ядро отдаёт либо готовое имя, либо полный путь к файлу. */
function appName(c: ConnectionInfo): string {
  const raw = c.metadata.process || c.metadata.processPath || "";
  return raw.split(/[\\/]/).pop() || "неизвестно";
}
