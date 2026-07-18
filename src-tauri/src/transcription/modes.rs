//! Product modes: UltraFast, FastAccurate, Precise, UltraPrecise.

use std::sync::Arc;

use crate::gemini::{
    encode_audio_base64, mime_for_ext, refine_precise, refine_precise_with_file,
    refine_ultraprecise, spawn_cleanup, transcribe_audio, transcribe_inline, transcribe_with_file,
    upload_and_wait, GeminiAudioTransport, TranscribeRequest,
};
use crate::models::{AppState, HistoryEntry, SanitizerDebug, TranscriptionEngine};
use crate::pipeline_contract::TranscriptionMode;
use crate::transcription::legacy::transcribe_bytes;
use crate::transcription::telemetry::{
    compute_realtime_factor, est_throughput, est_total_tokens, log_latency,
};
use crate::transcription::types::AcousticOutcome;

/// Outcome of a mode-based pipeline run (ready for history + clipboard).
#[derive(Debug, Clone, Default)]
pub struct ModePipelineResult {
    pub final_text: String,
    pub mode: TranscriptionMode,
    pub model: String,
    pub stages: Vec<String>,
    pub used_fallback: bool,
    pub fallback_reason: Option<String>,
    pub whisper_text: Option<String>,
    pub gemini_text: Option<String>,
    pub sanitizer_text: Option<String>,
    pub transcription_latency_ms: u64,
    pub history_engine_label: String,
    pub whisper_ms: Option<u64>,
    pub upload_ms: Option<u64>,
    pub gemini_ms: Option<u64>,
    pub debug_info: Option<SanitizerDebug>,
    pub audio_prepare_ms: Option<u64>,
    pub base64_ms: Option<u64>,
    pub sanitizer_ms: Option<u64>,
    pub files_upload_ms: Option<u64>,
    pub files_poll_ms: Option<u64>,
    pub files_poll_count: Option<u32>,
    pub gemini_generate_ms: Option<u64>,
    pub gemini_delete_ms: Option<u64>,
    pub strict_literals_ms: Option<u64>,
    pub total_pipeline_ms: Option<u64>,
    pub gemini_transport: Option<String>,
    pub warnings: Vec<String>,
    pub content_type: Option<String>,
}

fn pipeline_debug_snapshot(result: &ModePipelineResult) -> SanitizerDebug {
    let mut user_parts = Vec::new();
    if let Some(w) = result.whisper_text.as_ref().filter(|s| !s.is_empty()) {
        user_parts.push(format!("[WHISPER]\n{}", w));
    }
    if let Some(s) = result.sanitizer_text.as_ref().filter(|t| !t.is_empty()) {
        user_parts.push(format!("[SANITIZER]\n{}", s));
    }
    if let Some(g) = result.gemini_text.as_ref().filter(|t| !t.is_empty()) {
        user_parts.push(format!("[GEMINI]\n{}", g));
    }
    if user_parts.is_empty() {
        user_parts.push(format!("[FINAL]\n{}", result.final_text));
    }
    let request_json = serde_json::json!({
        "kind": "product_pipeline",
        "mode": result.mode.as_str(),
        "model": result.model,
        "stages": result.stages,
        "transport": result.gemini_transport,
        "used_fallback": result.used_fallback,
        "fallback_reason": result.fallback_reason,
        "whisper_ms": result.whisper_ms,
        "sanitizer_ms": result.sanitizer_ms,
        "base64_ms": result.base64_ms,
        "files_upload_ms": result.files_upload_ms,
        "files_poll_ms": result.files_poll_ms,
        "files_poll_count": result.files_poll_count,
        "gemini_generate_ms": result.gemini_generate_ms,
        "total_pipeline_ms": result.total_pipeline_ms,
    });
    SanitizerDebug {
        endpoint: format!("product-mode:{}", result.mode.as_str()),
        model: result.model.clone(),
        temperature: 0.0,
        reasoning_enabled: false,
        reasoning_effort: String::new(),
        reasoning_effort_applied: false,
        reasoning_supported_by_model: false,
        system_prompt: format!(
            "Pipeline de produto.\nTransporte: {:?}\nEtapas: {}",
            result.gemini_transport,
            result.stages.join(" → ")
        ),
        user_message: user_parts.join("\n\n"),
        request_json: serde_json::to_string_pretty(&request_json).unwrap_or_else(|_| "{}".into()),
        response_status: Some(200),
        response_content: Some(result.final_text.clone()),
        response_reasoning: None,
        error: result.fallback_reason.clone(),
    }
}

fn apply_timing_from_gemini(
    result: &mut ModePipelineResult,
    g: &crate::gemini::GeminiGenerateResult,
) {
    if let Some(t) = g.transport {
        result.gemini_transport = Some(t.as_str().to_string());
    }
    result.base64_ms = g.timing.base64_ms.or(result.base64_ms);
    result.files_upload_ms = g.timing.files_upload_ms.or(result.files_upload_ms);
    result.files_poll_ms = g.timing.files_poll_ms.or(result.files_poll_ms);
    result.files_poll_count = g.timing.files_poll_count.or(result.files_poll_count);
    result.gemini_generate_ms = g.timing.generate_ms.or(result.gemini_generate_ms);
    result.gemini_delete_ms = g.timing.delete_ms.or(result.gemini_delete_ms);
    result.gemini_ms = g.timing.generate_ms.or(Some(g.latency_ms));
    if let Some(u) = g.timing.files_upload_ms {
        let poll = g.timing.files_poll_ms.unwrap_or(0);
        result.upload_ms = Some(u + poll);
    }
}

