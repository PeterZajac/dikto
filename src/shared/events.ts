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

export interface StatePayload {
  phase: Phase;
  message: string | null;
}
export interface AmplitudePayload {
  value: number;
}
export interface PartialPayload {
  text: string;
}
