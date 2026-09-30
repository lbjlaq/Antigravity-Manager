use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{env, fs, path::PathBuf, time::Duration};
use tokio::process::Command;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;
const VERSION_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

const OPENCLAW_DIR: &str = ".openclaw";
const OPENCLAW_CONFIG_FILE: &str = "openclaw.json";
const BACKUP_SUFFIX: &str = ".antigravity-manager.bak";
const PROVIDER_ID: &str = "antigravity-manager";

static OPENCLAW_CONFIG_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn acquire_openclaw_config_lock() -> std::sync::MutexGuard<'static, ()> {
    OPENCLAW_CONFIG_MUTEX.lock().unwrap_or_else(|poisoned| {
        tracing::warn!("OPENCLAW_CONFIG_MUTEX was poisoned, recovering lock");
        poisoned.into_inner()
    })
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OpenClawStatus {
    pub installed: bool,
    pub version: Option<String>,
    pub detected_version_target: Option<String>, // "v1" (<2026.8.1) or "v2" (>=2026.8.1)
    pub is_synced: bool,
    pub has_backup: bool,
    pub current_base_url: Option<String>,
    pub files: Vec<String>,
    pub configured_models: Vec<String>,
    pub is_active: bool,
    pub default_model: Option<String>,
    pub synced_version: Option<String>, // "v1" or "v2"
}

fn get_openclaw_dir() -> Option<PathBuf> {
    env::var_os("OPENCLAW_DIR")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(OPENCLAW_DIR)))
}

fn get_config_path() -> Option<PathBuf> {
    env::var_os("OPENCLAW_CONFIG_PATH")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .or_else(|| get_openclaw_dir().map(|dir| dir.join(OPENCLAW_CONFIG_FILE)))
}

fn get_backup_path() -> Option<PathBuf> {
    get_config_path().map(|path| {
        path.with_file_name(format!(
            "{}{}",
            path.file_name().unwrap_or_default().to_string_lossy(),
            BACKUP_SUFFIX
        ))
    })
}

fn normalize_base_url(input: &str) -> String {
    let trimmed = input.trim().trim_end_matches('/');
    if trimmed.ends_with("/v1") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/v1")
    }
}

fn find_in_path(executable: &str) -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        if let Ok(path_var) = env::var("PATH") {
            for dir in path_var.split(';') {
                for ext in ["cmd", "exe", "bat"] {
                    let path = PathBuf::from(dir).join(format!("{executable}.{ext}"));
                    if path.exists() {
                        return Some(path);
                    }
                }
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Ok(path_var) = env::var("PATH") {
            for dir in path_var.split(':') {
                let path = PathBuf::from(dir).join(executable);
                if path.exists() {
                    return Some(path);
                }
            }
        }
    }
    None
}

fn resolve_openclaw_path() -> Option<PathBuf> {
    if let Some(path) = find_in_path("openclaw") {
        return Some(path);
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Some(home) = dirs::home_dir() {
            for path in [
                home.join(".openclaw/bin/openclaw"),
                home.join(".local/bin/openclaw"),
                PathBuf::from("/opt/homebrew/bin/openclaw"),
                PathBuf::from("/usr/local/bin/openclaw"),
                PathBuf::from("/usr/bin/openclaw"),
            ] {
                if path.exists() {
                    return Some(path);
                }
            }
        }
    }
    None
}

fn extract_version(raw: &str) -> String {
    let trimmed = raw.trim();
    for part in trimmed.split_whitespace() {
        let candidate = part.rsplit('/').next().unwrap_or(part);
        let clean = candidate
            .strip_prefix('v')
            .or_else(|| candidate.strip_prefix('V'))
            .unwrap_or(candidate);
        if is_valid_version(clean) {
            return clean.to_string();
        }
    }
    let fallback: String = trimmed
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    if fallback.contains('.') {
        fallback
    } else {
        "unknown".to_string()
    }
}

fn is_valid_version(value: &str) -> bool {
    value.chars().next().is_some_and(|c| c.is_ascii_digit())
        && value.contains('.')
        && value.chars().all(|c| c.is_ascii_digit() || c == '.')
}

