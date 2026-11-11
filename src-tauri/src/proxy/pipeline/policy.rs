/// 上游响应与错误在统一流水线中的唯一判定分类
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpstreamClassification {
    /// 真正的上游配额耗尽/速率限制（仅限 429 或 529）
    RateLimited { retry_after: Option<u64> },
    /// 模型不存在或不支持（404 或 模型不存在文本错误）
    ModelNotFound,
    /// 瞬时服务器故障（500/503等，属于服务故障，只换号/退避，绝不打入账号冷却池）
    TransientServerError,
    /// 网关内部生成的自产生错误（如 All accounts limited / Token pool is empty 等，绝不自噬锁定）
    InternalGatewayMessage,
    /// 思考签名失效或跨模型异构签名污染错误（400 Invalid signature in thinking block）
    ThoughtSignatureError,
    /// 上游多模态图像解码或处理失败错误（400 Unable to process input image 等）
    ImageProcessingError,
    /// 其他普通客户端错误
    OtherClientError(u16),
}

/// 429 结构化 details 信号（优先于消息关键词）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Structured429Signal {
    /// 请求级限流 / 资源耗尽
    RequestLevel,
    /// 账号级硬配额耗尽
    HardQuota,
    /// 明确的重试延迟（RetryInfo）
    ExplicitDelay,
    /// 有 details 但无法识别
    Unknown,
}

impl UpstreamClassification {
    /// 统一分类器：协议无关，全局唯一真理
    pub fn classify(status: u16, body: &str, retry_after_header: Option<&str>) -> Self {
        let lower = body.to_lowercase();
        // 0. 网关自答（model not exist / no quota accounts）最高优先级豁免：
        //    它们不对应任何账号，绝不允许触发换号、粘性解绑或账号冷却。
        if crate::proxy::common::model_gate::is_gateway_self_answer(body) {
            return UpstreamClassification::InternalGatewayMessage;
        }

        // 1. 优先排除网关自身内部错误（防自噬死循环与假 503 误报）
        if Self::is_internal_gateway_exhaustion(&lower) {
            return UpstreamClassification::InternalGatewayMessage;
        }

        // 2. 判定模型不存在 (广义模式匹配：无论 status 是 404, 400 还是上游服务端抛出的 500/503)
        let has_model_not_found_cue = lower.contains("model not found")
            || lower.contains("unknown model")
            || lower.contains("does not exist")
            || lower.contains("is not found")
            || lower.contains("unsupported model")
            || lower.contains("not found for api version")
            || lower.contains("publisher model")
            || lower.contains("model_not_found")
            || lower.contains("no such model")
            || lower.contains("invalid model")
            || lower.contains("model is not available");

        if status == 404 || has_model_not_found_cue {
            return UpstreamClassification::ModelNotFound;
        }

        // 3. 判定 Thinking 签名失效或跨模型异构签名污染
        //
        // cue 集合是四协议的并集（Claude 适配器历史上维护了最宽的一份，
        // 包含字段缺失 / 反序列化失败 / 块顺序错误等上游原始措辞）。
        // 任何新增方言都应补到这里，而不是在各适配器里再写一份 contains。
        let has_signature_error_cue = lower.contains("invalid thought signature")
            || lower.contains("invalid `signature`")
            || lower.contains("invalid signature")
            || lower.contains("thought_signature")
            || lower.contains("thoughtsignature")
            || lower.contains("thinking.signature")
            || lower.contains("thinking.thinking")
            || lower.contains("corrupted thought signature")
            || lower.contains("failed to deserialise")
            || lower.contains("thinking block")
            || lower.contains("found `text`")
            || lower.contains("found 'text'")
            || lower.contains("must be `thinking`")
            || lower.contains("must be 'thinking'");

        if status == 400 && has_signature_error_cue {
            return UpstreamClassification::ThoughtSignatureError;
        }

        // 4. 判定上游图像解码/处理失败 (400 Unable to process input image)
        if Self::is_image_processing_error_str(status, body) {
            return UpstreamClassification::ImageProcessingError;
        }

        // 5. 判定真正的限流 (仅限 429 和 529)
        if status == 429 || status == 529 {
            let delay = crate::proxy::upstream::retry::parse_retry_delay(body, retry_after_header)
                .map(|ms| ms.saturating_add(999) / 1000);
            return UpstreamClassification::RateLimited { retry_after: delay };
        }

        // 5. 瞬时故障 (500 / 503 等，绝非账号限流)
        if status == 500 || status == 503 {
            return UpstreamClassification::TransientServerError;
        }

        UpstreamClassification::OtherClientError(status)
    }

