/**
 * Update checking, all of it. One hook so the banner and the Settings section
 * can never disagree about what is going on.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export type UpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "available"; version: string; notes: string }
  | { kind: "downloading"; version: string; percent: number }
  | { kind: "ready" }
  | { kind: "error"; message: string };

/** Late enough not to compete with the wizard and the settings load. */
const FIRST_CHECK_DELAY_MS = 5_000;
const CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000;
/** Holds the version, not a boolean, so a newer release reappears. */
const DISMISSED_KEY = "dikto.update.dismissed";

function readDismissed(): string | null {
  try {
    return window.localStorage.getItem(DISMISSED_KEY);
  } catch {
    return null;
  }
}

export function useUpdateCheck() {
  const [state, setState] = useState<UpdateState>({ kind: "idle" });
  const [dismissed, setDismissed] = useState<string | null>(readDismissed);
  const updateRef = useRef<Update | null>(null);

  const run = useCallback(async (manual: boolean) => {
    setState({ kind: "checking" });
    try {
      const update = await check();
      updateRef.current = update;
      if (update) {
        setState({ kind: "available", version: update.version, notes: update.body ?? "" });
      } else {
        setState({ kind: "idle" });
      }
    } catch (e) {
      // Being offline is normal, so the automatic path stays quiet; only a
      // "Check now" the user asked for is allowed to report a failure.
      updateRef.current = null;
      if (manual) setState({ kind: "error", message: typeof e === "string" ? e : String(e) });
      else setState({ kind: "idle" });
    }
  }, []);

  useEffect(() => {
    const first = window.setTimeout(() => void run(false), FIRST_CHECK_DELAY_MS);
    const repeat = window.setInterval(() => void run(false), CHECK_INTERVAL_MS);
    return () => {
      window.clearTimeout(first);
      window.clearInterval(repeat);
    };
  }, [run]);

  const install = useCallback(async () => {
    const update = updateRef.current;
    if (!update) return;
    const version = update.version;
    setState({ kind: "downloading", version, percent: 0 });
    let total = 0;
    let downloaded = 0;
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") {
          total = event.data.contentLength ?? 0;
        } else if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          const percent = total > 0 ? Math.min(100, Math.round((downloaded / total) * 100)) : 0;
          setState({ kind: "downloading", version, percent });
        }
      });
      setState({ kind: "ready" });
      await relaunch();
    } catch (e) {
      setState({ kind: "error", message: typeof e === "string" ? e : String(e) });
    }
  }, []);

  const dismiss = useCallback(() => {
    if (state.kind !== "available") return;
    try {
      window.localStorage.setItem(DISMISSED_KEY, state.version);
    } catch {
      // A locked-down webview still gets the dismissal for this session.
    }
    setDismissed(state.version);
  }, [state]);

  // "checking" stays invisible — a banner that flashes on every poll is noise.
  const visible =
    state.kind === "available"
      ? state.version !== dismissed
      : state.kind === "downloading" || state.kind === "ready" || state.kind === "error";

  return { state, visible, check: () => run(true), install, dismiss };
}
