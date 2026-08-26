//! Audio + draft text → refined transcript via Gemini multimodal.

use base64::{engine::general_purpose, Engine as _};

use super::client::{
    adaptive_generate_timeout, build_file_request, build_inline_request, mime_for_ext, GEMINI_MODEL,
};
use super::files::{spawn_cleanup, upload_and_wait};
use super::prompts::{
    precise_refinement_prompt, refinement_prompt, transcription_prompt,
    ultraprecise_refinement_prompt, GeminiPrompt, PRECISE_PROMPT_VERSION, REFINE_PROMPT_VERSION,
    TRANSCRIBE_PROMPT_VERSION, ULTRAPRECISE_PROMPT_VERSION,
};
use super::transport::{
    estimate_wav_duration_ms, select_gemini_audio_transport, GeminiAudioTransport,
};
use super::types::{
    GeminiFileRef, GeminiGenerateResult, GeminiOperation, GeminiStageTiming, RefineRequest,
};

/// Encode audio once; returns (base64, base64_ms).
pub fn encode_audio_base64(bytes: &[u8]) -> (String, u64) {
    let t0 = std::time::Instant::now();
    let b64 = general_purpose::STANDARD.encode(bytes);
    (b64, t0.elapsed().as_millis() as u64)
}

/// Refines a draft transcription against the source audio (hybrid transport).
pub async fn refine_with_audio(req: RefineRequest) -> Result<GeminiGenerateResult, String> {
    let t0 = std::time::Instant::now();
    let mime = mime_for_ext(&req.ext);
    let display = if req.display_name.trim().is_empty() {
        format!("haumea-refine.{}", req.ext)
    } else {
        req.display_name.clone()
    };
    let duration = req
        .duration_ms
        .or_else(|| estimate_wav_duration_ms(&req.audio_bytes));
    let transport = select_gemini_audio_transport(req.audio_bytes.len(), duration, mime)?;
    let prompt = refinement_prompt(&req.draft_text, req.file_tagging_enabled);

    let mut out = generate_with_transport(
        &req.api_key,
        GEMINI_MODEL,
        &req.audio_bytes,
        mime,
        &display,
        &prompt,
        transport,
        GeminiOperation::Refine,
        REFINE_PROMPT_VERSION,
        duration,
        None,
    )
    .await?;
    out.latency_ms = t0.elapsed().as_millis() as u64;
    Ok(out)
}

/// Precise-mode refine: hybrid. Prefer precomputed base64 for parallel prep.
pub async fn refine_precise(
    api_key: &str,
    model: &str,
    audio: &[u8],
    ext: &str,
    display_name: &str,
    whisper_hypothesis: &str,
    glossary_block: &str,
    file_tagging_enabled: bool,
    duration_ms: Option<u64>,
    precomputed_b64: Option<(String, u64)>,
) -> Result<GeminiGenerateResult, String> {
    let mime = mime_for_ext(ext);
    let duration = duration_ms.or_else(|| estimate_wav_duration_ms(audio));
    let transport = select_gemini_audio_transport(audio.len(), duration, mime)?;
    let prompt =
        precise_refinement_prompt(whisper_hypothesis, glossary_block, file_tagging_enabled);
    generate_with_transport(
        api_key,
        model,
        audio,
        mime,
        display_name,
        &prompt,
        transport,
        GeminiOperation::Refine,
        PRECISE_PROMPT_VERSION,
        duration,
        precomputed_b64,
    )
    .await
}

/// Precise-mode refine using an **already uploaded** remote file (caller owns cleanup).
pub async fn refine_precise_with_file(
    api_key: &str,
    model: &str,
    file: &GeminiFileRef,
    whisper_hypothesis: &str,
    glossary_block: &str,
    file_tagging_enabled: bool,
    duration_ms: Option<u64>,
    audio_bytes: usize,
) -> Result<GeminiGenerateResult, String> {
    let prompt =
        precise_refinement_prompt(whisper_hypothesis, glossary_block, file_tagging_enabled);
    generate_with_remote_file(
        api_key,
        model,
        file,
        &prompt,
        GeminiOperation::Refine,
        PRECISE_PROMPT_VERSION,
        adaptive_generate_timeout(duration_ms, audio_bytes),
    )
    .await
}

