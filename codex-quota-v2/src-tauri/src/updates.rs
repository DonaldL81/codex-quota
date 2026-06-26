use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::AsyncWriteExt;

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const RELEASE_API_URL: &str = "https://api.github.com/repos/DonaldL81/codex-quota/releases/latest";
const USER_AGENT: &str = concat!("Codex-Quota-Monitor/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub available: bool,
    pub current_version: String,
    pub latest_version: String,
    pub release_url: String,
    pub portable_asset_url: Option<String>,
    pub setup_asset_url: Option<String>,
    pub portable_file_name: Option<String>,
    pub setup_file_name: Option<String>,
    pub package_kind: PackageKind,
    pub message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PackageKind {
    Portable,
    Installer,
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
}

#[tauri::command]
pub async fn check_update() -> Result<UpdateInfo, String> {
    let release = github_release().await?;
    let latest_version = normalize_version(&release.tag_name)
        .or_else(|| release.assets.iter().find_map(|asset| version_from_asset(&asset.name)))
        .unwrap_or_else(|| CURRENT_VERSION.to_string());
    let portable_asset = find_asset(&release.assets, "portable.exe");
    let setup_asset = find_asset(&release.assets, "setup.exe");
    let available = is_newer_version(&latest_version, CURRENT_VERSION);
    let package_kind = current_package_kind();

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
        setup_asset_url: setup_asset.map(|asset| asset.browser_download_url.clone()),
        portable_file_name: portable_asset.map(|asset| asset.name.clone()),
        setup_file_name: setup_asset.map(|asset| asset.name.clone()),
        package_kind,
        message,
    })
}

#[tauri::command]
pub async fn download_portable_update(
    app: AppHandle,
    url: String,
    save_path: String,
) -> Result<String, String> {
    emit_progress(&app, "downloading", 0, 0, None, "正在下载便携版更新");
    download_to_path(&app, &url, PathBuf::from(save_path)).await
}

#[tauri::command]
pub async fn download_installer_update(app: AppHandle, url: String) -> Result<String, String> {
    let file_name = url
        .rsplit('/')
        .next()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("Codex-Quota-Update-Setup.exe");
    let target_path = app
        .path()
        .app_cache_dir()
        .map_err(|error| format!("无法读取缓存目录: {error}"))?
        .join(file_name);

    emit_progress(&app, "downloading", 0, 0, None, "正在下载安装更新");
    let saved_path = download_to_path(&app, &url, target_path).await?;
    emit_progress(&app, "installing", 100, 0, None, "下载完成，正在启动安装程序");

    Command::new(&saved_path)
        .arg("/S")
        .spawn()
        .map_err(|error| format!("无法启动安装程序: {error}"))?;
    app.exit(0);
    Ok(saved_path)
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

async fn download_to_path(app: &AppHandle, url: &str, path: PathBuf) -> Result<String, String> {
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

    let total = response.content_length();
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
        emit_progress(app, "downloading", percent, downloaded, total, "正在下载更新");
    }

    file.flush()
        .await
        .map_err(|error| format!("保存更新文件失败: {error}"))?;
    emit_progress(app, "finished", 100, downloaded, total, "下载完成");

    Ok(path.to_string_lossy().to_string())
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

fn current_package_kind() -> PackageKind {
    let exe_name = std::env::current_exe()
        .ok()
        .and_then(|path| path.file_name().map(|name| name.to_string_lossy().to_string()))
        .unwrap_or_default()
        .to_ascii_lowercase();
    if exe_name.contains("portable") {
        PackageKind::Portable
    } else {
        PackageKind::Installer
    }
}
