use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use crate::proxy::common::variant_mapping::{VariantTier, GEMINI_FAMILIES};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

const OPENCODE_DIR: &str = ".config/opencode";
const OPENCODE_CONFIG_FILE: &str = "opencode.json";
const OPENCODE_CONFIG_FILE_JSONC: &str = "opencode.jsonc";
const ANTIGRAVITY_CONFIG_FILE: &str = "antigravity.json";
const ANTIGRAVITY_ACCOUNTS_FILE: &str = "antigravity-accounts.json";
const BACKUP_SUFFIX: &str = ".antigravity-manager.bak";
const OLD_BACKUP_SUFFIX: &str = ".antigravity.bak";

const ANTIGRAVITY_PROVIDER_ID: &str = "antigravity-manager";
const APIKEY_FUN_PROVIDER_ID: &str = "apikey-fun";
const MAX_PROVIDER_ID_LEN: usize = 128;
const OPENAI_COMPATIBLE_NPM: &str = "@ai-sdk/openai-compatible";

static OPENCODE_CONFIG_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn acquire_opencode_config_lock() -> std::sync::MutexGuard<'static, ()> {
    OPENCODE_CONFIG_MUTEX.lock().unwrap_or_else(|poisoned| {
        tracing::warn!("OPENCODE_CONFIG_MUTEX was poisoned, recovering lock");
        poisoned.into_inner()
    })
}

fn atomically_write_config(config_path: &std::path::Path, config: &Value) -> Result<(), String> {
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory {:?}: {}", parent, e))?;
    }

    let json_str = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    let file_name = config_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("config.json");
    let tmp_path =
        config_path.with_file_name(format!("{}.tmp.{}", file_name, uuid::Uuid::new_v4()));

    // Set permissions at creation, before any credentials reach the file.
    // create_new also refuses to follow a pre-existing temporary-file symlink.
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&tmp_path)
        .map_err(|e| format!("Failed to create temp file: {}", e))?;
    let write_result = (|| -> std::io::Result<()> {
        use std::io::Write;
        #[cfg(unix)]
        if let Ok(metadata) = fs::metadata(config_path) {
            file.set_permissions(metadata.permissions())?;
        }
        file.write_all(json_str.as_bytes())?;
        file.sync_all()
    })();
    drop(file);
    if let Err(e) = write_result {
        let _ = fs::remove_file(&tmp_path);
        return Err(format!("Failed to write temp file: {}", e));
    }

    fs::rename(&tmp_path, config_path).map_err(|e| {
        let _ = fs::remove_file(&tmp_path);
        format!("Failed to rename config file: {}", e)
    })?;
    Ok(())
}

/// Variant type for model variants
#[derive(Debug, Clone, Copy)]
enum VariantType {
    /// Claude-style thinking with budget_tokens
    ClaudeThinking,
    /// Gemini 3 Pro style with thinking budgets
    Gemini3Pro,
    /// Gemini 3 Flash style with thinking budgets
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
    let mut catalog = vec![
        // Claude models
        ModelDef {
            id: "claude-sonnet-4-6",
            name: "Claude Sonnet 4.6",
            context_limit: 200_000,
            output_limit: 64_000,
            input_modalities: &["text", "image", "pdf"],
            output_modalities: &["text"],
            reasoning: true,
            variant_type: Some(VariantType::ClaudeThinking),
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
            id: "claude-sonnet-4-5",
            name: "Claude Sonnet 4.5",
            context_limit: 200_000,
            output_limit: 64_000,
            input_modalities: &["text", "image", "pdf"],
            output_modalities: &["text"],
            reasoning: true,
            variant_type: Some(VariantType::ClaudeThinking),
        },
        ModelDef {
            id: "claude-sonnet-4-5-thinking",
            name: "Claude Sonnet 4.5 Thinking",
            context_limit: 200_000,
            output_limit: 64_000,
            input_modalities: &["text", "image", "pdf"],
            output_modalities: &["text"],
            reasoning: true,
            variant_type: Some(VariantType::ClaudeThinking),
        },
        ModelDef {
            id: "claude-opus-4-5",
            name: "Claude Opus 4.5",
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
            id: "claude-opus-4-6",
            name: "Claude Opus 4.6",
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
    ];

    catalog.extend(GEMINI_FAMILIES.iter().map(|family| ModelDef {
        id: family.canonical_id,
        name: family.display_name,
        context_limit: family.context_limit,
        output_limit: family.output_limit,
        input_modalities: family.input_modalities,
        output_modalities: family.output_modalities,
        reasoning: family.reasoning,
        variant_type: match family.canonical_id {
            "gemini-3.1-pro" => Some(VariantType::Gemini3Pro),
            "gemini-3.7-flash" | "gemini-3.5-flash" => Some(VariantType::Gemini3Flash),
            _ => None,
        },
    }));

    catalog.extend([
        ModelDef {
            id: "gemini-3.1-flash-lite",
            name: "Gemini 3.1 Flash Lite",
            context_limit: 1_048_576,
            output_limit: 65_536,
            input_modalities: &["text", "image", "pdf"],
            output_modalities: &["text"],
            reasoning: true,
            variant_type: None,
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
    ]);

    catalog
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

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct CanonicalFamilyDto {
    pub canonical_id: String,
    pub display_name: String,
    pub match_ids: Vec<String>,
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
    #[serde(
        rename = "rateLimitResetTimes",
        skip_serializing_if = "Option::is_none"
    )]
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
    #[serde(
        rename = "cachedQuotaUpdatedAt",
        skip_serializing_if = "Option::is_none"
    )]
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

/// Resolve the active OpenCode config file name for the given directory.
///
/// OpenCode accepts both `opencode.json` and `opencode.jsonc`. To avoid creating a
/// parallel `opencode.json` next to a user's existing `opencode.jsonc`, we probe the
/// directory: if a `.jsonc` file already exists we keep using it, otherwise we fall
/// back to the default `opencode.json`. This keeps each user on the file they
/// already use.
///
/// Pass `Some(dir)` to probe a real directory, or `None` to get the default file
/// name without filesystem access (used by pure-function helpers).
fn resolve_active_config_file_name(dir: Option<&PathBuf>) -> &'static str {
    if let Some(dir) = dir {
        let jsonc = dir.join(OPENCODE_CONFIG_FILE_JSONC);
        if jsonc.exists() {
            return OPENCODE_CONFIG_FILE_JSONC;
        }
    }
    OPENCODE_CONFIG_FILE
}

fn get_config_paths() -> Option<(PathBuf, PathBuf, PathBuf)> {
    get_opencode_dir().map(|dir| {
        let config_file = resolve_active_config_file_name(Some(&dir));
        (
            dir.join(config_file),
            dir.join(ANTIGRAVITY_CONFIG_FILE),
            dir.join(ANTIGRAVITY_ACCOUNTS_FILE),
        )
    })
}

/// Strip JSONC comments (both `// line` and `/* block */`) so the remainder can be
/// parsed by `serde_json`, which only understands strict JSON.
///
/// This is intentionally a small, conservative scanner: it tracks whether the current
/// position is inside a string literal (respecting `\"` escapes) so that `//` or `/*`
/// appearing inside a string value is never mistaken for a comment. Trailing commas are
/// handled separately by [`strip_jsonc_trailing_commas`].
fn strip_jsonc_comments(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    let mut in_string = false;

    while i < bytes.len() {
        let c = bytes[i];

        if in_string {
            out.push(c);
            if c == b'\\' && i + 1 < bytes.len() {
                // Keep the escaped char verbatim (e.g. \", \\, \/).
                out.push(bytes[i + 1]);
                i += 2;
                continue;
            }
            if c == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }

        match c {
            b'"' => {
                in_string = true;
                out.push(b'"');
                i += 1;
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                // Line comment: skip until newline.
                i += 2;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                // Block comment: skip until closing */.
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }

    // Only ASCII syntax is removed; all UTF-8 string bytes remain intact.
    String::from_utf8(out).expect("JSONC normalization preserves UTF-8")
}

/// Remove trailing commas (a `,` followed, after optional whitespace, by a closing
/// `}` or `]`) so `serde_json` can parse JSONC that permits them. Like the comment
/// stripper, this respects string literals so a comma at the end of a string value is
/// never touched.
fn strip_jsonc_trailing_commas(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    let mut in_string = false;

    while i < bytes.len() {
        let c = bytes[i];

        if in_string {
            out.push(c);
            if c == b'\\' && i + 1 < bytes.len() {
                out.push(bytes[i + 1]);
                i += 2;
                continue;
            }
            if c == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }

        if c == b'"' {
            in_string = true;
            out.push(b'"');
            i += 1;
            continue;
        }

        // When we hit a comma outside a string, look ahead past whitespace. If the next
        // significant character closes an object/array, this is a trailing comma — drop
        // it. Otherwise keep the comma (it separates real elements).
        if c == b',' {
            let mut j = i + 1;
            let mut is_trailing = false;
            while j < bytes.len() {
                match bytes[j] {
                    b' ' | b'\t' | b'\n' | b'\r' => j += 1,
                    b'}' | b']' => {
                        is_trailing = true;
                        break;
                    }
                    _ => break,
                }
            }
            if is_trailing {
                // Skip the comma (don't push it); leave the whitespace to be pushed normally.
                i += 1;
            } else {
                out.push(b',');
                i += 1;
            }
            continue;
        }

        out.push(c);
        i += 1;
    }

    // Only ASCII syntax is removed; all UTF-8 string bytes remain intact.
    String::from_utf8(out).expect("JSONC normalization preserves UTF-8")
}

/// Read and parse an OpenCode config file, tolerating JSONC comments and trailing commas.
#[cfg(test)]
fn parse_config_file(path: &PathBuf) -> Option<Value> {
    let content = fs::read_to_string(path).ok()?;
    parse_jsonc(&content)
}

/// Parse a JSON/JSONC string: try strict JSON first, then normalize comments and
/// trailing commas and retry. Used everywhere a user opencode config is read.
fn parse_jsonc(content: &str) -> Option<Value> {
    serde_json::from_str(content)
        .or_else(|_| {
            let normalized = strip_jsonc_trailing_commas(&strip_jsonc_comments(content));
            serde_json::from_str(&normalized)
        })
        .ok()
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
        let pnpm_opencode_cmd = PathBuf::from(&local_app_data)
            .join("pnpm")
            .join("opencode.cmd");
        if pnpm_opencode_cmd.exists() {
            tracing::debug!(
                "Found opencode.cmd in LOCALAPPDATA\\pnpm: {:?}",
                pnpm_opencode_cmd
            );
            return Some(pnpm_opencode_cmd);
        }
        let pnpm_opencode_exe = PathBuf::from(&local_app_data)
            .join("pnpm")
            .join("opencode.exe");
        if pnpm_opencode_exe.exists() {
            tracing::debug!(
                "Found opencode.exe in LOCALAPPDATA\\pnpm: {:?}",
                pnpm_opencode_exe
            );
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
    let nvm_dirs = [home.join(".nvm").join("versions").join("node")];

    for nvm_dir in &nvm_dirs {
        if let Some(path) = scan_node_versions(nvm_dir) {
            return Some(path);
        }
    }

    // Scan fnm directories
    let fnm_dirs = [
        home.join(".fnm").join("node-versions"),
        home.join("Library")
            .join("Application Support")
            .join("fnm")
            .join("node-versions"),
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
        cmd.arg("--version").creation_flags(CREATE_NO_WINDOW);
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
    let output = Command::new(opencode_path).arg("--version").output();

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
    value
        .get("provider")
        .and_then(|p| p.get(provider_name))
        .and_then(|prov| prov.get("options"))
}

pub fn get_sync_status(proxy_url: &str) -> (bool, bool, Option<String>) {
    let _lock = acquire_opencode_config_lock();

    let Some((config_path, _, _)) = get_config_paths() else {
        return (false, false, None);
    };

    let mut is_synced = true;
    let mut has_backup = false;
    let mut current_base_url = None;

    // Backups may have been created against either opencode.json or opencode.jsonc.
    // Check both the active file name's backup and the canonical json backup so a
    // previously-synced state is still detected after switching file types.
    let active_file_name = config_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(OPENCODE_CONFIG_FILE);
    let backup_candidates = [
        format!("{}{}", active_file_name, BACKUP_SUFFIX),
        format!("{}{}", active_file_name, OLD_BACKUP_SUFFIX),
        format!("{}{}", OPENCODE_CONFIG_FILE, BACKUP_SUFFIX),
        format!("{}{}", OPENCODE_CONFIG_FILE, OLD_BACKUP_SUFFIX),
    ];
    for name in &backup_candidates {
        if config_path.with_file_name(name).exists() {
            has_backup = true;
            break;
        }
    }

    if !config_path.exists() {
        return (false, has_backup, None);
    }

    let content = match fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(_) => return (false, has_backup, None),
    };

    let json: Value = parse_jsonc(&content).unwrap_or_default();

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

    fs::copy(path, &backup_path).map_err(|e| format!("Failed to create backup: {}", e))?;

    Ok(())
}

fn restore_backup_to_target(
    backup_path: &PathBuf,
    target_path: &PathBuf,
    label: &str,
) -> Result<(), String> {
    if target_path.exists() {
        fs::remove_file(target_path)
            .map_err(|e| format!("Failed to remove existing {}: {}", label, e))?;
    }

    fs::rename(backup_path, target_path).map_err(|e| format!("Failed to restore {}: {}", label, e))
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
            "budget_tokens": budget,
            "budgetTokens": budget
        }
    })
}

