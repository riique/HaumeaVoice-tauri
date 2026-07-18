//! Shared types for the legacy transcription pipeline.

use crate::models::SanitizerDebug;

/// Result of Stage 1 (acoustic STT), before sanitization.
#[derive(Debug, Clone)]
pub struct AcousticOutcome {
    pub whisper_text: String,
    pub deepgram_text: String,
    /// True only when both engines succeeded in dual mode.
    pub effective_dual: bool,
    /// True when Deepgram actually produced a transcript (or was attempted
    /// successfully on the Deepgram-only path).
    pub deepgram_ran: bool,
}

/// Result of Stage 2 (sanitizer / pick_raw), ready for clipboard + history.
#[derive(Debug, Clone)]
pub struct SanitizeOutcome {
    pub final_text: String,
    pub debug_info: Option<SanitizerDebug>,
    pub sanitizer_latency_ms: u64,
    /// Max word count across acoustic streams (throughput numerator).
    pub raw_words: usize,
    pub warnings: Vec<String>,
    pub used_raw_fallback: bool,
    pub changed: bool,
    /// Resolved content type applied to the sanitizer prompt (never `auto`).
    pub content_type: crate::pipeline_contract::ContentType,
}
