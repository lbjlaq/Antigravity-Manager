//! Request/response payload audit helpers.
//!
//! - Header serialization with API-key redaction
//! - Session / thinking markers are preserved for ops comparison
//! - Optional concise storage mode to keep SQLite small

use axum::http::HeaderMap;
use serde_json::{json, Map, Value};

const THINKING_SESSION_HEADERS: &[&str] = &[
    "x-session-id",
    "x-antigravity-session-id",
    "x-conversation-id",
    "conversation-id",
    "x-chat-id",
    "chat-id",
    "x-thread-id",
    "thread-id",
    "x-client-session-id",
    "x-cursor-session-id",
    "cursor-session-id",
    "x-vscode-session-id",
    "anthropic-session-id",
    "mcp-session-id",
];

fn is_thinking_session_header(name: &str) -> bool {
    THINKING_SESSION_HEADERS
        .iter()
        .any(|h| name.eq_ignore_ascii_case(h))
}

fn is_sensitive_header(name: &str) -> bool {
    if is_thinking_session_header(name) {
        return false;
    }
    let n = name.to_ascii_lowercase();
    matches!(
        n.as_str(),
        "authorization"
            | "proxy-authorization"
            | "x-api-key"
            | "api-key"
            | "x-goog-api-key"
            | "anthropic-api-key"
            | "x-auth-token"
            | "x-access-token"
            | "cookie"
            | "set-cookie"
    ) || n.contains("api-key")
        || n.contains("apikey")
        || n.contains("access-token")
        || n.contains("access_token")
        || (n.contains("token") && !n.contains("session") && !n.contains("count"))
}

pub fn redact_header_value(name: &str, value: &str) -> String {
    if is_thinking_session_header(name) {
        return value.to_string();
    }
    if !is_sensitive_header(name) {
        return value.to_string();
    }
    let trimmed = value.trim();
    if trimmed
        .get(..7)
        .map_or(false, |p| p.eq_ignore_ascii_case("bearer "))
    {
        return "Bearer ***REDACTED***".to_string();
    }
    "***REDACTED***".to_string()
}

pub fn headers_to_redacted_json(headers: &HeaderMap) -> String {
    header_pairs_to_redacted_json(
        headers
            .iter()
            .filter_map(|(k, v)| v.to_str().ok().map(|s| (k.as_str(), s))),
    )
}

pub fn header_pairs_to_redacted_json<'a, I>(pairs: I) -> String
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    let mut map = Map::new();
    for (key, raw) in pairs {
        let redacted = redact_header_value(key, raw);
        match map.get_mut(key) {
            Some(Value::Array(arr)) => arr.push(json!(redacted)),
            Some(existing) => {
                let prev = existing.clone();
                *existing = json!([prev, redacted]);
            }
            None => {
                map.insert(key.to_string(), json!(redacted));
            }
        }
    }
    serde_json::to_string(&Value::Object(map)).unwrap_or_else(|_| "{}".to_string())
}

fn truncate_chars(s: &str, max: usize) -> String {
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i >= max {
            out.push('…');
            return out;
        }
        out.push(ch);
    }
    out
}

