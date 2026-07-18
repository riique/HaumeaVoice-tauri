//! Audio → text via Gemini multimodal (inline Base64 or Files API).

use base64::{engine::general_purpose, Engine as _};

use super::client::{
    build_file_request, build_inline_request, generate_content, mime_for_ext, GEMINI_MODEL,
};
use super::files::{spawn_cleanup, upload_and_wait};
use super::prompts::{fast_accurate_transcription_prompt, TRANSCRIBE_PROMPT_VERSION};
use super::transport::{
    estimate_wav_duration_ms, select_gemini_audio_transport, GeminiAudioTransport,
};
use super::types::{GeminiGenerateResult, GeminiOperation, GeminiStageTiming, TranscribeRequest};

/// Transcribes audio with Gemini. Short clips: inline Base64. Large: Files API.
pub async fn transcribe_audio(req: TranscribeRequest) -> Result<GeminiGenerateResult, String> {
    let t0 = std::time::Instant::now();
    let mime = mime_for_ext(&req.ext);
    let display = if req.display_name.trim().is_empty() {
        format!("haumea-stt.{}", req.ext)
    } else {
        req.display_name.clone()
    };

    let duration = req
        .duration_ms
        .or_else(|| estimate_wav_duration_ms(&req.audio_bytes));
    let transport = select_gemini_audio_transport(req.audio_bytes.len(), duration, mime)?;
    let prompt = fast_accurate_transcription_prompt(&req.glossary_block, &req.content_note);

    let (text, timing, remote) = match transport {
        GeminiAudioTransport::Inline => {
            let tb = std::time::Instant::now();
            let b64 = general_purpose::STANDARD.encode(&req.audio_bytes);
            let base64_ms = tb.elapsed().as_millis() as u64;
            let body = build_inline_request(&prompt, mime, &b64);
            let (text, generate_ms) = generate_content(&req.api_key, &body).await?;
            (
                text,
                GeminiStageTiming {
                    base64_ms: Some(base64_ms),
                    generate_ms: Some(generate_ms),
                    ..Default::default()
                },
                None,
            )
        }
        GeminiAudioTransport::FilesApi => {
            let (guard, up) =
                upload_and_wait(&req.api_key, &req.audio_bytes, mime, &display).await?;
            let name = guard.name().to_string();
            let body = build_file_request(&prompt, guard.mime_type(), guard.uri());
            let gen = generate_content(&req.api_key, &body).await;
            spawn_cleanup(guard);
            let (text, generate_ms) = gen?;
            (
                text,
                GeminiStageTiming {
                    files_upload_ms: Some(up.upload_ms),
                    files_poll_ms: Some(up.poll_ms),
                    files_poll_count: Some(up.poll_count),
                    generate_ms: Some(generate_ms),
                    delete_ms: None, // async off critical path
                    ..Default::default()
                },
                Some(name),
            )
        }
    };

    Ok(GeminiGenerateResult {
        operation: GeminiOperation::Transcribe,
        text,
        model: GEMINI_MODEL.to_string(),
        prompt_version: TRANSCRIBE_PROMPT_VERSION.to_string(),
        latency_ms: t0.elapsed().as_millis() as u64,
        remote_file_name: remote,
        transport: Some(transport),
        timing,
    })
}
