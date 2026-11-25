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

fn rewrite_empty_heal_request_id(val: &mut Value, attempt: usize) {
    if let Some(s) = val.as_str() {
        let base = s
            .rsplit_once("_empty_heal")
            .filter(|(_, suffix)| !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()))
            .map(|(prefix, _)| prefix)
            .unwrap_or(s);
        *val = json!(format!("{}_empty_heal{}", base, attempt));
    }
}

/// 构造用于「早退空流 / signature-only EOF」自愈的重试请求 Body (Fixes #4055)
///
/// 在原请求体基础上更新 `requestId` 防止上游去重，并调用 `try_degrade_thinking_step`
/// 执行渐进式思考降级（如 `HIGH -> MEDIUM -> LOW`），返回 `(new_body, degraded)`。
pub fn build_premature_empty_retry_body(prev_body: &Value, attempt: usize) -> (Value, bool) {
    let mut retry_body = prev_body.clone();
    if let Some(req_id) = retry_body.get_mut("requestId") {
        rewrite_empty_heal_request_id(req_id, attempt);
    }
    if let Some(req) = retry_body
        .get_mut("request")
        .and_then(|r| r.as_object_mut())
    {
        if let Some(req_id) = req.get_mut("requestId") {
            rewrite_empty_heal_request_id(req_id, attempt);
        }
    }
    let degraded = InboundThinkingPipeline::try_degrade_thinking_step(&mut retry_body, true);
    (retry_body, degraded)
}

fn extract_model_for_log(body: &Value) -> &str {
    body.get("model")
        .and_then(|v| v.as_str())
        .or_else(|| {
            body.get("request")
                .and_then(|r| r.get("model"))
                .and_then(|v| v.as_str())
        })
        .unwrap_or("unknown")
}

/// 统一的「原地自愈」请求状态（每个请求一份）
#[derive(Debug, Default, Clone, Copy)]
pub struct AutoHealFlags {
    /// 已因 tool_choice/thinking 冲突剥离过 thinkingConfig
    pub retried_tool_choice_conflict: bool,
    /// 已因 400 不支持的 forced tool_choice 降级过 responseSchema
    pub retried_with_response_schema_demoted: bool,
    /// 已因 503/过载降级过思考预算
    pub retried_with_budget_degraded: bool,
    /// 已因 503 容量枯竭 (MODEL_CAPACITY_EXHAUSTED) 原地升档为 High
    pub retried_with_capacity_promoted: bool,
    /// 已因 400 图像解码失败剔除过损坏图片
    pub retried_with_image_omitted: bool,
    /// 已因 400 "Thinking level is unsupported" 把扩展思考档钳制为 HIGH
    pub retried_with_thinking_level_clamped: bool,
}

