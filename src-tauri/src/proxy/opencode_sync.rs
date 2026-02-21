use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;
use std::fs;
use std::collections::HashMap;
use std::env;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

const OPENCODE_DIR: &str = ".config/opencode";
const OPENCODE_CONFIG_FILE: &str = "opencode.json";
const ANTIGRAVITY_CONFIG_FILE: &str = "antigravity.json";
const ANTIGRAVITY_ACCOUNTS_FILE: &str = "antigravity-accounts.json";
const BACKUP_SUFFIX: &str = ".antigravity-manager.bak";
const OLD_BACKUP_SUFFIX: &str = ".antigravity.bak";

const ANTIGRAVITY_PROVIDER_ID: &str = "antigravity-manager";

/// Variant type for model variants
#[derive(Debug, Clone, Copy)]
enum VariantType {
    /// Claude-style thinking with budget_tokens
    ClaudeThinking,
    /// Gemini 3 Pro style with thinkingLevel
    Gemini3Pro,
    /// Gemini 3 Flash style with thinkingLevel
    Gemini3Flash,
    /// Gemini 2.5 thinking style
    Gemini25Thinking,
}

/// Model definition with metadata and variants
#[derive(Debug, Clone)]
struct ModelDef {
    id: &'static str,
    name: &'static str,
    context_limit: u32,
    output_limit: u32,
    input_modalities: &'static [&'static str],
    output_modalities: &'static [&'static str],
    reasoning: bool,
    variant_type: Option<VariantType>,
}

/// Build the complete model catalog for antigravity-manager provider
fn build_model_catalog() -> Vec<ModelDef> {
    vec![
        // Claude models
        ModelDef {
            id: "claude-sonnet-4-6",
            name: "Claude Sonnet 4.6",
            context_limit: 200_000,
            output_limit: 64_000,
            input_modalities: &["text", "image", "pdf"],
            output_modalities: &["text"],
            reasoning: false,
            variant_type: None,
        },
        ModelDef {
            id: "claude-sonnet-4-6-thinking",
            name: "Claude Sonnet 4.6 Thinking",
            context_limit: 200_000,
            output_limit: 64_000,
            input_modalities: &["text", "image", "pdf"],
            output_modalities: &["text"],
            reasoning: true,
            variant_type: Some(VariantType::ClaudeThinking),
        },
        ModelDef {
            id: "claude-opus-4-5-thinking",
            name: "Claude Opus 4.5 Thinking",
            context_limit: 200_000,
            output_limit: 64_000,
            input_modalities: &["text", "image", "pdf"],
            output_modalities: &["text"],
            reasoning: true,
            variant_type: Some(VariantType::ClaudeThinking),
        },
        ModelDef {
            id: "claude-opus-4-6-thinking",
            name: "Claude Opus 4.6 Thinking",
            context_limit: 200_000,
            output_limit: 64_000,
            input_modalities: &["text", "image", "pdf"],
            output_modalities: &["text"],
            reasoning: true,
            variant_type: Some(VariantType::ClaudeThinking),
        },
        // Gemini 3.1 Pro models
        ModelDef {
            id: "gemini-3.1-pro-high",
            name: "Gemini 3.1 Pro High",
            context_limit: 1_048_576,
            output_limit: 65_535,
            input_modalities: &["text", "image", "pdf"],
            output_modalities: &["text", "image"],
            reasoning: true,
            variant_type: Some(VariantType::Gemini3Pro),
        },
        ModelDef {
            id: "gemini-3.1-pro-low",
            name: "Gemini 3.1 Pro Low",
            context_limit: 1_048_576,
            output_limit: 65_535,
            input_modalities: &["text", "image", "pdf"],
            output_modalities: &["text", "image"],
            reasoning: true,
            variant_type: Some(VariantType::Gemini3Pro),
        },
        ModelDef {
            id: "gemini-3-flash",
            name: "Gemini 3 Flash",
            context_limit: 1_048_576,
            output_limit: 65_536,
            input_modalities: &["text", "image", "pdf"],
            output_modalities: &["text"],
            reasoning: true,
            variant_type: Some(VariantType::Gemini3Flash),
        },
        ModelDef {
            id: "gemini-3-pro-image",
            name: "Gemini 3 Pro Image",
            context_limit: 1_048_576,
            output_limit: 65_535,
            input_modalities: &["text", "image", "pdf"],
            output_modalities: &["text", "image"],
            reasoning: false,
            variant_type: None,
        },
        // Gemini 2.5 models
        ModelDef {
            id: "gemini-2.5-flash",
            name: "Gemini 2.5 Flash",
            context_limit: 1_048_576,
            output_limit: 65_536,
            input_modalities: &["text", "image", "pdf"],
            output_modalities: &["text"],
            reasoning: false,
            variant_type: None,
        },
        ModelDef {
            id: "gemini-2.5-flash-lite",
            name: "Gemini 2.5 Flash Lite",
            context_limit: 1_048_576,
            output_limit: 65_536,
            input_modalities: &["text", "image", "pdf"],
            output_modalities: &["text"],
            reasoning: false,
            variant_type: None,
        },
        ModelDef {
            id: "gemini-2.5-flash-thinking",
            name: "Gemini 2.5 Flash Thinking",
            context_limit: 1_048_576,
            output_limit: 65_536,
            input_modalities: &["text", "image", "pdf"],
            output_modalities: &["text"],
            reasoning: true,
            variant_type: Some(VariantType::Gemini25Thinking),
        },
        ModelDef {
            id: "gemini-2.5-pro",
            name: "Gemini 2.5 Pro",
            context_limit: 1_048_576,
            output_limit: 65_536,
            input_modalities: &["text", "image", "pdf"],
            output_modalities: &["text"],
            reasoning: true,
            variant_type: None,
        },
    ]
}