/// Build Gemini 3 effort-based variant for the @ai-sdk/anthropic SDK.
///
/// The provider serializes `{ "effort": "<tier>" }` as `output_config.effort`
/// on the outgoing Anthropic request. No thinking-budget fields are used.
fn build_gemini3_effort_variant(tier: VariantTier) -> Value {
    let effort = match tier {
        VariantTier::Low => "low",
        VariantTier::Medium => "medium",
        VariantTier::High => "high",
    };
    serde_json::json!({ "effort": effort })
}

/// Build Gemini 2.5 thinking variant with thinkingConfig and thinking
fn build_gemini25_thinking_variant(budget: u32) -> Value {
    serde_json::json!({
        "thinkingConfig": {
            "thinkingBudget": budget
        },
        "thinking": {
            "type": "enabled",
            "budget_tokens": budget,
            "budgetTokens": budget
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
            variants.insert(
                "low".to_string(),
                build_gemini3_effort_variant(VariantTier::Low),
            );
            variants.insert(
                "medium".to_string(),
                serde_json::json!({ "disabled": true }),
            );
            variants.insert(
                "high".to_string(),
                build_gemini3_effort_variant(VariantTier::High),
            );
            variants.insert("max".to_string(), serde_json::json!({ "disabled": true }));
            Some(Value::Object(variants))
        }
        Some(VariantType::Gemini3Flash) => {
            let mut variants = serde_json::Map::new();
            variants.insert(
                "low".to_string(),
                build_gemini3_effort_variant(VariantTier::Low),
            );
            variants.insert(
                "medium".to_string(),
                build_gemini3_effort_variant(VariantTier::Medium),
            );
            variants.insert(
                "high".to_string(),
                build_gemini3_effort_variant(VariantTier::High),
            );
            variants.insert("max".to_string(), serde_json::json!({ "disabled": true }));
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

    model_obj.insert(
        "name".to_string(),
        Value::String(model_def.name.to_string()),
    );

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

/// A model to sync, optionally carrying the display name from the frontend.
/// When `name` is provided (the common case from the OpenCode sync modal), it is used
/// verbatim so the config shows the same recognizable name the user saw in the UI.
/// When absent (e.g. a plain id list), a name is derived from the id.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInput {
    pub id: String,
    pub name: Option<String>,
}

/// Sensible per-series defaults derived from the model id prefix. Real model metadata
/// lives in the catalog; this is only used for ids the catalog doesn't know (e.g. a
/// newly released model or an account-specific variant), so the entry still gets useful
/// limit/modalities instead of a bare `{ "name": ... }`. Returns None for unknown
/// families, in which case only the name is written.
fn series_defaults_for(model_id: &str) -> Option<Value> {
    let id = model_id.to_lowercase();
    // Gemini 3.x / 3.5 / 3.1 family: 1M context, 64k output, multimodal in, text out.
    if id.starts_with("gemini-3") || id.starts_with("gemini-3.1") || id.starts_with("gemini-3.5") {
        return Some(serde_json::json!({
            "limit": { "context": 1_048_576, "output": 65_536 },
            "modalities": { "input": ["text", "image", "pdf"], "output": ["text"] },
        }));
    }
    // Gemini 2.5 family: same shape.
    if id.starts_with("gemini-2.5") || id.starts_with("gemini-2.0") {
        return Some(serde_json::json!({
            "limit": { "context": 1_048_576, "output": 65_536 },
            "modalities": { "input": ["text", "image", "pdf"], "output": ["text"] },
        }));
    }
    // Claude family: 200k context, 64k output.
    if id.starts_with("claude") {
        return Some(serde_json::json!({
            "limit": { "context": 200_000, "output": 64_000 },
            "modalities": { "input": ["text", "image", "pdf"], "output": ["text"] },
        }));
    }
    None
}

/// Build a minimal but valid model entry for a model id that is not in the catalog.
///
/// OpenCode's schema only requires a `name` for a model entry, so a model the user
/// explicitly selected — even one we have no metadata for — should still be written
/// rather than silently dropped. If a display name was passed from the frontend we use
/// it verbatim (so the config matches what the user saw); otherwise we derive one.
/// When we can infer the model family from the id we also fill in limit/modalities.
fn build_fallback_model_json(model_id: &str, display_name: Option<&str>) -> Value {
    // Prefer the caller-provided display name; fall back to a cleaned-up id.
    let name = display_name
        .filter(|n| !n.trim().is_empty())
        .map(|n| n.to_string())
        .unwrap_or_else(|| humanize_model_id(model_id));

    let mut entry = serde_json::Map::new();
    entry.insert("name".to_string(), Value::String(name));
    if let Some(defaults) = series_defaults_for(model_id) {
        if let Some(obj) = defaults.as_object() {
            for (k, v) in obj.iter() {
                entry.insert(k.clone(), v.clone());
            }
        }
    }
    Value::Object(entry)
}

/// Bare model id without a `vendor/` prefix (`anthropic/claude-sonnet-4-6` -> `claude-sonnet-4-6`).
fn strip_model_vendor_prefix(model_id: &str) -> &str {
    model_id.rsplit('/').next().unwrap_or(model_id)
}

fn is_short_version_token(s: &str) -> bool {
    let len = s.len();
    (1..=2).contains(&len) && s.chars().all(|c| c.is_ascii_digit())
}

/// Join adjacent 1–2 digit version tokens with dots (`4-6` -> `4.6`).
fn merge_hyphenated_version_tokens(model_id: &str) -> String {
    let parts: Vec<&str> = model_id.split('-').filter(|s| !s.is_empty()).collect();
    let mut out: Vec<String> = Vec::with_capacity(parts.len());
    let mut i = 0;
    while i < parts.len() {
        if i + 1 < parts.len()
            && is_short_version_token(parts[i])
            && is_short_version_token(parts[i + 1])
        {
            out.push(format!("{}.{}", parts[i], parts[i + 1]));
            i += 2;
        } else {
            out.push(parts[i].to_string());
            i += 1;
        }
    }
    out.join("-")
}

/// IDs to try against the catalog: original, vendor-stripped, dotted/dashed version variants.
fn catalog_lookup_ids(model_id: &str) -> Vec<String> {
    let bare = strip_model_vendor_prefix(model_id.trim());
    let mut ids = Vec::new();
    let mut push = |id: String| {
        if !id.is_empty() && !ids.iter().any(|existing| existing == &id) {
            ids.push(id);
        }
    };
    push(model_id.trim().to_string());
    push(bare.to_string());
    push(bare.replace('.', "-"));
    push(merge_hyphenated_version_tokens(bare));
    ids
}

fn lookup_catalog_model<'a>(
    catalog: &HashMap<&str, &'a ModelDef>,
    model_id: &str,
) -> Option<&'a ModelDef> {
    for candidate in catalog_lookup_ids(model_id) {
        if let Some(model) = catalog.get(candidate.as_str()) {
            return Some(*model);
        }
    }
    None
}

/// Derive a readable name from a model id, preserving version dots
/// (e.g. "gemini-3.5-flash-low" -> "Gemini 3.5 Flash Low",
/// "claude-sonnet-4-6" -> "Claude Sonnet 4.6").
fn humanize_model_id(model_id: &str) -> String {
    merge_hyphenated_version_tokens(strip_model_vendor_prefix(model_id))
        .split('-')
        .filter(|s| !s.is_empty())
        .map(|s| {
            let mut chars = s.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Return a Gemini canonical catalog id for a canonical id or one of its aliases.
fn canonical_gemini_model_id(model_id: &str) -> Option<&'static str> {
    GEMINI_FAMILIES.iter().find_map(|family| {
        (family.canonical_id == model_id
            || family
                .aliases
                .iter()
                .any(|(alias_id, _)| *alias_id == model_id))
        .then_some(family.canonical_id)
    })
}

/// Normalize Gemini aliases and preserve the first frontend entry for each model id.
fn normalize_model_inputs(model_inputs: &[ModelInput]) -> Vec<ModelInput> {
    let mut seen_model_ids = HashSet::new();

    model_inputs
        .iter()
        .filter_map(|input| {
            let model_id = canonical_gemini_model_id(&input.id).unwrap_or(&input.id);
            if !seen_model_ids.insert(model_id.to_string()) {
                return None;
            }

            let mut normalized = input.clone();
            normalized.id = model_id.to_string();
            Some(normalized)
        })
        .collect()
}

/// Migrate known Gemini alias keys without touching non-Gemini user models.
fn migrate_gemini_alias_models(provider: &mut Value) {
    let Some(models) = provider.get_mut("models").and_then(Value::as_object_mut) else {
        return;
    };

    for family in GEMINI_FAMILIES {
        for (alias_id, _) in family.aliases {
            let Some(alias_value) = models.remove(*alias_id) else {
                continue;
            };

            let merged = match models.remove(family.canonical_id) {
                Some(canonical_value) => {
                    match (canonical_value.as_object(), alias_value.as_object()) {
                        (Some(canonical), Some(alias)) => {
                            let mut merged = canonical.clone();
                            for (field, value) in alias {
                                match merged.get(field) {
                                    Some(canonical_value) if canonical_value != value => {
                                        tracing::warn!(
                                            canonical_model = family.canonical_id,
                                            alias_model = alias_id,
                                            field = field.as_str(),
                                            "OpenCode sync model field conflict; canonical value retained"
                                        );
                                    }
                                    Some(_) => {}
                                    None => {
                                        merged.insert(field.clone(), value.clone());
                                    }
                                }
                            }
                            Value::Object(merged)
                        }
                        _ => {
                            tracing::warn!(
                                canonical_model = family.canonical_id,
                                alias_model = alias_id,
                                "OpenCode sync could not merge a non-object alias model; canonical value retained"
                            );
                            canonical_value
                        }
                    }
                }
                None => alias_value,
            };
            models.insert(family.canonical_id.to_string(), merged);
        }
    }
}

/// Merge catalog models into provider.models without deleting user models.
/// Each entry carries an optional display name from the frontend so fallback entries
/// for non-catalog models get a recognizable name.
fn merge_catalog_models(provider: &mut Value, model_inputs: Option<&[ModelInput]>) {
    if provider.get("models").is_none() {
        provider["models"] = serde_json::json!({});
    }

    let catalog = build_model_catalog();
    let catalog_map: HashMap<&str, &ModelDef> = catalog.iter().map(|m| (m.id, m)).collect();

    if let Some(models) = provider.get_mut("models").and_then(|m| m.as_object_mut()) {
        // When no specific models are requested, sync the whole catalog.
        let catalog_ids: Vec<&str> = catalog_map.keys().copied().collect();

        let normalized_inputs = normalize_model_inputs(model_inputs.unwrap_or(&[]));
        for input in &normalized_inputs {
            let model_id = input.id.as_str();
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
            } else {
                // Fallback: the id isn't in our catalog (e.g. a dynamically discovered
                // model from the user's account quota, or a new model not yet listed).
                // Previously these were silently dropped, which caused selected models to
                // never appear in the config. OpenCode only requires a `name`, so write a
                // minimal entry using the frontend-provided display name when available.
                // Preserve any user-defined object the user may already have.
                if !models.contains_key(model_id) {
                    models.insert(
                        model_id.to_string(),
                        build_fallback_model_json(model_id, input.name.as_deref()),
                    );
                }
            }
        }

        // No inputs at all => sync the entire catalog (None means "all").
        if model_inputs.is_none() {
            for model_id in &catalog_ids {
                if let Some(model_def) = catalog_map.get(model_id) {
                    if !models.contains_key(*model_id) {
                        models.insert((*model_id).to_string(), build_model_json(model_def));
                    }
                }
            }
        }
    }
}

pub fn sync_opencode_config(
    proxy_url: &str,
    api_key: &str,
    sync_accounts: bool,
    models_to_sync: Option<Vec<ModelInput>>,
) -> Result<(), String> {
    let _lock = acquire_opencode_config_lock();

    let Some((config_path, _ag_config_path, ag_accounts_path)) = get_config_paths() else {
        return Err("Failed to get OpenCode config directory".to_string());
    };

    let mut config = match fs::read_to_string(&config_path) {
        Ok(content) => parse_jsonc(&content)
            .filter(Value::is_object)
            .ok_or_else(|| {
                "OpenCode config must be a valid JSON/JSONC object; file left unchanged".to_string()
            })?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
        Err(error) => return Err(format!("Failed to read OpenCode config: {}", error)),
    };

    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create directory: {}", e))?;
    }

    create_backup(&config_path)?;

    config = apply_sync_to_config(config, proxy_url, api_key, models_to_sync.as_deref());

    atomically_write_config(&config_path, &config)?;

    if sync_accounts {
        sync_accounts_file(&ag_accounts_path)?;
    }

    Ok(())
}

/// Provider ids become JSON keys in opencode.json and are accepted over the admin
/// HTTP API, so restrict them to a safe charset.
fn validate_provider_id(provider_id: &str) -> Result<(), String> {
    if provider_id.is_empty() {
        return Err("OpenCode provider id is required".to_string());
    }
    if provider_id.len() > MAX_PROVIDER_ID_LEN {
        return Err(format!(
            "Invalid OpenCode provider id: must be at most {} characters",
            MAX_PROVIDER_ID_LEN
        ));
    }
    if !provider_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(format!(
            "Invalid OpenCode provider id '{}': only letters, digits, '-' and '_' are allowed",
            provider_id
        ));
    }
    Ok(())
}

