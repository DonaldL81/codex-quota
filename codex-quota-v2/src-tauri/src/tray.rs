use crate::window;
use std::sync::Mutex;
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};

const TRAY_ID: &str = "main";
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const AUTO_REFRESH_PRESETS: &[(u32, &str)] = &[
    (30, "30s"),
    (60, "1min"),
    (300, "5min"),
    (600, "10min"),
    (1200, "20min"),
    (1800, "30min"),
    (3600, "60min"),
];
const COLOR_SCHEMES: &[(&str, &str)] = &[
    ("red", "红"),
    ("orange", "橙"),
    ("yellow", "黄"),
    ("green", "绿"),
    ("cyan", "青"),
    ("blue", "蓝"),
    ("purple", "紫"),
    ("black", "黑"),
    ("white", "白"),
];
const OPACITY_PRESETS: &[(u32, &str)] = &[
    (100, "100%"),
    (90, "90%"),
    (80, "80%"),
    (70, "70%"),
    (60, "60%"),
    (50, "50%"),
    (40, "40%"),
    (30, "30%"),
    (20, "20%"),
    (10, "10%"),
];
type Color = [u8; 4];

struct AutostartMenuState {
    checked: Mutex<bool>,
}

struct AutoRefreshMenuState {
    seconds: Mutex<u32>,
}

struct AppearanceMenuState {
    color_scheme: Mutex<String>,
    dark_mode: Mutex<bool>,
    opacity: Mutex<u32>,
}

