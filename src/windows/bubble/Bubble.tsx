import { useEffect, useRef, useState, type CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { LogicalSize } from "@tauri-apps/api/dpi";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  AmplitudePayload,
  EVENT_AMPLITUDE,
  EVENT_PARTIAL,
  EVENT_STATE,
  PartialPayload,
  Phase,
  StatePayload,
} from "../../shared/events";
import { useT } from "../../shared/i18n";
import "./bubble.css";

const BAR_COUNT = 24;

/** The bubble's normal footprint, matching tauri.conf.json. */
const SIZE_DEFAULT = { width: 340, height: 64 };
/** Errors are full API messages; the window has to grow or they get cut off. */
const SIZE_ERROR = { width: 460, height: 190 };
/** An error pill dismisses itself after this long; the take stays in History. */
const ERROR_AUTO_HIDE_MS = 8_000;

const cancel = () => void invoke("cancel_dictation");
const retry = () => void invoke("retry_transcription");

export default function Bubble() {
  const [phase, setPhase] = useState<Phase>("idle");
  const [message, setMessage] = useState<string | null>(null);
  const [partial, setPartial] = useState("");
  const [bars, setBars] = useState<number[]>(Array(BAR_COUNT).fill(0));
  const [amp, setAmp] = useState(0);
  const [seconds, setSeconds] = useState(0);
  const [retrying, setRetrying] = useState(false);
  // Set by the backend when the audio is still on disk and re-running STT could fix it.
  const [retryable, setRetryable] = useState(false);
  const timerRef = useRef<number | null>(null);
  const t = useT();

  const handleRetry = () => {
    setRetrying(true);
    retry();
  };

  useEffect(() => {
    let cancelled = false;
    const unsubs: Array<() => void> = [];
    const track = (p: Promise<() => void>) =>
      p.then((u) => {
        if (cancelled) u();
        else unsubs.push(u);
      });
    track(
      listen<StatePayload>(EVENT_STATE, (e) => {
        setPhase(e.payload.phase);
        setMessage(e.payload.message);
        setRetryable(e.payload.retryable === true);
        setRetrying(false);
        if (e.payload.phase === "recording") {
          setPartial("");
          setSeconds(0);
          setBars(Array(BAR_COUNT).fill(0));
          setAmp(0);
        }
      })
    );
    track(
      listen<AmplitudePayload>(EVENT_AMPLITUDE, (e) => {
        const v = Math.min(1, e.payload.value * 6);
        setAmp(v);
        setBars((prev) => [...prev.slice(1), v]);
      })
    );
    track(
      listen<PartialPayload>(EVENT_PARTIAL, (e) => {
        setPartial(e.payload.text);
      })
    );
    return () => {
      cancelled = true;
      unsubs.forEach((u) => u());
    };
  }, []);

  useEffect(() => {
    if (phase === "recording") {
      timerRef.current = window.setInterval(() => setSeconds((s) => s + 1), 1000);
    } else if (timerRef.current !== null) {
      window.clearInterval(timerRef.current);
      timerRef.current = null;
    }
    return () => {
      if (timerRef.current !== null) window.clearInterval(timerRef.current);
    };
  }, [phase]);

  // Result/error messages shown in the idle pill fade back to the mini-dot
  // on their own — nothing else clears them once the pipeline is idle.
  useEffect(() => {
    if (phase !== "idle" || !message) return;
    const t = window.setTimeout(() => setMessage(null), 2200);
    return () => window.clearTimeout(t);
  }, [phase, message]);

  // Errors linger long enough to read and hit retry, then get out of the way
  // exactly like ✕ would (the pipeline returns to idle, History keeps the take).
  useEffect(() => {
    if (phase !== "error") return;
    const t = window.setTimeout(cancel, ERROR_AUTO_HIDE_MS);
    return () => window.clearTimeout(t);
  }, [phase, message]);

  // Idle mini-dot must not swallow clicks meant for whatever is behind the
  // bubble window; any active state needs clicks again (cancel/retry).
  const isIdleDot = phase === "idle" && !message;
  useEffect(() => {
    void getCurrentWindow().setIgnoreCursorEvents(isIdleDot);
  }, [isIdleDot]);

  // A long error would be clipped by the window frame no matter how the CSS
  // wraps it, so grow the window itself while one is showing. This needs
  // `resizable: true` in tauri.conf.json — tao pins min=max size otherwise and
  // setSize silently does nothing. The macOS panel style mask carries no
  // resizable bit, so the user still can't drag the bubble bigger.
  useEffect(() => {
    const { width, height } = phase === "error" ? SIZE_ERROR : SIZE_DEFAULT;
    void getCurrentWindow().setSize(new LogicalSize(width, height));
  }, [phase]);

  const mmss = `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;

  if (isIdleDot) {
    return (
      <div className="bubble-idle" aria-label={t("bubble.idleAria")}>
        <span className="bubble-idle__dot" />
      </div>
    );
  }

  return (
    <div
      className={`bubble bubble--${phase}`}
      data-tauri-drag-region
      style={{ "--amp": amp } as CSSProperties}
    >
      {phase === "recording" && (
        <button className="bubble__hit" onClick={cancel} title={t("bubble.cancelTitle")}>
          <span className="bubble__rec-dot" aria-hidden />
          <Waveform bars={bars} />
          {partial ? (
            <span className="bubble__partial">{partial}</span>
          ) : (
            <span className="bubble__timer">{mmss}</span>
          )}
        </button>
      )}
      {/* The message carries rate-limit retry progress; fall back when idle. */}
      {phase === "transcribing" && (
        <span className="bubble__status">{message ?? t("bubble.transcribing")}</span>
      )}
      {phase === "cleaning" && (
        <span className="bubble__status">{message ?? t("bubble.cleaning")}</span>
      )}
      {phase === "injecting" && <span className="bubble__status">{t("bubble.injecting")}</span>}
      {phase === "idle" && message && (
        <span className="bubble__status bubble__status--done">{message}</span>
      )}
      {phase === "error" && (
        <div className="bubble__error">
          <span className="bubble__status bubble__status--error">⚠ {message}</span>
          <div className="bubble__error-actions">
            {retryable && (
              <button className="bubble__retry" onClick={handleRetry} disabled={retrying}>
                {t("bubble.retry")}
              </button>
            )}
            <span className="bubble__error-hint">{t("bubble.savedHint")}</span>
            <button className="bubble__retry" onClick={cancel}>
              ✕
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

function Waveform({ bars }: { bars: number[] }) {
  return (
    <div className="waveform" aria-hidden>
      {bars.map((v, i) => (
        <div
          key={i}
          className="waveform__bar"
          style={{ height: `${Math.max(8, v * 100)}%`, "--i": i } as CSSProperties}
        />
      ))}
    </div>
  );
}