/// 判断版本是否 >= 2026.8.1 (v2.0)
/// 例如: 2026.8.1 -> v2, 2026.9.2 -> v2, 2.0.0 -> v2
/// 2026.7.749 -> v1, 1.0.0 -> v1
pub fn classify_openclaw_version(version_str: &str) -> &'static str {
    let nums: Vec<u64> = version_str
        .split('.')
        .filter_map(|part| part.parse::<u64>().ok())
        .collect();

    if nums.is_empty() {
        return "v2"; // 缺省使用最新 2.0
    }

    // 经典 SemVer: 2.x.x
    if nums[0] >= 2 && nums[0] < 1000 {
        return "v2";
    }

    // 日期版本号: 2026.8.1 边界判断
    if nums[0] > 2026 {
        return "v2";
    }
    if nums[0] == 2026 {
        let minor = nums.get(1).copied().unwrap_or(0);
        let patch = nums.get(2).copied().unwrap_or(0);
        if minor > 8 || (minor == 8 && patch >= 1) {
            return "v2";
        }
        return "v1";
    }

    "v1"
}

async fn run_version_command(mut command: Command, timeout: Duration) -> Option<String> {
    command.kill_on_drop(true);
    #[cfg(target_os = "windows")]
    command.creation_flags(CREATE_NO_WINDOW);
    let output = tokio::time::timeout(timeout, command.output()).await;

    match output {
        Ok(Ok(output)) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            Some(extract_version(if stdout.trim().is_empty() {
                &stderr
            } else {
                &stdout
            }))
        }
        _ => None,
    }
}

async fn run_openclaw_version(path: &PathBuf) -> Option<String> {
    let mut command = Command::new(path);
    command.arg("--version");
    run_version_command(command, VERSION_PROBE_TIMEOUT).await
}

pub async fn check_openclaw_installed() -> (bool, Option<String>, Option<String>) {
    match resolve_openclaw_path() {
        Some(path) => {
            let version = run_openclaw_version(&path).await;
            let target = version
                .as_deref()
                .map(classify_openclaw_version)
                .map(str::to_string);
            (true, version, target)
        }
        None => (false, None, None),
    }
}

fn read_openclaw_config(path: &PathBuf) -> Result<Value, String> {
    match fs::read_to_string(path) {
        Ok(content) if content.trim().is_empty() => Ok(json!({})),
        Ok(content) => serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse openclaw.json: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(format!("Failed to read openclaw.json: {e}")),
    }
}

fn atomically_write_config(path: &PathBuf, config: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create directory: {e}"))?;
    }
    let json_str = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Failed to serialize config: {e}"))?;

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(OPENCLAW_CONFIG_FILE);
    let tmp_path = path.with_file_name(format!("{file_name}.tmp.{}", uuid::Uuid::new_v4()));

    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file = options
        .open(&tmp_path)
        .map_err(|e| format!("Failed to create temp file: {e}"))?;

    let write_res = (|| -> std::io::Result<()> {
        use std::io::Write;
        #[cfg(unix)]
        if let Ok(metadata) = fs::metadata(path) {
            file.set_permissions(metadata.permissions())?;
        }
        file.write_all(json_str.as_bytes())?;
        file.sync_all()
    })();

    drop(file);

    if let Err(e) = write_res {
        let _ = fs::remove_file(&tmp_path);
        return Err(format!("Failed to write config: {e}"));
    }

    fs::rename(&tmp_path, path).map_err(|e| {
        let _ = fs::remove_file(&tmp_path);
        format!("Failed to rename config file: {e}")
    })
}

fn create_backup(path: &PathBuf) -> Result<(), String> {
    let backup = path.with_file_name(format!(
        "{}{}",
        path.file_name().unwrap_or_default().to_string_lossy(),
        BACKUP_SUFFIX
    ));
    if !backup.exists() {
        let current = read_openclaw_config(path)?;
        atomically_write_config(&backup, &current)?;
    }
    Ok(())
}

fn is_managed_provider(provider_or_model: &str) -> bool {
    provider_or_model == PROVIDER_ID || provider_or_model.starts_with(&format!("{PROVIDER_ID}/"))
}

