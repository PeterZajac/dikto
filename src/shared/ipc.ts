/**
 * Typed IPC layer — mirrors src-tauri/src/settings.rs, history.rs and
 * commands.rs exactly. Every Rust command name string lives here, once.
 */
import { invoke } from "@tauri-apps/api/core";

export type LanguageMode = "auto" | "sk" | "cs" | "en";
export type CleanupStyle = "light" | "strong";

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
}

export interface Dictation {
  id: number;
  ts: number;
  raw: string;
  clean: string;
  language: string | null;
  duration_ms: number;
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
  historyList: (search?: string, limit?: number) =>
    invoke<Dictation[]>("history_list", { search: search ?? null, limit: limit ?? null }),
  historyDelete: (id: number) => invoke<void>("history_delete", { id }),
  historyClear: () => invoke<void>("history_clear"),
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
