//! Stage 2 sanitization and history entry assembly for the legacy pipeline.

use std::sync::Arc;

use crate::models::{AppState, HistoryEntry, TranscriptionEngine};
use crate::transcription::fallback::{coalesce_empty_final, pick_raw_acoustic};
use crate::transcription::telemetry::{
    acoustic_word_count, compute_realtime_factor, est_throughput, est_total_tokens,
    history_deepgram_mode, history_engine_label, log_latency,
};
use crate::transcription::types::SanitizeOutcome;

/// Runs the Groq sanitizer (or pick_raw) exactly as the pre-extraction path.
pub async fn run_sanitize(
    state: &Arc<AppState>,
    whisper_text: &str,
    deepgram_text: &str,
    dual_mode: bool,
) -> SanitizeOutcome {
    let sanitizer_key = {
        let guard = state.api_keys.read();
        guard.groq.clone().filter(|k| !k.trim().is_empty())
    };
    let (
        model_id,
        supports_reasoning,
        system_prompt,
        glossary_block,
        vocab_snapshot,
        reasoning_enabled,
        reasoning_effort,
    ) = {
        let sanitizer = *state.sanitizer.read();
        let vocab = state.vocabulary.read().clone();
        (
            sanitizer.api_model_id(),
            sanitizer.supports_reasoning(),
            state.system_prompt.read().clone(),
            crate::vocabulary::format_glossary_for_prompt(&vocab),
            vocab,
            *state.reasoning_enabled.read(),
            state.reasoning_effort.read().clone(),
        )
    };

    let system_prompt_to_use = if dual_mode {
        format!(
            "{}\n\n--- INSTRUÇÃO DE MOTOR DUPLO ---\nVocê recebeu duas transcrições acústicas brutas (Transcrição A e Transcrição B) do mesmo áudio. Compare-as, corrija falhas fonéticas, pontue de forma correta e mescle as informações de forma inteligente para produzir o melhor texto unificado.",
            system_prompt
        )
    } else {
        system_prompt
    };

    let raw_words = acoustic_word_count(whisper_text, deepgram_text);
    let mut debug_info = None;
    let mut warnings: Vec<String> = Vec::new();
    let mut used_raw_fallback = false;
    let mut changed = false;
    let start_sanitizer = std::time::Instant::now();

    // Content-type hint (auto heuristic resolves to a concrete type).
    let content_type = crate::sanitizer_json::resolve_content_type(
        *state.content_type.read(),
        &pick_raw_acoustic(whisper_text, deepgram_text),
    );
    let system_prompt_to_use = format!(
        "{}{}",
        system_prompt_to_use,
        crate::sanitizer_json::content_type_instruction(content_type)
    );

    let final_text = if !*state.sanitizer_enabled.read() {
        let picked = pick_raw_acoustic(whisper_text, deepgram_text);
        log::info!(
            "transcription: sanitizer disabled, using pick_raw ({} chars)",
            picked.len()
        );
        picked
    } else {
        match sanitizer_key {
            Some(key) => {
                let outcome = crate::groq::call_sanitizer_api(
                    whisper_text,
                    deepgram_text,
                    model_id,
                    &system_prompt_to_use,
                    &glossary_block,
                    &key,
                    reasoning_enabled,
                    &reasoning_effort,
                    supports_reasoning,
                )
                .await;

                debug_info = Some(outcome.debug);
                warnings.extend(outcome.warnings);
                changed = outcome.changed;
                used_raw_fallback = outcome.used_raw_fallback;
                match outcome.result {
                    Ok(sanitized)
                        if sanitized.trim() == crate::groq::FALLBACK_RETRY_SENTINEL
                            || outcome.used_raw_fallback
                            || sanitized.trim().is_empty() =>
                    {
                        log::warn!(
                            "transcription: sanitizer fallback to raw (sentinel/parse/empty)"
                        );
                        used_raw_fallback = true;
                        warnings.push("sanitizer_used_raw_fallback".into());
                        pick_raw_acoustic(whisper_text, deepgram_text)
                    }
                    Ok(sanitized) => {
                        log::info!(
                            "transcription: sanitizer structured text ({} chars, changed={})",
                            sanitized.len(),
                            changed
                        );
                        sanitized
                    }
                    Err(e) => {
                        log::error!("transcription: sanitizer failed: {e}; using pick_raw");
                        used_raw_fallback = true;
                        warnings.push(format!("sanitizer_error: {}", e));
                        pick_raw_acoustic(whisper_text, deepgram_text)
                    }
                }
            }
            None => {
                log::warn!(
                    "transcription: no sanitizer API key set for model, falling back to pick_raw"
                );
                used_raw_fallback = true;
                warnings.push("sanitizer_missing_api_key".into());
                pick_raw_acoustic(whisper_text, deepgram_text)
            }
        }
    };

    let final_text = coalesce_empty_final(final_text, whisper_text, deepgram_text);
    // Strict literals: deterministic alias→canonical only for unequivocal hits.
    let (final_text, strict_hits) =
        crate::vocabulary::apply_strict_literals(&final_text, &vocab_snapshot);
    if !strict_hits.is_empty() {
        log::info!(
            "transcription: strict literals applied ({})",
            strict_hits.join(", ")
        );
        warnings.push(format!("strict_literals:{}", strict_hits.len()));
    }
    let sanitizer_latency_ms = start_sanitizer.elapsed().as_millis() as u64;

    SanitizeOutcome {
        final_text,
        debug_info,
        sanitizer_latency_ms,
        raw_words,
        warnings,
        used_raw_fallback,
        changed,
        content_type,
    }
}

