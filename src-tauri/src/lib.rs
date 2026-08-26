pub mod audio;
pub mod audio_processing;
pub mod audio_store;
pub mod commands;
pub mod context;
pub mod deepgram;
pub mod gemini;
pub mod groq;
pub mod history;
pub mod learning;
pub mod mic_control;
pub mod models;
pub mod native_messaging;
pub mod openrouter;
pub mod output_policy;
pub mod pipeline_contract;
pub mod pipeline_run;
pub mod sanitizer_json;
pub mod scratchpad;
pub mod secrets;
pub mod settings;
pub mod shortcuts;
pub mod snippets;
pub mod transcription;
pub mod transformations;
pub mod vocabulary;

use models::{GadgetSessionAnchor, GadgetVisualState, SharedState, WidgetVisibilityMode};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicIsize, Ordering};
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

const GADGET_BOTTOM_MARGIN: f64 = 18.0;

#[derive(Clone, Copy, Debug, PartialEq)]
struct GadgetGeometry {
    width: f64,
    height: f64,
}

/// The native footprint is state-derived and stays close to the painted pill.
/// The small extra inset is only for the real shadow, not a permanent hitbox.
fn gadget_geometry(state: GadgetVisualState) -> GadgetGeometry {
    match state {
        GadgetVisualState::Hidden => GadgetGeometry {
            width: 1.0,
            height: 1.0,
        },
        GadgetVisualState::Idle | GadgetVisualState::Appearing => GadgetGeometry {
            width: 72.0,
            height: 52.0,
        },
        GadgetVisualState::Hover => GadgetGeometry {
            width: 142.0,
            height: 58.0,
        },
        GadgetVisualState::Initializing | GadgetVisualState::Stopping => GadgetGeometry {
            width: 74.0,
            height: 52.0,
        },
        GadgetVisualState::Recording => GadgetGeometry {
            width: 186.0,
            height: 48.0,
        },
        GadgetVisualState::Processing => GadgetGeometry {
            width: 78.0,
            height: 52.0,
        },
        GadgetVisualState::ProcessingLong => GadgetGeometry {
            width: 158.0,
            height: 52.0,
        },
        GadgetVisualState::Success => GadgetGeometry {
            width: 54.0,
            height: 52.0,
        },
        GadgetVisualState::Error => GadgetGeometry {
            width: 326.0,
            height: 58.0,
        },
    }
}

fn bottom_center_placement(
    anchor: &GadgetSessionAnchor,
    geometry: GadgetGeometry,
) -> (i32, i32, u32, u32) {
    let width = (geometry.width * anchor.scale).round().max(1.0) as u32;
    let height = (geometry.height * anchor.scale).round().max(1.0) as u32;
    let x = anchor.work_x as i64 + (anchor.work_width as i64 - width as i64) / 2;
    let y = anchor.work_y as i64 + anchor.work_height as i64
        - height as i64
        - (GADGET_BOTTOM_MARGIN * anchor.scale).round() as i64;
    (x as i32, y as i32, width, height)
}

fn monitor_anchor(monitor: &tauri::Monitor) -> GadgetSessionAnchor {
    let work = monitor.work_area();
    GadgetSessionAnchor {
        display_name: monitor.name().map(ToOwned::to_owned),
        work_x: work.position.x,
        work_y: work.position.y,
        work_width: work.size.width,
        work_height: work.size.height,
        scale: monitor.scale_factor(),
    }
}

fn choose_gadget_anchor(app: &tauri::AppHandle) -> Option<GadgetSessionAnchor> {
    let monitors = app.available_monitors().ok()?;
    if monitors.is_empty() {
        return None;
    }

    if let Some(saved_name) = crate::settings::load_widget_display() {
        if let Some(monitor) = monitors
            .iter()
            .find(|monitor| monitor.name().map(String::as_str) == Some(saved_name.as_str()))
        {
            return Some(monitor_anchor(monitor));
        }
        log::warn!(
            "gadget: persisted display '{}' is unavailable; using cursor/primary fallback",
            saved_name
        );
    }

    if let Ok(cursor) = app.cursor_position() {
        if let Some(monitor) = monitors.iter().find(|monitor| {
            let position = monitor.position();
            let size = monitor.size();
            cursor.x >= position.x as f64
                && cursor.x < (position.x as f64 + size.width as f64)
                && cursor.y >= position.y as f64
                && cursor.y < (position.y as f64 + size.height as f64)
        }) {
            return Some(monitor_anchor(monitor));
        }
    }

    app.primary_monitor()
        .ok()
        .flatten()
        .map(|monitor| monitor_anchor(&monitor))
}