/// Normalize OpenCode base URL to ensure it ends with `/v1` (Anthropic protocol requirement)
/// - Trims trailing `/`
/// - If already ends with `/v1`, keeps it as-is
/// - Otherwise appends `/v1`
fn normalize_opencode_base_url(input: &str) -> String {
    let trimmed = input.trim().trim_end_matches('/');
    if trimmed.ends_with("/v1") {
        trimmed.to_string()
    } else {
        format!("{}/v1", trimmed)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OpencodeStatus {
    pub installed: bool,
    pub version: Option<String>,
    pub is_synced: bool,
    pub has_backup: bool,
    pub current_base_url: Option<String>,
    pub files: Vec<String>,
}

/// Plugin schema v3 account structure
#[derive(Debug, Serialize, Deserialize, Clone)]
struct PluginAccount {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    email: Option<String>,
    #[serde(rename = "refreshToken")]
    refresh_token: String,
    #[serde(default, rename = "projectId", skip_serializing_if = "Option::is_none")]
    project_id: Option<String>,
    #[serde(rename = "addedAt")]
    added_at: i64,
    #[serde(rename = "lastUsed")]
    last_used: i64,
    #[serde(rename = "rateLimitResetTimes", skip_serializing_if = "Option::is_none")]
    rate_limit_reset_times: Option<HashMap<String, i64>>,
    // Optional preserved state fields
    #[serde(rename = "managedProjectId", skip_serializing_if = "Option::is_none")]
    managed_project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    enabled: Option<bool>,
    #[serde(rename = "lastSwitchReason", skip_serializing_if = "Option::is_none")]
    last_switch_reason: Option<String>,
    #[serde(rename = "coolingDownUntil", skip_serializing_if = "Option::is_none")]
    cooling_down_until: Option<i64>,
    #[serde(rename = "cooldownReason", skip_serializing_if = "Option::is_none")]
    cooldown_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fingerprint: Option<Value>,
    #[serde(rename = "cachedQuota", skip_serializing_if = "Option::is_none")]
    cached_quota: Option<Value>,
    #[serde(rename = "cachedQuotaUpdatedAt", skip_serializing_if = "Option::is_none")]
    cached_quota_updated_at: Option<i64>,
    #[serde(rename = "fingerprintHistory", skip_serializing_if = "Option::is_none")]
    fingerprint_history: Option<Value>,
}

/// Plugin schema v3 accounts file structure
#[derive(Debug, Serialize, Deserialize)]
struct PluginAccountsFile {
    version: i32,
    accounts: Vec<PluginAccount>,
    #[serde(rename = "activeIndex")]
    active_index: i32,
    #[serde(rename = "activeIndexByFamily")]
    active_index_by_family: HashMap<String, i32>,
}

fn get_opencode_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(OPENCODE_DIR))
}

fn get_config_paths() -> Option<(PathBuf, PathBuf, PathBuf)> {
    get_opencode_dir().map(|dir| {
        (
            dir.join(OPENCODE_CONFIG_FILE),
            dir.join(ANTIGRAVITY_CONFIG_FILE),
            dir.join(ANTIGRAVITY_ACCOUNTS_FILE),
        )
    })
}

fn extract_version(raw: &str) -> String {
    let trimmed = raw.trim();
    
    // Try to extract version from formats like "opencode/1.2.3" or "codex-cli 0.86.0"
    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    for part in parts {
        // Check for format like "opencode/1.2.3"
        if let Some(slash_idx) = part.find('/') {
            let after_slash = &part[slash_idx + 1..];
            if is_valid_version(after_slash) {
                return after_slash.to_string();
            }
        }
        // Check if part itself looks like a version
        if is_valid_version(part) {
            return part.to_string();
        }
    }
    
    // Fallback: extract last sequence of digits and dots
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

fn is_valid_version(s: &str) -> bool {
    // A valid version should start with digit and contain at least one dot
    s.chars().next().map_or(false, |c| c.is_ascii_digit())
        && s.contains('.')
        && s.chars().all(|c| c.is_ascii_digit() || c == '.')
}

fn resolve_opencode_path() -> Option<PathBuf> {
    // First, try to find in PATH
    if let Some(path) = find_in_path("opencode") {
        tracing::debug!("Found opencode in PATH: {:?}", path);
        return Some(path);
    }
    
    // Try fallback locations based on OS
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
fn resolve_opencode_path_windows() -> Option<PathBuf> {
    // Check npm global location
    if let Ok(app_data) = env::var("APPDATA") {
        let npm_opencode_cmd = PathBuf::from(&app_data).join("npm").join("opencode.cmd");
        if npm_opencode_cmd.exists() {
            tracing::debug!("Found opencode.cmd in APPDATA\\npm: {:?}", npm_opencode_cmd);
            return Some(npm_opencode_cmd);
        }
        let npm_opencode_exe = PathBuf::from(&app_data).join("npm").join("opencode.exe");
        if npm_opencode_exe.exists() {
            tracing::debug!("Found opencode.exe in APPDATA\\npm: {:?}", npm_opencode_exe);
            return Some(npm_opencode_exe);
        }
    }
    
    // Check pnpm location
    if let Ok(local_app_data) = env::var("LOCALAPPDATA") {
        let pnpm_opencode_cmd = PathBuf::from(&local_app_data).join("pnpm").join("opencode.cmd");
        if pnpm_opencode_cmd.exists() {
            tracing::debug!("Found opencode.cmd in LOCALAPPDATA\\pnpm: {:?}", pnpm_opencode_cmd);
            return Some(pnpm_opencode_cmd);
        }
        let pnpm_opencode_exe = PathBuf::from(&local_app_data).join("pnpm").join("opencode.exe");
        if pnpm_opencode_exe.exists() {
            tracing::debug!("Found opencode.exe in LOCALAPPDATA\\pnpm: {:?}", pnpm_opencode_exe);
            return Some(pnpm_opencode_exe);
        }
    }
    
    // Check Yarn location
    if let Ok(local_app_data) = env::var("LOCALAPPDATA") {
        let yarn_opencode = PathBuf::from(&local_app_data)
            .join("Yarn")
            .join("bin")
            .join("opencode.cmd");
        if yarn_opencode.exists() {
            tracing::debug!("Found opencode.cmd in Yarn bin: {:?}", yarn_opencode);
            return Some(yarn_opencode);
        }
    }
    
    // Scan NVM_HOME
    if let Ok(nvm_home) = env::var("NVM_HOME") {
        if let Some(path) = scan_nvm_directory(&nvm_home) {
            return Some(path);
        }
    }
    
    // Try common NVM locations
    if let Some(home) = dirs::home_dir() {
        let nvm_default = home.join(".nvm");
        if let Some(path) = scan_nvm_directory(&nvm_default) {
            return Some(path);
        }
    }
    
    None
}

#[cfg(not(target_os = "windows"))]
fn resolve_opencode_path_unix() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    
    // Common user bin locations
    let user_bins = [
        home.join(".local").join("bin").join("opencode"),
        home.join(".npm-global").join("bin").join("opencode"),
        home.join(".volta").join("bin").join("opencode"),
        home.join("bin").join("opencode"),
    ];
    
    for path in &user_bins {
        if path.exists() {
            tracing::debug!("Found opencode in user bin: {:?}", path);
            return Some(path.clone());
        }
    }
    
    // System-wide locations
    let system_bins = [
        PathBuf::from("/opt/homebrew/bin/opencode"),
        PathBuf::from("/usr/local/bin/opencode"),
        PathBuf::from("/usr/bin/opencode"),
    ];
    
    for path in &system_bins {
        if path.exists() {
            tracing::debug!("Found opencode in system bin: {:?}", path);
            return Some(path.clone());
        }
    }
    
    // Scan nvm directories
    let nvm_dirs = [
        home.join(".nvm").join("versions").join("node"),
    ];
    
    for nvm_dir in &nvm_dirs {
        if let Some(path) = scan_node_versions(nvm_dir) {
            return Some(path);
        }
    }
    
    // Scan fnm directories
    let fnm_dirs = [
        home.join(".fnm").join("node-versions"),
        home.join("Library").join("Application Support").join("fnm").join("node-versions"),
    ];
    
    for fnm_dir in &fnm_dirs {
        if let Some(path) = scan_fnm_versions(fnm_dir) {
            return Some(path);
        }
    }
    
    None
}

#[cfg(target_os = "windows")]
fn scan_nvm_directory(nvm_path: impl AsRef<std::path::Path>) -> Option<PathBuf> {
    let nvm_path = nvm_path.as_ref();
    if !nvm_path.exists() {
        return None;
    }
    
    let entries = fs::read_dir(nvm_path).ok()?;
    
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let opencode_cmd = path.join("opencode.cmd");
            if opencode_cmd.exists() {
                tracing::debug!("Found opencode.cmd in NVM: {:?}", opencode_cmd);
                return Some(opencode_cmd);
            }
            let opencode_exe = path.join("opencode.exe");
            if opencode_exe.exists() {
                tracing::debug!("Found opencode.exe in NVM: {:?}", opencode_exe);
                return Some(opencode_exe);
            }
        }
    }
    
    None
}

