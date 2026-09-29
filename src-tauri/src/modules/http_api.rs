//! HTTP API 设置模块
//!
//! [阶段 5] 原 19527 端口的独立 HTTP API 服务（账号切换等）已被 8045 端口的
//! `/api` 管理面完全吞并（见 lib.rs 的 "[PHASE 1]" 注释），`spawn_server` /
//! `start_server` / `ApiState` 等服务体因无任何调用方而移除。
//! 仅保留设置文件的读写实现，供 `/api/system/http-api/settings` 与同名 Tauri 命令共用。

use serde::{Deserialize, Serialize};

/// Default port for HTTP API server (legacy; kept for backward-compatible settings files)
pub const DEFAULT_PORT: u16 = 19527;

// ============================================================================
// Settings
// ============================================================================

/// HTTP API Settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpApiSettings {
    /// Whether to enable HTTP API service
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Listening port
    #[serde(default = "default_port")]
    pub port: u16,
}

fn default_enabled() -> bool {
    true
}

fn default_port() -> u16 {
    DEFAULT_PORT
}

impl Default for HttpApiSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            port: DEFAULT_PORT,
        }
    }
}

/// Load HTTP API settings
pub fn load_settings() -> Result<HttpApiSettings, String> {
    let data_dir = crate::modules::account::get_data_dir()
        .map_err(|e| format!("Failed to get data dir: {}", e))?;
    let settings_path = data_dir.join("http_api_settings.json");

    if !settings_path.exists() {
        return Ok(HttpApiSettings::default());
    }

    let content = std::fs::read_to_string(&settings_path)
        .map_err(|e| format!("Failed to read settings file: {}", e))?;

    serde_json::from_str(&content).map_err(|e| format!("Failed to parse settings: {}", e))
}

/// Save HTTP API settings
pub fn save_settings(settings: &HttpApiSettings) -> Result<(), String> {
    let data_dir = crate::modules::account::get_data_dir()
        .map_err(|e| format!("Failed to get data dir: {}", e))?;
    let settings_path = data_dir.join("http_api_settings.json");

    let content = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;

    std::fs::write(&settings_path, content)
        .map_err(|e| format!("Failed to write settings file: {}", e))
}