fn apply_gadget_presentation(
    app: &tauri::AppHandle,
    state: &SharedState,
    requested: GadgetVisualState,
) -> Result<GadgetVisualState, String> {
    let resolved = if requested == GadgetVisualState::Hidden
        && *state.widget_visibility_mode.read() == WidgetVisibilityMode::Always
    {
        GadgetVisualState::Idle
    } else {
        requested
    };
    *state.gadget_visual_state.write() = resolved;

    let Some(window) = app.get_webview_window("gadget") else {
        return Err("gadget window is unavailable".to_string());
    };

    if resolved == GadgetVisualState::Hidden {
        let _ = window.set_ignore_cursor_events(true);
        window.hide().map_err(|error| error.to_string())?;
        *state.gadget_hit_rect.write() = None;
        *state.gadget_session_anchor.lock() = None;
        return Ok(resolved);
    }

    let anchor = {
        let mut frozen = state.gadget_session_anchor.lock();
        if frozen.is_none() {
            *frozen = choose_gadget_anchor(app);
        }
        frozen.clone()
    }
    .ok_or_else(|| "no display is available for the gadget".to_string())?;

    let geometry = gadget_geometry(resolved);
    let (x, y, physical_width, physical_height) = bottom_center_placement(&anchor, geometry);

    *state.gadget_hit_rect.write() = None;
    let _ = window.set_ignore_cursor_events(true);
    window
        .set_size(tauri::PhysicalSize::new(physical_width, physical_height))
        .map_err(|error| error.to_string())?;
    window
        .set_position(tauri::PhysicalPosition::new(x, y))
        .map_err(|error| error.to_string())?;
    window.show().map_err(|error| error.to_string())?;
    log::info!(
        "gadget: state={:?} display={:?} work=({},{} {}x{}) window=({},{} {}x{}) scale={:.2}",
        resolved,
        anchor.display_name,
        anchor.work_x,
        anchor.work_y,
        anchor.work_width,
        anchor.work_height,
        x,
        y,
        physical_width,
        physical_height,
        anchor.scale,
    );
    Ok(resolved)
}

pub(crate) fn begin_gadget_session(app: &tauri::AppHandle, state: &SharedState) {
    let mut anchor = state.gadget_session_anchor.lock();
    if anchor.is_none() {
        *anchor = choose_gadget_anchor(app);
    }
    drop(anchor);
    if let Err(error) = apply_gadget_presentation(app, state, GadgetVisualState::Appearing) {
        log::warn!("gadget: failed to begin session: {}", error);
    }
}

