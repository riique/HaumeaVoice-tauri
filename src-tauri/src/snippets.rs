//! Deterministic, local voice snippets resolved after model processing.

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, sync::OnceLock};

static PATH: OnceLock<PathBuf> = OnceLock::new();
static LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceSnippet {
    pub id: String,
    pub trigger: String,
    pub expansion: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub require_activation_phrase: bool,
}

fn default_true() -> bool {
    true
}

pub fn init(data_dir: PathBuf) {
    let _ = PATH.set(data_dir.join("snippets.json"));
    let _ = LOCK.set(Mutex::new(()));
}

fn read_unlocked() -> Result<Vec<VoiceSnippet>, String> {
    crate::storage::read_json(PATH.get().ok_or("Armazenamento indisponível")?)
}

pub fn list() -> Result<Vec<VoiceSnippet>, String> {
    let _config = crate::models::CONFIG_LOCK.lock();
    let _guard = LOCK.get_or_init(|| Mutex::new(())).lock();
    read_unlocked()
}

pub fn replace(snippets: Vec<VoiceSnippet>) -> Result<Vec<VoiceSnippet>, String> {
    let _config = crate::models::CONFIG_LOCK.lock();
    let _guard = LOCK.get_or_init(|| Mutex::new(())).lock();
    validate(&snippets)?;
    let path = PATH.get().ok_or("snippet storage is not initialized")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let json = serde_json::to_string_pretty(&snippets).map_err(|error| error.to_string())?;
    crate::storage::write_json(
        path,
        &serde_json::from_str::<serde_json::Value>(&json).map_err(|error| error.to_string())?,
    )?;
    Ok(snippets)
}

fn validate(snippets: &[VoiceSnippet]) -> Result<(), String> {
    let mut triggers = std::collections::HashSet::new();
    for snippet in snippets {
        let trigger = normalize(&snippet.trigger);
        if snippet.id.trim().is_empty() || trigger.is_empty() || snippet.expansion.is_empty() {
            return Err("snippet id, trigger and expansion must be non-empty".into());
        }
        if !triggers.insert(trigger) {
            return Err("snippet triggers must be unique".into());
        }
    }
    Ok(())
}

pub fn resolve(text: &str, snippets: &[VoiceSnippet]) -> Option<(String, String)> {
    let spoken = normalize(text.trim_matches(|c: char| matches!(c, '.' | '!' | '?' | ',' | ';')));
    snippets.iter().find_map(|snippet| {
        if !snippet.enabled {
            return None;
        }
        let trigger = normalize(&snippet.trigger);
        let explicit = format!("snippet {trigger}");
        let expand = format!("expandir {trigger}");
        let matches = if snippet.require_activation_phrase {
            spoken == explicit || spoken == expand
        } else {
            spoken == trigger || spoken == explicit || spoken == expand
        };
        matches.then(|| (snippet.expansion.clone(), snippet.id.clone()))
    })
}

fn normalize(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn github() -> VoiceSnippet {
        VoiceSnippet {
            id: "github".into(),
            trigger: "meu github".into(),
            expansion: "https://github.com/example".into(),
            enabled: true,
            require_activation_phrase: false,
        }
    }

    #[test]
    fn exact_trigger_expands_and_preserves_literal_value() {
        assert_eq!(
            resolve("Meu GitHub.", &[github()]).unwrap().0,
            "https://github.com/example"
        );
    }

    #[test]
    fn normal_sentence_is_not_a_false_positive() {
        assert!(resolve("acesse meu github quando puder", &[github()]).is_none());
    }

    #[test]
    fn explicit_activation_can_be_required() {
        let mut snippet = github();
        snippet.require_activation_phrase = true;
        assert!(resolve("meu github", &[snippet.clone()]).is_none());
        assert!(resolve("snippet meu github", &[snippet]).is_some());
    }
}
