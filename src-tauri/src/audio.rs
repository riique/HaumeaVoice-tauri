//! Audio capture and WAV encoding pipeline.
//!
//! All processing happens exclusively in RAM. No intermediate files
//! are ever written to disk. The output of [`create_wav_buffer`] is
//! a self-contained `Vec<u8>` ready to be streamed to a cloud API.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use crate::models::{AppState, DeepgramMode, TranscriptionEngine};

/// Emits the `transcribing` state boolean **only to the gadget window**,
/// which is the sole subscriber (see `GadgetView.tsx`). Broadcast emits via
/// `app.emit` also wake the much busier main window's IPC bridge for nothing
/// — extra pressure that compounds the gadget AppHang when the overlay is
/// backgrounded and the WebView2 JS task queue is throttled.
/// Best-effort: silently no-ops when the gadget window is gone.
fn emit_transcribing(handle: &tauri::AppHandle, value: bool) {
    use tauri::{Emitter, Manager};
    if let Some(g) = handle.get_webview_window("gadget") {
        let _ = g.emit("transcribing", value);
    }
}

/// Target sample rate for the final WAV buffer. Cloud STT engines
/// such as Groq Whisper and Deepgram Nova-3 expect 16 kHz audio.
pub const TARGET_SAMPLE_RATE: u32 = 16_000;
/// Number of channels in the output WAV. Always mono.
const CHANNELS: u16 = 1;
/// Bit depth of the output WAV. Always 16-bit signed PCM.
const BITS_PER_SAMPLE: u16 = 16;

pub fn spawn_audio_level_emitter(handle: tauri::AppHandle, state: Arc<AppState>) {
    let pending_emit = Arc::new(AtomicBool::new(false));

    std::thread::spawn(move || {
        while state.is_recording() {
            std::thread::sleep(std::time::Duration::from_millis(80));

            let capture_rate = *state.capture_rate.read();
            // Shorter analysis window → snappier reaction to speech onsets.
            let window = ((capture_rate as usize) / 20).max(256);
            let mut level = state.recent_level(window);

            // Gate only true silence / very low room tone, then expand speech
            // into the full 0..1 visual range. Lower threshold + steeper map
            // so quiet-to-normal talking visibly drives the gadget bars.
            level = if level < 0.012 {
                0.0
            } else {
                // Subtract gate, stretch by ~0.38 full-scale span, then gamma
                // < 1 so mid speech reads stronger without instantly clipping.
                ((level - 0.012) / 0.38).clamp(0.0, 1.0).powf(0.55)
            };

            if pending_emit.swap(true, Ordering::AcqRel) {
                continue;
            }

            let app_for_emit = handle.clone();
            let pending_for_emit = pending_emit.clone();
            let pending_for_error = pending_emit.clone();
            let queued = handle.run_on_main_thread(move || {
                use tauri::{Emitter, Manager};
                if let Some(gadget) = app_for_emit.get_webview_window("gadget") {
                    let _ = gadget.emit(crate::models::event_names::AUDIO_LEVEL, level);
                }
                pending_for_emit.store(false, Ordering::Release);
            });

            if queued.is_err() {
                pending_for_error.store(false, Ordering::Release);
            }
        }

        let app_for_emit = handle.clone();
        let _ = handle.run_on_main_thread(move || {
            use tauri::{Emitter, Manager};
            if let Some(gadget) = app_for_emit.get_webview_window("gadget") {
                let _ = gadget.emit(crate::models::event_names::AUDIO_LEVEL, 0.0_f32);
            }
        });
    });
}

/// Errors that can occur while acquiring the microphone or building
/// the input stream. All variants are graceful: the caller should
/// emit a `recording-cancelled` event and keep the Tauri process
/// alive regardless of the failure mode.
#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("no default input device is available on this host")]
    NoInputDevice,
    #[error("default input device failed to open: {0}")]
    DeviceOpen(String),
    #[error("unsupported sample format returned by the device: {0:?}")]
    UnsupportedSampleFormat(SampleFormat),
    #[error("failed to build the input stream: {0}")]
    StreamBuild(String),
    #[error("failed to start the input stream: {0}")]
    StreamPlay(String),
}

/// Builds a 44-byte canonical RIFF/WAVE header followed by the
/// little-endian 16-bit PCM payload extracted from `samples`.
///
/// The resulting `Vec<u8>` is fully self-contained and lives only in
/// RAM. Metadata is hardcoded to 16000 Hz, mono, 16-bit as required
/// by the downstream STT providers.
///
/// Layout (offsets are zero-based):
///   0  "RIFF"            (4 bytes)
///   4  ChunkSize         (u32 LE) = 36 + data length
///   8  "WAVE"            (4 bytes)
///  12  "fmt "            (4 bytes)
///  16  Subchunk1Size     (u32 LE) = 16 (PCM)
///  20  AudioFormat       (u16 LE) = 1 (PCM)
///  22  NumChannels       (u16 LE) = 1
///  24  SampleRate        (u32 LE) = 16000
///  28  ByteRate          (u32 LE) = 32000
///  32  BlockAlign        (u16 LE) = 2
///  34  BitsPerSample     (u16 LE) = 16
///  36  "data"            (4 bytes)
///  40  Subchunk2Size     (u32 LE) = data length
///  44  PCM samples       (i16 LE, interleaved)
pub fn create_wav_buffer(samples: &[i16]) -> Vec<u8> {
    let data_len = samples.len() * (BITS_PER_SAMPLE as usize / 8);
    let chunk_size = 36 + data_len;
    let byte_rate = (TARGET_SAMPLE_RATE * CHANNELS as u32 * BITS_PER_SAMPLE as u32) / 8;
    let block_align = (CHANNELS * BITS_PER_SAMPLE) / 8;

    let mut buffer: Vec<u8> = Vec::with_capacity(44 + data_len);

    // --- RIFF header ------------------------------------------------
    buffer.extend_from_slice(b"RIFF");
    buffer.extend_from_slice(&(chunk_size as u32).to_le_bytes());
    buffer.extend_from_slice(b"WAVE");

    // --- fmt sub-chunk ---------------------------------------------
    buffer.extend_from_slice(b"fmt ");
    buffer.extend_from_slice(&16u32.to_le_bytes()); // PCM subchunk size
    buffer.extend_from_slice(&1u16.to_le_bytes()); // PCM audio format
    buffer.extend_from_slice(&CHANNELS.to_le_bytes());
    buffer.extend_from_slice(&TARGET_SAMPLE_RATE.to_le_bytes());
    buffer.extend_from_slice(&byte_rate.to_le_bytes());
    buffer.extend_from_slice(&block_align.to_le_bytes());
    buffer.extend_from_slice(&BITS_PER_SAMPLE.to_le_bytes());

    // --- data sub-chunk ---------------------------------------------
    buffer.extend_from_slice(b"data");
    buffer.extend_from_slice(&(data_len as u32).to_le_bytes());

    // --- PCM payload (i16 -> LE bytes) -----------------------------
    for &sample in samples {
        buffer.extend_from_slice(&sample.to_le_bytes());
    }

    buffer
}

