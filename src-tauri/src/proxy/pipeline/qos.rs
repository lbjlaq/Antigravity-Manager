//! 协议无关的 QoS 计量。

use bytes::{Bytes, BytesMut};
use futures::{Stream, StreamExt};
use std::sync::{Arc, Mutex};

use crate::proxy::pipeline::usage::CanonicalUsage;
use crate::proxy::token_manager::TokenManager;

/// 判断转换后的 Gemini 上游请求载荷（或模型特征）是否启用了深度思考模式。
///
/// 遵循 Pipeline-First 架构原则：
/// 1. 优先以规范化后的 Gemini 请求载荷为准（检查 `generationConfig.thinkingConfig`）；
/// 2. 检查显式关闭标记（如 `thinkingBudget == 0` 或 `thinkingLevel == "off"` / `"disabled"`）；
/// 3. 若载荷未明确指明，结合模型提示词进行兜底启发式识别（涵盖 argon, o1/o3/o4, -high, -thinking 等）。
pub fn payload_enables_thinking(gemini_body: &serde_json::Value, model_hint: Option<&str>) -> bool {
    let root = gemini_body.get("request").unwrap_or(gemini_body);
    if let Some(tc) = root
        .get("generationConfig")
        .and_then(|gc| gc.get("thinkingConfig"))
    {
        // 显式检查 thinkingBudget: 为 0 时表示明确关闭思考
        if let Some(budget) = tc.get("thinkingBudget").and_then(|b| b.as_i64()) {
            if budget == 0 {
                return false;
            }
            if budget > 0 || budget == -1 {
                return true;
            }
        }
        // 显式检查 thinkingLevel: off / disabled / none 或 0 表示明确关闭；数字 > 0 或非关闭字符串表示开启
        if let Some(level_val) = tc.get("thinkingLevel") {
            if let Some(level) = level_val.as_str() {
                let lower_level = level.trim().to_lowercase();
                if lower_level == "off" || lower_level == "disabled" || lower_level == "none" {
                    return false;
                }
                return true;
            }
            if let Some(n) = level_val.as_i64() {
                return n > 0;
            }
        }
        // includeThoughts 为 true 则开启，为 false 则关闭
        if let Some(include_thoughts) = tc.get("includeThoughts").and_then(|t| t.as_bool()) {
            return include_thoughts;
        }
        return true;
    }

    if let Some(model) = model_hint {
        let lower = model.trim().to_lowercase();
        if lower.contains("thinking")
            || lower.contains("argon")
            || lower.contains("o1")
            || lower.contains("o3")
            || lower.contains("o4")
            || lower.ends_with("-high")
            || lower.contains("-high-")
            || crate::proxy::model_specs::is_explicit_heuristic_tier_model(&lower)
            || crate::proxy::model_specs::is_gemini_v4_or_above(&lower)
        {
            return true;
        }
    }

    false
}

/// 上报并评估首包 TTFT 耗时到 QoS 守卫。
///
/// 传入的 `input_tokens` 只是 handler 侧的估值；若 Pipeline 已在**转出报文**上
/// 估过 token（四条协议口径统一），优先采用后者，避免各协议分桶标准不一致。
pub fn report_qos_ttft(
    token_manager: &TokenManager,
    account_id: &str,
    session_id: Option<&str>,
    ttft_ms: f64,
    is_deep_thinking: bool,
    input_tokens: Option<u32>,
) {
    let tier = token_manager
        .get_account_tier(account_id)
        .unwrap_or_else(|| "PRO".to_string());
    let unified_tokens = crate::proxy::pipeline::budget::estimated_tokens().or(input_tokens);
    token_manager.record_qos_ttft(
        account_id,
        session_id,
        &tier,
        ttft_ms,
        is_deep_thinking,
        unified_tokens,
    );
}

#[derive(Debug, Default)]
struct QosAttemptState {
    ttft_ms: Option<f64>,
    phase1_evaluated: bool,
    stream_ended: bool,
    finalized: bool,
    latest_usage: Option<CanonicalUsage>,
}

/// 协议无关的两阶段 QoS 尝试跟踪器（Pipeline First）：
/// - Phase 1 (`on_first_byte`): 仅在「即便按 0 缓存全量冷启动也严重超标」时立即落锤；
///   其余情况暂挂，避免把上游隐式缓存冷丢失误判为账号限流。
/// - Phase 2 (`finalize` on stream EOF/Drop): 使用上游 Gemini SSE `usageMetadata`
///   的真实 `uncached_input_tokens = total - cached` 完成最终裁决并准确入桶。
pub struct QosAttemptTracker {
    token_manager: Arc<TokenManager>,
    account_id: String,
    session_id: Option<String>,
    tier: String,
    is_deep_thinking: bool,
    estimated_input_tokens: Option<u32>,
    eval_lock: Mutex<()>,
    state: Mutex<QosAttemptState>,
}

