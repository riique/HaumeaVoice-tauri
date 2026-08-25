//! OpenRouter multimodal Gemini client using Chat Completions `input_audio`.

use base64::{engine::general_purpose, Engine as _};
use reqwest::header::{HeaderMap, HeaderValue, REFERER};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

const CHAT_COMPLETIONS_URL: &str = "https://openrouter.ai/api/v1/chat/completions";
const TRANSCRIPTIONS_URL: &str = "https://openrouter.ai/api/v1/audio/transcriptions";
const STT_MODELS_URL: &str = "https://openrouter.ai/api/v1/models?output_modalities=transcription";
const APP_REFERER: &str = "https://haumea.fun/haumea-voice";
const APP_TITLE: &str = "Haumea Voice";
static STT_MODEL_IDS: tokio::sync::OnceCell<HashSet<String>> = tokio::sync::OnceCell::const_new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenRouterAudioRoute {
    MultimodalLlm,
    SpeechToText,
}

impl OpenRouterAudioRoute {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MultimodalLlm => "multimodal-llm",
            Self::SpeechToText => "speech-to-text",
        }
    }
}

fn app_attribution_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(REFERER, HeaderValue::from_static(APP_REFERER));
    headers.insert("X-OpenRouter-Title", HeaderValue::from_static(APP_TITLE));
    headers
}

#[derive(Debug, Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<Message<'a>>,
    stream: bool,
}

#[derive(Debug, Serialize)]
struct Message<'a> {
    role: &'static str,
    content: Vec<ContentPart<'a>>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum ContentPart<'a> {
    #[serde(rename = "text")]
    Text { text: &'a str },
    #[serde(rename = "input_audio")]
    InputAudio { input_audio: InputAudio<'a> },
}

#[derive(Debug, Serialize)]
struct InputAudio<'a> {
    data: &'a str,
    format: &'a str,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    #[serde(default)]
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Option<ApiUsage>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ResponseMessage {
    #[serde(default)]
    content: String,
}

#[derive(Debug, Serialize)]
struct TranscriptionRequest<'a> {
    model: &'a str,
    input_audio: InputAudio<'a>,
    /// Portuguese is the product's primary language. Providers may still
    /// preserve code-switching and technical identifiers in the transcript.
    language: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<ProviderPreferences<'a>>,
}

#[derive(Debug, Serialize)]
struct ProviderPreferences<'a> {
    only: [&'a str; 1],
    allow_fallbacks: bool,
}

#[derive(Debug, Deserialize)]
struct TranscriptionResponse {
    #[serde(default)]
    text: String,
    #[serde(default)]
    usage: Option<ApiUsage>,
}

#[derive(Debug, Deserialize)]
struct ApiUsage {
    #[serde(default)]
    total_tokens: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct ModelCatalogResponse {
    #[serde(default)]
    data: Vec<ModelCatalogEntry>,
}

#[derive(Debug, Deserialize)]
struct ModelCatalogEntry {
    id: String,
}

#[derive(Debug, Clone)]
pub struct OpenRouterGenerateResult {
    pub text: String,
    pub base64_ms: u64,
    pub request_ms: u64,
    pub generation_id: Option<String>,
    pub reported_total_tokens: Option<usize>,
}

fn model_looks_dedicated_stt(model: &str) -> bool {
    let id = model.to_ascii_lowercase();
    id.contains("whisper") || id.contains("chirp") || id.contains("transcribe")
}

fn whisper_provider_preferences(model: &str) -> Option<ProviderPreferences<'static>> {
    model
        .to_ascii_lowercase()
        .contains("whisper")
        .then_some(ProviderPreferences {
            only: ["groq"],
            allow_fallbacks: false,
        })
}

async fn fetch_stt_model_ids() -> Result<HashSet<String>, String> {
    let response = tokio::time::timeout(
        Duration::from_secs(10),
        reqwest::Client::new().get(STT_MODELS_URL).send(),
    )
    .await
    .map_err(|_| "timeout ao consultar o catálogo de modelos STT".to_string())?
    .map_err(|e| format!("falha ao consultar o catálogo de modelos STT: {e}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!(
            "catálogo de modelos STT retornou status {}",
            status.as_u16()
        ));
    }
    let catalog: ModelCatalogResponse = response
        .json()
        .await
        .map_err(|e| format!("catálogo de modelos STT inválido: {e}"))?;
    Ok(catalog.data.into_iter().map(|entry| entry.id).collect())
}

