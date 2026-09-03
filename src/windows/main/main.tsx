import React from "react";
import ReactDOM from "react-dom/client";
import "../../shared/tokens.css";
import App from "./App";
import { initLang } from "../../shared/i18n";

initLang();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
