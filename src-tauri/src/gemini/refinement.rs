//! Audio + draft text → refined transcript via Gemini multimodal.

use base64::{engine::general_purpose, Engine as _};

use super::client::{
    build_file_request, build_inline_request, generate_content, mime_for_ext, GEMINI_MODEL,
};
use super::files::{spawn_cleanup, upload_and_wait};
use super::prompts::{
    precise_refinement_prompt, refinement_prompt, transcription_prompt,
    ultraprecise_refinement_prompt, PRECISE_PROMPT_VERSION, REFINE_PROMPT_VERSION,
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
    let prompt = refinement_prompt(&req.draft_text);

    let mut out = generate_with_transport(
        &req.api_key,
        &req.audio_bytes,
        mime,
        &display,
        &prompt,
        transport,
        GeminiOperation::Refine,
        REFINE_PROMPT_VERSION,
        None,
    )
    .await?;
    out.latency_ms = t0.elapsed().as_millis() as u64;
    Ok(out)
}

/// Precise-mode refine: hybrid. Prefer precomputed base64 for parallel prep.
pub async fn refine_precise(
    api_key: &str,
    audio: &[u8],
    ext: &str,
    display_name: &str,
    whisper_hypothesis: &str,
    glossary_block: &str,
    duration_ms: Option<u64>,
    precomputed_b64: Option<(String, u64)>,
) -> Result<GeminiGenerateResult, String> {
    let mime = mime_for_ext(ext);
    let duration = duration_ms.or_else(|| estimate_wav_duration_ms(audio));
    let transport = select_gemini_audio_transport(audio.len(), duration, mime)?;
    let content_note = crate::sanitizer_json::detect_content_type(whisper_hypothesis).as_str();
    let prompt = precise_refinement_prompt(whisper_hypothesis, glossary_block, content_note);
    generate_with_transport(
        api_key,
        audio,
        mime,
        display_name,
        &prompt,
        transport,
        GeminiOperation::Refine,
        PRECISE_PROMPT_VERSION,
        precomputed_b64,
    )
    .await
}

/// Precise-mode refine using an **already uploaded** remote file (caller owns cleanup).
pub async fn refine_precise_with_file(
    api_key: &str,
    file: &GeminiFileRef,
    whisper_hypothesis: &str,
    glossary_block: &str,
) -> Result<GeminiGenerateResult, String> {
    let content_note = crate::sanitizer_json::detect_content_type(whisper_hypothesis).as_str();
    let prompt = precise_refinement_prompt(whisper_hypothesis, glossary_block, content_note);
    generate_with_remote_file(
        api_key,
        file,
        &prompt,
        GeminiOperation::Refine,
        PRECISE_PROMPT_VERSION,
    )
    .await
}

/// Pure STT against an already-uploaded file (Whisper failed path in Precise mode).
pub async fn transcribe_with_file(
    api_key: &str,
    file: &GeminiFileRef,
) -> Result<GeminiGenerateResult, String> {
    generate_with_remote_file(
        api_key,
        file,
        transcription_prompt(),
        GeminiOperation::Transcribe,
        TRANSCRIBE_PROMPT_VERSION,
    )
    .await
}

/// Pure STT inline (Whisper failed + short audio).
pub async fn transcribe_inline(
    api_key: &str,
    audio: &[u8],
    mime: &str,
    precomputed_b64: Option<(String, u64)>,
) -> Result<GeminiGenerateResult, String> {
    let (b64, base64_ms) = match precomputed_b64 {
        Some(p) => p,
        None => encode_audio_base64(audio),
    };
    let body = build_inline_request(transcription_prompt(), mime, &b64);
    let (text, generate_ms) = generate_content(api_key, &body).await?;
    Ok(GeminiGenerateResult {
        operation: GeminiOperation::Transcribe,
        text,
        model: GEMINI_MODEL.to_string(),
        prompt_version: TRANSCRIBE_PROMPT_VERSION.to_string(),
        latency_ms: generate_ms + base64_ms,
        remote_file_name: None,
        transport: Some(GeminiAudioTransport::Inline),
        timing: GeminiStageTiming {
            base64_ms: Some(base64_ms),
            generate_ms: Some(generate_ms),
            ..Default::default()
        },
    })
}