/// True when the error came from input validation rather than I/O, so callers can
/// answer 4xx instead of 5xx. Matches on explicit markers, never on generic
/// OS strings such as "Invalid argument".
pub fn is_provider_validation_error(message: &str) -> bool {
    const MARKERS: [&str; 6] = [
        "provider id is required",
        "Invalid OpenCode provider id",
        "is reserved",
        "already belongs to another API key",
        "OpenCode API key and base URL are required",
        "is not managed by Antigravity-Manager",
    ];
    MARKERS.iter().any(|marker| message.contains(marker))
}

pub fn sync_opencode_openai_provider(
    provider_id: &str,
    provider_name: &str,
    proxy_url: &str,
    api_key: &str,
    models_to_sync: Option<Vec<ModelInput>>,
) -> Result<(), String> {
    let provider_id = provider_id.trim();
    validate_provider_id(provider_id)?;

    if provider_id.eq_ignore_ascii_case(ANTIGRAVITY_PROVIDER_ID) {
        return Err(format!(
            "Provider id '{}' is reserved for Antigravity-Manager internal configuration",
            ANTIGRAVITY_PROVIDER_ID
        ));
    }

    let Some((config_path, _, _)) = get_config_paths() else {
        return Err("Failed to get OpenCode config directory".to_string());
    };

    sync_openai_provider_to_path(
        &config_path,
        provider_id,
        provider_name,
        proxy_url,
        api_key,
        models_to_sync.as_deref(),
    )
}

fn sync_openai_provider_to_path(
    config_path: &PathBuf,
    provider_id: &str,
    provider_name: &str,
    proxy_url: &str,
    api_key: &str,
    models_to_sync: Option<&[ModelInput]>,
) -> Result<(), String> {
    if api_key.trim().is_empty() || proxy_url.trim().is_empty() {
        return Err("OpenCode API key and base URL are required".to_string());
    }
    let _lock = acquire_opencode_config_lock();

    // A read/parse failure must never turn a user's existing config into {}.
    let mut config = match fs::read_to_string(config_path) {
        Ok(content) => parse_jsonc(&content)
            .filter(Value::is_object)
            .ok_or_else(|| {
                "OpenCode config must be a valid JSON/JSONC object; file left unchanged".to_string()
            })?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
        Err(error) => return Err(format!("Failed to read OpenCode config: {}", error)),
    };

    if provider_id.starts_with("apikey-fun-") {
        if let Some(existing) = config.get("provider").and_then(|p| p.get(provider_id)) {
            let existing_key = existing
                .get("options")
                .and_then(|options| options.get("apiKey"))
                .and_then(Value::as_str);
            if existing_key.map(str::trim) != Some(api_key.trim()) {
                return Err(format!(
                    "OpenCode provider '{}' already belongs to another API key",
                    provider_id
                ));
            }
        }
    }

    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create directory: {}", e))?;
    }

    create_backup(config_path)?;

    config = apply_openai_compatible_provider_sync(
        config,
        provider_id,
        provider_name,
        proxy_url,
        api_key,
        models_to_sync,
    );

    atomically_write_config(config_path, &config)?;

    Ok(())
}

