//! Meta Model API Speech Recognition (ASR) client for Muse Voice Transcribe.
//!
//! Uses the dedicated non-realtime ASR endpoint:
//! `POST https://api.meta.ai/v1/asr/transcribe`
//! with `multipart/form-data` containing the request JSON and WAV audio.

use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const META_ASR_ENDPOINT: &str = "https://api.meta.ai/v1/asr/transcribe";
pub const META_DEFAULT_MODEL: &str = "muse-voice-transcribe-1.0";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetaTranscribeRequestPayload {
    pub model: String,
    pub audio_encoding: String,
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keywords: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language_bias: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetaTurn {
    pub turn_id: i32,
    pub start_ms: i64,
    pub end_ms: i64,
    pub transcript: String,
    pub speaker: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetaTranscribeResponse {
    pub session_id: Option<String>,
    pub transcript: String,
    pub audio_duration_ms: Option<u64>,
    #[serde(default)]
    pub turns: Vec<MetaTurn>,
}

#[derive(Debug, Clone)]
pub struct MetaAcousticOutcome {
    pub text: String,
    pub latency_ms: u64,
    pub audio_duration_ms: Option<u64>,
    pub bytes_sent: usize,
    pub model: String,
    pub session_id: Option<String>,
}

/// Transcribes a WAV audio buffer using Meta's Muse Voice Transcribe ASR endpoint.
///
/// `keywords`: List of custom terms (from user vocabulary) to bias recognition.
/// `language_bias`: If None, languageBias is omitted so Meta's model automatically
/// detects any spoken language and supports multi-language code-switching dynamically.
pub async fn transcribe(
    audio: &[u8],
    api_key: &str,
    model: Option<&str>,
    keywords: Option<Vec<String>>,
    language_bias: Option<Vec<String>>,
) -> Result<MetaAcousticOutcome, String> {
    let t0 = std::time::Instant::now();
    let model_id = model.unwrap_or(META_DEFAULT_MODEL);
    let bytes_sent = audio.len();

    let request_payload = MetaTranscribeRequestPayload {
        model: model_id.to_string(),
        audio_encoding: "WAV".to_string(),
        mode: "PUSH_TO_TALK".to_string(),
        keywords: keywords.filter(|k| !k.is_empty()),
        language_bias: language_bias.filter(|l| !l.is_empty()),
    };

    let request_json = serde_json::to_string(&request_payload)
        .map_err(|e| format!("Falha ao serializar payload Meta ASR: {e}"))?;

    let form = reqwest::multipart::Form::new()
        .part(
            "request",
            reqwest::multipart::Part::text(request_json)
                .mime_str("application/json")
                .map_err(|e| format!("Erro ao criar parte JSON multipart: {e}"))?,
        )
        .part(
            "audio",
            reqwest::multipart::Part::bytes(audio.to_vec())
                .file_name("recording.wav")
                .mime_str("audio/wav")
                .map_err(|e| format!("Erro ao criar parte áudio multipart: {e}"))?,
        );

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| format!("Falha ao criar cliente HTTP: {e}"))?;

    log::info!(
        "meta_asr: enviando request model={} bytes={} mode=PUSH_TO_TALK language_bias={:?} keywords={}",
        model_id,
        bytes_sent,
        request_payload.language_bias,
        request_payload.keywords.as_ref().map(|k| k.len()).unwrap_or(0),
    );

    let response = client
        .post(META_ASR_ENDPOINT)
        .header("Authorization", format!("Bearer {}", api_key.trim()))
        .header("Accept", "application/json")
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("Falha na conexão com Meta ASR ({META_ASR_ENDPOINT}): {e}"))?;

    let status = response.status();
    let latency_ms = t0.elapsed().as_millis() as u64;

    if !status.is_success() {
        let error_body = response
            .text()
            .await
            .unwrap_or_else(|_| "<falha ao ler corpo da resposta>".to_string());
        log::error!(
            "meta_asr: erro status={} latency={}ms body={}",
            status,
            latency_ms,
            error_body
        );

        return match status.as_u16() {
            401 | 403 => Err(
                "Chave da API da Meta inválida ou sem permissão para o modelo Muse Voice Transcribe."
                    .to_string(),
            ),
            413 => Err("Áudio excede o limite de 32 MB suportado pela Meta.".to_string()),
            429 => Err("Limite de requisições da Meta excedido (Rate Limit). Tente novamente em instantes.".to_string()),
            400 => Err(format!(
                "Requisição rejeitada pela Meta (HTTP 400). Verifique formato de áudio: {error_body}"
            )),
            500..=599 => Err(format!(
                "Erro interno nos servidores da Meta (HTTP {status}): {error_body}"
            )),
            _ => Err(format!("Meta ASR falhou com status {status}: {error_body}")),
        };
    }

    let parsed: MetaTranscribeResponse = response
        .json()
        .await
        .map_err(|e| format!("Falha ao decodificar resposta JSON da Meta: {e}"))?;

    log::info!(
        "meta_asr: concluído em {}ms, texto_chars={}, session_id={:?}",
        latency_ms,
        parsed.transcript.len(),
        parsed.session_id
    );

    Ok(MetaAcousticOutcome {
        text: parsed.transcript,
        latency_ms,
        audio_duration_ms: parsed.audio_duration_ms,
        bytes_sent,
        model: model_id.to_string(),
        session_id: parsed.session_id,
    })
}
