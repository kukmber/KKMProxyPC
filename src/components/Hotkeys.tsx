import { useEffect, useState } from "react";
import { Button, Tooltip } from "@fluentui/react-components";
import { DismissRegular, KeyboardRegular } from "@fluentui/react-icons";
import { api, HotkeyInfo } from "../api";
import { useNotify } from "../toast";

const TITLES: Record<string, string> = {
  vpn: "VPN",
  tg: "Прокси для Telegram",
  zapret: "Обход блокировок",
  restart: "Перезапустить всё",
  window: "Показать окно",
};

/** Сочетания работают поверх любой программы — не нужно открывать окно. */
export function Hotkeys() {
  const notify = useNotify();
  const [items, setItems] = useState<HotkeyInfo[]>([]);
  const [capturing, setCapturing] = useState<string | null>(null);

  useEffect(() => {
    api.hotkeys().then(setItems);
  }, []);

  const save = async (action: string, accelerator: string) => {
    try {
      await api.setHotkey(action, accelerator);
      setItems(await api.hotkeys());
      if (accelerator) notify.ok(`Назначено: ${pretty(accelerator)}`);
    } catch (e) {
      notify.error("Не удалось назначить", e);
    } finally {
      setCapturing(null);
    }
  };

  // Пока ловим нажатие, окно не должно реагировать на клавиши как обычно.
  useEffect(() => {
    if (!capturing) return;
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") return setCapturing(null);
      const accel = toAccelerator(e);
      if (accel) save(capturing, accel);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [capturing]);

  return (
    <div className="card">
      {items.map((h) => (
        <div className="row" key={h.action}>
          <div className="text">
            <span className="title">{TITLES[h.action] ?? h.action}</span>
            <span className="desc">{h.about}</span>
          </div>
          <Button
            icon={<KeyboardRegular />}
            appearance={capturing === h.action ? "primary" : "secondary"}
            onClick={() => setCapturing(capturing === h.action ? null : h.action)}
            style={{ minWidth: 168, fontVariantNumeric: "tabular-nums" }}
          >
            {capturing === h.action ? "Нажмите клавиши…" : h.accelerator ? pretty(h.accelerator) : "Не назначено"}
          </Button>
          <Tooltip content="Убрать сочетание" relationship="label">
            <Button
              appearance="subtle"
              icon={<DismissRegular />}
              disabled={!h.accelerator}
              onClick={() => save(h.action, "")}
            />
          </Tooltip>
        </div>
      ))}
    </div>
  );
}

/**
 * Собирает сочетание в том виде, который понимает система: Ctrl+Shift+V.
 * Одни модификаторы не считаются — нужна обычная клавиша.
 */
function toAccelerator(e: KeyboardEvent): string | null {
  const mods: string[] = [];
  if (e.ctrlKey) mods.push("Ctrl");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");
  if (e.metaKey) mods.push("Super");

  const key = e.code;
  let main = "";
  if (/^Key[A-Z]$/.test(key)) main = key.slice(3);
  else if (/^Digit\d$/.test(key)) main = key.slice(5);
  else if (/^F\d{1,2}$/.test(key)) main = key;
  else if (key === "Space") main = "Space";
  else if (key === "Enter") main = "Enter";
  else if (key.startsWith("Numpad")) main = `Num${key.slice(6)}`;
  else if (key === "Insert" || key === "Delete" || key === "Home" || key === "End") main = key;
  else if (key === "PageUp" || key === "PageDown") main = key;
  else if (key.startsWith("Arrow")) main = key.slice(5);
  if (!main) return null;

  // Без модификатора сочетание перехватит любую букву во всей системе.
  if (!mods.length && !/^F\d{1,2}$/.test(main)) return null;
  return [...mods, main].join("+");
}

function pretty(accel: string): string {
  return accel.replace(/\+/g, " + ").replace("Super", "Win");
}
