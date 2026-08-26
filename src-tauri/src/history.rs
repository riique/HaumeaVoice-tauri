//! Persistent transcription history.
//!
//! Each finished transcription is appended to a JSON file stored inside the
//! per-user Tauri app data directory (`history.json`). The file holds a plain
//! `Vec<HistoryEntry>` with the newest entry first. Reads/writes are guarded
//! by a process-wide `Mutex` so concurrent recording stops (which run on the
//! Tokio runtime) cannot interleave and corrupt the file.

use crate::models::HistoryEntry;
use crate::pipeline_contract::{ContentType, TranscriptionMode};
use crate::pipeline_run::{AudioTransport, PipelineRun, PIPELINE_RUN_SCHEMA_VERSION};
use parking_lot::Mutex;
use std::{fs, path::PathBuf, sync::OnceLock};

static HISTORY_PATH: OnceLock<PathBuf> = OnceLock::new();
static HISTORY_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

/// Called once during app setup with the resolved Tauri app data directory.
/// Stores the path so every subsequent helper can read/write without having
/// to thread the `AppHandle` through the audio pipeline.
pub fn init(data_dir: PathBuf) {
    let _ = HISTORY_PATH.set(data_dir);
    let _ = HISTORY_LOCK.set(Mutex::new(()));
}

fn path() -> Option<&'static PathBuf> {
    HISTORY_PATH.get()
}

fn lock() -> &'static Mutex<()> {
    HISTORY_LOCK.get_or_init(|| Mutex::new(()))
}

fn legacy_mode(value: Option<&str>) -> TranscriptionMode {
    match value.unwrap_or_default() {
        "fast-accurate" => TranscriptionMode::FastAccurate,
        "precise" => TranscriptionMode::Precise,
        "ultra-precise" => TranscriptionMode::UltraPrecise,
        _ => TranscriptionMode::UltraFast,
    }
}

fn legacy_transport(entry: &HistoryEntry) -> AudioTransport {
    match entry
        .gemini_transport
        .as_deref()
        .or(entry.deepgram_mode.as_deref())
    {
        Some("multipart") => AudioTransport::Multipart,
        Some("files_api") | Some("resumable_file") => AudioTransport::ResumableFile,
        Some("streaming_final") | Some("websocket_stream") => AudioTransport::WebSocketStream,
        Some("raw_binary") | Some("batch") => AudioTransport::RawBinary,
        Some("url") => AudioTransport::Url,
        _ => AudioTransport::InlineBase64,
    }
}

