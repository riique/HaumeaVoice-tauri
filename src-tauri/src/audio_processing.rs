//! Conservative post-capture processing for microphone recordings.
//!
//! The microphone can produce clean but quiet speech. A fixed gain would also
//! make room tone in pauses louder, so this module first estimates speech and
//! noise-like frames, then combines a bounded speech gain with a downward
//! expander. Quiet frames receive approximately unity *net* gain, while clear
//! speech approaches the requested gain. Processing is skipped when the frame
//! distribution does not separate speech from background reliably.

const FRAME_MS: usize = 20;
const TARGET_ACTIVE_DBFS: f64 = -24.0;
const MAX_GAIN_DB: f64 = 12.0;
const PEAK_LIMIT_DBFS: f64 = -3.0;
const MIN_USEFUL_GAIN_DB: f64 = 0.5;
const MIN_DYNAMIC_RANGE_DB: f64 = 8.0;
const SPEECH_ABOVE_NOISE_DB: f64 = 6.0;
const ABSOLUTE_SPEECH_GATE_DBFS: f64 = -50.0;
const GATE_TRANSITION_DB: f64 = 3.0;
const ATTACK_MS: f64 = 3.0;

#[derive(Clone, Copy, Debug, Default)]
pub struct AudioProcessingStats {
    pub applied: bool,
    pub gain_db: f64,
    pub noise_floor_dbfs: f64,
    pub speech_threshold_dbfs: f64,
    pub active_rms_before_dbfs: f64,
    pub active_rms_after_dbfs: f64,
    pub peak_before_dbfs: f64,
    pub peak_after_dbfs: f64,
    pub active_frame_percent: f64,
}

/// Raises quiet microphone speech without making noise-only pauses louder.
///
/// The returned samples are a new buffer so the captured/resampled input stays
/// available to the caller until it chooses which representation to persist.
pub fn enhance_microphone_audio(
    samples: &[i16],
    sample_rate: u32,
) -> (Vec<i16>, AudioProcessingStats) {
    if samples.is_empty() || sample_rate == 0 {
        return (samples.to_vec(), AudioProcessingStats::default());
    }

    let frame_size = ((sample_rate as usize * FRAME_MS) / 1_000).max(1);
    let frame_levels: Vec<f64> = samples.chunks(frame_size).map(rms_dbfs).collect();
    if frame_levels.len() < 3 {
        return (samples.to_vec(), unchanged_stats(samples));
    }

    let mut sorted_levels = frame_levels.clone();
    sorted_levels.sort_by(f64::total_cmp);
    let noise_floor_dbfs = percentile(&sorted_levels, 0.20);
    let upper_level_dbfs = percentile(&sorted_levels, 0.90);
    let speech_threshold_dbfs =
        (noise_floor_dbfs + SPEECH_ABOVE_NOISE_DB).max(ABSOLUTE_SPEECH_GATE_DBFS);
    let active_frames: Vec<usize> = frame_levels
        .iter()
        .enumerate()
        .filter_map(|(index, &level)| (level >= speech_threshold_dbfs).then_some(index))
        .collect();
    let minimum_active_frames = 3.max(frame_levels.len() / 20);
    let separation_is_reliable = upper_level_dbfs - noise_floor_dbfs >= MIN_DYNAMIC_RANGE_DB
        && active_frames.len() >= minimum_active_frames;

    let peak_before = peak_linear(samples);
    let mut stats = AudioProcessingStats {
        applied: false,
        gain_db: 0.0,
        noise_floor_dbfs,
        speech_threshold_dbfs,
        active_rms_before_dbfs: -120.0,
        active_rms_after_dbfs: -120.0,
        peak_before_dbfs: linear_to_dbfs(peak_before),
        peak_after_dbfs: linear_to_dbfs(peak_before),
        active_frame_percent: 100.0 * active_frames.len() as f64 / frame_levels.len() as f64,
    };

    // Without a trustworthy speech/noise split, gain could turn steady room
    // tone into an audible artifact. Preserve the original instead.
    if !separation_is_reliable || peak_before <= 0.0 {
        return (samples.to_vec(), stats);
    }

    let active_rms_before = rms_for_frames(samples, frame_size, &active_frames);
    stats.active_rms_before_dbfs = linear_to_dbfs(active_rms_before);
    stats.active_rms_after_dbfs = stats.active_rms_before_dbfs;
    if active_rms_before <= 0.0 {
        return (samples.to_vec(), stats);
    }

    let speech_gain_db = TARGET_ACTIVE_DBFS - stats.active_rms_before_dbfs;
    let peak_headroom_db = PEAK_LIMIT_DBFS - stats.peak_before_dbfs;
    let gain_db = speech_gain_db.min(peak_headroom_db).clamp(0.0, MAX_GAIN_DB);
    stats.gain_db = gain_db;
    if gain_db < MIN_USEFUL_GAIN_DB {
        return (samples.to_vec(), stats);
    }

    let speech_gain = db_to_linear(gain_db);
    let quiet_pre_gain = 1.0 / speech_gain;
    let attack_coefficient = smoothing_coefficient(ATTACK_MS, sample_rate);
    let peak_limit = db_to_linear(PEAK_LIMIT_DBFS);
    let mut expander_envelope = quiet_pre_gain;
    let mut output = Vec::with_capacity(samples.len());

    for (frame_index, frame) in samples.chunks(frame_size).enumerate() {
        let target_pre_gain = expander_target(
            frame_levels[frame_index],
            speech_threshold_dbfs,
            quiet_pre_gain,
        );
        for &sample in frame {
            if target_pre_gain > expander_envelope {
                // Smooth only the upward transition into speech to avoid a
                // click or an over-eager first consonant. Once a frame falls
                // back below the speech threshold, restore unity *net* gain
                // immediately: keeping a slow release here audibly raises the
                // room tone after every phrase.
                expander_envelope += (target_pre_gain - expander_envelope) * attack_coefficient;
            } else {
                expander_envelope = target_pre_gain;
            }
            let normalized = sample as f64 / 32768.0;
            let processed =
                (normalized * speech_gain * expander_envelope).clamp(-peak_limit, peak_limit);
            output.push((processed * 32768.0).round().clamp(-32768.0, 32767.0) as i16);
        }
    }

    stats.applied = true;
    stats.peak_after_dbfs = linear_to_dbfs(peak_linear(&output));
    stats.active_rms_after_dbfs =
        linear_to_dbfs(rms_for_frames(&output, frame_size, &active_frames));
    (output, stats)
}

