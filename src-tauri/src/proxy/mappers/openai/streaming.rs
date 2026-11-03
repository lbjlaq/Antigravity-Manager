// OpenAI 流式转换
use bytes::Bytes;
use chrono::Utc;
use futures::{Stream, StreamExt};
use rand::Rng;
use serde_json::{json, Value};
use std::pin::Pin;
use uuid::Uuid;

use crate::proxy::mappers::error_classifier::{
    preview_payload, report_stream_error, StreamErrorReport,
};
use crate::proxy::pipeline::{run_stream_lifecycle, ProtocolStreamHandler, StreamLifecycleConfig};

/// 保存 thoughtSignature 到会话缓存
pub fn store_thought_signature(sig: &str, session_id: &str, message_count: usize) {
    if sig.is_empty() {
        return;
    }

    // 2. [CRITICAL] 存储到 Session 隔离缓存 (对齐 Claude 协议)
    crate::proxy::SignatureCache::global().cache_session_signature(
        session_id,
        sig.to_string(),
        message_count,
    );

    tracing::debug!(
        "[ThoughtSig] 存储 Session 签名 (sid: {}, len: {}, msg_count: {})",
        session_id,
        sig.len(),
        message_count
    );
}

/// Extract and convert Gemini usageMetadata to OpenAI usage format
/// Supports both legacy v1internal format and new Interactions API format.
///
/// Key semantic difference:
/// - Old format: candidatesTokenCount = all output tokens (text + thinking + tool)
/// - New format: total_output_tokens = text + tool output only; thought tokens are separate (total_thought_tokens)
///
/// For Codex, we must sum them back together as `completion_tokens`.
fn extract_usage_metadata(u: &Value) -> Option<super::models::OpenAIUsage> {
    let canonical = crate::proxy::pipeline::CanonicalUsage::from_gemini(u);
    let mut usage = super::models::OpenAIUsage::from(&canonical);
    usage.input_tokens_by_modality = u.get("input_tokens_by_modality").cloned();
    usage.total_tool_use_tokens = u
        .get("total_tool_use_tokens")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);
    Some(usage)
}

#[allow(dead_code)]
pub fn create_openai_sse_stream<S, E>(
    gemini_stream: Pin<Box<S>>,
    model: String,
    session_id: String,
    message_count: usize,
    client_tool_names: Option<std::collections::HashSet<String>>,
    include_usage: bool,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, String>> + Send>>
where
    S: Stream<Item = Result<Bytes, E>> + Send + ?Sized + 'static,
    E: std::fmt::Display + Send + 'static,
{
    create_openai_sse_stream_with_anchor(
        gemini_stream,
        model,
        session_id,
        message_count,
        client_tool_names,
        include_usage,
        None,
    )
}

/// OpenAI Chat 协议流渲染器（实现 ProtocolStreamHandler Trait）
pub struct OpenAIChatStreamHandler {
    pub stream_id: String,
    pub created_ts: i64,
    pub model: String,
    pub session_id: String,
    pub message_count: usize,
    pub client_tool_names: std::collections::HashSet<String>,
    pub include_usage: bool,
    pub emitted_tool_calls: std::collections::HashSet<String>,
    pub final_usage: Option<super::models::OpenAIUsage>,
    pub tool_call_index: usize,
    pub thinking_acc: crate::proxy::thinking_store::TurnAccumulator,
    pub has_content: bool,
    pub has_thinking: bool,
    pub is_thinking_active: bool,
    pub has_emitted_finish_reason: bool,
    pub has_emitted_done: bool,
}

impl ProtocolStreamHandler for OpenAIChatStreamHandler {
    fn process_line(&mut self, line: &str) -> Vec<Bytes> {
        if !line.starts_with("data: ") {
            return Vec::new();
        }
        let json_part = line.trim_start_matches("data: ").trim();
        if json_part.is_empty() || json_part == "[DONE]" {
            return Vec::new();
        }

        let mut chunks = Vec::new();
        if let Ok(mut json) = serde_json::from_str::<Value>(json_part) {
            let actual_data = if let Some(inner) = json.get_mut("response").map(|v| v.take()) {
                inner
            } else {
                json
            };
            if let Some(u) = actual_data.get("usageMetadata") {
                self.final_usage = extract_usage_metadata(u);
            }

            if let Some(candidates) = actual_data.get("candidates").and_then(|c| c.as_array()) {
                if !candidates.is_empty() {
                    tracing::debug!("[Stream-Debug] Raw Candidate: {:?}", candidates[0]);
                }
                for (idx, candidate) in candidates.iter().enumerate() {
                    let parts = candidate
                        .get("content")
                        .and_then(|c| c.get("parts"))
                        .and_then(|p| p.as_array());
                    let mut content_out = String::new();
                    let mut thought_out = String::new();

                    if let Some(parts_list) = parts {
                        for part in parts_list {
                            self.thinking_acc.ingest_part(part);
                            let is_thought_part = part
                                .get("thought")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);
                            if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                                if is_thought_part {
                                    let clean_text = text
                                        .replace("<think>\n", "")
                                        .replace("<think>", "")
                                        .replace("\n</think>", "")
                                        .replace("</think>", "");
                                    thought_out.push_str(&clean_text);
                                } else {
                                    content_out.push_str(text);
                                }
                            }
                            if let Some(sig) = part
                                .get("thoughtSignature")
                                .or(part.get("thought_signature"))
                                .and_then(|s| s.as_str())
                            {
                                store_thought_signature(sig, &self.session_id, self.message_count);
                            }
                            if let Some(img) = part.get("inlineData") {
                                let mime_type = img
                                    .get("mimeType")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("image/png");
                                let data = img.get("data").and_then(|v| v.as_str()).unwrap_or("");
                                if !data.is_empty() {
                                    content_out.push_str(&format!(
                                        "![image](data:{};base64,{})",
                                        mime_type, data
                                    ));
                                }
                            }
                            if let Some(func_call) = part.get("functionCall") {
                                let call_key = serde_json::to_string(func_call).unwrap_or_default();
                                if !self.emitted_tool_calls.contains(&call_key) {
                                    self.emitted_tool_calls.insert(call_key);
                                    let name = func_call
                                        .get("name")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("unknown");
                                    let mut args =
                                        func_call.get("args").unwrap_or(&json!({})).clone();

                                    super::response::normalize_and_sanitize_tool_args(
                                        name, &mut args,
                                    );

                                    let final_name = super::response::resolve_shell_tool_name(
                                        name,
                                        &self.client_tool_names,
                                    );

                                    let call_id = func_call
                                        .get("id")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string())
                                        .unwrap_or_else(|| {
                                            let mut hasher =
                                                std::collections::hash_map::DefaultHasher::new();
                                            use std::hash::{Hash, Hasher};
                                            serde_json::to_string(func_call)
                                                .unwrap_or_default()
                                                .hash(&mut hasher);
                                            self.tool_call_index.hash(&mut hasher);
                                            std::time::SystemTime::now()
                                                .duration_since(std::time::UNIX_EPOCH)
                                                .unwrap_or_default()
                                                .as_nanos()
                                                .hash(&mut hasher);
                                            format!("call_{:x}", hasher.finish())
                                        });

                                    if let Some(sig) = part
                                        .get("thoughtSignature")
                                        .or(part.get("thought_signature"))
                                        .and_then(|s| s.as_str())
                                    {
                                        crate::proxy::SignatureCache::global()
                                            .cache_tool_signature(
                                                &self.session_id,
                                                &call_id,
                                                sig.to_string(),
                                            );
                                    }
                                    self.thinking_acc.record_tool_id(name, &call_id);

                                    let args_str = serde_json::to_string(&args).unwrap_or_default();
                                    let tool_call_chunk = json!({
                                        "id": &self.stream_id,
                                        "object": "chat.completion.chunk",
                                        "created": self.created_ts,
                                        "model": &self.model,
                                        "choices": [{
                                            "index": idx as u32,
                                            "delta": {
                                                "role": "assistant",
                                                "tool_calls": [{
                                                    "index": self.tool_call_index,
                                                    "id": call_id,
                                                    "type": "function",
                                                    "function": { "name": final_name, "arguments": args_str }
                                                }]
                                            },
                                            "finish_reason": serde_json::Value::Null
                                        }]
                                    });

                                    self.tool_call_index += 1;
                                    self.has_content = true;
                                    self.is_thinking_active = false;
                                    let sse_out = format!(
                                        "data: {}\n\n",
                                        serde_json::to_string(&tool_call_chunk).unwrap_or_default()
                                    );
                                    chunks.push(Bytes::from(sse_out));
                                }
                            }
                        }
                    }

                    if let Some(grounding) = candidate.get("groundingMetadata") {
                        let mut grounding_text = String::new();
                        if let Some(queries) =
                            grounding.get("webSearchQueries").and_then(|q| q.as_array())
                        {
                            let query_list: Vec<&str> =
                                queries.iter().filter_map(|v| v.as_str()).collect();
                            if !query_list.is_empty() {
                                grounding_text.push_str("\n\n---\n**🔍 已为您搜索：** ");
                                grounding_text.push_str(&query_list.join(", "));
                            }
                        }
                        if let Some(g_chunks) =
                            grounding.get("groundingChunks").and_then(|c| c.as_array())
                        {
                            let mut links = Vec::new();
                            for (i, chunk) in g_chunks.iter().enumerate() {
                                if let Some(web) = chunk.get("web") {
                                    let title = web
                                        .get("title")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("网页来源");
                                    let uri =
                                        web.get("uri").and_then(|v| v.as_str()).unwrap_or("#");
                                    links.push(format!("[{}] [{}]({})", i + 1, title, uri));
                                }
                            }
                            if !links.is_empty() {
                                grounding_text.push_str("\n\n**🌐 来源引文：**\n");
                                grounding_text.push_str(&links.join("\n"));
                            }
                        }
                        if !grounding_text.is_empty() {
                            content_out.push_str(&grounding_text);
                        }
                    }

                    let raw_finish_reason = candidate
                        .get("finishReason")
                        .or_else(|| candidate.get("finish_reason"))
                        .and_then(|f| f.as_str());

                    let gemini_finish_reason = raw_finish_reason.map(|f| match f {
                        "STOP" => "stop",
                        "MAX_TOKENS" => "length",
                        "SAFETY" => "content_filter",
                        "RECITATION" => "content_filter",
                        "MALFORMED_FUNCTION_CALL" => "stop",
                        _ => "stop",
                    });

                    let finish_reason =
                        if !self.emitted_tool_calls.is_empty() && gemini_finish_reason.is_some() {
                            Some("tool_calls")
                        } else {
                            gemini_finish_reason
                        };

                    if !thought_out.is_empty() {
                        self.has_thinking = true;
                        self.is_thinking_active = true;
                        let reasoning_chunk = json!({
                            "id": &self.stream_id,
                            "object": "chat.completion.chunk",
                            "created": self.created_ts,
                            "model": &self.model,
                            "choices": [{
                                "index": idx as u32,
                                "delta": { "role": "assistant", "content": serde_json::Value::Null, "reasoning_content": thought_out },
                                "finish_reason": serde_json::Value::Null
                            }]
                        });
                        let sse_out = format!(
                            "data: {}\n\n",
                            serde_json::to_string(&reasoning_chunk).unwrap_or_default()
                        );
                        chunks.push(Bytes::from(sse_out));
                    }

                    if !content_out.is_empty() || finish_reason.is_some() {
                        self.is_thinking_active = false;
                        if !content_out.is_empty() {
                            self.has_content = true;
                        }
                        if finish_reason.is_some() {
                            self.has_emitted_finish_reason = true;
                        }
                        let delta = if !content_out.is_empty() {
                            json!({ "content": content_out })
                        } else {
                            json!({})
                        };
                        let mut openai_chunk = json!({
                            "id": &self.stream_id,
                            "object": "chat.completion.chunk",
                            "created": self.created_ts,
                            "model": &self.model,
                            "choices": [{
                                "index": idx as u32,
                                "delta": delta,
                                "finish_reason": finish_reason
                            }]
                        });
                        if finish_reason.is_some() && !self.include_usage {
                            if let Some(ref usage) = self.final_usage {
                                openai_chunk["usage"] = serde_json::to_value(usage).unwrap();
                            }
                            self.final_usage = None;
                        }
                        let sse_out = format!(
                            "data: {}\n\n",
                            serde_json::to_string(&openai_chunk).unwrap_or_default()
                        );
                        chunks.push(Bytes::from(sse_out));
                    }
                }
            }
        } else {
            report_sse_json_parse(
                "openai",
                "create_openai_sse_stream_with_anchor",
                json_part,
                format!(
                    "model={} session={} messages={}",
                    self.model, self.session_id, self.message_count
                ),
            );
        }

        chunks
    }

    fn has_content(&self) -> bool {
        self.has_content
    }

    fn has_thinking(&self) -> bool {
        self.has_thinking
    }

    fn is_thinking_active(&self) -> bool {
        self.is_thinking_active
    }

    fn is_terminal(&self) -> bool {
        self.has_emitted_done
    }

    fn emit_finalize(&mut self) -> Vec<Bytes> {
        let mut chunks = Vec::new();

        // [收尾保底防御 Fixes #3716]
        // 若流已产生正文或工具调用，但上游在未显式下发 finishReason 的情况下关闭了连接 (EOF) 或仅下发了 usageMetadata：
        // 严格遵循 OpenAI 流式契约，在发射 [DONE] 之前补发带有终态 finish_reason ("stop" 或 "tool_calls") 的收尾块。
        // 防止下游客户端（如 OpenCode、Cline）判定为 finish=unknown 并错误触发空 assistant 续步导致上游 400 报错。
        if !self.has_emitted_finish_reason
            && (self.has_content || !self.emitted_tool_calls.is_empty())
        {
            self.has_emitted_finish_reason = true;
            let final_reason = if !self.emitted_tool_calls.is_empty() {
                "tool_calls"
            } else {
                "stop"
            };
            let mut finish_chunk = json!({
                "id": &self.stream_id,
                "object": "chat.completion.chunk",
                "created": self.created_ts,
                "model": &self.model,
                "choices": [{
                    "index": 0,
                    "delta": {},
                    "finish_reason": final_reason
                }]
            });
            if !self.include_usage {
                if let Some(usage) = self.final_usage.take() {
                    finish_chunk["usage"] = serde_json::to_value(usage).unwrap_or_default();
                }
            }
            chunks.push(Bytes::from(format!(
                "data: {}\n\n",
                serde_json::to_string(&finish_chunk).unwrap_or_default()
            )));
        }

        if self.include_usage {
            if let Some(usage) = self.final_usage.take() {
                let usage_chunk = json!({
                    "id": &self.stream_id,
                    "object": "chat.completion.chunk",
                    "created": self.created_ts,
                    "model": &self.model,
                    "choices": [],
                    "usage": usage
                });
                chunks.push(Bytes::from(format!(
                    "data: {}\n\n",
                    serde_json::to_string(&usage_chunk).unwrap_or_default()
                )));
            }
        }
        self.has_emitted_done = true;
        chunks.push(Bytes::from("data: [DONE]\n\n"));
        chunks
    }

    fn emit_abnormal_finalize(&mut self, _error_report: &StreamErrorReport) -> Vec<Bytes> {
        let mut chunks = Vec::new();

        // 终态与未产生内容防护：若已经发射 [DONE]，或从未产生任何正文/工具调用，绝不可伪造收尾
        if self.has_emitted_done || (!self.has_content && self.emitted_tool_calls.is_empty()) {
            return chunks;
        }

        self.is_thinking_active = false;

        // 若流尚未发射 finish_reason，按异常收尾契约下发带有 "tool_calls" 或 "length" 的终态帧
        if !self.has_emitted_finish_reason {
            self.has_emitted_finish_reason = true;
            let finish_reason = if !self.emitted_tool_calls.is_empty() {
                "tool_calls"
            } else {
                "length"
            };

            let mut finish_chunk = json!({
                "id": &self.stream_id,
                "object": "chat.completion.chunk",
                "created": self.created_ts,
                "model": &self.model,
                "choices": [{
                    "index": 0,
                    "delta": {},
                    "finish_reason": finish_reason
                }]
            });
            if !self.include_usage {
                if let Some(usage) = self.final_usage.take() {
                    finish_chunk["usage"] = serde_json::to_value(usage).unwrap_or_default();
                }
            }
            chunks.push(Bytes::from(format!(
                "data: {}\n\n",
                serde_json::to_string(&finish_chunk).unwrap_or_default()
            )));
        }

        if self.include_usage {
            if let Some(usage) = self.final_usage.take() {
                let usage_chunk = json!({
                    "id": &self.stream_id,
                    "object": "chat.completion.chunk",
                    "created": self.created_ts,
                    "model": &self.model,
                    "choices": [],
                    "usage": usage
                });
                chunks.push(Bytes::from(format!(
                    "data: {}\n\n",
                    serde_json::to_string(&usage_chunk).unwrap_or_default()
                )));
            }
        }
        self.has_emitted_done = true;
        chunks.push(Bytes::from("data: [DONE]\n\n"));
        chunks
    }

    fn emit_initial_error(&mut self, error_report: &StreamErrorReport) -> Vec<Bytes> {
        vec![
            Bytes::from(openai_chat_error_frame(
                &self.stream_id,
                self.created_ts,
                &self.model,
                "chat.completion.chunk",
                error_report,
            )),
            Bytes::from("data: [DONE]\n\n"),
        ]
    }

    fn on_finish(&mut self) {
        let acc = std::mem::take(&mut self.thinking_acc);
        acc.commit(&self.session_id);
    }
}

