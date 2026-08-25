use cpal::Stream;
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Deserializer, Serialize};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

/// Sanitizer model selected manually by the user from the UI.
/// Each variant maps to a cloud-hosted LLM used for semantic validation
/// of the transcribed text before final delivery.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SanitizerModel {
    #[serde(rename = "llama-70b")]
    #[default]
    Llama70b,
    #[serde(rename = "gpt-oss-20b")]
    GptOss20b,
    #[serde(rename = "gpt-oss-120b")]
    GptOss120b,
    #[serde(rename = "qwen3-27b")]
    Qwen327b,
}

impl SanitizerModel {
    /// Maps the user-facing enum variant to the exact model string
    /// expected by the Groq / OpenAI Chat Completions endpoint.
    ///
    /// The GPT-OSS models are published on GroqCloud under the `openai/`
    /// namespace; the bare `gpt-oss-*` ids return `model_not_found`, which
    /// would silently fall the pipeline back to the raw transcription.
    pub fn api_model_id(&self) -> &'static str {
        match self {
            Self::Llama70b => "llama-3.3-70b-versatile",
            Self::GptOss20b => "openai/gpt-oss-20b",
            Self::GptOss120b => "openai/gpt-oss-120b",
            Self::Qwen327b => "qwen/qwen3.6-27b",
        }
    }

    /// Whether this model honours Groq's native `reasoning_effort` request
    /// parameter. Only the GPT-OSS family accepts `low`/`medium`/`high`;
    /// LLaMA 3.3 70B and the Qwen 3.6 27B preview model on Groq have no
    /// documented native reasoning control we can safely send, so enabling
    /// reasoning for them is a no-op (and sending the parameter would risk
    /// an API rejection that silently falls the pipeline back to raw text).
    pub fn supports_reasoning(&self) -> bool {
        matches!(self, Self::GptOss20b | Self::GptOss120b)
    }
}

/// Rectangle of the gadget overlay's *visible pill*, reported by the frontend
/// in logical pixels relative to the gadget window's top-left corner.
///
/// The gadget window is a fixed, mostly-transparent box; without this the whole
/// box would swallow mouse clicks. The background cursor watcher (see
/// `lib.rs`) uses this rect to keep the window click-through everywhere except
/// over the pill itself.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct GadgetHitRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Active transcription engine selected manually by the user from the UI.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TranscriptionEngine {
    #[default]
    GroqWhisper,
    DeepgramNova3,
    GeminiMultimodal,
}

/// Deepgram delivery mode. Controls how audio is sent to Deepgram when the
/// Nova-3 engine is selected (including dual-engine mode).
///
/// * [`Batch`] — classic REST upload of the complete file (current default).
/// * [`StreamingFinal`] — WebSocket streaming with `interim_results=false`;
///   only the final transcript is returned after the full audio has been
///   pushed (no partials reach the UI). Processing starts incrementally as
///   chunks arrive, typically reducing time-to-final vs batch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeepgramMode {
    #[default]
    Batch,
    StreamingFinal,
}

impl DeepgramMode {
    /// Wire-stable identifier used in logs and UI (`"batch"` / `"streaming_final"`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Batch => "batch",
            Self::StreamingFinal => "streaming_final",
        }
    }
}

/// API keys for the supported cloud engines. Held in memory (RAM) for the
/// running session and also persisted to `api_keys.json` in the app data
/// directory (see the `secrets` module) so they survive an app restart.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ApiKeys {
    #[serde(default, deserialize_with = "deserialize_key_list")]
    pub groq: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_key_list")]
    pub google: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_key_list")]
    pub deepgram: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_key_list")]
    pub openrouter: Vec<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum StoredKeyList {
    One(String),
    Many(Vec<String>),
}

fn deserialize_key_list<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let stored = Option::<StoredKeyList>::deserialize(deserializer)?;
    Ok(match stored {
        Some(StoredKeyList::One(key)) => vec![key],
        Some(StoredKeyList::Many(keys)) => keys,
        None => Vec::new(),
    })
}