fn unchanged_stats(samples: &[i16]) -> AudioProcessingStats {
    let peak = linear_to_dbfs(peak_linear(samples));
    AudioProcessingStats {
        peak_before_dbfs: peak,
        peak_after_dbfs: peak,
        noise_floor_dbfs: -120.0,
        speech_threshold_dbfs: -120.0,
        active_rms_before_dbfs: -120.0,
        active_rms_after_dbfs: -120.0,
        ..AudioProcessingStats::default()
    }
}

fn percentile(sorted: &[f64], fraction: f64) -> f64 {
    let index = ((sorted.len() - 1) as f64 * fraction).round() as usize;
    sorted[index.min(sorted.len() - 1)]
}

fn rms_dbfs(samples: &[i16]) -> f64 {
    if samples.is_empty() {
        return -120.0;
    }
    let mean_square = samples
        .iter()
        .map(|&sample| {
            let normalized = sample as f64 / 32768.0;
            normalized * normalized
        })
        .sum::<f64>()
        / samples.len() as f64;
    linear_to_dbfs(mean_square.sqrt())
}

fn rms_for_frames(samples: &[i16], frame_size: usize, frames: &[usize]) -> f64 {
    let mut sum_square = 0.0;
    let mut count = 0usize;
    for &frame_index in frames {
        let start = frame_index * frame_size;
        let end = (start + frame_size).min(samples.len());
        for &sample in &samples[start..end] {
            let normalized = sample as f64 / 32768.0;
            sum_square += normalized * normalized;
            count += 1;
        }
    }
    if count == 0 {
        0.0
    } else {
        (sum_square / count as f64).sqrt()
    }
}

fn peak_linear(samples: &[i16]) -> f64 {
    samples
        .iter()
        .map(|&sample| (sample as f64 / 32768.0).abs())
        .fold(0.0, f64::max)
}

