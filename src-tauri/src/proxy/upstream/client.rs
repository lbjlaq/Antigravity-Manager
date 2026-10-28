// 上游客户端实现
// 基于高性能通讯接口封装

use dashmap::DashMap;
use once_cell::sync::Lazy;
use rquest::{header, Client, Response, StatusCode};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::Duration;

/// 上游请求载荷所属媒体分类（用于分场景首响应头超时调度）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PayloadMediaKind {
    /// 视频载荷
    Video,
    /// 图像载荷（图像生成或内联图片）
    Image,
    /// 文字代码聊天（常规对话）
    TextChat,
    /// 其他轻量请求（countTokens、辅助接口等）
    Other,
}

impl PayloadMediaKind {
    /// 相对「统一首字延迟（TTFT）」基准的媒体增量（秒）
    ///
    /// 为什么不再维护独立的绝对超时表：`v1internal:streamGenerateContent` 的响应头
    /// 跟随首个 chunk 一起下发，`TTFB` 与 `TTFT` 是同一个物理等待。拆成两张绝对表
    /// 只会让同一个等待被两个处置相反的看门狗切割（30s 掐死 180s 的预算）。
    ///
    /// 因此这里只保留「相对 TTFT 的增量」：
    /// - 文字代码聊天：+0（就是 TTFT 本身）
    /// - 图片：+45（图像生成/内联图片 prefill 更久）
    /// - 视频：+60
    /// - 其他辅助接口：-30（如标题生成，应当更快失败而不是陪跑）
    pub fn first_byte_offset_secs(&self) -> i64 {
        match self {
            Self::TextChat => 0,
            Self::Image => 45,
            Self::Video => 60,
            Self::Other => -30,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::TextChat => "text_chat",
            Self::Image => "image",
            Self::Video => "video",
            Self::Other => "other",
        }
    }
}

/// 扫描请求载荷判定所属媒体场景（通用且协议无关）
pub fn detect_payload_media_kind(
    body: &Value,
    method: &str,
    is_image_gen: bool,
) -> PayloadMediaKind {
    if method == "countTokens" || method == "loadCodeAssist" || method == "fetchAvailableModels" {
        return PayloadMediaKind::Other;
    }
    if is_image_gen {
        return PayloadMediaKind::Image;
    }

    if payload_contains_mime_prefix(body, "video/") {
        return PayloadMediaKind::Video;
    }
    if payload_contains_mime_prefix(body, "image/") {
        return PayloadMediaKind::Image;
    }

    PayloadMediaKind::TextChat
}

fn payload_contains_mime_prefix(body: &Value, prefix: &str) -> bool {
    let root = if body.get("request").is_some() {
        body.get("request").unwrap()
    } else {
        body
    };
    if let Some(contents) = root.get("contents").and_then(Value::as_array) {
        for content in contents {
            if let Some(parts) = content.get("parts").and_then(Value::as_array) {
                for part in parts {
                    let inline_mime = part
                        .get("inlineData")
                        .or_else(|| part.get("inline_data"))
                        .and_then(|obj| obj.get("mimeType").or_else(|| obj.get("mime_type")))
                        .and_then(Value::as_str);
                    if let Some(m) = inline_mime {
                        if m.starts_with(prefix) {
                            return true;
                        }
                    }
                    let file_mime = part
                        .get("fileData")
                        .or_else(|| part.get("file_data"))
                        .and_then(|obj| obj.get("mimeType").or_else(|| obj.get("mime_type")))
                        .and_then(Value::as_str);
                    if let Some(m) = file_mime {
                        if m.starts_with(prefix) {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

/// 解析端点易读简称 (Daily, Sandbox, Prod, Custom)
pub fn parse_endpoint_name(base_url: &str) -> &'static str {
    if base_url.contains("sandbox") {
        "Sandbox"
    } else if base_url.contains("daily") {
        "Daily"
    } else if base_url.contains("cloudcode-pa.googleapis.com") {
        "Prod"
    } else {
        "Custom"
    }
}

/// 端点降级与请求尝试的链路记录信息
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FallbackAttemptLog {
    /// 尝试的端点简称 (如 "Daily", "Sandbox", "Prod")
    pub endpoint_name: String,
    /// 尝试的端点 URL
    pub endpoint_url: String,
    /// HTTP 状态码 (网络错误时为 None)
    pub status: Option<u16>,
    /// 耗时 (秒)
    pub duration_s: f64,
    /// 距离上一尝试失败到当前尝试发起之间的 fallback 切换延迟 (秒)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub switch_delay_s: Option<f64>,
    /// 错误描述
    pub error: String,
    /// 是否为最终命中的有效端点
    pub is_effective: bool,
}

/// 上游调用结果，包含响应和降级尝试记录
pub struct UpstreamCallResult {
    /// 最终的 HTTP 响应
    pub response: Response,
    /// 所有端点尝试记录 (包含前置失败项与最终有效项)
    pub fallback_attempts: Vec<FallbackAttemptLog>,
    /// 最终有效端点发起请求的时间戳 (用于计算纯粹的点到点首包 TTFT)
    pub effective_attempt_start: std::time::Instant,
}

impl UpstreamCallResult {
    /// 是否发生了端点降级 (存在非有效端点尝试)
    pub fn has_fallback(&self) -> bool {
        self.fallback_attempts.iter().any(|a| !a.is_effective)
    }

    /// 获取前置失败端点累计的总开销 (秒，包含失败耗时与切换延迟)
    pub fn fallback_overhead_s(&self) -> f64 {
        self.fallback_attempts
            .iter()
            .filter(|a| !a.is_effective)
            .map(|a| a.duration_s + a.switch_delay_s.unwrap_or(0.0))
            .sum()
    }
}

/// 首字预算基准硬上限（秒）
///
/// 必须显著小于下游客户端的首包看门狗（jeikcode 为 180s），
/// 否则客户端先断开，网关白等一轮并留下一个“无日志的孤儿请求”。
/// 预留 30s 给建连 + 上传 + 端点切换，故文字基准取 150s。
pub const MAX_FIRST_BYTE_BUDGET_SECS: u64 = 150;

/// 叠加媒体增量后的最终上限（图片 +45 / 视频 +60）
///
/// 仍须小于 `pipeline::budget::DEFAULT_GATEWAY_REQUEST_BUDGET_SECS`（默认 270s，可 env 覆盖）。
pub const MAX_FIRST_BYTE_CEILING_SECS: u64 = MAX_FIRST_BYTE_BUDGET_SECS + 60;

/// 由请求体体积粗估 token 规模（仅用于超时曲线分档，不参与计费/统计）
///
/// 实测本协议族约 3.5 bytes/token（150k 上下文 ≈ 520KB，210k ≈ 730KB）。
pub fn estimate_payload_tokens(body_len: usize) -> u32 {
    ((body_len as u64).saturating_mul(2) / 7).min(u32::MAX as u64) as u32
}

/// 按 token 规模给出 TTFT 下限（与大上下文 prefill 的真实耗时曲线对齐）
///
/// 与 `handlers::common::calculate_adaptive_peek_timeout_full` 同源分档，
/// 避免“预算 30s / 阈值 20s”这类跨层错配。
fn adaptive_ttft_secs(estimated_tokens: u32) -> u64 {
    match estimated_tokens {
        t if t > 200_000 => 150,
        t if t > 100_000 => 120,
        t if t > 50_000 => 90,
        t if t > 20_000 => 60,
        _ => 30,
    }
}

/// 上传预算来源（诊断可观测性：明确本次预算是按体积自适应还是用户手填）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadBudgetSource {
    /// 按请求体大小 ÷ 上传带宽估算
    AdaptiveSize,
    /// 按体积 ÷ 被动实测上行速度估算
    AdaptiveObserved,
}

impl UploadBudgetSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AdaptiveSize => "adaptive_size",
            Self::AdaptiveObserved => "adaptive_observed_speed",
        }
    }
}

/// 一次上游调用的分阶段超时计划
#[derive(Debug, Clone)]
pub struct UpstreamTimeoutPlan {
    /// 单个端点尝试中的上传阶段预算（秒）
    pub upload_budget_secs: u64,
    /// 首选端点「统一首字延迟」预算（秒，含上传完成后的排队与 prefill）
    pub first_byte_primary_secs: u64,
    /// 备用端点「统一首字延迟」预算（秒，增量减半）
    pub first_byte_fallback_secs: u64,
    /// 整条请求共享的首字预算（秒，端点之间共同消耗，不再逐端点相乘）
    pub first_byte_total_secs: u64,
    /// 允许尝试的端点数量上限（大体积请求默认只试 1 个，抑制重传放大）
    pub endpoint_attempt_limit: usize,
    /// 全链路总预算（秒）
    pub total_budget_secs: u64,
    /// 上传无进展空闲超时（秒）
    pub stall_secs: u64,
    /// 上传预算来源
    pub budget_source: UploadBudgetSource,
    /// 是否属于大体积请求
    pub large_body: bool,
    /// 计划生成时采到的被动上行速度样本（bytes/s）
    pub observed_bps: Option<u64>,
}

/// 发送阶段（用于错误分类与诊断）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendStage {
    /// 仍在向上游写出请求体
    Upload,
    /// 请求体已发完，正在等待首个上游字节（响应头与首 chunk 同源，即 TTFT）
    AwaitFirstByte,
    /// 已拿到响应头或底层已完成
    Completed,
}

impl SendStage {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Upload => "upload",
            Self::AwaitFirstByte => "await_first_byte",
            Self::Completed => "completed",
        }
    }
}

/// 请求体上传进度快照（仅统计已被传输层消费的字节，不含本地磁盘 IO）
pub struct UploadProgress {
    total: usize,
    sent: std::sync::atomic::AtomicUsize,
    finished: std::sync::atomic::AtomicBool,
    started: std::time::Instant,
    last_progress_ms: std::sync::atomic::AtomicU64,
    /// 首个字节交付给传输层的时刻（毫秒，相对 started；u64::MAX 表示尚未开始）
    first_byte_ms: std::sync::atomic::AtomicU64,
}

impl UploadProgress {
    pub fn new(total: usize) -> Self {
        Self {
            total,
            sent: std::sync::atomic::AtomicUsize::new(0),
            finished: std::sync::atomic::AtomicBool::new(total == 0),
            started: std::time::Instant::now(),
            last_progress_ms: std::sync::atomic::AtomicU64::new(0),
            first_byte_ms: std::sync::atomic::AtomicU64::new(u64::MAX),
        }
    }

    /// 记录已交付给传输层的字节数
    pub fn record(&self, bytes: usize) {
        use std::sync::atomic::Ordering;
        let now_ms = self.started.elapsed().as_millis() as u64;
        let sent = self.sent.fetch_add(bytes, Ordering::Relaxed) + bytes;
        self.last_progress_ms.store(now_ms, Ordering::Relaxed);
        let _ = self.first_byte_ms.compare_exchange(
            u64::MAX,
            now_ms,
            Ordering::Relaxed,
            Ordering::Relaxed,
        );
        if self.total > 0 && sent >= self.total {
            self.finished.store(true, Ordering::Release);
        }
    }

    /// 主动标记上传完成（固定长度 body 无进度反馈时使用）
    pub fn mark_finished(&self) {
        use std::sync::atomic::Ordering;
        self.last_progress_ms
            .store(self.started.elapsed().as_millis() as u64, Ordering::Relaxed);
        self.finished.store(true, Ordering::Release);
    }

    pub fn is_finished(&self) -> bool {
        self.finished.load(std::sync::atomic::Ordering::Acquire)
    }

    /// 首个字节开始传输后的已用时长（尚未开始则为 None）
    ///
    /// 上传预算必须从“真正开始传输”而不是“发起请求”起算：
    /// 建连/TLS/写头不应占用按体积估算出来的传输预算。
    pub fn elapsed_since_first_byte(&self) -> Option<std::time::Duration> {
        use std::sync::atomic::Ordering;
        let first = self.first_byte_ms.load(Ordering::Relaxed);
        if first == u64::MAX {
            return None;
        }
        let now_ms = self.started.elapsed().as_millis() as u64;
        Some(std::time::Duration::from_millis(
            now_ms.saturating_sub(first),
        ))
    }