/// Builds a successful history entry (caller persists + emits).
pub fn build_success_entry(
    state: &AppState,
    id: String,
    date: String,
    audio_path: Option<String>,
    engine: TranscriptionEngine,
    whisper_text: &str,
    deepgram_text: &str,
    final_text: String,
    duration_ms: u64,
    source: &str,
    transcription_latency_ms: u64,
    dual_mode: bool,
    deepgram_ran: bool,
    sanitize: &SanitizeOutcome,
    log_context: &str,
) -> HistoryEntry {
    let words = final_text.split_whitespace().count();
    let transcription_throughput = est_throughput(sanitize.raw_words, transcription_latency_ms);
    let sanitizer_throughput = est_throughput(words, sanitize.sanitizer_latency_ms);
    let total_tokens = Some(est_total_tokens(words));
    let total_latency_ms = transcription_latency_ms + sanitize.sanitizer_latency_ms;
    let throughput = sanitizer_throughput.unwrap_or(0.0);
    let realtime_factor = compute_realtime_factor(transcription_latency_ms, duration_ms);
    let deepgram_mode = history_deepgram_mode(state, deepgram_ran);

    log_latency(
        transcription_latency_ms,
        sanitize.sanitizer_latency_ms,
        duration_ms,
        realtime_factor,
        deepgram_mode.as_deref(),
        dual_mode,
        log_context,
    );

    HistoryEntry {
        id,
        date,
        words,
        engine: history_engine_label(engine, dual_mode, whisper_text, deepgram_text),
        text: final_text.clone(),
        audio_path,
        evaluation: None,
        duration_ms,
        source: source.to_string(),
        latency_ms: total_latency_ms,
        throughput,
        transcription_latency_ms: Some(transcription_latency_ms),
        sanitizer_latency_ms: Some(sanitize.sanitizer_latency_ms),
        transcription_throughput,
        sanitizer_throughput,
        realtime_factor,
        deepgram_mode,
        total_tokens,
        is_error: Some(false),
        error_message: None,
        debug_info: sanitize.debug_info.clone(),
        mode: None,
        model: None,
        stages: None,
        used_fallback: Some(sanitize.used_raw_fallback),
        fallback_reason: if sanitize.used_raw_fallback {
            Some("sanitizer_raw_fallback".into())
        } else {
            None
        },
        content_type: Some(sanitize.content_type.as_str().to_string()),
        whisper_text: if whisper_text.is_empty() {
            None
        } else {
            Some(whisper_text.to_string())
        },
        sanitizer_text: if sanitize.used_raw_fallback {
            None
        } else {
            Some(final_text)
        },
        gemini_text: None,
        warnings: if sanitize.warnings.is_empty() {
            None
        } else {
            Some(sanitize.warnings.clone())
        },
        audio_prepare_ms: None,
        base64_ms: None,
        whisper_ms: None,
        sanitizer_ms: Some(sanitize.sanitizer_latency_ms),
        files_upload_ms: None,
        files_poll_ms: None,
        files_poll_count: None,
        gemini_generate_ms: None,
        gemini_delete_ms: None,
        strict_literals_ms: None,
        clipboard_ms: None,
        total_pipeline_ms: Some(total_latency_ms),
        gemini_transport: None,
    }
}

