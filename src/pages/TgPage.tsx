import { useEffect, useState } from "react";
import { Button, Field, Input, Spinner, Tooltip } from "@fluentui/react-components";
import {
  CopyRegular,
  OpenRegular,
  PlayRegular,
  SendRegular,
  StopRegular,
} from "@fluentui/react-icons";
import { openUrl } from "@tauri-apps/plugin-opener";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { api, formatDuration, TgStatus } from "../api";
import { useNow, useTgStatus } from "../hooks";
import { useNotify } from "../toast";

export function TgPage() {
  const notify = useNotify();
  const status = useTgStatus();
  const [port, setPort] = useState("");
  const [conns, setConns] = useState(0);
  const [busyPort, setBusyPort] = useState(false);

  useEffect(() => {
    api.getSettings().then((s) => setPort(String(s.tgPort)));
  }, []);

  // Своей статистики ядро не отдаёт — число клиентов считает Windows.
  useEffect(() => {
    if (status.state !== "running") return setConns(0);
    const tick = () => api.tgConnections().then(setConns).catch(() => {});
    tick();
    const id = setInterval(tick, 2000);
    return () => clearInterval(id);
  }, [status.state]);

  const running = status.state === "running";
  const busy = status.state === "starting" || status.state === "stopping";

  const toggle = async () => {
    try {
      if (running || status.state === "starting") await api.tgStop();
      else await api.tgStart();
    } catch (e) {
      notify.error("Не удалось запустить прокси", e);
    }
  };

  const applyPort = async () => {
    const n = Number(port);
    if (!Number.isInteger(n) || n < 1 || n > 65535) return notify.error("Порт должен быть числом от 1 до 65535");
    setBusyPort(true);
    try {
      await api.setTgPort(n);
      notify.ok(running ? "Порт изменён, прокси перезапущен" : "Порт изменён");
    } catch (e) {
      notify.error("Не удалось изменить порт", e);
    } finally {
      setBusyPort(false);
    }
  };

  return (
    <div className="page">
      <h1 className="page-title">Прокси для Telegram</h1>

      <div className="card tg-hero">
        <div className="orb small" data-state={status.state}>
          <div className="orb-core">{busy ? <Spinner size="small" /> : <SendRegular style={{ fontSize: 28 }} />}</div>
        </div>
        <div className="text">
          <span className="title" style={{ fontSize: 17, fontWeight: 600 }}>
            {{ running: "Работает", starting: "Запуск…", stopping: "Остановка…", stopped: "Остановлен" }[status.state]}
          </span>
          <span className={`desc${status.error ? " error" : ""}`}>
            {status.error ??
              (running
                ? "Telegram ходит через WebSocket — помогает, когда прямой доступ режут"
                : "Запустите и примените ссылку в Telegram")}
          </span>
        </div>
        {running && <Uptime status={status} conns={conns} />}
        <Button
          appearance={running || status.state === "starting" ? "secondary" : "primary"}
          size="large"
          icon={running || status.state === "starting" ? <StopRegular /> : <PlayRegular />}
          disabled={status.state === "stopping"}
          onClick={toggle}
          style={{ borderRadius: 999, minWidth: 148 }}
        >
          {running || status.state === "starting" ? "Остановить" : "Запустить"}
        </Button>
      </div>

      <div className="section-title">Ссылка для Telegram</div>
      <div className="card">
        <div className="row" style={{ gap: 10 }}>
          <span className={`mono ellipsis link-box${running ? "" : " off"}`}>
            {status.link ?? "Появится после запуска"}
          </span>
          <Tooltip content="Копировать ссылку" relationship="label">
            <Button
              icon={<CopyRegular />}
              disabled={!status.link}
              onClick={() => writeText(status.link!).then(() => notify.ok("Ссылка скопирована"))}
            />
          </Tooltip>
          <Button
            appearance="primary"
            icon={<OpenRegular />}
            disabled={!status.link}
            onClick={() =>
              openUrl(status.link!).catch((e) =>
                notify.error("Не удалось открыть Telegram", e),
              )
            }
          >
            Применить в Telegram
          </Button>
        </div>
      </div>
      <p className="hint" style={{ margin: "2px 4px" }}>
        Кнопка открывает Telegram с готовыми настройками — останется подтвердить. Прокси слушает только этот компьютер,
        другим устройствам ссылка не подойдёт.
      </p>

      <div className="section-title">Настройки</div>
      <div className="card">
        <div className="row">
          <div className="text">
            <span className="title">Порт</span>
            <span className="desc">Если порт занят, прокси возьмёт свободный. Секрет постоянный — ссылка не меняется.</span>
          </div>
          <Field>
            <Input
              value={port}
              onChange={(_, d) => setPort(d.value.replace(/\D/g, "").slice(0, 5))}
              onKeyDown={(e) => e.key === "Enter" && applyPort()}
              style={{ width: 110 }}
              className="mono"
            />
          </Field>
          <Button onClick={applyPort} disabled={busyPort}>
            Применить
          </Button>
        </div>
      </div>
      {running && status.port !== undefined && status.port !== Number(port) && (
        <p className="hint" style={{ margin: "2px 4px", color: "var(--mid)" }}>
          Порт {port} занят другой программой — прокси слушает {status.port}. Ссылка выше уже с этим портом.
        </p>
      )}
    </div>
  );
}

function Uptime({ status, conns }: { status: TgStatus; conns: number }) {
  const now = useNow(1000, true);
  return (
    <div style={{ textAlign: "right", display: "flex", flexDirection: "column", gap: 2 }}>
      <span style={{ fontSize: 19, fontWeight: 600, fontVariantNumeric: "tabular-nums" }}>
        {formatDuration(now - (status.startedAt ?? now))}
      </span>
      <span className="hint">
        {conns} {plural(conns, "подключение", "подключения", "подключений")}
      </span>
    </div>
  );
}

function plural(n: number, one: string, few: string, many: string) {
  const m10 = n % 10;
  const m100 = n % 100;
  if (m10 === 1 && m100 !== 11) return one;
  if (m10 >= 2 && m10 <= 4 && (m100 < 12 || m100 > 14)) return few;
  return many;
}
