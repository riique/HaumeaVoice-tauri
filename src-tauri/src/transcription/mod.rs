//! Legacy transcription orchestration (Phase 02).
//!
//! Extracted from `audio.rs` so capture/WAV/clipboard stay separate from
//! engine selection, dual STT, sanitization, fallbacks and history metrics.
//! Behaviour matches the pre-extraction path; future modes will plug in here
//! without growing the capture module again.

pub mod fallback;
pub mod legacy;
pub mod modes;
pub mod pipeline;
pub mod telemetry;
pub mod types;

pub use fallback::{coalesce_empty_final, pick_raw_acoustic, single_engine_slots};
pub use legacy::{
    deepgram_from_live_or_posthoc, run_acoustic_file, run_acoustic_mic, run_dual_posthoc,
    transcribe_bytes,
};
pub use modes::{
    mode_failed_history, mode_result_to_history, run_product_mode, run_product_mode_with_duration,
    should_use_product_mode, ModePipelineResult,
};
pub use pipeline::{
    build_failed_entry, build_success_entry, emit_saved, run_sanitize, update_failed_entry,
};
pub use types::{AcousticOutcome, SanitizeOutcome};
