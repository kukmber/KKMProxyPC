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
import { CheckmarkCircleFilled, DismissCircleRegular } from "@fluentui/react-icons";
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
    ?.filter((r) => r.id !== "none")
    .reduce<ProbeResult | null>((a, b) => (!a || b.ok > a.ok ? b : a), null);
  const worthIt = best && baseline && best.ok > baseline.ok;

  return (
    <Dialog open={props.open} onOpenChange={(_, d) => !d.open && !running && props.onClose()}>
      <DialogSurface style={{ maxWidth: 560 }}>
        <DialogBody>
          <DialogTitle>Автоподбор стратегии</DialogTitle>
          <DialogContent style={{ display: "flex", flexDirection: "column", gap: 12 }}>
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
              <div className="card" style={{ overflow: "hidden" }}>
                {results.map((r) => (
                  <div className="row" key={r.id} style={{ minHeight: 52, padding: "8px 14px" }}>
                    <div className="text">
                      <span className="title">
                        {r.title}
                        {best && r.id === best.id && worthIt && (
                          <CheckmarkCircleFilled style={{ color: "var(--ok)", marginLeft: 6, verticalAlign: "-2px" }} />
                        )}
                      </span>
                      <span className="desc">
                        {r.error
                          ? r.error
                          : r.failed.length
                            ? `не открылись: ${r.failed.join(", ")}`
                            : "открылись все"}
                      </span>
                    </div>
                    <span
                      style={{
                        fontVariantNumeric: "tabular-nums",
                        color: r.ok === r.total ? "var(--ok)" : r.ok ? "var(--mid)" : "var(--bad)",
                      }}
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
            )}

            {results && !worthIt && (
              <MessageBar intent="info" icon={<DismissCircleRegular />}>
                <MessageBarBody>
                  Без обхода открылось столько же. Значит, эти сайты ваш провайдер сейчас не блокирует — обход можно
                  не включать либо проверить на других адресах.
                </MessageBarBody>
              </MessageBar>
            )}
          </DialogContent>
          <DialogActions>
            {/* Кнопка остаётся доступной: без единого элемента, который может принять
                фокус, Fluent уводит его на страницу позади и та реагирует на нажатия. */}
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
