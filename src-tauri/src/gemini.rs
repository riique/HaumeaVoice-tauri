//! Gemini multimodal pronunciation evaluator.
//!
//! Sends the persisted audio (inline, base64-encoded) together with its
//! transcribed text to the Google Generative Language API and asks Gemini to
//! produce a Markdown speech assessment following a fixed CEFR-based structure
//! (international oral proficiency rubric). The frontend parses that fixed
//! structure to render each section with its own custom design.
//!
//! All network IO is async (Tokio) so the Tauri main thread is never blocked.

use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};
use std::{sync::OnceLock, time::Duration};

/// Multimodal Gemini model capable of native audio understanding.
const GEMINI_MODEL: &str = "gemini-3.5-flash";

/// Base endpoint for the Generative Language `generateContent` call. The model
/// and the API key (as a query parameter) are appended at request time.
const GEMINI_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models";

/// Audio evaluation can take longer than a plain transcription because the
/// model reasons over the whole clip, so the timeout is generous.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

fn http_client() -> Result<&'static reqwest::Client, String> {
    if let Some(client) = HTTP_CLIENT.get() {
        return Ok(client);
    }
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| format!("falha ao construir o cliente http: {}", e))?;
    let _ = HTTP_CLIENT.set(client);
    HTTP_CLIENT
        .get()
        .ok_or_else(|| "falha ao inicializar o cliente http".to_string())
}

/// Instruction handed to Gemini alongside the audio. Asks for a fixed
/// Markdown structure (the international oral-proficiency rubric) so the
/// frontend can parse every section and render it with a dedicated design.
/// The model is told to assess the speech in whatever language is actually
/// spoken in the clip, while keeping the answer in Brazilian Portuguese.
fn build_prompt(transcript: &str) -> String {
    format!(
        "Analise o áudio como um avaliador internacional de proficiência oral e \
comunicação.\n\n\
Responda em português do Brasil, em Markdown, sem introduções fora da \
estrutura pedida.\n\n\
Objetivo da avaliação:\n\
- medir inteligibilidade, pronúncia, fluência, ritmo, entonação, gramática \
oral, vocabulário, coesão, naturalidade, segurança e adequação ao contexto;\n\
- dar uma nota geral;\n\
- classificar o desempenho na escala internacional CEFR (A1, A2, B1, B2, C1, C2);\n\
- indicar o quão próximo o desempenho está de uma fala nativa, sem exagerar a \
conclusão.\n\n\
Escalas obrigatórias:\n\
- Nota geral: 0 a 10, com 1 casa decimal.\n\
- CEFR estimado: A1, A2, B1, B2, C1 ou C2.\n\
- Referência internacional de fala: Básico em desenvolvimento, Intermediário \
funcional, Fluente profissional, Quase nativo ou Nativo.\n\
- Proximidade de fala nativa: 0 a 100.\n\
- Confiança da avaliação: baixa, média ou alta.\n\n\
Regras:\n\
1. Priorize o áudio como fonte principal.\n\
2. Use a transcrição apenas como apoio, porque ela pode ter sido limpa \
automaticamente.\n\
3. Se o áudio estiver curto demais, ruim, com ruído forte, silêncio ou material \
insuficiente, diga isso explicitamente e reduza a confiança.\n\
4. Não invente palavras, contexto, sotaque, nacionalidade ou nível que o áudio \
não sustente.\n\
5. A avaliação deve equilibrar pontos fortes e pontos fracos.\n\
6. Diferencie com rigor:\n\
   - fluência funcional;\n\
   - fluência avançada;\n\
   - quase nativo;\n\
   - nativo.\n\
7. Só use \"Nativo\" se houver evidência muito forte e consistente. Na dúvida, \
use uma classificação abaixo.\n\
8. Se o áudio estiver em outro idioma, avalie no idioma falado, mas mantenha a \
resposta em português.\n\
9. Quando citar evidências, prefira trechos curtos ou paráfrases claramente \
reconhecíveis do próprio áudio.\n\
10. Seja específico, direto, técnico e construtivo.\n\n\
Estrutura obrigatória da resposta:\n\
## Resumo Executivo\n\
Escreva de 2 a 4 frases com o diagnóstico principal.\n\n\
## Placar\n\
- Nota geral: X/10\n\
- CEFR estimado: ...\n\
- Referência internacional de fala: ...\n\
- Proximidade de fala nativa: X/100\n\
- Confiança da avaliação: ...\n\n\
## Forças\n\
Liste de 3 a 5 pontos fortes objetivos.\n\n\
## Pontos de Atenção\n\
Liste de 3 a 5 pontos que mais limitam a performance.\n\n\
## Pronúncia e Inteligibilidade\n\
Avalie articulação, sons, sotaque, compreensão e inteligibilidade geral.\n\n\
## Fluência e Ritmo\n\
Avalie pausas, velocidade, hesitações, continuidade e naturalidade do fluxo.\n\n\
## Gramática Oral e Estrutura\n\
Avalie construção de frases, concordância, precisão e organização das ideias \
ao falar.\n\n\
## Vocabulário e Adequação\n\
Avalie variedade lexical, precisão vocabular, repetições e adequação ao \
contexto.\n\n\
## Naturalidade e Registro\n\
Avalie segurança, espontaneidade, entonação, registro e o quanto a fala soa \
natural.\n\n\
## Evidências do Áudio\n\
Liste de 3 a 5 evidências curtas do áudio que sustentam a avaliação.\n\n\
## Plano de Melhoria\n\
- Traga 5 ações práticas e priorizadas.\n\
- Traga 3 exercícios específicos para subir um nível.\n\n\
## Veredito Final\n\
Feche com 1 parágrafo explicando por que essa foi a nota geral, qual o nível \
internacional mais provável e o que falta para chegar ao próximo patamar.\n\n\
Transcrição de referência (use apenas como apoio):\n\"\"\"\n{}\n\"\"\"",
        transcript
    )
}

