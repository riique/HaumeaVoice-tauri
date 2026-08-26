//! Small local-first capture surface for dictations that must not be pasted.

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, sync::OnceLock};

static PATH: OnceLock<PathBuf> = OnceLock::new();
static LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScratchpadNote {
    pub id: String,
    pub created_at_ms: u64,
    pub text: String,
    #[serde(default)]
    pub pipeline_run_id: Option<String>,
    #[serde(default)]
    pub profile_id: Option<String>,
}

pub fn init(data_dir: PathBuf) {
    let _ = PATH.set(data_dir.join("scratchpad.json"));
    let _ = LOCK.set(Mutex::new(()));
}

fn read_unlocked() -> Vec<ScratchpadNote> {
    PATH.get()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|contents| serde_json::from_str(&contents).ok())
        .unwrap_or_default()
}

fn write_unlocked(notes: &[ScratchpadNote]) -> Result<(), String> {
    let path = PATH.get().ok_or("scratchpad storage is not initialized")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let json = serde_json::to_string_pretty(notes).map_err(|error| error.to_string())?;
    fs::write(path, json).map_err(|error| error.to_string())
}

pub fn add(
    text: String,
    run_id: Option<String>,
    profile_id: Option<String>,
) -> Result<ScratchpadNote, String> {
    let _guard = LOCK.get_or_init(|| Mutex::new(())).lock();
    let now = crate::pipeline_run::epoch_ms();
    let note = ScratchpadNote {
        id: format!("scratch-{now}"),
        created_at_ms: now,
        text,
        pipeline_run_id: run_id,
        profile_id,
    };
    let mut notes = read_unlocked();
    notes.insert(0, note.clone());
    write_unlocked(&notes)?;
    Ok(note)
}

pub fn list() -> Vec<ScratchpadNote> {
    let _guard = LOCK.get_or_init(|| Mutex::new(())).lock();
    read_unlocked()
}

pub fn delete(id: &str) -> Result<bool, String> {
    let _guard = LOCK.get_or_init(|| Mutex::new(())).lock();
    let mut notes = read_unlocked();
    let before = notes.len();
    notes.retain(|note| note.id != id);
    if notes.len() == before {
        return Ok(false);
    }
    write_unlocked(&notes)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_contract_is_local_and_keeps_pipeline_link() {
        let note = ScratchpadNote {
            id: "n1".into(),
            created_at_ms: 1,
            text: "nota".into(),
            pipeline_run_id: Some("r1".into()),
            profile_id: Some("study".into()),
        };
        let value = serde_json::to_value(note).unwrap();
        assert_eq!(value["pipeline_run_id"], "r1");
    }
}
