import React from "react";
import ReactDOM from "react-dom/client";
import "../../shared/tokens.css";
import Bubble from "./Bubble";
import { initLang } from "../../shared/i18n";

initLang();

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <Bubble />
  </React.StrictMode>,
);
