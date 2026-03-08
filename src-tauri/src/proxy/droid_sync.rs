use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

const DROID_DIR: &str = ".factory";
const DROID_CONFIG_FILE: &str = "settings.json";
const BACKUP_SUFFIX: &str = ".antigravity.bak";
const AG_ID_PREFIX: &str = "custom:AG-";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DroidStatus {
    pub installed: bool,
    pub version: Option<String>,
    pub is_synced: bool,
    pub has_backup: bool,
    pub current_base_url: Option<String>,
    pub files: Vec<String>,
    pub synced_count: usize,
}

fn get_droid_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(DROID_DIR))
}

fn get_config_path() -> Option<PathBuf> {
    get_droid_dir().map(|dir| dir.join(DROID_CONFIG_FILE))
}

fn find_in_path(executable: &str) -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let extensions = ["exe", "cmd", "bat"];
        if let Ok(path_var) = env::var("PATH") {
            for dir in path_var.split(';') {
                for ext in &extensions {
                    let full_path = PathBuf::from(dir).join(format!("{}.{}", executable, ext));
                    if full_path.exists() {
                        return Some(full_path);
                    }
                }
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        if let Ok(path_var) = env::var("PATH") {
            for dir in path_var.split(':') {
                let full_path = PathBuf::from(dir).join(executable);
                if full_path.exists() {
                    return Some(full_path);
                }
            }
        }
    }

    None
}

fn resolve_droid_path() -> Option<PathBuf> {
    if let Some(path) = find_in_path("droid") {
        tracing::debug!("Found droid in PATH: {:?}", path);
        return Some(path);
    }

    #[cfg(not(target_os = "windows"))]
    {
        let home = dirs::home_dir()?;
        let candidates = [
            home.join(".local/bin/droid"),
            home.join(".factory/bin/droid"),
            home.join("bin/droid"),
            PathBuf::from("/opt/homebrew/bin/droid"),
            PathBuf::from("/usr/local/bin/droid"),
            PathBuf::from("/usr/bin/droid"),
        ];
        for path in &candidates {
            if path.exists() {
                tracing::debug!("Found droid at: {:?}", path);
                return Some(path.clone());
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(app_data) = env::var("APPDATA") {
            let npm_path = PathBuf::from(&app_data).join("npm").join("droid.cmd");
            if npm_path.exists() {
                return Some(npm_path);
            }
        }
        if let Ok(local_app_data) = env::var("LOCALAPPDATA") {
            let pnpm_path = PathBuf::from(&local_app_data)
                .join("pnpm")
                .join("droid.cmd");
            if pnpm_path.exists() {
                return Some(pnpm_path);
            }
        }
    }

    None
}

fn extract_version(raw: &str) -> String {
    let trimmed = raw.trim();
    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    for part in parts {
        if let Some(slash_idx) = part.find('/') {
            let after = &part[slash_idx + 1..];
            if after.contains('.') && after.chars().next().map_or(false, |c| c.is_ascii_digit()) {
                return after.to_string();
            }
        }
        if part.contains('.')
            && part.chars().next().map_or(false, |c| c.is_ascii_digit())
            && part.chars().all(|c| c.is_ascii_digit() || c == '.')
        {
            return part.to_string();
        }
    }
    let version_chars: String = trimmed
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    if !version_chars.is_empty() && version_chars.contains('.') {
        return version_chars;
    }
    "unknown".to_string()
}

pub fn check_droid_installed() -> (bool, Option<String>) {
    tracing::debug!("Checking droid installation...");

    let droid_path = match resolve_droid_path() {
        Some(path) => {
            tracing::debug!("Resolved droid path: {:?}", path);
            path
        }
        None => {
            tracing::debug!("Could not resolve droid path");
            return (false, None);
        }
    };

    let mut cmd = Command::new(&droid_path);
    cmd.arg("--version");
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);

    match cmd.output() {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let raw = if stdout.trim().is_empty() {
                stderr.to_string()
            } else {
                stdout.to_string()
            };
            tracing::debug!("droid --version output: {}", raw.trim());
            let version = extract_version(&raw);
            (true, Some(version))
        }
        _ => (true, Some("unknown".to_string())),
    }
}

/// 统计已有 customModels 中有多少由 Antigravity 添加的（id 以 custom:AG- 开头）
fn count_synced_models(json: &Value) -> (usize, Option<String>) {
    let mut count = 0;
    let mut first_url = None;

    if let Some(arr) = json.get("customModels").and_then(|v| v.as_array()) {
        for m in arr {
            let id = m.get("id").and_then(|v| v.as_str()).unwrap_or_default();
            if !id.starts_with(AG_ID_PREFIX) {
                continue;
            }
            count += 1;
            if first_url.is_none() {
                first_url = m
                    .get("baseUrl")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
            }
        }
    }
    (count, first_url)
}

