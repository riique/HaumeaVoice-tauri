//! Persistent transcription history.
//!
//! Each finished transcription is appended to a JSON file stored inside the
//! per-user Tauri app data directory (`history.json`). The file holds a plain
//! `Vec<HistoryEntry>` with the newest entry first. Reads/writes are guarded
//! by a process-wide `Mutex` so concurrent recording stops (which run on the
//! Tokio runtime) cannot interleave and corrupt the file.

use crate::models::HistoryEntry;
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

/// Reads the full history from disk. Returns an empty vec if the file does
/// not exist yet or cannot be parsed (treated as a fresh start rather than a
/// hard error so a corrupted file never blocks the UI).
pub fn load_all() -> Vec<HistoryEntry> {
    let _guard = lock().lock();
    let Some(file) = path() else {
        return Vec::new();
    };
    match fs::read_to_string(file) {
        Ok(contents) if !contents.trim().is_empty() => {
            serde_json::from_str(&contents).unwrap_or_default()
        }
        _ => Vec::new(),
    }
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

    let mut entries: Vec<HistoryEntry> = match fs::read_to_string(file) {
        Ok(contents) if !contents.trim().is_empty() => {
            serde_json::from_str(&contents).unwrap_or_default()
        }
        _ => Vec::new(),
    };
    entries.insert(0, entry);

    if let Err(e) = fs::create_dir_all(file.parent().unwrap_or(file.as_path())) {
        log::error!("history: could not create data dir: {}", e);
        return;
    }
    match serde_json::to_string_pretty(&entries) {
        Ok(json) => {
            if let Err(e) = fs::write(file, json) {
                log::error!("history: failed to write file: {}", e);
            }
        }
        Err(e) => log::error!("history: failed to serialize: {}", e),
    }
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

    let mut entries: Vec<HistoryEntry> = match fs::read_to_string(file) {
        Ok(contents) if !contents.trim().is_empty() => {
            serde_json::from_str(&contents).unwrap_or_default()
        }
        _ => Vec::new(),
    };

    let mut found = false;
    for entry in entries.iter_mut() {
        if entry.id == id {
            entry.evaluation = Some(feedback.to_string());
            found = true;
            break;
        }
    }

    if found {
        match serde_json::to_string_pretty(&entries) {
            Ok(json) => {
                if let Err(e) = fs::write(file, json) {
                    log::error!("history: failed to write evaluation: {}", e);
                    return false;
                }
            }
            Err(e) => {
                log::error!("history: failed to serialize evaluation: {}", e);
                return false;
            }
        }
    }
    found
}

/// Empties the history file. Used by the "Limpar Tudo" button.
pub fn clear() {
    let _guard = lock().lock();
    let Some(file) = path() else {
        return;
    };
    let entries: Vec<HistoryEntry> = match fs::read_to_string(file) {
        Ok(contents) if !contents.trim().is_empty() => {
            serde_json::from_str(&contents).unwrap_or_default()
        }
        _ => Vec::new(),
    };
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
    let mut entries: Vec<HistoryEntry> = match fs::read_to_string(file) {
        Ok(contents) if !contents.trim().is_empty() => {
            serde_json::from_str(&contents).unwrap_or_default()
        }
        _ => Vec::new(),
    };
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
    match serde_json::to_string_pretty(&entries) {
        Ok(json) => fs::write(file, json).is_ok(),
        Err(_) => false,
    }
}

/// Updates only the final text (and word count) of an entry — preserves evaluation.
pub fn update_text(id: &str, text: &str) -> bool {
    let _guard = lock().lock();
    let Some(file) = path() else {
        return false;
    };
    let mut entries: Vec<HistoryEntry> = match fs::read_to_string(file) {
        Ok(contents) if !contents.trim().is_empty() => {
            serde_json::from_str(&contents).unwrap_or_default()
        }
        _ => Vec::new(),
    };
    let mut found = false;
    for entry in entries.iter_mut() {
        if entry.id == id {
            entry.text = text.to_string();
            entry.words = text.split_whitespace().count();
            entry.is_error = Some(false);
            entry.error_message = None;
            found = true;
            break;
        }
    }
    if !found {
        return false;
    }
    match serde_json::to_string_pretty(&entries) {
        Ok(json) => fs::write(file, json).is_ok(),
        Err(_) => false,
    }
}

/// Updates an existing history entry with new details.
pub fn update_entry(updated: HistoryEntry) -> bool {
    let _guard = lock().lock();
    let Some(file) = path() else {
        return false;
    };

    let mut entries: Vec<HistoryEntry> = match fs::read_to_string(file) {
        Ok(contents) if !contents.trim().is_empty() => {
            serde_json::from_str(&contents).unwrap_or_default()
        }
        _ => Vec::new(),
    };

    let mut found = false;
    for entry in entries.iter_mut() {
        if entry.id == updated.id {
            *entry = updated.clone();
            found = true;
            break;
        }
    }

    if found {
        match serde_json::to_string_pretty(&entries) {
            Ok(json) => {
                if let Err(e) = fs::write(file, json) {
                    log::error!("history: failed to update entry: {}", e);
                    return false;
                }
            }
            Err(e) => {
                log::error!("history: failed to serialize updated entry: {}", e);
                return false;
            }
        }
    }
    found
}
