use crate::models::{
    ApiKeysPayload, DeepgramMode, EngineConfigPayload, SharedState, TranscriptionEngine,
};
use serde::Serialize;
use tauri::State;

/// Error type returned by every IPC command. Serializes to a plain
/// string so the TypeScript frontend receives a readable message
/// inside `invoke`'s rejected promise.
#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error("invalid configuration payload: {0}")]
    InvalidPayload(String),
    #[error("recording already in progress")]
    AlreadyRecording,
    #[error("no recording in progress")]
    NotRecording,
    #[error("internal state error: {0}")]
    Internal(String),
}

impl Serialize for CommandError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.to_string().as_ref())
    }
}

/// Snapshot returned to the frontend after a configuration update so
/// the UI can confirm the persisted selection.
#[derive(Debug, Clone, Serialize)]
pub struct EngineConfigSnapshot {
    pub engine: TranscriptionEngine,
    pub sanitizer: crate::models::SanitizerModel,
    pub dual_engine: bool,
    pub reasoning_enabled: bool,
    pub reasoning_effort: String,
    pub deepgram_mode: DeepgramMode,
}

/// `update_engine_config`
///
/// Receives the manual engine and sanitizer selections from the
/// frontend and persists them into the global `AppState` under their
/// respective `RwLock` guards, as well as on-disk in settings.json.
#[tauri::command]
pub async fn update_engine_config(
    state: State<'_, SharedState>,
    payload: EngineConfigPayload,
) -> Result<EngineConfigSnapshot, CommandError> {
    log::info!(
        "update_engine_config: engine={:?} sanitizer={:?} dual_engine={} reasoning={} effort={} deepgram_mode={}",
        payload.engine,
        payload.sanitizer,
        payload.dual_engine,
        payload.reasoning_enabled,
        payload.reasoning_effort,
        payload.deepgram_mode.as_str()
    );

    let shared = state.inner().clone();
    tokio::task::spawn_blocking(move || {
        {
            *shared.engine.write() = payload.engine;
        }
        {
            *shared.sanitizer.write() = payload.sanitizer;
        }
        {
            *shared.dual_engine.write() = payload.dual_engine;
        }
        {
            *shared.reasoning_enabled.write() = payload.reasoning_enabled;
        }
        {
            *shared.reasoning_effort.write() = payload.reasoning_effort.clone();
        }
        {
            *shared.deepgram_mode.write() = payload.deepgram_mode;
        }

        crate::settings::save_engine_config_batch(
            Some(payload.engine),
            Some(payload.sanitizer),
            payload.dual_engine,
            payload.reasoning_enabled,
            payload.reasoning_effort.clone(),
            payload.deepgram_mode,
        );

        Ok(EngineConfigSnapshot {
            engine: payload.engine,
            sanitizer: payload.sanitizer,
            dual_engine: payload.dual_engine,
            reasoning_enabled: payload.reasoning_enabled,
            reasoning_effort: payload.reasoning_effort,
            deepgram_mode: payload.deepgram_mode,
        })
    })
    .await
    .map_err(|e| CommandError::Internal(e.to_string()))?
}

/// `get_engine_config`
///
/// Returns the currently active transcription engine and sanitizer model selection.
#[tauri::command]
pub fn get_engine_config(state: State<'_, SharedState>) -> EngineConfigSnapshot {
    EngineConfigSnapshot {
        engine: *state.engine.read(),
        sanitizer: *state.sanitizer.read(),
        dual_engine: *state.dual_engine.read(),
        reasoning_enabled: *state.reasoning_enabled.read(),
        reasoning_effort: state.reasoning_effort.read().clone(),
        deepgram_mode: *state.deepgram_mode.read(),
    }
}

/// Validates the format of a non-empty API key for the given provider.
/// Returns `Ok(())` if the key is empty (treated as “not set”) or matches
/// the expected prefix; returns `Err(message)` for clearly malformed keys
/// so the user gets immediate feedback instead of a cryptic failure at
/// transcription time.
fn validate_api_key(provider: &str, key: &str) -> Result<(), String> {
    if key.is_empty() {
        return Ok(());
    }
    match provider {
        "groq" => {
            if !key.starts_with("gsk_") {
                return Err("A chave da API do Groq deve começar com 'gsk_'.".to_string());
            }
        }
        "google" => {
            if !key.starts_with("AIza") {
                return Err(
                    "A chave da API do Google (Gemini) deve começar com 'AIza'.".to_string()
                );
            }
        }
        // Deepgram keys are UUIDs: 8-4-4-4-12 hex digits separated by dashes.
        "deepgram" if !is_valid_uuid(key) => {
            return Err("A chave da API do Deepgram deve ser um UUID (formato xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx).".to_string());
        }
        _ => {}
    }
    Ok(())
}