fn redact_json_value(val: &mut Value) {
    match val {
        Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                let lower = k.to_ascii_lowercase();
                if lower == "api_key"
                    || lower == "apikey"
                    || lower.ends_with("_api_key")
                    || lower == "token"
                    || lower.ends_with("_token")
                    || lower == "secret"
                    || lower.ends_with("_secret")
                    || lower == "password"
                    || lower.ends_with("_password")
                {
                    if let Value::String(_) = v {
                        *v = Value::String("[REDACTED]".to_string());
                    }
                } else {
                    redact_json_value(v);
                }
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                redact_json_value(item);
            }
        }
        _ => {}
    }
}

/// 根据模型名称动态推导精确的上下文窗口 (contextWindow)、最大输出 (maxTokens)、思考开关及输入模态
/// 用户特别强调：
/// - Gemini 3 及以上：上下文 1024000 (1024K)，输出 65536
/// - Claude 系列：上下文基本都是 256K (256000)，输出 65536
pub fn resolve_model_specs(model_id: &str) -> (u64, u64, bool, Vec<&'static str>) {
    let lower = model_id.to_lowercase();

    let is_gemini = lower.contains("gemini");
    let is_claude = lower.contains("claude");
    let is_reasoning = lower.contains("thinking")
        || lower.contains("reasoning")
        || lower.contains("claude-3-7")
        || lower.contains("gemini-2.5")
        || lower.contains("gemini-3")
        || lower.contains("o1")
        || lower.contains("o3")
        || lower.contains("r1");

    let inputs = vec!["text", "image"];

    if is_gemini {
        // 用户指定: Gemini 3 及以上上下文 1024000，输出 65536
        let context_window = 1024000;
        let max_tokens = 65536;
        (context_window, max_tokens, is_reasoning, inputs)
    } else if is_claude {
        // 用户指定: Claude 系列上下文基本都是 256K (256000)，输出 65536
        let context_window = 256000;
        let max_tokens = 65536;
        (context_window, max_tokens, is_reasoning, inputs)
    } else if lower.contains("o1") || lower.contains("o3") || lower.contains("o4") {
        let context_window = 200000;
        let max_tokens = 100000;
        (context_window, max_tokens, true, inputs)
    } else if lower.contains("gpt-4") || lower.contains("codex") {
        let context_window = 128000;
        let max_tokens = 16384;
        (context_window, max_tokens, is_reasoning, inputs)
    } else if lower.contains("deepseek") {
        let context_window = 128000;
        let max_tokens = if is_reasoning { 65536 } else { 8192 };
        (context_window, max_tokens, is_reasoning, vec!["text"])
    } else {
        // 缺省通用配置
        let context_window = 128000;
        let max_tokens = 16384;
        (context_window, max_tokens, is_reasoning, inputs)
    }
}

