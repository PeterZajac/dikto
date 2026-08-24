import { useEffect, useState } from "react";
import { api } from "../../../../shared/ipc";

type Status = "checking" | "online" | "offline";

/**
 * Optional cleanup step: Claude tidies the transcript through a local Meridian
 * proxy. Skippable — dictation works fine on the raw transcript.
 */
export default function CleanupStep({ onStatusChange }: { onStatusChange: (ready: boolean) => void }) {
  const [status, setStatus] = useState<Status>("checking");

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
      <p className="wizard-step__eyebrow">Čistenie textu (voliteľné)</p>
      <h1 className="wizard-step__title">Doladenie textu</h1>
      <p className="wizard-step__desc">
        Meridian pred vložením opraví interpunkciu a plynulosť prepisu pomocou Claude. Je to
        voliteľné — bez neho sa vloží surový prepis z Whisperu.
      </p>

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
            {status === "online" && "beží a je pripravený"}
            {status === "offline" && "nie je dostupný"}
            {status === "checking" && "zisťujem stav…"}
          </span>
        </div>
        <button type="button" className="wizard-btn" onClick={check} disabled={status === "checking"}>
          Skúsiť znova
        </button>
      </div>

      {status === "offline" && (
        <p className="wizard-note">
          Spusti Meridian v termináli príkazom <code>meridian</code> a klikni na „Skúsiť znova".
          Alebo jednoducho pokračuj ďalej — diktovanie bude fungovať aj bez neho.
        </p>
      )}
    </>
  );
}