#[cfg(not(target_os = "windows"))]
fn scan_node_versions(versions_dir: impl AsRef<std::path::Path>) -> Option<PathBuf> {
    let versions_dir = versions_dir.as_ref();
    if !versions_dir.exists() {
        return None;
    }
    
    let entries = fs::read_dir(versions_dir).ok()?;
    
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let opencode = path.join("bin").join("opencode");
            if opencode.exists() {
                tracing::debug!("Found opencode in nvm: {:?}", opencode);
                return Some(opencode);
            }
        }
    }
    
    None
}

#[cfg(not(target_os = "windows"))]
fn scan_fnm_versions(versions_dir: impl AsRef<std::path::Path>) -> Option<PathBuf> {
    let versions_dir = versions_dir.as_ref();
    if !versions_dir.exists() {
        return None;
    }
    
    let entries = fs::read_dir(versions_dir).ok()?;
    
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let opencode = path.join("installation").join("bin").join("opencode");
            if opencode.exists() {
                tracing::debug!("Found opencode in fnm: {:?}", opencode);
                return Some(opencode);
            }
        }
    }
    
    None
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

#[cfg(target_os = "windows")]
fn run_opencode_version(opencode_path: &PathBuf) -> Option<String> {
    let path_str = opencode_path.to_string_lossy();
    
    // Check if it's a .cmd or .bat file that needs cmd.exe
    let is_cmd = path_str.ends_with(".cmd") || path_str.ends_with(".bat");
    
    let output = if is_cmd {
        let mut cmd = Command::new("cmd.exe");
        cmd.arg("/C")
            .arg(opencode_path)
            .arg("--version")
            .creation_flags(CREATE_NO_WINDOW);
        cmd.output()
    } else {
        let mut cmd = Command::new(opencode_path);
        cmd.arg("--version")
            .creation_flags(CREATE_NO_WINDOW);
        cmd.output()
    };
    
    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            
            // Some tools output version to stderr
            let raw = if stdout.trim().is_empty() {
                stderr.to_string()
            } else {
                stdout.to_string()
            };
            
            tracing::debug!("opencode --version output: {}", raw.trim());
            Some(extract_version(&raw))
        }
        Ok(output) => {
            tracing::debug!("opencode --version failed with status: {:?}", output.status);
            None
        }
        Err(e) => {
            tracing::debug!("Failed to run opencode --version: {}", e);
            None
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn run_opencode_version(opencode_path: &PathBuf) -> Option<String> {
    let output = Command::new(opencode_path)
        .arg("--version")
        .output();
    
    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            
            // Some tools output version to stderr
            let raw = if stdout.trim().is_empty() {
                stderr.to_string()
            } else {
                stdout.to_string()
            };
            
            tracing::debug!("opencode --version output: {}", raw.trim());
            Some(extract_version(&raw))
        }
        Ok(output) => {
            tracing::debug!("opencode --version failed with status: {:?}", output.status);
            None
        }
        Err(e) => {
            tracing::debug!("Failed to run opencode --version: {}", e);
            None
        }
    }
}

pub fn check_opencode_installed() -> (bool, Option<String>) {
    tracing::debug!("Checking opencode installation...");
    
    let opencode_path = match resolve_opencode_path() {
        Some(path) => {
            tracing::debug!("Resolved opencode path: {:?}", path);
            path
        }
        None => {
            tracing::debug!("Could not resolve opencode path");
            return (false, None);
        }
    };
    
    match run_opencode_version(&opencode_path) {
        Some(version) => {
            tracing::debug!("opencode version detected: {}", version);
            (true, Some(version))
        }
        None => {
            tracing::debug!("Failed to get opencode version");
            (false, None)
        }
    }
}

fn get_provider_options<'a>(value: &'a Value, provider_name: &str) -> Option<&'a Value> {
    value.get("provider")
        .and_then(|p| p.get(provider_name))
        .and_then(|prov| prov.get("options"))
}

pub fn get_sync_status(proxy_url: &str) -> (bool, bool, Option<String>) {
    let Some((config_path, _, _)) = get_config_paths() else {
        return (false, false, None);
    };

    let mut is_synced = true;
    let mut has_backup = false;
    let mut current_base_url = None;

    let backup_path = config_path.with_file_name(
        format!("{}{}", OPENCODE_CONFIG_FILE, BACKUP_SUFFIX)
    );
    let old_backup_path = config_path.with_file_name(
        format!("{}{}", OPENCODE_CONFIG_FILE, OLD_BACKUP_SUFFIX)
    );
    if backup_path.exists() || old_backup_path.exists() {
        has_backup = true;
    }

    if !config_path.exists() {
        return (false, has_backup, None);
    }

    let content = match fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(_) => return (false, has_backup, None),
    };

    let json: Value = serde_json::from_str(&content).unwrap_or_default();

    // Normalize proxy URL for comparison
    let normalized_proxy = normalize_opencode_base_url(proxy_url);

    // Only check antigravity-manager provider
    let ag_opts = get_provider_options(&json, ANTIGRAVITY_PROVIDER_ID);
    let ag_url = ag_opts
        .and_then(|o| o.get("baseURL"))
        .and_then(|v| v.as_str());
    let ag_key = ag_opts
        .and_then(|o| o.get("apiKey"))
        .and_then(|v| v.as_str());

    if let (Some(url), Some(_key)) = (ag_url, ag_key) {
        current_base_url = Some(url.to_string());
        // Normalize config URL before comparison
        let normalized_config_url = normalize_opencode_base_url(url);
        if normalized_config_url != normalized_proxy {
            is_synced = false;
        }
    } else {
        is_synced = false;
    }

    (is_synced, has_backup, current_base_url)
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

    fs::copy(path, &backup_path)
        .map_err(|e| format!("Failed to create backup: {}", e))?;

    Ok(())
}

fn restore_backup_to_target(backup_path: &PathBuf, target_path: &PathBuf, label: &str) -> Result<(), String> {
    if target_path.exists() {
        fs::remove_file(target_path)
            .map_err(|e| format!("Failed to remove existing {}: {}", label, e))?;
    }

    fs::rename(backup_path, target_path)
        .map_err(|e| format!("Failed to restore {}: {}", label, e))
}

