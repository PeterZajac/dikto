import { useEffect, useState } from "react";
import { api } from "../../../../shared/ipc";
import { isMac } from "../../../../shared/platform";

const POLL_MS = 2000;

export default function PermissionsStep() {
  const [accessibility, setAccessibility] = useState<boolean | null>(null);

  useEffect(() => {
    let cancelled = false;
    const check = () => {
      api
        .permissionsStatus()
        .then((s) => {
          if (!cancelled) setAccessibility(s.accessibility);
        })
        .catch(() => {
          if (!cancelled) setAccessibility(null);
        });
    };
    check();
    const id = window.setInterval(check, POLL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, []);

  return (
    <>
      <p className="wizard-step__eyebrow">Povolenia</p>
      <h1 className="wizard-step__title">Over prístupové oprávnenia</h1>
      <p className="wizard-step__desc">
        {isMac
          ? "Appka potrebuje systémové povolenia, aby vedela vkladať nadiktovaný text a nahrávať mikrofón."
          : "Windows sa pri prvom nahrávaní opýta na prístup k mikrofónu — stačí ho povoliť."}
      </p>

      {isMac && (
        <div className="wizard-row">
          <div className="wizard-row__text">
            <span className="wizard-row__label">
              <StatusDot ok={accessibility} />
              Asistenčný prístup
            </span>
            <span className="wizard-row__hint">{accessibility ? "povolené" : "potrebné pre vkladanie textu"}</span>
          </div>
          <button
            type="button"
            className="wizard-btn"
            onClick={() => void api.openPrivacySettings("accessibility")}
          >
            Otvoriť nastavenia
          </button>
        </div>
      )}

      <div className="wizard-row">
        <div className="wizard-row__text">
          <span className="wizard-row__label">
            <span className="wizard-dot" aria-hidden />
            Mikrofón
          </span>
          <span className="wizard-row__hint">zistí sa pri prvom diktovaní</span>
        </div>
        <button type="button" className="wizard-btn" onClick={() => void api.openPrivacySettings("microphone")}>
          Otvoriť nastavenia
        </button>
      </div>

      {import.meta.env.DEV && (
        <p className="wizard-note">
          V dev režime (<code>pnpm tauri dev</code>) drží tieto povolenia terminál, nie táto appka —
          skontroluj ich pre Terminal/iTerm v Nastaveniach systému.
        </p>
      )}
    </>
  );
}

function StatusDot({ ok }: { ok: boolean | null }) {
  const cls = ok === true ? "wizard-dot wizard-dot--ok" : ok === false ? "wizard-dot wizard-dot--fail" : "wizard-dot";
  return <span className={cls} aria-hidden />;
}