/// UltraFast: audio → Whisper Large V3 Turbo → text (no sanitizer).
pub async fn run_ultra_fast(
    state: &Arc<AppState>,
    audio: Vec<u8>,
    file_name: &str,
    mime: &str,
    _duration_ms: Option<u64>,
) -> Result<ModePipelineResult, String> {
    let t0 = std::time::Instant::now();
    log::info!("modes: UltraFast → Whisper only (sanitizer off)");

    let text = transcribe_bytes(
        state,
        audio,
        file_name,
        mime,
        TranscriptionEngine::GroqWhisper,
    )
    .await?;

    if text.trim().is_empty() {
        return Err("Nenhum texto detectado na gravação.".to_string());
    }

    let ms = t0.elapsed().as_millis() as u64;
    Ok(ModePipelineResult {
        final_text: text.trim().to_string(),
        mode: TranscriptionMode::UltraFast,
        model: "whisper-large-v3-turbo".into(),
        stages: vec!["whisper".into()],
        whisper_text: Some(text.trim().to_string()),
        transcription_latency_ms: ms,
        history_engine_label: "UltraFast/Whisper".into(),
        whisper_ms: Some(ms),
        total_pipeline_ms: Some(ms),
        ..Default::default()
    })
}

/// FastAccurate: Gemini (inline or Files) + glossary + strict literals.
pub async fn run_fast_accurate(
    state: &Arc<AppState>,
    audio: Vec<u8>,
    ext: &str,
    file_name: &str,
    mime: &str,
    duration_ms: Option<u64>,
) -> Result<ModePipelineResult, String> {
    let t0 = std::time::Instant::now();
    let fallback = *state.gemini_fallback_to_whisper.read();
    let api_key = {
        let guard = state.api_keys.read();
        guard.google.clone().filter(|k| !k.trim().is_empty())
    };
    let glossary_block = {
        let vocab = state.vocabulary.read().clone();
        crate::vocabulary::format_glossary_for_prompt(&vocab)
    };
    // Content type for FastAccurate is pre-hint only (no acoustic text yet).
    // After Gemini we re-resolve on the transcript for history; preference still applies.
    let pref_ct = *state.content_type.read();
    let content_note = if pref_ct == crate::pipeline_contract::ContentType::Auto {
        String::new() // detect after we have text; empty = neutral prompt
    } else {
        pref_ct.as_str().to_string()
    };
    log::info!(
        "modes: FastAccurate → Gemini hybrid (fallback_to_whisper={})",
        fallback
    );

    let mut stages = Vec::new();
    let mut used_fallback = false;
    let mut fallback_reason = None;
    let mut whisper_text = None;
    let mut gemini_text = None;
    let mut label = "FastAccurate/Gemini".to_string();
    let mut result_meta = ModePipelineResult::default();

    let (final_text, model) = match api_key {
        None => {
            if !fallback {
                return Err("Configure a chave de API do Google (Gemini) em Ajustes.".to_string());
            }
            stages.push("gemini_skipped_no_key".into());
            used_fallback = true;
            fallback_reason = Some("gemini_missing_api_key".into());
            label = "FastAccurate/WhisperFallback".into();
            let tw = std::time::Instant::now();
            let w = transcribe_bytes(
                state,
                audio,
                file_name,
                mime,
                TranscriptionEngine::GroqWhisper,
            )
            .await?;
            result_meta.whisper_ms = Some(tw.elapsed().as_millis() as u64);
            stages.push("whisper_fallback".into());
            whisper_text = Some(w.clone());
            (w, "whisper-large-v3-turbo".to_string())
        }
        Some(key) => {
            stages.push("gemini_transcribe".into());
            let req = TranscribeRequest {
                audio_bytes: audio.clone(),
                ext: ext.to_string(),
                api_key: key,
                display_name: file_name.to_string(),
                duration_ms,
                glossary_block,
                content_note: content_note.clone(),
            };
            match transcribe_audio(req).await {
                Ok(r) if !r.text.trim().is_empty() => {
                    apply_timing_from_gemini(&mut result_meta, &r);
                    if let Some(t) = r.transport {
                        stages.push(format!("transport:{}", t.as_str()));
                    }
                    gemini_text = Some(r.text.clone());
                    (r.text, r.model)
                }
                Ok(_) => {
                    if !fallback {
                        return Err("O Gemini não retornou texto.".to_string());
                    }
                    used_fallback = true;
                    fallback_reason = Some("gemini_empty".into());
                    label = "FastAccurate/WhisperFallback".into();
                    let tw = std::time::Instant::now();
                    let w = transcribe_bytes(
                        state,
                        audio,
                        file_name,
                        mime,
                        TranscriptionEngine::GroqWhisper,
                    )
                    .await?;
                    result_meta.whisper_ms = Some(tw.elapsed().as_millis() as u64);
                    stages.push("whisper_fallback".into());
                    whisper_text = Some(w.clone());
                    (w, "whisper-large-v3-turbo".to_string())
                }
                Err(e) => {
                    if !fallback {
                        return Err(format!("Gemini: {}", e));
                    }
                    used_fallback = true;
                    fallback_reason = Some(format!("gemini_error: {}", e));
                    label = "FastAccurate/WhisperFallback".into();
                    let tw = std::time::Instant::now();
                    let w = transcribe_bytes(
                        state,
                        audio,
                        file_name,
                        mime,
                        TranscriptionEngine::GroqWhisper,
                    )
                    .await
                    .map_err(|we| {
                        format!(
                            "Gemini falhou ({}) e o fallback Whisper também falhou: {}",
                            e, we
                        )
                    })?;
                    result_meta.whisper_ms = Some(tw.elapsed().as_millis() as u64);
                    stages.push("whisper_fallback".into());
                    whisper_text = Some(w.clone());
                    (w, "whisper-large-v3-turbo".to_string())
                }
            }
        }
    };

    if final_text.trim().is_empty() {
        return Err("Nenhum texto detectado na gravação.".to_string());
    }

    let ms = t0.elapsed().as_millis() as u64;
    let resolved_ct = crate::sanitizer_json::resolve_content_type(pref_ct, final_text.trim());
    Ok(ModePipelineResult {
        final_text: final_text.trim().to_string(),
        mode: TranscriptionMode::FastAccurate,
        model,
        stages,
        used_fallback,
        fallback_reason,
        whisper_text,
        gemini_text,
        transcription_latency_ms: ms,
        history_engine_label: label,
        total_pipeline_ms: Some(ms),
        whisper_ms: result_meta.whisper_ms,
        upload_ms: result_meta.upload_ms,
        gemini_ms: result_meta.gemini_ms,
        base64_ms: result_meta.base64_ms,
        files_upload_ms: result_meta.files_upload_ms,
        files_poll_ms: result_meta.files_poll_ms,
        files_poll_count: result_meta.files_poll_count,
        gemini_generate_ms: result_meta.gemini_generate_ms,
        gemini_transport: result_meta.gemini_transport,
        content_type: Some(resolved_ct.as_str().to_string()),
        ..Default::default()
    })
}

