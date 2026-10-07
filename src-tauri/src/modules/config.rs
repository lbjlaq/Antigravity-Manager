use serde_json;
use std::fs;

use super::account::get_data_dir;
use crate::models::AppConfig;

const CONFIG_FILE: &str = "gui_config.json";

/// Load application configuration
pub fn load_app_config() -> Result<AppConfig, String> {
    let data_dir = get_data_dir()?;
    let config_path = data_dir.join(CONFIG_FILE);

    if !config_path.exists() {
        let config = AppConfig::new();
        // [FIX #1460] Persist initial config to prevent new API Key on every refresh
        if let Err(e) = save_app_config(&config) {
            tracing::warn!(
                "Failed to persist initial config to {}: {}. An in-memory config will be used; the next start may generate a new API key if the file is still missing.",
                config_path.display(),
                e
            );
        }
        return Ok(config);
    }

    let content = fs::read_to_string(&config_path)
        .map_err(|e| format!("failed_to_read_config_file: {}", e))?;

    let (config, modified) = parse_and_migrate_config(&content)?;

    // If migration occurred or empty config was generated, auto-save once to clean up the file
    if modified {
        if let Err(e) = save_app_config(&config) {
            tracing::warn!(
                "Failed to persist healed or migrated config to {}: {}. An in-memory config will be used; the next start may generate a new API key if the file is still empty.",
                config_path.display(),
                e
            );
        }
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

/// Save application configuration (atomic write)
pub fn save_app_config(config: &AppConfig) -> Result<(), String> {
    let data_dir = get_data_dir()?;
    let config_path = data_dir.join(CONFIG_FILE);

    let content = serde_json::to_string_pretty(config)
        .map_err(|e| format!("failed_to_serialize_config: {}", e))?;

    crate::utils::fs::write_atomic(&config_path, content.as_bytes())
        .map_err(|e| format!("failed_to_save_config: {}", e))
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
}
