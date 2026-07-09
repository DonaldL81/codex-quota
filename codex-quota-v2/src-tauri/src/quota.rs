use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fmt;
use std::process::Stdio;
use std::time::Duration;
use std::{
    env, fs,
    path::{Path, PathBuf},
};
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdout, Command};
use tokio::time::timeout;

const CLIENT_NAME: &str = "codex-quota-monitor-v2";
const CLIENT_VERSION: &str = "2.2.7";
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(15);
const CACHE_FILE: &str = "last-quota.json";
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Debug)]
pub struct QuotaError(String);

impl fmt::Display for QuotaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for QuotaError {}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaSnapshot {
    pub status: String,
    pub limit_name: String,
    pub plan_type: String,
    pub updated_at: String,
    pub primary_remaining: i64,
    pub primary_reset: String,
    pub secondary_remaining: i64,
    pub secondary_reset: String,
}

#[derive(Clone, Debug)]
struct WindowQuota {
    remaining: i64,
    reset: String,
}

pub async fn read_quota() -> Result<QuotaSnapshot, QuotaError> {
    let codex_path = find_codex_path().ok_or_else(|| {
        QuotaError(
            "Cannot find Codex CLI. Install Codex Desktop or set CODEX_QUOTA_CODEX_PATH.".into(),
        )
    })?;

    let mut command = Command::new(codex_path);
    command
        .args(["app-server", "--listen", "stdio://"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);

    let mut child = command
        .spawn()
        .map_err(|error| QuotaError(format!("Cannot start Codex app-server: {error}")))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| QuotaError("Cannot read Codex app-server stdout.".into()))?;
    let mut lines = BufReader::new(stdout).lines();

    let result = async {
        send_json_rpc(
            &mut child,
            1,
            "initialize",
            Some(json!({
                "clientInfo": {
                    "name": CLIENT_NAME,
                    "version": CLIENT_VERSION
                },
                "capabilities": {}
            })),
        )
        .await?;
        let _ = read_response(&mut lines, 1).await?;

        send_json_rpc(&mut child, 2, "account/rateLimits/read", None).await?;
        let payload = read_response(&mut lines, 2).await?;
        if let Some(error) = payload.get("error") {
            return Err(QuotaError(
                error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("Codex returned an error.")
                    .to_string(),
            ));
        }

        normalize_rate_limits(payload.get("result").unwrap_or(&Value::Null))
    }
    .await;

    let _ = child.kill().await;
    result
}

pub fn read_cached_quota(app: &AppHandle) -> Option<QuotaSnapshot> {
    let path = cache_file(app)?;
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn write_cached_quota(app: &AppHandle, snapshot: &QuotaSnapshot) {
    let Some(path) = cache_file(app) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string(snapshot) {
        let _ = fs::write(path, text);
    }
}

fn cache_file(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|path| path.join(CACHE_FILE))
}

fn find_codex_path() -> Option<PathBuf> {
    if let Ok(path) = env::var("CODEX_QUOTA_CODEX_PATH") {
        let path = PathBuf::from(path);
        if path.exists() {
            return Some(path);
        }
    }

    if let Ok(local_app_data) = env::var("LOCALAPPDATA") {
        let codex_dir = PathBuf::from(local_app_data).join("OpenAI").join("Codex");
        let bin_dir = codex_dir.join("bin");
        let direct_path = bin_dir.join("codex.exe");
        if direct_path.exists() {
            return Some(direct_path);
        }
        if let Some(nested_path) = find_nested_codex_exe(&bin_dir) {
            return Some(nested_path);
        }
    }

    Some(PathBuf::from("codex.exe"))
}

fn find_nested_codex_exe(bin_dir: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(bin_dir).ok()?;
    let mut candidates = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("codex.exe"))
        .filter(|path| path.exists())
        .collect::<Vec<_>>();

    candidates.sort_by(|left, right| {
        let left_modified = left
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok();
        let right_modified = right
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok();
        right_modified
            .cmp(&left_modified)
            .then_with(|| right.cmp(left))
    });
    candidates.into_iter().next()
}

async fn send_json_rpc(
    child: &mut Child,
    id: i64,
    method: &str,
    params: Option<Value>,
) -> Result<(), QuotaError> {
    let stdin = child
        .stdin
        .as_mut()
        .ok_or_else(|| QuotaError("Cannot write to Codex app-server stdin.".into()))?;
    let mut payload = json!({
        "id": id,
        "method": method,
    });
    if let Some(params) = params {
        payload["params"] = params;
    }

    let line = serde_json::to_string(&payload)
        .map_err(|error| QuotaError(format!("Cannot serialize JSON-RPC request: {error}")))?;
    stdin
        .write_all(line.as_bytes())
        .await
        .map_err(|error| QuotaError(format!("Cannot send JSON-RPC request: {error}")))?;
    stdin
        .write_all(b"\n")
        .await
        .map_err(|error| QuotaError(format!("Cannot send JSON-RPC newline: {error}")))?;
    stdin
        .flush()
        .await
        .map_err(|error| QuotaError(format!("Cannot flush JSON-RPC request: {error}")))?;
    Ok(())
}