/// Precise: Whisper ∥ (Base64 or upload) → Gemini refine.
pub async fn run_precise(
    state: &Arc<AppState>,
    audio: Vec<u8>,
    ext: &str,
    file_name: &str,
    mime: &str,
    duration_ms: Option<u64>,
) -> Result<ModePipelineResult, String> {
    let t0 = std::time::Instant::now();
    let api_key = {
        let guard = state.api_keys.read();
        guard.google.clone().filter(|k| !k.trim().is_empty())
    };
    let Some(api_key) = api_key else {
        return Err("Configure a chave de API do Google (Gemini) em Ajustes.".to_string());
    };
    let glossary_block = {
        let vocab = state.vocabulary.read().clone();
        crate::vocabulary::format_glossary_for_prompt(&vocab)
    };
    let vocab_snapshot = state.vocabulary.read().clone();
    let display = if file_name.trim().is_empty() {
        format!("haumea-precise.{}", ext)
    } else {
        file_name.to_string()
    };
    let mime_g = mime_for_ext(ext);
    let transport = crate::gemini::select_gemini_audio_transport(
        audio.len(),
        duration_ms.or_else(|| crate::gemini::estimate_wav_duration_ms(&audio)),
        mime_g,
    )?;

    log::info!(
        "modes: Precise → Whisper ∥ prep, transport={:?}",
        transport.as_str()
    );

    let whisper_audio = audio.clone();
    let state_w = state.clone();
    let file_name_w = file_name.to_string();
    let mime_w = mime.to_string();
    let whisper_fut = async move {
        let tw = std::time::Instant::now();
        let r = transcribe_bytes(
            &state_w,
            whisper_audio,
            &file_name_w,
            &mime_w,
            TranscriptionEngine::GroqWhisper,
        )
        .await;
        (r, tw.elapsed().as_millis() as u64)
    };

    let prep_audio = audio.clone();
    let key_u = api_key.clone();
    let display_u = display.clone();
    let prep_fut = async move {
        match transport {
            GeminiAudioTransport::Inline => {
                let (b64, ms) = encode_audio_base64(&prep_audio);
                PrepOutcome::Inline { b64, base64_ms: ms }
            }
            GeminiAudioTransport::FilesApi => {
                let tu = std::time::Instant::now();
                match upload_and_wait(&key_u, &prep_audio, mime_g, &display_u).await {
                    Ok((guard, timing)) => PrepOutcome::Files {
                        guard,
                        timing,
                        wall_ms: tu.elapsed().as_millis() as u64,
                    },
                    Err(e) => PrepOutcome::FilesErr(e),
                }
            }
        }
    };

    let ((whisper_res, whisper_ms), prep) = tokio::join!(whisper_fut, prep_fut);
    let mut stages = vec![
        format!("whisper_parallel:{}ms", whisper_ms),
        format!("transport:{}", transport.as_str()),
    ];
    let mut meta = ModePipelineResult {
        whisper_ms: Some(whisper_ms),
        gemini_transport: Some(transport.as_str().into()),
        ..Default::default()
    };

    match (whisper_res, prep) {
        (Ok(w), PrepOutcome::Inline { b64, base64_ms }) => {
            meta.base64_ms = Some(base64_ms);
            stages.push(format!("base64:{}ms", base64_ms));
            let w_trim = w.trim().to_string();
            if w_trim.is_empty() {
                let pure =
                    transcribe_inline(&api_key, &audio, mime_g, Some((b64, base64_ms))).await;
                return finish_precise_pure(
                    pure,
                    t0,
                    stages,
                    meta,
                    whisper_ms,
                    None,
                    "whisper_empty_gemini_pure",
                    &vocab_snapshot,
                );
            }
            let refined = refine_precise(
                &api_key,
                &audio,
                ext,
                &display,
                &w_trim,
                &glossary_block,
                duration_ms,
                Some((b64, base64_ms)),
            )
            .await;
            finish_precise_refine(
                refined,
                t0,
                stages,
                meta,
                w_trim,
                whisper_ms,
                &vocab_snapshot,
            )
        }
        (
            Ok(w),
            PrepOutcome::Files {
                guard,
                timing,
                wall_ms,
            },
        ) => {
            meta.files_upload_ms = Some(timing.upload_ms);
            meta.files_poll_ms = Some(timing.poll_ms);
            meta.files_poll_count = Some(timing.poll_count);
            meta.upload_ms = Some(wall_ms);
            stages.push(format!("upload_parallel:{}ms", wall_ms));
            let w_trim = w.trim().to_string();
            if w_trim.is_empty() {
                stages.push("whisper_empty".into());
                let file_ref = guard.file_ref();
                let pure = transcribe_with_file(&api_key, &file_ref).await;
                spawn_cleanup(guard);
                return finish_precise_pure(
                    pure,
                    t0,
                    stages,
                    meta,
                    whisper_ms,
                    Some(wall_ms),
                    "whisper_empty_gemini_pure",
                    &vocab_snapshot,
                );
            }
            let file_ref = guard.file_ref();
            let refined =
                refine_precise_with_file(&api_key, &file_ref, &w_trim, &glossary_block).await;
            spawn_cleanup(guard);
            finish_precise_refine(
                refined,
                t0,
                stages,
                meta,
                w_trim,
                whisper_ms,
                &vocab_snapshot,
            )
        }
        (Ok(w), PrepOutcome::FilesErr(up_err)) => {
            stages.push("upload_failed".into());
            let w_trim = w.trim().to_string();
            if w_trim.is_empty() {
                return Err(format!(
                    "Upload Gemini falhou ({}) e o Whisper não retornou texto.",
                    up_err
                ));
            }
            let ts = std::time::Instant::now();
            let (final_text, hits) =
                crate::vocabulary::apply_strict_literals(&w_trim, &vocab_snapshot);
            meta.strict_literals_ms = Some(ts.elapsed().as_millis() as u64);
            if !hits.is_empty() {
                stages.push(format!("strict_literals:{}", hits.len()));
            }
            let total = t0.elapsed().as_millis() as u64;
            Ok(ModePipelineResult {
                final_text,
                mode: TranscriptionMode::Precise,
                model: "whisper-large-v3-turbo".into(),
                stages,
                used_fallback: true,
                fallback_reason: Some(format!("upload_failed: {}", up_err)),
                whisper_text: Some(w_trim),
                transcription_latency_ms: total,
                history_engine_label: "Precise/WhisperOnly".into(),
                whisper_ms: Some(whisper_ms),
                total_pipeline_ms: Some(total),
                gemini_transport: meta.gemini_transport,
                warnings: vec![format!("upload_failed: {}", up_err)],
                ..meta
            })
        }
        (Err(w_err), PrepOutcome::Inline { b64, base64_ms }) => {
            meta.base64_ms = Some(base64_ms);
            stages.push(format!("whisper_failed:{}", w_err));
            let pure = transcribe_inline(&api_key, &audio, mime_g, Some((b64, base64_ms))).await;
            finish_precise_pure(
                pure,
                t0,
                stages,
                meta,
                whisper_ms,
                None,
                &format!("whisper_failed: {}", w_err),
                &vocab_snapshot,
            )
        }
        (
            Err(w_err),
            PrepOutcome::Files {
                guard,
                timing,
                wall_ms,
            },
        ) => {
            meta.files_upload_ms = Some(timing.upload_ms);
            meta.files_poll_ms = Some(timing.poll_ms);
            meta.files_poll_count = Some(timing.poll_count);
            meta.upload_ms = Some(wall_ms);
            stages.push(format!("whisper_failed:{}", w_err));
            let file_ref = guard.file_ref();
            let pure = transcribe_with_file(&api_key, &file_ref).await;
            spawn_cleanup(guard);
            finish_precise_pure(
                pure,
                t0,
                stages,
                meta,
                whisper_ms,
                Some(wall_ms),
                &format!("whisper_failed: {}", w_err),
                &vocab_snapshot,
            )
        }
        (Err(w_err), PrepOutcome::FilesErr(up_err)) => Err(format!(
            "Ambos falharam no modo Preciso:\n• Whisper: {}\n• Upload Gemini: {}",
            w_err, up_err
        )),
    }
}

