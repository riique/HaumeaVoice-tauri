use crate::audio::{cancel_capture, start_capture, stop_capture};
use crate::models::{event_names, RecordingPhase, RecordingToggle, SharedState, ShortcutConfig};
use parking_lot::Mutex;
use std::{fs, path::PathBuf, sync::OnceLock};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

/// Default shortcut strings, mirrored by [`ShortcutConfig::default`]. Parsed by
/// the `global-hotkey` crate via `TryFrom<&str>` for `Shortcut`.
pub const SHORTCUT_TOGGLE: &str = "Control+B";
pub const SHORTCUT_CANCEL: &str = "Control+Q";

static SHORTCUTS_PATH: OnceLock<PathBuf> = OnceLock::new();
static SHORTCUTS_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

/// Errors that may occur while (un)registering global shortcuts.
#[derive(Debug, thiserror::Error)]
pub enum ShortcutError {
    #[error("failed to register shortcut {shortcut}: {source}")]
    Register {
        shortcut: String,
        #[source]
        source: tauri_plugin_global_shortcut::Error,
    },
    #[error("failed to unregister all shortcuts: {0}")]
    UnregisterAll(tauri_plugin_global_shortcut::Error),
}

/* ----------------------------- Persistence ----------------------------- */

/// Called once during setup with the resolved `shortcuts.json` path.
pub fn init_store(file: PathBuf) {
    let _ = SHORTCUTS_PATH.set(file);
    let _ = SHORTCUTS_LOCK.set(Mutex::new(()));
}

/// Loads the persisted shortcut config, falling back to the defaults when the
/// file is missing or unparsable.
pub fn load_store() -> ShortcutConfig {
    let Some(lock) = SHORTCUTS_LOCK.get() else {
        return ShortcutConfig::default();
    };
    let _guard = lock.lock();
    let Some(file) = SHORTCUTS_PATH.get() else {
        return ShortcutConfig::default();
    };
    match fs::read_to_string(file) {
        Ok(contents) if !contents.trim().is_empty() => {
            serde_json::from_str(&contents).unwrap_or_default()
        }
        _ => ShortcutConfig::default(),
    }
}

/// Persists `cfg` to disk. Failures are logged, not propagated.
pub fn save_store(cfg: &ShortcutConfig) {
    let Some(lock) = SHORTCUTS_LOCK.get() else {
        return;
    };
    let _guard = lock.lock();
    let Some(file) = SHORTCUTS_PATH.get() else {
        return;
    };
    if let Some(parent) = file.parent() {
        let _ = fs::create_dir_all(parent);
    }
    match serde_json::to_string_pretty(cfg) {
        Ok(json) => {
            if let Err(e) = fs::write(file, json) {
                log::error!("shortcuts: failed to write file: {}", e);
            }
        }
        Err(e) => log::error!("shortcuts: failed to serialize: {}", e),
    }
}

