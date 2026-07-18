//! Permanent on-disk storage for the audio that produced each transcription.
//!
//! Every transcription — whether captured from the microphone or uploaded as a
//! file — has its source audio copied into `{app_data_dir}/audio/{id}.{ext}`.
//! The resulting path is stored on the matching [`crate::models::HistoryEntry`]
//! so the pronunciation evaluator can later re-read the exact audio bytes and
//! hand them to Gemini.
//!
//! Like the `history` and `secrets` modules, the directory is resolved once
//! during setup and access is guarded by a process-wide `Mutex`.

use parking_lot::Mutex;
use std::{fs, path::PathBuf, sync::OnceLock};

static AUDIO_DIR: OnceLock<PathBuf> = OnceLock::new();
static AUDIO_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

/// Called once during app setup with the `audio` sub-directory of the app data
/// directory. Creates the directory eagerly so the first save never races on a
/// missing parent.
pub fn init(dir: PathBuf) {
    let _ = fs::create_dir_all(&dir);
    let _ = AUDIO_DIR.set(dir);
    let _ = AUDIO_LOCK.set(Mutex::new(()));
}

fn dir() -> Option<&'static PathBuf> {
    AUDIO_DIR.get()
}

fn lock() -> &'static Mutex<()> {
    AUDIO_LOCK.get_or_init(|| Mutex::new(()))
}

/// Persists `bytes` as `{id}.{ext}` inside the audio directory and returns the
/// absolute path as a `String`. Returns `None` if the store was never
/// initialised or the write failed (the caller treats audio persistence as
/// best-effort: a failure must not abort the transcription itself).
pub fn save(id: &str, ext: &str, bytes: &[u8]) -> Option<String> {
    let _guard = lock().lock();
    let dir = dir()?;
    if let Err(e) = fs::create_dir_all(dir) {
        log::error!("audio_store: could not create audio dir: {}", e);
        return None;
    }
    let safe_ext = sanitize_ext(ext);
    let file = dir.join(format!("{}.{}", id, safe_ext));
    match fs::write(&file, bytes) {
        Ok(()) => {
            log::info!("audio_store: saved {} bytes to {:?}", bytes.len(), file);
            Some(file.to_string_lossy().into_owned())
        }
        Err(e) => {
            log::error!("audio_store: failed to write audio file: {}", e);
            None
        }
    }
}

/// Reads the audio bytes previously persisted at `path`. Returns an error
/// string (rather than panicking) so the evaluation command can surface a
/// readable diagnostic to the UI.
pub fn read(path: &str) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|e| format!("could not read saved audio at {}: {}", path, e))
}

/// Removes every file in the audio directory. Called alongside
/// `history::clear` so wiping the history also reclaims the disk space.
pub fn clear() {
    let _guard = lock().lock();
    let Some(dir) = dir() else {
        return;
    };
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Maps a file extension to the MIME type sent to the transcription engines.
/// Defaults to a generic binary type for unknown extensions (the cloud APIs
/// still attempt container auto-detection in that case).
pub fn mime_for_ext(ext: &str) -> &'static str {
    match sanitize_ext(ext).as_str() {
        "wav" | "wave" => "audio/wav",
        "mp3" => "audio/mpeg",
        "m4a" | "mp4" | "aac" => "audio/mp4",
        "flac" => "audio/flac",
        "ogg" | "oga" => "audio/ogg",
        "webm" => "audio/webm",
        _ => "application/octet-stream",
    }
}

/// Strips anything that is not an ASCII alphanumeric from the extension and
/// lower-cases it, defaulting to `bin` when empty. Guards against a malicious
/// or malformed upload filename injecting path separators into the saved name.
fn sanitize_ext(ext: &str) -> String {
    let cleaned: String = ext
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_lowercase();
    if cleaned.is_empty() {
        "bin".to_string()
    } else {
        cleaned
    }
}
