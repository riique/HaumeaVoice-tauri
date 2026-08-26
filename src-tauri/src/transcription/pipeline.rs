//! Stage 2 sanitization and history entry assembly for the legacy pipeline.

use std::sync::Arc;

use crate::models::{AppState, HistoryEntry, TranscriptionEngine};
use crate::pipeline_contract::TranscriptionMode;
use crate::pipeline_run::{
    epoch_ms, AttemptStatus, AudioTransport, PipelineError, PipelineErrorKind, PipelineRun,
    ProviderAttempt, StageKind, StageRecord, TranscriptVersions, UsageRecord,
    PIPELINE_RUN_SCHEMA_VERSION,
};
use crate::transcription::fallback::{coalesce_empty_final, pick_raw_acoustic};
use crate::transcription::telemetry::{
    acoustic_word_count, compute_realtime_factor, est_throughput, est_total_tokens,
    history_deepgram_mode, history_engine_label, log_latency,
};
use crate::transcription::types::SanitizeOutcome;

fn provider_identity(engine: TranscriptionEngine) -> (&'static str, &'static str, AudioTransport) {
    match engine {
        TranscriptionEngine::GroqWhisper => {
            ("groq", "whisper-large-v3-turbo", AudioTransport::Multipart)
        }
        TranscriptionEngine::DeepgramNova3 => ("deepgram", "nova-3", AudioTransport::RawBinary),
        TranscriptionEngine::GeminiMultimodal => (
            "google-ai-studio",
            "gemini-audio",
            AudioTransport::InlineBase64,
        ),
    }
}

fn legacy_success_run(
    state: &AppState,
    history_id: &str,
    engine: TranscriptionEngine,
    whisper_text: &str,
    deepgram_text: &str,
    final_text: &str,
    transcription_latency_ms: u64,
    dual_mode: bool,
    deepgram_ran: bool,
    sanitize: &SanitizeOutcome,
) -> PipelineRun {
    let mode = TranscriptionMode::from_legacy(engine, dual_mode);
    let now = epoch_ms();
    let mut run = PipelineRun::success(
        format!("{history_id}-run-{now}"),
        mode,
        final_text.to_string(),
    );
    run.session_id = format!("{history_id}-session");
    run.started_at_ms =
        now.saturating_sub(transcription_latency_ms.saturating_add(sanitize.sanitizer_latency_ms));
    run.transcript = TranscriptVersions {
        raw: Some(if !whisper_text.trim().is_empty() {
            whisper_text.to_string()
        } else {
            deepgram_text.to_string()
        }),
        refined: Some(final_text.to_string()),
        formatted: Some(final_text.to_string()),
        delivered: Some(final_text.to_string()),
        user_corrected: None,
    };
    run.fallback.used = sanitize.used_raw_fallback;
    run.fallback.reason = sanitize
        .used_raw_fallback
        .then(|| "sanitizer_raw_fallback".to_string());
    run.timings.provider_ms = Some(transcription_latency_ms);
    run.timings.refinement_ms = Some(sanitize.sanitizer_latency_ms);
    run.timings.sanitizer_ms = Some(sanitize.sanitizer_latency_ms);
    run.timings.total_ms = transcription_latency_ms.saturating_add(sanitize.sanitizer_latency_ms);
    run.debug_info = sanitize.debug_info.clone();
    if let Some(session) = state.recording_session.lock().clone() {
        run.session_id = session.id;
        run.started_at_ms = session.started_at_ms;
        run.context = session.context.persisted_metadata();
        run.profile_id = Some(session.profile.profile_id);
        run.formatting_level = session.formatting_level;
        run.destination = session.destination;
        run.delivery.destination = session.destination;
    }

    let (provider, model, transport) = provider_identity(engine);
    run.add_attempt(ProviderAttempt {
        id: format!("{}-attempt-1", run.id),
        provider: provider.into(),
        model: model.into(),
        transport,
        started_at_ms: run.started_at_ms,
        duration_ms: Some(transcription_latency_ms),
        status: AttemptStatus::Success,
        usage: UsageRecord {
            audio_seconds: None,
            total_tokens: None,
            ..UsageRecord::default()
        },
        ..ProviderAttempt::default()
    });
    if deepgram_ran && engine != TranscriptionEngine::DeepgramNova3 {
        run.add_attempt(ProviderAttempt {
            id: format!("{}-attempt-2", run.id),
            provider: "deepgram".into(),
            model: "nova-3".into(),
            transport: if state.deepgram_mode.read().as_str() == "streaming_final" {
                AudioTransport::WebSocketStream
            } else {
                AudioTransport::RawBinary
            },
            started_at_ms: run.started_at_ms,
            duration_ms: Some(transcription_latency_ms),
            status: AttemptStatus::Success,
            ..ProviderAttempt::default()
        });
    }
    run.add_stage(StageRecord::completed(
        StageKind::Recognition,
        transcription_latency_ms,
    ));
    run.add_stage(StageRecord::completed(
        StageKind::SemanticRefinement,
        sanitize.sanitizer_latency_ms,
    ));
    run.finish_success();
    run
}

