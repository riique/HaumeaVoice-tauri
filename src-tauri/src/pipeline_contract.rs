//! Pipeline contracts for the future transcription modes.
//!
//! Phase 01: types, serialization, legacy compatibility helpers, and local
//! mocks only. Nothing here is wired to the live `audio.rs` path or the UI yet.
//! Existing engine / dual / Deepgram / sanitizer settings remain authoritative
//! at runtime until a later phase switches the orchestrator over.

use crate::models::{
    DeepgramMode, HistoryEntry, SanitizerDebug, SanitizerModel, TranscriptionEngine,
};
use serde::{Deserialize, Serialize};

// ─── Modes & content ────────────────────────────────────────────────────────

/// Product-facing transcription mode (future selector).
///
/// Wire ids are stable kebab-case for settings.json / IPC.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TranscriptionMode {
    /// Whisper only.
    #[default]
    UltraFast,
    /// Gemini with audio.
    FastAccurate,
    /// Whisper + Gemini with audio.
    Precise,
    /// Whisper + sanitizer + Gemini with audio.
    UltraPrecise,
}

impl TranscriptionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UltraFast => "ultra-fast",
            Self::FastAccurate => "fast-accurate",
            Self::Precise => "precise",
            Self::UltraPrecise => "ultra-precise",
        }
    }

    /// Human label for future UI (clear product names).
    pub fn display_name_pt(self) -> &'static str {
        match self {
            Self::UltraFast => "Ultrarrápido",
            Self::FastAccurate => "Rápido e preciso",
            Self::Precise => "Preciso",
            Self::UltraPrecise => "Ultrapreciso",
        }
    }

    pub fn uses_whisper(self) -> bool {
        matches!(self, Self::UltraFast | Self::Precise | Self::UltraPrecise)
    }

    pub fn uses_gemini_audio(self) -> bool {
        matches!(
            self,
            Self::FastAccurate | Self::Precise | Self::UltraPrecise
        )
    }

    pub fn uses_sanitizer(self) -> bool {
        matches!(self, Self::UltraPrecise)
    }

    /// Maps current engine + dual flags onto the closest future mode.
    ///
    /// Runtime stays on the legacy path until a later phase adopts modes.
    /// Dual → Precise (multi-pass accuracy intent). Single acoustic → UltraFast.
    /// Gemini enum (unused for STT) → FastAccurate as the natural home.
    pub fn from_legacy(engine: TranscriptionEngine, dual_engine: bool) -> Self {
        if dual_engine {
            return Self::Precise;
        }
        match engine {
            TranscriptionEngine::GroqWhisper | TranscriptionEngine::DeepgramNova3 => {
                Self::UltraFast
            }
            TranscriptionEngine::GeminiMultimodal => Self::FastAccurate,
        }
    }
}

/// Optional content hint for future prompt routing (not applied yet).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContentType {
    #[default]
    Auto,
    Programming,
    GeneralSpeech,
    Study,
}

impl ContentType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Programming => "programming",
            Self::GeneralSpeech => "general-speech",
            Self::Study => "study",
        }
    }
}

// ─── Stage identity ─────────────────────────────────────────────────────────

/// Named pipeline stage for timings, warnings, and intermediate outputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineStage {
    Whisper,
    Deepgram,
    Sanitizer,
    GeminiAudio,
    Finalize,
}

impl PipelineStage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Whisper => "whisper",
            Self::Deepgram => "deepgram",
            Self::Sanitizer => "sanitizer",
            Self::GeminiAudio => "gemini_audio",
            Self::Finalize => "finalize",
        }
    }
}

// ─── Request / result contracts ─────────────────────────────────────────────

/// Input to a future pipeline run. Carries audio + resolved preferences.
/// Not consumed by `audio.rs` in phase 01.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineRequest {
    /// Stable id (history / audio file stem).
    pub id: String,
    /// WAV or original upload bytes (not persisted inside this struct on disk).
    #[serde(skip)]
    pub audio_bytes: Vec<u8>,
    pub file_name: String,
    pub mime: String,
    pub mode: TranscriptionMode,
    #[serde(default)]
    pub content_type: ContentType,
    /// Legacy engine still selected in settings (preserved for dual/Deepgram).
    pub engine: TranscriptionEngine,
    pub dual_engine: bool,
    pub deepgram_mode: DeepgramMode,
    pub sanitizer: SanitizerModel,
    pub sanitizer_enabled: bool,
    pub reasoning_enabled: bool,
    pub reasoning_effort: String,
    pub system_prompt: String,
    #[serde(default)]
    pub custom_words: Vec<String>,
    /// `"mic"` | `"file"`.
    pub source: String,
    pub duration_ms: u64,
    pub copy_to_clipboard: bool,
}

impl PipelineRequest {
    /// Builds a request snapshot from the **current** legacy settings shape
    /// without dropping any preference the user already has.
    pub fn from_legacy_settings(
        id: impl Into<String>,
        audio_bytes: Vec<u8>,
        file_name: impl Into<String>,
        mime: impl Into<String>,
        engine: TranscriptionEngine,
        dual_engine: bool,
        deepgram_mode: DeepgramMode,
        sanitizer: SanitizerModel,
        sanitizer_enabled: bool,
        reasoning_enabled: bool,
        reasoning_effort: impl Into<String>,
        system_prompt: impl Into<String>,
        custom_words: Vec<String>,
        source: impl Into<String>,
        duration_ms: u64,
        copy_to_clipboard: bool,
    ) -> Self {
        let engine_v = engine;
        let dual_v = dual_engine;
        Self {
            id: id.into(),
            audio_bytes,
            file_name: file_name.into(),
            mime: mime.into(),
            mode: TranscriptionMode::from_legacy(engine_v, dual_v),
            content_type: ContentType::default(),
            engine: engine_v,
            dual_engine: dual_v,
            deepgram_mode,
            sanitizer,
            sanitizer_enabled,
            reasoning_enabled,
            reasoning_effort: reasoning_effort.into(),
            system_prompt: system_prompt.into(),
            custom_words,
            source: source.into(),
            duration_ms,
            copy_to_clipboard,
        }
    }
}