fn simplify_part(part: &Value) -> Value {
    let mut obj = Map::new();
    if let Some(thought) = part.get("thought") {
        obj.insert("thought".into(), thought.clone());
    }
    if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
        obj.insert("text".into(), json!(text));
    }
    for sig_key in [
        "thoughtSignature",
        "thought_signature",
        "signature",
        "thinking_signature",
    ] {
        if let Some(sig) = part.get(sig_key) {
            obj.insert(sig_key.to_string(), sig.clone());
        }
    }
    if let Some(fc) = part.get("functionCall") {
        let mut fc_out = Map::new();
        if let Some(name) = fc.get("name") {
            fc_out.insert("name".into(), name.clone());
        }
        if let Some(id) = fc.get("id") {
            fc_out.insert("id".into(), id.clone());
        }
        if let Some(args) = fc.get("args") {
            fc_out.insert("args".into(), args.clone());
        }
        obj.insert("functionCall".into(), Value::Object(fc_out));
    }
    if let Some(fr) = part.get("functionResponse") {
        let mut fr_out = Map::new();
        if let Some(name) = fr.get("name") {
            fr_out.insert("name".into(), name.clone());
        }
        if let Some(id) = fr.get("id") {
            fr_out.insert("id".into(), id.clone());
        }
        if let Some(resp) = fr.get("response") {
            fr_out.insert("response".into(), resp.clone());
        }
        obj.insert("functionResponse".into(), Value::Object(fr_out));
    }
    if let Some(inline) = part.get("inlineData").or_else(|| part.get("inline_data")) {
        let mime = inline
            .get("mimeType")
            .or_else(|| inline.get("mime_type"))
            .cloned()
            .unwrap_or(json!("unknown"));
        let data_len = inline
            .get("data")
            .and_then(|d| d.as_str())
            .map(|s| s.len())
            .unwrap_or(0);
        obj.insert(
            "inlineData".into(),
            json!({
                "mimeType": mime,
                "data": format!("[base64 image: {} bytes]", data_len),
            }),
        );
    }
    if obj.is_empty() {
        return part.clone();
    }
    Value::Object(obj)
}

fn simplify_message(msg: &Value) -> Value {
    let mut out = Map::new();
    if let Some(role) = msg.get("role") {
        out.insert("role".into(), role.clone());
    }
    if let Some(name) = msg.get("name") {
        out.insert("name".into(), name.clone());
    }
    if let Some(tool_call_id) = msg.get("tool_call_id") {
        out.insert("tool_call_id".into(), tool_call_id.clone());
    }
    if let Some(parts) = msg.get("parts").and_then(|p| p.as_array()) {
        out.insert(
            "parts".into(),
            Value::Array(parts.iter().map(simplify_part).collect()),
        );
    }
    if let Some(content) = msg.get("content") {
        out.insert("content".into(), simplify_content(content));
    }
    if let Some(thinking) = msg.get("thinking") {
        out.insert("thinking".into(), thinking.clone());
    }
    if let Some(rc) = msg.get("reasoning_content") {
        out.insert("reasoning_content".into(), rc.clone());
    }
    for sig_key in [
        "thinking_signature",
        "thought_signature",
        "signature",
        "thoughtSignature",
    ] {
        if let Some(sig) = msg.get(sig_key) {
            out.insert(sig_key.to_string(), sig.clone());
        }
    }
    if let Some(tool_calls) = msg.get("tool_calls").and_then(|t| t.as_array()) {
        out.insert(
            "tool_calls".into(),
            Value::Array(
                tool_calls
                    .iter()
                    .map(|tc| {
                        if let Some(tc_obj) = tc.as_object() {
                            let mut tc_out = tc_obj.clone();
                            if let Some(func) = tc_obj.get("function") {
                                tc_out.insert("function".into(), func.clone());
                            }
                            Value::Object(tc_out)
                        } else {
                            tc.clone()
                        }
                    })
                    .collect(),
            ),
        );
    }
    Value::Object(out)
}

/// OpenAI Responses 的 `input` 项结构与 chat message 不同（`type` / `call_id` / `arguments` /
/// `output` 等），单独裁剪：沿用 `simplify_message` 的体积控制，再补回调试必需的字段。
fn simplify_responses_input_item(item: &Value) -> Value {
    let mut out = simplify_message(item);
    if let Some(obj) = out.as_object_mut() {
        for key in ["type", "role", "call_id", "arguments", "output"] {
            if let Some(v) = item.get(key) {
                obj.insert(key.to_string(), v.clone());
            }
        }
    }
    out
}

