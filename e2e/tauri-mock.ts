/**
 * In-page stand-in for the Tauri runtime. Installed with `page.addInitScript`
 * before the app loads, it answers every `invoke` the frontend makes from an
 * in-memory state and lets tests push backend events with `window.__mock`.
 * Mirrors `@tauri-apps/api/mocks` (callback registry + event plugin) so the
 * real `listen`/`invoke` code paths run unchanged.
 */
import type { Dictation, Settings } from "../src/shared/ipc";

export interface MockSeed {
  settings?: Partial<Settings>;
  history?: Dictation[];
  hasGroqKey?: boolean;
  groqTestOk?: boolean;
  meridianOnline?: boolean;
  meridianModels?: string[];
  cleanupTestOk?: boolean;
  accessibility?: boolean;
  /** Commands whose promise should reject, with the rejection message. */
  failing?: Record<string, string>;
  windowLabel?: "main" | "bubble";
}

export interface MockCall {
  cmd: string;
  args: Record<string, unknown>;
}

export const DEFAULT_SETTINGS: Settings = {
  hotkey: "AltGr",
  language: "auto",
  cleanup_enabled: false,
  cleanup_model: "claude-sonnet-5",
  meridian_url: "http://127.0.0.1:3456",
  groq_url: "https://api.groq.com",
  cleanup_style: "light",
  wizard_done: true,
  bubble_pos: null,
  autostart: false,
  groq_api_key: "",
  history_retention_days: 7,
  ui_language: "en",
};

export function dictation(overrides: Partial<Dictation> & { id: number }): Dictation {
  return {
    ts: Date.now() - overrides.id * 60_000,
    raw: `raw text ${overrides.id}`,
    clean: `Clean text ${overrides.id}.`,
    language: "en",
    duration_ms: 4_200,
    status: "done",
    audio_path: `${overrides.id}.wav`,
    error: null,
    ...overrides,
  };
}

/** Runs inside the page. Must stay self-contained: it is serialised by Playwright. */
export function installTauriMock(seed: MockSeed & { settings: Settings }) {
  const state = {
    settings: seed.settings,
    history: seed.history ?? [],
    hasGroqKey: seed.hasGroqKey ?? false,
    groqTestOk: seed.groqTestOk ?? true,
    meridianOnline: seed.meridianOnline ?? false,
    meridianModels: seed.meridianModels ?? [],
    cleanupTestOk: seed.cleanupTestOk ?? true,
    accessibility: seed.accessibility ?? true,
    failing: seed.failing ?? {},
    version: "9.9.9",
  };
  const calls: { cmd: string; args: Record<string, unknown> }[] = [];

  // ---- callback registry + event plugin, as in @tauri-apps/api/mocks ----
  const callbacks = new Map<number, (data: unknown) => void>();
  const listeners = new Map<string, number[]>();
  const registerCallback = (cb: (data: unknown) => void, once = false) => {
    const id = window.crypto.getRandomValues(new Uint32Array(1))[0];
    callbacks.set(id, (data) => {
      if (once) callbacks.delete(id);
      cb(data);
    });
    return id;
  };
  const runCallback = (id: number, data: unknown) => callbacks.get(id)?.(data);
  const emit = (event: string, payload: unknown) => {
    for (const id of listeners.get(event) ?? []) runCallback(id, { event, id, payload });
  };

  const handleEvent = (cmd: string, args: Record<string, unknown>) => {
    if (cmd === "plugin:event|listen") {
      const ev = args.event as string;
      const handler = args.handler as number;
      listeners.set(ev, [...(listeners.get(ev) ?? []), handler]);
      return handler;
    }
    if (cmd === "plugin:event|unlisten") {
      const ev = args.event as string;
      listeners.set(ev, (listeners.get(ev) ?? []).filter((h) => h !== args.eventId));
      callbacks.delete(args.eventId as number);
      return null;
    }
    if (cmd === "plugin:event|emit") {
      emit(args.event as string, args.payload);
      return null;
    }
    return null;
  };

  const handleCommand = (cmd: string, args: Record<string, unknown>): unknown => {
    const s = state.settings;
    switch (cmd) {
      case "get_settings":
        return { ...s };
      case "set_settings": {
        state.settings = { ...(args.new as typeof s) };
        emit("settings:changed", { ...state.settings });
        return null;
      }
      case "has_groq_key":
        return state.hasGroqKey || s.groq_api_key !== "";
      case "set_groq_key":
        state.settings = { ...s, groq_api_key: args.key as string };
        state.hasGroqKey = true;
        return null;
      case "test_groq_key":
        return state.groqTestOk;
      case "meridian_status":
        return state.meridianOnline;
      case "meridian_models":
        return state.meridianOnline ? [...state.meridianModels] : [];
      case "test_cleanup":
        if (!state.cleanupTestOk) throw "meridian api 500: boom";
        return null;
      case "history_list": {
        const q = ((args.search as string | null) ?? "").toLowerCase();
        return state.history
          .filter((d) => !q || d.clean.toLowerCase().includes(q) || d.raw.toLowerCase().includes(q))
          .map((d) => ({ ...d }));
      }
      case "history_delete":
        state.history = state.history.filter((d) => d.id !== args.id);
        emit("history:changed", {});
        return null;
      case "history_clear":
        state.history = [];
        emit("history:changed", {});
        return null;
      case "history_retry": {
        const row = state.history.find((d) => d.id === args.id);
        if (!row) throw "entry does not exist";
        row.status = "done";
        row.error = null;
        row.raw = "retried raw";
        row.clean = "Retried and transcribed.";
        emit("history:changed", {});
        return null;
      }
      case "history_audio_path":
        return state.history.find((d) => d.id === args.id)?.audio_path ?? null;
      case "history_export_audio":
        return `/Users/tester/Downloads/dikto-${args.id}.wav`;
      case "permissions_status":
        return { accessibility: state.accessibility };
      case "finish_wizard":
        state.settings = { ...s, wizard_done: true };
        return null;
      case "plugin:app|version":
        return state.version;
      case "plugin:autostart|is_enabled":
        return s.autostart;
      default:
        // open_url, open_privacy_settings, hotkey_capture_start,
        // cancel_dictation, retry_transcription, plugin:window|*, autostart
        return null;
    }
  };

  const invoke = async (cmd: string, args: Record<string, unknown> = {}) => {
    if (cmd.startsWith("plugin:event|")) return handleEvent(cmd, args);
    calls.push({ cmd, args });
    if (cmd in state.failing) throw state.failing[cmd];
    return handleCommand(cmd, args);
  };

  const w = window as unknown as Record<string, unknown>;
  w.__TAURI_INTERNALS__ = {
    invoke,
    transformCallback: registerCallback,
    unregisterCallback: (id: number) => callbacks.delete(id),
    runCallback,
    callbacks,
    convertFileSrc: (p: string) => p,
    metadata: { currentWindow: { label: seed.windowLabel ?? "main" }, currentWebview: { label: seed.windowLabel ?? "main" } },
  };
  w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: (_e: string, id: number) => callbacks.delete(id) };
  w.__mock = { state, calls, emit };
}

/** Shape of `window.__mock`, for `page.evaluate` callers. */
export interface MockHandle {
  state: {
    settings: Settings;
    history: Dictation[];
    hasGroqKey: boolean;
    groqTestOk: boolean;
    meridianOnline: boolean;
    meridianModels: string[];
    cleanupTestOk: boolean;
    accessibility: boolean;
    failing: Record<string, string>;
  };
  calls: MockCall[];
  emit: (event: string, payload: unknown) => void;
}
