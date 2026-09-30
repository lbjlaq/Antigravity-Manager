use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// 统一用量与缓存计算引擎（CanonicalUsage）
/// 权威收拢 Google Gemini 原生 usageMetadata，并提供向四大协议专属格式的标准散开方法
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalUsage {
    /// 全量总上下文输入 (Total Prompt Context = uncached + cached)
    pub total_input_tokens: u32,
    /// 真正未命中的增量输入 (Uncached Input = total_input_tokens - cached_tokens)
    pub uncached_input_tokens: u32,
    /// 命中的缓存 Token (Cached Tokens)
    pub cached_tokens: u32,
    /// 模型输出总量 (Output Tokens, 含思维链)
    pub output_tokens: u32,
    /// 思考思维链 Token 消耗 (Reasoning Tokens)
    pub reasoning_tokens: u32,
    /// 总消耗 Token = total_input_tokens + output_tokens
    pub total_tokens: u32,
}

impl CanonicalUsage {
    /// 直接由离散字段构造标准用量对象
    pub fn new(
        total_input_tokens: u32,
        cached_tokens: u32,
        output_tokens: u32,
        reasoning_tokens: u32,
    ) -> Self {
        let uncached_input_tokens = total_input_tokens.saturating_sub(cached_tokens);
        let total_tokens = total_input_tokens.saturating_add(output_tokens);
        Self {
            total_input_tokens,
            cached_tokens,
            uncached_input_tokens,
            output_tokens,
            reasoning_tokens,
            total_tokens,
        }
    }

