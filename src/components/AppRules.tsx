import { useEffect, useState } from "react";
import {
  Button,
  Combobox,
  Option,
  Spinner,
  Tooltip,
} from "@fluentui/react-components";
import { AddRegular, DeleteRegular } from "@fluentui/react-icons";
import { api, AppRule } from "../api";
import { useNotify } from "../toast";

const ACTIONS: { value: string; label: string; about: string }[] = [
  { value: "vpn", label: "Через VPN", about: "Весь трафик программы идёт через сервер подписки" },
  { value: "direct", label: "Напрямую", about: "Программа ходит в обход VPN" },
  { value: "block", label: "Блокировать", about: "Программе закрыт доступ в сеть" },
];

/**
 * Правила «эта программа — так». Работают на Windows благодаря тому, что ядро
 * умеет определять, какая программа открыла соединение.
 */
export function AppRules({ connected }: { connected: boolean }) {
  const notify = useNotify();
  const [rules, setRules] = useState<AppRule[]>([]);
  const [processes, setProcesses] = useState<string[]>([]);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    api.appRules().then((r) => {
      setRules(r);
      setLoaded(true);
    });
    api.runningProcesses().then(setProcesses);
  }, []);

  const save = async (next: AppRule[]) => {
    const prev = rules;
    setRules(next);
    setBusy(true);
    try {
      await api.setAppRules(next);
      if (connected) notify.ok("Правила применены, VPN перезапущен");
    } catch (e) {
      setRules(prev);
      notify.error("Не удалось сохранить правила", e);
    } finally {
      setBusy(false);
    }
  };

  const add = () => {
    const name = draft.trim();
    if (!name) return;
    if (rules.some((r) => r.process.toLowerCase() === name.toLowerCase())) {
      return notify.error("Такое правило уже есть");
    }
    setDraft("");
    save([...rules, { process: name, action: "vpn" }]);
  };

  return (
    <div className="list" style={{ padding: "0 12px 12px" }}>
      <p className="hint" style={{ margin: "0 2px 10px" }}>
        Правило решает, куда идёт трафик конкретной программы, независимо от сайта. Например, Discord — через VPN,
        а игры — напрямую, чтобы не терять скорость. Применяются раньше правил подписки.
      </p>

      <div style={{ display: "flex", gap: 8, marginBottom: 10 }}>
        <Combobox
          freeform
          placeholder="Имя файла программы, например Discord.exe"
          value={draft}
          onInput={(e) => setDraft((e.target as HTMLInputElement).value)}
          onOptionSelect={(_, d) => setDraft(d.optionValue ?? "")}
          onKeyDown={(e) => e.key === "Enter" && add()}
          style={{ flex: 1, minWidth: 0 }}
        >
          {processes
            .filter((p) => !draft || p.toLowerCase().includes(draft.toLowerCase()))
            .slice(0, 50)
            .map((p) => (
              <Option key={p} value={p}>
                {p}
              </Option>
            ))}
        </Combobox>
        <Button appearance="primary" icon={<AddRegular />} onClick={add} disabled={busy || !draft.trim()}>
          Добавить
        </Button>
      </div>

      {!loaded && <Spinner size="tiny" />}
      {loaded && !rules.length && (
        <div className="hint" style={{ padding: "12px 2px" }}>
          Правил пока нет. Начните вводить имя — список подскажет запущенные программы.
        </div>
      )}

      {rules.map((r, i) => (
        <div className="apprule" key={r.process}>
          <span className="ellipsis mono" title={r.process}>
            {r.process}
          </span>
          <div className="segmented" style={{ flex: "0 0 auto" }}>
            {ACTIONS.map((a) => (
              <button
                key={a.value}
                title={a.about}
                className={r.action === a.value ? "on" : undefined}
                disabled={busy}
                onClick={() => {
                  if (r.action === a.value) return;
                  const next = [...rules];
                  next[i] = { ...r, action: a.value };
                  save(next);
                }}
              >
                {a.label}
              </button>
            ))}
          </div>
          <Tooltip content="Удалить правило" relationship="label">
            <Button
              appearance="subtle"
              size="small"
              icon={<DeleteRegular />}
              disabled={busy}
              onClick={() => save(rules.filter((x) => x.process !== r.process))}
            />
          </Tooltip>
        </div>
      ))}

      {!connected && rules.length > 0 && (
        <p className="hint" style={{ margin: "10px 2px 0" }}>
          Правила сохранены и применятся при подключении.
        </p>
      )}
    </div>
  );
}
