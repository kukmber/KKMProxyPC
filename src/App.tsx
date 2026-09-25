import { useState } from "react";
import { Toaster, Tooltip, useId } from "@fluentui/react-components";
import {
  ShieldCheckmarkRegular,
  ShieldCheckmarkFilled,
  SendRegular,
  SendFilled,
  GlobeShieldRegular,
  GlobeShieldFilled,
  TextBulletListLtrRegular,
  TextBulletListLtrFilled,
  SettingsRegular,
  SettingsFilled,
  BoardSplitRegular,
  BoardSplitFilled,
  FluentIcon,
} from "@fluentui/react-icons";
import { ControlPage } from "./pages/ControlPage";
import { VpnPage } from "./pages/VpnPage";
import { TgPage } from "./pages/TgPage";
import { ZapretPage } from "./pages/ZapretPage";
import { LogsPage } from "./pages/LogsPage";
import { SettingsPage } from "./pages/SettingsPage";
import { useCores, useTgStatus, useVpnStatus, useZapretStatus } from "./hooks";
import { ToasterIdContext } from "./toast";
import cucumber from "./assets/cucumber.png";

type Page = "control" | "vpn" | "tg" | "dpi" | "logs" | "settings";

export default function App() {
  const [page, setPage] = useState<Page>("control");
  const status = useVpnStatus();
  const tg = useTgStatus();
  const dpi = useZapretStatus();
  const [cores] = useCores();
  const updates = cores.some((c) => c.updateAvailable);
  const toasterId = useId("toaster");

  const top: { id: Page; label: string; icon: FluentIcon; active: FluentIcon; dot?: string }[] = [
    { id: "control", label: "Пульт", icon: BoardSplitRegular, active: BoardSplitFilled },
    { id: "vpn", label: "VPN", icon: ShieldCheckmarkRegular, active: ShieldCheckmarkFilled, dot: status.state },
    { id: "tg", label: "Прокси для Telegram", icon: SendRegular, active: SendFilled, dot: tg.state },
    { id: "dpi", label: "Zapret — обход блокировок", icon: GlobeShieldRegular, active: GlobeShieldFilled, dot: dpi.state },
    { id: "logs", label: "Журнал", icon: TextBulletListLtrRegular, active: TextBulletListLtrFilled },
  ];

  const railButton = (id: Page, label: string, Icon: FluentIcon, Active: FluentIcon, dot?: string) => (
    <Tooltip key={id} content={label} relationship="label" positioning="after" withArrow>
      <button className={`rail-btn${page === id ? " active" : ""}`} onClick={() => setPage(id)}>
        {page === id ? <Active /> : <Icon />}
        {dot && dot !== "stopped" && <span className="rail-dot" data-state={dot} />}
      </button>
    </Tooltip>
  );

  return (
    <ToasterIdContext.Provider value={toasterId}>
      <div className="shell">
        <nav className="rail">
          <img className="rail-mark" src={cucumber} alt="KKMProxy" draggable={false} />
          {top.map((t) => railButton(t.id, t.label, t.icon, t.active, t.dot))}
          <div style={{ flex: 1 }} />
          {railButton(
            "settings",
            updates ? "Настройки — есть обновления ядер" : "Настройки",
            SettingsRegular,
            SettingsFilled,
            updates ? "update" : undefined,
          )}
        </nav>
        <main className="content">
          {page === "control" && <ControlPage />}
          {page === "vpn" && <VpnPage status={status} />}
          {page === "tg" && <TgPage />}
          {page === "dpi" && <ZapretPage />}
          {page === "logs" && <LogsPage />}
          {page === "settings" && <SettingsPage />}
        </main>
      </div>
      <Toaster toasterId={toasterId} position="bottom-end" />
    </ToasterIdContext.Provider>
  );
}