enum PrepOutcome {
    Inline {
        b64: String,
        base64_ms: u64,
    },
    Files {
        guard: crate::gemini::RemoteFileGuard,
        timing: crate::gemini::UploadTiming,
        wall_ms: u64,
    },
    FilesErr(String),
}

fn finish_precise_refine(
    refined: Result<crate::gemini::GeminiGenerateResult, String>,
    t0: std::time::Instant,
    mut stages: Vec<String>,
    mut meta: ModePipelineResult,
    w_trim: String,
    whisper_ms: u64,
    vocab: &[crate::vocabulary::VocabularyTerm],
) -> Result<ModePipelineResult, String> {
    match refined {
        Ok(r) if !r.text.trim().is_empty() => {
            apply_timing_from_gemini(&mut meta, &r);
            if let Some(g) = r.timing.generate_ms {
                stages.push(format!("gemini_refine:{}ms", g));
            }
            let ts = std::time::Instant::now();
            let (final_text, hits) = crate::vocabulary::apply_strict_literals(r.text.trim(), vocab);
            meta.strict_literals_ms = Some(ts.elapsed().as_millis() as u64);
            if !hits.is_empty() {
                stages.push(format!("strict_literals:{}", hits.len()));
            }
            let total = t0.elapsed().as_millis() as u64;
            Ok(ModePipelineResult {
                final_text,
                mode: TranscriptionMode::Precise,
                model: r.model,
                stages,
                whisper_text: Some(w_trim),
                gemini_text: Some(r.text.trim().to_string()),
                transcription_latency_ms: total,
                history_engine_label: "Precise/Whisper+Gemini".into(),
                whisper_ms: Some(whisper_ms),
                total_pipeline_ms: Some(total),
                ..meta
            })
        }
        Ok(_) | Err(_) => {
            let reason = match &refined {
                Ok(_) => "gemini_refine_empty".to_string(),
                Err(e) => format!("gemini_refine_error: {}", e),
            };
            log::warn!(
                "modes: Precise refine failed ({}); delivering Whisper",
                reason
            );
            stages.push("whisper_deliver".into());
            let ts = std::time::Instant::now();
            let (final_text, hits) = crate::vocabulary::apply_strict_literals(&w_trim, vocab);
            meta.strict_literals_ms = Some(ts.elapsed().as_millis() as u64);
            if !hits.is_empty() {
                stages.push(format!("strict_literals:{}", hits.len()));
            }
            let total = t0.elapsed().as_millis() as u64;
            Ok(ModePipelineResult {
                final_text,
                mode: TranscriptionMode::Precise,
                model: "whisper-large-v3-turbo".into(),
                stages,
                used_fallback: true,
                fallback_reason: Some(reason.clone()),
                whisper_text: Some(w_trim),
                transcription_latency_ms: total,
                history_engine_label: "Precise/WhisperFallback".into(),
                whisper_ms: Some(whisper_ms),
                total_pipeline_ms: Some(total),
                warnings: vec![reason],
                ..meta
            })
        }
    }
}