    /// 统一判定是否为网关内部账号池耗尽/限流错误（杜绝假 503 误报）
    pub fn is_internal_gateway_exhaustion(lower: &str) -> bool {
        (lower.contains("all accounts")
            && (lower.contains("limited")
                || lower.contains("rate-limited")
                || lower.contains("rate_limited")
                || lower.contains("failed")
                || lower.contains("unhealthy")
                || lower.contains("exhausted")
                || lower.contains("suspended")))
            || lower.contains("no accounts available")
            || lower.contains("token pool is empty")
            || lower.contains("no quota accounts")
    }

    /// 该分类是否应该触发 TokenManager 的账号冷却锁定
    pub fn should_lock_account(&self) -> bool {
        matches!(self, UpstreamClassification::RateLimited { .. })
    }

    /// 无 retryDelay / Retry-After 的请求级 429（Google 通用 RESOURCE_EXHAUSTED）。
    /// 它不是账号级硬配额枯竭；首轮不应把账号打入冷却，否则小账号池会被逐一连坐。
    pub fn is_request_level_429_without_retry_delay(
        status: u16,
        body: &str,
        retry_after_header: Option<&str>,
    ) -> bool {
        if status != 429 || retry_after_header.is_some() {
            return false;
        }
        // 明确 retryDelay / quotaResetDelay：不是“无延迟”形态。
        if crate::proxy::upstream::retry::parse_retry_delay(body, retry_after_header).is_some() {
            return false;
        }
        // 结构化 details 优先：QuotaFailure → 硬配额；RateLimit/ErrorInfo → 请求级。
        if let Some(signal) = Self::structured_429_signal(body) {
            return matches!(
                signal,
                Structured429Signal::RequestLevel | Structured429Signal::Unknown
            );
        }
        // 无结构化 details 时的兜底关键词表（与 rotation.rs 的既有判定保持一致）。
        let lower = body.to_lowercase();
        let is_hard_quota = lower.contains("exceeded your current quota")
            || lower.contains("insufficient_quota")
            || lower.contains("weekly quota")
            || lower.contains("daily quota")
            || lower.contains("per day")
            || lower.contains("individual quota")
            || lower.contains("quotaresetdelay");
        !is_hard_quota
    }

    /// 解析 429 body 中的结构化 details：
    /// - `QuotaFailure` / quota 类 reason → `HardQuota`
    /// - `RetryInfo` → `ExplicitDelay`
    /// - `ErrorInfo` 的 rate-limit / resource-exhausted reason → `RequestLevel`
    /// - details 存在但无法识别 → `Unknown`
    /// - 无 details → None（回退关键词表）
    pub(crate) fn structured_429_signal(body: &str) -> Option<Structured429Signal> {
        let value: serde_json::Value = serde_json::from_str(body).ok()?;
        let error = value.get("error").unwrap_or(&value);
        let details = error.get("details")?.as_array()?;
        if details.is_empty() {
            return None;
        }

        let mut has_quota_failure = false;
        let mut has_retry_info = false;
        let mut has_rate_limit = false;
        for detail in details {
            let type_lower = detail
                .get("@type")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_lowercase();
            let reason_lower = detail
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_lowercase();

            if type_lower.contains("quotafailure")
                || reason_lower.contains("quota_exhausted")
                || reason_lower.contains("daily_limit")
                || reason_lower.contains("weekly_limit")
                || reason_lower.contains("individual_quota")
                || reason_lower.contains("quota_exceeded")
            {
                has_quota_failure = true;
            }
            if type_lower.contains("retryinfo") {
                has_retry_info = true;
            }
            if reason_lower.contains("rate_limit")
                || reason_lower.contains("resource_exhausted")
                || reason_lower.contains("rate_exceeded")
                || reason_lower.contains("throttled")
            {
                has_rate_limit = true;
            }
        }

        if has_quota_failure {
            Some(Structured429Signal::HardQuota)
        } else if has_retry_info {
            Some(Structured429Signal::ExplicitDelay)
        } else if has_rate_limit {
            Some(Structured429Signal::RequestLevel)
        } else {
            // details 存在但不是已知的硬配额信号：按请求级处理，避免误锁账号。
            Some(Structured429Signal::Unknown)
        }
    }