fn ensure_object(value: &mut Value, key: &str) {
    let needs_reset = match value.get(key) {
        None => true,
        Some(v) if !v.is_object() => true,
        _ => false,
    };
    if needs_reset {
        value[key] = serde_json::json!({});
    }
}

fn ensure_provider_object(provider: &mut serde_json::Map<String, Value>, name: &str) {
    let needs_reset = match provider.get(name) {
        None => true,
        Some(v) if !v.is_object() => true,
        _ => false,
    };
    if needs_reset {
        provider.insert(name.to_string(), serde_json::json!({}));
    }
}

fn merge_provider_options(provider: &mut Value, base_url: &str, api_key: &str) {
    if provider.get("options").is_none() {
        provider["options"] = serde_json::json!({});
    }
    
    if let Some(options) = provider.get_mut("options").and_then(|o| o.as_object_mut()) {
        options.insert("baseURL".to_string(), Value::String(base_url.to_string()));
        options.insert("apiKey".to_string(), Value::String(api_key.to_string()));
    }
}

fn ensure_provider_string_field(provider: &mut Value, key: &str, value: &str) {
    if let Some(obj) = provider.as_object_mut() {
        obj.insert(key.to_string(), Value::String(value.to_string()));
    }
}

/// Build Claude-style thinking variant with thinkingConfig and thinking
fn build_claude_thinking_variant(budget: u32) -> Value {
    serde_json::json!({
        "thinkingConfig": {
            "thinkingBudget": budget
        },
        "thinking": {
            "type": "enabled",
            "budget_tokens": budget
        }
    })
}

/// Build Gemini 3 style variant with thinkingLevel
fn build_gemini3_variant(level: &str) -> Value {
    serde_json::json!({
        "thinkingLevel": level
    })
}

/// Build Gemini 2.5 thinking variant with thinkingConfig and thinking
fn build_gemini25_thinking_variant(budget: u32) -> Value {
    serde_json::json!({
        "thinkingConfig": {
            "thinkingBudget": budget
        },
        "thinking": {
            "type": "enabled",
            "budget_tokens": budget
        }
    })
}

/// Build variants object based on variant type
fn build_variants_object(variant_type: Option<VariantType>) -> Option<Value> {
    match variant_type {
        Some(VariantType::ClaudeThinking) => {
            let mut variants = serde_json::Map::new();
            variants.insert("low".to_string(), build_claude_thinking_variant(8192));
            variants.insert("medium".to_string(), build_claude_thinking_variant(16384));
            variants.insert("high".to_string(), build_claude_thinking_variant(24576));
            variants.insert("max".to_string(), build_claude_thinking_variant(32768));
            Some(Value::Object(variants))
        }
        Some(VariantType::Gemini3Pro) => {
            let mut variants = serde_json::Map::new();
            variants.insert("low".to_string(), build_gemini3_variant("low"));
            variants.insert("high".to_string(), build_gemini3_variant("high"));
            Some(Value::Object(variants))
        }
        Some(VariantType::Gemini3Flash) => {
            let mut variants = serde_json::Map::new();
            variants.insert("minimal".to_string(), build_gemini3_variant("minimal"));
            variants.insert("low".to_string(), build_gemini3_variant("low"));
            variants.insert("medium".to_string(), build_gemini3_variant("medium"));
            variants.insert("high".to_string(), build_gemini3_variant("high"));
            Some(Value::Object(variants))
        }
        Some(VariantType::Gemini25Thinking) => {
            let mut variants = serde_json::Map::new();
            variants.insert("low".to_string(), build_gemini25_thinking_variant(8192));
            variants.insert("medium".to_string(), build_gemini25_thinking_variant(12288));
            variants.insert("high".to_string(), build_gemini25_thinking_variant(16384));
            variants.insert("max".to_string(), build_gemini25_thinking_variant(24576));
            Some(Value::Object(variants))
        }
        None => None,
    }
}

/// Build model JSON object with full metadata
fn build_model_json(model_def: &ModelDef) -> Value {
    let mut model_obj = serde_json::Map::new();
    
    model_obj.insert("name".to_string(), Value::String(model_def.name.to_string()));
    
    let limits = serde_json::json!({
        "context": model_def.context_limit,
        "output": model_def.output_limit,
    });
    model_obj.insert("limit".to_string(), limits);
    
    let modalities = serde_json::json!({
        "input": model_def.input_modalities,
        "output": model_def.output_modalities,
    });
    model_obj.insert("modalities".to_string(), modalities);
    
    if model_def.reasoning {
        model_obj.insert("reasoning".to_string(), Value::Bool(true));
    }
    
    // Build variants as object map instead of array
    if let Some(variants) = build_variants_object(model_def.variant_type) {
        model_obj.insert("variants".to_string(), variants);
    }
    
    Value::Object(model_obj)
}

/// Merge catalog models into provider.models without deleting user models
fn merge_catalog_models(provider: &mut Value, model_ids: Option<&[&str]>) {
    if provider.get("models").is_none() {
        provider["models"] = serde_json::json!({});
    }
    
    let catalog = build_model_catalog();
    let catalog_map: HashMap<&str, &ModelDef> = catalog.iter().map(|m| (m.id, m)).collect();
    
    if let Some(models) = provider.get_mut("models").and_then(|m| m.as_object_mut()) {
        let ids_to_sync: Vec<&str> = match model_ids {
            Some(ids) => ids.to_vec(),
            None => catalog_map.keys().copied().collect(),
        };
        
        for model_id in ids_to_sync {
            if let Some(model_def) = catalog_map.get(model_id) {
                let catalog_model = build_model_json(model_def);
                
                if let Some(existing) = models.get(model_id) {
                    // Merge: keep user-defined fields, update catalog fields
                    if let Some(existing_obj) = existing.as_object() {
                        let mut merged = existing_obj.clone();
                        
                        // Update/insert catalog fields
                        if let Some(catalog_obj) = catalog_model.as_object() {
                            for (key, value) in catalog_obj.iter() {
                                merged.insert(key.clone(), value.clone());
                            }
                        }
                        
                        models.insert(model_id.to_string(), Value::Object(merged));
                    } else {
                        // Existing is not an object, replace with catalog
                        models.insert(model_id.to_string(), catalog_model);
                    }
                } else {
                    // Model doesn't exist, insert full catalog entry
                    models.insert(model_id.to_string(), catalog_model);
                }
            }
        }
    }
}

