pub mod audio;
pub mod audio_store;
pub mod commands;
pub mod deepgram;
pub mod gemini;
pub mod groq;
pub mod history;
pub mod mic_control;
pub mod models;
pub mod openrouter;
pub mod pipeline_contract;
pub mod sanitizer_json;
pub mod secrets;
pub mod settings;
pub mod shortcuts;
pub mod transcription;
pub mod vocabulary;

use models::SharedState;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, AtomicIsize, AtomicU64, Ordering};
use std::sync::Arc;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};

/// Cached gadget HWND (raw pointer bits). `0` means unknown / invalid.
/// Refreshed when the gadget is created and whenever we successfully apply a
/// click-through toggle on the main thread. Used so the cursor watcher can do
/// pure Win32 hit-testing **off** the Tauri event loop.
#[cfg(target_os = "windows")]
static GADGET_HWND: AtomicIsize = AtomicIsize::new(0);

#[cfg(target_os = "windows")]
fn cache_gadget_hwnd(window: &tauri::WebviewWindow) {
    if let Ok(hwnd) = window.hwnd() {
        GADGET_HWND.store(hwnd.0 as isize, Ordering::Release);
    }
}

#[cfg(not(target_os = "windows"))]
fn cache_gadget_hwnd(_window: &tauri::WebviewWindow) {}

/// Logical size of the floating gadget window. Kept tight so the overlay
/// footprint is minimal — just enough to house the pill in any state.
const GADGET_W: f64 = 280.0;
const GADGET_H: f64 = 56.0;

#[derive(Clone, Copy, Debug)]
struct MonitorBounds {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    scale: f64,
}

/// Chooses the monitor that contains most of the requested gadget rectangle,
/// then clamps the complete overlay inside that monitor. When a saved monitor
/// no longer exists, the nearest current monitor wins instead of leaving the
/// gadget outside the virtual desktop.
fn clamp_gadget_placement(
    desired_x: i32,
    desired_y: i32,
    monitors: &[MonitorBounds],
) -> (i32, i32, u32, u32) {
    if monitors.is_empty() {
        return (desired_x, desired_y, GADGET_W as u32, GADGET_H as u32);
    }

    let overlap = |monitor: &MonitorBounds| -> i64 {
        let width = (GADGET_W * monitor.scale).round().max(1.0) as i64;
        let height = (GADGET_H * monitor.scale).round().max(1.0) as i64;
        let left = (desired_x as i64).max(monitor.x as i64);
        let top = (desired_y as i64).max(monitor.y as i64);
        let right = (desired_x as i64 + width).min(monitor.x as i64 + monitor.width as i64);
        let bottom = (desired_y as i64 + height).min(monitor.y as i64 + monitor.height as i64);
        (right - left).max(0) * (bottom - top).max(0)
    };

    let distance = |monitor: &MonitorBounds| -> i64 {
        let right = monitor.x as i64 + monitor.width as i64;
        let bottom = monitor.y as i64 + monitor.height as i64;
        let x = desired_x as i64;
        let y = desired_y as i64;
        let dx = if x < monitor.x as i64 {
            monitor.x as i64 - x
        } else if x > right {
            x - right
        } else {
            0
        };
        let dy = if y < monitor.y as i64 {
            monitor.y as i64 - y
        } else if y > bottom {
            y - bottom
        } else {
            0
        };
        dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy))
    };

    let best_overlap = monitors.iter().map(overlap).max().unwrap_or(0);
    let monitor = if best_overlap > 0 {
        monitors.iter().max_by_key(|monitor| overlap(monitor))
    } else {
        monitors.iter().min_by_key(|monitor| distance(monitor))
    }
    .expect("monitors is not empty");

    let width = (GADGET_W * monitor.scale).round().max(1.0) as u32;
    let height = (GADGET_H * monitor.scale).round().max(1.0) as u32;
    let margin = (8.0 * monitor.scale).round() as i32;
    let min_x = monitor.x.saturating_add(margin);
    let min_y = monitor.y.saturating_add(margin);
    let max_x = (monitor.x as i64 + monitor.width as i64 - width as i64 - margin as i64)
        .max(min_x as i64) as i32;
    let max_y = (monitor.y as i64 + monitor.height as i64 - height as i64 - margin as i64)
        .max(min_y as i64) as i32;

    (
        desired_x.clamp(min_x, max_x),
        desired_y.clamp(min_y, max_y),
        width,
        height,
    )
}

