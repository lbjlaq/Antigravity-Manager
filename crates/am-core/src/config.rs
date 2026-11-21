use crate::types::TargetHost;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// 配额自动轮询配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaPollingConfig {
    /// 是否开启定时静默轮询 (默认 false, 环保省电)
    pub enabled: bool,
    /// 轮询间隔 (分钟, 默认 30 分钟)
    pub interval_minutes: u64,
    /// 是否仅对当前处于活跃状态的账号进行轮询
    pub active_only: bool,
}

impl Default for QuotaPollingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_minutes: 30,
            active_only: true,
        }
    }
}

/// 智能自动切换配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoSwitchConfig {
    /// 是否开启额度耗尽自动切号 (默认 false)
    pub enabled: bool,
    /// 生效的指定目标应用列表 (如 ["app", "ide"])
    pub targets: Vec<TargetHost>,
    /// 额度耗尽触发切换的阈值 (默认 <= 0.05 即 5%)
    pub trigger_exhaustion_threshold: f64,
    /// 是否在遭遇 429 限流时触发切换
    pub trigger_on_rate_limit: bool,
    /// 切换策略 (max-quota, least-used, priority, random)
    pub strategy: String,
    /// 偏好模型池 (balanced, claude, gemini)
    pub pool: String,
    /// 切换时是否发送系统桌面通知
    pub desktop_notification: bool,
}

impl Default for AutoSwitchConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            targets: vec![TargetHost::App, TargetHost::Ide],
            trigger_exhaustion_threshold: 0.0,
            trigger_on_rate_limit: true,
            strategy: "max-quota".to_string(),
            pool: "balanced".to_string(),
            desktop_notification: true,
        }
    }
}

/// 全局管理配置结构体 (~/.antigravity_tools/config.json)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    pub quota_polling: QuotaPollingConfig,
    pub auto_switch: AutoSwitchConfig,
}

impl AppConfig {
    pub fn config_path() -> Result<PathBuf> {
        let home = dirs::home_dir().context("Cannot resolve user home directory")?;
        Ok(home.join(".antigravity_tools").join("config.json"))
    }

    pub fn load() -> Self {
        if let Ok(p) = Self::config_path() {
            if p.exists() {
                if let Ok(content) = fs::read_to_string(&p) {
                    if let Ok(cfg) = serde_json::from_str(&content) {
                        return cfg;
                    }
                }
            }
        }
        Self::default()
    }

    pub fn save(&self) -> Result<()> {
        let p = Self::config_path()?;
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        fs::write(p, content)?;
        Ok(())
    }
}