fn finish_precise_pure(
    pure: Result<crate::gemini::GeminiGenerateResult, String>,
    t0: std::time::Instant,
    mut stages: Vec<String>,
    mut meta: ModePipelineResult,
    whisper_ms: u64,
    upload_ms: Option<u64>,
    reason: &str,
    vocab: &[crate::vocabulary::VocabularyTerm],
) -> Result<ModePipelineResult, String> {
    match pure {
        Ok(r) if !r.text.trim().is_empty() => {
            apply_timing_from_gemini(&mut meta, &r);
            stages.push("gemini_pure".into());
            let ts = std::time::Instant::now();
            let (final_text, hits) = crate::vocabulary::apply_strict_literals(r.text.trim(), vocab);
            meta.strict_literals_ms = Some(ts.elapsed().as_millis() as u64);
            if !hits.is_empty() {
                stages.push(format!("strict_literals:{}", hits.len()));
            }
            let total = t0.elapsed().as_millis() as u64;
            Ok(ModePipelineResult {
                final_text,
                mode: TranscriptionMode::Precise,
                model: r.model,
                stages,
                used_fallback: true,
                fallback_reason: Some(reason.into()),
                gemini_text: Some(r.text.trim().to_string()),
                transcription_latency_ms: total,
                history_engine_label: "Precise/GeminiPure".into(),
                whisper_ms: Some(whisper_ms),
                upload_ms,
                total_pipeline_ms: Some(total),
                warnings: vec![reason.into()],
                ..meta
            })
        }
        Ok(_) => Err(format!(
            "Whisper indisponível e o Gemini não retornou texto ({})",
            reason
        )),
        Err(ge) => Err(format!(
            "Ambos falharam no modo Preciso:\n• Whisper path: {}\n• Gemini: {}",
            reason, ge
        )),
    }
}

