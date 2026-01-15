// Claude 辅助函数
// JSON Schema 清理、签名处理等

// 已移除未使用的 Value 导入

/// 将 JSON Schema 中的类型名称转为大写 (Gemini 要求)
/// 例如: "string" -> "STRING", "integer" -> "INTEGER"
// 已移除未使用的 uppercase_schema_types 函数

/// 根据模型名称获取上下文 Token 限制
pub fn get_context_limit_for_model(model: &str) -> u32 {
    if model.contains("pro") {
        2_097_152 // 2M for Pro
    } else if model.contains("flash") {
        1_048_576 // 1M for Flash
    } else {
        1_048_576 // Default 1M
    }
}

pub fn to_claude_usage(usage_metadata: &super::models::UsageMetadata, scaling_enabled: bool, context_limit: u32) -> super::models::Usage {
    let prompt_tokens = usage_metadata.prompt_token_count.unwrap_or(0);
    let cached_tokens = usage_metadata.cached_content_token_count.unwrap_or(0);
    
    // 【智能阈值回归算法】- 既利用大窗口，又在临界点引导压缩
    let total_raw = prompt_tokens;
    
    let scaled_total = if scaling_enabled && total_raw > 0 {
        const SCALING_THRESHOLD: u32 = 30_000;
        const TARGET_MAX: f64 = 195_000.0; // 接近 Claude 的 200k 限制

        if total_raw <= SCALING_THRESHOLD {
            total_raw
        } else {
            // 设置回归触发点：当真实用量达到限制的 70% 时开始回归
            let perception_start = (context_limit as f64 * 0.7) as u32;
            
            if total_raw <= perception_start {
                // 第一阶段：安全区 - 维持原有的 sqrt 激进压缩
                let excess = (total_raw - SCALING_THRESHOLD) as f64;
                // 系数 25.0 使 100k -> ~50k (保持与原逻辑一致的舒适度)
                let compressed_excess = excess.sqrt() * 25.0; 
                (SCALING_THRESHOLD as f64 + compressed_excess) as u32
            } else {
                // 第二阶段：回归区 - 从 70% 到 100% 线性回归到 195k
                // 计算当前处于 70% - 100% 的比例
                let range = (context_limit as f64 * 0.3) as f64;
                let progress = (total_raw - perception_start) as f64 / range;
                
                // 计算第一阶段末端的数值作为起点
                let base_excess = (perception_start - SCALING_THRESHOLD) as f64;
                let start_value = SCALING_THRESHOLD as f64 + base_excess.sqrt() * 25.0;
                
                // 线性插值回归
                let regression = (TARGET_MAX - start_value) * progress;
                (start_value + regression) as u32
            }
        }
    } else {
        total_raw
    };
    
    // 【调试日志】方便手动验证
    if scaling_enabled && total_raw > 30_000 {
        tracing::debug!(
            "[Claude-Scaling] Raw Tokens: {}, Scaled Report: {}, Ratio: {:.2}%",
            total_raw, scaled_total, (scaled_total as f64 / total_raw as f64) * 100.0
        );
    }
    
    // 按比例分配缩放后的总量到 input 和 cache_read
    let (reported_input, reported_cache) = if total_raw > 0 {
        let cache_ratio = (cached_tokens as f64) / (total_raw as f64);
        let sc_cache = (scaled_total as f64 * cache_ratio) as u32;
        (scaled_total.saturating_sub(sc_cache), Some(sc_cache))
    } else {
        (scaled_total, None)
    };
    
    super::models::Usage {
        input_tokens: reported_input,
        output_tokens: usage_metadata.candidates_token_count.unwrap_or(0),
        cache_read_input_tokens: reported_cache,
        cache_creation_input_tokens: Some(0),
        server_tool_use: None,
    }
}

/// 提取 thoughtSignature
// 已移除未使用的 extract_thought_signature 函数

#[cfg(test)]
mod tests {
    use super::*;
    // 移除了未使用的 serde_json::json

    // 已移除对 uppercase_schema_types 的过期测试

    #[test]
    fn test_to_claude_usage() {
        use super::super::models::UsageMetadata;

        let usage = UsageMetadata {
            prompt_token_count: Some(100),
            candidates_token_count: Some(50),
            total_token_count: Some(150),
            cached_content_token_count: None,
        };

        let claude_usage = to_claude_usage(&usage, true, 1_000_000);
        assert_eq!(claude_usage.input_tokens, 100);
        assert_eq!(claude_usage.output_tokens, 50);

        // 测试 70% 负载 ( percepción_start = 700k )
        let usage_70 = UsageMetadata {
            prompt_token_count: Some(700_000),
            candidates_token_count: Some(10),
            total_token_count: Some(700_010),
            cached_content_token_count: None,
        };
        let res_70 = to_claude_usage(&usage_70, true, 1_000_000);
        // sqrt(670k) * 25 + 30k = 818.5 * 25 + 30k = 20462 + 30k = 50462
        assert!(res_70.input_tokens > 50000 && res_70.input_tokens < 51000);

        // 测试 100% 负载 ( 1M )
        let usage_100 = UsageMetadata {
            prompt_token_count: Some(1_000_000),
            candidates_token_count: Some(10),
            total_token_count: Some(1_000_010),
            cached_content_token_count: None,
        };
        let res_100 = to_claude_usage(&usage_100, true, 1_000_000);
        // 应该非常接近 195,000
        assert_eq!(res_100.input_tokens, 195_000);
        
        // 测试 90% 负载 ( 900k )
        let usage_90 = UsageMetadata {
            prompt_token_count: Some(900_000),
            candidates_token_count: Some(10),
            total_token_count: Some(900_010),
            cached_content_token_count: None,
        };
        let res_90 = to_claude_usage(&usage_90, true, 1_000_000);
        // Regression range: 700k -> 1M (300k range)
        // 900k is 2/3 of the way.
        // Start: ~50462, End: 195000. Diff: ~144538.
        // Value: 50462 + 2/3 * 144538 = 50462 + 96358 = 146820
        assert!(res_90.input_tokens > 146000 && res_90.input_tokens < 147500);
    }
}
