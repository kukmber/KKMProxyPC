import { useEffect, useState } from "react";
import { Button, Link, Radio, RadioGroup, Spinner } from "@fluentui/react-components";
import { GlobeShieldRegular, PlayRegular, StopRegular } from "@fluentui/react-icons";
import { api, formatDuration, StrategyInfo } from "../api";
import { useNow, useZapretStatus } from "../hooks";
import { useNotify } from "../toast";

export function ZapretPage() {
  const notify = useNotify();
  const status = useZapretStatus();
  const [strategies, setStrategies] = useState<StrategyInfo[]>([]);
  const [current, setCurrent] = useState("general");
  const [elevated, setElevated] = useState(true);
  const [busy, setBusy] = useState(false);
  const now = useNow(1000, status.state === "running");

  useEffect(() => {
    api.zapretStrategies().then(setStrategies);
    api.getSettings().then((s) => setCurrent(s.zapretStrategy));
    api.platformInfo().then((p) => setElevated(p.elevated));
  }, []);

  const running = status.state === "running";
  const working = status.state === "starting" || status.state === "stopping";

  const toggle = async () => {
    try {
      if (running || status.state === "starting") await api.zapretStop();
      else await api.zapretStart();
    } catch (e) {
      notify.error("Не удалось включить обход", e);
    }
  };

  const pick = async (id: string) => {
    const prev = current;
    setCurrent(id);
    setBusy(true);
    try {
      await api.setZapretStrategy(id);
      notify.ok(running ? "Стратегия изменена, обход перезапущен" : "Стратегия выбрана");
    } catch (e) {
      setCurrent(prev);
      notify.error("Стратегия не подошла", e);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="page">
      <h1 className="page-title">Zapret</h1>
      <p className="hint" style={{ margin: "-4px 4px 12px", maxWidth: 720 }}>
        Обход блокировок — Discord, YouTube и другие сайты без помощи VPN. Работает для всей системы: трафик остаётся
        прямым, меняется только то, по чему провайдер узнаёт запрос. Тот же приём помогает и прокси для Telegram.
      </p>

      <div className="card tg-hero">
        <div className="orb small" data-state={status.state}>
          <div className="orb-core">
            {working ? <Spinner size="small" /> : <GlobeShieldRegular style={{ fontSize: 28 }} />}
          </div>
        </div>
        <div className="text">
          <span className="title" style={{ fontSize: 17, fontWeight: 600 }}>
            {{ running: "Включён", starting: "Включение…", stopping: "Выключение…", stopped: "Выключен" }[status.state]}
          </span>
          <span className={`desc${status.error ? " error" : ""}`}>
            {status.error ??
              (running
                ? `Стратегия «${status.strategy}» · ${formatDuration(now - (status.startedAt ?? now))}`
                : elevated
                  ? "Включите и проверьте нужный сайт"
                  : "Нужны права администратора — обход работает через драйвер WinDivert")}
          </span>
        </div>
        {!elevated && !running ? (
          <Button onClick={() => api.restartAsAdmin(false).catch((e) => notify.error("Перезапуск отменён", e))}>
            Перезапустить от администратора
          </Button>
        ) : (
          <Button
            appearance={running || status.state === "starting" ? "secondary" : "primary"}
            size="large"
            icon={running || status.state === "starting" ? <StopRegular /> : <PlayRegular />}
            disabled={status.state === "stopping"}
            onClick={toggle}
            style={{ borderRadius: 999, minWidth: 148 }}
          >
            {running || status.state === "starting" ? "Выключить" : "Включить"}
          </Button>
        )}
      </div>

      <div className="section-title">Стратегия</div>
      <div className="card" style={{ padding: "6px 18px 12px" }}>
        <RadioGroup value={current} onChange={(_, d) => pick(d.value)} disabled={busy}>
          {strategies.map((s) => (
            <Radio
              key={s.id}
              value={s.id}
              label={
                <span style={{ display: "flex", flexDirection: "column", gap: 2 }}>
                  <span>{s.title}</span>
                  <span className="hint">{s.about}</span>
                </span>
              }
            />
          ))}
        </RadioGroup>
      </div>
      <p className="hint" style={{ margin: "2px 4px" }}>
        Стратегии взяты из набора <Link onClick={() => api.openExternal("https://github.com/bol-van/zapret")}>bol-van/zapret</Link>.
        Если сайт не открылся — попробуйте вторую: провайдеры фильтруют по-разному. Редактор своих стратегий, списки
        доменов и автоподбор появятся позже.
      </p>
    </div>
  );
}