struct TrayVisualState {
    primary_remaining: Mutex<Option<i64>>,
    secondary_remaining: Mutex<Option<i64>>,
    status: Mutex<String>,
    update_available: Mutex<bool>,
    update_checked: Mutex<bool>,
    latest_version: Mutex<Option<String>>,
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let small = MenuItem::with_id(app, "small", "打开小窗", true, None::<&str>)?;
    let large = MenuItem::with_id(app, "large", "打开大窗", true, None::<&str>)?;
    let topmost = CheckMenuItem::with_id(
        app,
        "topmost",
        "置顶窗口",
        true,
        window::get_state(app).always_on_top,
        None::<&str>,
    )?;
    let autostart = CheckMenuItem::with_id(
        app,
        "autostart",
        "开机自启动",
        true,
        autostart_checked(app),
        None::<&str>,
    )?;
    let current_auto_refresh_seconds = auto_refresh_seconds(app);
    let auto_refresh_30s = auto_refresh_item(app, 30, "30s", current_auto_refresh_seconds)?;
    let auto_refresh_1min = auto_refresh_item(app, 60, "1min", current_auto_refresh_seconds)?;
    let auto_refresh_5min = auto_refresh_item(app, 300, "5min", current_auto_refresh_seconds)?;
    let auto_refresh_10min = auto_refresh_item(app, 600, "10min", current_auto_refresh_seconds)?;
    let auto_refresh_20min = auto_refresh_item(app, 1200, "20min", current_auto_refresh_seconds)?;
    let auto_refresh_30min = auto_refresh_item(app, 1800, "30min", current_auto_refresh_seconds)?;
    let auto_refresh_60min = auto_refresh_item(app, 3600, "60min", current_auto_refresh_seconds)?;
    let auto_refresh = Submenu::with_items(
        app,
        format!("自动刷新 {}", auto_refresh_label(current_auto_refresh_seconds)),
        true,
        &[
            &auto_refresh_30s,
            &auto_refresh_1min,
            &auto_refresh_5min,
            &auto_refresh_10min,
            &auto_refresh_20min,
            &auto_refresh_30min,
            &auto_refresh_60min,
        ],
    )?;
    let current_color_scheme = color_scheme(app);
    let color_red = color_scheme_item(app, "red", "红", &current_color_scheme)?;
    let color_orange = color_scheme_item(app, "orange", "橙", &current_color_scheme)?;
    let color_yellow = color_scheme_item(app, "yellow", "黄", &current_color_scheme)?;
    let color_green = color_scheme_item(app, "green", "绿", &current_color_scheme)?;
    let color_cyan = color_scheme_item(app, "cyan", "青", &current_color_scheme)?;
    let color_blue = color_scheme_item(app, "blue", "蓝", &current_color_scheme)?;
    let color_purple = color_scheme_item(app, "purple", "紫", &current_color_scheme)?;
    let color_black = color_scheme_item(app, "black", "黑", &current_color_scheme)?;
    let color_white = color_scheme_item(app, "white", "白", &current_color_scheme)?;
    let color_menu = Submenu::with_items(
        app,
        format!("主题色：{}", color_scheme_label(&current_color_scheme)),
        true,
        &[
            &color_red,
            &color_orange,
            &color_yellow,
            &color_green,
            &color_cyan,
            &color_blue,
            &color_purple,
            &color_black,
            &color_white,
        ],
    )?;
    let dark_mode = CheckMenuItem::with_id(
        app,
        "dark-mode",
        "深色模式",
        true,
        dark_mode_checked(app),
        None::<&str>,
    )?;
    let current_opacity = opacity(app);
    let opacity_100 = opacity_item(app, 100, "100%", current_opacity)?;
    let opacity_90 = opacity_item(app, 90, "90%", current_opacity)?;
    let opacity_80 = opacity_item(app, 80, "80%", current_opacity)?;
    let opacity_70 = opacity_item(app, 70, "70%", current_opacity)?;
    let opacity_60 = opacity_item(app, 60, "60%", current_opacity)?;
    let opacity_50 = opacity_item(app, 50, "50%", current_opacity)?;
    let opacity_40 = opacity_item(app, 40, "40%", current_opacity)?;
    let opacity_30 = opacity_item(app, 30, "30%", current_opacity)?;
    let opacity_20 = opacity_item(app, 20, "20%", current_opacity)?;
    let opacity_10 = opacity_item(app, 10, "10%", current_opacity)?;
    let opacity_menu = Submenu::with_items(
        app,
        format!("透明度 {}", opacity_label(current_opacity)),
        true,
        &[
            &opacity_100,
            &opacity_90,
            &opacity_80,
            &opacity_70,
            &opacity_60,
            &opacity_50,
            &opacity_40,
            &opacity_30,
            &opacity_20,
            &opacity_10,
        ],
    )?;
    let update_available = update_available(app);
    let version_label = if update_available {
        update_version_label(app)
    } else if update_checked(app) {
        format!("版本 {}（最新）", display_version())
    } else {
        format!("版本 {}", display_version())
    };
    let restart = MenuItem::with_id(app, "restart", "重启", true, None::<&str>)?;
    let version = MenuItem::with_id(app, "version", version_label, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let separator_1 = PredefinedMenuItem::separator(app)?;
    let separator_2 = PredefinedMenuItem::separator(app)?;
    let separator_3 = PredefinedMenuItem::separator(app)?;
    let separator_4 = PredefinedMenuItem::separator(app)?;

    Menu::with_items(
        app,
        &[
            &small,
            &large,
            &topmost,
            &separator_1,
            &color_menu,
            &dark_mode,
            &opacity_menu,
            &separator_2,
            &auto_refresh,
            &separator_3,
            &autostart,
            &restart,
            &quit,
            &separator_4,
            &version,
        ],
    )
}

pub fn set_autostart_checked(app: &AppHandle, checked: bool) -> tauri::Result<()> {
    if let Some(state) = app.try_state::<AutostartMenuState>() {
        if let Ok(mut current) = state.checked.lock() {
            *current = checked;
        }
    }

    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_menu(Some(build_menu(app)?))?;
    }
    Ok(())
}

pub fn set_auto_refresh_seconds(app: &AppHandle, seconds: u32) -> tauri::Result<()> {
    if let Some(state) = app.try_state::<AutoRefreshMenuState>() {
        if let Ok(mut current) = state.seconds.lock() {
            *current = seconds;
        }
    }

    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_menu(Some(build_menu(app)?))?;
    }
    Ok(())
}

fn update_appearance_state(
    app: &AppHandle,
    color_scheme: &str,
    dark_mode: bool,
    opacity: u32,
) {
    if let Some(state) = app.try_state::<AppearanceMenuState>() {
        if is_color_scheme(color_scheme) {
            if let Ok(mut current) = state.color_scheme.lock() {
                *current = color_scheme.to_string();
            }
        }
        if let Ok(mut current) = state.dark_mode.lock() {
            *current = dark_mode;
        }
        if is_opacity_preset(opacity) {
            if let Ok(mut current) = state.opacity.lock() {
                *current = opacity;
            }
        }
    }
}

pub fn set_appearance(
    app: &AppHandle,
    color_scheme: &str,
    dark_mode: bool,
    opacity: u32,
) -> tauri::Result<()> {
    update_appearance_state(app, color_scheme, dark_mode, opacity);

    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_menu(Some(build_menu(app)?))?;
    }
    Ok(())
}

