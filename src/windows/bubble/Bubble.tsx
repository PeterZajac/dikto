import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
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

export default function Bubble() {
  const [phase, setPhase] = useState<Phase>("idle");
  const [message, setMessage] = useState<string | null>(null);
  const [partial, setPartial] = useState("");
  const [bars, setBars] = useState<number[]>(Array(BAR_COUNT).fill(0));
  const [seconds, setSeconds] = useState(0);
  const timerRef = useRef<number | null>(null);

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
        if (e.payload.phase === "recording") {
          setPartial("");
          setSeconds(0);
        }
      })
    );
    track(
      listen<AmplitudePayload>(EVENT_AMPLITUDE, (e) => {
        setBars((prev) => [...prev.slice(1), Math.min(1, e.payload.value * 6)]);
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

  const mmss = `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;

  return (
    <div className={`bubble bubble--${phase}`} data-tauri-drag-region>
      {phase === "recording" && (
        <>
          <Waveform bars={bars} />
          {partial ? (
            <span className="bubble__partial">{partial}</span>
          ) : (
            <span className="bubble__timer">● {mmss}</span>
          )}
        </>
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
        <span className="bubble__status bubble__status--error">⚠ {message}</span>
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
          style={{ height: `${Math.max(8, v * 100)}%` }}
        />
      ))}
    </div>
  );
}
