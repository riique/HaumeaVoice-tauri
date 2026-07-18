//! Pronunciation evaluation — preserved product path (inline audio + CEFR rubric).
//!
//! Uses base64 `inline_data` (same as the previous single-file `gemini.rs`) so
//! evaluation does not depend on the Files API. Separate from STT/refine.

use base64::{engine::general_purpose, Engine as _};

use super::client::{
    generate_content, mime_for_ext, require_api_key, Content, GenerateContentRequest, InlineData,
    Part,
};
use super::prompts::{pronunciation_prompt, PRONUNCIATION_PROMPT_VERSION};

/// Sends audio + transcript to Gemini and returns Markdown CEFR feedback.
///
/// Public surface kept stable for `commands::evaluate_pronunciation`.
pub async fn evaluate_pronunciation(
    audio_bytes: Vec<u8>,
    ext: &str,
    transcript: &str,
    api_key: &str,
) -> Result<String, String> {
    require_api_key(api_key)?;
    if audio_bytes.is_empty() {
        return Err("áudio vazio; não é possível avaliar a pronúncia".to_string());
    }

    let mime = mime_for_ext(ext);
    let encoded = general_purpose::STANDARD.encode(&audio_bytes);

    let body = GenerateContentRequest {
        contents: vec![Content {
            parts: vec![
                Part::Text {
                    text: pronunciation_prompt(transcript),
                },
                Part::Inline {
                    inline_data: InlineData {
                        mime_type: mime.to_string(),
                        data: encoded,
                    },
                },
            ],
        }],
        generation_config: None,
    };

    log::debug!(
        "gemini pronunciation: prompt_version={} mime={} bytes={}",
        PRONUNCIATION_PROMPT_VERSION,
        mime,
        audio_bytes.len()
    );

    let (feedback, _) = generate_content(api_key, &body).await?;
    if feedback.trim().is_empty() {
        return Err("o Gemini não retornou nenhum feedback".to_string());
    }
    Ok(feedback)
}