/// Precondition: the caller must already hold `OPENCODE_CONFIG_MUTEX`
/// (see `acquire_opencode_config_lock`). This function does not lock itself.
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
            if let Some(existing_accounts) =
                existing_json.get("accounts").and_then(|a| a.as_array())
            {
                for acc in existing_accounts {
                    if let Ok(plugin_acc) = serde_json::from_value::<PluginAccount>(acc.clone()) {
                        // Index by refresh_token (primary key for matching)
                        existing_accounts_by_refresh_token
                            .insert(plugin_acc.refresh_token.clone(), plugin_acc.clone());
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
            if let Some(family_indices) = existing_json
                .get("activeIndexByFamily")
                .and_then(|v| v.as_object())
            {
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

    let value = serde_json::to_value(&new_data)
        .map_err(|e| format!("Failed to serialize accounts: {}", e))?;
    atomically_write_config(accounts_path, &value)?;

    Ok(())
}

pub fn restore_opencode_config() -> Result<(), String> {
    let _lock = acquire_opencode_config_lock();

    let Some((config_path, _, accounts_path)) = get_config_paths() else {
        return Err("Failed to get OpenCode config directory".to_string());
    };

    let mut restored = false;

    // Backups are named after the config file they protected. A user may have been
    // using opencode.json or opencode.jsonc, and the active path may now differ from
    // whichever backup exists. Look for backups under both file names, preferring the
    // active one, and restore the backup to its original (backup-name minus suffix)
    // target path so we don't resurrect a stale parallel file.
    let dir = config_path.parent();
    let active_file_name = config_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(OPENCODE_CONFIG_FILE);
    let config_candidates: [(&str, &str); 4] = [
        (active_file_name, BACKUP_SUFFIX),
        (active_file_name, OLD_BACKUP_SUFFIX),
        (OPENCODE_CONFIG_FILE, BACKUP_SUFFIX),
        (OPENCODE_CONFIG_FILE, OLD_BACKUP_SUFFIX),
    ];
    for (file_name, suffix) in &config_candidates {
        let backup_path = config_path.with_file_name(format!("{}{}", file_name, suffix));
        if backup_path.exists() {
            let target = dir
                .map(|d| d.join(file_name))
                .unwrap_or_else(|| config_path.clone());
            restore_backup_to_target(&backup_path, &target, "config")?;
            restored = true;
            break;
        }
    }

    // Try new backup suffix first, fall back to old suffix for backward compatibility
    let accounts_backup_new =
        accounts_path.with_file_name(format!("{}{}", ANTIGRAVITY_ACCOUNTS_FILE, BACKUP_SUFFIX));
    let accounts_backup_old = accounts_path.with_file_name(format!(
        "{}{}",
        ANTIGRAVITY_ACCOUNTS_FILE, OLD_BACKUP_SUFFIX
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
    models_to_sync: Option<&[ModelInput]>,
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
            migrate_gemini_alias_models(ag_provider);
            merge_catalog_models(ag_provider, models_to_sync);
        }
    }

    config
}

/// Replace the provider's model list with the given inputs. The list mirrors the
/// models actually exposed by the upstream key, so models absent from the input are
/// dropped (unlike the Antigravity sync which merges). Known catalog ids still get
/// full catalog metadata, and user-defined fields on surviving models are preserved.
fn replace_provider_models(provider: &mut Value, model_inputs: Option<&[ModelInput]>) {
    if provider.get("models").is_none() {
        provider["models"] = serde_json::json!({});
    }

    // An absent or empty list means "keep whatever is there" — e.g. the user synced
    // before querying models, or called the HTTP API with no models field.
    let Some(inputs) = model_inputs else {
        return;
    };
    if inputs.is_empty() {
        return;
    }

    let catalog = build_model_catalog();
    let catalog_map: HashMap<&str, &ModelDef> = catalog.iter().map(|m| (m.id, m)).collect();
    let existing_models: serde_json::Map<String, Value> = provider
        .get("models")
        .and_then(|m| m.as_object())
        .cloned()
        .unwrap_or_default();

    let mut models = serde_json::Map::new();
    for input in inputs {
        let model_id = input.id.trim();
        if model_id.is_empty() {
            continue;
        }
        let entry = match lookup_catalog_model(&catalog_map, model_id) {
            Some(model_def) => {
                let catalog_model = build_model_json(model_def);
                match existing_models.get(model_id) {
                    Some(existing) if existing.is_object() => {
                        let mut merged = existing.as_object().unwrap().clone();
                        if let Some(catalog_obj) = catalog_model.as_object() {
                            for (key, value) in catalog_obj {
                                merged.insert(key.clone(), value.clone());
                            }
                        }
                        Value::Object(merged)
                    }
                    _ => catalog_model,
                }
            }
            None => {
                // Unknown upstream models often have manually configured limits,
                // tool support, or options that cannot be recovered from the catalog.
                let mut entry = existing_models
                    .get(model_id)
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                if let Value::Object(defaults) =
                    build_fallback_model_json(model_id, input.name.as_deref())
                {
                    for (key, value) in defaults {
                        entry.entry(key).or_insert(value);
                    }
                }
                Value::Object(entry)
            }
        };
        models.insert(model_id.to_string(), entry);
    }
    provider["models"] = Value::Object(models);
}

fn apply_openai_compatible_provider_sync(
    mut config: Value,
    provider_id: &str,
    provider_name: &str,
    proxy_url: &str,
    api_key: &str,
    models_to_sync: Option<&[ModelInput]>,
) -> Value {
    if !config.is_object() {
        config = serde_json::json!({});
    }

    if config.get("$schema").is_none() {
        config["$schema"] = Value::String("https://opencode.ai/config.json".to_string());
    }

    let normalized_url = normalize_opencode_base_url(proxy_url);
    let display_name = if provider_name.trim().is_empty() {
        "APIKEY.FUN"
    } else {
        provider_name.trim()
    };

    ensure_object(&mut config, "provider");

    if let Some(provider) = config.get_mut("provider").and_then(|p| p.as_object_mut()) {
        ensure_provider_object(provider, provider_id);
        if let Some(target) = provider.get_mut(provider_id) {
            ensure_provider_string_field(target, "npm", OPENAI_COMPATIBLE_NPM);
            ensure_provider_string_field(target, "name", display_name);
            ensure_object(target, "options");
            merge_provider_options(target, &normalized_url, api_key);
            replace_provider_models(target, models_to_sync);
        }
    }

    config
}

/// Pure function: Apply clear logic to config JSON
/// Returns the modified config Value
fn apply_clear_to_config(mut config: Value, proxy_url: Option<&str>, clear_legacy: bool) -> Value {
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
    use crate::proxy::common::variant_mapping::resolve_real_model;

    /// Helper: build a ModelInput with just an id (no display name).
    fn minput(id: &str) -> ModelInput {
        ModelInput {
            id: id.to_string(),
            name: None,
        }
    }

    /// Helper: build a ModelInput carrying a display name.
    fn minput_named(id: &str, name: &str) -> ModelInput {
        ModelInput {
            id: id.to_string(),
            name: Some(name.to_string()),
        }
    }

    #[test]
    fn merge_catalog_models_normalizes_aliases_to_canonical() {
        let mut provider = serde_json::json!({ "models": {} });
        let inputs = [minput("gemini-3.1-pro-high"), minput("gemini-3.1-pro-low")];

        merge_catalog_models(&mut provider, Some(&inputs));

        let models = provider["models"].as_object().expect("models object");
        assert_eq!(models.len(), 1);
        assert!(models.contains_key("gemini-3.1-pro"));
        assert!(!models.contains_key("gemini-3.1-pro-high"));
        assert!(!models.contains_key("gemini-3.1-pro-low"));
    }

    #[test]
    fn merge_catalog_models_keeps_one_pro_and_one_flash_for_legacy_inputs() {
        let mut provider = serde_json::json!({ "models": {} });
        let inputs = [
            minput("gemini-3.1-pro-high"),
            minput("gemini-3.1-pro-low"),
            minput("gemini-3-flash"),
        ];

        merge_catalog_models(&mut provider, Some(&inputs));

        let models = provider["models"].as_object().expect("models object");
        assert_eq!(models.len(), 2);
        assert!(models.contains_key("gemini-3.1-pro"));
        assert!(models.contains_key("gemini-3.5-flash"));
    }

    #[test]
    fn apply_sync_migrates_old_alias_keys_to_canonical() {
        let config = serde_json::json!({
            "provider": {
                ANTIGRAVITY_PROVIDER_ID: {
                    "models": {
                        "gemini-3.1-pro-high": { "from_high": true },
                        "gemini-3.1-pro-low": { "from_low": true },
                        "custom-model": { "preserved": true }
                    }
                }
            }
        });

        let updated = apply_sync_to_config(config, "http://localhost:8045", "key", Some(&[]));
        let models = updated["provider"][ANTIGRAVITY_PROVIDER_ID]["models"]
            .as_object()
            .expect("models object");
        let canonical = models
            .get("gemini-3.1-pro")
            .and_then(Value::as_object)
            .expect("canonical model");

        assert_eq!(canonical.get("from_high"), Some(&Value::Bool(true)));
        assert_eq!(canonical.get("from_low"), Some(&Value::Bool(true)));
        assert!(!models.contains_key("gemini-3.1-pro-high"));
        assert!(!models.contains_key("gemini-3.1-pro-low"));
        assert_eq!(
            models.get("custom-model"),
            Some(&serde_json::json!({ "preserved": true }))
        );
    }

    #[test]
    fn apply_sync_keeps_non_conflicting_user_fields() {
        let config = serde_json::json!({
            "provider": {
                ANTIGRAVITY_PROVIDER_ID: {
                    "models": {
                        "gemini-3.1-pro": { "canonical_only": "keep" },
                        "gemini-3.1-pro-high": { "alias_only": "also keep" }
                    }
                }
            }
        });

        let updated = apply_sync_to_config(config, "http://localhost:8045", "key", Some(&[]));
        let canonical = updated["provider"][ANTIGRAVITY_PROVIDER_ID]["models"]["gemini-3.1-pro"]
            .as_object()
            .expect("canonical model");

        assert_eq!(
            canonical.get("canonical_only"),
            Some(&Value::String("keep".to_string()))
        );
        assert_eq!(
            canonical.get("alias_only"),
            Some(&Value::String("also keep".to_string()))
        );
    }

    #[test]
    fn apply_sync_warns_on_conflicts_and_canonical_wins() {
        use std::sync::{Arc, Mutex};
        use tracing::{Event, Level, Subscriber};
        use tracing_subscriber::{
            layer::{Context, SubscriberExt},
            registry::LookupSpan,
            Layer,
        };

        #[derive(Clone)]
        struct WarnCapture(Arc<Mutex<usize>>);

        impl<S> Layer<S> for WarnCapture
        where
            S: Subscriber + for<'lookup> LookupSpan<'lookup>,
        {
            fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
                if *event.metadata().level() == Level::WARN {
                    *self.0.lock().expect("warning counter lock") += 1;
                }
            }
        }

        let config = serde_json::json!({
            "provider": {
                ANTIGRAVITY_PROVIDER_ID: {
                    "models": {
                        "gemini-3.1-pro": { "custom": "canonical" },
                        "gemini-3.1-pro-high": { "custom": "alias" }
                    }
                }
            }
        });
        let warnings = Arc::new(Mutex::new(0));
        let subscriber = tracing_subscriber::registry().with(WarnCapture(warnings.clone()));
        let updated = tracing::subscriber::with_default(subscriber, || {
            apply_sync_to_config(config, "http://localhost:8045", "key", Some(&[]))
        });

        assert_eq!(
            updated["provider"][ANTIGRAVITY_PROVIDER_ID]["models"]["gemini-3.1-pro"]["custom"],
            "canonical"
        );
        assert_eq!(*warnings.lock().expect("warning counter lock"), 1);
    }

    fn preserved_catalog_json_snapshot(models: &[ModelDef]) -> String {
        let entries = models
            .iter()
            .filter(|model| {
                model.id.starts_with("claude-")
                    || model.id == "gemini-3-pro-image"
                    || model.id.starts_with("gemini-2.5-")
            })
            .map(|model| serde_json::json!({ "id": model.id, "model": build_model_json(model) }))
            .collect::<Vec<_>>();

        serde_json::to_string(&entries).unwrap()
    }

    #[test]
    fn catalog_preserves_non_gemini3_variant_json_snapshot() {
        let expected = vec![
            ModelDef {
                id: "claude-sonnet-4-6",
                name: "Claude Sonnet 4.6",
                context_limit: 200_000,
                output_limit: 64_000,
                input_modalities: &["text", "image", "pdf"],
                output_modalities: &["text"],
                reasoning: true,
                variant_type: Some(VariantType::ClaudeThinking),
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
                id: "claude-sonnet-4-5",
                name: "Claude Sonnet 4.5",
                context_limit: 200_000,
                output_limit: 64_000,
                input_modalities: &["text", "image", "pdf"],
                output_modalities: &["text"],
                reasoning: true,
                variant_type: Some(VariantType::ClaudeThinking),
            },
            ModelDef {
                id: "claude-sonnet-4-5-thinking",
                name: "Claude Sonnet 4.5 Thinking",
                context_limit: 200_000,
                output_limit: 64_000,
                input_modalities: &["text", "image", "pdf"],
                output_modalities: &["text"],
                reasoning: true,
                variant_type: Some(VariantType::ClaudeThinking),
            },
            ModelDef {
                id: "claude-opus-4-5",
                name: "Claude Opus 4.5",
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
                id: "claude-opus-4-6",
                name: "Claude Opus 4.6",
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
        ];

        assert_eq!(
            preserved_catalog_json_snapshot(&build_model_catalog()),
            preserved_catalog_json_snapshot(&expected)
        );
    }

    #[test]
    fn catalog_uses_canonical_gemini_family_entries() {
        let catalog = build_model_catalog();
        let variant_ids = catalog
            .iter()
            .filter(|model| {
                matches!(
                    model.variant_type,
                    Some(VariantType::Gemini3Pro) | Some(VariantType::Gemini3Flash)
                )
            })
            .map(|model| model.id)
            .collect::<std::collections::BTreeSet<_>>();
        let canonical_ids = GEMINI_FAMILIES
            .iter()
            .map(|family| family.canonical_id)
            .collect::<std::collections::BTreeSet<_>>();

        assert_eq!(variant_ids, canonical_ids);

        for family in GEMINI_FAMILIES {
            let model = catalog
                .iter()
                .find(|model| model.id == family.canonical_id)
                .unwrap();
            assert_eq!(model.name, family.display_name);
            assert_eq!(model.context_limit, family.context_limit);
            assert_eq!(model.output_limit, family.output_limit);
            assert_eq!(model.input_modalities, family.input_modalities);
            assert_eq!(model.output_modalities, family.output_modalities);
            assert_eq!(model.reasoning, family.reasoning);

            match family.canonical_id {
                "gemini-3.1-pro" => {
                    assert!(matches!(model.variant_type, Some(VariantType::Gemini3Pro)));
                }
                "gemini-3.7-flash" | "gemini-3.5-flash" => {
                    assert!(matches!(
                        model.variant_type,
                        Some(VariantType::Gemini3Flash)
                    ));
                }
                _ => panic!("unexpected Gemini family: {}", family.canonical_id),
            }
        }
    }

    #[test]
    fn catalog_marks_gemini_31_flash_lite_as_non_variant() {
        let catalog = build_model_catalog();
        let flash_lite = catalog
            .iter()
            .find(|model| model.id == "gemini-3.1-flash-lite")
            .unwrap();

        assert!(matches!(flash_lite.variant_type, None));
    }

    /// Map an Anthropic SDK effort string to the internal VariantTier.
    fn effort_to_tier(effort: &str) -> VariantTier {
        match effort {
            "low" => VariantTier::Low,
            "medium" => VariantTier::Medium,
            "high" => VariantTier::High,
            _ => panic!("unrecognized effort variant: {effort}"),
        }
    }

    #[test]
    fn build_variants_pro_resolve_to_real_ids() {
        let variants = build_variants_object(Some(VariantType::Gemini3Pro))
            .expect("Gemini 3.1 Pro variants must be configured");

        for (name, expected_id) in [("low", "gemini-3.1-pro-low"), ("high", "gemini-pro-agent")] {
            let variant = &variants[name];
            let effort = variant["effort"]
                .as_str()
                .expect("Gemini 3 variant must expose effort string");

            let tier = effort_to_tier(effort);
            assert_eq!(
                resolve_real_model("gemini-3.1-pro", tier)
                    .expect("Gemini 3.1 Pro tier must resolve")
                    .id,
                expected_id
            );
        }

        assert_eq!(
            variants
                .as_object()
                .expect("variants must be an object")
                .len(),
            4
        );
        assert_eq!(variants["medium"]["disabled"], true);
        assert_eq!(variants["max"]["disabled"], true);

        // Verify the JSON shape contains only `effort` — no budget fields
        let low = &variants["low"];
        assert_eq!(low.as_object().unwrap().len(), 1);
        assert!(low.get("effort").is_some());
        assert!(low.get("thinking").is_none());
    }

    #[test]
    fn build_variants_flash_resolve_to_real_ids() {
        let variants = build_variants_object(Some(VariantType::Gemini3Flash))
            .expect("Gemini 3.5 Flash variants must be configured");

        for (name, expected_id) in [
            ("low", "gemini-3.5-flash-extra-low"),
            ("medium", "gemini-3.5-flash-low"),
            ("high", "gemini-3-flash-agent"),
        ] {
            let variant = &variants[name];
            let effort = variant["effort"]
                .as_str()
                .expect("Gemini 3 variant must expose effort string");

            let tier = effort_to_tier(effort);
            assert_eq!(
                resolve_real_model("gemini-3.5-flash", tier)
                    .expect("Gemini 3.5 Flash tier must resolve")
                    .id,
                expected_id
            );
        }

        assert_eq!(
            variants
                .as_object()
                .expect("variants must be an object")
                .len(),
            4
        );
        assert_eq!(variants["max"]["disabled"], true);

        // Verify the JSON shape contains only `effort` — no budget fields
        let low = &variants["low"];
        assert_eq!(low.as_object().unwrap().len(), 1);
        assert!(low.get("effort").is_some());
        assert!(low.get("thinking").is_none());
    }

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
        assert_eq!(
            normalize_opencode_base_url("http://localhost:3000"),
            "http://localhost:3000/v1"
        );
        assert_eq!(
            normalize_opencode_base_url("http://localhost:3000/"),
            "http://localhost:3000/v1"
        );
    }

    #[test]
    fn test_normalize_opencode_base_url_with_v1() {
        assert_eq!(
            normalize_opencode_base_url("http://localhost:3000/v1"),
            "http://localhost:3000/v1"
        );
        assert_eq!(
            normalize_opencode_base_url("http://localhost:3000/v1/"),
            "http://localhost:3000/v1"
        );
    }

    #[test]
    fn test_normalize_opencode_base_url_with_whitespace() {
        assert_eq!(
            normalize_opencode_base_url("  http://localhost:3000  "),
            "http://localhost:3000/v1"
        );
        assert_eq!(
            normalize_opencode_base_url("  http://localhost:3000/v1  "),
            "http://localhost:3000/v1"
        );
    }

    #[test]
    fn test_normalize_opencode_base_url_no_double_v1() {
        // Ensure we don't create double /v1/v1
        assert_eq!(
            normalize_opencode_base_url("http://localhost:3000/v1"),
            "http://localhost:3000/v1"
        );
        assert_eq!(
            normalize_opencode_base_url("http://localhost:3000/v1/"),
            "http://localhost:3000/v1"
        );
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
        assert!(
            provider.get("google").is_some(),
            "google provider should be preserved"
        );
        assert!(
            provider.get("anthropic").is_some(),
            "anthropic provider should be preserved"
        );
        assert_eq!(
            provider
                .get("google")
                .unwrap()
                .get("options")
                .unwrap()
                .get("apiKey")
                .unwrap(),
            "google-key"
        );
        assert_eq!(
            provider
                .get("anthropic")
                .unwrap()
                .get("options")
                .unwrap()
                .get("apiKey")
                .unwrap(),
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
        assert!(
            models.contains_key("claude-sonnet-4-6"),
            "should have claude-sonnet-4-6"
        );
        assert!(
            models.contains_key("gemini-3.1-pro"),
            "should have gemini-3.1-pro"
        );
        assert!(
            models.contains_key("gemini-2.5-pro"),
            "should have gemini-2.5-pro"
        );

        // Check model structure
        let claude_model = models.get("claude-sonnet-4-6").unwrap();
        assert_eq!(claude_model.get("name").unwrap(), "Claude Sonnet 4.6");
        assert!(claude_model.get("limit").is_some());
        assert!(claude_model.get("modalities").is_some());
    }

    #[test]
    fn test_sync_with_filtered_models() {
        let config = serde_json::json!({});
        let models_to_sync = [minput("claude-sonnet-4-6"), minput("gemini-3.1-pro")];

        let result = apply_sync_to_config(
            config,
            "http://localhost:3000",
            "test-api-key",
            Some(&models_to_sync),
        );

        let provider = result.get("provider").unwrap();
        let ag = provider.get(ANTIGRAVITY_PROVIDER_ID).unwrap();
        let models = ag.get("models").unwrap().as_object().unwrap();

        assert!(models.contains_key("claude-sonnet-4-6"));
        let pro_model = models.get("gemini-3.1-pro").unwrap();
        assert_eq!(pro_model.get("name").unwrap(), "Gemini 3.1 Pro");
        assert_eq!(pro_model["limit"]["output"], 65_535);
        assert!(
            !models.contains_key("gemini-2.5-pro"),
            "should not have unselected models"
        );
    }

    #[test]
    fn test_openai_compatible_sync_creates_apikey_fun_provider() {
        let config = serde_json::json!({
            "provider": {
                ANTIGRAVITY_PROVIDER_ID: {
                    "npm": "@ai-sdk/anthropic",
                    "name": "Antigravity Manager",
                    "options": { "apiKey": "ag-key" }
                }
            }
        });
        let models_to_sync = [
            minput("gpt-5.5"),
            minput_named("claude-sonnet-4-6", "Claude Sonnet 4.6"),
        ];

        let result = apply_openai_compatible_provider_sync(
            config,
            APIKEY_FUN_PROVIDER_ID,
            "APIKEY.FUN",
            "https://api.apikey.fun",
            "fun-key",
            Some(&models_to_sync),
        );

        let provider = result.get("provider").unwrap();
        assert!(
            provider.get(ANTIGRAVITY_PROVIDER_ID).is_some(),
            "antigravity-manager provider should be preserved"
        );
        let fun = provider.get(APIKEY_FUN_PROVIDER_ID).unwrap();
        assert_eq!(fun.get("npm").unwrap(), OPENAI_COMPATIBLE_NPM);
        assert_eq!(fun.get("name").unwrap(), "APIKEY.FUN");
        assert_eq!(
            fun.get("options").unwrap().get("baseURL").unwrap(),
            "https://api.apikey.fun/v1"
        );
        assert_eq!(
            fun.get("options").unwrap().get("apiKey").unwrap(),
            "fun-key"
        );

        let models = fun.get("models").unwrap().as_object().unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(
            models.get("gpt-5.5").unwrap().get("name").unwrap(),
            "Gpt 5.5"
        );

        // claude-sonnet-4-6 is in the catalog: full metadata, dotted display name.
        let claude = models.get("claude-sonnet-4-6").unwrap();
        assert_eq!(claude.get("name").unwrap(), "Claude Sonnet 4.6");
        assert_eq!(claude["limit"]["context"], 200_000);
        assert_eq!(claude["limit"]["output"], 64_000);
        assert!(claude.get("modalities").is_some());
    }

    #[test]
    fn test_openai_compatible_sync_replaces_models() {
        let config = serde_json::json!({
            "provider": {
                APIKEY_FUN_PROVIDER_ID: {
                    "models": {
                        "old-model": { "name": "Old Model" }
                    }
                }
            }
        });

        let result = apply_openai_compatible_provider_sync(
            config,
            APIKEY_FUN_PROVIDER_ID,
            "APIKEY.FUN",
            "https://api.apikey.fun/v1",
            "fun-key",
            Some(&[minput("gpt-5.5")]),
        );

        let models = result["provider"][APIKEY_FUN_PROVIDER_ID]["models"]
            .as_object()
            .unwrap();
        assert!(models.contains_key("gpt-5.5"));
        assert!(!models.contains_key("old-model"));
    }

    #[test]
    fn test_profile_collision_leaves_config_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(OPENCODE_CONFIG_FILE);
        let original = r#"{"provider":{"apikey-fun-abcdef":{"options":{"apiKey":"first-key"}}}}"#;
        fs::write(&path, original).unwrap();
        let error = sync_openai_provider_to_path(
            &path,
            "apikey-fun-abcdef",
            "APIKEY.FUN",
            "https://api.example.com",
            "second-key",
            None,
        )
        .unwrap_err();
        assert!(is_provider_validation_error(&error));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 1);
    }

    #[test]
    fn test_concurrent_profile_sync_preserves_both_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(OPENCODE_CONFIG_FILE);
        std::thread::scope(|scope| {
            for id in ["apikey-fun-111111", "apikey-fun-222222"] {
                let path = &path;
                scope.spawn(move || {
                    sync_openai_provider_to_path(path, id, id, "https://api.example.com", id, None)
                        .unwrap();
                });
            }
        });
        let config = parse_config_file(&path).unwrap();
        assert_eq!(config["provider"].as_object().unwrap().len(), 2);
        for id in ["apikey-fun-111111", "apikey-fun-222222"] {
            assert_eq!(config["provider"][id]["options"]["apiKey"], id);
        }
    }

    #[test]
    fn test_atomic_write_cleans_temp_after_rename_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("directory");
        fs::create_dir(&target).unwrap();
        assert!(atomically_write_config(&target, &serde_json::json!({})).is_err());
        assert!(target.is_dir());
        assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn test_atomic_write_private_permissions_and_replacement() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("nested").join(OPENCODE_CONFIG_FILE);
        atomically_write_config(&target, &serde_json::json!({"key": "first"})).unwrap();
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();
        atomically_write_config(&target, &serde_json::json!({"key": "second"})).unwrap();
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o640
        );
        assert_eq!(parse_config_file(&target).unwrap()["key"], "second");
        assert_eq!(fs::read_dir(target.parent().unwrap()).unwrap().count(), 1);
    }

    #[test]
    fn test_apply_remove_provider_removes_correct_provider() {
        let config = serde_json::json!({
            "provider": {
                "apikey-fun-123456": { "name": "APIKEY.FUN (123456)" },
                "apikey-fun-abcdef": { "name": "APIKEY.FUN (abcdef)" },
                "antigravity-manager": { "name": "Antigravity" }
            }
        });

        let (result, changed) = apply_remove_provider(config, "apikey-fun-123456");
        assert!(changed);
        let providers = result["provider"].as_object().unwrap();
        assert_eq!(providers.len(), 2);
        assert!(!providers.contains_key("apikey-fun-123456"));
        assert!(providers.contains_key("apikey-fun-abcdef"));
        assert!(providers.contains_key("antigravity-manager"));
    }

    #[test]
    fn test_apply_remove_last_provider_removes_provider_key() {
        let config = serde_json::json!({
            "$schema": "https://opencode.ai/config.json",
            "provider": {
                "apikey-fun-123456": { "name": "APIKEY.FUN (123456)" }
            }
        });

        let (result, changed) = apply_remove_provider(config, "apikey-fun-123456");
        assert!(changed);
        assert!(result.get("provider").is_none());
        assert_eq!(result["$schema"], "https://opencode.ai/config.json");
    }

    #[test]
    fn test_apply_remove_nonexistent_provider_returns_false() {
        let config = serde_json::json!({
            "provider": {
                "antigravity-manager": { "name": "Antigravity" }
            }
        });

        let (result, changed) = apply_remove_provider(config, "nonexistent");
        assert!(!changed);
        assert!(result["provider"]
            .as_object()
            .unwrap()
            .contains_key("antigravity-manager"));
    }

    #[test]
    fn test_apply_remove_on_empty_provider_map_does_not_mutate() {
        let config = serde_json::json!({ "provider": {} });

        let (result, changed) = apply_remove_provider(config, "apikey-fun-abc123");
        assert!(!changed);
        assert!(
            result.get("provider").is_some(),
            "provider key must survive when nothing was removed"
        );
    }

    #[test]
    fn test_remove_opencode_provider_rejects_invalid_and_unmanaged_ids() {
        // Empty / whitespace-only
        assert!(remove_opencode_provider("   ").is_err());
        // Path traversal characters are rejected by the charset check
        assert!(remove_opencode_provider("../../etc/passwd").is_err());
        // Reserved provider cannot be removed through this path
        assert!(remove_opencode_provider(ANTIGRAVITY_PROVIDER_ID).is_err());
        // Providers not owned by this feature are refused
        for id in ["anthropic", "openai", "github-copilot", "openrouter"] {
            let err = remove_opencode_provider(id).expect_err("unmanaged provider must be refused");
            assert!(
                err.contains("is not managed by Antigravity-Manager"),
                "unexpected error for {}: {}",
                id,
                err
            );
        }
    }

    #[test]
    fn test_is_provider_validation_error_classification() {
        assert!(is_provider_validation_error(
            "OpenCode provider id is required"
        ));
        assert!(is_provider_validation_error(
            "Invalid OpenCode provider id 'a/b': only letters, digits, '-' and '_' are allowed"
        ));
        assert!(is_provider_validation_error(
            "Provider 'antigravity-manager' is reserved and cannot be removed; use clear instead"
        ));
        assert!(is_provider_validation_error(
            "Provider 'openai' is not managed by Antigravity-Manager and cannot be removed"
        ));
        // Generic OS errors must stay 5xx
        assert!(!is_provider_validation_error(
            "Failed to write temp file: Invalid argument (os error 22)"
        ));
        assert!(!is_provider_validation_error(
            "Failed to read OpenCode config: Permission denied"
        ));
    }

    #[test]
    fn test_extract_providers_from_config_sorts_and_skips_invalid() {
        let config = serde_json::json!({
            "provider": {
                "zeta": {
                    "name": "Zeta",
                    "models": { "b-model": {}, "a-model": {} }
                },
                "alpha": {
                    "name": "Alpha",
                    "npm": "@ai-sdk/openai-compatible",
                    "options": {
                        "baseURL": "https://api.example.com/v1",
                        "apiKey": "sk-test"
                    },
                    "models": { "m1": {} }
                },
                "invalid": "not-an-object"
            }
        });

        let providers = extract_providers_from_config(&config);
        assert_eq!(providers.len(), 2);
        assert_eq!(providers[0].id, "alpha");
        assert_eq!(providers[0].name.as_deref(), Some("Alpha"));
        assert_eq!(
            providers[0].npm.as_deref(),
            Some("@ai-sdk/openai-compatible")
        );
        assert_eq!(
            providers[0].base_url.as_deref(),
            Some("https://api.example.com/v1")
        );
        assert_eq!(providers[0].api_key.as_deref(), Some("sk-test"));
        assert_eq!(providers[0].models, vec!["m1".to_string()]);

        assert_eq!(providers[1].id, "zeta");
        assert_eq!(
            providers[1].models,
            vec!["a-model".to_string(), "b-model".to_string()]
        );
    }

    #[test]
    fn test_openai_compatible_sync_empty_models_keeps_existing() {
        let config = serde_json::json!({
            "provider": {
                APIKEY_FUN_PROVIDER_ID: {
                    "models": {
                        "gpt-4o": { "name": "GPT-4o", "custom": true }
                    }
                }
            }
        });

        let result = apply_openai_compatible_provider_sync(
            config,
            APIKEY_FUN_PROVIDER_ID,
            "APIKEY.FUN",
            "https://api.apikey.fun/v1",
            "fun-key",
            Some(&[]),
        );

        let models = result["provider"][APIKEY_FUN_PROVIDER_ID]["models"]
            .as_object()
            .unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models.get("gpt-4o").unwrap().get("name").unwrap(), "GPT-4o");
        assert_eq!(
            models.get("gpt-4o").unwrap().get("custom").unwrap(),
            &Value::Bool(true)
        );
    }

    #[test]
    fn test_provider_id_validation() {
        assert!(validate_provider_id("apikey-fun").is_ok());
        assert!(validate_provider_id("My_Provider2").is_ok());
        assert!(validate_provider_id("").is_err());
        assert!(validate_provider_id("  ").is_err());
        assert!(validate_provider_id("bad/id").is_err());
        assert!(validate_provider_id("bad id").is_err());
    }

    #[test]
    fn test_openai_sync_preserves_unknown_model_settings() {
        let existing = serde_json::json!({
            "name": "My GPT",
            "limit": { "context": 128_000, "output": 16_384 },
            "options": { "reasoningEffort": "high" },
            "tool_call": true
        });
        let result = apply_openai_compatible_provider_sync(
            serde_json::json!({ "provider": { APIKEY_FUN_PROVIDER_ID: {
                "models": { "gpt-5.5": existing.clone() }
            }}}),
            APIKEY_FUN_PROVIDER_ID,
            "APIKEY.FUN",
            "https://api.apikey.fun/v1/",
            "fun-key",
            Some(&[minput("gpt-5.5")]),
        );
        assert_eq!(
            result["provider"][APIKEY_FUN_PROVIDER_ID]["models"]["gpt-5.5"],
            existing
        );
        assert_eq!(
            result["provider"][APIKEY_FUN_PROVIDER_ID]["options"]["baseURL"],
            "https://api.apikey.fun/v1"
        );
    }

    #[test]
    fn test_openai_sync_repairs_non_object_options() {
        for options in [
            Value::Null,
            serde_json::json!([]),
            serde_json::json!("invalid"),
        ] {
            let result = apply_openai_compatible_provider_sync(
                serde_json::json!({ "provider": { APIKEY_FUN_PROVIDER_ID: { "options": options }}}),
                APIKEY_FUN_PROVIDER_ID,
                "APIKEY.FUN",
                "https://api.apikey.fun",
                "fun-key",
                None,
            );
            assert_eq!(
                result["provider"][APIKEY_FUN_PROVIDER_ID]["options"]["apiKey"],
                "fun-key"
            );
        }
    }

    #[test]
    fn test_openai_sync_leaves_invalid_config_untouched() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(OPENCODE_CONFIG_FILE_JSONC);
        for content in ["{ broken", "[]", "null", ""] {
            fs::write(&path, content).unwrap();
            let result = sync_openai_provider_to_path(
                &path,
                APIKEY_FUN_PROVIDER_ID,
                "APIKEY.FUN",
                "https://api.apikey.fun",
                "fun-key",
                None,
            );
            assert!(result.is_err(), "must reject invalid config: {content}");
            assert_eq!(fs::read_to_string(&path).unwrap(), content);
            assert!(!path.with_extension("tmp").exists());
            assert!(!tmp
                .path()
                .join(format!("{OPENCODE_CONFIG_FILE_JSONC}{BACKUP_SUFFIX}"))
                .exists());
        }
        // A read error must also propagate instead of replacing the config.
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(sync_openai_provider_to_path(
            &path,
            APIKEY_FUN_PROVIDER_ID,
            "APIKEY.FUN",
            "https://api.apikey.fun",
            "fun-key",
            None,
        )
        .is_err());
        assert!(path.is_dir());
    }

    #[test]
    fn test_openai_sync_preserves_jsonc_unicode_and_backup() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(OPENCODE_CONFIG_FILE_JSONC);
        let content = r#"{
            // User settings must survive syncing another provider.
            "instructions": ["инструкции.md", "日本語.md", "🚀.md",],
            "provider": {"custom": {"name": "Мой провайдер",},},
        }"#;
        fs::write(&path, content).unwrap();
        sync_openai_provider_to_path(
            &path,
            APIKEY_FUN_PROVIDER_ID,
            "APIKEY.FUN",
            "https://api.apikey.fun/v1/",
            "fun-key",
            Some(&[minput("gpt-5.5")]),
        )
        .unwrap();
        let config: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            config["instructions"],
            serde_json::json!(["инструкции.md", "日本語.md", "🚀.md"])
        );
        assert_eq!(config["provider"]["custom"]["name"], "Мой провайдер");
        assert_eq!(
            config["provider"][APIKEY_FUN_PROVIDER_ID]["options"]["baseURL"],
            "https://api.apikey.fun/v1"
        );
        let backup = tmp
            .path()
            .join(format!("{OPENCODE_CONFIG_FILE_JSONC}{BACKUP_SUFFIX}"));
        assert_eq!(fs::read_to_string(&backup).unwrap(), content);
        sync_openai_provider_to_path(
            &path,
            APIKEY_FUN_PROVIDER_ID,
            "APIKEY.FUN",
            "https://api.apikey.fun/v1",
            "next-key",
            None,
        )
        .unwrap();
        assert_eq!(fs::read_to_string(&backup).unwrap(), content);
    }

    #[test]
    fn test_openai_sync_creates_missing_config_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("new").join(OPENCODE_CONFIG_FILE);
        sync_openai_provider_to_path(
            &path,
            APIKEY_FUN_PROVIDER_ID,
            "APIKEY.FUN",
            "https://api.apikey.fun",
            "fun-key",
            None,
        )
        .unwrap();
        let config = parse_config_file(&path).unwrap();
        assert_eq!(
            config["provider"][APIKEY_FUN_PROVIDER_ID]["options"]["apiKey"],
            "fun-key"
        );
    }

    #[test]
    fn test_humanize_joins_hyphenated_version() {
        assert_eq!(humanize_model_id("claude-sonnet-4-6"), "Claude Sonnet 4.6");
        assert_eq!(
            humanize_model_id("claude-sonnet-4-6-thinking"),
            "Claude Sonnet 4.6 Thinking"
        );
        assert_eq!(
            humanize_model_id("gemini-3.5-flash-low"),
            "Gemini 3.5 Flash Low"
        );
        assert_eq!(
            humanize_model_id("anthropic/claude-opus-4-6"),
            "Claude Opus 4.6"
        );
        assert_eq!(humanize_model_id("grok-4.20-0309"), "Grok 4.20 0309");
    }

    #[test]
    fn test_openai_compatible_sync_matches_dotted_and_prefixed_ids() {
        let result = apply_openai_compatible_provider_sync(
            serde_json::json!({}),
            APIKEY_FUN_PROVIDER_ID,
            "APIKEY.FUN",
            "https://api.apikey.fun/v1",
            "fun-key",
            Some(&[
                minput("claude-sonnet-4.6"),
                minput("anthropic/claude-opus-4-6"),
            ]),
        );
        let models = result["provider"][APIKEY_FUN_PROVIDER_ID]["models"]
            .as_object()
            .unwrap();
        assert_eq!(
            models
                .get("claude-sonnet-4.6")
                .unwrap()
                .get("name")
                .unwrap(),
            "Claude Sonnet 4.6"
        );
        assert_eq!(
            models
                .get("anthropic/claude-opus-4-6")
                .unwrap()
                .get("name")
                .unwrap(),
            "Claude Opus 4.6"
        );
        assert_eq!(models["claude-sonnet-4.6"]["limit"]["context"], 200_000);
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
        assert!(
            provider.get(ANTIGRAVITY_PROVIDER_ID).is_none(),
            "antigravity-manager should be removed"
        );
        assert!(
            provider.get("google").is_some(),
            "google should be preserved"
        );
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
        assert!(
            !models.contains_key("claude-sonnet-4-5"),
            "antigravity model should be removed"
        );
        // Non-antigravity models should be preserved
        assert!(
            models.contains_key("claude-3"),
            "non-antigravity model should be preserved"
        );
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
        assert!(
            anthropic.get("options").is_none(),
            "options should be removed when baseURL matches"
        );
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
        assert!(
            anthropic.get("options").is_some(),
            "options should be preserved when no proxy_url"
        );
        assert!(
            anthropic.get("models").is_some(),
            "models should be preserved when no proxy_url"
        );
    }

    // Tests for base_url_matches

    #[test]
    fn test_base_url_matches_with_v1() {
        assert!(base_url_matches(
            "http://localhost:3000/v1",
            "http://localhost:3000"
        ));
        assert!(base_url_matches(
            "http://localhost:3000",
            "http://localhost:3000/v1"
        ));
        assert!(base_url_matches(
            "http://localhost:3000/v1/",
            "http://localhost:3000"
        ));
    }

    #[test]
    fn test_base_url_matches_without_v1() {
        assert!(base_url_matches(
            "http://localhost:3000",
            "http://localhost:3000"
        ));
        assert!(base_url_matches(
            "http://localhost:3000/",
            "http://localhost:3000/"
        ));
    }

    #[test]
    fn test_base_url_matches_different_urls() {
        assert!(!base_url_matches(
            "http://localhost:3000",
            "http://other-host:3000"
        ));
        assert!(!base_url_matches(
            "http://localhost:3000/v1",
            "http://localhost:4000/v1"
        ));
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
        assert!(
            result.get("provider").is_none(),
            "empty provider object should be removed"
        );
    }

    /// Regression: model ids not present in the hardcoded catalog must still be
    /// written to the config (previously silently dropped). A minimal `{ "name": ... }`
    /// entry is valid per the OpenCode schema.
    #[test]
    fn test_sync_creates_fallback_model_for_unknown_id() {
        let config = serde_json::json!({});
        // "some-new-model" is deliberately not in build_model_catalog()
        let models_to_sync = [minput("some-new-model")];

        let result = apply_sync_to_config(
            config,
            "http://localhost:3000",
            "test-api-key",
            Some(&models_to_sync),
        );

        let models = result
            .get("provider")
            .unwrap()
            .get(ANTIGRAVITY_PROVIDER_ID)
            .unwrap()
            .get("models")
            .unwrap()
            .as_object()
            .unwrap();

        assert!(
            models.contains_key("some-new-model"),
            "unknown model id must still be written, not dropped"
        );
        let entry = models.get("some-new-model").unwrap();
        assert!(
            entry.get("name").is_some(),
            "fallback entry must at least have a name"
        );
    }

    /// Mixing known catalog ids with unknown ids should write all of them.
    #[test]
    fn test_sync_filtered_models_with_known_and_unknown_ids() {
        let config = serde_json::json!({});
        let models_to_sync = [
            minput("claude-sonnet-4-6"),
            minput("gemini-3.1-pro"),
            minput("custom-future-model"),
        ];

        let result = apply_sync_to_config(
            config,
            "http://localhost:3000",
            "test-api-key",
            Some(&models_to_sync),
        );

        let models = result
            .get("provider")
            .unwrap()
            .get(ANTIGRAVITY_PROVIDER_ID)
            .unwrap()
            .get("models")
            .unwrap()
            .as_object()
            .unwrap();

        // Known ids get full catalog metadata
        let claude = models.get("claude-sonnet-4-6").unwrap();
        assert_eq!(claude.get("name").unwrap(), "Claude Sonnet 4.6");
        assert!(claude.get("limit").is_some());

        // Unknown id gets a minimal fallback entry
        let custom = models.get("custom-future-model").unwrap();
        assert!(custom.get("name").is_some());
        assert_eq!(models["gemini-3.1-pro"]["limit"]["output"], 65_535);
    }

    /// Fallback entries should not clobber a model object the user already defined.
    #[test]
    fn test_sync_fallback_preserves_user_defined_model() {
        let config = serde_json::json!({
            "provider": {
                "antigravity-manager": {
                    "models": {
                        "custom-future-model": {
                            "name": "My Custom Name",
                            "limit": { "context": 128000, "output": 4096 }
                        }
                    }
                }
            }
        });
        let models_to_sync = [minput("custom-future-model")];

        let result = apply_sync_to_config(
            config,
            "http://localhost:3000",
            "test-api-key",
            Some(&models_to_sync),
        );

        let entry = result
            .get("provider")
            .unwrap()
            .get(ANTIGRAVITY_PROVIDER_ID)
            .unwrap()
            .get("models")
            .unwrap()
            .get("custom-future-model")
            .unwrap();

        // User's own name/limit must be preserved, not overwritten by the fallback.
        assert_eq!(entry.get("name").unwrap(), "My Custom Name");
        assert_eq!(
            entry.get("limit").unwrap().get("context").unwrap(),
            &serde_json::json!(128000)
        );
    }

    /// The fallback name derivation should produce human-readable names.
    /// The fallback name derivation should preserve version dots (e.g. "3.5").
    #[test]
    fn test_build_fallback_model_json_readable_name() {
        // No display name: derived from id, dots preserved.
        let entry = build_fallback_model_json("gemini-2.5-pro", None);
        assert_eq!(entry.get("name").unwrap(), "Gemini 2.5 Pro");

        // 3.5 should stay together, not split into "3 5".
        let entry = build_fallback_model_json("gemini-3.5-flash-low", None);
        assert_eq!(entry.get("name").unwrap(), "Gemini 3.5 Flash Low");
    }

    /// When the frontend provides a display name, it must be used verbatim so the
    /// config shows the same name the user saw (e.g. "Gemini 3.5 Flash (High)").
    #[test]
    fn test_build_fallback_model_json_uses_display_name() {
        let entry =
            build_fallback_model_json("gemini-3.5-flash-high", Some("Gemini 3.5 Flash (High)"));
        assert_eq!(entry.get("name").unwrap(), "Gemini 3.5 Flash (High)");
    }

    /// Fallback entries for known families should pick up sensible limit/modalities.
    #[test]
    fn test_build_fallback_model_json_series_defaults() {
        // A gemini-3.x id gets 1M context + multimodal input.
        let entry = build_fallback_model_json("gemini-3.5-flash-low", None);
        let limit = entry.get("limit").unwrap();
        assert_eq!(limit.get("context").unwrap(), &serde_json::json!(1_048_576));
        assert!(entry.get("modalities").is_some());

        // A claude id gets 200k context.
        let entry = build_fallback_model_json("claude-sonnet-4-9", None);
        let limit = entry.get("limit").unwrap();
        assert_eq!(limit.get("context").unwrap(), &serde_json::json!(200_000));

        // An unknown family (not gemini/claude) only gets a name.
        let entry = build_fallback_model_json("acme-model-1", None);
        assert!(entry.get("limit").is_none());
        assert!(entry.get("name").is_some());
    }

    /// resolve_active_config_file_name should prefer .jsonc when None is passed
    /// (pure helper path), and the default file should be opencode.json.
    #[test]
    fn test_resolve_active_config_file_name_default() {
        // Without probing a directory, the default is opencode.json.
        assert_eq!(resolve_active_config_file_name(None), OPENCODE_CONFIG_FILE);
    }

    #[test]
    fn test_resolve_active_config_file_name_prefers_existing_jsonc() {
        // Create a temp dir that has only opencode.jsonc and confirm it is preferred.
        let tmp = tempfile::tempdir().expect("create temp dir");
        let dir = tmp.path().to_path_buf();
        std::fs::write(dir.join(OPENCODE_CONFIG_FILE_JSONC), "{}").expect("write jsonc");
        assert_eq!(
            resolve_active_config_file_name(Some(&dir)),
            OPENCODE_CONFIG_FILE_JSONC
        );
    }

    #[test]
    fn test_resolve_active_config_file_name_falls_back_to_json() {
        // A temp dir with no config files at all defaults to opencode.json.
        let tmp = tempfile::tempdir().expect("create temp dir");
        let dir = tmp.path().to_path_buf();
        assert_eq!(
            resolve_active_config_file_name(Some(&dir)),
            OPENCODE_CONFIG_FILE
        );
    }

    /// strip_jsonc_comments must remove line and block comments while preserving
    /// `//` and `/* */` that appear inside string values.
    #[test]
    fn test_strip_jsonc_comments_line_and_block() {
        let input = r#"{
  // a line comment
  "key": "value", /* trailing block comment */
  "url": "https://example.com/path" // not a comment inside string
}"#;
        let stripped = strip_jsonc_comments(input);
        // The resulting string must be valid JSON.
        let parsed: Value =
            serde_json::from_str(&stripped).expect("stripped jsonc must be valid JSON");
        assert_eq!(parsed.get("key").unwrap(), "value");
        // The URL's // must be preserved (it is inside a string).
        assert_eq!(parsed.get("url").unwrap(), "https://example.com/path");
    }

    #[test]
    fn test_strip_jsonc_comments_preserves_escaped_quotes() {
        // A string containing an escaped quote followed by // must not confuse the scanner.
        let input = r##"{"msg": "a \"quoted//\" end", "n": 1 // comment
}"##;
        let stripped = strip_jsonc_comments(input);
        let parsed: Value =
            serde_json::from_str(&stripped).expect("stripped jsonc must be valid JSON");
        assert_eq!(parsed.get("msg").unwrap(), "a \"quoted//\" end");
        assert_eq!(parsed.get("n").unwrap(), 1);
    }

    /// strip_jsonc_trailing_commas must drop a comma right before `}` or `]` (with
    /// optional whitespace between), while keeping commas that separate real elements.
    #[test]
    fn test_strip_jsonc_trailing_commas() {
        let input = "{\n  \"a\": 1,\n  \"b\": 2,\n}"; // trailing comma before }
        let stripped = strip_jsonc_trailing_commas(input);
        let parsed: Value = serde_json::from_str(&stripped).expect("stripped must be valid JSON");
        assert_eq!(parsed.get("a").unwrap(), 1);
        assert_eq!(parsed.get("b").unwrap(), 2);
    }

    #[test]
    fn test_strip_jsonc_trailing_commas_in_nested_array() {
        // trailing comma before ] and before }, with whitespace/newlines in between.
        let input = "{\n  \"arr\": [1, 2, 3,],\n  \"obj\": { \"k\": \"v\", },\n}";
        let stripped = strip_jsonc_trailing_commas(input);
        let parsed: Value = serde_json::from_str(&stripped).expect("stripped must be valid JSON");
        assert_eq!(parsed.get("arr").unwrap().as_array().unwrap().len(), 3);
        assert_eq!(parsed.get("obj").unwrap().get("k").unwrap(), "v");
    }

    #[test]
    fn test_strip_jsonc_trailing_commas_keeps_real_commas_in_strings() {
        // A comma inside a string must NOT be removed even if followed by }.
        let input = "{\"msg\": \"hello, }\",}";
        let stripped = strip_jsonc_trailing_commas(input);
        let parsed: Value = serde_json::from_str(&stripped).expect("stripped must be valid JSON");
        assert_eq!(parsed.get("msg").unwrap(), "hello, }");
    }

    /// parse_config_file must read a jsonc file with BOTH comments and trailing commas
    /// — this is the regression for the user-reported "config destroyed" case where a
    /// trailing comma made serde_json fail and the whole config was replaced with {}.
    #[test]
    fn test_parse_config_file_handles_jsonc_comments_and_trailing_commas() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let path = tmp.path().join("opencode.jsonc");
        std::fs::write(
            &path,
            r#"{
  // user's config with a trailing comma (valid JSONC, invalid strict JSON)
  "$schema": "https://opencode.ai/config.json",
  "provider": {
    "myown": { "name": "My Own Provider" },
  },
}"#,
        )
        .unwrap();

        let parsed = parse_config_file(&path).expect("jsonc with trailing comma must parse");
        assert_eq!(
            parsed
                .get("provider")
                .unwrap()
                .get("myown")
                .unwrap()
                .get("name")
                .unwrap(),
            "My Own Provider"
        );
    }

    /// parse_config_file must read a jsonc file (with comments) into a Value.
    #[test]
    fn test_parse_config_file_handles_jsonc_comments() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let path = tmp.path().join("opencode.jsonc");
        std::fs::write(
            &path,
            r#"{
  // my opencode config
  "$schema": "https://opencode.ai/config.json",
  "provider": {
    "myown": { "name": "My Own Provider" }
  }
}"#,
        )
        .unwrap();

        let parsed = parse_config_file(&path).expect("jsonc file must parse");
        assert_eq!(
            parsed
                .get("provider")
                .unwrap()
                .get("myown")
                .unwrap()
                .get("name")
                .unwrap(),
            "My Own Provider"
        );
    }

    /// The frontend-provided display name must flow through to the config for models
    /// that aren't in the catalog. This is the regression for the user-reported case
    /// where "Gemini 3.5 Flash (High)" became a stripped "Gemini 3 5 Flash Low".
    #[test]
    fn test_sync_uses_frontend_display_name_for_unknown_model() {
        let config = serde_json::json!({});
        let models_to_sync = [
            minput_named("custom-unknown-flash", "Custom Unknown Flash (High)"),
            minput_named("custom-unknown-agent", "Custom Unknown Agent"),
        ];

        let result = apply_sync_to_config(
            config,
            "http://localhost:3000",
            "test-api-key",
            Some(&models_to_sync),
        );

        let models = result
            .get("provider")
            .unwrap()
            .get(ANTIGRAVITY_PROVIDER_ID)
            .unwrap()
            .get("models")
            .unwrap()
            .as_object()
            .unwrap();

        // The display name must be used as-is, preserving parentheses/variant info.
        assert_eq!(
            models
                .get("custom-unknown-flash")
                .unwrap()
                .get("name")
                .unwrap(),
            "Custom Unknown Flash (High)"
        );
        assert_eq!(
            models
                .get("custom-unknown-agent")
                .unwrap()
                .get("name")
                .unwrap(),
            "Custom Unknown Agent"
        );
    }
}

