import { useEffect, useState } from "react";
import {
  Button,
  Link,
  MessageBar,
  MessageBarActions,
  MessageBarBody,
  MessageBarTitle,
  Spinner,
  Switch,
} from "@fluentui/react-components";
import { ArrowSyncRegular, ArrowDownloadRegular, CheckmarkCircleRegular } from "@fluentui/react-icons";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, CoreInfo, PlatformInfo, Startup } from "../api";
import { useCores } from "../hooks";
import { AppUpdateBanner, useAppUpdate } from "../components/AppUpdate";
import { Hotkeys } from "../components/Hotkeys";
import { useNotify } from "../toast";

const ROLE: Record<string, string> = {
  mihomo: "VPN",
  tgws: "Прокси для Telegram",
  zapret: "Обход блокировок",
};

export function SettingsPage() {
  const notify = useNotify();
  const [cores, setCores] = useCores();
  const [platform, setPlatform] = useState<PlatformInfo | null>(null);
  const [startup, setStartup] = useState<Startup | null>(null);
  const [tun, setTun] = useState(false);
  const [checking, setChecking] = useState(false);
  const appUpdate = useAppUpdate();
  const [busy, setBusy] = useState<Set<string>>(new Set());

  useEffect(() => {
    api.platformInfo().then(setPlatform);
    api.getStartup().then(setStartup);
    api.getSettings().then((s) => setTun(s.connection === "tun"));
  }, []);

  const change = async (patch: Partial<Startup>) => {
    if (!startup) return;
    const next = { ...startup, ...patch };
    setStartup(next);
    try {
      await api.setStartup(next);
      // Способ автозапуска выбирает программа, поэтому читаем его заново.
      setStartup(await api.getStartup());
    } catch (e) {
      setStartup(startup);
      notify.error("Не удалось изменить настройку", e);
    }
  };

  const check = async () => {
    setChecking(true);
    try {
      const list = await api.checkCoreUpdates();
      setCores(list);
      if (!list.some((c) => c.updateAvailable)) notify.ok("Все ядра последних версий");
    } catch (e) {
      notify.error("Не удалось проверить обновления", e);
    } finally {
      setChecking(false);
    }
  };

  const update = async (c: CoreInfo) => {
    setBusy((b) => new Set(b).add(c.id));
    try {
      const v = await api.updateCore(c.id);
      notify.ok(`${c.title} ${v} установлен`);
    } catch (e) {
      notify.error(`Не удалось обновить ${c.title}`, e);
    } finally {
      setBusy((b) => {
        const n = new Set(b);
        n.delete(c.id);
        return n;
      });
    }
  };

  const pending = cores.filter((c) => c.updateAvailable || !c.installed);

  const row = (title: string, desc: string, checked: boolean, on: (v: boolean) => void, disabled = false) => (
    <div className="row">
      <div className="text">
        <span className="title">{title}</span>
        <span className="desc">{desc}</span>
      </div>
      <Switch
        checked={checked}
        aria-label={title}
        disabled={disabled || !startup}
        onChange={(_, d) => on(d.checked)}
      />
    </div>
  );

  return (
    <div className="page">
      <h1 className="page-title">Настройки</h1>

      <AppUpdateBanner update={appUpdate.update} />

      <div className="section-title">Запуск</div>
      <div className="card">
        {row(
          "Запускать вместе с Windows",
          !startup?.withWindows
            ? "Программа появится в трее сразу после входа в систему"
            : startup.elevated
              ? "Задачей планировщика, с правами администратора — TUN и Zapret поднимутся"
              : "Записью в реестре, без прав администратора",
          startup?.withWindows ?? false,
          (v) => change({ withWindows: v }),
        )}
        {row(
          "Запускать свёрнутым",
          "При старте вместе с Windows окно не открывается",
          startup?.minimized ?? true,
          (v) => change({ minimized: v }),
          !startup?.withWindows || !startup?.tray,
        )}
        {row("Сразу включать VPN", "Подключение начнётся при запуске программы", startup?.vpn ?? false, (v) =>
          change({ vpn: v }),
        )}
        {row("Сразу включать TgWsProxy", "Прокси для Telegram поднимется сам", startup?.tg ?? false, (v) =>
          change({ tg: v }),
        )}
        {row(
          "Сразу включать Zapret",
          "Нужны права администратора, иначе включение не удастся",
          startup?.zapret ?? false,
          (v) => change({ zapret: v }),
        )}
      </div>

      {startup?.withWindows && !startup.elevated && (tun || startup.zapret) && (
        <MessageBar intent="warning" style={{ marginTop: 8 }}>
          <MessageBarBody>
            <MessageBarTitle>Автозапуск не поднимет {tun ? "режим TUN" : "обход блокировок"}</MessageBarTitle>
            Записи автозапуска Windows выполняет без прав администратора, а {tun ? "TUN" : "Zapret"} без них не
            работает. Перезапустите программу от администратора и включите автозапуск заново — тогда он будет
            создан задачей планировщика и получит права.
          </MessageBarBody>
          <MessageBarActions>
            <Button onClick={() => api.restartAsAdmin(false).catch((e) => notify.error("Перезапуск отменён", e))}>
              Перезапустить от администратора
            </Button>
          </MessageBarActions>
        </MessageBar>
      )}

      <div className="section-title">Горячие клавиши</div>
      <Hotkeys />
      <p className="hint" style={{ margin: "2px 4px" }}>
        Работают поверх любой программы — окно открывать не нужно. Нажмите кнопку и введите сочетание;
        нужен хотя бы один модификатор (Ctrl, Alt, Shift или Win) либо клавиша F1–F12. Esc — отмена.
      </p>

      <div className="section-title">Окно</div>
      <div className="card">
        {row(
          "Сворачивать в трей",
          startup?.tray
            ? "Крестик прячет окно, программа продолжает работать. Выход — через меню значка"
            : "Крестик полностью закрывает программу и останавливает всё, что запущено",
          startup?.tray ?? true,
          (v) => change({ tray: v, minimized: v ? (startup?.minimized ?? true) : false }),
        )}
      </div>

      <div className="section-title">
        Ядра
        <span className="spacer" />
        {pending.length > 1 && (
          <Button appearance="primary" size="small" onClick={() => pending.forEach(update)} disabled={busy.size > 0}>
            Обновить всё
          </Button>
        )}
        <Button
          appearance="subtle"
          size="small"
          icon={checking ? <Spinner size="tiny" /> : <ArrowSyncRegular />}
          onClick={check}
          disabled={checking}
        >
          Проверить
        </Button>
      </div>
      <div className="card">
        {cores.map((c) => {
          const working = busy.has(c.id);
          return (
            <div className="row" key={c.id}>
              <div className="text">
                <span className="title">
                  {c.title} <span className="hint">· {ROLE[c.id]}</span>
                </span>
                <span className="desc">
                  {!c.installed
                    ? "Не установлено — скачается при первом запуске"
                    : c.updateAvailable
                      ? `${c.version} → доступна ${c.latest}`
                      : `${c.version ?? "версия неизвестна"}${c.latest ? " · последняя" : ""}`}
                  {" · "}
                  <Link onClick={() => openUrl(`https://github.com/${c.repo}/releases`)}>{c.repo}</Link>
                </span>
              </div>
              {working ? (
                <Spinner size="tiny" label="Обновление…" />
              ) : c.updateAvailable || !c.installed ? (
                <Button icon={<ArrowDownloadRegular />} onClick={() => update(c)}>
                  {c.installed ? "Обновить" : "Скачать"}
                </Button>
              ) : c.latest ? (
                <CheckmarkCircleRegular style={{ fontSize: 20, color: "var(--ok)" }} />
              ) : null}
            </div>
          );
        })}
      </div>
      <p className="hint" style={{ margin: "2px 4px" }}>
        Новые версии проверяются сами раз в сутки — у значка настроек появится точка. Работающее ядро при обновлении
        перезапускается само.
      </p>

      <div className="section-title">Система</div>
      <div className="card">
        <div className="row">
          <div className="text">
            <span className="title">Права администратора</span>
            <span className="desc">
              {platform?.elevated ? "Есть — доступны TUN и обход блокировок" : "Нужны для режима TUN и Zapret"}
            </span>
          </div>
          {platform && !platform.elevated && (
            <Button onClick={() => api.restartAsAdmin(false).catch((e) => notify.error("Перезапуск отменён", e))}>
              Перезапустить от администратора
            </Button>
          )}
        </div>
        <div className="row">
          <div className="text">
            <span className="title">KKMProxy для Windows</span>
            <span className="desc">
              Версия {platform?.version}
              {appUpdate.update ? ` · доступна ${appUpdate.update.version}` : appUpdate.error ? ` · ${appUpdate.error}` : ""}
            </span>
          </div>
          <Button
            icon={appUpdate.checking ? <Spinner size="tiny" /> : <ArrowSyncRegular />}
            disabled={appUpdate.checking}
            onClick={async () => {
              const u = await appUpdate.check(false);
              if (!u) notify.ok("У вас последняя версия");
            }}
          >
            Проверить обновление
          </Button>
        </div>
      </div>
    </div>
  );
}
