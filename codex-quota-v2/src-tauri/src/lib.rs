mod quota;
mod tray;
mod updates;
mod window;

use quota::QuotaSnapshot;
use serde::Deserialize;
use tauri::{AppHandle, Emitter, Manager, WindowEvent};

#[tauri::command]
async fn read_quota(app: AppHandle) -> Result<QuotaSnapshot, String> {
    let snapshot = quota::read_quota()
        .await
        .map_err(|error| error.to_string())?;
    let previous = quota::read_cached_quota(&app);
    quota::reject_suspicious_full_snapshot(&snapshot, previous.as_ref())
        .map_err(|error| error.to_string())?;
    quota::write_cached_quota(&app, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
fn read_cached_quota(app: AppHandle) -> Option<QuotaSnapshot> {
    quota::read_cached_quota(&app)
}

#[tauri::command]
fn show_panel(app: AppHandle, mode: String) -> Result<(), String> {
    window::show_panel(&app, &mode).map_err(|error| error.to_string())
}

#[tauri::command]
fn hide_panel(app: AppHandle) -> Result<(), String> {
    window::hide_panel(&app).map_err(|error| error.to_string())
}

#[tauri::command]
fn toggle_topmost(app: AppHandle) -> Result<bool, String> {
    let next = window::toggle_topmost(&app).map_err(|error| error.to_string())?;
    tray::refresh_menu(&app).map_err(|error| error.to_string())?;
    Ok(next)
}

#[tauri::command]
fn get_window_state(app: AppHandle) -> window::WindowState {
    window::get_state(&app)
}

#[tauri::command]
fn set_mode(app: AppHandle, mode: String) -> Result<(), String> {
    window::set_mode(&app, &mode).map_err(|error| error.to_string())
}

#[tauri::command]
fn remember_window_state(app: AppHandle) {
    window::remember_window_state(&app);
}

#[tauri::command]
fn show_context_menu(app: AppHandle) -> Result<(), String> {
    tray::popup_context_menu(&app).map_err(|error| error.to_string())
}

#[tauri::command]
fn set_autostart_menu_checked(app: AppHandle, checked: bool) -> Result<(), String> {
    tray::set_autostart_checked(&app, checked).map_err(|error| error.to_string())
}

#[tauri::command]
fn set_auto_refresh_menu_seconds(app: AppHandle, seconds: u32) -> Result<(), String> {
    tray::set_auto_refresh_seconds(&app, seconds).map_err(|error| error.to_string())
}

#[tauri::command]
fn set_appearance_menu_state(
    app: AppHandle,
    color_scheme: String,
    dark_mode: bool,
    opacity: u32,
) -> Result<(), String> {
    tray::set_appearance(&app, &color_scheme, dark_mode, opacity).map_err(|error| error.to_string())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TrayQuotaState {
    primary_remaining: Option<i64>,
    secondary_remaining: Option<i64>,
    status: String,
}

#[tauri::command]
fn update_tray_quota(app: AppHandle, state: TrayQuotaState) -> Result<(), String> {
    tray::update_quota_icon(
        &app,
        state.primary_remaining,
        state.secondary_remaining,
        &state.status,
    )
    .map_err(|error| error.to_string())
}

#[tauri::command]
fn set_update_available(
    app: AppHandle,
    available: bool,
    latest_version: Option<String>,
) -> Result<(), String> {
    tray::set_update_available(&app, available, latest_version).map_err(|error| error.to_string())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .on_window_event(|window, event| {
            if window.label() == "main"
                && matches!(event, WindowEvent::Moved(_) | WindowEvent::Resized(_))
            {
                let app = window.app_handle().clone();
                window::remember_window_state(&app);
            }
        })
        .setup(|app| {
            if updates::prepare_portable_runtime(app.handle()) {
                return Ok(());
            }
            window::init_state(app.handle())?;
            tray::init_tray(app.handle())?;
            if let Some(window) = app.get_webview_window("main") {
                window::show_panel(app.handle(), "small")?;
                window.emit("quota-refresh-requested", ())?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            read_quota,
            read_cached_quota,
            show_panel,
            hide_panel,
            toggle_topmost,
            get_window_state,
            set_mode,
            remember_window_state,
            show_context_menu,
            set_autostart_menu_checked,
            set_auto_refresh_menu_seconds,
            set_appearance_menu_state,
            update_tray_quota,
            set_update_available,
            updates::check_update,
            updates::download_portable_update
        ])
        .run(tauri::generate_context!())
        .expect("error while running Codex Quota Monitor");
}
