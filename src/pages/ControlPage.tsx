import { ReactNode, useState } from "react";
import { Button, Spinner, Switch, Tooltip } from "@fluentui/react-components";
import {
  ArrowSyncRegular,
  GlobeShieldFilled,
  SendFilled,
  ShieldCheckmarkFilled,
} from "@fluentui/react-icons";
import { api, formatDuration, VpnState } from "../api";
import { useNow, useTgStatus, useVpnStatus, useZapretStatus } from "../hooks";
import { useNotify } from "../toast";

interface Unit {
  key: string;
  title: string;
  about: string;
  icon: ReactNode;
  state: VpnState;
  startedAt?: number;
  error?: string;
  on: () => Promise<void>;
  off: () => Promise<void>;
}

export function ControlPage() {
  const notify = useNotify();
  const vpn = useVpnStatus();
  const tg = useTgStatus();
  const dpi = useZapretStatus();
  const [restarting, setRestarting] = useState(false);

  const units: Unit[] = [
    {
      key: "vpn",
      title: "VPN",
      about: "Весь трафик через ваш сервер",
      icon: <ShieldCheckmarkFilled />,
      state: vpn.state,
      startedAt: vpn.startedAt,
      error: vpn.error,
      on: api.vpnStart,
      off: api.vpnStop,
    },
    {
      key: "tg",
      title: "TgWsProxy",
      about: "Telegram через WebSocket, без VPN",
      icon: <SendFilled />,
      state: tg.state,
      startedAt: tg.startedAt,
      error: tg.error,
      on: api.tgStart,
      off: api.tgStop,
    },
    {
      key: "dpi",
      title: "Zapret",
      about: "Discord, YouTube и другие — без VPN",
      icon: <GlobeShieldFilled />,
      state: dpi.state,
      startedAt: dpi.startedAt,
      error: dpi.error,
      on: api.zapretStart,
      off: api.zapretStop,
    },
  ];

  const anyRunning = units.some((u) => u.state === "running");

  const toggle = async (u: Unit, want: boolean) => {
    try {
      await (want ? u.on() : u.off());
    } catch (e) {
      notify.error(`${u.title}: не удалось ${want ? "включить" : "выключить"}`, e);
    }
  };

  const restartAll = async () => {
    setRestarting(true);
    try {
      const done = await api.restartAll();
      notify.ok(done.length ? `Перезапущено: ${done.join(", ")}` : "Перезапускать нечего — всё выключено");
    } catch (e) {
      notify.error("Не удалось перезапустить", e);
    } finally {
      setRestarting(false);
    }
  };

  return (
    <div className="page">
      <h1 className="page-title">Пульт</h1>
      <p className="hint" style={{ margin: "-4px 4px 12px" }}>
        Три способа обойти блокировки. Их можно включать по отдельности или вместе.
      </p>

      <div className="units">
        {units.map((u) => (
          <UnitTile key={u.key} unit={u} onToggle={toggle} />
        ))}
      </div>

      <div className="card row" style={{ marginTop: 14 }}>
        <div className="text">
          <span className="title">Перезапустить всё</span>
          <span className="desc">
            {anyRunning
              ? "Поднимет заново то, что сейчас включено. Помогает, когда связь «залипла» после смены сети или сна"
              : "Сейчас ничего не включено"}
          </span>
        </div>
        <Button
          appearance="primary"
          icon={restarting ? <Spinner size="tiny" /> : <ArrowSyncRegular />}
          onClick={restartAll}
          disabled={restarting || !anyRunning}
        >
          Перезапустить
        </Button>
      </div>
    </div>
  );
}

const LABEL: Record<string, string> = {
  starting: "Запуск…",
  stopping: "Остановка…",
  stopped: "Выключено",
  running: "Включено",
};

function UnitTile({ unit, onToggle }: { unit: Unit; onToggle: (u: Unit, want: boolean) => void }) {
  const busy = unit.state === "starting" || unit.state === "stopping";
  const running = unit.state === "running";
  const now = useNow(1000, running);

  const tile = (
    <div className={`unit card${running ? " on" : ""}`}>
      <div className="unit-icon">{busy ? <Spinner size="small" /> : unit.icon}</div>
      <span className="unit-title">{unit.title}</span>
      <span className="hint unit-about">{unit.about}</span>
      <div className="unit-foot">
        <Switch
          checked={running || unit.state === "starting"}
          disabled={busy}
          onChange={(_, d) => onToggle(unit, d.checked)}
          label={running ? formatDuration(now - (unit.startedAt ?? now)) : LABEL[unit.state]}
        />
      </div>
    </div>
  );

  return unit.error && unit.state === "stopped" ? (
    <Tooltip content={unit.error} relationship="description">
      {tile}
    </Tooltip>
  ) : (
    tile
  );
}
