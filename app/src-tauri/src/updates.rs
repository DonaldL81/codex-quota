use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::AsyncWriteExt;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const RELEASE_API_URL: &str = "https://api.github.com/repos/DonaldL81/codex-quota/releases/latest";
const USER_AGENT: &str = concat!("Codex-Quota-Monitor/", env!("CARGO_PKG_VERSION"));
const APP_NAME: &str = "Codex Quota Monitor";
const STABLE_PORTABLE_EXE: &str = "Codex Quota Monitor.exe";
const NO_SHORTCUT_MARKER_FILE: &str = ".codex-quota-no-shortcut";
const SHORTCUT_NAME: &str = "Codex Quota Monitor.lnk";
const SHORTCUT_STATE_FILE: &str = "shortcut-state.json";
const PORTABLE_UPDATER_SCRIPT: &str = include_str!("../../scripts/portable-updater.ps1");
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub available: bool,
    pub current_version: String,
    pub latest_version: String,
    pub release_url: String,
    pub portable_asset_url: Option<String>,
    pub portable_file_name: Option<String>,
    pub portable_asset_size: Option<u64>,
    pub message: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct UpdateProgress {
    phase: String,
    percent: u8,
    downloaded: u64,
    total: Option<u64>,
    message: String,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
    size: Option<u64>,
}

#[tauri::command]
pub async fn check_update() -> Result<UpdateInfo, String> {
    let release = github_release().await?;
    let latest_version = normalize_version(&release.tag_name)
        .or_else(|| {
            release
                .assets
                .iter()
                .find_map(|asset| version_from_asset(&asset.name))
        })
        .unwrap_or_else(|| CURRENT_VERSION.to_string());
    let portable_asset = find_asset(&release.assets, "portable.exe");
    let available = is_newer_version(&latest_version, CURRENT_VERSION);

    let message = if available {
        format!("发现新版本 {latest_version}")
    } else {
        format!("当前已是最新版本 {CURRENT_VERSION}")
    };

    Ok(UpdateInfo {
        available,
        current_version: CURRENT_VERSION.to_string(),
        latest_version,
        release_url: release.html_url,
        portable_asset_url: portable_asset.map(|asset| asset.browser_download_url.clone()),
        portable_file_name: portable_asset.map(|asset| asset.name.clone()),
        portable_asset_size: portable_asset.and_then(|asset| asset.size),
        message,
    })
}

#[tauri::command]
pub async fn download_portable_update(
    app: AppHandle,
    url: String,
    file_name: Option<String>,
    expected_size: Option<u64>,
) -> Result<String, String> {
    let file_name = safe_file_name(
        file_name.as_deref(),
        &url,
        "Codex-Quota-Update-Portable.exe",
    );
    let stage_path = app
        .path()
        .app_cache_dir()
        .map_err(|error| format!("无法读取缓存目录: {error}"))?
        .join("updates")
        .join(file_name);
    let target_path = portable_target_path()?;

    emit_progress(&app, "downloading", 0, 0, None, "正在下载便携版更新");
    let saved_path = download_to_path(&app, &url, stage_path, expected_size).await?;
    emit_progress(&app, "installing", 100, 0, None, "下载完成，正在重启更新");
    start_portable_update_helper(
        &app,
        PathBuf::from(saved_path),
        target_path.clone(),
        Some(std::process::id()),
        false,
    )
    .await?;
    app.exit(0);
    Ok(target_path.to_string_lossy().to_string())
}

async fn github_release() -> Result<GithubRelease, String> {
    let client = reqwest::Client::new();
    client
        .get(RELEASE_API_URL)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .send()
        .await
        .map_err(|error| format!("检查更新失败: {error}"))?
        .error_for_status()
        .map_err(|error| format!("检查更新失败: {error}"))?
        .json::<GithubRelease>()
        .await
        .map_err(|error| format!("解析更新信息失败: {error}"))
}