    /// 返回 (已发送字节, 是否完成, 无进展时长)
    pub fn snapshot(&self) -> (usize, bool, std::time::Duration) {
        use std::sync::atomic::Ordering;
        let sent = self.sent.load(Ordering::Relaxed);
        let finished = self.finished.load(Ordering::Acquire);
        let now_ms = self.started.elapsed().as_millis() as u64;
        let last_ms = self.last_progress_ms.load(Ordering::Relaxed);
        (
            sent,
            finished,
            std::time::Duration::from_millis(now_ms.saturating_sub(last_ms)),
        )
    }

    pub fn total(&self) -> usize {
        self.total
    }

    /// 上传完成后的有效平均速度（bytes/s）
    pub fn completed_throughput_bps(&self, upload_elapsed: std::time::Duration) -> Option<u64> {
        let secs = upload_elapsed.as_secs_f64();
        if secs < 0.5 {
            return None;
        }
        let sent = self.sent.load(std::sync::atomic::Ordering::Relaxed);
        if sent == 0 {
            return None;
        }
        Some((sent as f64 / secs) as u64)
    }
}

/// 被动上行吞吐估计器：仅在真实请求上传成功后更新，不发起任何测速请求
static UPLOAD_THROUGHPUT_EWMA_BPS: once_cell::sync::Lazy<parking_lot::RwLock<Option<f64>>> =
    once_cell::sync::Lazy::new(|| parking_lot::RwLock::new(None));

/// 读取当前被动上行速度估计（bytes/s）
pub fn current_upload_throughput_bps() -> Option<u64> {
    UPLOAD_THROUGHPUT_EWMA_BPS.read().map(|v| v as u64)
}

/// 用一个完整上传样本更新 EWMA（样本过小或过快时直接忽略）
fn observe_upload_throughput(bps: u64) {
    if bps < 32 * 1024 || bps > 1024 * 1024 * 1024 {
        return;
    }
    let mut guard = UPLOAD_THROUGHPUT_EWMA_BPS.write();
    let next = match *guard {
        Some(prev) => prev * 0.7 + (bps as f64) * 0.3,
        None => bps as f64,
    };
    *guard = Some(next);
}

/// 清空被动上行速度估计（单元测试隔离与运行时重置）
pub fn reset_upload_throughput_estimate() {
    *UPLOAD_THROUGHPUT_EWMA_BPS.write() = None;
}

/// 寄存器一次成功完成的上传样本
pub fn record_upload_throughput_sample(bytes: usize, elapsed: std::time::Duration) {
    let secs = elapsed.as_secs_f64();
    if bytes == 0 || secs < 0.5 {
        return;
    }
    observe_upload_throughput((bytes as f64 / secs) as u64);
}

/// 分阶段发送结果
struct PhasedSendOutcome {
    /// 底层发送结果（被看门狗掐断时附带合成错误）
    result: Result<Response, String>,
    /// 进程中断时所在阶段
    stage: SendStage,
    /// 已交付给传输层的字节数
    bytes_sent: usize,
    /// 上传阶段耗时（仅当上传确实完成时有值）
    upload_elapsed: Option<std::time::Duration>,
}

/// 邮箱脱敏：只显示前3位 + *** + @域名前2位 + ***
/// 例: "userexample@gmail.com" → "use***@gm***"
pub fn mask_email(email: &str) -> String {
    if let Some(at_pos) = email.find('@') {
        let local = &email[..at_pos];
        let domain = &email[at_pos + 1..];
        let local_prefix: String = local.chars().take(3).collect();
        let domain_prefix: String = domain.chars().take(2).collect();
        format!("{}***@{}***", local_prefix, domain_prefix)
    } else {
        // 不是合法邮箱格式，直接截取前5位
        let prefix: String = email.chars().take(5).collect();
        format!("{}***", prefix)
    }
}

/// 大体积请求在上传阶段被中断且已按策略终止重传的错误标记。
///
/// 协议 Handler 通过 `is_upload_stage_failure` 判定后不再跨账号重发同一大 body，
/// 避免“一个短上传看门狗导致同一大文件被反复从头重传”的放大效应。
pub const UPLOAD_STAGE_TERMINAL_MARKER: &str = "[upstream_upload_stage_terminal]";

/// 判断上游错误是否为“大体积上传阶段终止、不应再跨账号重传”的终态失败
pub fn is_upload_stage_failure(error_text: &str) -> bool {
    error_text.contains(UPLOAD_STAGE_TERMINAL_MARKER)
}

/// 首字节预算耗尽的终态标记。
///
/// 语义与 `UPLOAD_STAGE_TERMINAL_MARKER` 对称，但处置更严：
/// 首字节超时的成因是上游排队 / 大上下文 prefill，**不是账号故障**。
/// 若把它当成账号失败去轮换，每一次重试都会让上游从头 prefill 同一份大 body，
/// 形成“越重试越慢”的活锁（实测 7 分钟 22 次、全在首字节阶段）。
///
/// 因此 Handler 见到本标记后：不标记账号失败、不跨账号重放，直接返回 504 交客户端决定。
pub const FIRST_BYTE_STAGE_TERMINAL_MARKER: &str = "[upstream_first_byte_terminal]";

/// 判断上游错误是否为“首字节预算耗尽、不应跨账号重放”的终态失败
pub fn is_first_byte_stage_failure(error_text: &str) -> bool {
    error_text.contains(FIRST_BYTE_STAGE_TERMINAL_MARKER)
}

/// 任一终态重放抑制标记（上传阶段 / 首字节阶段）
pub fn is_terminal_replay_suppressed(error_text: &str) -> bool {
    is_upload_stage_failure(error_text) || is_first_byte_stage_failure(error_text)
}

/// 剔除仅用于内部链路控制与诊断的标记，避免泄漏到客户端错误报文
pub fn strip_internal_markers(error_text: &str) -> String {
    error_text
        .replace(UPLOAD_STAGE_TERMINAL_MARKER, "")
        .replace(FIRST_BYTE_STAGE_TERMINAL_MARKER, "")
}

/// [NEW] 错误日志脱敏：抹除报错信息中的 access_token, proxy_url 等敏感凭证
pub fn sanitize_error_for_log(error_text: &str) -> String {
    // 抹除常见敏感 key 的值
    let re = regex::Regex::new(r#"(?i)(access_token|refresh_token|id_token|authorization|api_key|secret|password|proxy_url|http_proxy|https_proxy)\s*[:=]\s*[^"'\\\s,}\]]+"#).unwrap();
    let redacted = re.replace_all(error_text, "$1=<redacted>");

    // 抹除 Bearer token
    let re_bearer = regex::Regex::new(r#"(?i)(bearer\s+)[^"'\\\s,}\]]+"#).unwrap();
    let redacted = re_bearer.replace_all(&redacted, "$1<redacted>");

    // 限制长度防止日志炸弹 (UTF-8 字符边界安全保护)
    if redacted.len() > 1000 {
        format!(
            "{}... (truncated)",
            crate::proxy::mappers::common_utils::safe_truncate_str(&redacted, 1000)
        )
    } else {
        redacted.into_owned()
    }
}

// Cloud Code v1internal endpoints (fallback order: Daily → Sandbox → Prod)
//
// Daily 优先 —— 它是官方 IDE 原生唯一主力端点（`language_server` 的启动参数即指向它），
// 稳定支持思维链与工具调用；Sandbox 为沙箱备用容灾；Prod 端点因全流量必报 429 已被彻底剔除。
//
// [FIX Issue #3525 / PR #3526] 顺序同时是**正确性**要求，不只是可用性偏好：
// 同一账号 / 模型 / 代理下，Sandbox 对部分地区返回**终止性 400**
// `User location is not supported for the API use.`，而 `should_try_next_endpoint`
// 只对 408 / 404 / 5xx 回退 → 400 不触发回退，故 Sandbox 排首位会让已验证可用的
// Daily 永远不被尝试。Sandbox 保留为回退项、回退判定规则不变 ——
// **不对所有 400 无条件重试**。
#[allow(dead_code)]
const V1_INTERNAL_BASE_URL_PROD: &str = "https://cloudcode-pa.googleapis.com/v1internal";
const V1_INTERNAL_BASE_URL_DAILY: &str = "https://daily-cloudcode-pa.googleapis.com/v1internal";
const V1_INTERNAL_BASE_URL_SANDBOX: &str =
    "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal";

pub const V1_INTERNAL_BASE_URL_FALLBACKS: [&str; 2] = [
    V1_INTERNAL_BASE_URL_DAILY, // 优先级 1: Daily (官方 IDE 原生主力端点，毫秒级响应，稳定支持长流式与思维链)
    V1_INTERNAL_BASE_URL_SANDBOX, // 优先级 2: Sandbox (沙箱备用，极速响应，实测完美承载 100k+ 上下文，无缝容灾)
];

/// 提取端点的主机名或规范标识，使不同 API 路径（如 :loadCodeAssist、:fetchAvailableModels、v1internal）
/// 能共享底层主机的健康度状态，避免同一死端点在不同业务模块重复超时。
pub fn extract_endpoint_host(url_or_host: &str) -> &str {
    let s = if let Some(stripped) = url_or_host.strip_prefix("https://") {
        stripped
    } else if let Some(stripped) = url_or_host.strip_prefix("http://") {
        stripped
    } else {
        url_or_host
    };
    let host_part = s.split('/').next().unwrap_or(s);
    host_part.split(':').next().unwrap_or(host_part)
}

/// 判断 HTTP 状态码是否属于应当尝试下一个候选端点的临时/可重试故障。
pub fn should_try_next_endpoint(status: StatusCode) -> bool {
    UpstreamClient::should_try_next_endpoint(status)
}

/// 判断特定端点在特定状态码下是否可重试或回退。
/// 特别针对 Sandbox 沙箱端点：非美 IP 会返回 400 Bad Request (FAILED_PRECONDITION: User location is not supported)，
/// 此时必须标记该端点失败并快速回退到 Prod 生产端点。
pub fn should_try_next_endpoint_for_url(status: StatusCode, endpoint_url: &str) -> bool {
    UpstreamClient::should_try_next_endpoint_for_url(status, endpoint_url)
}

/// 端点健康追踪器：记录故障端点与冷却期，动态优化上游端点 Fallback 顺序，避免无状态重复踩坑。
#[derive(Debug, Clone)]
pub struct EndpointHealthTracker {
    /// 记录最近一次故障的时间戳 (host -> Instant)
    failures: Arc<DashMap<String, std::time::Instant>>,
    /// 冷却观察期 (默认 60s)
    cooldown: std::time::Duration,
}

static GLOBAL_ENDPOINT_HEALTH: Lazy<EndpointHealthTracker> =
    Lazy::new(EndpointHealthTracker::default);

/// 获取全局端点健康追踪器实例（跨模块共享，包括主网关请求、项目解析 loadCodeAssist、配额查询）
pub fn get_global_endpoint_health() -> &'static EndpointHealthTracker {
    &GLOBAL_ENDPOINT_HEALTH
}

impl EndpointHealthTracker {
    pub fn new(cooldown: std::time::Duration) -> Self {
        Self {
            failures: Arc::new(DashMap::new()),
            cooldown,
        }
    }

    /// 标记端点调用失败（记录当前时间戳，以 host 聚合）
    pub fn mark_failure(&self, endpoint: &str) {
        let host = extract_endpoint_host(endpoint);
        let now = std::time::Instant::now();
        self.failures.insert(host.to_string(), now);
        tracing::warn!(
            host = %host,
            endpoint = %endpoint,
            cooldown_secs = ?self.cooldown.as_secs(),
            "Marked upstream endpoint host as unhealthy (entering cooldown)"
        );
    }

    /// 标记端点调用成功（清除故障标记，立即恢复健康）
    pub fn mark_success(&self, endpoint: &str) {
        let host = extract_endpoint_host(endpoint);
        if self.failures.remove(host).is_some() {
            tracing::info!(
                host = %host,
                endpoint = %endpoint,
                "Upstream endpoint host recovered and marked healthy"
            );
        }
    }

    /// 检查指定端点当前是否健康（无故障记录，或已超出冷却期）
    #[allow(dead_code)]
    pub fn is_healthy(&self, endpoint: &str) -> bool {
        let host = extract_endpoint_host(endpoint);
        if let Some(entry) = self.failures.get(host) {
            let last_fail = *entry.value();
            if last_fail.elapsed() >= self.cooldown {
                drop(entry);
                self.failures.remove(host);
                true
            } else {
                false
            }
        } else {
            true
        }
    }

    /// 根据各端点健康状况与冷却期，动态排列优先级顺序。
    /// - 健康端点排在最前，保持原有的相对优先级；
    /// - 处于冷却期内的端点排在后部，按故障发生时间由远及近（最早失败的相对排在前面，优先探活）；
    /// - 永远保留所有端点，确保在全故障极端情况下仍能兜底尝试。
    pub fn get_ordered_endpoints<'a>(&self, default_endpoints: &[&'a str]) -> Vec<&'a str> {
        let mut healthy = Vec::new();
        let mut unhealthy = Vec::new();

        for &ep in default_endpoints {
            let host = extract_endpoint_host(ep);
            if let Some(entry) = self.failures.get(host) {
                let last_fail = *entry.value();
                if last_fail.elapsed() >= self.cooldown {
                    drop(entry);
                    self.failures.remove(host);
                    healthy.push(ep);
                } else {
                    unhealthy.push((ep, last_fail));
                }
            } else {
                healthy.push(ep);
            }
        }

        // 不健康的端点：按照失败时间从早到晚排序（先失败的更早尝试探活）
        unhealthy.sort_by_key(|&(_, time)| time);

        let mut result = healthy;
        for (ep, _) in unhealthy {
            result.push(ep);
        }

        result
    }
}