pub fn read_opencode_config_content(file_name: Option<String>) -> Result<String, String> {
    let _lock = acquire_opencode_config_lock();

    let Some((opencode_path, ag_config_path, ag_accounts_path)) = get_config_paths() else {
        return Err("Failed to get OpenCode config directory".to_string());
    };

    // Allowlist of permitted file names
    let allowed_files = [
        OPENCODE_CONFIG_FILE,
        OPENCODE_CONFIG_FILE_JSONC,
        ANTIGRAVITY_CONFIG_FILE,
        ANTIGRAVITY_ACCOUNTS_FILE,
    ];

    // Determine which file to read. Both opencode.json and opencode.jsonc map to the
    // active opencode config path (which is resolved by probing the directory), so a
    // caller asking for "opencode.json" still gets the user's actual config when it
    // happens to be opencode.jsonc.
    let target_path = match file_name.as_deref() {
        Some(name) if name == ANTIGRAVITY_CONFIG_FILE => ag_config_path,
        Some(name) if name == ANTIGRAVITY_ACCOUNTS_FILE => ag_accounts_path,
        Some(name) if name == OPENCODE_CONFIG_FILE || name == OPENCODE_CONFIG_FILE_JSONC => {
            opencode_path
        }
        Some(name) => {
            return Err(format!(
                "Invalid file name: {}. Allowed: {:?}",
                name, allowed_files
            ))
        }
        None => opencode_path, // Default to the active opencode config (json or jsonc)
    };

    if !target_path.exists() {
        return Err(format!("Config file does not exist: {:?}", target_path));
    }

    fs::read_to_string(&target_path).map_err(|e| format!("Failed to read config: {}", e))
}