/// Per-stage outcome (success or structured failure).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineStageResult {
    pub stage: PipelineStage,
    pub ok: bool,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub latency_ms: u64,
    #[serde(default)]
    pub error: Option<String>,
}

/// Non-fatal pipeline notice surfaced to logs / future UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineWarning {
    pub stage: PipelineStage,
    pub code: String,
    pub message: String,
}

impl PipelineWarning {
    pub fn new(stage: PipelineStage, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            stage,
            code: code.into(),
            message: message.into(),
        }
    }
}

/// Latency breakdown for a full run.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PipelineTimings {
    #[serde(default)]
    pub whisper_ms: Option<u64>,
    #[serde(default)]
    pub deepgram_ms: Option<u64>,
    #[serde(default)]
    pub sanitizer_ms: Option<u64>,
    #[serde(default)]
    pub gemini_ms: Option<u64>,
    #[serde(default)]
    pub total_ms: u64,
}

impl PipelineTimings {
    pub fn recompute_total(&mut self) {
        self.total_ms = self.whisper_ms.unwrap_or(0)
            + self.deepgram_ms.unwrap_or(0)
            + self.sanitizer_ms.unwrap_or(0)
            + self.gemini_ms.unwrap_or(0);
    }
}

/// Full outcome of a pipeline run (future orchestrator return type).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineResult {
    pub id: String,
    pub mode: TranscriptionMode,
    #[serde(default)]
    pub content_type: ContentType,
    /// Final text delivered to clipboard / history (may be empty on hard fail).
    pub final_text: String,
    #[serde(default)]
    pub whisper_text: Option<String>,
    #[serde(default)]
    pub deepgram_text: Option<String>,
    #[serde(default)]
    pub sanitizer_text: Option<String>,
    #[serde(default)]
    pub gemini_text: Option<String>,
    #[serde(default)]
    pub stages: Vec<PipelineStageResult>,
    #[serde(default)]
    pub warnings: Vec<PipelineWarning>,
    #[serde(default)]
    pub timings: PipelineTimings,
    /// Models actually used (ids as sent to providers).
    #[serde(default)]
    pub models_used: Vec<String>,
    /// True when final_text came from a fallback path (raw STT, secondary engine, etc.).
    #[serde(default)]
    pub used_fallback: bool,
    #[serde(default)]
    pub fallback_reason: Option<String>,
    #[serde(default)]
    pub is_error: bool,
    #[serde(default)]
    pub error_message: Option<String>,
    #[serde(default)]
    pub debug_info: Option<SanitizerDebug>,
    /// Engine label written to history (legacy-compatible strings).
    #[serde(default)]
    pub history_engine_label: String,
}

impl PipelineResult {
    pub fn success(
        id: impl Into<String>,
        mode: TranscriptionMode,
        final_text: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            mode,
            content_type: ContentType::default(),
            final_text: final_text.into(),
            whisper_text: None,
            deepgram_text: None,
            sanitizer_text: None,
            gemini_text: None,
            stages: Vec::new(),
            warnings: Vec::new(),
            timings: PipelineTimings::default(),
            models_used: Vec::new(),
            used_fallback: false,
            fallback_reason: None,
            is_error: false,
            error_message: None,
            debug_info: None,
            history_engine_label: String::new(),
        }
    }

    pub fn hard_error(
        id: impl Into<String>,
        mode: TranscriptionMode,
        message: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            mode,
            content_type: ContentType::default(),
            final_text: String::new(),
            whisper_text: None,
            deepgram_text: None,
            sanitizer_text: None,
            gemini_text: None,
            stages: Vec::new(),
            warnings: Vec::new(),
            timings: PipelineTimings::default(),
            models_used: Vec::new(),
            used_fallback: false,
            fallback_reason: None,
            is_error: true,
            error_message: Some(message.into()),
            debug_info: None,
            history_engine_label: String::new(),
        }
    }

    /// Projects this result onto a legacy [`HistoryEntry`] without dropping fields
    /// the Histórico already understands.
    pub fn to_history_entry(
        &self,
        date: impl Into<String>,
        audio_path: Option<String>,
        duration_ms: u64,
        source: impl Into<String>,
        deepgram_mode: Option<String>,
    ) -> HistoryEntry {
        let words = self.final_text.split_whitespace().count();
        let transcription_latency_ms = self
            .timings
            .whisper_ms
            .into_iter()
            .chain(self.timings.deepgram_ms)
            .max();
        let sanitizer_latency_ms = self.timings.sanitizer_ms;
        let total = if self.timings.total_ms > 0 {
            self.timings.total_ms
        } else {
            transcription_latency_ms.unwrap_or(0) + sanitizer_latency_ms.unwrap_or(0)
        };
        let engine = if self.history_engine_label.is_empty() {
            format!("{:?}", self.mode)
        } else {
            self.history_engine_label.clone()
        };
        HistoryEntry {
            id: self.id.clone(),
            date: date.into(),
            words,
            engine,
            text: self.final_text.clone(),
            audio_path,
            evaluation: None,
            duration_ms,
            source: source.into(),
            latency_ms: total,
            throughput: 0.0,
            transcription_latency_ms,
            sanitizer_latency_ms,
            transcription_throughput: None,
            sanitizer_throughput: None,
            realtime_factor: None,
            deepgram_mode,
            total_tokens: Some((words as f64 * 1.3).round() as usize),
            is_error: Some(self.is_error),
            error_message: self.error_message.clone(),
            debug_info: self.debug_info.clone(),
            mode: Some(self.mode.as_str().to_string()),
            model: self.models_used.first().cloned(),
            stages: if self.stages.is_empty() {
                None
            } else {
                Some(
                    self.stages
                        .iter()
                        .map(|s| s.stage.as_str().to_string())
                        .collect::<Vec<_>>()
                        .join(","),
                )
            },
            used_fallback: Some(self.used_fallback),
            fallback_reason: self.fallback_reason.clone(),
            content_type: Some(self.content_type.as_str().to_string()),
            whisper_text: self.whisper_text.clone(),
            sanitizer_text: None,
            gemini_text: self.gemini_text.clone(),
            warnings: if self.warnings.is_empty() {
                None
            } else {
                Some(self.warnings.iter().map(|w| w.message.clone()).collect())
            },
            audio_prepare_ms: None,
            base64_ms: None,
            whisper_ms: self.timings.whisper_ms,
            sanitizer_ms: self.timings.sanitizer_ms,
            files_upload_ms: None,
            files_poll_ms: None,
            files_poll_count: None,
            gemini_generate_ms: self.timings.gemini_ms,
            gemini_delete_ms: None,
            strict_literals_ms: None,
            clipboard_ms: None,
            total_pipeline_ms: Some(total),
            gemini_transport: None,
        }
    }
}

