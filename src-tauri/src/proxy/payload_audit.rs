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

/// Canonical form shared by header and query-parameter names: lowercase, with `_`, `-`, any
/// other non-alphanumeric character and camelCase boundaries all collapsed to a single `-`
/// (`maxOutputTokens` -> `max-output-tokens`, `APIKey` -> `api-key`, `x_goog_api_key` ->
/// `x-goog-api-key`).
fn normalize_name(name: &str) -> String {
    fn push_sep(out: &mut String) {
        if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::with_capacity(name.len() + 4);
    for (i, &c) in chars.iter().enumerate() {
        if !c.is_alphanumeric() {
            push_sep(&mut out);
            continue;
        }
        if c.is_uppercase() && i > 0 {
            let prev = chars[i - 1];
            let next_is_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
            if prev.is_lowercase()
                || prev.is_ascii_digit()
                || (prev.is_uppercase() && next_is_lower)
            {
                push_sep(&mut out);
            }
        }
        out.extend(c.to_lowercase());
    }
    out.trim_end_matches('-').to_string()
}

/// Exact (normalized) session markers kept in clear for ops comparison.
fn is_thinking_session_header(normalized: &str) -> bool {
    THINKING_SESSION_HEADERS.contains(&normalized)
}

/// Substrings that mark a credential-bearing name (checked on the normalized name).
const SENSITIVE_NAME_PARTS: &[&str] = &[
    "token",
    "secret",
    "password",
    "passwd",
    "auth",
    "bearer",
    "cookie",
    "credential",
    "api-key",
    "apikey",
];

/// Leading words of the token-count metrics that stay in clear (`input-tokens`,
/// `prompt-token-count`, `cached-content-token-count`, ...).
const TOKEN_METRIC_PREFIXES: &[&str] = &[
    "input",
    "output",
    "total",
    "cached",
    "cached-content",
    "cache-read",
    "cache-creation",
    "reasoning",
    "prompt",
    "completion",
    "candidates",
    "thoughts",
    "tool-use-prompt",
    "max",
    "max-output",
    "max-completion",
];

/// Explicit allowlist of token-count metrics, on a normalized name: bare `token-count`,
/// `<prefix>-tokens` and `<prefix>-token-count`. Anything else containing `token`
/// (`id-tokens`, `oauth-tokens`, bare `tokens`, ...) is treated as a credential.
fn is_token_metric_name(n: &str) -> bool {
    n == "token-count"
        || n.strip_suffix("-tokens")
            .or_else(|| n.strip_suffix("-token-count"))
            .is_some_and(|stem| TOKEN_METRIC_PREFIXES.contains(&stem))
}

/// Fail-closed credential-name check shared by headers and URI query parameters: a name is
/// redacted unless it is an exact session marker or an allowlisted token-count metric.
fn is_sensitive_name(name: &str) -> bool {
    let n = normalize_name(name);
    if is_thinking_session_header(&n) {
        return false;
    }
    if n == "key" || n.ends_with("-key") {
        return true;
    }
    SENSITIVE_NAME_PARTS
        .iter()
        .any(|part| n.contains(part) && !(*part == "token" && is_token_metric_name(&n)))
}

pub fn redact_header_value(name: &str, value: &str) -> String {
    if !is_sensitive_name(name) {
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

/// Redacts the values of credential-bearing query parameters (e.g. `?key=...`)
/// in a request URI. Everything else is preserved byte-for-byte.
pub fn redact_uri(uri: &str) -> String {
    let (before_fragment, fragment) = match uri.find('#') {
        Some(i) => (&uri[..i], &uri[i..]),
        None => (uri, ""),
    };
    let Some((path, query)) = before_fragment.split_once('?') else {
        return uri.to_string();
    };

    let redacted_query = query
        .split('&')
        .map(|pair| match pair.split_once('=') {
            Some((name, _))
                if url::form_urlencoded::parse(name.as_bytes())
                    .next()
                    .is_some_and(|(decoded, _)| is_sensitive_name(&decoded)) =>
            {
                format!("{}=***REDACTED***", name)
            }
            _ => pair.to_string(),
        })
        .collect::<Vec<_>>()
        .join("&");

    format!("{}?{}{}", path, redacted_query, fragment)
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

fn redact_data_url(url_str: &str) -> String {
    if url_str.starts_with("data:") {
        let comma_pos = url_str.find(',').unwrap_or(0);
        let header = &url_str[..comma_pos];
        let len = url_str.len().saturating_sub(comma_pos);
        format!("{},[base64 image: {} bytes]", header, len)
    } else {
        url_str.to_string()
    }
}

fn simplify_image_url_value(img_url: &Value) -> Value {
    if let Some(url_str) = img_url.as_str() {
        return Value::String(redact_data_url(url_str));
    }
    if let Some(obj) = img_url.as_object() {
        if let Some(url_str) = obj.get("url").and_then(|u| u.as_str()) {
            if url_str.starts_with("data:") {
                let mut cloned = obj.clone();
                cloned.insert("url".into(), Value::String(redact_data_url(url_str)));
                return Value::Object(cloned);
            }
        }
    }
    img_url.clone()
}

fn simplify_content_block(obj: &Map<String, Value>) -> Value {
    let mut slim = Map::new();
    if let Some(t) = obj.get("type") {
        slim.insert("type".into(), t.clone());
    }
    if let Some(text) = obj.get("text") {
        slim.insert("text".into(), simplify_content(text));
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
    if let Some(nested_content) = obj.get("content") {
        slim.insert("content".into(), simplify_content(nested_content));
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
        slim.insert("image_url".into(), simplify_image_url_value(img_url));
    }
    if let Some(file_data) = obj.get("file_data").and_then(|v| v.as_str()) {
        let redacted = if file_data.starts_with("data:") {
            redact_data_url(file_data)
        } else {
            format!("[base64 file: {} bytes]", file_data.len())
        };
        slim.insert("file_data".into(), Value::String(redacted));
    }
    if let Some(audio_obj) = obj.get("input_audio").and_then(|v| v.as_object()) {
        let mut audio_slim = audio_obj.clone();
        if let Some(data_str) = audio_obj.get("data").and_then(|d| d.as_str()) {
            audio_slim.insert(
                "data".into(),
                Value::String(format!("[base64 audio: {} bytes]", data_str.len())),
            );
        }
        slim.insert("input_audio".into(), Value::Object(audio_slim));
    }
    if slim.is_empty() {
        Value::Object(obj.clone())
    } else {
        Value::Object(slim)
    }
}

/// OpenAI Responses 的 `input` 项结构与 chat message 不同（`type` / `call_id` / `arguments` /
/// `output` 等），单独裁剪：沿用 `simplify_message` 的体积控制，再补回调试必需的字段，
/// 同时对 `output`（如 Codex `view_image` 返回的 `function_call_output` 图片数组）与顶层 `image_url` 执行 Base64 脱敏。
fn simplify_responses_input_item(item: &Value) -> Value {
    let mut out = simplify_message(item);
    if let Some(obj) = out.as_object_mut() {
        for key in ["type", "role", "name", "namespace", "call_id", "arguments"] {
            if let Some(v) = item.get(key) {
                obj.insert(key.to_string(), v.clone());
            }
        }
        if let Some(input) = item.get("input") {
            obj.insert("input".to_string(), simplify_content(input));
        }
        if let Some(tools) = item.get("tools") {
            obj.insert("tools".to_string(), simplify_tools(tools));
        }
        if let Some(output) = item.get("output") {
            obj.insert("output".to_string(), simplify_content(output));
        }
        if let Some(img_url) = item.get("image_url") {
            obj.insert("image_url".to_string(), simplify_image_url_value(img_url));
        }
    }
    out
}

fn simplify_content(content: &Value) -> Value {
    match content {
        Value::String(s) => {
            if s.starts_with("data:") {
                Value::String(redact_data_url(s))
            } else {
                // 文本内嵌的 data URL（如序列化进 prompt 的会话转录）同样折叠，避免日志被 Base64 撑爆
                Value::String(
                    crate::proxy::mappers::prompt_sanitizer::PromptSanitizer::collapse_text_data_urls(
                        s, 1024,
                    )
                    .unwrap_or_else(|| s.clone()),
                )
            }
        }
        Value::Array(arr) => Value::Array(
            arr.iter()
                .map(|block| {
                    if let Some(obj) = block.as_object() {
                        simplify_content_block(obj)
                    } else {
                        block.clone()
                    }
                })
                .collect(),
        ),
        Value::Object(obj) => simplify_content_block(obj),
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
    fn query_param_sensitivity() {
        for name in [
            "key",
            "KEY",
            "api_key",
            "apikey",
            "x-goog-api-key",
            "access_token",
            "token",
            "auth_token",
        ] {
            assert!(is_sensitive_name(name), "{name} should be sensitive");
        }
        for name in ["alt", "model", "stream", "session_id"] {
            assert!(!is_sensitive_name(name), "{name} should not be sensitive");
        }
    }

    #[test]
    fn redact_uri_masks_key_query_param() {
        let out =
            redact_uri("/v1beta/models/gemini-x:streamGenerateContent?alt=sse&key=AIzaSECRET");
        assert_eq!(
            out,
            "/v1beta/models/gemini-x:streamGenerateContent?alt=sse&key=***REDACTED***"
        );
        assert!(!out.contains("AIzaSECRET"));
    }

    #[test]
    fn redact_uri_masks_every_repeated_key() {
        let out = redact_uri("/v1/models?key=a&key=b");
        assert_eq!(out, "/v1/models?key=***REDACTED***&key=***REDACTED***");
    }

    #[test]
    fn redact_uri_leaves_non_sensitive_uris_unchanged() {
        assert_eq!(redact_uri("/v1/chat/completions"), "/v1/chat/completions");
        assert_eq!(redact_uri("/x?alt=sse"), "/x?alt=sse");
        assert_eq!(
            redact_uri("/x?session_id=abc&a=%41%20b&&c#frag"),
            "/x?session_id=abc&a=%41%20b&&c#frag"
        );
    }

    #[test]
    fn redact_uri_masks_named_variants_and_keeps_order() {
        assert_eq!(
            redact_uri("/x?api_key=s&model=m"),
            "/x?api_key=***REDACTED***&model=m"
        );
    }

    #[test]
    fn redact_uri_decodes_percent_encoded_names() {
        let out = redact_uri("/x?k%65y=s");
        assert_eq!(out, "/x?k%65y=***REDACTED***");
        assert!(!out.contains("=s"));
    }

    #[test]
    fn redact_uri_handles_flag_pairs_empty_values_and_fragments() {
        assert_eq!(redact_uri("/x?flag&key="), "/x?flag&key=***REDACTED***");
        assert_eq!(
            redact_uri("/x?flag&&key=secret#key=frag"),
            "/x?flag&&key=***REDACTED***#key=frag"
        );
        assert_eq!(redact_uri("/x#a?key=s"), "/x#a?key=s");
    }

    #[test]
    fn redact_uri_masks_session_token_names_but_keeps_session_id_and_counts() {
        for uri in [
            "/x?session_token=st-secret",
            "/x?session-token=st-secret",
            "/x?session%5Ftoken=st-secret",
        ] {
            let out = redact_uri(uri);
            assert!(!out.contains("st-secret"), "{uri} leaked: {out}");
            assert!(out.contains("=***REDACTED***"), "{uri} -> {out}");
        }
        assert_eq!(redact_uri("/x?session_id=abc"), "/x?session_id=abc");
        assert_eq!(redact_uri("/x?max_token_count=5"), "/x?max_token_count=5");
    }

    #[test]
    fn redact_header_value_masks_session_tokens_only() {
        assert_eq!(
            redact_header_value("x-session-token", "s"),
            "***REDACTED***"
        );
        assert_eq!(
            redact_header_value("sessionAuthToken", "s"),
            "***REDACTED***"
        );
        assert_eq!(redact_header_value("x-session-id", "abc"), "abc");
        assert_eq!(redact_header_value("token-count", "5"), "5");
    }

    #[test]
    fn account_token_names_are_redacted_but_token_counts_are_not() {
        assert_eq!(
            redact_header_value("x-account-token", "s"),
            "***REDACTED***"
        );
        assert_eq!(redact_header_value("accountToken", "s"), "***REDACTED***");
        assert_eq!(redact_header_value("token-count", "5"), "5");

        let out = redact_uri("/x?account_token=acct-secret");
        assert!(!out.contains("acct-secret"), "leaked: {out}");
        assert_eq!(out, "/x?account_token=***REDACTED***");
        assert_eq!(redact_uri("/x?max_token_count=5"), "/x?max_token_count=5");
        assert_eq!(redact_uri("/x?account_id=42"), "/x?account_id=42");
    }

    #[test]
    fn token_metric_names_are_not_redacted_in_uris_and_headers() {
        for name in [
            "input_tokens",
            "output_tokens",
            "total_tokens",
            "cached_tokens",
            "reasoning_tokens",
            "max_tokens",
            "max-tokens",
            "maxOutputTokens",
            "prompt_token_count",
            "candidates_token_count",
            "token_count",
            "total_token_count",
            "thoughts_token_count",
            "cached_content_token_count",
        ] {
            assert!(!is_sensitive_name(name), "{name} should not be sensitive");
            assert_eq!(redact_header_value(name, "123"), "123", "{name}");
            let uri = format!("/x?{name}=123");
            assert_eq!(redact_uri(&uri), uri, "{name}");
        }
    }

    #[test]
    fn token_credential_names_stay_redacted_in_uris_and_headers() {
        for name in [
            "token",
            "tokens",
            "access_token",
            "refresh_token",
            "id_token",
            "id_tokens",
            "oauth_tokens",
            "api_tokens",
            "apitokens",
            "gh_tokens",
            "bearer_tokens",
            "my_tokens",
            "session_token",
            "account_token",
            "auth_token",
            "refresh_tokens",
            "x-goog-api-key",
            "api-key",
            "api_key",
            "key",
            "x-ratelimit-remaining-tokens",
        ] {
            assert!(is_sensitive_name(name), "{name} should be sensitive");
            assert_eq!(
                redact_header_value(name, "secret"),
                "***REDACTED***",
                "{name}"
            );
            let out = redact_uri(&format!("/x?{name}=secret&input_tokens=7"));
            assert!(!out.contains("secret"), "{name} leaked: {out}");
            assert!(out.ends_with("&input_tokens=7"), "{name} -> {out}");
        }
        assert_eq!(redact_uri("/x?key=secret"), "/x?key=***REDACTED***");
    }

    #[test]
    fn name_normalizer_unifies_separators_and_camel_case() {
        assert_eq!(normalize_name("maxOutputTokens"), "max-output-tokens");
        assert_eq!(normalize_name("max_output_tokens"), "max-output-tokens");
        assert_eq!(normalize_name("APIKey"), "api-key");
        assert_eq!(normalize_name("X_Goog-Api_Key"), "x-goog-api-key");
        assert_eq!(normalize_name("KEY"), "key");
        assert_eq!(normalize_name("_token__"), "token");
    }

    #[test]
    fn credential_names_are_redacted_fail_closed_in_uris_and_headers() {
        for name in [
            "key",
            "api_key",
            "api-key",
            "apiKey",
            "x-goog-api-key",
            "token",
            "access_token",
            "refresh_token",
            "refresh_tokens",
            "id_tokens",
            "oauth_tokens",
            "api_tokens",
            "apitokens",
            "gh_tokens",
            "bearer_tokens",
            "session_token",
            "account_token",
            "my_tokens",
            "max_tokens_secret",
            "input_tokens_auth",
        ] {
            assert!(is_sensitive_name(name), "{name} should be sensitive");
            assert_eq!(
                redact_header_value(name, "secret"),
                "***REDACTED***",
                "{name}"
            );
            let out = redact_uri(&format!("/x?{name}=secret&alt=sse"));
            assert_eq!(out, format!("/x?{name}=***REDACTED***&alt=sse"), "{name}");
        }
        assert_eq!(
            redact_header_value("authorization", "Bearer abc"),
            "Bearer ***REDACTED***"
        );
        // `key` / `api_key` header names must not be stored in clear.
        let mut headers = HeaderMap::new();
        headers.insert("key", HeaderValue::from_static("k-secret"));
        headers.insert("api_key", HeaderValue::from_static("ak-secret"));
        let json = headers_to_redacted_json(&headers);
        assert!(
            !json.contains("k-secret") && !json.contains("ak-secret"),
            "{json}"
        );
    }

    #[test]
    fn token_metrics_and_plain_params_stay_in_clear_in_uris_and_headers() {
        for name in [
            "input_tokens",
            "output_tokens",
            "total_tokens",
            "cached_tokens",
            "reasoning_tokens",
            "max_tokens",
            "maxOutputTokens",
            "prompt_token_count",
            "candidates_token_count",
            "total_token_count",
            "token_count",
            "alt",
        ] {
            assert!(!is_sensitive_name(name), "{name} should not be sensitive");
            assert_eq!(redact_header_value(name, "7"), "7", "{name}");
            let uri = format!("/x?{name}=7");
            assert_eq!(redact_uri(&uri), uri, "{name}");
        }
        assert_eq!(redact_uri("/x?alt=sse"), "/x?alt=sse");
        assert_eq!(redact_header_value("alt", "sse"), "sse");
    }

    #[test]
    fn session_markers_stay_in_clear_but_do_not_exempt_session_token() {
        for name in ["x-session-id", "mcp-session-id", "x_antigravity_session_id"] {
            assert!(!is_sensitive_name(name), "{name}");
        }
        for name in ["session-token", "x-session-token", "mcp-session-token"] {
            assert!(is_sensitive_name(name), "{name}");
        }
    }

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
    fn test_simplify_responses_payload_redacts_input_image_and_tool_output_base64() {
        let b64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
        let data_url = format!("data:image/png;base64,{b64}");
        let req = json!({
            "model": "gemini-4-argon-high",
            "instructions": "You are Codex.",
            "input": [
                {
                    "type": "message",
                    "role": "user",
                    "content": [
                        { "type": "input_text", "text": "Look at this screenshot" },
                        { "type": "input_image", "image_url": data_url.clone() }
                    ]
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_view_img_1",
                    "output": [
                        { "type": "input_image", "image_url": data_url }
                    ]
                }
            ]
        });

        let simplified = simplify_payload_json(&req);
        let serialized = serde_json::to_string(&simplified).unwrap();
        assert!(
            !serialized.contains(b64),
            "Concise mode must redact base64 in both message input_image and function_call_output: {serialized}"
        );
        assert!(
            serialized.contains("[base64 image:"),
            "Redacted placeholder should be present: {serialized}"
        );
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

    #[test]
    fn test_simplify_responses_input_item_preserves_custom_tool_namespace_and_additional_tools() {
        let embedded_img = format!("echo ok data:image/png;base64,{}", "A".repeat(1200));
        let req = json!({
            "model": "gpt-5.4",
            "input": [
                {
                    "type": "additional_tools",
                    "tools": [
                        { "type": "custom", "name": "exec", "description": "run cmd" }
                    ]
                },
                {
                    "type": "custom_tool_call",
                    "name": "exec",
                    "namespace": "functions",
                    "call_id": "call_custom_1",
                    "input": embedded_img
                },
                {
                    "type": "function_call",
                    "name": "js",
                    "namespace": "mcp__node_repl",
                    "call_id": "call_fn_1",
                    "arguments": "{\"code\":\"1+1\"}"
                }
            ]
        });

        let simplified = simplify_payload_json(&req);
        let input = simplified["input"].as_array().expect("input array");
        assert_eq!(input[0]["type"], "additional_tools");
        assert_eq!(input[0]["tools"][0]["name"], "exec");

        assert_eq!(input[1]["type"], "custom_tool_call");
        assert_eq!(input[1]["name"], "exec");
        assert_eq!(input[1]["namespace"], "functions");
        assert_eq!(input[1]["call_id"], "call_custom_1");
        assert!(input[1]["input"].as_str().unwrap().starts_with("echo ok "));
        assert!(input[1]["input"].as_str().unwrap().contains("base64"));

        assert_eq!(input[2]["type"], "function_call");
        assert_eq!(input[2]["name"], "js");
        assert_eq!(input[2]["namespace"], "mcp__node_repl");
        assert_eq!(input[2]["call_id"], "call_fn_1");
        assert_eq!(input[2]["arguments"], "{\"code\":\"1+1\"}");
    }
}
