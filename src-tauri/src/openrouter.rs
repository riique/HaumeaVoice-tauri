//! OpenRouter dedicated speech-to-text client for Google Chirp 3.

use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

pub const CHIRP_3_MODEL: &str = "google/chirp-3";
const TRANSCRIPTIONS_URL: &str = "https://openrouter.ai/api/v1/audio/transcriptions";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(65);

#[derive(Debug, Serialize)]
struct TranscriptionRequest<'a> {
    model: &'static str,
    input_audio: InputAudio<'a>,
    language: &'static str,
}

#[derive(Debug, Serialize)]
struct InputAudio<'a> {
    data: &'a str,
    format: &'a str,
}

#[derive(Debug, Default, Deserialize)]
struct TranscriptionResponse {
    #[serde(default)]
    text: String,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Debug, Default, Deserialize)]
struct Usage {
    #[serde(default)]
    seconds: Option<f64>,
    #[serde(default)]
    cost: Option<f64>,
    #[serde(default)]
    total_tokens: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct ChirpTranscription {
    pub text: String,
    pub base64_ms: u64,
    pub request_ms: u64,
    pub audio_seconds: Option<f64>,
    pub cost_usd: Option<f64>,
    pub total_tokens: Option<u64>,
    pub generation_id: Option<String>,
}

pub async fn transcribe_chirp3(
    audio: &[u8],
    ext: &str,
    api_key: &str,
) -> Result<ChirpTranscription, String> {
    if audio.is_empty() {
        return Err("OpenRouter Chirp 3: o áudio está vazio.".into());
    }
    let key = api_key.trim();
    if key.is_empty() {
        return Err("Configure a chave da API do OpenRouter em Provedores e APIs.".into());
    }

    let format = audio_format(ext)?;
    let encode_started = Instant::now();
    let encoded = general_purpose::STANDARD.encode(audio);
    let base64_ms = encode_started.elapsed().as_millis() as u64;
    let body = TranscriptionRequest {
        model: CHIRP_3_MODEL,
        input_audio: InputAudio {
            data: &encoded,
            format,
        },
        language: "pt",
    };

    let request_started = Instant::now();
    let response = tokio::time::timeout(
        REQUEST_TIMEOUT,
        reqwest::Client::new()
            .post(TRANSCRIPTIONS_URL)
            .bearer_auth(key)
            .header("X-OpenRouter-Title", "Haumea Voice")
            .json(&body)
            .send(),
    )
    .await
    .map_err(|_| {
        format!(
            "OpenRouter Chirp 3: timeout após {} segundos.",
            REQUEST_TIMEOUT.as_secs()
        )
    })?
    .map_err(|e| format!("OpenRouter Chirp 3: falha de rede: {e}"))?;

    let status = response.status();
    let generation_id = response
        .headers()
        .get("x-generation-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let response_body = response
        .text()
        .await
        .map_err(|e| format!("OpenRouter Chirp 3: falha ao ler a resposta: {e}"))?;
    let request_ms = request_started.elapsed().as_millis() as u64;

    if !status.is_success() {
        return Err(format!(
            "OpenRouter Chirp 3 retornou status {}: {}",
            status.as_u16(),
            truncate_error(&response_body, 800)
        ));
    }

    let parsed: TranscriptionResponse = serde_json::from_str(&response_body)
        .map_err(|e| format!("OpenRouter Chirp 3: resposta inválida: {e}"))?;
    let text = parsed.text.trim().to_string();
    if text.is_empty() {
        return Err("OpenRouter Chirp 3 não retornou texto.".into());
    }
    let usage = parsed.usage.unwrap_or_default();

    Ok(ChirpTranscription {
        text,
        base64_ms,
        request_ms,
        audio_seconds: usage.seconds,
        cost_usd: usage.cost,
        total_tokens: usage.total_tokens,
        generation_id,
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
            "OpenRouter Chirp 3: formato de áudio não suportado: {other}"
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
    fn request_matches_openrouter_stt_contract() {
        let body = TranscriptionRequest {
            model: CHIRP_3_MODEL,
            input_audio: InputAudio {
                data: "UklGRg==",
                format: "wav",
            },
            language: "pt",
        };
        let json = serde_json::to_value(body).unwrap();
        assert_eq!(json["model"], "google/chirp-3");
        assert_eq!(json["input_audio"]["data"], "UklGRg==");
        assert_eq!(json["input_audio"]["format"], "wav");
        assert_eq!(json["language"], "pt");
    }

    #[test]
    fn parses_usage_and_text() {
        let response: TranscriptionResponse = serde_json::from_str(
            r#"{"text":"  teste rápido  ","usage":{"seconds":2.5,"cost":0.0007,"total_tokens":42}}"#,
        )
        .unwrap();
        assert_eq!(response.text.trim(), "teste rápido");
        let usage = response.usage.unwrap();
        assert_eq!(usage.seconds, Some(2.5));
        assert_eq!(usage.cost, Some(0.0007));
        assert_eq!(usage.total_tokens, Some(42));
    }

    #[test]
    fn validates_supported_formats() {
        assert_eq!(audio_format(".WAV").unwrap(), "wav");
        assert_eq!(audio_format("m4a").unwrap(), "m4a");
        assert!(audio_format("exe").is_err());
    }

    #[test]
    fn truncates_unicode_errors_safely() {
        assert_eq!(truncate_error("áudio inválido", 5), "áudio…");
    }
}