/// Lightweight UUID format check (8-4-4-4-12 hex digits) without pulling
/// in the `uuid` or `regex` crates.
fn is_valid_uuid(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    // Dashes at positions 8, 13, 18, 23
    for &pos in &[8, 13, 18, 23] {
        if bytes[pos] != b'-' {
            return false;
        }
    }
    // All other positions must be hex digits
    bytes.iter().enumerate().all(|(i, &b)| {
        b == b'-' || (i != 8 && i != 13 && i != 18 && i != 23 && b.is_ascii_hexdigit())
    })
}

/// `save_api_keys`
///
/// Replaces the stored API keys atomically: updates the in-memory `ApiKeys`
/// struct and persists the same snapshot to `api_keys.json` so the keys
/// survive an app restart or machine reboot. Keys are kept as plain `String`
/// — the masking is strictly a UI concern.
#[tauri::command]
pub async fn save_api_keys(
    state: State<'_, SharedState>,
    payload: ApiKeysPayload,
) -> Result<(), CommandError> {
    log::info!(
        "save_api_keys: groq={} google={} deepgram={}",
        payload.groq.as_ref().map(|k| k.len()).unwrap_or(0),
        payload.google.as_ref().map(|k| k.len()).unwrap_or(0),
        payload.deepgram.as_ref().map(|k| k.len()).unwrap_or(0),
    );

    // Validate key formats before persisting so the user gets immediate
    // feedback instead of a failure at transcription time.
    if let Some(ref k) = payload.groq {
        validate_api_key("groq", k).map_err(CommandError::InvalidPayload)?;
    }
    if let Some(ref k) = payload.google {
        validate_api_key("google", k).map_err(CommandError::InvalidPayload)?;
    }
    if let Some(ref k) = payload.deepgram {
        validate_api_key("deepgram", k).map_err(CommandError::InvalidPayload)?;
    }

    let shared = state.inner().clone();
    tokio::task::spawn_blocking(move || {
        let keys = crate::models::ApiKeys {
            groq: payload.groq.filter(|s| !s.is_empty()),
            google: payload.google.filter(|s| !s.is_empty()),
            deepgram: payload.deepgram.filter(|s| !s.is_empty()),
        };

        {
            let mut guard = shared.api_keys.write();
            *guard = keys.clone();
        }

        // Persist outside the lock; the in-memory state is already authoritative
        // for the running session, so a failed write only affects the next launch.
        crate::secrets::save(&keys);
        Ok(())
    })
    .await
    .map_err(|e| CommandError::Internal(e.to_string()))?
}

/// `get_api_keys`
///
/// Returns the currently stored API keys so the settings screen can populate
/// its input fields on load (the keys are read back from `api_keys.json` at
/// startup into the in-memory state).
#[tauri::command]
pub async fn get_api_keys(
    state: State<'_, SharedState>,
) -> Result<crate::models::ApiKeys, CommandError> {
    let shared = state.inner().clone();
    let keys = tokio::task::spawn_blocking(move || shared.api_keys.read().clone())
        .await
        .map_err(|e| CommandError::Internal(e.to_string()))?;
    Ok(keys)
}

/// `transcribe_file`
///
/// Reads a local audio file selected/dropped in the frontend, transcribes it
/// through the engine currently active in settings, persists the source audio
/// to disk and appends a history entry (exactly like a microphone capture,
/// minus the clipboard injection). Returns the final sanitised text.
#[tauri::command]
pub async fn transcribe_file(
    state: State<'_, SharedState>,
    path: String,
) -> Result<String, CommandError> {
    log::info!("transcribe_file: {}", path);
    let shared = state.inner().clone();
    crate::audio::transcribe_file_path(&shared, path)
        .await
        .map_err(CommandError::Internal)
}

