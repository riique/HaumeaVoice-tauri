use crate::models::{
    ApiKeysPayload, DeepgramMode, EngineConfigPayload, GadgetVisualState, SharedState,
    TranscriptionEngine, WidgetPreferences, WidgetVisibilityMode,
};
use crate::pipeline_contract::TranscriptionMode;
use serde::{Deserialize, Serialize};
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

/// Product-mode configuration (Phase 04+). Independent from legacy engine cards
/// so existing installs keep the dual/Deepgram path until modes are enabled.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeConfigPayload {
    pub modes_enabled: bool,
    pub mode: TranscriptionMode,
    #[serde(default = "default_true_fallback")]
    pub gemini_fallback_to_whisper: bool,
    #[serde(default)]
    pub content_type: crate::pipeline_contract::ContentType,
    #[serde(default)]
    pub gemini_pipelines: crate::pipeline_contract::GeminiPipelineConfig,
}

fn default_true_fallback() -> bool {
    true
}

#[derive(Debug, Clone, Serialize)]
pub struct ModeConfigSnapshot {
    pub modes_enabled: bool,
    pub mode: TranscriptionMode,
    pub gemini_fallback_to_whisper: bool,
    pub content_type: crate::pipeline_contract::ContentType,
    pub gemini_pipelines: crate::pipeline_contract::GeminiPipelineConfig,
    /// Human labels for UI (copywriting).
    pub mode_label: String,
    pub mode_description: String,
}

fn mode_copy(mode: TranscriptionMode) -> (&'static str, &'static str) {
    match mode {
        TranscriptionMode::UltraFast => (
            "Ultrarrápido",
            "Whisper via OpenRouter STT com provedor Groq fixo",
        ),
        TranscriptionMode::FastAccurate => ("Rápido e preciso", "Transcrição direta com Gemini"),
        TranscriptionMode::Precise => ("Preciso", "Whisper e Gemini em paralelo"),
        TranscriptionMode::UltraPrecise => {
            ("Ultrapreciso", "Whisper, validador e Gemini em sequência")
        }
    }
}

#[tauri::command]
pub fn get_mode_config(state: State<'_, SharedState>) -> ModeConfigSnapshot {
    let mode = *state.transcription_mode.read();
    let (label, desc) = mode_copy(mode);
    ModeConfigSnapshot {
        modes_enabled: *state.modes_enabled.read(),
        mode,
        gemini_fallback_to_whisper: *state.gemini_fallback_to_whisper.read(),
        content_type: *state.content_type.read(),
        gemini_pipelines: state.gemini_pipelines.read().clone(),
        mode_label: label.to_string(),
        mode_description: desc.to_string(),
    }
}

#[tauri::command]
pub async fn update_mode_config(
    state: State<'_, SharedState>,
    payload: ModeConfigPayload,
) -> Result<ModeConfigSnapshot, CommandError> {
    for choice in [
        &payload.gemini_pipelines.fast_accurate,
        &payload.gemini_pipelines.precise,
        &payload.gemini_pipelines.ultra_precise,
    ] {
        choice
            .resolved_model_id()
            .map_err(CommandError::InvalidPayload)?;
    }

    log::info!(
        "update_mode_config: enabled={} mode={:?} gemini_fallback={}",
        payload.modes_enabled,
        payload.mode,
        payload.gemini_fallback_to_whisper
    );

    let shared = state.inner().clone();
    tokio::task::spawn_blocking(move || {
        *shared.modes_enabled.write() = payload.modes_enabled;
        *shared.transcription_mode.write() = payload.mode;
        *shared.gemini_fallback_to_whisper.write() = payload.gemini_fallback_to_whisper;
        *shared.content_type.write() = payload.content_type;
        *shared.gemini_pipelines.write() = payload.gemini_pipelines.clone();
        crate::settings::save_mode_config_batch(
            payload.modes_enabled,
            payload.mode,
            payload.gemini_fallback_to_whisper,
            payload.content_type,
            payload.gemini_pipelines.clone(),
        );
        let (label, desc) = mode_copy(payload.mode);
        Ok(ModeConfigSnapshot {
            modes_enabled: payload.modes_enabled,
            mode: payload.mode,
            gemini_fallback_to_whisper: payload.gemini_fallback_to_whisper,
            content_type: payload.content_type,
            gemini_pipelines: payload.gemini_pipelines,
            mode_label: label.to_string(),
            mode_description: desc.to_string(),
        })
    })
    .await
    .map_err(|e| CommandError::Internal(e.to_string()))?
}

