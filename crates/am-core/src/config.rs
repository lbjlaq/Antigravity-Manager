use crate::types::TargetHost;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

fn default_poll_interval() -> u64 {
    30
}

fn default_true() -> bool {
    true
}

fn default_targets() -> Vec<TargetHost> {
    vec![TargetHost::App, TargetHost::Ide]
}

fn default_exhaustion_threshold() -> f64 {
    5.0
}

fn default_strategy() -> String {
    "max-quota".to_string()
}

fn default_pool() -> String {
    "balanced".to_string()
}

/// 配额自动轮询配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaPollingConfig {
    /// 是否开启定时静默轮询 (默认 false, 环保省电)
    #[serde(default)]
    pub enabled: bool,
    /// 轮询间隔 (分钟, 默认 30 分钟)
    #[serde(default = "default_poll_interval")]
    pub interval_minutes: u64,
    /// 是否仅对当前处于活跃状态的账号进行轮询
    #[serde(default = "default_true")]
    pub active_only: bool,
}

impl Default for QuotaPollingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_minutes: default_poll_interval(),
            active_only: true,
        }
    }
}

/// 智能自动切换配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoSwitchConfig {
    /// 是否开启额度耗尽自动切号 (默认 false)
    #[serde(default)]
    pub enabled: bool,
    /// 生效的指定目标应用列表 (如 ["app", "ide"])
    #[serde(default = "default_targets")]
    pub targets: Vec<TargetHost>,
    /// 额度耗尽触发切换的百分比阈值 (0.0 ~ 100.0, 默认 <= 5.0 即 5%；自动兼容 0~1 旧小数格式)
    #[serde(default = "default_exhaustion_threshold")]
    pub trigger_exhaustion_threshold: f64,
    /// 是否在遭遇 429 限流时触发切换
    #[serde(default = "default_true")]
    pub trigger_on_rate_limit: bool,
    /// 切换策略 (max-quota, expiring-soon, tier-first, least-used, priority, random)
    #[serde(default = "default_strategy")]
    pub strategy: String,
    /// 偏好模型池 (balanced, claude, gemini)
    #[serde(default = "default_pool")]
    pub pool: String,
    /// 切换时是否发送系统桌面通知
    #[serde(default = "default_true")]
    pub desktop_notification: bool,
}

impl AutoSwitchConfig {
    /// 归一化获取百分制阈值 (0.0 ~ 100.0)，兼容历史 `0.05` / `0.1` 小数配置
    pub fn effective_exhaustion_threshold_pct(&self) -> f64 {
        let raw = self.trigger_exhaustion_threshold;
        if raw > 0.0 && raw < 1.0 {
            raw * 100.0
        } else {
            raw.clamp(0.0, 100.0)
        }
    }
}

impl Default for AutoSwitchConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            targets: default_targets(),
            trigger_exhaustion_threshold: default_exhaustion_threshold(),
            trigger_on_rate_limit: true,
            strategy: default_strategy(),
            pool: default_pool(),
            desktop_notification: true,
        }
    }
}

/// 自定义宿主路径配置 (允许用户覆写默认配置目录)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CustomPathsConfig {
    #[serde(default)]
    pub app: Option<String>,
    #[serde(default)]
    pub ide: Option<String>,
    #[serde(default)]
    pub plugin: Option<String>,
    #[serde(default)]
    pub cli: Option<String>,
}

fn default_tray_click_action() -> String {
    "popover".to_string()
}

fn default_lang() -> String {
    "zh".to_string()
}

fn default_update_interval_hours() -> u64 {
    24
}

/// 应用在线更新与自动检查偏好
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateConfig {
    /// 启动后静默自动检查更新 (默认 true)
    #[serde(default = "default_true")]
    pub auto_check: bool,
    /// 自动检查最小冷却间隔（小时，默认 24 小时）
    #[serde(default = "default_update_interval_hours")]
    pub check_interval_hours: u64,
    /// 最近一次自动检查 UNIX 时间戳（秒）
    #[serde(default)]
    pub last_check_time: u64,
    /// 用户主动跳过提醒的版本号（如 "0.0.2"）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skipped_version: Option<String>,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            auto_check: true,
            check_interval_hours: default_update_interval_hours(),
            last_check_time: 0,
            skipped_version: None,
        }
    }
}

/// 系统菜单栏 / 托盘悬浮窗与启动生命周期配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrayConfig {
    /// 是否启用系统菜单栏/托盘图标 (默认 true)
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 左键点击托盘图标行为: "popover" (悬浮小窗) | "main" (打开/隐藏主窗口) | "menu" (弹出原生菜单)
    #[serde(default = "default_tray_click_action")]
    pub left_click_action: String,
    /// 点击主窗口关闭按钮时是否隐藏到后台托盘而非退出程序 (默认 true)
    #[serde(default = "default_true")]
    pub close_to_tray: bool,
    /// 是否开启系统开机自启动 (默认 false)
    #[serde(default)]
    pub auto_launch: bool,
    /// 免打扰启动（留在托盘）：开机自启时不弹出主窗口；手动打开仍显示主窗口；系统无托盘时自动回退显示窗口 (默认 true)
    #[serde(default = "default_true")]
    pub quiet_autostart: bool,
    /// 轻量模式：隐藏到托盘或免打扰自启时直接销毁释放前端 WebView 进程，唤醒时按需重建 (默认 false)
    #[serde(default)]
    pub lightweight_mode: bool,
    /// 托盘原生菜单当前语言代码 (zh, zh-TW, en, ja, ko, ru, es, pt, tr, vi, ar, my)
    #[serde(default = "default_lang")]
    pub language: String,
}