impl QosAttemptTracker {
    pub fn new(
        token_manager: Arc<TokenManager>,
        account_id: String,
        session_id: Option<String>,
        is_deep_thinking: bool,
        fallback_input_tokens: Option<u32>,
    ) -> Self {
        let tier = token_manager
            .get_account_tier(&account_id)
            .unwrap_or_else(|| "PRO".to_string());
        let estimated_input_tokens =
            crate::proxy::pipeline::budget::estimated_tokens().or(fallback_input_tokens);
        Self {
            token_manager,
            account_id,
            session_id,
            tier,
            is_deep_thinking,
            estimated_input_tokens,
            eval_lock: Mutex::new(()),
            state: Mutex::new(QosAttemptState::default()),
        }
    }

    pub fn on_first_byte(&self, ttft_ms: f64) {
        let _eval_guard = self.eval_lock.lock().unwrap_or_else(|e| e.into_inner());
        let already_ended = {
            let mut guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if guard.ttft_ms.is_some() {
                return;
            }
            guard.ttft_ms = Some(ttft_ms);
            guard.stream_ended
        };

        if already_ended {
            self.finalize_locked();
            return;
        }

        let evaluated = self.token_manager.record_qos_phase1_ttft(
            &self.account_id,
            self.session_id.as_deref(),
            &self.tier,
            ttft_ms,
            self.is_deep_thinking,
            self.estimated_input_tokens,
        );

        let need_finalize = {
            let mut guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if evaluated {
                guard.phase1_evaluated = true;
            }
            guard.stream_ended && !guard.finalized
        };

        if need_finalize {
            self.finalize_locked();
        }
    }

    pub fn on_usage(&self, usage: CanonicalUsage) {
        let mut guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
        match &mut guard.latest_usage {
            Some(prev) => {
                let total = if usage.total_input_tokens > 0 {
                    usage.total_input_tokens
                } else {
                    prev.total_input_tokens
                };
                let cached = if usage.cached_tokens > 0 {
                    usage.cached_tokens
                } else {
                    prev.cached_tokens
                };
                let out = if usage.output_tokens > 0 {
                    usage.output_tokens
                } else {
                    prev.output_tokens
                };
                let reasoning = if usage.reasoning_tokens > 0 {
                    usage.reasoning_tokens
                } else {
                    prev.reasoning_tokens
                };
                *prev = CanonicalUsage::new(total, cached, out, reasoning);
            }
            None => {
                if usage.total_input_tokens > 0 {
                    guard.latest_usage = Some(usage);
                }
            }
        }
    }

    pub fn finalize(&self) {
        let _eval_guard = self.eval_lock.lock().unwrap_or_else(|e| e.into_inner());
        self.finalize_locked();
    }

    fn finalize_locked(&self) {
        let (ttft_ms, phase1_evaluated, latest_usage) = {
            let mut guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
            guard.stream_ended = true;
            if guard.finalized {
                return;
            }
            let Some(ttft_ms) = guard.ttft_ms else {
                return;
            };
            guard.finalized = true;
            (ttft_ms, guard.phase1_evaluated, guard.latest_usage.clone())
        };

        if phase1_evaluated {
            if let Some(usage) = latest_usage {
                self.token_manager.update_qos_watermark(
                    &self.account_id,
                    self.session_id.as_deref(),
                    usage.total_input_tokens,
                );
            }
            return;
        }

        if let Some(usage) = latest_usage {
            self.token_manager.record_qos_with_usage(
                &self.account_id,
                self.session_id.as_deref(),
                &self.tier,
                ttft_ms,
                self.is_deep_thinking,
                usage.total_input_tokens,
                usage.uncached_input_tokens,
            );
        } else {
            self.token_manager.record_qos_without_usage(
                &self.account_id,
                self.session_id.as_deref(),
                &self.tier,
                ttft_ms,
                self.is_deep_thinking,
                self.estimated_input_tokens,
            );
        }
    }
}

struct FinalizeQosOnDrop(Arc<QosAttemptTracker>);

impl Drop for FinalizeQosOnDrop {
    fn drop(&mut self) {
        self.0.finalize();
    }
}

const MAX_USAGE_LINE_BUF_BYTES: usize = 8 * 1024 * 1024;

/// 从上游原生 Gemini SSE 单行中抽取 `CanonicalUsage`（快速旁路跳过非 usage 行）
pub fn extract_gemini_sse_usage(line: &str) -> Option<CanonicalUsage> {
    let trimmed = line.trim();
    let json_part = trimmed.strip_prefix("data:")?.trim();
    if json_part.is_empty() || json_part == "[DONE]" || !json_part.contains("usageMetadata") {
        return None;
    }
    let val: serde_json::Value = serde_json::from_str(json_part).ok()?;
    let inner = val.get("response").unwrap_or(&val);
    if inner.get("usageMetadata").is_none() {
        return None;
    }
    let usage = CanonicalUsage::from_gemini(inner);
    if usage.total_input_tokens > 0 {
        Some(usage)
    } else {
        None
    }
}