/* --------------------------- Request payloads --------------------------- */

#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<Content>,
}

#[derive(Serialize)]
struct Content {
    parts: Vec<Part>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum Part {
    Text {
        text: String,
    },
    Inline {
        #[serde(rename = "inline_data")]
        inline_data: InlineData,
    },
}

#[derive(Serialize)]
struct InlineData {
    mime_type: String,
    data: String,
}

/* --------------------------- Response payloads -------------------------- */

#[derive(Deserialize)]
struct GeminiResponse {
    #[serde(default)]
    candidates: Vec<Candidate>,
}

#[derive(Deserialize)]
struct Candidate {
    #[serde(default)]
    content: Option<ResponseContent>,
}

#[derive(Deserialize)]
struct ResponseContent {
    #[serde(default)]
    parts: Vec<ResponsePart>,
}

#[derive(Deserialize)]
struct ResponsePart {
    #[serde(default)]
    text: Option<String>,
}

/// Maps a file extension to a MIME type from Gemini's accepted audio set.
/// Gemini is stricter than the STT engines, so m4a/aac collapse to `audio/aac`
/// and mp3 uses `audio/mp3` (the form documented for the API).
fn gemini_mime_for_ext(ext: &str) -> &'static str {
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
        // wav and anything else default to wav (the microphone format).
        _ => "audio/wav",
    }
}

/// Sends `audio_bytes` (with the extension used to pick a Gemini-compatible
/// MIME type) plus `transcript` to Gemini and returns the Markdown assessment.
///
/// Every failure path (missing key, network, non-success status, malformed
/// JSON, empty answer) is collapsed into a human-readable `String` so the
/// command layer can surface it directly to the UI.
pub async fn evaluate_pronunciation(
    audio_bytes: Vec<u8>,
    ext: &str,
    transcript: &str,
    api_key: &str,
) -> Result<String, String> {
    if api_key.trim().is_empty() {
        return Err("a chave de API do Google não está configurada".to_string());
    }

    let mime = gemini_mime_for_ext(ext);
    let encoded = general_purpose::STANDARD.encode(&audio_bytes);

    let body = GeminiRequest {
        contents: vec![Content {
            parts: vec![
                Part::Text {
                    text: build_prompt(transcript),
                },
                Part::Inline {
                    inline_data: InlineData {
                        mime_type: mime.to_string(),
                        data: encoded,
                    },
                },
            ],
        }],
    };

    let url = format!(
        "{}/{}:generateContent?key={}",
        GEMINI_BASE_URL, GEMINI_MODEL, api_key
    );

    let client = http_client()?;

    let response = client
        .post(url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("falha na requisição ao Gemini: {}", e))?;

    let status = response.status().as_u16();
    if status != 200 {
        let err_body = response.text().await.unwrap_or_default();
        return Err(format!("Gemini retornou status {}: {}", status, err_body));
    }

    let parsed: GeminiResponse = response
        .json()
        .await
        .map_err(|e| format!("falha ao interpretar a resposta do Gemini: {}", e))?;

    let feedback = parsed
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

    if feedback.trim().is_empty() {
        return Err("o Gemini não retornou nenhum feedback".to_string());
    }

    Ok(feedback.trim().to_string())
}