/// `evaluate_pronunciation`
///
/// Retrieves the saved audio for the history entry `id`, sends it together with
/// its transcribed text to Gemini Multimodal (using the stored Google API key)
/// and returns a Markdown speech assessment. The feedback is also persisted
/// onto the history entry so it survives a restart and re-opening the card.
#[tauri::command]
pub async fn evaluate_pronunciation(
    state: State<'_, SharedState>,
    id: String,
) -> Result<String, CommandError> {
    log::info!("evaluate_pronunciation: id={}", id);

    let entry = crate::history::get(&id)
        .ok_or_else(|| CommandError::Internal("histórico não encontrado".to_string()))?;

    let audio_path = entry.audio_path.ok_or_else(|| {
        CommandError::Internal("este item não possui áudio salvo para avaliar".to_string())
    })?;

    let google_key = {
        let guard = state.api_keys.read();
        guard.google.clone().filter(|k| !k.trim().is_empty())
    }
    .ok_or_else(|| {
        CommandError::Internal("configure a chave de API do Google (Gemini) em Ajustes".to_string())
    })?;

    let audio_bytes = crate::audio_store::read(&audio_path).map_err(CommandError::Internal)?;

    let ext = std::path::Path::new(&audio_path)
        .extension()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "wav".to_string());

    let feedback =
        crate::gemini::evaluate_pronunciation(audio_bytes, &ext, &entry.text, &google_key)
            .await
            .map_err(CommandError::Internal)?;

    // Persist the feedback so it is shown again without re-calling Gemini.
    crate::history::set_evaluation(&id, &feedback);

    Ok(feedback)
}

/// `toggle_recording_state`
///
/// Manual toggle used by the UI when the user clicks the record
/// button. Routes through the exact same pipeline as the global
/// `<Ctrl+B>` shortcut ([`crate::shortcuts::handle_toggle`]): starting
/// a capture opens the microphone and begins accumulating samples;
/// stopping it tears the stream down and dispatches the audio to the
/// active transcription engine, sanitises the text, copies it to the
/// clipboard and pastes it into the focused field. Returns the new
/// recording flag so the UI can update its timer immediately (the
/// `recording-*` Tauri events are emitted by `handle_toggle` too, for
/// cross-window sync).
#[tauri::command]
pub fn toggle_recording_state(
    app: tauri::AppHandle,
    state: State<'_, SharedState>,
) -> Result<bool, CommandError> {
    log::info!("toggle_recording_state (button) -> handle_toggle");
    let next = crate::shortcuts::handle_toggle(&app, state.inner());
    Ok(next)
}

/// `get_recording_state`
///
/// Read-only accessor used by the frontend on startup or after a
/// window refresh to sync the timer display with the backend truth.
#[tauri::command]
pub fn get_recording_state(state: State<'_, SharedState>) -> bool {
    state.is_recording()
}

/// `get_history`
///
/// Returns the full persisted transcription history, newest first. The
/// list lives in `history.json` inside the app data directory and is
/// kept in sync across invocations by the `history` module.
#[tauri::command]
pub async fn get_history() -> Vec<crate::models::HistoryEntry> {
    tokio::task::spawn_blocking(crate::history::load_all)
        .await
        .unwrap_or_default()
}

/// `clear_history`
///
/// Wipes the persisted transcription history. Used by the "Limpar Tudo"
/// button in the Histórico view.
#[tauri::command]
pub async fn clear_history() {
    tokio::task::spawn_blocking(|| {
        crate::history::clear();
        crate::audio_store::clear();
    })
    .await
    .unwrap_or_default();
}

/// `save_system_prompt`
///
/// Stores the user-edited sanitizer system prompt in the in-memory
/// application state and persists it to settings.json.
#[tauri::command]
pub async fn save_system_prompt(
    state: State<'_, SharedState>,
    prompt: String,
) -> Result<(), CommandError> {
    log::info!("save_system_prompt: {} chars", prompt.len());
    let shared = state.inner().clone();
    tokio::task::spawn_blocking(move || {
        *shared.system_prompt.write() = prompt.clone();
        crate::settings::save_system_prompt(prompt);
        Ok(())
    })
    .await
    .map_err(|e| CommandError::Internal(e.to_string()))?
}

/// `get_shortcuts`
///
/// Returns the currently active recording shortcuts so the Atalhos view can
/// display the live bindings on load.
#[tauri::command]
pub fn get_shortcuts(state: State<'_, SharedState>) -> crate::models::ShortcutConfig {
    state.shortcuts.read().clone()
}

