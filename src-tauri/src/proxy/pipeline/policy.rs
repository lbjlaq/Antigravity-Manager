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
    /// 其他普通客户端错误
    OtherClientError(u16),
}

impl UpstreamClassification {
    /// 统一分类器：协议无关，全局唯一真理
    pub fn classify(status: u16, body: &str, retry_after_header: Option<&str>) -> Self {
        let lower = body.to_lowercase();
        // 1. 优先排除网关自身内部错误（防自噬死循环）
        if lower.contains("all accounts limited")
            || lower.contains("no accounts available")
            || lower.contains("all accounts failed")
            || lower.contains("token pool is empty")
            || lower.contains("all accounts exhausted")
            || lower.contains("all accounts unhealthy")
        {
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
        let has_signature_error_cue = lower.contains("invalid thought signature")
            || lower.contains("invalid `signature`")
            || lower.contains("invalid signature")
            || lower.contains("thought_signature")
            || lower.contains("thoughtsignature")
            || lower.contains("thinking.signature")
            || lower.contains("thinking.thinking")
            || lower.contains("corrupted thought signature");

        if status == 400 && has_signature_error_cue {
            return UpstreamClassification::ThoughtSignatureError;
        }

        // 4. 判定真正的限流 (仅限 429 和 529)
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

    /// 该分类是否应该触发 TokenManager 的账号冷却锁定
    pub fn should_lock_account(&self) -> bool {
        matches!(self, UpstreamClassification::RateLimited { .. })
    }

    /// 这个账号不能再被当前会话粘住。401 / 403 / 429 / 529。
    pub fn abandons_sticky_account(&self) -> bool {
        match self {
            UpstreamClassification::RateLimited { .. } => true,
            UpstreamClassification::OtherClientError(401 | 403) => true,
            _ => false,
        }
    }

    /// 该分类是否为模型不存在
    pub fn is_model_not_found(&self) -> bool {
        matches!(self, UpstreamClassification::ModelNotFound)
    }

    /// 该分类是否为思考签名失效/异构签名污染
    pub fn is_thought_signature_error(&self) -> bool {
        matches!(self, UpstreamClassification::ThoughtSignatureError)
    }

    /// 该分类是否为网关自身内部消息
    pub fn is_internal_gateway_message(&self) -> bool {
        matches!(self, UpstreamClassification::InternalGatewayMessage)
    }
}