pub fn create_openai_sse_stream_with_anchor<S, E>(
    gemini_stream: Pin<Box<S>>,
    model: String,
    session_id: String,
    message_count: usize,
    client_tool_names: Option<std::collections::HashSet<String>>,
    include_usage: bool,
    causal_anchor: Option<String>,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, String>> + Send>>
where
    S: Stream<Item = Result<Bytes, E>> + Send + ?Sized + 'static,
    E: std::fmt::Display + Send + 'static,
{
    let stream_id = format!("chatcmpl-{}", Uuid::new_v4());
    let created_ts = Utc::now().timestamp();
    let empty_set = std::collections::HashSet::new();
    let client_tool_names = client_tool_names.unwrap_or(empty_set);
    let thinking_acc = if let Some(ref a) = causal_anchor {
        crate::proxy::thinking_store::TurnAccumulator::with_anchor(a)
    } else {
        crate::proxy::thinking_store::TurnAccumulator::new()
    };

    let trace_info = format!(
        "model={} session={} messages={}",
        model, session_id, message_count
    );

    let handler = OpenAIChatStreamHandler {
        stream_id,
        created_ts,
        model,
        session_id,
        message_count,
        client_tool_names,
        include_usage,
        emitted_tool_calls: std::collections::HashSet::new(),
        final_usage: None,
        tool_call_index: 0,
        thinking_acc,
        has_content: false,
        has_thinking: false,
        is_thinking_active: false,
        has_emitted_finish_reason: false,
        has_emitted_done: false,
    };

    let config =
        StreamLifecycleConfig::new("openai", "create_openai_sse_stream_with_anchor", trace_info);

    run_stream_lifecycle(gemini_stream, handler, config)
}

/// Legacy Completions（`text_completion`）流渲染器。
///
/// 心跳、首字宽限、过渡断崖跃迁、稳态滑动超时与错误终态全部由
/// `pipeline::stream_lifecycle::run_stream_lifecycle` 统一驱动；
/// 此处仅保留 Gemini SSE → OpenAI `text_completion` 的协议差异。
pub struct LegacyCompletionStreamHandler {
    stream_id: String,
    created_ts: i64,
    model: String,
    session_id: String,
    message_count: usize,
    final_usage: Option<super::models::OpenAIUsage>,
    thinking_acc: crate::proxy::thinking_store::TurnAccumulator,
    has_content: bool,
    has_thinking: bool,
    is_thinking_active: bool,
    thinking_committed: bool,
}

impl ProtocolStreamHandler for LegacyCompletionStreamHandler {
    fn process_line(&mut self, line: &str) -> Vec<Bytes> {
        let mut frames = Vec::new();
        if !line.starts_with("data: ") {
            return frames;
        }
        let json_part = line.trim_start_matches("data: ").trim();
        if json_part.is_empty() || json_part == "[DONE]" {
            return frames;
        }

        let mut json = match serde_json::from_str::<Value>(json_part) {
            Ok(json) => json,
            Err(_) => {
                report_sse_json_parse(
                    "openai-legacy",
                    "create_legacy_sse_stream",
                    json_part,
                    format!(
                        "model={} session={} messages={}",
                        self.model, self.session_id, self.message_count
                    ),
                );
                return frames;
            }
        };

        let actual_data = if let Some(inner) = json.get_mut("response").map(|v| v.take()) {
            inner
        } else {
            json
        };
        if let Some(u) = actual_data.get("usageMetadata") {
            self.final_usage = extract_usage_metadata(u);
        }

        let mut content_out = String::new();
        if let Some(candidates) = actual_data.get("candidates").and_then(|c| c.as_array()) {
            if let Some(candidate) = candidates.first() {
                if let Some(parts) = candidate
                    .get("content")
                    .and_then(|c| c.get("parts"))
                    .and_then(|p| p.as_array())
                {
                    for part in parts {
                        self.thinking_acc.ingest_part(part);
                        let is_thought = part
                            .get("thought")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                            if is_thought {
                                let clean_text = text
                                    .replace(" thinking\n", "")
                                    .replace(" thinking", "")
                                    .replace("\n response", "")
                                    .replace(" response", "");
                                if !clean_text.is_empty() {
                                    self.has_thinking = true;
                                    self.has_content = true;
                                    self.is_thinking_active = true;
                                }
                                content_out.push_str(&clean_text);
                            } else {
                                if !text.is_empty() {
                                    self.has_content = true;
                                    self.is_thinking_active = false;
                                }
                                content_out.push_str(text);
                            }
                        }
                        if let Some(sig) = part
                            .get("thoughtSignature")
                            .or(part.get("thought_signature"))
                            .and_then(|s| s.as_str())
                        {
                            store_thought_signature(sig, &self.session_id, self.message_count);
                        }
                    }
                }
            }
        }

        let finish_reason = actual_data
            .get("candidates")
            .and_then(|c| c.as_array())
            .and_then(|c| c.first())
            .and_then(|c| c.get("finishReason"))
            .and_then(|f| f.as_str())
            .map(|f| match f {
                "STOP" => "stop",
                "MAX_TOKENS" => "length",
                "SAFETY" => "content_filter",
                "RECITATION" => "content_filter",
                _ => "stop",
            });

        let mut legacy_chunk = json!({
            "id": &self.stream_id,
            "object": "text_completion",
            "created": self.created_ts,
            "model": &self.model,
            "choices": [{ "text": content_out, "index": 0, "logprobs": null, "finish_reason": finish_reason }]
        });
        if let Some(ref usage) = self.final_usage {
            legacy_chunk["usage"] = serde_json::to_value(usage).unwrap();
        }
        if finish_reason.is_some() {
            self.final_usage = None;
        }
        frames.push(Bytes::from(format!(
            "data: {}\n\n",
            serde_json::to_string(&legacy_chunk).unwrap_or_default()
        )));
        frames
    }

    fn has_content(&self) -> bool {
        self.has_content
    }

    fn has_thinking(&self) -> bool {
        self.has_thinking
    }

    fn is_thinking_active(&self) -> bool {
        self.is_thinking_active
    }

    fn finalize_without_content(&self) -> bool {
        // 空 text_completion 仍以 [DONE] 表达流正常结束，保持历史行为。
        true
    }

    fn emit_finalize(&mut self) -> Vec<Bytes> {
        vec![Bytes::from("data: [DONE]\n\n")]
    }

    fn emit_abnormal_finalize(&mut self, error_report: &StreamErrorReport) -> Vec<Bytes> {
        vec![
            Bytes::from(openai_chat_error_frame(
                &self.stream_id,
                self.created_ts,
                &self.model,
                "text_completion",
                error_report,
            )),
            Bytes::from("data: [DONE]\n\n"),
        ]
    }

    fn emit_initial_error(&mut self, _error_report: &StreamErrorReport) -> Vec<Bytes> {
        // 统一由 emit_error_finalize 下发协议终结错误，避免重复错误帧。
        Vec::new()
    }

    fn emit_error_finalize(&mut self, error_report: &StreamErrorReport) -> (Vec<Bytes>, bool) {
        (self.emit_abnormal_finalize(error_report), true)
    }

    fn on_finish(&mut self) {
        if self.thinking_committed {
            return;
        }
        self.thinking_committed = true;
        let acc = std::mem::take(&mut self.thinking_acc);
        acc.commit(&self.session_id);
    }
}