pub fn get_sync_status(_proxy_url: &str) -> (bool, bool, Option<String>, usize) {
    let config_path = match get_config_path() {
        Some(p) => p,
        None => return (false, false, None, 0),
    };

    let backup_path = config_path.with_file_name(format!("{}{}", DROID_CONFIG_FILE, BACKUP_SUFFIX));
    let has_backup = backup_path.exists();

    if !config_path.exists() {
        return (false, has_backup, None, 0);
    }

    let content = match fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(_) => return (false, has_backup, None, 0),
    };

    let json: Value = serde_json::from_str(&content).unwrap_or_default();
    let (synced_count, first_url) = count_synced_models(&json);
    (synced_count > 0, has_backup, first_url, synced_count)
}

fn create_backup(path: &PathBuf) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let backup_path = path.with_file_name(format!(
        "{}{}",
        path.file_name().unwrap_or_default().to_string_lossy(),
        BACKUP_SUFFIX
    ));
    if backup_path.exists() {
        return Ok(());
    }
    fs::copy(path, &backup_path).map_err(|e| format!("Failed to create backup: {}", e))?;
    tracing::info!("Created backup: {:?}", backup_path);
    Ok(())
}

/// 接收前端 preview 里完整的 customModels 数组，直接替换写入
pub fn sync_droid_config(full_custom_models: Vec<Value>) -> Result<usize, String> {
    let config_path =
        get_config_path().ok_or_else(|| "Failed to get Droid config directory".to_string())?;

    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create directory: {}", e))?;
    }

    create_backup(&config_path)?;

    let mut config: Value = if config_path.exists() {
        let content = fs::read_to_string(&config_path)
            .map_err(|e| format!("Failed to read config: {}", e))?;
        serde_json::from_str(&content).map_err(|e| format!("Failed to parse config: {}", e))?
    } else {
        serde_json::json!({})
    };

    if !config.is_object() {
        config = serde_json::json!({});
    }

    let ag_count = full_custom_models
        .iter()
        .filter(|m| {
            m.get("id")
                .and_then(|v| v.as_str())
                .map(|s| s.starts_with(AG_ID_PREFIX))
                .unwrap_or(false)
        })
        .count();

    config
        .as_object_mut()
        .unwrap()
        .insert("customModels".to_string(), Value::Array(full_custom_models));

    let tmp_path = config_path.with_extension("tmp");
    fs::write(&tmp_path, serde_json::to_string_pretty(&config).unwrap())
        .map_err(|e| format!("Failed to write temp file: {}", e))?;
    fs::rename(&tmp_path, &config_path)
        .map_err(|e| format!("Failed to rename config file: {}", e))?;

    Ok(ag_count)
}

pub fn restore_droid_config() -> Result<(), String> {
    let config_path =
        get_config_path().ok_or_else(|| "Failed to get Droid config directory".to_string())?;

    let backup_path = config_path.with_file_name(format!("{}{}", DROID_CONFIG_FILE, BACKUP_SUFFIX));
    if backup_path.exists() {
        fs::rename(&backup_path, &config_path)
            .map_err(|e| format!("Failed to restore config: {}", e))?;
        Ok(())
    } else {
        Err("No backup file found".to_string())
    }
}

pub fn read_droid_config_content() -> Result<String, String> {
    let config_path =
        get_config_path().ok_or_else(|| "Failed to get Droid config directory".to_string())?;

    if !config_path.exists() {
        return Ok("{}".to_string());
    }

    fs::read_to_string(&config_path).map_err(|e| format!("Failed to read config: {}", e))
}

// Tauri Commands

#[tauri::command]
pub async fn get_droid_sync_status(proxy_url: String) -> Result<DroidStatus, String> {
    let (installed, version) = check_droid_installed();
    let (is_synced, has_backup, current_base_url, synced_count) = if installed {
        get_sync_status(&proxy_url)
    } else {
        (false, false, None, 0)
    };

    Ok(DroidStatus {
        installed,
        version,
        is_synced,
        has_backup,
        current_base_url,
        files: vec![DROID_CONFIG_FILE.to_string()],
        synced_count,
    })
}

#[tauri::command]
pub async fn execute_droid_sync(custom_models: Vec<Value>) -> Result<usize, String> {
    sync_droid_config(custom_models)
}

#[tauri::command]
pub async fn execute_droid_restore() -> Result<(), String> {
    restore_droid_config()
}

#[tauri::command]
pub async fn get_droid_config_content() -> Result<String, String> {
    read_droid_config_content()
}
