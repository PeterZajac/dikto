import { useEffect, useRef, useState, type CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
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
import "./bubble.css";

const BAR_COUNT = 24;

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
  const timerRef = useRef<number | null>(null);

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

  // Idle mini-dot must not swallow clicks meant for whatever is behind the
  // bubble window; any active state needs clicks again (cancel/retry).
  const isIdleDot = phase === "idle" && !message;
  useEffect(() => {
    void getCurrentWindow().setIgnoreCursorEvents(isIdleDot);
  }, [isIdleDot]);

  const mmss = `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;

  if (isIdleDot) {
    return (
      <div className="bubble-idle" aria-label="Dikto je aktívne">
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
        <button className="bubble__hit" onClick={cancel} title="Zrušiť (Esc)">
          <span className="bubble__rec-dot" aria-hidden />
          <Waveform bars={bars} />
          {partial ? (
            <span className="bubble__partial">{partial}</span>
          ) : (
            <span className="bubble__timer">{mmss}</span>
          )}
        </button>
      )}
      {phase === "transcribing" && <span className="bubble__status">prepisujem…</span>}
      {phase === "cleaning" && (
        <span className="bubble__status">{message ?? "✨ upravujem text…"}</span>
      )}
      {phase === "injecting" && <span className="bubble__status">vkladám…</span>}
      {phase === "idle" && message && (
        <span className="bubble__status bubble__status--done">{message}</span>
      )}
      {phase === "error" && (
        <>
          <span className="bubble__status bubble__status--error">⚠ {message}</span>
          {message?.startsWith("prepis zlyhal") && (
            <button className="bubble__retry" onClick={handleRetry} disabled={retrying}>
              skúsiť znova
            </button>
          )}
          <button className="bubble__retry" onClick={cancel}>
            ✕
          </button>
        </>
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