pub fn create_legacy_sse_stream<S, E>(
    gemini_stream: Pin<Box<S>>,
    model: String,
    session_id: String,
    message_count: usize,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, String>> + Send>>
where
    S: Stream<Item = Result<Bytes, E>> + Send + ?Sized + 'static,
    E: std::fmt::Display + Send + 'static,
{
    let charset = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::thread_rng();
    let random_str: String = (0..28)
        .map(|_| {
            let idx = rng.gen_range(0..charset.len());
            charset.chars().nth(idx).unwrap()
        })
        .collect();

    let trace_info = format!(
        "model={} session={} messages={}",
        model, session_id, message_count
    );
    let handler = LegacyCompletionStreamHandler {
        stream_id: format!("cmpl-{}", random_str),
        created_ts: Utc::now().timestamp(),
        model,
        session_id,
        message_count,
        final_usage: None,
        thinking_acc: crate::proxy::thinking_store::TurnAccumulator::new(),
        has_content: false,
        has_thinking: false,
        is_thinking_active: false,
        thinking_committed: false,
    };
    let config =
        StreamLifecycleConfig::new("openai-legacy", "create_legacy_sse_stream", trace_info);
    run_stream_lifecycle(gemini_stream, handler, config)
}

#[track_caller]
fn report_sse_json_parse(
    adapter: &'static str,
    function: &'static str,
    json_part: &str,
    params: impl std::fmt::Display,
) {
    let parse_err = match serde_json::from_str::<Value>(json_part) {
        Err(err) => err,
        Ok(_) => return,
    };
    let _ = report_stream_error(
        adapter,
        function,
        &format!("json parse error: {parse_err}"),
        format!(
            "{params} line_bytes={} preview={}",
            json_part.len(),
            preview_payload(json_part)
        ),
    );
}

fn split_namespace_tool_name(qualified_name: &str) -> (String, Option<String>) {
    let name = qualified_name.trim();
    if name.starts_with("mcp__") {
        return (name.to_string(), None);
    }
    if let Some(pos) = name.find("__") {
        if pos > 0 {
            let namespace = name[..pos].to_string();
            let actual_name = name[pos + 2..].to_string();
            return (actual_name, Some(namespace));
        }
    }
    (name.to_string(), None)
}

fn openai_chat_error_frame(
    stream_id: &str,
    created_ts: i64,
    model: &str,
    object: &str,
    report: &StreamErrorReport,
) -> String {
    let error_chunk = json!({
        "id": stream_id,
        "object": object,
        "created": created_ts,
        "model": model,
        "choices": [],
        "error": {
            "type": report.classified.error_type,
            "message": report.client_message(),
            "code": "stream_error",
            "i18n_key": report.classified.i18n_key,
            "function": report.function,
            "call_site": report.call_site(),
            "params": report.params
        }
    });
    format!(
        "data: {}\n\n",
        serde_json::to_string(&error_chunk).unwrap_or_default()
    )
}

fn inject_seq(mut event: Value, seq: &mut u64) -> Value {
    if let Some(obj) = event.as_object_mut() {
        obj.insert("sequence_number".to_string(), json!(*seq));
    }
    *seq += 1;
    event
}

/// Serialize one named Responses API SSE frame.
///
/// Codex Desktop consumes the SSE `event` field as well as the JSON payload's
/// `type`. Keeping both in sync matches the native Responses stream and avoids
/// reasoning/tool lifecycle events being treated as anonymous data messages.
fn codex_sse_frame(event: &Value) -> Bytes {
    let event_name = event
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("message");
    let payload = serde_json::to_string(event).unwrap_or_else(|_| "{}".to_string());
    Bytes::from(format!("event: {event_name}\ndata: {payload}\n\n"))
}

/// Codex / Responses SSE 流渲染器。
///
/// 心跳、首字宽限、过渡断崖跃迁、稳态滑动超时、异常优雅收尾与首包错误契约
/// 全部由 `pipeline::stream_lifecycle::run_stream_lifecycle` 统一驱动；
/// 此处仅保留 Gemini SSE → Responses API 事件流（reasoning / message / tool /
/// grounding / 终态）的协议差异。
pub struct CodexResponsesStreamHandler {
    model: String,
    session_id: String,
    message_count: usize,
    response_id: String,
    cache_tool_calls: bool,
    completion_tx:
        Option<tokio::sync::oneshot::Sender<(Vec<Value>, tokio::sync::oneshot::Sender<()>)>>,
    ack_rx: Option<tokio::sync::oneshot::Receiver<()>>,
    ack_failed: bool,
    created_at: u64,
    item_id_prefix: String,
    message_item_id: String,
    sequence_number: u64,
    message_item_emitted: bool,
    reasoning_open: bool,
    reasoning_item_seq: u32,
    active_reasoning_item_id: String,
    emitted_tool_calls: std::collections::HashSet<String>,
    accumulated_text: String,
    accumulated_thinking: String,
    thinking_acc: crate::proxy::thinking_store::TurnAccumulator,
    has_thinking: bool,
    has_seen_tool_calls: bool,
    final_finish_reason: Option<String>,
    final_outputs_map: std::collections::BTreeMap<u32, Value>,
    next_output_index: u32,
    message_output_index: u32,
    reasoning_output_index: u32,
    final_usage: Option<super::models::OpenAIUsage>,
    thinking_committed: bool,
}

impl CodexResponsesStreamHandler {
    fn new(
        model: String,
        session_id: String,
        message_count: usize,
        response_id: String,
        completion_tx: Option<
            tokio::sync::oneshot::Sender<(Vec<Value>, tokio::sync::oneshot::Sender<()>)>,
        >,
        cache_tool_calls: bool,
    ) -> Self {
        let item_id_prefix = uuid::Uuid::new_v4().simple().to_string();
        let message_item_id = format!("msg_{}_0", &item_id_prefix[..16]);
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            model,
            session_id,
            message_count,
            response_id,
            cache_tool_calls,
            completion_tx,
            ack_rx: None,
            ack_failed: false,
            created_at,
            item_id_prefix,
            message_item_id,
            sequence_number: 0,
            message_item_emitted: false,
            reasoning_open: false,
            reasoning_item_seq: 0,
            active_reasoning_item_id: String::new(),
            emitted_tool_calls: std::collections::HashSet::new(),
            accumulated_text: String::new(),
            accumulated_thinking: String::new(),
            thinking_acc: crate::proxy::thinking_store::TurnAccumulator::new(),
            has_thinking: false,
            has_seen_tool_calls: false,
            final_finish_reason: None,
            final_outputs_map: std::collections::BTreeMap::new(),
            next_output_index: 0,
            message_output_index: 0,
            reasoning_output_index: 0,
            final_usage: None,
            thinking_committed: false,
        }
    }

    fn push_event(&mut self, frames: &mut Vec<Bytes>, event: Value) {
        let event = inject_seq(event, &mut self.sequence_number);
        frames.push(codex_sse_frame(&event));
    }

    fn push_error_event(&mut self, frames: &mut Vec<Bytes>, error_report: &StreamErrorReport) {
        let err_ev = json!({
            "type": "error",
            "code": error_report.classified.error_type,
            "message": error_report.client_message(),
            "function": error_report.function,
            "call_site": error_report.call_site(),
            "params": error_report.params,
        });
        self.push_event(frames, err_ev);
    }

    /// 关闭仍打开的 reasoning summary，保证输出项生命周期不重叠。
    fn close_reasoning(&mut self, frames: &mut Vec<Bytes>) {
        if !self.reasoning_open {
            return;
        }
        let text_done = json!({
            "type": "response.reasoning_summary_text.done",
            "item_id": &self.active_reasoning_item_id,
            "output_index": self.reasoning_output_index,
            "summary_index": 0,
            "text": &self.accumulated_thinking
        });
        self.push_event(frames, text_done);

        let summary_part_done = json!({
            "type": "response.reasoning_summary_part.done",
            "item_id": &self.active_reasoning_item_id,
            "output_index": self.reasoning_output_index,
            "summary_index": 0,
            "part": {
                "type": "summary_text",
                "text": &self.accumulated_thinking
            }
        });
        self.push_event(frames, summary_part_done);

        let reasoning_item = json!({
            "id": &self.active_reasoning_item_id,
            "type": "reasoning",
            "status": "completed",
            "summary": [{
                "type": "summary_text",
                "text": &self.accumulated_thinking
            }]
        });

        let done_ev = json!({
            "type": "response.output_item.done",
            "output_index": self.reasoning_output_index,
            "item": &reasoning_item
        });
        self.push_event(frames, done_ev);

        self.final_outputs_map
            .insert(self.reasoning_output_index, reasoning_item);
        self.reasoning_open = false;
    }

    /// 惰性打开 assistant message 输出项（正文/grounding 首次到达时）。
    fn open_message(&mut self, frames: &mut Vec<Bytes>) {
        if self.message_item_emitted {
            return;
        }
        self.message_item_emitted = true;
        self.message_output_index = self.next_output_index;
        self.next_output_index += 1;

        let output_item_added = json!({
            "type": "response.output_item.added",
            "output_index": self.message_output_index,
            "item": {
                "id": &self.message_item_id,
                "type": "message",
                "role": "assistant",
                "phase": "commentary",
                "status": "in_progress",
                "content": []
            }
        });
        self.push_event(frames, output_item_added);

        let content_part_added = json!({
            "type": "response.content_part.added",
            "item_id": &self.message_item_id,
            "output_index": self.message_output_index,
            "content_index": 0,
            "part": { "type": "output_text", "text": "", "annotations": [] }
        });
        self.push_event(frames, content_part_added);
    }

    fn handle_part(&mut self, part: &Value, frames: &mut Vec<Bytes>) {
        self.thinking_acc.ingest_part(part);
        let is_thought = part
            .get("thought")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let is_text_or_tool = part.get("text").is_some()
            || part.get("functionCall").is_some()
            || part.get("inlineData").is_some();

        // 正文或工具项开始前先关闭 reasoning，避免输出项生命周期重叠。
        if is_text_or_tool && !is_thought {
            self.close_reasoning(frames);
        }

        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
            let clean_text = if is_thought {
                text.replace(" thinking\n", "")
                    .replace(" thinking", "")
                    .replace("\n response", "")
                    .replace(" response", "")
            } else {
                text.to_string()
            };
            if !clean_text.is_empty() {
                if is_thought && self.message_item_emitted {
                    // 正文一旦开始，它就是本次响应的权威结果；迟到的思考增量
                    // 不得混入正文，也不得再打开重叠的 reasoning 输出项。
                    tracing::warn!(
                        "[Codex-Stream] Dropping late thought delta after assistant text started"
                    );
                } else if is_thought {
                    if !self.reasoning_open {
                        self.reasoning_output_index = self.next_output_index;
                        self.next_output_index += 1;
                        self.active_reasoning_item_id = format!(
                            "rs_{}_{}",
                            &self.item_id_prefix[..16],
                            self.reasoning_item_seq
                        );
                        self.reasoning_item_seq += 1;
                        self.accumulated_thinking.clear();

                        let output_item_added = json!({
                            "type": "response.output_item.added",
                            "output_index": self.reasoning_output_index,
                            "item": {
                                "id": &self.active_reasoning_item_id,
                                "type": "reasoning",
                                "status": "in_progress",
                                "summary": []
                            }
                        });
                        self.push_event(frames, output_item_added);

                        let part_added = json!({
                            "type": "response.reasoning_summary_part.added",
                            "item_id": &self.active_reasoning_item_id,
                            "output_index": self.reasoning_output_index,
                            "summary_index": 0,
                            "part": { "type": "summary_text", "text": "" }
                        });
                        self.push_event(frames, part_added);

                        self.reasoning_open = true;
                    }

                    self.accumulated_thinking.push_str(&clean_text);
                    self.has_thinking = true;
                    let delta_ev = json!({
                        "type": "response.reasoning_summary_text.delta",
                        "item_id": &self.active_reasoning_item_id,
                        "output_index": self.reasoning_output_index,
                        "summary_index": 0,
                        "delta": clean_text
                    });
                    self.push_event(frames, delta_ev);
                } else {
                    self.open_message(frames);
                    self.accumulated_text.push_str(&clean_text);
                    let delta_ev = json!({
                        "type": "response.output_text.delta",
                        "item_id": &self.message_item_id,
                        "output_index": self.message_output_index,
                        "content_index": 0,
                        "delta": clean_text
                    });
                    self.push_event(frames, delta_ev);
                }
            }
        }

        if let Some(sig) = part
            .get("thoughtSignature")
            .or(part.get("thought_signature"))
            .and_then(|s| s.as_str())
        {
            store_thought_signature(sig, &self.session_id, self.message_count);
        }

        if let Some(func_call) = part.get("functionCall") {
            self.handle_tool_call(func_call, part, frames);
        }
    }

    fn handle_tool_call(&mut self, func_call: &Value, part: &Value, frames: &mut Vec<Bytes>) {
        let call_key = serde_json::to_string(func_call).unwrap_or_default();
        if self.emitted_tool_calls.contains(&call_key) {
            return;
        }
        self.emitted_tool_calls.insert(call_key.clone());

        let name = func_call
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let mut args = func_call.get("args").unwrap_or(&json!({})).clone();

        // [FIX #1575 & #3430] 标准化并清洗 shell / PowerShell 等工具参数名称与必填字段
        super::response::normalize_and_sanitize_tool_args(name, &mut args);

        let args_str = serde_json::to_string(&args).unwrap_or_default();

        let sequence_number = self.sequence_number;
        let call_id = func_call
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                use std::hash::{Hash, Hasher};
                call_key.hash(&mut hasher);
                sequence_number.hash(&mut hasher);
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
                    .hash(&mut hasher);
                format!("call_{:x}", hasher.finish())
            });

        if let Some(sig) = part
            .get("thoughtSignature")
            .or(part.get("thought_signature"))
            .and_then(|s| s.as_str())
        {
            crate::proxy::SignatureCache::global().cache_tool_signature(
                &self.session_id,
                &call_id,
                sig.to_string(),
            );
        }
        self.thinking_acc.record_tool_id(name, &call_id);

        let (actual_name, namespace) = split_namespace_tool_name(name);
        let tool_item_id = format!("item-{}", &Uuid::new_v4().to_string()[..16]);
        let is_custom_tool = actual_name == "apply_patch"
            || actual_name == "apply_patch_v2"
            || actual_name == "shell";

        let final_args_str = args_str.clone();

        let mut item_obj = json!({
            "id": &tool_item_id,
            "type": if is_custom_tool { "custom_tool_call" } else { "function_call" },
            "status": "completed",
            "name": actual_name,
            "call_id": &call_id,
        });
        if is_custom_tool {
            item_obj["input"] = json!(&final_args_str);
        } else {
            item_obj["arguments"] = json!(&final_args_str);
        }
        if let Some(ns) = namespace {
            item_obj["namespace"] = json!(ns);
        }

        let tool_output_index = self.next_output_index;
        self.next_output_index += 1;

        self.has_seen_tool_calls = true;

        let mut added_item = item_obj.clone();
        added_item["status"] = json!("in_progress");
        if is_custom_tool {
            added_item["input"] = json!("");
        } else {
            added_item["arguments"] = json!("");
        }
        let added_ev = json!({
            "type": "response.output_item.added",
            "output_index": tool_output_index,
            "item": added_item
        });
        self.push_event(frames, added_ev);

        let mut delta_ev = json!({
            "type": if is_custom_tool { "response.custom_tool_call_input.delta" } else { "response.function_call_arguments.delta" },
            "item_id": &tool_item_id,
            "output_index": tool_output_index,
            "delta": &final_args_str
        });
        if is_custom_tool {
            delta_ev["call_id"] = json!(&call_id);
        }
        self.push_event(frames, delta_ev);

        let mut args_done_ev = json!({
            "type": if is_custom_tool { "response.custom_tool_call_input.done" } else { "response.function_call_arguments.done" },
            "item_id": &tool_item_id,
            "output_index": tool_output_index,
        });
        if is_custom_tool {
            args_done_ev["call_id"] = json!(&call_id);
            args_done_ev["input"] = json!(&final_args_str);
        } else {
            args_done_ev["arguments"] = json!(&final_args_str);
        }
        self.push_event(frames, args_done_ev);

        let done_ev = json!({
            "type": "response.output_item.done",
            "output_index": tool_output_index,
            "item": item_obj
        });
        self.push_event(frames, done_ev);

        let tc_val = item_obj.clone();
        if self.cache_tool_calls {
            crate::proxy::handlers::openai::insert_cached_tool_call(
                call_id.clone(),
                tc_val.clone(),
            );
        }
        self.final_outputs_map.insert(tool_output_index, tc_val);
    }

    fn handle_grounding(&mut self, grounding: &Value, frames: &mut Vec<Bytes>) {
        let mut grounding_text = String::new();
        if let Some(queries) = grounding.get("webSearchQueries").and_then(|q| q.as_array()) {
            let query_list: Vec<&str> = queries.iter().filter_map(|v| v.as_str()).collect();
            if !query_list.is_empty() {
                grounding_text.push_str("\n\n---\n**🔍 已为您搜索：** ");
                grounding_text.push_str(&query_list.join(", "));
            }
        }
        if let Some(chunks) = grounding.get("groundingChunks").and_then(|c| c.as_array()) {
            let mut links = Vec::new();
            for (i, chunk) in chunks.iter().enumerate() {
                if let Some(web) = chunk.get("web") {
                    let title = web
                        .get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or("网页来源");
                    let uri = web.get("uri").and_then(|v| v.as_str()).unwrap_or("#");
                    links.push(format!("[{}] [{}]({})", i + 1, title, uri));
                }
            }
            if !links.is_empty() {
                grounding_text.push_str("\n\n**🌐 来源引文：**\n");
                grounding_text.push_str(&links.join("\n"));
            }
        }
        if grounding_text.is_empty() {
            return;
        }

        self.open_message(frames);
        self.accumulated_text.push_str(&grounding_text);
        let delta_ev = json!({
            "type": "response.output_text.delta",
            "item_id": &self.message_item_id,
            "output_index": self.message_output_index,
            "content_index": 0,
            "delta": grounding_text
        });
        self.push_event(frames, delta_ev);
    }

    fn commit_thinking(&mut self) {
        if self.thinking_committed {
            return;
        }
        self.thinking_committed = true;
        self.thinking_acc.clone().commit(&self.session_id);
        if self.session_id != self.response_id {
            self.thinking_acc.clone().commit(&self.response_id);
        }
    }

    fn build_terminal_frames(&mut self, error_report: Option<&StreamErrorReport>) -> Vec<Bytes> {
        let mut frames = Vec::new();

        // 上游流结束时仍打开的 reasoning summary 必须先闭合。
        self.close_reasoning(&mut frames);

        // 代理生成的诊断可能在上游结束后才可见；惰性打开 message，避免被 reasoning 吞掉。
        if !self.message_item_emitted && !self.accumulated_text.is_empty() {
            self.open_message(&mut frames);
        }

        if self.message_item_emitted {
            let text_done = json!({
                "type": "response.output_text.done",
                "item_id": &self.message_item_id,
                "output_index": self.message_output_index,
                "content_index": 0,
                "text": &self.accumulated_text
            });
            self.push_event(&mut frames, text_done);

            let content_part_done = json!({
                "type": "response.content_part.done",
                "item_id": &self.message_item_id,
                "output_index": self.message_output_index,
                "content_index": 0,
                "part": {
                    "type": "output_text",
                    "text": &self.accumulated_text,
                    "annotations": []
                }
            });
            self.push_event(&mut frames, content_part_done);

            // 工具轮次的 message 只是过程性 commentary；无工具调用的 message 才是最终答案。
            let message_phase = if self.has_seen_tool_calls {
                "commentary"
            } else {
                "final_answer"
            };
            let message_item = json!({
                "id": &self.message_item_id,
                "type": "message",
                "role": "assistant",
                "phase": message_phase,
                "status": "completed",
                "content": [{
                    "type": "output_text",
                    "text": &self.accumulated_text,
                    "annotations": []
                }]
            });

            let output_item_done = json!({
                "type": "response.output_item.done",
                "output_index": self.message_output_index,
                "item": message_item.clone()
            });
            self.push_event(&mut frames, output_item_done);

            self.final_outputs_map
                .insert(self.message_output_index, message_item);
        }

        let final_outputs: Vec<Value> = std::mem::take(&mut self.final_outputs_map)
            .into_values()
            .collect();

        let missing_actionable_output = !self.message_item_emitted && !self.has_seen_tool_calls;
        let mut terminal_status = if missing_actionable_output {
            "incomplete"
        } else {
            match self.final_finish_reason.as_deref() {
                Some("MAX_TOKENS")
                | Some("SAFETY")
                | Some("RECITATION")
                | Some("BLOCKLIST")
                | Some("PROHIBITED_CONTENT")
                | Some("SPII")
                | Some("IMAGE_SAFETY")
                | Some("IMAGE_PROHIBITED_CONTENT")
                | None => "incomplete",
                _ => "completed",
            }
        };
        let mut incomplete_details = if terminal_status == "incomplete" {
            let reason = match self.final_finish_reason.as_deref() {
                Some("MAX_TOKENS") => "max_output_tokens",
                Some("SAFETY")
                | Some("RECITATION")
                | Some("BLOCKLIST")
                | Some("PROHIBITED_CONTENT")
                | Some("SPII")
                | Some("IMAGE_SAFETY")
                | Some("IMAGE_PROHIBITED_CONTENT") => "content_filter",
                _ => "interrupted",
            };
            json!({"reason": reason})
        } else {
            Value::Null
        };
        let mut terminal_error = if missing_actionable_output {
            json!({
                "code": "empty_response",
                "message": "Gemini stream ended without a final assistant message or tool call."
            })
        } else if self.final_finish_reason.is_none() {
            json!({
                "code": "upstream_interrupted",
                "message": "Gemini stream ended without finishReason."
            })
        } else {
            Value::Null
        };
        if let Some(report) = error_report {
            terminal_status = "failed";
            incomplete_details = Value::Null;
            terminal_error = json!({
                "code": report.classified.error_type,
                "message": report.client_message(),
                "function": report.function,
                "call_site": report.call_site(),
                "params": report.params,
            });
        }
        let terminal_type = format!("response.{terminal_status}");
        let completed_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        if terminal_status == "completed" {
            if let Some(tx) = self.completion_tx.take() {
                let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
                if tx.send((final_outputs.clone(), ack_tx)).is_ok() {
                    self.ack_rx = Some(ack_rx);
                } else {
                    self.ack_failed = true;
                }
            }
        }

        self.commit_thinking();

        let mut completed_ev = json!({
            "type": terminal_type,
            "response": {
                "id": &self.response_id,
                "object": "response",
                "created_at": self.created_at,
                "completed_at": completed_at,
                "status": terminal_status,
                "model": &self.model,
                "output": final_outputs,
                "incomplete_details": incomplete_details,
                "error": terminal_error
            }
        });

        if let Some(resp_obj) = completed_ev
            .get_mut("response")
            .and_then(|r| r.as_object_mut())
        {
            if let Some(ref usage) = self.final_usage {
                resp_obj.insert("usage".to_string(), usage.to_responses_usage_value());
            } else {
                resp_obj.insert(
                    "usage".to_string(),
                    json!({
                        "input_tokens": 0,
                        "input_tokens_details": {
                            "cached_tokens": 0
                        },
                        "output_tokens": 0,
                        "output_tokens_details": {
                            "reasoning_tokens": 0
                        },
                        "total_tokens": 0
                    }),
                );
            }
        }

        self.push_event(&mut frames, completed_ev);
        frames
    }
}

