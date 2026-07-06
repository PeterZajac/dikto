import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { listen } from "@tauri-apps/api/event";
import Sidebar, { type FooterWarning, type NavPage } from "./Sidebar";
import SettingsPage from "./pages/Settings";
import HistoryPage from "./pages/History";
import { api } from "../../shared/ipc";
import { EVENT_PIPELINE_DEAD, type PipelineDeadPayload } from "../../shared/events";
import "./app.css";

export default function App() {
  const [page, setPage] = useState<NavPage>("settings");
  const [version, setVersion] = useState("");
  const [accessibilityOk, setAccessibilityOk] = useState(true);
  const [pipelineDeadMessage, setPipelineDeadMessage] = useState<string | null>(null);

  useEffect(() => {
    getVersion()
      .then(setVersion)
      .catch(() => setVersion(""));
  }, []);

  useEffect(() => {
    const checkAccessibility = () => {
      api
        .permissionsStatus()
        .then((status) => setAccessibilityOk(status.accessibility))
        .catch(() => setAccessibilityOk(true));
    };
    checkAccessibility();
    window.addEventListener("focus", checkAccessibility);
    return () => window.removeEventListener("focus", checkAccessibility);
  }, []);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    listen<PipelineDeadPayload>(EVENT_PIPELINE_DEAD, (event) => {
      setPipelineDeadMessage(event.payload.message);
    }).then((u) => {
      if (cancelled) u();
      else unlisten = u;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  const warnings: FooterWarning[] = [];
  if (!accessibilityOk) {
    warnings.push({
      id: "accessibility",
      label: "Prístupnosť",
      detail:
        "Aplikácia nemá povolenie Asistenčný prístup — vkladanie nadiktovaného textu preto nebude fungovať.",
      actionLabel: "Otvoriť nastavenia systému",
      onAction: () => void api.openPrivacySettings("accessibility"),
    });
  }
  if (pipelineDeadMessage) {
    warnings.push({
      id: "pipeline-dead",
      label: "Diktovanie nedostupné",
      detail: pipelineDeadMessage,
    });
  }

  return (
    <div className="shell">
      <Sidebar page={page} onNavigate={setPage} version={version} warnings={warnings} />
      <main className="content">{page === "settings" ? <SettingsPage /> : <HistoryPage />}</main>
    </div>
  );
}
