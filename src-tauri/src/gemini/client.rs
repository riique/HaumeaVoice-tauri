//! Shared HTTP client, model ids, timeouts and response parsing.

use serde::{Deserialize, Serialize};
use std::{sync::OnceLock, time::Duration};

/// Multimodal model used for audio STT, refinement and pronunciation.
pub const GEMINI_MODEL: &str = "gemini-3.5-flash";

pub const API_ROOT: &str = "https://generativelanguage.googleapis.com/v1beta";
pub const UPLOAD_ROOT: &str = "https://generativelanguage.googleapis.com/upload/v1beta";

/// Default client idle timeout (must cover the longest single request we make).
const CLIENT_TIMEOUT: Duration = Duration::from_secs(180);

pub const TIMEOUT_UPLOAD: Duration = Duration::from_secs(60);
pub const TIMEOUT_POLL: Duration = Duration::from_secs(15);
pub const TIMEOUT_GENERATE: Duration = Duration::from_secs(120);
pub const TIMEOUT_DELETE: Duration = Duration::from_secs(15);
/// Max wall time waiting for Files API PROCESSING → ACTIVE.
pub const TIMEOUT_FILE_READY: Duration = Duration::from_secs(90);
pub const POLL_INTERVAL: Duration = Duration::from_millis(800);

static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

pub fn http_client() -> Result<&'static reqwest::Client, String> {
    if let Some(client) = HTTP_CLIENT.get() {
        return Ok(client);
    }
    let client = reqwest::Client::builder()
        .timeout(CLIENT_TIMEOUT)
        .build()
        .map_err(|e| format!("falha ao construir o cliente http do Gemini: {}", e))?;
    let _ = HTTP_CLIENT.set(client);
    HTTP_CLIENT
        .get()
        .ok_or_else(|| "falha ao inicializar o cliente http do Gemini".to_string())
}

/// Maps a file extension to a Gemini-accepted audio MIME type.
pub fn mime_for_ext(ext: &str) -> &'static str {
    let e: String = ext
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_lowercase();
    match e.as_str() {
        "mp3" => "audio/mp3",
        "m4a" | "mp4" | "aac" => "audio/aac",
        "flac" => "audio/flac",
        "ogg" | "oga" => "audio/ogg",
        "aiff" | "aif" => "audio/aiff",
        "webm" => "audio/webm",
        "wav" | "wave" => "audio/wav",
        _ => "audio/wav",
    }
}

pub fn require_api_key(api_key: &str) -> Result<(), String> {
    if api_key.trim().is_empty() {
        return Err("a chave de API do Google não está configurada".to_string());
    }
    Ok(())
}

/* --------------------------- generateContent shapes --------------------------- */

#[derive(Debug, Serialize)]
pub struct GenerateContentRequest {
    pub contents: Vec<Content>,
    /// Prefer deterministic transcription; supported on Generative Language API.
    #[serde(skip_serializing_if = "Option::is_none", rename = "generationConfig")]
    pub generation_config: Option<GenerationConfig>,
}

#[derive(Debug, Serialize)]
pub struct GenerationConfig {
    /// Near-deterministic decoding for STT / refine.
    pub temperature: f32,
}

