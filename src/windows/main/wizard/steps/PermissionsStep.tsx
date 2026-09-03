import { useEffect, useState } from "react";
import { api } from "../../../../shared/ipc";
import { isMac } from "../../../../shared/platform";
import { useT } from "../../../../shared/i18n";

const POLL_MS = 2000;

export default function PermissionsStep() {
  const [accessibility, setAccessibility] = useState<boolean | null>(null);
  const t = useT();

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
      <p className="wizard-step__eyebrow">{t("wizard.permissions.eyebrow")}</p>
      <h1 className="wizard-step__title">{t("wizard.permissions.title")}</h1>
      <p className="wizard-step__desc">
        {isMac ? t("wizard.permissions.descMac") : t("wizard.permissions.descWin")}
      </p>

      {isMac && (
        <div className="wizard-row">
          <div className="wizard-row__text">
            <span className="wizard-row__label">
              <StatusDot ok={accessibility} />
              {t("wizard.permissions.accessibility")}
            </span>
            <span className="wizard-row__hint">
              {accessibility ? t("wizard.permissions.granted") : t("wizard.permissions.neededForPaste")}
            </span>
          </div>
          <button
            type="button"
            className="wizard-btn"
            onClick={() => void api.openPrivacySettings("accessibility")}
          >
            {t("wizard.permissions.open")}
          </button>
        </div>
      )}

      <div className="wizard-row">
        <div className="wizard-row__text">
          <span className="wizard-row__label">
            <span className="wizard-dot" aria-hidden />
            {t("wizard.permissions.microphone")}
          </span>
          <span className="wizard-row__hint">{t("wizard.permissions.micHint")}</span>
        </div>
        <button type="button" className="wizard-btn" onClick={() => void api.openPrivacySettings("microphone")}>
          {t("wizard.permissions.open")}
        </button>
      </div>

      {import.meta.env.DEV && <p className="wizard-note">{t("wizard.permissions.devNote")}</p>}
    </>
  );
}

function StatusDot({ ok }: { ok: boolean | null }) {
  const cls = ok === true ? "wizard-dot wizard-dot--ok" : ok === false ? "wizard-dot wizard-dot--fail" : "wizard-dot";
  return <span className={cls} aria-hidden />;
}
