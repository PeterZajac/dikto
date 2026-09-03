import { isMac } from "../../../../shared/platform";
import { useT } from "../../../../shared/i18n";

export default function WelcomeStep() {
  const t = useT();
  return (
    <>
      <p className="wizard-step__eyebrow">{t("wizard.welcome.eyebrow")}</p>
      <h1 className="wizard-step__title">Dikto</h1>
      <p className="wizard-step__desc">{t("wizard.welcome.desc")}</p>
      <div className="wizard-keycap-scene">
        <div className="wizard-keycap" aria-hidden>
          <span className="wizard-keycap__glyph">{isMac ? "⌥" : "Ctrl"}</span>
          <span className="wizard-keycap__label">{t("wizard.welcome.keycapLabel")}</span>
        </div>
      </div>
    </>
  );
}
