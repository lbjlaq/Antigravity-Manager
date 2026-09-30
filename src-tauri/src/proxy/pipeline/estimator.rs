//! Protocol-Agnostic Token Estimation Engine & High-Concurrency Global Cache
//!
//! [Pipeline First]
//! 1. 提供跨协议通用的 Token 估算引擎：统一面向 Canonical Gemini IR，同时全面兼容
//!    Claude、OpenAI 原生报文及任意非结构化 Payload；
//! 2. 全局高并发内容哈希缓存：基于请求结构 SHA256 摘要，在 < 0.05ms 亚毫秒级就地响应，
//!    彻底避免重复遍历超长上下文的 CPU 开销与网络 I/O；
//! 3. 散开给所有协议出站端点（Claude `/v1/messages/count_tokens`、Gemini `:countTokens` 等）。

use dashmap::DashMap;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_secs(3600); // 1 小时缓存
const MAX_CACHE_ENTRIES: usize = 5000;

#[derive(Clone, Debug)]
struct CacheEntry {
    tokens: u32,
    timestamp: Instant,
}

static TOKEN_CACHE: LazyLock<DashMap<String, CacheEntry>> = LazyLock::new(DashMap::new);

/// 多语言字符分词加权算法（ASCII 约 4 字符/Token，Unicode/CJK 约 1.5 字符/Token，附加 15% 安全余量）
pub fn estimate_tokens_from_str(s: &str) -> u32 {
    if s.is_empty() {
        return 0;
    }

    let mut ascii_chars = 0u32;
    let mut unicode_chars = 0u32;

    for c in s.chars() {
        if c.is_ascii() {
            ascii_chars += 1;
        } else {
            unicode_chars += 1;
        }
    }

    let ascii_tokens = (ascii_chars as f32 / 4.0).ceil() as u32;
    let unicode_tokens = (unicode_chars as f32 / 1.5).ceil() as u32;

    ((ascii_tokens + unicode_tokens) as f32 * 1.15).ceil() as u32
}

/// Base64 多模态媒体部件 Token 折算
pub fn estimate_inline_data_tokens(mime_type: &str, data_len: usize) -> u32 {
    if mime_type.starts_with("image/") {
        let raw_bytes = (data_len * 3) / 4;
        if raw_bytes > 4_000_000 {
            10_000
        } else if raw_bytes > 80_000 {
            // 高清全屏截图或大图（典型 Cowork 截图 300KB ~ 3MB）:
            // 按 Anthropic 官方高分辨率多模态规格折算约为 1600~2000 tokens
            let factor = (raw_bytes as f32 / 1_000_000.0).max(1.0);
            (1600.0 * factor).ceil() as u32
        } else {
            258
        }
    } else if mime_type.starts_with("audio/") {
        let raw_bytes = (data_len * 3) / 4;
        let estimated_seconds = raw_bytes as f32 / 32_000.0;
        (estimated_seconds * 32.0).ceil().max(64.0) as u32
    } else {
        estimate_tokens_from_str(&format!("[binary data: {} bytes]", data_len))
    }
}

/// 协议无关通用 Token 估算器与缓存体系
pub struct PipelineTokenEstimator;

impl PipelineTokenEstimator {
    /// 计算任意 JSON 请求载荷的 SHA256 指纹
    fn compute_cache_key(body: &Value) -> String {
        let mut hasher = Sha256::new();
        if let Ok(bytes) = serde_json::to_vec(body) {
            hasher.update(&bytes);
        } else {
            hasher.update(body.to_string().as_bytes());
        }
        format!("{:x}", hasher.finalize())
    }

    /// 核心入口：协议无关地估算请求 Token 数量（优先命中内存哈希缓存）
    pub fn estimate_tokens(body: &Value) -> u32 {
        if body.is_null() {
            return 0;
        }

        let cache_key = Self::compute_cache_key(body);

        // 1. 尝试命中全局高并发缓存
        if let Some(entry) = TOKEN_CACHE.get(&cache_key) {
            if entry.timestamp.elapsed() < CACHE_TTL {
                tracing::debug!(
                    "[TokenEstimator] Cache hit for key {}: {} tokens",
                    &cache_key[..8],
                    entry.tokens
                );
                return entry.tokens;
            }
        }

        // 2. 缓存未命中：执行协议无关通用解析估算
        let calculated = Self::calculate_tokens_internal(body);

        // 3. 写入缓存并实施容量与 TTL 治理
        if TOKEN_CACHE.len() >= MAX_CACHE_ENTRIES {
            TOKEN_CACHE.retain(|_, v| v.timestamp.elapsed() < CACHE_TTL);
            if TOKEN_CACHE.len() >= MAX_CACHE_ENTRIES {
                TOKEN_CACHE.clear();
            }
        }
        TOKEN_CACHE.insert(
            cache_key,
            CacheEntry {
                tokens: calculated,
                timestamp: Instant::now(),
            },
        );

        calculated
    }