fn linear_to_dbfs(value: f64) -> f64 {
    if value <= 0.0 {
        -120.0
    } else {
        20.0 * value.log10()
    }
}

fn db_to_linear(value: f64) -> f64 {
    10.0_f64.powf(value / 20.0)
}

fn smoothing_coefficient(milliseconds: f64, sample_rate: u32) -> f64 {
    1.0 - (-1.0 / (milliseconds * sample_rate as f64 / 1_000.0)).exp()
}

fn expander_target(frame_dbfs: f64, threshold_dbfs: f64, quiet_pre_gain: f64) -> f64 {
    let lower = threshold_dbfs - GATE_TRANSITION_DB;
    let upper = threshold_dbfs + GATE_TRANSITION_DB;
    if frame_dbfs <= lower {
        return quiet_pre_gain;
    }
    if frame_dbfs >= upper {
        return 1.0;
    }
    let position = ((frame_dbfs - lower) / (upper - lower)).clamp(0.0, 1.0);
    let smooth = position * position * (3.0 - 2.0 * position);
    quiet_pre_gain + (1.0 - quiet_pre_gain) * smooth
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE: u32 = 16_000;

    fn sine(amplitude: f64, seconds: f64, frequency: f64) -> Vec<i16> {
        let count = (SAMPLE_RATE as f64 * seconds) as usize;
        (0..count)
            .map(|index| {
                let phase =
                    2.0 * std::f64::consts::PI * frequency * index as f64 / SAMPLE_RATE as f64;
                (phase.sin() * amplitude * 32767.0) as i16
            })
            .collect()
    }

    fn rms(samples: &[i16]) -> f64 {
        db_to_linear(rms_dbfs(samples))
    }

    #[test]
    fn silence_is_preserved() {
        let input = vec![0_i16; SAMPLE_RATE as usize];
        let (output, stats) = enhance_microphone_audio(&input, SAMPLE_RATE);
        assert_eq!(output, input);
        assert!(!stats.applied);
        assert_eq!(stats.gain_db, 0.0);
    }

    #[test]
    fn quiet_speech_is_raised_without_raising_leading_room_tone() {
        let room_tone = sine(0.001, 1.0, 120.0);
        let speech = sine(0.025, 1.0, 440.0);
        let mut input = room_tone.clone();
        input.extend_from_slice(&speech);
        input.extend_from_slice(&room_tone);

        let (output, stats) = enhance_microphone_audio(&input, SAMPLE_RATE);
        assert!(stats.applied);
        assert!((10.0..=12.0).contains(&stats.gain_db));
        assert!(stats.active_rms_after_dbfs - stats.active_rms_before_dbfs > 9.0);
        assert!(stats.peak_after_dbfs <= PEAK_LIMIT_DBFS + 0.05);

        // Check a stable noise-only region before speech. Net gain there must
        // remain approximately unity even while speech receives ~12 dB.
        let before_noise = rms(&input[SAMPLE_RATE as usize / 4..SAMPLE_RATE as usize * 3 / 4]);
        let after_noise = rms(&output[SAMPLE_RATE as usize / 4..SAMPLE_RATE as usize * 3 / 4]);
        assert!((linear_to_dbfs(after_noise) - linear_to_dbfs(before_noise)).abs() < 0.25);
    }

    #[test]
    fn steady_signal_without_noise_separation_is_not_aggressively_changed() {
        let input = sine(0.02, 2.0, 440.0);
        let (output, stats) = enhance_microphone_audio(&input, SAMPLE_RATE);
        assert_eq!(output, input);
        assert!(!stats.applied);
    }

    #[test]
    fn peak_headroom_prevents_clipping() {
        let room_tone = sine(0.001, 1.0, 120.0);
        let mut speech = sine(0.025, 1.0, 440.0);
        speech[SAMPLE_RATE as usize / 2] = 30_000;
        let mut input = room_tone.clone();
        input.extend_from_slice(&speech);
        input.extend_from_slice(&room_tone);

        let (output, stats) = enhance_microphone_audio(&input, SAMPLE_RATE);
        assert!(!stats.applied || stats.peak_after_dbfs <= PEAK_LIMIT_DBFS + 0.05);
        assert!(output.iter().all(|&sample| sample != i16::MAX));
    }
}