/// UltraPrecise refine (hybrid).
pub async fn refine_ultraprecise(
    api_key: &str,
    audio: &[u8],
    ext: &str,
    display_name: &str,
    whisper_raw: &str,
    sanitized: &str,
    glossary_block: &str,
    content_note: &str,
    duration_ms: Option<u64>,
    precomputed_b64: Option<(String, u64)>,
) -> Result<GeminiGenerateResult, String> {
    let mime = mime_for_ext(ext);
    let duration = duration_ms.or_else(|| estimate_wav_duration_ms(audio));
    let transport = select_gemini_audio_transport(audio.len(), duration, mime)?;
    let prompt =
        ultraprecise_refinement_prompt(whisper_raw, sanitized, glossary_block, content_note);
    generate_with_transport(
        api_key,
        audio,
        mime,
        display_name,
        &prompt,
        transport,
        GeminiOperation::Refine,
        ULTRAPRECISE_PROMPT_VERSION,
        precomputed_b64,
    )
    .await
}

/// UltraPrecise refine using an already-uploaded remote file (caller owns cleanup).
pub async fn refine_ultraprecise_with_file(
    api_key: &str,
    file: &GeminiFileRef,
    whisper_raw: &str,
    sanitized: &str,
    glossary_block: &str,
    content_note: &str,
) -> Result<GeminiGenerateResult, String> {
    let prompt =
        ultraprecise_refinement_prompt(whisper_raw, sanitized, glossary_block, content_note);
    generate_with_remote_file(
        api_key,
        file,
        &prompt,
        GeminiOperation::Refine,
        ULTRAPRECISE_PROMPT_VERSION,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn generate_with_transport(
    api_key: &str,
    audio: &[u8],
    mime: &str,
    display_name: &str,
    prompt: &str,
    transport: GeminiAudioTransport,
    operation: GeminiOperation,
    prompt_version: &str,
    precomputed_b64: Option<(String, u64)>,
) -> Result<GeminiGenerateResult, String> {
    let t0 = std::time::Instant::now();
    match transport {
        GeminiAudioTransport::Inline => {
            let (b64, base64_ms) = match precomputed_b64 {
                Some(p) => p,
                None => encode_audio_base64(audio),
            };
            let body = build_inline_request(prompt, mime, &b64);
            let (text, generate_ms) = generate_content(api_key, &body).await?;
            Ok(GeminiGenerateResult {
                operation,
                text,
                model: GEMINI_MODEL.to_string(),
                prompt_version: prompt_version.to_string(),
                latency_ms: t0.elapsed().as_millis() as u64,
                remote_file_name: None,
                transport: Some(GeminiAudioTransport::Inline),
                timing: GeminiStageTiming {
                    base64_ms: Some(base64_ms),
                    generate_ms: Some(generate_ms),
                    ..Default::default()
                },
            })
        }
        GeminiAudioTransport::FilesApi => {
            let (guard, up) = upload_and_wait(api_key, audio, mime, display_name).await?;
            let name = guard.name().to_string();
            let body = build_file_request(prompt, guard.mime_type(), guard.uri());
            let gen = generate_content(api_key, &body).await;
            spawn_cleanup(guard);
            let (text, generate_ms) = gen?;
            Ok(GeminiGenerateResult {
                operation,
                text,
                model: GEMINI_MODEL.to_string(),
                prompt_version: prompt_version.to_string(),
                latency_ms: t0.elapsed().as_millis() as u64,
                remote_file_name: Some(name),
                transport: Some(GeminiAudioTransport::FilesApi),
                timing: GeminiStageTiming {
                    files_upload_ms: Some(up.upload_ms),
                    files_poll_ms: Some(up.poll_ms),
                    files_poll_count: Some(up.poll_count),
                    generate_ms: Some(generate_ms),
                    delete_ms: None,
                    ..Default::default()
                },
            })
        }
    }
}

async fn generate_with_remote_file(
    api_key: &str,
    file: &GeminiFileRef,
    prompt: &str,
    operation: GeminiOperation,
    prompt_version: &str,
) -> Result<GeminiGenerateResult, String> {
    let body = build_file_request(prompt, &file.mime_type, &file.uri);
    let (text, generate_ms) = generate_content(api_key, &body).await?;
    Ok(GeminiGenerateResult {
        operation,
        text,
        model: GEMINI_MODEL.to_string(),
        prompt_version: prompt_version.to_string(),
        latency_ms: generate_ms,
        remote_file_name: Some(file.name.clone()),
        transport: Some(GeminiAudioTransport::FilesApi),
        timing: GeminiStageTiming {
            generate_ms: Some(generate_ms),
            ..Default::default()
        },
    })
}
