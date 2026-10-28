use serde::{Deserialize, Serialize};
// use std::path::PathBuf;
use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

// ============================================================================
// 辅助工具函数
// ============================================================================

/// 标准化代理 URL，如果缺失协议则默认补全 http://
pub fn normalize_proxy_url(url: &str) -> String {
    let url = url.trim();
    if url.is_empty() {
        return String::new();
    }
    if !url.contains("://") {
        format!("http://{}", url)
    } else {
        url.to_string()
    }
}

// ============================================================================
// 全局 Thinking Budget 配置存储
// 用于在 request transform 函数中访问配置（无需修改函数签名）
// ============================================================================
#[cfg(test)]
pub static TEST_CONFIG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

static GLOBAL_THINKING_BUDGET_CONFIG: OnceLock<RwLock<ThinkingBudgetConfig>> = OnceLock::new();

/// 获取当前 Thinking Budget 配置
pub fn get_thinking_budget_config() -> ThinkingBudgetConfig {
    GLOBAL_THINKING_BUDGET_CONFIG
        .get()
        .and_then(|lock| lock.read().ok())
        .map(|cfg| cfg.clone())
        .unwrap_or_default()
}

/// 更新全局 Thinking Budget 配置
pub fn update_thinking_budget_config(config: ThinkingBudgetConfig) {
    if let Some(lock) = GLOBAL_THINKING_BUDGET_CONFIG.get() {
        if let Ok(mut cfg) = lock.write() {
            *cfg = config.clone();
            tracing::info!(
                "[Thinking-Budget] Global config updated: source={:?}, flash_mode={:?} (L:{}, M:{}, H:{}, T:{}), pro_mode={:?} (L:{}, H:{}), claude_mode={:?} (L:{}, M:{}, H:{})",
                config.control_source,
                config.flash_mode,
                config.flash_low,
                config.flash_medium,
                config.flash_high,
                config.flash_tiered,
                config.pro_mode,
                config.pro_low,
                config.pro_high,
                config.claude_mode,
                config.claude_low,
                config.claude_medium,
                config.claude_high
            );
        }
    } else {
        // 首次初始化
        let _ = GLOBAL_THINKING_BUDGET_CONFIG.set(RwLock::new(config.clone()));
        tracing::info!(
            "[Thinking-Budget] Global config initialized: source={:?}, flash_mode={:?} (L:{}, M:{}, H:{}, T:{}), pro_mode={:?} (L:{}, H:{}), claude_mode={:?} (L:{}, M:{}, H:{})",
            config.control_source,
            config.flash_mode,
            config.flash_low,
            config.flash_medium,
            config.flash_high,
            config.flash_tiered,
            config.pro_mode,
            config.pro_low,
            config.pro_high,
            config.claude_mode,
            config.claude_low,
            config.claude_medium,
            config.claude_high
        );
    }
}

// ============================================================================
// 全局系统提示词配置存储
// 用户可在设置中配置一段全局提示词，自动注入到所有请求的 systemInstruction 中
// ============================================================================
static GLOBAL_SYSTEM_PROMPT_CONFIG: OnceLock<RwLock<GlobalSystemPromptConfig>> = OnceLock::new();

/// 获取当前全局系统提示词配置
pub fn get_global_system_prompt() -> GlobalSystemPromptConfig {
    GLOBAL_SYSTEM_PROMPT_CONFIG
        .get()
        .and_then(|lock| lock.read().ok())
        .map(|cfg| cfg.clone())
        .unwrap_or_default()
}

/// 更新全局系统提示词配置
pub fn update_global_system_prompt_config(config: GlobalSystemPromptConfig) {
    if let Some(lock) = GLOBAL_SYSTEM_PROMPT_CONFIG.get() {
        if let Ok(mut cfg) = lock.write() {
            *cfg = config.clone();
            tracing::info!(
                "[Global-System-Prompt] Config updated: enabled={}, content_len={}",
                config.enabled,
                config.content.len()
            );
        }
    } else {
        // 首次初始化
        let _ = GLOBAL_SYSTEM_PROMPT_CONFIG.set(RwLock::new(config.clone()));
        tracing::info!(
            "[Global-System-Prompt] Config initialized: enabled={}, content_len={}",
            config.enabled,
            config.content.len()
        );
    }
}

// ============================================================================
// 全局图像思维模式配置存储
// ============================================================================
static GLOBAL_IMAGE_THINKING_MODE: OnceLock<RwLock<String>> = OnceLock::new();

pub fn get_image_thinking_mode() -> String {
    GLOBAL_IMAGE_THINKING_MODE
        .get()
        .and_then(|lock| lock.read().ok())
        .map(|s| s.clone())
        .unwrap_or_else(|| "enabled".to_string())
}

pub fn update_image_thinking_mode(mode: Option<String>) {
    let val = mode.unwrap_or_else(|| "enabled".to_string());
    if let Some(lock) = GLOBAL_IMAGE_THINKING_MODE.get() {
        if let Ok(mut cfg) = lock.write() {
            if *cfg != val {
                *cfg = val.clone();
                tracing::info!("[Image-Thinking] Global config updated: {}", val);
            }
        }
    } else {
        let _ = GLOBAL_IMAGE_THINKING_MODE.set(RwLock::new(val.clone()));
    }
}

// ============================================================================
// 全局多模态交互与保鲜滑窗配置存储
// ============================================================================
static GLOBAL_MULTIMODAL_CONFIG: OnceLock<RwLock<MultimodalConfig>> = OnceLock::new();

pub fn get_multimodal_config() -> MultimodalConfig {
    GLOBAL_MULTIMODAL_CONFIG
        .get()
        .and_then(|lock| lock.read().ok())
        .map(|cfg| cfg.clone())
        .unwrap_or_default()
}