/// Performs only provider-agnostic safety validation on a non-empty API key.
/// Provider key formats are opaque and may change; authentication is verified
/// by the provider when the credential is first used.
fn validate_api_key(provider: &str, key: &str) -> Result<(), String> {
    if key.is_empty() {
        return Ok(());
    }
    if key.chars().any(char::is_control) {
        return Err(format!(
            "A chave da API do {} contém caracteres de controle inválidos.",
            provider
        ));
    }
    Ok(())
}

#[cfg(test)]
mod api_key_validation_tests {
    use super::validate_api_key;

    #[test]
    fn accepts_opaque_provider_key_formats() {
        assert!(validate_api_key("deepgram", "0123456789abcdef0123456789abcdef01234567").is_ok());
        assert!(validate_api_key("google", "future-format-without-known-prefix").is_ok());
        assert!(validate_api_key("groq", "another-opaque-format").is_ok());
    }

    #[test]
    fn rejects_embedded_control_characters() {
        assert!(validate_api_key("google", "valid-part\nsecond-part").is_err());
    }
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
        "save_api_keys: groq={} google={} deepgram={} openrouter={}",
        payload.groq.len(),
        payload.google.len(),
        payload.deepgram.len(),
        payload.openrouter.len(),
    );

    // Treat provider credentials as opaque. Do not reject valid keys based on
    // guessed prefixes or shapes (Deepgram keys, for example, are not UUIDs).
    for (provider, keys) in [
        ("groq", &payload.groq),
        ("google", &payload.google),
        ("deepgram", &payload.deepgram),
        ("openrouter", &payload.openrouter),
    ] {
        for key in keys {
            validate_api_key(provider, key.trim()).map_err(CommandError::InvalidPayload)?;
        }
    }

    let shared = state.inner().clone();
    tokio::task::spawn_blocking(move || {
        let keys = crate::models::ApiKeys {
            groq: payload.groq,
            google: payload.google,
            deepgram: payload.deepgram,
            openrouter: payload.openrouter,
        }
        .normalized();

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

    let google_key = state.next_google_key().ok_or_else(|| {
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

/// Cancels the active capture through the same path as the global panic
/// shortcut. This lets the floating gadget discard a recording without
/// producing audio output or starting the transcription pipeline.
#[tauri::command]
pub fn cancel_recording(
    app: tauri::AppHandle,
    state: State<'_, SharedState>,
) -> Result<(), CommandError> {
    crate::shortcuts::handle_cancel(&app, state.inner());
    Ok(())
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

#[derive(Debug, Clone, Serialize)]
pub struct AudioStorageConfig {
    pub custom_directory: Option<String>,
    pub effective_directory: String,
    pub default_directory: String,
}

fn audio_storage_snapshot() -> Result<AudioStorageConfig, CommandError> {
    let default = crate::audio_store::default_directory().ok_or_else(|| {
        CommandError::Internal("armazenamento de áudio ainda não inicializado".to_string())
    })?;
    let effective = crate::audio_store::effective_directory().unwrap_or_else(|| default.clone());
    Ok(AudioStorageConfig {
        custom_directory: crate::settings::load_audio_directory(),
        effective_directory: effective.to_string_lossy().into_owned(),
        default_directory: default.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
pub fn get_audio_storage_config() -> Result<AudioStorageConfig, CommandError> {
    audio_storage_snapshot()
}

/// Changes only the destination for future audio files. Existing history
/// entries retain their absolute paths and are neither moved nor deleted.
#[tauri::command]
pub async fn set_audio_storage_directory(
    path: Option<String>,
) -> Result<AudioStorageConfig, CommandError> {
    tokio::task::spawn_blocking(move || {
        let saved = match path.filter(|value| !value.trim().is_empty()) {
            Some(path) => Some(
                crate::audio_store::prepare_custom_directory(&path)
                    .map_err(CommandError::InvalidPayload)?
                    .to_string_lossy()
                    .into_owned(),
            ),
            None => None,
        };
        crate::settings::save_audio_directory(saved);
        audio_storage_snapshot()
    })
    .await
    .map_err(|e| CommandError::Internal(e.to_string()))?
}

/// Opens Windows Explorer with the persisted source audio selected. Resolving
/// by history id keeps arbitrary paths out of the frontend IPC surface.
#[tauri::command]
pub async fn reveal_history_audio(id: String) -> Result<(), CommandError> {
    tokio::task::spawn_blocking(move || {
        let entry = crate::history::get(&id)
            .ok_or_else(|| CommandError::Internal("histórico não encontrado".to_string()))?;
        let path = entry.audio_path.ok_or_else(|| {
            CommandError::Internal("este item não possui áudio salvo".to_string())
        })?;
        let canonical = std::fs::canonicalize(&path).map_err(|e| {
            CommandError::Internal(format!("não foi possível localizar o áudio salvo: {e}"))
        })?;

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            std::process::Command::new("explorer.exe")
                .arg(format!("/select,{}", canonical.display()))
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()
                .map_err(|e| {
                    CommandError::Internal(format!("não foi possível abrir o Explorer: {e}"))
                })?;
            Ok(())
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = canonical;
            Err(CommandError::Internal(
                "mostrar o arquivo está disponível apenas no Windows".to_string(),
            ))
        }
    })
    .await
    .map_err(|e| CommandError::Internal(e.to_string()))?
}

/// `clear_history`
///
/// Wipes the persisted transcription history. Used by the "Limpar Tudo"
/// button in the Histórico view.
#[tauri::command]
pub async fn clear_history() {
    tokio::task::spawn_blocking(|| {
        crate::history::clear();
    })
    .await
    .unwrap_or_default();
}

/// Deletes a single history entry (and its audio file if present).
#[tauri::command]
pub async fn delete_history_entry(id: String) -> Result<(), CommandError> {
    let ok = tokio::task::spawn_blocking(move || crate::history::delete_entry(&id))
        .await
        .map_err(|e| CommandError::Internal(e.to_string()))?;
    if ok {
        Ok(())
    } else {
        Err(CommandError::Internal(
            "entrada de histórico não encontrada".into(),
        ))
    }
}

/// Updates the final text of a history entry (manual edit). Keeps evaluation.
#[tauri::command]
pub async fn update_history_text(id: String, text: String) -> Result<(), CommandError> {
    let ok = tokio::task::spawn_blocking(move || crate::history::update_text(&id, &text))
        .await
        .map_err(|e| CommandError::Internal(e.to_string()))?;
    if ok {
        Ok(())
    } else {
        Err(CommandError::Internal(
            "entrada de histórico não encontrada".into(),
        ))
    }
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

/// `get_custom_words` — legacy: enabled canonical spellings only.
#[tauri::command]
pub fn get_custom_words(state: State<'_, SharedState>) -> Vec<String> {
    crate::vocabulary::canonical_list(&state.vocabulary.read())
}

/// `set_custom_words` — legacy: replaces vocabulary with simple words.
#[tauri::command]
pub async fn set_custom_words(
    state: State<'_, SharedState>,
    words: Vec<String>,
) -> Result<Vec<String>, CommandError> {
    let terms = crate::vocabulary::migrate_from_strings(&words);
    let terms =
        crate::vocabulary::normalize_and_validate(terms).map_err(CommandError::InvalidPayload)?;
    let result = crate::vocabulary::canonical_list(&terms);
    log::info!("set_custom_words: {} terms (legacy)", result.len());

    let shared = state.inner().clone();
    let terms_store = terms.clone();
    tokio::task::spawn_blocking(move || {
        *shared.vocabulary.write() = terms_store.clone();
        crate::settings::save_vocabulary(terms_store);
    })
    .await
    .map_err(|e| CommandError::Internal(e.to_string()))?;

    Ok(result)
}

/// Full structured vocabulary.
#[tauri::command]
pub fn get_vocabulary(state: State<'_, SharedState>) -> Vec<crate::vocabulary::VocabularyTerm> {
    state.vocabulary.read().clone()
}

/// Replace structured vocabulary after validation.
#[tauri::command]
pub async fn set_vocabulary(
    state: State<'_, SharedState>,
    terms: Vec<crate::vocabulary::VocabularyTerm>,
) -> Result<Vec<crate::vocabulary::VocabularyTerm>, CommandError> {
    let cleaned =
        crate::vocabulary::normalize_and_validate(terms).map_err(CommandError::InvalidPayload)?;
    log::info!("set_vocabulary: {} terms", cleaned.len());

    let shared = state.inner().clone();
    let result = cleaned.clone();
    tokio::task::spawn_blocking(move || {
        *shared.vocabulary.write() = cleaned.clone();
        crate::settings::save_vocabulary(cleaned);
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
    let mode = if value {
        WidgetVisibilityMode::Always
    } else {
        WidgetVisibilityMode::Auto
    };
    set_widget_visibility_mode(app, state, mode)
        .await
        .map(|_| ())
}

#[tauri::command]
pub fn get_widget_preferences(state: State<'_, SharedState>) -> WidgetPreferences {
    WidgetPreferences {
        visibility_mode: *state.widget_visibility_mode.read(),
        dock: *state.widget_dock.read(),
        display: crate::settings::load_widget_display(),
    }
}

#[tauri::command]
pub async fn set_widget_visibility_mode(
    app: tauri::AppHandle,
    state: State<'_, SharedState>,
    mode: WidgetVisibilityMode,
) -> Result<WidgetPreferences, CommandError> {
    log::info!("set_widget_visibility_mode: {:?}", mode);
    *state.widget_visibility_mode.write() = mode;
    *state.compact_mode.write() = mode == WidgetVisibilityMode::Always;
    tokio::task::spawn_blocking(move || crate::settings::save_widget_visibility_mode(mode))
        .await
        .map_err(|error| CommandError::Internal(error.to_string()))?;

    let current = *state.gadget_visual_state.read();
    if matches!(
        current,
        GadgetVisualState::Hidden | GadgetVisualState::Idle | GadgetVisualState::Hover
    ) {
        let target = if mode == WidgetVisibilityMode::Always {
            GadgetVisualState::Idle
        } else {
            GadgetVisualState::Hidden
        };
        crate::present_gadget(&app, state.inner(), target).map_err(CommandError::Internal)?;
    }

    let snapshot = get_widget_preferences(state);
    use tauri::Emitter;
    if let Err(error) = app.emit(
        crate::models::event_names::WIDGET_PREFERENCES_CHANGED,
        &snapshot,
    ) {
        log::warn!("failed to emit widget-preferences-changed: {}", error);
    }
    Ok(snapshot)
}

#[tauri::command]
pub fn set_gadget_visual_state(
    app: tauri::AppHandle,
    state: State<'_, SharedState>,
    visual_state: GadgetVisualState,
) -> Result<GadgetVisualState, CommandError> {
    crate::present_gadget(&app, state.inner(), visual_state).map_err(CommandError::Internal)
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