// ─── Pure helpers (testable without network) ────────────────────────────────

/// Sentinel the live sanitizer may return (mirrors `groq::FALLBACK_RETRY_SENTINEL`).
pub const FALLBACK_RETRY_SENTINEL: &str = "[FALLBACK_RETRY]";

/// Strips common LLM preambles / headers the sanitizer is instructed not to emit.
/// Used by future finalize logic and covered by unit tests now.
pub fn strip_sanitizer_artifacts(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let mut lines: Vec<&str> = trimmed.lines().collect();
    // Drop leading markdown headings / labels that models sometimes leak.
    while let Some(first) = lines.first() {
        let t = first.trim();
        let lower = t.to_ascii_lowercase();
        let is_header = t.starts_with('#')
            || lower.starts_with("here is")
            || lower.starts_with("here's")
            || lower.starts_with("aqui está")
            || lower.starts_with("aqui esta")
            || lower.starts_with("texto final")
            || lower.starts_with("transcrição")
            || lower.starts_with("transcricao")
            || lower.starts_with("cleaned:")
            || lower.starts_with("output:");
        if is_header && lines.len() > 1 {
            lines.remove(0);
        } else {
            break;
        }
    }

    // Drop trailing glossary / notes blocks.
    if let Some(idx) = lines.iter().position(|l| {
        let lower = l.trim().to_ascii_lowercase();
        lower.starts_with("glossário")
            || lower.starts_with("glossario")
            || lower.starts_with("nota:")
            || lower.starts_with("notes:")
            || lower.starts_with("---")
    }) {
        if idx > 0 {
            lines.truncate(idx);
        }
    }

    lines
        .join("\n")
        .trim()
        .trim_matches(|c| c == '"' || c == '\u{201c}' || c == '\u{201d}')
        .to_string()
}

/// Picks the best raw acoustic when sanitizer is off or failed (mirrors
/// `audio::pick_raw_acoustic` policy for contract tests).
pub fn pick_raw_acoustic(whisper_text: &str, deepgram_text: &str) -> String {
    let w = whisper_text.trim();
    let d = deepgram_text.trim();
    match (w.is_empty(), d.is_empty()) {
        (true, true) => String::new(),
        (false, true) => w.to_string(),
        (true, false) => d.to_string(),
        (false, false) => {
            if w.eq_ignore_ascii_case(d) {
                return w.to_string();
            }
            let wl = w.chars().count();
            let dl = d.chars().count();
            if wl > dl.saturating_add(8) {
                w.to_string()
            } else {
                d.to_string()
            }
        }
    }
}

/// When finalized text is blank, prefer Deepgram raw then Whisper raw.
pub fn coalesce_empty_final(
    finalized: String,
    whisper_text: &str,
    deepgram_text: &str,
) -> (String, bool, Option<String>) {
    if !finalized.trim().is_empty() {
        return (finalized, false, None);
    }
    if !deepgram_text.trim().is_empty() {
        return (
            deepgram_text.trim().to_string(),
            true,
            Some("sanitizer_empty_prefer_deepgram".into()),
        );
    }
    if !whisper_text.trim().is_empty() {
        return (
            whisper_text.trim().to_string(),
            true,
            Some("sanitizer_empty_prefer_whisper".into()),
        );
    }
    (finalized, false, None)
}

/// Resolves sanitizer output into deliverable text + fallback metadata.
pub fn resolve_sanitizer_output(
    sanitizer_raw: Option<&str>,
    whisper_text: &str,
    deepgram_text: &str,
) -> (String, bool, Option<String>, Vec<PipelineWarning>) {
    let mut warnings = Vec::new();
    let Some(raw) = sanitizer_raw else {
        let picked = pick_raw_acoustic(whisper_text, deepgram_text);
        warnings.push(PipelineWarning::new(
            PipelineStage::Sanitizer,
            "sanitizer_skipped_or_failed",
            "Validador indisponível; usando texto acústico.",
        ));
        return (picked, true, Some("sanitizer_missing".into()), warnings);
    };

    let cleaned = strip_sanitizer_artifacts(raw);
    if cleaned.trim() == FALLBACK_RETRY_SENTINEL {
        let picked = pick_raw_acoustic(whisper_text, deepgram_text);
        warnings.push(PipelineWarning::new(
            PipelineStage::Sanitizer,
            "fallback_retry_sentinel",
            "Validador pediu fallback para o texto acústico.",
        ));
        return (
            picked,
            true,
            Some("fallback_retry_sentinel".into()),
            warnings,
        );
    }
    if cleaned.trim().is_empty() {
        let (text, used, reason) = coalesce_empty_final(String::new(), whisper_text, deepgram_text);
        warnings.push(PipelineWarning::new(
            PipelineStage::Sanitizer,
            "sanitizer_empty",
            "Validador devolveu texto vazio.",
        ));
        return (text, used, reason, warnings);
    }

    // Header-only cleanup already applied; if strip removed everything meaningful
    // but original had content that was only headers, fall back.
    if cleaned.trim().is_empty() && !raw.trim().is_empty() {
        let picked = pick_raw_acoustic(whisper_text, deepgram_text);
        warnings.push(PipelineWarning::new(
            PipelineStage::Sanitizer,
            "sanitizer_header_only",
            "Resposta do validador continha só cabeçalhos.",
        ));
        return (picked, true, Some("sanitizer_header_only".into()), warnings);
    }

    (cleaned, false, None, warnings)
}

