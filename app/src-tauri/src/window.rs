use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, sync::Mutex, thread, time::Duration};
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, PhysicalSize, Position, Rect, Size,
    WebviewUrl, WebviewWindowBuilder,
};

const MAIN_WINDOW_LABEL: &str = "main";
const RING_WINDOW_LABEL: &str = "ring";
const SMALL_W: f64 = 210.0;
const SMALL_COLLAPSED_W: f64 = 160.0;
const SMALL_H: f64 = 30.0;
const LARGE_W: f64 = 200.0;
const LARGE_H: f64 = 50.0;
const LARGE_MIN_W: f64 = 200.0;
const LARGE_MIN_H: f64 = 50.0;
const LARGE_MAX_W: f64 = 340.0;
const LARGE_MAX_H: f64 = 140.0;
const RING_SIZE: f64 = 100.0;
const RING_MAX_SIZE: f64 = 300.0;
const WINDOW_STATE_SCHEMA_VERSION: u32 = 1;
const WINDOW_SIZE_FORMAT: &str = "logical-outer-v1";
const STATE_WRITE_DEBOUNCE_MS: u64 = 220;
const SCREEN_MARGIN: i32 = 8;
const STATE_FILE: &str = "window-state.json";
static STATE_WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowState {
    pub mode: String,
    pub always_on_top: bool,
    pub visible: bool,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            mode: "small".into(),
            always_on_top: true,
            visible: false,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SavedWindowState {
    schema_version: Option<u32>,
    size_format: Option<String>,
    position: Option<SavedPosition>,
    large_size: Option<SavedSize>,
    ring_size: Option<SavedSize>,
    // Kept only so existing state files can be read without a destructive reset.
    ring_size_format: Option<String>,
    always_on_top: Option<bool>,
    mode: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SavedPosition {
    x: i32,
    y: i32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SavedSize {
    width: f64,
    height: f64,
}

#[derive(Clone, Debug)]
struct RuntimeWindowState {
    mode: String,
    always_on_top: bool,
    visible: bool,
    position: Option<SavedPosition>,
    large_size: Option<SavedSize>,
    ring_size: Option<SavedSize>,
    persist_revision: u64,
}

impl Default for RuntimeWindowState {
    fn default() -> Self {
        Self {
            mode: "small".into(),
            always_on_top: true,
            visible: false,
            position: None,
            large_size: None,
            ring_size: None,
            persist_revision: 0,
        }
    }
}

type SharedWindowState = Mutex<RuntimeWindowState>;

pub fn init_state(app: &AppHandle) -> tauri::Result<()> {
    let persisted = load_persisted_state(app).unwrap_or_default();
    let large_size = load_large_size(&persisted);
    let ring_size = load_ring_size(&persisted);
    let state = RuntimeWindowState {
        mode: persisted
            .mode
            .as_deref()
            .map(normalize_mode)
            .unwrap_or("small")
            .into(),
        always_on_top: persisted.always_on_top.unwrap_or(true),
        visible: false,
        position: persisted.position,
        large_size,
        ring_size,
        persist_revision: 0,
    };
    app.manage(SharedWindowState::new(state));
    save_persisted_state(app);
    Ok(())
}

pub fn prepare_ring_window(app: &AppHandle) -> tauri::Result<()> {
    if app.get_webview_window(RING_WINDOW_LABEL).is_some() {
        return Ok(());
    }

    let state = get_runtime_state(app);
    let size = state
        .ring_size
        .map(clamp_ring_size)
        .unwrap_or_else(default_ring_size);
    let window =
        WebviewWindowBuilder::new(app, RING_WINDOW_LABEL, WebviewUrl::App("index.html".into()))
            .title("Codex 额度")
            .inner_size(size.width, size.height)
            .min_inner_size(RING_SIZE, RING_SIZE)
            .max_inner_size(RING_MAX_SIZE, RING_MAX_SIZE)
            .decorations(false)
            .transparent(true)
            .resizable(true)
            .maximizable(false)
            .skip_taskbar(true)
            .always_on_top(state.always_on_top)
            .visible(false)
            .shadow(false)
            .build()?;

    configure_ring_native_frame(&window)?;
    set_ring_outer_size(&window, size.width, size.height)?;
    Ok(())
}

pub fn get_state(app: &AppHandle) -> WindowState {
    app.state::<SharedWindowState>()
        .lock()
        .map(|state| WindowState {
            mode: state.mode.clone(),
            always_on_top: state.always_on_top,
            visible: state.visible,
        })
        .unwrap_or_default()
}

pub fn active_window_label(app: &AppHandle) -> &'static str {
    window_label_for_mode(&get_runtime_state(app).mode)
}

pub fn show_panel(app: &AppHandle, mode: &str) -> tauri::Result<()> {
    show_panel_near_tray(app, mode, None)
}

pub fn show_existing_instance(app: &AppHandle) -> tauri::Result<()> {
    let mode = app
        .try_state::<SharedWindowState>()
        .and_then(|state| state.lock().ok().map(|state| state.mode.clone()))
        .unwrap_or_else(|| "small".into());
    show_panel(app, &mode)?;
    let _ = app.emit("quota-refresh-requested", ());
    Ok(())
}

pub fn show_panel_near_tray(
    app: &AppHandle,
    mode: &str,
    tray_rect: Option<Rect>,
) -> tauri::Result<()> {
    let normalized = normalize_mode(mode);
    let previous_mode = get_runtime_state(app).mode;
    if previous_mode != normalized {
        capture_window_state_for_label(app, window_label_for_mode(&previous_mode));
    }

    let label = window_label_for_mode(normalized);
    let Some(window) = app.get_webview_window(label) else {
        return Ok(());
    };

    if normalized == "ring" {
        if let Some(main) = app.get_webview_window(MAIN_WINDOW_LABEL) {
            let _ = main.hide();
        }
    } else if let Some(ring) = app.get_webview_window(RING_WINDOW_LABEL) {
        let _ = ring.hide();
    }

    if normalized != "ring" {
        configure_main_window_size(app, normalized)?;
    }
    let (logical_width, logical_height) = panel_size(app, normalized);

    if normalized == "ring" {
        configure_ring_native_frame(&window)?;
        set_ring_outer_size(&window, logical_width, logical_height)?;
    } else {
        window.set_size(Size::Logical(LogicalSize::new(
            logical_width,
            logical_height,
        )))?;
    }

    let scale_factor = window.scale_factor().unwrap_or(1.0);
    let physical_size = if normalized == "ring" {
        window.outer_size().unwrap_or_else(|_| {
            logical_size_to_physical(logical_width, logical_height, scale_factor)
        })
    } else {
        logical_size_to_physical(logical_width, logical_height, scale_factor)
    };
    let position = preferred_position(app, tray_rect, physical_size);
    window.set_position(Position::Physical(position))?;

    if let Ok(mut state) = app.state::<SharedWindowState>().lock() {
        state.mode = normalized.into();
        state.visible = true;
    }
    window.emit("mode-changed", normalized)?;

    let always_on_top = get_runtime_state(app).always_on_top;
    window.set_always_on_top(always_on_top)?;
    if normalized == "ring" {
        configure_ring_native_frame(&window)?;
    }
    window.show()?;
    if normalized == "ring" {
        configure_ring_native_frame(&window)?;
    }
    window.set_focus()?;

    let _ = app.emit("panel-visibility-changed", true);
    save_persisted_state(app);
    Ok(())
}

pub fn hide_panel(app: &AppHandle) -> tauri::Result<()> {
    remember_window_state(app);
    for label in [MAIN_WINDOW_LABEL, RING_WINDOW_LABEL] {
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.hide();
        }
    }
    if let Ok(mut state) = app.state::<SharedWindowState>().lock() {
        state.visible = false;
    }
    let _ = app.emit("panel-visibility-changed", false);
    save_persisted_state(app);
    Ok(())
}

pub fn set_mode(app: &AppHandle, mode: &str) -> tauri::Result<()> {
    show_panel(app, mode)
}

pub fn set_small_actions_collapsed(app: &AppHandle, collapsed: bool) -> tauri::Result<()> {
    if get_runtime_state(app).mode != "small" {
        return Ok(());
    }
    let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        return Ok(());
    };
    let width = if collapsed {
        SMALL_COLLAPSED_W
    } else {
        SMALL_W
    };
    window.set_min_size(Some(Size::Logical(LogicalSize::new(width, SMALL_H))))?;
    window.set_max_size(Some(Size::Logical(LogicalSize::new(width, SMALL_H))))?;
    window.set_size(Size::Logical(LogicalSize::new(width, SMALL_H)))?;
    Ok(())
}

