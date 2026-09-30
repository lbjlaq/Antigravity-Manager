//! 纯思考空回复流式自愈管道（Thinking Dropout Auto-Healing Pipeline）
//!
//! 核心防御场景：
//! 超长上下文（如 40 万+ tokens / 700+ 轮）下，大模型（尤其是 Gemini Flash 系列）在完成思考后，
//! 概率性因注意力坍缩直接输出结束符 `<end_of_turn>` / `STOP`，导致 `candidatesTokenCount == 0`，
//! 即“只返回了思考块，正文与工具调用皆为空”。
//! 客户端（如 JeikCode / Codex / Claude Code 等）收到仅有思考的回复后，会因状态机无法推进而直接异常断开。
//!
//! 自愈拦截策略（Tail Interception & Auto-Piping）：
//! 1. 思考块正常实时透传给下游客户端，确保首字延迟（TTFT）与实时思考动画丝滑展示；
//! 2. 拦截流末尾的过早终止符（`finishReason: "STOP"` 与 `[DONE]`），不向客户端发射；
//! 3. 向客户端发射 SSE 心跳注释（`: auto-healing empty thinking\n\n`）保持连接存活；
//! 4. 自动构造带原上下文的自愈续跑请求（根据语言自适应追加 `"继续"` 或 `"Continue."`），在相同账号上并发起上游调用；
//! 5. 将上游续跑流无缝缝合至当前下游客户端连接；
//! 6. 严格实施单次自愈上限（`max_auto_heals = 1`）与失败兜底熔断，彻底消除死循环风险与客户端断开。

use bytes::{Bytes, BytesMut};
use futures::{Stream, StreamExt};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use crate::proxy::upstream::client::UpstreamClient;

/// 思考空回复自愈上下文
pub struct ThinkingAutoHealContext {
    pub upstream: Arc<UpstreamClient>,
    pub method: &'static str,
    pub access_token: String,
    pub original_body: Value,
    pub query_string: Option<&'static str>,
    pub extra_headers: HashMap<String, String>,
    pub account_id: Option<String>,
    pub trace_id: String,
}

/// 根据历史上下文的用户语言偏好，解析最自然的续跑提示词
pub fn resolve_continuation_prompt(body: &Value) -> &'static str {
    let contents = body
        .get("request")
        .and_then(|r| r.get("contents"))
        .or_else(|| body.get("contents"))
        .and_then(|c| c.as_array());

    if let Some(contents) = contents {
        for content in contents.iter().rev() {
            if content.get("role").and_then(|r| r.as_str()) == Some("user") {
                if let Some(parts) = content.get("parts").and_then(|p| p.as_array()) {
                    for p in parts.iter().rev() {
                        if let Some(text) = p.get("text").and_then(|t| t.as_str()) {
                            if text.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)) {
                                return "继续";
                            } else {
                                return "Continue.";
                            }
                        }
                    }
                }
            }
        }
    }
    "Continue."
}

/// 构造用于自愈续跑的上游请求 Body
pub fn create_auto_heal_continuation_body(
    original_body: &Value,
    continuation_prompt: &str,
) -> Value {
    let mut new_body = original_body.clone();

    // 1. 为 requestId 追加自愈标记，避免上游缓存或去重干扰
    if let Some(req_id) = new_body.get_mut("requestId").and_then(|v| v.as_str()) {
        new_body["requestId"] = json!(format!("{}_heal1", req_id));
    }
    if let Some(req) = new_body.get_mut("request").and_then(|r| r.as_object_mut()) {
        if let Some(req_id) = req.get_mut("requestId").and_then(|v| v.as_str()) {
            req["requestId"] = json!(format!("{}_heal1", req_id));
        }
    }

    // 2. 追加 User 续跑提示轮次
    let user_turn = json!({
        "role": "user",
        "parts": [{ "text": continuation_prompt }]
    });

    if let Some(contents) = new_body
        .get_mut("request")
        .and_then(|r| r.get_mut("contents"))
        .and_then(|c| c.as_array_mut())
    {
        contents.push(user_turn);
    } else if let Some(contents) = new_body.get_mut("contents").and_then(|c| c.as_array_mut()) {
        contents.push(user_turn);
    }

    new_body
}