    /// 从 Google upstream 原生 usageMetadata 或包含 usageMetadata 的任意 JSON 中权威抽取收拢
    pub fn from_gemini(raw: &Value) -> Self {
        let usage = raw.get("usageMetadata").unwrap_or(raw);

        let total_input = usage
            .get("promptTokenCount")
            .or_else(|| usage.get("prompt_tokens"))
            .or_else(|| usage.get("input_tokens"))
            .or_else(|| usage.get("total_input_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;

        let cached = usage
            .get("cache_read_input_tokens")
            .or_else(|| usage.get("cachedContentTokenCount"))
            .or_else(|| usage.get("cached_content_token_count"))
            .or_else(|| usage.get("total_cached_tokens"))
            .or_else(|| usage.get("cached_tokens"))
            .or_else(|| {
                usage
                    .get("prompt_tokens_details")
                    .and_then(|d| d.get("cached_tokens"))
            })
            .or_else(|| {
                usage
                    .get("input_tokens_details")
                    .and_then(|d| d.get("cached_tokens"))
            })
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;

        // 提取思考思维链 token (支持 candidatesTokensDetails / total_thought_tokens / completion_tokens_details)
        let mut reasoning = 0u32;
        if let Some(details) = usage
            .get("candidatesTokensDetails")
            .and_then(|d| d.as_array())
        {
            for item in details {
                let modality = item.get("modality").and_then(|m| m.as_str()).unwrap_or("");
                if modality.eq_ignore_ascii_case("THINKING")
                    || modality.eq_ignore_ascii_case("REASONING")
                {
                    reasoning = reasoning.saturating_add(
                        item.get("tokenCount").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                    );
                }
            }
        }
        if reasoning == 0 {
            if let Some(r) = usage
                .get("total_thought_tokens")
                .or_else(|| usage.get("totalThoughtTokens"))
                .or_else(|| usage.get("thoughtsTokenCount"))
                .or_else(|| {
                    usage
                        .get("completion_tokens_details")
                        .and_then(|d| d.get("reasoning_tokens"))
                })
                .or_else(|| {
                    usage
                        .get("output_tokens_details")
                        .and_then(|d| d.get("reasoning_tokens"))
                })
                .and_then(|v| v.as_u64())
            {
                reasoning = r as u32;
            }
        }

        let raw_output = usage
            .get("total_output_tokens")
            .or_else(|| usage.get("candidatesTokenCount"))
            .or_else(|| usage.get("candidates_token_count"))
            .or_else(|| usage.get("completion_tokens"))
            .or_else(|| usage.get("output_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;

        let has_new_format = usage.get("total_output_tokens").is_some();
        let tool_use = usage
            .get("total_tool_use_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;

        let output = if has_new_format {
            raw_output
                .saturating_add(reasoning)
                .saturating_add(tool_use)
        } else {
            raw_output
        };

        // 计算全量与增量
        // 如果输入数据是 Anthropic 格式（raw_input 存的是 uncached），做加和还原全量
        let (canonical_total_input, uncached_input) = if let Some(cr) = usage
            .get("cache_read_input_tokens")
            .and_then(|v| v.as_u64())
        {
            let cc = usage
                .get("cache_creation_input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;
            let total = total_input.saturating_add(cr as u32).saturating_add(cc);
            (total, total_input)
        } else if cached > 0 && total_input < cached {
            (total_input.saturating_add(cached), total_input)
        } else {
            (total_input, total_input.saturating_sub(cached))
        };

        let total = usage
            .get("total_tokens")
            .or_else(|| usage.get("totalTokenCount"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
            .unwrap_or_else(|| canonical_total_input.saturating_add(output));

        Self {
            total_input_tokens: canonical_total_input,
            uncached_input_tokens: uncached_input,
            cached_tokens: cached,
            output_tokens: output,
            reasoning_tokens: reasoning,
            total_tokens: total,
        }
    }

    /// 缩放算法（对齐 Claude Code 上下文阈值）：超过 30k 且开启 scaling 时按阶梯缩放
    pub fn scale_claude_tokens(total_raw: u32, context_limit: u32) -> u32 {
        if total_raw <= 30_000 {
            return total_raw;
        }
        const TARGET_MAX: f64 = 195_000.0;
        let ratio = total_raw as f64 / context_limit.max(1) as f64;
        if ratio <= 0.5 {
            let display_ratio = ratio * 0.6;
            (display_ratio * TARGET_MAX) as u32
        } else if ratio <= 0.7 {
            let progress = (ratio - 0.5) / 0.2;
            let display_ratio = 0.3 + progress * 0.2;
            (display_ratio * TARGET_MAX) as u32
        } else if ratio <= 0.85 {
            let progress = (ratio - 0.7) / 0.15;
            let display_ratio = 0.5 + progress * 0.2;
            (display_ratio * TARGET_MAX) as u32
        } else {
            let progress = (ratio - 0.85) / 0.15;
            let display_ratio = 0.7 + progress * 0.27;
            (display_ratio.min(0.97) * TARGET_MAX) as u32
        }
    }

    /// 散开为 Anthropic Claude 格式：
    /// Claude 规范要求 input_tokens 仅表达未缓存增量，缓存单列在 cache_read_input_tokens
    pub fn to_claude_usage(&self, scaling_enabled: bool, context_limit: u32) -> Value {
        let (reported_input, reported_cache) = if scaling_enabled
            && self.total_input_tokens > 30_000
        {
            let scaled_total = Self::scale_claude_tokens(self.total_input_tokens, context_limit);
            if self.total_input_tokens > 0 {
                let cache_ratio = (self.cached_tokens as f64) / (self.total_input_tokens as f64);
                let sc_cache = (scaled_total as f64 * cache_ratio) as u32;
                (scaled_total.saturating_sub(sc_cache), Some(sc_cache))
            } else {
                (scaled_total, None)
            }
        } else {
            (
                self.uncached_input_tokens,
                if self.cached_tokens > 0 {
                    Some(self.cached_tokens)
                } else {
                    None
                },
            )
        };

        json!({
            "input_tokens": reported_input,
            "output_tokens": self.output_tokens,
            "cache_read_input_tokens": reported_cache,
            "cache_creation_input_tokens": 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canonical_usage_from_gemini_and_diffusion() {
        let gemini_raw = json!({
            "usageMetadata": {
                "promptTokenCount": 99000,
                "cachedContentTokenCount": 94000,
                "candidatesTokenCount": 4100,
                "candidatesTokensDetails": [
                    { "modality": "TEXT", "tokenCount": 3100 },
                    { "modality": "THINKING", "tokenCount": 1000 }
                ]
            }
        });

        let usage = CanonicalUsage::from_gemini(&gemini_raw);
        assert_eq!(usage.total_input_tokens, 99000);
        assert_eq!(usage.cached_tokens, 94000);
        assert_eq!(usage.uncached_input_tokens, 5000);
        assert_eq!(usage.output_tokens, 4100);
        assert_eq!(usage.reasoning_tokens, 1000);
        assert_eq!(usage.total_tokens, 103100);

        // 验证散开到 Claude
        let claude_val = usage.to_claude_usage(false, 200000);
        assert_eq!(claude_val["input_tokens"], 5000);
        assert_eq!(claude_val["output_tokens"], 4100);
        assert_eq!(claude_val["cache_read_input_tokens"], 94000);
    }

    #[test]
    fn test_canonical_usage_healing_anthropic_input() {
        // 模拟 Anthropic 历史格式：input_tokens 存的是未缓存部分 (5000)，cached 存的是 (94000)
        let raw = json!({
            "input_tokens": 5000,
            "cached_tokens": 94000,
            "output_tokens": 110
        });
        let usage = CanonicalUsage::from_gemini(&raw);
        assert_eq!(usage.total_input_tokens, 99000);
        assert_eq!(usage.cached_tokens, 94000);
        assert_eq!(usage.uncached_input_tokens, 5000);
        assert_eq!(usage.output_tokens, 110);
    }
}
