//! Chrome/Chromium Native Messaging bridge for limited browser context.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::Duration;

use crate::context::BrowserContextEndpoint;

const MAX_NATIVE_MESSAGE_BYTES: usize = 64 * 1024;

pub fn is_native_messaging_invocation() -> bool {
    std::env::args()
        .nth(1)
        .is_some_and(|argument| argument.starts_with("chrome-extension://"))
}

fn browser_endpoint_path() -> Result<PathBuf, String> {
    let app_data = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| "APPDATA is unavailable".to_string())?;
    Ok(app_data
        .join("com.haumeavoice.app")
        .join("browser-context.endpoint"))
}

pub fn run_native_messaging_host() -> Result<(), String> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut reader = stdin.lock();
    let mut writer = stdout.lock();

    loop {
        let mut length_bytes = [0_u8; 4];
        match reader.read_exact(&mut length_bytes) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(error) => return Err(error.to_string()),
        }
        let length = u32::from_le_bytes(length_bytes) as usize;
        if length == 0 || length > MAX_NATIVE_MESSAGE_BYTES {
            write_response(&mut writer, false, "invalid_message_size")?;
            return Err("invalid native message size; stream closed to preserve framing".into());
        }
        let mut payload = vec![0_u8; length];
        reader
            .read_exact(&mut payload)
            .map_err(|error| error.to_string())?;
        let message: serde_json::Value = match serde_json::from_slice(&payload) {
            Ok(message) => message,
            Err(_) => {
                write_response(&mut writer, false, "invalid_json")?;
                continue;
            }
        };
        match forward_context(message) {
            Ok(response) => {
                let bytes = serde_json::to_vec(&response).map_err(|_| "invalid response")?;
                writer
                    .write_all(&(bytes.len() as u32).to_le_bytes())
                    .and_then(|_| writer.write_all(&bytes))
                    .and_then(|_| writer.flush())
                    .map_err(|e| e.to_string())?;
            }
            Err(error) => {
                log::debug!("native messaging: app IPC unavailable: {error}");
                write_response(&mut writer, false, "app_unavailable")?;
            }
        }
    }
}

fn forward_context(mut message: serde_json::Value) -> Result<serde_json::Value, String> {
    let endpoint_bytes =
        std::fs::read(browser_endpoint_path()?).map_err(|error| error.to_string())?;
    let endpoint: BrowserContextEndpoint =
        serde_json::from_slice(&endpoint_bytes).map_err(|error| error.to_string())?;
    let address: std::net::SocketAddr = endpoint
        .address
        .parse()
        .map_err(|error: std::net::AddrParseError| error.to_string())?;
    if !address.ip().is_loopback() {
        return Err("invalid endpoint address".into());
    }
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(1))
        .map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .and_then(|_| stream.set_write_timeout(Some(Duration::from_secs(2))))
        .map_err(|error| error.to_string())?;
    if !message.is_object() {
        return Err("invalid message".into());
    }
    message["token"] = endpoint.token.into();
    let payload = serde_json::to_vec(&message).map_err(|_| "invalid message")?;
    if payload.len() > MAX_NATIVE_MESSAGE_BYTES {
        return Err("browser context exceeds IPC limit".into());
    }
    stream
        .write_all(&(payload.len() as u32).to_le_bytes())
        .and_then(|_| stream.write_all(&payload))
        .and_then(|_| stream.flush())
        .map_err(|error| error.to_string())?;
    let mut length_bytes = [0_u8; 4];
    stream
        .read_exact(&mut length_bytes)
        .map_err(|error| error.to_string())?;
    let response_length = u32::from_le_bytes(length_bytes) as usize;
    if response_length == 0 || response_length > 1024 {
        return Err("invalid app IPC response".into());
    }
    let mut response = vec![0_u8; response_length];
    stream
        .read_exact(&mut response)
        .map_err(|error| error.to_string())?;
    let response: serde_json::Value =
        serde_json::from_slice(&response).map_err(|error| error.to_string())?;
    if response["ok"] == true {
        Ok(response)
    } else {
        Err("app rejected browser context".into())
    }
}

fn write_response(writer: &mut impl Write, ok: bool, status: &str) -> Result<(), String> {
    let payload = serde_json::to_vec(&serde_json::json!({ "ok": ok, "status": status }))
        .map_err(|error| error.to_string())?;
    writer
        .write_all(&(payload.len() as u32).to_le_bytes())
        .and_then(|_| writer.write_all(&payload))
        .and_then(|_| writer.flush())
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_uses_native_messaging_length_prefix() {
        let mut bytes = Vec::new();
        write_response(&mut bytes, true, "stored").unwrap();
        let length = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
        assert_eq!(length, bytes.len() - 4);
        let value: serde_json::Value = serde_json::from_slice(&bytes[4..]).unwrap();
        assert_eq!(value["ok"], true);
    }
}