fn demote_response_schema_in_payload(body: &mut Value) -> bool {
    let req = if body.get("request").is_some() {
        body.get_mut("request").unwrap()
    } else {
        body
    };
    let Some(schema) = req
        .get_mut("generationConfig")
        .and_then(Value::as_object_mut)
        .and_then(|gc| {
            let schema = gc.remove("responseSchema")?;
            gc.remove("responseMimeType");
            Some(schema)
        })
    else {
        return false;
    };

    let schema_text = format!(
        "\n\n[OUTPUT INSTRUCTION: You MUST respond ONLY with a valid, raw JSON object conforming strictly to the following JSON Schema. Do NOT include markdown code blocks (```json), commentary, or any text other than the raw JSON object.]\n{}",
        serde_json::to_string_pretty(&schema).unwrap_or_else(|_| schema.to_string())
    );

    if let Some(contents_arr) = req.get_mut("contents").and_then(Value::as_array_mut) {
        let mut injected = false;
        for content in contents_arr.iter_mut().rev() {
            if content.get("role").and_then(|r| r.as_str()) == Some("user") {
                if let Some(parts) = content.get_mut("parts").and_then(|p| p.as_array_mut()) {
                    parts.push(json!({ "text": schema_text }));
                    injected = true;
                    break;
                }
            }
        }
        if !injected {
            contents_arr.push(json!({
                "role": "user",
                "parts": [{ "text": schema_text }]
            }));
        }
    }
    true
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
/// 2.5 400 不支持的 forced tool_choice → 将 responseSchema 降级至末轮 user 消息并保留 thinkingConfig；
/// 3. 400 账号不支持扩展思考档（`THINKING_LEVEL_XHIGH`）→ 钳制为 HIGH；
/// 4. 400 图像解码失败 → 剔除损坏图片并降级为文本占位。
///
/// 共同点：**都是对已转换完成的 Gemini 报文做原地修复，且必须在同账号重试**
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
    let is_capacity_exhausted =
        crate::proxy::upstream::client::UpstreamClient::is_model_capacity_exhausted(
            rquest::StatusCode::from_u16(status_code)
                .unwrap_or(rquest::StatusCode::SERVICE_UNAVAILABLE),
            error_text,
        );

    if is_capacity_exhausted && !flags.retried_with_capacity_promoted {
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
    // 注意：若上游明确是 MODEL_CAPACITY_EXHAUSTED（容量枯竭），专属自愈策略是分支 0（升档或原地失败）；
    // 严禁在此处将其视为普通过载执行降级，避免 high <-> medium 乒乓震荡 (Issue #4000)。
    // 同时，若请求已升档至 high 算力通道 (retried_with_capacity_promoted)，也禁止再被降级回去。
    if !is_capacity_exhausted
        && !flags.retried_with_capacity_promoted
        && crate::proxy::pipeline::UpstreamClassification::is_server_overloaded(
            status_code,
            error_text,
        )
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

    // 2. 强制工具调用与思考冲突：仅剥离 thinkingConfig 后重试 (Fixes #3721)
    if crate::proxy::pipeline::UpstreamClassification::is_thinking_tool_choice_conflict(
        status_code,
        error_text,
    ) && !flags.retried_tool_choice_conflict
    {
        flags.retried_tool_choice_conflict = true;
        {
            let req = if body.get("request").is_some() {
                body.get_mut("request").unwrap()
            } else {
                &mut *body
            };
            if let Some(gc) = req
                .get_mut("generationConfig")
                .and_then(Value::as_object_mut)
            {
                gc.remove("thinkingConfig");
            }
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

    // 2.5 不支持的 forced tool_choice（如上游将 responseSchema 转为 forced tool）：降级 responseSchema 至末轮 user 消息并保留 thinkingConfig (Ref #4033, PR #4042)
    if crate::proxy::pipeline::UpstreamClassification::is_unsupported_forced_tool_choice(
        status_code,
        error_text,
    ) && !flags.retried_with_response_schema_demoted
    {
        if demote_response_schema_in_payload(body) {
            flags.retried_with_response_schema_demoted = true;
            *degraded = Some(body.clone());
            tracing::warn!(
                "[AutoHeal] upstream 400 unsupported forced tool_choice: demoted responseSchema to user turn (preserving thinkingConfig) and retrying on same account (Ref #4033)"
            );
            return Some(AutoHealOutcome {
                kind: "demote_response_schema_on_forced_tool_choice",
                delay_ms: 50,
            });
        }
        return None;
    }

    // 3. 账号不支持扩展思考档（400 "Thinking level is unsupported: THINKING_LEVEL_XHIGH"）：
    //    官方目录对所有账号都把 -xhigh 声明为 thinkingLevel 100，但部分账号生成时拒绝。
    //    同账号钳制为 HIGH（-xhigh 模型降为 -high）重试一次，避免整轮对话以 400 结束；
    //    支持 XHIGH 的账号永远不会收到这个 400，因此不受影响。
    if crate::proxy::pipeline::UpstreamClassification::is_unsupported_thinking_level(
        status_code,
        error_text,
    ) && !flags.retried_with_thinking_level_clamped
        && InboundThinkingPipeline::try_clamp_thinking_level(body)
    {
        flags.retried_with_thinking_level_clamped = true;
        *degraded = Some(body.clone());
        tracing::warn!(
            "[AutoHeal] upstream 400 thinking level unsupported: clamped extended thinkingLevel to HIGH and retrying on same account err={}",
            crate::proxy::upstream::client::strip_internal_markers(error_text)
        );
        return Some(AutoHealOutcome {
            kind: "clamp_unsupported_thinking_level",
            delay_ms: 50,
        });
    }

    // 4. 图像解码/处理失败：剔除损坏图片（降级为文本占位）后重试
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

/// 构造纯思考收尾无假正文的标准终止帧组（finishReason: STOP 与 [DONE]）
pub fn make_clean_empty_thought_stop_chunks(last_usage: Option<&Value>) -> (Bytes, Bytes) {
    let mut candidate_obj = serde_json::Map::new();
    candidate_obj.insert("finishReason".to_string(), json!("STOP"));
    let mut stop_resp = json!({
        "candidates": [Value::Object(candidate_obj)]
    });
    if let Some(usage) = last_usage {
        stop_resp["usageMetadata"] = usage.clone();
    }
    let stop_chunk = format!(
        "data: {}\n\n",
        serde_json::to_string(&stop_resp).unwrap_or_default()
    );
    (Bytes::from(stop_chunk), Bytes::from("data: [DONE]\n\n"))
}

const MAX_PENDING_EMPTY_FRAMES: usize = 8;
const MAX_PREMATURE_EMPTY_RETRIES: usize = 2;
const MIN_PREMATURE_EMPTY_RETRY_BUDGET_SECS: u64 = 10;
const EMPTY_STOP_ERROR: &str =
    "Upstream stream ended with STOP before emitting content or tool calls";

fn is_empty_stop(saw_content: bool, saw_tool_call: bool, finish_reason: Option<&str>) -> bool {
    !saw_content && !saw_tool_call && finish_reason == Some("STOP")
}

/// 解析并跟踪上游单条 SSE `data: <json>` 负载的思考 / 正文 / 工具 / 终结状态：
/// - 若尚未产生任何实质内容或终结符（如 Issue #4055 的 `thoughtSignature + text: ""` 空壳帧），
///   暂存进 `pending_empty_frames` 而不立即透传，防止空转帧触发提前 EOF 时泄漏到下游或污染签名库；
/// - 一旦后续帧产出任何实质内容或终结符，按序先冲刷全部暂存帧再发出当前帧。
#[allow(clippy::too_many_arguments)]
fn inspect_and_buffer_sse_json(
    json_part: &str,
    saw_thought: &mut bool,
    saw_content: &mut bool,
    saw_tool_call: &mut bool,
    saw_finish_reason: &mut bool,
    finish_reason_val: &mut Option<String>,
    last_usage_val: &mut Option<Value>,
    pending_empty_frames: &mut Vec<Bytes>,
) -> Vec<Bytes> {
    let was_before_real_output =
        !*saw_thought && !*saw_content && !*saw_tool_call && !*saw_finish_reason;

    let mut json = match serde_json::from_str::<Value>(json_part) {
        Ok(v) => v,
        Err(_) => {
            return vec![Bytes::from(format!("data: {}\n\n", json_part))];
        }
    };

    let has_resp = json.get("response").is_some();
    let inner = if has_resp {
        json.get_mut("response").unwrap()
    } else {
        &mut json
    };
    if let Some(usage) = inner.get("usageMetadata").or_else(|| inner.get("usage")) {
        *last_usage_val = Some(usage.clone());
    }
    let mut chunk_has_finish = false;
    let mut chunk_finish_is_stop = false;
    let mut chunk_has_parts = false;

    if let Some(candidates) = inner.get_mut("candidates").and_then(|c| c.as_array_mut()) {
        for cand in candidates.iter_mut() {
            inspect_gemini_candidate_parts(cand, saw_thought, saw_content, saw_tool_call);
            if let Some(parts) = cand
                .get("content")
                .and_then(|c| c.get("parts"))
                .and_then(|p| p.as_array())
            {
                if !parts.is_empty() {
                    chunk_has_parts = true;
                }
            }
            if let Some(fr) = cand.get("finishReason").and_then(|f| f.as_str()) {
                chunk_has_finish = true;
                *saw_finish_reason = true;
                *finish_reason_val = Some(fr.to_string());
                if fr == "STOP" {
                    chunk_finish_is_stop = true;
                }
            }
        }
    }

    let has_any_signal = *saw_thought || *saw_content || *saw_tool_call || *saw_finish_reason;
    if !has_any_signal {
        pending_empty_frames.push(Bytes::from(format!("data: {}\n\n", json_part)));
        if pending_empty_frames.len() >= MAX_PENDING_EMPTY_FRAMES {
            return pending_empty_frames.drain(..).collect();
        }
        return Vec::new();
    }

    let mut out = if was_before_real_output && !pending_empty_frames.is_empty() {
        pending_empty_frames.drain(..).collect()
    } else {
        Vec::new()
    };

    if *saw_content || *saw_tool_call {
        out.push(Bytes::from(format!("data: {}\n\n", json_part)));
    } else if chunk_has_finish && chunk_finish_is_stop {
        // 思考空回复异常候选帧（仅限 finishReason == "STOP"）：
        // 若当前帧还包含思考部件，清洗剥离 finishReason 与 usageMetadata 后放行思考，不发射终止信号
        if *saw_thought && chunk_has_parts {
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
            out.push(Bytes::from(format!("data: {}\n\n", sanitized)));
        }
    } else {
        // 普通思考块、中间流帧、以及非 STOP 终止帧（如 MAX_TOKENS、SAFETY 等）：原样透传
        out.push(Bytes::from(format!("data: {}\n\n", json_part)));
    }

    out
}

/// 统一流式自愈包装器（Pipeline First）：
///
/// 同时防护两类上游退化流：
/// 1. **纯思考空回复**（有思考、0 正文、0 工具调用且以 `finishReason: STOP` 收尾）→ 自动续跑 1 次；
/// 2. **早退空流 / signature-only EOF**（如仅返回 `{"thoughtSignature": ..., "text": ""}`、无 `finishReason` 即断流, Fixes #4055）
///    → 暂存空壳帧不泄漏，按当前剩余全局预算在同连接内执行最多 2 次渐进思考降级重放（如 `HIGH -> MEDIUM -> LOW`）。
pub fn wrap_stream_with_empty_thinking_auto_heal<S, E>(
    stream: Pin<Box<S>>,
    ctx: ThinkingAutoHealContext,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, String>> + Send>>
where
    S: Stream<Item = Result<Bytes, E>> + Send + 'static,
    E: std::fmt::Display + Send + 'static,
{
    let upstream = ctx.upstream;
    let method = ctx.method;
    let access_token = ctx.access_token;
    let query_string = ctx.query_string;
    let extra_headers = ctx.extra_headers;
    let account_id = ctx.account_id;
    let budget_for_call = ctx.budget.clone();

    wrap_stream_with_auto_heal_inner(
        stream,
        ctx.original_body,
        ctx.trace_id,
        ctx.budget,
        move |req_body| {
            let upstream = upstream.clone();
            let access_token = access_token.clone();
            let extra_headers = extra_headers.clone();
            let account_id = account_id.clone();
            let budget = budget_for_call.clone();
            async move {
                let res = crate::proxy::pipeline::budget::scope_optional(
                    budget,
                    upstream.call_v1_internal_with_headers(
                        method,
                        &access_token,
                        req_body,
                        query_string,
                        extra_headers,
                        account_id.as_deref(),
                    ),
                )
                .await;
                match res {
                    Ok(call_success) if call_success.response.status().is_success() => {
                        Ok(Box::pin(call_success.response.bytes_stream()))
                    }
                    Ok(call_fail) => Err(format!("HTTP {}", call_fail.response.status())),
                    Err(e) => Err(e.to_string()),
                }
            }
        },
    )
}

fn wrap_stream_with_auto_heal_inner<S, E, F, Fut, S2, E2>(
    stream: Pin<Box<S>>,
    original_body: Value,
    trace_id: String,
    budget: Option<Arc<crate::proxy::pipeline::budget::RequestContext>>,
    mut retry_call: F,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, String>> + Send>>
where
    S: Stream<Item = Result<Bytes, E>> + Send + 'static,
    E: std::fmt::Display + Send + 'static,
    F: FnMut(Value) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<Pin<Box<S2>>, String>> + Send + 'static,
    S2: Stream<Item = Result<Bytes, E2>> + Send + ?Sized + 'static,
    E2: std::fmt::Debug + Send + 'static,
{
    let stream = async_stream::stream! {
        let mut buffer = BytesMut::new();
        let mut saw_thought = false;
        let mut saw_content = false;
        let mut saw_tool_call = false;
        let mut saw_finish_reason = false;
        let mut finish_reason_val: Option<String> = None;
        let mut last_usage_val: Option<Value> = None;
        let mut pending_empty_frames: Vec<Bytes> = Vec::new();

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
                                // 仅当确认属于思考空回复异常（无正文、无工具调用且以 STOP 结束）时才暂缓发射 [DONE] 以便进行自愈
                                let is_empty_thought_stop_collapse = is_empty_stop(
                                    saw_content, saw_tool_call, finish_reason_val.as_deref(),
                                );
                                if !is_empty_thought_stop_collapse {
                                    for pending in pending_empty_frames.drain(..) {
                                        yield Ok(pending);
                                    }
                                    yield Ok(Bytes::from("data: [DONE]\n\n"));
                                }
                                continue;
                            }

                            for chunk in inspect_and_buffer_sse_json(
                                json_part,
                                &mut saw_thought,
                                &mut saw_content,
                                &mut saw_tool_call,
                                &mut saw_finish_reason,
                                &mut finish_reason_val,
                                &mut last_usage_val,
                                &mut pending_empty_frames,
                            ) {
                                yield Ok(chunk);
                            }
                        } else {
                            // 保持非 data 行（如保活注释）正常下发
                            yield Ok(Bytes::from(format!("{}\n\n", line)));
                        }
                    }
                }
                Err(e) => {
                    for pending in pending_empty_frames.drain(..) {
                        yield Ok(pending);
                    }
                    yield Err(e.to_string());
                    return;
                }
            }
        }

        // 冲刷残留缓冲区（含末尾无换行符的 data 行）
        if !buffer.is_empty() {
            if let Ok(line_str) = std::str::from_utf8(&buffer) {
                let line = line_str.trim();
                if !line.is_empty() {
                    if line.starts_with("data: ") {
                        let json_part = line.trim_start_matches("data: ").trim();
                        if json_part == "[DONE]" {
                            let is_empty_thought_stop_collapse = is_empty_stop(
                                saw_content, saw_tool_call, finish_reason_val.as_deref(),
                            );
                            if !is_empty_thought_stop_collapse {
                                for pending in pending_empty_frames.drain(..) {
                                    yield Ok(pending);
                                }
                                yield Ok(Bytes::from("data: [DONE]\n\n"));
                            }
                        } else {
                            for chunk in inspect_and_buffer_sse_json(
                                json_part,
                                &mut saw_thought,
                                &mut saw_content,
                                &mut saw_tool_call,
                                &mut saw_finish_reason,
                                &mut finish_reason_val,
                                &mut last_usage_val,
                                &mut pending_empty_frames,
                            ) {
                                yield Ok(chunk);
                            }
                        }
                    } else {
                        yield Ok(Bytes::from(format!("{}\n\n", line)));
                    }
                }
            }
        }

        if !saw_thought && is_empty_stop(saw_content, saw_tool_call, finish_reason_val.as_deref()) {
            yield Err(EMPTY_STOP_ERROR.to_string());
            return;
        }

        // 1. 早退空流 / signature-only EOF 自愈 (Fixes #4055)
        // 触发条件：上游在未产出任何思考、正文、工具调用或 finishReason 的情况下直接 EOF
        let is_premature_empty_eof =
            !saw_thought && !saw_content && !saw_tool_call && !saw_finish_reason;

        if is_premature_empty_eof {
            // 丢弃孤立的 signature-only 空壳帧，防止污染下游或缓存
            pending_empty_frames.clear();

            let has_budget = crate::proxy::pipeline::budget::scope_optional(
                budget.clone(),
                async {
                    crate::proxy::pipeline::budget::remaining_secs()
                        .map_or(true, |r| r >= MIN_PREMATURE_EMPTY_RETRY_BUDGET_SECS)
                },
            )
            .await;

            if has_budget {
                let mut current_body = original_body.clone();
                for attempt in 1..=MAX_PREMATURE_EMPTY_RETRIES {
                    let (next_body, degraded) =
                        build_premature_empty_retry_body(&current_body, attempt);
                    if attempt > 1 && !degraded {
                        break;
                    }

                    if attempt > 1 {
                        let still_has_budget = crate::proxy::pipeline::budget::scope_optional(
                            budget.clone(),
                            async {
                                crate::proxy::pipeline::budget::remaining_secs()
                                    .map_or(true, |r| r >= MIN_PREMATURE_EMPTY_RETRY_BUDGET_SECS)
                            },
                        )
                        .await;
                        if !still_has_budget {
                            break;
                        }
                    }

                    tracing::warn!(
                        "[{}] [Stream-AutoHeal] 🚨 Premature empty/signature-only EOF before content/finishReason. Triggering progressive retry {}/{} (model={}, degraded={})...",
                        trace_id,
                        attempt,
                        MAX_PREMATURE_EMPTY_RETRIES,
                        extract_model_for_log(&next_body),
                        degraded
                    );
                    yield Ok(Bytes::from(": auto-healing premature empty stream\n\n"));

                    match retry_call(next_body.clone()).await {
                        Ok(mut retry_stream) => {
                            let mut r_thought = false;
                            let mut r_content = false;
                            let mut r_tool = false;
                            let mut r_finish = false;
                            let mut r_finish_val: Option<String> = None;
                            let mut r_usage: Option<Value> = None;
                            let mut r_pending: Vec<Bytes> = Vec::new();
                            let mut r_buffer = BytesMut::new();
                            let mut r_err = false;

                            while let Some(chunk_res) = retry_stream.next().await {
                                match chunk_res {
                                    Ok(bytes) => {
                                        r_buffer.extend_from_slice(&bytes);
                                        while let Some(pos) =
                                            r_buffer.iter().position(|&b| b == b'\n')
                                        {
                                            let line_raw = r_buffer.split_to(pos + 1);
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
                                                let json_part =
                                                    line.trim_start_matches("data: ").trim();
                                                if json_part == "[DONE]" {
                                                    if (r_thought || r_content || r_tool || r_finish)
                                                        && !is_empty_stop(r_content, r_tool, r_finish_val.as_deref())
                                                    {
                                                        for p in r_pending.drain(..) {
                                                            yield Ok(p);
                                                        }
                                                        yield Ok(Bytes::from("data: [DONE]\n\n"));
                                                    }
                                                    continue;
                                                }
                                                for out_chunk in inspect_and_buffer_sse_json(
                                                    json_part,
                                                    &mut r_thought,
                                                    &mut r_content,
                                                    &mut r_tool,
                                                    &mut r_finish,
                                                    &mut r_finish_val,
                                                    &mut r_usage,
                                                    &mut r_pending,
                                                ) {
                                                    yield Ok(out_chunk);
                                                }
                                            } else {
                                                yield Ok(Bytes::from(format!("{}\n\n", line)));
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        tracing::warn!(
                                            "[{}] [Stream-AutoHeal] Premature empty retry {}/{} read error: {:?}",
                                            trace_id,
                                            attempt,
                                            MAX_PREMATURE_EMPTY_RETRIES,
                                            e
                                        );
                                        r_err = true;
                                        break;
                                    }
                                }
                            }

                            if !r_err && !r_buffer.is_empty() {
                                if let Ok(line_str) = std::str::from_utf8(&r_buffer) {
                                    let line = line_str.trim();
                                    if !line.is_empty() {
                                        if line.starts_with("data: ") {
                                            let json_part =
                                                line.trim_start_matches("data: ").trim();
                                            if json_part == "[DONE]" {
                                                if (r_thought || r_content || r_tool || r_finish)
                                                    && !is_empty_stop(r_content, r_tool, r_finish_val.as_deref()) {
                                                    for p in r_pending.drain(..) {
                                                        yield Ok(p);
                                                    }
                                                    yield Ok(Bytes::from("data: [DONE]\n\n"));
                                                }
                                            } else {
                                                for out_chunk in inspect_and_buffer_sse_json(
                                                    json_part,
                                                    &mut r_thought,
                                                    &mut r_content,
                                                    &mut r_tool,
                                                    &mut r_finish,
                                                    &mut r_finish_val,
                                                    &mut r_usage,
                                                    &mut r_pending,
                                                ) {
                                                    yield Ok(out_chunk);
                                                }
                                            }
                                        } else {
                                            yield Ok(Bytes::from(format!("{}\n\n", line)));
                                        }
                                    }
                                }
                            }

                            if is_empty_stop(r_content, r_tool, r_finish_val.as_deref()) {
                                yield Err(EMPTY_STOP_ERROR.to_string());
                                return;
                            }
                            if r_thought || r_content || r_tool || r_finish {
                                tracing::info!(
                                    "[{}] [Stream-AutoHeal] ✓ Premature empty EOF healed on retry {}/{}",
                                    trace_id,
                                    attempt,
                                    MAX_PREMATURE_EMPTY_RETRIES
                                );
                                return;
                            }
                            if r_err {
                                break;
                            }
                            r_pending.clear();
                            current_body = next_body;
                        }
                        Err(err) => {
                            tracing::warn!(
                                "[{}] [Stream-AutoHeal] Premature empty retry {}/{} failed: {}",
                                trace_id,
                                attempt,
                                MAX_PREMATURE_EMPTY_RETRIES,
                                err
                            );
                            break;
                        }
                    }
                }
            }
            return;
        }

        // 2. 纯思考空回复续跑自愈判断条件：
        // 1. 存在思考块 (saw_thought)
        // 2. 无任何正文 (saw_content == false)
        // 3. 无任何工具调用 (saw_tool_call == false)
        // 4. 上游返回了正常结束符 STOP (finish_reason == Some("STOP"))
        // 注意：若因为 token 耗尽等导致截断（如 MAX_TOKENS、SAFETY），属于正常语义截断，严禁拦截自愈
        let is_empty_thought_stop_collapse = saw_thought
            && !saw_content
            && !saw_tool_call
            && saw_finish_reason
            && finish_reason_val.as_deref() == Some("STOP");

        if is_empty_thought_stop_collapse {
            tracing::warn!(
                "[{}] [Stream-AutoHeal] 🚨 Detected empty thinking completion (thought present, 0 content, 0 tool_calls, finishReason=STOP). Triggering auto-heal continuation 1/1...",
                trace_id
            );

            // 发射 SSE 心跳注释保持客户端下游连接存活
            yield Ok(Bytes::from(": auto-healing empty thinking\n\n"));

            let prompt = resolve_continuation_prompt(&original_body);
            let heal_body = create_auto_heal_continuation_body(&original_body, prompt);

            match retry_call(heal_body).await {
                Ok(mut stream2) => {
                    tracing::info!(
                        "[{}] [Stream-AutoHeal] ✓ Continuation request succeeded (HTTP 200), piping healed stream directly into client connection...",
                        trace_id
                    );
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
                                                if let Some(usage) = inner.get("usageMetadata").or_else(|| inner.get("usage")) {
                                                    last_usage_val = Some(usage.clone());
                                                }
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
                                tracing::warn!("[{}] [Stream-AutoHeal] Stream 2 read error: {:?}", trace_id, e);
                                break;
                            }
                        }
                    }

                    // 若续跑流依然未产出任何可视正文或工具调用，遵循零正文篡改原则输出标准 STOP 与 [DONE]
                    if !stream2_saw_output {
                        tracing::warn!(
                            "[{}] [Stream-AutoHeal] Continuation stream also returned 0 output. Emitting clean STOP and [DONE] without fake content.",
                            trace_id
                        );
                        let (stop_chunk, done_chunk) =
                            make_clean_empty_thought_stop_chunks(last_usage_val.as_ref());
                        yield Ok(stop_chunk);
                        yield Ok(done_chunk);
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "[{}] [Stream-AutoHeal] Continuation request failed: {}. Emitting clean STOP and [DONE] without fake content.",
                        trace_id, e
                    );
                    let (stop_chunk, done_chunk) =
                        make_clean_empty_thought_stop_chunks(last_usage_val.as_ref());
                    yield Ok(stop_chunk);
                    yield Ok(done_chunk);
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
    fn test_make_clean_empty_thought_stop_chunks_with_usage() {
        let usage = json!({
            "promptTokenCount": 25,
            "candidatesTokenCount": 512,
            "totalTokenCount": 537
        });
        let (stop_chunk, done_chunk) = make_clean_empty_thought_stop_chunks(Some(&usage));
        let stop_str = std::str::from_utf8(&stop_chunk).unwrap();
        assert!(stop_str.starts_with("data: "));
        assert!(stop_str.ends_with("\n\n"));
        let json_str = stop_str.trim_start_matches("data: ").trim();
        let parsed: Value = serde_json::from_str(json_str).unwrap();

        // 关键断言：1. 严禁包含 parts 或任何伪造的 "task ready" 正文
        assert!(parsed["candidates"][0].get("content").is_none());
        assert!(!stop_str.contains("task ready"));

        // 2. 正式输出 finishReason: "STOP"
        assert_eq!(parsed["candidates"][0]["finishReason"], "STOP");

        // 3. 完整保全思考产生的使用量元数据
        assert_eq!(parsed["usageMetadata"]["candidatesTokenCount"], 512);

        // 4. 正式输出 [DONE] 终结信号
        assert_eq!(done_chunk.as_ref(), b"data: [DONE]\n\n");
    }

    #[test]
    fn test_make_clean_empty_thought_stop_chunks_without_usage() {
        let (stop_chunk, done_chunk) = make_clean_empty_thought_stop_chunks(None);
        let stop_str = std::str::from_utf8(&stop_chunk).unwrap();
        let json_str = stop_str.trim_start_matches("data: ").trim();
        let parsed: Value = serde_json::from_str(json_str).unwrap();

        assert!(parsed["candidates"][0].get("content").is_none());
        assert!(!stop_str.contains("task ready"));
        assert_eq!(parsed["candidates"][0]["finishReason"], "STOP");
        assert!(parsed.get("usageMetadata").is_none());
        assert_eq!(done_chunk.as_ref(), b"data: [DONE]\n\n");
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
    fn test_auto_heal_clamps_unsupported_thinking_level_once() {
        let err = r#"{"error":{"code":400,"message":"Thinking level is unsupported: THINKING_LEVEL_XHIGH","status":"INVALID_ARGUMENT"}}"#;
        let classification =
            crate::proxy::pipeline::UpstreamClassification::classify(400, err, None);
        let mut body = json!({
            "model": "gemini-4-hydrogen-xhigh",
            "generationConfig": {
                "thinkingConfig": { "includeThoughts": true, "thinkingLevel": 100 }
            }
        });
        let mut degraded = None;
        let mut flags = AutoHealFlags::default();
        let out = try_auto_heal(
            &classification,
            400,
            err,
            &mut body,
            &mut degraded,
            &mut flags,
        );
        assert_eq!(
            out.map(|o| o.kind),
            Some("clamp_unsupported_thinking_level")
        );
        assert!(flags.retried_with_thinking_level_clamped);
        let retried = degraded.as_ref().expect("same-account retry body");
        assert_eq!(retried["model"], "gemini-4-hydrogen-high");
        assert_eq!(
            retried["generationConfig"]["thinkingConfig"]["thinkingLevel"],
            "HIGH"
        );

        // 单次上限：同一请求第二次 400 交回原错误策略
        let again = try_auto_heal(
            &classification,
            400,
            err,
            &mut body,
            &mut degraded,
            &mut flags,
        );
        assert!(again.is_none());

        // 已是 HIGH 的请求收到同样的 400：没有可钳制的档位，不重试
        let mut high = json!({
            "model": "gemini-4-hydrogen-high",
            "generationConfig": { "thinkingConfig": { "thinkingLevel": "HIGH" } }
        });
        let mut degraded_high = None;
        let mut fresh = AutoHealFlags::default();
        assert!(try_auto_heal(
            &classification,
            400,
            err,
            &mut high,
            &mut degraded_high,
            &mut fresh,
        )
        .is_none());
        assert!(degraded_high.is_none());
        assert!(!fresh.retried_with_thinking_level_clamped);
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

    #[test]
    fn test_auto_heal_capacity_exhausted_no_ping_pong_issue_4000() {
        // Case 1: 客户端请求本身就是 high 档位，遇到 503 MODEL_CAPACITY_EXHAUSTED 时，
        // 既不能被 promote（已是最高），也绝不能被判定为普通过载降级为 medium（避免拉长请求时间与篡改模型）。
        let classification = crate::proxy::pipeline::UpstreamClassification::classify(
            503,
            "The model is overloaded. Please try again later. MODEL_CAPACITY_EXHAUSTED",
            None,
        );
        let err_text = "The model is overloaded. Please try again later. MODEL_CAPACITY_EXHAUSTED";
        let mut high_body = json!({
            "model": "gemini-4-hydrogen-high",
            "generationConfig": {
                "thinkingConfig": {
                    "thinkingLevel": "HIGH"
                }
            }
        });
        let mut degraded = None;
        let mut flags = AutoHealFlags::default();

        let out = try_auto_heal(
            &classification,
            503,
            err_text,
            &mut high_body,
            &mut degraded,
            &mut flags,
        );
        assert!(
            out.is_none(),
            "Initial -high request on capacity exhaustion should not be degraded or ping-ponged"
        );
        assert!(!flags.retried_with_budget_degraded);
        assert!(!flags.retried_with_capacity_promoted);
        assert_eq!(high_body["model"], "gemini-4-hydrogen-high");

        // Case 2: 客户端请求为 medium 档位，遇到 503 MODEL_CAPACITY_EXHAUSTED 时，
        // 第一次成功升档为 high；当第二次仍然遇到 503 时，绝不能被降级回 medium。
        let mut medium_body = json!({
            "model": "gemini-4-hydrogen-medium",
            "generationConfig": {
                "thinkingConfig": {
                    "thinkingLevel": "MEDIUM"
                }
            }
        });
        let mut degraded2 = None;
        let mut flags2 = AutoHealFlags::default();

        let out2 = try_auto_heal(
            &classification,
            503,
            err_text,
            &mut medium_body,
            &mut degraded2,
            &mut flags2,
        );
        assert!(out2.is_some());
        assert_eq!(out2.unwrap().kind, "promote_tier_to_high_capacity");
        assert!(flags2.retried_with_capacity_promoted);
        assert_eq!(medium_body["model"], "gemini-4-hydrogen-high");

        // 第二次上游依旧 503 MODEL_CAPACITY_EXHAUSTED
        let mut degraded3 = None;
        let out3 = try_auto_heal(
            &classification,
            503,
            err_text,
            &mut medium_body,
            &mut degraded3,
            &mut flags2,
        );
        assert!(
            out3.is_none(),
            "Already promoted request must not fall back to degrade_thinking_budget"
        );
        assert!(!flags2.retried_with_budget_degraded);
        assert_eq!(medium_body["model"], "gemini-4-hydrogen-high");
    }

    #[test]
    fn test_auto_heal_capacity_exhausted_skips_lite_and_unsupported_models_issue_4006() {
        let classification = crate::proxy::pipeline::UpstreamClassification::classify(
            503,
            "The model is overloaded. Please try again later. MODEL_CAPACITY_EXHAUSTED",
            None,
        );
        let err_text = "The model is overloaded. Please try again later. MODEL_CAPACITY_EXHAUSTED";
        let mut lite_body = json!({
            "model": "gemini-3.5-flash-lite",
            "generationConfig": {
                "thinkingConfig": {
                    "thinkingLevel": "LOW"
                }
            }
        });
        let mut degraded = None;
        let mut flags = AutoHealFlags::default();

        let out = try_auto_heal(
            &classification,
            503,
            err_text,
            &mut lite_body,
            &mut degraded,
            &mut flags,
        );
        assert!(
            out.is_none(),
            "flash-lite must not be promoted to -high or degraded"
        );
        assert!(!flags.retried_with_capacity_promoted);
        assert!(!flags.retried_with_budget_degraded);
        assert_eq!(lite_body["model"], "gemini-3.5-flash-lite");
        assert_eq!(
            lite_body["generationConfig"]["thinkingConfig"]["thinkingLevel"],
            "LOW"
        );
    }

    #[test]
    fn test_auto_heal_demotes_response_schema_on_forced_tool_choice_400() {
        let err_text = r#"{"error":{"code":400,"message":"tool_choice: type \"tool\" and \"any\" are not supported for this model.","status":"INVALID_ARGUMENT"}}"#;
        let classification =
            crate::proxy::pipeline::UpstreamClassification::classify(400, err_text, None);

        // 1. Root payload with thinkingConfig + responseSchema + systemInstruction
        let mut root_body = json!({
            "systemInstruction": {
                "parts": [{ "text": "You are a helpful assistant." }]
            },
            "contents": [
                { "role": "user", "parts": [{ "text": "First question" }] },
                { "role": "model", "parts": [{ "text": "First answer" }] },
                { "role": "user", "parts": [{ "text": "Return JSON matching schema" }] }
            ],
            "generationConfig": {
                "thinkingConfig": { "includeThoughts": true, "thinkingBudget": 1024 },
                "responseMimeType": "application/json",
                "responseSchema": {
                    "type": "object",
                    "properties": { "ok": { "type": "boolean" } },
                    "required": ["ok"]
                }
            }
        });
        let mut degraded = None;
        let mut flags = AutoHealFlags::default();

        let out = try_auto_heal(
            &classification,
            400,
            err_text,
            &mut root_body,
            &mut degraded,
            &mut flags,
        );
        assert_eq!(
            out.map(|o| o.kind),
            Some("demote_response_schema_on_forced_tool_choice")
        );
        assert!(flags.retried_with_response_schema_demoted);
        assert!(!flags.retried_tool_choice_conflict);
        assert!(degraded.is_some());

        // thinkingConfig & systemInstruction must remain untouched; responseSchema/responseMimeType removed
        assert!(root_body["generationConfig"]
            .get("thinkingConfig")
            .is_some());
        assert!(root_body["generationConfig"]
            .get("responseSchema")
            .is_none());
        assert!(root_body["generationConfig"]
            .get("responseMimeType")
            .is_none());
        assert_eq!(
            root_body["systemInstruction"]["parts"][0]["text"],
            "You are a helpful assistant."
        );
        // Schema prompt must be appended to the LAST user turn, not the first
        assert_eq!(
            root_body["contents"][0]["parts"].as_array().unwrap().len(),
            1
        );
        let last_user_parts = root_body["contents"][2]["parts"].as_array().unwrap();
        assert_eq!(last_user_parts.len(), 2);
        let injected_text = last_user_parts[1]["text"].as_str().unwrap();
        assert!(injected_text.contains("[OUTPUT INSTRUCTION:"));
        assert!(injected_text.contains("\"ok\""));

        // Second attempt must be blocked by retried_with_response_schema_demoted
        let mut degraded_retry = None;
        assert!(try_auto_heal(
            &classification,
            400,
            err_text,
            &mut root_body,
            &mut degraded_retry,
            &mut flags,
        )
        .is_none());
        assert!(degraded_retry.is_none());

        // 2. Wrapped {"request": {...}} payload
        let mut wrapped_body = json!({
            "project": "p1",
            "request": {
                "contents": [
                    { "role": "model", "parts": [{ "text": "Only model turn" }] }
                ],
                "generationConfig": {
                    "thinkingConfig": { "thinkingLevel": "HIGH" },
                    "responseMimeType": "application/json",
                    "responseSchema": { "type": "object" }
                }
            }
        });
        let mut wrapped_degraded = None;
        let mut wrapped_flags = AutoHealFlags::default();
        let wrapped_out = try_auto_heal(
            &classification,
            400,
            err_text,
            &mut wrapped_body,
            &mut wrapped_degraded,
            &mut wrapped_flags,
        );
        assert_eq!(
            wrapped_out.map(|o| o.kind),
            Some("demote_response_schema_on_forced_tool_choice")
        );
        assert!(wrapped_body["request"]["generationConfig"]
            .get("thinkingConfig")
            .is_some());
        assert!(wrapped_body["request"]["generationConfig"]
            .get("responseSchema")
            .is_none());
        let wrapped_contents = wrapped_body["request"]["contents"].as_array().unwrap();
        assert_eq!(wrapped_contents.len(), 2);
        assert_eq!(wrapped_contents[1]["role"], "user");

        // 3. No responseSchema present -> skip futile retry
        let mut no_schema_body = json!({
            "contents": [{ "role": "user", "parts": [{ "text": "hi" }] }],
            "generationConfig": {
                "thinkingConfig": { "thinkingBudget": 1024 }
            }
        });
        let mut no_schema_degraded = None;
        let mut no_schema_flags = AutoHealFlags::default();
        let no_schema_out = try_auto_heal(
            &classification,
            400,
            err_text,
            &mut no_schema_body,
            &mut no_schema_degraded,
            &mut no_schema_flags,
        );
        assert!(no_schema_out.is_none());
        assert!(!no_schema_flags.retried_with_response_schema_demoted);
        assert!(no_schema_degraded.is_none());
        assert!(no_schema_body["generationConfig"]
            .get("thinkingConfig")
            .is_some());
    }

    #[tokio::test]
    async fn test_issue_4055_signature_only_empty_eof_auto_heals_with_degraded_thinking() {
        let signature_only_sse = "data: {\"response\":{\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"thoughtSignature\":\"EuIDCt8DAWkUfRNLU1\",\"text\":\"\"}]}}],\"usageMetadata\":{\"promptTokenCount\":124160,\"totalTokenCount\":124160},\"modelVersion\":\"gemini-4-hydrogen\",\"responseId\":\"LFgGa5Py\"}}\n\n";
        let healed_sse = "data: {\"response\":{\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"Healed answer\"}]},\"finishReason\":\"STOP\"}]}}\n\ndata: [DONE]\n\n";

        let stream1 = Box::pin(async_stream::stream! {
            yield Ok::<Bytes, String>(Bytes::from(signature_only_sse));
        });

        let original_body = json!({
            "requestId": "req_4055_1",
            "model": "gemini-4-hydrogen-high",
            "labels": { "model_enum": "MODEL_PLACEHOLDER_M525" },
            "request": {
                "contents": [{ "role": "user", "parts": [{ "text": "hello" }] }],
                "generationConfig": {
                    "thinkingConfig": {
                        "includeThoughts": true,
                        "thinkingLevel": "HIGH"
                    }
                }
            }
        });

        let recorded_bodies = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
        let recorded_clone = recorded_bodies.clone();

        let mut wrapped = wrap_stream_with_auto_heal_inner(
            stream1,
            original_body,
            "trace_4055_1".to_string(),
            None,
            move |req_body| {
                recorded_clone.lock().unwrap().push(req_body);
                async move {
                    let retry_stream: Pin<Box<dyn Stream<Item = Result<Bytes, String>> + Send>> =
                        Box::pin(async_stream::stream! {
                            yield Ok::<Bytes, String>(Bytes::from(healed_sse));
                        });
                    Ok(retry_stream)
                }
            },
        );

        let mut chunks = Vec::new();
        while let Some(item) = wrapped.next().await {
            chunks.push(String::from_utf8_lossy(&item.unwrap()).to_string());
        }
        let output = chunks.join("");

        let captured = recorded_bodies.lock().unwrap();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0]["model"], "gemini-4-hydrogen-medium");
        assert_eq!(
            captured[0]["labels"]["model_enum"],
            "MODEL_PLACEHOLDER_M526"
        );
        assert_eq!(
            captured[0]["request"]["generationConfig"]["thinkingConfig"]["thinkingLevel"],
            "MEDIUM"
        );
        assert_eq!(captured[0]["requestId"], "req_4055_1_empty_heal1");

        assert!(
            !output.contains("EuIDCt8DAWkUfRNLU1"),
            "Orphan signature-only frame from aborted stream1 must not be leaked"
        );
        assert!(output.contains(": auto-healing premature empty stream"));
        assert!(output.contains("Healed answer"));
        assert!(output.contains("data: [DONE]"));
    }

    #[tokio::test]
    async fn test_issue_4055_progressive_degrade_high_to_medium_to_low() {
        let signature_only_sse = "data: {\"response\":{\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"thoughtSignature\":\"EuIDEmpty\",\"text\":\"\"}]}}]}}\n\n";
        let healed_tool_sse = "data: {\"response\":{\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"functionCall\":{\"name\":\"bash\",\"args\":{\"cmd\":\"ls\"}}}]},\"finishReason\":\"STOP\"}]}}\n\ndata: [DONE]\n\n";

        let stream1 = Box::pin(async_stream::stream! {
            yield Ok::<Bytes, String>(Bytes::from(signature_only_sse));
        });

        let original_body = json!({
            "requestId": "req_4055_2",
            "model": "gemini-4-hydrogen-high",
            "labels": { "model_enum": "MODEL_PLACEHOLDER_M525" },
            "request": {
                "contents": [{ "role": "user", "parts": [{ "text": "run tool" }] }],
                "generationConfig": {
                    "thinkingConfig": {
                        "includeThoughts": true,
                        "thinkingLevel": "HIGH"
                    }
                }
            }
        });

        let recorded_bodies = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
        let recorded_clone = recorded_bodies.clone();
        let attempt_counter = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let attempt_clone = attempt_counter.clone();

        let mut wrapped = wrap_stream_with_auto_heal_inner(
            stream1,
            original_body,
            "trace_4055_2".to_string(),
            None,
            move |req_body| {
                recorded_clone.lock().unwrap().push(req_body);
                let idx = attempt_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                async move {
                    let payload = if idx == 0 {
                        signature_only_sse
                    } else {
                        healed_tool_sse
                    };
                    let retry_stream: Pin<Box<dyn Stream<Item = Result<Bytes, String>> + Send>> =
                        Box::pin(async_stream::stream! {
                            yield Ok::<Bytes, String>(Bytes::from(payload));
                        });
                    Ok(retry_stream)
                }
            },
        );

        let mut chunks = Vec::new();
        while let Some(item) = wrapped.next().await {
            chunks.push(String::from_utf8_lossy(&item.unwrap()).to_string());
        }
        let output = chunks.join("");

        let captured = recorded_bodies.lock().unwrap();
        assert_eq!(captured.len(), 2);
        // Attempt 1: MEDIUM
        assert_eq!(captured[0]["model"], "gemini-4-hydrogen-medium");
        assert_eq!(
            captured[0]["labels"]["model_enum"],
            "MODEL_PLACEHOLDER_M526"
        );
        assert_eq!(
            captured[0]["request"]["generationConfig"]["thinkingConfig"]["thinkingLevel"],
            "MEDIUM"
        );
        assert_eq!(captured[0]["requestId"], "req_4055_2_empty_heal1");
        // Attempt 2: LOW
        assert_eq!(captured[1]["model"], "gemini-4-hydrogen-low");
        assert_eq!(
            captured[1]["labels"]["model_enum"],
            "MODEL_PLACEHOLDER_M527"
        );
        assert_eq!(
            captured[1]["request"]["generationConfig"]["thinkingConfig"]["thinkingLevel"],
            "LOW"
        );
        assert_eq!(captured[1]["requestId"], "req_4055_2_empty_heal2");

        assert!(!output.contains("EuIDEmpty"));
        assert!(output.contains("\"functionCall\""));
        assert!(output.contains("data: [DONE]"));
    }

    #[tokio::test]
    async fn test_issue_4055_leading_empty_signature_frame_flushed_when_real_content_follows() {
        let frame1 = "data: {\"response\":{\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"thoughtSignature\":\"SigRealFirst\",\"text\":\"\"}]}}]}}\n\n";
        let frame2 = "data: {\"response\":{\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"Real text\"}]},\"finishReason\":\"STOP\"}]}}\n\ndata: [DONE]\n\n";

        let stream1 = Box::pin(async_stream::stream! {
            yield Ok::<Bytes, String>(Bytes::from(frame1));
            yield Ok::<Bytes, String>(Bytes::from(frame2));
        });

        let retry_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let retry_clone = retry_count.clone();

        let mut wrapped = wrap_stream_with_auto_heal_inner(
            stream1,
            json!({ "model": "gemini-4-hydrogen-high" }),
            "trace_4055_3".to_string(),
            None,
            move |_req_body| {
                retry_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                async move {
                    let retry_stream: Pin<Box<dyn Stream<Item = Result<Bytes, String>> + Send>> =
                        Box::pin(async_stream::stream! {
                            if false {
                                yield Ok::<Bytes, String>(Bytes::new());
                            }
                        });
                    Ok(retry_stream)
                }
            },
        );

        let mut chunks = Vec::new();
        while let Some(item) = wrapped.next().await {
            chunks.push(String::from_utf8_lossy(&item.unwrap()).to_string());
        }
        let output = chunks.join("");

        assert_eq!(retry_count.load(std::sync::atomic::Ordering::SeqCst), 0);
        let sig_pos = output.find("SigRealFirst").expect("leading frame flushed");
        let text_pos = output.find("Real text").expect("content frame present");
        assert!(sig_pos < text_pos);
    }

    #[tokio::test]
    async fn test_empty_stop_is_not_success_in_any_streaming_format() {
        use crate::proxy::mappers::{claude, gemini, openai::streaming as openai};

        let upstream = || {
            wrap_stream_with_auto_heal_inner(
                Box::pin(futures::stream::iter([Ok::<Bytes, String>(Bytes::from_static(
                    b"data: {\"response\":{\"candidates\":[{\"finishReason\":\"STOP\"}]}}\n\ndata: [DONE]\n\n",
                ))])),
                json!({}),
                "empty-stop-test".into(),
                None,
                |_| async { panic!("bare STOP should not replay the request"); #[allow(unreachable_code)] Ok(Box::pin(futures::stream::empty::<Result<Bytes, String>>())) },
            )
        };
        let model = || "gemini-test".to_string();
        let session = || "empty-stop-test".to_string();
        let formats = [
            (
                "messages",
                claude::create_claude_sse_stream(
                    upstream(),
                    session(),
                    String::new(),
                    None,
                    false,
                    1_000,
                    None,
                    1,
                    None,
                    vec![],
                ),
                "message_stop",
            ),
            (
                "chat",
                openai::create_openai_sse_stream(upstream(), model(), session(), 1, None, false),
                "\"finish_reason\":\"stop\"",
            ),
            (
                "responses",
                openai::create_codex_sse_stream(
                    upstream(),
                    model(),
                    session(),
                    1,
                    0,
                    "resp_test".into(),
                    None,
                    false,
                ),
                "response.completed",
            ),
            (
                "gemini",
                crate::proxy::pipeline::run_stream_lifecycle(
                    upstream(),
                    gemini::streaming::GeminiStreamHandler::new(
                        None,
                        session(),
                        model(),
                        1,
                        session(),
                        false,
                        None,
                        String::new(),
                        model(),
                    ),
                    crate::proxy::pipeline::StreamLifecycleConfig::new("gemini", "test", session()),
                ),
                "\"finishReason\":\"STOP\"",
            ),
        ];
        for (format, mut stream, success_marker) in formats {
            let mut frames = String::new();
            let mut saw_error = false;
            while let Some(frame) = stream.next().await {
                match frame {
                    Ok(bytes) => frames.push_str(&String::from_utf8_lossy(&bytes)),
                    Err(_) => saw_error = true,
                }
            }
            assert!(
                saw_error || frames.contains("\"error\""),
                "{format}: {frames}"
            );
            assert!(!frames.contains(success_marker), "{format}: {frames}");
        }
    }

    #[tokio::test]
    async fn test_issue_4055_exhausted_premature_empty_eof_emits_openai_error_via_lifecycle() {
        let signature_only_sse = "data: {\"response\":{\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"thoughtSignature\":\"EuIDFail\",\"text\":\"\"}]}}]}}\n\n";

        let stream1 = Box::pin(async_stream::stream! {
            yield Ok::<Bytes, String>(Bytes::from(signature_only_sse));
        });

        let auto_healed = wrap_stream_with_auto_heal_inner(
            stream1,
            json!({
                "requestId": "req_4055_fail",
                "model": "gemini-4-hydrogen-high",
                "request": {
                    "contents": [{ "role": "user", "parts": [{ "text": "hello" }] }],
                    "generationConfig": { "thinkingConfig": { "thinkingLevel": "HIGH" } }
                }
            }),
            "trace_4055_4".to_string(),
            None,
            move |_req_body| async move {
                let empty_retry: Pin<Box<dyn Stream<Item = Result<Bytes, String>> + Send>> =
                    Box::pin(async_stream::stream! {
                        yield Ok::<Bytes, String>(Bytes::from(signature_only_sse));
                    });
                Ok(empty_retry)
            },
        );

        let mut openai_stream =
            crate::proxy::mappers::openai::streaming::create_openai_sse_stream_with_anchor(
                auto_healed,
                "gemini-4-hydrogen-high".to_string(),
                "sess_4055".to_string(),
                1,
                None,
                false,
                None,
                false,
            );

        let mut ok_output = Vec::new();
        let mut saw_err = false;
        while let Some(item) = openai_stream.next().await {
            match item {
                Ok(b) => ok_output.push(String::from_utf8_lossy(&b).to_string()),
                Err(_) => saw_err = true,
            }
        }
        let full_output = ok_output.join("");
        assert!(
            full_output.contains("\"error\""),
            "Exhausted premature empty EOF must emit an OpenAI error SSE frame instead of silent 200 empty body; got: {}",
            full_output
        );
        assert!(
            full_output.contains("\"choices\":[]") && !full_output.contains("\"delta\""),
            "Must NOT fabricate assistant choices/content on exhausted premature empty EOF; got: {}",
            full_output
        );
        assert!(
            !full_output.contains("EuIDFail"),
            "Orphan signature-only frame must not be leaked on exhausted failure"
        );
        assert!(
            saw_err,
            "Must still propagate Err when retries are exhausted"
        );
    }
}
