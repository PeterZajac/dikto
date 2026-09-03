import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { listen } from "@tauri-apps/api/event";
import Sidebar, { type FooterWarning, type NavPage } from "./Sidebar";
import SettingsPage from "./pages/Settings";
import HistoryPage from "./pages/History";
import Wizard from "./wizard/Wizard";
import { api } from "../../shared/ipc";
import type { Settings } from "../../shared/ipc";
import { EVENT_PIPELINE_DEAD, EVENT_SETTINGS_CHANGED, type PipelineDeadPayload } from "../../shared/events";
import { useT } from "../../shared/i18n";
import "./app.css";

export default function App() {
  const [page, setPage] = useState<NavPage>("settings");
  const [version, setVersion] = useState("");
  const [accessibilityOk, setAccessibilityOk] = useState(true);
  const [pipelineDeadMessage, setPipelineDeadMessage] = useState<string | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const t = useT();

  useEffect(() => {
    let cancelled = false;
    api
      .getSettings()
      .then((s) => {
        if (!cancelled) setSettings(s);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);

  // Cross-window sync (tray, other windows) — also covers the wizard's
  // finish_wizard, which sets local state directly instead (see below);
  // this keeps App in sync with settings changes made elsewhere.
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    listen<Settings>(EVENT_SETTINGS_CHANGED, (event) => setSettings(event.payload)).then((u) => {
      if (cancelled) u();
      else unlisten = u;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

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
      label: t("warn.accessibility.label"),
      detail: t("warn.accessibility.detail"),
      actionLabel: t("warn.accessibility.action"),
      onAction: () => void api.openPrivacySettings("accessibility"),
    });
  }
  if (pipelineDeadMessage) {
    warnings.push({
      id: "pipeline-dead",
      label: t("warn.pipelineDead.label"),
      detail: pipelineDeadMessage,
    });
  }

  return (
    <>
      <div className="shell">
        <Sidebar page={page} onNavigate={setPage} version={version} warnings={warnings} />
        <main className="content">{page === "settings" ? <SettingsPage /> : <HistoryPage />}</main>
      </div>
      {settings && !settings.wizard_done && (
        // finish_wizard doesn't emit settings:changed, so flip the flag locally
        // instead of waiting on a refetch that will never come.
        <Wizard onFinish={() => setSettings((prev) => (prev ? { ...prev, wizard_done: true } : prev))} />
      )}
    </>
  );
}
