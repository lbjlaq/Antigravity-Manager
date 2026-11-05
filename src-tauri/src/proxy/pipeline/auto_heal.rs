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

use crate::proxy::pipeline::inbound::InboundThinkingPipeline;
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
    /// 请求级预算上下文（构造时捕获）
    ///
    /// 续跑发生在 `Body::from_stream` 被 poll 时，此时入口中间件的
    /// `task_local` 已经失效；必须显式恢复，否则续跑不受全局预算与
    /// 大 body 重放护栏约束。
    pub budget: Option<Arc<crate::proxy::pipeline::budget::RequestContext>>,
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

/// 向 Gemini contents 中安全注入或融合续跑提示词，严格保持角色交替
pub fn append_or_merge_continuation(contents: &mut Vec<Value>, continuation_prompt: &str) {
    let last_role = contents
        .last()
        .and_then(|c| c.get("role"))
        .and_then(|r| r.as_str())
        .unwrap_or("");

    if last_role == "user" {
        // [FIX #3694] Gemini API 严格要求 multi-turn 对话中角色严格交替 (user <-> model)。
        // 若原上下文最后一轮已为 user 轮次，追加新的 user 轮次将产生连续 user [user, user]，
        // 触发 Gemini HTTP 400 拒收报错: "Please ensure that multiturn requests alternate between user and model roles."
        // 因此直接将续跑提示词合入该 user 轮次的 parts 末尾。
        if let Some(last_turn) = contents.last_mut() {
            if let Some(parts) = last_turn.get_mut("parts").and_then(|p| p.as_array_mut()) {
                if let Some(last_part) = parts.last_mut().and_then(|p| p.as_object_mut()) {
                    if let Some(Value::String(text)) = last_part.get_mut("text") {
                        if !text.is_empty() {
                            text.push_str("\n\n");
                        }
                        text.push_str(continuation_prompt);
                    } else {
                        parts.push(json!({ "text": continuation_prompt }));
                    }
                } else {
                    parts.push(json!({ "text": continuation_prompt }));
                }
            } else {
                last_turn["parts"] = json!([{ "text": continuation_prompt }]);
            }
        }
    } else {
        // 最后一轮为 model/assistant 或内容为空，安全追加独立的 user 轮次
        contents.push(json!({
            "role": "user",
            "parts": [{ "text": continuation_prompt }]
        }));
    }
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

    // 2. 严格交替注入/融合 User 续跑提示轮次
    if let Some(contents) = new_body
        .get_mut("request")
        .and_then(|r| r.get_mut("contents"))
        .and_then(|c| c.as_array_mut())
    {
        append_or_merge_continuation(contents, continuation_prompt);
    } else if let Some(contents) = new_body.get_mut("contents").and_then(|c| c.as_array_mut()) {
        append_or_merge_continuation(contents, continuation_prompt);
    }

    new_body
}

/// 统一的「原地自愈」请求状态（每个请求一份）
#[derive(Debug, Default, Clone, Copy)]
pub struct AutoHealFlags {
    /// 已因 tool_choice/thinking 冲突剥离过 thinkingConfig
    pub retried_tool_choice_conflict: bool,
    /// 已因 503/过载降级过思考预算
    pub retried_with_budget_degraded: bool,
    /// 已因 503 容量枯竭 (MODEL_CAPACITY_EXHAUSTED) 原地升档为 High
    pub retried_with_capacity_promoted: bool,
    /// 已因 400 图像解码失败剔除过损坏图片
    pub retried_with_image_omitted: bool,
}

/// 一次自愈的执行结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoHealOutcome {
    /// 诊断用短标签（写入日志/阶段时间线）
    pub kind: &'static str,
    /// 同账号重试前的等待时长
    pub delay_ms: u64,
}