/// Creates the always-on-top floating gadget overlay. It loads the same bundle
/// as the main window but is told apart by its `"gadget"` label (the frontend
/// renders a different root for it). The window is transparent, frameless,
/// hidden from the taskbar and always on top. It is **not** click-through so
/// the user can drag it to reposition; the frontend marks the pill container
/// with `data-tauri-drag-region` to enable native window dragging.
///
/// Position persistence uses **physical** pixels (relative to the virtual
/// desktop origin) rather than logical pixels. This is the crucial fix for
/// multi-monitor setups: logical coordinates depend on the scale factor of
/// the monitor the window is being created on, which is the *primary*
/// monitor at creation time — so a position saved on a 1.5x DPI secondary
/// monitor would be reinterpreted against the 1.0x primary monitor and land
/// in the wrong spot. Physical pixels are scale-independent and therefore
/// round-trip reliably across any monitor configuration.
fn setup_gadget(app: &tauri::AppHandle) {
    let monitors: Vec<MonitorBounds> = app
        .available_monitors()
        .unwrap_or_default()
        .into_iter()
        .map(|monitor| MonitorBounds {
            x: monitor.position().x,
            y: monitor.position().y,
            width: monitor.size().width,
            height: monitor.size().height,
            scale: monitor.scale_factor(),
        })
        .collect();

    // Default position: top-center of the primary monitor (physical pixels).
    let default_pos = app.primary_monitor().ok().flatten().map(|monitor| {
        let scale = monitor.scale_factor();
        let phys_w = monitor.size().width as i32;
        let x = ((phys_w as f64) - (GADGET_W * scale)) / 2.0;
        (
            monitor.position().x.saturating_add(x as i32),
            monitor.position().y.saturating_add((18.0 * scale) as i32),
        )
    });

    // Prefer the persisted physical position; fall back to logical (legacy)
    // position converted to physical using the primary monitor's scale; finally
    // fall back to the computed default.
    let (desired_x, desired_y) =
        if let Some((x, y)) = crate::settings::load_gadget_physical_position() {
            (x, y)
        } else if let Some((lx, ly)) = crate::settings::load_gadget_position() {
            let scale = app
                .primary_monitor()
                .ok()
                .flatten()
                .map(|m| m.scale_factor())
                .unwrap_or(1.0);
            ((lx * scale) as i32, (ly * scale) as i32)
        } else if let Some((x, y)) = default_pos {
            (x, y)
        } else {
            (520, 18)
        };

    let (phys_x, phys_y, init_phys_w, init_phys_h) =
        clamp_gadget_placement(desired_x, desired_y, &monitors);

    let builder = WebviewWindowBuilder::new(app, "gadget", WebviewUrl::App("index.html".into()))
        .title("Haumea Gadget")
        .inner_size(GADGET_W, GADGET_H)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(false)
        .visible(true)
        .focused(false);

    match builder.build() {
        Ok(window) => {
            // Move the window to the selected monitor, then apply the inner
            // size using that monitor's DPI. Doing this *after* build avoids the builder's
            // logical-only position path, which is what was clamping the
            // gadget to the primary monitor on multi-monitor systems.
            use tauri::PhysicalPosition;
            let _ = window.set_position(PhysicalPosition::new(phys_x, phys_y));
            let _ = window.set_size(tauri::PhysicalSize::new(init_phys_w, init_phys_h));
            // Cache HWND so the cursor watcher can hit-test with pure Win32 APIs
            // off the Tauri event loop (the main fix for AppHang under Task Manager).
            cache_gadget_hwnd(&window);
            log::info!(
                "gadget: overlay window created at physical ({}, {})",
                phys_x,
                phys_y
            );
        }
        Err(e) => log::error!("gadget: failed to create overlay window: {}", e),
    }
}

/// Win32 hit-test: is the cursor over the gadget's visible pill?
///
/// Uses only thread-safe Win32 reads (`GetCursorPos`, `GetWindowRect`,
/// `GetDpiForWindow`) so this never touches Tauri's event loop. The pill rect
/// lives in logical pixels relative to the window origin and is scaled by the
/// window DPI. Until the frontend reports a rect, returns `true` (fully
/// interactive — fail-safe).
#[cfg(target_os = "windows")]
fn win32_cursor_inside_pill(hwnd_raw: isize, state: &SharedState) -> bool {
    use windows::Win32::Foundation::{HWND, POINT, RECT};
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetCursorPos, GetWindowRect, IsWindow, IsWindowVisible,
    };

    if hwnd_raw == 0 {
        return true;
    }
    let hwnd = HWND(hwnd_raw as *mut _);

    // SAFETY: HWND was captured from a live WebviewWindow; IsWindow validates
    // it before any further use. These Win32 getters are documented as safe to
    // call from any thread.
    unsafe {
        if !IsWindow(hwnd).as_bool() {
            GADGET_HWND.store(0, Ordering::Release);
            return true;
        }
        if !IsWindowVisible(hwnd).as_bool() {
            return true;
        }

        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return true;
        }
        let mut pt = POINT::default();
        if GetCursorPos(&mut pt).is_err() {
            return true;
        }

        let dpi = GetDpiForWindow(hwnd);
        let scale = if dpi == 0 { 1.0 } else { dpi as f64 / 96.0 };

        match *state.gadget_hit_rect.read() {
            None => true,
            Some(r) => {
                let left = rect.left as f64 + r.x * scale;
                let top = rect.top as f64 + r.y * scale;
                let right = left + r.width * scale;
                let bottom = top + r.height * scale;
                let cx = pt.x as f64;
                let cy = pt.y as f64;
                cx >= left && cx <= right && cy >= top && cy <= bottom
            }
        }
    }
}