impl Default for EndpointHealthTracker {
    fn default() -> Self {
        Self::new(std::time::Duration::from_secs(30))
    }
}

pub struct UpstreamClient {
    default_client: RwLock<Client>,
    proxy_pool: Option<Arc<crate::proxy::proxy_pool::ProxyPoolManager>>,
    client_cache: DashMap<String, Client>, // proxy_id -> Client
    user_agent_override: RwLock<Option<String>>,
    endpoint_health: EndpointHealthTracker,
}

impl UpstreamClient {
    pub fn new(
        proxy_config: Option<crate::proxy::config::UpstreamProxyConfig>,
        proxy_pool: Option<Arc<crate::proxy::proxy_pool::ProxyPoolManager>>,
    ) -> Self {
        let default_client = match Self::build_client_internal(proxy_config.clone()) {
            Ok(client) => client,
            Err(err_with_proxy) => {
                tracing::error!(
                    error = %err_with_proxy,
                    "Failed to create default HTTP client with configured upstream proxy; retrying without proxy"
                );
                match Self::build_client_internal(None) {
                    Ok(client) => client,
                    Err(err_without_proxy) => {
                        tracing::error!(
                            error = %err_without_proxy,
                            "Failed to create default HTTP client without proxy; falling back to bare client"
                        );
                        Client::new()
                    }
                }
            }
        };

        Self {
            default_client: RwLock::new(default_client),
            proxy_pool,
            client_cache: DashMap::new(),
            user_agent_override: RwLock::new(None),
            endpoint_health: get_global_endpoint_health().clone(),
        }
    }

    /// [HOT-RELOAD] Rebuild the default HTTP client using the supplied upstream
    /// proxy config. Called from `update_proxy` so changes to the upstream proxy
    /// take effect without restarting the app.
    pub async fn rebuild_default_client(
        &self,
        proxy_config: Option<crate::proxy::config::UpstreamProxyConfig>,
    ) {
        let new_client = match Self::build_client_internal(proxy_config.clone()) {
            Ok(c) => c,
            Err(err_with_proxy) => {
                tracing::error!(
                    error = %err_with_proxy,
                    "Hot-reload: failed to rebuild default HTTP client with configured upstream proxy; retrying without proxy"
                );
                match Self::build_client_internal(None) {
                    Ok(c) => c,
                    Err(err_without_proxy) => {
                        tracing::error!(
                            error = %err_without_proxy,
                            "Hot-reload: failed to rebuild default HTTP client without proxy; keeping previous client"
                        );
                        return;
                    }
                }
            }
        };
        let mut guard = self.default_client.write().await;
        *guard = new_client;
        tracing::info!("UpstreamClient default_client rebuilt (upstream proxy hot-reloaded)");
    }

    /// [HOT-RELOAD] Drop all per-proxy cached clients. Call after the pool
    /// configuration changes (proxy URL/credentials edited, proxy removed,
    /// bindings changed) so the next request rebuilds with fresh settings.
    pub fn clear_client_cache(&self) {
        let size = self.client_cache.len();
        self.client_cache.clear();
        if size > 0 {
            tracing::info!("UpstreamClient cleared {} cached per-proxy clients", size);
        }
    }

    /// Base client builder configured with common connection pool, timeouts, and TCP keep-alive
    fn base_client_builder() -> rquest::ClientBuilder {
        // [Fixes #3763] 连接期看门狗可配置：仅约束 TCP/TLS 建连阶段，超时即切换下一端点；
        // 上传/响应头阶段由请求级分阶段预算接管。客户端在构建时快照该值，修改后需重启服务。
        let timeout_cfg = crate::proxy::get_upload_timeout_config();
        let connect_secs = timeout_cfg.connect_secs.clamp(3, 120);
        let builder = Client::builder()
            .emulation(rquest_util::Emulation::Chrome123)
            // Connection settings (优化连接复用，减少建立开销)
            // [FIX #3763] 建连仅涉及 TCP/TLS 握手，默认 10s，兼顾跨国网络抖动与快速识别死端点
            .connect_timeout(Duration::from_secs(connect_secs))
            .pool_max_idle_per_host(10) // 每主机最多 10 个空闲连接 (防止过多闲置连接被上游服务端静默掐断)
            // [FIX] 空闲连接收紧至 15s，避免 GFE 在 20~30s 静默掐断空闲 TCP 后客户端复用死连接引发 error reading a body from connection
            .pool_idle_timeout(Duration::from_secs(15))
            // [FIX #3593 / #3571] 彻底恢复 Chrome 123 原生协议行为：
            // 1. 实测证实 Google GFE 对客户端 HTTP/2 PING 帧静默丢弃、不回复 PING ACK，
            //    导致 hyper2 在 keep_alive_timeout 到期时本地自杀式掐死连接并抛出 error reading a body from connection；
            // 2. 移除破坏性的底层 HTTP/2 PING 配置，回归 4.8.8 稳态基线；
            // 3. 依赖标准 L4 TCP Keep-Alive (30s) 保活，防 NAT / GFE 空闲超时掐断长连接，下游已有 PR #3578 的 3s SSE 注释心跳防客户端超时。
            .tcp_keepalive(Duration::from_secs(30)) // TCP 稳态保活探测 (从 60s 收紧至 30s)
            .timeout(Duration::from_secs(7200));

        Self::apply_default_user_agent(builder)
    }

    /// Internal helper to build a client with optional upstream proxy config
    fn build_client_internal(
        proxy_config: Option<crate::proxy::config::UpstreamProxyConfig>,
    ) -> Result<Client, rquest::Error> {
        let mut builder = Self::base_client_builder();

        if let Some(config) = proxy_config {
            if config.enabled && !config.url.is_empty() {
                let url = crate::proxy::config::normalize_proxy_url(&config.url);
                if let Ok(proxy) = rquest::Proxy::all(&url) {
                    builder = builder.proxy(proxy);
                    tracing::info!("UpstreamClient enabled proxy: {}", url);
                }
            }
        }

        builder.build()
    }

    /// Build a client with a specific PoolProxyConfig (from ProxyPool)
    fn build_client_with_proxy(
        &self,
        proxy_config: crate::proxy::proxy_pool::PoolProxyConfig,
    ) -> Result<Client, rquest::Error> {
        // Reuse base settings similar to default client but with specific proxy
        let builder = Self::base_client_builder().proxy(proxy_config.proxy);
        builder.build()
    }

    fn apply_default_user_agent(builder: rquest::ClientBuilder) -> rquest::ClientBuilder {
        let ua = crate::constants::USER_AGENT.as_str();
        if header::HeaderValue::from_str(ua).is_ok() {
            builder.user_agent(ua)
        } else {
            tracing::warn!(
                user_agent = %ua,
                "Invalid default User-Agent value, using fallback"
            );
            builder.user_agent("antigravity")
        }
    }

    /// Set dynamic User-Agent override
    pub async fn set_user_agent_override(&self, ua: Option<String>) {
        let mut lock = self.user_agent_override.write().await;
        *lock = ua;
        tracing::debug!("UpstreamClient User-Agent override updated: {:?}", lock);
    }

    /// Get current User-Agent (sanitized with safety floor >= 4.3.0)
    pub async fn get_user_agent(&self) -> String {
        let ua_override = self.user_agent_override.read().await;
        match ua_override.as_ref() {
            Some(ua) => crate::constants::sanitize_egress_user_agent(ua),
            None => crate::constants::USER_AGENT.clone(),
        }
    }

    /// Get client for a specific account (or default if no proxy bound)
    pub async fn get_client(&self, account_id: Option<&str>) -> Client {
        if let Some(pool) = &self.proxy_pool {
            if let Some(acc_id) = account_id {
                // Try to get per-account proxy
                match pool.get_proxy_for_account(acc_id).await {
                    Ok(Some(proxy_cfg)) => {
                        // Check cache
                        if let Some(client) = self.client_cache.get(&proxy_cfg.entry_id) {
                            return client.clone();
                        }
                        // Build new client and cache it
                        match self.build_client_with_proxy(proxy_cfg.clone()) {
                            Ok(client) => {
                                self.client_cache
                                    .insert(proxy_cfg.entry_id.clone(), client.clone());
                                tracing::info!(
                                    "Using ProxyPool proxy ID: {} for account: {}",
                                    proxy_cfg.entry_id,
                                    acc_id
                                );
                                return client;
                            }
                            Err(e) => {
                                tracing::error!("Failed to build client for proxy {}: {}, falling back to default", proxy_cfg.entry_id, e);
                            }
                        }
                    }
                    Ok(None) => {
                        // No proxy found or required for this account, use default
                    }
                    Err(e) => {
                        tracing::error!(
                            "Error getting proxy for account {}: {}, falling back to default",
                            acc_id,
                            e
                        );
                    }
                }
            }
        }
        // Fallback to default client
        self.default_client.read().await.clone()
    }

    /// Build v1internal URL
    fn build_url(base_url: &str, method: &str, query_string: Option<&str>) -> String {
        if let Some(qs) = query_string {
            format!("{}:{}?{}", base_url, method, qs)
        } else {
            format!("{}:{}", base_url, method)
        }
    }

    /// Determine if we should try next endpoint (fallback logic)
    pub fn should_try_next_endpoint(status: StatusCode) -> bool {
        status == StatusCode::REQUEST_TIMEOUT
            || status == StatusCode::NOT_FOUND
            || status.is_server_error()
    }

    /// Determine if we should try next endpoint considering endpoint-specific characteristics.
    /// Specifically: Sandbox cluster rejects non-US IPs with 400 Bad Request (FAILED_PRECONDITION).
    /// When this occurs on a sandbox endpoint, we must rotate to Prod instead of returning 400 to the client.
    pub fn should_try_next_endpoint_for_url(status: StatusCode, endpoint_url: &str) -> bool {
        Self::should_try_next_endpoint(status)
            || (status == StatusCode::BAD_REQUEST && endpoint_url.contains("sandbox"))
    }

    /// 判定上游错误是否为模型算力/集群容量耗尽 (MODEL_CAPACITY_EXHAUSTED / 503 / 504)
    pub fn is_model_capacity_exhausted(status: StatusCode, error_body: &str) -> bool {
        (status == StatusCode::SERVICE_UNAVAILABLE || status == StatusCode::GATEWAY_TIMEOUT)
            && (error_body.contains("MODEL_CAPACITY_EXHAUSTED")
                || error_body.contains("No capacity available for model"))
    }

