use once_cell::sync::Lazy;
use serde_json;
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use super::account::get_data_dir;
use crate::models::AppConfig;

/// Process-wide lock serializing read-modify-write updates to `gui_config.json`
/// so concurrent admin endpoints and generic `save_config` calls never interleave.
static CONFIG_WRITE_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

static CLI_SILENT_MODE_OVERRIDE: AtomicBool = AtomicBool::new(false);

fn parse_bool_env_value(val: Option<&str>) -> bool {
    val.map(|v| {
        matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
    .unwrap_or(false)
}

/// Enable silent mode for the current process (via `--silent-mode` CLI flag).
pub fn set_cli_silent_mode_override(enabled: bool) {
    CLI_SILENT_MODE_OVERRIDE.store(enabled, Ordering::Relaxed);
}

/// Returns true if silent mode is forced by `--silent-mode` or env vars (`ABV_SILENT_MODE` / `ANTIGRAVITY_SILENT_MODE`).
pub fn is_runtime_silent_mode_override() -> bool {
    CLI_SILENT_MODE_OVERRIDE.load(Ordering::Relaxed)
        || parse_bool_env_value(std::env::var("ABV_SILENT_MODE").ok().as_deref())
        || parse_bool_env_value(std::env::var("ANTIGRAVITY_SILENT_MODE").ok().as_deref())
}

/// Effective silent mode state given an already-loaded `AppConfig`.
pub fn is_silent_mode_effective(config: &AppConfig) -> bool {
    config.silent_mode || is_runtime_silent_mode_override()
}

/// Returns true if silent mode is active via runtime overrides or persisted `gui_config.json`.
pub fn is_silent_mode_enabled() -> bool {
    if is_runtime_silent_mode_override() {
        return true;
    }
    load_app_config().map(|c| c.silent_mode).unwrap_or(false)
}

const CONFIG_FILE: &str = "gui_config.json";

/// Load application configuration without acquiring `CONFIG_WRITE_LOCK`.
/// Callers performing read-modify-write must hold `CONFIG_WRITE_LOCK`.
fn load_app_config_unlocked(auto_persist_on_change: bool) -> Result<AppConfig, String> {
    let data_dir = get_data_dir()?;
    let config_path = data_dir.join(CONFIG_FILE);

    if !config_path.exists() {
        let config = AppConfig::new();
        // [FIX #1460] Persist initial config to prevent new API Key on every refresh
        if auto_persist_on_change {
            if let Err(e) = save_app_config_unlocked(&config) {
                tracing::warn!(
                    "Failed to persist initial config to {}: {}. An in-memory config will be used; the next start may generate a new API key if the file is still missing.",
                    config_path.display(),
                    e
                );
            }
        }
        return Ok(config);
    }

    let content = fs::read_to_string(&config_path)
        .map_err(|e| format!("failed_to_read_config_file: {}", e))?;

    let (config, modified) = parse_and_migrate_config(&content)?;

    // If migration occurred or empty config was generated, auto-save once to clean up the file
    if modified && auto_persist_on_change {
        if let Err(e) = save_app_config_unlocked(&config) {
            tracing::warn!(
                "Failed to persist healed or migrated config to {}: {}. An in-memory config will be used; the next start may generate a new API key if the file is still empty.",
                config_path.display(),
                e
            );
        }
    }

    Ok(config)
}

/// Load application configuration
pub fn load_app_config() -> Result<AppConfig, String> {
    let data_dir = get_data_dir()?;
    let config_path = data_dir.join(CONFIG_FILE);

    if !config_path.exists() {
        let _guard = CONFIG_WRITE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        return load_app_config_unlocked(true);
    }

    let content = fs::read_to_string(&config_path)
        .map_err(|e| format!("failed_to_read_config_file: {}", e))?;

    let (config, modified) = parse_and_migrate_config(&content)?;

    if modified {
        let _guard = CONFIG_WRITE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Re-read under lock so migration write never clobbers a concurrent mutation.
        return load_app_config_unlocked(true);
    }

    Ok(config)
}

/// Parse, migrate and normalize raw configuration JSON string.
/// Returns (AppConfig, modified_flag).
pub fn parse_and_migrate_config(content: &str) -> Result<(AppConfig, bool), String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        tracing::warn!("Configuration content is empty, generating default configuration");
        return Ok((AppConfig::new(), true));
    }

    let mut v: serde_json::Value =
        serde_json::from_str(trimmed).map_err(|e| format!("failed_to_parse_config_file: {}", e))?;

    let mut modified = false;

    // Migration logic
    if let Some(proxy) = v.get_mut("proxy") {
        // [FIX #1738] Enhanced type checking for custom_mapping
        // Ensures the field is always parsed as an object, preventing type mismatch errors
        let mut custom_mapping = match proxy.get("custom_mapping") {
            Some(m) if m.is_object() => m.as_object().unwrap().clone(),
            Some(m) => {
                // If custom_mapping is not an object type (e.g., string), log warning and reset to empty
                tracing::warn!(
                    "Invalid custom_mapping type (expected object, got {:?}), resetting to empty",
                    m
                );
                serde_json::Map::new()
            }
            None => serde_json::Map::new(),
        };

        // Migrate Anthropic mapping
        if let Some(anthropic) = proxy
            .get_mut("anthropic_mapping")
            .and_then(|m| m.as_object_mut())
        {
            for (k, v) in anthropic.iter() {
                // Only move non-series fields, as series fields are now handled by Preset logic or builtin tables
                if !k.ends_with("-series") {
                    if !custom_mapping.contains_key(k) {
                        custom_mapping.insert(k.clone(), v.clone());
                    }
                }
            }
            // Remove old field
            proxy.as_object_mut().unwrap().remove("anthropic_mapping");
            modified = true;
        }

        // Migrate OpenAI mapping
        if let Some(openai) = proxy
            .get_mut("openai_mapping")
            .and_then(|m| m.as_object_mut())
        {
            for (k, v) in openai.iter() {
                if !k.ends_with("-series") {
                    if !custom_mapping.contains_key(k) {
                        custom_mapping.insert(k.clone(), v.clone());
                    }
                }
            }
            // Remove old field
            proxy.as_object_mut().unwrap().remove("openai_mapping");
            modified = true;
        }

        // 旧版启动时注入的精确映射会挡住 3.x Flash 裸模型的档位路由。
        // 只删除仍等于当时默认值的条目；用户改过的目标保留。
        for (k, old_default) in [
            ("gemini-3.6-flash", "gemini-3.6-flash-tiered"),
            ("gemini-3.7-flash", "gemini-3.7-flash-tiered"),
            ("gemini-3.8-flash", "gemini-3.8-flash-tiered"),
        ] {
            if custom_mapping.get(k).and_then(|v| v.as_str()) == Some(old_default) {
                custom_mapping.remove(k);
                modified = true;
            }
        }

        // 清理已废弃的旧内置规则 gemini-3.x-flash（已由纯数据驱动 DynamicTierRouter 取代）
        if custom_mapping.contains_key("gemini-3.x-flash") {
            custom_mapping.remove("gemini-3.x-flash");
            modified = true;
        }

        // 检查 builtin_mapping_version 并进行内置自适应暗路由规则版本迁移 (防复活保障)
        let builtin_version = proxy
            .get("builtin_mapping_version")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let mut custom_mapping_expose = match proxy.get("custom_mapping_expose") {
            Some(m) if m.is_object() => m.as_object().unwrap().clone(),
            _ => serde_json::Map::new(),
        };

        if builtin_version < 1 {
            // 首次升级至版本 1：注入 3 条自适应暗路由规则（默认 expose: false）
            let builtins = [
                ("claude-fable-*", "auto:claude-opus", false),
                ("claude-opus-4-7*", "auto:claude-opus-4-7", false),
                ("claude-haiku-*", "auto:gemini-flash-low", false),
            ];
            for (pattern, target, expose) in builtins {
                if !custom_mapping.contains_key(pattern) {
                    custom_mapping.insert(
                        pattern.to_string(),
                        serde_json::Value::String(target.to_string()),
                    );
                }
                if !custom_mapping_expose.contains_key(pattern) {
                    custom_mapping_expose
                        .insert(pattern.to_string(), serde_json::Value::Bool(expose));
                }
            }
            // 存量自定义映射如果未显式声明 expose，通配与自适应规则默认 false，精确自定义规则默认 true
            for (k, v) in custom_mapping.iter() {
                if !custom_mapping_expose.contains_key(k) {
                    let is_auto = v.as_str().map_or(false, |s| s.starts_with("auto:"));
                    let default_exp = !k.contains('*') && !is_auto;
                    custom_mapping_expose.insert(k.clone(), serde_json::Value::Bool(default_exp));
                }
            }
            proxy.as_object_mut().unwrap().insert(
                "builtin_mapping_version".to_string(),
                serde_json::Value::Number(1.into()),
            );
            proxy.as_object_mut().unwrap().insert(
                "custom_mapping_expose".to_string(),
                serde_json::Value::Object(custom_mapping_expose.clone()),
            );
            modified = true;
        }

        if builtin_version < 2 {
            // 升级至版本 2：注入精确辅助模型映射（默认 expose: false；用户已有该 key 时不覆盖，删除后不复活）
            let key = "gemini-3.1-flash-lite-preview";
            if !custom_mapping.contains_key(key) {
                custom_mapping.insert(
                    key.to_string(),
                    serde_json::Value::String("auto:background".to_string()),
                );
            }
            if !custom_mapping_expose.contains_key(key) {
                custom_mapping_expose.insert(key.to_string(), serde_json::Value::Bool(false));
            }
            proxy.as_object_mut().unwrap().insert(
                "builtin_mapping_version".to_string(),
                serde_json::Value::Number(2.into()),
            );
            proxy.as_object_mut().unwrap().insert(
                "custom_mapping_expose".to_string(),
                serde_json::Value::Object(custom_mapping_expose.clone()),
            );
            modified = true;
        }

        // 旧出厂默认 flash_high = 16384 或 32768。只在首次启动且模式为 default 时改成官方 -1。
        if let Some(tb) = proxy
            .get_mut("thinking_budget")
            .and_then(|t| t.as_object_mut())
        {
            let migrated_32k = tb
                .get("thinking_budget_32k_legacy_migrated")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if !migrated_32k {
                let flash_mode_is_default = tb
                    .get("flash_mode")
                    .and_then(|v| v.as_str())
                    .map(|s| s.eq_ignore_ascii_case("default"))
                    .unwrap_or(false);
                let pro_mode_is_default = tb
                    .get("pro_mode")
                    .and_then(|v| v.as_str())
                    .map(|s| s.eq_ignore_ascii_case("default"))
                    .unwrap_or(false);

                if flash_mode_is_default {
                    if let Some(val) = tb.get("flash_high").and_then(|v| v.as_i64()) {
                        if val == 16384 || val == 32768 {
                            tb.insert("flash_high".to_string(), serde_json::Value::from(-1));
                        }
                    }
                }

                if pro_mode_is_default {
                    if let Some(val) = tb.get("pro_high").and_then(|v| v.as_i64()) {
                        if val == 32768 {
                            tb.insert("pro_high".to_string(), serde_json::Value::from(-1));
                        }
                    }
                }

                tb.insert(
                    "thinking_budget_32k_legacy_migrated".to_string(),
                    serde_json::Value::from(true),
                );
                modified = true;
            }

            let migrated = tb
                .get("flash_high_legacy_migrated")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if !migrated {
                if tb.get("flash_high").and_then(|v| v.as_i64()) == Some(16384) {
                    tb.insert("flash_high".to_string(), serde_json::Value::from(-1));
                }
                tb.insert(
                    "flash_high_legacy_migrated".to_string(),
                    serde_json::Value::from(true),
                );
                modified = true;
            }
        }
        if let Some(log_retention) = proxy
            .get_mut("log_retention")
            .and_then(|m| m.as_object_mut())
        {
            if let Some(max_disk_mb) = log_retention.get("max_disk_mb").and_then(|v| v.as_u64()) {
                if max_disk_mb == 0 {
                    log_retention.insert("max_disk_mb".to_string(), serde_json::Value::from(1024));
                    modified = true;
                }
            }
        }

        // Migrate legacy User-Agent in user_agent_override and saved_user_agent to >= 4.3.0
        // to prevent upstream Google 404/429 model rejections
        for ua_field in ["user_agent_override", "saved_user_agent"] {
            if let Some(ua_val) = proxy.get(ua_field).and_then(|v| v.as_str()) {
                let sanitized = crate::constants::sanitize_egress_user_agent(ua_val);
                if sanitized != ua_val {
                    tracing::info!(
                        field = %ua_field,
                        old = %ua_val,
                        new = %sanitized,
                        "Migrating legacy User-Agent config to supported stable floor"
                    );
                    proxy
                        .as_object_mut()
                        .unwrap()
                        .insert(ua_field.to_string(), serde_json::Value::String(sanitized));
                    modified = true;
                }
            }
        }

        // [2026-10-29] 思考控制权已收敛为「网关 + 自适应」：
        // 旧配置中的 client（客户端直控）统一迁移回 gateway，避免 UI 显示已不存在的模式。
        if let Some(tb) = proxy
            .get_mut("thinking_budget")
            .and_then(|t| t.as_object_mut())
        {
            if tb.get("control_source").and_then(|v| v.as_str()) == Some("client") {
                tb.insert(
                    "control_source".to_string(),
                    serde_json::Value::String("gateway".to_string()),
                );
                tracing::info!(
                    "Migrating thinking_budget.control_source: client -> gateway (client control mode removed)"
                );
                modified = true;
            }
        }

        // 自动迁移/初始化 prompt_rewrite 配置：若旧配置中缺失该字段，则注入默认开启并搭载官方推荐预设
        if let Some(proxy_obj) = proxy.as_object_mut() {
            if !proxy_obj.contains_key("prompt_rewrite") {
                let default_prompt_rewrite =
                    serde_json::to_value(crate::proxy::config::PromptRewriteConfig::default())
                        .unwrap_or(serde_json::Value::Null);
                if !default_prompt_rewrite.is_null() {
                    proxy_obj.insert("prompt_rewrite".to_string(), default_prompt_rewrite);
                    tracing::info!("Initializing default prompt_rewrite config (enabled with recommended presets)");
                    modified = true;
                }
            }
        }

        if modified {
            proxy.as_object_mut().unwrap().insert(
                "custom_mapping".to_string(),
                serde_json::Value::Object(custom_mapping),
            );
        }
    }

    let config: AppConfig = serde_json::from_value(v)
        .map_err(|e| format!("failed_to_convert_config_after_migration: {}", e))?;

    Ok((config, modified))
}