pub fn update_multimodal_config(config: MultimodalConfig) {
    if let Some(lock) = GLOBAL_MULTIMODAL_CONFIG.get() {
        if let Ok(mut cfg) = lock.write() {
            if *cfg != config {
                *cfg = config.clone();
                tracing::info!(
                    "[Multimodal-Config] Global config updated: sliding_window={}, strategy={}, max_fresh_images={}, strip_remote_urls={}, max_total_mb={}",
                    config.enable_sliding_window,
                    config.strategy,
                    config.max_fresh_images,
                    config.strip_remote_urls,
                    config.max_total_image_mb,
                );
            }
        }
    } else {
        let _ = GLOBAL_MULTIMODAL_CONFIG.set(RwLock::new(config.clone()));
        tracing::info!(
            "[Multimodal-Config] Global config initialized: sliding_window={}, strategy={}, max_fresh_images={}, strip_remote_urls={}, max_total_mb={}",
            config.enable_sliding_window,
            config.strategy,
            config.max_fresh_images,
            config.strip_remote_urls,
            config.max_total_image_mb,
        );
    }
}

// ============================================================================
// 全局流式梯度超时配置存储
// ============================================================================
static GLOBAL_STREAM_TIMEOUT_CONFIG: OnceLock<RwLock<StreamTimeoutConfig>> = OnceLock::new();

