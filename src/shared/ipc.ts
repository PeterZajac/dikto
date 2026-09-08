/**
 * Typed IPC layer — mirrors src-tauri/src/settings.rs, history.rs and
 * commands.rs exactly. Every Rust command name string lives here, once.
 */
import { invoke } from "@tauri-apps/api/core";

export type LanguageMode = "auto" | "sk" | "cs" | "en";
export type CleanupStyle = "light" | "strong";
export type UiLanguage = "en" | "sk";

export interface Settings {
  hotkey: string;
  language: LanguageMode;
  cleanup_enabled: boolean;
  cleanup_model: string;
  meridian_url: string;
  groq_url: string;
  cleanup_style: CleanupStyle;
  wizard_done: boolean;
  bubble_pos: [number, number] | null;
  autostart: boolean;
  groq_api_key: string;
  /** Days a finished dictation stays in history (text + audio). 0 = keep forever. */
  history_retention_days: number;
  /** Language of the app UI (bubble, settings, tray). Independent of `language`. */
  ui_language: UiLanguage;
}

/** "pending" = recorded, not transcribed yet. "failed" = audio kept, retryable. */
export type DictationStatus = "pending" | "done" | "failed";

export interface Dictation {
  id: number;
  ts: number;
  raw: string;
  clean: string;
  language: string | null;
  duration_ms: number;
  status: DictationStatus;
  audio_path: string | null;
  error: string | null;
}

/** Mirrors `Note` in src-tauri/src/notes.rs. An empty title renders as a
 *  localised "Untitled" — the placeholder is never stored. */
export interface Note {
  id: number;
  title: string;
  body: string;
  created_at: number;
  updated_at: number;
}

export interface PermissionsStatus {
  accessibility: boolean;
}

export type PrivacyPane = "accessibility" | "microphone";

export const api = {
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (next: Settings) => invoke<void>("set_settings", { new: next }),
  hasGroqKey: () => invoke<boolean>("has_groq_key"),
  setGroqKey: (key: string) => invoke<void>("set_groq_key", { key }),
  testGroqKey: () => invoke<boolean>("test_groq_key"),
  meridianStatus: () => invoke<boolean>("meridian_status"),
  // Model ids from Meridian's /v1/models; [] when Meridian is unreachable.
  meridianModels: () => invoke<string[]>("meridian_models"),
  historyList: (search?: string, limit?: number) =>
    invoke<Dictation[]>("history_list", { search: search ?? null, limit: limit ?? null }),
  historyDelete: (id: number) => invoke<void>("history_delete", { id }),
  historyClear: () => invoke<void>("history_clear"),
  // Re-transcribes a stored recording in place; never pastes anywhere.
  historyRetry: (id: number) => invoke<void>("history_retry", { id }),
  historyAudioPath: (id: number) => invoke<string | null>("history_audio_path", { id }),
  // Saves the WAV into the user's Downloads folder, returning the final path.
  historyExportAudio: (id: number) => invoke<string>("history_export_audio", { id }),
  notesList: (search?: string, limit?: number) =>
    invoke<Note[]>("notes_list", { search: search ?? null, limit: limit ?? null }),
  notesGet: (id: number) => invoke<Note | null>("notes_get", { id }),
  notesCreate: (title?: string) => invoke<Note>("notes_create", { title: title ?? null }),
  // Partial patch: an omitted field is left as it is on the row.
  notesUpdate: (id: number, patch: { title?: string; body?: string }) =>
    invoke<void>("notes_update", { id, title: patch.title ?? null, body: patch.body ?? null }),
  notesDelete: (id: number) => invoke<void>("notes_delete", { id }),
  // One-token round trip through Meridian — proves it answers, not just listens.
  testCleanup: () => invoke<void>("test_cleanup"),
  permissionsStatus: () => invoke<PermissionsStatus>("permissions_status"),
  openPrivacySettings: (pane: PrivacyPane) => invoke<void>("open_privacy_settings", { pane }),
  // Hard-allowlisted server-side to https://console.groq.com — see open_url in commands.rs.
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  // cancel: true disarms the capture flag without a keypress (used by the
  // Settings UI's 10s capture-mode timeout).
  hotkeyCaptureStart: (cancel = false) => invoke<void>("hotkey_capture_start", { cancel }),
  finishWizard: () => invoke<void>("finish_wizard"),
  cancelDictation: () => invoke<void>("cancel_dictation"),
  retryTranscription: () => invoke<void>("retry_transcription"),
};