impl ProtocolStreamHandler for CodexResponsesStreamHandler {
    fn initial_frames(&mut self) -> Vec<Bytes> {
        let mut frames = Vec::new();

        // Native Responses lifecycle: created must be followed by in_progress.
        let lifecycle_response = json!({
            "id": &self.response_id,
            "object": "response",
            "created_at": self.created_at,
            "status": "in_progress",
            "model": &self.model,
            "output": [],
            "error": null,
            "incomplete_details": null,
            "usage": null
        });
        let created_ev =
            json!({ "type": "response.created", "response": lifecycle_response.clone() });
        self.push_event(&mut frames, created_ev);
        let in_progress_ev =
            json!({ "type": "response.in_progress", "response": lifecycle_response });
        self.push_event(&mut frames, in_progress_ev);

        frames
    }

    fn process_line(&mut self, line: &str) -> Vec<Bytes> {
        let mut frames = Vec::new();
        if !line.starts_with("data: ") {
            return frames;
        }
        let json_part = line.trim_start_matches("data: ").trim();
        if json_part.is_empty() || json_part == "[DONE]" {
            return frames;
        }

        let mut json = match serde_json::from_str::<Value>(json_part) {
            Ok(json) => json,
            Err(_) => {
                report_sse_json_parse(
                    "openai-responses",
                    "create_codex_sse_stream",
                    json_part,
                    format!(
                        "model={} session={} response_id={} messages={}",
                        self.model, self.session_id, self.response_id, self.message_count
                    ),
                );
                return frames;
            }
        };

        let actual_data = if let Some(inner) = json.get_mut("response").map(|v| v.take()) {
            inner
        } else {
            json
        };

        if let Some(u) = actual_data.get("usageMetadata") {
            self.final_usage = extract_usage_metadata(u);
        }

        if let Some(candidates) = actual_data.get("candidates").and_then(|c| c.as_array()) {
            if !candidates.is_empty() {
                tracing::debug!("[Codex-Stream-Debug] Raw Candidate: {:?}", candidates[0]);
            }
            if let Some(candidate) = candidates.first() {
                if let Some(reason) = candidate.get("finishReason").and_then(Value::as_str) {
                    self.final_finish_reason = Some(reason.to_string());
                }
                if let Some(parts) = candidate
                    .get("content")
                    .and_then(|c| c.get("parts"))
                    .and_then(|p| p.as_array())
                {
                    for part in parts {
                        self.handle_part(part, &mut frames);
                    }
                }

                // 处理 groundingMetadata (搜索引文)
                if let Some(grounding) = candidate.get("groundingMetadata") {
                    self.handle_grounding(grounding, &mut frames);
                }
            }
        }

        frames
    }

