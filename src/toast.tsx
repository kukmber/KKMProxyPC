import { createContext, useCallback, useContext } from "react";
import { Toast, ToastBody, ToastTitle, useToastController } from "@fluentui/react-components";
import { errorText } from "./api";

export const ToasterIdContext = createContext("toaster");

export function useNotify() {
  const { dispatchToast } = useToastController(useContext(ToasterIdContext));
  const show = useCallback(
    (intent: "success" | "error" | "info", title: string, body?: string) =>
      dispatchToast(
        <Toast>
          <ToastTitle>{title}</ToastTitle>
          {body && <ToastBody className="selectable">{body}</ToastBody>}
        </Toast>,
        { intent, timeout: intent === "error" ? 8000 : 3000 },
      ),
    [dispatchToast],
  );
  return {
    ok: (title: string, body?: string) => show("success", title, body),
    info: (title: string, body?: string) => show("info", title, body),
    error: (title: string, e?: unknown) => show("error", title, e === undefined ? undefined : errorText(e)),
  };
}