/// `set_shortcuts`
///
/// Rebinds the global start/cancel recording shortcuts to the supplied key
/// combinations, re-registering them with the OS, persisting the choice and
/// returning the applied config. Rejects invalid/unavailable combinations
/// while keeping the previous binding intact.
#[tauri::command]
pub async fn set_shortcuts(
    app: tauri::AppHandle,
    state: State<'_, SharedState>,
    toggle: String,
    cancel: String,
) -> Result<crate::models::ShortcutConfig, CommandError> {
    log::info!("set_shortcuts: toggle={} cancel={}", toggle, cancel);
    let shared = state.inner().clone();
    tokio::task::spawn_blocking(move || {
        crate::shortcuts::apply_new(&app, &shared, toggle, cancel).map_err(CommandError::Internal)
    })
    .await
    .map_err(|e| CommandError::Internal(e.to_string()))?
}

/// `get_system_prompt`
///
/// Returns the currently stored sanitizer system prompt so the settings
/// UI can populate the textarea on load.
#[tauri::command]
pub fn get_system_prompt(state: State<'_, SharedState>) -> String {
    state.system_prompt.read().clone()
}

/// `get_custom_words`
///
/// Returns the user's custom vocabulary (canonical spellings) so the
/// Vocabulário settings tab can render the saved list on load.
#[tauri::command]
pub fn get_custom_words(state: State<'_, SharedState>) -> Vec<String> {
    state.custom_words.read().clone()
}

/// `set_custom_words`
///
/// Replaces the user's custom vocabulary. The incoming list is normalised —
/// each entry is trimmed, blanks are dropped, and case-insensitive duplicates
/// are removed (keeping the first spelling seen) — before being stored in the
/// in-memory state and persisted to `settings.json`. The cleaned list is
/// returned so the frontend can re-sync its UI with the canonical result.
#[tauri::command]
pub async fn set_custom_words(
    state: State<'_, SharedState>,
    words: Vec<String>,
) -> Result<Vec<String>, CommandError> {
    let cleaned: Vec<String> = {
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        words
            .into_iter()
            .map(|w| w.trim().to_string())
            .filter(|w| !w.is_empty())
            .filter(|w| seen.insert(w.to_lowercase()))
            .collect()
    };
    log::info!("set_custom_words: {} words", cleaned.len());

    let shared = state.inner().clone();
    let result = cleaned.clone();
    tokio::task::spawn_blocking(move || {
        *shared.custom_words.write() = cleaned.clone();
        crate::settings::save_custom_words(cleaned);
    })
    .await
    .map_err(|e| CommandError::Internal(e.to_string()))?;

    Ok(result)
}

/// `get_compact_mode`
///
/// Returns the persisted gadget compact-mode flag so both the settings screen
/// and the floating gadget window can sync their initial state on load.
#[tauri::command]
pub fn get_compact_mode(state: State<'_, SharedState>) -> bool {
    *state.compact_mode.read()
}

/// `set_compact_mode`
///
/// Updates the gadget compact-mode flag, persists it to `settings.json` and
/// broadcasts a `compact-mode-changed` event so the floating gadget window can
/// re-render its idle appearance live without a restart.
#[tauri::command]
pub async fn set_compact_mode(
    app: tauri::AppHandle,
    state: State<'_, SharedState>,
    value: bool,
) -> Result<(), CommandError> {
    log::info!("set_compact_mode: {}", value);
    let shared = state.inner().clone();
    tokio::task::spawn_blocking(move || {
        *shared.compact_mode.write() = value;
        crate::settings::save_compact(value);

        use tauri::Emitter;
        if let Err(e) = app.emit(crate::models::event_names::COMPACT_MODE_CHANGED, value) {
            log::warn!("failed to emit compact-mode-changed: {}", e);
        }
        Ok(())
    })
    .await
    .map_err(|e| CommandError::Internal(e.to_string()))?
}

/// `set_gadget_hit_rect`
///
/// Reported by the gadget overlay whenever its visible pill changes size or
/// state. Stores the pill's rectangle (logical pixels, relative to the gadget
/// window's top-left) so the background cursor watcher can make the overlay
/// window click-through everywhere except over the pill — eliminating the
/// invisible dead-zone that used to swallow clicks around the gadget.
#[tauri::command]
pub fn set_gadget_hit_rect(state: State<'_, SharedState>, rect: crate::models::GadgetHitRect) {
    *state.gadget_hit_rect.write() = Some(rect);
}