/// Pure STT against an already-uploaded file (Whisper failed path in Precise mode).
pub async fn transcribe_with_file(
    api_key: &str,
    model: &str,
    file: &GeminiFileRef,
    file_tagging_enabled: bool,
    untrusted_context: Option<&str>,
    duration_ms: Option<u64>,
    audio_bytes: usize,
) -> Result<GeminiGenerateResult, String> {
    let mut prompt = transcription_prompt(file_tagging_enabled);
    if let Some(context) = untrusted_context {
        prompt.user_prompt.push_str("\n\n");
        prompt.user_prompt.push_str(context);
    }
    generate_with_remote_file(
        api_key,
        model,
        file,
        &prompt,
        GeminiOperation::Transcribe,
        TRANSCRIBE_PROMPT_VERSION,
        adaptive_generate_timeout(duration_ms, audio_bytes),
    )
    .await
}

/// Pure STT inline (Whisper failed + short audio).
pub async fn transcribe_inline(
    api_key: &str,
    model: &str,
    audio: &[u8],
    mime: &str,
    file_tagging_enabled: bool,
    untrusted_context: Option<&str>,
    precomputed_b64: Option<(String, u64)>,
) -> Result<GeminiGenerateResult, String> {
    let (b64, base64_ms) = match precomputed_b64 {
        Some(p) => p,
        None => encode_audio_base64(audio),
    };
    let mut prompt = transcription_prompt(file_tagging_enabled);
    if let Some(context) = untrusted_context {
        prompt.user_prompt.push_str("\n\n");
        prompt.user_prompt.push_str(context);
    }
    let body = build_inline_request(&prompt.system_instruction, &prompt.user_prompt, mime, &b64);
    let duration = estimate_wav_duration_ms(audio);
    let timeout = adaptive_generate_timeout(duration, audio.len());
    let generated =
        super::client::generate_content_with_model(api_key, model, &body, timeout).await?;
    Ok(GeminiGenerateResult {
        operation: GeminiOperation::Transcribe,
        text: generated.text,
        model: model.to_string(),
        prompt_version: TRANSCRIBE_PROMPT_VERSION.to_string(),
        latency_ms: generated.duration_ms + base64_ms,
        remote_file_name: None,
        transport: Some(GeminiAudioTransport::Inline),
        timing: GeminiStageTiming {
            base64_ms: Some(base64_ms),
            generate_ms: Some(generated.duration_ms),
            ..Default::default()
        },
        usage: generated.usage,
    })
}

/// UltraPrecise refine (hybrid).
pub async fn refine_ultraprecise(
    api_key: &str,
    model: &str,
    audio: &[u8],
    ext: &str,
    display_name: &str,
    whisper_raw: &str,
    sanitized: &str,
    glossary_block: &str,
    file_tagging_enabled: bool,
    duration_ms: Option<u64>,
    precomputed_b64: Option<(String, u64)>,
) -> Result<GeminiGenerateResult, String> {
    let mime = mime_for_ext(ext);
    let duration = duration_ms.or_else(|| estimate_wav_duration_ms(audio));
    let transport = select_gemini_audio_transport(audio.len(), duration, mime)?;
    let prompt = ultraprecise_refinement_prompt(
        whisper_raw,
        sanitized,
        glossary_block,
        file_tagging_enabled,
    );
    generate_with_transport(
        api_key,
        model,
        audio,
        mime,
        display_name,
        &prompt,
        transport,
        GeminiOperation::Refine,
        ULTRAPRECISE_PROMPT_VERSION,
        duration,
        precomputed_b64,
    )
    .await
}