pub fn toggle_topmost(app: &AppHandle) -> tauri::Result<bool> {
    let current = get_runtime_state(app);
    let next = !current.always_on_top;
    for label in [MAIN_WINDOW_LABEL, RING_WINDOW_LABEL] {
        if let Some(window) = app.get_webview_window(label) {
            window.set_always_on_top(next)?;
            if label == RING_WINDOW_LABEL {
                configure_ring_native_frame(&window)?;
            }
        }
    }
    if let Ok(mut state) = app.state::<SharedWindowState>().lock() {
        state.always_on_top = next;
    }
    let _ = app.emit("topmost-changed", next);
    save_persisted_state(app);
    Ok(next)
}

pub fn handle_tray_left_click(app: &AppHandle, tray_rect: Rect) -> tauri::Result<()> {
    let state = get_runtime_state(app);
    if state.visible && state.always_on_top {
        hide_panel(app)
    } else if state.visible {
        bring_to_front(app)
    } else {
        show_panel_near_tray(app, &state.mode, Some(tray_rect))
    }
}

fn capture_window_state_for_label(app: &AppHandle, label: &str) {
    let Some(window) = app.get_webview_window(label) else {
        return;
    };

    let position = window.outer_position().ok();
    let size = window.outer_size().ok();
    let scale_factor = window.scale_factor().unwrap_or(1.0);

    if let Ok(mut state) = app.state::<SharedWindowState>().lock() {
        if let Some(position) = position {
            state.position = Some(SavedPosition {
                x: position.x,
                y: position.y,
            });
        }
        if label == MAIN_WINDOW_LABEL && state.mode == "large" {
            if let Some(size) = size {
                let logical = size.to_logical::<f64>(scale_factor);
                state.large_size = Some(clamp_large_size(SavedSize {
                    width: logical.width,
                    height: logical.height,
                }));
            }
        } else if label == RING_WINDOW_LABEL && state.mode == "ring" {
            if let Some(size) = size {
                let logical = size.to_logical::<f64>(scale_factor);
                state.ring_size = Some(clamp_ring_size(SavedSize {
                    width: logical.width,
                    height: logical.height,
                }));
            }
        }
    }
}