async fn download_to_path(
    app: &AppHandle,
    url: &str,
    path: PathBuf,
    expected_size: Option<u64>,
) -> Result<String, String> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| format!("无法创建保存目录: {error}"))?;
    }

    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .send()
        .await
        .map_err(|error| format!("下载更新失败: {error}"))?
        .error_for_status()
        .map_err(|error| format!("下载更新失败: {error}"))?;

    let total = response
        .content_length()
        .or(expected_size.filter(|size| *size > 0));
    let mut downloaded = 0u64;
    let mut file = tokio::fs::File::create(&path)
        .await
        .map_err(|error| format!("无法写入更新文件: {error}"))?;
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| format!("下载更新失败: {error}"))?;
        file.write_all(&chunk)
            .await
            .map_err(|error| format!("写入更新文件失败: {error}"))?;
        downloaded += chunk.len() as u64;
        let percent = total
            .map(|total| ((downloaded as f64 / total as f64) * 100.0).round() as u8)
            .unwrap_or(0)
            .min(100);
        emit_progress(
            app,
            "downloading",
            percent,
            downloaded,
            total,
            "正在下载更新",
        );
    }

    file.flush()
        .await
        .map_err(|error| format!("保存更新文件失败: {error}"))?;
    emit_progress(app, "finished", 100, downloaded, total, "下载完成");

    Ok(path.to_string_lossy().to_string())
}

async fn start_portable_update_helper(
    app: &AppHandle,
    source_path: PathBuf,
    target_path: PathBuf,
    process_id: Option<u32>,
    no_shortcut: bool,
) -> Result<(), String> {
    let script_path = portable_updater_script_path(app)?;
    if let Some(parent) = script_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| format!("无法创建更新脚本目录: {error}"))?;
    }
    tokio::fs::write(&script_path, PORTABLE_UPDATER_SCRIPT)
        .await
        .map_err(|error| format!("无法写入更新脚本: {error}"))?;

    spawn_portable_updater(
        &script_path,
        &source_path,
        &target_path,
        process_id,
        no_shortcut,
    )?;
    Ok(())
}

fn start_portable_update_helper_sync(
    app: &AppHandle,
    source_path: &Path,
    target_path: &Path,
    process_id: Option<u32>,
    no_shortcut: bool,
) -> Result<(), String> {
    let script_path = portable_updater_script_path(app)?;
    if let Some(parent) = script_path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("无法创建更新脚本目录: {error}"))?;
    }
    fs::write(&script_path, PORTABLE_UPDATER_SCRIPT)
        .map_err(|error| format!("无法写入更新脚本: {error}"))?;
    spawn_portable_updater(
        &script_path,
        source_path,
        target_path,
        process_id,
        no_shortcut,
    )
}

fn portable_updater_script_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_cache_dir()
        .map_err(|error| format!("无法读取缓存目录: {error}"))?
        .join("portable-updater.ps1"))
}

fn spawn_portable_updater(
    script_path: &Path,
    source_path: &Path,
    target_path: &Path,
    process_id: Option<u32>,
    no_shortcut: bool,
) -> Result<(), String> {
    let mut command = Command::new("powershell.exe");
    command
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-File")
        .arg(script_path)
        .arg("-Source")
        .arg(source_path)
        .arg("-Target")
        .arg(target_path);
    if let Some(process_id) = process_id {
        command.arg("-ProcessId").arg(process_id.to_string());
    }
    if no_shortcut {
        command.arg("-NoShortcut");
    }
    #[cfg(target_os = "windows")]
    command.creation_flags(CREATE_NO_WINDOW);

    command
        .spawn()
        .map_err(|error| format!("无法启动便携版更新: {error}"))?;
    Ok(())
}

fn safe_file_name(file_name: Option<&str>, url: &str, fallback: &str) -> String {
    file_name
        .or_else(|| url.rsplit('/').next())
        .and_then(|name| Path::new(name).file_name())
        .map(|name| name.to_string_lossy().trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

fn portable_target_path() -> Result<PathBuf, String> {
    let local_app_data = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| "无法读取 LOCALAPPDATA 目录".to_string())?;
    Ok(local_app_data
        .join("Programs")
        .join(APP_NAME)
        .join(STABLE_PORTABLE_EXE))
}

pub fn prepare_portable_runtime(app: &AppHandle) -> bool {
    let current_exe = match std::env::current_exe() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("无法读取当前程序路径: {error}");
            return false;
        }
    };

    if should_migrate_versioned_portable(&current_exe) {
        match portable_target_path().and_then(|target_path| {
            start_portable_update_helper_sync(
                app,
                &current_exe,
                &target_path,
                Some(std::process::id()),
                false,
            )
        }) {
            Ok(()) => {
                app.exit(0);
                return true;
            }
            Err(error) => {
                eprintln!("便携版稳定入口迁移失败: {error}");
            }
        }
    }

    if is_portable_runtime(&current_exe) {
        if let Err(error) = ensure_portable_shortcut(app, &current_exe) {
            eprintln!("快捷方式维护失败: {error}");
        }
    }
    false
}

