import React from "react";
import { flushSync } from "react-dom";
import ReactDOM from "react-dom/client";
import App from "./App";
import { ready } from "./ready";
import Widget from "./Widget";
import { currentWindow, showWhenPainted } from "./window";
import "@fontsource-variable/inter";
import "./styles/tokens.css";
import "./styles/global.css";

const isWidget = currentWindow()?.label === "widget";
const Screen = isWidget ? Widget : App;

ready().then(() => {
  flushSync(() =>
    ReactDOM.createRoot(document.getElementById("root")!).render(
      <React.StrictMode>
        <Screen />
      </React.StrictMode>
    ),
  );
  if (!isWidget) showWhenPainted();
});
