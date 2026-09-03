import { useEffect, useRef, useState } from "react";
import { api } from "../../../../shared/ipc";
import { t, useT } from "../../../../shared/i18n";

const SAVED_FLASH_MS = 2500;

type TestState = "idle" | "testing" | "ok" | "fail";

export default function GroqKeyStep({ onHasKeyChange }: { onHasKeyChange: (v: boolean) => void }) {
  const [hasKey, setHasKey] = useState(false);
  const [draft, setDraft] = useState("");
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [test, setTest] = useState<TestState>("idle");
  const savedTimer = useRef<number | undefined>(undefined);
  useT();

  useEffect(() => {
    api
      .hasGroqKey()
      .then((v) => {
        setHasKey(v);
        onHasKeyChange(v);
      })
      .catch(() => {});
    return () => window.clearTimeout(savedTimer.current);
  }, []);

  const save = () => {
    const key = draft.trim();
    if (!key) return;
    setSaving(true);
    api
      .setGroqKey(key)
      .then(() => {
        setHasKey(true);
        onHasKeyChange(true);
        setDraft("");
        setTest("idle");
        setSaved(true);
        window.clearTimeout(savedTimer.current);
        savedTimer.current = window.setTimeout(() => setSaved(false), SAVED_FLASH_MS);
      })
      .finally(() => setSaving(false));
  };

  const runTest = () => {
    setTest("testing");
    api
      .testGroqKey()
      .then((ok) => setTest(ok ? "ok" : "fail"))
      .catch(() => setTest("fail"));
  };

  return (
    <>
      <p className="wizard-step__eyebrow">{t("wizard.groq.eyebrow")}</p>
      <h1 className="wizard-step__title">{t("wizard.groq.title")}</h1>
      <p className="wizard-step__desc">{t("wizard.groq.desc")}</p>

      <button
        type="button"
        className="wizard-btn wizard-step__link"
        onClick={() => void api.openUrl("https://console.groq.com")}
      >
        {t("wizard.groq.open")}
      </button>

      <div className="wizard-field-row">
        <input
          type="password"
          className="wizard-field"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          placeholder={hasKey ? "••••••••••••" : "gsk_…"}
          autoComplete="off"
          spellCheck={false}
        />
        <button type="button" className="wizard-btn" onClick={runTest} disabled={!hasKey || test === "testing"}>
          {t("wizard.groq.test")}
        </button>
        <button
          type="button"
          className="wizard-btn wizard-btn--primary"
          onClick={save}
          disabled={saving || !draft.trim()}
        >
          {t("wizard.groq.save")}
        </button>
      </div>

      <p
        className={`wizard-inline-status${
          test === "fail" ? " wizard-inline-status--fail" : test === "ok" || saved || hasKey ? " wizard-inline-status--ok" : ""
        }`}
      >
        {statusNote(saved, test, hasKey)}
      </p>
    </>
  );
}

function statusNote(saved: boolean, test: TestState, hasKey: boolean): string {
  if (saved) return t("wizard.groq.saved");
  if (test === "testing") return t("wizard.groq.testing");
  if (test === "ok") return t("wizard.groq.testOk");
  if (test === "fail") return t("wizard.groq.testFail");
  return hasKey ? t("wizard.groq.saved") : "";
}
