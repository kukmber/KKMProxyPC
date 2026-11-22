import { useEffect, useState } from "react";
import type { CSSProperties } from "react";
import { Button, Field, Input, Spinner, Switch, Textarea, Tooltip } from "@fluentui/react-components";
import {
  ArrowSyncRegular,
  ChevronDownRegular,
  ChevronRightRegular,
  CopyRegular,
  OpenRegular,
  SendRegular,
} from "@fluentui/react-icons";
import { openUrl } from "@tauri-apps/plugin-opener";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { api, TgAdvanced } from "../api";
import { useCores, useTgStatus } from "../hooks";
import { useNotify } from "../toast";
import { ServiceCard, UpdateBanner } from "../components/ServiceCard";

export function TgPage() {
  const notify = useNotify();
  const status = useTgStatus();
  const [cores] = useCores();
  const [host, setHost] = useState("127.0.0.1");
  const [port, setPort] = useState("1443");
  const [secret, setSecret] = useState("");
  const [busy, setBusy] = useState(false);
  const [advanced, setAdvanced] = useState<TgAdvanced | null>(null);
  const [openAdv, setOpenAdv] = useState(false);
  const [testing, setTesting] = useState<string | null>(null);
  const [testOut, setTestOut] = useState<Record<string, string>>({});
  const core = cores.find((c) => c.id === "tgws");

  const load = () =>
    api.getSettings().then((s) => {
      setHost(s.tgHost);
      setPort(String(s.tgPort));
      // Ключ показываем и до первого запуска — он уже сохранён в настройках.
      if (s.tgSecret) setSecret(s.tgSecret);
      if (s.tgAdvanced) setAdvanced(s.tgAdvanced);
    });

  useEffect(() => {
    load();
  }, []);

  // Пока прокси не запускали, ключа в состоянии нет — берём его из настроек.
  useEffect(() => {
    if (status.secret) setSecret(status.secret);
  }, [status.secret]);

  const running = status.state === "running";
  const realPort = running && status.port ? status.port : Number(port);

  const toggle = async (on: boolean) => {
    try {
      if (on) await api.tgStart();
      else await api.tgStop();
    } catch (e) {
      notify.error("Не удалось запустить прокси", e);
    }
  };

  const save = async () => {
    setBusy(true);
    try {
      await api.setTgParams(host, Number(port), secret);
      notify.ok(running ? "Параметры сохранены, прокси перезапущен" : "Параметры сохранены");
    } catch (e) {
      notify.error("Параметры не подошли", e);
      load();
    } finally {
      setBusy(false);
    }
  };

  const regenerate = async () => {
    setBusy(true);
    try {
      const s = await api.regenerateTgSecret();
      setSecret(s);
      notify.ok("Новый ключ создан", "Ссылку в Telegram нужно применить заново");
    } catch (e) {
      notify.error("Не удалось создать ключ", e);
    } finally {
      setBusy(false);
    }
  };

  const patch = (p: Partial<TgAdvanced>) => setAdvanced((a) => (a ? { ...a, ...p } : a));

  const saveAdvanced = async () => {
    if (!advanced) return;
    setBusy(true);
    try {
      await api.setTgAdvanced(advanced);
      notify.ok(running ? "Сохранено, прокси перезапущен" : "Сохранено");
    } catch (e) {
      notify.error("Не удалось сохранить", e);
    } finally {
      setBusy(false);
    }
  };

  // Проверка идёт тем же путём, что и работа: ядро пытается поднять
  // WebSocket через домен и показывает, что ответило.
  const test = async (kind: "cfproxy" | "worker") => {
    if (!advanced) return;
    setTesting(kind);
    setTestOut((o) => ({ ...o, [kind]: "" }));
    try {
      const out = await api.tgTest(kind, kind === "worker" ? advanced.cfWorkerDomains : advanced.cfDomains);
      setTestOut((o) => ({ ...o, [kind]: out }));
    } catch (e) {
      setTestOut((o) => ({ ...o, [kind]: `Не прошло: ${e}` }));
    } finally {
      setTesting(null);
    }
  };

  return (
    <div className="page">
      <h1 className="page-title">Telegram</h1>

      <UpdateBanner core={core} />

      <ServiceCard
        icon={<SendRegular />}
        title="Telegram"
        subtitle={`${status.host ?? host}:${realPort}`}
        state={status.state}
        version={core?.version}
        error={status.error}
        hint={running ? "Прокси принимает подключения." : "Нажмите для запуска."}
        onToggle={toggle}
      />

      <div className="section-title">Подключение</div>
      <div className="card" style={{ padding: "14px 18px" }}>
        <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
          <span className={`mono ellipsis link-box${status.link ? "" : " off"}`}>
            {status.link ?? "Появится после запуска"}
          </span>
          <Tooltip content="Копировать ссылку" relationship="label">
            <Button
              shape="circular"
              icon={<CopyRegular />}
              disabled={!status.link}
              onClick={() => writeText(status.link!).then(() => notify.ok("Ссылка скопирована"))}
            />
          </Tooltip>
          <Tooltip content="Открыть в Telegram" relationship="label">
            <Button
              shape="circular"
              appearance="primary"
              icon={<OpenRegular />}
              disabled={!status.link}
              onClick={() => openUrl(status.link!).catch((e) => notify.error("Не удалось открыть Telegram", e))}
            />
          </Tooltip>
          <Tooltip content="Создать новый секретный ключ" relationship="label">
            <Button shape="circular" icon={<ArrowSyncRegular />} disabled={busy} onClick={regenerate} />
          </Tooltip>
        </div>
        <p className="hint" style={{ margin: "10px 0 0" }}>
          Нажмите <OpenRegular style={{ verticalAlign: "-2px" }} /> для подключения одним нажатием. Вручную:
          Telegram → Настройки → Продвинутые настройки → Тип соединения → Использовать прокси.
        </p>
      </div>

      <div className="section-title">Параметры</div>
      <div className="card" style={{ padding: "16px 18px" }}>
        <div className="params">
          <Field label="Хост">
            <Input value={host} onChange={(_, d) => setHost(d.value.trim())} className="mono" />
          </Field>
          <Field label="Порт">
            <Input
              value={port}
              onChange={(_, d) => setPort(d.value.replace(/\D/g, "").slice(0, 5))}
              className="mono"
            />
          </Field>
          <Field label="Секретный ключ (32 знака, MTProto)" style={{ gridColumn: "1 / -1" }}>
            <Input
              value={secret}
              onChange={(_, d) => setSecret(d.value.replace(/[^0-9a-fA-F]/g, "").slice(0, 32))}
              className="mono"
            />
          </Field>
        </div>
        <div style={{ display: "flex", gap: 10, alignItems: "center", marginTop: 14 }}>
          <Button appearance="primary" onClick={save} disabled={busy}>
            Сохранить
          </Button>
          <span className="hint">
            127.0.0.1 — только этот компьютер, 0.0.0.0 — ещё и устройства в вашей сети. Если порт занят, прокси
            возьмёт свободный.
          </span>
        </div>
      </div>

      <div className="section-title">Продвинутое</div>
      <div className="card" style={{ padding: "4px 6px" }}>
        <Button
          appearance="transparent"
          onClick={() => setOpenAdv(!openAdv)}
          icon={openAdv ? <ChevronDownRegular /> : <ChevronRightRegular />}
          style={{ width: "100%", justifyContent: "flex-start", textAlign: "left", padding: "10px 12px" }}
        >
          <span>
            <span style={{ display: "block", fontWeight: 600 }}>
              Cloudflare Proxy, Worker и адреса серверов
            </span>
            <span className="hint">
              Для тех, кто понимает, зачем это нужно. Неверные значения ломают подключение — если не уверены, не
              трогайте.
            </span>
          </span>
        </Button>

        {openAdv && advanced && (
          <div style={{ padding: "6px 14px 16px", display: "flex", flexDirection: "column", gap: 20 }}>
            <div>
              <div className="caps">Cloudflare Proxy</div>
              <Switch
                checked={advanced.cfProxy}
                onChange={(_, d) => patch({ cfProxy: d.checked })}
                label="Свои домены Cloudflare"
              />
              <div className="hint" style={{ margin: "2px 0 8px" }}>
                В Cloudflare добавьте записи kws1–kws5 на адреса серверов Telegram, включите «оранжевое облако» и
                режим SSL «Flexible». Домены — по одному в строке.
              </div>
              <Textarea
                value={advanced.cfDomains}
                onChange={(_, d) => patch({ cfDomains: d.value })}
                placeholder={"kws.example.com\nещё.example.com"}
                resize="vertical"
                textarea={{ className: "mono", style: { minHeight: 62 } }}
              />
              <div style={{ display: "flex", flexWrap: "wrap", gap: "4px 18px", marginTop: 10 }}>
                <Switch
                  checked={!advanced.noH2}
                  onChange={(_, d) => patch({ noH2: !d.checked })}
                  label="Медиа через HTTP/2"
                />
                <Switch
                  checked={advanced.cfBalance}
                  onChange={(_, d) => patch({ cfBalance: d.checked })}
                  label="Раскладывать по доменам"
                />
                <Switch
                  checked={advanced.defaultDomains}
                  onChange={(_, d) => patch({ defaultDomains: d.checked })}
                  label="Публичный список Flowseal"
                />
              </div>
              <div style={{ display: "flex", gap: 10, alignItems: "center", marginTop: 10 }}>
                <Button onClick={() => test("cfproxy")} disabled={testing !== null}>
                  Проверить
                </Button>
                {testing === "cfproxy" && <Spinner size="tiny" />}
                <span className="hint">Проверяются домены из поля выше — сохранять заранее не нужно.</span>
              </div>
              {testOut.cfproxy && <pre className="mono" style={TEST_OUT}>{testOut.cfproxy}</pre>}
            </div>

            <div>
              <div className="caps">Cloudflare Worker</div>
              <Switch
                checked={advanced.cfWorker}
                onChange={(_, d) => patch({ cfWorker: d.checked })}
                label="Свой Worker"
              />
              <div className="hint" style={{ margin: "2px 0 8px" }}>
                Туннель до серверов Telegram без своей зоны в Cloudflare: Worker открывает TCP по адресу из запроса.
              </div>
              <Textarea
                value={advanced.cfWorkerDomains}
                onChange={(_, d) => patch({ cfWorkerDomains: d.value })}
                placeholder="worker.example.workers.dev"
                resize="vertical"
                textarea={{ className: "mono", style: { minHeight: 44 } }}
              />
              <div style={{ display: "flex", gap: 10, alignItems: "center", marginTop: 10 }}>
                <Button onClick={() => test("worker")} disabled={testing !== null}>
                  Проверить
                </Button>
                {testing === "worker" && <Spinner size="tiny" />}
              </div>
              {testOut.worker && <pre className="mono" style={TEST_OUT}>{testOut.worker}</pre>}
            </div>

            <div>
              <div className="caps">Адреса серверов Telegram</div>
              <Textarea
                value={advanced.dcIps}
                onChange={(_, d) => patch({ dcIps: d.value })}
                placeholder={"2:149.154.167.220\n4:149.154.167.220"}
                resize="vertical"
                textarea={{ className: "mono", style: { minHeight: 62 } }}
              />
              <div className="hint" style={{ marginTop: 6 }}>
                По строке вида «номер:адрес». Пусто — ядро выберет адреса само; менять стоит, только если знаете
                рабочие.
              </div>
            </div>

            <div>
              <div className="caps">Прочее</div>
              <div style={{ display: "flex", flexWrap: "wrap", gap: "4px 18px" }}>
                <Switch
                  checked={advanced.verbose}
                  onChange={(_, d) => patch({ verbose: d.checked })}
                  label="Подробный журнал"
                />
                <Switch
                  checked={advanced.cfDisableTls}
                  onChange={(_, d) => patch({ cfDisableTls: d.checked })}
                  label="Без TLS для Cloudflare (ws://, порт 80)"
                />
              </div>
              <div className="params" style={{ marginTop: 12 }}>
                <Field label="Буфер сокета, КиБ" hint="0 — как в ядре">
                  <Input
                    value={String(advanced.bufKb)}
                    onChange={(_, d) => patch({ bufKb: num(d.value) })}
                    className="mono"
                  />
                </Field>
                <Field label="Прогретых соединений на сервер" hint="0 — как в ядре">
                  <Input
                    value={String(advanced.poolSize)}
                    onChange={(_, d) => patch({ poolSize: num(d.value) })}
                    className="mono"
                  />
                </Field>
                <Field label="Макс. размер журнала, МБ" hint="Сколько держать в памяти">
                  <Input
                    value={String(advanced.logMb)}
                    onChange={(_, d) => patch({ logMb: num(d.value) })}
                    className="mono"
                  />
                </Field>
              </div>
            </div>

            <div style={{ display: "flex", gap: 10, alignItems: "center" }}>
              <Button appearance="primary" onClick={saveAdvanced} disabled={busy}>
                Сохранить
              </Button>
              <span className="hint">
                {running
                  ? "Прокси перезапустится сразу — ссылка в Telegram останется прежней."
                  : "Настройки применятся при следующем запуске прокси."}
              </span>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

/** Поле с числом: пустая строка — это ноль, а не ошибка. */
function num(value: string): number {
  const n = Number(value.replace(/\D/g, ""));
  return Number.isFinite(n) ? n : 0;
}

/** Вывод проверки доменов: длинные строки переносим, окно не растягиваем. */
const TEST_OUT: CSSProperties = {
  margin: "8px 0 0",
  padding: "8px 10px",
  borderRadius: 6,
  background: "var(--subtle-fill)",
  fontSize: 12,
  lineHeight: 1.45,
  whiteSpace: "pre-wrap",
  wordBreak: "break-word",
  maxHeight: 180,
  overflow: "auto",
};