/// 获取当前流式梯度超时配置（支持环境变量覆盖，保证 Headless/CLI 一致性）
pub fn get_stream_timeout_config() -> StreamTimeoutConfig {
    let base = GLOBAL_STREAM_TIMEOUT_CONFIG
        .get()
        .and_then(|lock| lock.read().ok())
        .map(|cfg| cfg.clone())
        .unwrap_or_default();

    // 允许通过环境变量覆盖，符合 AGENTS.md Headless & CLI Parity 纪律
    let initial_ttft_secs = std::env::var("ANTIGRAVITY_TIMEOUT_TTFT_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(base.initial_ttft_secs);

    let transition_secs = std::env::var("ANTIGRAVITY_TIMEOUT_TRANSITION_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(base.transition_secs);

    let streaming_sliding_secs = std::env::var("ANTIGRAVITY_TIMEOUT_STREAMING_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(base.streaming_sliding_secs);

    StreamTimeoutConfig {
        initial_ttft_secs,
        transition_secs,
        streaming_sliding_secs,
    }
}

/// 更新全局流式梯度超时配置
pub fn update_stream_timeout_config(config: StreamTimeoutConfig) {
    if let Some(lock) = GLOBAL_STREAM_TIMEOUT_CONFIG.get() {
        if let Ok(mut cfg) = lock.write() {
            if *cfg != config {
                *cfg = config.clone();
                tracing::info!(
                    "[Stream-Timeouts] Global config updated: ttft={}s, transition={}s, sliding={}s",
                    config.initial_ttft_secs,
                    config.transition_secs,
                    config.streaming_sliding_secs
                );
            }
        }
    } else {
        let _ = GLOBAL_STREAM_TIMEOUT_CONFIG.set(RwLock::new(config.clone()));
        tracing::info!(
            "[Stream-Timeouts] Global config initialized: ttft={}s, transition={}s, sliding={}s",
            config.initial_ttft_secs,
            config.transition_secs,
            config.streaming_sliding_secs
        );
    }
}

// ============================================================================
// 全局上游发送阶段分级超时配置存储 (Fixes #3763)
// ============================================================================
static GLOBAL_UPLOAD_TIMEOUT_CONFIG: OnceLock<RwLock<UploadTimeoutConfig>> = OnceLock::new();

/// 获取当前上传阶段超时配置（支持环境变量覆盖，保证 Headless/CLI 一致性）
pub fn get_upload_timeout_config() -> UploadTimeoutConfig {
    let base = GLOBAL_UPLOAD_TIMEOUT_CONFIG
        .get()
        .and_then(|lock| lock.read().ok())
        .map(|cfg| cfg.clone())
        .unwrap_or_default();

    // 允许通过环境变量覆盖，符合 AGENTS.md Headless & CLI Parity 纪律
    let connect_secs = env_u64("ANTIGRAVITY_TIMEOUT_CONNECT_SECS", base.connect_secs);
    let min_upload_mbps = std::env::var("ANTIGRAVITY_TIMEOUT_UPLOAD_MIN_MBPS")
        .ok()
        .and_then(|s| s.trim().parse::<f64>().ok())
        .unwrap_or(base.min_upload_mbps);
    let extra_wait_ratio = std::env::var("ANTIGRAVITY_TIMEOUT_UPLOAD_EXTRA_WAIT_RATIO")
        .ok()
        .and_then(|s| s.trim().parse::<f64>().ok())
        .unwrap_or(base.extra_wait_ratio);
    let stall_secs = env_u64("ANTIGRAVITY_TIMEOUT_UPLOAD_STALL_SECS", base.stall_secs);
    let header_wait_secs = env_u64(
        "ANTIGRAVITY_TIMEOUT_HEADER_WAIT_SECS",
        base.header_wait_secs,
    );
    let large_body_threshold_mb = env_u64(
        "ANTIGRAVITY_TIMEOUT_LARGE_BODY_MB",
        base.large_body_threshold_mb,
    );
    let large_body_endpoint_attempts = env_u64(
        "ANTIGRAVITY_TIMEOUT_LARGE_BODY_ENDPOINT_ATTEMPTS",
        base.large_body_endpoint_attempts as u64,
    ) as usize;

    UploadTimeoutConfig {
        connect_secs,
        min_upload_mbps,
        extra_wait_ratio,
        stall_secs,
        header_wait_secs,
        large_body_threshold_mb,
        large_body_endpoint_attempts,
    }
    .normalized()
}

fn env_u64(key: &str, fallback: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(fallback)
}

/// 更新全局上传阶段超时配置
pub fn update_upload_timeout_config(config: UploadTimeoutConfig) {
    let config = config.normalized();
    if let Some(lock) = GLOBAL_UPLOAD_TIMEOUT_CONFIG.get() {
        if let Ok(mut cfg) = lock.write() {
            if *cfg != config {
                *cfg = config.clone();
                tracing::info!(
                    "[Upstream-Timeouts] Global config updated: connect={}s, min_mbps={:.2}, extra_ratio={:.2}, stall={}s, header_wait={}s, large_body={}MB/{}",
                    config.connect_secs,
                    config.min_upload_mbps,
                    config.extra_wait_ratio,
                    config.stall_secs,
                    config.header_wait_secs,
                    config.large_body_threshold_mb,
                    config.large_body_endpoint_attempts
                );
            }
        }
    } else {
        let _ = GLOBAL_UPLOAD_TIMEOUT_CONFIG.set(RwLock::new(config.clone()));
        tracing::info!(
            "[Upstream-Timeouts] Global config initialized: connect={}s, min_mbps={:.2}, extra_ratio={:.2}, stall={}s, header_wait={}s, large_body={}MB/{}",
            config.connect_secs,
            config.min_upload_mbps,
            config.extra_wait_ratio,
            config.stall_secs,
            config.header_wait_secs,
            config.large_body_threshold_mb,
            config.large_body_endpoint_attempts
        );
    }
}

static GLOBAL_PAYLOAD_STORAGE_MODE: OnceLock<RwLock<String>> = OnceLock::new();
static GLOBAL_LOG_RETENTION_DAYS: OnceLock<RwLock<u32>> = OnceLock::new();
static GLOBAL_THINKING_STORE_ENABLED: OnceLock<RwLock<bool>> = OnceLock::new();
static GLOBAL_THINKING_RETENTION_DAYS: OnceLock<RwLock<u32>> = OnceLock::new();
static GLOBAL_THINKING_MAX_MEMORY_TURNS: OnceLock<RwLock<u32>> = OnceLock::new();
static GLOBAL_ENABLE_GLOBAL_COMPACTION_STRIP: OnceLock<RwLock<bool>> = OnceLock::new();

fn write_or_init<T: Clone>(slot: &OnceLock<RwLock<T>>, value: T) {
    if let Some(lock) = slot.get() {
        if let Ok(mut cfg) = lock.write() {
            *cfg = value;
        }
    } else {
        let _ = slot.set(RwLock::new(value));
    }
}

pub fn get_payload_storage_mode() -> String {
    GLOBAL_PAYLOAD_STORAGE_MODE
        .get()
        .and_then(|lock| lock.read().ok())
        .map(|v| v.clone())
        .unwrap_or_else(|| "simple".to_string())
}

pub fn get_log_retention_days() -> u32 {
    GLOBAL_LOG_RETENTION_DAYS
        .get()
        .and_then(|lock| lock.read().ok())
        .map(|v| *v)
        .unwrap_or(30)
        .clamp(1, 3650)
}

pub fn is_thinking_store_enabled() -> bool {
    GLOBAL_THINKING_STORE_ENABLED
        .get()
        .and_then(|lock| lock.read().ok())
        .map(|v| *v)
        .unwrap_or(true)
}

pub fn is_global_compaction_strip_enabled() -> bool {
    GLOBAL_ENABLE_GLOBAL_COMPACTION_STRIP
        .get()
        .and_then(|lock| lock.read().ok())
        .map(|v| *v)
        .unwrap_or(false)
}

pub fn set_global_compaction_strip_enabled(enabled: bool) {
    write_or_init(&GLOBAL_ENABLE_GLOBAL_COMPACTION_STRIP, enabled);
}

pub fn get_thinking_retention_days() -> u32 {
    GLOBAL_THINKING_RETENTION_DAYS
        .get()
        .and_then(|lock| lock.read().ok())
        .map(|v| *v)
        .unwrap_or(15)
        .clamp(1, 3650)
}

pub fn get_thinking_max_memory_turns() -> usize {
    GLOBAL_THINKING_MAX_MEMORY_TURNS
        .get()
        .and_then(|lock| lock.read().ok())
        .map(|v| *v as usize)
        .unwrap_or(600)
        .clamp(10, 10_000)
}

pub fn update_global_audit_config(
    payload_storage_mode: String,
    log_retention_days: u32,
    thinking_store_enabled: bool,
    thinking_retention_days: u32,
    thinking_max_memory_turns: Option<u32>,
    enable_global_compaction_strip: Option<bool>,
) {
    let mode = if payload_storage_mode == "full" {
        "full"
    } else {
        "simple"
    };
    write_or_init(&GLOBAL_PAYLOAD_STORAGE_MODE, mode.to_string());
    write_or_init(
        &GLOBAL_LOG_RETENTION_DAYS,
        log_retention_days.clamp(1, 3650),
    );
    write_or_init(&GLOBAL_THINKING_STORE_ENABLED, thinking_store_enabled);
    write_or_init(
        &GLOBAL_THINKING_RETENTION_DAYS,
        thinking_retention_days.clamp(1, 3650),
    );
    let max_turns = thinking_max_memory_turns.unwrap_or(600).clamp(10, 10_000);
    write_or_init(&GLOBAL_THINKING_MAX_MEMORY_TURNS, max_turns);
    let strip_enabled = enable_global_compaction_strip.unwrap_or(false);
    write_or_init(&GLOBAL_ENABLE_GLOBAL_COMPACTION_STRIP, strip_enabled);
    tracing::info!(
        "[Audit] storage_mode={}, log_retention_days={}, thinking_store={}, thinking_retention_days={}, thinking_max_memory_turns={}, global_compaction_strip={}",
        mode,
        log_retention_days.clamp(1, 3650),
        thinking_store_enabled,
        thinking_retention_days.clamp(1, 3650),
        max_turns,
        strip_enabled
    );
}

/// 全局系统提示词配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalSystemPromptConfig {
    /// 是否启用全局系统提示词
    #[serde(default)]
    pub enabled: bool,
    /// 系统提示词内容
    #[serde(default)]
    pub content: String,
}

impl Default for GlobalSystemPromptConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            content: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyAuthMode {
    Off,
    Strict,
    AllExceptHealth,
    Auto,
}

impl Default for ProxyAuthMode {
    fn default() -> Self {
        Self::Auto
    }
}

/// 实验性功能配置 (Feature Flags)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentalConfig {
    /// 启用双层签名缓存 (Signature Cache)
    #[serde(default = "default_true")]
    pub enable_signature_cache: bool,

    /// 启用工具循环自动恢复 (Tool Loop Recovery)
    #[serde(default = "default_true")]
    pub enable_tool_loop_recovery: bool,

    /// 启用跨模型兼容性检查 (Cross-Model Checks)
    #[serde(default = "default_true")]
    pub enable_cross_model_checks: bool,

    /// 默认关闭。只影响回给客户端的用量数字，不改写上下文。
    #[serde(default = "default_false")]
    pub enable_usage_scaling: bool,

    /// 监控报文体存储模式: `simple`（默认，精简落库）或 `full`（原文）
    #[serde(default = "default_payload_storage_mode")]
    pub payload_storage_mode: String,

    /// 请求日志保留天数
    #[serde(default = "default_log_retention_days")]
    pub log_retention_days: u32,

    /// 服务端思考块回填（默认开启，可关闭）
    #[serde(default = "default_thinking_store_enabled")]
    pub thinking_store_enabled: bool,

    /// 思考块 SQLite 记录保留天数
    #[serde(default = "default_thinking_retention_days")]
    pub thinking_retention_days: u32,

    /// 每轮会话在内存中保留的最大思考块轮次（默认 600，滑动窗口淘汰并由 SQLite 索引承接）
    #[serde(default = "default_thinking_max_memory_turns")]
    pub thinking_max_memory_turns: u32,

    /// Claude Desktop Cowork 模式自动响应式自愈压缩 (Auto Reactive Compact for Cowork)
    #[serde(default = "default_false")]
    pub enable_cowork_auto_compact: bool,

    /// Cowork 自动响应式压缩触发阈值 (默认 200,000 Tokens)
    #[serde(default = "default_cowork_compact_threshold")]
    pub cowork_compact_threshold: u32,

    /// 启用 Claude Cowork 手动深度归档协议支持 (高危选项，默认 false)
    #[serde(default = "default_false")]
    pub enable_cowork_manual_compact: bool,

    /// 全局压缩剥离签名 (Global Compaction Signature Stripping) (试验性选项，默认 false)
    #[serde(default = "default_false")]
    pub enable_global_compaction_strip: bool,
}

impl Default for ExperimentalConfig {
    fn default() -> Self {
        Self {
            enable_signature_cache: true,
            enable_tool_loop_recovery: false,
            enable_cross_model_checks: true,
            enable_usage_scaling: false,
            payload_storage_mode: default_payload_storage_mode(),
            log_retention_days: default_log_retention_days(),
            thinking_store_enabled: default_thinking_store_enabled(),
            thinking_retention_days: default_thinking_retention_days(),
            thinking_max_memory_turns: default_thinking_max_memory_turns(),
            enable_cowork_auto_compact: false,
            cowork_compact_threshold: default_cowork_compact_threshold(),
            enable_cowork_manual_compact: false,
            enable_global_compaction_strip: false,
        }
    }
}

fn default_cowork_compact_threshold() -> u32 {
    200_000
}

fn default_payload_storage_mode() -> String {
    "simple".to_string()
}
fn default_log_retention_days() -> u32 {
    30
}
fn default_thinking_store_enabled() -> bool {
    true
}
fn default_thinking_retention_days() -> u32 {
    15
}
fn default_thinking_max_memory_turns() -> u32 {
    600
}

/// 思考预算控制权大选择
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingControlSource {
    /// 网关权威控制（首选 / 推荐）：网关全权接管，按模型系列与档位标准字典进行权威解析与注入
    Gateway,
    /// 客户端直接控制（危险，不推荐）：四大协议归一化后，直接提取客户端传入的思考预算透传给上游
    Client,
}

impl Default for ThinkingControlSource {
    fn default() -> Self {
        Self::Gateway
    }
}

/// Thinking Budget 模式
/// 控制如何处理模型调用时的思考预算
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingBudgetMode {
    /// 默认模式（官方自适应）：完全透传官方模型 ID，不注入 thinkingBudget
    #[serde(rename = "default")]
    Default,
    /// 自定义思考预算模式：按各档位配置注入对应预算
    #[serde(rename = "custom")]
    Custom,
    /// 旧版兼容模式：自动限制
    #[serde(rename = "auto")]
    Auto,
    /// 旧版兼容模式：透传
    #[serde(rename = "passthrough")]
    Passthrough,
    /// 旧版兼容模式：自适应
    #[serde(rename = "adaptive")]
    Adaptive,
}

impl Default for ThinkingBudgetMode {
    fn default() -> Self {
        Self::Custom
    }
}

/// Thinking Budget 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThinkingBudgetConfig {
    /// 控制权大选择：网关控制 vs 客户端控制
    #[serde(default = "default_control_source")]
    pub control_source: ThinkingControlSource,

    // --- Gemini Flash 系列配置 ---
    #[serde(default = "default_thinking_budget_mode")]
    pub flash_mode: ThinkingBudgetMode,
    #[serde(default = "default_flash_low")]
    pub flash_low: i32, // 默认 1000
    #[serde(default = "default_flash_medium")]
    pub flash_medium: i32, // 默认 4000
    #[serde(default = "default_flash_high")]
    pub flash_high: i32, // 默认 -1，走官方模型结构体
    /// 旧默认值 16384 已迁移为官方 -1。置位后不再重复改写用户后来手选的 16384。
    #[serde(default)]
    pub flash_high_legacy_migrated: bool,
    #[serde(default = "default_flash_tiered")]
    pub flash_tiered: i32, // 默认 -1

    // --- Gemini Pro 系列配置（官方仅 Low 与 High 两档） ---
    #[serde(default = "default_thinking_budget_mode")]
    pub pro_mode: ThinkingBudgetMode,
    #[serde(default = "default_pro_low")]
    pub pro_low: i32, // 默认 1001
    #[serde(default = "default_pro_high")]
    pub pro_high: i32, // 默认 10001

    // --- Claude 系列配置 ---
    #[serde(default = "default_thinking_budget_mode")]
    pub claude_mode: ThinkingBudgetMode,
    #[serde(default = "default_claude_budget")]
    pub claude_budget: i32, // 统一思考预算 (默认 16000, 填 -1 自适应)
    #[serde(default = "default_claude_low")]
    pub claude_low: i32, // 默认 1024
    #[serde(default = "default_claude_medium")]
    pub claude_medium: i32, // 默认 4096
    #[serde(default = "default_claude_high")]
    pub claude_high: i32, // 默认 16000

    // --- 历史向后兼容字段 ---
    #[serde(default = "default_thinking_budget_mode")]
    pub mode: ThinkingBudgetMode,
    #[serde(default = "default_thinking_budget_custom_value")]
    pub custom_value: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default = "default_flash_low")]
    pub custom_low: i32,
    #[serde(default = "default_flash_medium")]
    pub custom_medium: i32,
    #[serde(default = "default_custom_high")]
    pub custom_high: i32,
    #[serde(default = "default_flash_tiered")]
    pub custom_tiered: i32,
}

