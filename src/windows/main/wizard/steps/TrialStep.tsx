import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { EVENT_STATE, type Phase, type StatePayload } from "../../../../shared/events";
import { useT } from "../../../../shared/i18n";

export default function TrialStep({ onSuccess }: { onSuccess: () => void }) {
  const [success, setSuccess] = useState(false);
  const lastPhase = useRef<Phase>("idle");
  const t = useT();

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    listen<StatePayload>(EVENT_STATE, (event) => {
      const { phase } = event.payload;
      // Injecting → idle is the only transition that means text landed.
      if (phase === "idle" && lastPhase.current === "injecting") {
        setSuccess(true);
        onSuccess();
      }
      lastPhase.current = phase;
    }).then((u) => {
      if (cancelled) u();
      else unlisten = u;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [onSuccess]);

  return (
    <>
      <p className="wizard-step__eyebrow">{t("wizard.trial.eyebrow")}</p>
      <h1 className="wizard-step__title">{t("wizard.trial.title")}</h1>
      <p className="wizard-step__desc">{t("wizard.trial.desc")}</p>

      <textarea
        className={`wizard-textarea${success ? " wizard-textarea--success" : ""}`}
        defaultValue=""
        placeholder={t("wizard.trial.placeholder")}
        spellCheck={false}
      />

      {success && <p className="wizard-success-banner">{t("wizard.trial.success")}</p>}
    </>
  );
}
