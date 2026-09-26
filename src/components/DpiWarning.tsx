import { MessageBar, MessageBarBody, MessageBarTitle } from "@fluentui/react-components";
import { useDpiConflict } from "../hooks";

/**
 * Предупреждение о втором обходе DPI. Две такие программы перехватывают одни
 * и те же пакеты через WinDivert, и сеть пропадает целиком — поэтому говорим
 * об этом до того, как человек попробует включить наш обход.
 */
export function DpiWarning() {
  const conflict = useDpiConflict();
  if (!conflict) return null;
  return (
    <MessageBar intent="warning" style={{ marginBottom: 10 }}>
      <MessageBarBody>
        <MessageBarTitle>Уже работает другой обход блокировок</MessageBarTitle>
        <div className="selectable" style={{ overflowWrap: "anywhere" }}>
          {conflict.path} (PID {conflict.pid})
        </div>
        Два обхода одновременно перехватывают одни и те же пакеты, и интернет пропадает целиком. Выключите обход в той
        программе — наш не запустится, пока она работает.
      </MessageBarBody>
    </MessageBar>
  );
}