fn default_control_source() -> ThinkingControlSource {
    ThinkingControlSource::Gateway
}

fn default_thinking_budget_mode() -> ThinkingBudgetMode {
    ThinkingBudgetMode::Custom
}

fn default_flash_low() -> i32 {
    -1
}
fn default_flash_medium() -> i32 {
    -1
}
fn default_flash_high() -> i32 {
    -1
}

fn default_custom_high() -> i32 {
    -1
}
fn default_flash_tiered() -> i32 {
    -1
}

fn default_pro_low() -> i32 {
    -1
}
fn default_pro_high() -> i32 {
    -1
}

fn default_claude_budget() -> i32 {
    -1 // -1 = 采用官方默认的档位预算
}
fn default_claude_low() -> i32 {
    -1
}
fn default_claude_medium() -> i32 {
    -1
}
fn default_claude_high() -> i32 {
    -1
}

impl Default for ThinkingBudgetConfig {
    fn default() -> Self {
        Self {
            control_source: default_control_source(),
            flash_mode: default_thinking_budget_mode(),
            flash_low: default_flash_low(),
            flash_medium: default_flash_medium(),
            flash_high: default_flash_high(),
            flash_high_legacy_migrated: false,
            flash_tiered: default_flash_tiered(),

            pro_mode: default_thinking_budget_mode(),
            pro_low: default_pro_low(),
            pro_high: default_pro_high(),

            claude_mode: default_thinking_budget_mode(),
            claude_budget: default_claude_budget(),
            claude_low: default_claude_low(),
            claude_medium: default_claude_medium(),
            claude_high: default_claude_high(),

            mode: default_thinking_budget_mode(),
            custom_value: default_thinking_budget_custom_value(),
            effort: None,
            custom_low: default_flash_low(),
            custom_medium: default_flash_medium(),
            custom_high: default_custom_high(),
            custom_tiered: default_flash_tiered(),
        }
    }
}

