import { useEffect, useMemo, useRef, useState } from "react";
import { Button, Dropdown, Input, Option, Tab, TabList, Tooltip } from "@fluentui/react-components";
import { CopyRegular, DeleteRegular, SearchRegular } from "@fluentui/react-icons";
import { listen } from "@tauri-apps/api/event";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { api, LogLine } from "../api";
import { useNotify } from "../toast";

const SOURCES = [
  { value: "vpn", label: "Ядро VPN" },
  { value: "app", label: "Приложение" },
];

const LEVELS: Record<string, number> = { debug: 0, info: 1, warning: 2, error: 3 };
const LEVEL_LABEL: Record<string, string> = { info: "Все сообщения", warning: "Предупреждения и ошибки", error: "Только ошибки" };
const LEVEL_COLOR: Record<string, string> = { warning: "var(--warn)", error: "var(--bad)", debug: "var(--text-secondary)" };
const SHOWN = 1500;

export function LogsPage() {
  const notify = useNotify();
  const [source, setSource] = useState("vpn");
  const [lines, setLines] = useState<LogLine[]>([]);
  const [query, setQuery] = useState("");
  const [minLevel, setMinLevel] = useState("info");
  const boxRef = useRef<HTMLDivElement>(null);
  const stick = useRef(true);

  useEffect(() => {
    api.getLogs().then(setLines);
    const un = listen<LogLine>("log", (e) => setLines((l) => (l.length > 3000 ? l.slice(-2500) : l).concat(e.payload)));
    return () => {
      un.then((f) => f());
    };
  }, []);

  const shown = useMemo(() => {
    const q = query.toLowerCase();
    return lines
      .filter((l) => l.source === source && (LEVELS[l.level] ?? 1) >= LEVELS[minLevel] && (!q || l.msg.toLowerCase().includes(q)))
      .slice(-SHOWN);
  }, [lines, source, query, minLevel]);

  // Прокручиваем вниз, только если пользователь и так был внизу.
  useEffect(() => {
    const el = boxRef.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [shown]);

  const copy = async () => {
    const text = shown.map((l) => `${time(l.ts)} [${l.level}] ${l.msg}`).join("\n");
    await writeText(text);
    notify.ok(`Скопировано строк: ${shown.length}`);
  };

  const clear = async () => {
    await api.clearLogs(source);
    setLines((l) => l.filter((x) => x.source !== source));
  };

  return (
    <div className="page" style={{ height: "calc(100vh - 24px)" }}>
      <h1 className="page-title" style={{ marginBottom: 8 }}>
        Журнал
      </h1>
      <TabList selectedValue={source} onTabSelect={(_, d) => setSource(d.value as string)}>
        {SOURCES.map((s) => (
          <Tab key={s.value} value={s.value}>
            {s.label}
          </Tab>
        ))}
      </TabList>
      <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
        <Input
          contentBefore={<SearchRegular />}
          placeholder="Фильтр"
          value={query}
          onChange={(_, d) => setQuery(d.value)}
          style={{ flex: 1 }}
        />
        <Dropdown
          style={{ minWidth: 230 }}
          value={LEVEL_LABEL[minLevel]}
          selectedOptions={[minLevel]}
          onOptionSelect={(_, d) => setMinLevel(d.optionValue ?? "info")}
        >
          {Object.entries(LEVEL_LABEL).map(([k, v]) => (
            <Option key={k} value={k}>
              {v}
            </Option>
          ))}
        </Dropdown>
        <Tooltip content="Копировать" relationship="label">
          <Button icon={<CopyRegular />} onClick={copy} disabled={!shown.length} />
        </Tooltip>
        <Tooltip content="Очистить" relationship="label">
          <Button icon={<DeleteRegular />} onClick={clear} />
        </Tooltip>
      </div>
      <div
        ref={boxRef}
        className="card selectable mono"
        onScroll={(e) => {
          const el = e.currentTarget;
          stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
        }}
        style={{ flex: 1, minHeight: 0, overflow: "auto", padding: "10px 14px", fontSize: 12, lineHeight: "19px" }}
      >
        {shown.length === 0 && <div className="hint">Пока пусто</div>}
        {shown.map((l) => (
          <div key={l.id} style={{ whiteSpace: "pre-wrap", wordBreak: "break-word", color: LEVEL_COLOR[l.level] }}>
            <span style={{ opacity: 0.55 }}>{time(l.ts)} </span>
            {l.msg}
          </div>
        ))}
      </div>
    </div>
  );
}

function time(ts: number) {
  return new Date(ts).toLocaleTimeString("ru-RU");
}
