import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { ready } from "./ready";
import "@fontsource-variable/inter";
import "./styles/tokens.css";
import "./styles/global.css";

ready().then(() => {
  ReactDOM.createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>
  );
});