fn normalized_keys(keys: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for key in keys {
        let key = key.trim().to_string();
        if !key.is_empty() && !out.contains(&key) {
            out.push(key);
        }
    }
    out
}

impl ApiKeys {
    pub fn normalized(self) -> Self {
        Self {
            groq: normalized_keys(self.groq),
            google: normalized_keys(self.google),
            deepgram: normalized_keys(self.deepgram),
            openrouter: normalized_keys(self.openrouter),
        }
    }
}

#[cfg(test)]
mod api_keys_tests {
    use std::sync::atomic::AtomicUsize;

    use super::{ApiKeys, AppState};

    #[test]
    fn migrates_legacy_single_keys_to_lists() {
        let keys: ApiKeys =
            serde_json::from_str(r#"{"groq":"g-one","google":"a-one","deepgram":null}"#).unwrap();
        assert_eq!(keys.groq, vec!["g-one"]);
        assert_eq!(keys.google, vec!["a-one"]);
        assert!(keys.deepgram.is_empty());
    }

    #[test]
    fn normalizes_lists_without_losing_order() {
        let keys: ApiKeys =
            serde_json::from_str(r#"{"google":[" first ","second","first",""]}"#).unwrap();
        assert_eq!(keys.normalized().google, vec!["first", "second"]);
    }

    #[test]
    fn rotates_provider_keys_in_stable_order() {
        let keys = vec!["one".into(), "two".into()];
        let cursor = AtomicUsize::new(0);

        assert_eq!(AppState::next_key(&keys, &cursor).as_deref(), Some("one"));
        assert_eq!(AppState::next_key(&keys, &cursor).as_deref(), Some("two"));
        assert_eq!(AppState::next_key(&keys, &cursor).as_deref(), Some("one"));
    }
}

/// Payload received from the frontend when the user changes the active
/// engine and sanitizer model in the settings screen.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineConfigPayload {
    pub engine: TranscriptionEngine,
    pub sanitizer: SanitizerModel,
    #[serde(default)]
    pub dual_engine: bool,
    #[serde(default)]
    pub reasoning_enabled: bool,
    #[serde(default = "default_reasoning_effort")]
    pub reasoning_effort: String,
    /// Deepgram transport mode (`batch` | `streaming_final`). Defaults to
    /// batch so older frontends that omit the field keep working.
    #[serde(default)]
    pub deepgram_mode: DeepgramMode,
}

fn default_reasoning_effort() -> String {
    "medium".to_string()
}

/// User-customisable global recording shortcuts. Persisted to
/// `shortcuts.json` in the app data directory and re-registered with the
/// global-shortcut plugin whenever the user rebinds them. Strings use the
/// `global-hotkey` format parsed by the plugin, e.g. `"Control+B"`,
/// `"Control+Shift+Q"`, `"Alt+F2"`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortcutConfig {
    pub toggle: String,
    pub cancel: String,
}

impl Default for ShortcutConfig {
    fn default() -> Self {
        Self {
            toggle: "Control+B".to_string(),
            cancel: "Control+Q".to_string(),
        }
    }
}

/// Payload received from the frontend when the user saves API keys
/// in the settings screen.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeysPayload {
    #[serde(default)]
    pub groq: Vec<String>,
    #[serde(default)]
    pub google: Vec<String>,
    #[serde(default)]
    pub deepgram: Vec<String>,
    #[serde(default)]
    pub openrouter: Vec<String>,
}

