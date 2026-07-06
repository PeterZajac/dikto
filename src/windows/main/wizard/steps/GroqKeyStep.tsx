import { useEffect, useRef, useState } from "react";
import { api } from "../../../../shared/ipc";

const SAVED_FLASH_MS = 2500;

type TestState = "idle" | "testing" | "ok" | "fail";

export default function GroqKeyStep({ onHasKeyChange }: { onHasKeyChange: (v: boolean) => void }) {
  const [hasKey, setHasKey] = useState(false);
  const [draft, setDraft] = useState("");
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [test, setTest] = useState<TestState>("idle");
  const savedTimer = useRef<number | undefined>(undefined);

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
      <p className="wizard-step__eyebrow">Groq kľúč</p>
      <h1 className="wizard-step__title">Priprav prepis reči</h1>
      <p className="wizard-step__desc">
        Prepis hlasu beží cez Groq Whisper — bezplatný tier stačí na bežné diktovanie. Vytvor si účet
        a vlož si vygenerovaný API kľúč nižšie.
      </p>

      <button
        type="button"
        className="wizard-btn wizard-step__link"
        onClick={() => void api.openUrl("https://console.groq.com")}
      >
        Otvoriť console.groq.com ↗
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
          Otestovať
        </button>
        <button
          type="button"
          className="wizard-btn wizard-btn--primary"
          onClick={save}
          disabled={saving || !draft.trim()}
        >
          Uložiť
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
  if (saved) return "✓ kľúč uložený";
  if (test === "testing") return "testujem spojenie…";
  if (test === "ok") return "✓ spojenie funguje";
  if (test === "fail") return "✗ spojenie zlyhalo";
  return hasKey ? "✓ kľúč uložený" : "";
}