pub fn remember_window_state(app: &AppHandle) {
    let label = active_window_label(app);
    capture_window_state_for_label(app, label);
    save_persisted_state(app);
}

pub fn handle_window_changed(app: &AppHandle, label: &str) {
    if active_window_label(app) == label {
        capture_window_state_for_label(app, label);
        schedule_persisted_state(app);
    }
}

fn bring_to_front(app: &AppHandle) -> tauri::Result<()> {
    let label = active_window_label(app);
    let Some(window) = app.get_webview_window(label) else {
        return Ok(());
    };
    let state = get_runtime_state(app);
    if !state.always_on_top {
        window.set_always_on_top(true)?;
    }
    if label == RING_WINDOW_LABEL {
        configure_ring_native_frame(&window)?;
    }
    window.show()?;
    if label == RING_WINDOW_LABEL {
        configure_ring_native_frame(&window)?;
    }
    window.set_focus()?;

    if !state.always_on_top {
        let app_handle = app.clone();
        let label = label.to_string();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(140));
            if !get_runtime_state(&app_handle).always_on_top {
                if let Some(window) = app_handle.get_webview_window(&label) {
                    let _ = window.set_always_on_top(false);
                    if label == RING_WINDOW_LABEL {
                        let _ = configure_ring_native_frame(&window);
                    }
                }
            }
        });
    }
    Ok(())
}