/// Global application state guarded by locks for safe concurrent
/// access across Tauri commands and the global shortcut listener.
///
/// Configuration fields use `RwLock` (reads outnumber writes) while
/// the audio pipeline uses `Mutex` because the data callback pushes
/// samples in short, exclusive bursts. The active `cpal::Stream`
/// handle is stored as `Option` so it can be swapped out cleanly when
/// a recording starts or stops.
pub struct AppState {
    pub engine: RwLock<TranscriptionEngine>,
    pub sanitizer: RwLock<SanitizerModel>,
    pub recording: RwLock<bool>,
    pub api_keys: RwLock<ApiKeys>,
    pub audio_stream: Mutex<Option<Stream>>,
    pub audio_buffer: Mutex<Vec<i16>>,
    pub test_stream: Mutex<Option<Stream>>,
    /// Native sample rate (Hz) of the active capture stream. The microphone is
    /// recorded at the device's own rate and resampled to 16 kHz only when the
    /// final WAV is assembled, because forcing an unsupported rate on the
    /// stream itself fails on Windows/WASAPI shared mode.
    pub capture_rate: RwLock<u32>,
    /// Wall-clock instant the current recording began, used to compute the
    /// elapsed time so the timer survives navigating between views. `None`
    /// while idle.
    pub recording_since: Mutex<Option<std::time::Instant>>,
    pub system_prompt: RwLock<String>,
    /// When `true`, the floating gadget collapses to a bare icon while idle;
    /// when `false` it shows the icon plus the "Haumea Voice" wordmark. The
    /// recording state always expands the gadget regardless of this flag.
    /// Persisted to `settings.json` (see the `settings` module).
    pub compact_mode: RwLock<bool>,
    /// User-customisable global recording shortcuts (toggle/cancel).
    pub shortcuts: RwLock<ShortcutConfig>,
    /// Tauri `AppHandle` set once during `setup`. Held by the audio
    /// pipeline so a finished transcription can be persisted to disk and
    /// announced to the UI via the `transcription-saved` event without
    /// needing access to an `AppHandle` injection site.
    pub app_handle: RwLock<Option<tauri::AppHandle>>,
    pub dual_engine: RwLock<bool>,
    pub reasoning_enabled: RwLock<bool>,
    /// Deepgram transport: REST batch vs WebSocket streaming (final only).
    /// Persisted to `settings.json`. Only used when Deepgram is in the path.
    pub deepgram_mode: RwLock<DeepgramMode>,
    /// Live Deepgram WebSocket session (streaming_final + mic). Opened when
    /// recording starts and finished/aborted when recording stops/cancels.
    /// `None` while idle or in batch mode.
    pub deepgram_live: Mutex<Option<crate::deepgram::DeepgramLiveSession>>,
    /// When `true` (default), the acoustic transcription is passed through the
    /// Groq Chat Completions sanitizer (Stage 2) for orthographic cleanup and
    /// formatting. When `false`, the **raw** acoustic transcription is copied
    /// to the clipboard / pasted directly, skipping the sanitization round-
    /// trip entirely — useful when the user wants unmodified output or wants
    /// to save the extra network call. Persisted to `settings.json`.
    pub sanitizer_enabled: RwLock<bool>,
    pub reasoning_effort: RwLock<String>,
    /// Structured vocabulary (canonical, aliases, category, strict, enabled).
    /// Persisted to `settings.json`; legacy `custom_words` is migrated on load.
    pub vocabulary: RwLock<Vec<crate::vocabulary::VocabularyTerm>>,
    /// Latest visible-pill rectangle reported by the gadget overlay. `None`
    /// until the gadget reports for the first time. Consumed by the cursor
    /// watcher to make the overlay click-through outside the pill.
    pub gadget_hit_rect: RwLock<Option<GadgetHitRect>>,
    /// When `true`, mic/file runs use the product mode pipeline
    /// ([`crate::pipeline_contract::TranscriptionMode`]) for supported modes.
    /// When `false`, the legacy engine/dual/sanitizer path is used unchanged.
    pub modes_enabled: RwLock<bool>,
    /// Selected product mode (UltraFast / FastAccurate / …).
    pub transcription_mode: RwLock<crate::pipeline_contract::TranscriptionMode>,
    /// FastAccurate: if Gemini fails or is unavailable, fall back to Whisper.
    pub gemini_fallback_to_whisper: RwLock<bool>,
    /// Content-type hint for sanitizer / UltraPrecise prompts (`Auto` = heuristic).
    pub content_type: RwLock<crate::pipeline_contract::ContentType>,
    pub gemini_pipelines: RwLock<crate::pipeline_contract::GeminiPipelineConfig>,
    groq_key_cursor: AtomicUsize,
    google_key_cursor: AtomicUsize,
    deepgram_key_cursor: AtomicUsize,
    openrouter_key_cursor: AtomicUsize,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            engine: RwLock::new(TranscriptionEngine::default()),
            sanitizer: RwLock::new(SanitizerModel::default()),
            recording: RwLock::new(false),
            api_keys: RwLock::new(ApiKeys::default()),
            audio_stream: Mutex::new(None),
            audio_buffer: Mutex::new(Vec::new()),
            test_stream: Mutex::new(None),
            capture_rate: RwLock::new(16_000),
            recording_since: Mutex::new(None),
            system_prompt: RwLock::new(String::new()),
            compact_mode: RwLock::new(false),
            shortcuts: RwLock::new(ShortcutConfig::default()),
            app_handle: RwLock::new(None),
            dual_engine: RwLock::new(false),
            reasoning_enabled: RwLock::new(false),
            deepgram_mode: RwLock::new(DeepgramMode::default()),
            deepgram_live: Mutex::new(None),
            sanitizer_enabled: RwLock::new(true),
            reasoning_effort: RwLock::new("medium".to_string()),
            vocabulary: RwLock::new(Vec::new()),
            gadget_hit_rect: RwLock::new(None),
            modes_enabled: RwLock::new(false),
            transcription_mode: RwLock::new(crate::pipeline_contract::TranscriptionMode::UltraFast),
            gemini_fallback_to_whisper: RwLock::new(true),
            content_type: RwLock::new(crate::pipeline_contract::ContentType::Auto),
            gemini_pipelines: RwLock::new(crate::pipeline_contract::GeminiPipelineConfig::default()),
            groq_key_cursor: AtomicUsize::new(0),
            google_key_cursor: AtomicUsize::new(0),
            deepgram_key_cursor: AtomicUsize::new(0),
            openrouter_key_cursor: AtomicUsize::new(0),
        }
    }

    fn next_key(keys: &[String], cursor: &AtomicUsize) -> Option<String> {
        if keys.is_empty() {
            return None;
        }
        let index = cursor.fetch_add(1, Ordering::Relaxed) % keys.len();
        Some(keys[index].clone())
    }

    pub fn next_groq_key(&self) -> Option<String> {
        Self::next_key(&self.api_keys.read().groq, &self.groq_key_cursor)
    }

    pub fn next_google_key(&self) -> Option<String> {
        Self::next_key(&self.api_keys.read().google, &self.google_key_cursor)
    }

    pub fn next_deepgram_key(&self) -> Option<String> {
        Self::next_key(&self.api_keys.read().deepgram, &self.deepgram_key_cursor)
    }

    pub fn next_openrouter_key(&self) -> Option<String> {
        Self::next_key(
            &self.api_keys.read().openrouter,
            &self.openrouter_key_cursor,
        )
    }

    /// Convenience helper used by the panic shortcut and the toggle
    /// command to flip the recording flag and return the new value.
    pub fn set_recording(&self, value: bool) -> bool {
        let mut guard = self.recording.write();
        *guard = value;
        value
    }

    /// Returns a snapshot of the current recording state.
    pub fn is_recording(&self) -> bool {
        *self.recording.read()
    }

    /// Stamps the moment the current recording began.
    pub fn mark_recording_start(&self) {
        *self.recording_since.lock() = Some(std::time::Instant::now());
    }

    /// Clears the recording start stamp (on stop or cancel).
    pub fn clear_recording_start(&self) {
        *self.recording_since.lock() = None;
    }

    /// Milliseconds elapsed since the current recording began, or `0` if idle.
    /// Lets the UI restore the timer after navigating away and back.
    pub fn recording_elapsed_ms(&self) -> u64 {
        self.recording_since
            .lock()
            .map(|t| t.elapsed().as_millis() as u64)
            .unwrap_or(0)
    }

    /// Returns a snapshot of the active engine.
    pub fn active_engine(&self) -> TranscriptionEngine {
        *self.engine.read()
    }

    /// Empties the in-memory sample buffer. Called both when a new
    /// recording starts (to discard stale data) and when a recording
    /// is cancelled (to free the allocated capacity).
    pub fn clear_audio_buffer(&self) {
        self.audio_buffer.lock().clear();
    }

    /// Takes ownership of the accumulated samples, leaving an empty
    /// buffer in place. Used when finalising a WAV file so the state
    /// is ready for the next session.
    pub fn drain_audio_buffer(&self) -> Vec<i16> {
        std::mem::take(&mut *self.audio_buffer.lock())
    }

    /// Computes a normalised loudness level (0.0..=1.0) over the most recent
    /// `window` samples currently buffered. Used by the gadget's live waveform:
    /// a background thread polls this while recording and emits the value to the
    /// overlay window. Returns `0.0` when the buffer is empty. The RMS is scaled
    /// up because speech rarely approaches full-scale amplitude.
    pub fn recent_level(&self, window: usize) -> f32 {
        // Copy the small analysis window and release the capture lock before
        // calculating RMS. This keeps the callback's critical section short
        // without forcing it to discard samples when the meter runs.
        let samples = {
            let buf = self.audio_buffer.lock();
            let n = buf.len();
            if n == 0 {
                return 0.0;
            }
            let start = n.saturating_sub(window);
            buf[start..].to_vec()
        };
        let sum_sq: f64 = samples
            .iter()
            .map(|&s| {
                let f = s as f64 / 32768.0;
                f * f
            })
            .sum();
        let rms = (sum_sq / samples.len() as f64).sqrt();
        // Perceptual boost: conversational speech sits far below full-scale,
        // so amplify aggressively for the gadget waveform. Tuned so normal
        // talking near the mic reliably pushes the meter into the upper range.
        ((rms * 7.5) as f32).clamp(0.0, 1.0)
    }

    /// Stops and drops the active input stream (if any), releasing
    /// the microphone hardware immediately. Returns `true` if a
    /// stream was actually dropped.
    pub fn drop_audio_stream(&self) -> bool {
        self.audio_stream.lock().take().is_some()
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

// cpal::Stream contains Windows COM raw pointers (*mut ()) and is marked NotSendSyncAcrossAllPlatforms.
// Since all fields in AppState are wrapped in Mutex or RwLock, it is safe to implement Send and Sync manually.
unsafe impl Send for AppState {}
unsafe impl Sync for AppState {}

/// Type alias used by Tauri's managed state. Wrapped in Arc so the
/// global shortcut handler can hold a cheap clone of the handle.
pub type SharedState = Arc<AppState>;

/// Serializable snapshot sent to the frontend via events so the UI
/// can update the timer display and status string.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "type")]
pub enum RecordingEvent {
    /// Emitted when a recording session has started.
    RecordingStarted,
    /// Emitted when a recording session has been stopped normally.
    RecordingStopped,
    /// Emitted when the user cancelled (panic shortcut) a session in
    /// progress. The frontend should reset the timer to 00:00.
    RecordingCancelled,
}

/// Names of the events emitted to the frontend. Kept as a small
/// module-level constant list to avoid magic strings drifting between
/// the Rust and TypeScript sides of the codebase.
pub mod event_names {
    pub const RECORDING_STARTED: &str = "recording-started";
    pub const RECORDING_STOPPED: &str = "recording-stopped";
    pub const RECORDING_CANCELLED: &str = "recording-cancelled";
    /// Emitted after a transcription has been produced and persisted to
    /// the history file. Payload is the full [`HistoryEntry`] snapshot.
    pub const TRANSCRIPTION_SAVED: &str = "transcription-saved";
    /// Emitted on every poll tick while recording with a normalised loudness
    /// level (`f32` in 0.0..=1.0) so the gadget can animate its live waveform.
    pub const AUDIO_LEVEL: &str = "audio-level";
    /// Emitted when the gadget compact-mode preference changes. Payload is the
    /// new `bool` value.
    pub const COMPACT_MODE_CHANGED: &str = "compact-mode-changed";
}

/// Developer-mode snapshot of the sanitizer (Groq Chat Completions) request and
/// response for a single transcription. Captured on every sanitization so the
/// Histórico can expose exactly what was sent — model, parameters, the reasoning
/// level actually applied and the raw JSON body — when developer mode is on.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SanitizerDebug {
    /// Endpoint the request was POSTed to.
    pub endpoint: String,
    /// Exact `model` id sent to Groq (e.g. `openai/gpt-oss-120b`).
    pub model: String,
    /// Decoding temperature sent in the request.
    pub temperature: f32,
    /// Whether the user had reasoning enabled in settings.
    pub reasoning_enabled: bool,
    /// Reasoning effort selected in settings (`low`/`medium`/`high`).
    pub reasoning_effort: String,
    /// Whether `reasoning_effort` was actually sent as a request parameter.
    /// True only when reasoning is enabled *and* the model supports it.
    pub reasoning_effort_applied: bool,
    /// Whether the selected model supports Groq's native reasoning parameter.
    pub reasoning_supported_by_model: bool,
    /// Final system prompt assembled and sent (base + glossary + dual-engine).
    pub system_prompt: String,
    /// User message sent (the raw `[WHISPER_RAW]` / `[DEEPGRAM_RAW]` payload).
    pub user_message: String,
    /// The exact request body serialized to pretty JSON.
    pub request_json: String,
    /// HTTP status of the response, when a response was received.
    #[serde(default)]
    pub response_status: Option<u16>,
    /// Final content returned by the model (the sanitized text).
    #[serde(default)]
    pub response_content: Option<String>,
    /// Reasoning trace returned by the model, truncated for storage.
    #[serde(default)]
    pub response_reasoning: Option<String>,
    /// Error string when the request failed (network/api/parse).
    #[serde(default)]
    pub error: Option<String>,
}