/// Acquires the default input device and opens a capture stream at the
/// device's **native** sample rate and channel count. Forcing a non-native
/// config (e.g. 16 kHz mono) on Windows WASAPI shared mode silently fails
/// or returns an error, which was the root cause of transcription never
/// working. Samples are down-mixed to mono `i16` inside the data callback
/// and the native rate is stored in `state.capture_rate` so `stop_capture`
/// can resample to 16 kHz when building the final WAV.
pub fn start_capture(state: &Arc<AppState>) -> Result<(), AudioError> {
    state.clear_audio_buffer();

    let host = cpal::default_host();
    let configured_device = crate::settings::load_input_device();
    let device = if let Some(ref name) = configured_device {
        let mut found = None;
        if let Ok(mut devices) = host.input_devices() {
            if let Some(d) = devices.find(|d| d.name().map(|n| &n == name).unwrap_or(false)) {
                found = Some(d);
            }
        }
        found.or_else(|| host.default_input_device())
    } else {
        host.default_input_device()
    };

    let device = device.ok_or(AudioError::NoInputDevice)?;

    log::info!(
        "audio: opened input device {:?}",
        device.name().unwrap_or_else(|_| "<unknown>".into())
    );

    let supported = device
        .default_input_config()
        .map_err(|e| AudioError::DeviceOpen(e.to_string()))?;

    let sample_format = supported.sample_format();
    let native_rate = supported.sample_rate().0;
    let native_channels = supported.channels();

    log::info!(
        "audio: native format {} Hz, {} ch, {:?}",
        native_rate,
        native_channels,
        sample_format
    );

    // Persist so the WAV resampler knows the source rate later.
    *state.capture_rate.write() = native_rate;

    // Use the device's own config — no forced rate or channel count.
    let config: StreamConfig = supported.into();

    let err_callback = |err: cpal::StreamError| {
        log::error!("audio: stream error: {}", err);
    };

    let ch = native_channels as usize;

    let stream: Stream = match sample_format {
        SampleFormat::I16 => {
            let st = state.clone();
            device
                .build_input_stream(
                    &config,
                    move |samples: &[i16], _: &cpal::InputCallbackInfo| {
                        if !st.is_recording() {
                            return;
                        }
                        let mono = downmix_i16(samples, ch);
                        // Use try_lock to avoid blocking the real-time audio
                        // thread if the buffer is momentarily held by another
                        // caller (e.g. drain_audio_buffer during stop).
                        if let Some(mut guard) = st.audio_buffer.try_lock() {
                            guard.extend_from_slice(&mono);
                        }
                        // Fan-out to live Deepgram while the user is speaking
                        // so stop only needs a Finalize flush (not a full re-upload).
                        push_live_deepgram_pcm(&st, &mono);
                    },
                    err_callback,
                    None,
                )
                .map_err(|e| AudioError::StreamBuild(e.to_string()))?
        }
        SampleFormat::F32 => {
            let st = state.clone();
            device
                .build_input_stream(
                    &config,
                    move |samples: &[f32], _: &cpal::InputCallbackInfo| {
                        if !st.is_recording() {
                            return;
                        }
                        let mono = downmix_f32(samples, ch);
                        if let Some(mut guard) = st.audio_buffer.try_lock() {
                            guard.extend_from_slice(&mono);
                        }
                        push_live_deepgram_pcm(&st, &mono);
                    },
                    err_callback,
                    None,
                )
                .map_err(|e| AudioError::StreamBuild(e.to_string()))?
        }
        other => {
            return Err(AudioError::UnsupportedSampleFormat(other));
        }
    };

    stream
        .play()
        .map_err(|e| AudioError::StreamPlay(e.to_string()))?;

    *state.audio_stream.lock() = Some(stream);

    // When streaming_final is selected and Deepgram is in the path, open the
    // WebSocket now and process audio during capture (not only after stop).
    maybe_start_deepgram_live(state);

    Ok(())
}

fn downmix_i16(samples: &[i16], ch: usize) -> Vec<i16> {
    if ch <= 1 {
        return samples.to_vec();
    }
    let mut mono = Vec::with_capacity(samples.len() / ch);
    for frame in samples.chunks(ch) {
        let sum: i32 = frame.iter().map(|&s| s as i32).sum();
        mono.push((sum / ch as i32) as i16);
    }
    mono
}