fn default_thinking_budget_custom_value() -> u32 {
    24576
}

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugLoggingConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub output_dir: Option<String>,
}

impl Default for DebugLoggingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            output_dir: None,
        }
    }
}

fn default_sliding_strategy() -> String {
    "count".to_string()
}

/// 多模态交互与保鲜滑窗配置
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MultimodalConfig {
    /// 是否启用多模态历史保鲜滑动窗口 (默认关闭，贯彻保真透传)
    #[serde(default)]
    pub enable_sliding_window: bool,

    /// 保鲜策略模式: "count" (按图片张数) 或 "memory" (按累积内存大小)
    #[serde(default = "default_sliding_strategy")]
    pub strategy: String,

    /// 滑动窗口保鲜最大图片张数（默认 10，填 0 为不限张数）
    #[serde(default = "default_max_fresh_images")]
    pub max_fresh_images: usize,

    /// 历史图片剥离时，是否一并剥离远程 / OSS 直链图片 (默认关闭，默认仅剥离 Base64)
    #[serde(default)]
    pub strip_remote_urls: bool,

    /// 多模态图片累积最大解码容量限制 (MB，默认 32MB，物理防爆安全红线)
    #[serde(default = "default_max_total_image_mb")]
    pub max_total_image_mb: usize,
}

fn default_max_fresh_images() -> usize {
    10
}

fn default_max_total_image_mb() -> usize {
    32
}

impl Default for MultimodalConfig {
    fn default() -> Self {
        Self {
            enable_sliding_window: false,
            strategy: default_sliding_strategy(),
            max_fresh_images: default_max_fresh_images(),
            strip_remote_urls: false,
            max_total_image_mb: default_max_total_image_mb(),
        }
    }
}

/// 上游超时配置（连接期 / 上传阶段 / 首响应头 / 大文件重试）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UploadTimeoutConfig {
    /// 连接期看门狗（每个端点独立建连时限，秒，默认 10s）；超时即切换下一端点。
    /// 该值在 HTTP 客户端构建时生效，修改后需重启代理服务
    #[serde(default = "default_connect_secs")]
    pub connect_secs: u64,

    /// 上传带宽（Mbps，默认 2.0，推荐 1-5 Mbps）
    #[serde(default = "default_min_upload_mbps")]
    pub min_upload_mbps: f64,

    /// 额外等待系数（默认 0.4；最大等待时间 = 大小 ÷ 带宽 × (1 + 系数)）
    #[serde(default = "default_extra_wait_ratio")]
    pub extra_wait_ratio: f64,

    /// 停滞超时（秒，默认 45s）。完全没有字节进展超过该时长中断；若最大等待时间更短则取较短者
    #[serde(default = "default_upload_stall_secs")]
    pub stall_secs: u64,

    /// 统一首字延迟的旧版兼容下限（秒，默认 0 = 不额外抬高）
    ///
    /// 自本版本起「首响应头」与「首字」合并为统一首字延迟（TTFB≡TTFT），
    /// 实际预算 = max(用户 TTFT 设置, 按 token 规模的自适应下限) + 媒体增量，
    /// 媒体增量：常规 +0 / 图片 +45 / 视频 +60 / 辅助 −30，备用端点增量减半。
    /// 本字段仅在非 0 时作为上述结果的**下限**，不再有 30/25s 的内置绝对表。
    #[serde(default = "default_header_wait_secs")]
    pub header_wait_secs: u64,

    /// 大文件判定阈值（MB，默认 8MB）
    #[serde(default = "default_large_body_threshold_mb")]
    pub large_body_threshold_mb: u64,

    /// 大文件最多尝试次数（默认 1；默认只发一次，避免反复重传占满上行）
    #[serde(default = "default_large_body_endpoint_attempts")]
    pub large_body_endpoint_attempts: usize,
}