pub fn sync_openclaw_provider(
    proxy_url: String,
    api_key: String,
    target_version: String, // "v1" (<2026.8.1) or "v2" (>=2026.8.1)
    models: Vec<String>,
    activate: bool,
    default_model: Option<String>,
) -> Result<(), String> {
    let _lock = acquire_openclaw_config_lock();
    let normalized_url = normalize_base_url(&proxy_url);
    if normalized_url.trim().is_empty() || normalized_url == "/v1" || api_key.trim().is_empty() {
        return Err("OpenClaw base URL and API key are required".to_string());
    }

    let models: Vec<String> = models
        .into_iter()
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty())
        .collect();

    let path = get_config_path().ok_or("Failed to get OpenClaw config directory")?;
    let mut config = read_openclaw_config(&path)?;
    create_backup(&path)?;

    // 1. 构建 Provider 模型数组
    let model_items: Vec<Value> = models
        .iter()
        .map(|m| {
            let (context_window, max_tokens, is_reasoning, input_modalities) =
                resolve_model_specs(m);
            json!({
                "id": m,
                "name": format!("{m} (Antigravity)"),
                "contextWindow": context_window,
                "maxTokens": max_tokens,
                "reasoning": is_reasoning,
                "input": input_modalities
            })
        })
        .collect();

    // 2. 写入 models.providers.antigravity-manager
    let provider_entry = json!({
        "baseUrl": normalized_url,
        "apiKey": api_key.trim(),
        "api": "openai-completions",
        "models": model_items
    });

    if config.get("models").is_none() {
        config["models"] = json!({});
    }
    config["models"]["mode"] = json!("merge");
    if config["models"].get("providers").is_none() {
        config["models"]["providers"] = json!({});
    }
    config["models"]["providers"][PROVIDER_ID] = provider_entry;

    // 3. 针对不同版本进行模型与白名单匹配
    if config.get("agents").is_none() {
        config["agents"] = json!({});
    }
    if config["agents"].get("defaults").is_none() {
        config["agents"]["defaults"] = json!({});
    }

    let is_v1 = target_version.eq_ignore_ascii_case("v1");

    if is_v1 {
        // v1.0 规范 (< 2026.8.1): 使用 agents.defaults.models 字典注册可用模型
        if config["agents"]["defaults"].get("models").is_none() {
            config["agents"]["defaults"]["models"] = json!({});
        }
        if let Some(map) = config["agents"]["defaults"]["models"].as_object_mut() {
            // 清理旧的 antigravity-manager 记录
            let keys_to_remove: Vec<String> = map
                .keys()
                .filter(|k| k.starts_with(&format!("{PROVIDER_ID}/")))
                .cloned()
                .collect();
            for k in keys_to_remove {
                map.remove(&k);
            }
            // 批量添加新同步模型
            for m in &models {
                map.insert(format!("{PROVIDER_ID}/{m}"), json!({}));
            }
        }
    } else {
        // v2.0 规范 (>= 2026.8.1): 使用 agents.defaults.modelPolicy.allow 白名单
        if config["agents"]["defaults"].get("modelPolicy").is_none() {
            config["agents"]["defaults"]["modelPolicy"] = json!({});
        }
        let allow_entry = format!("{PROVIDER_ID}/*");
        let allow_arr = config["agents"]["defaults"]["modelPolicy"]
            .get("allow")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut next_allow: Vec<Value> = allow_arr
            .into_iter()
            .filter(|v| {
                v.as_str()
                    .is_some_and(|s| !s.starts_with(&format!("{PROVIDER_ID}/")) && s != allow_entry)
            })
            .collect();
        next_allow.push(json!(allow_entry));
        config["agents"]["defaults"]["modelPolicy"]["allow"] = json!(next_allow);
    }

    // 4. 激活默认模型 (如勾选)
    let selected_default = default_model
        .as_deref()
        .filter(|m| !m.trim().is_empty())
        .or_else(|| models.first().map(String::as_str));

    if activate {
        if let Some(model_name) = selected_default {
            if config["agents"]["defaults"].get("model").is_none() {
                config["agents"]["defaults"]["model"] = json!({});
            }
            config["agents"]["defaults"]["model"]["primary"] =
                json!(format!("{PROVIDER_ID}/{model_name}"));
        }
    }

    atomically_write_config(&path, &config)
}

pub fn restore_openclaw_config() -> Result<(), String> {
    let _lock = acquire_openclaw_config_lock();
    let path = get_config_path().ok_or("Failed to get OpenClaw config directory")?;
    let backup_path = get_backup_path().ok_or("Failed to get OpenClaw config directory")?;
    if !backup_path.exists() {
        return Err("No backup file found".to_string());
    }
    let restored = read_openclaw_config(&backup_path)?;
    atomically_write_config(&path, &restored)?;
    fs::remove_file(backup_path).map_err(|e| format!("Failed to remove backup: {e}"))
}