fn downmix_f32(samples: &[f32], ch: usize) -> Vec<i16> {
    if ch <= 1 {
        return samples
            .iter()
            .map(|s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
            .collect();
    }
    let mut mono = Vec::with_capacity(samples.len() / ch);
    for frame in samples.chunks(ch) {
        let sum: f32 = frame.iter().sum();
        let avg = (sum / ch as f32).clamp(-1.0, 1.0);
        mono.push((avg * 32767.0) as i16);
    }
    mono
}

/// Non-blocking fan-out of mono PCM into the live Deepgram session (if any).
fn push_live_deepgram_pcm(state: &AppState, mono: &[i16]) {
    let Some(guard) = state.deepgram_live.try_lock() else {
        return;
    };
    if let Some(session) = guard.as_ref() {
        session.push_mono_i16(mono);
    }
}

/// Opens a live Deepgram WebSocket when mode/engine/key allow it.
fn maybe_start_deepgram_live(state: &Arc<AppState>) {
    // Abort any stale session from a previous recording.
    if let Some(old) = state.deepgram_live.lock().take() {
        old.abort();
    }

    let mode = *state.deepgram_mode.read();
    if mode != DeepgramMode::StreamingFinal {
        return;
    }

    let dual = *state.dual_engine.read();
    let engine = state.active_engine();
    let deepgram_in_path = dual || engine == TranscriptionEngine::DeepgramNova3;
    if !deepgram_in_path {
        return;
    }

    let api_key = {
        let guard = state.api_keys.read();
        guard.deepgram.clone().filter(|k| !k.trim().is_empty())
    };
    let Some(api_key) = api_key else {
        log::warn!("audio: streaming_final selected but Deepgram API key missing; live session skipped");
        return;
    };

    let sample_rate = *state.capture_rate.read();
    let session = crate::deepgram::spawn_live_session(api_key, sample_rate);
    *state.deepgram_live.lock() = Some(session);
    log::info!(
        "audio: deepgram LIVE streaming_final started @ {} Hz (process while recording)",
        sample_rate
    );
}

/// Finishes a live Deepgram session if present; on failure falls back to
/// **batch REST** with the full local WAV (preferred over WS post-hoc).
async fn deepgram_from_live_or_posthoc(
    state: &Arc<AppState>,
    live: Option<crate::deepgram::DeepgramLiveSession>,
    wav: Vec<u8>,
    mime: &str,
) -> Result<String, String> {
    let api_key = {
        let guard = state.api_keys.read();
        guard.deepgram.clone().filter(|k| !k.trim().is_empty())
    };
    let Some(api_key) = api_key else {
        return Err("Chave de API do Deepgram não configurada.".to_string());
    };
    let mode = *state.deepgram_mode.read();

    if let Some(session) = live {
        let t0 = std::time::Instant::now();
        match session.finish().await {
            Ok(text) => {
                log::info!(
                    "audio: deepgram LIVE finish in {}ms ({} chars) — no full re-upload",
                    t0.elapsed().as_millis(),
                    text.len()
                );
                return Ok(text);
            }
            Err(e) => {
                log::warn!(
                    "audio: deepgram LIVE failed after stop ({}ms): {}; falling back to batch REST",
                    t0.elapsed().as_millis(),
                    e
                );
                // Live failed mid-utterance: send the complete WAV via pre-recorded API.
                return crate::deepgram::transcribe(
                    wav,
                    mime,
                    &api_key,
                    crate::models::DeepgramMode::Batch,
                )
                .await;
            }
        }
    }

    // No live session (connect never started, or mode is batch-only).
    crate::deepgram::transcribe(wav, mime, &api_key, mode).await
}

/// Stops the active capture stream (if any), drains the accumulated
/// samples, builds the final in-memory WAV buffer and routes it
/// through a two-stage pipeline driven entirely by the managed state.
///
/// Stage 1 - Acoustic transcription:
///   The active engine stored in `state.engine` selects the cloud STT
///   provider. `GroqWhisper` dispatches the buffer to
///   [`crate::groq::call_whisper_api`]; `DeepgramNova3` dispatches the
///   same buffer to [`crate::deepgram::transcribe`] (batch or streaming
///   final, depending on `deepgram_mode`). Any other
///   variant aborts the pipeline. If the chosen acoustic engine fails
///   (network, auth, parse) the pipeline is aborted immediately and
///   the error is logged; the user is expected to retry.
///
/// Stage 2 - Unified sanitization and clipboard injection:
///   The raw text produced by whichever acoustic engine ran is handed
///   to [`crate::groq::call_sanitizer_api`] (Llama 3.3 70B or
///   GPT-OSS 120B, plus the user-edited system prompt). The sanitizer
///   always runs on the Groq Chat Completions endpoint regardless of
///   the acoustic engine, so the Groq API key is required here
///   independently of Stage 1.
///
/// Resilience contract:
///   * If the acoustic engine fails, the pipeline aborts (no text is
///     emitted).
///   * If the sanitizer network call fails for any reason, the raw
///     acoustic transcription is copied to the clipboard instead so
///     the user never loses dictated content.
///   * If the sanitizer returns the sentinel `[FALLBACK_RETRY]`, the
///     raw acoustic transcription is also used.
///
/// The function is async because every HTTP exchange must run off the
/// Tauri main thread. Callers should spawn it on the multi-threaded
/// Tokio runtime via [`tauri::async_runtime::spawn`].
pub async fn stop_capture(state: &Arc<AppState>) -> Option<String> {
    if let Some(handle) = state.app_handle.read().as_ref() {
        emit_transcribing(handle, true);
    }

    // Drop guard: emits transcribing=false even if the future panics or is
    // cancelled, preventing the gadget from staying stuck on "Processando...".
    struct TranscribingGuard(Option<tauri::AppHandle>);
    impl Drop for TranscribingGuard {
        fn drop(&mut self) {
            if let Some(handle) = self.0.as_ref() {
                emit_transcribing(handle, false);
            }
        }
    }
    let _guard = TranscribingGuard(state.app_handle.read().clone());

    stop_capture_inner(state).await
}

async fn stop_capture_inner(state: &Arc<AppState>) -> Option<String> {
    let _ = state.drop_audio_stream();

    // Take the live session (if any) before draining so we can catch up any
    // samples that arrived after `recording` flipped to false.
    let live_session = state.deepgram_live.lock().take();
    {
        let buf = state.audio_buffer.lock();
        if let Some(ref session) = live_session {
            session.catch_up_from_buffer(&buf);
        }
    }

    let raw_samples = state.drain_audio_buffer();
    if raw_samples.is_empty() {
        log::warn!("audio: stop requested but buffer was empty");
        if let Some(session) = live_session {
            session.abort();
        }
        return None;
    }

    let capture_rate = *state.capture_rate.read();
    let raw_count = raw_samples.len();

    // Duration is based on the number of mono samples at the native rate.
    let duration_ms = (raw_count as u64 * 1000) / capture_rate as u64;

    // Resample to 16 kHz if the device captured at a different rate.
    let samples = if capture_rate == TARGET_SAMPLE_RATE {
        raw_samples
    } else {
        resample(&raw_samples, capture_rate, TARGET_SAMPLE_RATE)
    };

    let wav = create_wav_buffer(&samples);
    let kb = wav.len() / 1024;
    log::info!(
        "audio: WAV buffer generated in RAM ({} samples @ {} Hz -> {} samples @ {} Hz, {} KB)",
        raw_count,
        capture_rate,
        samples.len(),
        TARGET_SAMPLE_RATE,
        kb
    );

    let dual_mode = *state.dual_engine.read();
    let engine = state.active_engine();
    log::info!(
        "audio: active transcription engine {:?} (dual_mode={}, live_deepgram={})",
        engine,
        dual_mode,
        live_session.is_some()
    );

    // Motor latency is measured from stop — with live streaming this should
    // be only Finalize+drain, not a full re-upload of the recording.
    let start_time = std::time::Instant::now();

    // 1. Eagerly generate ID and save audio so we can retry!
    let id = chrono_like_id();
    let audio_path = crate::audio_store::save(&id, "wav", &wav);

    let effective_dual;
    let deepgram_ran;
    let (whisper_text, deepgram_text) = if dual_mode {
        // Groq Whisper (post-hoc WAV) || Deepgram live finish / post-hoc fallback
        let wav_clone = wav.clone();
        let state_clone1 = state.clone();
        let state_clone2 = state.clone();
        let groq_fut = transcribe_bytes(
            &state_clone1,
            wav_clone,
            "audio.wav",
            "audio/wav",
            TranscriptionEngine::GroqWhisper,
        );
        let deepgram_fut =
            deepgram_from_live_or_posthoc(&state_clone2, live_session, wav.clone(), "audio/wav");

        let (groq_res, deepgram_res) = tokio::join!(groq_fut, deepgram_fut);

        match (groq_res, deepgram_res) {
            (Ok(g), Ok(d)) => {
                effective_dual = true;
                deepgram_ran = true;
                (g, d)
            }
            (Ok(g), Err(de_err)) => {
                log::warn!(
                    "audio: Deepgram falhou no modo duplo, usando apenas Groq Whisper: {}",
                    de_err
                );
                effective_dual = false;
                deepgram_ran = false;
                (g, String::new())
            }
            (Err(groq_err), Ok(d)) => {
                log::warn!(
                    "audio: Groq Whisper falhou no modo duplo, usando apenas Deepgram: {}",
                    groq_err
                );
                effective_dual = false;
                deepgram_ran = true;
                (String::new(), d)
            }
            (Err(groq_err), Err(de_err)) => {
                log::error!("audio: ambos os motores de transcrição falharam no modo duplo");
                let err_msg = format!(
                    "Ambos os motores de transcrição falharam no modo duplo:\n\
                     • Groq Whisper: {}\n\
                     • Deepgram Nova-3: {}",
                    groq_err, de_err
                );
                save_failed_transcription(
                    state,
                    id,
                    audio_path,
                    duration_ms,
                    "mic",
                    engine,
                    err_msg,
                );
                return None;
            }
        }
    } else if engine == TranscriptionEngine::DeepgramNova3 {
        effective_dual = false;
        match deepgram_from_live_or_posthoc(state, live_session, wav.clone(), "audio/wav").await {
            Ok(text) => {
                let (w, d, dg_ran) = single_engine_slots(TranscriptionEngine::DeepgramNova3, text);
                deepgram_ran = dg_ran;
                (w, d)
            }
            Err(err_msg) => {
                save_failed_transcription(
                    state,
                    id,
                    audio_path,
                    duration_ms,
                    "mic",
                    engine,
                    format!("Deepgram: {}", err_msg),
                );
                return None;
            }
        }
    } else {
        // Pure Groq (or other) — abort unused live session if somehow open.
        if let Some(session) = live_session {
            session.abort();
        }
        effective_dual = false;
        match transcribe_bytes(state, wav.clone(), "audio.wav", "audio/wav", engine).await {
            Ok(text) => {
                let (w, d, dg_ran) = single_engine_slots(engine, text);
                deepgram_ran = dg_ran;
                (w, d)
            }
            Err(err_msg) => {
                save_failed_transcription(
                    state,
                    id,
                    audio_path,
                    duration_ms,
                    "mic",
                    engine,
                    err_msg,
                );
                return None;
            }
        }
    };

    if whisper_text.trim().is_empty() && deepgram_text.trim().is_empty() {
        let err_msg = "Nenhum texto detectado na gravação.".to_string();
        save_failed_transcription(
            state,
            id.clone(),
            audio_path.clone(),
            duration_ms,
            "mic",
            engine,
            err_msg,
        );
        return None;
    }

    let elapsed = start_time.elapsed().as_millis() as u64;

    let final_text = finalize_transcription(
        state,
        id,
        audio_path,
        engine,
        whisper_text,
        deepgram_text,
        true,
        duration_ms,
        "mic",
        elapsed,
        effective_dual,
        deepgram_ran,
    )
    .await?;

    log::debug!("audio: final output ({} chars)", final_text.len());

    Some(final_text)
}

/// Linear resampler: converts mono i16 samples from `from_rate` to `to_rate`.
/// Good enough for speech (no audible artefacts at 48→16 kHz). Avoids pulling
/// in a full DSP crate for a single use-case.
fn resample(input: &[i16], from_rate: u32, to_rate: u32) -> Vec<i16> {
    if from_rate == to_rate || input.is_empty() {
        return input.to_vec();
    }
    let ratio = from_rate as f64 / to_rate as f64;
    let out_len = ((input.len() as f64) / ratio).ceil() as usize;
    let mut output = Vec::with_capacity(out_len);
    let last = input.len() - 1;
    for i in 0..out_len {
        let src = i as f64 * ratio;
        // Clamp idx to prevent an out-of-bounds access when floating-point
        // rounding produces a value equal to input.len().
        let idx = (src as usize).min(last);
        let frac = src - idx as f64;
        let a = input[idx] as f64;
        let b = if idx + 1 < input.len() {
            input[idx + 1] as f64
        } else {
            a
        };
        output.push((a + (b - a) * frac) as i16);
    }
    output
}

/// Caps the length of an API error body so a giant JSON blob does not bloat
/// the history card. Truncates on a UTF-8 char boundary (never mid-codepoint)
/// and appends an ellipsis when something was cut.
fn truncate_err_body(body: &str, max_chars: usize) -> String {
    if body.chars().count() <= max_chars {
        return body.to_string();
    }
    let mut out: String = body.chars().take(max_chars).collect();
    out.push('…');
    out
}

/// Last-line-of-defense coalescer for the *final* transcript text.
///
/// The Groq Chat Completions sanitizer can — independent of any network or
/// parse error — return `Ok("")` (a successful response whose content is
/// empty). This is most often seen with the GPT-OSS 120B model: its chain-
/// of-thought reasoning diverges, exhausts the token budget, and the
/// assistant emits nothing usable in the `content` field. Without this
/// guard the empty string would simply flow through the clipboard / paste
/// pipeline and the user would receive nothing despite having spoken.
///
/// Contract (user request): when the finalized text is blank, prefer the
/// **Deepgram raw transcription** over the Whisper raw, because in dual
/// mode Deepgram is the independent second acoustic pass least likely to
/// share the same failure mode as the LLM sanitizer. If Deepgram is also
/// empty (single-engine Groq Whisper runs, for example, set
/// `deepgram_text` to a clone of the Whisper text), fall back to the
/// Whisper raw. `String::new()` is returned only when both raw texts are
/// themselves blank — in which case the upstream "Nenhum texto detectado"
/// short-circuit will already have fired, so this branch is unreachable in
/// practice.
fn coalesce_empty_final(finalized: String, whisper_text: &str, deepgram_text: &str) -> String {
    if !finalized.trim().is_empty() {
        return finalized;
    }
    if !deepgram_text.trim().is_empty() {
        log::warn!(
            "audio: sanitizer returned empty final_text (likely GPT-OSS reasoning divergence); \
             falling back to DeepGram raw transcription ({} chars)",
            deepgram_text.trim().len()
        );
        return deepgram_text.trim().to_string();
    }
    if !whisper_text.trim().is_empty() {
        log::warn!(
            "audio: sanitizer returned empty final_text and Deepgram raw is also empty; \
             falling back to Whisper raw transcription ({} chars)",
            whisper_text.trim().len()
        );
        return whisper_text.trim().to_string();
    }
    finalized
}

/// Maps a [`crate::groq::GroqNetworkError`] to a specific, human-readable
/// Portuguese message that is safe to surface in the Histórico tab. The
/// raw API error body is truncated via [`truncate_err_body`] so it stays
/// readable in the UI.
fn groq_err_to_message(e: &crate::groq::GroqNetworkError) -> String {
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

/// Routes `bytes` to the cloud STT provider selected by `engine` and returns
/// the raw (un-sanitised) transcript. Shared by microphone capture and file
/// upload. `file_name`/`mime` describe the payload so Groq's multipart decoder
/// and Deepgram's container detection select the correct codec.
///
/// Returns `Err(message)` with a specific, human-readable Portuguese reason on
/// failure so callers can persist it verbatim into the history entry instead
/// of a generic "falhou" string.
async fn transcribe_bytes(
    state: &Arc<AppState>,
    bytes: Vec<u8>,
    file_name: &str,
    mime: &str,
    engine: TranscriptionEngine,
) -> Result<String, String> {
    match engine {
        TranscriptionEngine::GroqWhisper => {
            let api_key = {
                let guard = state.api_keys.read();
                guard.groq.clone().filter(|k| !k.trim().is_empty())
            };
            let Some(api_key) = api_key else {
                log::error!("audio: groq api key is missing, skipping transcription");
                return Err("Chave de API do Groq não configurada.".to_string());
            };
            log::info!("audio: dispatching audio to groq whisper");
            match crate::groq::call_whisper_api(bytes, file_name, mime, &api_key).await {
                Ok(text) => {
                    log::info!(
                        "audio: whisper transcription received ({} chars)",
                        text.len()
                    );
                    Ok(text)
                }
                Err(e) => {
                    log::error!("audio: groq whisper transcription failed: {}", e);
                    Err(groq_err_to_message(&e))
                }
            }
        }
        TranscriptionEngine::DeepgramNova3 => {
            let api_key = {
                let guard = state.api_keys.read();
                guard.deepgram.clone().filter(|k| !k.trim().is_empty())
            };
            let Some(api_key) = api_key else {
                log::error!("audio: deepgram api key is missing, skipping transcription");
                return Err("Chave de API do Deepgram não configurada.".to_string());
            };
            let mode = *state.deepgram_mode.read();
            log::info!(
                "audio: dispatching audio to deepgram nova-3 (mode={})",
                mode.as_str()
            );
            match crate::deepgram::transcribe(bytes, mime, &api_key, mode).await {
                Ok(text) => {
                    log::info!(
                        "audio: deepgram transcription received ({} chars, mode={})",
                        text.len(),
                        mode.as_str()
                    );
                    Ok(text)
                }
                Err(e) => {
                    log::error!(
                        "audio: deepgram transcription failed (mode={}): {}",
                        mode.as_str(),
                        e
                    );
                    Err(format!("Deepgram: {}", e))
                }
            }
        }
        other => {
            let msg = format!(
                "O motor {:?} não está conectado ao pipeline de captura de áudio.",
                other
            );
            log::error!("audio: {}", msg);
            Err(msg)
        }
    }
}

/// Deepgram mode label for history when Deepgram STT actually ran.
fn history_deepgram_mode(state: &AppState, deepgram_ran: bool) -> Option<String> {
    if deepgram_ran {
        Some(state.deepgram_mode.read().as_str().to_string())
    } else {
        None
    }
}

/// Acoustic real-time factor: how long the STT took relative to audio length.
fn compute_realtime_factor(transcription_latency_ms: u64, duration_ms: u64) -> Option<f64> {
    if transcription_latency_ms > 0 && duration_ms > 0 {
        Some(transcription_latency_ms as f64 / duration_ms as f64)
    } else {
        None
    }
}

/// When the sanitizer is off or fails, pick the best available raw acoustic text.
/// Dual mode must not silently discard Deepgram if Whisper is non-empty but worse.
fn pick_raw_acoustic(whisper_text: &str, deepgram_text: &str) -> String {
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
            // Prefer the longer transcript (more content recovered). When lengths
            // are close, prefer Deepgram (product bias: numerals, "Haumea").
            let wl = w.chars().count();
            let dl = d.chars().count();
            if wl > dl.saturating_add(8) {
                log::info!(
                    "audio: pick_raw chose Whisper ({} vs {} chars)",
                    wl,
                    dl
                );
                w.to_string()
            } else {
                log::info!(
                    "audio: pick_raw chose Deepgram ({} vs {} chars)",
                    dl,
                    wl
                );
                d.to_string()
            }
        }
    }
}

