use crate::utils::command::{output_with_timeout, CommandExtWrapper};
use crate::utils::fs::{replace_file_atomic, write_atomic_restricted};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const VERSION_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

const DROID_DIR: &str = ".factory";
const DROID_CONFIG_FILE: &str = "settings.json";
const BACKUP_SUFFIX: &str = ".antigravity.bak";
const AG_ID_PREFIX: &str = "custom:AG-";

static DROID_CONFIG_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn acquire_droid_config_lock() -> std::sync::MutexGuard<'static, ()> {
    DROID_CONFIG_MUTEX.lock().unwrap_or_else(|poisoned| {
        tracing::warn!("DROID_CONFIG_MUTEX was poisoned, recovering lock");
        poisoned.into_inner()
    })
}

fn is_safe_droid_path(path: &Path) -> bool {
    if !path.exists() || !path.is_file() || !path.is_absolute() {
        return false;
    }
    let path_str = path.to_string_lossy();
    if path_str.chars().any(|c| matches!(c, '\0' | '\r' | '\n')) {
        return false;
    }
    #[cfg(target_os = "windows")]
    {
        const DANGEROUS_CHARS: &[char] = &[
            '&', '|', ';', '<', '>', '(', ')', '`', '$', '^', '%', '!', '"',
        ];
        if path_str.chars().any(|c| DANGEROUS_CHARS.contains(&c)) {
            tracing::warn!(
                "[Droid-Sync] Executable path contains dangerous characters: {}",
                path_str
            );
            return false;
        }
    }
    true
}

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

    if !is_safe_droid_path(&droid_path) {
        return (true, Some("unknown".to_string()));
    }

    let mut cmd = Command::new(&droid_path);
    cmd.arg("--version");
    cmd.creation_flags_windows();

    match output_with_timeout(&mut cmd, VERSION_PROBE_TIMEOUT) {
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

fn backup_path_for(config_path: &Path) -> PathBuf {
    config_path.with_file_name(format!(
        "{}{}",
        config_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy(),
        BACKUP_SUFFIX
    ))
}

fn get_sync_status_at(config_path: &Path) -> (bool, bool, Option<String>, usize) {
    let backup_path = backup_path_for(config_path);
    let has_backup = backup_path.exists();

    if !config_path.exists() {
        return (false, has_backup, None, 0);
    }

    let content = match fs::read_to_string(config_path) {
        Ok(c) => c,
        Err(_) => return (false, has_backup, None, 0),
    };

    let json: Value = serde_json::from_str(&content).unwrap_or_default();
    let (synced_count, first_url) = count_synced_models(&json);
    (synced_count > 0, has_backup, first_url, synced_count)
}

pub fn get_sync_status(_proxy_url: &str) -> (bool, bool, Option<String>, usize) {
    let _lock = acquire_droid_config_lock();
    let config_path = match get_config_path() {
        Some(p) => p,
        None => return (false, false, None, 0),
    };
    get_sync_status_at(&config_path)
}

fn create_backup(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let backup_path = backup_path_for(path);
    if backup_path.exists() {
        return Ok(());
    }
    let original = fs::read(path).map_err(|e| format!("Failed to read file for backup: {}", e))?;
    write_atomic_restricted(&backup_path, &original, 0o600)
        .map_err(|e| format!("Failed to create backup: {}", e))?;
    tracing::info!("Created backup: {:?}", backup_path);
    Ok(())
}

fn sync_droid_config_at(
    config_path: &Path,
    full_custom_models: Vec<Value>,
) -> Result<usize, String> {
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create directory: {}", e))?;
    }

    create_backup(config_path)?;

    let mut config: Value = if config_path.exists() {
        let content =
            fs::read_to_string(config_path).map_err(|e| format!("Failed to read config: {}", e))?;
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

    let serialized = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    write_atomic_restricted(config_path, serialized.as_bytes(), 0o600)?;

    Ok(ag_count)
}

/// 接收前端 preview 里完整的 customModels 数组，直接替换写入
pub fn sync_droid_config(full_custom_models: Vec<Value>) -> Result<usize, String> {
    let _lock = acquire_droid_config_lock();
    let config_path =
        get_config_path().ok_or_else(|| "Failed to get Droid config directory".to_string())?;
    sync_droid_config_at(&config_path, full_custom_models)
}

fn restore_droid_config_at(config_path: &Path) -> Result<(), String> {
    let backup_path = backup_path_for(config_path);
    if backup_path.exists() {
        replace_file_atomic(&backup_path, config_path)
            .map_err(|e| format!("Failed to restore config: {}", e))?;
        Ok(())
    } else {
        Err("No backup file found".to_string())
    }
}

pub fn restore_droid_config() -> Result<(), String> {
    let _lock = acquire_droid_config_lock();
    let config_path =
        get_config_path().ok_or_else(|| "Failed to get Droid config directory".to_string())?;
    restore_droid_config_at(&config_path)
}

fn read_droid_config_content_at(config_path: &Path) -> Result<String, String> {
    if !config_path.exists() {
        return Ok("{}".to_string());
    }

    fs::read_to_string(config_path).map_err(|e| format!("Failed to read config: {}", e))
}

pub fn read_droid_config_content() -> Result<String, String> {
    let _lock = acquire_droid_config_lock();
    let config_path =
        get_config_path().ok_or_else(|| "Failed to get Droid config directory".to_string())?;
    read_droid_config_content_at(&config_path)
}

// Tauri Commands

#[tauri::command]
pub async fn get_droid_sync_status(proxy_url: String) -> Result<DroidStatus, String> {
    tokio::task::spawn_blocking(move || {
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
    })
    .await
    .unwrap_or_else(|_| Err("Task panicked".to_string()))
}

#[tauri::command]
pub async fn execute_droid_sync(custom_models: Vec<Value>) -> Result<usize, String> {
    tokio::task::spawn_blocking(move || sync_droid_config(custom_models))
        .await
        .unwrap_or_else(|_| Err("Task panicked".to_string()))
}

#[tauri::command]
pub async fn execute_droid_restore() -> Result<(), String> {
    tokio::task::spawn_blocking(move || restore_droid_config())
        .await
        .unwrap_or_else(|_| Err("Task panicked".to_string()))
}

#[tauri::command]
pub async fn get_droid_config_content() -> Result<String, String> {
    tokio::task::spawn_blocking(move || read_droid_config_content())
        .await
        .unwrap_or_else(|_| Err("Task panicked".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_is_safe_droid_path_rejects_relative_and_metacharacters() {
        let temp_dir =
            std::env::temp_dir().join(format!("test_droid_path_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();

        let safe_file = temp_dir.join("droid.exe");
        fs::write(&safe_file, b"").unwrap();
        assert!(is_safe_droid_path(&safe_file));
        assert!(!is_safe_droid_path(Path::new("droid.exe")));
        assert!(!is_safe_droid_path(&temp_dir.join("missing.exe")));

        #[cfg(target_os = "windows")]
        {
            let unsafe_file = temp_dir.join("droid&calc.cmd");
            fs::write(&unsafe_file, b"").unwrap();
            assert!(!is_safe_droid_path(&unsafe_file));
        }

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_sync_and_restore_droid_config_preserves_backup_and_restricts_permissions() {
        let temp_dir =
            std::env::temp_dir().join(format!("test_droid_sync_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();
        let config_path = temp_dir.join("settings.json");

        let original_content = "{\n  \"theme\": \"dark\",\n  \"customModels\": []\n}\n";
        fs::write(&config_path, original_content).unwrap();

        let models_v1 = vec![json!({
            "id": "custom:AG-gemini-3.8-flash",
            "model": "gemini-3.8-flash",
            "baseUrl": "http://127.0.0.1:8045",
            "apiKey": "sk-secret-v1"
        })];
        let count1 = sync_droid_config_at(&config_path, models_v1).expect("first sync succeeds");
        assert_eq!(count1, 1);

        let backup_path = backup_path_for(&config_path);
        assert!(backup_path.exists());
        assert_eq!(fs::read_to_string(&backup_path).unwrap(), original_content);

        // Second sync must not overwrite the original backup
        let models_v2 = vec![json!({
            "id": "custom:AG-claude-sonnet-4-6",
            "model": "claude-sonnet-4-6",
            "baseUrl": "http://127.0.0.1:9000",
            "apiKey": "sk-secret-v2"
        })];
        let count2 = sync_droid_config_at(&config_path, models_v2).expect("second sync succeeds");
        assert_eq!(count2, 1);
        assert_eq!(fs::read_to_string(&backup_path).unwrap(), original_content);

        let (synced, has_backup, base_url, synced_count) = get_sync_status_at(&config_path);
        assert!(synced);
        assert!(has_backup);
        assert_eq!(base_url.as_deref(), Some("http://127.0.0.1:9000"));
        assert_eq!(synced_count, 1);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&config_path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(&backup_path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }

        restore_droid_config_at(&config_path).expect("restore succeeds");
        assert!(!backup_path.exists());
        assert_eq!(
            read_droid_config_content_at(&config_path).unwrap(),
            original_content
        );

        let entries: Vec<_> = fs::read_dir(&temp_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(!entries.iter().any(|name| name.contains(".tmp")));

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