/// Builds a failed history entry for a new transcription attempt.
pub fn build_failed_entry(
    state: &AppState,
    id: String,
    date: String,
    audio_path: Option<String>,
    duration_ms: u64,
    source: &str,
    engine: TranscriptionEngine,
    error_msg: String,
) -> HistoryEntry {
    let dual_mode = *state.dual_engine.read();
    let deepgram_attempted = dual_mode || engine == TranscriptionEngine::DeepgramNova3;
    HistoryEntry {
        id,
        date,
        words: 0,
        engine: format!("{:?}", engine),
        text: String::new(),
        audio_path,
        evaluation: None,
        duration_ms,
        source: source.to_string(),
        latency_ms: 0,
        throughput: 0.0,
        transcription_latency_ms: None,
        sanitizer_latency_ms: None,
        transcription_throughput: None,
        sanitizer_throughput: None,
        realtime_factor: None,
        deepgram_mode: history_deepgram_mode(state, deepgram_attempted),
        total_tokens: None,
        is_error: Some(true),
        error_message: Some(error_msg),
        debug_info: None,
        mode: None,
        model: None,
        stages: None,
        used_fallback: None,
        fallback_reason: None,
        content_type: None,
        whisper_text: None,
        sanitizer_text: None,
        gemini_text: None,
        warnings: None,
        audio_prepare_ms: None,
        base64_ms: None,
        whisper_ms: None,
        sanitizer_ms: None,
        files_upload_ms: None,
        files_poll_ms: None,
        files_poll_count: None,
        gemini_generate_ms: None,
        gemini_delete_ms: None,
        strict_literals_ms: None,
        clipboard_ms: None,
        total_pipeline_ms: None,
        gemini_transport: None,
    }
}

/// Rewrites an existing entry as failed (retry path).
pub fn update_failed_entry(
    state: &AppState,
    entry: &HistoryEntry,
    error_msg: String,
) -> HistoryEntry {
    let dual = *state.dual_engine.read();
    let eng = state.active_engine();
    let deepgram_attempted = dual || eng == TranscriptionEngine::DeepgramNova3;
    HistoryEntry {
        id: entry.id.clone(),
        date: entry.date.clone(),
        words: 0,
        engine: entry.engine.clone(),
        text: String::new(),
        audio_path: entry.audio_path.clone(),
        evaluation: None,
        duration_ms: entry.duration_ms,
        source: entry.source.clone(),
        latency_ms: 0,
        throughput: 0.0,
        transcription_latency_ms: None,
        sanitizer_latency_ms: None,
        transcription_throughput: None,
        sanitizer_throughput: None,
        realtime_factor: None,
        deepgram_mode: history_deepgram_mode(state, deepgram_attempted),
        total_tokens: None,
        is_error: Some(true),
        error_message: Some(error_msg),
        debug_info: None,
        mode: None,
        model: None,
        stages: None,
        used_fallback: None,
        fallback_reason: None,
        content_type: None,
        whisper_text: None,
        sanitizer_text: None,
        gemini_text: None,
        warnings: None,
        audio_prepare_ms: None,
        base64_ms: None,
        whisper_ms: None,
        sanitizer_ms: None,
        files_upload_ms: None,
        files_poll_ms: None,
        files_poll_count: None,
        gemini_generate_ms: None,
        gemini_delete_ms: None,
        strict_literals_ms: None,
        clipboard_ms: None,
        total_pipeline_ms: None,
        gemini_transport: None,
    }
}

/// Emits `transcription-saved` to the UI.
pub fn emit_saved(state: &AppState, entry: &HistoryEntry) {
    if let Some(handle) = state.app_handle.read().as_ref() {
        use tauri::Emitter;
        if let Err(e) = handle.emit(crate::models::event_names::TRANSCRIPTION_SAVED, entry) {
            log::warn!("transcription: failed to emit transcription-saved: {}", e);
        }
    }
}