pub fn clear_openclaw_config() -> Result<(), String> {
    let _lock = acquire_openclaw_config_lock();
    let path = get_config_path().ok_or("Failed to get OpenClaw config directory")?;
    if !path.exists() {
        return Ok(());
    }
    let mut config = read_openclaw_config(&path)?;
    create_backup(&path)?;

    // 1. 移除 models.providers.antigravity-manager
    if let Some(providers) = config
        .get_mut("models")
        .and_then(|m| m.get_mut("providers"))
        .and_then(|p| p.as_object_mut())
    {
        providers.remove(PROVIDER_ID);
    }

    // 2. 清理 agents.defaults.models 里面的 antigravity-manager/*
    if let Some(agent_models) = config
        .get_mut("agents")
        .and_then(|a| a.get_mut("defaults"))
        .and_then(|d| d.get_mut("models"))
        .and_then(|m| m.as_object_mut())
    {
        let to_remove: Vec<String> = agent_models
            .keys()
            .filter(|k| is_managed_provider(k))
            .cloned()
            .collect();
        for k in to_remove {
            agent_models.remove(&k);
        }
    }

    // 3. 清理 agents.defaults.modelPolicy.allow
    if let Some(allow) = config
        .get_mut("agents")
        .and_then(|a| a.get_mut("defaults"))
        .and_then(|d| d.get_mut("modelPolicy"))
        .and_then(|p| p.get_mut("allow"))
        .and_then(|v| v.as_array_mut())
    {
        allow.retain(|v| {
            v.as_str()
                .is_some_and(|s| s != format!("{PROVIDER_ID}/*") && !is_managed_provider(s))
        });
    }

    // 4. 若主模型指向 antigravity-manager，则清理
    if let Some(primary) = config
        .get("agents")
        .and_then(|a| a.get("defaults"))
        .and_then(|d| d.get("model"))
        .and_then(|m| m.get("primary"))
        .and_then(|p| p.as_str())
    {
        if is_managed_provider(primary) {
            if let Some(model_obj) = config["agents"]["defaults"]["model"].as_object_mut() {
                model_obj.remove("primary");
            }
        }
    }

    atomically_write_config(&path, &config)
}

pub fn read_openclaw_config_content() -> Result<String, String> {
    let _lock = acquire_openclaw_config_lock();
    let path = get_config_path().ok_or("Failed to get OpenClaw config directory")?;
    if !path.exists() {
        return Err(format!("Config file does not exist: {path:?}"));
    }
    let mut config = read_openclaw_config(&path)?;
    redact_json_value(&mut config);
    serde_json::to_string_pretty(&config).map_err(|e| format!("Failed to format config: {e}"))
}

#[tauri::command]
pub async fn get_openclaw_sync_status(proxy_url: Option<String>) -> Result<OpenClawStatus, String> {
    let (installed, version, detected_version_target) = check_openclaw_installed().await;
    tokio::task::spawn_blocking(move || {
        let _lock = acquire_openclaw_config_lock();
        let path = get_config_path();
        let has_backup = get_backup_path().is_some_and(|p| p.exists());

        let mut status = OpenClawStatus {
            installed,
            version,
            detected_version_target,
            is_synced: false,
            has_backup,
            current_base_url: None,
            files: vec![OPENCLAW_CONFIG_FILE.to_string()],
            configured_models: Vec::new(),
            is_active: false,
            default_model: None,
            synced_version: None,
        };

        let Some(path) = path else {
            return Ok(status);
        };
        let Ok(config) = read_openclaw_config(&path) else {
            return Ok(status);
        };

        // 读取 provider 配置
        if let Some(provider) = config
            .get("models")
            .and_then(|m| m.get("providers"))
            .and_then(|p| p.get(PROVIDER_ID))
        {
            if let Some(url) = provider.get("baseUrl").and_then(|u| u.as_str()) {
                status.current_base_url = Some(url.to_string());
                if let Some(expected) = &proxy_url {
                    status.is_synced = normalize_base_url(url) == normalize_base_url(expected);
                } else {
                    status.is_synced = true;
                }
            }

            if let Some(models) = provider.get("models").and_then(|m| m.as_array()) {
                status.configured_models = models
                    .iter()
                    .filter_map(|item| {
                        item.get("id")
                            .and_then(|id| id.as_str())
                            .map(str::to_string)
                    })
                    .collect();
            }
        }

        // 读取 active 模型与版本特征
        if let Some(primary) = config
            .get("agents")
            .and_then(|a| a.get("defaults"))
            .and_then(|d| d.get("model"))
            .and_then(|m| m.get("primary"))
            .and_then(|p| p.as_str())
        {
            if is_managed_provider(primary) {
                status.is_active = true;
                status.default_model = primary
                    .strip_prefix(&format!("{PROVIDER_ID}/"))
                    .map(str::to_string);
            }
        }

        // 判断当前配置更偏向 v1 还是 v2
        let has_v1_models = config
            .get("agents")
            .and_then(|a| a.get("defaults"))
            .and_then(|d| d.get("models"))
            .and_then(|m| m.as_object())
            .is_some_and(|map| map.keys().any(|k| is_managed_provider(k)));

        let has_v2_allow = config
            .get("agents")
            .and_then(|a| a.get("defaults"))
            .and_then(|d| d.get("modelPolicy"))
            .and_then(|p| p.get("allow"))
            .and_then(|v| v.as_array())
            .is_some_and(|arr| {
                arr.iter().any(|item| {
                    item.as_str()
                        .is_some_and(|s| s == format!("{PROVIDER_ID}/*") || is_managed_provider(s))
                })
            });

        if has_v2_allow {
            status.synced_version = Some("v2".to_string());
        } else if has_v1_models {
            status.synced_version = Some("v1".to_string());
        }

        Ok(status)
    })
    .await
    .unwrap_or_else(|_| Err("Failed to execute check".to_string()))
}