fn save_app_config_unlocked(config: &AppConfig) -> Result<(), String> {
    let data_dir = get_data_dir()?;
    let config_path = data_dir.join(CONFIG_FILE);

    let content = serde_json::to_string_pretty(config)
        .map_err(|e| format!("failed_to_serialize_config: {}", e))?;

    crate::utils::fs::write_atomic(&config_path, content.as_bytes())
        .map_err(|e| format!("failed_to_save_config: {}", e))
}

/// Save application configuration (atomic write, serialized by `CONFIG_WRITE_LOCK`)
pub fn save_app_config(config: &AppConfig) -> Result<(), String> {
    let _guard = CONFIG_WRITE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    save_app_config_unlocked(config)
}

/// Atomically read-modify-write `gui_config.json` under `CONFIG_WRITE_LOCK`.
pub fn update_app_config<F>(mutator: F) -> Result<AppConfig, String>
where
    F: FnOnce(&mut AppConfig),
{
    let _guard = CONFIG_WRITE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut cfg = load_app_config_unlocked(false)?;
    mutator(&mut cfg);
    save_app_config_unlocked(&cfg)?;
    Ok(cfg)
}

/// Atomically read-modify-write `gui_config.json` under `CONFIG_WRITE_LOCK`,
/// returning an error from `mutator` without writing if validation fails.
pub fn try_update_app_config<F, T>(mutator: F) -> Result<(AppConfig, T), String>
where
    F: FnOnce(&mut AppConfig) -> Result<T, String>,
{
    let _guard = CONFIG_WRITE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut cfg = load_app_config_unlocked(false)?;
    let out = mutator(&mut cfg)?;
    save_app_config_unlocked(&cfg)?;
    Ok((cfg, out))
}