fn legacy_failed_run(
    history_id: &str,
    engine: TranscriptionEngine,
    dual_mode: bool,
    error_msg: &str,
) -> PipelineRun {
    let now = epoch_ms();
    let mode = TranscriptionMode::from_legacy(engine, dual_mode);
    let error = PipelineError {
        kind: PipelineErrorKind::Provider,
        code: "recognition_failed".into(),
        message: error_msg.to_string(),
        retryable: true,
    };
    let mut run = PipelineRun::hard_error(
        format!("{history_id}-run-{now}"),
        mode,
        error_msg.to_string(),
    );
    run.session_id = format!("{history_id}-session");
    run.started_at_ms = now;
    run.error = Some(error.clone());
    let (provider, model, transport) = provider_identity(engine);
    run.add_attempt(ProviderAttempt {
        id: format!("{}-attempt-1", run.id),
        provider: provider.into(),
        model: model.into(),
        transport,
        started_at_ms: now,
        duration_ms: Some(0),
        status: AttemptStatus::Failed,
        error: Some(error.clone()),
        ..ProviderAttempt::default()
    });
    run.add_stage(StageRecord::failed(StageKind::Recognition, 0, error));
    run
}

/// Runs the Groq sanitizer (or pick_raw) exactly as the pre-extraction path.
pub async fn run_sanitize(
    state: &Arc<AppState>,
    whisper_text: &str,
    deepgram_text: &str,
    dual_mode: bool,
) -> SanitizeOutcome {
    let sanitizer_key = state.next_groq_key();
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
    let context_preferences = state.context_preferences.read().clone();
    let context_block = state
        .recording_session
        .lock()
        .as_ref()
        .filter(|session| session.profile.allow_context_to_cloud)
        .and_then(|session| {
            crate::context::package_untrusted_context(&session.context, &context_preferences)
        });
    let mut debug_info = None;
    let mut warnings: Vec<String> = Vec::new();
    let mut used_raw_fallback = false;
    let mut changed = false;
    let start_sanitizer = std::time::Instant::now();

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
                    context_block.as_deref(),
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
    let final_text = crate::transcription::remove_known_transcription_artifacts(&final_text);
    let sanitizer_latency_ms = start_sanitizer.elapsed().as_millis() as u64;

    SanitizeOutcome {
        final_text,
        debug_info,
        sanitizer_latency_ms,
        raw_words,
        warnings,
        used_raw_fallback,
        changed,
        // Kept only for compatibility with historical/debug structures. The
        // active prompt no longer branches on a user-selected content type.
        content_type: crate::pipeline_contract::ContentType::Auto,
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
    let pipeline_run = legacy_success_run(
        state,
        &id,
        engine,
        whisper_text,
        deepgram_text,
        &final_text,
        transcription_latency_ms,
        dual_mode,
        deepgram_ran,
        sanitize,
    );

    HistoryEntry {
        schema_version: PIPELINE_RUN_SCHEMA_VERSION,
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
        pipeline_runs: vec![pipeline_run],
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
    let pipeline_run = legacy_failed_run(&id, engine, dual_mode, &error_msg);
    HistoryEntry {
        schema_version: PIPELINE_RUN_SCHEMA_VERSION,
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
        pipeline_runs: vec![pipeline_run],
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
    let mut pipeline_runs = entry.pipeline_runs.clone();
    pipeline_runs.push(legacy_failed_run(&entry.id, eng, dual, &error_msg));
    HistoryEntry {
        schema_version: PIPELINE_RUN_SCHEMA_VERSION,
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
        pipeline_runs,
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