impl Default for TrayConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            left_click_action: default_tray_click_action(),
            close_to_tray: true,
            auto_launch: false,
            quiet_autostart: true,
            lightweight_mode: false,
            language: default_lang(),
        }
    }
}

/// GUI 界面状态自动记忆（持久化至 config.json，防止清理 WebView 缓存或换包后丢失）
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UiStateConfig {
    /// 上次停留的主窗口页面 ("dashboard" | "accounts" | "stats" | "settings")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_tab: Option<String>,
    /// 首页仪表盘上次选中的宿主 ("app" | "ide" | "plugin" | "cli")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dashboard_host: Option<String>,
    /// 首页仪表盘备用账号池选号/排序策略 ("max-quota" | "expiring-soon" | "tier-first" | "least-used")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dashboard_pick_strategy: Option<String>,
    /// 首页仪表盘模型池偏好 ("balanced" | "claude" | "gemini")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dashboard_pool_preference: Option<String>,
    /// 账号管理页排序方式 ("health_default" | "tier_desc" | "name_asc" | "custom")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accounts_sort_mode: Option<String>,
    /// 账号管理页视图模式 ("table" | "card")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accounts_view_mode: Option<String>,
    /// 界面与字体整体等比例缩放百分比（85..=150，默认 120）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_zoom: Option<u16>,
}

impl UiStateConfig {
    pub fn is_empty(&self) -> bool {
        self.last_tab.is_none()
            && self.dashboard_host.is_none()
            && self.dashboard_pick_strategy.is_none()
            && self.dashboard_pool_preference.is_none()
            && self.accounts_sort_mode.is_none()
            && self.accounts_view_mode.is_none()
            && self.ui_zoom.is_none()
    }

    pub fn merge_from(&mut self, patch: UiStateConfig) {
        if patch.last_tab.is_some() {
            self.last_tab = patch.last_tab;
        }
        if patch.dashboard_host.is_some() {
            self.dashboard_host = patch.dashboard_host;
        }
        if patch.dashboard_pick_strategy.is_some() {
            self.dashboard_pick_strategy = patch.dashboard_pick_strategy;
        }
        if patch.dashboard_pool_preference.is_some() {
            self.dashboard_pool_preference = patch.dashboard_pool_preference;
        }
        if patch.accounts_sort_mode.is_some() {
            self.accounts_sort_mode = patch.accounts_sort_mode;
        }
        if patch.accounts_view_mode.is_some() {
            self.accounts_view_mode = patch.accounts_view_mode;
        }
        if patch.ui_zoom.is_some() {
            self.ui_zoom = patch.ui_zoom;
        }
    }
}

/// 全局管理配置结构体 (~/.antigravity_tools/config.json)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub quota_polling: QuotaPollingConfig,
    #[serde(default)]
    pub auto_switch: AutoSwitchConfig,
    #[serde(default)]
    pub custom_paths: CustomPathsConfig,
    #[serde(default)]
    pub tray: TrayConfig,
    #[serde(default)]
    pub ui_state: UiStateConfig,
    #[serde(default)]
    pub update: UpdateConfig,
}

static CONFIG_IO_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();

fn config_io_lock() -> &'static std::sync::Mutex<()> {
    CONFIG_IO_LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

impl AppConfig {
    pub fn config_path() -> Result<PathBuf> {
        let home = dirs::home_dir().context("Cannot resolve user home directory")?;
        Ok(home.join(".antigravity_tools").join("config.json"))
    }

    fn load_unlocked() -> Self {
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

    fn save_unlocked(&self) -> Result<()> {
        let p = Self::config_path()?;
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        crate::storage::atomic_write_secure(&p, &content)?;
        Ok(())
    }

    pub fn load() -> Self {
        let _guard = config_io_lock().lock().unwrap_or_else(|e| e.into_inner());
        Self::load_unlocked()
    }

    pub fn save(&self) -> Result<()> {
        let _guard = config_io_lock().lock().unwrap_or_else(|e| e.into_inner());
        self.save_unlocked()
    }

    /// 进程内串行化原子读-改-写，消除 `patch_ui_state` / `save_app_config` / `toggle_auto_launch` 并发覆盖竞态
    pub fn update<F>(mutator: F) -> Result<Self>
    where
        F: FnOnce(&mut Self),
    {
        let _guard = config_io_lock().lock().unwrap_or_else(|e| e.into_inner());
        let mut cfg = Self::load_unlocked();
        mutator(&mut cfg);
        cfg.save_unlocked()?;
        Ok(cfg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_zoom_defaults_to_none_and_merges_roundtrip() {
        let mut state = UiStateConfig::default();
        assert!(state.is_empty());
        assert_eq!(state.ui_zoom, None);

        state.merge_from(UiStateConfig {
            ui_zoom: Some(120),
            ..Default::default()
        });
        assert!(!state.is_empty());
        assert_eq!(state.ui_zoom, Some(120));

        let cfg = AppConfig {
            ui_state: state,
            ..Default::default()
        };
        let raw = serde_json::to_string(&cfg).unwrap();
        let restored: AppConfig = serde_json::from_str(&raw).unwrap();
        assert_eq!(restored.ui_state.ui_zoom, Some(120));
    }
}