#[tauri::command]
pub async fn execute_openclaw_sync(
    proxy_url: String,
    api_key: String,
    target_version: String, // "v1" or "v2"
    models: Vec<String>,
    activate: bool,
    default_model: Option<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        sync_openclaw_provider(
            proxy_url,
            api_key,
            target_version,
            models,
            activate,
            default_model,
        )
    })
    .await
    .unwrap_or_else(|_| Err("Failed to execute sync".to_string()))
}

#[tauri::command]
pub async fn execute_openclaw_restore() -> Result<(), String> {
    tokio::task::spawn_blocking(restore_openclaw_config)
        .await
        .unwrap_or_else(|_| Err("Failed to execute restore".to_string()))
}

#[tauri::command]
pub async fn execute_openclaw_clear() -> Result<(), String> {
    tokio::task::spawn_blocking(clear_openclaw_config)
        .await
        .unwrap_or_else(|_| Err("Failed to execute clear".to_string()))
}

#[tauri::command]
pub async fn get_openclaw_config_content() -> Result<String, String> {
    tokio::task::spawn_blocking(read_openclaw_config_content)
        .await
        .unwrap_or_else(|_| Err("Failed to read config".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_openclaw_version() {
        assert_eq!(classify_openclaw_version("2026.8.1"), "v2");
        assert_eq!(classify_openclaw_version("2026.9.2"), "v2");
        assert_eq!(classify_openclaw_version("2.0.0"), "v2");
        assert_eq!(classify_openclaw_version("2026.7.749"), "v1");
        assert_eq!(classify_openclaw_version("2026.7.1"), "v1");
        assert_eq!(classify_openclaw_version("1.0.0"), "v1");
    }

    #[test]
    fn test_extract_version() {
        assert_eq!(extract_version("openclaw 2026.8.1"), "2026.8.1");
        assert_eq!(extract_version("v2026.7.749 (local)"), "2026.7.749");
    }

    #[test]
    fn test_resolve_model_specs() {
        // Gemini 3.8 / 3.0 / 2.5 系列 (1024000 上下文, 65536 输出)
        let (cw_gemini, max_gemini, _, input_gemini) = resolve_model_specs("gemini-3.8-flash-high");
        assert_eq!(cw_gemini, 1024000);
        assert_eq!(max_gemini, 65536);
        assert_eq!(input_gemini, vec!["text", "image"]);

        // Claude 系列 (256000 上下文, 65536 输出)
        let (cw_claude, max_claude, _, input_claude) =
            resolve_model_specs("claude-3-5-sonnet-latest");
        assert_eq!(cw_claude, 256000);
        assert_eq!(max_claude, 65536);
        assert_eq!(input_claude, vec!["text", "image"]);

        let (cw_claude_37, max_claude_37, r_claude_37, _) =
            resolve_model_specs("claude-3-7-sonnet-thinking");
        assert_eq!(cw_claude_37, 256000);
        assert_eq!(max_claude_37, 65536);
        assert!(r_claude_37);
    }
}