/// Reasserts the native topmost Z-order without activating the gadget. The
/// asynchronous flag is important: this runs from the cursor worker and must
/// never synchronously wait on the Tauri/Windows UI thread (the old source of
/// AppHangB1 under focus storms).
///
/// It also recovers an overlay left entirely outside the virtual desktop after
/// a monitor is unplugged by moving it to the top-center of the primary work
/// area. A partially visible gadget is left alone so normal cross-monitor
/// dragging is not interrupted.
#[cfg(target_os = "windows")]
fn win32_keep_gadget_visible_and_topmost(hwnd_raw: isize) -> bool {
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONULL,
        MONITOR_DEFAULTTOPRIMARY,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowRect, IsWindow, SetWindowPos, HWND_TOPMOST, SWP_ASYNCWINDOWPOS, SWP_NOACTIVATE,
        SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
    };

    if hwnd_raw == 0 {
        return false;
    }
    let hwnd = HWND(hwnd_raw as *mut _);

    // SAFETY: the cached HWND is validated before use. SetWindowPos is queued
    // asynchronously when the owner belongs to another input thread, so this
    // worker cannot block the main event loop.
    unsafe {
        if !IsWindow(hwnd).as_bool() {
            GADGET_HWND.store(0, Ordering::Release);
            return false;
        }

        let visible_monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONULL);
        let mut x = 0;
        let mut y = 0;
        let mut flags =
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS | SWP_SHOWWINDOW;

        if visible_monitor.0.is_null() {
            let primary = MonitorFromWindow(hwnd, MONITOR_DEFAULTTOPRIMARY);
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            let mut rect = RECT::default();
            if !primary.0.is_null()
                && GetMonitorInfoW(primary, &mut info).as_bool()
                && GetWindowRect(hwnd, &mut rect).is_ok()
            {
                let width = (rect.right - rect.left).max(1);
                let work_width = (info.rcWork.right - info.rcWork.left).max(width);
                x = info.rcWork.left + (work_width - width) / 2;
                y = info.rcWork.top + 18;
                flags &= !SWP_NOMOVE;
                log::warn!(
                    "gadget: saved monitor disappeared; recovering overlay at ({}, {})",
                    x,
                    y
                );
            }
        }

        SetWindowPos(hwnd, HWND_TOPMOST, x, y, 0, 0, flags).is_ok()
    }
}

/// Returns the current foreground HWND as raw pointer bits. This is a cheap,
/// read-only Win32 query used by the gadget worker to react immediately when
/// the user switches to another application. A zero value is valid while the
/// desktop is transitioning between foreground windows.
#[cfg(target_os = "windows")]
fn win32_foreground_window_raw() -> isize {
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    // SAFETY: GetForegroundWindow takes no pointers and only returns a borrowed
    // HWND value owned by Windows. We never dereference the handle here.
    unsafe { GetForegroundWindow().0 as isize }
}

#[cfg(not(target_os = "windows"))]
fn win32_keep_gadget_visible_and_topmost(_hwnd_raw: isize) -> bool {
    false
}

static GADGET_POS_X: AtomicI32 = AtomicI32::new(0);
static GADGET_POS_Y: AtomicI32 = AtomicI32::new(0);
static GADGET_POS_VERSION: AtomicU64 = AtomicU64::new(0);
static GADGET_POS_WRITER_RUNNING: AtomicBool = AtomicBool::new(false);

/// Debounces per-pixel `Moved` events into one trailing background write. This
/// both preserves the exact final drag position and avoids spawning a thread
/// (or touching disk) for every few pixels of movement.
fn queue_gadget_position_save(x: i32, y: i32) {
    GADGET_POS_X.store(x, Ordering::Relaxed);
    GADGET_POS_Y.store(y, Ordering::Relaxed);
    GADGET_POS_VERSION.fetch_add(1, Ordering::AcqRel);

    if GADGET_POS_WRITER_RUNNING.swap(true, Ordering::AcqRel) {
        return;
    }

    std::thread::spawn(|| loop {
        let observed = GADGET_POS_VERSION.load(Ordering::Acquire);
        std::thread::sleep(std::time::Duration::from_millis(350));
        if GADGET_POS_VERSION.load(Ordering::Acquire) != observed {
            continue;
        }

        crate::settings::save_gadget_physical_position(
            GADGET_POS_X.load(Ordering::Relaxed),
            GADGET_POS_Y.load(Ordering::Relaxed),
        );

        GADGET_POS_WRITER_RUNNING.store(false, Ordering::Release);
        if GADGET_POS_VERSION.load(Ordering::Acquire) == observed {
            break;
        }

        // Close the tiny race between clearing the flag and checking the
        // version. If another event already started a writer, this one exits;
        // otherwise it reacquires ownership and persists the newer position.
        if GADGET_POS_WRITER_RUNNING.swap(true, Ordering::AcqRel) {
            break;
        }
    });
}