fn simplify_content(content: &Value) -> Value {
    match content {
        Value::String(s) => Value::String(s.clone()),
        Value::Array(arr) => Value::Array(
            arr.iter()
                .map(|block| {
                    if let Some(obj) = block.as_object() {
                        let mut slim = Map::new();
                        if let Some(t) = obj.get("type") {
                            slim.insert("type".into(), t.clone());
                        }
                        if let Some(text) = obj.get("text") {
                            slim.insert("text".into(), text.clone());
                        }
                        if let Some(thinking) = obj.get("thinking") {
                            slim.insert("thinking".into(), thinking.clone());
                        }
                        for sig_key in [
                            "signature",
                            "thoughtSignature",
                            "thinking_signature",
                            "thought_signature",
                        ] {
                            if let Some(sig) = obj.get(sig_key) {
                                slim.insert(sig_key.to_string(), sig.clone());
                            }
                        }
                        if let Some(id) = obj.get("id") {
                            slim.insert("id".into(), id.clone());
                        }
                        if let Some(name) = obj.get("name") {
                            slim.insert("name".into(), name.clone());
                        }
                        if let Some(input) = obj.get("input") {
                            slim.insert("input".into(), input.clone());
                        }
                        if let Some(tool_use_id) = obj.get("tool_use_id") {
                            slim.insert("tool_use_id".into(), tool_use_id.clone());
                        }
                        if let Some(is_error) = obj.get("is_error") {
                            slim.insert("is_error".into(), is_error.clone());
                        }
                        if let Some(source) = obj.get("source") {
                            if source.get("type").and_then(|t| t.as_str()) == Some("base64") {
                                let media_type = source
                                    .get("media_type")
                                    .cloned()
                                    .unwrap_or(json!("unknown"));
                                let len = source
                                    .get("data")
                                    .and_then(|d| d.as_str())
                                    .map(|s| s.len())
                                    .unwrap_or(0);
                                slim.insert(
                                    "source".into(),
                                    json!({
                                        "type": "base64",
                                        "media_type": media_type,
                                        "data": format!("[base64 image: {} bytes]", len)
                                    }),
                                );
                            } else {
                                slim.insert("source".into(), source.clone());
                            }
                        }
                        if let Some(img_url) = obj.get("image_url") {
                            if let Some(url_str) = img_url.get("url").and_then(|u| u.as_str()) {
                                if url_str.starts_with("data:") {
                                    let comma_pos = url_str.find(',').unwrap_or(0);
                                    let header = &url_str[..comma_pos];
                                    let len = url_str.len() - comma_pos;
                                    slim.insert("image_url".into(), json!({
                                        "url": format!("{},[base64 image: {} bytes]", header, len)
                                    }));
                                } else {
                                    slim.insert("image_url".into(), img_url.clone());
                                }
                            } else {
                                slim.insert("image_url".into(), img_url.clone());
                            }
                        }
                        if slim.is_empty() {
                            Value::Object(obj.clone())
                        } else {
                            Value::Object(slim)
                        }
                    } else {
                        block.clone()
                    }
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

/// 完整保留 tools Schema (包含所有的 functionDeclarations, parameters, googleSearch 等)，确保运维排查时能看到完整的工具定义与参数结构
fn simplify_tools(tools: &Value) -> Value {
    tools.clone()
}

/// 精简报文字段的**展示顺序**：按用户 / 开发者关注度从高到低排列。
/// 未列入的字段按其原有相对顺序追加在后面（依赖 `serde_json` 的 `preserve_order`）。
/// 目标：模型 → 思考块 → 上下文 → 用量/缓存 → 工具调用 → 其余，四协议行为一致。
const CORE_FIELD_ORDER: [&str; 22] = [
    // 1. 模型标识
    "model",
    // 2. 思考块：开关 / 强度 / 内容 / 签名
    "thinking",
    "reasoning_effort",
    "reasoning",
    "reasoning_content",
    "generationConfig",
    "generation_config",
    "thinking_signature",
    "thought_signature",
    "signature",
    "thoughtSignature",
    "_session_thinking_id",
    // 3. 上下文：系统提示词与对话
    "system",
    "systemInstruction",
    "instructions",
    "messages",
    "contents",
    "input",
    // 4. 用量与缓存
    "usage",
    "usageMetadata",
    // 5. 工具调用（关注度次之）
    "tool_calls",
    "tools",
];

/// 按关注度重排顶层字段 —— **只调整顺序，不删减任何字段**。
/// 四协议与调试日志共用，保证展示顺序一致；`request` 包装层递归重排并保持在前。
pub fn reorder_payload_fields(value: &Value) -> Value {
    let Value::Object(map) = value else {
        return value.clone();
    };
    let mut ordered = Map::new();
    let mut rest: Map<String, Value> = Map::new();
    for (k, v) in map {
        if k == "request" {
            ordered.insert(k.clone(), reorder_payload_fields(v));
        } else {
            rest.insert(k.clone(), v.clone());
        }
    }
    for key in CORE_FIELD_ORDER {
        if let Some(v) = rest.remove(key) {
            ordered.insert(key.to_string(), v);
        }
    }
    for (k, v) in rest {
        ordered.insert(k, v);
    }
    Value::Object(ordered)
}

pub fn simplify_payload_json(value: &Value) -> Value {
    let inner = value.get("request").unwrap_or(value);
    let mut concise = Map::new();

    // 1. 标识符与会话路由
    for key in [
        "_session_thinking_id",
        "sessionId",
        "session_id",
        "conversation_id",
        "chat_id",
        "thread_id",
        "previous_response_id",
        "requestId",
        "request_id",
        "id",
        "model",
        "stream",
        "temperature",
        "top_p",
        "top_k",
        "max_tokens",
        "max_output_tokens",
        "thinking",
        "reasoning_effort",
        "reasoning",
        "reasoning_content",
        "error",
        "gateway_error",
        "upstream_error",
        "type",
        "code",
        "status",
    ] {
        if let Some(v) = inner.get(key).or_else(|| value.get(key)) {
            concise.insert(key.to_string(), v.clone());
        }
    }

    // 2. 思考签名与 Thinking
    for sig_key in [
        "thinking_signature",
        "thought_signature",
        "signature",
        "thoughtSignature",
    ] {
        if let Some(v) = inner.get(sig_key).or_else(|| value.get(sig_key)) {
            concise.insert(sig_key.to_string(), v.clone());
        }
    }

    // 3. 正式回复与顶层 content
    if let Some(content) = inner.get("content").or_else(|| value.get("content")) {
        concise.insert("content".into(), simplify_content(content));
    }

    // 4. 顶层 tool_calls
    if let Some(tool_calls) = inner.get("tool_calls").or_else(|| value.get("tool_calls")) {
        if let Some(arr) = tool_calls.as_array() {
            concise.insert(
                "tool_calls".into(),
                Value::Array(
                    arr.iter()
                        .map(|tc| {
                            if let Some(tc_obj) = tc.as_object() {
                                let mut tc_out = tc_obj.clone();
                                if let Some(func) = tc_obj.get("function") {
                                    tc_out.insert("function".into(), func.clone());
                                }
                                Value::Object(tc_out)
                            } else {
                                tc.clone()
                            }
                        })
                        .collect(),
                ),
            );
        } else {
            concise.insert("tool_calls".into(), tool_calls.clone());
        }
    }

    // 5. System Instructions
    if let Some(sys) = inner.get("system").or_else(|| value.get("system")) {
        concise.insert("system".into(), sys.clone());
    }
    if let Some(sys) = inner
        .get("systemInstruction")
        .or_else(|| value.get("systemInstruction"))
    {
        concise.insert("systemInstruction".into(), sys.clone());
    }

    // 5.5 OpenAI Responses 协议：instructions（系统指令）与 input（对话上下文）
    //     这两个是 Responses / Codex 的核心载荷，且不是 messages 的别名 —— 不保留会让
    //     日志里"系统提示词块与上下文块整段消失"（客户端原文看不到，而中转报文是 Gemini
    //     格式故看起来完整）。
    if let Some(instructions) = inner
        .get("instructions")
        .or_else(|| value.get("instructions"))
    {
        concise.insert("instructions".into(), instructions.clone());
    }
    if let Some(input) = inner.get("input").or_else(|| value.get("input")) {
        concise.insert(
            "input".into(),
            match input {
                Value::Array(arr) => {
                    Value::Array(arr.iter().map(simplify_responses_input_item).collect())
                }
                other => other.clone(),
            },
        );
    }

    // 6. Configs (generationConfig, thinkingConfig, toolConfig, tool_config, safetySettings)
    if let Some(cfg) = inner
        .get("generationConfig")
        .or_else(|| value.get("generationConfig"))
    {
        concise.insert("generationConfig".into(), cfg.clone());
    }
    if let Some(cfg) = inner
        .get("generation_config")
        .or_else(|| value.get("generation_config"))
    {
        concise.insert("generation_config".into(), cfg.clone());
    }
    if let Some(cfg) = inner.get("toolConfig").or_else(|| value.get("toolConfig")) {
        concise.insert("toolConfig".into(), cfg.clone());
    }
    if let Some(cfg) = inner
        .get("tool_config")
        .or_else(|| value.get("tool_config"))
    {
        concise.insert("tool_config".into(), cfg.clone());
    }
    if let Some(tc) = inner
        .get("tool_choice")
        .or_else(|| value.get("tool_choice"))
    {
        concise.insert("tool_choice".into(), tc.clone());
    }
    if let Some(ss) = inner
        .get("safetySettings")
        .or_else(|| value.get("safetySettings"))
    {
        concise.insert("safetySettings".into(), ss.clone());
    }

    // 7. Messages & Contents
    if let Some(messages) = inner.get("messages").or_else(|| value.get("messages")) {
        if let Some(arr) = messages.as_array() {
            concise.insert(
                "messages".into(),
                Value::Array(arr.iter().map(simplify_message).collect()),
            );
        }
    }
    if let Some(contents) = inner.get("contents").or_else(|| value.get("contents")) {
        if let Some(arr) = contents.as_array() {
            concise.insert(
                "contents".into(),
                Value::Array(arr.iter().map(simplify_message).collect()),
            );
        }
    }

    // 8. Tools (完整 Schema)
    if let Some(tools) = inner.get("tools").or_else(|| value.get("tools")) {
        concise.insert("tools".into(), simplify_tools(tools));
    }

    // 9. Usage Statistics
    if let Some(usage) = inner.get("usage").or_else(|| value.get("usage")) {
        concise.insert("usage".into(), usage.clone());
    }
    if let Some(usage) = inner
        .get("usageMetadata")
        .or_else(|| value.get("usageMetadata"))
    {
        concise.insert("usageMetadata".into(), usage.clone());
    }

    // 9.5 耗时诊断指标 (_timing)
    if let Some(timing) = inner.get("_timing").or_else(|| value.get("_timing")) {
        concise.insert("_timing".into(), timing.clone());
    }

    // 10. Choices & Candidates
    if let Some(choices) = value.get("choices").or_else(|| inner.get("choices")) {
        if let Some(arr) = choices.as_array() {
            concise.insert(
                "choices".into(),
                Value::Array(
                    arr.iter()
                        .map(|choice| {
                            if let Some(obj) = choice.as_object() {
                                let mut choice_out = obj.clone();
                                if let Some(msg) = obj.get("message") {
                                    choice_out.insert("message".into(), simplify_message(msg));
                                }
                                if let Some(delta) = obj.get("delta") {
                                    choice_out.insert("delta".into(), simplify_message(delta));
                                }
                                Value::Object(choice_out)
                            } else {
                                choice.clone()
                            }
                        })
                        .collect(),
                ),
            );
        } else {
            concise.insert("choices".into(), choices.clone());
        }
    }
    if let Some(candidates) = inner.get("candidates").or_else(|| value.get("candidates")) {
        if let Some(arr) = candidates.as_array() {
            concise.insert(
                "candidates".into(),
                Value::Array(
                    arr.iter()
                        .map(|cand| {
                            if let Some(obj) = cand.as_object() {
                                let mut cand_out = obj.clone();
                                if let Some(content) = obj.get("content") {
                                    cand_out.insert("content".into(), simplify_message(content));
                                }
                                Value::Object(cand_out)
                            } else {
                                cand.clone()
                            }
                        })
                        .collect(),
                ),
            );
        } else {
            concise.insert("candidates".into(), candidates.clone());
        }
    }

    if concise.is_empty() {
        return simplify_unknown(value);
    }

    // 按关注度重排字段：核心信息前置；未列入 CORE_FIELD_ORDER 的字段保持原相对顺序追加
    reorder_payload_fields(&Value::Object(concise))
}

fn simplify_unknown(value: &Value) -> Value {
    match value {
        Value::String(s) => Value::String(s.clone()),
        Value::Array(arr) => Value::Array(arr.iter().take(50).map(simplify_unknown).collect()),
        Value::Object(map) => {
            let mut out = Map::new();
            for (k, v) in map.iter().take(50) {
                out.insert(k.clone(), simplify_unknown(v));
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

pub fn apply_storage_mode_to_body(raw: Option<String>, mode: &str) -> Option<String> {
    let raw = raw?;
    if mode != "simple" {
        return Some(raw);
    }
    match serde_json::from_str::<Value>(&raw) {
        Ok(json) => serde_json::to_string_pretty(&simplify_payload_json(&json))
            .ok()
            .or(Some(raw)),
        Err(_) => Some(truncate_chars(&raw, 8000)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn redacts_api_keys_but_keeps_session_markers() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer sk-secret-customer-key"),
        );
        headers.insert("x-api-key", HeaderValue::from_static("sk-another"));
        headers.insert(
            "x-session-id",
            HeaderValue::from_static("sess-ops-compare-001"),
        );
        headers.insert(
            "x-antigravity-session-id",
            HeaderValue::from_static("ag-think-42"),
        );
        headers.insert("user-agent", HeaderValue::from_static("claude-code/1.0"));

        let json = headers_to_redacted_json(&headers);
        assert!(json.contains("***REDACTED***"), "{json}");
        assert!(!json.contains("sk-secret-customer-key"), "{json}");
        assert!(!json.contains("sk-another"), "{json}");
        assert!(json.contains("sess-ops-compare-001"), "{json}");
        assert!(json.contains("ag-think-42"), "{json}");
        assert!(json.contains("claude-code/1.0"), "{json}");
    }

    #[test]
    fn test_simplify_tools_preserves_full_schema() {
        let tools = json!([
            {
                "functionDeclarations": [
                    {
                        "name": "edit_file",
                        "description": "Edit a file in codebase",
                        "parameters": {
                            "type": "OBJECT",
                            "properties": {
                                "file_path": { "type": "STRING", "description": "path to file" },
                                "content": { "type": "STRING" }
                            },
                            "required": ["file_path", "content"]
                        }
                    }
                ]
            },
            {
                "googleSearch": {}
            }
        ]);

        let req = json!({
            "model": "gemini-2.5-pro",
            "tools": tools,
            "contents": [
                {
                    "role": "user",
                    "parts": [{"text": "Please edit foo.rs"}]
                }
            ]
        });

        let simplified = simplify_payload_json(&req);
        assert_eq!(
            simplified["tools"], tools,
            "Tools schema should be completely preserved!"
        );
        assert_eq!(simplified["model"], "gemini-2.5-pro");
    }

    #[test]
    fn test_simplify_responses_payload_keeps_instructions_and_input() {
        // 回归：Responses / Codex 协议的系统提示词在 `instructions`、上下文在 `input`。
        // 这两个键一度不在白名单里 → simple 模式下整段消失（客户端原文看不到系统提示词与
        // 上下文，而中转报文是 Gemini 格式故看起来完整）。
        let req = json!({
            "model": "gemini-2.5-pro",
            "instructions": "You are Codex, a coding agent.",
            "input": [
                { "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "hi" }] },
                { "type": "function_call_output", "call_id": "call_1", "output": "ok" }
            ],
            "max_output_tokens": 1024
        });

        let simplified = simplify_payload_json(&req);

        assert_eq!(
            simplified["instructions"], "You are Codex, a coding agent.",
            "instructions 必须保留: {simplified}"
        );
        assert!(
            simplified["input"].is_array(),
            "input 必须保留为数组: {simplified}"
        );
        assert_eq!(simplified["input"][0]["role"], "user");
        assert_eq!(simplified["input"][1]["type"], "function_call_output");
        assert_eq!(simplified["input"][1]["call_id"], "call_1");
        assert_eq!(simplified["input"][1]["output"], "ok");
        assert_eq!(simplified["max_output_tokens"], 1024);
    }

    #[test]
    fn test_simplify_consolidated_response_preserves_all_ops_fields() {
        let consolidated = json!({
            "_session_thinking_id": "f4379d8e-02a3-4e44-902b-1a87c4a1b30c",
            "content": "Here is the result of your query.",
            "thinking": "First, let's consider the problem deeply...",
            "thinking_signature": "sig_abcd_1234567890_very_long_valid_signature",
            "tool_calls": [
                {
                    "id": "call_99",
                    "type": "function",
                    "function": {
                        "name": "get_weather",
                        "arguments": "{\"location\":\"Beijing\"}"
                    }
                }
            ],
            "usage": {
                "input_tokens": 33724,
                "output_tokens": 35,
                "total_tokens": 33759,
                "cached_tokens": 0,
                "cache_hit_rate": "0.0%"
            }
        });

        let simplified = simplify_payload_json(&consolidated);
        assert_eq!(
            simplified["_session_thinking_id"],
            "f4379d8e-02a3-4e44-902b-1a87c4a1b30c"
        );
        assert_eq!(simplified["content"], "Here is the result of your query.");
        assert_eq!(
            simplified["thinking"],
            "First, let's consider the problem deeply..."
        );
        assert_eq!(
            simplified["thinking_signature"],
            "sig_abcd_1234567890_very_long_valid_signature"
        );
        assert_eq!(
            simplified["tool_calls"][0]["function"]["name"],
            "get_weather"
        );
        assert_eq!(
            simplified["tool_calls"][0]["function"]["arguments"],
            "{\"location\":\"Beijing\"}"
        );
        assert_eq!(simplified["usage"]["input_tokens"], 33724);
        assert_eq!(simplified["usage"]["output_tokens"], 35);
    }

    #[test]
    fn test_simplify_gemini_parts_preserves_args_and_signatures() {
        let gemini_resp = json!({
            "candidates": [
                {
                    "content": {
                        "role": "model",
                        "parts": [
                            {
                                "thought": true,
                                "text": "Analyzing the request...",
                                "thoughtSignature": "tsig_xyz_987654"
                            },
                            {
                                "functionCall": {
                                    "name": "search_code",
                                    "args": { "query": "struct Token" }
                                }
                            }
                        ]
                    }
                }
            ],
            "usageMetadata": {
                "promptTokenCount": 1500,
                "candidatesTokenCount": 80
            }
        });

        let simplified = simplify_payload_json(&gemini_resp);
        let cand = &simplified["candidates"][0]["content"]["parts"];
        assert_eq!(cand[0]["thought"], true);
        assert_eq!(cand[0]["text"], "Analyzing the request...");
        assert_eq!(cand[0]["thoughtSignature"], "tsig_xyz_987654");
        assert_eq!(cand[1]["functionCall"]["name"], "search_code");
        assert_eq!(cand[1]["functionCall"]["args"]["query"], "struct Token");
        assert_eq!(simplified["usageMetadata"]["promptTokenCount"], 1500);
    }
}
