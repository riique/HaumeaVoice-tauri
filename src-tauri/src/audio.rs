//! Audio capture, WAV encoding, clipboard delivery and history I/O.
//!
//! Transcription engine selection, dual STT, sanitization and metrics live in
//! [`crate::transcription`]. Capture still happens in RAM until the final WAV
//! is assembled and optionally persisted via [`crate::audio_store`].

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
                        // Never discard a capture block. The stream is stopped
                        // before the buffer is drained, and the level meter only
                        // holds this lock long enough to copy a small window.
                        st.audio_buffer.lock().extend_from_slice(&mono);
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
                        st.audio_buffer.lock().extend_from_slice(&mono);
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
        log::warn!(
            "audio: streaming_final selected but Deepgram API key missing; live session skipped"
        );
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

/// Stops the active capture stream, builds WAV, runs the legacy transcription
/// pipeline ([`crate::transcription`]), then clipboard + history.
pub async fn stop_capture(state: &Arc<AppState>) -> Option<String> {
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

    stop_capture_inner(state).await
}

async fn stop_capture_inner(state: &Arc<AppState>) -> Option<String> {
    let _ = state.drop_audio_stream();

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
    let duration_ms = (raw_count as u64 * 1000) / capture_rate as u64;

    let samples = if capture_rate == TARGET_SAMPLE_RATE {
        raw_samples
    } else {
        resample(&raw_samples, capture_rate, TARGET_SAMPLE_RATE)
    };

    let wav = create_wav_buffer(&samples);
    log::info!(
        "audio: WAV buffer generated in RAM ({} samples @ {} Hz -> {} samples @ {} Hz, {} KB)",
        raw_count,
        capture_rate,
        samples.len(),
        TARGET_SAMPLE_RATE,
        wav.len() / 1024
    );

    let engine = state.active_engine();
    let start_time = std::time::Instant::now();
    let id = chrono_like_id();
    let audio_path = crate::audio_store::save(&id, "wav", &wav);

    // Product modes (UltraFast / FastAccurate) — abort unused Deepgram live session.
    if crate::transcription::should_use_product_mode(state) {
        if let Some(session) = live_session {
            session.abort();
        }
        let mode = *state.transcription_mode.read();
        match crate::transcription::run_product_mode_with_duration(
            state,
            wav,
            "audio.wav",
            "audio/wav",
            "wav",
            Some(duration_ms),
        )
        .await
        {
            Ok(result) => {
                let _ = start_time.elapsed();
                let final_text = result.final_text.clone();
                deliver_clipboard_and_paste(&final_text).await;
                let entry = crate::transcription::mode_result_to_history(
                    id,
                    now_timestamp(),
                    audio_path,
                    duration_ms,
                    "mic",
                    &result,
                );
                crate::history::push(entry.clone());
                crate::transcription::emit_saved(state, &entry);
                log::debug!("audio: mode output ({} chars)", final_text.len());
                return Some(final_text);
            }
            Err(err_msg) => {
                let entry = crate::transcription::mode_failed_history(
                    id,
                    now_timestamp(),
                    audio_path,
                    duration_ms,
                    "mic",
                    mode,
                    err_msg,
                );
                crate::history::push(entry.clone());
                crate::transcription::emit_saved(state, &entry);
                return None;
            }
        }
    }

    // Legacy engine / dual / sanitizer path.
    let acoustic = match crate::transcription::run_acoustic_mic(state, wav, live_session).await {
        Ok(a) => a,
        Err(err_msg) => {
            let entry = crate::transcription::build_failed_entry(
                state,
                id,
                now_timestamp(),
                audio_path,
                duration_ms,
                "mic",
                engine,
                err_msg,
            );
            crate::history::push(entry.clone());
            crate::transcription::emit_saved(state, &entry);
            return None;
        }
    };

    if acoustic.whisper_text.trim().is_empty() && acoustic.deepgram_text.trim().is_empty() {
        let entry = crate::transcription::build_failed_entry(
            state,
            id,
            now_timestamp(),
            audio_path,
            duration_ms,
            "mic",
            engine,
            "Nenhum texto detectado na gravação.".to_string(),
        );
        crate::history::push(entry.clone());
        crate::transcription::emit_saved(state, &entry);
        return None;
    }

    let elapsed = start_time.elapsed().as_millis() as u64;
    let sanitize = crate::transcription::run_sanitize(
        state,
        &acoustic.whisper_text,
        &acoustic.deepgram_text,
        acoustic.effective_dual,
    )
    .await;

    let final_text = sanitize.final_text.clone();
    deliver_clipboard_and_paste(&final_text).await;

    let entry = crate::transcription::build_success_entry(
        state,
        id,
        now_timestamp(),
        audio_path,
        engine,
        &acoustic.whisper_text,
        &acoustic.deepgram_text,
        final_text.clone(),
        duration_ms,
        "mic",
        elapsed,
        acoustic.effective_dual,
        acoustic.deepgram_ran,
        &sanitize,
        "mic",
    );
    crate::history::push(entry.clone());
    crate::transcription::emit_saved(state, &entry);

    log::debug!("audio: final output ({} chars)", final_text.len());
    Some(final_text)
}

