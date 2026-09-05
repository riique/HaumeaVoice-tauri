#[cfg(windows)]
pub struct Instance(windows::Win32::Foundation::HANDLE);
#[cfg(windows)]
impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
#[cfg(windows)]
pub fn acquire() -> Result<Option<Instance>, String> {
    use windows::{
        core::w,
        Win32::{
            Foundation::{GetLastError, ERROR_ALREADY_EXISTS},
            System::Threading::CreateMutexW,
            UI::WindowsAndMessaging::{FindWindowW, SetForegroundWindow, ShowWindow, SW_RESTORE},
        },
    };
    unsafe {
        let mutex = CreateMutexW(
            None,
            false,
            w!("Local\\HaumeaVoice.Tauri.SingleInstance.v1"),
        )
        .map_err(|e| e.to_string())?;
        let existing = GetLastError() == ERROR_ALREADY_EXISTS;
        let instance = Instance(mutex);
        if existing {
            if let Ok(window) = FindWindowW(None, w!("Sonora")) {
                let _ = ShowWindow(window, SW_RESTORE);
                let _ = SetForegroundWindow(window);
            }
            return Ok(None);
        }
        Ok(Some(instance))
    }
}
#[cfg(not(windows))]
pub fn acquire() -> Result<Option<()>, String> {
    Ok(Some(()))
}