    /// 依据“请求体大小 ÷ 上传带宽 × (1 + 额外等待系数)”估算上传阶段最大等待时间（秒）
    pub fn calculate_upload_budget_secs(
        body_len: usize,
        cfg: &crate::proxy::config::UploadTimeoutConfig,
        observed_bps: Option<u64>,
    ) -> (u64, UploadBudgetSource) {
        // Mbps（十进制）→ B/s：1 Mbps = 1_000_000 bit/s ÷ 8 = 125_000 B/s
        let floor_bps = ((cfg.min_upload_mbps.max(0.1)) * 125_000.0).round() as u64;
        let floor_bps = floor_bps.max(1);
        let ceiling_bps = floor_bps.saturating_mul(4);
        let (effective_bps, source) = match observed_bps {
            Some(bps) if bps > floor_bps => {
                (bps.min(ceiling_bps), UploadBudgetSource::AdaptiveObserved)
            }
            _ => (floor_bps, UploadBudgetSource::AdaptiveSize),
        };

        // 最大等待时间 = 请求体大小 ÷ 上传带宽 × (1 + 额外等待系数)
        let transfer_secs = (body_len as f64) / (effective_bps as f64);
        let wait_secs = transfer_secs * (1.0 + cfg.extra_wait_ratio.max(0.0));
        let budget = (wait_secs.ceil() as u64).max(5);
        (budget, source)
    }

    /// 计算「统一首字延迟（TTFT）」预算（秒）
    ///
    /// 语义：上传完成 → 首个上游字节（响应头与首个 chunk 同源，TTFB≡TTFT）。
    /// - 基准 = max(用户设置的首字响应宽限, 按 token 规模的 prefill 自适应下限)
    /// - 叠加媒体增量：常规 +0 / 图片 +45 / 视频 +60 / 辅助 -30
    /// - 备用端点增量减半，避免一个慢端点吃掉整条请求的预算
    /// - 旧配置 `header_wait_secs` 降级为下限（灰度期兼容，不再是 30/25 硬编码）
    pub fn calculate_first_byte_wait_secs(
        media_kind: PayloadMediaKind,
        attempt_idx: usize,
        cfg: &crate::proxy::config::UploadTimeoutConfig,
        stream_cfg: &crate::proxy::config::StreamTimeoutConfig,
        body_len: usize,
        is_capacity_exhausted: bool,
    ) -> u64 {
        // 容量枯竭时统一收敛至 15s 快速探活，尽快把轮换机会留给健康账号
        if is_capacity_exhausted {
            return 15;
        }

        let estimated_tokens = crate::proxy::pipeline::budget::estimated_tokens()
            .unwrap_or_else(|| estimate_payload_tokens(body_len));
        let base = stream_cfg
            .initial_ttft_secs
            .max(adaptive_ttft_secs(estimated_tokens))
            .clamp(15, MAX_FIRST_BYTE_BUDGET_SECS);

        let offset = media_kind.first_byte_offset_secs();
        // 次要端点增量减半（仅对“加时”方向）；负增量保持原值，
        // 否则辅助接口的备用端点会比首选端点等得更久，语义反转。
        let scaled_offset = if attempt_idx == 0 || offset <= 0 {
            offset
        } else {
            offset / 2
        };
        let value =
            (base as i64 + scaled_offset).clamp(15, MAX_FIRST_BYTE_CEILING_SECS as i64) as u64;
        value.max(cfg.header_wait_secs)
    }

    /// 构造带进度回传的分块请求体
    ///
    /// 按 64 KiB 切块 yield，传输层只有在前一块被写入连接后才会拉取下一块，
    /// 因此 `UploadProgress` 记录的是“已被传输层消费”的字节，可用于真实的停滞检测。
    fn build_progress_body(body_bytes: Vec<u8>, progress: Arc<UploadProgress>) -> rquest::Body {
        const CHUNK_SIZE: usize = 64 * 1024;
        // 使用 Bytes 切片实现零拷贝分块（仅引用计数），避免大 body 再复制一份
        let body = bytes::Bytes::from(body_bytes);
        let stream = futures::stream::unfold(
            (body, 0usize, progress),
            |(body, offset, progress)| async move {
                if offset >= body.len() {
                    return None;
                }
                let end = (offset + CHUNK_SIZE).min(body.len());
                let chunk = body.slice(offset..end);
                // 只有传输层拉取下一块时才记账，从而真实反映背压与网络进展
                progress.record(chunk.len());
                Some((Ok::<_, std::io::Error>(chunk), (body, end, progress)))
            },
        );
        rquest::Body::wrap_stream(stream)
    }

    /// 分阶段主管道：上传阶段看「进展 + 体积/速度预算」，上传完成后进入「统一首字延迟」
    ///
    /// 返回的 `SendStage` 用于外部区分是“上传被截断”还是“已发完在等首字节”，
    /// 以避免把上传中断误报成远端已确认失败，并支撑大体积请求的重传放大抑制。
    #[allow(clippy::too_many_arguments)]
    async fn send_with_phased_budget(
        req_builder: rquest::RequestBuilder,
        progress: Arc<UploadProgress>,
        body_len: usize,
        upload_budget: Duration,
        stall_timeout: Duration,
        first_byte_wait: Duration,
        hard_deadline: std::time::Instant,
    ) -> PhasedSendOutcome {
        use tokio::time::MissedTickBehavior;

        let send_future = req_builder.send();
        tokio::pin!(send_future);

        let attempt_start = std::time::Instant::now();
        let mut upload_done_at: Option<std::time::Instant> = None;

        let mut ticker = tokio::time::interval(Duration::from_millis(250));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        ticker.tick().await; // 消费 interval 的首个立即 tick

        let mut slow_reported = false;
        loop {
            tokio::select! {
                biased;
                res = &mut send_future => {
                    let (sent, finished, _) = progress.snapshot();
                    let upload_elapsed = upload_done_at.map(|t| t.duration_since(attempt_start));
                    if finished {
                        let elapsed = upload_elapsed.unwrap_or_else(|| attempt_start.elapsed());
                        if let Some(bps) = progress.completed_throughput_bps(elapsed) {
                            observe_upload_throughput(bps);
                        }
                    }
                    let stage = if finished {
                        SendStage::Completed
                    } else {
                        SendStage::Upload
                    };
                    if let Err(ref err) = res {
                        tracing::debug!(
                            stage = stage.as_str(),
                            bytes_sent = sent,
                            error = %err,
                            "Phased upstream send failed"
                        );
                    }
                    return PhasedSendOutcome {
                        result: res.map_err(|e| e.to_string()),
                        stage,
                        bytes_sent: sent,
                        upload_elapsed,
                    };
                }
                _ = ticker.tick() => {
                    let now = std::time::Instant::now();
                    let (sent, finished, idle) = progress.snapshot();

                    if finished && upload_done_at.is_none() {
                        upload_done_at = Some(now);
                    }

                    // 慢请求实时留痕：单次上游尝试一旦越过阈值就立即打一条，
                    // 不必等请求结束（挂死 150s 的请求也能在 25s 时就被看到）。
                    if !slow_reported {
                        let threshold = crate::proxy::stage_trace::slow_threshold_ms();
                        if threshold > 0 && attempt_start.elapsed().as_millis() >= threshold {
                            slow_reported = true;
                            crate::proxy::stage_trace::StageTrace::notice_current(
                                "upstream_slow",
                                format!(
                                    "elapsed={:.1}s phase={} sent={}/{} upload_budget={}s first_byte_budget={}s",
                                    attempt_start.elapsed().as_secs_f64(),
                                    if upload_done_at.is_some() {
                                        "await_first_byte"
                                    } else {
                                        "upload"
                                    },
                                    sent,
                                    body_len,
                                    upload_budget.as_secs(),
                                    first_byte_wait.as_secs()
                                ),
                            );
                        }
                    }

                    // 1. 首字节等待阶段（上传已结束；响应头与首个 chunk 同源）
                    if let Some(done_at) = upload_done_at {
                        if now.duration_since(done_at) >= first_byte_wait {
                            let upload_elapsed = Some(done_at.duration_since(attempt_start));
                            return PhasedSendOutcome {
                                result: Err(format!(
                                    "First byte budget exhausted after {}s (upload completed: {}s, {} bytes, stage=await_first_byte)",
                                    first_byte_wait.as_secs(),
                                    upload_elapsed.map(|d| d.as_secs_f64()).unwrap_or_default(),
                                    sent
                                )),
                                stage: SendStage::AwaitFirstByte,
                                bytes_sent: sent,
                                upload_elapsed,
                            };
                        }
                    } else {
                        // 2. 上传阶段：持续有进展不算卡死；预算从“首个字节真正开始传输”起算，
                        //    建连/TLS/写头不占用按体积估算的传输预算；尚未开传时由停滞检测兜底。
                        if idle >= stall_timeout {
                            return PhasedSendOutcome {
                                result: Err(format!(
                                    "Upload stalled: no progress for {}s (sent {}/{} bytes, upload_budget={}s)",
                                    idle.as_secs(),
                                    sent,
                                    body_len,
                                    upload_budget.as_secs()
                                )),
                                stage: SendStage::Upload,
                                bytes_sent: sent,
                                upload_elapsed: None,
                            };
                        }
                        if let Some(transfer_elapsed) = progress.elapsed_since_first_byte() {
                            if transfer_elapsed >= upload_budget {
                                return PhasedSendOutcome {
                                    result: Err(format!(
                                        "Upload budget exhausted after {}s of transfer (sent {}/{} bytes)",
                                        upload_budget.as_secs(),
                                        sent,
                                        body_len
                                    )),
                                    stage: SendStage::Upload,
                                    bytes_sent: sent,
                                    upload_elapsed: None,
                                };
                            }
                        }
                    }

                    // 3. 全链总预算硬截止（优先保障用户可感知的上限）
                    if now >= hard_deadline {
                        let stage = if upload_done_at.is_some() {
                            SendStage::AwaitFirstByte
                        } else {
                            SendStage::Upload
                        };
                        return PhasedSendOutcome {
                            result: Err(format!(
                                "Total upstream budget exhausted (sent {}/{} bytes, stage={})",
                                sent,
                                body_len,
                                stage.as_str()
                            )),
                            stage,
                            bytes_sent: sent,
                            upload_elapsed: upload_done_at.map(|t| t.duration_since(attempt_start)),
                        };
                    }
                }
            }
        }
    }

    /// 生成一次上游调用的分阶段超时计划（上传 / 统一首字延迟 / 端点尝试上限 / 全链总预算）
    pub fn plan_upstream_timeouts(
        media_kind: PayloadMediaKind,
        body_len: usize,
        cfg: &crate::proxy::config::UploadTimeoutConfig,
    ) -> UpstreamTimeoutPlan {
        Self::plan_upstream_timeouts_with_observed(
            media_kind,
            body_len,
            cfg,
            current_upload_throughput_bps(),
        )
    }

    /// 同 `plan_upstream_timeouts`，但显式传入被动测速样本（便于确定性与可重复验证）
    pub fn plan_upstream_timeouts_with_observed(
        media_kind: PayloadMediaKind,
        body_len: usize,
        cfg: &crate::proxy::config::UploadTimeoutConfig,
        observed_bps: Option<u64>,
    ) -> UpstreamTimeoutPlan {
        let (upload_budget_secs, budget_source) =
            Self::calculate_upload_budget_secs(body_len, cfg, observed_bps);

        let large_body = body_len >= cfg.large_body_threshold_bytes();
        let max_endpoints = V1_INTERNAL_BASE_URL_FALLBACKS.len();
        let endpoint_attempt_limit = if large_body {
            cfg.large_body_endpoint_attempts.clamp(1, max_endpoints)
        } else {
            max_endpoints
        };

        let stream_cfg = crate::proxy::config::get_stream_timeout_config();
        let primary =
            Self::calculate_first_byte_wait_secs(media_kind, 0, cfg, &stream_cfg, body_len, false);
        let fallback =
            Self::calculate_first_byte_wait_secs(media_kind, 1, cfg, &stream_cfg, body_len, false);
        // 首字预算在端点之间共享（不再逐端点相乘），并受含媒体增量后的硬上限约束
        let first_byte_total_secs = primary.max(fallback).min(MAX_FIRST_BYTE_CEILING_SECS);

        // 每个端点尝试都包含完整上传阶段；首字预算全链只计一份
        let total_budget_secs = upload_budget_secs
            .saturating_mul(endpoint_attempt_limit as u64)
            .saturating_add(first_byte_total_secs);

        // 停滞超时：上传时因网络错误持续卡死的超时时间；若最大等待时间更短，则自动取最大等待时间
        let stall_secs = cfg.stall_secs.min(upload_budget_secs).max(3);

        UpstreamTimeoutPlan {
            upload_budget_secs,
            first_byte_primary_secs: primary,
            first_byte_fallback_secs: fallback,
            first_byte_total_secs,
            endpoint_attempt_limit,
            total_budget_secs,
            stall_secs,
            budget_source,
            large_body,
            observed_bps,
        }
    }

