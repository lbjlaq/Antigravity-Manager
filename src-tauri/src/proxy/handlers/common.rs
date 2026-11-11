use crate::proxy::server::AppState;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

// ===== 重试 / 退避 / 轮换策略已下沉到 Pipeline（协议无关）=====
//
// 这里仅保留兼容导出，避免四个适配器与既有测试一次性大改；
// 新代码请直接用 `crate::proxy::pipeline::rotation::*`。
pub use crate::proxy::pipeline::rotation::{
    apply_retry_strategy, calculate_max_retry_attempts, clamp_retry_after_for_claude,
    determine_retry_strategy, determine_retry_strategy_adaptive,
    determine_retry_strategy_with_grace, extract_retry_after_seconds, is_model_not_found_error,
    next_rotation_attempt, record_attempted_account, should_rotate_account, RequestRetryState,
    RetryStrategy,
};

/// 请求内「结构化失败状态」聚合器已下沉到 `pipeline::terminal`（协议无关）。
///
/// 这里仅保留兼容导出，避免四个适配器与既有测试一次性大改；
/// 新代码请直接用 `crate::proxy::pipeline::terminal::FailureStatusTracker`。
pub use crate::proxy::pipeline::terminal::FailureStatusTracker;

/// 全局请求预算已下沉到 Pipeline（`proxy::pipeline::budget`），四大协议适配器统一生效。
///
/// 这里仅保留兼容导出，避免既有引用点一次性大改；新代码请直接用
/// `crate::proxy::pipeline::budget::gateway_request_budget_secs()`。
pub use crate::proxy::pipeline::budget::{
    gateway_request_budget_secs,
    DEFAULT_GATEWAY_REQUEST_BUDGET_SECS as MAX_GATEWAY_REQUEST_BUDGET_SECS,
};

// ===== QoS / 错误解析已下沉到 Pipeline（协议无关）=====
pub use crate::proxy::pipeline::errors::{is_transient_token_error, parse_raw_upstream_error};
pub use crate::proxy::pipeline::qos::report_qos_ttft;

/// [Issue #3634, #3693] 统一自适应计算流预读（Peek）阶段的超时时间。
/// - Compaction 摘要请求：大 Prompt 处理，默认 180s；
/// - 图像生成请求：图像生成耗时较长（特别是 4K 高分辨率渲染），默认 90s；
/// - 思考/深度推理模型请求：思考阶段 TTFT 较长，底线 60s（大上下文 90s/120s）；
/// - 正常会话请求：根据估算 Token 数量动态自适应，避免大上下文 Prompt 预热（TTFT）超过 30s 被过早误杀导致全池轮换雪崩；
///   * > 100k tokens: 120s
///   * > 50k tokens: 90s
///   * > 20k tokens: 60s
///   * 默认: 30s
pub fn calculate_adaptive_peek_timeout(
    is_compaction: bool,
    is_image_gen: bool,
    estimated_tokens: Option<u32>,
) -> u64 {
    calculate_adaptive_peek_timeout_full(is_compaction, is_image_gen, false, estimated_tokens)
}