/// `get_recording_elapsed`
///
/// Returns the milliseconds elapsed since the current recording began, or `0`
/// if no recording is in progress. The frontend uses this to restore the
/// timer display after navigating away from Início and back.
#[tauri::command]
pub fn get_recording_elapsed(state: State<'_, SharedState>) -> u64 {
    state.recording_elapsed_ms()
}

/// `get_dev_mode`
///
/// Returns the persisted developer-mode flag so the settings screen and the
/// Histórico view can decide whether to expose the request-inspection panel.
#[tauri::command]
pub async fn get_dev_mode() -> Result<bool, CommandError> {
    tokio::task::spawn_blocking(crate::settings::load_dev_mode)
        .await
        .map_err(|e| CommandError::Internal(e.to_string()))
}

/// `set_dev_mode`
///
/// Persists the developer-mode flag. Capture of the sanitizer request happens
/// unconditionally on every transcription; this flag only gates the UI that
/// surfaces it in the Histórico.
#[tauri::command]
pub async fn set_dev_mode(value: bool) -> Result<(), CommandError> {
    log::info!("set_dev_mode: {}", value);
    tokio::task::spawn_blocking(move || {
        crate::settings::save_dev_mode(value);
        Ok(())
    })
    .await
    .map_err(|e| CommandError::Internal(e.to_string()))?
}

/// `get_sanitizer_enabled`
///
/// Returns whether the semantic validator (Stage 2 sanitization) is active.
/// When disabled, the raw acoustic transcription is delivered to the clipboard
/// without the Groq Chat Completions cleanup round-trip.
#[tauri::command]
pub fn get_sanitizer_enabled(state: State<'_, SharedState>) -> bool {
    *state.sanitizer_enabled.read()
}

/// `set_sanitizer_enabled`
///
/// Toggles the semantic validator on/off and persists the choice to
/// `settings.json`. Emitting the new value to the frontend is unnecessary
/// here: the only consumer is the settings toggle itself, which already
/// optimistically updated its local state.
#[tauri::command]
pub async fn set_sanitizer_enabled(
    state: State<'_, SharedState>,
    value: bool,
) -> Result<(), CommandError> {
    log::info!("set_sanitizer_enabled: {}", value);
    let shared = state.inner().clone();
    tokio::task::spawn_blocking(move || {
        *shared.sanitizer_enabled.write() = value;
        crate::settings::save_sanitizer_enabled(value);
        Ok(())
    })
    .await
    .map_err(|e| CommandError::Internal(e.to_string()))?
}

/// `list_audio_devices`
///
/// Queries CPAL for all available audio input devices on the host system.
#[tauri::command]
pub async fn list_audio_devices() -> Result<Vec<String>, CommandError> {
    tokio::task::spawn_blocking(|| {
        use cpal::traits::{DeviceTrait, HostTrait};
        let host = cpal::default_host();
        let devices = host.input_devices().map_err(|e| {
            CommandError::Internal(format!("Falha ao listar dispositivos de áudio: {}", e))
        })?;
        let mut names = Vec::new();
        for device in devices {
            if let Ok(name) = device.name() {
                names.push(name);
            }
        }
        Ok(names)
    })
    .await
    .map_err(|e| CommandError::Internal(e.to_string()))?
}

/// `get_input_device`
///
/// Returns the currently active input device selection from the settings store.
#[tauri::command]
pub async fn get_input_device() -> Option<String> {
    tokio::task::spawn_blocking(crate::settings::load_input_device)
        .await
        .unwrap_or_default()
}

/// `set_input_device`
///
/// Saves the user's manual input device selection to settings.
#[tauri::command]
pub async fn set_input_device(device: Option<String>) -> Result<(), CommandError> {
    log::info!("set_input_device: {:?}", device);
    tokio::task::spawn_blocking(move || {
        crate::settings::save_input_device(device);
        Ok(())
    })
    .await
    .map_err(|e| CommandError::Internal(e.to_string()))?
}

/// `start_mic_test`
///
/// Instantiates a temporary input stream bound to the configured mic to pipe
/// loudness events back to the UI.
#[tauri::command]
pub async fn start_mic_test(
    app: tauri::AppHandle,
    state: State<'_, SharedState>,
) -> Result<(), CommandError> {
    log::info!("start_mic_test");
    let shared = state.inner().clone();
    tokio::task::spawn_blocking(move || {
        crate::audio::start_mic_test_stream(&app, &shared).map_err(CommandError::Internal)
    })
    .await
    .map_err(|e| CommandError::Internal(e.to_string()))?
}

