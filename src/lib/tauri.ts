import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/**
 * Type-safe wrappers around the Rust IPC commands exposed by the Tauri
 * backend (see `src-tauri/src/commands.rs`).
 *
 * Every function returns a Promise that rejects with the serialized
 * `CommandError` string if the backend returns `Err`.
 */

export type TranscriptionEngine = "groq-whisper" | "deepgram-nova3" | "gemini-multimodal";
export type SanitizerModel =
  | "llama-70b"
  | "gpt-oss-20b"
  | "gpt-oss-120b"
  | "qwen3-27b";

/** Deepgram transport: REST batch vs WebSocket streaming (final only). */
export type DeepgramMode = "batch" | "streaming_final";

/** Product transcription modes (Phase 04+). */
export type TranscriptionMode =
  | "ultra-fast"
  | "fast-accurate"
  | "precise"
  | "ultra-precise";

export interface EngineConfigPayload {
  engine: TranscriptionEngine;
  sanitizer: SanitizerModel;
  dual_engine: boolean;
  reasoning_enabled: boolean;
  reasoning_effort: string;
  deepgram_mode: DeepgramMode;
}

export interface EngineConfigSnapshot {
  engine: TranscriptionEngine;
  sanitizer: string;
  dual_engine: boolean;
  reasoning_enabled: boolean;
  reasoning_effort: string;
  deepgram_mode: DeepgramMode;
}

export type ContentType =
  | "auto"
  | "programming"
  | "study";
export type GeminiModel = "flash-lite35" | "flash36";
export type GeminiProvider = "google-ai-studio" | "open-router";
export type OpenRouterWhisperModel = "large-v3-turbo" | "large-v3";
export interface GeminiPipelineChoice {
  model: GeminiModel;
  provider: GeminiProvider;
  use_custom_model: boolean;
  custom_model: string;
}
export interface GeminiPipelineConfig {
  ultra_fast_whisper: OpenRouterWhisperModel;
  fast_accurate: GeminiPipelineChoice;
  precise: GeminiPipelineChoice;
  ultra_precise: GeminiPipelineChoice;
}

export interface ModeConfigPayload {
  modes_enabled: boolean;
  mode: TranscriptionMode;
  gemini_fallback_to_whisper: boolean;
  content_type: ContentType;
  gemini_pipelines: GeminiPipelineConfig;
}

export interface ModeConfigSnapshot {
  modes_enabled: boolean;
  mode: TranscriptionMode;
  gemini_fallback_to_whisper: boolean;
  content_type: ContentType;
  gemini_pipelines: GeminiPipelineConfig;
  mode_label: string;
  mode_description: string;
}

export async function getModeConfig(): Promise<ModeConfigSnapshot> {
  return invoke<ModeConfigSnapshot>("get_mode_config");
}

export async function updateModeConfig(
  payload: ModeConfigPayload,
): Promise<ModeConfigSnapshot> {
  return invoke<ModeConfigSnapshot>("update_mode_config", { payload });
}

export interface ApiKeysPayload {
  groq: string[];
  google: string[];
  deepgram: string[];
  openrouter: string[];
}

export async function updateEngineConfig(
  payload: EngineConfigPayload,
): Promise<EngineConfigSnapshot> {
  return invoke<EngineConfigSnapshot>("update_engine_config", { payload });
}

export async function saveApiKeys(payload: ApiKeysPayload): Promise<void> {
  await invoke<void>("save_api_keys", { payload });
}

/** Returns the persisted API keys so the settings screen can prefill them. */
export async function getApiKeys(): Promise<ApiKeysPayload> {
  return invoke<ApiKeysPayload>("get_api_keys");
}

/**
 * Transcribes a local audio file at `path` (selected or dropped in the
 * Transcrição view). Resolves with the final sanitised text and rejects with
 * a readable error string from the backend.
 */
export async function transcribeFile(path: string): Promise<string> {
  return invoke<string>("transcribe_file", { path });
}

/**
 * Sends the saved audio + transcript of history entry `id` to Gemini and
 * resolves with the Markdown pronunciation feedback (also persisted on the
 * entry by the backend).
 */
export async function evaluatePronunciation(id: string): Promise<string> {
  return invoke<string>("evaluate_pronunciation", { id });
}

export interface ShortcutConfig {
  toggle: string;
  cancel: string;
}

/** Returns the currently active recording shortcuts. */
export async function getShortcuts(): Promise<ShortcutConfig> {
  return invoke<ShortcutConfig>("get_shortcuts");
}