#[tauri::command]
pub async fn get_opencode_sync_status(proxy_url: String) -> Result<OpencodeStatus, String> {
    tokio::task::spawn_blocking(move || {
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
                OPENCODE_CONFIG_FILE_JSONC.to_string(),
                ANTIGRAVITY_CONFIG_FILE.to_string(),
                ANTIGRAVITY_ACCOUNTS_FILE.to_string(),
            ],
        })
    })
    .await
    .unwrap_or_else(|_| Err("Failed to execute check".to_string()))
}

#[tauri::command]
pub fn get_canonical_families() -> Vec<CanonicalFamilyDto> {
    GEMINI_FAMILIES
        .iter()
        .map(|family| {
            let mut normalized_match_ids = HashSet::new();
            let mut match_ids = Vec::new();

            for match_id in std::iter::once(family.canonical_id)
                .chain(family.aliases.iter().map(|(alias, _)| *alias))
                .chain(family.tiers.iter().map(|(_, spec)| spec.id))
            {
                if normalized_match_ids.insert(match_id.to_lowercase()) {
                    match_ids.push(match_id.to_string());
                }
            }

            CanonicalFamilyDto {
                canonical_id: family.canonical_id.to_string(),
                display_name: family.display_name.to_string(),
                match_ids,
            }
        })
        .collect()
}