pub fn install_newer_portable_from_second_instance(app: &AppHandle, args: &[String]) -> bool {
    let Some(source_path) = args
        .iter()
        .find_map(|arg| second_instance_portable_path(arg))
    else {
        return false;
    };
    let Some(source_version) = source_path
        .file_name()
        .and_then(|name| version_from_asset(&name.to_string_lossy()))
    else {
        return false;
    };

    if !is_newer_version(&source_version, CURRENT_VERSION) {
        return false;
    }

    match portable_target_path().and_then(|target_path| {
        start_portable_update_helper_sync(
            app,
            &source_path,
            &target_path,
            Some(std::process::id()),
            false,
        )
    }) {
        Ok(()) => {
            app.exit(0);
            true
        }
        Err(error) => {
            eprintln!("新版便携版稳定入口迁移失败: {error}");
            false
        }
    }
}

fn second_instance_portable_path(arg: &str) -> Option<PathBuf> {
    let path = PathBuf::from(arg.trim_matches('"'));
    if path.is_file() && should_migrate_versioned_portable(&path) {
        Some(path)
    } else {
        None
    }
}

fn should_migrate_versioned_portable(path: &Path) -> bool {
    path.file_name()
        .map(|name| name.to_string_lossy().to_ascii_lowercase())
        .is_some_and(|name| name.ends_with("portable.exe"))
        && !is_stable_portable_path(path)
}

fn is_portable_runtime(path: &Path) -> bool {
    is_stable_portable_path(path)
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ShortcutState {
    last_shortcut_version: Option<String>,
    shortcut_missing_after_version: Option<String>,
}

fn ensure_portable_shortcut(app: &AppHandle, target_path: &Path) -> Result<(), String> {
    if target_path
        .parent()
        .map(|parent| parent.join(NO_SHORTCUT_MARKER_FILE).is_file())
        .unwrap_or(false)
    {
        return Ok(());
    }

    let shortcut_path = desktop_shortcut_path()?;
    let mut state = read_shortcut_state(app);
    if shortcut_path.is_file() {
        create_or_update_shortcut(app, target_path)?;
        state.last_shortcut_version = Some(CURRENT_VERSION.to_string());
        state.shortcut_missing_after_version = None;
        return write_shortcut_state(app, &state);
    }

    if state.last_shortcut_version.as_deref() == Some(CURRENT_VERSION) {
        state.shortcut_missing_after_version = Some(CURRENT_VERSION.to_string());
        return write_shortcut_state(app, &state);
    }

    create_or_update_shortcut(app, target_path)?;
    state.last_shortcut_version = Some(CURRENT_VERSION.to_string());
    state.shortcut_missing_after_version = None;
    write_shortcut_state(app, &state)
}

fn desktop_shortcut_path() -> Result<PathBuf, String> {
    let mut command = Command::new("powershell.exe");
    command
        .arg("-NoProfile")
        .arg("-Command")
        .arg("[Environment]::GetFolderPath('Desktop')");
    #[cfg(target_os = "windows")]
    command.creation_flags(CREATE_NO_WINDOW);
    let output = command
        .output()
        .map_err(|error| format!("无法读取桌面目录: {error}"))?;
    if !output.status.success() {
        return Err("无法读取桌面目录".to_string());
    }
    let desktop = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if desktop.is_empty() {
        return Err("桌面目录为空".to_string());
    }
    Ok(PathBuf::from(desktop).join(SHORTCUT_NAME))
}

fn create_or_update_shortcut(app: &AppHandle, target_path: &Path) -> Result<(), String> {
    let script_path = app
        .path()
        .app_cache_dir()
        .map_err(|error| format!("无法读取缓存目录: {error}"))?
        .join("create-shortcut.ps1");
    if let Some(parent) = script_path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("无法创建快捷方式脚本目录: {error}"))?;
    }
    fs::write(&script_path, create_shortcut_script())
        .map_err(|error| format!("无法写入快捷方式脚本: {error}"))?;

    let mut command = Command::new("powershell.exe");
    command
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-File")
        .arg(&script_path)
        .arg("-Target")
        .arg(target_path)
        .arg("-ShortcutName")
        .arg(SHORTCUT_NAME);
    #[cfg(target_os = "windows")]
    command.creation_flags(CREATE_NO_WINDOW);

    let status = command
        .status()
        .map_err(|error| format!("无法创建快捷方式: {error}"))?;
    if !status.success() {
        return Err("创建快捷方式失败".to_string());
    }
    Ok(())
}

