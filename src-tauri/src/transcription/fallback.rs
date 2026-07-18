//! Raw-text selection and error mapping for the legacy pipeline.

use crate::models::TranscriptionEngine;

/// Caps the length of an API error body so a giant JSON blob does not bloat
/// the history card. Truncates on a UTF-8 char boundary.
pub fn truncate_err_body(body: &str, max_chars: usize) -> String {
    if body.chars().count() <= max_chars {
        return body.to_string();
    }
    let mut out: String = body.chars().take(max_chars).collect();
    out.push('…');
    out
}

/// Maps a [`crate::groq::GroqNetworkError`] to a human-readable Portuguese message.
pub fn groq_err_to_message(e: &crate::groq::GroqNetworkError) -> String {
    use crate::groq::GroqNetworkError as E;
    match e {
        E::Request(r) => format!("Erro de rede ao contatar o Groq: {}", r),
        E::ApiError { status, body } => format!(
            "Groq retornou status {}: {}",
            status,
            truncate_err_body(body, 300)
        ),
        E::Parse(p) => format!("Falha ao interpretar a resposta do Groq: {}", p),
        E::MissingText => "A resposta do Groq não contém o campo de texto.".to_string(),
        E::MissingApiKey => "Chave de API do Groq não configurada.".to_string(),
    }
}

/// When the sanitizer is off or fails, pick the best available raw acoustic text.
pub fn pick_raw_acoustic(whisper_text: &str, deepgram_text: &str) -> String {
    let w = whisper_text.trim();
    let d = deepgram_text.trim();
    match (w.is_empty(), d.is_empty()) {
        (true, true) => String::new(),
        (false, true) => w.to_string(),
        (true, false) => d.to_string(),
        (false, false) => {
            if w.eq_ignore_ascii_case(d) {
                return w.to_string();
            }
            let wl = w.chars().count();
            let dl = d.chars().count();
            if wl > dl.saturating_add(8) {
                log::info!(
                    "transcription: pick_raw chose Whisper ({} vs {} chars)",
                    wl,
                    dl
                );
                w.to_string()
            } else {
                log::info!(
                    "transcription: pick_raw chose Deepgram ({} vs {} chars)",
                    dl,
                    wl
                );
                d.to_string()
            }
        }
    }
}

/// When finalized text is blank, prefer Deepgram raw then Whisper raw.
pub fn coalesce_empty_final(finalized: String, whisper_text: &str, deepgram_text: &str) -> String {
    if !finalized.trim().is_empty() {
        return finalized;
    }
    if !deepgram_text.trim().is_empty() {
        log::warn!(
            "transcription: sanitizer returned empty final_text (likely GPT-OSS reasoning divergence); \
             falling back to DeepGram raw transcription ({} chars)",
            deepgram_text.trim().len()
        );
        return deepgram_text.trim().to_string();
    }
    if !whisper_text.trim().is_empty() {
        log::warn!(
            "transcription: sanitizer returned empty final_text and Deepgram raw is also empty; \
             falling back to Whisper raw transcription ({} chars)",
            whisper_text.trim().len()
        );
        return whisper_text.trim().to_string();
    }
    finalized
}

/// Map a single-engine transcript into the correct sanitizer slot.
pub fn single_engine_slots(engine: TranscriptionEngine, text: String) -> (String, String, bool) {
    match engine {
        TranscriptionEngine::DeepgramNova3 => (String::new(), text, true),
        TranscriptionEngine::GroqWhisper => (text, String::new(), false),
        other => {
            log::warn!(
                "transcription: single_engine_slots for unexpected engine {:?}",
                other
            );
            (text, String::new(), false)
        }
    }
}

/// Resolve dual-mode STT pair results into slots + flags.
pub fn resolve_dual_results(
    groq_res: Result<String, String>,
    deepgram_res: Result<String, String>,
) -> Result<(String, String, bool, bool), String> {
    match (groq_res, deepgram_res) {
        (Ok(g), Ok(d)) => Ok((g, d, true, true)),
        (Ok(g), Err(de_err)) => {
            log::warn!(
                "transcription: Deepgram falhou no modo duplo, usando apenas Groq Whisper: {}",
                de_err
            );
            Ok((g, String::new(), false, false))
        }
        (Err(groq_err), Ok(d)) => {
            log::warn!(
                "transcription: Groq Whisper falhou no modo duplo, usando apenas Deepgram: {}",
                groq_err
            );
            Ok((String::new(), d, false, true))
        }
        (Err(groq_err), Err(de_err)) => Err(format!(
            "Ambos os motores de transcrição falharam no modo duplo:\n\
             • Groq Whisper: {}\n\
             • Deepgram Nova-3: {}",
            groq_err, de_err
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_raw_prefers_longer_or_deepgram_when_close() {
        assert_eq!(pick_raw_acoustic("hello", ""), "hello");
        assert_eq!(pick_raw_acoustic("", "world"), "world");
        assert_eq!(pick_raw_acoustic("abc", "abcdefghi"), "abcdefghi");
        // lengths within 8 → Deepgram bias
        assert_eq!(pick_raw_acoustic("12345", "123456"), "123456");
    }

    #[test]
    fn coalesce_prefers_deepgram_on_empty_final() {
        let out = coalesce_empty_final(String::new(), "w", "d");
        assert_eq!(out, "d");
        let out = coalesce_empty_final(String::new(), "w", "");
        assert_eq!(out, "w");
        let out = coalesce_empty_final("keep".into(), "w", "d");
        assert_eq!(out, "keep");
    }

    #[test]
    fn single_slots_do_not_duplicate() {
        let (w, d, dg) = single_engine_slots(TranscriptionEngine::GroqWhisper, "hi".into());
        assert_eq!(w, "hi");
        assert!(d.is_empty());
        assert!(!dg);

        let (w, d, dg) = single_engine_slots(TranscriptionEngine::DeepgramNova3, "hi".into());
        assert!(w.is_empty());
        assert_eq!(d, "hi");
        assert!(dg);
    }

    #[test]
    fn dual_partial_and_both_fail() {
        let ok = resolve_dual_results(Ok("a".into()), Err("x".into())).unwrap();
        assert_eq!(ok.0, "a");
        assert!(ok.1.is_empty());
        assert!(!ok.2 && !ok.3);

        let err = resolve_dual_results(Err("e1".into()), Err("e2".into()));
        assert!(err.unwrap_err().contains("Groq Whisper"));
    }

    #[test]
    fn truncate_err_body_utf8_safe() {
        let s = "á".repeat(10);
        let t = truncate_err_body(&s, 3);
        assert!(t.ends_with('…'));
        assert_eq!(t.chars().count(), 4);
    }
}
