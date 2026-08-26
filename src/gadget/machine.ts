import type { GadgetVisualState, WidgetVisibilityMode } from "../lib/tauri";

export type GadgetState = GadgetVisualState;
export type GadgetContent =
  | "none"
  | "idle-mark"
  | "idle-actions"
  | "activity"
  | "waveform"
  | "processing-dots"
  | "processing-label"
  | "success-mark"
  | "error-message";
export type GadgetInteraction = "none" | "passive" | "start" | "recording-controls" | "retry";

export interface GadgetStateDefinition {
  visibility: "hidden" | "visible";
  geometry: { width: number; height: number };
  content: GadgetContent;
  interaction: GadgetInteraction;
  animation: "none" | "reveal" | "morph" | "settle" | "feedback";
  timeoutMs: number | null;
  accessibleLabel: string;
}

/**
 * Single presentation table for every dictation-bar state. React content,
 * timeout scheduling, accessibility and native geometry all key off the same
 * explicit state name; no independent `visible`/`recording`/`loading` soup.
 */
export const GADGET_STATES: Record<GadgetState, GadgetStateDefinition> = {
  hidden: {
    visibility: "hidden",
    geometry: { width: 1, height: 1 },
    content: "none",
    interaction: "none",
    animation: "none",
    timeoutMs: null,
    accessibleLabel: "Barra de ditado oculta",
  },
  idle: {
    visibility: "visible",
    geometry: { width: 72, height: 52 },
    content: "idle-mark",
    interaction: "start",
    animation: "settle",
    timeoutMs: null,
    accessibleLabel: "Haumea pronto para ditar",
  },
  hover: {
    visibility: "visible",
    geometry: { width: 142, height: 58 },
    content: "idle-actions",
    interaction: "start",
    animation: "morph",
    timeoutMs: null,
    accessibleLabel: "Iniciar ditado",
  },
  appearing: {
    visibility: "visible",
    geometry: { width: 72, height: 52 },
    content: "activity",
    interaction: "passive",
    animation: "reveal",
    timeoutMs: 160,
    accessibleLabel: "Iniciando ditado",
  },
  initializing: {
    visibility: "visible",
    geometry: { width: 74, height: 52 },
    content: "activity",
    interaction: "passive",
    animation: "morph",
    timeoutMs: null,
    accessibleLabel: "Preparando microfone",
  },
  recording: {
    visibility: "visible",
    geometry: { width: 186, height: 48 },
    content: "waveform",
    interaction: "recording-controls",
    animation: "reveal",
    timeoutMs: null,
    accessibleLabel: "Gravando ditado",
  },
  stopping: {
    visibility: "visible",
    geometry: { width: 74, height: 52 },
    content: "activity",
    interaction: "passive",
    animation: "settle",
    timeoutMs: null,
    accessibleLabel: "Finalizando gravação",
  },
  processing: {
    visibility: "visible",
    geometry: { width: 78, height: 52 },
    content: "processing-dots",
    interaction: "passive",
    animation: "morph",
    timeoutMs: 1800,
    accessibleLabel: "Processando transcrição",
  },
  processing_long: {
    visibility: "visible",
    geometry: { width: 158, height: 52 },
    content: "processing-label",
    interaction: "passive",
    animation: "morph",
    timeoutMs: null,
    accessibleLabel: "Transcrição ainda em processamento",
  },
  success: {
    visibility: "visible",
    geometry: { width: 54, height: 52 },
    content: "success-mark",
    interaction: "passive",
    animation: "feedback",
    timeoutMs: 520,
    accessibleLabel: "Transcrição inserida",
  },
  error: {
    visibility: "visible",
    geometry: { width: 326, height: 58 },
    content: "error-message",
    interaction: "retry",
    animation: "feedback",
    timeoutMs: 8000,
    accessibleLabel: "Falha na transcrição",
  },
};

export function restState(mode: WidgetVisibilityMode): GadgetState {
  return mode === "always" ? "idle" : "hidden";
}

export function stateAfterTimeout(
  state: GadgetState,
  mode: WidgetVisibilityMode,
): GadgetState | null {
  if (state === "appearing") return "initializing";
  if (state === "processing") return "processing_long";
  if (state === "success" || state === "error") return restState(mode);
  return null;
}

/** Compact processing only has room for the activity dots. */
export function showsProcessingLabel(state: GadgetState): boolean {
  return state === "processing_long";
}