fn configure_main_window_size(app: &AppHandle, mode: &str) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        return Ok(());
    };

    if mode == "large" {
        window.set_max_size(Some(Size::Logical(LogicalSize::new(
            LARGE_MAX_W,
            LARGE_MAX_H,
        ))))?;
        window.set_min_size(Some(Size::Logical(LogicalSize::new(
            LARGE_MIN_W,
            LARGE_MIN_H,
        ))))?;
        window.set_resizable(true)?;
    } else {
        window.set_resizable(false)?;
        window.set_min_size(Some(Size::Logical(LogicalSize::new(
            SMALL_COLLAPSED_W,
            SMALL_H,
        ))))?;
        window.set_max_size(Some(Size::Logical(LogicalSize::new(
            SMALL_COLLAPSED_W,
            SMALL_H,
        ))))?;
    }
    Ok(())
}

fn normalize_mode(mode: &str) -> &'static str {
    match mode {
        "large" => "large",
        "ring" => "ring",
        _ => "small",
    }
}

fn window_label_for_mode(mode: &str) -> &'static str {
    if mode == "ring" {
        RING_WINDOW_LABEL
    } else {
        MAIN_WINDOW_LABEL
    }
}

fn panel_size(app: &AppHandle, mode: &str) -> (f64, f64) {
    if mode == "large" {
        let state = get_runtime_state(app);
        let size = state.large_size.map(clamp_large_size).unwrap_or(SavedSize {
            width: LARGE_W,
            height: LARGE_H,
        });
        (size.width, size.height)
    } else if mode == "ring" {
        let state = get_runtime_state(app);
        let size = state
            .ring_size
            .map(clamp_ring_size)
            .unwrap_or_else(default_ring_size);
        (size.width, size.height)
    } else {
        (SMALL_COLLAPSED_W, SMALL_H)
    }
}

fn get_runtime_state(app: &AppHandle) -> RuntimeWindowState {
    app.state::<SharedWindowState>()
        .lock()
        .map(|state| state.clone())
        .unwrap_or_default()
}

fn logical_size_to_physical(width: f64, height: f64, scale_factor: f64) -> PhysicalSize<u32> {
    PhysicalSize::new(
        (width * scale_factor).round().max(1.0) as u32,
        (height * scale_factor).round().max(1.0) as u32,
    )
}

fn preferred_position(
    app: &AppHandle,
    tray_rect: Option<Rect>,
    panel_size: PhysicalSize<u32>,
) -> PhysicalPosition<i32> {
    if let Some(saved) = get_runtime_state(app).position {
        return clamp_position_to_work_area(
            app,
            PhysicalPosition::new(saved.x, saved.y),
            panel_size,
        );
    }

    if let Some(rect) = tray_rect {
        let scale_factor = app
            .primary_monitor()
            .ok()
            .flatten()
            .map(|monitor| monitor.scale_factor())
            .unwrap_or(1.0);
        let tray_pos = rect.position.to_physical::<i32>(scale_factor);
        let tray_size = rect.size.to_physical::<u32>(scale_factor);
        let x = tray_pos.x + tray_size.width as i32 - panel_size.width as i32;
        let y = tray_pos.y - panel_size.height as i32 - 8;
        return clamp_position_to_work_area(app, PhysicalPosition::new(x, y), panel_size);
    }

    let fallback = app
        .primary_monitor()
        .ok()
        .flatten()
        .map(|monitor| {
            let work = monitor.work_area();
            PhysicalPosition::new(
                work.position.x + work.size.width as i32 - panel_size.width as i32 - 16,
                work.position.y + work.size.height as i32 - panel_size.height as i32 - 16,
            )
        })
        .unwrap_or_else(|| PhysicalPosition::new(200, 200));
    clamp_position_to_work_area(app, fallback, panel_size)
}

