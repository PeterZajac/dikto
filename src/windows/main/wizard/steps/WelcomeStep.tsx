import { isMac } from "../../../../shared/platform";

export default function WelcomeStep() {
  return (
    <>
      <p className="wizard-step__eyebrow">Vitaj</p>
      <h1 className="wizard-step__title">Dikto</h1>
      <p className="wizard-step__desc">
        Podrž klávesovú skratku, povedz čo potrebuješ, a text sa objaví presne tam, kde práve píšeš —
        v mailoch, v editore, kdekoľvek má kurzor fokus.
      </p>
      <div className="wizard-keycap-scene">
        <div className="wizard-keycap" aria-hidden>
          <span className="wizard-keycap__glyph">{isMac ? "⌥" : "Ctrl"}</span>
          <span className="wizard-keycap__label">pravý</span>
        </div>
      </div>
    </>
  );
}