/// Dual-engine partial failure: keep the surviving acoustic stream.
pub fn resolve_dual_partial(
    whisper: Result<String, String>,
    deepgram: Result<String, String>,
) -> (
    String,
    String,
    bool,
    bool,
    Vec<PipelineWarning>,
    Option<String>,
) {
    let mut warnings = Vec::new();
    match (whisper, deepgram) {
        (Ok(w), Ok(d)) => (w, d, true, true, warnings, None),
        (Ok(w), Err(e)) => {
            warnings.push(PipelineWarning::new(
                PipelineStage::Deepgram,
                "dual_deepgram_failed",
                format!("Deepgram falhou no modo duplo: {e}"),
            ));
            (w, String::new(), false, false, warnings, None)
        }
        (Err(e), Ok(d)) => {
            warnings.push(PipelineWarning::new(
                PipelineStage::Whisper,
                "dual_whisper_failed",
                format!("Whisper falhou no modo duplo: {e}"),
            ));
            (String::new(), d, false, true, warnings, None)
        }
        (Err(we), Err(de)) => {
            let msg = format!("Ambos os motores falharam:\n• Whisper: {we}\n• Deepgram: {de}");
            warnings.push(PipelineWarning::new(
                PipelineStage::Finalize,
                "dual_both_failed",
                msg.clone(),
            ));
            (
                String::new(),
                String::new(),
                false,
                false,
                warnings,
                Some(msg),
            )
        }
    }
}

// ─── Legacy settings snapshot (compat, no drop) ─────────────────────────────

/// On-disk-shaped preferences that must round-trip when new mode fields appear.
/// Mirrors the fields users already have; optional mode/content default safely.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LegacySettingsSnapshot {
    #[serde(default)]
    pub engine: Option<TranscriptionEngine>,
    #[serde(default)]
    pub sanitizer: Option<SanitizerModel>,
    #[serde(default)]
    pub dual_engine: bool,
    #[serde(default)]
    pub deepgram_mode: DeepgramMode,
    #[serde(default = "default_true")]
    pub sanitizer_enabled: bool,
    #[serde(default)]
    pub reasoning_enabled: bool,
    #[serde(default = "default_effort")]
    pub reasoning_effort: String,
    #[serde(default)]
    pub custom_words: Vec<String>,
    /// Future field: absent on old installs → default UltraFast mapping via from_legacy.
    #[serde(default)]
    pub transcription_mode: Option<TranscriptionMode>,
    #[serde(default)]
    pub content_type: ContentType,
}

fn default_true() -> bool {
    true
}

fn default_effort() -> String {
    "medium".to_string()
}

impl Default for LegacySettingsSnapshot {
    fn default() -> Self {
        Self {
            engine: Some(TranscriptionEngine::GroqWhisper),
            sanitizer: Some(SanitizerModel::Llama70b),
            dual_engine: false,
            deepgram_mode: DeepgramMode::Batch,
            sanitizer_enabled: true,
            reasoning_enabled: false,
            reasoning_effort: "medium".into(),
            custom_words: Vec::new(),
            transcription_mode: None,
            content_type: ContentType::Auto,
        }
    }
}

impl LegacySettingsSnapshot {
    /// Effective mode without discarding legacy engine/dual.
    pub fn effective_mode(&self) -> TranscriptionMode {
        self.transcription_mode.unwrap_or_else(|| {
            TranscriptionMode::from_legacy(self.engine.unwrap_or_default(), self.dual_engine)
        })
    }

    /// Ensures serialize → deserialize keeps every legacy field the user had.
    pub fn round_trip_json(&self) -> Result<Self, String> {
        let s = serde_json::to_string(self).map_err(|e| e.to_string())?;
        serde_json::from_str(&s).map_err(|e| e.to_string())
    }
}

// ─── Local mocks (no network) ───────────────────────────────────────────────

/// In-memory fake providers for unit tests.
#[derive(Debug, Clone, Default)]
pub struct MockProviders {
    pub whisper: Option<Result<String, String>>,
    pub deepgram: Option<Result<String, String>>,
    pub sanitizer: Option<Result<String, String>>,
    pub gemini: Option<Result<String, String>>,
}