/// UltraPrecise: Whisper → sanitizer ∥ prep → Gemini.
pub async fn run_ultra_precise(
    state: &Arc<AppState>,
    audio: Vec<u8>,
    ext: &str,
    file_name: &str,
    mime: &str,
    duration_ms: Option<u64>,
) -> Result<ModePipelineResult, String> {
    let t0 = std::time::Instant::now();
    let api_key = {
        let guard = state.api_keys.read();
        guard.google.clone().filter(|k| !k.trim().is_empty())
    };
    let glossary_block = {
        let vocab = state.vocabulary.read().clone();
        crate::vocabulary::format_glossary_for_prompt(&vocab)
    };
    let vocab_snapshot = state.vocabulary.read().clone();
    let display = if file_name.trim().is_empty() {
        format!("haumea-ultraprecise.{}", ext)
    } else {
        file_name.to_string()
    };
    let mime_g = mime_for_ext(ext);
    let transport = crate::gemini::select_gemini_audio_transport(
        audio.len(),
        duration_ms.or_else(|| crate::gemini::estimate_wav_duration_ms(&audio)),
        mime_g,
    )?;

    log::info!(
        "modes: UltraPrecise → Whisper→sanitizer ∥ prep ({})",
        transport.as_str()
    );

    // Branch A: Whisper then sanitizer (sequential chain).
    let whisper_audio = audio.clone();
    let state_w = state.clone();
    let file_name_w = file_name.to_string();
    let mime_w = mime.to_string();
    let chain_fut = async move {
        let tw = std::time::Instant::now();
        let wr = transcribe_bytes(
            &state_w,
            whisper_audio,
            &file_name_w,
            &mime_w,
            TranscriptionEngine::GroqWhisper,
        )
        .await;
        let whisper_ms = tw.elapsed().as_millis() as u64;
        match wr {
            Ok(w) if !w.trim().is_empty() => {
                let w = w.trim().to_string();
                let ts = std::time::Instant::now();
                let sanitize = crate::transcription::run_sanitize(&state_w, &w, "", false).await;
                let sanitizer_ms = ts.elapsed().as_millis() as u64;
                Ok((w, sanitize, whisper_ms, sanitizer_ms))
            }
            Ok(_) => Err(("empty".into(), whisper_ms)),
            Err(e) => Err((e, whisper_ms)),
        }
    };

    // Branch B: Base64 or Files upload (parallel with Whisper→sanitizer).
    let prep_audio = audio.clone();
    let key_u = api_key.clone();
    let display_u = display.clone();
    let prep_fut = async move {
        match transport {
            GeminiAudioTransport::Inline => {
                let (b64, ms) = encode_audio_base64(&prep_audio);
                PrepOutcome::Inline { b64, base64_ms: ms }
            }
            GeminiAudioTransport::FilesApi => match key_u {
                Some(ref k) => match upload_and_wait(k, &prep_audio, mime_g, &display_u).await {
                    Ok((guard, timing)) => PrepOutcome::Files {
                        guard,
                        timing,
                        wall_ms: timing.upload_ms + timing.poll_ms,
                    },
                    Err(e) => PrepOutcome::FilesErr(e),
                },
                None => PrepOutcome::FilesErr("missing google key".into()),
            },
        }
    };

    let (chain_res, prep) = tokio::join!(chain_fut, prep_fut);

    let (whisper_text, sanitize, whisper_ms, sanitizer_ms) = match chain_res {
        Ok(v) => v,
        Err((e, whisper_ms)) => {
            if let PrepOutcome::Files { guard, .. } = prep {
                spawn_cleanup(guard);
            }
            if e == "empty" {
                return Err("Nenhum texto detectado na gravação (Whisper vazio).".into());
            }
            return Err(format!(
                "Whisper falhou no Ultrapreciso: {} ({}ms)",
                e, whisper_ms
            ));
        }
    };

    let mut stages = vec![
        format!("whisper:{}ms", whisper_ms),
        format!("sanitizer:{}ms", sanitizer_ms),
        format!("transport:{}", transport.as_str()),
    ];
    if sanitize.used_raw_fallback {
        stages.push("sanitizer_raw_fallback".into());
    }
    for w in &sanitize.warnings {
        stages.push(format!("sanitizer_warn:{}", w));
    }

    let sanitized_text = if sanitize.final_text.trim().is_empty() {
        whisper_text.clone()
    } else {
        sanitize.final_text.clone()
    };
    let resolved_ct =
        crate::sanitizer_json::resolve_content_type(*state.content_type.read(), &whisper_text);
    let content_note = resolved_ct.as_str().to_string();

    let mut meta = ModePipelineResult {
        whisper_ms: Some(whisper_ms),
        sanitizer_ms: Some(sanitizer_ms),
        gemini_transport: Some(transport.as_str().into()),
        debug_info: sanitize.debug_info.clone(),
        warnings: sanitize.warnings.clone(),
        ..Default::default()
    };

    let (final_text, model, used_fallback, fallback_reason, gemini_text, label) =
        match (api_key.as_ref(), prep) {
            (Some(key), PrepOutcome::Inline { b64, base64_ms }) => {
                meta.base64_ms = Some(base64_ms);
                stages.push(format!("base64:{}ms", base64_ms));
                let refined = refine_ultraprecise(
                    key,
                    &audio,
                    ext,
                    &display,
                    &whisper_text,
                    &sanitized_text,
                    &glossary_block,
                    &content_note,
                    duration_ms,
                    Some((b64, base64_ms)),
                )
                .await;
                match refined {
                    Ok(r) if !r.text.trim().is_empty() => {
                        apply_timing_from_gemini(&mut meta, &r);
                        if let Some(g) = r.timing.generate_ms {
                            stages.push(format!("gemini_ultraprecise:{}ms", g));
                        }
                        (
                            r.text.trim().to_string(),
                            r.model,
                            false,
                            None,
                            Some(r.text.trim().to_string()),
                            "UltraPrecise/Whisper+Sanitizer+Gemini",
                        )
                    }
                    Ok(_) | Err(_) => {
                        let reason = match &refined {
                            Ok(_) => "gemini_empty".to_string(),
                            Err(e) => format!("gemini_error: {}", e),
                        };
                        stages.push("gemini_failed_use_sanitized".into());
                        meta.warnings.push(reason.clone());
                        (
                            sanitized_text.clone(),
                            if sanitize.used_raw_fallback {
                                "whisper-large-v3-turbo".into()
                            } else {
                                "sanitizer+whisper".into()
                            },
                            true,
                            Some(reason),
                            None,
                            "UltraPrecise/SanitizedFallback",
                        )
                    }
                }
            }
            (
                Some(key),
                PrepOutcome::Files {
                    guard,
                    timing,
                    wall_ms,
                },
            ) => {
                meta.files_upload_ms = Some(timing.upload_ms);
                meta.files_poll_ms = Some(timing.poll_ms);
                meta.files_poll_count = Some(timing.poll_count);
                meta.upload_ms = Some(wall_ms);
                stages.push(format!("upload_parallel:{}ms", wall_ms));
                let file_ref = guard.file_ref();
                let refined = crate::gemini::refine_ultraprecise_with_file(
                    key,
                    &file_ref,
                    &whisper_text,
                    &sanitized_text,
                    &glossary_block,
                    &content_note,
                )
                .await;
                spawn_cleanup(guard);
                match refined {
                    Ok(r) if !r.text.trim().is_empty() => {
                        apply_timing_from_gemini(&mut meta, &r);
                        if let Some(g) = r.timing.generate_ms {
                            stages.push(format!("gemini_ultraprecise:{}ms", g));
                        }
                        (
                            r.text.trim().to_string(),
                            r.model,
                            false,
                            None,
                            Some(r.text.trim().to_string()),
                            "UltraPrecise/Whisper+Sanitizer+Gemini",
                        )
                    }
                    Ok(_) | Err(_) => {
                        let reason = match &refined {
                            Ok(_) => "gemini_empty".to_string(),
                            Err(e) => format!("gemini_error: {}", e),
                        };
                        stages.push("gemini_failed_use_sanitized".into());
                        meta.warnings.push(reason.clone());
                        (
                            sanitized_text.clone(),
                            "sanitizer+whisper".into(),
                            true,
                            Some(reason),
                            None,
                            "UltraPrecise/SanitizedFallback",
                        )
                    }
                }
            }
            (None, prep) => {
                if let PrepOutcome::Files { guard, .. } = prep {
                    spawn_cleanup(guard);
                }
                stages.push("gemini_missing_key".into());
                meta.warnings.push("gemini_missing_api_key".into());
                (
                    sanitized_text.clone(),
                    "sanitizer+whisper".into(),
                    true,
                    Some("gemini_missing_api_key".into()),
                    None,
                    "UltraPrecise/NoGemini",
                )
            }
            (_, PrepOutcome::FilesErr(e)) => {
                stages.push("upload_failed".into());
                meta.warnings.push(format!("upload_failed: {}", e));
                (
                    sanitized_text.clone(),
                    "sanitizer+whisper".into(),
                    true,
                    Some(format!("upload_failed: {}", e)),
                    None,
                    "UltraPrecise/NoGemini",
                )
            }
        };

    if final_text.trim().is_empty() {
        return Err("Falha total no Ultrapreciso: sem texto final.".into());
    }

    let ts = std::time::Instant::now();
    let (final_text, hits) =
        crate::vocabulary::apply_strict_literals(final_text.trim(), &vocab_snapshot);
    meta.strict_literals_ms = Some(ts.elapsed().as_millis() as u64);
    if !hits.is_empty() {
        stages.push(format!("strict_literals:{}", hits.len()));
    }

    let total = t0.elapsed().as_millis() as u64;
    let san_text = if sanitize.used_raw_fallback {
        None
    } else {
        Some(sanitized_text)
    };
    Ok(ModePipelineResult {
        final_text,
        mode: TranscriptionMode::UltraPrecise,
        model,
        stages,
        used_fallback,
        fallback_reason,
        whisper_text: Some(whisper_text),
        gemini_text,
        sanitizer_text: san_text,
        transcription_latency_ms: total,
        history_engine_label: label.into(),
        total_pipeline_ms: Some(total),
        debug_info: meta.debug_info,
        whisper_ms: meta.whisper_ms,
        sanitizer_ms: meta.sanitizer_ms,
        base64_ms: meta.base64_ms,
        upload_ms: meta.upload_ms,
        files_upload_ms: meta.files_upload_ms,
        files_poll_ms: meta.files_poll_ms,
        files_poll_count: meta.files_poll_count,
        gemini_generate_ms: meta.gemini_generate_ms,
        gemini_ms: meta.gemini_generate_ms,
        gemini_transport: meta.gemini_transport,
        strict_literals_ms: meta.strict_literals_ms,
        warnings: meta.warnings,
        content_type: Some(content_note),
        ..Default::default()
    })
}