#[tauri::command]
pub async fn execute_opencode_sync(
    proxy_url: String,
    api_key: String,
    sync_accounts: Option<bool>,
    models: Option<Vec<ModelInput>>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        sync_opencode_config(&proxy_url, &api_key, sync_accounts.unwrap_or(false), models)
    })
    .await
    .unwrap_or_else(|_| Err("Failed to execute sync".to_string()))
}

#[tauri::command]
pub async fn execute_opencode_openai_sync(
    proxy_url: String,
    api_key: String,
    provider_id: Option<String>,
    provider_name: Option<String>,
    models: Option<Vec<ModelInput>>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        sync_opencode_openai_provider(
            provider_id
                .as_deref()
                .filter(|id| !id.trim().is_empty())
                .unwrap_or(APIKEY_FUN_PROVIDER_ID),
            provider_name
                .as_deref()
                .filter(|name| !name.trim().is_empty())
                .unwrap_or("APIKEY.FUN"),
            &proxy_url,
            &api_key,
            models,
        )
    })
    .await
    .unwrap_or_else(|_| Err("Failed to execute sync".to_string()))
}

#[tauri::command]
pub async fn execute_opencode_restore() -> Result<(), String> {
    tokio::task::spawn_blocking(move || restore_opencode_config())
        .await
        .unwrap_or_else(|_| Err("Failed to execute restore".to_string()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetOpencodeConfigRequest {
    pub file_name: Option<String>,
}

#[tauri::command]
pub async fn get_opencode_config_content(
    request: GetOpencodeConfigRequest,
) -> Result<String, String> {
    tokio::task::spawn_blocking(move || read_opencode_config_content(request.file_name))
        .await
        .unwrap_or_else(|_| Err("Failed to read config".to_string()))
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
    let _lock = acquire_opencode_config_lock();

    let Some((config_path, _, accounts_path)) = get_config_paths() else {
        return Err("Failed to get OpenCode config directory".to_string());
    };

    // Process opencode.json
    if config_path.exists() {
        // Create backup before modifying
        create_backup(&config_path)?;

        let content = fs::read_to_string(&config_path)
            .map_err(|e| format!("Failed to read config: {}", e))?;

        // Tolerate JSONC (comments + trailing commas) when the user's config is opencode.jsonc.
        let config: Value = parse_jsonc(&content)
            .filter(Value::is_object)
            .ok_or_else(|| {
                "OpenCode config must be a valid JSON/JSONC object; file left unchanged".to_string()
            })?;
        let config = apply_clear_to_config(config, proxy_url.as_deref(), clear_legacy);

        atomically_write_config(&config_path, &config)?;
    }

    // Process antigravity-accounts.json
    let accounts_backup_new =
        accounts_path.with_file_name(format!("{}{}", ANTIGRAVITY_ACCOUNTS_FILE, BACKUP_SUFFIX));
    let accounts_backup_old = accounts_path.with_file_name(format!(
        "{}{}",
        ANTIGRAVITY_ACCOUNTS_FILE, OLD_BACKUP_SUFFIX
    ));

    if accounts_backup_new.exists() {
        // Restore from new backup
        restore_backup_to_target(&accounts_backup_new, &accounts_path, "accounts from backup")?;
    } else if accounts_backup_old.exists() {
        // Restore from old backup
        restore_backup_to_target(
            &accounts_backup_old,
            &accounts_path,
            "accounts from old backup",
        )?;
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
        let remove_models_key = if let Some(models) = provider_obj
            .get_mut("models")
            .and_then(|m| m.as_object_mut())
        {
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
        let remove_options_key = if let Some(options) = provider_obj
            .get_mut("options")
            .and_then(|o| o.as_object_mut())
        {
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

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OpencodeProviderSummary {
    pub id: String,
    pub name: Option<String>,
    pub npm: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub models: Vec<String>,
}

pub fn extract_providers_from_config(config: &Value) -> Vec<OpencodeProviderSummary> {
    let mut result = Vec::new();
    if let Some(providers) = config.get("provider").and_then(Value::as_object) {
        for (id, val) in providers {
            let Some(val_obj) = val.as_object() else {
                continue;
            };
            let name = val_obj
                .get("name")
                .and_then(Value::as_str)
                .map(String::from);
            let npm = val_obj.get("npm").and_then(Value::as_str).map(String::from);
            let options = val_obj.get("options");
            let base_url = options
                .and_then(|o| o.get("baseURL"))
                .and_then(Value::as_str)
                .map(String::from);
            let api_key = options
                .and_then(|o| o.get("apiKey"))
                .and_then(Value::as_str)
                .map(String::from);

            let mut models = Vec::new();
            if let Some(models_obj) = val_obj.get("models").and_then(Value::as_object) {
                models = models_obj.keys().cloned().collect();
                models.sort();
            }

            result.push(OpencodeProviderSummary {
                id: id.clone(),
                name,
                npm,
                base_url,
                api_key,
                models,
            });
        }
    }

    result.sort_by(|a, b| a.id.cmp(&b.id));
    result
}

pub fn read_opencode_providers() -> Result<Vec<OpencodeProviderSummary>, String> {
    let _lock = acquire_opencode_config_lock();

    let Some((config_path, _, _)) = get_config_paths() else {
        return Err("Failed to get OpenCode config directory".to_string());
    };

    if !config_path.exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(&config_path)
        .map_err(|e| format!("Failed to read OpenCode config: {}", e))?;
    let config: Value = parse_jsonc(&content)
        .filter(Value::is_object)
        .ok_or_else(|| "OpenCode config is not valid JSON/JSONC".to_string())?;

    Ok(extract_providers_from_config(&config))
}

pub fn apply_remove_provider(mut config: Value, provider_id: &str) -> (Value, bool) {
    let provider_id = provider_id.trim();
    let mut changed = false;
    if let Some(providers) = config.get_mut("provider").and_then(Value::as_object_mut) {
        if providers.remove(provider_id).is_some() {
            changed = true;
            if providers.is_empty() {
                if let Some(config_obj) = config.as_object_mut() {
                    config_obj.remove("provider");
                }
            }
        }
    }
    (config, changed)
}

pub fn remove_opencode_provider(provider_id: &str) -> Result<(), String> {
    let provider_id = provider_id.trim();
    validate_provider_id(provider_id)?;

    if provider_id == ANTIGRAVITY_PROVIDER_ID {
        return Err(format!(
            "Provider '{}' is reserved and cannot be removed; use clear instead",
            ANTIGRAVITY_PROVIDER_ID
        ));
    }

    // Only profiles managed by this feature may be removed.
    if provider_id != APIKEY_FUN_PROVIDER_ID
        && !provider_id.starts_with(&format!("{}-", APIKEY_FUN_PROVIDER_ID))
    {
        return Err(format!(
            "Provider '{}' is not managed by Antigravity-Manager and cannot be removed",
            provider_id
        ));
    }

    let _lock = acquire_opencode_config_lock();

    let Some((config_path, _, _)) = get_config_paths() else {
        return Err("Failed to get OpenCode config directory".to_string());
    };

    if !config_path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(&config_path)
        .map_err(|e| format!("Failed to read OpenCode config: {}", e))?;
    let config: Value = parse_jsonc(&content)
        .filter(Value::is_object)
        .ok_or_else(|| "OpenCode config must be a valid JSON/JSONC object".to_string())?;

    let (updated_config, changed) = apply_remove_provider(config, provider_id);

    if changed {
        create_backup(&config_path)?;
        atomically_write_config(&config_path, &updated_config)?;
    }

    Ok(())
}

#[tauri::command]
pub async fn get_opencode_providers() -> Result<Vec<OpencodeProviderSummary>, String> {
    tokio::task::spawn_blocking(read_opencode_providers)
        .await
        .unwrap_or_else(|_| Err("Failed to read OpenCode providers".to_string()))
}

#[tauri::command]
pub async fn execute_opencode_remove_provider(provider_id: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || remove_opencode_provider(&provider_id))
        .await
        .unwrap_or_else(|_| Err("Failed to execute remove provider".to_string()))
}

#[tauri::command]
pub async fn execute_opencode_clear(
    proxy_url: Option<String>,
    clear_legacy: Option<bool>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        clear_opencode_config(proxy_url, clear_legacy.unwrap_or(false))
    })
    .await
    .unwrap_or_else(|_| Err("Failed to execute clear".to_string()))
}

#[cfg(test)]
mod canonical_family_tests {
    use super::*;

    #[test]
    fn canonical_families_expose_the_complete_public_dto() {
        let families = get_canonical_families();

        assert_eq!(families.len(), GEMINI_FAMILIES.len());
        assert_eq!(
            families,
            vec![
                CanonicalFamilyDto {
                    canonical_id: "gemini-3.5-flash".to_string(),
                    display_name: "Gemini 3.5 Flash".to_string(),
                    match_ids: vec![
                        "gemini-3.5-flash".to_string(),
                        "gemini-3.5-flash-high".to_string(),
                        "gemini-3.5-flash-medium".to_string(),
                        "gemini-3.5-flash-low".to_string(),
                        "gemini-3-flash".to_string(),
                        "gemini-3.5-flash-extra-low".to_string(),
                        "gemini-3-flash-agent".to_string(),
                    ],
                },
                CanonicalFamilyDto {
                    canonical_id: "gemini-3.1-pro".to_string(),
                    display_name: "Gemini 3.1 Pro".to_string(),
                    match_ids: vec![
                        "gemini-3.1-pro".to_string(),
                        "gemini-3.1-pro-high".to_string(),
                        "gemini-pro".to_string(),
                        "gemini-3.1-pro-low".to_string(),
                        "gemini-pro-agent".to_string(),
                    ],
                },
            ]
        );

        let serialized = serde_json::to_string(&families).unwrap();
        assert!(!serialized.contains("thinking_budget"));
        assert!(!serialized.contains("max_output_tokens"));
    }

    #[test]
    fn canonical_families_match_ids_are_unique_globally_after_normalization() {
        let mut owners = HashMap::new();

        for family in get_canonical_families() {
            let mut family_match_ids = HashSet::new();
            for match_id in &family.match_ids {
                let normalized = match_id.to_lowercase();
                assert!(
                    family_match_ids.insert(normalized.clone()),
                    "{} contains duplicate match ID {}",
                    family.canonical_id,
                    match_id
                );
                assert!(
                    owners
                        .insert(normalized.clone(), family.canonical_id.clone())
                        .is_none(),
                    "{} belongs to multiple canonical families",
                    normalized
                );
            }
        }
    }
}
