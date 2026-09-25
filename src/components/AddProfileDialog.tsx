import { useEffect, useRef, useState } from "react";
import {
  Button,
  Dialog,
  DialogActions,
  DialogBody,
  DialogContent,
  DialogSurface,
  DialogTitle,
  Field,
  Input,
  MessageBar,
  MessageBarBody,
  Spinner,
} from "@fluentui/react-components";
import { ClipboardPasteRegular, DocumentRegular } from "@fluentui/react-icons";
import { readText } from "@tauri-apps/plugin-clipboard-manager";
import { api, errorText, Profile } from "../api";

const LOOKS_LIKE_LINK = /^(https?|vless|vmess|trojan|ss|hysteria2|hy2):\/\//i;

export function AddProfileDialog(props: { open: boolean; onClose: () => void; onAdded: (p: Profile) => void }) {
  const [url, setUrl] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);

  // При открытии подставляем ссылку из буфера обмена, если она там есть.
  useEffect(() => {
    if (!props.open) return;
    setError(null);
    setUrl("");
    readText()
      .then((t) => {
        const s = t?.trim() ?? "";
        if (LOOKS_LIKE_LINK.test(s) && !s.includes("\n")) setUrl(s);
      })
      .catch(() => {});
  }, [props.open]);

  const run = async (f: () => Promise<Profile>) => {
    setBusy(true);
    setError(null);
    try {
      props.onAdded(await f());
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  const paste = async () => {
    const t = (await readText().catch(() => ""))?.trim() ?? "";
    if (!t) return setError("Буфер обмена пуст");
    if (t.includes("\n") || !LOOKS_LIKE_LINK.test(t)) {
      // Несколько ссылок или целый конфиг — добавляем как содержимое.
      return run(() => api.addProfileContent("Из буфера обмена", t));
    }
    setUrl(t);
  };

  const onFile = async (file: File) => {
    const text = await file.text();
    const name = file.name.replace(/\.(ya?ml|txt|conf)$/i, "");
    run(() => api.addProfileContent(name, text));
  };

  return (
    <Dialog open={props.open} onOpenChange={(_, d) => !d.open && !busy && props.onClose()}>
      <DialogSurface style={{ maxWidth: 520 }}>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            if (url.trim()) run(() => api.addProfileUrl(url));
          }}
        >
          <DialogBody>
            <DialogTitle>Добавить подписку</DialogTitle>
            <DialogContent style={{ display: "flex", flexDirection: "column", gap: 12 }}>
              <Field
                label="Ссылка на подписку или сервер"
                hint="Подходят Clash/Mihomo YAML, base64 и ссылки vless://, hysteria2://, trojan://, ss://, vmess://"
              >
                <Input
                  autoFocus
                  value={url}
                  onChange={(_, d) => setUrl(d.value)}
                  placeholder="https://…"
                  disabled={busy}
                  className="mono"
                />
              </Field>
              <div style={{ display: "flex", gap: 8 }}>
                <Button icon={<ClipboardPasteRegular />} onClick={paste} disabled={busy}>
                  Вставить из буфера
                </Button>
                <Button icon={<DocumentRegular />} onClick={() => fileRef.current?.click()} disabled={busy}>
                  Из файла…
                </Button>
                <input
                  ref={fileRef}
                  type="file"
                  accept=".yaml,.yml,.txt,.conf"
                  hidden
                  onChange={(e) => {
                    const f = e.target.files?.[0];
                    e.target.value = "";
                    if (f) onFile(f);
                  }}
                />
              </div>
              {error && (
                <MessageBar intent="error">
                  <MessageBarBody className="selectable">{error}</MessageBarBody>
                </MessageBar>
              )}
            </DialogContent>
            <DialogActions>
              <Button onClick={props.onClose} disabled={busy}>
                Отмена
              </Button>
              <Button type="submit" appearance="primary" disabled={busy || !url.trim()} icon={busy ? <Spinner size="tiny" /> : undefined}>
                Добавить
              </Button>
            </DialogActions>
          </DialogBody>
        </form>
      </DialogSurface>
    </Dialog>
  );
}