fn default_connect_secs() -> u64 {
    10
}

fn default_min_upload_mbps() -> f64 {
    2.0
}

fn default_extra_wait_ratio() -> f64 {
    0.4
}

fn default_upload_stall_secs() -> u64 {
    45
}

fn default_header_wait_secs() -> u64 {
    0
}

fn default_large_body_threshold_mb() -> u64 {
    8
}

fn default_large_body_endpoint_attempts() -> usize {
    1
}

impl Default for UploadTimeoutConfig {
    fn default() -> Self {
        Self {
            connect_secs: default_connect_secs(),
            min_upload_mbps: default_min_upload_mbps(),
            extra_wait_ratio: default_extra_wait_ratio(),
            stall_secs: default_upload_stall_secs(),
            header_wait_secs: default_header_wait_secs(),
            large_body_threshold_mb: default_large_body_threshold_mb(),
            large_body_endpoint_attempts: default_large_body_endpoint_attempts(),
        }
    }
}

impl UploadTimeoutConfig {
    /// 归一化非法配置，避免 0 值或倒挂阈值导致发送阶段瞬间自断
    pub fn normalized(mut self) -> Self {
        self.connect_secs = self.connect_secs.clamp(3, 120);
        self.min_upload_mbps = if self.min_upload_mbps.is_finite() {
            self.min_upload_mbps.clamp(0.1, 1000.0)
        } else {
            default_min_upload_mbps()
        };
        self.extra_wait_ratio = if self.extra_wait_ratio.is_finite() {
            self.extra_wait_ratio.clamp(0.0, 10.0)
        } else {
            default_extra_wait_ratio()
        };
        self.stall_secs = self.stall_secs.clamp(5, 600);
        self.header_wait_secs = self.header_wait_secs.min(600);
        self.large_body_threshold_mb = self.large_body_threshold_mb.max(1);
        self.large_body_endpoint_attempts = self.large_body_endpoint_attempts.max(1);
        self
    }

    /// 大体积请求阈值（字节）
    pub fn large_body_threshold_bytes(&self) -> usize {
        (self.large_body_threshold_mb as usize).saturating_mul(1024 * 1024)
    }
}

/// 流式梯度超时配置 (首字响应、状态切换、推流滑动)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StreamTimeoutConfig {
    /// 首字响应宽限时长（秒，默认 180s）
    #[serde(default = "default_initial_ttft_secs")]
    pub initial_ttft_secs: u64,

    /// 思考至正文/工具调用的状态切换宽限时长（秒，默认 180s）
    #[serde(default = "default_transition_secs")]
    pub transition_secs: u64,

    /// 稳态推流相邻 Token 滑动窗口超时（秒，默认 120s）
    #[serde(default = "default_streaming_sliding_secs")]
    pub streaming_sliding_secs: u64,
}

pub fn default_initial_ttft_secs() -> u64 {
    180
}

pub fn default_transition_secs() -> u64 {
    180
}

pub fn default_streaming_sliding_secs() -> u64 {
    120
}

impl Default for StreamTimeoutConfig {
    fn default() -> Self {
        Self {
            initial_ttft_secs: default_initial_ttft_secs(),
            transition_secs: default_transition_secs(),
            streaming_sliding_secs: default_streaming_sliding_secs(),
        }
    }
}

/// IP 黑名单配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpBlacklistConfig {
    /// 是否启用黑名单
    #[serde(default)]
    pub enabled: bool,

    /// 自定义封禁消息
    #[serde(default = "default_block_message")]
    pub block_message: String,
}

impl Default for IpBlacklistConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            block_message: default_block_message(),
        }
    }
}

fn default_block_message() -> String {
    "Access denied".to_string()
}

/// IP 白名单配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpWhitelistConfig {
    /// 是否启用白名单模式 (启用后只允许白名单IP访问)
    #[serde(default)]
    pub enabled: bool,

    /// 白名单优先模式 (白名单IP跳过黑名单检查)
    #[serde(default = "default_true")]
    pub whitelist_priority: bool,
}

impl Default for IpWhitelistConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            whitelist_priority: true,
        }
    }
}

/// 安全监控配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityMonitorConfig {
    /// IP 黑名单配置
    #[serde(default)]
    pub blacklist: IpBlacklistConfig,

    /// IP 白名单配置
    #[serde(default)]
    pub whitelist: IpWhitelistConfig,
}

impl Default for SecurityMonitorConfig {
    fn default() -> Self {
        Self {
            blacklist: IpBlacklistConfig::default(),
            whitelist: IpWhitelistConfig::default(),
        }
    }
}

/// 图片任务调度配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageSchedulerConfig {
    #[serde(default = "default_image_per_account_concurrency")]
    pub per_account_concurrency: usize,
}

impl Default for ImageSchedulerConfig {
    fn default() -> Self {
        Self {
            per_account_concurrency: default_image_per_account_concurrency(),
        }
    }
}

fn default_image_per_account_concurrency() -> usize {
    4
}

