import { useEffect, useState } from "react";
import { api } from "../../../../shared/ipc";
import { useT } from "../../../../shared/i18n";

type Status = "checking" | "online" | "offline";

/**
 * Optional cleanup step: Claude tidies the transcript through a local Meridian
 * proxy. Skippable — dictation works fine on the raw transcript.
 */
export default function CleanupStep({ onStatusChange }: { onStatusChange: (ready: boolean) => void }) {
  const [status, setStatus] = useState<Status>("checking");
  const t = useT();

  const check = () => {
    setStatus("checking");
    api
      .meridianStatus()
      .then((ok) => {
        setStatus(ok ? "online" : "offline");
        onStatusChange(ok);
      })
      .catch(() => {
        setStatus("offline");
        onStatusChange(false);
      });
  };

  useEffect(check, []);

  return (
    <>
      <p className="wizard-step__eyebrow">{t("wizard.cleanup.eyebrow")}</p>
      <h1 className="wizard-step__title">{t("wizard.cleanup.title")}</h1>
      <p className="wizard-step__desc">{t("wizard.cleanup.desc")}</p>

      <div className="wizard-row">
        <div className="wizard-row__text">
          <span className="wizard-row__label">
            <span
              className={`wizard-dot${status === "online" ? " wizard-dot--ok" : status === "offline" ? " wizard-dot--fail" : ""}`}
              aria-hidden
            />
            Meridian
          </span>
          <span className="wizard-row__hint">
            {status === "online" && t("wizard.cleanup.online")}
            {status === "offline" && t("wizard.cleanup.offline")}
            {status === "checking" && t("wizard.cleanup.checking")}
          </span>
        </div>
        <button type="button" className="wizard-btn" onClick={check} disabled={status === "checking"}>
          {t("wizard.cleanup.retry")}
        </button>
      </div>

      {status === "offline" && (
        <p className="wizard-note">{t("wizard.cleanup.note")}</p>
      )}
    </>
  );
}