pub(crate) fn present_gadget(
    app: &tauri::AppHandle,
    state: &SharedState,
    visual_state: GadgetVisualState,
) -> Result<GadgetVisualState, String> {
    apply_gadget_presentation(app, state, visual_state)
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
fn setup_gadget(app: &tauri::AppHandle, state: &SharedState) {
    let initial = if *state.widget_visibility_mode.read() == WidgetVisibilityMode::Always {
        GadgetVisualState::Idle
    } else {
        GadgetVisualState::Hidden
    };
    let geometry = gadget_geometry(initial);
    let builder = WebviewWindowBuilder::new(app, "gadget", WebviewUrl::App("index.html".into()))
        .title("Haumea Dictation Bar")
        .inner_size(geometry.width, geometry.height)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(false)
        .visible(false)
        .focused(false)
        .focusable(false);

    match builder.build() {
        Ok(window) => {
            cache_gadget_hwnd(&window);
            if let Err(error) = apply_gadget_presentation(app, state, initial) {
                log::warn!("gadget: initial presentation failed: {}", error);
            }
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
            return false;
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
            None => false,
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
/// - Cadence ~200 ms (imperceptible for a small floating pill).
///
/// Until the first pill rect is reported the overlay stays click-through.
fn spawn_gadget_cursor_watcher_safe(app: tauri::AppHandle, state: SharedState) {
    let pending_toggle = Arc::new(AtomicBool::new(false));

    std::thread::spawn(move || {
        let mut last_ignore: Option<bool> = None;
        let mut tick: u32 = 0;

        loop {
            std::thread::sleep(std::time::Duration::from_millis(200));
            tick = tick.wrapping_add(1);

            #[cfg(target_os = "windows")]
            {
                let hwnd_raw = GADGET_HWND.load(Ordering::Acquire);

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

                let inside = win32_cursor_inside_pill(hwnd_raw, &state);
                // If HWND was invalidated during hit-test, skip this tick.
                if GADGET_HWND.load(Ordering::Acquire) == 0 {
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
                        None => false,
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

#[cfg(target_os = "windows")]
fn win32_foreground_monitor_anchor() -> Option<GadgetSessionAnchor> {
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    // SAFETY: all calls are read-only OS queries. The foreground HWND and
    // monitor handle are validated by their documented null results.
    unsafe {
        let foreground = GetForegroundWindow();
        if foreground.0.is_null() || foreground.0 as isize == GADGET_HWND.load(Ordering::Acquire) {
            return None;
        }
        let monitor = MonitorFromWindow(foreground, MONITOR_DEFAULTTONEAREST);
        if monitor.0.is_null() {
            return None;
        }
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return None;
        }
        let dpi = GetDpiForWindow(foreground);
        let work = info.rcWork;
        Some(GadgetSessionAnchor {
            display_name: None,
            work_x: work.left,
            work_y: work.top,
            work_width: (work.right - work.left).max(1) as u32,
            work_height: (work.bottom - work.top).max(1) as u32,
            scale: if dpi == 0 { 1.0 } else { dpi as f64 / 96.0 },
        })
    }
}

#[cfg(target_os = "windows")]
fn spawn_gadget_focus_monitor_watcher(app: tauri::AppHandle, state: SharedState) {
    let pending_move = Arc::new(AtomicBool::new(false));
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(250));
        let Some(next_anchor) = win32_foreground_monitor_anchor() else {
            continue;
        };
        if *state.gadget_visual_state.read() == GadgetVisualState::Hidden {
            continue;
        }
        let unchanged = state
            .gadget_session_anchor
            .lock()
            .as_ref()
            .map(|current| {
                current.work_x == next_anchor.work_x
                    && current.work_y == next_anchor.work_y
                    && current.work_width == next_anchor.work_width
                    && current.work_height == next_anchor.work_height
                    && (current.scale - next_anchor.scale).abs() < 0.01
            })
            .unwrap_or(false);
        if unchanged || pending_move.swap(true, Ordering::AcqRel) {
            continue;
        }

        let app_for_move = app.clone();
        let state_for_move = state.clone();
        let pending_for_move = pending_move.clone();
        let pending_for_error = pending_move.clone();
        let queued = app.run_on_main_thread(move || {
            let current_state = *state_for_move.gadget_visual_state.read();
            if current_state != GadgetVisualState::Hidden {
                *state_for_move.gadget_session_anchor.lock() = Some(next_anchor);
                if let Err(error) =
                    apply_gadget_presentation(&app_for_move, &state_for_move, current_state)
                {
                    log::warn!("gadget: focus-monitor move failed: {}", error);
                }
            }
            pending_for_move.store(false, Ordering::Release);
        });
        if queued.is_err() {
            pending_for_error.store(false, Ordering::Release);
        }
    });
}

#[cfg(not(target_os = "windows"))]
fn spawn_gadget_focus_monitor_watcher(_app: tauri::AppHandle, _state: SharedState) {}

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
                        context::init(dir.clone());
                        scratchpad::init(dir.clone());
                        snippets::init(dir.clone());
                        learning::init(dir.clone());
                        *state.compact_mode.write() = settings::load_compact();
                        *state.widget_visibility_mode.write() =
                            settings::load_widget_visibility_mode();
                        *state.widget_dock.write() = settings::load_widget_dock();
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
                        *state.file_tagging_enabled.write() = settings::load_file_tagging_enabled();
                        *state.gemini_pipelines.write() = settings::load_gemini_pipelines();
                        *state.context_preferences.write() = settings::load_context_preferences();
                        *state.output_profiles.write() = settings::load_output_profiles();
                        *state.formatting_level.write() = settings::load_formatting_level();
                        *state.dictation_destination.write() =
                            settings::load_dictation_destination();
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
                setup_gadget(app.handle(), &state);

                // Make the overlay click-through outside its visible pill so it
                // no longer eats clicks that land near (but not on) the gadget.
                spawn_gadget_cursor_watcher_safe(app.handle().clone(), state.clone());
                spawn_gadget_focus_monitor_watcher(app.handle().clone(), state.clone());

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
            commands::cancel_recording,
            commands::get_recording_state,
            commands::get_recording_elapsed,
            commands::get_history,
            commands::get_audio_storage_config,
            commands::set_audio_storage_directory,
            commands::reveal_history_audio,
            commands::read_history_audio,
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
            commands::get_widget_preferences,
            commands::set_widget_visibility_mode,
            commands::set_gadget_visual_state,
            commands::set_gadget_hit_rect,
            commands::get_shortcuts,
            commands::set_shortcuts,
            commands::list_audio_devices,
            commands::get_input_device,
            commands::set_input_device,
            commands::start_mic_test,
            commands::stop_mic_test,
            commands::retry_transcription,
            commands::retry_transcription_with_fallback,
            commands::undo_ai_edit,
            commands::get_autostart,
            commands::set_autostart,
            commands::get_dev_mode,
            commands::set_dev_mode,
            commands::get_context_preferences,
            commands::set_context_preferences,
            commands::get_output_policy_config,
            commands::set_output_policy_config,
            commands::get_scratchpad_notes,
            commands::delete_scratchpad_note,
            commands::get_snippets,
            commands::set_snippets,
            commands::get_vocabulary_suggestions,
            commands::resolve_vocabulary_suggestion,
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

            // Closing the main window only hides it: the app keeps running in
            // the system tray. "Sair" in the tray menu performs a real exit.
            // The gadget is lifecycle-managed by its state machine; a close
            // request must not destroy the reusable WebView.
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
    use super::{bottom_center_placement, gadget_geometry};
    use crate::models::{GadgetSessionAnchor, GadgetVisualState};

    #[test]
    fn centers_recording_on_negative_mixed_dpi_work_area() {
        let anchor = GadgetSessionAnchor {
            display_name: Some("DISPLAY-2".to_string()),
            work_x: -2560,
            work_y: -200,
            work_width: 2560,
            work_height: 1392,
            scale: 1.5,
        };
        let placement =
            bottom_center_placement(&anchor, gadget_geometry(GadgetVisualState::Recording));
        assert_eq!(placement, (-1420, 1093, 279, 72));
    }

    #[test]
    fn uses_work_area_not_full_monitor_height() {
        let anchor = GadgetSessionAnchor {
            display_name: None,
            work_x: 0,
            work_y: 0,
            work_width: 1920,
            work_height: 1040,
            scale: 1.0,
        };
        let placement = bottom_center_placement(&anchor, gadget_geometry(GadgetVisualState::Idle));
        assert_eq!(placement, (924, 970, 72, 52));
    }

    #[test]
    fn every_visual_state_has_a_positive_native_footprint() {
        use GadgetVisualState::*;
        for state in [
            Hidden,
            Idle,
            Hover,
            Appearing,
            Initializing,
            Recording,
            Stopping,
            Processing,
            ProcessingLong,
            Success,
            Error,
        ] {
            let geometry = gadget_geometry(state);
            assert!(geometry.width > 0.0 && geometry.height > 0.0);
        }
    }
}