/// Runs a **local** mock pipeline for the given mode. Never touches the network.
pub fn run_mock_pipeline(req: &PipelineRequest, mocks: &MockProviders) -> PipelineResult {
    let mut result = PipelineResult::success(&req.id, req.mode, "");
    result.content_type = req.content_type;
    result.history_engine_label = match (req.dual_engine, req.engine) {
        (true, _) => "Groq+Deepgram".into(),
        (false, TranscriptionEngine::DeepgramNova3) => "DeepgramNova3".into(),
        (false, TranscriptionEngine::GroqWhisper) => "GroqWhisper".into(),
        (false, TranscriptionEngine::GeminiMultimodal) => "GeminiMultimodal".into(),
    };

    let mut whisper_text = String::new();
    let mut deepgram_text = String::new();

    // Acoustic stages depending on mode + legacy dual.
    let want_whisper = req.mode.uses_whisper()
        || req.dual_engine
        || req.engine == TranscriptionEngine::GroqWhisper;
    let want_deepgram = req.dual_engine || req.engine == TranscriptionEngine::DeepgramNova3;
    // Future modes that ignore Deepgram still allow legacy dual path in mock
    // when dual_engine is true so we never "lose" dual in tests.

    if want_whisper {
        match mocks.whisper.clone().unwrap_or_else(|| Ok(String::new())) {
            Ok(t) => {
                whisper_text = t.clone();
                result.whisper_text = Some(t.clone());
                result.stages.push(PipelineStageResult {
                    stage: PipelineStage::Whisper,
                    ok: true,
                    text: Some(t),
                    model: Some("whisper-large-v3-turbo".into()),
                    latency_ms: 10,
                    error: None,
                });
                result.models_used.push("whisper-large-v3-turbo".into());
                result.timings.whisper_ms = Some(10);
            }
            Err(e) => {
                result.stages.push(PipelineStageResult {
                    stage: PipelineStage::Whisper,
                    ok: false,
                    text: None,
                    model: Some("whisper-large-v3-turbo".into()),
                    latency_ms: 10,
                    error: Some(e.clone()),
                });
                if !want_deepgram && !req.mode.uses_gemini_audio() {
                    result.is_error = true;
                    result.error_message = Some(e);
                    result.timings.recompute_total();
                    return result;
                }
            }
        }
    }

    if want_deepgram {
        match mocks.deepgram.clone().unwrap_or_else(|| Ok(String::new())) {
            Ok(t) => {
                deepgram_text = t.clone();
                result.deepgram_text = Some(t.clone());
                result.stages.push(PipelineStageResult {
                    stage: PipelineStage::Deepgram,
                    ok: true,
                    text: Some(t),
                    model: Some("nova-3".into()),
                    latency_ms: 12,
                    error: None,
                });
                result.models_used.push("nova-3".into());
                result.timings.deepgram_ms = Some(12);
            }
            Err(e) => {
                result.stages.push(PipelineStageResult {
                    stage: PipelineStage::Deepgram,
                    ok: false,
                    text: None,
                    model: Some("nova-3".into()),
                    latency_ms: 12,
                    error: Some(e.clone()),
                });
                if whisper_text.is_empty() && !req.mode.uses_gemini_audio() {
                    result.is_error = true;
                    result.error_message = Some(e);
                    result.timings.recompute_total();
                    return result;
                }
                result.warnings.push(PipelineWarning::new(
                    PipelineStage::Deepgram,
                    "deepgram_failed",
                    e,
                ));
            }
        }
    }

    // Dual partial resolution when both were requested.
    if req.dual_engine {
        let w = if result
            .stages
            .iter()
            .any(|s| s.stage == PipelineStage::Whisper && s.ok)
        {
            Ok(whisper_text.clone())
        } else if result
            .stages
            .iter()
            .any(|s| s.stage == PipelineStage::Whisper && !s.ok)
        {
            Err(result
                .stages
                .iter()
                .find(|s| s.stage == PipelineStage::Whisper)
                .and_then(|s| s.error.clone())
                .unwrap_or_else(|| "whisper failed".into()))
        } else {
            Ok(String::new())
        };
        let d = if result
            .stages
            .iter()
            .any(|s| s.stage == PipelineStage::Deepgram && s.ok)
        {
            Ok(deepgram_text.clone())
        } else if result
            .stages
            .iter()
            .any(|s| s.stage == PipelineStage::Deepgram && !s.ok)
        {
            Err(result
                .stages
                .iter()
                .find(|s| s.stage == PipelineStage::Deepgram)
                .and_then(|s| s.error.clone())
                .unwrap_or_else(|| "deepgram failed".into()))
        } else {
            Ok(String::new())
        };
        let (w2, d2, _eff, _dg, warns, hard) = resolve_dual_partial(w, d);
        whisper_text = w2;
        deepgram_text = d2;
        result.warnings.extend(warns);
        if let Some(msg) = hard {
            result.is_error = true;
            result.error_message = Some(msg);
            result.timings.recompute_total();
            return result;
        }
    }

    if whisper_text.trim().is_empty()
        && deepgram_text.trim().is_empty()
        && !req.mode.uses_gemini_audio()
    {
        result.is_error = true;
        result.error_message = Some("Nenhum texto detectado na gravação.".into());
        result.timings.recompute_total();
        return result;
    }

    // Sanitizer: UltraPrecise always; UltraFast when legacy sanitizer_enabled.
    // FastAccurate / Precise skip sanitizer in the mock (Gemini path owns refine).
    let run_sanitizer = match req.mode {
        TranscriptionMode::UltraPrecise => true,
        TranscriptionMode::UltraFast => req.sanitizer_enabled,
        TranscriptionMode::FastAccurate | TranscriptionMode::Precise => false,
    };

    if run_sanitizer {
        let san_res = mocks
            .sanitizer
            .clone()
            .unwrap_or_else(|| Ok(pick_raw_acoustic(&whisper_text, &deepgram_text)));
        match san_res {
            Ok(raw) => {
                let (text, used_fb, reason, warns) =
                    resolve_sanitizer_output(Some(&raw), &whisper_text, &deepgram_text);
                result.sanitizer_text = Some(raw);
                result.warnings.extend(warns);
                result.used_fallback = used_fb;
                result.fallback_reason = reason;
                result.final_text = text;
                result.stages.push(PipelineStageResult {
                    stage: PipelineStage::Sanitizer,
                    ok: true,
                    text: Some(result.final_text.clone()),
                    model: Some(req.sanitizer.api_model_id().into()),
                    latency_ms: 20,
                    error: None,
                });
                result.models_used.push(req.sanitizer.api_model_id().into());
                result.timings.sanitizer_ms = Some(20);
            }
            Err(e) => {
                let (text, _, reason, warns) =
                    resolve_sanitizer_output(None, &whisper_text, &deepgram_text);
                result.warnings.extend(warns);
                result.warnings.push(PipelineWarning::new(
                    PipelineStage::Sanitizer,
                    "sanitizer_error",
                    e,
                ));
                result.used_fallback = true;
                result.fallback_reason = reason.or(Some("sanitizer_error".into()));
                result.final_text = text;
                result.stages.push(PipelineStageResult {
                    stage: PipelineStage::Sanitizer,
                    ok: false,
                    text: Some(result.final_text.clone()),
                    model: Some(req.sanitizer.api_model_id().into()),
                    latency_ms: 20,
                    error: result.fallback_reason.clone(),
                });
                result.timings.sanitizer_ms = Some(20);
            }
        }
    } else {
        result.final_text = pick_raw_acoustic(&whisper_text, &deepgram_text);
    }

    // Gemini audio stage (future modes).
    if req.mode.uses_gemini_audio() {
        match mocks.gemini.clone().unwrap_or_else(|| Ok(String::new())) {
            Ok(t) if !t.trim().is_empty() => {
                result.gemini_text = Some(t.clone());
                // Precise / UltraPrecise: Gemini refines; FastAccurate: Gemini is source.
                if result.final_text.trim().is_empty()
                    || req.mode == TranscriptionMode::FastAccurate
                {
                    result.final_text = t.clone();
                } else if req.mode == TranscriptionMode::Precise
                    || req.mode == TranscriptionMode::UltraPrecise
                {
                    // Prefer non-empty Gemini refine when provided by mock.
                    result.final_text = t.clone();
                }
                result.stages.push(PipelineStageResult {
                    stage: PipelineStage::GeminiAudio,
                    ok: true,
                    text: Some(t),
                    model: Some("gemini-3.5-flash".into()),
                    latency_ms: 40,
                    error: None,
                });
                result.models_used.push("gemini-3.5-flash".into());
                result.timings.gemini_ms = Some(40);
            }
            Ok(_) => {
                result.warnings.push(PipelineWarning::new(
                    PipelineStage::GeminiAudio,
                    "gemini_empty",
                    "Gemini devolveu texto vazio; mantendo etapa anterior.",
                ));
                result.used_fallback = true;
                result.fallback_reason = Some("gemini_empty".into());
                if result.final_text.trim().is_empty() {
                    result.final_text = pick_raw_acoustic(&whisper_text, &deepgram_text);
                }
            }
            Err(e) => {
                result.warnings.push(PipelineWarning::new(
                    PipelineStage::GeminiAudio,
                    "gemini_failed",
                    e,
                ));
                result.used_fallback = true;
                result.fallback_reason = Some("gemini_failed".into());
                if result.final_text.trim().is_empty() {
                    result.final_text = pick_raw_acoustic(&whisper_text, &deepgram_text);
                }
                if result.final_text.trim().is_empty() {
                    result.is_error = true;
                    result.error_message =
                        Some("Falha no Gemini e sem texto acústico de reserva.".into());
                }
            }
        }
    }

    if result.final_text.trim().is_empty() && !result.is_error {
        result.is_error = true;
        result.error_message = Some("Nenhum texto detectado na gravação.".into());
    }

    result.timings.recompute_total();
    result
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::TranscriptionEngine;

    #[test]
    fn mode_serde_roundtrip_and_defaults() {
        assert_eq!(TranscriptionMode::default(), TranscriptionMode::UltraFast);
        for mode in [
            TranscriptionMode::UltraFast,
            TranscriptionMode::FastAccurate,
            TranscriptionMode::Precise,
            TranscriptionMode::UltraPrecise,
        ] {
            let json = serde_json::to_string(&mode).unwrap();
            let back: TranscriptionMode = serde_json::from_str(&json).unwrap();
            assert_eq!(back, mode);
            assert_eq!(
                serde_json::from_str::<TranscriptionMode>(&format!("\"{}\"", mode.as_str()))
                    .unwrap(),
                mode
            );
        }
    }

    #[test]
    fn content_type_serde_and_default() {
        assert_eq!(ContentType::default(), ContentType::Auto);
        let json = r#""programming""#;
        assert_eq!(
            serde_json::from_str::<ContentType>(json).unwrap(),
            ContentType::Programming
        );
        for ct in [
            ContentType::Auto,
            ContentType::Programming,
            ContentType::GeneralSpeech,
            ContentType::Study,
        ] {
            let s = serde_json::to_string(&ct).unwrap();
            let back: ContentType = serde_json::from_str(&s).unwrap();
            assert_eq!(back, ct);
        }
    }

    #[test]
    fn unknown_mode_json_fails_closed() {
        let err = serde_json::from_str::<TranscriptionMode>(r#""teleport""#);
        assert!(err.is_err());
    }

    #[test]
    fn unknown_content_type_fails_closed() {
        let err = serde_json::from_str::<ContentType>(r#""podcast""#);
        assert!(err.is_err());
    }

    #[test]
    fn legacy_settings_without_mode_field_still_load() {
        let old = r#"{
            "engine": "groq-whisper",
            "sanitizer": "llama-70b",
            "dual_engine": true,
            "deepgram_mode": "streaming_final",
            "sanitizer_enabled": true,
            "reasoning_enabled": false,
            "reasoning_effort": "medium",
            "custom_words": ["Haumea", "Tokio"]
        }"#;
        let snap: LegacySettingsSnapshot = serde_json::from_str(old).unwrap();
        assert_eq!(snap.engine, Some(TranscriptionEngine::GroqWhisper));
        assert_eq!(snap.sanitizer, Some(SanitizerModel::Llama70b));
        assert!(snap.dual_engine);
        assert_eq!(snap.deepgram_mode, DeepgramMode::StreamingFinal);
        assert_eq!(snap.custom_words, vec!["Haumea", "Tokio"]);
        assert!(snap.transcription_mode.is_none());
        assert_eq!(snap.effective_mode(), TranscriptionMode::Precise);
        // Round-trip keeps vocabulary and dual.
        let back = snap.round_trip_json().unwrap();
        assert_eq!(back, snap);
    }

    #[test]
    fn legacy_settings_empty_object_defaults_safe() {
        let snap: LegacySettingsSnapshot = serde_json::from_str("{}").unwrap();
        assert!(!snap.dual_engine);
        assert!(snap.sanitizer_enabled);
        assert_eq!(snap.deepgram_mode, DeepgramMode::Batch);
        assert_eq!(snap.reasoning_effort, "medium");
        assert_eq!(snap.content_type, ContentType::Auto);
    }

    #[test]
    fn from_legacy_mapping_preserves_intent() {
        assert_eq!(
            TranscriptionMode::from_legacy(TranscriptionEngine::GroqWhisper, false),
            TranscriptionMode::UltraFast
        );
        assert_eq!(
            TranscriptionMode::from_legacy(TranscriptionEngine::DeepgramNova3, false),
            TranscriptionMode::UltraFast
        );
        assert_eq!(
            TranscriptionMode::from_legacy(TranscriptionEngine::GroqWhisper, true),
            TranscriptionMode::Precise
        );
        assert_eq!(
            TranscriptionMode::from_legacy(TranscriptionEngine::GeminiMultimodal, false),
            TranscriptionMode::FastAccurate
        );
    }

    #[test]
    fn pipeline_request_from_legacy_keeps_keys_of_config() {
        let req = PipelineRequest::from_legacy_settings(
            "1",
            vec![1, 2, 3],
            "audio.wav",
            "audio/wav",
            TranscriptionEngine::DeepgramNova3,
            true,
            DeepgramMode::StreamingFinal,
            SanitizerModel::GptOss120b,
            true,
            true,
            "high",
            "system",
            vec!["Haumea".into()],
            "mic",
            1500,
            true,
        );
        assert_eq!(req.engine, TranscriptionEngine::DeepgramNova3);
        assert!(req.dual_engine);
        assert_eq!(req.deepgram_mode, DeepgramMode::StreamingFinal);
        assert_eq!(req.sanitizer, SanitizerModel::GptOss120b);
        assert_eq!(req.reasoning_effort, "high");
        assert_eq!(req.custom_words, vec!["Haumea"]);
        assert_eq!(req.mode, TranscriptionMode::Precise);
        // audio_bytes skipped in serde but present in memory
        assert_eq!(req.audio_bytes.len(), 3);
        let json = serde_json::to_string(&req).unwrap();
        assert!(!json.contains("audio_bytes"));
    }

    #[test]
    fn pipeline_result_serde_roundtrip() {
        let mut r = PipelineResult::success("42", TranscriptionMode::UltraPrecise, "olá mundo");
        r.whisper_text = Some("ola mundo".into());
        r.warnings
            .push(PipelineWarning::new(PipelineStage::Sanitizer, "x", "y"));
        r.timings.whisper_ms = Some(11);
        r.timings.sanitizer_ms = Some(22);
        r.timings.recompute_total();
        r.models_used = vec![
            "whisper-large-v3-turbo".into(),
            "openai/gpt-oss-120b".into(),
        ];
        let json = serde_json::to_string_pretty(&r).unwrap();
        let back: PipelineResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.final_text, "olá mundo");
        assert_eq!(back.timings.total_ms, 33);
        assert_eq!(back.warnings.len(), 1);
        assert_eq!(back.mode, TranscriptionMode::UltraPrecise);
    }

    #[test]
    fn strip_sanitizer_header_and_glossary() {
        let raw = "# Texto final\nHere is the cleaned text:\nOlá, Haumea.\n\nGlossário:\n- foo";
        let cleaned = strip_sanitizer_artifacts(raw);
        assert!(cleaned.contains("Olá, Haumea"));
        assert!(!cleaned.to_ascii_lowercase().contains("gloss"));
        assert!(!cleaned.contains("Here is"));
    }

    #[test]
    fn sanitizer_empty_falls_back_to_deepgram() {
        let (text, used, reason, warns) =
            resolve_sanitizer_output(Some("   "), "whisper only", "deepgram wins");
        assert_eq!(text, "deepgram wins");
        assert!(used);
        assert_eq!(reason.as_deref(), Some("sanitizer_empty_prefer_deepgram"));
        assert!(!warns.is_empty());
    }

    #[test]
    fn sanitizer_sentinel_falls_back_to_raw() {
        let (text, used, reason, _) =
            resolve_sanitizer_output(Some(FALLBACK_RETRY_SENTINEL), "w text", "");
        assert_eq!(text, "w text");
        assert!(used);
        assert_eq!(reason.as_deref(), Some("fallback_retry_sentinel"));
    }

    #[test]
    fn sanitizer_missing_uses_pick_raw() {
        let (text, used, _, _) =
            resolve_sanitizer_output(None, "short", "much longer deepgram text here");
        assert_eq!(text, "much longer deepgram text here");
        assert!(used);
    }

    #[test]
    fn empty_stt_both_sides() {
        let (w, d, _, _, _, hard) = resolve_dual_partial(Ok(String::new()), Ok(String::new()));
        assert!(w.is_empty() && d.is_empty());
        assert!(hard.is_none());
        let (text, _, _) = coalesce_empty_final(String::new(), "", "");
        assert!(text.is_empty());
    }

    #[test]
    fn dual_partial_whisper_ok_deepgram_fail() {
        let (w, d, eff, dg, warns, hard) =
            resolve_dual_partial(Ok("hello".into()), Err("401".into()));
        assert_eq!(w, "hello");
        assert!(d.is_empty());
        assert!(!eff && !dg);
        assert!(hard.is_none());
        assert_eq!(warns[0].code, "dual_deepgram_failed");
    }

    #[test]
    fn dual_both_fail_hard_error() {
        let (_, _, _, _, _, hard) = resolve_dual_partial(Err("down".into()), Err("down2".into()));
        assert!(hard.unwrap().contains("Whisper"));
    }

    #[test]
    fn mock_ultra_fast_whisper_only() {
        let req = PipelineRequest::from_legacy_settings(
            "m1",
            vec![],
            "a.wav",
            "audio/wav",
            TranscriptionEngine::GroqWhisper,
            false,
            DeepgramMode::Batch,
            SanitizerModel::Llama70b,
            false,
            false,
            "medium",
            "",
            vec![],
            "mic",
            1000,
            true,
        );
        assert_eq!(req.mode, TranscriptionMode::UltraFast);
        let mocks = MockProviders {
            whisper: Some(Ok("texto do whisper".into())),
            ..Default::default()
        };
        let out = run_mock_pipeline(&req, &mocks);
        assert!(!out.is_error);
        assert_eq!(out.final_text, "texto do whisper");
        assert!(out.deepgram_text.is_none() || out.deepgram_text.as_deref() == Some(""));
    }

    #[test]
    fn mock_sanitizer_empty_fallback() {
        let mut req = PipelineRequest::from_legacy_settings(
            "m2",
            vec![],
            "a.wav",
            "audio/wav",
            TranscriptionEngine::GroqWhisper,
            true,
            DeepgramMode::Batch,
            SanitizerModel::GptOss120b,
            true,
            false,
            "medium",
            "",
            vec![],
            "mic",
            1000,
            true,
        );
        req.mode = TranscriptionMode::UltraFast;
        let mocks = MockProviders {
            whisper: Some(Ok("whisper raw".into())),
            deepgram: Some(Ok("deepgram raw longer".into())),
            sanitizer: Some(Ok(String::new())),
            ..Default::default()
        };
        let out = run_mock_pipeline(&req, &mocks);
        assert!(!out.is_error);
        assert!(out.used_fallback);
        assert_eq!(out.final_text, "deepgram raw longer");
    }

    #[test]
    fn mock_partial_dual_still_delivers() {
        let req = PipelineRequest::from_legacy_settings(
            "m3",
            vec![],
            "a.wav",
            "audio/wav",
            TranscriptionEngine::GroqWhisper,
            true,
            DeepgramMode::Batch,
            SanitizerModel::Llama70b,
            false,
            false,
            "medium",
            "",
            vec![],
            "mic",
            500,
            true,
        );
        let mocks = MockProviders {
            whisper: Some(Err("timeout".into())),
            deepgram: Some(Ok("só deepgram".into())),
            ..Default::default()
        };
        let out = run_mock_pipeline(&req, &mocks);
        assert!(!out.is_error);
        assert_eq!(out.final_text, "só deepgram");
        assert!(out.warnings.iter().any(|w| w.code == "dual_whisper_failed"));
    }

    #[test]
    fn mock_empty_stt_is_error() {
        let req = PipelineRequest::from_legacy_settings(
            "m4",
            vec![],
            "a.wav",
            "audio/wav",
            TranscriptionEngine::GroqWhisper,
            false,
            DeepgramMode::Batch,
            SanitizerModel::Llama70b,
            false,
            false,
            "medium",
            "",
            vec![],
            "mic",
            100,
            true,
        );
        let mocks = MockProviders {
            whisper: Some(Ok("   ".into())),
            ..Default::default()
        };
        let out = run_mock_pipeline(&req, &mocks);
        assert!(out.is_error);
        assert!(out.error_message.unwrap().contains("Nenhum texto"));
    }

    #[test]
    fn mock_header_only_sanitizer() {
        let mut req = PipelineRequest::from_legacy_settings(
            "m5",
            vec![],
            "a.wav",
            "audio/wav",
            TranscriptionEngine::GroqWhisper,
            false,
            DeepgramMode::Batch,
            SanitizerModel::Llama70b,
            true,
            false,
            "medium",
            "",
            vec![],
            "mic",
            100,
            true,
        );
        req.mode = TranscriptionMode::UltraFast;
        let mocks = MockProviders {
            whisper: Some(Ok("fala real".into())),
            sanitizer: Some(Ok("## Resumo\nHere is the transcription:".into())),
            ..Default::default()
        };
        let out = run_mock_pipeline(&req, &mocks);
        // After strip, remaining may be empty or residual — must not crash.
        assert!(!out.is_error);
        assert!(!out.final_text.trim().is_empty());
    }

    #[test]
    fn history_entry_from_pipeline_result() {
        let mut r = PipelineResult::success("99", TranscriptionMode::UltraFast, "duas palavras");
        r.history_engine_label = "GroqWhisper".into();
        r.timings.whisper_ms = Some(50);
        r.timings.sanitizer_ms = Some(30);
        r.timings.recompute_total();
        let entry = r.to_history_entry(
            "2026-07-18 12:00",
            Some("C:/a/99.wav".into()),
            2000,
            "mic",
            None,
        );
        assert_eq!(entry.id, "99");
        assert_eq!(entry.words, 2);
        assert_eq!(entry.engine, "GroqWhisper");
        assert_eq!(entry.latency_ms, 80);
        assert_eq!(entry.is_error, Some(false));
        assert_eq!(entry.audio_path.as_deref(), Some("C:/a/99.wav"));

        let err = PipelineResult::hard_error("100", TranscriptionMode::UltraFast, "falhou");
        let e2 = err.to_history_entry("2026-07-18 12:01", None, 0, "file", Some("batch".into()));
        assert_eq!(e2.is_error, Some(true));
        assert_eq!(e2.error_message.as_deref(), Some("falhou"));
        assert_eq!(e2.deepgram_mode.as_deref(), Some("batch"));
    }

    #[test]
    fn mode_stage_flags() {
        assert!(TranscriptionMode::UltraFast.uses_whisper());
        assert!(!TranscriptionMode::UltraFast.uses_gemini_audio());
        assert!(!TranscriptionMode::UltraFast.uses_sanitizer());

        assert!(!TranscriptionMode::FastAccurate.uses_whisper());
        assert!(TranscriptionMode::FastAccurate.uses_gemini_audio());

        assert!(TranscriptionMode::Precise.uses_whisper());
        assert!(TranscriptionMode::Precise.uses_gemini_audio());
        assert!(!TranscriptionMode::Precise.uses_sanitizer());

        assert!(TranscriptionMode::UltraPrecise.uses_whisper());
        assert!(TranscriptionMode::UltraPrecise.uses_sanitizer());
        assert!(TranscriptionMode::UltraPrecise.uses_gemini_audio());
    }

    #[test]
    fn display_names_pt_are_product_copy() {
        assert_eq!(
            TranscriptionMode::UltraFast.display_name_pt(),
            "Ultrarrápido"
        );
        assert_eq!(
            TranscriptionMode::FastAccurate.display_name_pt(),
            "Rápido e preciso"
        );
        assert_eq!(TranscriptionMode::Precise.display_name_pt(), "Preciso");
        assert_eq!(
            TranscriptionMode::UltraPrecise.display_name_pt(),
            "Ultrapreciso"
        );
    }
}
