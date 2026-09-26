import { useEffect, useState } from "react";
import { Button, Checkbox, Link, Textarea, Tooltip } from "@fluentui/react-components";
import {
  ChevronDownRegular,
  ChevronRightRegular,
  CheckmarkCircleFilled,
  DismissCircleRegular,
  GlobeShieldRegular,
  ListRegular,
  WandRegular,
} from "@fluentui/react-icons";
import { api, HostsState, ProbeResult, StrategyInfo } from "../api";
import { useCores, useZapretStatus } from "../hooks";
import { useNotify } from "../toast";
import { ServiceCard, UpdateBanner } from "../components/ServiceCard";
import { AutotuneDialog } from "../components/AutotuneDialog";
import { DpiWarning } from "../components/DpiWarning";

const CUSTOM = "custom";

/** Результаты последнего подбора: переживают переход между вкладками. */
let lastProbe: ProbeResult[] | null = null;

export function ZapretPage() {
  const notify = useNotify();
  const status = useZapretStatus();
  const [cores] = useCores();
  const [strategies, setStrategies] = useState<StrategyInfo[]>([]);
  const [current, setCurrent] = useState("general");
  const [custom, setCustom] = useState("");
  const [hosts, setHosts] = useState<HostsState | null>(null);
  const [hostsOpen, setHostsOpen] = useState(false);
  const [customHosts, setCustomHosts] = useState("");
  const [elevated, setElevated] = useState(true);
  const [busy, setBusy] = useState(false);
  const [tuning, setTuning] = useState(false);
  const [probe, setProbe] = useState<ProbeResult[] | null>(lastProbe);
  const core = cores.find((c) => c.id === "zapret");

  useEffect(() => {
    api.zapretStrategies().then(setStrategies);
    api.getSettings().then((s) => {
      setCurrent(s.zapretStrategy);
      setCustom(s.zapretCustom);
    });
    api.zapretHosts().then((h) => {
      setHosts(h);
      setCustomHosts(h.custom);
    });
    api.platformInfo().then((p) => setElevated(p.elevated));
  }, []);

  const running = status.state === "running";
  const title = current === CUSTOM ? "Своя" : (strategies.find((s) => s.id === current)?.title ?? current);
  // Счёт «рабочих» показываем только после настоящей проверки: у неё есть
  // замеры с ненулевым числом сайтов.
  const tested = probe?.some((p) => p.total > 0) ? probe : null;
  const working = tested ? tested.filter((p) => p.id !== "none" && !p.error && p.ok > 0).length : null;
  // Лучшая — та, что открыла больше всех и больше, чем без обхода.
  const baseline = tested?.find((p) => p.id === "none");
  const bestId = (() => {
    const top = tested
      ?.filter((p) => p.id !== "none" && !p.error)
      .reduce<ProbeResult | null>((a, b) => (!a || b.ok > a.ok ? b : a), null);
    return top && baseline && top.ok > baseline.ok ? top.id : null;
  })();

  const toggle = async (on: boolean) => {
    try {
      if (on) await api.zapretStart();
      else await api.zapretStop();
    } catch (e) {
      notify.error("Не удалось включить обход", e);
    }
  };

  const pick = async (id: string) => {
    if (id === current || busy) return;
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

  const saveCustom = async () => {
    setBusy(true);
    try {
      await api.setZapretCustom(custom);
      notify.ok("Своя стратегия проверена и сохранена");
    } catch (e) {
      notify.error("winws не принял параметры", e);
    } finally {
      setBusy(false);
    }
  };

  const saveHosts = async (next: { sets?: string[]; custom?: string }) => {
    if (!hosts) return;
    const sets = next.sets ?? hosts.sets;
    const customText = next.custom ?? customHosts;
    setBusy(true);
    try {
      const total = await api.setZapretHosts(sets, customText);
      setHosts({ ...hosts, sets, custom: customText, total });
      if (next.custom !== undefined) notify.ok(`В списке ${total} адресов`);
    } catch (e) {
      notify.error("Не удалось сохранить список", e);
    } finally {
      setBusy(false);
    }
  };

  const toggleSet = (id: string) => {
    if (!hosts) return;
    const sets = hosts.sets.includes(id) ? hosts.sets.filter((s) => s !== id) : [...hosts.sets, id];
    saveHosts({ sets });
  };

  const rows = [
    ...strategies,
    { id: CUSTOM, title: "Своя", about: "Аргументы winws как есть — для тех, кто знает, что пишет" },
  ];

  return (
    <div className="page">
      <h1 className="page-title">Zapret</h1>

      <DpiWarning />

      <UpdateBanner core={core} />

      <ServiceCard
        icon={<GlobeShieldRegular />}
        title="Обход блокировок (Zapret)"
        subtitle={`Стратегия «${title}» · свой список: ${hosts?.total ?? 0}`}
        state={status.state}
        version={core?.version}
        error={status.error}
        disabled={!elevated}
        hint={
          elevated
            ? running
              ? "Discord, YouTube и другие сайты без VPN."
              : "Нажмите для запуска."
            : "Нужны права администратора — обход работает через драйвер WinDivert."
        }
        onToggle={toggle}
        action={
          !elevated ? (
            <Button onClick={() => api.restartAsAdmin(false).catch((e) => notify.error("Перезапуск отменён", e))}>
              Перезапустить от администратора
            </Button>
          ) : undefined
        }
      />

      <div className="card" style={{ padding: "14px 18px", marginTop: 10 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
          <ListRegular style={{ fontSize: 20, color: "var(--text-secondary)" }} />
          <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", gap: 2 }}>
            <span style={{ fontSize: 15, fontWeight: 600 }}>
              Список сайтов{" "}
              <span className="hint" style={{ fontWeight: 400 }}>{hosts?.total ?? 0} записей</span>
            </span>
            <span className="hint">
              Добавляются к собственному списку набора — тот уже покрывает Discord, YouTube и другие сервисы
            </span>
          </div>
          <Button
            appearance="subtle"
            icon={hostsOpen ? <ChevronDownRegular /> : <ChevronRightRegular />}
            iconPosition="after"
            onClick={() => setHostsOpen((v) => !v)}
          >
            {hostsOpen ? "Свернуть" : "Управление списком"}
          </Button>
        </div>

        {hostsOpen && hosts && (
          <>
            <div className="caps">Готовые наборы</div>
            <div className="sets">
              {hosts.available.map((s) => {
                const on = hosts.sets.includes(s.id);
                return (
                  <div key={s.id} className={`set${on ? " on" : ""}`} onClick={() => toggleSet(s.id)}>
                    <Checkbox checked={on} aria-label={s.title} onChange={() => toggleSet(s.id)} />
                    <span className="set-body">
                      <span className="set-title">
                        {s.title}
                        <span className="hint">{s.count} зап.</span>
                      </span>
                      <span className="hint">{s.about}</span>
                    </span>
                  </div>
                );
              })}
            </div>

            <div className="caps">Свои адреса</div>
            <Textarea
              value={customHosts}
              onChange={(_, d) => setCustomHosts(d.value)}
              placeholder={"example.com\n*.example.org\nmy.site.ru"}
              resize="vertical"
              textarea={{ className: "mono", style: { minHeight: 110 } }}
              style={{ width: "100%" }}
            />
            <div style={{ display: "flex", gap: 10, alignItems: "center", marginTop: 10 }}>
              <Button onClick={() => saveHosts({ custom: customHosts })} disabled={busy}>
                Сохранить список
              </Button>
              <span className="hint">По одному адресу в строке. Поддомены подхватываются сами.</span>
            </div>
          </>
        )}
      </div>

      <div className="section-title">
        Стратегии
        <span className="hint" style={{ fontWeight: 400 }}>
          {working !== null ? `${working} / ${tested!.length - 1} рабочих` : `${strategies.length} шт.`}
        </span>
        <span className="spacer" />
        <Button
          appearance="subtle"
          size="small"
          icon={<WandRegular />}
          onClick={() => setTuning(true)}
          disabled={!elevated}
        >
          {probe ? "Перепроверить" : "Подобрать автоматически"}
        </Button>
      </div>
      <div className="card strategy-list" style={{ padding: 6 }}>
        {rows.map((s) => {
          const r = probe?.find((p) => p.id === s.id);
          return (
            <button
              key={s.id}
              className={`strategy-row${current === s.id ? " on" : ""}`}
              onClick={() => pick(s.id)}
              disabled={busy || tuning}
            >
              <div className="text">
                <span style={{ fontWeight: current === s.id ? 600 : 400 }}>
                  {s.title}
                  {s.id === bestId && <span className="best-badge">лучшая</span>}
                </span>
                <span className="hint">{s.about}</span>
              </div>
              {r && !r.error && (
                <Tooltip
                  content={r.failed.length ? `не открылись: ${r.failed.join(", ")}` : "открылись все"}
                  relationship="label"
                >
                  <span style={{ display: "flex", alignItems: "center", gap: 6 }}>
                    {r.ok > 0 ? (
                      <CheckmarkCircleFilled style={{ color: "var(--ok)" }} />
                    ) : (
                      <DismissCircleRegular style={{ color: "var(--bad)" }} />
                    )}
                    <span className="hint" style={{ fontVariantNumeric: "tabular-nums" }}>
                      {r.ok}/{r.total}
                    </span>
                  </span>
                </Tooltip>
              )}
            </button>
          );
        })}
        {current === CUSTOM && (
          <div style={{ display: "flex", flexDirection: "column", gap: 8, padding: "4px 14px 12px" }}>
            <Textarea
              value={custom}
              onChange={(_, d) => setCustom(d.value)}
              placeholder="--wf-tcp=80,443 --dpi-desync=fake,split2 --dpi-desync-autottl=2"
              resize="vertical"
              textarea={{ className: "mono", style: { minHeight: 88 } }}
            />
            <div style={{ display: "flex", gap: 10, alignItems: "center" }}>
              <Button appearance="primary" onClick={saveCustom} disabled={busy}>
                Проверить и сохранить
              </Button>
              <span className="hint">Параметры прогоняются через winws — неверный набор не сохранится.</span>
            </div>
          </div>
        )}
      </div>
      <p className="hint" style={{ margin: "2px 4px" }}>
        Стратегии — это набор{" "}
        <Link onClick={() => api.openExternal("https://github.com/Flowseal/zapret-discord-youtube")}>
          Flowseal/zapret-discord-youtube
        </Link>{" "}
        на ядре{" "}
        <Link onClick={() => api.openExternal("https://github.com/bol-van/zapret")}>bol-van/zapret</Link>. Провайдеры
        фильтруют по-разному, поэтому нормально, что подходит не первая.
      </p>

      <AutotuneDialog
        open={tuning}
        domains={[]}
        onClose={() => setTuning(false)}
        onResults={(r) => {
          lastProbe = r;
          setProbe(r);
        }}
        onApply={(id) => {
          setTuning(false);
          pick(id);
        }}
      />
    </div>
  );
}
