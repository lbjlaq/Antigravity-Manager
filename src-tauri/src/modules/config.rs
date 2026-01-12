use serde_json;
use std::fs;

use super::account::get_data_dir;
use crate::models::AppConfig;

const CONFIG_FILE: &str = "gui_config.json";

/// 加载应用配置
pub fn load_app_config() -> Result<AppConfig, String> {
    let data_dir = get_data_dir()?;
    let config_path = data_dir.join(CONFIG_FILE);

    if !config_path.exists() {
        return Ok(AppConfig::new());
    }

    let content =
        fs::read_to_string(&config_path).map_err(|e| format!("读取配置文件失败: {}", e))?;

    let mut v: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("解析配置文件失败: {}", e))?;

    let mut modified = false;

    // 迁移逻辑
    if let Some(proxy) = v.get_mut("proxy") {
        let mut custom_mapping = proxy
            .get("custom_mapping")
            .and_then(|m| m.as_object()).cloned()
            .unwrap_or_default();

        // 迁移 Anthropic 映射
        if let Some(anthropic) = proxy
            .get_mut("anthropic_mapping")
            .and_then(|m| m.as_object_mut())
        {
            for (k, v) in anthropic.iter() {
                // 只有非系列字段才搬移。因为系列字段现在由 Preset 逻辑或内置表处理
                if !k.ends_with("-series")
                    && !custom_mapping.contains_key(k) {
                        custom_mapping.insert(k.clone(), v.clone());
                    }
            }
            // 移除旧字段
            proxy.as_object_mut().unwrap().remove("anthropic_mapping");
            modified = true;
        }

        // 迁移 OpenAI 映射
        if let Some(openai) = proxy
            .get_mut("openai_mapping")
            .and_then(|m| m.as_object_mut())
        {
            for (k, v) in openai.iter() {
                if !k.ends_with("-series")
                    && !custom_mapping.contains_key(k) {
                        custom_mapping.insert(k.clone(), v.clone());
                    }
            }
            // 移除旧字段
            proxy.as_object_mut().unwrap().remove("openai_mapping");
            modified = true;
        }

        if modified {
            proxy.as_object_mut().unwrap().insert(
                "custom_mapping".to_string(),
                serde_json::Value::Object(custom_mapping),
            );
        }
    }

    let config: AppConfig =
        serde_json::from_value(v).map_err(|e| format!("迁移后转换配置失败: {}", e))?;

    // 如果发生了迁移，自动保存一次以清理文件
    if modified {
        let _ = save_app_config(&config);
    }

    Ok(config)
}

/// 保存应用配置
pub fn save_app_config(config: &AppConfig) -> Result<(), String> {
    let data_dir = get_data_dir()?;
    let config_path = data_dir.join(CONFIG_FILE);

    let content =
        serde_json::to_string_pretty(config).map_err(|e| format!("序列化配置失败: {}", e))?;

    fs::write(&config_path, content).map_err(|e| format!("保存配置失败: {}", e))
}