impl GenerateContentRequest {
    pub fn with_parts(parts: Vec<Part>) -> Self {
        Self {
            contents: vec![Content { parts }],
            generation_config: Some(GenerationConfig { temperature: 0.0 }),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Content {
    pub parts: Vec<Part>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum Part {
    Text {
        text: String,
    },
    Inline {
        #[serde(rename = "inline_data")]
        inline_data: InlineData,
    },
    File {
        #[serde(rename = "file_data")]
        file_data: FileData,
    },
}

#[derive(Debug, Serialize)]
pub struct InlineData {
    pub mime_type: String,
    pub data: String,
}

#[derive(Debug, Serialize)]
pub struct FileData {
    pub mime_type: String,
    pub file_uri: String,
}

#[derive(Debug, Deserialize)]
pub struct GenerateContentResponse {
    #[serde(default)]
    pub candidates: Vec<Candidate>,
    #[serde(default)]
    pub error: Option<ApiErrorBody>,
}

#[derive(Debug, Deserialize)]
pub struct Candidate {
    #[serde(default)]
    pub content: Option<ResponseContent>,
}

#[derive(Debug, Deserialize)]
pub struct ResponseContent {
    #[serde(default)]
    pub parts: Vec<ResponsePart>,
}

#[derive(Debug, Deserialize)]
pub struct ResponsePart {
    #[serde(default)]
    pub text: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ApiErrorBody {
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub code: Option<i64>,
    #[serde(default)]
    #[allow(dead_code)]
    pub status: Option<String>,
}

/// Extracts concatenated text from a successful generateContent JSON body.
pub fn extract_text(parsed: GenerateContentResponse) -> Result<String, String> {
    if let Some(err) = parsed.error {
        let msg = err.message.unwrap_or_else(|| "erro desconhecido".into());
        return Err(format!(
            "Gemini API error{}: {}",
            err.code.map(|c| format!(" ({c})")).unwrap_or_default(),
            msg
        ));
    }

    let text = parsed
        .candidates
        .into_iter()
        .next()
        .and_then(|c| c.content)
        .map(|c| {
            c.parts
                .into_iter()
                .filter_map(|p| p.text)
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();

    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        return Err("o Gemini não retornou texto".to_string());
    }
    Ok(trimmed)
}

pub fn generate_url(model: &str, api_key: &str) -> String {
    format!(
        "{}/models/{}:generateContent?key={}",
        API_ROOT, model, api_key
    )
}

/// POST generateContent with a per-request timeout override.
/// Returns `(text, generate_wall_ms)`.
pub async fn generate_content(
    api_key: &str,
    body: &GenerateContentRequest,
) -> Result<(String, u64), String> {
    require_api_key(api_key)?;
    let client = http_client()?;
    let url = generate_url(GEMINI_MODEL, api_key);
    let t0 = std::time::Instant::now();

    let response = tokio::time::timeout(TIMEOUT_GENERATE, client.post(url).json(body).send())
        .await
        .map_err(|_| {
            format!(
                "timeout ao gerar conteúdo no Gemini ({}s)",
                TIMEOUT_GENERATE.as_secs()
            )
        })?
        .map_err(|e| format!("falha na requisição ao Gemini: {}", e))?;

    let status = response.status().as_u16();
    let body_text = response.text().await.unwrap_or_default();
    let generate_ms = t0.elapsed().as_millis() as u64;
    if status != 200 {
        return Err(format!("Gemini retornou status {}: {}", status, body_text));
    }

    let parsed: GenerateContentResponse = serde_json::from_str(&body_text)
        .map_err(|e| format!("falha ao interpretar a resposta do Gemini: {}", e))?;
    let text = extract_text(parsed)?;
    Ok((text, generate_ms))
}

/// Build generateContent body with text prompt + inline Base64 audio.
pub fn build_inline_request(prompt: &str, mime: &str, base64_data: &str) -> GenerateContentRequest {
    GenerateContentRequest::with_parts(vec![
        Part::Text {
            text: prompt.to_string(),
        },
        Part::Inline {
            inline_data: InlineData {
                mime_type: mime.to_string(),
                data: base64_data.to_string(),
            },
        },
    ])
}

/// Build generateContent body with text prompt + remote file_data.
pub fn build_file_request(prompt: &str, mime: &str, file_uri: &str) -> GenerateContentRequest {
    GenerateContentRequest::with_parts(vec![
        Part::Text {
            text: prompt.to_string(),
        },
        Part::File {
            file_data: FileData {
                mime_type: mime.to_string(),
                file_uri: file_uri.to_string(),
            },
        },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_mapping() {
        assert_eq!(mime_for_ext("wav"), "audio/wav");
        assert_eq!(mime_for_ext("MP3"), "audio/mp3");
        assert_eq!(mime_for_ext("m4a"), "audio/aac");
        assert_eq!(mime_for_ext("flac"), "audio/flac");
        assert_eq!(mime_for_ext("unknown"), "audio/wav");
    }

    #[test]
    fn extract_text_from_candidates() {
        let json = r#"{
            "candidates": [{
                "content": { "parts": [{ "text": "  olá mundo  " }] }
            }]
        }"#;
        let parsed: GenerateContentResponse = serde_json::from_str(json).unwrap();
        assert_eq!(extract_text(parsed).unwrap(), "olá mundo");
    }

    #[test]
    fn extract_text_api_error() {
        let json = r#"{ "error": { "code": 403, "message": "permission denied" } }"#;
        let parsed: GenerateContentResponse = serde_json::from_str(json).unwrap();
        let err = extract_text(parsed).unwrap_err();
        assert!(err.contains("403"));
        assert!(err.contains("permission denied"));
    }

    #[test]
    fn empty_key_rejected() {
        assert!(require_api_key("").is_err());
        assert!(require_api_key("   ").is_err());
    }
}