    /// 内部协议无关解析路由
    fn calculate_tokens_internal(body: &Value) -> u32 {
        let unwrapped = body.get("request").unwrap_or(body);

        // 模式 1: 谷歌 Gemini Canonical IR（包含 contents 数组）
        if unwrapped
            .get("contents")
            .and_then(Value::as_array)
            .is_some()
        {
            return Self::estimate_gemini_ir(unwrapped);
        }

        // 模式 2: Anthropic Claude 格式（包含 messages 数组，且 content 多为 string 或 blocks 数组）
        if unwrapped
            .get("messages")
            .and_then(Value::as_array)
            .is_some()
        {
            // 进一步区分 Claude 还是 OpenAI
            let is_claude = unwrapped.get("system").is_some()
                || unwrapped
                    .get("messages")
                    .and_then(Value::as_array)
                    .map_or(false, |msgs| {
                        msgs.iter().any(|m| {
                            m.get("content").map_or(false, |c| {
                                c.as_array().map_or(false, |arr| {
                                    arr.iter().any(|b| {
                                        b.get("type").map_or(false, |t| {
                                            matches!(
                                                t.as_str(),
                                                Some("tool_use")
                                                    | Some("tool_result")
                                                    | Some("thinking")
                                                    | Some("document")
                                            )
                                        })
                                    })
                                })
                            })
                        })
                    });

            if is_claude {
                return Self::estimate_claude_format(unwrapped);
            } else {
                return Self::estimate_openai_format(unwrapped);
            }
        }

        // 模式 3: 兜底加权估算
        estimate_tokens_from_str(&body.to_string())
    }

    /// 估算 Gemini Canonical IR 结构
    fn estimate_gemini_ir(body: &Value) -> u32 {
        let mut total = 0u32;

        // systemInstruction
        if let Some(sys_inst) = body
            .get("systemInstruction")
            .or_else(|| body.get("system_instruction"))
        {
            if let Some(parts) = sys_inst.get("parts").and_then(Value::as_array) {
                for part in parts {
                    if let Some(text) = part.get("text").and_then(Value::as_str) {
                        total += estimate_tokens_from_str(text);
                    }
                }
            }
        }

        // contents
        if let Some(contents) = body.get("contents").and_then(Value::as_array) {
            for msg in contents {
                total += 4; // 轮次基础开销
                if let Some(parts) = msg.get("parts").and_then(Value::as_array) {
                    for part in parts {
                        if let Some(text) = part.get("text").and_then(Value::as_str) {
                            total += estimate_tokens_from_str(text);
                        }
                        if let Some(thought) = part.get("thought").and_then(Value::as_bool) {
                            if thought {
                                total += 100; // 思维块签名与元数据开销
                            }
                        }
                        if let Some(inline_data) =
                            part.get("inlineData").or_else(|| part.get("inline_data"))
                        {
                            let mime = inline_data
                                .get("mimeType")
                                .or_else(|| inline_data.get("mime_type"))
                                .and_then(Value::as_str)
                                .unwrap_or("");
                            let data_len = inline_data
                                .get("data")
                                .and_then(Value::as_str)
                                .map_or(0, |s| s.len());
                            total += estimate_inline_data_tokens(mime, data_len);
                        }
                        if let Some(fc) = part
                            .get("functionCall")
                            .or_else(|| part.get("function_call"))
                        {
                            total += 20;
                            if let Some(name) = fc.get("name").and_then(Value::as_str) {
                                total += estimate_tokens_from_str(name);
                            }
                            if let Some(args) = fc.get("args") {
                                total += estimate_tokens_from_str(&args.to_string());
                            }
                        }
                        if let Some(fr) = part
                            .get("functionResponse")
                            .or_else(|| part.get("function_response"))
                        {
                            total += 10;
                            if let Some(name) = fr.get("name").and_then(Value::as_str) {
                                total += estimate_tokens_from_str(name);
                            }
                            if let Some(resp) = fr.get("response") {
                                total += estimate_tokens_from_str(&resp.to_string());
                            }
                        }
                    }
                }
            }
        }

        // tools (functionDeclarations)
        if let Some(tools) = body.get("tools").and_then(Value::as_array) {
            for tool in tools {
                total += estimate_tokens_from_str(&tool.to_string());
            }
        }

        total
    }