fn clamp_position_to_work_area(
    app: &AppHandle,
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
) -> PhysicalPosition<i32> {
    let monitors = app.available_monitors().unwrap_or_default();
    let target_monitor = monitors
        .iter()
        .find(|monitor| {
            let work = monitor.work_area();
            position.x >= work.position.x
                && position.x <= work.position.x + work.size.width as i32
                && position.y >= work.position.y
                && position.y <= work.position.y + work.size.height as i32
        })
        .or_else(|| monitors.first());

    let Some(monitor) = target_monitor else {
        return position;
    };
    let work = monitor.work_area();
    let min_x = work.position.x + SCREEN_MARGIN;
    let min_y = work.position.y + SCREEN_MARGIN;
    let max_x = work.position.x + work.size.width as i32 - size.width as i32 - SCREEN_MARGIN;
    let max_y = work.position.y + work.size.height as i32 - size.height as i32 - SCREEN_MARGIN;

    PhysicalPosition::new(
        position.x.clamp(min_x, max_x),
        position.y.clamp(min_y, max_y),
    )
}

fn clamp_large_size(size: SavedSize) -> SavedSize {
    SavedSize {
        width: size.width.clamp(LARGE_MIN_W, LARGE_MAX_W),
        height: size.height.clamp(LARGE_MIN_H, LARGE_MAX_H),
    }
}

fn clamp_ring_size(size: SavedSize) -> SavedSize {
    let width = size.width.clamp(RING_SIZE, RING_MAX_SIZE);
    let height = size.height.clamp(RING_SIZE, RING_MAX_SIZE);
    SavedSize { width, height }
}

fn default_ring_size() -> SavedSize {
    SavedSize {
        width: RING_SIZE,
        height: RING_SIZE,
    }
}

fn load_large_size(persisted: &SavedWindowState) -> Option<SavedSize> {
    persisted.large_size.clone().map(clamp_large_size)
}

fn load_ring_size(persisted: &SavedWindowState) -> Option<SavedSize> {
    persisted.ring_size.clone().map(clamp_ring_size)
}

#[cfg(windows)]
fn configure_ring_native_frame(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_STYLE, SWP_FRAMECHANGED,
        SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WS_MAXIMIZEBOX, WS_MINIMIZEBOX,
        WS_SYSMENU,
    };

    let hwnd = window.hwnd()?.0;

    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        let caption_controls = (WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX) as isize;
        let next_style = style & !caption_controls;
        if next_style == style {
            return Ok(());
        }
        SetWindowLongPtrW(hwnd, GWL_STYLE, next_style);
        if SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
        ) == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn configure_ring_native_frame(_window: &tauri::WebviewWindow) -> tauri::Result<()> {
    Ok(())
}

#[cfg(windows)]
fn set_ring_outer_size(
    window: &tauri::WebviewWindow,
    logical_width: f64,
    logical_height: f64,
) -> tauri::Result<()> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER,
    };

    let hwnd = window.hwnd()?.0;
    let scale_factor = window.scale_factor()?;
    let width = (logical_width * scale_factor).round() as i32;
    let height = (logical_height * scale_factor).round() as i32;

    unsafe {
        if SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            0,
            0,
            width,
            height,
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOZORDER,
        ) == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn set_ring_outer_size(
    window: &tauri::WebviewWindow,
    logical_width: f64,
    logical_height: f64,
) -> tauri::Result<()> {
    window.set_size(Size::Logical(LogicalSize::new(
        logical_width,
        logical_height,
    )))
}

fn state_file(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|path| path.join(STATE_FILE))
}