/// Spawns a lightweight background thread that keeps the gadget overlay
/// click-through everywhere except over its visible pill.
///
/// ## Why this design (AppHangB1 / "Não respondendo")
///
/// Earlier versions either:
/// 1. Called `set_ignore_cursor_events` from a worker (tao marshals
///    `SetWindowPos(SWP_FRAMECHANGED)` via synchronous `SendMessage` into the
///    WNDPROC on the main thread → stall when Windows throttles a background
///    overlay, e.g. when Task Manager steals focus), or
/// 2. Queued a full hit-test on the main thread every 100 ms (still hammers the
///    single shared event loop with HWND reads under focus storms).
///
/// Current approach:
/// - **Hit-test on the worker** with pure Win32 APIs + cached HWND (no Tauri
///   window calls per tick).
/// - **Main-thread work only when the ignore flag actually changes**, and at
///   most one pending toggle at a time.
/// - Cadence ~200 ms (imperceptible for a ~280×56 pill).
///
/// Until the first pill rect is reported the overlay stays fully interactive.
fn spawn_gadget_cursor_watcher_safe(app: tauri::AppHandle, state: SharedState) {
    let pending_toggle = Arc::new(AtomicBool::new(false));

    #[cfg(target_os = "windows")]
    log::info!(
        "gadget: native topmost keeper enabled (foreground-aware + 400ms fallback, async Win32 Z-order)"
    );

    std::thread::spawn(move || {
        let mut last_ignore: Option<bool> = None;
        #[cfg(target_os = "windows")]
        let mut last_foreground_hwnd = 0isize;
        let mut tick: u32 = 0;

        loop {
            std::thread::sleep(std::time::Duration::from_millis(200));
            tick = tick.wrapping_add(1);

            #[cfg(target_os = "windows")]
            {
                let mut hwnd_raw = GADGET_HWND.load(Ordering::Acquire);

                // Rare HWND re-resolve if cache is empty (window recreated, etc.).
                // Coalesced with the toggle pending flag so we never flood main.
                if hwnd_raw == 0 && tick % 5 == 0 && !pending_toggle.load(Ordering::Acquire) {
                    let app_resolve = app.clone();
                    let pending = pending_toggle.clone();
                    if pending.swap(true, Ordering::AcqRel) {
                        continue;
                    }
                    let queued = app.run_on_main_thread(move || {
                        if let Some(w) = app_resolve.get_webview_window("gadget") {
                            cache_gadget_hwnd(&w);
                        }
                        pending.store(false, Ordering::Release);
                    });
                    if queued.is_err() {
                        pending_toggle.store(false, Ordering::Release);
                    }
                    continue;
                }

                if hwnd_raw == 0 {
                    continue;
                }

                // Reassert as soon as the foreground application changes (at
                // most one 200 ms polling interval later), with a 400ms fallback
                // for topmost popups that do not become the foreground window.
                // Unlike Tauri's `set_always_on_top`, this does not change
                // window styles and never synchronously crosses into the UI
                // thread.
                let foreground_hwnd = win32_foreground_window_raw();
                let foreground_changed = foreground_hwnd != last_foreground_hwnd;
                if foreground_changed {
                    last_foreground_hwnd = foreground_hwnd;
                }
                if foreground_changed || tick % 2 == 0 {
                    let _ = win32_keep_gadget_visible_and_topmost(hwnd_raw);
                    hwnd_raw = GADGET_HWND.load(Ordering::Acquire);
                    if hwnd_raw == 0 {
                        last_ignore = None;
                        continue;
                    }
                }

                let inside = win32_cursor_inside_pill(hwnd_raw, &state);
                // If HWND was invalidated during hit-test, skip this tick.
                hwnd_raw = GADGET_HWND.load(Ordering::Acquire);
                if hwnd_raw == 0 {
                    last_ignore = None;
                    continue;
                }

                let want_ignore = !inside;
                if last_ignore == Some(want_ignore) {
                    continue;
                }

                // At most one pending HWND mutation on the event loop.
                if pending_toggle.swap(true, Ordering::AcqRel) {
                    continue;
                }

                last_ignore = Some(want_ignore);
                log::info!(
                    "gadget: cursor watcher toggle (ignore={} inside={})",
                    want_ignore,
                    inside
                );

                let app_for_toggle = app.clone();
                let pending_for_toggle = pending_toggle.clone();
                let pending_for_error = pending_toggle.clone();
                let queued = app.run_on_main_thread(move || {
                    if let Some(w) = app_for_toggle.get_webview_window("gadget") {
                        cache_gadget_hwnd(&w);
                        let _ = w.set_ignore_cursor_events(want_ignore);
                    }
                    pending_for_toggle.store(false, Ordering::Release);
                });
                if queued.is_err() {
                    pending_for_error.store(false, Ordering::Release);
                    // Allow a re-apply on the next edge if the queue failed.
                    last_ignore = None;
                }
            }

            #[cfg(not(target_os = "windows"))]
            {
                // Non-Windows fallback: infrequent main-thread evaluation only
                // when needed (same coalescing rules).
                if pending_toggle.swap(true, Ordering::AcqRel) {
                    continue;
                }
                let app_for_tick = app.clone();
                let state_for_tick = state.clone();
                let pending_for_tick = pending_toggle.clone();
                let pending_for_error = pending_toggle.clone();
                let last_snapshot = last_ignore;
                let queued = app.run_on_main_thread(move || {
                    let Some(window) = app_for_tick.get_webview_window("gadget") else {
                        pending_for_tick.store(false, Ordering::Release);
                        return;
                    };
                    if !window.is_visible().unwrap_or(false) {
                        pending_for_tick.store(false, Ordering::Release);
                        return;
                    }
                    let inside = match *state_for_tick.gadget_hit_rect.read() {
                        None => true,
                        Some(r) => {
                            match (app_for_tick.cursor_position(), window.outer_position()) {
                                (Ok(cursor), Ok(pos)) => {
                                    let scale = window.scale_factor().unwrap_or(1.0);
                                    let left = pos.x as f64 + r.x * scale;
                                    let top = pos.y as f64 + r.y * scale;
                                    let right = left + r.width * scale;
                                    let bottom = top + r.height * scale;
                                    cursor.x >= left
                                        && cursor.x <= right
                                        && cursor.y >= top
                                        && cursor.y <= bottom
                                }
                                _ => true,
                            }
                        }
                    };
                    let want_ignore = !inside;
                    if last_snapshot != Some(want_ignore) {
                        log::info!(
                            "gadget: cursor watcher toggle (ignore={} inside={})",
                            want_ignore,
                            inside
                        );
                        let _ = window.set_ignore_cursor_events(want_ignore);
                    }
                    pending_for_tick.store(false, Ordering::Release);
                });
                if queued.is_err() {
                    pending_for_error.store(false, Ordering::Release);
                }
                // Best-effort local tracking; next tick may re-apply.
                if last_ignore.is_none() {
                    last_ignore = Some(false);
                }
            }
        }
    });
}