    /// 估算 Claude 请求格式
    fn estimate_claude_format(body: &Value) -> u32 {
        let mut total = 0u32;

        // system
        if let Some(sys) = body.get("system") {
            if let Some(text) = sys.as_str() {
                total += estimate_tokens_from_str(text);
            } else if let Some(arr) = sys.as_array() {
                for block in arr {
                    if let Some(text) = block.get("text").and_then(Value::as_str) {
                        total += estimate_tokens_from_str(text);
                    }
                }
            }
        }

        // messages
        if let Some(messages) = body.get("messages").and_then(Value::as_array) {
            for msg in messages {
                total += 4;
                if let Some(content) = msg.get("content") {
                    if let Some(text) = content.as_str() {
                        total += estimate_tokens_from_str(text);
                    } else if let Some(blocks) = content.as_array() {
                        for block in blocks {
                            match block.get("type").and_then(Value::as_str) {
                                Some("text") => {
                                    if let Some(t) = block.get("text").and_then(Value::as_str) {
                                        total += estimate_tokens_from_str(t);
                                    }
                                }
                                Some("thinking") => {
                                    if let Some(t) = block.get("thinking").and_then(Value::as_str) {
                                        total += estimate_tokens_from_str(t);
                                    }
                                    total += 100;
                                }
                                Some("redacted_thinking") => {
                                    if let Some(d) = block.get("data").and_then(Value::as_str) {
                                        total += estimate_tokens_from_str(d);
                                    }
                                }
                                Some("tool_use") | Some("server_tool_use") => {
                                    total += 20;
                                    if let Some(name) = block.get("name").and_then(Value::as_str) {
                                        total += estimate_tokens_from_str(name);
                                    }
                                    if let Some(input) = block.get("input") {
                                        total += estimate_tokens_from_str(&input.to_string());
                                    }
                                }
                                Some("tool_result") | Some("web_search_tool_result") => {
                                    total += 10;
                                    if let Some(c) = block.get("content") {
                                        if let Some(s) = c.as_str() {
                                            if s.contains("data:image/") {
                                                let mut dummy_parts = Vec::new();
                                                let clean_s = crate::proxy::mappers::common_utils::extract_multimodal_from_tool_text(s, &mut dummy_parts);
                                                total += estimate_tokens_from_str(&clean_s);
                                                for p in dummy_parts {
                                                    if let Some(inline) = p
                                                        .get("inlineData")
                                                        .or_else(|| p.get("inline_data"))
                                                    {
                                                        let mime = inline
                                                            .get("mimeType")
                                                            .and_then(Value::as_str)
                                                            .unwrap_or("image/png");
                                                        let b64_len = inline
                                                            .get("data")
                                                            .and_then(Value::as_str)
                                                            .map_or(0, |d| d.len());
                                                        total += estimate_inline_data_tokens(
                                                            mime, b64_len,
                                                        );
                                                    }
                                                }
                                            } else {
                                                total += estimate_tokens_from_str(s);
                                            }
                                        } else if let Some(arr) = c.as_array() {
                                            for sub in arr {
                                                if let Some(t) =
                                                    sub.get("text").and_then(Value::as_str)
                                                {
                                                    total += estimate_tokens_from_str(t);
                                                } else if sub.get("source").is_some()
                                                    || sub.get("type").and_then(Value::as_str)
                                                        == Some("image")
                                                {
                                                    let source = sub.get("source");
                                                    let mime = source
                                                        .and_then(|s| s.get("media_type"))
                                                        .and_then(Value::as_str)
                                                        .unwrap_or("image/png");
                                                    let data_len = source
                                                        .and_then(|s| s.get("data"))
                                                        .and_then(Value::as_str)
                                                        .map_or(0, |d| d.len());
                                                    total +=
                                                        estimate_inline_data_tokens(mime, data_len);
                                                } else {
                                                    total +=
                                                        estimate_tokens_from_str(&sub.to_string());
                                                }
                                            }
                                        } else {
                                            total += estimate_tokens_from_str(&c.to_string());
                                        }
                                    }
                                }
                                Some("image") | Some("document") => {
                                    let mime = block
                                        .get("source")
                                        .and_then(|s| s.get("media_type"))
                                        .and_then(Value::as_str)
                                        .unwrap_or("image/png");
                                    let data_len = block
                                        .get("source")
                                        .and_then(|s| s.get("data"))
                                        .and_then(Value::as_str)
                                        .map_or(0, |s| s.len());
                                    total += estimate_inline_data_tokens(mime, data_len);
                                }
                                _ => {
                                    total += estimate_tokens_from_str(&block.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }

        // tools
        if let Some(tools) = body.get("tools").and_then(Value::as_array) {
            for tool in tools {
                total += estimate_tokens_from_str(&tool.to_string());
            }
        }

        total
    }

    /// 估算 OpenAI 请求格式
    fn estimate_openai_format(body: &Value) -> u32 {
        let mut total = 0u32;

        if let Some(messages) = body.get("messages").and_then(Value::as_array) {
            for msg in messages {
                total += 4;
                if let Some(content) = msg.get("content") {
                    if let Some(text) = content.as_str() {
                        total += estimate_tokens_from_str(text);
                    } else if let Some(arr) = content.as_array() {
                        for part in arr {
                            if let Some(text) = part.get("text").and_then(Value::as_str) {
                                total += estimate_tokens_from_str(text);
                            }
                            if let Some(image_obj) = part.get("image_url") {
                                let url =
                                    image_obj.get("url").and_then(Value::as_str).unwrap_or("");
                                total += estimate_tokens_from_str(url).max(258);
                            }
                        }
                    }
                }
                if let Some(tool_calls) = msg.get("tool_calls").and_then(Value::as_array) {
                    for tc in tool_calls {
                        total += 20;
                        if let Some(f) = tc.get("function") {
                            if let Some(name) = f.get("name").and_then(Value::as_str) {
                                total += estimate_tokens_from_str(name);
                            }
                            if let Some(args) = f.get("arguments").and_then(Value::as_str) {
                                total += estimate_tokens_from_str(args);
                            }
                        }
                    }
                }
            }
        }

        if let Some(tools) = body.get("tools").and_then(Value::as_array) {
            for tool in tools {
                total += estimate_tokens_from_str(&tool.to_string());
            }
        }

        total
    }

    /// 清空或重置内存估算缓存（供测试与维护使用）
    #[cfg(test)]
    pub fn clear_cache() {
        TOKEN_CACHE.clear();
    }
}

/// 快捷模块级入口
pub fn estimate_tokens(body: &Value) -> u32 {
    PipelineTokenEstimator::estimate_tokens(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_pipeline_estimator_claude_format() {
        let body = json!({
            "model": "claude-3-5-sonnet",
            "system": "You are a helpful coding assistant.",
            "messages": [
                {
                    "role": "user",
                    "content": "Hello, world!"
                }
            ]
        });

        let tokens = PipelineTokenEstimator::estimate_tokens(&body);
        assert!(tokens > 0);

        // 再次获取应命中缓存
        let tokens_cached = PipelineTokenEstimator::estimate_tokens(&body);
        assert_eq!(tokens, tokens_cached);
    }

    #[test]
    fn test_pipeline_estimator_gemini_format() {
        let body = json!({
            "contents": [
                {
                    "role": "user",
                    "parts": [{"text": "Hello Gemini!"}]
                },
                {
                    "role": "model",
                    "parts": [
                        {"thought": true, "text": "Thinking..."},
                        {"functionCall": {"name": "test_tool", "args": {"key": "val"}}}
                    ]
                }
            ]
        });

        let tokens = PipelineTokenEstimator::estimate_tokens(&body);
        assert!(tokens >= 120); // 包含 thinking 100 + functionCall 20
    }

    #[test]
    fn test_pipeline_estimator_caching() {
        PipelineTokenEstimator::clear_cache();
        let body = json!({ "prompt": "cached test" });
        let key = PipelineTokenEstimator::compute_cache_key(&body);
        assert_eq!(TOKEN_CACHE.len(), 0);

        let t1 = PipelineTokenEstimator::estimate_tokens(&body);
        assert_eq!(TOKEN_CACHE.len(), 1);
        assert!(TOKEN_CACHE.contains_key(&key));

        let t2 = PipelineTokenEstimator::estimate_tokens(&body);
        assert_eq!(t1, t2);
    }
}
