//! Gemini Files API: resumable upload, readiness polling, and delete.
//!
//! Every successful upload returns a [`RemoteFileGuard`] that deletes the remote
//! file on drop (best-effort async spawn) or via explicit [`RemoteFileGuard::cleanup`].

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::client::{
    http_client, require_api_key, API_ROOT, POLL_INTERVAL, TIMEOUT_DELETE, TIMEOUT_FILE_READY,
    TIMEOUT_POLL, TIMEOUT_UPLOAD, UPLOAD_ROOT,
};
use super::types::GeminiFileRef;

#[derive(Debug, Serialize)]
struct StartUploadBody {
    file: StartUploadFile,
}

#[derive(Debug, Serialize)]
struct StartUploadFile {
    display_name: String,
}

#[derive(Debug, Deserialize)]
struct FileResource {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    uri: Option<String>,
    #[serde(default, alias = "mimeType")]
    mime_type: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    error: Option<FileError>,
}

#[derive(Debug, Deserialize)]
struct FileError {
    #[serde(default)]
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FileGetResponse {
    #[serde(flatten)]
    file: FileResource,
}

/// RAII guard around a remote Gemini file. Ensures delete on success, error,
/// timeout and cancellation paths when [`cleanup`](Self::cleanup) or `Drop` runs.
pub struct RemoteFileGuard {
    api_key: String,
    name: String,
    uri: String,
    mime_type: String,
    cleaned: Arc<AtomicBool>,
}

impl RemoteFileGuard {
    pub fn file_ref(&self) -> GeminiFileRef {
        GeminiFileRef {
            name: self.name.clone(),
            uri: self.uri.clone(),
            mime_type: self.mime_type.clone(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn uri(&self) -> &str {
        &self.uri
    }

    pub fn mime_type(&self) -> &str {
        &self.mime_type
    }

    /// Explicit async cleanup (preferred over Drop when in an async context).
    pub async fn cleanup(self) {
        if self
            .cleaned
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            let _ = delete_file(&self.api_key, &self.name).await;
        }
    }

    /// Prevent automatic delete (tests / intentional keep). Prefer not using in prod.
    #[cfg(test)]
    pub fn disarm(self) {
        self.cleaned.store(true, Ordering::Release);
        // leak name intentionally
        std::mem::forget(self);
    }
}

impl Drop for RemoteFileGuard {
    fn drop(&mut self) {
        if self
            .cleaned
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let api_key = self.api_key.clone();
        let name = self.name.clone();
        // Best-effort: fire-and-forget on the Tauri/Tokio runtime if available.
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let _ = delete_file(&api_key, &name).await;
            });
        } else {
            // No runtime: spawn a detached thread with its own runtime slice.
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();
                if let Ok(rt) = rt {
                    let _ = rt.block_on(delete_file(&api_key, &name));
                }
            });
        }
    }
}

/// Timing for a Files API upload + readiness wait.
#[derive(Debug, Clone, Copy, Default)]
pub struct UploadTiming {
    pub upload_ms: u64,
    pub poll_ms: u64,
    pub poll_count: u32,
}

/// Uploads audio bytes, waits until ACTIVE, returns a guard that deletes on drop.
pub async fn upload_and_wait(
    api_key: &str,
    bytes: &[u8],
    mime: &str,
    display_name: &str,
) -> Result<(RemoteFileGuard, UploadTiming), String> {
    require_api_key(api_key)?;
    if bytes.is_empty() {
        return Err("áudio vazio; não é possível enviar ao Gemini Files API".to_string());
    }

    let t_up = std::time::Instant::now();
    let upload_url = start_resumable_upload(api_key, bytes.len(), mime, display_name).await?;
    let file = upload_bytes(&upload_url, bytes, mime).await?;
    let upload_ms = t_up.elapsed().as_millis() as u64;

    let name = file
        .name
        .clone()
        .ok_or_else(|| "resposta de upload sem name do arquivo".to_string())?;
    let uri = file
        .uri
        .clone()
        .unwrap_or_else(|| format!("{}/{}", API_ROOT, name));
    let mime_type = file.mime_type.clone().unwrap_or_else(|| mime.to_string());

    let guard = RemoteFileGuard {
        api_key: api_key.to_string(),
        name: name.clone(),
        uri,
        mime_type,
        cleaned: Arc::new(AtomicBool::new(false)),
    };

    // If polling fails, drop will still delete.
    let (poll_ms, poll_count) = wait_until_active(api_key, &name).await?;
    Ok((
        guard,
        UploadTiming {
            upload_ms,
            poll_ms,
            poll_count,
        },
    ))
}