pub fn set_update_available(
    app: &AppHandle,
    available: bool,
    latest_version: Option<String>,
) -> tauri::Result<()> {
    if let Some(state) = app.try_state::<TrayVisualState>() {
        if let Ok(mut current) = state.update_available.lock() {
            *current = available;
        }
        if let Ok(mut current) = state.update_checked.lock() {
            *current = true;
        }
        if let Ok(mut current) = state.latest_version.lock() {
            *current = available
                .then(|| latest_version.unwrap_or_default())
                .filter(|value| !value.is_empty());
        }
        render_tray_state(app, &state)?;
    }
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_menu(Some(build_menu(app)?))?;
    }
    Ok(())
}

pub fn refresh_menu(app: &AppHandle) -> tauri::Result<()> {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_menu(Some(build_menu(app)?))?;
    }
    Ok(())
}

pub fn popup_context_menu(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window("main") {
        let menu = build_menu(app)?;
        window.popup_menu(&menu)?;
    }
    Ok(())
}

pub fn init_tray(app: &AppHandle) -> tauri::Result<()> {
    app.manage(AutostartMenuState {
        checked: Mutex::new(false),
    });
    app.manage(AutoRefreshMenuState {
        seconds: Mutex::new(30),
    });
    app.manage(AppearanceMenuState {
        color_scheme: Mutex::new("blue".into()),
        dark_mode: Mutex::new(false),
        opacity: Mutex::new(90),
    });
    app.manage(TrayVisualState {
        primary_remaining: Mutex::new(None),
        secondary_remaining: Mutex::new(None),
        status: Mutex::new("idle".into()),
        update_available: Mutex::new(false),
        update_checked: Mutex::new(false),
        latest_version: Mutex::new(None),
    });

    let menu = build_menu(app)?;

    TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("Codex 额度")
        .icon(create_tray_image(None, None, "idle", false))
        .on_menu_event(|app, event| match event.id().as_ref() {
            "small" => {
                let _ = window::show_panel(app, "small");
            }
            "large" => {
                let _ = window::show_panel(app, "large");
            }
            "topmost" => {
                let _ = window::toggle_topmost(app);
                let _ = refresh_menu(app);
            }
            "autostart" => {
                let _ = app.emit("toggle-autostart-requested", ());
            }
            id if id.starts_with("auto-refresh-") => {
                if let Some(seconds) = parse_auto_refresh_menu_id(id) {
                    let _ = set_auto_refresh_seconds(app, seconds);
                    let _ = app.emit("auto-refresh-seconds-changed", seconds);
                }
            }
            id if id.starts_with("color-scheme-") => {
                if let Some(color_scheme) = parse_color_scheme_menu_id(id) {
                    let dark_mode = dark_mode_checked(app);
                    let current_opacity = opacity(app);
                    let _ = set_appearance(app, color_scheme, dark_mode, current_opacity);
                    let _ = app.emit("color-scheme-changed", color_scheme);
                }
            }
            "dark-mode" => {
                let next_dark_mode = !dark_mode_checked(app);
                let color_scheme = color_scheme(app);
                let current_opacity = opacity(app);
                let _ = set_appearance(app, &color_scheme, next_dark_mode, current_opacity);
                let _ = app.emit("dark-mode-changed", next_dark_mode);
            }
            id if id.starts_with("opacity-") => {
                if let Some(next_opacity) = parse_opacity_menu_id(id) {
                    let color_scheme = color_scheme(app);
                    let dark_mode = dark_mode_checked(app);
                    let _ = set_appearance(app, &color_scheme, dark_mode, next_opacity);
                    let _ = app.emit("opacity-changed", next_opacity);
                }
            }
            "version" => {
                if update_available(app) {
                    let _ = window::show_panel(app, "large");
                    let _ = app.emit("update-download-requested", ());
                } else {
                    let _ = app.emit("update-check-requested", ());
                }
            }
            "restart" => {
                app.restart();
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                rect,
                ..
            } = event
            {
                let app = tray.app_handle();
                let _ = window::handle_tray_left_click(&app, rect);
            }
        })
        .build(app)?;
    Ok(())
}

fn autostart_checked(app: &AppHandle) -> bool {
    app.try_state::<AutostartMenuState>()
        .and_then(|state| state.checked.lock().ok().map(|checked| *checked))
        .unwrap_or(false)
}

fn auto_refresh_seconds(app: &AppHandle) -> u32 {
    app.try_state::<AutoRefreshMenuState>()
        .and_then(|state| state.seconds.lock().ok().map(|seconds| *seconds))
        .unwrap_or(30)
}