/// Builds a history entry from a mode result.
pub fn mode_result_to_history(
    id: String,
    date: String,
    audio_path: Option<String>,
    duration_ms: u64,
    source: &str,
    result: &ModePipelineResult,
) -> HistoryEntry {
    let words = result.final_text.split_whitespace().count();
    let transcription_throughput = est_throughput(words, result.transcription_latency_ms);
    let realtime_factor = compute_realtime_factor(result.transcription_latency_ms, duration_ms);

    let san_ms = result.sanitizer_ms;
    log_latency(
        result
            .transcription_latency_ms
            .saturating_sub(san_ms.unwrap_or(0)),
        san_ms.unwrap_or(0),
        duration_ms,
        realtime_factor,
        None,
        false,
        result.mode.as_str(),
    );
    log::info!(
        "modes: {} breakdown whisper={:?} sanitizer={:?} base64={:?} upload={:?} poll={:?} generate={:?} transport={:?} total={}",
        result.mode.as_str(),
        result.whisper_ms,
        result.sanitizer_ms,
        result.base64_ms,
        result.files_upload_ms,
        result.files_poll_ms,
        result.gemini_generate_ms,
        result.gemini_transport,
        result.transcription_latency_ms
    );

    let mut warnings = result.warnings.clone();
    if result.used_fallback {
        if let Some(r) = &result.fallback_reason {
            if !warnings.iter().any(|w| w == r) {
                warnings.push(r.clone());
            }
        }
    }

    HistoryEntry {
        id,
        date,
        words,
        engine: result.history_engine_label.clone(),
        text: result.final_text.clone(),
        audio_path,
        evaluation: None,
        duration_ms,
        source: source.to_string(),
        latency_ms: result.transcription_latency_ms,
        throughput: transcription_throughput.unwrap_or(0.0),
        transcription_latency_ms: Some(result.transcription_latency_ms),
        // Real sanitizer time when measured; None when mode has no sanitizer.
        sanitizer_latency_ms: san_ms,
        transcription_throughput,
        sanitizer_throughput: san_ms.and_then(|ms| {
            if ms == 0 {
                None
            } else {
                est_throughput(words, ms)
            }
        }),
        realtime_factor,
        deepgram_mode: None,
        total_tokens: Some(est_total_tokens(words)),
        is_error: Some(false),
        error_message: None,
        debug_info: result
            .debug_info
            .clone()
            .or_else(|| Some(pipeline_debug_snapshot(result))),
        mode: Some(result.mode.as_str().to_string()),
        model: Some(result.model.clone()),
        stages: Some(result.stages.join(",")),
        used_fallback: Some(result.used_fallback),
        fallback_reason: result.fallback_reason.clone(),
        content_type: result.content_type.clone(),
        whisper_text: result.whisper_text.clone(),
        sanitizer_text: result.sanitizer_text.clone(),
        gemini_text: result.gemini_text.clone(),
        warnings: if warnings.is_empty() {
            None
        } else {
            Some(warnings)
        },
        audio_prepare_ms: result.audio_prepare_ms,
        base64_ms: result.base64_ms,
        whisper_ms: result.whisper_ms,
        sanitizer_ms: result.sanitizer_ms,
        files_upload_ms: result.files_upload_ms,
        files_poll_ms: result.files_poll_ms,
        files_poll_count: result.files_poll_count,
        gemini_generate_ms: result.gemini_generate_ms,
        gemini_delete_ms: result.gemini_delete_ms,
        strict_literals_ms: result.strict_literals_ms,
        clipboard_ms: None,
        total_pipeline_ms: result
            .total_pipeline_ms
            .or(Some(result.transcription_latency_ms)),
        gemini_transport: result.gemini_transport.clone(),
    }
}