/// Fire-and-forget remote delete (not on the transcription critical path).
pub fn spawn_cleanup(guard: RemoteFileGuard) {
    tokio::spawn(async move {
        let t0 = std::time::Instant::now();
        let name = guard.name().to_string();
        guard.cleanup().await;
        log::info!(
            "gemini files: async cleanup finished name={} delete_ms={}",
            name,
            t0.elapsed().as_millis()
        );
    });
}

async fn start_resumable_upload(
    api_key: &str,
    num_bytes: usize,
    mime: &str,
    display_name: &str,
) -> Result<String, String> {
    let client = http_client()?;
    let url = format!("{}/files", UPLOAD_ROOT);
    let body = StartUploadBody {
        file: StartUploadFile {
            display_name: display_name.to_string(),
        },
    };

    let response = tokio::time::timeout(
        TIMEOUT_UPLOAD,
        client
            .post(url)
            .header("x-goog-api-key", api_key)
            .header("X-Goog-Upload-Protocol", "resumable")
            .header("X-Goog-Upload-Command", "start")
            .header("X-Goog-Upload-Header-Content-Length", num_bytes.to_string())
            .header("X-Goog-Upload-Header-Content-Type", mime)
            .header("Content-Type", "application/json")
            .json(&body)
            .send(),
    )
    .await
    .map_err(|_| {
        format!(
            "timeout ao iniciar upload no Gemini ({}s)",
            TIMEOUT_UPLOAD.as_secs()
        )
    })?
    .map_err(|e| format!("falha ao iniciar upload no Gemini: {}", e.without_url()))?;

    let status = response.status().as_u16();
    if status != 200 {
        let t = response.text().await.unwrap_or_default();
        return Err(format!(
            "Gemini Files start upload status {}: {}",
            status, t
        ));
    }

    response
        .headers()
        .get("X-Goog-Upload-URL")
        .or_else(|| response.headers().get("x-goog-upload-url"))
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .ok_or_else(|| "Gemini não retornou X-Goog-Upload-URL".to_string())
}

async fn upload_bytes(upload_url: &str, bytes: &[u8], mime: &str) -> Result<FileResource, String> {
    let client = http_client()?;
    let response = tokio::time::timeout(
        TIMEOUT_UPLOAD,
        client
            .post(upload_url)
            .header("Content-Length", bytes.len().to_string())
            .header("X-Goog-Upload-Offset", "0")
            .header("X-Goog-Upload-Command", "upload, finalize")
            .header("Content-Type", mime)
            .body(bytes.to_vec())
            .send(),
    )
    .await
    .map_err(|_| {
        format!(
            "timeout ao enviar bytes ao Gemini ({}s)",
            TIMEOUT_UPLOAD.as_secs()
        )
    })?
    .map_err(|e| format!("falha ao enviar bytes ao Gemini: {}", e.without_url()))?;

    let status = response.status().as_u16();
    let body = response.text().await.unwrap_or_default();
    if status != 200 {
        return Err(format!(
            "Gemini Files finalize upload status {}: {}",
            status, body
        ));
    }

    // Response may be `{ "file": { ... } }` or the file object itself.
    if let Ok(wrapped) = serde_json::from_str::<serde_json::Value>(&body) {
        if let Some(file) = wrapped.get("file") {
            return serde_json::from_value(file.clone())
                .map_err(|e| format!("parse file resource: {}", e));
        }
    }
    serde_json::from_str(&body).map_err(|e| format!("parse file resource: {}", e))
}

async fn get_file(api_key: &str, name: &str) -> Result<FileResource, String> {
    let client = http_client()?;
    // name is like "files/abc123"
    let url = format!("{}/{}", API_ROOT, name);
    let response = tokio::time::timeout(
        TIMEOUT_POLL,
        client.get(url).header("x-goog-api-key", api_key).send(),
    )
    .await
    .map_err(|_| {
        format!(
            "timeout ao consultar arquivo no Gemini ({}s)",
            TIMEOUT_POLL.as_secs()
        )
    })?
    .map_err(|e| format!("falha ao consultar arquivo no Gemini: {}", e.without_url()))?;

    let status = response.status().as_u16();
    let body = response.text().await.unwrap_or_default();
    if status != 200 {
        return Err(format!("Gemini get file status {}: {}", status, body));
    }

    if let Ok(wrapped) = serde_json::from_str::<serde_json::Value>(&body) {
        if let Some(file) = wrapped.get("file") {
            return serde_json::from_value::<FileResource>(file.clone())
                .map_err(|e| format!("parse get file: {}", e));
        }
    }
    serde_json::from_str::<FileGetResponse>(&body)
        .map(|r| r.file)
        .or_else(|_| serde_json::from_str::<FileResource>(&body))
        .map_err(|e| format!("parse get file: {}", e))
}

