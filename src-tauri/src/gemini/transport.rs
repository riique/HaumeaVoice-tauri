//! Hybrid audio transport: short clips use inline Base64; large clips use Files API.

use serde::{Deserialize, Serialize};

/// Max raw audio bytes allowed for inline Base64 generateContent.
pub const GEMINI_INLINE_MAX_BYTES: usize = 10 * 1024 * 1024; // 10 MB

/// Max audio duration (ms) allowed for inline transport.
pub const GEMINI_INLINE_MAX_DURATION_MS: u64 = 5 * 60 * 1000; // 5 minutes

/// How audio is attached to a Gemini generateContent call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeminiAudioTransport {
    Inline,
    FilesApi,
}

impl GeminiAudioTransport {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inline => "inline",
            Self::FilesApi => "files_api",
        }
    }

    pub fn label_pt(self) -> &'static str {
        match self {
            Self::Inline => "Gemini inline",
            Self::FilesApi => "Gemini Files API",
        }
    }
}

/// MIME types accepted for Gemini audio parts.
pub fn is_supported_audio_mime(mime: &str) -> bool {
    let m = mime.trim().to_ascii_lowercase();
    matches!(
        m.as_str(),
        "audio/wav"
            | "audio/wave"
            | "audio/x-wav"
            | "audio/mpeg"
            | "audio/mp3"
            | "audio/mp4"
            | "audio/aac"
            | "audio/flac"
            | "audio/ogg"
            | "audio/webm"
            | "audio/aiff"
            | "audio/x-aiff"
    ) || m.starts_with("audio/")
}

/// Selects Inline vs Files API.
///
/// Rules:
/// - empty audio → error
/// - unsupported / empty mime → error
/// - unknown duration (`None`) is treated as `0` so the **byte** limit remains
///   the hard gate (conservative for payload size; short mic always has duration)
/// - Inline only when **both** size ≤ max bytes **and** duration ≤ max ms
pub fn select_gemini_audio_transport(
    size_bytes: usize,
    duration_ms: Option<u64>,
    mime_type: &str,
) -> Result<GeminiAudioTransport, String> {
    if size_bytes == 0 {
        return Err("áudio vazio; não é possível enviar ao Gemini".into());
    }
    let mime = mime_type.trim();
    if mime.is_empty() {
        return Err("MIME de áudio ausente".into());
    }
    if !is_supported_audio_mime(mime) {
        return Err(format!("MIME de áudio não suportado: {}", mime));
    }

    let duration = duration_ms.unwrap_or(0);
    if size_bytes <= GEMINI_INLINE_MAX_BYTES && duration <= GEMINI_INLINE_MAX_DURATION_MS {
        Ok(GeminiAudioTransport::Inline)
    } else {
        Ok(GeminiAudioTransport::FilesApi)
    }
}

/// Best-effort duration from a 16-bit mono PCM WAV (Sonora mic format).
/// Returns `None` if the buffer is not a parseable WAVE with data chunk.
pub fn estimate_wav_duration_ms(bytes: &[u8]) -> Option<u64> {
    if bytes.len() < 44 {
        return None;
    }
    if &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let mut i = 12usize;
    let mut sample_rate: Option<u32> = None;
    let mut bits: Option<u16> = None;
    let mut channels: Option<u16> = None;
    let mut data_size: Option<u32> = None;
    while i + 8 <= bytes.len() {
        let id = &bytes[i..i + 4];
        let size = u32::from_le_bytes(bytes[i + 4..i + 8].try_into().ok()?);
        let data_start = i + 8;
        let data_end = data_start.saturating_add(size as usize);
        if data_end > bytes.len() {
            break;
        }
        if id == b"fmt " && size >= 16 {
            channels = Some(u16::from_le_bytes(
                bytes[data_start + 2..data_start + 4].try_into().ok()?,
            ));
            sample_rate = Some(u32::from_le_bytes(
                bytes[data_start + 4..data_start + 8].try_into().ok()?,
            ));
            bits = Some(u16::from_le_bytes(
                bytes[data_start + 14..data_start + 16].try_into().ok()?,
            ));
        } else if id == b"data" {
            data_size = Some(size);
        }
        i = data_end + (size as usize % 2); // word align
    }
    let sr = sample_rate.filter(|r| *r > 0)?;
    let ch = channels.filter(|c| *c > 0)? as u32;
    let bps = bits.filter(|b| *b > 0)? as u32;
    let ds = data_size? as u64;
    let bytes_per_sec = (sr as u64) * (ch as u64) * (bps as u64 / 8);
    if bytes_per_sec == 0 {
        return None;
    }
    Some(ds.saturating_mul(1000) / bytes_per_sec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_below_limits() {
        let t = select_gemini_audio_transport(1024, Some(1000), "audio/wav").unwrap();
        assert_eq!(t, GeminiAudioTransport::Inline);
    }

    #[test]
    fn inline_exactly_at_limits() {
        let t = select_gemini_audio_transport(
            GEMINI_INLINE_MAX_BYTES,
            Some(GEMINI_INLINE_MAX_DURATION_MS),
            "audio/wav",
        )
        .unwrap();
        assert_eq!(t, GeminiAudioTransport::Inline);
    }

    #[test]
    fn files_when_bytes_over() {
        let t = select_gemini_audio_transport(GEMINI_INLINE_MAX_BYTES + 1, Some(1000), "audio/wav")
            .unwrap();
        assert_eq!(t, GeminiAudioTransport::FilesApi);
    }

    #[test]
    fn files_when_duration_over() {
        let t = select_gemini_audio_transport(
            1024,
            Some(GEMINI_INLINE_MAX_DURATION_MS + 1),
            "audio/wav",
        )
        .unwrap();
        assert_eq!(t, GeminiAudioTransport::FilesApi);
    }

    #[test]
    fn files_when_both_over() {
        let t = select_gemini_audio_transport(
            GEMINI_INLINE_MAX_BYTES + 1,
            Some(GEMINI_INLINE_MAX_DURATION_MS + 1),
            "audio/wav",
        )
        .unwrap();
        assert_eq!(t, GeminiAudioTransport::FilesApi);
    }

    #[test]
    fn unknown_duration_uses_size_only() {
        // Documented: None → 0 ms; small file stays Inline.
        let t = select_gemini_audio_transport(2048, None, "audio/wav").unwrap();
        assert_eq!(t, GeminiAudioTransport::Inline);
        let t2 =
            select_gemini_audio_transport(GEMINI_INLINE_MAX_BYTES + 1, None, "audio/wav").unwrap();
        assert_eq!(t2, GeminiAudioTransport::FilesApi);
    }

    #[test]
    fn empty_audio_errors() {
        assert!(select_gemini_audio_transport(0, Some(0), "audio/wav").is_err());
    }

    #[test]
    fn invalid_mime_errors() {
        assert!(select_gemini_audio_transport(100, Some(0), "").is_err());
        assert!(select_gemini_audio_transport(100, Some(0), "application/pdf").is_err());
    }

    #[test]
    fn mic_short_wav_inline() {
        // ~1s mono 16kHz 16-bit ≈ 32KB + header
        let t = select_gemini_audio_transport(33_000, Some(1000), "audio/wav").unwrap();
        assert_eq!(t, GeminiAudioTransport::Inline);
    }
}