fn migrate_entry(entry: &mut HistoryEntry) -> bool {
    let mut changed = entry.schema_version < PIPELINE_RUN_SCHEMA_VERSION;
    if entry.pipeline_runs.is_empty() {
        let mode = legacy_mode(entry.mode.as_deref());
        let mut run = if entry.is_error.unwrap_or(false) {
            PipelineRun::hard_error(
                format!("{}-run-legacy", entry.id),
                mode,
                entry
                    .error_message
                    .clone()
                    .unwrap_or_else(|| "legacy pipeline failure".into()),
            )
        } else {
            PipelineRun::success(format!("{}-run-legacy", entry.id), mode, entry.text.clone())
        };
        run.schema_version = PIPELINE_RUN_SCHEMA_VERSION;
        run.session_id = format!("{}-session-legacy", entry.id);
        run.history_engine_label = entry.engine.clone();
        run.model = entry.model.clone().unwrap_or_else(|| entry.engine.clone());
        run.gemini_transport = entry
            .gemini_transport
            .clone()
            .or_else(|| entry.deepgram_mode.clone());
        run.stages = entry
            .stages
            .as_deref()
            .map(|stages| {
                stages
                    .split(',')
                    .map(str::trim)
                    .filter(|stage| !stage.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        run.warnings = entry.warnings.clone().unwrap_or_default();
        run.used_fallback = entry.used_fallback.unwrap_or(false);
        run.fallback_reason = entry.fallback_reason.clone();
        run.whisper_text = entry.whisper_text.clone();
        run.sanitizer_text = entry.sanitizer_text.clone();
        run.gemini_text = entry.gemini_text.clone();
        run.final_text = entry.text.clone();
        run.transcript.raw = entry
            .whisper_text
            .clone()
            .or_else(|| entry.gemini_text.clone())
            .or_else(|| Some(entry.text.clone()));
        run.transcript.refined = entry
            .sanitizer_text
            .clone()
            .or_else(|| entry.gemini_text.clone())
            .or_else(|| Some(entry.text.clone()));
        run.transcript.formatted = Some(entry.text.clone());
        run.transcript.delivered = Some(entry.text.clone());
        run.transcription_latency_ms = entry.transcription_latency_ms.unwrap_or(entry.latency_ms);
        run.audio_prepare_ms = entry.audio_prepare_ms;
        run.base64_ms = entry.base64_ms;
        run.whisper_ms = entry.whisper_ms;
        run.sanitizer_ms = entry.sanitizer_ms.or(entry.sanitizer_latency_ms);
        run.files_upload_ms = entry.files_upload_ms;
        run.files_poll_ms = entry.files_poll_ms;
        run.files_poll_count = entry.files_poll_count;
        run.gemini_generate_ms = entry.gemini_generate_ms;
        run.gemini_delete_ms = entry.gemini_delete_ms;
        run.strict_literals_ms = entry.strict_literals_ms;
        run.total_pipeline_ms = entry.total_pipeline_ms.or(Some(entry.latency_ms));
        run.reported_total_tokens = entry.total_tokens;
        run.content_hint = entry
            .content_type
            .as_deref()
            .and_then(ContentType::from_str)
            .unwrap_or_default();
        run.debug_info = entry.debug_info.clone();
        run.normalize();
        if let Some(attempt) = run.attempts.last_mut() {
            attempt.transport = legacy_transport(entry);
            attempt
                .result
                .extra
                .insert("migrated_legacy_history".into(), true.into());
        }
        entry.pipeline_runs.push(run);
        changed = true;
    }
    for run in &mut entry.pipeline_runs {
        if run.schema_version < PIPELINE_RUN_SCHEMA_VERSION {
            run.schema_version = PIPELINE_RUN_SCHEMA_VERSION;
            changed = true;
        }
    }
    if entry.schema_version != PIPELINE_RUN_SCHEMA_VERSION {
        entry.schema_version = PIPELINE_RUN_SCHEMA_VERSION;
        changed = true;
    }
    changed
}

fn decode_entries(contents: &str) -> (Vec<HistoryEntry>, bool) {
    let mut entries: Vec<HistoryEntry> = match serde_json::from_str(contents) {
        Ok(entries) => entries,
        Err(error) => {
            log::error!("history: failed to parse existing history: {}", error);
            return (Vec::new(), false);
        }
    };
    let mut changed = false;
    for entry in &mut entries {
        changed |= migrate_entry(entry);
    }
    (entries, changed)
}

fn read_entries(file: &PathBuf) -> (Vec<HistoryEntry>, bool) {
    match fs::read_to_string(file) {
        Ok(contents) if !contents.trim().is_empty() => decode_entries(&contents),
        _ => (Vec::new(), false),
    }
}

fn persist_entries(file: &PathBuf, entries: &[HistoryEntry]) -> bool {
    if let Err(error) = fs::create_dir_all(file.parent().unwrap_or(file.as_path())) {
        log::error!("history: could not create data dir: {}", error);
        return false;
    }
    let json = match serde_json::to_string_pretty(entries) {
        Ok(json) => json,
        Err(error) => {
            log::error!("history: failed to serialize: {}", error);
            return false;
        }
    };
    if let Err(error) = fs::write(file, json) {
        log::error!("history: failed to write file: {}", error);
        return false;
    }
    true
}

fn persist_migration(file: &PathBuf, entries: &[HistoryEntry]) {
    let backup = file.with_extension("pre-v2.json");
    if file.exists() && !backup.exists() {
        if let Err(error) = fs::copy(file, &backup) {
            log::error!("history: migration backup failed: {}", error);
            return;
        }
    }
    if persist_entries(file, entries) {
        log::info!(
            "history: migrated entries to schema {}",
            PIPELINE_RUN_SCHEMA_VERSION
        );
    }
}

fn merge_updated_entry(existing: &HistoryEntry, updated: &HistoryEntry) -> HistoryEntry {
    let mut replacement = updated.clone();
    for prior_run in &existing.pipeline_runs {
        if !replacement
            .pipeline_runs
            .iter()
            .any(|run| run.id == prior_run.id)
        {
            replacement.pipeline_runs.insert(0, prior_run.clone());
        }
    }
    if replacement.evaluation.is_none() {
        replacement.evaluation = existing.evaluation.clone();
    }
    replacement.schema_version = PIPELINE_RUN_SCHEMA_VERSION;
    replacement
}

/// Reads the full history from disk. Returns an empty vec if the file does
/// not exist yet or cannot be parsed (treated as a fresh start rather than a
/// hard error so a corrupted file never blocks the UI).
pub fn load_all() -> Vec<HistoryEntry> {
    let _guard = lock().lock();
    let Some(file) = path() else {
        return Vec::new();
    };
    let (entries, migrated) = read_entries(file);
    if migrated {
        persist_migration(file, &entries);
    }
    entries
}

/// Appends `entry` to the front of the history list and persists it to disk.
/// Failures are logged but never propagated: history is a convenience, not a
/// requirement for the transcription to be considered successful.
pub fn push(entry: HistoryEntry) {
    let _guard = lock().lock();
    let Some(file) = path() else {
        log::warn!("history: data directory not initialised, skipping save");
        return;
    };

    let (mut entries, _) = read_entries(file);
    entries.insert(0, entry);
    let _ = persist_entries(file, &entries);
}

/// Returns a single history entry by id, or `None` if it does not exist.
/// Used by the pronunciation evaluator to recover the saved audio path and
/// transcribed text for a given card.
pub fn get(id: &str) -> Option<HistoryEntry> {
    load_all().into_iter().find(|e| e.id == id)
}

/// Stores the Gemini pronunciation feedback on the entry with the given id and
/// rewrites the history file. Returns `true` if the entry was found and
/// updated. The whole file is rewritten because it is small and this keeps the
/// on-disk format a plain JSON array.
pub fn set_evaluation(id: &str, feedback: &str) -> bool {
    let _guard = lock().lock();
    let Some(file) = path() else {
        return false;
    };

    let (mut entries, _) = read_entries(file);

    let mut found = false;
    for entry in entries.iter_mut() {
        if entry.id == id {
            entry.evaluation = Some(feedback.to_string());
            found = true;
            break;
        }
    }

    if found && !persist_entries(file, &entries) {
        return false;
    }
    found
}

/// Empties the history file. Used by the "Limpar Tudo" button.
pub fn clear() {
    let _guard = lock().lock();
    let Some(file) = path() else {
        return;
    };
    let (entries, _) = read_entries(file);
    for entry in entries {
        if let Some(path) = entry.audio_path {
            crate::audio_store::remove_with_original(&path);
        }
    }
    let _ = fs::write(file, "[]");
}

/// Deletes one history entry by id. Also best-effort removes its audio file.
pub fn delete_entry(id: &str) -> bool {
    let _guard = lock().lock();
    let Some(file) = path() else {
        return false;
    };
    let (mut entries, _) = read_entries(file);
    let before = entries.len();
    if let Some(pos) = entries.iter().position(|e| e.id == id) {
        if let Some(p) = entries[pos].audio_path.take() {
            crate::audio_store::remove_with_original(&p);
        }
        entries.remove(pos);
    }
    if entries.len() == before {
        return false;
    }
    persist_entries(file, &entries)
}

/// Updates only the final text (and word count) of an entry — preserves evaluation.
pub fn update_text(id: &str, text: &str) -> bool {
    let _guard = lock().lock();
    let Some(file) = path() else {
        return false;
    };
    let (mut entries, _) = read_entries(file);
    let mut found = false;
    for entry in entries.iter_mut() {
        if entry.id == id {
            entry.text = text.to_string();
            entry.words = text.split_whitespace().count();
            entry.is_error = Some(false);
            entry.error_message = None;
            if let Some(run) = entry.pipeline_runs.last_mut() {
                run.transcript.set_user_corrected(text.to_string());
            }
            found = true;
            break;
        }
    }
    if !found {
        return false;
    }
    persist_entries(file, &entries)
}

/// Updates an existing history entry with new details.
pub fn update_entry(updated: HistoryEntry) -> bool {
    let _guard = lock().lock();
    let Some(file) = path() else {
        return false;
    };

    let (mut entries, _) = read_entries(file);

    let mut found = false;
    for entry in entries.iter_mut() {
        if entry.id == updated.id {
            *entry = merge_updated_entry(entry, &updated);
            found = true;
            break;
        }
    }

    if found && !persist_entries(file, &entries) {
        return false;
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legacy_entry() -> HistoryEntry {
        serde_json::from_value(serde_json::json!({
            "id": "legacy-1",
            "date": "2026-08-25 10:00",
            "words": 2,
            "engine": "GroqWhisper",
            "text": "Open Router",
            "whisper_text": "open router",
            "sanitizer_text": "Open Router",
            "model": "whisper-large-v3-turbo",
            "stages": "whisper,sanitizer",
            "total_tokens": 4
        }))
        .unwrap()
    }

    #[test]
    fn legacy_entry_migrates_without_losing_projection() {
        let mut entry = legacy_entry();
        assert!(migrate_entry(&mut entry));
        assert_eq!(entry.schema_version, PIPELINE_RUN_SCHEMA_VERSION);
        assert_eq!(entry.text, "Open Router");
        assert_eq!(entry.pipeline_runs.len(), 1);
        let run = &entry.pipeline_runs[0];
        assert_eq!(run.transcript.raw.as_deref(), Some("open router"));
        assert_eq!(run.transcript.refined.as_deref(), Some("Open Router"));
        assert_eq!(run.attempts.len(), 1);
        assert_eq!(run.journal.len(), 2);
    }

    #[test]
    fn retry_merge_preserves_previous_pipeline_run() {
        let mut existing = legacy_entry();
        migrate_entry(&mut existing);
        let original_run_id = existing.pipeline_runs[0].id.clone();

        let mut updated = existing.clone();
        updated.pipeline_runs.clear();
        updated.pipeline_runs.push(PipelineRun::success(
            "legacy-1-run-retry",
            TranscriptionMode::UltraFast,
            "OpenRouter",
        ));
        let merged = merge_updated_entry(&existing, &updated);
        assert_eq!(merged.pipeline_runs.len(), 2);
        assert_eq!(merged.pipeline_runs[0].id, original_run_id);
        assert_eq!(merged.pipeline_runs[1].id, "legacy-1-run-retry");
    }
}