/**
 * Rebinds the global start/cancel recording shortcuts. Resolves with the
 * applied config or rejects with a readable error if the combination is
 * invalid or already in use by another application.
 */
export async function setShortcuts(
  toggle: string,
  cancel: string,
): Promise<ShortcutConfig> {
  return invoke<ShortcutConfig>("set_shortcuts", { toggle, cancel });
}

/** Toggles the recording flag in the backend and returns the new state. */
export async function toggleRecordingState(): Promise<boolean> {
  return invoke<boolean>("toggle_recording_state");
}

/** Returns the current recording flag from the backend. */
export async function getRecordingState(): Promise<boolean> {
  return invoke<boolean>("get_recording_state");
}

/** Milliseconds elapsed since the current recording began (backend truth). */
export async function getRecordingElapsed(): Promise<number> {
  return invoke<number>("get_recording_elapsed");
}

/** Returns the currently active transcription engine and sanitizer config. */
export async function getEngineConfig(): Promise<EngineConfigSnapshot> {
  return invoke<EngineConfigSnapshot>("get_engine_config");
}

/**
 * Developer-mode capture of the sanitizer (Groq Chat Completions) request and
 * response, mirroring the Rust `SanitizerDebug`. Present on entries transcribed
 * after this feature shipped; inspected from the Histórico when dev mode is on.
 */
export interface SanitizerDebug {
  endpoint: string;
  model: string;
  temperature: number;
  reasoning_enabled: boolean;
  reasoning_effort: string;
  reasoning_effort_applied: boolean;
  reasoning_supported_by_model: boolean;
  system_prompt: string;
  user_message: string;
  request_json: string;
  response_status?: number | null;
  response_content?: string | null;
  response_reasoning?: string | null;
  error?: string | null;
}

/** A single persisted transcription, mirroring the Rust `HistoryEntry`. */
export interface HistoryEntry {
  id: string;
  date: string;
  words: number;
  engine: string;
  text: string;
  audio_path?: string | null;
  evaluation?: string | null;
  duration_ms?: number;
  source?: string;
  latency_ms?: number;
  throughput?: number;
  transcription_latency_ms?: number | null;
  sanitizer_latency_ms?: number | null;
  transcription_throughput?: number | null;
  sanitizer_throughput?: number | null;
  /** Acoustic RTF: transcription_latency_ms / duration_ms (< 1 = faster than realtime). */
  realtime_factor?: number | null;
  /** Deepgram transport when used: `batch` | `streaming_final`. */
  deepgram_mode?: string | null;
  total_tokens?: number | null;
  is_error?: boolean | null;
  error_message?: string | null;
  debug_info?: SanitizerDebug | null;
  mode?: string | null;
  model?: string | null;
  stages?: string | null;
  used_fallback?: boolean | null;
  fallback_reason?: string | null;
  content_type?: string | null;
  whisper_text?: string | null;
  sanitizer_text?: string | null;
  gemini_text?: string | null;
  warnings?: string[] | null;
  audio_prepare_ms?: number | null;
  base64_ms?: number | null;
  whisper_ms?: number | null;
  sanitizer_ms?: number | null;
  files_upload_ms?: number | null;
  files_poll_ms?: number | null;
  files_poll_count?: number | null;
  gemini_generate_ms?: number | null;
  gemini_delete_ms?: number | null;
  strict_literals_ms?: number | null;
  clipboard_ms?: number | null;
  total_pipeline_ms?: number | null;
  gemini_transport?: string | null;
}

/** Returns the full persisted transcription history, newest first. */
export async function getHistory(): Promise<HistoryEntry[]> {
  return invoke<HistoryEntry[]>("get_history");
}

export interface AudioStorageConfig {
  custom_directory?: string | null;
  effective_directory: string;
  default_directory: string;
}

export async function getAudioStorageConfig(): Promise<AudioStorageConfig> {
  return invoke<AudioStorageConfig>("get_audio_storage_config");
}

export async function setAudioStorageDirectory(
  path: string | null,
): Promise<AudioStorageConfig> {
  return invoke<AudioStorageConfig>("set_audio_storage_directory", { path });
}

/** Opens Explorer with the saved audio file selected. */
export async function revealHistoryAudio(id: string): Promise<void> {
  await invoke<void>("reveal_history_audio", { id });
}

/** Regenerates a persisted transcription from its saved audio. */
export async function retryTranscription(id: string): Promise<string> {
  return invoke<string>("retry_transcription", { id });
}

/** Structured vocabulary category (Phase 06). */
export type VocabularyCategory =
  | "ai_model"
  | "provider"
  | "application"
  | "person"
  | "file"
  | "command"
  | "function"
  | "identifier"
  | "study_term"
  | "other";

