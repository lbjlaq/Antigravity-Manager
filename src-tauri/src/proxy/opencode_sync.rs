use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

const OPENCODE_DIR: &str = ".config/opencode";
const OPENCODE_CONFIG_FILE: &str = "opencode.json";
const ANTIGRAVITY_CONFIG_FILE: &str = "antigravity.json";
const ANTIGRAVITY_ACCOUNTS_FILE: &str = "antigravity-accounts.json";
const BACKUP_SUFFIX: &str = ".antigravity.bak";

const ANTHROPIC_MODELS: &[&str] = &[
    "claude-sonnet-4-5",
    "claude-sonnet-4-5-thinking",
    "claude-opus-4-5-thinking",
];

const GOOGLE_MODELS: &[&str] = &[
    "gemini-3-pro-high",
    "gemini-3-pro-low",
    "gemini-3-flash",
    "gemini-3-pro-image",
    "gemini-2.5-flash",
    "gemini-2.5-flash-lite",
    "gemini-2.5-flash-thinking",
    "gemini-2.5-pro",
];

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OpencodeStatus {
    pub installed: bool,
    pub version: Option<String>,
    pub is_synced: bool,
    pub has_backup: bool,
    pub current_base_url: Option<String>,
    pub files: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct OpencodeAccount {
    email: String,
    #[serde(rename = "refreshToken")]
    refresh_token: String,
    #[serde(rename = "projectId", skip_serializing_if = "Option::is_none")]
    project_id: Option<String>,
    #[serde(rename = "rateLimitResetTimes", skip_serializing_if = "Option::is_none")]
    rate_limit_reset_times: Option<HashMap<String, i64>>,
}

#[derive(Debug, Clone)]
struct OpencodePaths {
    dir: PathBuf,
    opencode_config: PathBuf,
    antigravity_config: PathBuf,
    antigravity_accounts: PathBuf,
}

fn known_files() -> [&'static str; 3] {
    [
        OPENCODE_CONFIG_FILE,
        ANTIGRAVITY_CONFIG_FILE,
        ANTIGRAVITY_ACCOUNTS_FILE,
    ]
}

fn get_opencode_dir() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or_else(|| "Failed to get home directory".to_string())?;
    Ok(home.join(OPENCODE_DIR))
}

fn get_config_paths() -> Result<OpencodePaths, String> {
    let dir = get_opencode_dir()?;
    Ok(OpencodePaths {
        opencode_config: dir.join(OPENCODE_CONFIG_FILE),
        antigravity_config: dir.join(ANTIGRAVITY_CONFIG_FILE),
        antigravity_accounts: dir.join(ANTIGRAVITY_ACCOUNTS_FILE),
        dir,
    })
}

fn backup_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    path.with_file_name(format!("{}{}", file_name, BACKUP_SUFFIX))
}

fn ensure_parent_dir(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory {}: {}", parent.display(), e))?;
    }
    Ok(())
}

fn create_backup(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }

    let backup = backup_path(path);
    if backup.exists() {
        return Ok(());
    }

    fs::copy(path, &backup).map_err(|e| {
        format!(
            "Failed to create backup {} from {}: {}",
            backup.display(),
            path.display(),
            e
        )
    })?;

    Ok(())
}

fn write_atomic(path: &Path, content: &str) -> Result<(), String> {
    ensure_parent_dir(path)?;
    let tmp_path = path.with_extension("tmp");
    fs::write(&tmp_path, content)
        .map_err(|e| format!("Failed to write temp file {}: {}", tmp_path.display(), e))?;
    fs::rename(&tmp_path, path).map_err(|e| {
        format!(
            "Failed to replace file {} with {}: {}",
            path.display(),
            tmp_path.display(),
            e
        )
    })
}

fn write_json_atomic(path: &Path, value: &Value) -> Result<(), String> {
    let content = serde_json::to_string_pretty(value)
        .map_err(|e| format!("Failed to serialize json for {}: {}", path.display(), e))?;
    write_atomic(path, &content)
}

fn read_json_or_default_object(path: &Path) -> Value {
    if !path.exists() {
        return json!({});
    }

    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(_) => return json!({}),
    };

    let parsed: Value = match serde_json::from_str(&content) {
        Ok(value) => value,
        Err(_) => return json!({}),
    };

    if parsed.is_object() {
        parsed
    } else {
        json!({})
    }
}