pub fn sync_opencode_config(
    proxy_url: &str,
    api_key: &str,
    sync_accounts: bool,
    models_to_sync: Option<Vec<String>>,
) -> Result<(), String> {
    let Some((config_path, _ag_config_path, ag_accounts_path)) = get_config_paths() else {
        return Err("Failed to get OpenCode config directory".to_string());
    };

    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create directory: {}", e))?;
    }

    create_backup(&config_path)?;

    let mut config: Value = if config_path.exists() {
        fs::read_to_string(&config_path)
            .ok()
            .and_then(|c| serde_json::from_str(&c).ok())
            .unwrap_or_else(|| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    let model_refs: Option<Vec<&str>> = models_to_sync
        .as_ref()
        .map(|models| models.iter().map(|m| m.as_str()).collect());
    config = apply_sync_to_config(config, proxy_url, api_key, model_refs.as_deref());

    let tmp_path = config_path.with_extension("tmp");
    fs::write(&tmp_path, serde_json::to_string_pretty(&config).unwrap())
        .map_err(|e| format!("Failed to write temp file: {}", e))?;
    fs::rename(&tmp_path, &config_path)
        .map_err(|e| format!("Failed to rename config file: {}", e))?;

    if sync_accounts {
        sync_accounts_file(&ag_accounts_path)?;
    }

    Ok(())
}

fn sync_accounts_file(accounts_path: &PathBuf) -> Result<(), String> {
    create_backup(accounts_path)?;

    // Read existing file for state preservation
    let existing_content = if accounts_path.exists() {
        fs::read_to_string(accounts_path).ok()
    } else {
        None
    };

    // Parse existing accounts for state preservation (match by refresh_token first, then email)
    let mut existing_accounts_by_refresh_token: HashMap<String, PluginAccount> = HashMap::new();
    let mut existing_accounts_by_email: HashMap<String, PluginAccount> = HashMap::new();
    let mut existing_active_index: i32 = 0;
    let mut existing_active_index_by_family: HashMap<String, i32> = HashMap::new();

    if let Some(ref content) = existing_content {
        if let Ok(existing_json) = serde_json::from_str::<Value>(content) {
            // Parse existing accounts
            if let Some(existing_accounts) = existing_json.get("accounts").and_then(|a| a.as_array()) {
                for acc in existing_accounts {
                    if let Ok(plugin_acc) = serde_json::from_value::<PluginAccount>(acc.clone()) {
                        // Index by refresh_token (primary key for matching)
                        existing_accounts_by_refresh_token.insert(plugin_acc.refresh_token.clone(), plugin_acc.clone());
                        // Index by email (fallback)
                        if let Some(email) = &plugin_acc.email {
                            existing_accounts_by_email.insert(email.clone(), plugin_acc);
                        }
                    }
                }
            }
            // Parse existing active indices
            if let Some(idx) = existing_json.get("activeIndex").and_then(|v| v.as_i64()) {
                existing_active_index = idx as i32;
            }
            if let Some(family_indices) = existing_json.get("activeIndexByFamily").and_then(|v| v.as_object()) {
                for (key, val) in family_indices {
                    if let Some(idx) = val.as_i64() {
                        existing_active_index_by_family.insert(key.clone(), idx as i32);
                    }
                }
            }
        }
    }

    let app_accounts = crate::modules::account::list_accounts()
        .map_err(|e| format!("Failed to list accounts: {}", e))?;

    let mut new_accounts: Vec<PluginAccount> = Vec::new();

    for acc in app_accounts {
        // Skip disabled accounts (preserve existing logic)
        if acc.disabled || acc.proxy_disabled {
            continue;
        }

        let refresh_token = acc.token.refresh_token.clone();
        let project_id = acc.token.project_id.clone();

        // Try to find existing account state (match by refresh_token first, then email fallback)
        let existing = existing_accounts_by_refresh_token
            .get(&refresh_token)
            .cloned()
            .or_else(|| existing_accounts_by_email.get(&acc.email).cloned());

        let plugin_account = if let Some(existing) = existing {
                // Preserve existing state
                PluginAccount {
                    email: Some(acc.email),
                    refresh_token,
                    project_id,
                    added_at: existing.added_at,
                    last_used: existing.last_used.max(acc.last_used),
                    rate_limit_reset_times: existing.rate_limit_reset_times,
                    managed_project_id: existing.managed_project_id,
                    enabled: existing.enabled,
                last_switch_reason: existing.last_switch_reason,
                cooling_down_until: existing.cooling_down_until,
                cooldown_reason: existing.cooldown_reason,
                fingerprint: existing.fingerprint,
                cached_quota: existing.cached_quota,
                cached_quota_updated_at: existing.cached_quota_updated_at,
                fingerprint_history: existing.fingerprint_history,
            }
        } else {
            // New account - use defaults
            let now = chrono::Utc::now().timestamp_millis();
            PluginAccount {
                email: Some(acc.email),
                refresh_token,
                project_id,
                added_at: now,
                last_used: acc.last_used,
                rate_limit_reset_times: None,
                managed_project_id: None,
                enabled: None,
                last_switch_reason: None,
                cooling_down_until: None,
                cooldown_reason: None,
                fingerprint: None,
                cached_quota: None,
                cached_quota_updated_at: None,
                fingerprint_history: None,
            }
        };

        new_accounts.push(plugin_account);
    }

    // Clamp activeIndex to valid range
    let account_count = new_accounts.len() as i32;
    let clamped_active_index = if account_count > 0 {
        existing_active_index.clamp(0, account_count - 1)
    } else {
        0
    };

    // Clamp activeIndexByFamily values
    let mut clamped_active_index_by_family = HashMap::new();
    for (family, idx) in existing_active_index_by_family {
        let clamped_idx = if account_count > 0 {
            idx.clamp(0, account_count - 1)
        } else {
            0
        };
        clamped_active_index_by_family.insert(family, clamped_idx);
    }

    // Ensure family indices always exist for plugin v3 behavior.
    if !clamped_active_index_by_family.contains_key("claude") {
        clamped_active_index_by_family.insert("claude".to_string(), clamped_active_index);
    }
    if !clamped_active_index_by_family.contains_key("gemini") {
        clamped_active_index_by_family.insert("gemini".to_string(), clamped_active_index);
    }

    // Build schema v3 output
    let new_data = PluginAccountsFile {
        version: 3,
        accounts: new_accounts,
        active_index: clamped_active_index,
        active_index_by_family: clamped_active_index_by_family,
    };

    let tmp_path = accounts_path.with_extension("tmp");
    fs::write(&tmp_path, serde_json::to_string_pretty(&new_data).unwrap())
        .map_err(|e| format!("Failed to write accounts temp file: {}", e))?;
    fs::rename(&tmp_path, accounts_path)
        .map_err(|e| format!("Failed to rename accounts file: {}", e))?;

    Ok(())
}

pub fn restore_opencode_config() -> Result<(), String> {
    let Some((config_path, _, accounts_path)) = get_config_paths() else {
        return Err("Failed to get OpenCode config directory".to_string());
    };

    let mut restored = false;

    // Try new backup suffix first, fall back to old suffix for backward compatibility
    let config_backup_new = config_path.with_file_name(format!(
        "{}{}", OPENCODE_CONFIG_FILE, BACKUP_SUFFIX
    ));
    let config_backup_old = config_path.with_file_name(format!(
        "{}{}", OPENCODE_CONFIG_FILE, OLD_BACKUP_SUFFIX
    ));
    
    if config_backup_new.exists() {
        restore_backup_to_target(&config_backup_new, &config_path, "config")?;
        restored = true;
    } else if config_backup_old.exists() {
        restore_backup_to_target(&config_backup_old, &config_path, "config")?;
        restored = true;
    }

    // Try new backup suffix first, fall back to old suffix for backward compatibility
    let accounts_backup_new = accounts_path.with_file_name(format!(
        "{}{}", ANTIGRAVITY_ACCOUNTS_FILE, BACKUP_SUFFIX
    ));
    let accounts_backup_old = accounts_path.with_file_name(format!(
        "{}{}", ANTIGRAVITY_ACCOUNTS_FILE, OLD_BACKUP_SUFFIX
    ));
    
    if accounts_backup_new.exists() {
        restore_backup_to_target(&accounts_backup_new, &accounts_path, "accounts")?;
        restored = true;
    } else if accounts_backup_old.exists() {
        restore_backup_to_target(&accounts_backup_old, &accounts_path, "accounts")?;
        restored = true;
    }

    if restored {
        Ok(())
    } else {
        Err("No backup files found".to_string())
    }
}

/// Pure function: Apply sync logic to config JSON
/// Returns the modified config Value
fn apply_sync_to_config(
    mut config: Value,
    proxy_url: &str,
    api_key: &str,
    models_to_sync: Option<&[&str]>,
) -> Value {
    if !config.is_object() {
        config = serde_json::json!({});
    }

    if config.get("$schema").is_none() {
        config["$schema"] = Value::String("https://opencode.ai/config.json".to_string());
    }

    let normalized_url = normalize_opencode_base_url(proxy_url);

    ensure_object(&mut config, "provider");

    if let Some(provider) = config.get_mut("provider").and_then(|p| p.as_object_mut()) {
        ensure_provider_object(provider, ANTIGRAVITY_PROVIDER_ID);
        if let Some(ag_provider) = provider.get_mut(ANTIGRAVITY_PROVIDER_ID) {
            ensure_provider_string_field(ag_provider, "npm", "@ai-sdk/anthropic");
            ensure_provider_string_field(ag_provider, "name", "Antigravity Manager");
            merge_provider_options(ag_provider, &normalized_url, api_key);
            merge_catalog_models(ag_provider, models_to_sync);
        }
    }

    config
}

/// Pure function: Apply clear logic to config JSON
/// Returns the modified config Value
fn apply_clear_to_config(
    mut config: Value,
    proxy_url: Option<&str>,
    clear_legacy: bool,
) -> Value {
    if let Some(provider) = config.get_mut("provider").and_then(|p| p.as_object_mut()) {
        // 1. Remove antigravity-manager provider
        provider.remove(ANTIGRAVITY_PROVIDER_ID);

        // 2. Cleanup legacy entries if requested
        if clear_legacy {
            if let Some(proxy) = proxy_url {
                // Clean up provider.anthropic
                if let Some(anthropic) = provider.get_mut("anthropic") {
                    cleanup_legacy_provider(anthropic, proxy);
                }

                // Clean up provider.google
                if let Some(google) = provider.get_mut("google") {
                    cleanup_legacy_provider(google, proxy);
                }
            }
        }

        // Remove empty provider object if it has no entries
        if provider.is_empty() {
            if let Some(config_obj) = config.as_object_mut() {
                config_obj.remove("provider");
            }
        }
    }

    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_version_opencode_format() {
        let input = "opencode/1.2.3";
        assert_eq!(extract_version(input), "1.2.3");
    }

    #[test]
    fn test_extract_version_codex_cli_format() {
        let input = "codex-cli 0.86.0\n";
        assert_eq!(extract_version(input), "0.86.0");
    }

    #[test]
    fn test_extract_version_simple() {
        let input = "v2.0.1";
        assert_eq!(extract_version(input), "2.0.1");
    }

    #[test]
    fn test_extract_version_unknown() {
        let input = "some random text without version";
        assert_eq!(extract_version(input), "unknown");
    }

    #[test]
    fn test_normalize_opencode_base_url_without_v1() {
        assert_eq!(normalize_opencode_base_url("http://localhost:3000"), "http://localhost:3000/v1");
        assert_eq!(normalize_opencode_base_url("http://localhost:3000/"), "http://localhost:3000/v1");
    }

    #[test]
    fn test_normalize_opencode_base_url_with_v1() {
        assert_eq!(normalize_opencode_base_url("http://localhost:3000/v1"), "http://localhost:3000/v1");
        assert_eq!(normalize_opencode_base_url("http://localhost:3000/v1/"), "http://localhost:3000/v1");
    }

    #[test]
    fn test_normalize_opencode_base_url_with_whitespace() {
        assert_eq!(normalize_opencode_base_url("  http://localhost:3000  "), "http://localhost:3000/v1");
        assert_eq!(normalize_opencode_base_url("  http://localhost:3000/v1  "), "http://localhost:3000/v1");
    }

    #[test]
    fn test_normalize_opencode_base_url_no_double_v1() {
        // Ensure we don't create double /v1/v1
        assert_eq!(normalize_opencode_base_url("http://localhost:3000/v1"), "http://localhost:3000/v1");
        assert_eq!(normalize_opencode_base_url("http://localhost:3000/v1/"), "http://localhost:3000/v1");
    }

    // Tests for apply_sync_to_config

    #[test]
    fn test_sync_preserves_existing_providers() {
        // Config with existing google and anthropic providers
        let config = serde_json::json!({
            "provider": {
                "google": {
                    "options": { "apiKey": "google-key" },
                    "models": { "gemini-pro": { "name": "Gemini Pro" } }
                },
                "anthropic": {
                    "options": { "apiKey": "anthropic-key" },
                    "models": { "claude-3": { "name": "Claude 3" } }
                }
            }
        });

        let result = apply_sync_to_config(config, "http://localhost:3000", "test-api-key", None);

        // Existing providers should be preserved
        let provider = result.get("provider").unwrap();
        assert!(provider.get("google").is_some(), "google provider should be preserved");
        assert!(provider.get("anthropic").is_some(), "anthropic provider should be preserved");
        assert_eq!(
            provider.get("google").unwrap().get("options").unwrap().get("apiKey").unwrap(),
            "google-key"
        );
        assert_eq!(
            provider.get("anthropic").unwrap().get("options").unwrap().get("apiKey").unwrap(),
            "anthropic-key"
        );
    }

    #[test]
    fn test_sync_creates_antigravity_provider() {
        let config = serde_json::json!({});

        let result = apply_sync_to_config(config, "http://localhost:3000", "test-api-key", None);

        // antigravity-manager provider should be created
        let provider = result.get("provider").unwrap();
        let ag = provider.get(ANTIGRAVITY_PROVIDER_ID).unwrap();

        // Check npm and name
        assert_eq!(ag.get("npm").unwrap(), "@ai-sdk/anthropic");
        assert_eq!(ag.get("name").unwrap(), "Antigravity Manager");

        // Check options
        let options = ag.get("options").unwrap();
        assert_eq!(options.get("baseURL").unwrap(), "http://localhost:3000/v1");
        assert_eq!(options.get("apiKey").unwrap(), "test-api-key");
    }

    #[test]
    fn test_sync_creates_models() {
        let config = serde_json::json!({});

        let result = apply_sync_to_config(config, "http://localhost:3000", "test-api-key", None);

        let provider = result.get("provider").unwrap();
        let ag = provider.get(ANTIGRAVITY_PROVIDER_ID).unwrap();
        let models = ag.get("models").unwrap().as_object().unwrap();

        // Should have all catalog models
        assert!(models.contains_key("claude-sonnet-4-6"), "should have claude-sonnet-4-6");
        assert!(models.contains_key("gemini-3.1-pro-high"), "should have gemini-3.1-pro-high");
        assert!(models.contains_key("gemini-2.5-pro"), "should have gemini-2.5-pro");

        // Check model structure
        let claude_model = models.get("claude-sonnet-4-6").unwrap();
        assert_eq!(claude_model.get("name").unwrap(), "Claude Sonnet 4.6");
        assert!(claude_model.get("limit").is_some());
        assert!(claude_model.get("modalities").is_some());
    }

    #[test]
    fn test_sync_with_filtered_models() {
        let config = serde_json::json!({});
        let models_to_sync = &["claude-sonnet-4-6", "gemini-3.1-pro-high"];

        let result = apply_sync_to_config(config, "http://localhost:3000", "test-api-key", Some(models_to_sync));

        let provider = result.get("provider").unwrap();
        let ag = provider.get(ANTIGRAVITY_PROVIDER_ID).unwrap();
        let models = ag.get("models").unwrap().as_object().unwrap();

        assert!(models.contains_key("claude-sonnet-4-6"));
        assert!(models.contains_key("gemini-3.1-pro-high"));
        assert!(!models.contains_key("gemini-2.5-pro"), "should not have unselected models");
    }

    // Tests for apply_clear_to_config

    #[test]
    fn test_clear_removes_antigravity_provider() {
        let config = serde_json::json!({
            "provider": {
                "antigravity-manager": {
                    "options": { "baseURL": "http://localhost:3000/v1" }
                },
                "google": { "options": { "apiKey": "key" } }
            }
        });

        let result = apply_clear_to_config(config, None, false);

        let provider = result.get("provider").unwrap();
        assert!(provider.get(ANTIGRAVITY_PROVIDER_ID).is_none(), "antigravity-manager should be removed");
        assert!(provider.get("google").is_some(), "google should be preserved");
    }

    #[test]
    fn test_clear_legacy_removes_antigravity_models() {
        let config = serde_json::json!({
            "provider": {
                "anthropic": {
                    "options": { "baseURL": "http://localhost:3000/v1", "apiKey": "key" },
                    "models": {
                        "claude-sonnet-4-5": { "name": "Claude" },
                        "claude-3": { "name": "Claude 3" }
                    }
                }
            }
        });

        let result = apply_clear_to_config(config, Some("http://localhost:3000"), true);

        let provider = result.get("provider").unwrap();
        let anthropic = provider.get("anthropic").unwrap();
        let models = anthropic.get("models").unwrap().as_object().unwrap();

        // Antigravity model IDs should be removed
        assert!(!models.contains_key("claude-sonnet-4-5"), "antigravity model should be removed");
        // Non-antigravity models should be preserved
        assert!(models.contains_key("claude-3"), "non-antigravity model should be preserved");
    }

    #[test]
    fn test_clear_legacy_removes_options_when_baseurl_matches() {
        let config = serde_json::json!({
            "provider": {
                "anthropic": {
                    "options": { "baseURL": "http://localhost:3000/v1", "apiKey": "key" }
                }
            }
        });

        let result = apply_clear_to_config(config, Some("http://localhost:3000"), true);

        let provider = result.get("provider").unwrap();
        let anthropic = provider.get("anthropic").unwrap();

        // Options should be removed when baseURL matches
        assert!(anthropic.get("options").is_none(), "options should be removed when baseURL matches");
    }

    #[test]
    fn test_clear_legacy_preserves_options_when_baseurl_different() {
        let config = serde_json::json!({
            "provider": {
                "anthropic": {
                    "options": { "baseURL": "http://other-proxy.com/v1", "apiKey": "key" }
                }
            }
        });

        let result = apply_clear_to_config(config, Some("http://localhost:3000"), true);

        let provider = result.get("provider").unwrap();
        let anthropic = provider.get("anthropic").unwrap();
        let options = anthropic.get("options").unwrap();

        // Options should be preserved when baseURL doesn't match
        assert_eq!(options.get("baseURL").unwrap(), "http://other-proxy.com/v1");
        assert_eq!(options.get("apiKey").unwrap(), "key");
    }

    #[test]
    fn test_clear_legacy_without_proxy_url_skips_cleanup() {
        let config = serde_json::json!({
            "provider": {
                "anthropic": {
                    "options": { "baseURL": "http://localhost:3000/v1", "apiKey": "key" },
                    "models": { "claude-sonnet-4-5": { "name": "Claude" } }
                }
            }
        });

        // clear_legacy=true but no proxy_url provided
        let result = apply_clear_to_config(config, None, true);

        let provider = result.get("provider").unwrap();
        let anthropic = provider.get("anthropic").unwrap();

        // Legacy cleanup should be skipped when proxy_url is None
        assert!(anthropic.get("options").is_some(), "options should be preserved when no proxy_url");
        assert!(anthropic.get("models").is_some(), "models should be preserved when no proxy_url");
    }

    // Tests for base_url_matches

    #[test]
    fn test_base_url_matches_with_v1() {
        assert!(base_url_matches("http://localhost:3000/v1", "http://localhost:3000"));
        assert!(base_url_matches("http://localhost:3000", "http://localhost:3000/v1"));
        assert!(base_url_matches("http://localhost:3000/v1/", "http://localhost:3000"));
    }

    #[test]
    fn test_base_url_matches_without_v1() {
        assert!(base_url_matches("http://localhost:3000", "http://localhost:3000"));
        assert!(base_url_matches("http://localhost:3000/", "http://localhost:3000/"));
    }

    #[test]
    fn test_base_url_matches_different_urls() {
        assert!(!base_url_matches("http://localhost:3000", "http://other-host:3000"));
        assert!(!base_url_matches("http://localhost:3000/v1", "http://localhost:4000/v1"));
    }

    #[test]
    fn test_clear_removes_empty_provider() {
        let config = serde_json::json!({
            "provider": {
                "antigravity-manager": {
                    "options": { "baseURL": "http://localhost:3000/v1" }
                }
            }
        });

        let result = apply_clear_to_config(config, None, false);

        // Provider object should be removed when empty
        assert!(result.get("provider").is_none(), "empty provider object should be removed");
    }
}

pub fn read_opencode_config_content(file_name: Option<String>) -> Result<String, String> {
    let Some((opencode_path, ag_config_path, ag_accounts_path)) = get_config_paths() else {
        return Err("Failed to get OpenCode config directory".to_string());
    };

    // Allowlist of permitted file names
    let allowed_files = [
        OPENCODE_CONFIG_FILE,
        ANTIGRAVITY_CONFIG_FILE,
        ANTIGRAVITY_ACCOUNTS_FILE,
    ];

    // Determine which file to read
    let target_path = match file_name.as_deref() {
        Some(name) if name == ANTIGRAVITY_CONFIG_FILE => ag_config_path,
        Some(name) if name == ANTIGRAVITY_ACCOUNTS_FILE => ag_accounts_path,
        Some(name) if name == OPENCODE_CONFIG_FILE => opencode_path,
        Some(name) => {
            return Err(format!(
                "Invalid file name: {}. Allowed: {:?}",
                name, allowed_files
            ))
        }
        None => opencode_path, // Default to opencode.json
    };

    if !target_path.exists() {
        return Err(format!("Config file does not exist: {:?}", target_path));
    }

    fs::read_to_string(&target_path)
        .map_err(|e| format!("Failed to read config: {}", e))
}

#[tauri::command]
pub async fn get_opencode_sync_status(proxy_url: String) -> Result<OpencodeStatus, String> {
    let (installed, version) = check_opencode_installed();
    let (is_synced, has_backup, current_base_url) = get_sync_status(&proxy_url);

    Ok(OpencodeStatus {
        installed,
        version,
        is_synced,
        has_backup,
        current_base_url,
        files: vec![
            OPENCODE_CONFIG_FILE.to_string(),
            ANTIGRAVITY_CONFIG_FILE.to_string(),
            ANTIGRAVITY_ACCOUNTS_FILE.to_string(),
        ],
    })
}

#[tauri::command]
pub async fn execute_opencode_sync(
    proxy_url: String,
    api_key: String,
    sync_accounts: Option<bool>,
    models: Option<Vec<String>>,
) -> Result<(), String> {
    sync_opencode_config(&proxy_url, &api_key, sync_accounts.unwrap_or(false), models)
}

#[tauri::command]
pub async fn execute_opencode_restore() -> Result<(), String> {
    restore_opencode_config()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetOpencodeConfigRequest {
    pub file_name: Option<String>,
}

#[tauri::command]
pub async fn get_opencode_config_content(request: GetOpencodeConfigRequest) -> Result<String, String> {
    read_opencode_config_content(request.file_name)
}

/// List of Antigravity model IDs that may have been added to legacy providers
const ANTIGRAVITY_MODEL_IDS: &[&str] = &[
    "claude-sonnet-4-6",
    "claude-sonnet-4-6-thinking",
    "claude-sonnet-4-5",
    "claude-sonnet-4-5-thinking",
    "claude-opus-4-5-thinking",
    "gemini-3.1-pro-high",
    "gemini-3.1-pro-low",
    "gemini-3-pro-high",
    "gemini-3-pro-low",
    "gemini-3-flash",
    "gemini-3-pro-image",
    "gemini-2.5-flash",
    "gemini-2.5-flash-lite",
    "gemini-2.5-flash-thinking",
    "gemini-2.5-pro",
];

/// Check if a base URL matches the proxy URL (supports both with and without /v1)
fn base_url_matches(config_url: &str, proxy_url: &str) -> bool {
    let normalized_config = normalize_opencode_base_url(config_url);
    let normalized_proxy = normalize_opencode_base_url(proxy_url);
    normalized_config == normalized_proxy
}

/// Clear OpenCode config by removing antigravity-manager provider and optionally cleaning up legacy entries
fn clear_opencode_config(proxy_url: Option<String>, clear_legacy: bool) -> Result<(), String> {
    let Some((config_path, _, accounts_path)) = get_config_paths() else {
        return Err("Failed to get OpenCode config directory".to_string());
    };

    // Process opencode.json
    if config_path.exists() {
        // Create backup before modifying
        create_backup(&config_path)?;

        let content = fs::read_to_string(&config_path)
            .map_err(|e| format!("Failed to read config: {}", e))?;
        
        let config: Value = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse config: {}", e))?;
        let config = apply_clear_to_config(config, proxy_url.as_deref(), clear_legacy);

        // Write updated config
        let tmp_path = config_path.with_extension("tmp");
        fs::write(&tmp_path, serde_json::to_string_pretty(&config).unwrap())
            .map_err(|e| format!("Failed to write temp file: {}", e))?;
        fs::rename(&tmp_path, &config_path)
            .map_err(|e| format!("Failed to rename config file: {}", e))?;
    }

    // Process antigravity-accounts.json
    let accounts_backup_new = accounts_path.with_file_name(format!(
        "{}{}", ANTIGRAVITY_ACCOUNTS_FILE, BACKUP_SUFFIX
    ));
    let accounts_backup_old = accounts_path.with_file_name(format!(
        "{}{}", ANTIGRAVITY_ACCOUNTS_FILE, OLD_BACKUP_SUFFIX
    ));

    if accounts_backup_new.exists() {
        // Restore from new backup
        restore_backup_to_target(&accounts_backup_new, &accounts_path, "accounts from backup")?;
    } else if accounts_backup_old.exists() {
        // Restore from old backup
        restore_backup_to_target(&accounts_backup_old, &accounts_path, "accounts from old backup")?;
    } else if accounts_path.exists() {
        // No backup found, delete the file
        fs::remove_file(&accounts_path)
            .map_err(|e| format!("Failed to remove accounts file: {}", e))?;
    }

    Ok(())
}

/// Cleanup legacy provider entries (anthropic/google) that were configured by old versions
fn cleanup_legacy_provider(provider: &mut Value, proxy_url: &str) {
    if let Some(provider_obj) = provider.as_object_mut() {
        // Remove Antigravity model IDs from models list.
        let remove_models_key = if let Some(models) = provider_obj.get_mut("models").and_then(|m| m.as_object_mut()) {
            for model_id in ANTIGRAVITY_MODEL_IDS {
                models.remove(*model_id);
            }
            models.is_empty()
        } else {
            false
        };
        if remove_models_key {
            provider_obj.remove("models");
        }

        // Check and remove options.baseURL and options.apiKey if baseURL matches proxy.
        let remove_options_key = if let Some(options) = provider_obj.get_mut("options").and_then(|o| o.as_object_mut()) {
            let should_cleanup = options
                .get("baseURL")
                .and_then(|v| v.as_str())
                .map(|base_url| base_url_matches(base_url, proxy_url))
                .unwrap_or(false);

            if should_cleanup {
                options.remove("baseURL");
                options.remove("apiKey");
            }
            options.is_empty()
        } else {
            false
        };
        if remove_options_key {
            provider_obj.remove("options");
        }
    }
}

#[tauri::command]
pub async fn execute_opencode_clear(
    proxy_url: Option<String>,
    clear_legacy: Option<bool>,
) -> Result<(), String> {
    clear_opencode_config(proxy_url, clear_legacy.unwrap_or(false))
}