/// 通用原地自愈执行器（协议无关，操作的是已转换的 Gemini Canonical IR）
///
/// 四个适配器过去各写一份同样的判断与改写：
/// 1. 503/529 过载 → 降级思考预算（保住 prefix cache）；
/// 2. 400 tool_choice/thinking 冲突 → 剥离 thinkingConfig；
/// 3. 400 图像解码失败 → 剔除损坏图片并降级为文本占位。
///
/// 三者的共同点：**都是对已转换完成的 Gemini 报文做原地修复，且必须在同账号重试**
/// （换号只会让同一份坏载荷再失败一次，并浪费轮换预算）。
///
/// 返回 `Some` 表示已在同一账号上完成修复，调用方应 `continue` 重试；
/// `None` 表示本次不需要 / 无法自愈，继续走既有错误策略。
pub fn try_auto_heal(
    classification: &crate::proxy::pipeline::UpstreamClassification,
    status_code: u16,
    error_text: &str,
    body: &mut Value,
    degraded: &mut Option<Value>,
    flags: &mut AutoHealFlags,
) -> Option<AutoHealOutcome> {
    // 0. 上游容量枯竭 (MODEL_CAPACITY_EXHAUSTED / 503)：原地将 medium/low/tiered 升档为 high 算力通道
    if crate::proxy::upstream::client::UpstreamClient::is_model_capacity_exhausted(
        rquest::StatusCode::from_u16(status_code).unwrap_or(rquest::StatusCode::SERVICE_UNAVAILABLE),
        error_text,
    ) && !flags.retried_with_capacity_promoted
    {
        if InboundThinkingPipeline::try_promote_to_high_capacity(body) {
            flags.retried_with_capacity_promoted = true;
            *degraded = Some(body.clone());
            tracing::warn!(
                "[AutoHeal] Upstream 503 MODEL_CAPACITY_EXHAUSTED: promoted tier to HIGH and retrying on same account"
            );
            return Some(AutoHealOutcome {
                kind: "promote_tier_to_high_capacity",
                delay_ms: 50,
            });
        }
    }

    // 1. 上游过载：降级思考预算，避免被调度到高算力集群继续排队
    if crate::proxy::pipeline::UpstreamClassification::is_server_overloaded(status_code, error_text)
        && !flags.retried_with_budget_degraded
        && InboundThinkingPipeline::try_degrade_thinking_budget(body)
    {
        flags.retried_with_budget_degraded = true;
        *degraded = Some(body.clone());
        tracing::warn!(
            "[AutoHeal] upstream {}/overloaded: degraded thinking budget to 4000 (preserving prefix cache) err={}",
            status_code,
            crate::proxy::upstream::client::strip_internal_markers(error_text)
        );
        return Some(AutoHealOutcome {
            kind: "degrade_thinking_budget",
            delay_ms: 150,
        });
    }

    // 2. 强制工具调用与思考冲突：剥离 thinkingConfig 后重试
    if crate::proxy::pipeline::UpstreamClassification::is_thinking_tool_choice_conflict(
        status_code,
        error_text,
    ) && !flags.retried_tool_choice_conflict
    {
        flags.retried_tool_choice_conflict = true;
        if let Some(gc) = body
            .get_mut("request")
            .and_then(|r| r.get_mut("generationConfig"))
            .and_then(Value::as_object_mut)
        {
            gc.remove("thinkingConfig");
        } else if let Some(gc) = body
            .get_mut("generationConfig")
            .and_then(Value::as_object_mut)
        {
            gc.remove("thinkingConfig");
        }
        *degraded = Some(body.clone());
        tracing::warn!(
            "[AutoHeal] upstream 400 tool_choice/thinking conflict: stripped thinkingConfig and retrying on same account"
        );
        return Some(AutoHealOutcome {
            kind: "strip_thinking_on_tool_choice_conflict",
            delay_ms: 50,
        });
    }

    // 3. 图像解码/处理失败：剔除损坏图片（降级为文本占位）后重试
    if classification.is_image_processing_error() && !flags.retried_with_image_omitted {
        let purged =
            crate::proxy::mappers::common_utils::purge_corrupted_images_from_gemini_payload(body);
        if purged == 0 {
            tracing::warn!(
                "[AutoHeal] upstream 400 unable to process input image, but no inline image was purged; skipping retry to prevent duplicate 400"
            );
            return None;
        }
        flags.retried_with_image_omitted = true;
        *degraded = Some(body.clone());
        tracing::warn!(
            "[AutoHeal] upstream 400 unable to process image: purged {} image(s) to text placeholders, retrying on same account",
            purged
        );
        return Some(AutoHealOutcome {
            kind: "purge_corrupted_images",
            delay_ms: 50,
        });
    }

    None
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
        if saw_thought && !saw_content && !saw_tool_call && saw_finish_reason {
            tracing::warn!(
                "[{}] [Stream-AutoHeal] 🚨 Detected empty thinking completion (thought present, 0 content, 0 tool_calls, finishReason={:?}). Triggering auto-heal continuation 1/1...",
                ctx.trace_id, finish_reason_val
            );

            // 发射 SSE 心跳注释保持客户端下游连接存活
            yield Ok(Bytes::from(": auto-healing empty thinking\n\n"));

            let prompt = resolve_continuation_prompt(&ctx.original_body);
            let heal_body = create_auto_heal_continuation_body(&ctx.original_body, prompt);

            let call_res = crate::proxy::pipeline::budget::scope_optional(
                ctx.budget.clone(),
                ctx.upstream.call_v1_internal_with_headers(
                    ctx.method,
                    &ctx.access_token,
                    heal_body,
                    ctx.query_string,
                    ctx.extra_headers.clone(),
                    ctx.account_id.as_deref(),
                ),
            )
            .await;

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
        // [FIX #3694] 最后一轮已是 user 时合入该轮次，严格保持单轮或交替规则，避免 [user, user] 导致 400
        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0]["role"], "user");
        assert_eq!(contents[0]["parts"][0]["text"], "Hello\n\nContinue.");
    }

    #[test]
    fn test_create_auto_heal_continuation_body_when_last_turn_is_model() {
        let original = json!({
            "request": {
                "contents": [
                    { "role": "user", "parts": [{ "text": "What is 2+2?" }] },
                    { "role": "model", "parts": [{ "text": "Let me calculate." }] }
                ]
            }
        });
        let healed = create_auto_heal_continuation_body(&original, "Continue.");
        let contents = healed["request"]["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 3);
        assert_eq!(contents[1]["role"], "model");
        assert_eq!(contents[2]["role"], "user");
        assert_eq!(contents[2]["parts"][0]["text"], "Continue.");
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
        // [FIX #3694] 最后一轮已是 user 时合入该轮次
        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0]["role"], "user");
        assert_eq!(contents[0]["parts"][0]["text"], "Do task\n\n继续");
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

    #[test]
    fn test_auto_heal_strips_thinking_on_tool_choice_conflict() {
        let classification = crate::proxy::pipeline::UpstreamClassification::classify(
            400,
            "thinking may not be enabled when tool_choice forces tool use",
            None,
        );
        let mut body = json!({
            "request": {
                "generationConfig": { "thinkingConfig": { "includeThoughts": true } }
            }
        });
        let mut degraded = None;
        let mut flags = AutoHealFlags::default();
        let out = try_auto_heal(
            &classification,
            400,
            "thinking may not be enabled when tool_choice forces tool use",
            &mut body,
            &mut degraded,
            &mut flags,
        );
        assert_eq!(
            out.map(|o| o.kind),
            Some("strip_thinking_on_tool_choice_conflict")
        );
        assert!(flags.retried_tool_choice_conflict);
        assert!(body["request"]["generationConfig"]
            .get("thinkingConfig")
            .is_none());
        assert!(degraded.is_some());

        // 第二次不再自愈（单次上限）
        let again = try_auto_heal(
            &classification,
            400,
            "thinking may not be enabled when tool_choice forces tool use",
            &mut body,
            &mut degraded,
            &mut flags,
        );
        assert!(again.is_none());
    }

    #[test]
    fn test_auto_heal_purges_corrupted_images_once() {
        let classification = crate::proxy::pipeline::UpstreamClassification::classify(
            400,
            "unable to process input image",
            None,
        );
        let mut body = json!({
            "contents": [{
                "role": "user",
                "parts": [{
                    "inlineData": { "mimeType": "image/png", "data": "!!!!not-base64!!!!" }
                }]
            }]
        });
        let mut degraded = None;
        let mut flags = AutoHealFlags::default();
        let out = try_auto_heal(
            &classification,
            400,
            "unable to process input image",
            &mut body,
            &mut degraded,
            &mut flags,
        );
        // 损坏图片被剔除（或占位）；关键是不再重试第二次
        assert!(out.is_some());
        assert!(flags.retried_with_image_omitted);
        let again = try_auto_heal(
            &classification,
            400,
            "unable to process input image",
            &mut body,
            &mut degraded,
            &mut flags,
        );
        assert!(again.is_none());
    }
}