fn available_files(paths: &OpencodePaths) -> Vec<String> {
    let mut files = Vec::new();

    if paths.opencode_config.exists() {
        files.push(OPENCODE_CONFIG_FILE.to_string());
    }
    if paths.antigravity_config.exists() {
        files.push(ANTIGRAVITY_CONFIG_FILE.to_string());
    }
    if paths.antigravity_accounts.exists() {
        files.push(ANTIGRAVITY_ACCOUNTS_FILE.to_string());
    }

    files
}

fn has_any_backup(paths: &OpencodePaths) -> bool {
    backup_path(&paths.opencode_config).exists()
        || backup_path(&paths.antigravity_config).exists()
        || backup_path(&paths.antigravity_accounts).exists()
}

fn ensure_object(value: &mut Value, key: &str) {
    let should_reset = match value.get(key) {
        Some(inner) => !inner.is_object(),
        None => true,
    };

    if should_reset {
        value[key] = json!({});
    }
}

fn ensure_provider_object(provider: &mut serde_json::Map<String, Value>, provider_name: &str) {
    let should_reset = match provider.get(provider_name) {
        Some(inner) => !inner.is_object(),
        None => true,
    };

    if should_reset {
        provider.insert(provider_name.to_string(), json!({}));
    }
}

fn merge_provider_options(provider: &mut Value, base_url: &str, api_key: &str) {
    ensure_object(provider, "options");
    if let Some(options) = provider.get_mut("options").and_then(|v| v.as_object_mut()) {
        options.insert("baseURL".to_string(), Value::String(base_url.to_string()));
        options.insert("apiKey".to_string(), Value::String(api_key.to_string()));
    }
}

fn add_missing_models(provider: &mut Value, model_ids: &[&str]) {
    ensure_object(provider, "models");
    if let Some(models) = provider.get_mut("models").and_then(|v| v.as_object_mut()) {
        for model_id in model_ids {
            if !models.contains_key(*model_id) {
                models.insert(model_id.to_string(), json!({ "name": model_id }));
            }
        }
    }
}

fn is_valid_version(value: &str) -> bool {
    value
        .chars()
        .next()
        .map(|ch| ch.is_ascii_digit())
        .unwrap_or(false)
        && value.contains('.')
        && value.chars().all(|ch| ch.is_ascii_digit() || ch == '.')
}

fn extract_version(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "unknown".to_string();
    }

    for part in trimmed.split_whitespace() {
        if let Some(slash_index) = part.find('/') {
            let after_slash = &part[slash_index + 1..];
            if is_valid_version(after_slash) {
                return after_slash.to_string();
            }
        }

        if is_valid_version(part) {
            return part.to_string();
        }
    }

    let detected: String = trimmed
        .chars()
        .skip_while(|ch| !ch.is_ascii_digit())
        .take_while(|ch| ch.is_ascii_digit() || *ch == '.')
        .collect();

    if is_valid_version(&detected) {
        detected
    } else {
        "unknown".to_string()
    }
}

fn find_in_path(executable: &str) -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        if let Ok(path_var) = env::var("PATH") {
            for directory in path_var.split(';') {
                for ext in ["exe", "cmd", "bat"] {
                    let full_path = PathBuf::from(directory).join(format!("{}.{}", executable, ext));
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
            for directory in path_var.split(':') {
                let full_path = PathBuf::from(directory).join(executable);
                if full_path.exists() {
                    return Some(full_path);
                }
            }
        }
    }

    None
}

#[cfg(target_os = "windows")]
fn scan_nvm_directory(base_path: &Path) -> Option<PathBuf> {
    if !base_path.exists() {
        return None;
    }

    let entries = fs::read_dir(base_path).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let cmd_path = path.join("opencode.cmd");
        if cmd_path.exists() {
            return Some(cmd_path);
        }

        let exe_path = path.join("opencode.exe");
        if exe_path.exists() {
            return Some(exe_path);
        }
    }

    None
}

#[cfg(not(target_os = "windows"))]
fn scan_node_versions(base_path: &Path) -> Option<PathBuf> {
    if !base_path.exists() {
        return None;
    }

    let entries = fs::read_dir(base_path).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let binary_path = path.join("bin").join("opencode");
        if binary_path.exists() {
            return Some(binary_path);
        }
    }

    None
}

#[cfg(not(target_os = "windows"))]
fn scan_fnm_versions(base_path: &Path) -> Option<PathBuf> {
    if !base_path.exists() {
        return None;
    }

    let entries = fs::read_dir(base_path).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let binary_path = path.join("installation").join("bin").join("opencode");
        if binary_path.exists() {
            return Some(binary_path);
        }
    }

    None
}