/// Word count for motor throughput: max of the two acoustics (not Whisper-only).
fn acoustic_word_count(whisper_text: &str, deepgram_text: &str) -> usize {
    let w = whisper_text.split_whitespace().count();
    let d = deepgram_text.split_whitespace().count();
    w.max(d)
}

/// Honest history engine label after dual may have degraded to a single STT.
fn history_engine_label(
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

/// Parallel Groq Whisper + Deepgram (post-hoc) for dual mode. Shared by
/// file upload and any path that cannot use a live Deepgram session.
async fn run_dual_posthoc(
    state: &Arc<AppState>,
    bytes: Vec<u8>,
    file_name: &str,
    mime: &str,
) -> Result<(String, String, bool, bool), String> {
    let wav_clone = bytes.clone();
    let state1 = state.clone();
    let state2 = state.clone();
    let groq_fut = transcribe_bytes(
        &state1,
        wav_clone,
        file_name,
        mime,
        TranscriptionEngine::GroqWhisper,
    );
    let deepgram_fut = transcribe_bytes(
        &state2,
        bytes,
        file_name,
        mime,
        TranscriptionEngine::DeepgramNova3,
    );
    let (groq_res, deepgram_res) = tokio::join!(groq_fut, deepgram_fut);
    match (groq_res, deepgram_res) {
        (Ok(g), Ok(d)) => Ok((g, d, true, true)),
        (Ok(g), Err(de_err)) => {
            log::warn!(
                "audio: Deepgram falhou no modo duplo, usando apenas Groq Whisper: {}",
                de_err
            );
            Ok((g, String::new(), false, false))
        }
        (Err(groq_err), Ok(d)) => {
            log::warn!(
                "audio: Groq Whisper falhou no modo duplo, usando apenas Deepgram: {}",
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

/// Map a single-engine transcript into the correct sanitizer slot so
/// [WHISPER_RAW] / [DEEPGRAM_RAW] are not filled with a fake duplicate.
fn single_engine_slots(engine: TranscriptionEngine, text: String) -> (String, String, bool) {
    match engine {
        TranscriptionEngine::DeepgramNova3 => (String::new(), text, true),
        TranscriptionEngine::GroqWhisper => (text, String::new(), false),
        other => {
            // Gemini etc. are not on the mic STT path; treat as generic whisper slot.
            log::warn!("audio: single_engine_slots for unexpected engine {:?}", other);
            (text, String::new(), false)
        }
    }
}

fn save_failed_transcription(
    state: &Arc<AppState>,
    id: String,
    audio_path: Option<String>,
    duration_ms: u64,
    source: &str,
    engine: TranscriptionEngine,
    error_msg: String,
) {
    let dual_mode = *state.dual_engine.read();
    // On hard failure we may not know if Deepgram ran; flag intent for telemetry.
    let deepgram_attempted =
        dual_mode || engine == TranscriptionEngine::DeepgramNova3;
    let entry = crate::models::HistoryEntry {
        id,
        date: now_timestamp(),
        words: 0,
        engine: format!("{:?}", engine),
        text: String::new(),
        audio_path,
        evaluation: None,
        duration_ms,
        source: source.to_string(),
        latency_ms: 0,
        throughput: 0.0,
        transcription_latency_ms: None,
        sanitizer_latency_ms: None,
        transcription_throughput: None,
        sanitizer_throughput: None,
        realtime_factor: None,
        deepgram_mode: history_deepgram_mode(state, deepgram_attempted),
        total_tokens: None,
        is_error: Some(true),
        error_message: Some(error_msg),
        debug_info: None,
    };
    crate::history::push(entry.clone());

    if let Some(handle) = state.app_handle.read().as_ref() {
        use tauri::Emitter;
        if let Err(e) = handle.emit(crate::models::event_names::TRANSCRIPTION_SAVED, &entry) {
            log::warn!("audio: failed to emit transcription-saved: {}", e);
        }
    }
}

/// Sanitises `raw_text` (Groq Chat Completions), optionally copies it to the
/// clipboard, persists the source audio to disk and appends a history entry,
/// announcing it to the UI. Shared terminal stage for microphone capture and
/// file upload. Any sanitizer failure falls back to the raw text so the user
/// never loses dictated content.
async fn finalize_transcription(
    state: &Arc<AppState>,
    id: String,
    audio_path: Option<String>,
    engine: TranscriptionEngine,
    whisper_text: String,
    deepgram_text: String,
    copy_to_clipboard: bool,
    duration_ms: u64,
    source: &str,
    latency_ms: u64, // tempo de transcrição
    dual_mode: bool,
    deepgram_ran: bool,
) -> Option<String> {
    let sanitizer_key = {
        let guard = state.api_keys.read();
        guard.groq.clone().filter(|k| !k.trim().is_empty())
    };
    let (
        model_id,
        supports_reasoning,
        system_prompt,
        custom_words,
        reasoning_enabled,
        reasoning_effort,
    ) = {
        let sanitizer = *state.sanitizer.read();
        (
            sanitizer.api_model_id(),
            sanitizer.supports_reasoning(),
            state.system_prompt.read().clone(),
            state.custom_words.read().clone(),
            *state.reasoning_enabled.read(),
            state.reasoning_effort.read().clone(),
        )
    };

    let system_prompt_to_use = if dual_mode {
        format!(
            "{}\n\n--- INSTRUÇÃO DE MOTOR DUPLO ---\nVocê recebeu duas transcrições acústicas brutas (Transcrição A e Transcrição B) do mesmo áudio. Compare-as, corrija falhas fonéticas, pontue de forma correta e mescle as informações de forma inteligente para produzir o melhor texto unificado.",
            system_prompt
        )
    } else {
        system_prompt
    };

    let raw_words = acoustic_word_count(&whisper_text, &deepgram_text);
    let mut debug_info: Option<crate::models::SanitizerDebug> = None;
    let start_sanitizer = std::time::Instant::now();
    let final_text = if !*state.sanitizer_enabled.read() {
        // Sanitizer disabled: pick the best raw acoustic (not Whisper-only).
        let picked = pick_raw_acoustic(&whisper_text, &deepgram_text);
        log::info!(
            "audio: sanitizer disabled, using pick_raw ({} chars)",
            picked.len()
        );
        picked
    } else {
        match sanitizer_key {
            Some(key) => {
                let outcome = crate::groq::call_sanitizer_api(
                    &whisper_text,
                    &deepgram_text,
                    model_id,
                    &system_prompt_to_use,
                    &custom_words,
                    &key,
                    reasoning_enabled,
                    &reasoning_effort,
                    supports_reasoning,
                )
                .await;

                debug_info = Some(outcome.debug);
                match outcome.result {
                    Ok(sanitized) => {
                        if sanitized.trim() == crate::groq::FALLBACK_RETRY_SENTINEL {
                            log::warn!(
                                "audio: sanitizer returned fallback sentinel, using pick_raw"
                            );
                            pick_raw_acoustic(&whisper_text, &deepgram_text)
                        } else {
                            log::info!(
                                "audio: sanitizer returned purified text ({} chars)",
                                sanitized.len()
                            );
                            sanitized
                        }
                    }
                    Err(e) => {
                        log::error!("audio: sanitizer failed: {e}; using pick_raw");
                        pick_raw_acoustic(&whisper_text, &deepgram_text)
                    }
                }
            }
            None => {
                log::warn!(
                    "audio: no sanitizer API key set for model, falling back to pick_raw"
                );
                pick_raw_acoustic(&whisper_text, &deepgram_text)
            }
        }
    };
    // Safety contract: when the sanitizer returned literally nothing (the
    // GPT-OSS 120B "reasoning divergence" failure mode), prefer the Deepgram
    // raw transcription over Whisper before letting an empty string reach the
    // clipboard / paste pipeline. See `coalesce_empty_final`.
    let final_text = coalesce_empty_final(final_text, &whisper_text, &deepgram_text);
    let sanitizer_latency_ms = start_sanitizer.elapsed().as_millis() as u64;

    if copy_to_clipboard {
        let clipboard_text = final_text.clone();
        let clipboard_fut = tokio::task::spawn_blocking(move || {
            let mut clipboard = arboard::Clipboard::new()?;
            clipboard.set_text(clipboard_text)?;
            Ok::<(), arboard::Error>(())
        });
        match tokio::time::timeout(std::time::Duration::from_secs(3), clipboard_fut).await {
            Ok(Ok(Ok(()))) => {
                log::info!(
                    "audio: final text copied to clipboard ({} chars)",
                    final_text.len()
                );
                // Insert the text directly into whatever field currently
                // has focus (search bars, editors, chat boxes) by
                // simulating the paste shortcut. Done after the clipboard
                // is populated so the keystroke pastes the fresh text.
                if let Err(e) = paste_into_focused_field() {
                    log::warn!("audio: auto-paste failed (text still on clipboard): {}", e);
                }
            }
            Ok(Ok(Err(e))) => log::error!("audio: failed to set clipboard text: {}", e),
            Ok(Err(e)) => log::error!("audio: clipboard task panicked: {}", e),
            Err(_) => log::error!("audio: clipboard access timed out after 3s"),
        }
    }

    let words = final_text.split_whitespace().count();

    let transcription_throughput = if latency_ms > 0 {
        let est_tokens = raw_words as f64 * 1.3;
        Some((est_tokens * 1000.0) / latency_ms as f64)
    } else {
        None
    };

    let sanitizer_throughput = if sanitizer_latency_ms > 0 {
        let est_tokens = words as f64 * 1.3;
        Some((est_tokens * 1000.0) / sanitizer_latency_ms as f64)
    } else {
        None
    };

    let total_tokens = Some((words as f64 * 1.3).round() as usize);
    let total_latency_ms = latency_ms + sanitizer_latency_ms;
    let throughput = sanitizer_throughput.unwrap_or(0.0);
    let realtime_factor = compute_realtime_factor(latency_ms, duration_ms);
    let deepgram_mode = history_deepgram_mode(state, deepgram_ran);

    if let Some(rtf) = realtime_factor {
        log::info!(
            "audio: latency telemetry transcription_ms={} sanitizer_ms={} total_ms={} \
             duration_ms={} rtf={:.3} deepgram_mode={} dual={}",
            latency_ms,
            sanitizer_latency_ms,
            total_latency_ms,
            duration_ms,
            rtf,
            deepgram_mode.as_deref().unwrap_or("-"),
            dual_mode
        );
    } else {
        log::info!(
            "audio: latency telemetry transcription_ms={} sanitizer_ms={} total_ms={} \
             deepgram_mode={} dual={}",
            latency_ms,
            sanitizer_latency_ms,
            total_latency_ms,
            deepgram_mode.as_deref().unwrap_or("-"),
            dual_mode
        );
    }

    let entry = crate::models::HistoryEntry {
        id,
        date: now_timestamp(),
        words,
        engine: history_engine_label(engine, dual_mode, &whisper_text, &deepgram_text),
        text: final_text.clone(),
        audio_path,
        evaluation: None,
        duration_ms,
        source: source.to_string(),
        latency_ms: total_latency_ms,
        throughput,
        transcription_latency_ms: Some(latency_ms),
        sanitizer_latency_ms: Some(sanitizer_latency_ms),
        transcription_throughput,
        sanitizer_throughput,
        realtime_factor,
        deepgram_mode,
        total_tokens,
        is_error: Some(false),
        error_message: None,
        debug_info,
    };
    crate::history::push(entry.clone());

    if let Some(handle) = state.app_handle.read().as_ref() {
        use tauri::Emitter;
        if let Err(e) = handle.emit(crate::models::event_names::TRANSCRIPTION_SAVED, &entry) {
            log::warn!("audio: failed to emit transcription-saved: {}", e);
        }
    }

    Some(final_text)
}

/// Retries an error/failed transcription from the history.
///
/// On success the existing history entry is updated in place (text, metrics,
/// `is_error=false`) and the `transcription-saved` event is emitted so the
/// UI refreshes. On failure the entry is **also** updated — with the new
/// `error_message` and `is_error=true` — so the user sees the latest,
/// specific reason for the failure right in the Histórico tab, and the
/// "Retranscrever" button stays available for another attempt.
pub async fn retry_transcription_handler(
    state: &Arc<AppState>,
    id: &str,
) -> Result<String, String> {
    // Signal the gadget that transcription is in progress.
    if let Some(handle) = state.app_handle.read().as_ref() {
        emit_transcribing(handle, true);
    }

    struct TranscribingGuard(Option<tauri::AppHandle>);
    impl Drop for TranscribingGuard {
        fn drop(&mut self) {
            if let Some(handle) = self.0.as_ref() {
                emit_transcribing(handle, false);
            }
        }
    }
    let _guard = TranscribingGuard(state.app_handle.read().clone());

    let entry = crate::history::get(id).ok_or_else(|| "Histórico não encontrado".to_string())?;

    let audio_path = entry
        .audio_path
        .clone()
        .ok_or_else(|| "Este item não possui áudio salvo para retentar".to_string())?;

    // Helper used by every failure path below: rewrites the history entry as
    // a failed attempt with the given message and re-emits it so the UI shows
    // the fresh error immediately. The original `Err` is then propagated so
    // the frontend also surfaces it as a toast under the card.
    let fail =
        |state: &Arc<AppState>, entry: &crate::models::HistoryEntry, msg: String| -> String {
            let dual = *state.dual_engine.read();
            let eng = state.active_engine();
            let deepgram_attempted =
                dual || eng == TranscriptionEngine::DeepgramNova3;
            let failed = crate::models::HistoryEntry {
                id: entry.id.clone(),
                date: entry.date.clone(),
                words: 0,
                engine: entry.engine.clone(),
                text: String::new(),
                audio_path: entry.audio_path.clone(),
                evaluation: None,
                duration_ms: entry.duration_ms,
                source: entry.source.clone(),
                latency_ms: 0,
                throughput: 0.0,
                transcription_latency_ms: None,
                sanitizer_latency_ms: None,
                transcription_throughput: None,
                sanitizer_throughput: None,
                realtime_factor: None,
                deepgram_mode: history_deepgram_mode(state, deepgram_attempted),
                total_tokens: None,
                is_error: Some(true),
                error_message: Some(msg.clone()),
                debug_info: None,
            };
            crate::history::update_entry(failed.clone());
            if let Some(handle) = state.app_handle.read().as_ref() {
                use tauri::Emitter;
                let _ = handle.emit(crate::models::event_names::TRANSCRIPTION_SAVED, &failed);
            }
            msg
        };

    let bytes = match crate::audio_store::read(&audio_path) {
        Ok(b) => b,
        Err(e) => {
            let msg = format!("Não foi possível ler o áudio salvo: {}", e);
            return Err(fail(state, &entry, msg));
        }
    };

    let ext = std::path::Path::new(&audio_path)
        .extension()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "wav".to_string());
    let mime = crate::audio_store::mime_for_ext(&ext);

    let engine = state.active_engine();
    let dual_mode = *state.dual_engine.read();
    let start_time = std::time::Instant::now();

    log::info!(
        "audio: retrying transcription for {} with engine={:?}, dual_mode={}",
        id,
        engine,
        dual_mode
    );

    let (whisper_text, deepgram_text, effective_dual, deepgram_ran) = if dual_mode {
        match run_dual_posthoc(state, bytes.clone(), "audio.wav", mime).await {
            Ok((g, d, eff, dg_ran)) => (g, d, eff, dg_ran),
            Err(msg) => return Err(fail(state, &entry, msg)),
        }
    } else {
        match transcribe_bytes(state, bytes.clone(), "audio.wav", mime, engine).await {
            Ok(text) => {
                let (w, d, dg_ran) = single_engine_slots(engine, text);
                (w, d, false, dg_ran)
            }
            Err(err_msg) => return Err(fail(state, &entry, err_msg)),
        }
    };

    if whisper_text.trim().is_empty() && deepgram_text.trim().is_empty() {
        let msg = "Nenhum texto foi detectado no áudio durante a retentativa.".to_string();
        return Err(fail(state, &entry, msg));
    }

    let elapsed = start_time.elapsed().as_millis() as u64;

    // Reuse the shared finalize path (sanitizer + pick_raw + history update
    // is push-based; for retry we update in place after finalize would push
    // a new entry — so keep retry sanitizer logic here but aligned with pick_raw).
    let sanitizer_key = {
        let guard = state.api_keys.read();
        guard.groq.clone().filter(|k| !k.trim().is_empty())
    };
    let (
        model_id,
        supports_reasoning,
        system_prompt,
        custom_words,
        reasoning_enabled,
        reasoning_effort,
    ) = {
        let sanitizer = *state.sanitizer.read();
        (
            sanitizer.api_model_id(),
            sanitizer.supports_reasoning(),
            state.system_prompt.read().clone(),
            state.custom_words.read().clone(),
            *state.reasoning_enabled.read(),
            state.reasoning_effort.read().clone(),
        )
    };

    let system_prompt_to_use = if effective_dual {
        format!(
            "{}\n\n--- INSTRUÇÃO DE MOTOR DUPLO ---\nVocê recebeu duas transcrições acústicas brutas (Transcrição A e Transcrição B) do mesmo áudio. Compare-as, corrija falhas fonéticas, pontue de forma correta e mescle as informações de forma inteligente para produzir o melhor texto unificado.",
            system_prompt
        )
    } else {
        system_prompt
    };

    let raw_words = acoustic_word_count(&whisper_text, &deepgram_text);
    let mut debug_info: Option<crate::models::SanitizerDebug> = None;
    let start_sanitizer = std::time::Instant::now();
    let final_text = if !*state.sanitizer_enabled.read() {
        let picked = pick_raw_acoustic(&whisper_text, &deepgram_text);
        log::info!(
            "audio: sanitizer disabled on retry, using pick_raw ({} chars)",
            picked.len()
        );
        picked
    } else {
        match sanitizer_key {
            Some(key) => {
                let outcome = crate::groq::call_sanitizer_api(
                    &whisper_text,
                    &deepgram_text,
                    model_id,
                    &system_prompt_to_use,
                    &custom_words,
                    &key,
                    reasoning_enabled,
                    &reasoning_effort,
                    supports_reasoning,
                )
                .await;

                debug_info = Some(outcome.debug);
                match outcome.result {
                    Ok(sanitized) => {
                        if sanitized.trim() == crate::groq::FALLBACK_RETRY_SENTINEL {
                            pick_raw_acoustic(&whisper_text, &deepgram_text)
                        } else {
                            sanitized
                        }
                    }
                    Err(_) => pick_raw_acoustic(&whisper_text, &deepgram_text),
                }
            }
            None => pick_raw_acoustic(&whisper_text, &deepgram_text),
        }
    };
    let final_text = coalesce_empty_final(final_text, &whisper_text, &deepgram_text);
    let sanitizer_latency_ms = start_sanitizer.elapsed().as_millis() as u64;

    // Copy to clipboard & paste if source was "mic"
    let copy_to_clipboard = entry.source == "mic";
    if copy_to_clipboard {
        let clipboard_text = final_text.clone();
        let clipboard_fut = tokio::task::spawn_blocking(move || {
            let mut clipboard = arboard::Clipboard::new()?;
            clipboard.set_text(clipboard_text)?;
            Ok::<(), arboard::Error>(())
        });
        match tokio::time::timeout(std::time::Duration::from_secs(3), clipboard_fut).await {
            Ok(Ok(Ok(()))) => {
                let _ = paste_into_focused_field();
            }
            Ok(Ok(Err(e))) => log::error!("audio: retry clipboard set_text failed: {}", e),
            Ok(Err(e)) => log::error!("audio: retry clipboard task panicked: {}", e),
            Err(_) => log::error!("audio: retry clipboard access timed out after 3s"),
        }
    }

    let words = final_text.split_whitespace().count();
    let transcription_throughput = if elapsed > 0 {
        let est_tokens = raw_words as f64 * 1.3;
        Some((est_tokens * 1000.0) / elapsed as f64)
    } else {
        None
    };

    let sanitizer_throughput = if sanitizer_latency_ms > 0 {
        let est_tokens = words as f64 * 1.3;
        Some((est_tokens * 1000.0) / sanitizer_latency_ms as f64)
    } else {
        None
    };

    let total_tokens = Some((words as f64 * 1.3).round() as usize);
    let total_latency_ms = elapsed + sanitizer_latency_ms;
    let throughput = sanitizer_throughput.unwrap_or(0.0);
    let realtime_factor = compute_realtime_factor(elapsed, entry.duration_ms);
    let deepgram_mode = history_deepgram_mode(state, deepgram_ran);

    log::info!(
        "audio: retry latency telemetry transcription_ms={} sanitizer_ms={} total_ms={} \
         duration_ms={} rtf={:?} deepgram_mode={} dual={}",
        elapsed,
        sanitizer_latency_ms,
        total_latency_ms,
        entry.duration_ms,
        realtime_factor,
        deepgram_mode.as_deref().unwrap_or("-"),
        effective_dual
    );

    // Update history entry in storage
    let updated_entry = crate::models::HistoryEntry {
        id: entry.id.clone(),
        date: entry.date.clone(),
        words,
        engine: history_engine_label(engine, effective_dual, &whisper_text, &deepgram_text),
        text: final_text.clone(),
        audio_path: Some(audio_path),
        evaluation: None,
        duration_ms: entry.duration_ms,
        source: entry.source.clone(),
        latency_ms: total_latency_ms,
        throughput,
        transcription_latency_ms: Some(elapsed),
        sanitizer_latency_ms: Some(sanitizer_latency_ms),
        transcription_throughput,
        sanitizer_throughput,
        realtime_factor,
        deepgram_mode,
        total_tokens,
        is_error: Some(false),
        error_message: None,
        debug_info,
    };

    crate::history::update_entry(updated_entry.clone());

    if let Some(handle) = state.app_handle.read().as_ref() {
        use tauri::Emitter;
        let _ = handle.emit(
            crate::models::event_names::TRANSCRIPTION_SAVED,
            &updated_entry,
        );
    }

    Ok(final_text)
}

/// Simulates the OS paste shortcut (`Ctrl+V`, or `Cmd+V` on macOS) so the
/// freshly-copied transcription is inserted into whichever input field
/// currently holds focus — the search bar, text editor or chat box the user
/// was typing in when they fired the recording shortcut.
///
/// This is best-effort: if the keystroke simulation fails the text is still on
/// the clipboard, so the user can paste it manually. A short delay gives the
/// foreground application a moment to settle and ensures the clipboard write is
/// visible to it before the paste is dispatched.
fn paste_into_focused_field() -> Result<(), String> {
    use enigo::{
        Direction::{Click, Press, Release},
        Enigo, Key, Keyboard, Settings,
    };

    // Give the target window time to (re)claim focus and the clipboard write
    // time to propagate before we inject the paste keystroke.
    std::thread::sleep(std::time::Duration::from_millis(150));

    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| format!("failed to init input simulator: {}", e))?;

    #[cfg(target_os = "macos")]
    let modifier = Key::Meta;
    #[cfg(not(target_os = "macos"))]
    let modifier = Key::Control;

    enigo
        .key(modifier, Press)
        .map_err(|e| format!("failed to press modifier: {}", e))?;
    let press_v = enigo.key(Key::Unicode('v'), Click);
    // Always release the modifier, even if the 'v' press failed, so we never
    // leave Ctrl/Cmd stuck down for the user.
    let release = enigo.key(modifier, Release);

    press_v.map_err(|e| format!("failed to press v: {}", e))?;
    release.map_err(|e| format!("failed to release modifier: {}", e))?;

    log::info!("audio: pasted transcription into focused field");
    Ok(())
}

/// Reads a local audio file from disk and runs it through the full
/// transcription pipeline using the engine currently selected in settings.
/// Returns the final (sanitised) text or a human-readable error for the UI.
/// Unlike microphone capture, an upload does not hijack the clipboard.
pub async fn transcribe_file_path(state: &Arc<AppState>, path: String) -> Result<String, String> {
    // Signal the gadget that a file transcription is in progress.
    if let Some(handle) = state.app_handle.read().as_ref() {
        emit_transcribing(handle, true);
    }

    // Drop guard: always clears the transcribing state on exit.
    struct TranscribingGuard(Option<tauri::AppHandle>);
    impl Drop for TranscribingGuard {
        fn drop(&mut self) {
            if let Some(handle) = self.0.as_ref() {
                emit_transcribing(handle, false);
            }
        }
    }
    let _guard = TranscribingGuard(state.app_handle.read().clone());

    // Reject excessively large files before reading them into memory to
    // prevent OOM. 50 MB is well above any reasonable speech clip.
    const MAX_AUDIO_FILE_SIZE: u64 = 50 * 1024 * 1024;
    let metadata = std::fs::metadata(&path)
        .map_err(|e| format!("não foi possível acessar o arquivo: {}", e))?;
    if metadata.len() > MAX_AUDIO_FILE_SIZE {
        return Err(format!(
            "arquivo muito grande ({} MB). O tamanho máximo é {} MB.",
            metadata.len() / 1024 / 1024,
            MAX_AUDIO_FILE_SIZE / 1024 / 1024
        ));
    }

    let bytes =
        std::fs::read(&path).map_err(|e| format!("não foi possível ler o arquivo: {}", e))?;
    if bytes.is_empty() {
        return Err("o arquivo de áudio está vazio".to_string());
    }

    let p = std::path::Path::new(&path);
    let file_name = p
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "upload.wav".to_string());
    let ext = p
        .extension()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "wav".to_string());
    let mime = crate::audio_store::mime_for_ext(&ext);

    let engine = state.active_engine();
    log::info!(
        "audio: upload '{}' ({}), engine {:?}",
        file_name,
        mime,
        engine
    );

    let start_time = std::time::Instant::now();
    let id = chrono_like_id();
    let audio_path = crate::audio_store::save(&id, &ext, &bytes);

    let dual_mode = *state.dual_engine.read();

    // Dual mode on file uploads must run both STTs for real — not a single
    // engine with a duplicated string into the sanitizer.
    let (whisper_text, deepgram_text, effective_dual, deepgram_ran) = if dual_mode {
        match run_dual_posthoc(state, bytes, &file_name, mime).await {
            Ok(v) => v,
            Err(err_msg) => {
                let full_msg = format!(
                    "Falha na transcrição do arquivo {:?}: {}",
                    file_name, err_msg
                );
                save_failed_transcription(
                    state,
                    id,
                    audio_path,
                    0,
                    "file",
                    engine,
                    full_msg.clone(),
                );
                return Err(full_msg);
            }
        }
    } else {
        match transcribe_bytes(state, bytes, &file_name, mime, engine).await {
            Ok(text) => {
                let (w, d, dg_ran) = single_engine_slots(engine, text);
                (w, d, false, dg_ran)
            }
            Err(err_msg) => {
                let full_msg = format!(
                    "Falha na transcrição do arquivo {:?}: {}",
                    file_name, err_msg
                );
                save_failed_transcription(
                    state,
                    id,
                    audio_path,
                    0,
                    "file",
                    engine,
                    full_msg.clone(),
                );
                return Err(full_msg);
            }
        }
    };

    if whisper_text.trim().is_empty() && deepgram_text.trim().is_empty() {
        let err_msg = "Nenhum texto detectado no arquivo de áudio.".to_string();
        save_failed_transcription(state, id, audio_path, 0, "file", engine, err_msg.clone());
        return Err(err_msg);
    }

    let elapsed = start_time.elapsed().as_millis() as u64;

    // File uploads carry no reliable duration (arbitrary container/codec), so
    // it is recorded as 0; the source is tagged as a file rather than the mic.
    finalize_transcription(
        state,
        id,
        audio_path,
        engine,
        whisper_text,
        deepgram_text,
        false,
        0,
        "file",
        elapsed,
        effective_dual,
        deepgram_ran,
    )
    .await
    .ok_or_else(|| "não foi possível finalizar a transcrição".to_string())
}

/// Produces a reasonably-unique id string from the current system time
/// (UTC milliseconds). Avoids pulling in a full UUID crate for a value
/// that only needs to be unique within a single user's history file.
fn chrono_like_id() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", dur.as_millis())
}

