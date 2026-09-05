//! Durable local writes. Never replace malformed JSON with an empty store.
use serde::{de::DeserializeOwned, Serialize};
use std::{
    fs,
    io::Write,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub fn read_json<T: DeserializeOwned + Default>(path: &Path) -> Result<T, String> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|_| "Arquivo de dados inválido. Preserve o arquivo e restaure um backup antes de salvar.".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(error) => Err(format!("Não foi possível ler os dados: {error}")),
    }
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("Diretório de dados inválido")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temp = parent.join(format!(
        ".sonora-write-{}-{}.tmp",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        drop(file);
        // The backup itself is replaced atomically; an interrupted write leaves
        // either the previous complete value or the new complete value.
        if path.exists() {
            let backup = path.with_extension("bak");
            let backup_temp = temp.with_extension("bak.tmp");
            fs::copy(path, &backup_temp).map_err(|e| e.to_string())?;
            fs::OpenOptions::new()
                .write(true)
                .open(&backup_temp)
                .and_then(|f| f.sync_all())
                .map_err(|e| e.to_string())?;
            fs::rename(&backup_temp, backup).map_err(|e| e.to_string())?;
        }
        fs::rename(&temp, path).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

pub fn write_json<T: Serialize + ?Sized>(path: &Path, value: &T) -> Result<(), String> {
    if path.exists() {
        let _: serde_json::Value = read_json(path)?;
    }
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    atomic_write(path, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn corrupt_store_is_preserved_and_previous_write_is_recoverable() {
        let dir = std::env::temp_dir().join(format!(
            "sonora-storage-test-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("data.json");
        write_json(&file, &vec![1]).unwrap();
        write_json(&file, &vec![2]).unwrap();
        assert_eq!(
            read_json::<Vec<i32>>(&file.with_extension("bak")).unwrap(),
            vec![1]
        );
        fs::write(&file, b"broken").unwrap();
        assert!(write_json(&file, &vec![3]).is_err());
        assert_eq!(fs::read(&file).unwrap(), b"broken");
        fs::remove_dir_all(dir).unwrap();
    }
}
