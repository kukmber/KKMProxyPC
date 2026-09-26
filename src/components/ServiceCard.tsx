import { ReactNode, useState } from "react";
import { Button, Spinner, Switch } from "@fluentui/react-components";
import { ArrowDownloadRegular, SparkleRegular } from "@fluentui/react-icons";
import { api, CoreInfo, VpnState } from "../api";
import { useNotify } from "../toast";

/** Полоса «вышла новая версия ядра» вверху раздела. */
export function UpdateBanner({ core }: { core?: CoreInfo }) {
  const notify = useNotify();
  const [hidden, setHidden] = useState(false);
  const [busy, setBusy] = useState(false);
  if (!core || !core.updateAvailable || hidden) return null;

  const update = async () => {
    setBusy(true);
    try {
      const v = await api.updateCore(core.id);
      notify.ok(`${core.title} обновлён до ${v}`);
    } catch (e) {
      notify.error(`Не удалось обновить ${core.title}`, e);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="banner">
      <div className="banner-icon">
        <SparkleRegular />
      </div>
      <div className="text">
        <span className="title">
          Доступно обновление {core.title} — {core.latest}
        </span>
        <span className="desc">Текущая версия: {core.version ?? "неизвестна"}</span>
      </div>
      <Button appearance="subtle" onClick={() => setHidden(true)} disabled={busy}>
        Позже
      </Button>
      <Button
        icon={busy ? <Spinner size="tiny" /> : <ArrowDownloadRegular />}
        onClick={update}
        disabled={busy}
      >
        Обновить
      </Button>
    </div>
  );
}

/** Карточка раздела: значок, название, адрес, тумблер и версия ядра. */
export function ServiceCard(props: {
  icon: ReactNode;
  title: string;
  subtitle: string;
  state: VpnState;
  version?: string;
  hint?: string;
  error?: string;
  disabled?: boolean;
  onToggle: (on: boolean) => void;
  action?: ReactNode;
}) {
  const running = props.state === "running";
  const busy = props.state === "starting" || props.state === "stopping";
  const label = { running: "On", starting: "Запуск…", stopping: "Остановка…", stopped: "Off" }[props.state];

  return (
    <div className={`service card${running ? " on" : ""}`}>
      <div className="service-icon">{busy ? <Spinner size="small" /> : props.icon}</div>
      <div className="text">
        <span className="service-title">{props.title}</span>
        <span className="desc mono">{props.subtitle}</span>
      </div>
      {props.action ?? (
        <div className="service-toggle">
          <Switch
            checked={running || props.state === "starting"}
            disabled={busy || props.disabled}
            onChange={(_, d) => props.onToggle(d.checked)}
          />
          {props.version && <span className="hint">{props.version}</span>}
        </div>
      )}
      <div className="service-foot">
        <span className={`service-state${running ? " on" : ""}`}>{label}</span>
        <span className={`hint${props.error ? " error" : ""}`}>
          {props.error ?? props.hint ?? (running ? "" : "Нажмите для запуска.")}
        </span>
      </div>
    </div>
  );
}