/// A single persisted transcription entry. Serialized to JSON in the
/// app data directory and also sent to the frontend over events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// Stable unique id (UTC milliseconds at creation time as a string).
    pub id: String,
    /// Human-readable timestamp, e.g. "2026-06-20 14:32".
    pub date: String,
    /// Number of words in the transcribed text.
    pub words: usize,
    /// Engine that produced the raw transcription.
    pub engine: String,
    /// Final text (after sanitization).
    pub text: String,
    /// Absolute path to the persisted source audio on disk, if it was saved
    /// (every new transcription saves one; older entries created before audio
    /// persistence existed have `None`). Required for pronunciation evaluation.
    #[serde(default)]
    pub audio_path: Option<String>,
    /// Markdown pronunciation feedback returned by Gemini, populated lazily the
    /// first time the user clicks "Avaliar Pronúncia" on this entry.
    #[serde(default)]
    pub evaluation: Option<String>,
    /// Duration of the source audio in milliseconds. Computed from the captured
    /// sample count for microphone recordings; `0` for file uploads (and for
    /// legacy entries created before this field existed).
    #[serde(default)]
    pub duration_ms: u64,
    /// How the transcription was produced: `"mic"` for a microphone recording
    /// or `"file"` for an uploaded audio file. Empty on legacy entries.
    #[serde(default)]
    pub source: String,
    /// Total processing latency in milliseconds.
    #[serde(default)]
    pub latency_ms: u64,
    /// Model generation throughput in tokens per second.
    #[serde(default)]
    pub throughput: f64,
    #[serde(default)]
    pub transcription_latency_ms: Option<u64>,
    #[serde(default)]
    pub sanitizer_latency_ms: Option<u64>,
    #[serde(default)]
    pub transcription_throughput: Option<f64>,
    #[serde(default)]
    pub sanitizer_throughput: Option<f64>,
    /// Real-time factor for the acoustic stage: `transcription_latency_ms / duration_ms`.
    /// Values below 1.0 mean faster than real-time (e.g. 0.25 = 4× realtime).
    /// Useful to compare Deepgram batch vs streaming_final on the same clip.
    #[serde(default)]
    pub realtime_factor: Option<f64>,
    /// Deepgram transport used when Deepgram was in the path (`batch` or
    /// `streaming_final`). `None` when only Groq/Gemini ran, or on legacy
    /// entries created before this field existed.
    #[serde(default)]
    pub deepgram_mode: Option<String>,
    #[serde(default)]
    pub total_tokens: Option<usize>,
    #[serde(default)]
    pub is_error: Option<bool>,
    #[serde(default)]
    pub error_message: Option<String>,
    /// Developer-mode capture of the sanitizer request/response. Populated on
    /// every successful sanitization; `None` for failed transcriptions (the
    /// sanitizer never ran) and for legacy entries created before this field.
    #[serde(default)]
    pub debug_info: Option<SanitizerDebug>,
    /// Product mode id when the new pipeline ran (`ultra-fast`, `fast-accurate`, …).
    #[serde(default)]
    pub mode: Option<String>,
    /// Primary model id used for the run (e.g. whisper-large-v3-turbo).
    #[serde(default)]
    pub model: Option<String>,
    /// Comma-separated stage names executed (e.g. `whisper`, `gemini_transcribe,whisper_fallback`).
    #[serde(default)]
    pub stages: Option<String>,
    /// True when a fallback path produced the final text.
    #[serde(default)]
    pub used_fallback: Option<bool>,
    /// Machine-readable fallback reason when [`used_fallback`] is true.
    #[serde(default)]
    pub fallback_reason: Option<String>,
    /// Content-type hint used for the run (`auto`, `programming`, …).
    #[serde(default)]
    pub content_type: Option<String>,
    /// Intermediate Whisper text when available.
    #[serde(default)]
    pub whisper_text: Option<String>,
    /// Intermediate sanitizer text when available.
    #[serde(default)]
    pub sanitizer_text: Option<String>,
    /// Intermediate Gemini text when available.
    #[serde(default)]
    pub gemini_text: Option<String>,
    /// Pipeline warnings (parse fallback, strict literals, etc.).
    #[serde(default)]
    pub warnings: Option<Vec<String>>,
    /// Structured stage timings (product modes). Absent on legacy entries.
    #[serde(default)]
    pub audio_prepare_ms: Option<u64>,
    #[serde(default)]
    pub base64_ms: Option<u64>,
    #[serde(default)]
    pub whisper_ms: Option<u64>,
    #[serde(default)]
    pub sanitizer_ms: Option<u64>,
    #[serde(default)]
    pub files_upload_ms: Option<u64>,
    #[serde(default)]
    pub files_poll_ms: Option<u64>,
    #[serde(default)]
    pub files_poll_count: Option<u32>,
    #[serde(default)]
    pub gemini_generate_ms: Option<u64>,
    #[serde(default)]
    pub gemini_delete_ms: Option<u64>,
    #[serde(default)]
    pub strict_literals_ms: Option<u64>,
    #[serde(default)]
    pub clipboard_ms: Option<u64>,
    #[serde(default)]
    pub total_pipeline_ms: Option<u64>,
    /// `inline` | `files_api`
    #[serde(default)]
    pub gemini_transport: Option<String>,
}
