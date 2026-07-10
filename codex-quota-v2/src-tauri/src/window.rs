use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, sync::Mutex, thread, time::Duration};
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, PhysicalSize, Position, Rect, Size,
};

const SMALL_W: f64 = 210.0;
const SMALL_H: f64 = 30.0;
const LARGE_W: f64 = 200.0;
const LARGE_H: f64 = 112.0;
const LARGE_MIN_W: f64 = 200.0;
const LARGE_MIN_H: f64 = 100.0;
const LARGE_MAX_W: f64 = 380.0;
const LARGE_MAX_H: f64 = 200.0;
const LEGACY_LARGE_DEFAULT_WIDTHS: [f64; 3] = [220.0, 242.0, 252.0];
const SCREEN_MARGIN: i32 = 8;
const STATE_FILE: &str = "window-state.json";

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
    position: Option<SavedPosition>,
    large_size: Option<SavedSize>,
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
}

impl Default for RuntimeWindowState {
    fn default() -> Self {
        Self {
            mode: "small".into(),
            always_on_top: true,
            visible: false,
            position: None,
            large_size: None,
        }
    }
}

type SharedWindowState = Mutex<RuntimeWindowState>;

pub fn init_state(app: &AppHandle) -> tauri::Result<()> {
    let persisted = load_persisted_state(app).unwrap_or_default();
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
        large_size: persisted.large_size.map(clamp_large_size),
    };
    app.manage(SharedWindowState::new(state));
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
    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };

    let normalized = normalize_mode(mode);
    configure_window_size(app, normalized)?;
    let (logical_width, logical_height) = panel_size(app, normalized);

    window.set_size(Size::Logical(LogicalSize::new(
        logical_width,
        logical_height,
    )))?;

    let scale_factor = window.scale_factor().unwrap_or(1.0);
    let physical_size = logical_size_to_physical(logical_width, logical_height, scale_factor);
    let position = preferred_position(app, tray_rect, physical_size);
    window.set_position(Position::Physical(position))?;

    let always_on_top = get_runtime_state(app).always_on_top;
    window.set_always_on_top(always_on_top)?;
    window.show()?;
    window.set_focus()?;

    if let Ok(mut state) = app.state::<SharedWindowState>().lock() {
        state.mode = normalized.into();
        state.visible = true;
    }
    let _ = app.emit("mode-changed", normalized);
    let _ = app.emit("panel-visibility-changed", true);
    save_persisted_state(app);
    Ok(())
}

pub fn hide_panel(app: &AppHandle) -> tauri::Result<()> {
    remember_window_state(app);
    if let Some(window) = app.get_webview_window("main") {
        window.hide()?;
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

pub fn toggle_topmost(app: &AppHandle) -> tauri::Result<bool> {
    let next = !get_runtime_state(app).always_on_top;
    if let Some(window) = app.get_webview_window("main") {
        window.set_always_on_top(next)?;
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

pub fn remember_window_state(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };

    let position = window.outer_position().ok();
    let size = window.inner_size().ok();
    let scale_factor = window.scale_factor().unwrap_or(1.0);

    if let Ok(mut state) = app.state::<SharedWindowState>().lock() {
        if let Some(position) = position {
            state.position = Some(SavedPosition {
                x: position.x,
                y: position.y,
            });
        }
        if state.mode == "large" {
            if let Some(size) = size {
                let logical = size.to_logical::<f64>(scale_factor);
                state.large_size = Some(clamp_large_size(SavedSize {
                    width: logical.width,
                    height: logical.height,
                }));
            }
        }
    }
    save_persisted_state(app);
}

fn bring_to_front(app: &AppHandle) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };
    let state = get_runtime_state(app);
    if !state.always_on_top {
        window.set_always_on_top(true)?;
    }
    window.show()?;
    window.set_focus()?;

    if !state.always_on_top {
        let app_handle = app.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(140));
            if !get_runtime_state(&app_handle).always_on_top {
                if let Some(window) = app_handle.get_webview_window("main") {
                    let _ = window.set_always_on_top(false);
                }
            }
        });
    }
    Ok(())
}

fn configure_window_size(app: &AppHandle, mode: &str) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window("main") else {
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
            SMALL_W,
            SMALL_H,
        ))))?;
        window.set_max_size(Some(Size::Logical(LogicalSize::new(
            SMALL_W,
            SMALL_H,
        ))))?;
    }
    Ok(())
}

fn normalize_mode(mode: &str) -> &'static str {
    if mode == "large" {
        "large"
    } else {
        "small"
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
    } else {
        (SMALL_W, SMALL_H)
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
        return clamp_position_to_work_area(app, PhysicalPosition::new(saved.x, saved.y), panel_size);
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

    PhysicalPosition::new(position.x.clamp(min_x, max_x), position.y.clamp(min_y, max_y))
}

fn clamp_large_size(size: SavedSize) -> SavedSize {
    let clamped_width = size.width.clamp(LARGE_MIN_W, LARGE_MAX_W);
    let width = if LEGACY_LARGE_DEFAULT_WIDTHS
        .iter()
        .any(|legacy| (clamped_width - legacy).abs() <= 1.0)
    {
        LARGE_W
    } else {
        clamped_width
    };

    SavedSize {
        width,
        height: size.height.clamp(LARGE_MIN_H, LARGE_MAX_H),
    }
}

fn state_file(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|path| path.join(STATE_FILE))
}

fn load_persisted_state(app: &AppHandle) -> Option<SavedWindowState> {
    let path = state_file(app)?;
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn save_persisted_state(app: &AppHandle) {
    let Some(path) = state_file(app) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let state = get_runtime_state(app);
    let saved = SavedWindowState {
        position: state.position,
        large_size: state.large_size,
        always_on_top: Some(state.always_on_top),
        mode: Some(state.mode),
    };
    if let Ok(text) = serde_json::to_string_pretty(&saved) {
        let _ = fs::write(path, text);
    }
}
