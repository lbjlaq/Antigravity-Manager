//! 协议无关的 QoS 计量。

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
}