async fn read_response(
    lines: &mut Lines<BufReader<ChildStdout>>,
    id: i64,
) -> Result<Value, QuotaError> {
    let task =
        async {
            while let Some(line) = lines.next_line().await.map_err(|error| {
                QuotaError(format!("Cannot read Codex app-server output: {error}"))
            })? {
                let Ok(message) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if message.get("id").and_then(Value::as_i64) == Some(id) {
                    return Ok(message);
                }
            }
            Err(QuotaError(
                "Codex app-server closed before responding.".into(),
            ))
        };

    timeout(RESPONSE_TIMEOUT, task)
        .await
        .map_err(|_| QuotaError("Timed out waiting for Codex app-server response.".into()))?
}

fn normalize_rate_limits(payload: &Value) -> Result<QuotaSnapshot, QuotaError> {
    let snapshot = payload
        .pointer("/rateLimitsByLimitId/codex")
        .or_else(|| payload.get("rateLimits"))
        .ok_or_else(|| QuotaError("Codex returned no rate limit data.".into()))?;

    let primary = convert_window("primary", snapshot.get("primary"))?;
    let secondary = convert_window("secondary", snapshot.get("secondary"))?;
    let limit_name = snapshot
        .get("limitName")
        .and_then(Value::as_str)
        .unwrap_or("Codex")
        .to_string();
    let plan_type = snapshot
        .get("planType")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    Ok(QuotaSnapshot {
        status: "ready".into(),
        limit_name,
        plan_type,
        updated_at: Local::now().format("%H:%M:%S").to_string(),
        primary_remaining: primary.remaining,
        primary_reset: primary.reset,
        secondary_remaining: secondary.remaining,
        secondary_reset: secondary.reset,
    })
}

fn convert_window(label: &str, source: Option<&Value>) -> Result<WindowQuota, QuotaError> {
    let source = source
        .ok_or_else(|| QuotaError(format!("Codex returned no {label} rate limit window.")))?;
    let used = source
        .get("usedPercent")
        .and_then(Value::as_f64)
        .ok_or_else(|| {
            QuotaError(format!(
                "Codex returned incomplete {label} rate limit data."
            ))
        })?;
    if !(0.0..=100.0).contains(&used) {
        return Err(QuotaError(format!(
            "Codex returned invalid {label} usage percent: {used}."
        )));
    }
    let remaining = (100.0 - used).round().clamp(0.0, 100.0) as i64;
    let reset = source
        .get("resetsAt")
        .and_then(Value::as_i64)
        .map(format_reset_time)
        .unwrap_or_else(|| "unknown".into());

    Ok(WindowQuota { remaining, reset })
}

fn format_reset_time(epoch_seconds: i64) -> String {
    let Some(date) = DateTime::from_timestamp(epoch_seconds, 0) else {
        return "unknown".into();
    };
    let local = date.with_timezone(&Local);
    let now = Local::now();
    if local.date_naive() == now.date_naive() {
        local.format("%H:%M").to_string()
    } else {
        local.format("%-m/%-d %H:%M").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalize_rate_limits_requires_used_percent() {
        let payload = json!({
            "rateLimitsByLimitId": {
                "codex": {
                    "limitName": "Codex",
                    "planType": "pro",
                    "primary": { "resetsAt": 1893456000 },
                    "secondary": { "usedPercent": 40.0, "resetsAt": 1893456000 }
                }
            }
        });

        let error = normalize_rate_limits(&payload).unwrap_err();
        assert!(error.0.contains("incomplete primary rate limit data"));
    }

    #[test]
    fn normalize_rate_limits_accepts_explicit_zero_usage() {
        let payload = json!({
            "rateLimitsByLimitId": {
                "codex": {
                    "limitName": "Codex",
                    "planType": "pro",
                    "primary": { "usedPercent": 0.0, "resetsAt": 1893456000 },
                    "secondary": { "usedPercent": 25.0, "resetsAt": 1893456000 }
                }
            }
        });

        let snapshot = normalize_rate_limits(&payload).unwrap();
        assert_eq!(snapshot.primary_remaining, 100);
        assert_eq!(snapshot.secondary_remaining, 75);
    }
}
