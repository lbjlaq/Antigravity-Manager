use crate::types::TargetHost;
use serde::{Deserialize, Serialize};

/// 单条请求与交互的原子账本实体 (The Atomic Request Record)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestLedgerEntry {
    /// 唯一请求 ID / Step 标识
    pub id: String,
    /// 时间戳 (微秒/毫秒)
    pub timestamp: i64,
    /// 格式化时间字符串 (如 "2026-11-21 22:44:35")
    pub time_str: String,

    // --- 维度 1: 账号归属 (Account Attribution) ---
    pub account_id: Option<String>,
    pub account_email: Option<String>,

    // --- 维度 2: 宿主应用 (Host Application) ---
    pub host_target: TargetHost,

    // --- 维度 3: 模型谱系 (Model Lineage) ---
    /// 用户界面选择的模型 (Requested Model, 如 "claude-3-7-sonnet")
    pub requested_model: String,
    /// 真实物理后端 (Physical Upstream, 如 "gemini-4-hydrogen-high", "gemini-4-neon")
    pub physical_model: String,
    /// 是否检测到静默降级为 conservative
    pub is_downgraded: bool,

    // --- 维度 4: 会话与工作区 (Context & Workspace) ---
    pub conversation_id: String,
    pub session_title: String,
    pub workspace_name: String,

    // --- 核心计量指标 (Metrics & Telemetry) ---
    pub prompt_tokens: u64,
    pub cached_tokens: u64,
    pub completion_tokens: u64,
    pub thinking_tokens: u64,
    pub ttft_ms: u64,
    pub duration_ms: u64,
    pub tokens_per_second: f64,
    pub estimated_cost: f64,
}

/// 视角 1: 会话深度上下文分析 (Session Context Snapshot)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ContextWindowSnapshot {
    pub session_id: String,
    pub title: String,
    pub workspace: String,
    pub host_target: String,
    /// 核心用量速览
    pub current_used_tokens: u64,
    pub budget_tokens: u64,
    pub remaining_tokens: u64,
    pub usage_percentage: f64,
    pub health_status: String, // "healthy", "warning", "critical"
    /// 模型与引擎
    pub physical_model: String,
    /// 分层水库拆解
    pub prompt_tokens: u64,
    pub cached_tokens: u64,
    pub completion_tokens: u64,
    pub thinking_tokens: u64,
    pub tool_tokens: u64,
    pub cache_hit_rate: f64,
    pub saved_cost_usd: f64,
    /// 交互频次
    pub step_count: u64,
    pub last_user_input: Option<String>,
    pub last_model_update: Option<String>,
}

/// 视角 2: 账号归属聚合 (Account Attribution Group)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountUsageGroup {
    pub account_id: String,
    pub email: String,
    pub tags: Vec<String>,
    pub total_requests: u64,
    pub new_input_tokens: u64,
    pub cached_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub cache_hit_rate: f64,
    pub estimated_cost_usd: f64,
    pub quota_gemini_pct: Option<f64>,
    pub quota_claude_pct: Option<f64>,
    pub is_healthy: bool,
    #[serde(default)]
    pub top_workspaces: Vec<(String, f64)>, // (workspace, pct)
    #[serde(default)]
    pub top_models: Vec<(String, f64)>, // (model, pct)
    #[serde(default)]
    pub hourly_heatmap: Vec<u32>, // 24-hour activity counts
}

/// 视角 3: 真实模型性能矩阵 (Model Performance Matrix)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPerformanceGroup {
    pub requested_model: String,
    pub physical_model: String,
    pub total_requests: u64,
    pub total_tokens: u64,
    pub avg_tps: f64,
    pub avg_ttft_ms: u64,
    pub cache_hit_rate: f64,
    pub share_percentage: f64,
    pub is_downgraded: bool,
    #[serde(default)]
    pub top_workspaces: Vec<(String, f64)>,
    #[serde(default)]
    pub top_accounts: Vec<(String, f64)>,
}

/// 视角 4: 宿主与工作区分流 (Host & Workspace Distribution)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostWorkspaceGroup {
    pub host_name: String,
    pub host_id: String,
    pub conversation_count: usize,
    pub total_steps: u64,
    pub total_tokens: u64,
    pub share_percentage: f64,
    pub top_workspaces: Vec<(String, u64)>, // (Workspace, steps)
}

/// 视角 5: 项目与工作区深度分析实体 (Project / Workspace Deep Dive)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProjectUsageGroup {
    pub workspace_name: String,
    pub path: String,
    pub total_steps: u64,
    pub conversation_count: usize,
    pub total_tokens: u64,
    pub estimated_cost_usd: f64,
    pub preferred_model: String,
    pub model_breakdown: Vec<(String, f64)>, // (model, pct)
    pub account_attribution: Vec<(String, f64)>, // (email, pct)
    pub last_active_time: String,
    pub session_titles: Vec<String>,
}

/// 视角 6: 24 小时生物钟热力分布 (Temporal Rhythm Group)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemporalRhythmGroup {
    pub time_slot: String, // "Night (00-06)", "Morning (06-12)", "Afternoon (12-18)", "Evening (18-24)"
    pub request_count: u64,
    pub token_volume: u64,
    pub share_percentage: f64,
    pub peak_tps: f64,
    pub label: String,
}

/// 综合分析大盘实体 (Master Analytics Report)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MasterAnalyticsReport {
    /// 全局概览指标
    pub total_requests: u64,
    pub total_tokens: u64,
    pub total_prompt_tokens: u64,
    pub total_cached_tokens: u64,
    pub total_output_tokens: u64,
    pub global_cache_hit_rate: f64,
    pub total_saved_cost_usd: f64,
    pub avg_speed_tps: f64,
    pub avg_ttft_ms: u64,

    /// 多维切片投影
    pub active_context: Option<ContextWindowSnapshot>,
    pub by_account: Vec<AccountUsageGroup>,
    pub by_project: Vec<ProjectUsageGroup>,
    pub by_model: Vec<ModelPerformanceGroup>,
    pub by_host: Vec<HostWorkspaceGroup>,
    pub timeline: Vec<TemporalRhythmGroup>,
    pub recent_traffic: Vec<RequestLedgerEntry>,
}