/// UltraPrecise refine using an already-uploaded remote file (caller owns cleanup).
pub async fn refine_ultraprecise_with_file(
    api_key: &str,
    model: &str,
    file: &GeminiFileRef,
    whisper_raw: &str,
    sanitized: &str,
    glossary_block: &str,
    file_tagging_enabled: bool,
    duration_ms: Option<u64>,
    audio_bytes: usize,
) -> Result<GeminiGenerateResult, String> {
    let prompt = ultraprecise_refinement_prompt(
        whisper_raw,
        sanitized,
        glossary_block,
        file_tagging_enabled,
    );
    generate_with_remote_file(
        api_key,
        model,
        file,
        &prompt,
        GeminiOperation::Refine,
        ULTRAPRECISE_PROMPT_VERSION,
        adaptive_generate_timeout(duration_ms, audio_bytes),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn generate_with_transport(
    api_key: &str,
    model: &str,
    audio: &[u8],
    mime: &str,
    display_name: &str,
    prompt: &GeminiPrompt,
    transport: GeminiAudioTransport,
    operation: GeminiOperation,
    prompt_version: &str,
    duration_ms: Option<u64>,
    precomputed_b64: Option<(String, u64)>,
) -> Result<GeminiGenerateResult, String> {
    let t0 = std::time::Instant::now();
    let generate_timeout = adaptive_generate_timeout(duration_ms, audio.len());
    match transport {
        GeminiAudioTransport::Inline => {
            let (b64, base64_ms) = match precomputed_b64 {
                Some(p) => p,
                None => encode_audio_base64(audio),
            };
            let body =
                build_inline_request(&prompt.system_instruction, &prompt.user_prompt, mime, &b64);
            let generated =
                super::client::generate_content_with_model(api_key, model, &body, generate_timeout)
                    .await?;
            Ok(GeminiGenerateResult {
                operation,
                text: generated.text,
                model: model.to_string(),
                prompt_version: prompt_version.to_string(),
                latency_ms: t0.elapsed().as_millis() as u64,
                remote_file_name: None,
                transport: Some(GeminiAudioTransport::Inline),
                timing: GeminiStageTiming {
                    base64_ms: Some(base64_ms),
                    generate_ms: Some(generated.duration_ms),
                    ..Default::default()
                },
                usage: generated.usage,
            })
        }
        GeminiAudioTransport::FilesApi => {
            let (guard, up) = upload_and_wait(api_key, audio, mime, display_name).await?;
            let name = guard.name().to_string();
            let body = build_file_request(
                &prompt.system_instruction,
                &prompt.user_prompt,
                guard.mime_type(),
                guard.uri(),
            );
            let gen =
                super::client::generate_content_with_model(api_key, model, &body, generate_timeout)
                    .await;
            spawn_cleanup(guard);
            let generated = gen?;
            Ok(GeminiGenerateResult {
                operation,
                text: generated.text,
                model: model.to_string(),
                prompt_version: prompt_version.to_string(),
                latency_ms: t0.elapsed().as_millis() as u64,
                remote_file_name: Some(name),
                transport: Some(GeminiAudioTransport::FilesApi),
                timing: GeminiStageTiming {
                    files_upload_ms: Some(up.upload_ms),
                    files_poll_ms: Some(up.poll_ms),
                    files_poll_count: Some(up.poll_count),
                    generate_ms: Some(generated.duration_ms),
                    delete_ms: None,
                    ..Default::default()
                },
                usage: generated.usage,
            })
        }
    }
}

async fn generate_with_remote_file(
    api_key: &str,
    model: &str,
    file: &GeminiFileRef,
    prompt: &GeminiPrompt,
    operation: GeminiOperation,
    prompt_version: &str,
    generate_timeout: std::time::Duration,
) -> Result<GeminiGenerateResult, String> {
    let body = build_file_request(
        &prompt.system_instruction,
        &prompt.user_prompt,
        &file.mime_type,
        &file.uri,
    );
    let generated =
        super::client::generate_content_with_model(api_key, model, &body, generate_timeout).await?;
    Ok(GeminiGenerateResult {
        operation,
        text: generated.text,
        model: model.to_string(),
        prompt_version: prompt_version.to_string(),
        latency_ms: generated.duration_ms,
        remote_file_name: Some(file.name.clone()),
        transport: Some(GeminiAudioTransport::FilesApi),
        timing: GeminiStageTiming {
            generate_ms: Some(generated.duration_ms),
            ..Default::default()
        },
        usage: generated.usage,
    })
}