fn auto_refresh_item(
    app: &AppHandle,
    seconds: u32,
    label: &str,
    current_seconds: u32,
) -> tauri::Result<CheckMenuItem<tauri::Wry>> {
    CheckMenuItem::with_id(
        app,
        format!("auto-refresh-{seconds}"),
        label,
        true,
        seconds == current_seconds,
        None::<&str>,
    )
}

fn parse_auto_refresh_menu_id(id: &str) -> Option<u32> {
    let seconds = id.strip_prefix("auto-refresh-")?.parse::<u32>().ok()?;
    AUTO_REFRESH_PRESETS
        .iter()
        .any(|(preset_seconds, _)| *preset_seconds == seconds)
        .then_some(seconds)
}

fn auto_refresh_label(seconds: u32) -> String {
    AUTO_REFRESH_PRESETS
        .iter()
        .find_map(|(preset_seconds, label)| (*preset_seconds == seconds).then_some(*label))
        .unwrap_or("30s")
        .to_string()
}

fn color_scheme(app: &AppHandle) -> String {
    app.try_state::<AppearanceMenuState>()
        .and_then(|state| state.color_scheme.lock().ok().map(|scheme| scheme.clone()))
        .filter(|scheme| is_color_scheme(scheme))
        .unwrap_or_else(|| "blue".into())
}

fn dark_mode_checked(app: &AppHandle) -> bool {
    app.try_state::<AppearanceMenuState>()
        .and_then(|state| state.dark_mode.lock().ok().map(|dark_mode| *dark_mode))
        .unwrap_or(false)
}

fn update_available(app: &AppHandle) -> bool {
    app.try_state::<TrayVisualState>()
        .and_then(|state| {
            state
                .update_available
                .lock()
                .ok()
                .map(|update_available| *update_available)
        })
        .unwrap_or(false)
}

fn update_checked(app: &AppHandle) -> bool {
    app.try_state::<TrayVisualState>()
        .and_then(|state| {
            state
                .update_checked
                .lock()
                .ok()
                .map(|update_checked| *update_checked)
        })
        .unwrap_or(false)
}

fn latest_version(app: &AppHandle) -> Option<String> {
    app.try_state::<TrayVisualState>().and_then(|state| {
        state
            .latest_version
            .lock()
            .ok()
            .and_then(|latest_version| latest_version.clone())
    })
}

fn update_version_label(app: &AppHandle) -> String {
    latest_version(app)
        .map(|version| format!("更新{} » {}", display_version(), version))
        .unwrap_or_else(|| "更新到最新版本".to_string())
}

fn opacity(app: &AppHandle) -> u32 {
    app.try_state::<AppearanceMenuState>()
        .and_then(|state| state.opacity.lock().ok().map(|opacity| *opacity))
        .filter(|opacity| is_opacity_preset(*opacity))
        .unwrap_or(90)
}

fn color_scheme_item(
    app: &AppHandle,
    scheme: &str,
    label: &str,
    current_scheme: &str,
) -> tauri::Result<CheckMenuItem<tauri::Wry>> {
    CheckMenuItem::with_id(
        app,
        format!("color-scheme-{scheme}"),
        label,
        true,
        scheme == current_scheme,
        None::<&str>,
    )
}

fn parse_color_scheme_menu_id(id: &str) -> Option<&'static str> {
    let scheme = id.strip_prefix("color-scheme-")?;
    COLOR_SCHEMES
        .iter()
        .find_map(|(preset_scheme, _)| (*preset_scheme == scheme).then_some(*preset_scheme))
}

fn color_scheme_label(scheme: &str) -> &str {
    COLOR_SCHEMES
        .iter()
        .find_map(|(preset_scheme, label)| (*preset_scheme == scheme).then_some(*label))
        .unwrap_or("蓝")
}

fn is_color_scheme(scheme: &str) -> bool {
    COLOR_SCHEMES
        .iter()
        .any(|(preset_scheme, _)| *preset_scheme == scheme)
}

fn opacity_item(
    app: &AppHandle,
    opacity: u32,
    label: &str,
    current_opacity: u32,
) -> tauri::Result<CheckMenuItem<tauri::Wry>> {
    CheckMenuItem::with_id(
        app,
        format!("opacity-{opacity}"),
        label,
        true,
        opacity == current_opacity,
        None::<&str>,
    )
}

