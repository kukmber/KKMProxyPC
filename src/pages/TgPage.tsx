import { useEffect, useState } from "react";
import { Button, Field, Input, Tooltip } from "@fluentui/react-components";
import {
  ArrowSyncRegular,
  CopyRegular,
  OpenRegular,
  SendRegular,
} from "@fluentui/react-icons";
import { openUrl } from "@tauri-apps/plugin-opener";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { api } from "../api";
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
  const core = cores.find((c) => c.id === "tgws");

  const load = () =>
    api.getSettings().then((s) => {
      setHost(s.tgHost);
      setPort(String(s.tgPort));
      // Ключ показываем и до первого запуска — он уже сохранён в настройках.
      if (s.tgSecret) setSecret(s.tgSecret);
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
    </div>
  );
}
