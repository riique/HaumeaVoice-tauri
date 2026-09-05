//! Bounded callback queue and incremental, recoverable PCM WAV capture.
use std::{
    fs::{self, File},
    io::{Seek, SeekFrom, Write},
    path::PathBuf,
    sync::mpsc::{sync_channel, SyncSender},
    thread::JoinHandle,
};
pub const MAX_CAPTURE_SECONDS: u64 = 15 * 60;
pub struct CaptureSpool {
    sender: Option<SyncSender<Vec<i16>>>,
    worker: Option<JoinHandle<Result<PathBuf, String>>>,
}
impl CaptureSpool {
    pub fn start(directory: PathBuf, id: &str, rate: u32) -> Result<Self, String> {
        if !(8_000..=192_000).contains(&rate) {
            return Err("Taxa de captura não suportada".into());
        }
        fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        let path = directory.join(format!("recovery-{id}.wav"));
        let mut file = File::options()
            .create_new(true)
            .write(true)
            .open(&path)
            .map_err(|e| e.to_string())?;
        file.write_all(&header(rate, 0))
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        let (sender, receiver) = sync_channel::<Vec<i16>>(64);
        let worker = std::thread::spawn(move || {
            let mut bytes = 0u32;
            let mut synced = std::time::Instant::now();
            for samples in receiver {
                let data: Vec<u8> = samples.into_iter().flat_map(i16::to_le_bytes).collect();
                bytes = bytes
                    .checked_add(data.len() as u32)
                    .ok_or("Áudio excede o limite")?;
                if u64::from(bytes) > u64::from(rate) * MAX_CAPTURE_SECONDS * 2 {
                    return Err("Limite de 15 minutos atingido; áudio preservado".into());
                }
                file.seek(SeekFrom::End(0))
                    .and_then(|_| file.write_all(&data))
                    .map_err(|e| e.to_string())?;
                file.seek(SeekFrom::Start(0))
                    .and_then(|_| file.write_all(&header(rate, bytes)))
                    .map_err(|e| e.to_string())?;
                if synced.elapsed().as_secs() >= 1 {
                    file.sync_data().map_err(|e| e.to_string())?;
                    synced = std::time::Instant::now();
                }
            }
            file.sync_all().map_err(|e| e.to_string())?;
            Ok(path)
        });
        Ok(Self {
            sender: Some(sender),
            worker: Some(worker),
        })
    }
    pub fn push(&self, samples: &[i16]) -> Result<(), String> {
        if samples.len() > 192_000 {
            return Err("Bloco de captura excede o limite".into());
        }
        self.sender
            .as_ref()
            .ok_or("Captura encerrada")?
            .try_send(samples.to_vec())
            .map_err(|_| {
                "O armazenamento não acompanhou a captura. Áudio parcial preservado.".into()
            })
    }
    pub fn finish(mut self) -> Result<PathBuf, String> {
        self.sender.take();
        self.worker
            .take()
            .ok_or("Captura encerrada")?
            .join()
            .map_err(|_| "Falha no gravador; áudio parcial preservado")?
    }
}
impl Drop for CaptureSpool {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn header(rate: u32, bytes: u32) -> Vec<u8> {
    let mut data = Vec::with_capacity(44);
    data.extend(b"RIFF");
    data.extend((36 + bytes).to_le_bytes());
    data.extend(b"WAVEfmt ");
    data.extend(16u32.to_le_bytes());
    data.extend(1u16.to_le_bytes());
    data.extend(1u16.to_le_bytes());
    data.extend(rate.to_le_bytes());
    data.extend((rate * 2).to_le_bytes());
    data.extend(2u16.to_le_bytes());
    data.extend(16u16.to_le_bytes());
    data.extend(b"data");
    data.extend(bytes.to_le_bytes());
    data
}
pub fn read_pcm(path: &std::path::Path) -> Result<Vec<i16>, String> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("Áudio de recuperação inválido".into());
    }
    if &bytes[12..16] != b"fmt "
        || bytes[16..20] != 16u32.to_le_bytes()
        || bytes[20..22] != 1u16.to_le_bytes()
        || bytes[22..24] != 1u16.to_le_bytes()
        || bytes[34..36] != 16u16.to_le_bytes()
        || &bytes[36..40] != b"data"
    {
        return Err("Formato de recuperação não é PCM mono de 16 bits".into());
    }
    // A crash may leave a stale length; complete PCM frames after the header remain recoverable.
    Ok(bytes[44..]
        .chunks_exact(2)
        .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
        .collect())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn incremental_capture_survives_without_transcription() {
        let directory = std::env::temp_dir().join(format!("sonora-spool-{}", std::process::id()));
        let spool = CaptureSpool::start(directory.clone(), "synthetic", 16000).unwrap();
        spool.push(&[1, -2, 300]).unwrap();
        spool.push(&[-100]).unwrap();
        let file = spool.finish().unwrap();
        assert_eq!(read_pcm(&file).unwrap(), vec![1, -2, 300, -100]);
        fs::remove_dir_all(directory).unwrap();
    }
}