pub fn mode_failed_history(
    id: String,
    date: String,
    audio_path: Option<String>,
    duration_ms: u64,
    source: &str,
    mode: TranscriptionMode,
    error_msg: String,
) -> HistoryEntry {
    HistoryEntry {
        id,
        date,
        words: 0,
        engine: format!("{}/error", mode.as_str()),
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
        deepgram_mode: None,
        total_tokens: None,
        is_error: Some(true),
        error_message: Some(error_msg),
        debug_info: None,
        mode: Some(mode.as_str().to_string()),
        model: None,
        stages: None,
        used_fallback: Some(false),
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

pub fn should_use_product_mode(state: &AppState) -> bool {
    if !*state.modes_enabled.read() {
        return false;
    }
    matches!(
        *state.transcription_mode.read(),
        TranscriptionMode::UltraFast
            | TranscriptionMode::FastAccurate
            | TranscriptionMode::Precise
            | TranscriptionMode::UltraPrecise
    )
}

pub async fn run_product_mode(
    state: &Arc<AppState>,
    audio: Vec<u8>,
    file_name: &str,
    mime: &str,
    ext: &str,
) -> Result<ModePipelineResult, String> {
    run_product_mode_with_duration(state, audio, file_name, mime, ext, None).await
}

pub async fn run_product_mode_with_duration(
    state: &Arc<AppState>,
    audio: Vec<u8>,
    file_name: &str,
    mime: &str,
    ext: &str,
    duration_ms: Option<u64>,
) -> Result<ModePipelineResult, String> {
    let mode = *state.transcription_mode.read();
    let mut result = match mode {
        TranscriptionMode::UltraFast => {
            run_ultra_fast(state, audio, file_name, mime, duration_ms).await?
        }
        TranscriptionMode::FastAccurate => {
            run_fast_accurate(state, audio, ext, file_name, mime, duration_ms).await?
        }
        TranscriptionMode::Precise => {
            run_precise(state, audio, ext, file_name, mime, duration_ms).await?
        }
        TranscriptionMode::UltraPrecise => {
            run_ultra_precise(state, audio, ext, file_name, mime, duration_ms).await?
        }
    };
    // Global strict pass (may double-apply with in-mode; safe / idempotent).
    let vocab = state.vocabulary.read().clone();
    let ts = std::time::Instant::now();
    let (text, hits) = crate::vocabulary::apply_strict_literals(&result.final_text, &vocab);
    let strict_ms = ts.elapsed().as_millis() as u64;
    result.strict_literals_ms = Some(
        result
            .strict_literals_ms
            .unwrap_or(0)
            .saturating_add(strict_ms),
    );
    if !hits.is_empty() {
        log::info!("modes: strict literals applied ({})", hits.join(", "));
        result
            .stages
            .push(format!("strict_literals:{}", hits.len()));
        result.final_text = text;
        result.warnings.extend(hits);
    }
    Ok(result)
}

#[allow(dead_code)]
pub fn acoustic_from_whisper(text: String) -> AcousticOutcome {
    AcousticOutcome {
        whisper_text: text,
        deepgram_text: String::new(),
        effective_dual: false,
        deepgram_ran: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline_contract::TranscriptionMode;

    #[test]
    fn mode_history_fields() {
        let r = ModePipelineResult {
            final_text: "olá mundo".into(),
            mode: TranscriptionMode::UltraFast,
            model: "whisper-large-v3-turbo".into(),
            stages: vec!["whisper".into()],
            transcription_latency_ms: 42,
            history_engine_label: "UltraFast/Whisper".into(),
            whisper_ms: Some(42),
            total_pipeline_ms: Some(42),
            whisper_text: Some("olá mundo".into()),
            ..Default::default()
        };
        let h =
            mode_result_to_history("1".into(), "2026-01-01 00:00".into(), None, 1000, "mic", &r);
        assert_eq!(h.mode.as_deref(), Some("ultra-fast"));
        assert_eq!(h.whisper_ms, Some(42));
        assert_eq!(h.sanitizer_latency_ms, None);
        assert_eq!(h.total_pipeline_ms, Some(42));
        assert!(h.debug_info.is_some());
    }

    #[test]
    fn ultra_history_keeps_sanitizer_ms() {
        let r = ModePipelineResult {
            final_text: "x".into(),
            mode: TranscriptionMode::UltraPrecise,
            model: "gemini-3.5-flash".into(),
            stages: vec!["sanitizer:120ms".into()],
            transcription_latency_ms: 500,
            history_engine_label: "UltraPrecise".into(),
            sanitizer_ms: Some(120),
            whisper_ms: Some(80),
            gemini_generate_ms: Some(200),
            gemini_transport: Some("inline".into()),
            total_pipeline_ms: Some(500),
            ..Default::default()
        };
        let h = mode_result_to_history("2".into(), "d".into(), None, 1000, "mic", &r);
        assert_eq!(h.sanitizer_latency_ms, Some(120));
        assert_eq!(h.sanitizer_ms, Some(120));
        assert_eq!(h.gemini_transport.as_deref(), Some("inline"));
        assert_eq!(h.gemini_generate_ms, Some(200));
    }

    #[test]
    fn failed_mode_entry() {
        let h = mode_failed_history(
            "3".into(),
            "d".into(),
            None,
            0,
            "mic",
            TranscriptionMode::FastAccurate,
            "boom".into(),
        );
        assert_eq!(h.is_error, Some(true));
        assert!(h.sanitizer_latency_ms.is_none());
    }
}
