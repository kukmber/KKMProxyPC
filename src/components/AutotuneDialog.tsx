import { useEffect, useState } from "react";
import {
  Button,
  Dialog,
  DialogActions,
  DialogBody,
  DialogContent,
  DialogSurface,
  DialogTitle,
  MessageBar,
  MessageBarBody,
  ProgressBar,
  Spinner,
} from "@fluentui/react-components";
import { CheckmarkCircleFilled } from "@fluentui/react-icons";
import { listen } from "@tauri-apps/api/event";
import { api, AutotuneProgress, errorText, ProbeResult } from "../api";

/** Перебирает стратегии и показывает, сколько сайтов открылось на каждой. */
export function AutotuneDialog(props: {
  open: boolean;
  domains: string[];
  onClose: () => void;
  onApply: (id: string) => void;
  onResults?: (results: ProbeResult[]) => void;
}) {
  const [progress, setProgress] = useState<AutotuneProgress | null>(null);
  const [results, setResults] = useState<ProbeResult[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!props.open) return;
    setResults(null);
    setError(null);
    setProgress(null);
    const un = listen<AutotuneProgress>("zapret-autotune", (e) => setProgress(e.payload));
    api
      .zapretAutotune(props.domains)
      .then((r) => {
        setResults(r);
        props.onResults?.(r);
      })
      .catch((e) => setError(errorText(e)));
    return () => {
      un.then((f) => f());
    };
  }, [props.open]);

  const running = !results && !error;
  // «Без обхода» — точка отсчёта: если с ним столько же, обход не нужен.
  const baseline = results?.find((r) => r.id === "none");
  const best = results
    ?.filter((r) => r.id !== "none" && !r.error)
    .reduce<ProbeResult | null>((a, b) => (!a || b.ok > a.ok ? b : a), null);
  const worthIt = !!best && !!baseline && best.ok > baseline.ok;

  return (
    <Dialog open={props.open} onOpenChange={(_, d) => !d.open && props.onClose()}>
      <DialogSurface style={{ maxWidth: 560 }}>
        <DialogBody>
          <DialogTitle>Автоподбор стратегии</DialogTitle>
          {/* Высоту задаём здесь: стратегий больше двух десятков, и без
              прокрутки список уезжает за край окна вместе с кнопками. */}
          <DialogContent style={{ display: "flex", flexDirection: "column", gap: 12, minHeight: 120 }}>
            <span className="hint">
              Проверяю по очереди каждую стратегию{" "}
              {props.domains.length
                ? `на ${props.domains.length} ${props.domains.length === 1 ? "сайте" : "сайтах"} из вашего списка`
                : "на стандартном наборе сайтов"}
              . Обход на это время включается и выключается, связь может ненадолго прерваться.
            </span>

            {running && (
              <>
                <ProgressBar value={progress ? progress.step / progress.total : undefined} thickness="large" />
                <span style={{ display: "flex", alignItems: "center", gap: 8 }}>
                  <Spinner size="tiny" />
                  {progress ? `${progress.step} из ${progress.total}: ${progress.title}` : "Запуск…"}
                </span>
              </>
            )}

            {error && (
              <MessageBar intent="error">
                <MessageBarBody className="selectable">{error}</MessageBarBody>
              </MessageBar>
            )}

            {results && (
              <>
                {worthIt ? (
                  <MessageBar intent="success">
                    <MessageBarBody>
                      Лучше всех — «{best!.title}»: {best!.ok} из {best!.total}, без обхода открылось {baseline!.ok}.
                    </MessageBarBody>
                  </MessageBar>
                ) : (
                  <MessageBar intent="warning">
                    <MessageBarBody>
                      Ни одна стратегия не открыла больше, чем без обхода ({baseline?.ok ?? 0} из{" "}
                      {baseline?.total ?? 0}). Если сейчас включён VPN, выключите его и проверьте снова.
                    </MessageBarBody>
                  </MessageBar>
                )}

                <div className="card probe-list">
                  {results.map((r) => (
                    <div className="probe-row" key={r.id}>
                      <div className="text">
                        <span className="title">
                          {r.title}
                          {best && r.id === best.id && worthIt && (
                            <CheckmarkCircleFilled
                              style={{ color: "var(--ok)", marginLeft: 6, verticalAlign: "-2px" }}
                            />
                          )}
                        </span>
                        <span className="desc">
                          {r.error ? r.error : r.failed.length ? `не открылись: ${r.failed.join(", ")}` : "открылись все"}
                        </span>
                      </div>
                      <span
                        className="probe-score"
                        style={{ color: r.ok === r.total ? "var(--ok)" : r.ok ? "var(--mid)" : "var(--bad)" }}
                      >
                        {r.ok} / {r.total}
                      </span>
                      {r.id !== "none" && !r.error && (
                        <Button size="small" onClick={() => props.onApply(r.id)}>
                          Выбрать
                        </Button>
                      )}
                    </div>
                  ))}
                </div>
              </>
            )}
          </DialogContent>
          <DialogActions>
            {/* Кнопка остаётся доступной: без единого элемента, который может
                принять фокус, Fluent уводит его на страницу позади. */}
            <Button onClick={props.onClose}>{running ? "Свернуть" : "Закрыть"}</Button>
            {worthIt && (
              <Button appearance="primary" onClick={() => props.onApply(best!.id)}>
                Выбрать «{best!.title}»
              </Button>
            )}
          </DialogActions>
        </DialogBody>
      </DialogSurface>
    </Dialog>
  );
}