fn load_persisted_state(app: &AppHandle) -> Option<SavedWindowState> {
    let path = state_file(app)?;
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn save_persisted_state(app: &AppHandle) {
    let Ok(_write_guard) = STATE_WRITE_LOCK.lock() else {
        return;
    };
    let Some(path) = state_file(app) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let state = get_runtime_state(app);
    let saved = SavedWindowState {
        schema_version: Some(WINDOW_STATE_SCHEMA_VERSION),
        size_format: Some(WINDOW_SIZE_FORMAT.into()),
        position: state.position,
        large_size: state.large_size,
        ring_size: state.ring_size,
        ring_size_format: None,
        always_on_top: Some(state.always_on_top),
        mode: Some(state.mode),
    };
    if let Ok(text) = serde_json::to_string_pretty(&saved) {
        let temporary = path.with_extension("json.tmp");
        if fs::write(&temporary, text).is_ok() && fs::rename(&temporary, &path).is_err() {
            let _ = fs::remove_file(&path);
            let _ = fs::rename(&temporary, &path);
        }
    }
}

fn schedule_persisted_state(app: &AppHandle) {
    let revision = match app.state::<SharedWindowState>().lock() {
        Ok(mut state) => {
            state.persist_revision = state.persist_revision.wrapping_add(1);
            state.persist_revision
        }
        Err(_) => return,
    };
    let app = app.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(STATE_WRITE_DEBOUNCE_MS));
        let is_latest = app
            .state::<SharedWindowState>()
            .lock()
            .map(|state| state.persist_revision == revision)
            .unwrap_or(false);
        if is_latest {
            save_persisted_state(&app);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_size_at_a_minimum_edge_keeps_native_dimensions() {
        let size = clamp_ring_size(SavedSize {
            width: 125.0,
            height: RING_SIZE,
        });

        assert_eq!(size.width, 125.0);
        assert_eq!(size.height, RING_SIZE);
    }

    #[test]
    fn ring_size_above_the_minimum_keeps_native_rectangle_dimensions() {
        let size = clamp_ring_size(SavedSize {
            width: 150.0,
            height: 180.0,
        });

        assert_eq!(size.width, 150.0);
        assert_eq!(size.height, 180.0);
    }

    #[test]
    fn ring_mode_uses_the_dedicated_window() {
        assert_eq!(window_label_for_mode("ring"), RING_WINDOW_LABEL);
        assert_eq!(window_label_for_mode("large"), MAIN_WINDOW_LABEL);
    }

    #[test]
    fn legal_large_size_is_not_treated_as_a_legacy_default() {
        let persisted = SavedWindowState {
            large_size: Some(SavedSize {
                width: 220.0,
                height: 60.0,
            }),
            ..Default::default()
        };

        let size = load_large_size(&persisted).unwrap();
        assert_eq!(size.width, 220.0);
        assert_eq!(size.height, 60.0);
    }

    #[test]
    fn legacy_ring_size_format_preserves_legal_dimensions() {
        let persisted = SavedWindowState {
            ring_size: Some(SavedSize {
                width: 131.0,
                height: 100.0,
            }),
            ring_size_format: Some("logical-inner-runtime-v12".into()),
            ..Default::default()
        };

        let size = load_ring_size(&persisted).unwrap();
        assert_eq!(size.width, 131.0);
        assert_eq!(size.height, RING_SIZE);
    }

    #[test]
    fn current_outer_size_format_preserves_rectangle() {
        let persisted = SavedWindowState {
            schema_version: Some(WINDOW_STATE_SCHEMA_VERSION),
            size_format: Some(WINDOW_SIZE_FORMAT.into()),
            ring_size: Some(SavedSize {
                width: 131.0,
                height: 100.0,
            }),
            ..Default::default()
        };

        let size = load_ring_size(&persisted).unwrap();
        assert_eq!(size.width, 131.0);
        assert_eq!(size.height, 100.0);
    }

    #[test]
    fn ring_size_clamps_each_native_edge_independently() {
        let persisted = SavedWindowState {
            ring_size: Some(SavedSize {
                width: 90.0,
                height: 340.0,
            }),
            ..Default::default()
        };

        let size = load_ring_size(&persisted).unwrap();
        assert_eq!(size.width, RING_SIZE);
        assert_eq!(size.height, RING_MAX_SIZE);
    }
}