/// Returns `(poll_wall_ms, poll_count)`.
async fn wait_until_active(api_key: &str, name: &str) -> Result<(u64, u32), String> {
    let deadline = std::time::Instant::now() + TIMEOUT_FILE_READY;
    let t0 = std::time::Instant::now();
    let mut poll_count = 0u32;
    loop {
        poll_count = poll_count.saturating_add(1);
        let file = get_file(api_key, name).await?;
        let state = file.state.as_deref().unwrap_or("").to_ascii_uppercase();
        if state == "ACTIVE" || state.is_empty() {
            // Some responses omit state when already usable.
            if state == "ACTIVE" || file.uri.is_some() {
                return Ok((t0.elapsed().as_millis() as u64, poll_count));
            }
        }
        if state == "FAILED" {
            let msg = file
                .error
                .and_then(|e| e.message)
                .unwrap_or_else(|| "processamento falhou".into());
            return Err(format!(
                "Gemini Files API falhou ao processar áudio: {}",
                msg
            ));
        }
        if std::time::Instant::now() >= deadline {
            return Err(format!(
                "timeout aguardando arquivo ACTIVE no Gemini ({}s)",
                TIMEOUT_FILE_READY.as_secs()
            ));
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

pub async fn delete_file(api_key: &str, name: &str) -> Result<(), String> {
    if api_key.trim().is_empty() || name.trim().is_empty() {
        return Ok(());
    }
    let client = match http_client() {
        Ok(c) => c,
        Err(_) => return Ok(()),
    };
    let url = format!("{}/{}", API_ROOT, name);
    match tokio::time::timeout(
        TIMEOUT_DELETE,
        client.delete(url).header("x-goog-api-key", api_key).send(),
    )
    .await
    {
        Ok(Ok(resp)) => {
            let status = resp.status().as_u16();
            // 200 / 204 / 404 are all acceptable cleanup outcomes.
            if status == 200 || status == 204 || status == 404 {
                log::info!("gemini files: deleted {} (status {})", name, status);
                Ok(())
            } else {
                let t = resp.text().await.unwrap_or_default();
                log::warn!("gemini files: delete {} status {}: {}", name, status, t);
                Err(format!("delete status {}: {}", status, t))
            }
        }
        Ok(Err(e)) => {
            let e = e.without_url();
            log::warn!("gemini files: delete {} network error: {}", name, e);
            Err(e.to_string())
        }
        Err(_) => {
            log::warn!(
                "gemini files: delete {} timed out after {}s",
                name,
                TIMEOUT_DELETE.as_secs()
            );
            Err("delete timeout".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_wrapped_and_flat_file() {
        let wrapped = r#"{
            "file": {
                "name": "files/abc",
                "uri": "https://generativelanguage.googleapis.com/v1beta/files/abc",
                "mimeType": "audio/wav",
                "state": "ACTIVE"
            }
        }"#;
        let v: serde_json::Value = serde_json::from_str(wrapped).unwrap();
        assert_eq!(v["file"]["name"], "files/abc");
        let file: FileResource = serde_json::from_value(v["file"].clone()).unwrap();
        assert_eq!(file.name.as_deref(), Some("files/abc"));
        assert_eq!(file.mime_type.as_deref(), Some("audio/wav"));
        assert_eq!(file.state.as_deref(), Some("ACTIVE"));
    }

    #[test]
    fn parse_file_with_aliases() {
        let j = r#"{"name":"files/x","mimeType":"audio/wav","state":"PROCESSING"}"#;
        let f: FileResource = serde_json::from_str(j).unwrap();
        assert_eq!(f.name.as_deref(), Some("files/x"));
        assert_eq!(f.mime_type.as_deref(), Some("audio/wav"));
        assert_eq!(f.state.as_deref(), Some("PROCESSING"));
    }
}
