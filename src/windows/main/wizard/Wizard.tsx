import { useState } from "react";
import { api } from "../../../shared/ipc";
import WelcomeStep from "./steps/WelcomeStep";
import PermissionsStep from "./steps/PermissionsStep";
import GroqKeyStep from "./steps/GroqKeyStep";
import CleanupStep from "./steps/CleanupStep";
import TrialStep from "./steps/TrialStep";
import "./wizard.css";

const STEP_COUNT = 5;
const GROQ_STEP = 2;
const CLEANUP_STEP = 3;
const TRIAL_STEP = 4;

export default function Wizard({ onFinish }: { onFinish: () => void }) {
  const [step, setStep] = useState(0);
  const [finishing, setFinishing] = useState(false);
  const [hasGroqKey, setHasGroqKey] = useState(false);
  const [cleanupReady, setCleanupReady] = useState(true);
  const [trialSuccess, setTrialSuccess] = useState(false);

  const finish = () => {
    if (finishing) return;
    setFinishing(true);
    api.finishWizard().finally(onFinish);
  };

  const goNext = () => {
    if (step === STEP_COUNT - 1) finish();
    else setStep((s) => Math.min(s + 1, STEP_COUNT - 1));
  };
  const goBack = () => setStep((s) => Math.max(s - 1, 0));

  const primaryLabel = (): string => {
    if (step === TRIAL_STEP) return trialSuccess ? "Dokončiť" : "Preskočiť";
    if (step === GROQ_STEP) return hasGroqKey ? "Ďalej" : "Preskočiť";
    if (step === CLEANUP_STEP) return cleanupReady ? "Ďalej" : "Preskočiť";
    return "Ďalej";
  };

  return (
    <div className="wizard-overlay">
      <div className="wizard">
        <button type="button" className="wizard__skip-corner" onClick={finish} disabled={finishing}>
          Preskočiť sprievodcu
        </button>

        <div className="wizard__dots" role="tablist" aria-label="Priebeh sprievodcu">
          {Array.from({ length: STEP_COUNT }).map((_, i) => (
            <span key={i} className={`wizard__dot${i === step ? " is-active" : i < step ? " is-done" : ""}`} />
          ))}
        </div>

        <div className="wizard__body" key={step}>
          {step === 0 && <WelcomeStep />}
          {step === 1 && <PermissionsStep />}
          {step === GROQ_STEP && <GroqKeyStep onHasKeyChange={setHasGroqKey} />}
          {step === CLEANUP_STEP && <CleanupStep onStatusChange={setCleanupReady} />}
          {step === TRIAL_STEP && <TrialStep onSuccess={() => setTrialSuccess(true)} />}
        </div>

        <div className="wizard__footer">
          <div className="wizard__nav">
            {step > 0 ? (
              <button type="button" className="wizard-btn wizard-btn--ghost" onClick={goBack} disabled={finishing}>
                Späť
              </button>
            ) : (
              <span className="wizard__nav-spacer" />
            )}
            <button type="button" className="wizard-btn wizard-btn--primary" onClick={goNext} disabled={finishing}>
              {primaryLabel()}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
