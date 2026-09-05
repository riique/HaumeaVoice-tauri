//! Local, deliberately conservative silence admission for microphone dictation.
//! This is not a speech recognizer: uncertain sound is admitted. In particular,
//! one short/quiet frame is enough; duration and the fraction of silence never
//! veto a recording. Work on original mono PCM, before normalization or upload.

pub fn is_clearly_silent(samples: &[i16], sample_rate: u32) -> bool {
    if samples.is_empty() {
        return true;
    }
    if sample_rate == 0 {
        return false;
    }
    let frame_size = (sample_rate as usize / 100).max(1); // 10 ms, including final partial frame
    let mut levels = Vec::with_capacity(samples.len().div_ceil(frame_size));
    let mut max_peak = 0.0_f64;
    for frame in samples.chunks(frame_size) {
        // Reject a stuck DC input without mistaking its offset for voice.
        let mean = frame.iter().map(|&s| s as f64).sum::<f64>() / frame.len() as f64;
        let energy = frame
            .iter()
            .map(|&s| {
                let centered = (s as f64 - mean) / 32768.0;
                max_peak = max_peak.max(centered.abs());
                centered * centered
            })
            .sum::<f64>()
            / frame.len() as f64;
        levels.push(energy.sqrt());
    }
    let max_rms = levels.iter().copied().fold(0.0_f64, f64::max);
    // Any clear signal survives, including clicks: false admission is preferable
    // to losing a word. These are very low levels (~-64 dBFS RMS / -52 dBFS peak).
    if max_rms > 0.00063 || max_peak > 0.0025 {
        return false;
    }
    levels.sort_by(f64::total_cmp);
    let floor = levels[levels.len() / 5].max(1.0 / 32768.0);
    // Preserve even quieter isolated syllables rising above the room tone.
    !(max_rms > 0.00012 && max_rms > floor * 2.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, ms: usize, amplitude: f64) -> Vec<i16> {
        (0..rate as usize * ms / 1000)
            .map(|i| {
                (amplitude * (std::f64::consts::TAU * 190.0 * i as f64 / rate as f64).sin()) as i16
            })
            .collect()
    }

    #[test]
    fn empty_digital_silence_dc_and_very_low_room_tone_are_local_no_speech() {
        assert!(is_clearly_silent(&[], 16000));
        assert!(is_clearly_silent(&vec![0; 16000], 16000));
        assert!(is_clearly_silent(&vec![900; 16000], 16000));
        let quiet_noise: Vec<i16> = (0..16000).map(|i| ((i * 7919 % 19) - 9) as i16).collect();
        assert!(is_clearly_silent(&quiet_noise, 16000));
    }

    #[test]
    fn short_signal_survives_at_capture_rates_and_any_position() {
        for rate in [16000, 44100, 48000] {
            for position in [0, 1, 2] {
                let mut samples = vec![0; rate as usize * 4];
                let word = tone(rate, 80, 45.0);
                let start = match position {
                    0 => 0,
                    1 => samples.len() / 2,
                    _ => samples.len() - word.len(),
                };
                samples[start..start + word.len()].copy_from_slice(&word);
                assert!(!is_clearly_silent(&samples, rate));
            }
        }
    }

    #[test]
    fn no_minimum_duration_or_speech_ratio_or_global_rms() {
        assert!(!is_clearly_silent(&tone(16000, 10, 80.0), 16000));
        let mut samples = vec![0; 16000 * 60];
        samples.extend(tone(16000, 20, 12.0));
        assert!(!is_clearly_silent(&samples, 16000));
        assert!(!is_clearly_silent(&tone(16000, 1000, 300.0), 16000));
        assert!(!is_clearly_silent(&[0, 1000, 0], 16000));
        assert!(!is_clearly_silent(&[1, 2, 3], 0));
    }

    #[test]
    fn local_spoken_words_and_short_phrase_survive_quiet_levels_and_long_pauses() {
        for bytes in [
            include_bytes!("../../tests/fixtures/speech/oi.wav").as_slice(),
            include_bytes!("../../tests/fixtures/speech/sim.wav").as_slice(),
            include_bytes!("../../tests/fixtures/speech/frase-curta.wav").as_slice(),
        ] {
            assert_eq!(&bytes[..4], b"RIFF");
            let mut cursor = 12;
            let mut speech = Vec::new();
            while cursor + 8 <= bytes.len() {
                let size =
                    u32::from_le_bytes(bytes[cursor + 4..cursor + 8].try_into().unwrap()) as usize;
                let chunk = &bytes[cursor + 8..cursor + 8 + size];
                if &bytes[cursor..cursor + 4] == b"fmt " {
                    assert_eq!(u16::from_le_bytes(chunk[2..4].try_into().unwrap()), 1);
                    assert_eq!(u32::from_le_bytes(chunk[4..8].try_into().unwrap()), 16000);
                    assert_eq!(u16::from_le_bytes(chunk[14..16].try_into().unwrap()), 16);
                }
                if &bytes[cursor..cursor + 4] == b"data" {
                    speech = chunk
                        .chunks_exact(2)
                        .map(|s| i16::from_le_bytes([s[0], s[1]]))
                        .collect();
                }
                cursor += 8 + size + size % 2;
            }
            let peak = speech.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0) as f64;
            assert!(peak > 100.0, "fixture must contain real synthesized speech");
            for quiet_peak in [20.0, 80.0, peak] {
                let quiet: Vec<i16> = speech
                    .iter()
                    .map(|&s| (s as f64 * quiet_peak / peak).round() as i16)
                    .collect();
                assert!(!is_clearly_silent(&quiet, 16000));
                let mut surrounded = vec![0; 16000 * 15];
                surrounded.extend_from_slice(&quiet);
                surrounded.resize(surrounded.len() + 16000 * 15, 0);
                assert!(!is_clearly_silent(&surrounded, 16000));
            }
        }
    }
}
