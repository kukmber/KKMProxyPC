import { useEffect, useState } from "react";
import {
  Button,
  Drawer,
  DrawerBody,
  DrawerHeader,
  DrawerHeaderTitle,
  Link,
  Menu,
  MenuDivider,
  MenuItem,
  MenuItemRadio,
  MenuList,
  MenuPopover,
  MenuTrigger,
  Spinner,
} from "@fluentui/react-components";
import {
  AddRegular,
  ArrowDownRegular,
  ArrowUpRegular,
  ChevronDownRegular,
  DismissRegular,
  PowerRegular,
  SettingsRegular,
  ShieldRegular,
} from "@fluentui/react-icons";
import { api, Connection, formatBytes, formatDuration, Mode, PlatformInfo, Profile, Settings, VpnStatus } from "../api";
import { useMihomo, useNow, useTraffic } from "../hooks";
import { useNotify } from "../toast";
import { cleanName, countryOf, Flag } from "../flags";
import { ProfilesSection } from "../components/ProfilesSection";
import { AddProfileDialog } from "../components/AddProfileDialog";
import { delayColor, ServersSection, TYPE_LABEL } from "../components/ServersSection";
import { Segmented } from "../components/Segmented";
import { useServers } from "../components/useServers";

export function VpnPage({ status }: { status: VpnStatus }) {
  const notify = useNotify();
  const [settings, setSettings] = useState<Settings | null>(null);
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [platform, setPlatform] = useState<PlatformInfo | null>(null);
  const [drawer, setDrawer] = useState(false);
  const [adding, setAdding] = useState(false);
  const mihomo = useMihomo(status);
  const servers = useServers(mihomo, settings?.mode ?? "rule");

  const reload = () => {
    api.listProfiles().then(setProfiles);
    api.getSettings().then(setSettings);
  };
  useEffect(() => {
    reload();
    api.platformInfo().then(setPlatform);
  }, []);

  const running = status.state === "running";
  const needsAdmin = settings?.connection === "tun" && platform && !platform.elevated;
  const active = profiles.find((p) => p.id === settings?.activeProfile);

  const toggle = async () => {
    try {
      if (running || status.state === "starting") await api.vpnStop();
      else await api.vpnStart();
    } catch (e) {
      notify.error("Не удалось подключиться", e);
    }
  };

  const changeMode = (mode: Mode) => {
    setSettings((s) => s && { ...s, mode });
    api.setMode(mode).catch((e) => notify.error("Не удалось сменить режим", e));
  };

  const changeConnection = (connection: Connection) => {
    setSettings((s) => s && { ...s, connection });
    api.setConnection(connection).catch((e) => notify.error("Не удалось переподключиться", e));
  };

  const switchProfile = (id: string) => {
    setSettings((s) => s && { ...s, activeProfile: id });
    api.setActiveProfile(id).then(reload).catch((e) => notify.error("Не удалось переключить подписку", e));
  };

  return (
    <div className="vpn">
      <section className="pane pane-console">
        <div className="console-top">
          {profiles.length ? (
            <Menu
              checkedValues={{ profile: settings?.activeProfile ? [settings.activeProfile] : [] }}
              onCheckedValueChange={(_, d) => d.checkedItems[0] && switchProfile(d.checkedItems[0])}
            >
              <MenuTrigger disableButtonEnhancement>
                <button className="profile-chip" title="Подписка">
                  <span className="ellipsis">{active?.name ?? "Выберите подписку"}</span>
                  <ChevronDownRegular />
                </button>
              </MenuTrigger>
              <MenuPopover>
                <MenuList>
                  {profiles.map((p) => (
                    <MenuItemRadio key={p.id} name="profile" value={p.id}>
                      {p.name}
                    </MenuItemRadio>
                  ))}
                  <MenuDivider />
                  <MenuItem icon={<AddRegular />} onClick={() => setAdding(true)}>
                    Добавить подписку…
                  </MenuItem>
                  <MenuItem icon={<SettingsRegular />} onClick={() => setDrawer(true)}>
                    Управление подписками
                  </MenuItem>
                </MenuList>
              </MenuPopover>
            </Menu>
          ) : null}
        </div>

        <Hero status={status} activeServer={servers.active} delayMs={servers.active ? servers.delays[servers.active.name]?.ms : undefined} />

        {profiles.length === 0 ? (
          <Button appearance="primary" size="large" icon={<AddRegular />} onClick={() => setAdding(true)} className="power">
            Добавить подписку
          </Button>
        ) : (
          <Button
            appearance={running || status.state === "starting" ? "secondary" : "primary"}
            size="large"
            icon={<PowerRegular />}
            className="power"
            disabled={(!active || !!needsAdmin) && !running || status.state === "stopping"}
            onClick={toggle}
          >
            {running || status.state === "starting" ? "Отключить" : "Подключить"}
          </Button>
        )}

        {running && <Speeds status={status} />}

        <div className="console-settings">
          <div className="setting">
            <span className="setting-label">Маршрут</span>
            <Segmented
              value={settings?.mode ?? "rule"}
              onChange={changeMode}
              options={[
                { value: "rule", label: "Правила", title: "Правила подписки решают, что идёт через VPN" },
                { value: "global", label: "Всё", title: "Весь трафик через VPN" },
                { value: "direct", label: "Напрямую", title: "VPN не используется" },
              ]}
            />
          </div>
          <div className="setting">
            <span className="setting-label">Способ</span>
            <Segmented
              value={settings?.connection ?? "sysproxy"}
              onChange={changeConnection}
              options={[
                { value: "sysproxy", label: "Системный прокси", title: "Без прав администратора; часть программ прокси игнорирует" },
                { value: "tun", label: "TUN", title: "Весь трафик системы, нужны права администратора" },
              ]}
            />
          </div>
          {needsAdmin && (
            <span className="hint">
              TUN перехватывает весь трафик системы, поэтому нужны права администратора.{" "}
              <Link onClick={() => api.restartAsAdmin(true).catch((e) => notify.error("Перезапуск отменён", e))}>
                Перезапустить
              </Link>
            </span>
          )}
        </div>
      </section>

      <ServersSection servers={servers} connected={running} mihomo={mihomo} />

      <Drawer type="overlay" position="end" size="medium" open={drawer} onOpenChange={(_, d) => setDrawer(d.open)}>
        <DrawerHeader>
          <DrawerHeaderTitle
            action={<Button appearance="subtle" icon={<DismissRegular />} onClick={() => setDrawer(false)} aria-label="Закрыть" />}
          >
            Подписки
          </DrawerHeaderTitle>
        </DrawerHeader>
        <DrawerBody>
          <ProfilesSection profiles={profiles} activeId={settings?.activeProfile} onChanged={reload} />
        </DrawerBody>
      </Drawer>

      <AddProfileDialog
        open={adding}
        onClose={() => setAdding(false)}
        onAdded={(p) => {
          setAdding(false);
          notify.ok("Подписка добавлена", p.name);
          reload();
        }}
      />
    </div>
  );
}