/// Builds the system tray icon and its context menu. The app keeps running in
/// the background (window hidden) when closed, and the tray lets the user
/// restore the window or quit for good. Returns the built tray so it stays
/// alive for the lifetime of the app.
fn setup_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let show_item = MenuItem::with_id(app, "show", "Mostrar Haumea Voice", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Sair", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_item, &quit_item])?;

    let mut builder = TrayIconBuilder::with_id("haumea-tray")
        .tooltip("Haumea Voice")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => restore_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // A left click on the tray icon restores the main window.
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                restore_main_window(tray.app_handle());
            }
        });

    // Reuse the app's window icon for the tray when one is embedded.
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder.build(app)?;
    Ok(())
}

/// Shows and focuses the main window, recreating visibility after a
/// close-to-tray. Safe to call when the window is already visible.
fn restore_main_window(app: &tauri::AppHandle) {
    log::info!("window: restore_main_window requested");
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Returns the per-user log directory for Haumea Voice. Computed from the
/// `APPDATA` environment variable and the Tauri bundle identifier so it is
/// available **before** the Tauri runtime initialises (the `app.path()`
/// resolver is not yet available at that point).
///
/// On Windows: `%APPDATA%\com.haumeavoice.app\logs\`
fn logs_dir() -> Option<std::path::PathBuf> {
    let appdata = std::env::var("APPDATA").ok()?;
    let dir = std::path::PathBuf::from(appdata)
        .join("com.haumeavoice.app")
        .join("logs");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Writer that sends every byte to both a file and stderr. Passed to
/// `env_logger` via `Target::Pipe` so runtime log lines are persisted to
/// `app.log` while still appearing in the console during `cargo tauri dev`.
struct TeeWriter {
    file: std::fs::File,
}

impl std::io::Write for TeeWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // stderr first (best-effort; never fail the whole write for it)
        let _ = std::io::stderr().write_all(buf);
        self.file.write_all(buf)?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        let _ = std::io::stderr().flush();
        self.file.flush()
    }
}

/// Initializes logging. In production builds the output is tee'd to
/// `%APPDATA%/com.haumeavoice.app/logs/app.log` (truncated each session)
/// so runtime messages survive for post-mortem inspection. In dev mode
/// (or if the file cannot be opened) output goes only to stderr.
fn init_logging() {
    let mut builder = env_logger::Builder::from_default_env();
    builder
        .filter_level(log::LevelFilter::Info)
        .format_timestamp_secs();

    if let Some(dir) = logs_dir() {
        let log_path = dir.join("app.log");
        if let Ok(file) = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&log_path)
        {
            builder.target(env_logger::Target::Pipe(Box::new(TeeWriter { file })));
        }
    }

    let _ = builder.try_init();
}

/// Installs a Windows vectored exception handler that dumps any *native*
/// (non-Rust-panic) fatal exception — access violation, stack overflow,
/// illegal instruction, etc. — to `crash.log` before the process is torn
/// down. Rust's `std::panic` hook (set in [`run`]) only catches panics; a
/// native crash otherwise leaves no in-app forensic trail (only a Windows
/// WER report, which is hard to read). The handler returns
/// `EXCEPTION_CONTINUE_SEARCH` so the default OS handler still runs and
/// produces its WER report — we just log first.
#[cfg(target_os = "windows")]
fn install_native_crash_handler() {
    use windows::Win32::System::Diagnostics::Debug::{
        AddVectoredExceptionHandler, EXCEPTION_POINTERS,
    };

    unsafe extern "system" fn handler(info: *mut EXCEPTION_POINTERS) -> i32 {
        // EXCEPTION_CONTINUE_SEARCH (0): let the default handler run after us.
        const CONTINUE_SEARCH: i32 = 0;
        if info.is_null() {
            return CONTINUE_SEARCH;
        }
        // SAFETY: the OS guarantees `info` is valid for the duration of the
        // handler call. We only read from it.
        let pointers = unsafe { &*info };
        let rec_ptr = pointers.ExceptionRecord;
        if rec_ptr.is_null() {
            return CONTINUE_SEARCH;
        }
        let record = unsafe { &*rec_ptr };
        // Use Debug repr for the code/NTSTATUS and `:p` for the address so we
        // never depend on the exact ABI types. Best-effort file write —
        // allocating inside an exception handler can deadlock if the exception
        // happened in the heap, but for the common access-violation / stack-
        // overflow cases this completes and is invaluable for diagnosis.
        let code = format!("{:?}", record.ExceptionCode);
        let addr = format!("{:p}", record.ExceptionAddress);
        let msg = format!(
            "[NATIVE EXCEPTION] code={} addr={}\nBacktrace: <not captured; see Windows WER report>\n",
            code, addr
        );
        log::error!("{}", msg.trim());
        if let Some(dir) = logs_dir() {
            let log_path = dir.join("crash.log");
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)
            {
                use std::io::Write;
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let _ = writeln!(f, "--- {} ---\n{}", ts, msg);
            }
        }
        CONTINUE_SEARCH
    }

    // first = 0 → our handler runs after earlier handlers, effectively for
    // every unhandled exception. The returned handle is intentionally never
    // freed: it must live for the whole process lifetime.
    let _ = unsafe { AddVectoredExceptionHandler(0, Some(handler)) };
    log::info!("crash: native vectored exception handler installed");
}