export interface VocabularyTerm {
  canonical: string;
  aliases: string[];
  category: VocabularyCategory;
  strict: boolean;
  enabled: boolean;
}

/** Legacy: enabled canonical spellings only. */
export async function getCustomWords(): Promise<string[]> {
  return invoke<string[]>("get_custom_words");
}

/** Legacy: replace vocabulary with simple words. */
export async function setCustomWords(words: string[]): Promise<string[]> {
  return invoke<string[]>("set_custom_words", { words });
}

export async function getVocabulary(): Promise<VocabularyTerm[]> {
  return invoke<VocabularyTerm[]>("get_vocabulary");
}

export async function setVocabulary(
  terms: VocabularyTerm[],
): Promise<VocabularyTerm[]> {
  return invoke<VocabularyTerm[]>("set_vocabulary", { terms });
}

/** Returns the persisted developer-mode flag. */
export async function getDevMode(): Promise<boolean> {
  return invoke<boolean>("get_dev_mode");
}

/** Persists the developer-mode flag (gates the request-inspection UI). */
export async function setDevMode(value: boolean): Promise<void> {
  await invoke<void>("set_dev_mode", { value });
}

/** Returns whether the semantic validator (sanitizer) is active. */
export async function getSanitizerEnabled(): Promise<boolean> {
  return invoke<boolean>("get_sanitizer_enabled");
}

/** Toggles the semantic validator on/off and persists the choice. */
export async function setSanitizerEnabled(value: boolean): Promise<void> {
  await invoke<void>("set_sanitizer_enabled", { value });
}

/** Returns the gadget compact-mode preference. */
export async function getCompactMode(): Promise<boolean> {
  return invoke<boolean>("get_compact_mode");
}

/** Persists the gadget compact-mode preference and notifies the gadget window. */
export async function setCompactMode(value: boolean): Promise<void> {
  await invoke<void>("set_compact_mode", { value });
}

/** Visible-pill rectangle of the gadget overlay, in logical pixels relative to
 *  the gadget window's top-left corner. */
export interface GadgetHitRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/**
 * Reports the gadget's visible-pill rectangle to the backend so the overlay
 * window can be made click-through everywhere except over the pill (preventing
 * the transparent area from swallowing nearby clicks).
 */
export async function setGadgetHitRect(rect: GadgetHitRect): Promise<void> {
  await invoke<void>("set_gadget_hit_rect", { rect });
}

/* ------------------------------- Events ------------------------------- */

export type RecordingEventType =
  | "recording-started"
  | "recording-stopped"
  | "recording-cancelled";

export function onRecordingEvent(
  handler: (type: RecordingEventType) => void,
): Promise<UnlistenFn> {
  const subscribe = async (name: RecordingEventType) => {
    return listen(name, () => handler(name));
  };

  // listen() resolves once per subscription; we wrap all three and return
  // a single unlisten that tears them all down.
  return (async () => {
    const unlisteners = await Promise.all([
      subscribe("recording-started"),
      subscribe("recording-stopped"),
      subscribe("recording-cancelled"),
    ]);
    return () => unlisteners.forEach((u) => u());
  })();
}

/** Lista todos os microfones conectados no host. */
export async function listAudioDevices(): Promise<string[]> {
  return invoke<string[]>("list_audio_devices");
}

/** Retorna o nome do microfone selecionado (ou null se for o padrão). */
export async function getInputDevice(): Promise<string | null> {
  return invoke<string | null>("get_input_device");
}

/** Salva o microfone desejado (ou null para usar o padrão). */
export async function setInputDevice(device: string | null): Promise<void> {
  await invoke<void>("set_input_device", { device });
}

/** Inicia a captura de teste do microfone. */
export async function startMicTest(): Promise<void> {
  await invoke<void>("start_mic_test");
}

/** Para a captura de teste do microfone. */
export async function stopMicTest(): Promise<void> {
  await invoke<void>("stop_mic_test");
}

/** Escuta o nível de áudio emitido pelo teste de microfone (0.0 a 1.0). */
export function onMicTestLevel(handler: (level: number) => void): Promise<UnlistenFn> {
  return listen<number>("mic-test-level", (event) => handler(event.payload));
}

/** Escuta mudanças no estado de transcrição (loading do gadget). */
export function onTranscribingEvent(handler: (transcribing: boolean) => void): Promise<UnlistenFn> {
  return listen<boolean>("transcribing", (event) => handler(event.payload));
}