fn parse_opacity_menu_id(id: &str) -> Option<u32> {
    let opacity = id.strip_prefix("opacity-")?.parse::<u32>().ok()?;
    is_opacity_preset(opacity).then_some(opacity)
}

fn opacity_label(opacity: u32) -> String {
    OPACITY_PRESETS
        .iter()
        .find_map(|(preset_opacity, label)| (*preset_opacity == opacity).then_some(*label))
        .unwrap_or("90%")
        .to_string()
}

fn is_opacity_preset(opacity: u32) -> bool {
    OPACITY_PRESETS
        .iter()
        .any(|(preset_opacity, _)| *preset_opacity == opacity)
}

fn display_version() -> String {
    APP_VERSION
        .strip_suffix(".0")
        .unwrap_or(APP_VERSION)
        .to_string()
}

pub fn update_quota_icon(
    app: &AppHandle,
    primary_remaining: Option<i64>,
    secondary_remaining: Option<i64>,
    status: &str,
) -> tauri::Result<()> {
    if let Some(state) = app.try_state::<TrayVisualState>() {
        if let Ok(mut current) = state.primary_remaining.lock() {
            *current = primary_remaining;
        }
        if let Ok(mut current) = state.secondary_remaining.lock() {
            *current = secondary_remaining;
        }
        if let Ok(mut current) = state.status.lock() {
            *current = status.to_string();
        }
        render_tray_state(app, &state)?;
    }
    Ok(())
}

fn render_tray_state(app: &AppHandle, state: &TrayVisualState) -> tauri::Result<()> {
    let primary_remaining = state
        .primary_remaining
        .lock()
        .ok()
        .and_then(|current| *current);
    let secondary_remaining = state
        .secondary_remaining
        .lock()
        .ok()
        .and_then(|current| *current);
    let status = state
        .status
        .lock()
        .ok()
        .map(|current| current.clone())
        .unwrap_or_else(|| "idle".into());
    let update_available = state
        .update_available
        .lock()
        .ok()
        .map(|current| *current)
        .unwrap_or(false);

    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_icon(Some(create_tray_image(
            primary_remaining,
            secondary_remaining,
            &status,
            update_available,
        )))?;
        tray.set_icon_as_template(false)?;
        tray.set_tooltip(Some(make_tooltip(
            primary_remaining,
            secondary_remaining,
            &status,
            update_available,
        )))?;
    }
    Ok(())
}

fn make_tooltip(
    primary_remaining: Option<i64>,
    secondary_remaining: Option<i64>,
    status: &str,
    update_available: bool,
) -> String {
    let update_text = if update_available { " · 有新版本" } else { "" };
    if status == "error" {
        return format!("Codex 额度暂时无法获取{update_text}");
    }
    match primary_remaining {
        Some(primary) => {
            let weekly = secondary_remaining
                .map(|secondary| format!(" / 周 {secondary}%"))
                .unwrap_or_default();
            format!("Codex 剩余: 5小时 {primary}%{weekly}{update_text}")
        }
        None => format!("正在读取 Codex 额度{update_text}"),
    }
}

fn create_tray_image(
    primary_remaining: Option<i64>,
    secondary_remaining: Option<i64>,
    status: &str,
    update_available: bool,
) -> Image<'static> {
    let size = 32;
    let mut rgba = vec![0; size * size * 4];
    let palette = pick_tray_palette(status);

    fill_cloud_mark(
        &mut rgba,
        size,
        palette.border,
        palette.background,
        palette.track,
        pick_quota_color(primary_remaining, status, palette.dim),
        pick_quota_color(secondary_remaining, status, palette.dim),
    );
    if update_available {
        fill_update_badge(&mut rgba, size);
    }

    Image::new_owned(rgba, size as u32, size as u32)
}

struct TrayPalette {
    dim: Color,
    border: Color,
    background: Color,
    track: Color,
}

fn pick_tray_palette(status: &str) -> TrayPalette {
    let dim = if matches!(status, "error" | "idle" | "loading") {
        [239, 68, 68, 255]
    } else {
        [70, 78, 90, 255]
    };
    TrayPalette {
        dim,
        border: [0, 0, 0, 255],
        background: [8, 10, 14, 255],
        track: [45, 50, 60, 255],
    }
}

fn pick_quota_color(percent: Option<i64>, status: &str, fallback: Color) -> Color {
    if status == "error" {
        return [239, 68, 68, 255];
    }
    let Some(value) = percent else {
        return fallback;
    };
    if value <= 20 {
        [239, 68, 68, 255]
    } else if value <= 50 {
        [245, 158, 11, 255]
    } else {
        [34, 197, 94, 255]
    }
}