/// 检查 Gemini candidate 中的部件类型分布
fn inspect_gemini_candidate_parts(
    candidate: &Value,
    saw_thought: &mut bool,
    saw_content: &mut bool,
    saw_tool_call: &mut bool,
) {
    if let Some(parts) = candidate
        .get("content")
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.as_array())
    {
        for part in parts {
            let is_thought = part
                .get("thought")
                .and_then(|t| t.as_bool())
                .unwrap_or(false);
            if is_thought {
                *saw_thought = true;
            }
            if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                if !is_thought && !text.trim().is_empty() {
                    *saw_content = true;
                }
            }
            if part.get("inlineData").is_some() || part.get("inline_data").is_some() {
                *saw_content = true;
            }
            if part.get("functionCall").is_some() {
                *saw_tool_call = true;
            }
        }
    }
}

/// 统一纯思考空回复自愈流式包装器（Pipeline First）
///
/// 对上游原始 Gemini SSE 流进行透明包装：
/// - 思考内容实时透传；
/// - 若正常产出正文或工具调用，全流程无任何额外开销；
/// - 若发现仅产出思考后立即返回终止符，截留终止信号并在同一连接中自动续跑；
/// - 续跑上限严格为 1 次，具有完备的熔断与兜底保障。
pub fn wrap_stream_with_empty_thinking_auto_heal<S, E>(
    stream: Pin<Box<S>>,
    ctx: ThinkingAutoHealContext,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, E>> + Send>>
where
    S: Stream<Item = Result<Bytes, E>> + Send + 'static,
    E: std::fmt::Display + Send + 'static,
{
    let stream = async_stream::stream! {
        let mut buffer = BytesMut::new();
        let mut saw_thought = false;
        let mut saw_content = false;
        let mut saw_tool_call = false;
        let mut saw_finish_reason = false;
        let mut finish_reason_val: Option<String> = None;
        let mut auto_healed = false;

        let mut stream1 = stream;
        while let Some(item) = stream1.next().await {
            match item {
                Ok(bytes) => {
                    buffer.extend_from_slice(&bytes);
                    while let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                        let line_raw = buffer.split_to(pos + 1);
                        let line_str = match std::str::from_utf8(&line_raw) {
                            Ok(s) => s,
                            Err(_) => {
                                yield Ok(line_raw.freeze());
                                continue;
                            }
                        };
                        let line = line_str.trim();
                        if line.is_empty() {
                            continue;
                        }

                        if line.starts_with("data: ") {
                            let json_part = line.trim_start_matches("data: ").trim();
                            if json_part == "[DONE]" {
                                if saw_content || saw_tool_call {
                                    yield Ok(Bytes::from("data: [DONE]\n\n"));
                                }
                                continue;
                            }

                            match serde_json::from_str::<Value>(json_part) {
                                Ok(mut json) => {
                                    let has_resp = json.get("response").is_some();
                                    let inner = if has_resp {
                                        json.get_mut("response").unwrap()
                                    } else {
                                        &mut json
                                    };
                                    let mut chunk_has_finish = false;
                                    let mut chunk_has_parts = false;

                                    if let Some(candidates) = inner.get_mut("candidates").and_then(|c| c.as_array_mut()) {
                                        for cand in candidates.iter_mut() {
                                            inspect_gemini_candidate_parts(
                                                cand,
                                                &mut saw_thought,
                                                &mut saw_content,
                                                &mut saw_tool_call,
                                            );
                                            if let Some(parts) = cand.get("content").and_then(|c| c.get("parts")).and_then(|p| p.as_array()) {
                                                if !parts.is_empty() {
                                                    chunk_has_parts = true;
                                                }
                                            }
                                            if let Some(fr) = cand.get("finishReason").and_then(|f| f.as_str()) {
                                                chunk_has_finish = true;
                                                saw_finish_reason = true;
                                                finish_reason_val = Some(fr.to_string());
                                            }
                                        }
                                    }

                                    if saw_content || saw_tool_call {
                                        // 正常回复流程：已看到正文或工具调用，原样放行
                                        yield Ok(Bytes::from(format!("data: {}\n\n", json_part)));
                                    } else if saw_thought && chunk_has_finish {
                                        // 思考空回复异常候选帧：
                                        // 若当前帧还包含思考部件，清洗剥离 finishReason 与 usageMetadata 后放行思考，不发射终止信号
                                        if chunk_has_parts {
                                            if let Some(candidates) = inner.get_mut("candidates").and_then(|c| c.as_array_mut()) {
                                                for cand in candidates.iter_mut() {
                                                    if let Some(obj) = cand.as_object_mut() {
                                                        obj.remove("finishReason");
                                                    }
                                                }
                                            }
                                            if let Some(obj) = inner.as_object_mut() {
                                                obj.remove("usageMetadata");
                                            }
                                            let sanitized = serde_json::to_string(&json).unwrap_or_default();
                                            yield Ok(Bytes::from(format!("data: {}\n\n", sanitized)));
                                        }
                                        // 若无有效部件仅有 finishReason，彻底拦截暂存，不往下游发送
                                    } else {
                                        // 普通思考块或中间流帧：原样透传
                                        yield Ok(Bytes::from(format!("data: {}\n\n", json_part)));
                                    }
                                }
                                Err(_) => {
                                    yield Ok(Bytes::from(format!("{}\n\n", line)));
                                }
                            }
                        } else {
                            // 保持非 data 行（如保活注释）正常下发
                            yield Ok(Bytes::from(format!("{}\n\n", line)));
                        }
                    }
                }
                Err(e) => {
                    yield Err(e);
                    return;
                }
            }
        }

        // 冲刷残留缓冲区
        if !buffer.is_empty() {
            if let Ok(line_str) = std::str::from_utf8(&buffer) {
                let line = line_str.trim();
                if !line.is_empty() {
                    yield Ok(Bytes::from(format!("{}\n\n", line)));
                }
            }
        }

        // 核心自愈判断条件：
        // 1. 存在思考块 (saw_thought)
        // 2. 无任何正文 (saw_content == false)
        // 3. 无任何工具调用 (saw_tool_call == false)
        // 4. 上游正常返回了结束符 (saw_finish_reason == true)
        // 5. 尚未执行过自愈 (auto_healed == false，严格 1 次上限)
        if saw_thought && !saw_content && !saw_tool_call && saw_finish_reason && !auto_healed {
            auto_healed = true;
            tracing::warn!(
                "[{}] [Stream-AutoHeal] 🚨 Detected empty thinking completion (thought present, 0 content, 0 tool_calls, finishReason={:?}). Triggering auto-heal continuation 1/1...",
                ctx.trace_id, finish_reason_val
            );

            // 发射 SSE 心跳注释保持客户端下游连接存活
            yield Ok(Bytes::from(": auto-healing empty thinking\n\n"));

            let prompt = resolve_continuation_prompt(&ctx.original_body);
            let heal_body = create_auto_heal_continuation_body(&ctx.original_body, prompt);

            let call_res = ctx.upstream.call_v1_internal_with_headers(
                ctx.method,
                &ctx.access_token,
                heal_body,
                ctx.query_string,
                ctx.extra_headers.clone(),
                ctx.account_id.as_deref(),
            ).await;

            match call_res {
                Ok(call_success) if call_success.response.status().is_success() => {
                    tracing::info!(
                        "[{}] [Stream-AutoHeal] ✓ Continuation request succeeded (HTTP 200), piping healed stream directly into client connection...",
                        ctx.trace_id
                    );
                    let mut stream2 = call_success.response.bytes_stream();
                    let mut stream2_saw_output = false;
                    let mut s2_buffer = BytesMut::new();

                    while let Some(chunk_res) = stream2.next().await {
                        match chunk_res {
                            Ok(bytes) => {
                                s2_buffer.extend_from_slice(&bytes);
                                while let Some(pos) = s2_buffer.iter().position(|&b| b == b'\n') {
                                    let line_raw = s2_buffer.split_to(pos + 1);
                                    if let Ok(line_str) = std::str::from_utf8(&line_raw) {
                                        let line = line_str.trim();
                                        if line.starts_with("data: ") && line != "data: [DONE]" {
                                            let json_part = line.trim_start_matches("data: ").trim();
                                            if let Ok(json) = serde_json::from_str::<Value>(json_part) {
                                                let inner = json.get("response").unwrap_or(&json);
                                                if let Some(candidates) = inner.get("candidates").and_then(|c| c.as_array()) {
                                                    for cand in candidates {
                                                        let mut dummy_thought = false;
                                                        let mut s2_content = false;
                                                        let mut s2_tool = false;
                                                        inspect_gemini_candidate_parts(
                                                            cand,
                                                            &mut dummy_thought,
                                                            &mut s2_content,
                                                            &mut s2_tool,
                                                        );
                                                        if s2_content || s2_tool {
                                                            stream2_saw_output = true;
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                yield Ok(bytes);
                            }
                            Err(e) => {
                                tracing::warn!("[{}] [Stream-AutoHeal] Stream 2 read error: {:?}", ctx.trace_id, e);
                                break;
                            }
                        }
                    }

                    // 兜底防御：若续跑流依然未产出任何可视正文或工具调用，注入微小兜底避免客户端崩溃
                    if !stream2_saw_output {
                        tracing::warn!(
                            "[{}] [Stream-AutoHeal] Continuation stream also returned 0 output. Injecting fallback message to prevent client disconnect.",
                            ctx.trace_id
                        );
                        let fallback_text = "task ready";
                        let fallback_chunk = format!(
                            "data: {}\n\n",
                            serde_json::to_string(&json!({
                                "candidates": [{
                                    "content": {
                                        "parts": [{ "text": fallback_text }],
                                        "role": "model"
                                    },
                                    "finishReason": "STOP"
                                }]
                            })).unwrap_or_default()
                        );
                        yield Ok(Bytes::from(fallback_chunk));
                    }
                }
                Ok(call_fail) => {
                    let status = call_fail.response.status();
                    tracing::error!(
                        "[{}] [Stream-AutoHeal] Continuation request returned HTTP {}. Injecting fallback message.",
                        ctx.trace_id, status
                    );
                    let fallback_text = "task ready";
                    let fallback_chunk = format!(
                        "data: {}\n\n",
                        serde_json::to_string(&json!({
                            "candidates": [{
                                "content": {
                                    "parts": [{ "text": fallback_text }],
                                    "role": "model"
                                },
                                "finishReason": "STOP"
                            }]
                        })).unwrap_or_default()
                    );
                    yield Ok(Bytes::from(fallback_chunk));
                }
                Err(e) => {
                    tracing::error!(
                        "[{}] [Stream-AutoHeal] Continuation request failed: {}. Injecting fallback message.",
                        ctx.trace_id, e
                    );
                    let fallback_text = "task ready";
                    let fallback_chunk = format!(
                        "data: {}\n\n",
                        serde_json::to_string(&json!({
                            "candidates": [{
                                "content": {
                                    "parts": [{ "text": fallback_text }],
                                    "role": "model"
                                },
                                "finishReason": "STOP"
                            }]
                        })).unwrap_or_default()
                    );
                    yield Ok(Bytes::from(fallback_chunk));
                }
            }
        }
    };
    Box::pin(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_continuation_prompt_chinese() {
        let body = json!({
            "request": {
                "contents": [
                    { "role": "user", "parts": [{ "text": "帮我看看这个报错怎么解决" }] }
                ]
            }
        });
        assert_eq!(resolve_continuation_prompt(&body), "继续");
    }

    #[test]
    fn test_resolve_continuation_prompt_english() {
        let body = json!({
            "request": {
                "contents": [
                    { "role": "user", "parts": [{ "text": "Please fix this bug in the repo" }] }
                ]
            }
        });
        assert_eq!(resolve_continuation_prompt(&body), "Continue.");
    }

    #[test]
    fn test_create_auto_heal_continuation_body() {
        let original = json!({
            "requestId": "agent/1234/1",
            "request": {
                "requestId": "agent/1234/1",
                "contents": [
                    { "role": "user", "parts": [{ "text": "Hello" }] }
                ]
            }
        });
        let healed = create_auto_heal_continuation_body(&original, "Continue.");
        assert_eq!(healed["requestId"], "agent/1234/1_heal1");
        assert_eq!(healed["request"]["requestId"], "agent/1234/1_heal1");
        let contents = healed["request"]["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 2);
        assert_eq!(contents[1]["role"], "user");
        assert_eq!(contents[1]["parts"][0]["text"], "Continue.");
    }

    #[test]
    fn test_inspect_candidate_parts_pure_thought() {
        let cand = json!({
            "content": {
                "parts": [
                    { "thought": true, "text": "I should run git pull" }
                ]
            }
        });
        let mut saw_thought = false;
        let mut saw_content = false;
        let mut saw_tool_call = false;
        inspect_gemini_candidate_parts(
            &cand,
            &mut saw_thought,
            &mut saw_content,
            &mut saw_tool_call,
        );
        assert!(saw_thought);
        assert!(!saw_content);
        assert!(!saw_tool_call);
    }

    #[test]
    fn test_inspect_candidate_parts_with_tool_call() {
        let cand = json!({
            "content": {
                "parts": [
                    { "thought": true, "text": "I will call bash" },
                    { "functionCall": { "name": "run_command", "args": {} } }
                ]
            }
        });
        let mut saw_thought = false;
        let mut saw_content = false;
        let mut saw_tool_call = false;
        inspect_gemini_candidate_parts(
            &cand,
            &mut saw_thought,
            &mut saw_content,
            &mut saw_tool_call,
        );
        assert!(saw_thought);
        assert!(!saw_content);
        assert!(saw_tool_call);
    }

    #[test]
    fn test_inspect_candidate_parts_with_content() {
        let cand = json!({
            "content": {
                "parts": [
                    { "thought": true, "text": "thinking..." },
                    { "text": "Here is the response" }
                ]
            }
        });
        let mut saw_thought = false;
        let mut saw_content = false;
        let mut saw_tool_call = false;
        inspect_gemini_candidate_parts(
            &cand,
            &mut saw_thought,
            &mut saw_content,
            &mut saw_tool_call,
        );
        assert!(saw_thought);
        assert!(saw_content);
        assert!(!saw_tool_call);
    }

    #[test]
    fn test_inspect_candidate_parts_inline_data() {
        let cand = json!({
            "content": {
                "parts": [
                    { "thought": true, "text": "generating image..." },
                    { "inlineData": { "mimeType": "image/png", "data": "base64..." } }
                ]
            }
        });
        let mut saw_thought = false;
        let mut saw_content = false;
        let mut saw_tool_call = false;
        inspect_gemini_candidate_parts(
            &cand,
            &mut saw_thought,
            &mut saw_content,
            &mut saw_tool_call,
        );
        assert!(saw_thought);
        assert!(saw_content);
        assert!(!saw_tool_call);
    }

    #[test]
    fn test_create_auto_heal_continuation_body_root_contents() {
        let original = json!({
            "requestId": "req_root_123",
            "contents": [
                { "role": "user", "parts": [{ "text": "Do task" }] }
            ]
        });
        let healed = create_auto_heal_continuation_body(&original, "继续");
        assert_eq!(healed["requestId"], "req_root_123_heal1");
        let contents = healed["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 2);
        assert_eq!(contents[1]["role"], "user");
        assert_eq!(contents[1]["parts"][0]["text"], "继续");
    }

    #[test]
    fn test_resolve_continuation_prompt_fallback() {
        let body = json!({
            "request": {
                "contents": []
            }
        });
        assert_eq!(resolve_continuation_prompt(&body), "Continue.");
    }
}