/// Core toggle logic shared by the IPC command path and the global
/// `<Ctrl+B>` shortcut handler.
///
/// Returns the requested recording flag immediately so callers on the main
/// thread (the global-shortcut callback) are never blocked. The
/// `recording-started` event is emitted only after CPAL confirms that the input
/// stream is running, so the UI is a reliable cue that speech is being captured.
///
/// **All blocking work** â€” COM microphone unmute, CPAL device acquisition,
/// stream playback â€” is dispatched to a background `std::thread::spawn` so
/// the Tauri event loop stays responsive. If the background work fails,
/// the recording state is reverted and a `recording-cancelled` event is
/// emitted.
pub fn handle_toggle(app: &AppHandle, state: &SharedState) -> bool {
    match state.toggle_recording_lifecycle() {
        RecordingToggle::Start(status) => {
            log::info!(
                "shortcut: start recording generation={} session={}",
                status.generation,
                status.session_id.as_deref().unwrap_or("-")
            );
            // Seed the session from the foreground application's monitor. A saved
            // display still wins; cursor and primary monitor are only fallbacks.
            crate::begin_gadget_session(app, state);
            if let Err(e) = app.emit(event_names::RECORDING_INITIALIZING, &status) {
                log::warn!(
                    "failed to emit {}: {}",
                    event_names::RECORDING_INITIALIZING,
                    e
                );
            }
            // Heavy work (COM unmute + CPAL device open) runs off the main
            // thread to prevent AppHang when drivers stall.
            let app_bg = app.clone();
            let state_bg = state.clone();
            let generation = status.generation;
            let session_id = status.session_id.clone().unwrap_or_default();
            std::thread::spawn(move || {
                let start_requested_at = std::time::Instant::now();

                // Opening the CPAL stream and asking Windows to unmute the input
                // are independent. Run them concurrently so a muted endpoint does
                // not add a fixed delay before capture can become ready.
                let (capture_result, was_muted) = std::thread::scope(|scope| {
                    let unmute = scope.spawn(crate::mic_control::ensure_mic_unmuted);
                    let capture_result = start_capture(&state_bg, generation, &session_id);
                    let was_muted = unmute.join().unwrap_or(false);
                    (capture_result, was_muted)
                });

                match capture_result {
                    Ok(()) => {
                        let Some((accepted, ready_status)) =
                            state_bg.recording_capture_ready(generation)
                        else {
                            log::warn!(
                                "shortcut: capture ready for stale generation={}",
                                generation
                            );
                            return;
                        };
                        if !accepted {
                            log::info!(
                                "shortcut: capture became ready after stop/cancel generation={}; stop path owns cleanup",
                                generation
                            );
                            return;
                        }

                        state_bg.mark_recording_start();
                        if let Err(e) = app_bg.emit(event_names::RECORDING_STARTED, &ready_status) {
                            log::warn!("failed to emit {}: {}", event_names::RECORDING_STARTED, e);
                        }
                        crate::audio::spawn_audio_level_emitter(app_bg.clone(), state_bg.clone());
                        log::info!(
                            "shortcut: live audio-level emitter started for gadget waveform"
                        );
                        log::info!(
                            "shortcut: capture ready in {}ms (mic_was_muted={})",
                            start_requested_at.elapsed().as_millis(),
                            was_muted
                        );
                    }
                    Err(e) => {
                        let superseded = matches!(e, crate::audio::AudioError::Superseded);
                        if superseded {
                            log::info!(
                                "shortcut: capture start superseded generation={}",
                                generation
                            );
                        } else {
                            log::error!("shortcut: failed to start capture: {}", e);
                        }
                        state_bg.clear_recording_start();
                        let failed_status = state_bg.recording_capture_failed(generation);
                        if let Some(failed_status) = failed_status
                            .filter(|status| status.phase == RecordingPhase::Idle && !superseded)
                        {
                            if let Err(ee) =
                                app_bg.emit(event_names::RECORDING_CANCELLED, &failed_status)
                            {
                                log::warn!(
                                    "failed to emit {}: {}",
                                    event_names::RECORDING_CANCELLED,
                                    ee
                                );
                            }
                        }
                    }
                }
            });
            true
        }
        RecordingToggle::Stop(status) => {
            log::info!(
                "shortcut: stop recording generation={} session={}",
                status.generation,
                status.session_id.as_deref().unwrap_or("-")
            );
            state.clear_recording_start();
            if let Err(e) = app.emit(event_names::RECORDING_STOPPED, &status) {
                log::warn!("failed to emit {}: {}", event_names::RECORDING_STOPPED, e);
            }
            let state_clone = state.clone();
            let app_clone = app.clone();
            let generation = status.generation;
            tauri::async_runtime::spawn(async move {
                while state_clone.recording_capture_start_pending(generation) {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                let _ = stop_capture(&state_clone).await;
                let finished = state_clone.finish_recording_stop(generation);
                if let Err(error) = app_clone.emit(event_names::RECORDING_IDLE, &finished) {
                    log::warn!("failed to emit {}: {}", event_names::RECORDING_IDLE, error);
                }
                log::info!(
                    "shortcut: recording lifecycle finished generation={} revision={} phase={:?}",
                    finished.generation,
                    finished.revision,
                    finished.phase
                );
            });
            false
        }
        RecordingToggle::Busy(status) => {
            log::warn!(
                "shortcut: toggle ignored while lifecycle is busy generation={} revision={} phase={:?}",
                status.generation,
                status.revision,
                status.phase
            );
            status.recording
        }
    }
}

/// Core cancel logic shared by the IPC command path and the global
/// `<Ctrl+Q>` panic shortcut handler. Forces the recording flag to
/// false, tears down the stream, discards the buffer without
/// producing any WAV output and emits the `recording-cancelled`
/// event so the UI resets the timer to 00:00.
pub fn handle_cancel(app: &AppHandle, state: &SharedState) {
    let status = state.request_recording_cancel();
    if status.phase != RecordingPhase::Cancelling {
        log::info!(
            "shortcut: cancel ignored generation={} phase={:?}",
            status.generation,
            status.phase
        );
        return;
    }
    state.clear_recording_start();

    cancel_capture(state);

    log::info!(
        "shortcut: cancel recording generation={} session={}",
        status.generation,
        status.session_id.as_deref().unwrap_or("-")
    );

    if let Err(e) = app.emit(event_names::RECORDING_CANCELLED, &status) {
        log::warn!("failed to emit {}: {}", event_names::RECORDING_CANCELLED, e);
    }
    let idle = state.finish_recording_cancel(status.generation);
    if idle.phase == RecordingPhase::Idle {
        if let Err(error) = app.emit(event_names::RECORDING_IDLE, &idle) {
            log::warn!("failed to emit {}: {}", event_names::RECORDING_IDLE, error);
        }
    }
}

/// Registers the toggle and cancel global shortcuts with the given key
/// combinations. The handler closures clone the `SharedState` handle so they
/// can read/flip the recording flag and drive the audio pipeline without the
/// `State<'_>` injection (which is only valid inside a `#[tauri::command]`).
///
/// An invalid or already-claimed combination surfaces as
/// [`ShortcutError::Register`], which the caller uses to validate a rebind.
pub fn register_all(app: &AppHandle, toggle: &str, cancel: &str) -> Result<(), ShortcutError> {
    app.global_shortcut()
        .on_shortcut(toggle, move |app_h, _sc, event| {
            // The callback fires on both key-down and key-up. Only react to
            // the key-down transition to avoid double-toggling.
            if event.state == ShortcutState::Pressed {
                let s = app_h.state::<SharedState>().inner().clone();
                handle_toggle(app_h, &s);
            }
        })
        .map_err(|e| ShortcutError::Register {
            shortcut: toggle.to_string(),
            source: e,
        })?;

    app.global_shortcut()
        .on_shortcut(cancel, move |app_h, _sc, event| {
            if event.state == ShortcutState::Pressed {
                let s = app_h.state::<SharedState>().inner().clone();
                handle_cancel(app_h, &s);
            }
        })
        .map_err(|e| ShortcutError::Register {
            shortcut: cancel.to_string(),
            source: e,
        })?;

    log::info!("global shortcuts registered: {} and {}", toggle, cancel);
    Ok(())
}

/// Rebinds the recording shortcuts at runtime: clears all current
/// registrations, registers the requested combinations, and on success updates
/// the in-memory state and persists to disk. If the new combination is invalid
/// or unavailable, the previous binding is restored and an error is returned so
/// the user keeps a working shortcut.
pub fn apply_new(
    app: &AppHandle,
    state: &SharedState,
    toggle: String,
    cancel: String,
) -> Result<ShortcutConfig, String> {
    let toggle = toggle.trim().to_string();
    let cancel = cancel.trim().to_string();

    if toggle.is_empty() || cancel.is_empty() {
        return Err("os atalhos não podem ficar vazios".to_string());
    }
    if toggle.eq_ignore_ascii_case(&cancel) {
        return Err("os atalhos de iniciar e cancelar devem ser diferentes".to_string());
    }

    // Start from a clean slate so the previous combinations are released.
    let _ = app.global_shortcut().unregister_all();

    match register_all(app, &toggle, &cancel) {
        Ok(()) => {
            let cfg = ShortcutConfig { toggle, cancel };
            *state.shortcuts.write() = cfg.clone();
            save_store(&cfg);
            log::info!("shortcuts: rebound to {} / {}", cfg.toggle, cfg.cancel);
            Ok(cfg)
        }
        Err(e) => {
            // Restore the previous, known-good binding.
            let prev = state.shortcuts.read().clone();
            let _ = app.global_shortcut().unregister_all();
            let _ = register_all(app, &prev.toggle, &prev.cancel);
            Err(format!(
                "não foi possível registrar esse atalho (pode estar em uso por outro app): {}",
                e
            ))
        }
    }
}
