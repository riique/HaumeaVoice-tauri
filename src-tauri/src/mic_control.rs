//! Automatic microphone unmute via the Windows Core Audio API.
//!
//! When the user triggers a recording, [`ensure_mic_unmuted`] is called
//! **before** the cpal capture stream is opened. It enumerates **every**
//! active capture (recording) endpoint and force-unmutes each one so the
//! very first audio sample is captured regardless of which device the
//! user has configured or which device Windows considers the "default".
//!
//! ## Why enumerate all endpoints?
//!
//! The user may have the Logitech G733 configured as their input device
//! in the app settings while Windows considers a different device (e.g.
//! a webcam mic) the "default".  By iterating over **all** active capture
//! endpoints we guarantee the G733's endpoint is reached.
//!
//! ## Why always call SetMute(FALSE)?
//!
//! Some USB headsets do not report their hardware mute state through
//! `IAudioEndpointVolume::GetMute()`.  The call returns `FALSE` even
//! though the device is effectively muted at a different layer.  By
//! unconditionally calling `SetMute(FALSE)` on every endpoint we cover
//! both the "reported mute" and "unreported mute" cases.
//!
//! ## Limitations
//!
//! A **hardware** mute (e.g. the G733 boom physically flipped up) is a
//! mechanical switch that software cannot override.  If the hardware mute
//! is engaged, `SetMute(FALSE)` may succeed at the API level but the
//! device will still produce silence.

/// Force-unmutes every active recording endpoint on the system.
///
/// Returns `true` if at least one endpoint was muted (and has been
/// unmutated), `false` otherwise.  On non-Windows platforms this is a
/// no-op returning `false`.
#[cfg(target_os = "windows")]
pub fn ensure_mic_unmuted() -> bool {
    use windows::Win32::Foundation::BOOL;
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{
        eCapture, eConsole, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
    };

    log::info!("mic_control: ensure_mic_unmuted() called");

    // Initialise COM on this thread.
    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let needs_uninit = hr.is_ok();
    if !needs_uninit {
        log::warn!("mic_control: CoInitializeEx returned {:?} — COM already initialised with different model", hr);
    }

    let result = (|| -> windows::core::Result<bool> {
        let enumerator: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };

        // Enumerate ALL active capture endpoints so we cover the configured
        // device even when it is not the Windows default.
        let collection = unsafe { enumerator.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE)? };
        let count = unsafe { collection.GetCount()? };
        log::info!("mic_control: found {} active capture endpoint(s)", count);

        let mut any_was_muted = false;

        for i in 0..count {
            let device = unsafe { collection.Item(i)? };

            // Log the device ID for diagnostics.
            let device_id = match unsafe { device.GetId() } {
                Ok(pwsz) => unsafe { pwsz.to_string() }.unwrap_or_else(|_| "<invalid>".to_string()),
                Err(_) => "<unknown>".to_string(),
            };

            let volume: IAudioEndpointVolume = match unsafe { device.Activate(CLSCTX_ALL, None) } {
                Ok(v) => v,
                Err(e) => {
                    log::warn!(
                        "mic_control: [{}] failed to activate IAudioEndpointVolume: {}",
                        device_id,
                        e
                    );
                    continue;
                }
            };

            // Check mute state before.
            let muted_before = unsafe { volume.GetMute()? };
            log::info!(
                "mic_control: [{}] endpoint #{} — mute_before={}",
                device_id,
                i,
                muted_before.as_bool()
            );

            if muted_before.as_bool() {
                any_was_muted = true;
            }

            // ALWAYS call SetMute(FALSE) — even when GetMute reports FALSE.
            // Some devices (notably certain Logitech headsets) do not report
            // their mute state through IAudioEndpointVolume::GetMute but the
            // SetMute call still clears the underlying mute flag.
            match unsafe { volume.SetMute(BOOL::from(false), std::ptr::null()) } {
                Ok(()) => {
                    log::info!("mic_control: [{}] SetMute(FALSE) succeeded", device_id);
                }
                Err(e) => {
                    log::warn!("mic_control: [{}] SetMute(FALSE) failed: {}", device_id, e);
                }
            }

            // Check mute state after to verify.
            let muted_after = unsafe { volume.GetMute()? };
            log::info!(
                "mic_control: [{}] endpoint #{} — mute_after={}",
                device_id,
                i,
                muted_after.as_bool()
            );
        }

        // Also explicitly handle the default console capture endpoint, in
        // case EnumAudioEndpoints missed it for some edge-case reason.
        if let Ok(default_device) =
            unsafe { enumerator.GetDefaultAudioEndpoint(eCapture, eConsole) }
        {
            let volume: IAudioEndpointVolume =
                unsafe { default_device.Activate(CLSCTX_ALL, None)? };
            let muted = unsafe { volume.GetMute()? };
            if muted.as_bool() {
                log::info!("mic_control: default endpoint was still muted — force-unmuting");
                unsafe { volume.SetMute(BOOL::from(false), std::ptr::null())? };
                any_was_muted = true;
            }
        }

        if any_was_muted {
            log::info!("mic_control: at least one endpoint was muted and has been unmutated");
        } else {
            log::info!("mic_control: no endpoints reported as muted");
        }

        Ok(any_was_muted)
    })();

    if needs_uninit {
        unsafe { CoUninitialize() };
    }

    match result {
        Ok(was_muted) => was_muted,
        Err(e) => {
            log::warn!("mic_control: error during unmute procedure: {}", e);
            false
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn ensure_mic_unmuted() -> bool {
    false
}
