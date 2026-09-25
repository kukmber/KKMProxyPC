import React, { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import { FluentProvider } from "@fluentui/react-components";
import App from "./App";
import { api } from "./api";
import { useSystemDark } from "./hooks";
import { darkTheme, lightTheme } from "./theme";
import "./styles.css";

function Root() {
  const dark = useSystemDark();
  const [mica, setMica] = useState(true);

  useEffect(() => {
    api.platformInfo().then((p) => setMica(p.mica)).catch(() => setMica(false));
  }, []);

  useEffect(() => {
    document.documentElement.dataset.theme = dark ? "dark" : "light";
    document.documentElement.dataset.mica = String(mica);
  }, [dark, mica]);

  return (
    <FluentProvider theme={dark ? darkTheme : lightTheme} style={{ background: "transparent", height: "100%" }}>
      <App />
    </FluentProvider>
  );
}

// Контекстное меню браузера в настольном приложении ни к чему.
window.addEventListener("contextmenu", (e) => {
  const t = e.target as HTMLElement;
  if (!t.closest("input, textarea, .selectable")) e.preventDefault();
});

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Root />
  </React.StrictMode>,
);