    fn has_content(&self) -> bool {
        self.message_item_emitted || self.has_seen_tool_calls
    }

    fn has_thinking(&self) -> bool {
        self.has_thinking
    }

    fn is_thinking_active(&self) -> bool {
        self.reasoning_open
    }

    fn finalize_without_content(&self) -> bool {
        // Responses 协议具备语义完整的 response.incomplete（含 empty_response 诊断），
        // 思考结束但无正文时仍应下发终态而非裸中断。
        true
    }

    fn emit_finalize(&mut self) -> Vec<Bytes> {
        self.build_terminal_frames(None)
    }

    fn emit_abnormal_finalize(&mut self, error_report: &StreamErrorReport) -> Vec<Bytes> {
        let mut frames = Vec::new();
        self.push_error_event(&mut frames, error_report);
        frames.extend(self.build_terminal_frames(Some(error_report)));
        frames
    }

    fn emit_initial_error(&mut self, _error_report: &StreamErrorReport) -> Vec<Bytes> {
        // 统一由 emit_error_finalize 下发 error + response.failed，避免重复。
        Vec::new()
    }

    fn emit_error_finalize(&mut self, error_report: &StreamErrorReport) -> (Vec<Bytes>, bool) {
        (self.emit_abnormal_finalize(error_report), true)
    }

    fn finalize_barrier(&mut self) -> Pin<Box<dyn std::future::Future<Output = bool> + Send + '_>> {
        Box::pin(async move {
            if self.ack_failed {
                return false;
            }
            match self.ack_rx.take() {
                Some(ack_rx) => ack_rx.await.is_ok(),
                None => true,
            }
        })
    }

    fn on_finish(&mut self) {
        self.commit_thinking();
    }
}