/// Atomically read-modify-write `gui_config.json` under `CONFIG_WRITE_LOCK`,
/// skipping disk write when `mutator` returns `false`.
pub fn update_app_config_if<F>(mutator: F) -> Result<(AppConfig, bool), String>
where
    F: FnOnce(&mut AppConfig) -> bool,
{
    let _guard = CONFIG_WRITE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut cfg = load_app_config_unlocked(false)?;
    let changed = mutator(&mut cfg);
    if changed {
        save_app_config_unlocked(&cfg)?;
    }
    Ok((cfg, changed))
}

/// Persist a generic full-`AppConfig` payload (from `save_config` / `POST /api/config`)
/// while preserving fields owned exclusively by dedicated endpoints or backend config
/// (`preferred_account_id`, `security_monitor`, `image_scheduler`, `proxy_pool.account_bindings`)
/// from the latest on-disk `gui_config.json` so stale frontend snapshots cannot revert them.
pub fn save_generic_app_config(mut incoming: AppConfig) -> Result<AppConfig, String> {
    let _guard = CONFIG_WRITE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let data_dir = get_data_dir()?;
    let config_path = data_dir.join(CONFIG_FILE);
    if config_path.exists() {
        if let Ok(disk) = load_app_config_unlocked(false) {
            incoming.proxy.preferred_account_id = disk.proxy.preferred_account_id;
            incoming.proxy.security_monitor = disk.proxy.security_monitor;
            incoming.proxy.image_scheduler = disk.proxy.image_scheduler;
            incoming.proxy.proxy_pool.account_bindings = disk.proxy.proxy_pool.account_bindings;
        }
    }

    save_app_config_unlocked(&incoming)?;
    Ok(incoming)
}