/// 在上游 Gemini SSE 流出口处旁路嗅探 `usageMetadata`，并在流结束或提前 Drop 时自动核销 QoS 评估。
/// 原始 `Bytes` 分片原封不动透传，零改动、零协议耦合。
pub fn wrap_gemini_stream_with_qos_usage<S, E>(
    stream: S,
    tracker: Arc<QosAttemptTracker>,
) -> std::pin::Pin<Box<dyn Stream<Item = Result<Bytes, E>> + Send>>
where
    S: Stream<Item = Result<Bytes, E>> + Send + 'static,
    E: Send + 'static,
{
    Box::pin(async_stream::stream! {
        let _guard = FinalizeQosOnDrop(tracker.clone());
        let mut line_buf = BytesMut::new();
        let mut stream = Box::pin(stream);

        while let Some(item) = stream.next().await {
            if let Ok(bytes) = &item {
                if line_buf.len() + bytes.len() <= MAX_USAGE_LINE_BUF_BYTES {
                    line_buf.extend_from_slice(bytes);
                    while let Some(pos) = line_buf.iter().position(|&b| b == b'\n') {
                        let line_raw = line_buf.split_to(pos + 1);
                        if let Ok(line_str) = std::str::from_utf8(&line_raw) {
                            if let Some(usage) = extract_gemini_sse_usage(line_str) {
                                tracker.on_usage(usage);
                            }
                        }
                    }
                } else {
                    // 超大单行（如内联图片 base64）跳过行缓存，防止无界内存增长
                    line_buf.clear();
                }
            }
            yield item;
        }

        if !line_buf.is_empty() {
            if let Ok(line_str) = std::str::from_utf8(&line_buf) {
                if let Some(usage) = extract_gemini_sse_usage(line_str) {
                    tracker.on_usage(usage);
                }
            }
            line_buf.clear();
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_payload_enables_thinking_from_config() {
        let payload_high = json!({
            "generationConfig": {
                "thinkingConfig": {
                    "thinkingLevel": "HIGH",
                    "includeThoughts": true
                }
            }
        });
        assert!(payload_enables_thinking(
            &payload_high,
            Some("gemini-3.8-flash-high")
        ));

        let payload_budget_zero = json!({
            "generationConfig": {
                "thinkingConfig": {
                    "thinkingBudget": 0
                }
            }
        });
        assert!(!payload_enables_thinking(
            &payload_budget_zero,
            Some("gemini-3.8-flash-high")
        ));

        let payload_level_off = json!({
            "generationConfig": {
                "thinkingConfig": {
                    "thinkingLevel": "off"
                }
            }
        });
        assert!(!payload_enables_thinking(
            &payload_level_off,
            Some("gemini-3.8-flash-high")
        ));

        let payload_include_false = json!({
            "generationConfig": {
                "thinkingConfig": {
                    "includeThoughts": false
                }
            }
        });
        assert!(!payload_enables_thinking(
            &payload_include_false,
            Some("gemini-3.8-flash-high")
        ));
    }

    #[test]
    fn test_payload_enables_thinking_from_model_heuristics() {
        let empty_payload = json!({});
        assert!(payload_enables_thinking(
            &empty_payload,
            Some("gemini-4-argon-medium")
        ));
        assert!(payload_enables_thinking(
            &empty_payload,
            Some("gemini-3.8-flash-high")
        ));
        assert!(payload_enables_thinking(&empty_payload, Some("o3-mini")));
        assert!(payload_enables_thinking(
            &empty_payload,
            Some("gemini-2.5-pro-thinking")
        ));
        assert!(!payload_enables_thinking(
            &empty_payload,
            Some("gemini-1.5-flash")
        ));
    }

    #[test]
    fn test_extract_gemini_sse_usage_handles_wrapped_and_unwrapped_frames() {
        let wrapped = r#"data: {"response":{"candidates":[],"usageMetadata":{"promptTokenCount":335017,"candidatesTokenCount":95,"cachedContentTokenCount":0}}}"#;
        let u1 = extract_gemini_sse_usage(wrapped).expect("wrapped usageMetadata");
        assert_eq!(u1.total_input_tokens, 335_017);
        assert_eq!(u1.cached_tokens, 0);
        assert_eq!(u1.uncached_input_tokens, 335_017);

        let unwrapped = r#"data: {"usageMetadata":{"promptTokenCount":335855,"candidatesTokenCount":125,"cachedContentTokenCount":332968}}"#;
        let u2 = extract_gemini_sse_usage(unwrapped).expect("unwrapped usageMetadata");
        assert_eq!(u2.total_input_tokens, 335_855);
        assert_eq!(u2.cached_tokens, 332_968);
        assert_eq!(u2.uncached_input_tokens, 2_887);

        assert!(extract_gemini_sse_usage("data: [DONE]").is_none());
        assert!(extract_gemini_sse_usage(": keepalive").is_none());
    }

    #[tokio::test]
    async fn test_qos_attempt_tracker_reconciles_split_sse_chunks_without_false_strike() {
        let dir = tempfile::tempdir().unwrap();
        let tm = Arc::new(TokenManager::new(dir.path().to_path_buf()));
        tm.qos_guardian.set_auto_jail_enabled(true);

        let acc_a = crate::proxy::token_manager::ProxyToken {
            account_id: "acc_a".to_string(),
            priority: 50,
            model_priorities: std::collections::HashMap::new(),
            access_token: "tok_a".to_string(),
            refresh_token: "ref_a".to_string(),
            expires_in: 3600,
            timestamp: 0,
            email: "a@example.com".to_string(),
            account_path: dir.path().join("a.json"),
            project_id: None,
            subscription_tier: Some("PRO".to_string()),
            remaining_quota: Some(100),
            protected_models: std::collections::HashSet::new(),
            forbidden_models: std::collections::HashSet::new(),
            reset_time: None,
            weekly_reset_time: None,
            validation_blocked: false,
            validation_blocked_until: 0,
            validation_url: None,
            model_quotas: std::collections::HashMap::new(),
            model_limits: std::collections::HashMap::new(),
            available_models: std::collections::HashSet::new(),
            model_metadata: std::collections::HashMap::new(),
            model_reset_times: std::collections::HashMap::new(),
            model_weekly_reset_times: std::collections::HashMap::new(),
            model_quota_windows: std::collections::HashMap::new(),
        };
        let mut acc_b = acc_a.clone();
        acc_b.account_id = "acc_b".to_string();
        acc_b.email = "b@example.com".to_string();
        tm.insert_token_for_test(acc_a);
        tm.insert_token_for_test(acc_b);
        tm.qos_guardian.push_sample("acc_b", 4000.0, false, false);

        // 先记录一轮 334K 正常热缓存请求，拉高会话水位
        let tracker0 = Arc::new(QosAttemptTracker::new(
            tm.clone(),
            "acc_a".to_string(),
            Some("sess-reconcile".to_string()),
            true,
            Some(334_244),
        ));
        tracker0.on_first_byte(6_300.0);
        tracker0.on_usage(CanonicalUsage::new(334_244, 330_775, 53, 0));
        tracker0.finalize();
        assert!(tm.qos_guardian.is_healthy("acc_a"));

        // 下一轮模拟跨 chunk 拆包的 Gemini SSE 流：TTFT 58.39s，且尾包 cachedContentTokenCount=0
        let tracker1 = Arc::new(QosAttemptTracker::new(
            tm.clone(),
            "acc_a".to_string(),
            Some("sess-reconcile".to_string()),
            true,
            Some(335_017),
        ));
        let chunks: Vec<Result<Bytes, std::io::Error>> = vec![
            Ok(Bytes::from(
                "data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"hi\"}]}}]}}\n\n",
            )),
            Ok(Bytes::from(
                "data: {\"response\":{\"usageMetadata\":{\"promptTokenCount\":335017,",
            )),
            Ok(Bytes::from(
                "\"candidatesTokenCount\":95,\"cachedContentTokenCount\":0}}}\n\n",
            )),
        ];
        let mut wrapped = Box::pin(wrap_gemini_stream_with_qos_usage(
            futures::stream::iter(chunks),
            tracker1.clone(),
        ));
        // 首包到达触发 Phase 1（处于灰色区间，必须暂挂）
        let _first = wrapped.next().await;
        tracker1.on_first_byte(58_390.0);
        assert!(tm.qos_guardian.is_healthy("acc_a"));

        // 消费完剩余流并结束，自动触发 Phase 2 核销（按 335K 未缓存豁免）
        while let Some(_item) = wrapped.next().await {}
        drop(wrapped);
        assert!(tm.qos_guardian.is_healthy("acc_a"));

        // 验证反向时序（单块/缓冲映射导致流先 EOF 再触发 on_first_byte）且真实命中缓存时仍能准确抓出限流
        let tracker2 = Arc::new(QosAttemptTracker::new(
            tm.clone(),
            "acc_a".to_string(),
            Some("sess-reconcile".to_string()),
            true,
            Some(336_000),
        ));
        tracker2.on_usage(CanonicalUsage::new(336_000, 334_983, 80, 0));
        tracker2.finalize();
        tracker2.on_first_byte(58_390.0);
        assert!(tm.qos_guardian.is_suspicious("acc_a"));
    }
}
