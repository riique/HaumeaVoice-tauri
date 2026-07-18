//! History labels and latency metrics for the legacy pipeline.

use crate::models::{AppState, TranscriptionEngine};

/// Deepgram mode label for history when Deepgram STT actually ran.
pub fn history_deepgram_mode(state: &AppState, deepgram_ran: bool) -> Option<String> {
    if deepgram_ran {
        Some(state.deepgram_mode.read().as_str().to_string())
    } else {
        None
    }
}

/// Acoustic real-time factor: STT latency relative to audio length.
pub fn compute_realtime_factor(transcription_latency_ms: u64, duration_ms: u64) -> Option<f64> {
    if transcription_latency_ms > 0 && duration_ms > 0 {
        Some(transcription_latency_ms as f64 / duration_ms as f64)
    } else {
        None
    }
}

/// Word count for motor throughput: max of the two acoustics.
pub fn acoustic_word_count(whisper_text: &str, deepgram_text: &str) -> usize {
    let w = whisper_text.split_whitespace().count();
    let d = deepgram_text.split_whitespace().count();
    w.max(d)
}

/// Honest history engine label after dual may have degraded to a single STT.
pub fn history_engine_label(
    engine: TranscriptionEngine,
    effective_dual: bool,
    whisper_text: &str,
    deepgram_text: &str,
) -> String {
    if effective_dual {
        return "Groq+Deepgram".to_string();
    }
    let w = !whisper_text.trim().is_empty();
    let d = !deepgram_text.trim().is_empty();
    match (w, d) {
        (true, false) => "GroqWhisper".to_string(),
        (false, true) => "DeepgramNova3".to_string(),
        _ => format!("{:?}", engine),
    }
}

/// Estimated tokens/sec from word count and latency (legacy heuristic).
pub fn est_throughput(words: usize, latency_ms: u64) -> Option<f64> {
    if latency_ms > 0 {
        let est_tokens = words as f64 * 1.3;
        Some((est_tokens * 1000.0) / latency_ms as f64)
    } else {
        None
    }
}

pub fn est_total_tokens(words: usize) -> usize {
    (words as f64 * 1.3).round() as usize
}

pub fn log_latency(
    transcription_ms: u64,
    sanitizer_ms: u64,
    duration_ms: u64,
    realtime_factor: Option<f64>,
    deepgram_mode: Option<&str>,
    dual: bool,
    context: &str,
) {
    let total = transcription_ms + sanitizer_ms;
    if let Some(rtf) = realtime_factor {
        log::info!(
            "transcription: {} latency telemetry transcription_ms={} sanitizer_ms={} total_ms={} \
             duration_ms={} rtf={:.3} deepgram_mode={} dual={}",
            context,
            transcription_ms,
            sanitizer_ms,
            total,
            duration_ms,
            rtf,
            deepgram_mode.unwrap_or("-"),
            dual
        );
    } else {
        log::info!(
            "transcription: {} latency telemetry transcription_ms={} sanitizer_ms={} total_ms={} \
             deepgram_mode={} dual={}",
            context,
            transcription_ms,
            sanitizer_ms,
            total,
            deepgram_mode.unwrap_or("-"),
            dual
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_label_dual_and_single() {
        assert_eq!(
            history_engine_label(TranscriptionEngine::GroqWhisper, true, "a", "b"),
            "Groq+Deepgram"
        );
        assert_eq!(
            history_engine_label(TranscriptionEngine::GroqWhisper, false, "a", ""),
            "GroqWhisper"
        );
        assert_eq!(
            history_engine_label(TranscriptionEngine::DeepgramNova3, false, "", "b"),
            "DeepgramNova3"
        );
    }

    #[test]
    fn rtf_and_throughput() {
        assert_eq!(compute_realtime_factor(250, 1000), Some(0.25));
        assert_eq!(compute_realtime_factor(0, 1000), None);
        assert!(est_throughput(10, 1000).unwrap() > 0.0);
        assert_eq!(est_total_tokens(10), 13);
    }

    #[test]
    fn acoustic_word_count_max() {
        assert_eq!(acoustic_word_count("one two", "one two three"), 3);
    }
}