/// 反代服务配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    /// 是否启用反代服务
    pub enabled: bool,

    /// 是否允许局域网访问
    /// - false: 仅本机访问 127.0.0.1（默认，隐私优先）
    /// - true: 允许局域网访问 0.0.0.0
    #[serde(default)]
    pub allow_lan_access: bool,

    /// Authorization policy for the proxy.
    /// - off: no auth required
    /// - strict: auth required for all routes
    /// - all_except_health: auth required for all routes except `/healthz`
    /// - auto: recommended defaults (currently: allow_lan_access => all_except_health, else off)
    #[serde(default)]
    pub auth_mode: ProxyAuthMode,

    /// 监听端口
    pub port: u16,

    /// API 密钥
    pub api_key: String,

    /// Web UI 管理后台密码 (可选，如未设置则使用 api_key)
    pub admin_password: Option<String>,

    /// 是否自动启动
    pub auto_start: bool,

    /// 自定义精确模型映射表 (key: 原始模型名, value: 目标模型名)
    #[serde(default)]
    pub custom_mapping: std::collections::HashMap<String, String>,

    /// API 请求超时时间(秒)
    #[serde(default = "default_request_timeout")]
    pub request_timeout: u64,

    /// 是否开启请求日志记录 (监控)
    #[serde(default)]
    pub enable_logging: bool,

    /// 是否捕获健康检查日志 (默认 false: 对 GET 成功请求与第三方网关探活探测全部过滤且不入库)
    #[serde(default)]
    pub capture_health_logs: bool,

    #[serde(default)]
    pub log_retention: LogRetentionConfig,

    /// 内部失败日志（error.log*）滑动窗口容量
    #[serde(default)]
    pub internal_error_log_retention: InternalErrorLogRetentionConfig,

    /// 调试日志配置 (保存完整链路)
    #[serde(default)]
    pub debug_logging: DebugLoggingConfig,

    /// 上游代理配置
    #[serde(default)]
    pub upstream_proxy: UpstreamProxyConfig,

    /// 是否只在 /v1/models 中暴露真实配额模型（隐藏内置虚拟别名）
    #[serde(default)]
    pub only_raw_quota_models: bool,

    /// 自定义 User-Agent 请求头 (可选覆盖)
    #[serde(default)]
    pub user_agent_override: Option<String>,

    /// 官方 Hub 版本自定义覆盖 (如 "2.23.1")
    #[serde(default)]
    pub hub_version: Option<String>,

    /// 官方构建提交编号 (CL) 自定义覆盖 (如 "998881661")
    #[serde(default)]
    pub hub_cl: Option<String>,

    /// 账号调度配置 (粘性会话/限流重试)
    #[serde(default)]
    pub scheduling: crate::proxy::sticky_config::StickySessionConfig,

    /// 实验性功能配置
    #[serde(default)]
    pub experimental: ExperimentalConfig,

    /// 安全监控配置 (IP 黑白名单)
    #[serde(default)]
    pub security_monitor: SecurityMonitorConfig,

    /// 固定账号模式的账号ID (Fixed Account Mode)
    /// - None: 使用轮询模式
    /// - Some(account_id): 固定使用指定账号
    #[serde(default)]
    pub preferred_account_id: Option<String>,

    /// Saved User-Agent string (persisted even when override is disabled)
    #[serde(default)]
    pub saved_user_agent: Option<String>,

    /// Thinking Budget 配置
    /// 控制如何处理 AI 深度思考时的 Token 预算
    #[serde(default)]
    pub thinking_budget: ThinkingBudgetConfig,

    /// 全局系统提示词配置
    /// 自动注入到所有 API 请求的 systemInstruction 中
    #[serde(default)]
    pub global_system_prompt: GlobalSystemPromptConfig,

    /// 图像思维模式配置
    /// - enabled: 保留思维链 (默认)
    /// - disabled: 移除思维链 (画质优先)
    #[serde(default)]
    pub image_thinking_mode: Option<String>,

    /// 图片上游任务的单账号并发数（重启后生效）
    #[serde(default)]
    pub image_scheduler: ImageSchedulerConfig,

    /// 代理池配置
    #[serde(default)]
    pub proxy_pool: ProxyPoolConfig,

    /// 多模态交互与保鲜滑窗配置
    #[serde(default)]
    pub multimodal: MultimodalConfig,

    /// 流式梯度超时配置 (首字响应、状态切换、推流滑动)
    #[serde(default)]
    pub stream_timeouts: StreamTimeoutConfig,

    /// 上游发送阶段分级超时配置 (连接 → 上传 → 等待响应头)
    #[serde(default)]
    pub upload_timeouts: UploadTimeoutConfig,
}

/// Request log retention policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogRetentionConfig {
    #[serde(default = "default_max_body_age_hours")]
    pub max_body_age_hours: u64,
    #[serde(default = "default_max_age_days")]
    pub max_age_days: u64,
    #[serde(default = "default_max_rows")]
    pub max_rows: u64,
    /// Application disk budget in MiB, including the database and WAL.
    #[serde(default = "default_max_disk_mb")]
    pub max_disk_mb: u64,
    /// Max log storage limit in GB (supports decimals, e.g. 0.5)
    #[serde(default = "default_max_storage_gb")]
    pub max_storage_gb: f64,
}

fn default_max_body_age_hours() -> u64 {
    24
}
fn default_max_age_days() -> u64 {
    30
}
fn default_max_rows() -> u64 {
    100_000
}
fn default_max_disk_mb() -> u64 {
    1024
}
fn default_max_storage_gb() -> f64 {
    1.0
}

impl LogRetentionConfig {
    pub fn budget_bytes(&self) -> u64 {
        if self.max_storage_gb > 0.0 {
            (self.max_storage_gb * 1024.0 * 1024.0 * 1024.0) as u64
        } else if self.max_disk_mb > 0 {
            self.max_disk_mb.saturating_mul(1024 * 1024)
        } else {
            0
        }
    }
}

impl Default for LogRetentionConfig {
    fn default() -> Self {
        Self {
            max_body_age_hours: 24,
            max_age_days: 30,
            max_rows: 100_000,
            max_disk_mb: 1024,
            max_storage_gb: 1.0,
        }
    }
}

/// Internal ERROR log file retention (error.log / error.log.YYYY-MM-DD).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InternalErrorLogRetentionConfig {
    /// Disk budget in MiB. Over limit, evict oldest 30% and keep appending.
    #[serde(default = "default_internal_error_max_storage_mb")]
    pub max_storage_mb: u64,
}

fn default_internal_error_max_storage_mb() -> u64 {
    500
}

impl InternalErrorLogRetentionConfig {
    /// `ABV_INTERNAL_ERROR_LOG_MB` overrides the config file when set to a positive integer.
    pub fn budget_bytes(&self) -> u64 {
        let mb = std::env::var("ABV_INTERNAL_ERROR_LOG_MB")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|&n| n > 0)
            .unwrap_or(self.max_storage_mb);
        let mb = if mb == 0 {
            default_internal_error_max_storage_mb()
        } else {
            mb
        };
        mb.saturating_mul(1024 * 1024)
    }
}