/// Automatically selects the OpenRouter audio endpoint. Dedicated STT models
/// are discovered from the provider's official model catalog and common STT
/// ids are recognized locally so Chirp/Whisper do not need a discovery round
/// trip. A catalog outage falls back to the multimodal endpoint.
pub async fn detect_audio_route(model: &str) -> OpenRouterAudioRoute {
    if model_looks_dedicated_stt(model) {
        return OpenRouterAudioRoute::SpeechToText;
    }
    if model.to_ascii_lowercase().starts_with("google/gemini-") {
        return OpenRouterAudioRoute::MultimodalLlm;
    }
    match STT_MODEL_IDS.get_or_try_init(fetch_stt_model_ids).await {
        Ok(ids) if ids.contains(model) => OpenRouterAudioRoute::SpeechToText,
        Ok(_) => OpenRouterAudioRoute::MultimodalLlm,
        Err(error) => {
            log::warn!(
                "OpenRouter: não foi possível detectar automaticamente o endpoint de {}: {}. Usando multimodal.",
                model,
                error
            );
            OpenRouterAudioRoute::MultimodalLlm
        }
    }
}

pub async fn generate_with_audio(
    audio: &[u8],
    ext: &str,
    prompt: &str,
    model: &str,
    api_key: &str,
    timeout: Duration,
) -> Result<OpenRouterGenerateResult, String> {
    if audio.is_empty() {
        return Err("OpenRouter Gemini: o áudio está vazio.".into());
    }
    if api_key.trim().is_empty() {
        return Err("Configure uma chave do OpenRouter em Provedores e APIs.".into());
    }

    let format = audio_format(ext)?;
    let encode_started = Instant::now();
    let encoded = general_purpose::STANDARD.encode(audio);
    let base64_ms = encode_started.elapsed().as_millis() as u64;
    let body = ChatRequest {
        model,
        messages: vec![Message {
            role: "user",
            content: vec![
                ContentPart::Text { text: prompt },
                ContentPart::InputAudio {
                    input_audio: InputAudio {
                        data: &encoded,
                        format,
                    },
                },
            ],
        }],
        stream: false,
    };

    let started = Instant::now();
    let exchange = async {
        let response = reqwest::Client::new()
            .post(CHAT_COMPLETIONS_URL)
            .bearer_auth(api_key.trim())
            .headers(app_attribution_headers())
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("OpenRouter Gemini: falha de rede: {e}"))?;
        let status = response.status();
        let generation_id = response
            .headers()
            .get("x-generation-id")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let response_body = response
            .text()
            .await
            .map_err(|e| format!("OpenRouter Gemini: falha ao ler a resposta: {e}"))?;
        Ok::<_, String>((status, generation_id, response_body))
    };
    let (status, generation_id, response_body) = tokio::time::timeout(timeout, exchange)
        .await
        .map_err(|_| {
        format!(
            "OpenRouter Gemini: timeout após {} segundos.",
            timeout.as_secs()
        )
    })??;
    let request_ms = started.elapsed().as_millis() as u64;
    if !status.is_success() {
        return Err(format!(
            "OpenRouter Gemini retornou status {}: {}",
            status.as_u16(),
            truncate_error(&response_body, 800)
        ));
    }
    let parsed: ChatResponse = serde_json::from_str(&response_body)
        .map_err(|e| format!("OpenRouter Gemini: resposta inválida: {e}"))?;
    let reported_total_tokens = parsed.usage.and_then(|usage| usage.total_tokens);
    let text = parsed
        .choices
        .into_iter()
        .next()
        .map(|choice| choice.message.content.trim().to_string())
        .unwrap_or_default();
    if text.is_empty() {
        return Err("OpenRouter Gemini não retornou texto.".into());
    }
    Ok(OpenRouterGenerateResult {
        text,
        base64_ms,
        request_ms,
        generation_id,
        reported_total_tokens,
    })
}

/// Sends audio to OpenRouter's dedicated speech-to-text endpoint. Unlike
/// [`generate_with_audio`], this contract accepts no prompt and must only be
/// used with models whose output modality is `transcription`.
pub async fn transcribe_audio(
    audio: &[u8],
    ext: &str,
    model: &str,
    api_key: &str,
    timeout: Duration,
) -> Result<OpenRouterGenerateResult, String> {
    if audio.is_empty() {
        return Err("OpenRouter STT: o áudio está vazio.".into());
    }
    if api_key.trim().is_empty() {
        return Err("Configure uma chave do OpenRouter em Provedores e APIs.".into());
    }

    let format = audio_format(ext)?;
    let encode_started = Instant::now();
    let encoded = general_purpose::STANDARD.encode(audio);
    let base64_ms = encode_started.elapsed().as_millis() as u64;
    let body = TranscriptionRequest {
        model,
        input_audio: InputAudio {
            data: &encoded,
            format,
        },
        language: "pt",
        // Product rule: every Whisper model sent through OpenRouter is pinned
        // to Groq, with no provider fallback.
        provider: whisper_provider_preferences(model),
    };

    let started = Instant::now();
    let exchange = async {
        let response = reqwest::Client::new()
            .post(TRANSCRIPTIONS_URL)
            .bearer_auth(api_key.trim())
            .headers(app_attribution_headers())
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("OpenRouter STT: falha de rede: {e}"))?;
        let status = response.status();
        let generation_id = response
            .headers()
            .get("x-generation-id")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let response_body = response
            .text()
            .await
            .map_err(|e| format!("OpenRouter STT: falha ao ler a resposta: {e}"))?;
        Ok::<_, String>((status, generation_id, response_body))
    };
    let (status, generation_id, response_body) = tokio::time::timeout(timeout, exchange)
        .await
        .map_err(|_| {
        format!(
            "OpenRouter STT: timeout após {} segundos.",
            timeout.as_secs()
        )
    })??;
    let request_ms = started.elapsed().as_millis() as u64;
    if !status.is_success() {
        return Err(format!(
            "OpenRouter STT retornou status {}: {}",
            status.as_u16(),
            truncate_error(&response_body, 800)
        ));
    }

    let parsed: TranscriptionResponse = serde_json::from_str(&response_body)
        .map_err(|e| format!("OpenRouter STT: resposta inválida: {e}"))?;
    let text = parsed.text.trim().to_string();
    if text.is_empty() {
        return Err("OpenRouter STT não retornou texto.".into());
    }
    Ok(OpenRouterGenerateResult {
        text,
        base64_ms,
        request_ms,
        generation_id,
        reported_total_tokens: parsed.usage.and_then(|usage| usage.total_tokens),
    })
}