fn fill_cloud_mark(
    rgba: &mut [u8],
    canvas_size: usize,
    border: Color,
    background: Color,
    track: Color,
    primary_color: Color,
    secondary_color: Color,
) {
    let scale = canvas_size as f64 / 32.0;
    for y in 0..canvas_size {
        for x in 0..canvas_size {
            let px = (x as f64 + 0.5) / scale;
            let py = (y as f64 + 0.5) / scale;
            let distance = cloud_signed_distance(px, py);
            if distance <= 0.0 {
                let color = if distance > -1.05 {
                    border
                } else {
                    background
                };
                set_pixel(rgba, canvas_size, x, y, color);
            }
        }
    }
    fill_cloud_progress_bar(rgba, canvas_size, 6.9, track);
    fill_cloud_progress_bar(rgba, canvas_size, 6.9, primary_color);
    fill_cloud_progress_bar(rgba, canvas_size, 18.2, track);
    fill_cloud_progress_bar(rgba, canvas_size, 18.2, secondary_color);
}

fn cloud_signed_distance(x: f64, y: f64) -> f64 {
    let circles = [
        (9.0, 14.5, 8.1),
        (13.8, 9.4, 8.1),
        (20.0, 9.2, 7.4),
        (23.4, 15.6, 7.8),
        (21.2, 21.4, 7.8),
        (14.0, 22.1, 7.6),
        (8.7, 19.9, 7.2),
    ];
    circles
        .iter()
        .map(|(cx, cy, radius)| ((x - cx).hypot(y - cy)) - radius)
        .fold(f64::INFINITY, f64::min)
}

fn fill_cloud_progress_bar(rgba: &mut [u8], canvas_size: usize, y: f64, color: Color) {
    let x = 7.2;
    let width = 17.6;
    let height = 6.2;
    fill_rounded_rect_in_cloud(
        rgba,
        canvas_size,
        x,
        y,
        width,
        height,
        height / 2.0,
        color,
    );
}

fn fill_rounded_rect_in_cloud(
    rgba: &mut [u8],
    canvas_size: usize,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    radius: f64,
    color: Color,
) {
    let scale = canvas_size as f64 / 32.0;
    let start_x = (x * scale).floor().max(0.0) as usize;
    let end_x = ((x + width) * scale).ceil().min(canvas_size as f64) as usize;
    let start_y = (y * scale).floor().max(0.0) as usize;
    let end_y = ((y + height) * scale).ceil().min(canvas_size as f64) as usize;

    for py in start_y..end_y {
        for px in start_x..end_x {
            let ux = (px as f64 + 0.5) / scale;
            let uy = (py as f64 + 0.5) / scale;
            if cloud_signed_distance(ux, uy) > -0.8 {
                continue;
            }
            if rounded_rect_signed_distance(ux, uy, x, y, width, height, radius) <= 0.0 {
                set_pixel(rgba, canvas_size, px, py, color);
            }
        }
    }
}

fn rounded_rect_signed_distance(
    px: f64,
    py: f64,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    radius: f64,
) -> f64 {
    let cx = x + width / 2.0;
    let cy = y + height / 2.0;
    let qx = (px - cx).abs() - (width / 2.0 - radius);
    let qy = (py - cy).abs() - (height / 2.0 - radius);
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius
}

fn set_pixel(rgba: &mut [u8], canvas_size: usize, x: usize, y: usize, color: Color) {
    if x >= canvas_size || y >= canvas_size {
        return;
    }
    let index = (y * canvas_size + x) * 4;
    rgba[index] = color[0];
    rgba[index + 1] = color[1];
    rgba[index + 2] = color[2];
    rgba[index + 3] = color[3];
}

fn fill_update_badge(rgba: &mut [u8], canvas_size: usize) {
    let scale = canvas_size as f64 / 32.0;
    let cx = 24.4;
    let cy = 7.5;
    let outer_radius = 5.7;
    let inner_radius = 4.2;
    for y in 0..canvas_size {
        for x in 0..canvas_size {
            let px = (x as f64 + 0.5) / scale;
            let py = (y as f64 + 0.5) / scale;
            let distance = (px - cx).hypot(py - cy);
            if distance <= outer_radius {
                let color = if distance > inner_radius {
                    [255, 255, 255, 255]
                } else {
                    [239, 68, 68, 255]
                };
                set_pixel(rgba, canvas_size, x, y, color);
            }
        }
    }
}
