export type Phase =
  | "idle"
  | "recording"
  | "transcribing"
  | "cleaning"
  | "injecting"
  | "error";

export const EVENT_STATE = "dictation:state";
export const EVENT_AMPLITUDE = "dictation:amplitude";
export const EVENT_PARTIAL = "dictation:partial";
export const EVENT_PIPELINE_DEAD = "dictation:pipeline-dead";
export const EVENT_SETTINGS_CHANGED = "settings:changed";
/** Fired whenever a dictation row is created or updated by the backend. */
export const EVENT_HISTORY_CHANGED = "history:changed";
export const EVENT_HOTKEY_CAPTURED = "hotkey:captured";

export interface StatePayload {
  phase: Phase;
  message: string | null;
  /** Set by the backend when the failure kept the audio and a retry could fix it. */
  retryable?: boolean;
}
export interface AmplitudePayload {
  value: number;
}
export interface PartialPayload {
  text: string;
}
export interface PipelineDeadPayload {
  message: string;
}
export interface HotkeyCapturedPayload {
  key: string;
}
