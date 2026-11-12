import React from "react";
import { flushSync } from "react-dom";
import ReactDOM from "react-dom/client";
import App from "./App";
import { markPlatform } from "./platform";
import { ready } from "./ready";
import { jumpWithTheWindow } from "./resizing";
import { smoothMouseWheel } from "./smoothWheel";
import { guardAgainstStaleHover } from "./staleHover";
import { installTooltips } from "./tooltip";
import Widget from "./Widget";
import { currentWindow, showWhenPainted } from "./window";
import { api } from "./api";
import "@fontsource-variable/inter";
import "./styles/tokens.css";
import "./styles/global.css";

const isWidget = currentWindow()?.label === "widget";
const Screen = isWidget ? Widget : App;
if (!isWidget) markPlatform();
guardAgainstStaleHover();
jumpWithTheWindow();
installTooltips();
smoothMouseWheel();

const loadedAt = performance.now();
const RELOAD_AFTER_MS = 10_000;

function failed(error: unknown, where?: string) {
  const message = error instanceof Error ? `${error.message}\n${error.stack ?? ""}` : String(error);
  void api
    .reportPageError(`${message}${where ? `\nin:${where}` : ""}`)
    .catch(() => {})
    .finally(() => {
      if (performance.now() - loadedAt > RELOAD_AFTER_MS) location.reload();
    });
}

ready().then(() => {
  flushSync(() =>
    ReactDOM.createRoot(document.getElementById("root")!, {
      onUncaughtError: (error, info) => failed(error, info.componentStack),
    }).render(
      <React.StrictMode>
        <Screen />
      </React.StrictMode>
    ),
  );
  if (!isWidget) showWhenPainted();
});