    /// 带 attempt 的冷却锁定裁决：请求级无延迟 429 的首轮不锁账号，
    /// 让 Rotation 的 GraceRetry 有机会原地恢复；从第二轮起恢复正常锁定。
    pub fn should_lock_account_on_attempt(
        &self,
        status: u16,
        body: &str,
        retry_after_header: Option<&str>,
        attempt: usize,
    ) -> bool {
        self.should_lock_account()
            && !(attempt == 0
                && Self::is_request_level_429_without_retry_delay(status, body, retry_after_header))
    }

    /// 这个账号不能再被当前会话粘住。401 / 403 / 404 / 429 / 529。
    pub fn abandons_sticky_account(&self) -> bool {
        matches!(
            self,
            UpstreamClassification::RateLimited { .. }
                | UpstreamClassification::ModelNotFound
                | UpstreamClassification::OtherClientError(401 | 403 | 404 | 429)
        )
    }

    /// 该分类是否为模型不存在
    pub fn is_model_not_found(&self) -> bool {
        matches!(self, UpstreamClassification::ModelNotFound)
    }

    /// 该分类是否为思考签名失效/异构签名污染
    pub fn is_thought_signature_error(&self) -> bool {
        matches!(self, UpstreamClassification::ThoughtSignatureError)
    }

    /// 该分类是否为上游图像解码/处理失败
    pub fn is_image_processing_error(&self) -> bool {
        matches!(self, UpstreamClassification::ImageProcessingError)
    }

    /// 该分类是否为网关自身内部消息
    pub fn is_internal_gateway_message(&self) -> bool {
        matches!(self, UpstreamClassification::InternalGatewayMessage)
    }

    /// 判断是否为上游多模态图像解码或处理错误 (400 "Unable to process input image")
    pub fn is_image_processing_error_str(status: u16, body: &str) -> bool {
        if status != 400 {
            return false;
        }
        let lower = body.to_lowercase();
        lower.contains("unable to process input image")
            || lower.contains("failed to process input image")
            || lower.contains("cannot process input image")
            || lower.contains("could not process image")
            || lower.contains("error processing input image")
            || lower.contains("invalid input image")
            || lower.contains("unable to decode image")
            || lower.contains("unsupported image format")
            || lower.contains("image decoding failed")
            || (lower.contains("process input image")
                && lower.contains("developers.generativeai.google"))
    }

    /// 判断是否为上游服务过载（HTTP 503 / 529，或 500/504 等返回 "model is overloaded"）
    pub fn is_server_overloaded(status: u16, body: &str) -> bool {
        status == 503
            || status == 529
            || ((status >= 500 || status == 408) && body.to_lowercase().contains("overloaded"))
    }

    /// 是否为强制工具调用与思考冲突错误 (400 "Thinking may not be enabled when tool_choice forces tool use", Fixes #3721)
    pub fn is_thinking_tool_choice_conflict(status: u16, body: &str) -> bool {
        if status != 400 {
            return false;
        }
        let lower = body.to_lowercase();
        lower.contains("thinking may not be enabled when tool_choice forces tool use")
            || (lower.contains("thinking")
                && lower.contains("tool_choice")
                && lower.contains("forces tool use"))
    }
}