/// Linear resampler: mono i16 from `from_rate` to `to_rate`.
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

async fn deliver_clipboard_and_paste(final_text: &str) {
    let clipboard_text = final_text.to_string();
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
            if let Err(e) = paste_into_focused_field() {
                log::warn!("audio: auto-paste failed (text still on clipboard): {}", e);
            }
        }
        Ok(Ok(Err(e))) => log::error!("audio: failed to set clipboard text: {}", e),
        Ok(Err(e)) => log::error!("audio: clipboard task panicked: {}", e),
        Err(_) => log::error!("audio: clipboard access timed out after 3s"),
    }
}

/// Retries a failed transcription from history using the legacy pipeline.
pub async fn retry_transcription_handler(
    state: &Arc<AppState>,
    id: &str,
) -> Result<String, String> {
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

    let fail =
        |state: &Arc<AppState>, entry: &crate::models::HistoryEntry, msg: String| -> String {
            let failed = crate::transcription::update_failed_entry(state, entry, msg.clone());
            crate::history::update_entry(failed.clone());
            crate::transcription::emit_saved(state, &failed);
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
    let start_time = std::time::Instant::now();

    log::info!(
        "audio: retrying transcription for {} modes={} engine={:?} dual={}",
        id,
        crate::transcription::should_use_product_mode(state),
        engine,
        *state.dual_engine.read()
    );

    if crate::transcription::should_use_product_mode(state) {
        let mode = *state.transcription_mode.read();
        match crate::transcription::run_product_mode_with_duration(
            state,
            bytes,
            "audio.wav",
            mime,
            &ext,
            None,
        )
        .await
        {
            Ok(result) => {
                let final_text = result.final_text.clone();
                if entry.source == "mic" {
                    deliver_clipboard_and_paste(&final_text).await;
                }
                let updated = crate::transcription::mode_result_to_history(
                    entry.id.clone(),
                    entry.date.clone(),
                    Some(audio_path),
                    entry.duration_ms,
                    &entry.source,
                    &result,
                );
                crate::history::update_entry(updated.clone());
                crate::transcription::emit_saved(state, &updated);
                return Ok(final_text);
            }
            Err(msg) => {
                let failed = crate::transcription::mode_failed_history(
                    entry.id.clone(),
                    entry.date.clone(),
                    entry.audio_path.clone(),
                    entry.duration_ms,
                    &entry.source,
                    mode,
                    msg.clone(),
                );
                crate::history::update_entry(failed.clone());
                crate::transcription::emit_saved(state, &failed);
                return Err(msg);
            }
        }
    }

    let acoustic =
        match crate::transcription::run_acoustic_file(state, bytes, "audio.wav", mime).await {
            Ok(a) => a,
            Err(msg) => return Err(fail(state, &entry, msg)),
        };

    if acoustic.whisper_text.trim().is_empty() && acoustic.deepgram_text.trim().is_empty() {
        let msg = "Nenhum texto foi detectado no áudio durante a retentativa.".to_string();
        return Err(fail(state, &entry, msg));
    }

    let elapsed = start_time.elapsed().as_millis() as u64;
    let sanitize = crate::transcription::run_sanitize(
        state,
        &acoustic.whisper_text,
        &acoustic.deepgram_text,
        acoustic.effective_dual,
    )
    .await;

    let final_text = sanitize.final_text.clone();
    if entry.source == "mic" {
        deliver_clipboard_and_paste(&final_text).await;
    }

    let updated_entry = crate::transcription::build_success_entry(
        state,
        entry.id.clone(),
        entry.date.clone(),
        Some(audio_path),
        engine,
        &acoustic.whisper_text,
        &acoustic.deepgram_text,
        final_text.clone(),
        entry.duration_ms,
        &entry.source,
        elapsed,
        acoustic.effective_dual,
        acoustic.deepgram_ran,
        &sanitize,
        "retry",
    );

    crate::history::update_entry(updated_entry.clone());
    crate::transcription::emit_saved(state, &updated_entry);

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

/// Reads a local audio file and runs the legacy transcription pipeline.
/// Unlike microphone capture, an upload does not hijack the clipboard.
pub async fn transcribe_file_path(state: &Arc<AppState>, path: String) -> Result<String, String> {
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
        "audio: upload '{}' ({}), modes={} engine {:?}",
        file_name,
        mime,
        crate::transcription::should_use_product_mode(state),
        engine
    );

    let start_time = std::time::Instant::now();
    let id = chrono_like_id();
    let audio_path = crate::audio_store::save(&id, &ext, &bytes);

    if crate::transcription::should_use_product_mode(state) {
        let mode = *state.transcription_mode.read();
        match crate::transcription::run_product_mode_with_duration(
            state, bytes, &file_name, mime, &ext, None,
        )
        .await
        {
            Ok(result) => {
                let final_text = result.final_text.clone();
                let entry = crate::transcription::mode_result_to_history(
                    id,
                    now_timestamp(),
                    audio_path,
                    0,
                    "file",
                    &result,
                );
                crate::history::push(entry.clone());
                crate::transcription::emit_saved(state, &entry);
                return Ok(final_text);
            }
            Err(err_msg) => {
                let full_msg = format!(
                    "Falha na transcrição do arquivo {:?}: {}",
                    file_name, err_msg
                );
                let entry = crate::transcription::mode_failed_history(
                    id,
                    now_timestamp(),
                    audio_path,
                    0,
                    "file",
                    mode,
                    full_msg.clone(),
                );
                crate::history::push(entry.clone());
                crate::transcription::emit_saved(state, &entry);
                return Err(full_msg);
            }
        }
    }

    let acoustic =
        match crate::transcription::run_acoustic_file(state, bytes, &file_name, mime).await {
            Ok(a) => a,
            Err(err_msg) => {
                let full_msg = format!(
                    "Falha na transcrição do arquivo {:?}: {}",
                    file_name, err_msg
                );
                let entry = crate::transcription::build_failed_entry(
                    state,
                    id,
                    now_timestamp(),
                    audio_path,
                    0,
                    "file",
                    engine,
                    full_msg.clone(),
                );
                crate::history::push(entry.clone());
                crate::transcription::emit_saved(state, &entry);
                return Err(full_msg);
            }
        };

    if acoustic.whisper_text.trim().is_empty() && acoustic.deepgram_text.trim().is_empty() {
        let err_msg = "Nenhum texto detectado no arquivo de áudio.".to_string();
        let entry = crate::transcription::build_failed_entry(
            state,
            id,
            now_timestamp(),
            audio_path,
            0,
            "file",
            engine,
            err_msg.clone(),
        );
        crate::history::push(entry.clone());
        crate::transcription::emit_saved(state, &entry);
        return Err(err_msg);
    }

    let elapsed = start_time.elapsed().as_millis() as u64;
    let sanitize = crate::transcription::run_sanitize(
        state,
        &acoustic.whisper_text,
        &acoustic.deepgram_text,
        acoustic.effective_dual,
    )
    .await;

    let final_text = sanitize.final_text.clone();
    let entry = crate::transcription::build_success_entry(
        state,
        id,
        now_timestamp(),
        audio_path,
        engine,
        &acoustic.whisper_text,
        &acoustic.deepgram_text,
        final_text.clone(),
        0,
        "file",
        elapsed,
        acoustic.effective_dual,
        acoustic.deepgram_ran,
        &sanitize,
        "file",
    );
    crate::history::push(entry.clone());
    crate::transcription::emit_saved(state, &entry);

    Ok(final_text)
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
/// Local wall-clock label `YYYY-MM-DD HH:MM` (not UTC).
fn now_timestamp() -> String {
    #[cfg(windows)]
    {
        use windows::Win32::System::SystemInformation::GetLocalTime;
        // windows 0.58: GetLocalTime() -> SYSTEMTIME
        let st = unsafe { GetLocalTime() };
        return format!(
            "{:04}-{:02}-{:02} {:02}:{:02}",
            st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute
        );
    }
    #[cfg(not(windows))]
    {
        // Fallback: UTC civil date (non-Windows builds are secondary).
        let dur = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let secs = dur.as_secs() as i64;
        let days = secs / 86400;
        let secs_of_day = secs % 86400;
        let h = secs_of_day / 3600;
        let m = (secs_of_day % 3600) / 60;
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