impl Default for InternalErrorLogRetentionConfig {
    fn default() -> Self {
        Self {
            max_storage_mb: default_internal_error_max_storage_mb(),
        }
    }
}

/// 上游代理配置
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpstreamProxyConfig {
    /// 是否启用
    pub enabled: bool,
    /// 代理地址 (http://, https://, socks5://)
    pub url: String,
}

pub fn default_custom_mapping() -> std::collections::HashMap<String, String> {
    std::collections::HashMap::new()
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            allow_lan_access: false, // 默认仅本机访问，隐私优先
            auth_mode: ProxyAuthMode::default(),
            port: 8045,
            api_key: format!("sk-{}", uuid::Uuid::new_v4().simple()),
            admin_password: None,
            auto_start: false,
            custom_mapping: default_custom_mapping(),
            request_timeout: default_request_timeout(),
            enable_logging: true,       // 默认开启，支持 token 统计功能
            capture_health_logs: false, // 默认关闭，过滤 GET 探活与第三方网关探测请求且不入库
            log_retention: LogRetentionConfig::default(),
            internal_error_log_retention: InternalErrorLogRetentionConfig::default(),
            debug_logging: DebugLoggingConfig::default(),
            upstream_proxy: UpstreamProxyConfig::default(),
            only_raw_quota_models: false,
            scheduling: crate::proxy::sticky_config::StickySessionConfig::default(),
            experimental: ExperimentalConfig::default(),
            security_monitor: SecurityMonitorConfig::default(),
            preferred_account_id: None, // 默认使用轮询模式
            user_agent_override: None,
            hub_version: None,
            hub_cl: None,
            saved_user_agent: None,
            thinking_budget: ThinkingBudgetConfig::default(),
            global_system_prompt: GlobalSystemPromptConfig::default(),
            proxy_pool: ProxyPoolConfig::default(),
            multimodal: MultimodalConfig::default(),
            stream_timeouts: StreamTimeoutConfig::default(),
            upload_timeouts: UploadTimeoutConfig::default(),
            image_thinking_mode: None,
            image_scheduler: ImageSchedulerConfig::default(),
        }
    }
}

fn default_request_timeout() -> u64 {
    120 // 默认 120 秒,原来 60 秒太短
}

impl ProxyConfig {
    /// 获取实际的监听地址
    /// - allow_lan_access = false: 返回 "127.0.0.1"（默认，隐私优先）
    /// - allow_lan_access = true: 返回 "0.0.0.0"（通配监听：底层自动启用 IPv6/IPv4 双栈监听，允许局域网与外部公网 IPv4/IPv6 访问）
    pub fn get_bind_address(&self) -> &str {
        if self.allow_lan_access {
            "0.0.0.0"
        } else {
            "127.0.0.1"
        }
    }
}

/// 代理认证信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyAuth {
    pub username: String,
    #[serde(
        serialize_with = "crate::utils::crypto::serialize_password",
        deserialize_with = "crate::utils::crypto::deserialize_password"
    )]
    pub password: String,
}

/// 单个代理配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyEntry {
    pub id: String,                       // 唯一标识
    pub name: String,                     // 显示名称
    pub url: String,                      // 代理地址 (http://, https://, socks5://)
    pub auth: Option<ProxyAuth>,          // 认证信息 (可选)
    pub enabled: bool,                    // 是否启用
    pub priority: i32,                    // 优先级 (数字越小优先级越高)
    pub tags: Vec<String>,                // 标签 (如 "美国", "住宅IP")
    pub max_accounts: Option<usize>,      // 最大绑定账号数 (0 = 无限制)
    pub health_check_url: Option<String>, // 健康检查 URL
    pub last_check_time: Option<i64>,     // 上次检查时间
    pub is_healthy: bool,                 // 健康状态
    pub latency: Option<u64>,             // 延迟 (毫秒) [NEW]
}

/// 代理池配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyPoolConfig {
    pub enabled: bool, // 是否启用代理池
    // pub mode: ProxyPoolMode,        // [REMOVED] 代理池模式，统一为 Hybrid 逻辑
    pub proxies: Vec<ProxyEntry>,         // 代理列表
    pub health_check_interval: u64,       // 健康检查间隔 (秒)
    pub auto_failover: bool,              // 自动故障转移
    pub strategy: ProxySelectionStrategy, // 代理选择策略
    /// 账号到代理的绑定关系 (account_id -> proxy_id)，持久化存储
    #[serde(default)]
    pub account_bindings: HashMap<String, String>,
}

impl Default for ProxyPoolConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            // mode: ProxyPoolMode::Global,
            proxies: Vec::new(),
            health_check_interval: 300,
            auto_failover: true,
            strategy: ProxySelectionStrategy::Priority,
            account_bindings: HashMap::new(),
        }
    }
}

/// 代理选择策略
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ProxySelectionStrategy {
    /// 轮询: 依次使用
    RoundRobin,
    /// 随机: 随机选择
    Random,
    /// 优先级: 按 priority 字段排序
    Priority,
    /// 最少连接: 选择当前使用最少的代理
    LeastConnections,
    /// 加权轮询: 根据健康状态和优先级
    WeightedRoundRobin,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_proxy_url() {
        // 测试已有协议
        assert_eq!(
            normalize_proxy_url("http://127.0.0.1:7890"),
            "http://127.0.0.1:7890"
        );
        assert_eq!(
            normalize_proxy_url("https://proxy.com"),
            "https://proxy.com"
        );
        assert_eq!(
            normalize_proxy_url("socks5://127.0.0.1:1080"),
            "socks5://127.0.0.1:1080"
        );
        assert_eq!(
            normalize_proxy_url("socks5h://127.0.0.1:1080"),
            "socks5h://127.0.0.1:1080"
        );

        // 测试缺少协议（默认补全 http://）
        assert_eq!(
            normalize_proxy_url("127.0.0.1:7890"),
            "http://127.0.0.1:7890"
        );
        assert_eq!(
            normalize_proxy_url("localhost:1082"),
            "http://localhost:1082"
        );

        // 测试边缘情况
        assert_eq!(normalize_proxy_url(""), "");
        assert_eq!(normalize_proxy_url("   "), "");
    }
}