/// 404「单模型不可用」的统一处置（协议无关）
///
/// 四个适配器过去各写 30 行：标记账号对**该模型**临时熔断，再决定是否换号。
/// 这里只保留决策与副作用，协议专属的响应体仍由适配器构造。
///
/// `mark` 由调用方注入（`token_manager.mark_model_unsupported`），避免 Pipeline
/// 直接依赖账号层实现。
///
/// 返回 `true` 表示账号池仍有其他候选，调用方应换号重试。
pub fn apply_model_not_found(attempt: usize, pool_size: usize, mut mark: impl FnMut()) -> bool {
    mark();
    attempt < pool_size
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_abandons_sticky_account_policies() {
        // 404 ModelNotFound 必须解除粘性绑定，确保会话不会死粘在缺失该模型的账号上
        let not_found = UpstreamClassification::classify(404, "model not found", None);
        assert_eq!(not_found, UpstreamClassification::ModelNotFound);
        assert!(
            not_found.abandons_sticky_account(),
            "ModelNotFound must abandon sticky account so session rotates away"
        );

        // 429 RateLimited 必须解除粘性绑定
        let rate_limited = UpstreamClassification::classify(429, "rate limited", None);
        assert!(rate_limited.abandons_sticky_account());

        // 401 & 403 必须解除粘性绑定
        let unauth = UpstreamClassification::classify(401, "unauthorized", None);
        assert!(unauth.abandons_sticky_account());
        let forbidden = UpstreamClassification::classify(403, "forbidden", None);
        assert!(forbidden.abandons_sticky_account());

        let client_429 = UpstreamClassification::OtherClientError(429);
        assert!(client_429.abandons_sticky_account());
        let client_404 = UpstreamClassification::OtherClientError(404);
        assert!(client_404.abandons_sticky_account());
    }

    #[test]
    fn test_is_server_overloaded() {
        assert!(UpstreamClassification::is_server_overloaded(503, ""));
        assert!(UpstreamClassification::is_server_overloaded(529, ""));
        assert!(UpstreamClassification::is_server_overloaded(
            500,
            r#"{"error":{"message":"The model is overloaded."}}"#
        ));
        assert!(UpstreamClassification::is_server_overloaded(
            504,
            "Gateway Timeout: upstream overloaded"
        ));
        assert!(!UpstreamClassification::is_server_overloaded(
            429,
            "quota exhausted"
        ));
        assert!(!UpstreamClassification::is_server_overloaded(200, "OK"));
        assert!(!UpstreamClassification::is_server_overloaded(
            404,
            "Not Found"
        ));
    }

    #[test]
    fn test_is_thinking_tool_choice_conflict() {
        let err_body = r#"{"type":"error","error":{"type":"invalid_request_error","message":"Thinking may not be enabled when tool_choice forces tool use."}}"#;
        assert!(UpstreamClassification::is_thinking_tool_choice_conflict(
            400, err_body
        ));
        assert!(!UpstreamClassification::is_thinking_tool_choice_conflict(
            200, err_body
        ));
        assert!(!UpstreamClassification::is_thinking_tool_choice_conflict(
            400,
            "Normal 400 bad request"
        ));
    }

    #[test]
    fn test_image_processing_error_classification() {
        let err_body = r#"API Error: 400 Unable to process input image. Please retry or report in https://developers.generativeai.google/guide/troubleshooting"#;
        let classification = UpstreamClassification::classify(400, err_body, None);
        assert_eq!(classification, UpstreamClassification::ImageProcessingError);
        assert!(classification.is_image_processing_error());
        assert!(!classification.should_lock_account());
        assert!(!classification.abandons_sticky_account());

        assert!(UpstreamClassification::is_image_processing_error_str(
            400, err_body
        ));
        assert!(UpstreamClassification::is_image_processing_error_str(
            400,
            r#"{"error":{"message":"Failed to process input image due to decoding error"}}"#
        ));
        assert!(!UpstreamClassification::is_image_processing_error_str(
            200, err_body
        ));
        assert!(!UpstreamClassification::is_image_processing_error_str(
            400,
            "Invalid signature in thinking block"
        ));
    }

    #[test]
    fn test_gateway_self_answer_never_penalizes_accounts() {
        // 404 model not exist：网关代答，绝不能解绑粘性会话或打入冷却
        let not_found = UpstreamClassification::classify(404, "model not exist", None);
        assert_eq!(not_found, UpstreamClassification::InternalGatewayMessage);
        assert!(!not_found.should_lock_account());
        assert!(!not_found.abandons_sticky_account());

        // 502 no quota accounts：即使文本包含 quota 关键词，也必须豁免
        let no_quota = UpstreamClassification::classify(502, "no quota accounts", None);
        assert_eq!(no_quota, UpstreamClassification::InternalGatewayMessage);
        assert!(!no_quota.should_lock_account());
        assert!(!no_quota.abandons_sticky_account());
    }

    #[test]
    fn test_request_level_429_first_attempt_does_not_lock_account() {
        let body = r#"{"error":{"code":429,"message":"Resource has been exhausted (e.g. check quota).","status":"RESOURCE_EXHAUSTED"}}"#;
        assert!(UpstreamClassification::is_request_level_429_without_retry_delay(429, body, None));
        let classification = UpstreamClassification::classify(429, body, None);
        assert!(!classification.should_lock_account_on_attempt(429, body, None, 0));
        assert!(classification.should_lock_account_on_attempt(429, body, None, 1));

        let hard_quota = "You have exceeded your current quota.";
        assert!(
            !UpstreamClassification::is_request_level_429_without_retry_delay(
                429, hard_quota, None
            )
        );
        let hard = UpstreamClassification::classify(429, hard_quota, None);
        assert!(hard.should_lock_account_on_attempt(429, hard_quota, None, 0));

        assert!(
            !UpstreamClassification::is_request_level_429_without_retry_delay(429, body, Some("5"))
        );
    }

    #[test]
    fn test_structured_429_details_take_priority_over_message_keywords() {
        // QuotaFailure details → 硬配额，即使 message 是通用 429 文案
        let quota = r#"{"error":{"code":429,"message":"Resource has been exhausted (e.g. check quota).","status":"RESOURCE_EXHAUSTED","details":[{"@type":"type.googleapis.com/google.rpc.QuotaFailure","violations":[{"quotaMetric":"generate_content_free_tier_requests"}]}]}}"#;
        assert!(
            !UpstreamClassification::is_request_level_429_without_retry_delay(429, quota, None)
        );

        // ErrorInfo rate-limit reason → 请求级，即使 message 里带 quota 字样
        let rate = r#"{"error":{"code":429,"message":"Quota exceeded for requests per minute.","status":"RESOURCE_EXHAUSTED","details":[{"@type":"type.googleapis.com/google.rpc.ErrorInfo","reason":"RATE_LIMIT_EXCEEDED","domain":"googleapis.com"}]}}"#;
        assert!(UpstreamClassification::is_request_level_429_without_retry_delay(429, rate, None));

        // RetryInfo 带明确延迟 → 不属于“无延迟”形态
        let retry = r#"{"error":{"code":429,"message":"Resource has been exhausted","status":"RESOURCE_EXHAUSTED","details":[{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"2s"}]}}"#;
        assert!(
            !UpstreamClassification::is_request_level_429_without_retry_delay(429, retry, None)
        );

        // 无 details 时仍走关键词兜底
        let bare = r#"{"error":{"code":429,"message":"Resource has been exhausted (e.g. check quota).","status":"RESOURCE_EXHAUSTED"}}"#;
        assert!(UpstreamClassification::is_request_level_429_without_retry_delay(429, bare, None));
        let hard = "You have exceeded your current quota.";
        assert!(!UpstreamClassification::is_request_level_429_without_retry_delay(429, hard, None));
    }
}