/// No-op on non-Windows platforms.
#[cfg(not(target_os = "windows"))]
fn install_native_crash_handler() {}

/// Heartbeat watchdog that proves whether Tauri's single shared event-loop
/// thread (the "main thread") is still pumping.
///
/// A background thread schedules a closure back onto the main thread every
/// 2 s via [`tauri::AppHandle::run_on_main_thread`]; the closure bumps a
/// monotonic counter. Soft stalls (≥2 s) are logged to `app.log` only.
/// **Serious** stalls (≥6 s, aligned with Windows AppHangB1) are also
/// appended to `crash.log` with context (recording flag, gadget HWND).
///
/// This is the decisive diagnostic for the recurring AppHangB1
/// ("Não respondendo"): when Windows marks the process Not Responding, the
/// last STALLED line in `crash.log` confirms the main thread was blocked.
fn spawn_main_thread_heartbeat(app: tauri::AppHandle) {
    static HEARTBEAT_TICK: AtomicI64 = AtomicI64::new(0);
    std::thread::spawn(move || {
        let mut stall_start: Option<std::time::Instant> = None;
        let mut wrote_crash_log = false;
        loop {
            // Schedule a tick on the main thread. If the main thread is
            // blocked, this closure simply won't run — which is exactly what
            // the counter comparison below detects.
            let before_tick = HEARTBEAT_TICK.load(Ordering::Relaxed);
            let _ = app.run_on_main_thread(move || {
                HEARTBEAT_TICK.fetch_add(1, Ordering::Relaxed);
            });
            std::thread::sleep(std::time::Duration::from_secs(2));
            let now_tick = HEARTBEAT_TICK.load(Ordering::Relaxed);
            if now_tick == before_tick {
                // Main thread hasn't ticked since the last check (≥2 s).
                match stall_start {
                    None => {
                        stall_start = Some(std::time::Instant::now());
                        wrote_crash_log = false;
                        let recording = app
                            .try_state::<SharedState>()
                            .map(|s| s.is_recording())
                            .unwrap_or(false);
                        #[cfg(target_os = "windows")]
                        let hwnd = GADGET_HWND.load(Ordering::Relaxed);
                        #[cfg(not(target_os = "windows"))]
                        let hwnd = 0isize;
                        log::warn!(
                            "heartbeat: main-thread soft-stall (tick={}, recording={}, gadget_hwnd={:#x})",
                            now_tick, recording, hwnd
                        );
                    }
                    Some(start) => {
                        let elapsed = start.elapsed().as_secs();
                        if elapsed >= 6 && !wrote_crash_log {
                            wrote_crash_log = true;
                            let recording = app
                                .try_state::<SharedState>()
                                .map(|s| s.is_recording())
                                .unwrap_or(false);
                            #[cfg(target_os = "windows")]
                            let hwnd = GADGET_HWND.load(Ordering::Relaxed);
                            #[cfg(not(target_os = "windows"))]
                            let hwnd = 0isize;
                            log::error!(
                                "heartbeat: main thread STALLED {}s (tick={}, recording={}, gadget_hwnd={:#x})",
                                elapsed, now_tick, recording, hwnd
                            );
                            if let Some(dir) = logs_dir() {
                                let log_path = dir.join("crash.log");
                                if let Ok(mut f) = std::fs::OpenOptions::new()
                                    .create(true)
                                    .append(true)
                                    .open(&log_path)
                                {
                                    use std::io::Write;
                                    let ts = std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_secs();
                                    let _ = writeln!(
                                        f,
                                        "--- {} ---\n[APPHANG DETECTED] main thread stalled {}s (tick={}, recording={}, gadget_hwnd={:#x})\n",
                                        ts, elapsed, now_tick, recording, hwnd
                                    );
                                }
                            }
                        } else if elapsed == 15 || elapsed == 30 || elapsed == 60 {
                            log::error!("heartbeat: main thread STILL stalled after {}s", elapsed);
                        }
                    }
                }
            } else if let Some(start) = stall_start.take() {
                let elapsed = start.elapsed().as_secs().min(120);
                let was_serious = wrote_crash_log;
                wrote_crash_log = false;
                log::warn!("heartbeat: main thread RESUMED after {}s stall", elapsed);
                // Only persist RESOLVED when we previously wrote a serious stall.
                if was_serious {
                    if let Some(dir) = logs_dir() {
                        let log_path = dir.join("crash.log");
                        if let Ok(mut f) = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(&log_path)
                        {
                            use std::io::Write;
                            let ts = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs();
                            let _ = writeln!(
                                f,
                                "--- {} ---\n[APPHANG RESOLVED] main thread resumed after {}s\n",
                                ts, elapsed
                            );
                        }
                    }
                }
            }
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_logging();
    log::info!("startup: run() entry");

    // In release GUI mode there is no console, so a panic message vanishes
    // silently. This hook writes the panic info and a backtrace to
    // `crash.log` inside the app's per-user log directory so any future
    // panic leaves a forensic trail that can be inspected post-mortem.
    std::panic::set_hook({
        let crash_dir = logs_dir();
        Box::new(move |info| {
            let bt = std::backtrace::Backtrace::force_capture();
            let msg = format!("[PANIC] {}\nBacktrace:\n{}\n", info, bt);
            // Also goes to app.log via the TeeWriter (best-effort).
            log::error!("{}", msg);

            if let Some(ref dir) = crash_dir {
                let log_path = dir.join("crash.log");
                use std::io::Write;
                if let Ok(mut f) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&log_path)
                {
                    let timestamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    let _ = writeln!(f, "--- {} ---\n{}", timestamp, msg);
                }
            }
        })
    });

    install_native_crash_handler();
    log::info!("startup: beginning tauri builder setup");

    let state: SharedState = Arc::new(models::AppState::new());

    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .manage(state.clone())
        .setup({
            let state = state.clone();
            move |app| {
                log::info!("startup: setup begin");
                // Resolve the per-user app data directory once and hand it
                // to the history module so transcription snapshots can be
                // persisted to `history.json`.
                match app.path().app_data_dir() {
                    Ok(dir) => {
                        let history_file = dir.join("history.json");
                        // Seed the file with an empty list if missing so the
                        // frontend never has to handle "file not found".
                        if !history_file.exists() {
                            let _ = std::fs::create_dir_all(&dir);
                            let _ = std::fs::write(&history_file, "[]");
                        }
                        history::init(history_file);
                        log::info!("history: storage at {:?}", dir);

                        // Wire up persistent API key storage and load any
                        // previously saved keys into the in-memory state so
                        // they are available immediately after a restart.
                        secrets::init(dir.join("api_keys.json"));
                        *state.api_keys.write() = secrets::load();
                        log::info!("secrets: storage at {:?}", dir);

                        // Permanent storage for the audio behind each
                        // transcription (used later by the pronunciation
                        // evaluator). Lives in an `audio` sub-directory.
                        audio_store::init(dir.join("audio"));
                        log::info!("audio_store: storage at {:?}", dir.join("audio"));

                        // Load any persisted custom recording shortcuts into
                        // the state before they are registered below.
                        shortcuts::init_store(dir.join("shortcuts.json"));
                        *state.shortcuts.write() = shortcuts::load_store();

                        // Lightweight UI preferences (gadget compact mode).
                        settings::init(dir.join("settings.json"));
                        *state.compact_mode.write() = settings::load_compact();
                        *state.system_prompt.write() = settings::load_system_prompt();

                        if let Some(engine) = settings::load_engine() {
                            *state.engine.write() = engine;
                        }
                        if let Some(sanitizer) = settings::load_sanitizer() {
                            *state.sanitizer.write() = sanitizer;
                        }
                        *state.dual_engine.write() = settings::load_dual_engine();
                        *state.reasoning_enabled.write() = settings::load_reasoning_enabled();
                        *state.deepgram_mode.write() = settings::load_deepgram_mode();
                        *state.sanitizer_enabled.write() = settings::load_sanitizer_enabled();
                        *state.reasoning_effort.write() = settings::load_reasoning_effort();
                        *state.vocabulary.write() = settings::load_vocabulary();
                        *state.modes_enabled.write() = settings::load_modes_enabled();
                        *state.transcription_mode.write() = settings::load_transcription_mode();
                        *state.gemini_fallback_to_whisper.write() =
                            settings::load_gemini_fallback_to_whisper();
                        *state.content_type.write() = settings::load_content_type();
                        *state.gemini_pipelines.write() = settings::load_gemini_pipelines();
                    }
                    Err(e) => {
                        log::warn!("history: could not resolve app data dir: {}", e);
                    }
                }

                // Register the global recording shortcuts from the (possibly
                // persisted) config. A bad persisted combination is logged
                // rather than crashing startup.
                let cfg = state.shortcuts.read().clone();
                if let Err(e) = shortcuts::register_all(app.handle(), &cfg.toggle, &cfg.cancel) {
                    log::error!("failed to register global shortcuts: {}", e);
                }

                // Hand the AppHandle to the audio pipeline so finished
                // transcriptions can be persisted and announced to the UI.
                *state.app_handle.write() = Some(app.handle().clone());

                // Build the system tray so the app can live in the background.
                if let Err(e) = setup_tray(app.handle()) {
                    log::error!("failed to create system tray: {}", e);
                }

                // Spawn the always-on-top floating gadget overlay.
                setup_gadget(app.handle());

                // Make the overlay click-through outside its visible pill so it
                // no longer eats clicks that land near (but not on) the gadget.
                spawn_gadget_cursor_watcher_safe(app.handle().clone(), state.clone());

                // If not launched via autostart, show the main window.
                let is_autostart = std::env::args().any(|arg| arg == "--autostart");
                if !is_autostart {
                    if let Some(w) = app.handle().get_webview_window("main") {
                        let _ = w.show();
                        let _ = w.set_focus();
                    }
                }

                // Launch the main-thread heartbeat watchdog so any future
                // AppHangB1 leaves a definitive STALLED/RESUMED trail in both
                // app.log and crash.log.
                spawn_main_thread_heartbeat(app.handle().clone());

                log::info!("startup: setup complete");
                Ok(())
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::update_engine_config,
            commands::get_engine_config,
            commands::get_mode_config,
            commands::update_mode_config,
            commands::save_api_keys,
            commands::get_api_keys,
            commands::transcribe_file,
            commands::evaluate_pronunciation,
            commands::toggle_recording_state,
            commands::get_recording_state,
            commands::get_recording_elapsed,
            commands::get_history,
            commands::get_audio_storage_config,
            commands::set_audio_storage_directory,
            commands::reveal_history_audio,
            commands::clear_history,
            commands::delete_history_entry,
            commands::update_history_text,
            commands::save_system_prompt,
            commands::get_system_prompt,
            commands::get_custom_words,
            commands::set_custom_words,
            commands::get_vocabulary,
            commands::set_vocabulary,
            commands::get_compact_mode,
            commands::set_compact_mode,
            commands::set_gadget_hit_rect,
            commands::get_shortcuts,
            commands::set_shortcuts,
            commands::list_audio_devices,
            commands::get_input_device,
            commands::set_input_device,
            commands::start_mic_test,
            commands::stop_mic_test,
            commands::retry_transcription,
            commands::get_autostart,
            commands::set_autostart,
            commands::get_dev_mode,
            commands::set_dev_mode,
            commands::get_sanitizer_enabled,
            commands::set_sanitizer_enabled,
        ])
        .on_window_event(|window, event| {
            // Diagnostic logging of focus / lifecycle / resize events for the
            // main and gadget windows. `Moved` is excluded (it fires per-pixel
            // during a drag and would flood the log). This trail is the key
            // forensic record when the app AppHangs: the last logged event
            // before the gap in app.log shows what the main thread was doing —
            // especially the focus transitions, which correlate with the
            // "crasha quando foco em outra tela" symptom.
            match &event {
                WindowEvent::Focused(focused) => {
                    log::info!("window: '{}' focus -> {}", window.label(), focused);
                    // Reassert immediately when our main window gains focus or
                    // the gadget loses it to another app. This direct Win32
                    // request does not mutate styles through Tauri's event loop.
                    #[cfg(target_os = "windows")]
                    if (*focused && window.label() == "main")
                        || (!*focused && window.label() == "gadget")
                    {
                        let hwnd = GADGET_HWND.load(Ordering::Acquire);
                        let _ = win32_keep_gadget_visible_and_topmost(hwnd);
                    }
                }
                WindowEvent::Destroyed => {
                    log::info!("window: '{}' destroyed", window.label());
                    #[cfg(target_os = "windows")]
                    if window.label() == "gadget" {
                        GADGET_HWND.store(0, Ordering::Release);
                    }
                }
                WindowEvent::Resized(size) => {
                    log::info!("window: '{}' resized to {:?}", window.label(), size);
                }
                _ => {}
            }

            if window.label() == "gadget" {
                if let WindowEvent::Moved(position) = event {
                    // `position` is a `PhysicalPosition<i32>` relative to the
                    // virtual desktop origin. Persisting it *as physical*
                    // (instead of dividing by the scale factor) makes the
                    // restore path independent of which monitor's DPI was in
                    // effect when the window was saved — the root cause of the
                    // gadget always snapping back to the primary monitor.
                    let x = position.x;
                    let y = position.y;
                    // Sanity-check: reject absurd values that occasionally show
                    // up during window creation/teardown on some WSL/Wine
                    // setups (e.g. i32::MIN). A real position is always within
                    // a few hundred thousand pixels of the origin.
                    if x.abs() < 1_000_000 && y.abs() < 1_000_000 {
                        queue_gadget_position_save(x, y);
                    }
                }
            }

            // Closing the main window only hides it: the app keeps running in
            // the system tray. "Sair" in the tray menu performs a real exit.
            // The gadget should never be closed or hidden — just block the
            // event without touching its visibility.
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    let _ = window.hide();
                }
                api.prevent_close();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Haumea Voice application");
}

#[cfg(test)]
mod gadget_placement_tests {
    use super::{clamp_gadget_placement, MonitorBounds};

    const PRIMARY: MonitorBounds = MonitorBounds {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
        scale: 1.0,
    };
    const SECONDARY: MonitorBounds = MonitorBounds {
        x: -2560,
        y: -200,
        width: 2560,
        height: 1440,
        scale: 1.5,
    };

    #[test]
    fn keeps_a_valid_secondary_monitor_position_and_dpi() {
        let placement = clamp_gadget_placement(-1800, 20, &[PRIMARY, SECONDARY]);
        assert_eq!(placement, (-1800, 20, 420, 84));
    }

    #[test]
    fn recovers_a_position_from_a_disconnected_monitor() {
        let (x, y, width, height) = clamp_gadget_placement(5000, 4000, &[PRIMARY]);
        assert_eq!((width, height), (280, 56));
        assert_eq!((x, y), (1632, 1016));
    }

    #[test]
    fn clamps_negative_edges_without_forcing_the_primary_monitor() {
        let (x, y, _, _) = clamp_gadget_placement(-2700, -500, &[PRIMARY, SECONDARY]);
        assert_eq!((x, y), (-2548, -188));
    }
}