#[cfg(target_os = "windows")]
fn resolve_opencode_path_windows() -> Option<PathBuf> {
    if let Ok(app_data) = env::var("APPDATA") {
        for candidate in [
            PathBuf::from(&app_data).join("npm").join("opencode.cmd"),
            PathBuf::from(&app_data).join("npm").join("opencode.exe"),
        ] {
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    if let Ok(local_app_data) = env::var("LOCALAPPDATA") {
        for candidate in [
            PathBuf::from(&local_app_data)
                .join("pnpm")
                .join("opencode.cmd"),
            PathBuf::from(&local_app_data)
                .join("pnpm")
                .join("opencode.exe"),
            PathBuf::from(&local_app_data)
                .join("Yarn")
                .join("bin")
                .join("opencode.cmd"),
        ] {
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    if let Ok(nvm_home) = env::var("NVM_HOME") {
        let nvm_path = PathBuf::from(nvm_home);
        if let Some(found) = scan_nvm_directory(&nvm_path) {
            return Some(found);
        }
    }

    if let Some(home) = dirs::home_dir() {
        let default_nvm = home.join(".nvm");
        if let Some(found) = scan_nvm_directory(&default_nvm) {
            return Some(found);
        }
    }

    None
}

#[cfg(not(target_os = "windows"))]
fn resolve_opencode_path_unix() -> Option<PathBuf> {
    let home = dirs::home_dir()?;

    for candidate in [
        home.join(".local").join("bin").join("opencode"),
        home.join(".npm-global").join("bin").join("opencode"),
        home.join("bin").join("opencode"),
        PathBuf::from("/opt/homebrew/bin/opencode"),
        PathBuf::from("/usr/local/bin/opencode"),
        PathBuf::from("/usr/bin/opencode"),
    ] {
        if candidate.exists() {
            return Some(candidate);
        }
    }

    if let Some(found) = scan_node_versions(&home.join(".nvm").join("versions").join("node")) {
        return Some(found);
    }

    if let Some(found) = scan_fnm_versions(&home.join(".fnm").join("node-versions")) {
        return Some(found);
    }

    if let Some(found) = scan_fnm_versions(
        &home
            .join("Library")
            .join("Application Support")
            .join("fnm")
            .join("node-versions"),
    ) {
        return Some(found);
    }

    None
}

fn resolve_opencode_path() -> Option<PathBuf> {
    if let Some(path) = find_in_path("opencode") {
        return Some(path);
    }

    #[cfg(target_os = "windows")]
    {
        resolve_opencode_path_windows()
    }

    #[cfg(not(target_os = "windows"))]
    {
        resolve_opencode_path_unix()
    }
}

#[cfg(target_os = "windows")]
fn run_opencode_version(binary_path: &Path) -> Option<String> {
    let binary = binary_path.to_string_lossy();
    let is_cmd = binary.ends_with(".cmd") || binary.ends_with(".bat");

    let output = if is_cmd {
        let mut command = Command::new("cmd.exe");
        command
            .arg("/C")
            .arg(binary_path)
            .arg("--version")
            .creation_flags(CREATE_NO_WINDOW);
        command.output()
    } else {
        let mut command = Command::new(binary_path);
        command.arg("--version").creation_flags(CREATE_NO_WINDOW);
        command.output()
    };

    let output = match output {
        Ok(output) if output.status.success() => output,
        _ => return None,
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let raw = if stdout.trim().is_empty() {
        stderr.to_string()
    } else {
        stdout.to_string()
    };

    Some(extract_version(&raw))
}

#[cfg(not(target_os = "windows"))]
fn run_opencode_version(binary_path: &Path) -> Option<String> {
    let output = match Command::new(binary_path).arg("--version").output() {
        Ok(output) if output.status.success() => output,
        _ => return None,
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let raw = if stdout.trim().is_empty() {
        stderr.to_string()
    } else {
        stdout.to_string()
    };

    Some(extract_version(&raw))
}

pub fn check_opencode_installed() -> (bool, Option<String>) {
    let binary_path = match resolve_opencode_path() {
        Some(path) => path,
        None => return (false, None),
    };

    let version = run_opencode_version(&binary_path);
    if version.is_some() {
        (true, version)
    } else {
        (false, None)
    }
}

fn get_provider_options<'a>(value: &'a Value, provider_name: &str) -> Option<&'a Value> {
    value
        .get("provider")
        .and_then(|provider| provider.get(provider_name))
        .and_then(|provider| provider.get("options"))
}

pub fn get_sync_status(proxy_url: &str) -> (bool, bool, Option<String>) {
    let paths = match get_config_paths() {
        Ok(paths) => paths,
        Err(_) => return (false, false, None),
    };

    let has_backup = has_any_backup(&paths);

    if !paths.opencode_config.exists() {
        return (false, has_backup, None);
    }

    let content = match fs::read_to_string(&paths.opencode_config) {
        Ok(content) => content,
        Err(_) => return (false, has_backup, None),
    };

    let config: Value = match serde_json::from_str(&content) {
        Ok(config) => config,
        Err(_) => return (false, has_backup, None),
    };

    let target_url = proxy_url.trim_end_matches('/');

    let anthropic_options = get_provider_options(&config, "anthropic");
    let anthropic_url = anthropic_options
        .and_then(|options| options.get("baseURL"))
        .and_then(|value| value.as_str());
    let anthropic_key = anthropic_options
        .and_then(|options| options.get("apiKey"))
        .and_then(|value| value.as_str());

    let google_options = get_provider_options(&config, "google");
    let google_url = google_options
        .and_then(|options| options.get("baseURL"))
        .and_then(|value| value.as_str());
    let google_key = google_options
        .and_then(|options| options.get("apiKey"))
        .and_then(|value| value.as_str());

    let mut is_synced = true;

    let current_base_url = anthropic_url
        .map(|url| url.to_string())
        .or_else(|| google_url.map(|url| url.to_string()));

    match (anthropic_url, anthropic_key) {
        (Some(url), Some(key)) if !key.trim().is_empty() => {
            if url.trim_end_matches('/') != target_url {
                is_synced = false;
            }
        }
        _ => is_synced = false,
    }

    match (google_url, google_key) {
        (Some(url), Some(key)) if !key.trim().is_empty() => {
            if url.trim_end_matches('/') != target_url {
                is_synced = false;
            }
        }
        _ => is_synced = false,
    }

    (is_synced, has_backup, current_base_url)
}

async fn sync_accounts_file(accounts_path: &Path) -> Result<(), String> {
    ensure_parent_dir(accounts_path)?;
    create_backup(accounts_path)?;

    let mut existing_rate_limits: HashMap<String, HashMap<String, i64>> = HashMap::new();

    if accounts_path.exists() {
        if let Ok(content) = fs::read_to_string(accounts_path) {
            if let Ok(value) = serde_json::from_str::<Value>(&content) {
                if let Some(accounts) = value.get("accounts").and_then(|v| v.as_array()) {
                    for account in accounts {
                        let email = account.get("email").and_then(|v| v.as_str());
                        let limits = account
                            .get("rateLimitResetTimes")
                            .and_then(|v| v.as_object());

                        if let (Some(email), Some(limits)) = (email, limits) {
                            let mut parsed_limits = HashMap::new();
                            for (model, timestamp) in limits {
                                if let Some(unix) = timestamp.as_i64() {
                                    parsed_limits.insert(model.clone(), unix);
                                }
                            }

                            if !parsed_limits.is_empty() {
                                existing_rate_limits.insert(email.to_string(), parsed_limits);
                            }
                        }
                    }
                }
            }
        }
    }

    let app_accounts = crate::modules::account::list_accounts()
        .await
        .map_err(|e| format!("Failed to list accounts: {}", e))?;

    let mut output_accounts = Vec::new();

    for account in app_accounts {
        if account.disabled || account.proxy_disabled {
            continue;
        }

        if account.token.refresh_token.trim().is_empty() {
            continue;
        }

        let rate_limit_reset_times = existing_rate_limits
            .get(&account.email)
            .cloned()
            .filter(|items| !items.is_empty());

        output_accounts.push(OpencodeAccount {
            email: account.email,
            refresh_token: account.token.refresh_token,
            project_id: account.token.project_id,
            rate_limit_reset_times,
        });
    }

    let output = json!({
        "accounts": output_accounts,
    });

    write_json_atomic(accounts_path, &output)
}

pub async fn sync_opencode_config(
    proxy_url: &str,
    api_key: &str,
    sync_accounts: bool,
) -> Result<(), String> {
    let paths = get_config_paths()?;
    fs::create_dir_all(&paths.dir)
        .map_err(|e| format!("Failed to create OpenCode directory {}: {}", paths.dir.display(), e))?;

    create_backup(&paths.opencode_config)?;
    create_backup(&paths.antigravity_config)?;

    let mut config = read_json_or_default_object(&paths.opencode_config);

    if config.get("$schema").is_none() {
        config["$schema"] = Value::String("https://opencode.ai/config.json".to_string());
    }

    let normalized_url = proxy_url.trim_end_matches('/').to_string();

    ensure_object(&mut config, "provider");
    if let Some(provider) = config.get_mut("provider").and_then(|v| v.as_object_mut()) {
        ensure_provider_object(provider, "anthropic");
        if let Some(anthropic) = provider.get_mut("anthropic") {
            merge_provider_options(anthropic, &normalized_url, api_key);
            add_missing_models(anthropic, ANTHROPIC_MODELS);
        }

        ensure_provider_object(provider, "google");
        if let Some(google) = provider.get_mut("google") {
            merge_provider_options(google, &normalized_url, api_key);
            add_missing_models(google, GOOGLE_MODELS);
        }
    }

    write_json_atomic(&paths.opencode_config, &config)?;

    let antigravity_config = json!({
        "baseURL": normalized_url,
        "apiKey": api_key,
        "updatedAt": chrono::Utc::now().timestamp(),
    });
    write_json_atomic(&paths.antigravity_config, &antigravity_config)?;

    if sync_accounts {
        sync_accounts_file(&paths.antigravity_accounts).await?;
    }

    Ok(())
}

fn restore_from_backup(target: &Path) -> Result<bool, String> {
    let backup = backup_path(target);
    if !backup.exists() {
        return Ok(false);
    }

    if target.exists() {
        fs::remove_file(target)
            .map_err(|e| format!("Failed to remove existing file {}: {}", target.display(), e))?;
    }

    fs::rename(&backup, target).map_err(|e| {
        format!(
            "Failed to restore backup {} to {}: {}",
            backup.display(),
            target.display(),
            e
        )
    })?;

    Ok(true)
}

pub fn restore_opencode_config() -> Result<(), String> {
    let paths = get_config_paths()?;

    let mut restored = false;
    for target in [
        &paths.opencode_config,
        &paths.antigravity_config,
        &paths.antigravity_accounts,
    ] {
        if restore_from_backup(target)? {
            restored = true;
        }
    }

    if restored {
        Ok(())
    } else {
        Err("No backup files found".to_string())
    }
}

pub fn read_opencode_config_content(file_name: Option<String>) -> Result<String, String> {
    let paths = get_config_paths()?;

    let requested_file = file_name.unwrap_or_else(|| OPENCODE_CONFIG_FILE.to_string());
    let target = match requested_file.as_str() {
        OPENCODE_CONFIG_FILE => &paths.opencode_config,
        ANTIGRAVITY_CONFIG_FILE => &paths.antigravity_config,
        ANTIGRAVITY_ACCOUNTS_FILE => &paths.antigravity_accounts,
        other => {
            return Err(format!(
                "Invalid file name '{}'. Allowed files: {}",
                other,
                known_files().join(", ")
            ))
        }
    };

    if !target.exists() {
        return Err(format!("Config file does not exist: {}", target.display()));
    }

    fs::read_to_string(target)
        .map_err(|e| format!("Failed to read config file {}: {}", target.display(), e))
}

#[tauri::command]
pub async fn get_opencode_sync_status(proxy_url: String) -> Result<OpencodeStatus, String> {
    let (installed, version) = check_opencode_installed();

    let (is_synced, has_backup, current_base_url) = if installed {
        get_sync_status(&proxy_url)
    } else {
        (false, false, None)
    };

    let files = match get_config_paths() {
        Ok(paths) => available_files(&paths),
        Err(_) => Vec::new(),
    };

    Ok(OpencodeStatus {
        installed,
        version,
        is_synced,
        has_backup,
        current_base_url,
        files,
    })
}

#[tauri::command]
pub async fn execute_opencode_sync(
    proxy_url: String,
    api_key: String,
    sync_accounts: Option<bool>,
) -> Result<(), String> {
    sync_opencode_config(&proxy_url, &api_key, sync_accounts.unwrap_or(false)).await
}

#[tauri::command]
pub async fn execute_opencode_restore() -> Result<(), String> {
    restore_opencode_config()
}

#[tauri::command]
pub async fn get_opencode_config_content(file_name: Option<String>) -> Result<String, String> {
    read_opencode_config_content(file_name)
}

#[cfg(test)]
mod tests {
    use super::extract_version;

    #[test]
    fn extract_version_from_opencode_format() {
        assert_eq!(extract_version("opencode/1.2.3"), "1.2.3");
    }

    #[test]
    fn extract_version_from_codex_style() {
        assert_eq!(extract_version("codex-cli 0.86.0"), "0.86.0");
    }

    #[test]
    fn extract_version_from_prefixed_tag() {
        assert_eq!(extract_version("v2.0.1"), "2.0.1");
    }

    #[test]
    fn extract_version_unknown_when_missing() {
        assert_eq!(extract_version("without version"), "unknown");
    }
}