    /// Call v1internal API (Basic Method)
    ///
    /// Initiates a basic network request, supporting multi-endpoint auto-fallback.
    /// [UPDATED] Takes optional account_id for per-account proxy selection.
    pub async fn call_v1_internal(
        &self,
        method: &str,
        access_token: &str,
        body: Value,
        query_string: Option<&str>,
        account_id: Option<&str>, // [NEW] Account ID for proxy selection
    ) -> Result<UpstreamCallResult, String> {
        self.call_v1_internal_with_headers(
            method,
            access_token,
            body,
            query_string,
            std::collections::HashMap::new(),
            account_id,
        )
        .await
    }

    /// [FIX #765] 调用 v1internal API，支持透传额外的 Headers
    /// [ENHANCED] 返回 UpstreamCallResult，包含降级尝试记录，用于 debug 日志
    pub async fn call_v1_internal_with_headers(
        &self,
        method: &str,
        access_token: &str,
        mut body: Value,
        query_string: Option<&str>,
        extra_headers: std::collections::HashMap<String, String>,
        account_id: Option<&str>, // [NEW] Account ID
    ) -> Result<UpstreamCallResult, String> {
        // [DEFENSE] 全局终极防御拦截：净化所有发往上游报文中的损坏/空 inlineData 以及触发 Google WAF 拦截的违规计费元数据，并最终统一对齐官方信封与前缀拓扑
        if let Some(inner) = body.get_mut("request") {
            crate::proxy::mappers::common_utils::sanitize_gemini_payload_inline_data(inner);
            crate::proxy::mappers::prompt_sanitizer::PromptSanitizer::sanitize_gemini_payload(
                inner,
            );
            crate::proxy::mappers::common_utils::ensure_gemini_payload_ends_with_user(inner);
        } else {
            crate::proxy::mappers::common_utils::sanitize_gemini_payload_inline_data(&mut body);
            crate::proxy::mappers::prompt_sanitizer::PromptSanitizer::sanitize_gemini_payload(
                &mut body,
            );
            crate::proxy::mappers::common_utils::ensure_gemini_payload_ends_with_user(&mut body);
        }
        if method == "countTokens" {
            crate::proxy::pipeline::InboundThinkingPipeline::align_count_tokens_envelope(&mut body);
        } else {
            crate::proxy::pipeline::InboundThinkingPipeline::align_official_envelope(&mut body);
        }

        // [Pipeline First] 在**统一的转出报文**（Gemini Canonical IR）上估一次 token：
        // 四条协议适配器最终都汇聚到这里，因此估算口径天然一致，
        // 不再依赖各 handler 自带的估算点（Gemini/OpenAI 原本根本没有）。
        // 该值供首字节预算按真实 prefill 规模取值，替换 body_len/3.5 的体积估算。
        let payload_tokens = crate::proxy::pipeline::estimator::estimate_tokens(&body);
        if payload_tokens > 0 {
            crate::proxy::pipeline::budget::set_estimated_tokens(payload_tokens);
        }

        // [NEW] Get client based on account (cached in proxy pool manager)
        let client = self.get_client(account_id).await;

        // 构建 Headers (所有端点复用)
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );
        let mut auth_val = header::HeaderValue::from_str(&format!("Bearer {}", access_token))
            .map_err(|e| e.to_string())?;
        auth_val.set_sensitive(true);
        headers.insert(header::AUTHORIZATION, auth_val);

        headers.insert(
            header::USER_AGENT,
            header::HeaderValue::from_str(&self.get_user_agent().await).unwrap_or_else(|e| {
                tracing::warn!("Invalid User-Agent header value, using fallback: {}", e);
                header::HeaderValue::from_str(crate::constants::USER_AGENT.as_str())
                    .unwrap_or_else(|_| header::HeaderValue::from_static("antigravity"))
            }),
        );

        // Keep body.project for content requests, but omit the quota-project header.
        let is_content_request = matches!(method, "generateContent" | "streamGenerateContent");
        if !is_content_request {
            if let Some(proj) = body.get("project").and_then(|v| v.as_str()) {
                if !proj.is_empty() && proj != "test-project" && proj != "project-id" {
                    if let Ok(hv) = header::HeaderValue::from_str(proj) {
                        headers.insert("x-goog-user-project", hv);
                    }
                }
            }
        }

        // 注入业务透传 Headers (如 anthropic-beta)
        // 严格过滤客户端特征伪头，确保出站请求头 100% 对齐官方 Antigravity Hub
        for (k, v) in extra_headers {
            let k_lower = k.to_ascii_lowercase();
            if k_lower == "user-agent"
                || k_lower == "x-session-id"
                || k_lower == "x-client-name"
                || k_lower == "x-client-version"
                || k_lower == "x-machine-id"
                || k_lower == "x-vscode-sessionid"
                || k_lower.starts_with("x-jeikcode")
            {
                continue;
            }
            if let Ok(hk) = header::HeaderName::from_bytes(k.as_bytes()) {
                if let Ok(hv) = header::HeaderValue::from_str(&v) {
                    headers.insert(hk, hv);
                }
            }
        }
        if is_content_request {
            headers.remove("x-goog-user-project");
        }

        // [PIPELINE ALIGNMENT] 统一对齐官方上游特权头：所有 Claude 系列模型出站统一注入 anthropic-beta 声明
        let target_model_str = body
            .get("model")
            .and_then(|v| v.as_str())
            .or_else(|| {
                body.get("request")
                    .and_then(|r| r.get("model"))
                    .and_then(|v| v.as_str())
            })
            .unwrap_or("");
        if target_model_str.to_lowercase().contains("claude")
            && !headers.contains_key("anthropic-beta")
        {
            headers.insert(
                header::HeaderName::from_static("anthropic-beta"),
                header::HeaderValue::from_static("claude-code-20250219"),
            );
        }

        // [DEBUG] Log headers for verification
        tracing::debug!(?headers, "Final Upstream Request Headers");

        let _ = crate::proxy::monitor::CURRENT_UPSTREAM_CAPTURE.try_with(|holder| {
            let pairs: Vec<(&str, &str)> = headers
                .iter()
                .filter_map(|(k, v)| v.to_str().ok().map(|s| (k.as_str(), s)))
                .collect();
            holder.set_headers_json(crate::proxy::payload_audit::header_pairs_to_redacted_json(
                pairs,
            ));
        });

        let mut has_triggered_downgrade = false;
        let mut fallback_attempts: Vec<FallbackAttemptLog> = Vec::new();
        let mut last_attempt_failed_at: Option<std::time::Instant> = None;
        let is_image_gen = body
            .get("requestType")
            .and_then(|v| v.as_str())
            .map(|rt| rt == "image_gen")
            .unwrap_or(false)
            || target_model_str.contains("image")
            || target_model_str.starts_with("imagen-")
            || body
                .get("request")
                .and_then(|r| r.get("generationConfig"))
                .and_then(|g| g.get("imageConfig"))
                .is_some();
        let body_bytes = serde_json::to_vec(&body).map_err(|e| e.to_string())?;
        // [Pipeline Replay Guard] 同一请求内对大 body 的上游调用次数封顶：
        // 429/5xx 轮换不再把同一份几百 KB 的报文体反复上传（带宽维度的笛卡尔积）。
        if let Err(call_no) =
            crate::proxy::pipeline::budget::register_upstream_call(body_bytes.len())
        {
            return Err(format!(
                "{}Cross-account replay cap reached for large body (call #{}, limit={}, body_len={})",
                FIRST_BYTE_STAGE_TERMINAL_MARKER,
                call_no,
                crate::proxy::pipeline::budget::MAX_LARGE_BODY_UPSTREAM_CALLS,
                body_bytes.len()
            ));
        }
        // 分阶段超时计划：上传阶段独立预算（按体积/实测速度自适应），
        // 上传完成后进入「统一首字延迟」（响应头与首 chunk 同源）；大体积请求仍保留重传放大抑制。
        let media_kind = detect_payload_media_kind(&body, method, is_image_gen);
        let upload_cfg = crate::proxy::get_upload_timeout_config();
        let stream_cfg = crate::proxy::config::get_stream_timeout_config();
        let timeout_plan = Self::plan_upstream_timeouts(media_kind, body_bytes.len(), &upload_cfg);
        // [Pipeline Budget] 四大协议共用的请求级硬上限：
        // 已经耗尽就不再发起新的上游尝试（避免带着 0 预算空转一圈又去轮换账号）。
        if crate::proxy::pipeline::budget::exhausted() {
            return Err(format!(
                "{}Gateway global request budget exhausted before upstream call (method={}, body_len={})",
                FIRST_BYTE_STAGE_TERMINAL_MARKER,
                method,
                body_bytes.len()
            ));
        }
        // 把本次调用的内部预算夹到“本请求剩余全局预算”（无入口 deadline 的后台任务不受影响）
        let total_budget_secs =
            crate::proxy::pipeline::budget::clamp_budget_secs(timeout_plan.total_budget_secs);
        tracing::debug!(
            "[Upload-Timeout] method={} kind={} body_len={} large_body={} upload_budget={}s (source={}, observed={:?}B/s) first_byte={}s/{}s (shared={}s) stall={}s attempts={} total_budget={}s",
            method,
            media_kind.as_str(),
            body_bytes.len(),
            timeout_plan.large_body,
            timeout_plan.upload_budget_secs,
            timeout_plan.budget_source.as_str(),
            timeout_plan.observed_bps,
            timeout_plan.first_byte_primary_secs,
            timeout_plan.first_byte_fallback_secs,
            timeout_plan.first_byte_total_secs,
            timeout_plan.stall_secs,
            timeout_plan.endpoint_attempt_limit,
            timeout_plan.total_budget_secs
        );
        let call_overall_start = std::time::Instant::now();
        let mut is_capacity_exhausted = false;

