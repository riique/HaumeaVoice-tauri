//! Persistent API key storage.
//!
//! The API keys entered in the settings screen are persisted to a JSON file
//! (`api_keys.json`) inside the per-user Tauri app data directory so they
//! survive an app restart or a machine reboot. Reads/writes are guarded by a
//! process-wide `Mutex` so a save triggered from the UI cannot interleave with
//! the load performed during startup.
//!
//! Note: the file is stored under the user's protected AppData directory but
//! the keys themselves are written in plain text (no OS-keychain encryption).
//! This mirrors how `history.json` is handled and is appropriate for a local
//! single-user desktop app.

use crate::models::ApiKeys;
use parking_lot::Mutex;
use std::{fs, path::PathBuf, sync::OnceLock};

static SECRETS_PATH: OnceLock<PathBuf> = OnceLock::new();
static SECRETS_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

/// Called once during app setup with the resolved `api_keys.json` path so
/// every subsequent helper can read/write without threading the `AppHandle`
/// through the command layer.
pub fn init(file: PathBuf) {
    let _ = SECRETS_PATH.set(file);
    let _ = SECRETS_LOCK.set(Mutex::new(()));
}

fn path() -> Option<&'static PathBuf> {
    SECRETS_PATH.get()
}

fn lock() -> &'static Mutex<()> {
    SECRETS_LOCK.get_or_init(|| Mutex::new(()))
}

/// Reads the persisted API keys from disk. Returns the default (all `None`)
/// when the file is missing or cannot be parsed, so a corrupted file never
/// blocks startup.
pub fn load() -> ApiKeys {
    let _guard = lock().lock();
    let Some(file) = path() else {
        return ApiKeys::default();
    };
    match fs::read_to_string(file) {
        Ok(contents) if !contents.trim().is_empty() => {
            serde_json::from_str(&contents).unwrap_or_default()
        }
        _ => ApiKeys::default(),
    }
}

/// Persists `keys` to disk, replacing any previous contents. Failures are
/// logged but not propagated: the in-memory state has already been updated, so
/// a failed write only means the keys won't survive the next restart.
pub fn save(keys: &ApiKeys) {
    let _guard = lock().lock();
    let Some(file) = path() else {
        log::warn!("secrets: data directory not initialised, skipping save");
        return;
    };

    if let Err(e) = fs::create_dir_all(file.parent().unwrap_or(file.as_path())) {
        log::error!("secrets: could not create data dir: {}", e);
        return;
    }
    match serde_json::to_string_pretty(keys) {
        Ok(json) => {
            if let Err(e) = fs::write(file, json) {
                log::error!("secrets: failed to write file: {}", e);
            }
        }
        Err(e) => log::error!("secrets: failed to serialize: {}", e),
    }
}
