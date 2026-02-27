use axum::{
    body::{to_bytes, Body},
    extract::{Json, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use bytes::{Bytes, BytesMut};
use futures::StreamExt;
use serde_json::{json, Value};

use crate::proxy::mappers::claude::models::{
    ClaudeRequest, ContentBlock as ClaudeContentBlock, Message as ClaudeMessage, MessageContent as ClaudeMessageContent,
    SystemPrompt, Tool as ClaudeTool,
};
use crate::proxy::mappers::openai::models::{
    OpenAIContent, OpenAIContentBlock, OpenAIImageUrl, OpenAIMessage, OpenAIRequest, ThinkingConfig,
    ToolCall, ToolFunction,
};
use crate::proxy::server::AppState;

const MAX_CURSOR_BODY_SIZE: usize = 100 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CursorReasoningMode {
    Hide,
    Raw,
    ThinkTags,
    Inline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorPayloadKind {
    OpenAiChat,
    ResponsesLike,
    AnthropicLike,
}

fn parse_cursor_reasoning_mode(value: &str) -> Option<CursorReasoningMode> {
    let normalized = value.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "hide" | "off" | "disabled" => Some(CursorReasoningMode::Hide),
        "raw" | "passthrough" | "native" => Some(CursorReasoningMode::Raw),
        "think_tags" | "think-tags" | "fold" | "cursor" => Some(CursorReasoningMode::ThinkTags),
        "inline" | "on" | "enabled" => Some(CursorReasoningMode::Inline),
        _ => None,
    }
}

fn resolve_cursor_reasoning_mode() -> CursorReasoningMode {
    // Cursor endpoint defaults to think_tags mode:
    // convert reasoning_content into <think>...</think> blocks that Cursor can fold.
    // 优先级: 页面配置(持久化) > 环境变量 > 默认值

    if let Some(mode) =
        parse_cursor_reasoning_mode(&crate::proxy::get_cursor_reasoning_mode())
    {
        return mode;
    }

    if let Ok(env_mode) = std::env::var("ANTI_CURSOR_REASONING_MODE") {
        if let Some(mode) = parse_cursor_reasoning_mode(&env_mode) {
            return mode;
        }
    }

    CursorReasoningMode::ThinkTags
}

impl CursorPayloadKind {
    fn as_str(self) -> &'static str {
        match self {
            CursorPayloadKind::OpenAiChat => "openai_chat",
            CursorPayloadKind::ResponsesLike => "responses_like",
            CursorPayloadKind::AnthropicLike => "anthropic_like",
        }
    }
}

fn is_anthropic_specific_block_type(t: &str) -> bool {
    matches!(
        t,
        "tool_use"
            | "tool_result"
            | "thinking"
            | "redacted_thinking"
            | "server_tool_use"
            | "web_search_tool_result"
            | "document"
    )
}

fn is_anthropic_like_messages(body: &Value) -> bool {
    if body.get("system").is_some()
        || body.get("thinking").is_some()
        || body.get("output_config").is_some()
        || body.get("top_k").is_some()
    {
        return true;
    }

    if body
        .get("tools")
        .and_then(|v| v.as_array())
        .map(|tools| tools.iter().any(|t| t.get("input_schema").is_some()))
        .unwrap_or(false)
    {
        return true;
    }

    body.get("messages")
        .and_then(|v| v.as_array())
        .map(|messages| {
            messages.iter().any(|msg| {
                msg.get("content")
                    .and_then(|v| v.as_array())
                    .map(|blocks| {
                        blocks.iter().any(|block| {
                            block
                                .get("type")
                                .and_then(|t| t.as_str())
                                .map(is_anthropic_specific_block_type)
                                .unwrap_or(false)
                        })
                    })
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

pub fn detect_cursor_payload_kind(body: &Value) -> CursorPayloadKind {
    let has_messages = body.get("messages").is_some();
    let is_responses_like =
        !has_messages && (body.get("instructions").is_some() || body.get("input").is_some());
    if is_responses_like {
        return CursorPayloadKind::ResponsesLike;
    }

    if is_anthropic_like_messages(body) {
        CursorPayloadKind::AnthropicLike
    } else {
        CursorPayloadKind::OpenAiChat
    }
}

pub fn normalize_cursor_payload_to_openai(
    body: Value,
    _headers: &HeaderMap,
) -> Result<(Value, CursorPayloadKind), (StatusCode, String)> {
    let kind = detect_cursor_payload_kind(&body);
    match kind {
        CursorPayloadKind::OpenAiChat | CursorPayloadKind::ResponsesLike => Ok((body, kind)),
        CursorPayloadKind::AnthropicLike => {
            let anthropic_req: ClaudeRequest = serde_json::from_value(body)
                .map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid Anthropic request: {}", e)))?;
            let openai_req = anthropic_to_openai_request(anthropic_req);
            let openai_json = serde_json::to_value(openai_req)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to normalize request: {}", e)))?;
            Ok((openai_json, kind))
        }
    }
}

fn system_prompt_to_openai_message(system: Option<SystemPrompt>) -> Option<OpenAIMessage> {
    let text = match system {
        Some(SystemPrompt::String(s)) => s,
        Some(SystemPrompt::Array(blocks)) => blocks
            .into_iter()
            .map(|b| b.text)
            .filter(|s| !s.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n\n"),
        None => String::new(),
    };

    if text.trim().is_empty() {
        None
    } else {
        Some(OpenAIMessage {
            role: "system".to_string(),
            content: Some(OpenAIContent::String(text)),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
            name: None,
        })
    }
}

fn json_value_to_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Array(arr) => arr
            .iter()
            .map(json_value_to_text)
            .filter(|s| !s.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(_) => serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string()),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Null => String::new(),
    }
}

fn content_blocks_to_user_content(blocks: Vec<OpenAIContentBlock>) -> Option<OpenAIContent> {
    if blocks.is_empty() {
        return None;
    }

    let all_text = blocks
        .iter()
        .all(|b| matches!(b, OpenAIContentBlock::Text { .. }));

    if all_text {
        let text = blocks
            .iter()
            .filter_map(|b| match b {
                OpenAIContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        if text.trim().is_empty() {
            None
        } else {
            Some(OpenAIContent::String(text))
        }
    } else {
        Some(OpenAIContent::Array(blocks))
    }
}

fn build_user_message(blocks: Vec<OpenAIContentBlock>) -> Option<OpenAIMessage> {
    let content = content_blocks_to_user_content(blocks)?;
    Some(OpenAIMessage {
        role: "user".to_string(),
        content: Some(content),
        reasoning_content: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
    })
}

fn image_block_to_openai(block: &ClaudeContentBlock) -> Option<OpenAIContentBlock> {
    if let ClaudeContentBlock::Image { source, .. } = block {
        let url = if source.source_type.eq_ignore_ascii_case("base64") {
            format!("data:{};base64,{}", source.media_type, source.data)
        } else {
            source.data.clone()
        };
        Some(OpenAIContentBlock::ImageUrl {
            image_url: OpenAIImageUrl { url, detail: None },
        })
    } else {
        None
    }
}

fn convert_assistant_message(msg: ClaudeMessage) -> Vec<OpenAIMessage> {
    match msg.content {
        ClaudeMessageContent::String(s) => vec![OpenAIMessage {
            role: "assistant".to_string(),
            content: Some(OpenAIContent::String(s)),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }],
        ClaudeMessageContent::Array(blocks) => {
            let mut text_parts: Vec<String> = Vec::new();
            let mut thinking_parts: Vec<String> = Vec::new();
            let mut tool_calls: Vec<ToolCall> = Vec::new();

            for block in blocks {
                match block {
                    ClaudeContentBlock::Text { text } => text_parts.push(text),
                    ClaudeContentBlock::Thinking { thinking, .. } => {
                        if !thinking.trim().is_empty() {
                            thinking_parts.push(thinking);
                        }
                    }
                    ClaudeContentBlock::RedactedThinking { .. } => {}
                    ClaudeContentBlock::ToolUse { id, name, input, .. } => {
                        tool_calls.push(ToolCall {
                            id,
                            r#type: "function".to_string(),
                            function: ToolFunction {
                                name,
                                arguments: serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_string()),
                            },
                        });
                    }
                    ClaudeContentBlock::ServerToolUse { id, name, input } => {
                        tool_calls.push(ToolCall {
                            id,
                            r#type: "function".to_string(),
                            function: ToolFunction {
                                name,
                                arguments: serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_string()),
                            },
                        });
                    }
                    ClaudeContentBlock::ToolResult { tool_use_id, content, .. } => {
                        let text = json_value_to_text(&content);
                        text_parts.push(format!("[tool_result:{}] {}", tool_use_id, text));
                    }
                    ClaudeContentBlock::WebSearchToolResult { tool_use_id, content } => {
                        let text = json_value_to_text(&content);
                        text_parts.push(format!("[web_search_tool_result:{}] {}", tool_use_id, text));
                    }
                    ClaudeContentBlock::Image { .. } => {
                        text_parts.push("[image omitted in assistant message]".to_string());
                    }
                    ClaudeContentBlock::Document { source, .. } => {
                        text_parts.push(format!(
                            "[document omitted in assistant message: {}]",
                            source.media_type
                        ));
                    }
                }
            }

            vec![OpenAIMessage {
                role: "assistant".to_string(),
                content: if text_parts.is_empty() {
                    None
                } else {
                    Some(OpenAIContent::String(text_parts.join("\n\n")))
                },
                reasoning_content: if thinking_parts.is_empty() {
                    None
                } else {
                    Some(thinking_parts.join("\n\n"))
                },
                tool_calls: if tool_calls.is_empty() { None } else { Some(tool_calls) },
                tool_call_id: None,
                name: None,
            }]
        }
    }
}

fn convert_user_message(msg: ClaudeMessage) -> Vec<OpenAIMessage> {
    match msg.content {
        ClaudeMessageContent::String(s) => vec![OpenAIMessage {
            role: "user".to_string(),
            content: Some(OpenAIContent::String(s)),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }],
        ClaudeMessageContent::Array(blocks) => {
            let mut out: Vec<OpenAIMessage> = Vec::new();
            let mut pending_user_blocks: Vec<OpenAIContentBlock> = Vec::new();

            let flush_user_blocks = |out: &mut Vec<OpenAIMessage>, blocks: &mut Vec<OpenAIContentBlock>| {
                if let Some(msg) = build_user_message(std::mem::take(blocks)) {
                    out.push(msg);
                }
            };

            for block in blocks {
                match &block {
                    ClaudeContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    } => {
                        flush_user_blocks(&mut out, &mut pending_user_blocks);
                        let mut text = json_value_to_text(content);
                        if is_error.unwrap_or(false) {
                            text = format!("[tool_error] {}", text);
                        }
                        out.push(OpenAIMessage {
                            role: "tool".to_string(),
                            content: Some(OpenAIContent::String(text)),
                            reasoning_content: None,
                            tool_calls: None,
                            tool_call_id: Some(tool_use_id.clone()),
                            name: None,
                        });
                    }
                    _ => {
                        match block {
                            ClaudeContentBlock::Text { text } => {
                                pending_user_blocks.push(OpenAIContentBlock::Text { text });
                            }
                            ClaudeContentBlock::Image { .. } => {
                                if let Some(image) = image_block_to_openai(&block) {
                                    pending_user_blocks.push(image);
                                }
                            }
                            ClaudeContentBlock::Document { source, .. } => {
                                pending_user_blocks.push(OpenAIContentBlock::Text {
                                    text: format!("[document omitted: {}]", source.media_type),
                                });
                            }
                            ClaudeContentBlock::Thinking { thinking, .. } => {
                                if !thinking.trim().is_empty() {
                                    pending_user_blocks.push(OpenAIContentBlock::Text {
                                        text: format!("[thinking] {}", thinking),
                                    });
                                }
                            }
                            ClaudeContentBlock::RedactedThinking { .. } => {}
                            ClaudeContentBlock::ToolUse { id, name, input, .. } => {
                                pending_user_blocks.push(OpenAIContentBlock::Text {
                                    text: format!(
                                        "[tool_use:{}:{}] {}",
                                        id,
                                        name,
                                        serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_string())
                                    ),
                                });
                            }
                            ClaudeContentBlock::ServerToolUse { id, name, input } => {
                                pending_user_blocks.push(OpenAIContentBlock::Text {
                                    text: format!(
                                        "[server_tool_use:{}:{}] {}",
                                        id,
                                        name,
                                        serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_string())
                                    ),
                                });
                            }
                            ClaudeContentBlock::WebSearchToolResult { tool_use_id, content } => {
                                pending_user_blocks.push(OpenAIContentBlock::Text {
                                    text: format!(
                                        "[web_search_tool_result:{}] {}",
                                        tool_use_id,
                                        json_value_to_text(&content)
                                    ),
                                });
                            }
                            ClaudeContentBlock::ToolResult { .. } => {}
                        }
                    }
                }
            }

            flush_user_blocks(&mut out, &mut pending_user_blocks);
            out
        }
    }
}

fn convert_tools(tools: Option<Vec<ClaudeTool>>) -> Option<Vec<Value>> {
    let mut out: Vec<Value> = Vec::new();
    for tool in tools.unwrap_or_default() {
        let mut name = tool.name.unwrap_or_default();
        if name.is_empty() {
            if let Some(t) = tool.type_ {
                if t.starts_with("web_search") {
                    name = "web_search".to_string();
                } else {
                    name = t;
                }
            }
        }
        if name.is_empty() {
            continue;
        }

        let parameters = tool.input_schema.unwrap_or_else(|| {
            json!({
                "type": "object",
                "properties": {}
            })
        });

        out.push(json!({
            "type": "function",
            "function": {
                "name": name,
                "description": tool.description.unwrap_or_default(),
                "parameters": parameters
            }
        }));
    }

    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

pub fn anthropic_to_openai_request(req: ClaudeRequest) -> OpenAIRequest {
    let mut messages: Vec<OpenAIMessage> = Vec::new();
    if let Some(system) = system_prompt_to_openai_message(req.system) {
        messages.push(system);
    }

    for msg in req.messages {
        let role = msg.role.as_str();
        if role == "assistant" {
            messages.extend(convert_assistant_message(msg));
        } else if role == "user" {
            messages.extend(convert_user_message(msg));
        } else {
            messages.push(OpenAIMessage {
                role: msg.role,
                content: Some(OpenAIContent::String(match msg.content {
                    ClaudeMessageContent::String(s) => s,
                    ClaudeMessageContent::Array(arr) => arr
                        .iter()
                        .filter_map(|b| match b {
                            ClaudeContentBlock::Text { text } => Some(text.clone()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                })),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
                name: None,
            });
        }
    }

    OpenAIRequest {
        model: req.model,
        messages,
        prompt: None,
        stream: req.stream,
        n: None,
        max_tokens: req.max_tokens,
        temperature: req.temperature,
        top_p: req.top_p,
        stop: None,
        response_format: None,
        tools: convert_tools(req.tools),
        tool_choice: None,
        parallel_tool_calls: None,
        instructions: None,
        input: None,
        size: req.size,
        quality: req.quality,
        person_generation: None,
        thinking: req.thinking.map(|t| ThinkingConfig {
            thinking_type: Some(t.type_),
            budget_tokens: t.budget_tokens,
            effort: t.effort,
        }),
        image_size: None,
    }
}

fn strip_reasoning_fields_in_value(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("reasoning_content");
            for v in map.values_mut() {
                strip_reasoning_fields_in_value(v);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                strip_reasoning_fields_in_value(item);
            }
        }
        _ => {}
    }
}

fn merge_reasoning_content_into_content(
    obj: &mut serde_json::Map<String, Value>,
    separator: &str,
) {
    let Some(reasoning_val) = obj.remove("reasoning_content") else {
        return;
    };
    let reasoning_text = json_value_to_text(&reasoning_val);
    if reasoning_text.trim().is_empty() {
        return;
    }

    let merged = match obj.remove("content") {
        Some(Value::String(content)) if !content.trim().is_empty() => {
            let reasoning_trimmed = reasoning_text.trim();
            let content_trimmed = content.trim();
            if reasoning_trimmed == content_trimmed || content_trimmed.starts_with(reasoning_trimmed) {
                content
            } else {
                format!("{}{}{}", reasoning_text, separator, content)
            }
        }
        Some(Value::Null) | None => reasoning_text,
        Some(other) => {
            let existing = json_value_to_text(&other);
            if existing.trim().is_empty() {
                reasoning_text
            } else if existing.trim() == reasoning_text.trim()
                || existing.trim().starts_with(reasoning_text.trim())
            {
                existing
            } else {
                format!("{}{}{}", reasoning_text, separator, existing)
            }
        }
    };

    obj.insert("content".to_string(), Value::String(merged));
}

fn inline_reasoning_for_non_stream_payload(value: &mut Value) {
    if let Some(choices) = value.get_mut("choices").and_then(|v| v.as_array_mut()) {
        for choice in choices {
            if let Some(message_obj) = choice.get_mut("message").and_then(|v| v.as_object_mut()) {
                merge_reasoning_content_into_content(message_obj, "\n\n");
            }
        }
    }
    strip_reasoning_fields_in_value(value);
}

fn inline_reasoning_for_stream_payload(value: &mut Value) {
    if let Some(choices) = value.get_mut("choices").and_then(|v| v.as_array_mut()) {
        for choice in choices {
            if let Some(delta_obj) = choice.get_mut("delta").and_then(|v| v.as_object_mut()) {
                merge_reasoning_content_into_content(delta_obj, "");
            }
        }
    }
    strip_reasoning_fields_in_value(value);
}

fn think_tags_for_non_stream_payload(value: &mut Value) {
    if let Some(choices) = value.get_mut("choices").and_then(|v| v.as_array_mut()) {
        for choice in choices {
            if let Some(message_obj) = choice.get_mut("message").and_then(|v| v.as_object_mut()) {
                let Some(reasoning_val) = message_obj.remove("reasoning_content") else {
                    continue;
                };
                let reasoning_text = json_value_to_text(&reasoning_val);
                if reasoning_text.trim().is_empty() {
                    continue;
                }

                let content_text = match message_obj.remove("content") {
                    Some(Value::String(s)) => s,
                    Some(Value::Null) | None => String::new(),
                    Some(other) => json_value_to_text(&other),
                };

                let wrapped = if content_text.trim().is_empty() {
                    format!("<think>\n{}\n</think>", reasoning_text)
                } else {
                    format!("<think>\n{}\n</think>\n\n{}", reasoning_text, content_text)
                };
                message_obj.insert("content".to_string(), Value::String(wrapped));
            }
        }
    }
    strip_reasoning_fields_in_value(value);
}

#[derive(Default)]
struct ThinkTagStreamState {
    thinking_open: bool,
}

fn think_tags_for_stream_payload(value: &mut Value, state: &mut ThinkTagStreamState) {
    if let Some(choices) = value.get_mut("choices").and_then(|v| v.as_array_mut()) {
        for choice in choices {
            let finish_reason = choice
                .get("finish_reason")
                .and_then(|v| v.as_str())
                .map(|s| !s.is_empty())
                .unwrap_or(false);

            if let Some(delta_obj) = choice.get_mut("delta").and_then(|v| v.as_object_mut()) {
                let reasoning_text = delta_obj
                    .remove("reasoning_content")
                    .map(|v| json_value_to_text(&v))
                    .unwrap_or_default();

                let existing_content = delta_obj.remove("content");
                let content_text = match &existing_content {
                    Some(Value::String(s)) => s.clone(),
                    Some(Value::Null) | None => String::new(),
                    Some(other) => json_value_to_text(other),
                };

                let mut out = String::new();
                if !reasoning_text.trim().is_empty() {
                    if !state.thinking_open {
                        out.push_str("<think>\n");
                        state.thinking_open = true;
                    }
                    out.push_str(&reasoning_text);
                }

                if !content_text.is_empty() {
                    if state.thinking_open {
                        out.push_str("\n</think>\n\n");
                        state.thinking_open = false;
                    }
                    out.push_str(&content_text);
                }

                if finish_reason && state.thinking_open {
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str("</think>\n");
                    state.thinking_open = false;
                }

                if out.is_empty() {
                    if let Some(orig) = existing_content {
                        delta_obj.insert("content".to_string(), orig);
                    }
                } else {
                    delta_obj.insert("content".to_string(), Value::String(out));
                }
            }
        }
    }
    strip_reasoning_fields_in_value(value);
}

fn sanitize_sse_line(
    line: &str,
    reasoning_mode: CursorReasoningMode,
    think_state: Option<&mut ThinkTagStreamState>,
) -> String {
    let newline = if line.ends_with("\r\n") {
        "\r\n"
    } else if line.ends_with('\n') {
        "\n"
    } else {
        ""
    };

    let trimmed_line = line.trim_end_matches(['\r', '\n']);
    let Some(data_part) = trimmed_line.strip_prefix("data: ") else {
        return line.to_string();
    };
    let payload = data_part.trim();
    if payload == "[DONE]" {
        return line.to_string();
    }

    match serde_json::from_str::<Value>(payload) {
        Ok(mut json_val) => {
            match reasoning_mode {
                CursorReasoningMode::Hide => strip_reasoning_fields_in_value(&mut json_val),
                CursorReasoningMode::Inline => inline_reasoning_for_stream_payload(&mut json_val),
                CursorReasoningMode::ThinkTags => {
                    if let Some(state) = think_state {
                        think_tags_for_stream_payload(&mut json_val, state);
                    } else {
                        think_tags_for_non_stream_payload(&mut json_val);
                    }
                }
                CursorReasoningMode::Raw => {}
            }
            let payload_out = serde_json::to_string(&json_val).unwrap_or_else(|_| payload.to_string());
            format!("data: {}{}", payload_out, newline)
        }
        Err(_) => line.to_string(),
    }
}

async fn sanitize_cursor_openai_output(
    response: Response,
    reasoning_mode: CursorReasoningMode,
) -> Response {
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();

    if content_type.contains("text/event-stream") {
        let (parts, body) = response.into_parts();
        let mut upstream = body.into_data_stream();
        let stream = async_stream::stream! {
            let mut buffer = BytesMut::new();
            let mut think_state = ThinkTagStreamState::default();
            while let Some(next) = upstream.next().await {
                match next {
                    Ok(chunk) => {
                        buffer.extend_from_slice(&chunk);
                        while let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                            let line_raw = buffer.split_to(pos + 1);
                            match std::str::from_utf8(&line_raw) {
                                Ok(line_str) => {
                                    let cleaned = sanitize_sse_line(
                                        line_str,
                                        reasoning_mode,
                                        Some(&mut think_state),
                                    );
                                    yield Ok::<Bytes, axum::Error>(Bytes::from(cleaned));
                                }
                                Err(_) => {
                                    yield Ok::<Bytes, axum::Error>(line_raw.freeze());
                                }
                            }
                        }
                    }
                    Err(err) => {
                        yield Err(err);
                        break;
                    }
                }
            }

            if !buffer.is_empty() {
                match std::str::from_utf8(&buffer) {
                    Ok(line_str) => {
                        let cleaned = sanitize_sse_line(
                            line_str,
                            reasoning_mode,
                            Some(&mut think_state),
                        );
                        yield Ok::<Bytes, axum::Error>(Bytes::from(cleaned));
                    }
                    Err(_) => {
                        yield Ok::<Bytes, axum::Error>(buffer.freeze());
                    }
                }
            }
        };

        return Response::from_parts(parts, Body::from_stream(stream));
    }

    let (mut parts, body) = response.into_parts();
    let bytes = match to_bytes(body, MAX_CURSOR_BODY_SIZE).await {
        Ok(b) => b,
        Err(_) => return Response::from_parts(parts, Body::empty()),
    };

    let body_out = match serde_json::from_slice::<Value>(&bytes) {
        Ok(mut json_val) => {
            match reasoning_mode {
                CursorReasoningMode::Hide => strip_reasoning_fields_in_value(&mut json_val),
                CursorReasoningMode::Inline => inline_reasoning_for_non_stream_payload(&mut json_val),
                CursorReasoningMode::ThinkTags => think_tags_for_non_stream_payload(&mut json_val),
                CursorReasoningMode::Raw => {}
            }
            Bytes::from(serde_json::to_vec(&json_val).unwrap_or_else(|_| bytes.to_vec()))
        }
        Err(_) => bytes,
    };

    parts.headers.remove(header::CONTENT_LENGTH);
    Response::from_parts(parts, Body::from(body_out))
}

pub async fn handle_cursor_chat_completions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let reasoning_mode = resolve_cursor_reasoning_mode();
    let (normalized_body, payload_kind) = match normalize_cursor_payload_to_openai(body, &headers) {
        Ok(v) => v,
        Err((status, msg)) => return (status, msg).into_response(),
    };

    let raw_response = match crate::proxy::handlers::openai::handle_chat_completions(
        State(state),
        headers,
        Json(normalized_body),
    )
    .await
    {
        Ok(resp) => resp.into_response(),
        Err((status, msg)) => (status, msg).into_response(),
    };

    let mut sanitized = sanitize_cursor_openai_output(raw_response, reasoning_mode).await;
    if let Ok(v) = HeaderValue::from_str(payload_kind.as_str()) {
        sanitized.headers_mut().insert("X-Cursor-Payload-Kind", v);
    }
    sanitized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_cursor_payload_kind() {
        let openai = json!({
            "model": "gpt-4o-mini",
            "messages": [{"role":"user","content":"hello"}]
        });
        assert_eq!(detect_cursor_payload_kind(&openai), CursorPayloadKind::OpenAiChat);

        let responses = json!({
            "model": "gpt-4.1",
            "instructions": "Be concise",
            "input": "hello"
        });
        assert_eq!(detect_cursor_payload_kind(&responses), CursorPayloadKind::ResponsesLike);

        let anthropic = json!({
            "model": "claude-opus-4-6-thinking",
            "messages": [
                {"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Shell","input":{"command":"pwd"}}]}
            ]
        });
        assert_eq!(detect_cursor_payload_kind(&anthropic), CursorPayloadKind::AnthropicLike);
    }

    #[test]
    fn test_anthropic_to_openai_request_tool_roundtrip() {
        let req = ClaudeRequest {
            model: "claude-opus-4-6-thinking".to_string(),
            messages: vec![
                ClaudeMessage {
                    role: "assistant".to_string(),
                    content: ClaudeMessageContent::Array(vec![
                        ClaudeContentBlock::Thinking {
                            thinking: "analyzing".to_string(),
                            signature: None,
                            cache_control: None,
                        },
                        ClaudeContentBlock::ToolUse {
                            id: "tool_1".to_string(),
                            name: "Shell".to_string(),
                            input: json!({"command":"pwd"}),
                            signature: None,
                            cache_control: None,
                        },
                    ]),
                },
                ClaudeMessage {
                    role: "user".to_string(),
                    content: ClaudeMessageContent::Array(vec![ClaudeContentBlock::ToolResult {
                        tool_use_id: "tool_1".to_string(),
                        content: json!("ok"),
                        is_error: Some(false),
                    }]),
                },
            ],
            system: None,
            tools: Some(vec![ClaudeTool {
                type_: None,
                name: Some("Shell".to_string()),
                description: Some("run shell".to_string()),
                input_schema: Some(json!({"type":"object","properties":{"command":{"type":"string"}}})),
            }]),
            stream: false,
            max_tokens: Some(256),
            temperature: Some(0.2),
            top_p: Some(0.9),
            top_k: None,
            thinking: None,
            metadata: None,
            output_config: None,
            size: None,
            quality: None,
        };

        let openai = anthropic_to_openai_request(req);
        assert_eq!(openai.model, "claude-opus-4-6-thinking");
        assert!(openai.tools.is_some());
        assert!(openai
            .messages
            .iter()
            .any(|m| m.role == "assistant" && m.tool_calls.as_ref().map(|v| !v.is_empty()).unwrap_or(false)));
        assert!(openai
            .messages
            .iter()
            .any(|m| m.role == "tool" && m.tool_call_id.as_deref() == Some("tool_1")));
    }

    #[test]
    fn test_sanitize_sse_line_removes_reasoning_content() {
        let line = "data: {\"object\":\"chat.completion.chunk\",\"choices\":[{\"delta\":{\"reasoning_content\":\"hidden\",\"content\":\"ok\"}}]}\n";
        let cleaned = sanitize_sse_line(line, CursorReasoningMode::Hide, None);
        assert!(!cleaned.contains("reasoning_content"));
        assert!(cleaned.contains("\"content\":\"ok\""));
    }

    #[test]
    fn test_sanitize_sse_line_inlines_reasoning_content() {
        let line = "data: {\"object\":\"chat.completion.chunk\",\"choices\":[{\"delta\":{\"reasoning_content\":\"think-\",\"content\":\"answer\"}}]}\n";
        let cleaned = sanitize_sse_line(line, CursorReasoningMode::Inline, None);
        assert!(!cleaned.contains("reasoning_content"));
        assert!(cleaned.contains("\"content\":\"think-answer\""));
    }

    #[test]
    fn test_sanitize_sse_line_raw_preserves_reasoning_content() {
        let line = "data: {\"object\":\"chat.completion.chunk\",\"choices\":[{\"delta\":{\"reasoning_content\":\"hidden\",\"content\":\"ok\"}}]}\n";
        let cleaned = sanitize_sse_line(line, CursorReasoningMode::Raw, None);
        assert!(cleaned.contains("reasoning_content"));
        assert!(cleaned.contains("\"content\":\"ok\""));
    }

    #[test]
    fn test_sanitize_sse_line_think_tags_wraps_reasoning_and_content() {
        let line1 = "data: {\"object\":\"chat.completion.chunk\",\"choices\":[{\"delta\":{\"reasoning_content\":\"thinking...\"},\"finish_reason\":null}]}\n";
        let line2 = "data: {\"object\":\"chat.completion.chunk\",\"choices\":[{\"delta\":{\"content\":\"final answer\"},\"finish_reason\":null}]}\n";
        let mut state = ThinkTagStreamState::default();

        let cleaned1 = sanitize_sse_line(line1, CursorReasoningMode::ThinkTags, Some(&mut state));
        assert!(cleaned1.contains("<think>"));
        assert!(!cleaned1.contains("</think>"));
        assert!(!cleaned1.contains("reasoning_content"));

        let cleaned2 = sanitize_sse_line(line2, CursorReasoningMode::ThinkTags, Some(&mut state));
        assert!(cleaned2.contains("</think>"));
        assert!(cleaned2.contains("final answer"));
    }
}