#[allow(clippy::too_many_arguments)]
pub fn create_codex_sse_stream<S, E>(
    gemini_stream: Pin<Box<S>>,
    model: String,
    session_id: String,
    message_count: usize,
    _assistant_turn_index: usize,
    response_id: String,
    completion_tx: Option<
        tokio::sync::oneshot::Sender<(Vec<Value>, tokio::sync::oneshot::Sender<()>)>,
    >,
    cache_tool_calls: bool,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, String>> + Send>>
where
    S: Stream<Item = Result<Bytes, E>> + Send + ?Sized + 'static,
    E: std::fmt::Display + Send + 'static,
{
    let trace_info = format!(
        "model={} session={} response_id={} messages={}",
        model, session_id, response_id, message_count
    );
    let handler = CodexResponsesStreamHandler::new(
        model,
        session_id,
        message_count,
        response_id,
        completion_tx,
        cache_tool_calls,
    );
    let config = StreamLifecycleConfig::new("openai-codex", "create_codex_sse_stream", trace_info);
    run_stream_lifecycle(gemini_stream, handler, config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;
    use serde_json::json;

    async fn collect_codex_stream(chunks: Vec<Value>) -> (String, Vec<Value>) {
        collect_codex_stream_with_cache(chunks, true).await
    }

    async fn collect_codex_stream_with_cache(
        chunks: Vec<Value>,
        cache_tool_calls: bool,
    ) -> (String, Vec<Value>) {
        let items: Vec<Result<Bytes, String>> = chunks
            .into_iter()
            .map(|chunk| Ok(Bytes::from(format!("data: {chunk}\n\n"))))
            .collect();
        let mut stream = create_codex_sse_stream(
            Box::pin(stream::iter(items)),
            "gemini-pro-agent".to_string(),
            "test-codex-session".to_string(),
            0,
            0,
            "resp-test-codex-session".to_string(),
            None,
            cache_tool_calls,
        );

        let mut raw = String::new();
        while let Some(item) = stream.next().await {
            raw.push_str(&String::from_utf8_lossy(&item.expect("codex stream item")));
        }
        let events = raw
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
            .filter_map(|data| serde_json::from_str::<Value>(data).ok())
            .collect();
        (raw, events)
    }

    async fn collect_legacy_stream(items: Vec<Result<Bytes, String>>) -> (String, Vec<Value>) {
        let mut stream = create_legacy_sse_stream(
            Box::pin(stream::iter(items)),
            "gemini-pro-agent".to_string(),
            "test-legacy-session".to_string(),
            0,
        );

        let mut raw = String::new();
        while let Some(item) = stream.next().await {
            raw.push_str(&String::from_utf8_lossy(&item.expect("legacy stream item")));
        }
        let events = raw
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
            .filter_map(|data| serde_json::from_str::<Value>(data).ok())
            .collect();
        (raw, events)
    }

    #[tokio::test]
    async fn test_legacy_stream_flattens_thinking_and_finishes_with_done() {
        let items = vec![
            Ok::<Bytes, String>(Bytes::from(
                "data: {\"candidates\":[{\"content\":{\"parts\":[{\"thought\":true,\"text\":\"Thinking.\"}]}}]}\n\n",
            )),
            Ok::<Bytes, String>(Bytes::from(
                "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Answer.\"}]},\"finishReason\":\"STOP\"}]}\n\n",
            )),
        ];
        let (raw, events) = collect_legacy_stream(items).await;

        assert_eq!(events[0]["choices"][0]["text"], "Thinking.");
        assert_eq!(events[1]["choices"][0]["text"], "Answer.");
        assert_eq!(events[1]["choices"][0]["finish_reason"], "stop");
        assert!(raw.ends_with("data: [DONE]\n\n"));
        assert!(!raw.contains("\"error\""));
    }

    #[tokio::test]
    async fn test_legacy_stream_error_after_content_emits_error_and_single_done() {
        let items = vec![
            Ok::<Bytes, String>(Bytes::from(
                "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Partial.\"}]}}]}\n\n",
            )),
            Err::<Bytes, String>(
                "error reading a body from connection: connection reset by peer".to_string(),
            ),
        ];
        let (raw, events) = collect_legacy_stream(items).await;

        assert_eq!(events[0]["choices"][0]["text"], "Partial.");
        let error_ev = events
            .iter()
            .find(|event| event.get("error").is_some())
            .expect("legacy error frame");
        assert_eq!(error_ev["error"]["code"], "stream_error");
        assert_eq!(raw.matches("data: [DONE]").count(), 1, "exactly one [DONE]");
    }

    #[tokio::test]
    async fn responses_midstream_body_cut_emits_classified_stream_error() {
        let items: Vec<Result<Bytes, String>> = vec![
            Ok(Bytes::from(
                "data: {\"candidates\":[{\"content\":{\"parts\":[{\"thought\":true,\"text\":\"hi\"}]}}]}\n\n",
            )),
            Err(
                "error reading a body from connection: connection closed before message completed"
                    .to_string(),
            ),
        ];
        let mut stream = create_codex_sse_stream(
            Box::pin(stream::iter(items)),
            "gemini-pro-agent".to_string(),
            "test-codex-session".to_string(),
            0,
            0,
            "resp-test-codex-session".to_string(),
            None,
            true,
        );

        let mut raw = String::new();
        while let Some(item) = stream.next().await {
            raw.push_str(&String::from_utf8_lossy(&item.expect("codex stream item")));
        }
        let events: Vec<Value> = raw
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
            .filter_map(|data| serde_json::from_str::<Value>(data).ok())
            .collect();

        let error_ev = events
            .iter()
            .find(|event| event["type"] == "error")
            .expect("classified error event");
        assert_eq!(error_ev["code"], "stream_error");
        let message = error_ev["message"].as_str().unwrap_or_default();
        assert!(message.contains("Stream interrupted before completion, please retry"));
        assert!(message.contains("fn=create_codex_sse_stream"));
        assert!(message.contains("session=test-codex-session"));
        assert!(message.contains("closed before message completed"));
        assert!(
            !message.contains("network or proxy"),
            "mid-stream body cut must not be labeled as a connect/proxy failure"
        );

        let failed = events
            .iter()
            .find(|event| event["type"] == "response.failed")
            .expect("failed response");
        assert_eq!(failed["response"]["status"], "failed");
        assert_eq!(failed["response"]["error"]["code"], "stream_error");
    }

    #[tokio::test]
    async fn responses_store_false_emits_complete_tool_call_without_caching_it() {
        let (_, events) = collect_codex_stream_with_cache(vec![json!({
            "response": {"candidates": [{
                "finishReason": "STOP",
                "content": {"parts": [{"functionCall": {"name":"shell_command", "args":{"command":"pwd"}}}]}
            }]}
        })], false).await;
        let completed = events
            .iter()
            .find(|event| event["type"] == "response.completed")
            .expect("completed response");
        let call = completed["response"]["output"]
            .as_array()
            .expect("output")
            .iter()
            .find(|item| item["type"] == "function_call")
            .expect("function call");
        let call_id = call["call_id"].as_str().expect("call id");
        assert_eq!(call["name"], "shell_command");
        assert_eq!(
            serde_json::from_str::<Value>(call["arguments"].as_str().expect("arguments"))
                .expect("JSON arguments"),
            json!({"command":"pwd"})
        );
        assert!(crate::proxy::handlers::openai::get_cached_tool_call(call_id).is_none());
        assert!(events
            .iter()
            .any(|event| event["type"] == "response.output_item.done"
                && event["item"]["call_id"] == call_id));
    }

    #[tokio::test]
    async fn codex_response_id_matches_saved_session_key() {
        let response_id = format!("resp-test-{}", uuid::Uuid::new_v4());
        let upstream = vec![Ok::<Bytes, String>(Bytes::from(
            "data: {\"response\":{\"candidates\":[{\"finishReason\":\"STOP\",\"content\":{\"parts\":[{\"thought\":true,\"text\":\"Checking.\"},{\"text\":\"done\"}]}}]}}\n\n",
        ))];
        let (completion_tx, completion_rx) =
            tokio::sync::oneshot::channel::<(Vec<Value>, tokio::sync::oneshot::Sender<()>)>();
        let save_response_id = response_id.clone();
        let save_task = tokio::spawn(async move {
            let (outputs, ack_tx) = completion_rx.await.expect("completed output");
            crate::proxy::http_session_store::save_session_delta(
                save_response_id,
                None,
                vec![json!({"id": "user-1", "role": "user", "content": "hello"})],
                outputs,
                String::new(),
                "gemini-pro-agent".to_string(),
                "routing-test-session".to_string(),
            )
            .await;
            ack_tx.send(()).expect("acknowledge session save");
        });
        let mut stream = create_codex_sse_stream(
            Box::pin(stream::iter(upstream)),
            "gemini-pro-agent".to_string(),
            "test-session".to_string(),
            1,
            0,
            response_id.clone(),
            Some(completion_tx),
            true,
        );

        let mut raw = String::new();
        let mut saw_completed = false;
        while let Some(item) = stream.next().await {
            let item = item.expect("stream item");
            let text = String::from_utf8_lossy(&item);
            if text.contains("event: response.completed") {
                saw_completed = true;
                let restored = crate::proxy::http_session_store::get_session(&response_id)
                    .await
                    .expect("session exists when response.completed is visible");
                assert_eq!(restored.input_items[0]["id"], "user-1");
                let completed: Value = serde_json::from_str(
                    text.lines()
                        .find_map(|line| line.strip_prefix("data: "))
                        .expect("completion data"),
                )
                .expect("completion JSON");
                assert_eq!(restored.input_items[1]["type"], "reasoning");
                assert_eq!(restored.input_items[1]["summary"][0]["text"], "Checking.");
                assert_eq!(
                    &restored.input_items[1..],
                    completed["response"]["output"].as_array().expect("output")
                );
                assert!(restored
                    .input_items
                    .iter()
                    .any(|item| item["role"] == "assistant"));
            }
            raw.push_str(&text);
        }
        save_task.await.expect("session save task");

        assert!(saw_completed);
        assert!(raw.contains(&format!("\"id\":\"{response_id}\"")));
    }

    #[test]
    fn response_branches_store_signatures_under_their_own_response_ids() {
        let branch_a = format!("resp-signature-a-{}", uuid::Uuid::new_v4());
        let branch_b = format!("resp-signature-b-{}", uuid::Uuid::new_v4());
        let signature_a = "a".repeat(64);
        let signature_b = "b".repeat(64);
        store_thought_signature(&signature_a, &branch_a, 1);
        store_thought_signature(&signature_b, &branch_b, 1);

        assert_eq!(
            crate::proxy::SignatureCache::global().get_session_signature(&branch_a),
            Some(signature_a)
        );
        assert_eq!(
            crate::proxy::SignatureCache::global().get_session_signature(&branch_b),
            Some(signature_b)
        );
    }

    #[tokio::test]
    async fn test_codex_reasoning_summary_and_tool_are_distinct_output_items() {
        let (raw, events) = collect_codex_stream(vec![
            json!({
                "response": {
                    "candidates": [{
                        "content": {"parts": [{"text": "Inspecting the workspace.", "thought": true}]}
                    }]
                }
            }),
            json!({
                "response": {
                    "candidates": [{
                        "content": {"parts": [{
                            "functionCall": {"name": "shell_command", "args": {"command": "Get-ChildItem"}}
                        }]}
                    }]
                }
            }),
            json!({
                "response": {
                    "candidates": [{
                        "finishReason": "STOP",
                        "content": {"parts": [{"text": ""}]}
                    }]
                }
            }),
        ])
        .await;

        assert!(raw.starts_with("event: response.created\ndata: "));
        let names: Vec<&str> = events
            .iter()
            .filter_map(|event| event["type"].as_str())
            .collect();
        assert_eq!(names[0], "response.created");
        assert_eq!(names[1], "response.in_progress");
        assert!(!names.contains(&"response.output_text.delta"));
        assert_eq!(
            &names[3..8],
            &[
                "response.reasoning_summary_part.added",
                "response.reasoning_summary_text.delta",
                "response.reasoning_summary_text.done",
                "response.reasoning_summary_part.done",
                "response.output_item.done",
            ]
        );
        assert!(names.contains(&"response.function_call_arguments.delta"));
        assert_eq!(names.last().copied(), Some("response.completed"));

        for (expected, event) in events.iter().enumerate() {
            assert_eq!(event["sequence_number"], expected as u64);
        }
        assert!(events
            .iter()
            .filter(|event| event["type"]
                .as_str()
                .is_some_and(|name| name.starts_with("response.reasoning_summary_")))
            .all(|event| event["summary_index"] == 0
                && event["output_index"] == 0
                && event["item_id"] == events[2]["item"]["id"]));

        let added: Vec<&Value> = events
            .iter()
            .filter(|event| event["type"] == "response.output_item.added")
            .collect();
        assert_eq!(added.len(), 2);
        assert_eq!(added[0]["item"]["type"], "reasoning");
        assert_eq!(added[0]["item"]["summary"], json!([]));
        assert_eq!(added[0]["item"]["status"], "in_progress");
        assert!(added[0]["item"]["id"]
            .as_str()
            .is_some_and(|id| id.starts_with("rs_")));
        assert_eq!(added[0]["output_index"], 0);
        assert_eq!(added[1]["item"]["type"], "function_call");
        assert_eq!(added[1]["output_index"], 1);

        let completed = events.last().expect("terminal event");
        let output = completed["response"]["output"]
            .as_array()
            .expect("completed output");
        assert_eq!(output.len(), 2);
        assert_eq!(output[0]["type"], "reasoning");
        assert_eq!(output[0]["summary"][0]["text"], "Inspecting the workspace.");
        assert_eq!(output[0], events[7]["item"]);
        assert_eq!(events[8]["item"], added[1]["item"]);
        assert_eq!(events[9]["item_id"], output[1]["id"]);
        assert_eq!(added[1]["item"]["call_id"], output[1]["call_id"]);
        assert!(!raw.contains("**Thinking**"));
        assert_eq!(output[1]["type"], "function_call");
    }

    #[tokio::test]
    async fn test_codex_final_message_is_promoted_and_persisted() {
        let (_, events) = collect_codex_stream(vec![
            json!({"candidates": [{"content": {"parts": [{"text": "Checking ", "thought": true}]}}]}),
            json!({"candidates": [{"content": {"parts": [{"text": "results.", "thought": true}]}}]}),
            json!({
                "candidates": [{
                    "content": {"parts": [{"text": "The task is complete."}]}
                }]
            }),
            json!({
                "candidates": [{
                    "finishReason": "STOP",
                    "content": {"parts": [{"text": ""}]}
                }]
            }),
        ])
        .await;

        let added = events
            .iter()
            .find(|event| {
                event["type"] == "response.output_item.added" && event["item"]["type"] == "message"
            })
            .expect("message added");
        assert_eq!(added["item"]["phase"], "commentary");

        let done = events
            .iter()
            .find(|event| {
                event["type"] == "response.output_item.done" && event["item"]["type"] == "message"
            })
            .expect("message done");
        assert_eq!(done["item"]["phase"], "final_answer");
        assert_eq!(done["item"]["content"][0]["text"], "The task is complete.");
        assert_eq!(done["item"]["content"][0]["annotations"], json!([]));
        assert_eq!(
            events[3]["part"],
            json!({"type": "summary_text", "text": ""})
        );
        assert_eq!(events[4]["delta"], "Checking ");
        assert_eq!(events[5]["delta"], "results.");
        assert_eq!(events[4]["item_id"], events[5]["item_id"]);
        assert_eq!(events[6]["text"], "Checking results.");
        assert_eq!(
            events[7]["part"],
            json!({"type": "summary_text", "text": "Checking results."})
        );
        assert_eq!(events[8]["type"], "response.output_item.done");
        assert_eq!(events[9]["item"], added["item"]);

        let terminal = events.last().expect("terminal event");
        assert_eq!(terminal["type"], "response.completed");
        assert_eq!(terminal["response"]["status"], "completed");
        assert_eq!(terminal["response"]["output"][0], events[8]["item"]);
        assert_eq!(terminal["response"]["output"][1], done["item"]);
        assert_eq!(terminal["response"]["output"][1]["phase"], "final_answer");
        assert_eq!(
            terminal["response"]["output"][1]["content"][0]["text"],
            "The task is complete."
        );
    }

    #[tokio::test]
    async fn test_codex_reasoning_summary_is_closed_at_stream_end() {
        let (_, events) = collect_codex_stream(vec![json!({
            "candidates": [{"content": {"parts": [{"text": "Checking.", "thought": true}]}}]
        })])
        .await;
        assert_eq!(events[5]["type"], "response.reasoning_summary_text.done");
        assert_eq!(events[5]["text"], "Checking.");
        assert_eq!(events[6]["type"], "response.reasoning_summary_part.done");
        assert_eq!(
            events[6]["part"],
            json!({"type": "summary_text", "text": "Checking."})
        );
        assert_eq!(events[7]["type"], "response.output_item.done");
        assert_eq!(events[7]["item"]["type"], "reasoning");
        assert_eq!(events[7]["item"]["status"], "completed");
        assert_eq!(events[7]["item"]["summary"], json!([events[6]["part"]]));
        let terminal = events.last().expect("terminal event");
        assert_eq!(terminal["response"]["output"], json!([events[7]["item"]]));
        assert_eq!(terminal["type"], "response.incomplete");
        assert_eq!(
            terminal["response"]["incomplete_details"]["reason"],
            "interrupted"
        );
        assert_eq!(terminal["response"]["error"]["code"], "empty_response");
    }

    #[tokio::test]
    async fn test_codex_late_thought_does_not_mix_into_final_answer() {
        let (raw, events) = collect_codex_stream(vec![
            json!({
                "candidates": [{
                    "content": {"parts": [{"text": "The task is complete."}]}
                }]
            }),
            json!({
                "candidates": [{
                    "content": {"parts": [{"text": "late private thought", "thought": true}]}
                }]
            }),
            json!({
                "candidates": [{
                    "finishReason": "STOP",
                    "content": {"parts": [{"text": ""}]}
                }]
            }),
        ])
        .await;

        assert!(!raw.contains("late private thought"));
        assert!(events.iter().all(|event| {
            !(event["type"] == "response.output_item.added"
                && event["item"]["id"]
                    .as_str()
                    .is_some_and(|id| id.starts_with("rs_")))
        }));
        let terminal = events.last().expect("terminal event");
        assert_eq!(terminal["type"], "response.completed");
        assert_eq!(
            terminal["response"]["output"][0]["content"][0]["text"],
            "The task is complete."
        );
    }

    #[tokio::test]
    async fn test_codex_tool_round_message_stays_commentary() {
        let (_, events) = collect_codex_stream(vec![
            json!({
                "candidates": [{
                    "content": {"parts": [{"text": "I will inspect the files first."}]}
                }]
            }),
            json!({
                "candidates": [{
                    "content": {"parts": [{
                        "functionCall": {"name": "shell_command", "args": {"command": "Get-ChildItem"}}
                    }]}
                }]
            }),
            json!({
                "candidates": [{
                    "finishReason": "STOP",
                    "content": {"parts": [{"text": ""}]}
                }]
            }),
        ])
        .await;

        let message_done = events
            .iter()
            .find(|event| {
                event["type"] == "response.output_item.done" && event["item"]["type"] == "message"
            })
            .expect("message done");
        assert_eq!(message_done["item"]["phase"], "commentary");

        let terminal = events.last().expect("terminal event");
        assert_eq!(terminal["type"], "response.completed");
        let output = terminal["response"]["output"]
            .as_array()
            .expect("completed output");
        assert_eq!(output[0]["type"], "message");
        assert_eq!(output[0]["phase"], "commentary");
        assert_eq!(output[1]["type"], "function_call");
    }

    #[tokio::test]
    async fn test_codex_empty_stop_is_incomplete_instead_of_blank_final_answer() {
        let (_, events) = collect_codex_stream(vec![json!({
            "candidates": [{
                "finishReason": "STOP",
                "content": {"parts": [{"text": ""}]}
            }]
        })])
        .await;

        assert!(events.iter().all(|event| {
            !(event["type"] == "response.output_item.added" && event["item"]["type"] == "message")
        }));
        let terminal = events.last().expect("terminal event");
        assert_eq!(terminal["type"], "response.incomplete");
        assert_eq!(terminal["response"]["status"], "incomplete");
        assert_eq!(terminal["response"]["error"]["code"], "empty_response");
    }

    #[tokio::test]
    async fn test_codex_stream_survives_transition_cliff_after_thinking() {
        // 模拟 Issue #3816: 模型输出思考块后，上游整包缓冲巨型 functionCall JSON，
        // 经历长静默。流式驱动器绝不在稳态超时后直接误判超时掐断，
        // 而是自动跃迁至 Transition 状态赋予宽限期并持续发送 : ping\n\n 保活，
        // 最终成功交付工具调用帧与 response.completed！
        let mock_stream = async_stream::stream! {
            yield Ok::<_, String>(Bytes::from(
                "data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"thought\":true,\"text\":\"Thinking about approach...\"}]}}]}}\n\n"
            ));
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            yield Ok::<_, String>(Bytes::from(
                "data: {\"response\":{\"candidates\":[{\"finishReason\":\"STOP\",\"content\":{\"parts\":[{\"functionCall\":{\"name\":\"exec_command\",\"args\":{\"cmd\":\"dir\"}}}]}}]}}\n\n"
            ));
        };
        let mut stream = create_codex_sse_stream(
            Box::pin(mock_stream),
            "gemini-4-argon-high".to_string(),
            "test-codex-cliff-session".to_string(),
            0,
            0,
            "resp-test-cliff".to_string(),
            None,
            true,
        );

        let mut events = Vec::new();
        while let Some(item) = stream.next().await {
            if let Ok(b) = item {
                let s = String::from_utf8_lossy(&b);
                for line in s.lines() {
                    if let Some(data) = line.strip_prefix("data: ") {
                        if let Ok(v) = serde_json::from_str::<Value>(data) {
                            events.push(v);
                        }
                    }
                }
            }
        }

        let completed = events
            .iter()
            .find(|e| e["type"] == "response.completed")
            .expect("response.completed must be emitted");
        assert_eq!(completed["response"]["status"], "completed");
        let outputs = completed["response"]["output"]
            .as_array()
            .expect("output array");
        assert!(outputs
            .iter()
            .any(|item| item["type"] == "function_call" && item["name"] == "exec_command"));
    }

    #[tokio::test]
    async fn test_openai_streaming_usage_only_at_end() {
        // Chunk 1: Partial content, no usage
        let chunk1_json = json!({
            "candidates": [{
                "content": {
                    "parts": [{ "text": "Hello" }]
                }
            }]
        });

        // Chunk 2: Finish reason + Usage metadata
        let chunk2_json = json!({
            "candidates": [{
                "finishReason": "STOP",
                "content": {
                    "parts": [{ "text": "" }]
                }
            }],
            "usageMetadata": {
                "promptTokenCount": 5,
                "candidatesTokenCount": 2,
                "totalTokenCount": 7
            }
        });

        // Use a helper to create the stream items compatible with the required signature
        let items: Vec<Result<Bytes, reqwest::Error>> = vec![
            Ok(Bytes::from(format!("data: {}\n\n", chunk1_json))),
            Ok(Bytes::from(format!("data: {}\n\n", chunk2_json))),
        ];

        let gemini_stream = Box::pin(stream::iter(items));

        let mut openai_stream = create_openai_sse_stream(
            gemini_stream,
            "gemini-1.5-flash".to_string(),
            "test-session".to_string(),
            0,
            None,
            false,
        );

        let mut chunks = Vec::new();
        while let Some(result) = openai_stream.next().await {
            if let Ok(bytes) = result {
                let s = String::from_utf8_lossy(&bytes).to_string();
                for line in s.lines() {
                    if line.starts_with("data: ") && !line.contains("[DONE]") {
                        chunks.push(line.to_string());
                    }
                }
            }
        }

        let mut found_usage = false;
        let mut found_finish = false;

        for (i, chunk_str) in chunks.iter().enumerate() {
            let json_str = chunk_str.trim_start_matches("data: ").trim();
            let json: Value = serde_json::from_str(json_str).unwrap();

            if i < chunks.len() - 1 {
                assert!(
                    json.get("usage").is_none(),
                    "Usage should not be in intermediate chunks. Found in chunk {}",
                    i
                );
            } else {
                if let Some(usage) = json.get("usage") {
                    found_usage = true;
                    assert_eq!(usage["prompt_tokens"], 5);
                    assert_eq!(usage["completion_tokens"], 2);
                    assert_eq!(usage["total_tokens"], 7);
                }
                if let Some(choices) = json.get("choices") {
                    if let Some(choice) = choices.get(0) {
                        if let Some(finish_reason) = choice.get("finish_reason") {
                            if finish_reason.as_str() == Some("stop") {
                                found_finish = true;
                            }
                        }
                    }
                }
            }
        }
        assert!(found_usage, "Usage should be found in the last chunk");
        assert!(found_finish, "Finish reason should be strictly 'stop'");
    }

    #[tokio::test]
    async fn test_openai_streaming_with_include_usage_true() {
        let chunk1_json = json!({
            "candidates": [{
                "content": {
                    "parts": [{ "text": "Hello" }]
                }
            }]
        });

        let chunk2_json = json!({
            "candidates": [{
                "finishReason": "STOP",
                "content": {
                    "parts": [{ "text": " world" }]
                }
            }],
            "usageMetadata": {
                "promptTokenCount": 10,
                "candidatesTokenCount": 5,
                "totalTokenCount": 15
            }
        });

        let items: Vec<Result<Bytes, reqwest::Error>> = vec![
            Ok(Bytes::from(format!("data: {}\n\n", chunk1_json))),
            Ok(Bytes::from(format!("data: {}\n\n", chunk2_json))),
        ];

        let gemini_stream = Box::pin(stream::iter(items));

        let mut openai_stream = create_openai_sse_stream(
            gemini_stream,
            "gemini-1.5-flash".to_string(),
            "test-session".to_string(),
            0,
            None,
            true, // include_usage = true
        );

        let mut chunks = Vec::new();
        while let Some(result) = openai_stream.next().await {
            if let Ok(bytes) = result {
                let s = String::from_utf8_lossy(&bytes).to_string();
                for line in s.lines() {
                    if line.starts_with("data: ") && !line.contains("[DONE]") {
                        chunks.push(line.to_string());
                    }
                }
            }
        }

        // With include_usage: true, the last chunk before [DONE] MUST have choices: [] and usage
        assert!(
            chunks.len() >= 3,
            "Expected at least 3 chunks: partial, finish, usage"
        );
        let last_chunk: Value =
            serde_json::from_str(chunks.last().unwrap().trim_start_matches("data: ").trim())
                .unwrap();
        assert_eq!(last_chunk["choices"], json!([]));
        assert!(
            last_chunk.get("usage").is_some(),
            "Standalone usage chunk must contain usage"
        );
        let usage = &last_chunk["usage"];
        assert_eq!(usage["prompt_tokens"], 10);
        assert_eq!(usage["completion_tokens"], 5);
        assert_eq!(usage["total_tokens"], 15);
    }

    #[tokio::test]
    async fn test_hermes_stream_without_include_usage_never_emits_empty_choices() {
        // Simulates real Gemini streaming where finishReason arrives in chunk 1,
        // and usageMetadata arrives in chunk 2 without candidates.
        let chunk1_json = json!({
            "candidates": [{
                "finishReason": "STOP",
                "content": {
                    "parts": [{ "text": "Task complete." }]
                }
            }]
        });

        let chunk2_json = json!({
            "usageMetadata": {
                "promptTokenCount": 20,
                "candidatesTokenCount": 8,
                "totalTokenCount": 28
            }
        });

        let items: Vec<Result<Bytes, reqwest::Error>> = vec![
            Ok(Bytes::from(format!("data: {}\n\n", chunk1_json))),
            Ok(Bytes::from(format!("data: {}\n\n", chunk2_json))),
        ];

        let gemini_stream = Box::pin(stream::iter(items));

        // Hermes / standard OpenAI Python SDK default: include_usage = false
        let mut openai_stream = create_openai_sse_stream(
            gemini_stream,
            "gemini-1.5-flash".to_string(),
            "hermes-session".to_string(),
            0,
            None,
            false,
        );

        let mut chunks = Vec::new();
        while let Some(result) = openai_stream.next().await {
            if let Ok(bytes) = result {
                let s = String::from_utf8_lossy(&bytes).to_string();
                for line in s.lines() {
                    if line.starts_with("data: ") && !line.contains("[DONE]") {
                        chunks.push(line.to_string());
                    }
                }
            }
        }

        // CRITICAL: Ensure NO chunk has empty choices: []!
        // Hermes iterates `chunk.choices[0]`. An empty choices: [] chunk crashes Hermes with IndexError!
        for chunk_str in &chunks {
            let json_str = chunk_str.trim_start_matches("data: ").trim();
            let json: Value = serde_json::from_str(json_str).unwrap();
            if let Some(choices) = json.get("choices").and_then(|c| c.as_array()) {
                assert!(
                    !choices.is_empty(),
                    "Crash hazard! Found empty choices: [] chunk when include_usage=false: {}",
                    json_str
                );
            }
        }
    }

    #[tokio::test]
    async fn test_openai_streaming_reasoning_content() {
        // Chunk with thought part
        let chunk_json = json!({
            "candidates": [{
                "content": {
                    "parts": [
                        { "text": "Thinking...", "thought": true },
                        { "text": "Hello world" }
                    ]
                }
            }]
        });

        let items: Vec<Result<Bytes, reqwest::Error>> =
            vec![Ok(Bytes::from(format!("data: {}\n\n", chunk_json)))];

        let gemini_stream = Box::pin(stream::iter(items));

        let mut openai_stream = create_openai_sse_stream(
            gemini_stream,
            "gemini-1.5-flash".to_string(),
            "test-session".to_string(),
            0,
            None,
            false,
        );

        let mut chunks = Vec::new();
        while let Some(result) = openai_stream.next().await {
            if let Ok(bytes) = result {
                let s = String::from_utf8_lossy(&bytes).to_string();
                for line in s.lines() {
                    if line.starts_with("data: ") && !line.contains("[DONE]") {
                        chunks.push(line.to_string());
                    }
                }
            }
        }

        let mut has_reasoning = false;
        let mut has_content = false;

        for chunk_str in &chunks {
            let json_str = chunk_str.trim_start_matches("data: ").trim();
            let json: Value = serde_json::from_str(json_str).unwrap();

            if let Some(choices) = json.get("choices") {
                if let Some(choice) = choices.get(0) {
                    if let Some(delta) = choice.get("delta") {
                        if let Some(rc) = delta.get("reasoning_content") {
                            assert_eq!(rc.as_str().unwrap(), "Thinking...");
                            has_reasoning = true;
                            // content should be null or not match thinking process
                            if let Some(content) = delta.get("content") {
                                assert!(content.is_null());
                            }
                        }
                        if let Some(c) = delta.get("content") {
                            if c.is_string() {
                                assert_eq!(c.as_str().unwrap(), "Hello world");
                                has_content = true;
                                assert!(delta.get("reasoning_content").is_none());
                            }
                        }
                    }
                }
            }
        }

        assert!(has_reasoning, "Should stream reasoning_content");
        assert!(has_content, "Should stream content");
    }

    #[tokio::test]
    async fn test_streaming_malformed_function_call_never_injects_hardcoded_online_prompt() {
        let chunk_json = json!({
            "candidates": [{
                "content": {
                    "parts": [
                        { "text": "Reasoning about weather...", "thought": true }
                    ]
                },
                "finishReason": "MALFORMED_FUNCTION_CALL"
            }]
        });

        let items: Vec<Result<Bytes, reqwest::Error>> =
            vec![Ok(Bytes::from(format!("data: {}\n\n", chunk_json)))];

        let gemini_stream = Box::pin(stream::iter(items));

        let mut openai_stream = create_openai_sse_stream(
            gemini_stream,
            "gemini-3.7-flash".to_string(),
            "test-malformed-session".to_string(),
            0,
            None,
            false,
        );

        let mut all_content = String::new();
        let mut final_finish_reason: Option<String> = None;

        while let Some(result) = openai_stream.next().await {
            if let Ok(bytes) = result {
                let s = String::from_utf8_lossy(&bytes).to_string();
                for line in s.lines() {
                    if line.starts_with("data: ") && !line.contains("[DONE]") {
                        let json_str = line.trim_start_matches("data: ").trim();
                        if let Ok(json) = serde_json::from_str::<Value>(json_str) {
                            if let Some(choices) = json.get("choices").and_then(|c| c.as_array()) {
                                if let Some(choice) = choices.first() {
                                    if let Some(delta) = choice.get("delta") {
                                        if let Some(c) =
                                            delta.get("content").and_then(|c| c.as_str())
                                        {
                                            all_content.push_str(c);
                                        }
                                    }
                                    if let Some(fr) =
                                        choice.get("finish_reason").and_then(|f| f.as_str())
                                    {
                                        final_finish_reason = Some(fr.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // 验证绝对不会被注入任何臆测性的天气/联网假文本
        assert!(
            all_content.is_empty(),
            "Expected empty content, got: {}",
            all_content
        );
        assert_eq!(final_finish_reason, Some("stop".to_string()));
    }

    #[tokio::test]
    async fn test_openai_stream_emits_fallback_finish_reason_when_upstream_omits_it() {
        // [Fixes #3716] 模拟上游只吐了文本且随流关闭正常 EOF，未显式发送 finishReason 候选块
        let chunk1_json = json!({
            "candidates": [{
                "content": {
                    "parts": [{ "text": "Hello, this is a response." }]
                }
            }]
        });

        // 第二帧只发送 usage，没有 candidates
        let chunk2_json = json!({
            "usageMetadata": {
                "promptTokenCount": 15,
                "candidatesTokenCount": 6,
                "totalTokenCount": 21
            }
        });

        let items: Vec<Result<Bytes, reqwest::Error>> = vec![
            Ok(Bytes::from(format!("data: {}\n\n", chunk1_json))),
            Ok(Bytes::from(format!("data: {}\n\n", chunk2_json))),
        ];

        let gemini_stream = Box::pin(stream::iter(items));

        let mut openai_stream = create_openai_sse_stream(
            gemini_stream,
            "gemini-3.8-flash-high".to_string(),
            "session-3716".to_string(),
            0,
            None,
            false,
        );

        let mut finish_reasons = Vec::new();
        let mut got_done = false;

        while let Some(result) = openai_stream.next().await {
            if let Ok(bytes) = result {
                let s = String::from_utf8_lossy(&bytes).to_string();
                for line in s.lines() {
                    if line.starts_with("data: ") {
                        if line.contains("[DONE]") {
                            got_done = true;
                        } else {
                            let json_str = line.trim_start_matches("data: ").trim();
                            if let Ok(json) = serde_json::from_str::<Value>(json_str) {
                                if let Some(choices) =
                                    json.get("choices").and_then(|c| c.as_array())
                                {
                                    for choice in choices {
                                        if let Some(fr) =
                                            choice.get("finish_reason").and_then(|f| f.as_str())
                                        {
                                            finish_reasons.push(fr.to_string());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        assert!(got_done, "Stream must terminate with [DONE]");
        // 关键断言：即使上游未显式发送 finishReason，网关在终态必须补发带有 finish_reason: 'stop' 的收尾块
        assert_eq!(
            finish_reasons,
            vec!["stop".to_string()],
            "Must emit finish_reason 'stop' before stream end to avoid finish=unknown"
        );
    }

    #[tokio::test]
    async fn test_openai_stream_abnormal_interruption_preserves_content_without_tampering() {
        let chunk1_json = json!({
            "candidates": [{
                "content": {
                    "parts": [{ "text": "Here is code:\n```python\nprint(123)" }]
                }
            }]
        });

        let items: Vec<Result<Bytes, String>> = vec![
            Ok(Bytes::from(format!("data: {}\n\n", chunk1_json))),
            Err("error reading a body from connection: connection reset by peer".to_string()),
        ];

        let gemini_stream = Box::pin(stream::iter(items));

        let mut openai_stream = create_openai_sse_stream(
            gemini_stream,
            "gemini-3.8-flash-high".to_string(),
            "session-abnormal".to_string(),
            0,
            None,
            false,
        );

        let mut all_chunks = Vec::new();
        let mut got_done = false;
        let mut finish_reasons = Vec::new();

        while let Some(result) = openai_stream.next().await {
            if let Ok(bytes) = result {
                let s = String::from_utf8_lossy(&bytes).to_string();
                for line in s.lines() {
                    if line.starts_with("data: ") {
                        if line.contains("[DONE]") {
                            got_done = true;
                        } else {
                            let json_str = line.trim_start_matches("data: ").trim();
                            if let Ok(json) = serde_json::from_str::<Value>(json_str) {
                                all_chunks.push(json.clone());
                                if let Some(choices) =
                                    json.get("choices").and_then(|c| c.as_array())
                                {
                                    for choice in choices {
                                        if let Some(fr) =
                                            choice.get("finish_reason").and_then(|f| f.as_str())
                                        {
                                            finish_reasons.push(fr.to_string());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        assert!(got_done, "Stream must terminate with [DONE]");
        assert_eq!(
            finish_reasons,
            vec!["length".to_string()],
            "Abnormal drop must emit finish_reason: 'length'"
        );

        let all_text: String = all_chunks
            .iter()
            .filter_map(|json| {
                json.get("choices")
                    .and_then(|c| c.get(0))
                    .and_then(|c| c.get("delta"))
                    .and_then(|d| d.get("content"))
                    .and_then(|t| t.as_str())
            })
            .collect();

        assert_eq!(all_text, "Here is code:\n```python\nprint(123)");
        // 绝不注入虚假代码围栏与横幅
        assert!(!all_text.contains("网关提示"));
        assert!(!all_text.contains("Gateway Notice"));
    }

    #[tokio::test]
    async fn test_openai_stream_abnormal_interruption_preserves_tool_calls() {
        let chunk1_json = json!({
            "candidates": [{
                "content": {
                    "parts": [{
                        "functionCall": {
                            "name": "run_bash",
                            "args": { "command": "cargo check" }
                        }
                    }]
                }
            }]
        });

        let items: Vec<Result<Bytes, String>> = vec![
            Ok(Bytes::from(format!("data: {}\n\n", chunk1_json))),
            Err("error reading a body: connection reset by peer".to_string()),
        ];

        let gemini_stream = Box::pin(stream::iter(items));

        let mut openai_stream = create_openai_sse_stream(
            gemini_stream,
            "gemini-3.8-flash-high".to_string(),
            "session-abnormal-tools".to_string(),
            0,
            None,
            false,
        );

        let mut finish_reasons = Vec::new();
        while let Some(result) = openai_stream.next().await {
            if let Ok(bytes) = result {
                let s = String::from_utf8_lossy(&bytes).to_string();
                for line in s.lines() {
                    if line.starts_with("data: ") && !line.contains("[DONE]") {
                        let json_str = line.trim_start_matches("data: ").trim();
                        if let Ok(json) = serde_json::from_str::<Value>(json_str) {
                            if let Some(choices) = json.get("choices").and_then(|c| c.as_array()) {
                                for choice in choices {
                                    if let Some(fr) =
                                        choice.get("finish_reason").and_then(|f| f.as_str())
                                    {
                                        finish_reasons.push(fr.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // 工具调用断流时必须保证终结原因为 "tool_calls"，绝不能降级为 "length"
        assert_eq!(
            finish_reasons,
            vec!["tool_calls".to_string()],
            "Tool calls must preserve finish_reason: 'tool_calls'"
        );
    }

    #[tokio::test]
    async fn test_openai_stream_completed_stream_does_not_double_finalize() {
        let chunk1_json = json!({
            "candidates": [{
                "content": {
                    "parts": [{ "text": "All done." }]
                },
                "finishReason": "STOP"
            }]
        });

        let items: Vec<Result<Bytes, String>> = vec![
            Ok(Bytes::from(format!("data: {}\n\n", chunk1_json))),
            Err("connection reset after stop".to_string()),
        ];

        let gemini_stream = Box::pin(stream::iter(items));

        let mut openai_stream = create_openai_sse_stream(
            gemini_stream,
            "gemini-3.8-flash-high".to_string(),
            "session-already-done".to_string(),
            0,
            None,
            false,
        );

        let mut finish_reasons = Vec::new();
        while let Some(result) = openai_stream.next().await {
            if let Ok(bytes) = result {
                let s = String::from_utf8_lossy(&bytes).to_string();
                for line in s.lines() {
                    if line.starts_with("data: ") && !line.contains("[DONE]") {
                        let json_str = line.trim_start_matches("data: ").trim();
                        if let Ok(json) = serde_json::from_str::<Value>(json_str) {
                            if let Some(choices) = json.get("choices").and_then(|c| c.as_array()) {
                                for choice in choices {
                                    if let Some(fr) =
                                        choice.get("finish_reason").and_then(|f| f.as_str())
                                    {
                                        finish_reasons.push(fr.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // 已正常下发 stop 的流，绝不可重复产生第二个 finish_reason (如 length)
        assert_eq!(
            finish_reasons,
            vec!["stop".to_string()],
            "Completed stream must not emit duplicate finish_reason"
        );
    }
}