#[cfg(test)]
pub(crate) struct TestDataDirGuard {
    _env_lock: std::sync::MutexGuard<'static, ()>,
    _config_lock: std::sync::MutexGuard<'static, ()>,
    prev_env: Option<String>,
    pub dir: std::path::PathBuf,
}

#[cfg(test)]
impl TestDataDirGuard {
    pub fn new(prefix: &str) -> Self {
        // Acquire TEST_CONFIG_LOCK before SHARED_TEST_ENV_LOCK (same order as commands::proxy::tests)
        // so tests writing gui_config.json via ProxyPoolManager::persist_bindings() cannot race ABV_DATA_DIR.
        let config_lock = crate::proxy::config::TEST_CONFIG_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let env_lock = crate::modules::account::SHARED_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let prev_env = std::env::var("ABV_DATA_DIR").ok();
        let dir = std::env::temp_dir().join(format!(
            "ag_test_{}_{}",
            prefix,
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).expect("create temp test data dir");
        std::env::set_var("ABV_DATA_DIR", &dir);
        Self {
            _env_lock: env_lock,
            _config_lock: config_lock,
            prev_env,
            dir,
        }
    }
}

#[cfg(test)]
impl Drop for TestDataDirGuard {
    fn drop(&mut self) {
        if let Some(ref prev) = self.prev_env {
            std::env::set_var("ABV_DATA_DIR", prev);
        } else {
            std::env::remove_var("ABV_DATA_DIR");
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_and_migrate_empty_or_whitespace_config() {
        // [FIX #3548] Ensure empty or whitespace-only config file does not trigger EOF error in headless mode
        let (empty_cfg, modified_empty) =
            parse_and_migrate_config("").expect("empty config should parse to default");
        assert!(modified_empty);
        assert!(!empty_cfg.proxy.api_key.is_empty());

        let (ws_cfg, modified_ws) = parse_and_migrate_config("   \n\t  \r\n  ")
            .expect("whitespace config should parse to default");
        assert!(modified_ws);
        assert!(!ws_cfg.proxy.api_key.is_empty());
    }

    #[test]
    fn test_parse_and_migrate_valid_and_invalid_json() {
        let invalid = parse_and_migrate_config("{ invalid_json: ");
        assert!(invalid.is_err());
        assert!(invalid.unwrap_err().contains("failed_to_parse_config_file"));

        let default_json = serde_json::to_string(&AppConfig::new()).unwrap();
        let (valid_cfg, _) =
            parse_and_migrate_config(&default_json).expect("valid config should parse");
        assert!(!valid_cfg.proxy.api_key.is_empty());
    }

    #[test]
    fn test_builtin_mapping_v1_to_v2_migration() {
        const KEY: &str = "gemini-3.1-flash-lite-preview";
        let migrate = |ver: u32, custom_val: Option<&str>| {
            let mut cfg = AppConfig::new();
            cfg.proxy.builtin_mapping_version = ver;
            cfg.proxy.custom_mapping.remove(KEY);
            cfg.proxy.custom_mapping_expose.remove(KEY);
            if let Some(v) = custom_val {
                cfg.proxy
                    .custom_mapping
                    .insert(KEY.to_string(), v.to_string());
            }
            parse_and_migrate_config(&serde_json::to_string(&cfg).unwrap()).unwrap()
        };

        // 1. v1 missing key -> migrated to v2 with auto:background + expose=false
        let (migrated, modified) = migrate(1, None);
        assert!(modified);
        assert_eq!(migrated.proxy.builtin_mapping_version, 2);
        assert_eq!(
            migrated.proxy.custom_mapping.get(KEY).map(String::as_str),
            Some("auto:background")
        );
        assert_eq!(
            migrated.proxy.custom_mapping_expose.get(KEY).copied(),
            Some(false)
        );

        // 2. v1 user-customized entry preserved
        let (custom, _) = migrate(1, Some("gemini-3.5-flash-lite"));
        assert_eq!(
            custom.proxy.custom_mapping.get(KEY).map(String::as_str),
            Some("gemini-3.5-flash-lite")
        );

        // 3. v2 deleted entry must not resurrect
        let (v2_deleted, _) = migrate(2, None);
        assert!(!v2_deleted.proxy.custom_mapping.contains_key(KEY));
    }

    #[test]
    fn test_parse_bool_env_value_and_effective_silent_mode() {
        for truthy in ["1", "true", "TRUE", " yes ", "On"] {
            assert!(parse_bool_env_value(Some(truthy)));
        }
        for falsy in ["", "0", "false", "no", "off", "random"] {
            assert!(!parse_bool_env_value(Some(falsy)));
        }
        assert!(!parse_bool_env_value(None));

        let mut cfg = AppConfig::new();
        cfg.silent_mode = true;
        assert!(is_silent_mode_effective(&cfg));
    }

    #[test]
    fn test_save_generic_app_config_preserves_dedicated_fields() {
        let _guard = TestDataDirGuard::new("preserve_dedicated");

        // 1. Seed disk with dedicated-endpoint-owned fields set to authoritative values
        let mut on_disk = AppConfig::new();
        on_disk.proxy.preferred_account_id = None;
        on_disk.proxy.security_monitor.blacklist.enabled = true;
        on_disk.proxy.security_monitor.blacklist.block_message = "authoritative-block".to_string();
        on_disk.proxy.image_scheduler.per_account_concurrency = 4;
        on_disk
            .proxy
            .proxy_pool
            .account_bindings
            .insert("acc-1".to_string(), "proxy-a".to_string());
        save_app_config(&on_disk).expect("seed disk config");

        // 2. Simulate a stale frontend AppConfig snapshot attempting to save an unrelated theme edit
        let mut stale = AppConfig::new();
        stale.theme = "dark".to_string();
        stale.proxy.preferred_account_id = Some("stale-account-id".to_string());
        stale.proxy.security_monitor.blacklist.enabled = false;
        stale.proxy.security_monitor.blacklist.block_message = "stale".to_string();
        stale.proxy.image_scheduler.per_account_concurrency = 1;
        stale.proxy.proxy_pool.account_bindings.clear();

        let saved = save_generic_app_config(stale).expect("save_generic_app_config");
        let reloaded = load_app_config().expect("reload config");

        // Unrelated edit persisted
        assert_eq!(saved.theme, "dark");
        assert_eq!(reloaded.theme, "dark");
        // Dedicated-endpoint fields preserved from disk
        assert_eq!(reloaded.proxy.preferred_account_id, None);
        assert!(reloaded.proxy.security_monitor.blacklist.enabled);
        assert_eq!(
            reloaded.proxy.security_monitor.blacklist.block_message,
            "authoritative-block"
        );
        assert_eq!(reloaded.proxy.image_scheduler.per_account_concurrency, 4);
        assert_eq!(
            reloaded
                .proxy
                .proxy_pool
                .account_bindings
                .get("acc-1")
                .map(String::as_str),
            Some("proxy-a")
        );
    }

    #[test]
    fn test_update_app_config_if_skips_write_when_unchanged() {
        let guard = TestDataDirGuard::new("update_if_noop");
        let mut initial = AppConfig::new();
        initial.proxy.enable_logging = true;
        initial.theme = "light".to_string();
        save_app_config(&initial).expect("seed disk config");

        let config_path = guard.dir.join(CONFIG_FILE);
        let bytes_before = std::fs::read(&config_path).expect("read seeded config bytes");
        let (_, changed_noop) = update_app_config_if(|cfg| {
            // Mutate an unrelated field while returning false: if the guard were bypassed,
            // raw bytes on disk and reloaded theme would diverge.
            cfg.theme = "unsaved-mutation".to_string();
            false
        })
        .expect("noop update");
        assert!(!changed_noop);
        assert_eq!(
            std::fs::read(&config_path).expect("read config bytes after noop"),
            bytes_before
        );
        assert_eq!(load_app_config().unwrap().theme, "light");

        let (updated, changed_real) = update_app_config_if(|cfg| {
            if cfg.proxy.enable_logging {
                cfg.proxy.enable_logging = false;
                true
            } else {
                false
            }
        })
        .expect("real update");
        assert!(changed_real);
        assert!(!updated.proxy.enable_logging);
        assert!(config_path.exists());
        assert!(!load_app_config().unwrap().proxy.enable_logging);
    }
}