function Hero(props: { status: VpnStatus; activeServer: { name: string; type: string } | null; delayMs?: number }) {
  const { status, activeServer } = props;
  const running = status.state === "running";
  const busy = status.state === "starting" || status.state === "stopping";
  const now = useNow(1000, running);
  const hasFlag = running && activeServer && countryOf(activeServer.name);

  return (
    <div className="hero">
      <div className="orb" data-state={status.state}>
        <div className="orb-core">
          {busy ? (
            <Spinner size="medium" />
          ) : hasFlag ? (
            <Flag name={activeServer!.name} size={46} />
          ) : (
            <ShieldRegular style={{ fontSize: 40 }} />
          )}
        </div>
      </div>

      <div className="hero-state">
        {{ running: "Защищено", starting: "Подключение", stopping: "Отключение", stopped: "Не подключено" }[status.state]}
      </div>

      {running ? (
        <>
          <div className="hero-timer">{formatDuration(now - (status.startedAt ?? now))}</div>
          {activeServer && (
            <div className="hero-server">
              <span className="ellipsis">{cleanName(activeServer.name)}</span>
              <span className="hint">{TYPE_LABEL[activeServer.type] ?? activeServer.type}</span>
              {props.delayMs && <span style={{ color: delayColor(props.delayMs) }}>{props.delayMs} мс</span>}
            </div>
          )}
        </>
      ) : (
        <div className={`hero-sub${status.error && status.state === "stopped" ? " error selectable" : ""}`}>
          {status.state === "starting" ? status.stage ?? "Запуск" : status.error && status.state === "stopped" ? status.error : " "}
        </div>
      )}
    </div>
  );
}

function Speeds({ status }: { status: VpnStatus }) {
  const traffic = useTraffic(useMihomo(status));
  return (
    <div className="speeds">
      <div>
        <ArrowDownRegular />
        <b>{formatBytes(traffic.down)}/с</b>
      </div>
      <div>
        <ArrowUpRegular />
        <b>{formatBytes(traffic.up)}/с</b>
      </div>
      <div className="hint" style={{ gridColumn: "1 / -1" }}>
        {formatBytes(traffic.downTotal + traffic.upTotal)} за сессию · {traffic.connections} соед.
        {status.connection !== "tun" && status.mixedPort ? ` · порт ${status.mixedPort}` : ""}
      </div>
    </div>
  );
}