pub fn calculate_adaptive_peek_timeout_full(
    is_compaction: bool,
    is_image_gen: bool,
    is_thinking: bool,
    estimated_tokens: Option<u32>,
) -> u64 {
    // 分档表已收敛到 `pipeline::stream_peek`（与 upstream::client 的 TTFT 预算同源）
    crate::proxy::pipeline::stream_peek::peek_budget_secs(
        is_compaction,
        is_image_gen,
        is_thinking,
        estimated_tokens,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_short_429_preserves_rotation_budget_and_structured_status() {
        let body = r#"{"error":{"details":[{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"1s"}]}}"#;

        let drive_failures = |account_count| {
            let mut state = RequestRetryState::default();
            let mut used_attempts = 0;
            let mut retry_same_account = false;
            let mut sends = Vec::new();

            while let Some(attempt) =
                next_rotation_attempt(&mut used_attempts, account_count, retry_same_account)
            {
                retry_same_account = false;
                sends.push(attempt);
                let account_id = format!("account-{}", attempt);
                let strategy = state.determine_strategy(&account_id, 429, body, None, false);
                if matches!(strategy, RetryStrategy::GraceRetry(_)) {
                    assert!(!should_rotate_account(429, Some(&strategy)));
                    retry_same_account = true;
                } else {
                    assert!(should_rotate_account(429, Some(&strategy)));
                }
            }
            sends
        };

        assert_eq!(drive_failures(1), vec![0, 0]);
        assert_eq!(drive_failures(2), vec![0, 0, 1, 1]);

        let mut all_429 = FailureStatusTracker::default();
        all_429.record(StatusCode::TOO_MANY_REQUESTS);
        all_429.record(StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(all_429.final_status(), StatusCode::TOO_MANY_REQUESTS);

        for non_429 in [StatusCode::FORBIDDEN, StatusCode::SERVICE_UNAVAILABLE] {
            let mut mixed = FailureStatusTracker::default();
            mixed.record(non_429);
            mixed.record(StatusCode::TOO_MANY_REQUESTS);
            assert_eq!(mixed.final_status(), non_429);
        }

        let mut all_503 = FailureStatusTracker::default();
        all_503.record(StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(all_503.final_status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}

/// 入站模型闸门（协议无关）。
///
/// - 缺失 / 不存在的 model-id → 404 `model not exist`
/// - 整池无配额 → 502 `no quota accounts`
///
/// 拒绝时：响应头带上 `X-Account-Email` 哨兵文案（日志账号列直接展示），
/// 并把英文排障说明写入 `upstream_recorder`，使「转出报文」面板可展开查看。
pub async fn enforce_model_gate(
    state: &AppState,
    requested_model: Option<&str>,
    protocol: &str,
    upstream_recorder: Option<&crate::proxy::monitor::UpstreamRequestBodyHolder>,
) -> Result<String, Response> {
    let decision = crate::proxy::common::model_gate::gate_inbound_model(
        requested_model,
        &state.custom_mapping,
        &state.token_manager,
    )
    .await;

    let Some((status, reason)) = decision.rejection() else {
        return Ok(decision.allowed_model().unwrap_or_default().to_string());
    };

    let requested = requested_model.unwrap_or("").trim().to_string();
    tracing::warn!(
        "[ModelGate] Blocked inbound model '{}' ({}): HTTP {} - {}",
        requested,
        protocol,
        status.as_u16(),
        reason
    );

    // 把英文详细原因写入「转出报文」面板（原本这里会是空的“无中转报文”）
    if let Some(holder) = upstream_recorder {
        let explanation =
            crate::proxy::common::model_gate::blocked_explanation(reason, &requested, protocol);
        holder
            .set(serde_json::to_string_pretty(&explanation).unwrap_or_else(|_| reason.to_string()));
    }

    let body = build_dual_track_error(protocol, status.as_u16(), &requested, reason);
    Err((
        status,
        [
            ("X-Account-Email", reason),
            ("X-Mapped-Model", requested.as_str()),
            ("X-Gateway-Error", reason),
        ],
        Json(body),
    )
        .into_response())
}

/// 模型列表投影的公共输入（协议无关）
#[derive(Debug, Clone)]
pub struct ModelListEntry {
    pub id: String,
    pub context_limit: i64,
    pub output_limit: i64,
    pub display_name: Option<String>,
    pub metadata: Option<crate::models::quota::ModelListingMeta>,
}

/// 统一收集 `/v1/models`、`/v1/models/claude`、`/v1beta/models` 的共同数据
///
/// 三个协议过去各自重复“取动态模型 + 取元数据 + 回退 token 上限”三遍；
/// 这里只保留一份数据投影，协议层只负责最终 JSON 形状。
pub async fn collect_model_list_entries(
    state: &crate::proxy::server::AppState,
) -> Vec<ModelListEntry> {
    use crate::proxy::common::model_mapping::{
        get_all_dynamic_models, get_all_dynamic_models_metadata,
    };

    let only_raw = *state.only_raw_quota_models.read().await;
    let model_ids =
        get_all_dynamic_models(&state.custom_mapping, Some(&state.token_manager), only_raw).await;
    let metadata_map = get_all_dynamic_models_metadata(
        &state.custom_mapping,
        Some(&state.token_manager),
        only_raw,
    )
    .await;

    model_ids
        .into_iter()
        .map(|id| {
            let (fallback_ctx, fallback_out) =
                crate::proxy::handlers::openai::resolve_model_token_limits(&id);
            let meta = metadata_map
                .get(&id)
                .or_else(|| metadata_map.get(&id.to_lowercase()))
                .cloned();
            let context_limit = meta
                .as_ref()
                .and_then(|m| m.max_tokens)
                .map(|v| v as i64)
                .unwrap_or(fallback_ctx);
            let output_limit = meta
                .as_ref()
                .and_then(|m| m.max_output_tokens)
                .map(|v| v as i64)
                .unwrap_or(fallback_out);
            let display_name = meta.as_ref().and_then(|m| m.display_name.clone());
            ModelListEntry {
                id,
                context_limit,
                output_limit,
                display_name,
                metadata: meta,
            }
        })
        .collect()
}

/// OpenAI / Anthropic 兼容的模型对象投影
pub fn openai_style_model_objects(entries: Vec<ModelListEntry>) -> Vec<serde_json::Value> {
    entries
        .into_iter()
        .map(|entry| {
            let mut obj = serde_json::json!({
                "id": entry.id,
                "object": "model",
                "created": 1706745600,
                "owned_by": "antigravity",
                "context_length": entry.context_limit,
                "max_context_length": entry.context_limit,
                "max_input_tokens": entry.context_limit,
                "max_model_len": entry.context_limit,
                "max_tokens": entry.context_limit,
                "input_token_limit": entry.context_limit,
                "max_completion_tokens": entry.output_limit,
                "max_output_tokens": entry.output_limit,
                "output_token_limit": entry.output_limit,
            });
            if let Some(meta) = entry.metadata {
                if let Ok(serde_json::Value::Object(map)) = serde_json::to_value(meta) {
                    if let Some(target) = obj.as_object_mut() {
                        for (k, v) in map {
                            target.insert(k, v);
                        }
                    }
                }
            }
            obj
        })
        .collect()
}

/// Detects model capabilities and configuration
/// POST /v1/models/detect
pub async fn handle_detect_model(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Response {
    let model_name = body.get("model").and_then(|v| v.as_str()).unwrap_or("");

    if model_name.is_empty() {
        return (StatusCode::BAD_REQUEST, "Missing 'model' field").into_response();
    }

    // 1. Resolve mapping
    let mapped_model = crate::proxy::common::model_mapping::resolve_model_route(
        model_name,
        &*state.custom_mapping.read().await,
    );

    // 2. Resolve capabilities
    let config = crate::proxy::mappers::common_utils::resolve_request_config(
        model_name,
        &mapped_model,
        &None, // We don't check tools for static capability detection
        None,  // size
        None,  // quality
        None,  // image_size
        None,  // body (not needed for static detection)
    );

    // 3. Construct response
    let mut response = json!({
        "model": model_name,
        "mapped_model": mapped_model,
        "type": config.request_type,
        "features": {
            "has_web_search": config.inject_google_search,
            "is_image_gen": config.request_type == "image_gen"
        }
    });

    if let Some(img_conf) = config.image_config {
        if let Some(obj) = response.as_object_mut() {
            obj.insert("config".to_string(), img_conf);
        }
    }

    Json(response).into_response()
}

/// [Issue #3414 / #3634] 统一构造带有 X-Mapped-Model、可选 X-Account-Email 以及针对协议感知的 Retry-After HeaderMap
pub fn build_token_error_headers_for_protocol<'a>(
    protocol: &str,
    mapped_model: Option<&'a str>,
    account_email: Option<&'a str>,
    error_text: &str,
) -> axum::http::HeaderMap {
    use axum::http::header::{HeaderName, HeaderValue};
    let mut headers = axum::http::HeaderMap::new();

    if let Some(model) = mapped_model {
        if let Ok(val) = HeaderValue::from_str(model) {
            headers.insert(HeaderName::from_static("x-mapped-model"), val);
        }
    }
    if let Some(email) = account_email {
        if let Ok(val) = HeaderValue::from_str(email) {
            headers.insert(HeaderName::from_static("x-account-email"), val);
        }
    }

    let raw_sec = extract_retry_after_seconds(error_text);
    let sec = if protocol == "claude" {
        Some(clamp_retry_after_for_claude(raw_sec))
    } else {
        raw_sec
    };

    if let Some(s) = sec {
        if let Ok(val) = HeaderValue::from_str(&s.to_string()) {
            headers.insert(axum::http::header::RETRY_AFTER, val);
        }
    }
    headers
}

/// [Issue #3414] 统一构造带有 X-Mapped-Model、可选 X-Account-Email 以及 Retry-After 的 HeaderMap (通用协议兼容)
pub fn build_token_error_headers<'a>(
    mapped_model: Option<&'a str>,
    account_email: Option<&'a str>,
    error_text: &str,
) -> axum::http::HeaderMap {
    build_token_error_headers_for_protocol("common", mapped_model, account_email, error_text)
}

fn map_status_code_to_gemini_status(status_code: u16) -> &'static str {
    match status_code {
        400 => "INVALID_ARGUMENT",
        401 => "UNAUTHENTICATED",
        403 => "PERMISSION_DENIED",
        404 => "NOT_FOUND",
        429 => "RESOURCE_EXHAUSTED",
        499 => "CANCELLED",
        500 => "INTERNAL",
        501 => "NOT_IMPLEMENTED",
        503 => "UNAVAILABLE",
        504 => "DEADLINE_EXCEEDED",
        _ => "UPSTREAM_ERROR",
    }
}

/// 检测是否存在因自定义 User-Agent 过时或未采用官方 Hub 格式导致上游拒绝访问的特征
pub fn check_outdated_user_agent_issue(
    model: &str,
    status_code: u16,
    error_text: &str,
    custom_ua: Option<&str>,
) -> Option<(String, String, String, &'static str, &'static str)> {
    let lang = crate::modules::config::load_app_config()
        .ok()
        .map(|c| c.language)
        .unwrap_or_else(|| "zh".to_string());
    check_outdated_user_agent_issue_with_lang(
        model,
        status_code,
        error_text,
        custom_ua,
        Some(&lang),
    )
}

/// 是否为可通过修正 UA 配置解决的 404/403（缺失 Hub 签名 / Hub 版本过低）。
///
/// 这类错误不是账号/模型故障：不应写入 900s 单模型熔断，也不应换号重放；
/// 否则用户改好 UA 后仍会被旧熔断卡 15 分钟。
pub fn is_outdated_user_agent_error(
    model: &str,
    status_code: u16,
    error_text: &str,
    custom_ua: Option<&str>,
) -> bool {
    check_outdated_user_agent_issue(model, status_code, error_text, custom_ua)
        .map(|(_, _, _, _, code)| code == "outdated_user_agent")
        .unwrap_or(false)
}

/// 支持指定语言环境的 User-Agent 门禁与熔断特征检测
pub fn check_outdated_user_agent_issue_with_lang(
    model: &str,
    status_code: u16,
    error_text: &str,
    custom_ua: Option<&str>,
    lang: Option<&str>,
) -> Option<(String, String, String, &'static str, &'static str)> {
    let lower_err = error_text.to_lowercase();
    let is_suspended_pool = lower_err.contains("temporarily suspended for model");
    let is_not_found = is_model_not_found_error(status_code, error_text) || is_suspended_pool;
    let is_forbidden = status_code == 403
        || lower_err.contains("permission_denied")
        || lower_err.contains("forbidden");

    if !(is_not_found || is_forbidden) {
        return None;
    }

    let i18n_err = crate::modules::i18n::get_gateway_error_strings(lang.unwrap_or("zh"));

    if let Some(ua) = custom_ua {
        let trimmed_ua = ua.trim();
        if !trimmed_ua.is_empty() {
            // 获取当前动态解析的官方 Hub 客户端指纹基线（来自官方远程 releases 探测或稳态基准）
            let active_fp = crate::modules::hub_version::get_current_hub_fingerprint();
            let active_baseline_ver = active_fp.version.as_str();

            // 判定是否缺少官方 Hub 签名，或 Hub 版本低于当前官方最新基准
            let is_hub = trimmed_ua.starts_with("antigravity/hub/");
            let (is_risk, reason) = if is_hub {
                if let Some(caps) = crate::constants::parse_antigravity_version(trimmed_ua) {
                    if crate::constants::compare_semver(&caps, active_baseline_ver)
                        == std::cmp::Ordering::Less
                    {
                        (true, "outdated_version")
                    } else {
                        (false, "ok")
                    }
                } else {
                    (false, "ok")
                }
            } else {
                (true, "missing_hub_signature") // 纯 IDE 格式 (如 antigravity/4.3.0) 缺少 Hub 签名
            };

            if is_risk {
                let current_upstream_ua = crate::modules::hub_version::get_current_hub_user_agent();
                let (title, diagnosis, suggestion) = if reason == "missing_hub_signature" {
                    (
                        format!("{} [{}]", i18n_err.outdated_ua_missing_sig_title, model),
                        i18n_err
                            .outdated_ua_missing_sig_diag
                            .replace("{model}", model)
                            .replace("{status}", &status_code.to_string())
                            .replace("{ua}", trimmed_ua),
                        i18n_err
                            .outdated_ua_missing_sig_sug
                            .replace("{recommended_ua}", &current_upstream_ua),
                    )
                } else {
                    let user_ver = crate::constants::parse_antigravity_version(trimmed_ua)
                        .unwrap_or_else(|| "未知".to_string());
                    (
                        format!("{} [{}]", i18n_err.outdated_ua_low_ver_title, model),
                        i18n_err
                            .outdated_ua_low_ver_diag
                            .replace("{model}", model)
                            .replace("{status}", &status_code.to_string())
                            .replace("{ua}", trimmed_ua)
                            .replace("{user_version}", &user_ver)
                            .replace("{baseline_version}", active_baseline_ver),
                        i18n_err
                            .outdated_ua_low_ver_sug
                            .replace("{recommended_ua}", &current_upstream_ua)
                            .replace("{baseline_version}", active_baseline_ver),
                    )
                };

                return Some((
                    title,
                    diagnosis,
                    suggestion,
                    "invalid_request_error",
                    "outdated_user_agent",
                ));
            }
        }
    }

    if is_suspended_pool {
        return Some((
            format!("{} [{}]", i18n_err.model_pool_suspended_title, model),
            i18n_err.model_pool_suspended_diag.replace("{model}", model),
            i18n_err.model_pool_suspended_sug.to_string(),
            "rate_limit_error",
            "model_pool_suspended",
        ));
    }

    None
}

/// 格式化双轨制错误报文（四大标准协议原始字段散开 + 顶层/UI层不阉割双轨诊断）
pub fn build_dual_track_error(
    protocol: &str, // "claude", "openai", "responses", or "gemini"
    status_code: u16,
    model: &str,
    error_text: &str,
) -> serde_json::Value {
    // 动态提取当前生效的自定义 User-Agent 覆盖与语言环境
    let cfg = crate::modules::config::load_app_config().ok();
    let active_ua_override = cfg
        .as_ref()
        .and_then(|c| c.proxy.user_agent_override.clone());
    let lang = cfg
        .as_ref()
        .map(|c| c.language.clone())
        .unwrap_or_else(|| "zh".to_string());
    build_dual_track_error_with_ua_and_lang(
        protocol,
        status_code,
        model,
        error_text,
        active_ua_override.as_deref(),
        Some(&lang),
    )
}

/// 格式化双轨制错误报文（可显式注入 custom_ua 用于单元测试或环境注入）
pub fn build_dual_track_error_with_ua(
    protocol: &str,
    status_code: u16,
    model: &str,
    error_text: &str,
    custom_ua: Option<&str>,
) -> serde_json::Value {
    let lang = crate::modules::config::load_app_config()
        .ok()
        .map(|c| c.language)
        .unwrap_or_else(|| "zh".to_string());
    build_dual_track_error_with_ua_and_lang(
        protocol,
        status_code,
        model,
        error_text,
        custom_ua,
        Some(&lang),
    )
}

/// 格式化双轨制错误报文（支持显式注入自定义 UA 及当前会话语言环境）
pub fn build_dual_track_error_with_ua_and_lang(
    protocol: &str,
    status_code: u16,
    model: &str,
    error_text: &str,
    custom_ua: Option<&str>,
    lang: Option<&str>,
) -> serde_json::Value {
    let effective_lang = lang.unwrap_or("zh");
    let i18n_err = crate::modules::i18n::get_gateway_error_strings(effective_lang);

    // [Fixes #3763] 剔除内部链路控制标记，避免大体积上传终态标记泄漏到客户端报文
    let sanitized_error_text = crate::proxy::upstream::client::strip_internal_markers(error_text);
    let error_text = sanitized_error_text.as_str();
    let lower = error_text.to_lowercase();
    let is_internal_limited =
        crate::proxy::pipeline::policy::UpstreamClassification::is_internal_gateway_exhaustion(
            &lower,
        );

    let is_network_error = lower.contains("http request failed")
        || lower.contains("failed to connect")
        || lower.contains("connection refused")
        || lower.contains("connection reset")
        || lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("error trying to connect")
        || lower.contains("dns error")
        || lower.contains("all endpoints failed")
        // [Fixes #3763] 分阶段上传诊断归入网络类故障，保留真实阶段语义
        || lower.contains("upload stalled")
        || lower.contains("upload budget exhausted")
        || lower.contains("upstream send failed")
        || lower.contains("no response")
        || lower.contains("未收到请求回报");

    let is_not_found = is_model_not_found_error(status_code, error_text);

    // 账号被上游封禁 / 权限拒绝：与「容量耗尽」「模型不存在」严格区分，
    // 避免 403 被 Claude 协议映射为 503 后与真实容量问题混淆。
    let is_account_forbidden = lower.contains("tos_violation")
        || lower.contains("violation of terms of service")
        || lower.contains("disabled in this account")
        || (status_code == 403 && lower.contains("permission_denied"));

    // 1. 深度解析并反转义上游错误
    let (raw_upstream_msg, parsed_upstream) = parse_raw_upstream_error(error_text);

    let (readable_prefix, diagnosis, suggestion, err_type, err_code) = if let Some(ua_diag) =
        check_outdated_user_agent_issue_with_lang(
            model,
            status_code,
            error_text,
            custom_ua,
            Some(effective_lang),
        ) {
        ua_diag
    } else if is_account_forbidden {
        (
            i18n_err.account_forbidden_title.replace("{model}", model),
            i18n_err
                .account_forbidden_diag
                .replace("{error}", &error_text.chars().take(400).collect::<String>()),
            i18n_err.account_forbidden_sug.to_string(),
            "permission_error",
            "account_forbidden",
        )
    } else if is_internal_limited {
        (
            i18n_err.all_accounts_limited_title.to_string(),
            i18n_err
                .all_accounts_limited_diag
                .replace("{error}", error_text),
            i18n_err.all_accounts_limited_sug.to_string(),
            "rate_limit_error",
            "all_accounts_limited",
        )
    } else if is_network_error {
        (
            i18n_err.upstream_network_error_title.to_string(),
            i18n_err
                .upstream_network_error_diag
                .replace("{error}", error_text),
            i18n_err.upstream_network_error_sug.to_string(),
            "network_error",
            "upstream_network_error",
        )
    } else if is_not_found {
        (
            i18n_err.model_not_found_title.replace("{model}", model),
            i18n_err.model_not_found_diag.replace("{model}", model),
            i18n_err.model_not_found_sug.to_string(),
            "invalid_request_error",
            "model_not_found",
        )
    } else if status_code == 429 || status_code == 529 {
        (
            i18n_err
                .rate_limit_title
                .replace("{status}", &status_code.to_string()),
            i18n_err.rate_limit_diag.replace("{model}", model),
            i18n_err.rate_limit_sug.to_string(),
            "rate_limit_error",
            "rate_limit_exceeded",
        )
    } else {
        (
            i18n_err
                .upstream_error_title
                .replace("{status}", &status_code.to_string()),
            i18n_err
                .upstream_error_diag
                .replace("{model}", model)
                .replace("{status}", &status_code.to_string()),
            i18n_err.upstream_error_sug.to_string(),
            "api_error",
            "upstream_error",
        )
    };

    let readable_message = format!(
        "{} {} {} {} {}",
        readable_prefix, i18n_err.label_diagnosis, diagnosis, i18n_err.label_suggestion, suggestion
    );

    // 2. 确定散开到各自协议标准 message 字段的内容：
    //    若是网关自身限制或网络异常（无有效上游业务报文参与），则使用网关诊断文本；
    //    若是上游发生的真实报错，100% 保持上游原始报错（raw_upstream_msg），绝不被网关硬编码覆盖！
    let effective_message =
        if is_internal_limited || is_network_error || raw_upstream_msg.trim().is_empty() {
            readable_message.clone()
        } else {
            raw_upstream_msg
        };

    let gateway_error_obj = serde_json::json!({
        "error_code": err_code,
        "model": model,
        "is_server_error": false,
        "diagnosis": diagnosis,
        "suggestion": suggestion,
        "readable_summary": readable_message
    });

    let upstream_error_obj = serde_json::json!({
        "status": status_code,
        "is_server_error": false,
        "response": parsed_upstream
    });

    match protocol {
        "claude" => serde_json::json!({
            "type": "error",
            "error": {
                "type": err_type,
                "code": err_code,
                "message": effective_message,
                "is_server_error": false,
                "gateway_error": gateway_error_obj,
                "upstream_error": upstream_error_obj
            }
        }),
        "openai" => serde_json::json!({
            "error": {
                "message": effective_message,
                "type": err_type,
                "param": serde_json::Value::Null,
                "code": err_code,
                "is_server_error": false,
                "gateway_error": gateway_error_obj,
                "upstream_error": upstream_error_obj
            }
        }),
        "responses" => serde_json::json!({
            "error": {
                "message": effective_message,
                "type": err_type,
                "code": err_code,
                "is_server_error": false,
                "gateway_error": gateway_error_obj,
                "upstream_error": upstream_error_obj
            }
        }),
        "gemini" => {
            let upstream_status = parsed_upstream
                .get("error")
                .and_then(|e| e.get("status"))
                .and_then(|s| s.as_str())
                .unwrap_or_else(|| map_status_code_to_gemini_status(status_code));

            serde_json::json!({
                "error": {
                    "code": status_code,
                    "message": effective_message,
                    "status": upstream_status,
                    "is_server_error": false,
                    "gateway_error": gateway_error_obj,
                    "upstream_error": upstream_error_obj
                }
            })
        }
        _ => serde_json::json!({
            "error": {
                "message": effective_message,
                "type": err_type,
                "code": err_code,
                "is_server_error": false,
                "gateway_error": gateway_error_obj,
                "upstream_error": upstream_error_obj
            }
        }),
    }
}

/// 将端点尝试记录列表安全序列化并编码为 Base64，便于注入 HTTP 响应头
pub fn encode_attempts_header(
    attempts: &[crate::proxy::upstream::client::FallbackAttemptLog],
) -> Option<String> {
    if attempts.is_empty() {
        return None;
    }
    use base64::Engine;
    serde_json::to_string(attempts)
        .ok()
        .map(|json_str| base64::engine::general_purpose::STANDARD.encode(json_str.as_bytes()))
}

#[cfg(test)]
mod retry_after_tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_extract_retry_after_seconds() {
        assert_eq!(
            extract_retry_after_seconds("All accounts limited. Wait 29s."),
            Some(29)
        );
        assert_eq!(
            extract_retry_after_seconds("Token error: All accounts limited. Wait 5s."),
            Some(5)
        );
        assert_eq!(
            extract_retry_after_seconds("All accounts rate-limited. Cooldown wait 27s."),
            Some(27)
        );
        assert_eq!(extract_retry_after_seconds("Token pool is empty"), None);
        assert_eq!(
            extract_retry_after_seconds("All accounts failed or unhealthy."),
            None
        );
    }

    #[test]
    fn test_clamp_retry_after_for_claude() {
        assert_eq!(clamp_retry_after_for_claude(Some(45)), 12);
        assert_eq!(clamp_retry_after_for_claude(Some(29)), 12);
        assert_eq!(clamp_retry_after_for_claude(Some(1800)), 12);
        assert_eq!(clamp_retry_after_for_claude(Some(5)), 5);
        assert_eq!(clamp_retry_after_for_claude(Some(0)), 8);
        assert_eq!(clamp_retry_after_for_claude(None), 8);
    }

    #[test]
    fn test_calculate_adaptive_peek_timeout() {
        // 1. 压缩摘要请求优先给予最宽限额 (180s)
        assert_eq!(calculate_adaptive_peek_timeout(true, false, None), 180);
        assert_eq!(
            calculate_adaptive_peek_timeout(true, false, Some(10_000)),
            180
        );

        // 2. 图像生成请求给予 90s
        assert_eq!(calculate_adaptive_peek_timeout(false, true, None), 90);

        // 3. 正常会话根据 token 规模自适应阶梯分配
        assert_eq!(
            calculate_adaptive_peek_timeout(false, false, Some(120_000)),
            120
        );
        assert_eq!(
            calculate_adaptive_peek_timeout(false, false, Some(100_000)),
            120
        );
        assert_eq!(
            calculate_adaptive_peek_timeout(false, false, Some(75_000)),
            90
        );
        assert_eq!(
            calculate_adaptive_peek_timeout(false, false, Some(50_000)),
            90
        );
        assert_eq!(
            calculate_adaptive_peek_timeout(false, false, Some(35_000)),
            60
        );
        assert_eq!(
            calculate_adaptive_peek_timeout(false, false, Some(20_000)),
            60
        );
        assert_eq!(
            calculate_adaptive_peek_timeout(false, false, Some(10_000)),
            30
        );
        assert_eq!(calculate_adaptive_peek_timeout(false, false, None), 30);

        // 4. 思考/推理模型赋予至少 60s 底线，防止 30s 误杀
        assert_eq!(
            calculate_adaptive_peek_timeout_full(false, false, true, None),
            60
        );
        assert_eq!(
            calculate_adaptive_peek_timeout_full(false, false, true, Some(10_000)),
            60
        );
        assert_eq!(
            calculate_adaptive_peek_timeout_full(false, false, true, Some(60_000)),
            90
        );
        assert_eq!(
            calculate_adaptive_peek_timeout_full(false, false, true, Some(120_000)),
            120
        );
    }

    #[test]
    fn test_503_529_backoff_is_tightly_bounded() {
        // 多账号第一轮 (attempt < pool_size): 快速轮换 (50ms)
        let s1 = determine_retry_strategy_adaptive(
            529,
            "The model is overloaded.",
            None,
            false,
            true,
            0,
            3,
        );
        assert_eq!(s1, RetryStrategy::FixedDelay(Duration::from_millis(50)));

        // 多账号第二轮 (attempt >= pool_size): 指数退避必须严格限制在 1s~4s 内，绝不死等 30s 阻塞客户端
        let s2 = determine_retry_strategy_adaptive(
            529,
            "The model is overloaded.",
            None,
            false,
            true,
            3,
            3,
        );
        match s2 {
            RetryStrategy::ExponentialBackoff { base_ms, max_ms } => {
                assert_eq!(base_ms, 1000);
                assert_eq!(max_ms, 4000);
            }
            other => panic!("Expected ExponentialBackoff, got {:?}", other),
        }
    }

    #[test]
    fn test_build_token_error_headers_for_protocol_claude() {
        let headers_clamped = build_token_error_headers_for_protocol(
            "claude",
            Some("claude-3-7-sonnet"),
            Some("test@example.com"),
            "All accounts limited. Wait 45s.",
        );
        assert_eq!(
            headers_clamped
                .get("retry-after")
                .unwrap()
                .to_str()
                .unwrap(),
            "12"
        );

        let headers_fallback = build_token_error_headers_for_protocol(
            "claude",
            Some("claude-3-7-sonnet"),
            None,
            "All accounts failed or unhealthy.",
        );
        // Claude 协议绝对不能缺少 retry-after，必须兜底下发 safe 范围秒数 (8s)
        assert_eq!(
            headers_fallback
                .get("retry-after")
                .unwrap()
                .to_str()
                .unwrap(),
            "8"
        );

        let headers_gemini = build_token_error_headers_for_protocol(
            "gemini",
            Some("gemini-2.5-pro"),
            None,
            "All accounts limited. Wait 45s.",
        );
        // Gemini / 通用协议如实下发原始提取值
        assert_eq!(
            headers_gemini.get("retry-after").unwrap().to_str().unwrap(),
            "45"
        );

        let headers_gemini_no_wait = build_token_error_headers_for_protocol(
            "gemini",
            Some("gemini-2.5-pro"),
            None,
            "All accounts failed or unhealthy.",
        );
        assert!(headers_gemini_no_wait.get("retry-after").is_none());
    }

    #[test]
    fn test_build_token_error_headers() {
        let headers = build_token_error_headers(
            Some("gemini-2.5-pro"),
            Some("test@example.com"),
            "All accounts limited. Wait 45s.",
        );
        assert_eq!(
            headers.get("x-mapped-model").unwrap().to_str().unwrap(),
            "gemini-2.5-pro"
        );
        assert_eq!(
            headers.get("x-account-email").unwrap().to_str().unwrap(),
            "test@example.com"
        );
        assert_eq!(headers.get("retry-after").unwrap().to_str().unwrap(), "45");

        let headers_no_wait = build_token_error_headers(
            Some("gemini-2.5-pro"),
            None,
            "All accounts failed or unhealthy.",
        );
        assert!(headers_no_wait.get("retry-after").is_none());
        assert_eq!(
            headers_no_wait
                .get("x-mapped-model")
                .unwrap()
                .to_str()
                .unwrap(),
            "gemini-2.5-pro"
        );
    }

    #[test]
    fn test_parse_raw_upstream_error_with_http_prefix_and_escapes() {
        let raw = r#"HTTP 400: {"error":{"code":400,"message":"API key not valid. Please pass a valid API key.","status":"INVALID_ARGUMENT"}}"#;
        let (msg, parsed) = parse_raw_upstream_error(raw);
        assert_eq!(msg, "API key not valid. Please pass a valid API key.");
        assert_eq!(parsed["error"]["code"], 400);
        assert_eq!(parsed["error"]["status"], "INVALID_ARGUMENT");
    }

    #[test]
    fn test_parse_raw_upstream_error_double_escaped() {
        let raw = r#""{\"error\":{\"code\":403,\"message\":\"User location not supported.\"}}""#;
        let (msg, parsed) = parse_raw_upstream_error(raw);
        assert_eq!(msg, "User location not supported.");
        assert_eq!(parsed["error"]["code"], 403);
    }

    #[test]
    fn test_build_dual_track_error_four_protocols() {
        let raw_err = r#"HTTP 404: {"error":{"code":404,"message":"models/gemini-not-exist is not found","status":"NOT_FOUND"}}"#;

        // 1. Claude
        let claude_res = build_dual_track_error("claude", 404, "gemini-not-exist", raw_err);
        assert_eq!(claude_res["type"], "error");
        // 标准协议底层字段散开为上游原始 message
        assert_eq!(
            claude_res["error"]["message"],
            "models/gemini-not-exist is not found"
        );
        assert_eq!(claude_res["error"]["code"], "model_not_found");
        // UI层双轨制诊断不阉割
        assert!(claude_res["error"]["gateway_error"]["diagnosis"].is_string());
        assert_eq!(claude_res["error"]["upstream_error"]["status"], 404);
        assert_eq!(
            claude_res["error"]["upstream_error"]["response"]["error"]["status"],
            "NOT_FOUND"
        );

        // 2. OpenAI
        let openai_res = build_dual_track_error("openai", 404, "gemini-not-exist", raw_err);
        assert_eq!(
            openai_res["error"]["message"],
            "models/gemini-not-exist is not found"
        );
        assert_eq!(openai_res["error"]["code"], "model_not_found");
        assert!(openai_res["error"]["gateway_error"].is_object());
        assert!(openai_res["error"]["upstream_error"].is_object());

        // 3. Responses
        let resp_res = build_dual_track_error("responses", 404, "gemini-not-exist", raw_err);
        assert_eq!(
            resp_res["error"]["message"],
            "models/gemini-not-exist is not found"
        );
        assert_eq!(resp_res["error"]["code"], "model_not_found");
        assert!(resp_res["error"]["gateway_error"].is_object());

        // 4. Gemini
        let gemini_res = build_dual_track_error("gemini", 404, "gemini-not-exist", raw_err);
        assert_eq!(gemini_res["error"]["code"], 404);
        assert_eq!(
            gemini_res["error"]["message"],
            "models/gemini-not-exist is not found"
        );
        assert_eq!(gemini_res["error"]["status"], "NOT_FOUND");
        assert!(gemini_res["error"]["gateway_error"].is_object());
        assert!(gemini_res["error"]["upstream_error"].is_object());
    }

    #[test]
    fn test_build_dual_track_error_internal_limited() {
        let raw_err = "All accounts limited. Wait 30s.";
        let claude_res = build_dual_track_error("claude", 429, "gemini-2.5-flash", raw_err);
        assert_eq!(claude_res["error"]["code"], "all_accounts_limited");
        assert!(claude_res["error"]["message"]
            .as_str()
            .unwrap()
            .contains("网关调度受限"));
    }

    #[test]
    fn test_build_dual_track_error_network_error() {
        let raw_err =
            "HTTP request failed at https://alkalicognition.googleapis.com: failed to connect";
        let res = build_dual_track_error("claude", 502, "claude-sonnet-4-6", raw_err);
        assert_eq!(res["error"]["code"], "upstream_network_error");
        assert_eq!(res["error"]["gateway_error"]["is_server_error"], false);
        assert_eq!(res["error"]["is_server_error"], false);
        assert!(res["error"]["message"]
            .as_str()
            .unwrap()
            .contains("非服务端故障"));
    }

    #[test]
    fn test_is_transient_token_error() {
        assert!(is_transient_token_error(
            "Token acquisition timeout (15s) - system too busy or deadlock detected"
        ));
        assert!(is_transient_token_error(
            "Token acquisition timeout (5s) - system too busy or deadlock detected"
        ));
        assert!(is_transient_token_error("System too busy"));
        assert!(is_transient_token_error("Potential deadlock detected"));
        assert!(!is_transient_token_error("Token pool is empty"));
        assert!(!is_transient_token_error(
            "invalid_grant: refresh token revoked"
        ));
        assert!(!is_transient_token_error("No available account with quota"));
    }

    #[test]
    fn test_failure_status_tracker_prevents_429_leak_on_timeout() {
        use reqwest::StatusCode;

        // 场景 1：首轮遭遇 429，后续轮换遭遇 504 超时或网络异常
        let mut tracker = FailureStatusTracker::default();
        tracker.record(StatusCode::TOO_MANY_REQUESTS);
        tracker.record(StatusCode::GATEWAY_TIMEOUT);
        // 必须返回 504，绝不能残留 429 导致客户端指数退避 1 分钟冷冻
        assert_eq!(tracker.final_status(), StatusCode::GATEWAY_TIMEOUT);

        // 场景 2：所有账号全量 429
        let mut all_429_tracker = FailureStatusTracker::default();
        all_429_tracker.record(StatusCode::TOO_MANY_REQUESTS);
        all_429_tracker.record(StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            all_429_tracker.final_status(),
            StatusCode::TOO_MANY_REQUESTS
        );
    }

    #[test]
    fn test_determine_retry_strategy_adaptive_google_multi_day_quota_exhaustion() {
        use reqwest::StatusCode;
        use std::time::Duration;

        let google_exhausted_body = r#"{
            "error": {
                "code": 429,
                "message": "Resource has been exhausted (e.g. check quota).",
                "status": "RESOURCE_EXHAUSTED",
                "details": [
                    {
                        "@type": "type.googleapis.com/google.rpc.ErrorInfo",
                        "reason": "QUOTA_EXHAUSTED",
                        "domain": "googleapis.com",
                        "metadata": {
                            "quotaResetDelay": "68h25m14.683204506s",
                            "service": "cloudcode-pa.googleapis.com"
                        }
                    },
                    {
                        "@type": "type.googleapis.com/google.rpc.RetryInfo",
                        "retryDelay": "246314.683204506s"
                    }
                ]
            }
        }"#;

        // 多账号环境下，明确标记为硬配额枯竭，立即切换下一账号 (50ms)
        let strategy =
            determine_retry_strategy_adaptive(429, google_exhausted_body, None, false, true, 0, 5);
        match strategy {
            RetryStrategy::FixedDelay(d) => {
                assert_eq!(d, Duration::from_millis(50));
            }
            other => panic!("Expected FixedDelay(50ms), got {:?}", other),
        }
    }

    #[test]
    fn test_check_outdated_user_agent_issue_detects_legacy_ide_ua() {
        let legacy_ua = "antigravity/4.3.0 darwin/arm64";
        let raw_404 = r#"HTTP 404: {"error":{"code":404,"message":"Requested entity was not found.","status":"NOT_FOUND"}}"#;

        // 场景 1: 上游直接 404 (中文测试)
        let res_zh = build_dual_track_error_with_ua_and_lang(
            "openai",
            404,
            "gemini-4-argon-low",
            raw_404,
            Some(legacy_ua),
            Some("zh"),
        );
        assert_eq!(res_zh["error"]["code"], "outdated_user_agent");
        let diag_zh = res_zh["error"]["gateway_error"]["diagnosis"]
            .as_str()
            .unwrap();
        assert!(diag_zh.contains("缺少官方 Hub 签名"));
        assert!(diag_zh.contains("antigravity/4.3.0 darwin/arm64"));
        assert_eq!(
            res_zh["error"]["gateway_error"]["error_code"],
            "outdated_user_agent"
        );

        // 场景 1b: 英文测试
        let res_en = build_dual_track_error_with_ua_and_lang(
            "openai",
            404,
            "gemini-4-argon-low",
            raw_404,
            Some(legacy_ua),
            Some("en"),
        );
        assert_eq!(res_en["error"]["code"], "outdated_user_agent");
        let diag_en = res_en["error"]["gateway_error"]["diagnosis"]
            .as_str()
            .unwrap();
        assert!(diag_en.contains("lacks the official Hub signature"));

        // 场景 2: 因所有账号 404 导致全池挂起熔断 (HTTP 503)
        let suspended_err =
            "All candidate accounts temporarily suspended for model: gemini-4-argon-low";
        let res_503 = build_dual_track_error_with_ua_and_lang(
            "openai",
            503,
            "gemini-4-argon-low",
            suspended_err,
            Some(legacy_ua),
            Some("zh"),
        );
        assert_eq!(res_503["error"]["code"], "outdated_user_agent");
        assert!(res_503["error"]["gateway_error"]["readable_summary"]
            .as_str()
            .unwrap()
            .contains("【上游指纹门禁异常】"));

        // 模型-not-found 分支使用这个纯判定：UA 风险时不得写 900s 熔断/换号。
        assert!(is_outdated_user_agent_error(
            "gemini-4-argon-high",
            404,
            raw_404,
            Some(legacy_ua)
        ));
        assert!(!is_outdated_user_agent_error(
            "gemini-4-argon-high",
            404,
            raw_404,
            Some(
                "antigravity/hub/2.28.0 (aidev_client; os_type=windows; arch=amd64; cl=1008300195)"
            )
        ));
    }

    #[test]
    fn test_check_outdated_user_agent_issue_detects_low_version_hub_ua() {
        // 具有官方 Hub 签名，但版本号 (2.11.0) 低于当前推荐默认基准 (OFFICIAL_HUB_VERSION 2.28.0)
        let low_hub_ua =
            "antigravity/hub/2.11.0 (aidev_client; os_type=windows; arch=amd64; cl=1008300195)";
        let raw_404 = r#"HTTP 404: {"error":{"code":404,"message":"Requested entity was not found.","status":"NOT_FOUND"}}"#;

        let res = build_dual_track_error_with_ua_and_lang(
            "claude",
            404,
            "gemini-4-argon",
            raw_404,
            Some(low_hub_ua),
            Some("zh"),
        );
        assert_eq!(res["error"]["code"], "outdated_user_agent");
        let diag = res["error"]["gateway_error"]["diagnosis"].as_str().unwrap();
        assert!(diag.contains("2.11.0"));
        assert!(diag.contains("低于当前官方推荐默认基准"));
    }

    #[test]
    fn test_check_outdated_user_agent_issue_passes_modern_hub_ua() {
        let modern_ua =
            "antigravity/hub/2.28.0 (aidev_client; os_type=windows; arch=amd64; cl=1008300195)";
        let raw_404 = r#"HTTP 404: {"error":{"code":404,"message":"Requested entity was not found.","status":"NOT_FOUND"}}"#;

        let res = build_dual_track_error_with_ua(
            "claude",
            404,
            "gemini-non-existent",
            raw_404,
            Some(modern_ua),
        );
        // 现代 Hub UA 不应误判为 outdated_user_agent，正常走 model_not_found
        assert_eq!(res["error"]["code"], "model_not_found");
    }

    #[test]
    fn test_check_outdated_user_agent_issue_detects_suspended_pool_without_custom_ua() {
        let suspended_err =
            "All candidate accounts temporarily suspended for model: gemini-4-argon-low";
        let res = build_dual_track_error_with_ua_and_lang(
            "openai",
            503,
            "gemini-4-argon-low",
            suspended_err,
            None,
            Some("zh"),
        );
        // 无自定义 UA 时，全池临时熔断给出清晰的 model_pool_suspended 诊断
        assert_eq!(res["error"]["code"], "model_pool_suspended");
        assert!(res["error"]["gateway_error"]["readable_summary"]
            .as_str()
            .unwrap()
            .contains("【模型全池临时熔断】"));
    }
}