/// Returns the current local time formatted as `YYYY-MM-DD HH:MM`.
fn now_timestamp() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs() as i64;
    let days = secs / 86400;
    let secs_of_day = secs % 86400;
    let h = secs_of_day / 3600;
    let m = (secs_of_day % 3600) / 60;
    // Civil-from-days algorithm (Howard Hinnant). Good enough for a label.
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02} {:02}:{:02}", year, month, d, h, m)
}

/// Hard-stops the active stream and discards every accumulated sample
/// without building a WAV buffer. Used by the panic shortcut.
pub fn cancel_capture(state: &Arc<AppState>) {
    let _ = state.drop_audio_stream();
    if let Some(session) = state.deepgram_live.lock().take() {
        session.abort();
        log::info!("audio: deepgram live session aborted on cancel");
    }
    state.clear_audio_buffer();
    log::info!("audio: capture cancelled, buffers released");
}

/// Creates a temporary cpal::Stream that reads from the configured input
/// device, calculates RMS level, and emits throttled `"mic-test-level"` events.
///
/// Levels are **coalesced to ≤ ~12.5 Hz** (same cadence as the live gadget
/// waveform). Emitting on every WASAPI buffer (~10 ms) without throttle was
/// saturating the main WebView IPC/React path while "Testar microfone" ran.
pub fn start_mic_test_stream(app: &tauri::AppHandle, state: &Arc<AppState>) -> Result<(), String> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::StreamConfig;
    use std::sync::atomic::{AtomicU32, AtomicU64, Ordering as AtomicOrdering};

    let host = cpal::default_host();
    let configured_device = crate::settings::load_input_device();

    let device = if let Some(ref name) = configured_device {
        let mut found = None;
        if let Ok(mut devices) = host.input_devices() {
            if let Some(d) = devices.find(|d| d.name().map(|n| &n == name).unwrap_or(false)) {
                found = Some(d);
            }
        }
        found.or_else(|| host.default_input_device())
    } else {
        host.default_input_device()
    };

    let device = device.ok_or_else(|| "Nenhum dispositivo de entrada encontrado".to_string())?;

    log::info!(
        "audio test: opening input device {:?}",
        device.name().unwrap_or_else(|_| "<unknown>".into())
    );

    let supported = device
        .default_input_config()
        .map_err(|e| format!("Falha ao abrir dispositivo: {}", e))?;

    let sample_format = supported.sample_format();
    let config: StreamConfig = supported.into();

    // Shared throttle state across format-specific callbacks.
    let last_emit_ms = Arc::new(AtomicU64::new(0));
    let last_level_bits = Arc::new(AtomicU32::new(0f32.to_bits()));

    let make_emit = |app: tauri::AppHandle| {
        let last_emit_ms = last_emit_ms.clone();
        let last_level_bits = last_level_bits.clone();
        move |level: f32| {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let prev_ms = last_emit_ms.load(AtomicOrdering::Relaxed);
            let elapsed = now_ms.saturating_sub(prev_ms);
            // Hard cap: at most one emit every 80 ms.
            if elapsed < 80 {
                return;
            }
            let prev_level = f32::from_bits(last_level_bits.load(AtomicOrdering::Relaxed));
            // Skip tiny level noise unless enough time passed for a keep-alive.
            if (level - prev_level).abs() < 0.015 && elapsed < 200 {
                return;
            }
            last_emit_ms.store(now_ms, AtomicOrdering::Relaxed);
            last_level_bits.store(level.to_bits(), AtomicOrdering::Relaxed);
            use tauri::{Emitter, Manager};
            // Prefer the main settings window; fall back to broadcast.
            if let Some(main) = app.get_webview_window("main") {
                let _ = main.emit("mic-test-level", level);
            } else {
                let _ = app.emit("mic-test-level", level);
            }
        }
    };

    let err_callback = |err: cpal::StreamError| {
        log::error!("audio test: stream error: {}", err);
    };

    let stream = match sample_format {
        cpal::SampleFormat::I16 => {
            let emit = make_emit(app.clone());
            device.build_input_stream(
                &config,
                move |samples: &[i16], _: &cpal::InputCallbackInfo| {
                    if samples.is_empty() {
                        return;
                    }
                    let sum_sq: f64 = samples
                        .iter()
                        .map(|&s| {
                            let f = s as f64 / 32768.0;
                            f * f
                        })
                        .sum();
                    let rms = (sum_sq / samples.len() as f64).sqrt();
                    let level = ((rms * 3.2) as f32).clamp(0.0, 1.0);
                    emit(level);
                },
                err_callback,
                None,
            )
        }
        cpal::SampleFormat::F32 => {
            let emit = make_emit(app.clone());
            device.build_input_stream(
                &config,
                move |samples: &[f32], _: &cpal::InputCallbackInfo| {
                    if samples.is_empty() {
                        return;
                    }
                    let sum_sq: f64 = samples.iter().map(|&s| s as f64 * s as f64).sum();
                    let rms = (sum_sq / samples.len() as f64).sqrt();
                    let level = ((rms * 3.2) as f32).clamp(0.0, 1.0);
                    emit(level);
                },
                err_callback,
                None,
            )
        }
        cpal::SampleFormat::U16 => {
            let emit = make_emit(app.clone());
            device.build_input_stream(
                &config,
                move |samples: &[u16], _: &cpal::InputCallbackInfo| {
                    if samples.is_empty() {
                        return;
                    }
                    let sum_sq: f64 = samples
                        .iter()
                        .map(|&s| {
                            let f = (s as f64 - 32768.0) / 32768.0;
                            f * f
                        })
                        .sum();
                    let rms = (sum_sq / samples.len() as f64).sqrt();
                    let level = ((rms * 3.2) as f32).clamp(0.0, 1.0);
                    emit(level);
                },
                err_callback,
                None,
            )
        }
        other => {
            return Err(format!(
                "Formato de áudio não suportado pelo dispositivo: {:?}",
                other
            ))
        }
    }
    .map_err(|e| format!("Falha ao criar stream de teste de microfone: {}", e))?;

    stream
        .play()
        .map_err(|e| format!("Falha ao iniciar stream de teste de microfone: {}", e))?;
    *state.test_stream.lock() = Some(stream);

    Ok(())
}