fn create_shortcut_script() -> &'static str {
    r#"
param(
  [Parameter(Mandatory = $true)][string]$Target,
  [Parameter(Mandatory = $true)][string]$ShortcutName
)

$ErrorActionPreference = "Stop"
$desktop = [Environment]::GetFolderPath("Desktop")
if (-not $desktop -or -not (Test-Path -LiteralPath $desktop)) {
  throw "Desktop folder does not exist."
}
$targetDir = Split-Path -Parent $Target
$shortcutPath = Join-Path $desktop $ShortcutName
$shell = New-Object -ComObject WScript.Shell
$shortcut = $shell.CreateShortcut($shortcutPath)
$shortcut.TargetPath = $Target
$shortcut.WorkingDirectory = $targetDir
$shortcut.IconLocation = $Target
$shortcut.Save()
"#
}

fn read_shortcut_state(app: &AppHandle) -> ShortcutState {
    shortcut_state_path(app)
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<ShortcutState>(&text).ok())
        .unwrap_or_default()
}

fn write_shortcut_state(app: &AppHandle, state: &ShortcutState) -> Result<(), String> {
    let path = shortcut_state_path(app).ok_or_else(|| "无法读取应用数据目录".to_string())?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("无法创建应用数据目录: {error}"))?;
    }
    let text = serde_json::to_string_pretty(state)
        .map_err(|error| format!("无法序列化快捷方式状态: {error}"))?;
    fs::write(path, text).map_err(|error| format!("无法写入快捷方式状态: {error}"))
}

fn shortcut_state_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|path| path.join(SHORTCUT_STATE_FILE))
}

fn emit_progress(
    app: &AppHandle,
    phase: &str,
    percent: u8,
    downloaded: u64,
    total: Option<u64>,
    message: &str,
) {
    let _ = app.emit(
        "update-progress",
        UpdateProgress {
            phase: phase.into(),
            percent,
            downloaded,
            total,
            message: message.into(),
        },
    );
}

fn find_asset<'a>(assets: &'a [GithubAsset], suffix: &str) -> Option<&'a GithubAsset> {
    assets
        .iter()
        .find(|asset| asset.name.to_ascii_lowercase().ends_with(suffix))
}

fn normalize_version(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_start_matches('v').trim_start_matches('V');
    if trimmed.split('.').count() >= 3 {
        Some(trimmed.to_string())
    } else {
        None
    }
}

fn version_from_asset(name: &str) -> Option<String> {
    name.split_whitespace()
        .find(|part| part.chars().filter(|ch| *ch == '.').count() >= 2)
        .and_then(normalize_version)
}

fn is_newer_version(latest: &str, current: &str) -> bool {
    let latest_parts = version_parts(latest);
    let current_parts = version_parts(current);
    latest_parts > current_parts
}

fn version_parts(version: &str) -> [u32; 3] {
    let mut parts = [0, 0, 0];
    for (index, value) in version
        .trim()
        .trim_start_matches('v')
        .split('.')
        .take(3)
        .enumerate()
    {
        parts[index] = value
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .collect::<String>()
            .parse::<u32>()
            .unwrap_or(0);
    }
    parts
}

fn is_stable_portable_path(path: &Path) -> bool {
    portable_target_path()
        .map(|target_path| {
            path.to_string_lossy()
                .eq_ignore_ascii_case(&target_path.to_string_lossy())
        })
        .unwrap_or(false)
}
