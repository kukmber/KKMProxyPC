import { createContext, ReactNode, useContext, useEffect, useState } from "react";
import { Button, ProgressBar, Spinner } from "@fluentui/react-components";
import { ArrowDownloadRegular, SparkleRegular } from "@fluentui/react-icons";
import { check, Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { errorText } from "../api";
import { useNotify } from "../toast";

/** Раз в шесть часов: чтобы вышедшая версия не ждала перезапуска программы. */
const RECHECK_MS = 6 * 60 * 60 * 1000;

interface UpdateState {
  update: Update | null;
  checking: boolean;
  error: string | null;
  check: (quiet: boolean) => Promise<Update | null>;
}

const Ctx = createContext<UpdateState>({
  update: null,
  checking: false,
  error: null,
  check: async () => null,
});

/** Одна проверка обновления на всю программу: её видят и Пульт, и Настройки. */
export function AppUpdateProvider({ children }: { children: ReactNode }) {
  const [update, setUpdate] = useState<Update | null>(null);
  const [checking, setChecking] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const run = async (quiet: boolean) => {
    setChecking(true);
    setError(null);
    try {
      const u = await check();
      const found = u?.available ? u : null;
      setUpdate(found);
      return found;
    } catch (e) {
      // При тихой проверке молчим: сеть может быть недоступна, это не повод шуметь.
      if (!quiet) setError(errorText(e));
      return null;
    } finally {
      setChecking(false);
    }
  };

  useEffect(() => {
    run(true);
    const id = setInterval(() => run(true), RECHECK_MS);
    return () => clearInterval(id);
  }, []);

  return <Ctx.Provider value={{ update, checking, error, check: run }}>{children}</Ctx.Provider>;
}

export function useAppUpdate() {
  return useContext(Ctx);
}

/** Полоса «вышла новая версия» с загрузкой и перезапуском. */
export function AppUpdateBanner() {
  const notify = useNotify();
  const { update } = useAppUpdate();
  const [progress, setProgress] = useState<number | null>(null);
  const [done, setDone] = useState(false);
  if (!update) return null;

  const install = async () => {
    setProgress(0);
    let total = 0;
    let got = 0;
    try {
      await update.downloadAndInstall((e) => {
        if (e.event === "Started") total = e.data.contentLength ?? 0;
        else if (e.event === "Progress") {
          got += e.data.chunkLength;
          setProgress(total ? got / total : null);
        } else if (e.event === "Finished") setDone(true);
      });
      setDone(true);
    } catch (e) {
      setProgress(null);
      notify.error("Не удалось обновить программу", e);
    }
  };

  return (
    <div className="banner">
      <div className="banner-icon">
        <SparkleRegular />
      </div>
      <div className="text">
        <span className="title">Вышла новая версия KKMProxy — {update.version}</span>
        <span className="desc">
          {done
            ? "Установлено. Перезапустите, чтобы начать пользоваться"
            : progress !== null
              ? "Загрузка…"
              : `Сейчас установлена ${update.currentVersion}`}
        </span>
        {progress !== null && !done && <ProgressBar value={progress} thickness="medium" style={{ marginTop: 6 }} />}
      </div>
      {done ? (
        <Button appearance="primary" onClick={() => relaunch()}>
          Перезапустить
        </Button>
      ) : (
        <Button
          appearance="primary"
          icon={progress !== null ? <Spinner size="tiny" /> : <ArrowDownloadRegular />}
          onClick={install}
          disabled={progress !== null}
        >
          Обновить
        </Button>
      )}
    </div>
  );
}