/// `stop_mic_test`
///
/// Tears down the active test stream and releases the microphone.
#[tauri::command]
pub async fn stop_mic_test(state: State<'_, SharedState>) -> Result<(), CommandError> {
    log::info!("stop_mic_test");
    let shared = state.inner().clone();
    tokio::task::spawn_blocking(move || {
        *shared.test_stream.lock() = None;
        Ok(())
    })
    .await
    .map_err(|e| CommandError::Internal(e.to_string()))?
}

/// `retry_transcription`
///
/// Retries a previously failed transcription using its persisted audio file.
#[tauri::command]
pub async fn retry_transcription(
    state: State<'_, SharedState>,
    id: String,
) -> Result<String, CommandError> {
    log::info!("retry_transcription: {}", id);
    let shared = state.inner().clone();
    crate::audio::retry_transcription_handler(&shared, &id)
        .await
        .map_err(CommandError::Internal)
}

/// Builds a `reg` Command pre-configured to never flash a console window.
///
/// On Windows, spawning any console application (.exe) from a GUI process
/// creates a visible `cmd.exe`-style window for the brief moment the child
/// is alive. When the user rapidly toggles between settings sub-tabs this
/// surfaces as an annoying CMD flicker, so we attach the
/// `CREATE_NO_WINDOW` creation flag to every `reg` invocation.
#[cfg(target_os = "windows")]
fn reg_command(args: &[&str]) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut cmd = std::process::Command::new("reg");
    cmd.args(args);
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

/// `get_autostart`
///
/// Checks if the application is registered to start automatically on Windows.
///
/// Runs on `spawn_blocking` because spawning `reg.exe` and waiting for it
/// is a blocking operation that would otherwise stall the Tauri IPC
/// dispatcher thread and freeze the UI for a couple of seconds every time
/// the settings view mounts.
#[tauri::command]
pub async fn get_autostart() -> Result<bool, CommandError> {
    tokio::task::spawn_blocking(|| {
        #[cfg(target_os = "windows")]
        {
            let output = reg_command(&[
                "query",
                "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run",
                "/v",
                "HaumeaVoice",
            ])
            .output()
            .map_err(|e| CommandError::Internal(e.to_string()))?;
            Ok(output.status.success())
        }
        #[cfg(not(target_os = "windows"))]
        {
            Ok(false)
        }
    })
    .await
    .map_err(|e| CommandError::Internal(e.to_string()))?
}

/// `set_autostart`
///
/// Enables or disables the application autostart registry key on Windows.
///
/// Like `get_autostart`, runs off the IPC thread so the blocking `reg`
/// invocation never stalls the frontend.
#[tauri::command]
pub async fn set_autostart(enabled: bool) -> Result<(), CommandError> {
    tokio::task::spawn_blocking(move || {
        #[cfg(target_os = "windows")]
        {
            if enabled {
                let exe_path = std::env::current_exe().map_err(|e| {
                    CommandError::Internal(format!(
                        "Não foi possível obter o caminho do executável: {}",
                        e
                    ))
                })?;
                let exe_path_str = exe_path.to_string_lossy();
                let val = format!("\"{}\" --autostart", exe_path_str);

                let status = reg_command(&[
                    "add",
                    "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run",
                    "/v",
                    "HaumeaVoice",
                    "/t",
                    "REG_SZ",
                    "/d",
                    &val,
                    "/f",
                ])
                .status()
                .map_err(|e| CommandError::Internal(e.to_string()))?;

                if !status.success() {
                    return Err(CommandError::Internal(
                        "Falha ao configurar inicialização no registro.".to_string(),
                    ));
                }
            } else {
                // Delete might fail if the key does not exist. We run it, but don't strictly crash if it fails because it's already removed.
                let _ = reg_command(&[
                    "delete",
                    "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run",
                    "/v",
                    "HaumeaVoice",
                    "/f",
                ])
                .status();
            }
            Ok(())
        }
        #[cfg(not(target_os = "windows"))]
        {
            Ok(())
        }
    })
    .await
    .map_err(|e| CommandError::Internal(e.to_string()))?
}