fn audio_format(ext: &str) -> Result<&'static str, String> {
    match ext
        .trim()
        .trim_start_matches('.')
        .to_ascii_lowercase()
        .as_str()
    {
        "wav" | "wave" => Ok("wav"),
        "mp3" => Ok("mp3"),
        "flac" => Ok("flac"),
        "m4a" => Ok("m4a"),
        "ogg" | "oga" => Ok("ogg"),
        "webm" => Ok("webm"),
        "aac" => Ok("aac"),
        other => Err(format!(
            "OpenRouter Gemini: formato de áudio não suportado: {other}"
        )),
    }
}

fn truncate_error(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let shortened: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{shortened}…")
    } else {
        shortened
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_matches_openrouter_audio_contract() {
        let body = ChatRequest {
            model: "google/gemini-3.6-flash",
            messages: vec![Message {
                role: "user",
                content: vec![
                    ContentPart::Text { text: "transcreva" },
                    ContentPart::InputAudio {
                        input_audio: InputAudio {
                            data: "UklGRg==",
                            format: "wav",
                        },
                    },
                ],
            }],
            stream: false,
        };
        let json = serde_json::to_value(body).unwrap();
        assert_eq!(json["model"], "google/gemini-3.6-flash");
        assert_eq!(json["messages"][0]["content"][1]["type"], "input_audio");
        assert_eq!(
            json["messages"][0]["content"][1]["input_audio"]["format"],
            "wav"
        );
    }

    #[test]
    fn request_matches_openrouter_transcription_contract() {
        let body = TranscriptionRequest {
            model: "google/chirp-3",
            input_audio: InputAudio {
                data: "UklGRg==",
                format: "wav",
            },
            language: "pt",
            provider: None,
        };
        let json = serde_json::to_value(body).unwrap();
        assert_eq!(json["model"], "google/chirp-3");
        assert_eq!(json["input_audio"]["format"], "wav");
        assert_eq!(json["language"], "pt");
    }

    #[test]
    fn attributes_requests_to_haumea_voice() {
        let headers = app_attribution_headers();
        assert_eq!(headers.get(REFERER).unwrap(), APP_REFERER);
        assert_eq!(headers.get("X-OpenRouter-Title").unwrap(), APP_TITLE);
    }

    #[test]
    fn automatic_route_recognizes_common_stt_and_llm_ids() {
        assert!(model_looks_dedicated_stt("google/chirp-3"));
        assert!(model_looks_dedicated_stt("openai/whisper-large-v3"));
        assert!(model_looks_dedicated_stt("openai/gpt-4o-mini-transcribe"));
        assert!(!model_looks_dedicated_stt("google/gemini-3.7-flash"));
    }

    #[test]
    fn stt_catalog_shape_extracts_model_ids() {
        let catalog: ModelCatalogResponse = serde_json::from_str(
            r#"{"data":[{"id":"google/chirp-3"},{"id":"openai/whisper-large-v3"}]}"#,
        )
        .unwrap();
        let ids: HashSet<_> = catalog.data.into_iter().map(|entry| entry.id).collect();
        assert!(ids.contains("google/chirp-3"));
        assert!(ids.contains("openai/whisper-large-v3"));
    }

    #[test]
    fn whisper_transcription_is_pinned_to_groq_without_fallback() {
        let body = TranscriptionRequest {
            model: "openai/whisper-large-v3",
            input_audio: InputAudio {
                data: "UklGRg==",
                format: "wav",
            },
            language: "pt",
            provider: whisper_provider_preferences("openai/whisper-large-v3"),
        };
        let value = serde_json::to_value(body).expect("serialize transcription request");
        assert_eq!(value["provider"]["only"][0], "groq");
        assert_eq!(value["provider"]["allow_fallbacks"], false);
    }
}