        // [TEMPORARY FIX #3074] 针对 403 SERVICE_DISABLED 的自动降级重试逻辑
        // 我们包装一层循环，以便在检测到特定错误时移除 Header 并重试
        loop {
            let mut last_err: Option<String> = None;
            let mut should_retry_without_header = false;

            // 动态健康度排序：优先尝试健康端点，跳过处于冷却期的故障端点，避免无状态重复累积 40s 超时
            let candidate_endpoints = self
                .endpoint_health
                .get_ordered_endpoints(&V1_INTERNAL_BASE_URL_FALLBACKS);
            let endpoints_snapshot = candidate_endpoints
                .iter()
                .map(|u| {
                    format!(
                        "{}:{}",
                        parse_endpoint_name(u),
                        if self.endpoint_health.is_healthy(u) {
                            "ok"
                        } else {
                            "cooldown"
                        }
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            if candidate_endpoints
                .iter()
                .any(|u| !self.endpoint_health.is_healthy(u))
            {
                // 端点处于冷却 = 降级状态，必须被看到
                crate::proxy::stage_trace::StageTrace::alert_current(
                    "endpoints_degraded",
                    endpoints_snapshot,
                );
            } else {
                crate::proxy::stage_trace::StageTrace::mark_current(
                    "endpoints",
                    endpoints_snapshot,
                );
            }

            // 遍历候选端点，失败时自动切换
            for (idx, base_url) in candidate_endpoints.iter().enumerate() {
                let url = Self::build_url(base_url, method, query_string);

                // [Fixes #3763] 重传放大抑制的分层判据：
                // - 发送阶段网络故障（上传停滞/超时/连接中断）可能需要完整重传，受大体积单次尝试策略约束；
                // - 早到的 HTTP 错误响应（404/5xx 等）通常在上游提前拒绝时返回，重试代价低，仍允许端点 fallback。
                let has_next_endpoint = idx + 1 < candidate_endpoints.len();
                let has_next_for_send_stage =
                    has_next_endpoint && idx + 1 < timeout_plan.endpoint_attempt_limit;

                let elapsed_secs = call_overall_start.elapsed().as_secs();
                let remaining_budget = total_budget_secs.saturating_sub(elapsed_secs);
                if remaining_budget == 0 && idx > 0 {
                    let budget_err = format!(
                        "Total upstream timeout budget ({}s) exhausted before attempting endpoint {} (elapsed={:.1}s), aborting fallback chain",
                        total_budget_secs, base_url, call_overall_start.elapsed().as_secs_f64()
                    );
                    tracing::warn!(%budget_err);
                    last_err = Some(budget_err);
                    break;
                }

                let mut req_builder = client.post(&url).headers(headers.clone());

                // [FIX] 仅对流式接口 (streamGenerateContent) 使用分块传输仿真
                // 对其他接口 (如 generateContent, loadCodeAssist) 发送正常的固定长度 Body
                // 否则图像生成会因为缺少 Content-Length 而被 Google 服务端拒绝或限流 (429)
                // [Fixes #3763] 流式路径按块 yield 并回传真实上传进度，使背压可观测、停滞可检测
                let progress = if method == "streamGenerateContent" {
                    let progress = Arc::new(UploadProgress::new(body_bytes.len()));
                    req_builder = req_builder.body(Self::build_progress_body(
                        body_bytes.clone(),
                        progress.clone(),
                    ));
                    Some(progress)
                } else {
                    req_builder = req_builder.body(body_bytes.clone());
                    None
                };

                let switch_delay_s = last_attempt_failed_at.map(|t| t.elapsed().as_secs_f64());
                let attempt_start = std::time::Instant::now();

                // [WATCHDOG] 分阶段超时：上传阶段受体积/速度自适应预算与停滞检测约束，
                // 上传完成后进入统一首字延迟（TTFB≡TTFT），不再用固定的 30/25s 提前截断大上下文 prefill。
                // 预算在端点之间共享：首个端点先给后续端点预留 1/3，
                // 避免主端点吃满预算后备用端点只剩残羹（也避免逐端点相乘的笛卡尔积）。
                let reserve_for_fallback = if has_next_for_send_stage {
                    timeout_plan.first_byte_total_secs / 3
                } else {
                    0
                };
                let first_byte_budget_secs = Self::calculate_first_byte_wait_secs(
                    media_kind,
                    idx,
                    &upload_cfg,
                    &stream_cfg,
                    body_bytes.len(),
                    is_capacity_exhausted,
                )
                .min(remaining_budget.saturating_sub(reserve_for_fallback).max(5));
                let upload_budget_secs = timeout_plan.upload_budget_secs.min(
                    remaining_budget
                        .saturating_sub(first_byte_budget_secs)
                        .max(10),
                );
                let hard_deadline =
                    std::time::Instant::now() + Duration::from_secs(remaining_budget.max(5));

                let attempt_detail = format!(
                    "endpoint={} host={} kind={} body_len={} upload_budget={}s first_byte_budget={}s reserve={}s remaining={}s",
                    idx,
                    parse_endpoint_name(base_url),
                    media_kind.as_str(),
                    body_bytes.len(),
                    upload_budget_secs,
                    first_byte_budget_secs,
                    reserve_for_fallback,
                    remaining_budget
                );
                if idx == 0 {
                    crate::proxy::stage_trace::StageTrace::mark_current(
                        "upstream_attempt",
                        attempt_detail,
                    );
                } else {
                    // 已经不是首个端点 = 发生了端点降级/切换，必须被看到
                    crate::proxy::stage_trace::StageTrace::alert_current(
                        "endpoint_switch",
                        attempt_detail,
                    );
                }

                let outcome = match progress {
                    Some(progress) => {
                        Self::send_with_phased_budget(
                            req_builder,
                            progress,
                            body_bytes.len(),
                            Duration::from_secs(upload_budget_secs),
                            Duration::from_secs(timeout_plan.stall_secs),
                            Duration::from_secs(first_byte_budget_secs),
                            hard_deadline,
                        )
                        .await
                    }
                    None => {
                        // 固定长度 body 无进度反馈：以“上传预算 + 首字预算”作为单端点总宽限。
                        let total_secs = upload_budget_secs
                            .saturating_add(first_byte_budget_secs)
                            .min(remaining_budget.max(5));
                        match tokio::time::timeout(
                            Duration::from_secs(total_secs),
                            req_builder.send(),
                        )
                        .await
                        {
                            Ok(res) => PhasedSendOutcome {
                                result: res.map_err(|e| e.to_string()),
                                stage: SendStage::Completed,
                                bytes_sent: body_bytes.len(),
                                upload_elapsed: None,
                            },
                            Err(_) => PhasedSendOutcome {
                                result: Err(format!(
                                    "First byte budget exhausted after {}s at {} (fixed-length body: upload_budget={}s + first_byte={}s, stage=await_first_byte)",
                                    total_secs, base_url, upload_budget_secs, first_byte_budget_secs
                                )),
                                stage: SendStage::AwaitFirstByte,
                                bytes_sent: 0,
                                upload_elapsed: None,
                            },
                        }
                    }
                };
                let duration_s = attempt_start.elapsed().as_secs_f64();
                let send_stage = outcome.stage;
                let sent_bytes = outcome.bytes_sent;
                let upload_elapsed = outcome.upload_elapsed;
                let response = outcome.result;

                match response {
                    Ok(resp) => {
                        let status = resp.status();
                        if status.is_success() {
                            self.endpoint_health.mark_success(base_url);
                            let detail = format!(
                                "status={} host={} elapsed={:.1}s",
                                status.as_u16(),
                                parse_endpoint_name(base_url),
                                duration_s
                            );
                            if idx > 0 || has_triggered_downgrade {
                                // 降级后才成功：属于异常恢复，必须被看到
                                crate::proxy::stage_trace::StageTrace::alert_current(
                                    "upstream_fallback_ok",
                                    detail,
                                );
                            } else {
                                crate::proxy::stage_trace::StageTrace::mark_current(
                                    "upstream_headers",
                                    detail,
                                );
                            }
                            if idx > 0 || has_triggered_downgrade {
                                tracing::info!(
                                    "✓ Upstream fallback succeeded | Endpoint: {} | Status: {} | Next endpoints available: {}",
                                    base_url,
                                    status,
                                    candidate_endpoints.len() - idx - 1
                                );
                            } else {
                                tracing::debug!(
                                    "✓ Upstream request succeeded | Endpoint: {} | Status: {}",
                                    base_url,
                                    status
                                );
                            }
                            fallback_attempts.push(FallbackAttemptLog {
                                endpoint_name: parse_endpoint_name(base_url).to_string(),
                                endpoint_url: url.clone(),
                                status: Some(status.as_u16()),
                                duration_s,
                                switch_delay_s,
                                error: String::new(),
                                is_effective: true,
                            });
                            return Ok(UpstreamCallResult {
                                response: resp,
                                fallback_attempts,
                                effective_attempt_start: attempt_start,
                            });
                        }

                        // [NEW] 检测 403 错误 (Issue #3074)
                        // 只要带有项目 Header 且返回 403，我们就尝试降级重试一次
                        if status == StatusCode::FORBIDDEN
                            && !has_triggered_downgrade
                            && headers.contains_key("x-goog-user-project")
                        {
                            last_attempt_failed_at = Some(std::time::Instant::now());
                            let raw_body = resp.text().await.unwrap_or_default();
                            let sanitized = sanitize_error_for_log(&raw_body);
                            let err_detail = if sanitized.trim().is_empty() {
                                "403 Forbidden with project header (downgrading without header)"
                                    .to_string()
                            } else {
                                sanitized.trim().to_string()
                            };
                            fallback_attempts.push(FallbackAttemptLog {
                                endpoint_name: parse_endpoint_name(base_url).to_string(),
                                endpoint_url: url.clone(),
                                status: Some(status.as_u16()),
                                duration_s,
                                switch_delay_s,
                                error: err_detail,
                                is_effective: false,
                            });
                            tracing::warn!(
                                "Detected 403 Forbidden with project header, retrying WITHOUT x-goog-user-project header (Account: {:?})",
                                account_id
                            );
                            should_retry_without_header = true;
                            break;
                        }

                        // 如果有下一个端点且当前错误可重试，则切换
                        if has_next_endpoint
                            && Self::should_try_next_endpoint_for_url(status, base_url)
                        {
                            self.endpoint_health.mark_failure(base_url);
                            last_attempt_failed_at = Some(std::time::Instant::now());
                            let raw_body = resp.text().await.unwrap_or_default();
                            if Self::is_model_capacity_exhausted(status, &raw_body) {
                                is_capacity_exhausted = true;
                                tracing::warn!(
                                    "Upstream endpoint {} returned model capacity exhausted (status={}), subsequent candidates will fast probe",
                                    base_url, status
                                );
                            }
                            let sanitized = sanitize_error_for_log(&raw_body);
                            let err_msg = if sanitized.trim().is_empty() {
                                format!("HTTP {}", status)
                            } else {
                                sanitized.trim().to_string()
                            };
                            tracing::warn!(
                                "Upstream endpoint returned {} at {} (method={}): {}",
                                status,
                                base_url,
                                method,
                                err_msg
                            );
                            // [NEW] 记录降级尝试
                            fallback_attempts.push(FallbackAttemptLog {
                                endpoint_name: parse_endpoint_name(base_url).to_string(),
                                endpoint_url: url.clone(),
                                status: Some(status.as_u16()),
                                duration_s,
                                switch_delay_s,
                                error: err_msg.clone(),
                                is_effective: false,
                            });
                            last_err = Some(err_msg);
                            continue;
                        }

                        // 不可重试的错误或已是最后一个端点，直接返回
                        fallback_attempts.push(FallbackAttemptLog {
                            endpoint_name: parse_endpoint_name(base_url).to_string(),
                            endpoint_url: url.clone(),
                            status: Some(status.as_u16()),
                            duration_s,
                            switch_delay_s,
                            error: format!("HTTP {}", status),
                            is_effective: false,
                        });
                        return Ok(UpstreamCallResult {
                            response: resp,
                            fallback_attempts,
                            effective_attempt_start: attempt_start,
                        });
                    }
                    Err(e) => {
                        self.endpoint_health.mark_failure(base_url);
                        last_attempt_failed_at = Some(std::time::Instant::now());
                        let effective_bps = upload_elapsed
                            .map(|elapsed| {
                                let secs = elapsed.as_secs_f64().max(0.001);
                                (sent_bytes as f64 / secs) as u64
                            })
                            .unwrap_or(0);
                        let terminal_large_body_failure =
                            timeout_plan.large_body && timeout_plan.endpoint_attempt_limit <= 1;
                        // 首字节预算耗尽 / 全局预算耗尽，均属终态：不再跨账号重放
                        let global_exhausted = crate::proxy::pipeline::budget::exhausted();
                        let first_byte_terminal = (send_stage == SendStage::AwaitFirstByte
                            && !has_next_for_send_stage)
                            || global_exhausted;
                        let detail = format!(
                            "Upstream send failed at {} (method={}, kind={}, body_len={}, stage={}, bytes_sent={}, effective_bps={}, budget_source={}, upload_budget={}s, first_byte_budget={}s, total_elapsed={:.1}s): {}",
                            base_url,
                            method,
                            media_kind.as_str(),
                            body_bytes.len(),
                            send_stage.as_str(),
                            sent_bytes,
                            effective_bps,
                            timeout_plan.budget_source.as_str(),
                            upload_budget_secs,
                            first_byte_budget_secs,
                            call_overall_start.elapsed().as_secs_f64(),
                            e
                        );
                        let msg = if first_byte_terminal {
                            format!("{}{}", FIRST_BYTE_STAGE_TERMINAL_MARKER, detail)
                        } else if terminal_large_body_failure {
                            format!("{}{}", UPLOAD_STAGE_TERMINAL_MARKER, detail)
                        } else {
                            detail
                        };
                        tracing::warn!(
                            %msg,
                            large_body = timeout_plan.large_body,
                            "Upstream send failed (staged diagnostics)"
                        );
                        crate::proxy::stage_trace::StageTrace::alert_current(
                            "upstream_send_err",
                            format!(
                                "stage={} host={} elapsed={:.1}s {}",
                                send_stage.as_str(),
                                parse_endpoint_name(base_url),
                                duration_s,
                                msg
                            ),
                        );
                        // [NEW] 记录网络错误的降级尝试
                        fallback_attempts.push(FallbackAttemptLog {
                            endpoint_name: parse_endpoint_name(base_url).to_string(),
                            endpoint_url: url.clone(),
                            status: None,
                            duration_s,
                            switch_delay_s,
                            error: msg.clone(),
                            is_effective: false,
                        });
                        last_err = Some(msg);

                        // 首字节预算耗尽：上游只是慢，不是账号故障。
                        // 不再尝试下一个端点/账号 —— 换端点只会让同一份大 body 从头 prefill。
                        if first_byte_terminal {
                            tracing::warn!(
                                body_len = body_bytes.len(),
                                first_byte_budget_secs,
                                "First-byte budget exhausted: suppressing cross-account replay to avoid prefill amplification"
                            );
                            break;
                        }

                        // [Fixes #3763] 大体积请求默认单次尝试：无论失败在“上传”还是“等首字节”阶段，
                        // 继续 fallback / 跨账号重发都只会从头上传同一大 body，造成重传放大。
                        if terminal_large_body_failure {
                            tracing::warn!(
                                body_len = body_bytes.len(),
                                "Large-body request failed under single-attempt policy; suppressing endpoint fallback and cross-account retry to avoid re-upload amplification"
                            );
                            break;
                        }

                        // 如果是最后一个可重传端点，退出循环
                        if !has_next_for_send_stage {
                            break;
                        }
                        continue;
                    }
                }
            }

            // 处理降级逻辑
            if should_retry_without_header {
                headers.remove("x-goog-user-project");
                has_triggered_downgrade = true;
                // 重启外层 loop，从第一个端点再次尝试
                continue;
            }

            // 如果没有触发降级且所有端点都尝试过，返回最后的错误
            let final_err = last_err.unwrap_or_else(|| "All endpoints failed".to_string());
            tracing::error!(
                error = %final_err,
                account = ?account_id,
                method = %method,
                "Upstream network request failed across all endpoints (non-server fault)"
            );
            return Err(final_err);
        }
    }

    /// 获取可用模型列表
    ///
    /// 获取远端模型列表，支持多端点自动 Fallback
    #[allow(dead_code)] // API ready for future model discovery feature
    pub async fn fetch_available_models(
        &self,
        access_token: &str,
        account_id: Option<&str>,
    ) -> Result<Value, String> {
        // 复用 call_v1_internal，然后解析 JSON
        let result = self
            .call_v1_internal(
                "fetchAvailableModels",
                access_token,
                serde_json::json!({}),
                None,
                account_id,
            )
            .await?;
        let json: Value = result
            .response
            .json()
            .await
            .map_err(|e| format!("Parse json failed: {}", e))?;
        Ok(json)
    }

    /// 辅助型 v1internal 调用（非用户请求路径，例如后台上下文摘要）。
    ///
    /// 为什么需要它：这类调用点位于 mapper / 辅助函数里，历史上拿不到 `AppState.upstream`
    /// 便手写 URL —— 结果形状与 host 双双漂移
    /// （`{host}/v1internal/projects/{p}/locations/global/models/{m}:generateContent`
    /// 在 daily / sandbox / prod 三个 host 上一律返回 HTML 404，导致该功能从未成功过）。
    ///
    /// 这里复用与主请求路径**完全相同**的四个来源，避免再次漂移：
    /// - 客户端 `self.get_client(account_id)` —— 同一套按账号代理池选择与 `client_cache`，
    ///   并随上游代理热更新一并生效（见 `rebuild_default_client` / `clear_client_cache`）。
    ///   账号绑定专属代理时辅助请求同样走它，不会从真实 IP 泄漏出去。
    /// - 端点顺序 `V1_INTERNAL_BASE_URL_FALLBACKS`（Daily → Sandbox → Prod）
    /// - URL 形状 `Self::build_url`（`{base}:{method}`，模型名放在 body 里）
    /// - 回退判定 `Self::should_try_next_endpoint`（408 / 404 / 5xx 才换端点）
    ///
    /// 与 `call_v1_internal` 的差异（有意为之）：不注入请求级 Header
    /// （`x-vscode-sessionid` 等）。超时由调用方用 `timeout_secs` 指定 ——
    /// 客户端默认 600s，对摘要过长，故显式收紧。
    pub async fn call_v1_internal_auxiliary(
        &self,
        method: &str,
        access_token: &str,
        mut body: Value,
        account_id: Option<&str>,
        timeout_secs: u64,
    ) -> Result<Value, String> {
        if method == "countTokens" {
            crate::proxy::pipeline::InboundThinkingPipeline::align_count_tokens_envelope(&mut body);
        } else {
            crate::proxy::pipeline::InboundThinkingPipeline::align_official_envelope(&mut body);
        }
        let client = self.get_client(account_id).await;
        let mut last_error = String::new();

        let candidate_endpoints = self
            .endpoint_health
            .get_ordered_endpoints(&V1_INTERNAL_BASE_URL_FALLBACKS);

        for base_url in candidate_endpoints.iter() {
            let url = Self::build_url(base_url, method, None);

            let response = match client
                .post(&url)
                .header("Authorization", format!("Bearer {}", access_token))
                .header("Content-Type", "application/json")
                .timeout(Duration::from_secs(timeout_secs))
                .json(&body)
                .send()
                .await
            {
                Ok(response) => response,
                Err(e) => {
                    self.endpoint_health.mark_failure(base_url);
                    tracing::warn!(
                        endpoint = %url,
                        error = %e,
                        "Auxiliary v1internal request failed, trying next endpoint"
                    );
                    last_error = format!("request to {} failed: {}", url, e);
                    continue;
                }
            };

            let status = response.status();
            if !status.is_success() {
                let text = response.text().await.unwrap_or_default();
                last_error = format!("{} returned {}: {}", url, status, text);

                // 与主请求路径同一判定：408 / 404 / 5xx 以及 sandbox 400 换端点；
                // 其余状态说明请求本身有问题，直接终止，不做无效重试。
                if !Self::should_try_next_endpoint_for_url(status, base_url) {
                    tracing::error!(
                        error = %last_error,
                        "Auxiliary v1internal request failed with non-retryable status (non-server fault)"
                    );
                    return Err(last_error);
                }
                self.endpoint_health.mark_failure(base_url);
                tracing::warn!(
                    endpoint = %url,
                    status = %status,
                    "Auxiliary v1internal request returned retryable status, trying next endpoint"
                );
                continue;
            }

            self.endpoint_health.mark_success(base_url);
            return response
                .json()
                .await
                .map_err(|e| format!("failed to parse response from {}: {}", url, e));
        }

        let final_err = if last_error.is_empty() {
            "no v1internal endpoint available".to_string()
        } else {
            last_error
        };
        tracing::error!(
            error = %final_err,
            "Auxiliary v1internal request failed across all endpoints (non-server fault)"
        );
        Err(final_err)
    }
}

/// 派生确定性 UUID 格式的客户端窗口 Session ID (RFC 4122 v4 格式)
#[allow(dead_code)]
fn derive_session_uuid(seed: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(b"antigravity-session-v1:");
    hasher.update(seed.as_bytes());
    let hash = hasher.finalize();
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-4{:01x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        hash[0], hash[1], hash[2], hash[3],
        hash[4], hash[5],
        hash[6] & 0x0f, hash[7],
        (hash[8] & 0x3f) | 0x80, hash[9],
        hash[10], hash[11], hash[12], hash[13], hash[14], hash[15]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_url() {
        let base_url = "https://cloudcode-pa.googleapis.com/v1internal";

        let url1 = UpstreamClient::build_url(base_url, "generateContent", None);
        assert_eq!(
            url1,
            "https://cloudcode-pa.googleapis.com/v1internal:generateContent"
        );

        let url2 = UpstreamClient::build_url(base_url, "streamGenerateContent", Some("alt=sse"));
        assert_eq!(
            url2,
            "https://cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse"
        );
    }

    #[test]
    fn test_endpoint_health_all_healthy_preserves_order() {
        let tracker = EndpointHealthTracker::new(Duration::from_secs(60));
        let endpoints = ["ep1", "ep2", "ep3"];
        let ordered = tracker.get_ordered_endpoints(&endpoints);
        assert_eq!(ordered, vec!["ep1", "ep2", "ep3"]);
    }

    #[test]
    fn test_endpoint_health_single_failure_moves_to_back() {
        let tracker = EndpointHealthTracker::new(Duration::from_secs(60));
        let endpoints = ["daily", "sandbox", "prod"];
        tracker.mark_failure("daily");

        let ordered = tracker.get_ordered_endpoints(&endpoints);
        assert_eq!(ordered, vec!["sandbox", "prod", "daily"]);
    }

    #[test]
    fn test_endpoint_health_two_failures_prioritizes_healthy() {
        let tracker = EndpointHealthTracker::new(Duration::from_secs(60));
        let endpoints = ["daily", "sandbox", "prod"];
        tracker.mark_failure("daily");
        tracker.mark_failure("sandbox");

        let ordered = tracker.get_ordered_endpoints(&endpoints);
        assert_eq!(ordered[0], "prod");
        assert_eq!(ordered.len(), 3);
        assert!(ordered.contains(&"daily"));
        assert!(ordered.contains(&"sandbox"));
    }

    #[test]
    fn test_endpoint_health_success_clears_failure() {
        let tracker = EndpointHealthTracker::new(Duration::from_secs(60));
        let endpoints = ["daily", "sandbox", "prod"];
        tracker.mark_failure("daily");
        assert_eq!(
            tracker.get_ordered_endpoints(&endpoints),
            vec!["sandbox", "prod", "daily"]
        );

        tracker.mark_success("daily");
        assert_eq!(
            tracker.get_ordered_endpoints(&endpoints),
            vec!["daily", "sandbox", "prod"]
        );
    }

    #[test]
    fn test_endpoint_health_all_failed_preserves_all_endpoints() {
        let tracker = EndpointHealthTracker::new(Duration::from_secs(60));
        let endpoints = ["daily", "sandbox", "prod"];
        tracker.mark_failure("daily");
        tracker.mark_failure("sandbox");
        tracker.mark_failure("prod");

        let ordered = tracker.get_ordered_endpoints(&endpoints);
        assert_eq!(ordered.len(), 3);
        assert!(ordered.contains(&"daily"));
        assert!(ordered.contains(&"sandbox"));
        assert!(ordered.contains(&"prod"));
    }

    #[test]
    fn test_endpoint_health_cooldown_expiry() {
        let tracker = EndpointHealthTracker::new(Duration::from_millis(10));
        let endpoints = ["daily", "sandbox", "prod"];
        tracker.mark_failure("daily");
        assert_eq!(
            tracker.get_ordered_endpoints(&endpoints),
            vec!["sandbox", "prod", "daily"]
        );

        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(
            tracker.get_ordered_endpoints(&endpoints),
            vec!["daily", "sandbox", "prod"]
        );
    }

    #[test]
    fn test_extract_endpoint_host() {
        assert_eq!(
            extract_endpoint_host("https://daily-cloudcode-pa.googleapis.com/v1internal"),
            "daily-cloudcode-pa.googleapis.com"
        );
        assert_eq!(
            extract_endpoint_host(
                "https://daily-cloudcode-pa.googleapis.com/v1internal:loadCodeAssist"
            ),
            "daily-cloudcode-pa.googleapis.com"
        );
        assert_eq!(
            extract_endpoint_host(
                "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:fetchAvailableModels"
            ),
            "daily-cloudcode-pa.sandbox.googleapis.com"
        );
        assert_eq!(
            extract_endpoint_host(
                "https://cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse"
            ),
            "cloudcode-pa.googleapis.com"
        );
        assert_eq!(
            extract_endpoint_host("daily-cloudcode-pa.googleapis.com"),
            "daily-cloudcode-pa.googleapis.com"
        );
    }

    #[test]
    fn test_endpoint_health_cross_api_sharing() {
        let tracker = EndpointHealthTracker::new(Duration::from_secs(60));
        let base_endpoints = [
            "https://daily-cloudcode-pa.googleapis.com/v1internal",
            "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal",
            "https://cloudcode-pa.googleapis.com/v1internal",
        ];
        let load_project_endpoints = [
            "https://daily-cloudcode-pa.googleapis.com/v1internal:loadCodeAssist",
            "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:loadCodeAssist",
            "https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist",
        ];

        // 模拟 loadCodeAssist 发现 daily 和 sandbox 故障
        tracker.mark_failure("https://daily-cloudcode-pa.googleapis.com/v1internal:loadCodeAssist");
        tracker.mark_failure(
            "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:loadCodeAssist",
        );

        // 验证主聊天请求端点立即动态重排，优先走健康的 prod
        let ordered_base = tracker.get_ordered_endpoints(&base_endpoints);
        assert_eq!(
            ordered_base[0],
            "https://cloudcode-pa.googleapis.com/v1internal"
        );

        // 验证其他 API (如 loadCodeAssist 本身) 也同步重排
        let ordered_load = tracker.get_ordered_endpoints(&load_project_endpoints);
        assert_eq!(
            ordered_load[0],
            "https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist"
        );

        // 模拟 prod 成功
        tracker.mark_success("https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist");
        assert!(tracker.is_healthy("https://cloudcode-pa.googleapis.com/v1internal"));
    }

    #[test]
    fn test_v1_internal_fallbacks_order_prioritizes_daily_over_sandbox() {
        assert_eq!(
            V1_INTERNAL_BASE_URL_FALLBACKS,
            [V1_INTERNAL_BASE_URL_DAILY, V1_INTERNAL_BASE_URL_SANDBOX,]
        );
    }

    #[test]
    fn test_should_try_next_endpoint_for_url() {
        use rquest::StatusCode;

        // 标准可重试状态码（所有端点均适用）
        assert!(should_try_next_endpoint_for_url(
            StatusCode::REQUEST_TIMEOUT,
            "https://cloudcode-pa.googleapis.com/v1internal"
        ));
        assert!(should_try_next_endpoint_for_url(
            StatusCode::NOT_FOUND,
            "https://daily-cloudcode-pa.googleapis.com/v1internal"
        ));
        assert!(should_try_next_endpoint_for_url(
            StatusCode::INTERNAL_SERVER_ERROR,
            "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal"
        ));

        // 400 Bad Request：仅在 Sandbox 端点标记为可重试以回退 Prod，非 Sandbox 端点直接终止
        assert!(should_try_next_endpoint_for_url(
            StatusCode::BAD_REQUEST,
            "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal"
        ));
        assert!(!should_try_next_endpoint_for_url(
            StatusCode::BAD_REQUEST,
            "https://daily-cloudcode-pa.googleapis.com/v1internal"
        ));
        assert!(!should_try_next_endpoint_for_url(
            StatusCode::BAD_REQUEST,
            "https://cloudcode-pa.googleapis.com/v1internal"
        ));
    }

    #[test]
    fn test_is_model_capacity_exhausted() {
        use rquest::StatusCode;

        let err_503 = r#"{"error":{"code":503,"message":"No capacity available for model gemini-3.8-flash-high on the server","status":"UNAVAILABLE","details":[{"@type":"type.googleapis.com/google.rpc.ErrorInfo","reason":"MODEL_CAPACITY_EXHAUSTED"}]}}"#;
        assert!(UpstreamClient::is_model_capacity_exhausted(
            StatusCode::SERVICE_UNAVAILABLE,
            err_503
        ));
        assert!(UpstreamClient::is_model_capacity_exhausted(
            StatusCode::GATEWAY_TIMEOUT,
            err_503
        ));

        let err_500 = r#"{"error":{"code":500,"message":"Internal server error"}}"#;
        assert!(!UpstreamClient::is_model_capacity_exhausted(
            StatusCode::INTERNAL_SERVER_ERROR,
            err_500
        ));

        let err_200 = r#"ok"#;
        assert!(!UpstreamClient::is_model_capacity_exhausted(
            StatusCode::OK,
            err_200
        ));
    }

    fn default_upload_cfg() -> crate::proxy::config::UploadTimeoutConfig {
        crate::proxy::config::UploadTimeoutConfig::default()
    }

    #[test]
    fn test_upload_budget_adaptive_manual_and_caps() {
        let cfg = default_upload_cfg();

        // 1) 小文本：预算收敛在 5~10s 级别，保留小请求的快速故障切换能力
        let (small, small_source) = UpstreamClient::calculate_upload_budget_secs(1024, &cfg, None);
        assert_eq!(small_source, UploadBudgetSource::AdaptiveSize);
        assert_eq!(small, 5, "small body budget should clamp to minimum 5s");

        // 2) 47.4MB 大视频：公式 请求体大小 ÷ 上传带宽 × (1 + 额外等待系数)
        // 47_374_672 / 250_000 ≈ 189.5s * 1.4 ≈ 265s
        let big = 47_374_672usize;
        let (big_budget, _) = UpstreamClient::calculate_upload_budget_secs(big, &cfg, None);
        assert!(
            big_budget >= 250 && big_budget <= 280,
            "47MB body budget should be around 263-265s, got {big_budget}"
        );

        // 3) 实测速度高于下限时参与估算：预算可收紧，仍受下限与停滞检测约束
        let (fast_budget, fast_source) =
            UpstreamClient::calculate_upload_budget_secs(big, &cfg, Some(1000 * 1024));
        assert_eq!(fast_source, UploadBudgetSource::AdaptiveObserved);
        assert!(fast_budget < big_budget);
        assert!(fast_budget >= 5);

        // 4) 低速实测样本（低于下限）回退到体积估算，避免把预算压得过紧
        let (_, slow_source) =
            UpstreamClient::calculate_upload_budget_secs(big, &cfg, Some(8 * 1024));
        assert_eq!(slow_source, UploadBudgetSource::AdaptiveSize);
    }

    #[test]
    fn test_first_byte_budget_merges_ttft_and_media_offsets() {
        let cfg = default_upload_cfg();
        let stream_cfg = crate::proxy::config::get_stream_timeout_config();

        // 统一首字延迟：文字基准（用户 TTFT 与自适应取大后封顶）
        let text = UpstreamClient::calculate_first_byte_wait_secs(
            PayloadMediaKind::TextChat,
            0,
            &cfg,
            &stream_cfg,
            1024,
            false,
        );
        assert!((15..=MAX_FIRST_BYTE_BUDGET_SECS).contains(&text));

        // 媒体增量：图片 +45、视频 +60、辅助 -30
        let image = UpstreamClient::calculate_first_byte_wait_secs(
            PayloadMediaKind::Image,
            0,
            &cfg,
            &stream_cfg,
            1024,
            false,
        );
        let video = UpstreamClient::calculate_first_byte_wait_secs(
            PayloadMediaKind::Video,
            0,
            &cfg,
            &stream_cfg,
            1024,
            false,
        );
        let other = UpstreamClient::calculate_first_byte_wait_secs(
            PayloadMediaKind::Other,
            0,
            &cfg,
            &stream_cfg,
            1024,
            false,
        );
        assert_eq!(image, (text + 45).min(MAX_FIRST_BYTE_CEILING_SECS));
        assert_eq!(video, (text + 60).min(MAX_FIRST_BYTE_CEILING_SECS));
        assert_eq!(other, text.saturating_sub(30).max(15));
        assert!(other < text && text < image && image <= video);

        // 次要端点：加时增量减半，负增量保持原值（否则辅助接口语义反转）
        let image_fallback = UpstreamClient::calculate_first_byte_wait_secs(
            PayloadMediaKind::Image,
            1,
            &cfg,
            &stream_cfg,
            1024,
            false,
        );
        assert_eq!(image_fallback, text + 45 / 2);
        let other_fallback = UpstreamClient::calculate_first_byte_wait_secs(
            PayloadMediaKind::Other,
            1,
            &cfg,
            &stream_cfg,
            1024,
            false,
        );
        assert_eq!(other_fallback, other);

        // 容量枯竭：固定 15s 快速探活
        assert_eq!(
            UpstreamClient::calculate_first_byte_wait_secs(
                PayloadMediaKind::Video,
                0,
                &cfg,
                &stream_cfg,
                1024,
                true
            ),
            15
        );

        // 大上下文：自适应下限抬升，不会被默认值压死
        let huge_body = 3_500_000usize;
        let huge = UpstreamClient::calculate_first_byte_wait_secs(
            PayloadMediaKind::TextChat,
            0,
            &cfg,
            &stream_cfg,
            huge_body,
            false,
        );
        assert!(huge >= MAX_FIRST_BYTE_BUDGET_SECS);

        // 旧配置 header_wait_secs 降级为下限
        let raised = crate::proxy::config::UploadTimeoutConfig {
            header_wait_secs: 200,
            ..cfg.clone()
        };
        assert_eq!(
            UpstreamClient::calculate_first_byte_wait_secs(
                PayloadMediaKind::TextChat,
                0,
                &raised,
                &stream_cfg,
                1024,
                false
            ),
            200
        );

        // 端点预算共享：总预算 = 上传×端点数 + 一份首字预算，而非逐端点相乘
        let small_plan = UpstreamClient::plan_upstream_timeouts_with_observed(
            PayloadMediaKind::TextChat,
            1024,
            &cfg,
            None,
        );
        assert!(!small_plan.large_body);
        assert_eq!(
            small_plan.endpoint_attempt_limit,
            V1_INTERNAL_BASE_URL_FALLBACKS.len()
        );
        assert_eq!(small_plan.first_byte_total_secs, text);
        assert_eq!(
            small_plan.total_budget_secs,
            small_plan.upload_budget_secs * small_plan.endpoint_attempt_limit as u64
                + small_plan.first_byte_total_secs
        );

        // 大体积请求默认仅允许 1 个端点，抑制同一大 body 重传放大
        let big_plan = UpstreamClient::plan_upstream_timeouts_with_observed(
            PayloadMediaKind::Video,
            cfg.large_body_threshold_bytes() + 1,
            &cfg,
            None,
        );
        assert!(big_plan.large_body);
        assert_eq!(big_plan.endpoint_attempt_limit, 1);
        assert_eq!(big_plan.stall_secs, 45);
        assert!(big_plan.total_budget_secs >= big_plan.upload_budget_secs);
    }

    #[test]
    fn test_connect_timeout_config() {
        let cfg = crate::proxy::config::UploadTimeoutConfig::default();
        assert_eq!(cfg.connect_secs, 10);

        let normalized = crate::proxy::config::UploadTimeoutConfig {
            connect_secs: 0,
            ..cfg
        }
        .normalized();
        assert_eq!(normalized.connect_secs, 3);
    }

    #[test]
    fn test_upload_stage_terminal_marker_roundtrip() {
        let msg = format!(
            "{}Upstream send failed (stage=upload, body_len=47374672)",
            UPLOAD_STAGE_TERMINAL_MARKER
        );
        assert!(is_upload_stage_failure(&msg));
        assert!(!is_upload_stage_failure("ordinary network error"));
        assert_eq!(
            strip_internal_markers(&msg),
            "Upstream send failed (stage=upload, body_len=47374672)"
        );
    }

    #[test]
    fn test_upload_progress_tracks_consumed_bytes_and_throughput() {
        let progress = UploadProgress::new(100);
        assert!(!progress.is_finished());
        // 尚未开始传输时没有传输预算计时，仅靠停滞检测兜底
        assert!(progress.elapsed_since_first_byte().is_none());

        progress.record(40);
        assert!(progress.elapsed_since_first_byte().is_some());
        let (sent, finished, _) = progress.snapshot();
        assert_eq!(sent, 40);
        assert!(!finished);

        progress.record(60);
        let (sent, finished, _) = progress.snapshot();
        assert_eq!(sent, 100);
        assert!(finished);

        // 平均速度样本
        let bps = progress
            .completed_throughput_bps(std::time::Duration::from_secs(2))
            .expect("throughput sample");
        assert_eq!(bps, 50);

        // 样本过短时不计入估计，避免瞬时抖动污染全局 EWMA
        assert!(progress
            .completed_throughput_bps(std::time::Duration::from_millis(100))
            .is_none());
        reset_upload_throughput_estimate();
        assert!(current_upload_throughput_bps().is_none());
        record_upload_throughput_sample(4 * 1024 * 1024, std::time::Duration::from_secs(2));
        assert!(current_upload_throughput_bps().is_some());
        reset_upload_throughput_estimate();
    }
}
