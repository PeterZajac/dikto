import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { EVENT_STATE, type StatePayload } from "../../../../shared/events";

export default function TrialStep({ onSuccess }: { onSuccess: () => void }) {
  const [success, setSuccess] = useState(false);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    listen<StatePayload>(EVENT_STATE, (event) => {
      const { phase, message } = event.payload;
      if (phase === "idle" && message?.includes("vložené")) {
        setSuccess(true);
        onSuccess();
      }
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
      <p className="wizard-step__eyebrow">Skúšobné diktovanie</p>
      <h1 className="wizard-step__title">Vyskúšaj to naživo</h1>
      <p className="wizard-step__desc">
        Klikni do poľa nižšie, podrž klávesovú skratku a povedz pár slov — text sa objaví priamo tu.
      </p>

      <textarea
        className={`wizard-textarea${success ? " wizard-textarea--success" : ""}`}
        defaultValue=""
        placeholder="klikni sem, podrž klávesu a hovor…"
        spellCheck={false}
      />

      {success && <p className="wizard-success-banner">✓ Super, funguje to!</p>}
    </>
  );
}
