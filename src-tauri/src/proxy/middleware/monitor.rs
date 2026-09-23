use crate::proxy::middleware::auth::UserTokenIdentity;
use crate::proxy::monitor::{ProxyRequestLog, UpstreamRequestBodyHolder};
use crate::proxy::server::AppState;
use axum::{
    body::Body,
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use base64::Engine as _;
use futures::{Stream, StreamExt};
use serde_json::Value;
use std::time::Instant;

const MAX_REQUEST_LOG_SIZE: usize = 100 * 1024 * 1024; // 100MB
const MAX_RESPONSE_LOG_SIZE: usize = 100 * 1024 * 1024; // 100MB for image responses
const MAX_LOGGED_FIELD_CHARS: usize = 500;

async fn next_chunk_while_receiver_open<S, T>(
    stream: &mut S,
    tx: &tokio::sync::mpsc::Sender<T>,
) -> Option<S::Item>
where
    S: Stream + Unpin,
{
    tokio::select! {
        biased;
        _ = tx.closed() => None,
        chunk = stream.next() => chunk,
    }
}

fn truncate_for_log(value: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (idx, ch) in value.chars().enumerate() {
        if idx >= max_chars {
            out.push_str("...");
            return out;
        }
        out.push(ch);
    }
    out
}

fn extract_quoted_param(header: &str, key: &str) -> Option<String> {
    let needle = format!("{}=\"", key);
    let start = header.find(&needle)? + needle.len();
    let rest = &header[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn extract_boundary(content_type: &str) -> Option<String> {
    content_type.split(';').find_map(|part| {
        let trimmed = part.trim();
        let value = trimmed.strip_prefix("boundary=")?;
        Some(value.trim_matches('"').to_string())
    })
}

fn is_health_check_path(path: &str) -> bool {
    let clean_path = path.split('?').next().unwrap_or(path).trim_end_matches('/');
    clean_path == "/health" || clean_path == "/healthz" || clean_path == "/api/health"
}

fn should_log_health_checks() -> bool {
    std::env::var("ABV_LOG_HEALTH_CHECKS")
        .map(|val| {
            let v = val.trim().to_ascii_lowercase();
            v == "1" || v == "true" || v == "yes" || v == "on"
        })
        .unwrap_or(false)
}

fn find_subslice(haystack: &[u8], needle: &[u8], start: usize) -> Option<usize> {
    if needle.is_empty() || start >= haystack.len() {
        return None;
    }
    haystack[start..]
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|idx| start + idx)
}

fn trim_part_tail(mut part: &[u8]) -> &[u8] {
    if part.ends_with(b"\r\n") {
        part = &part[..part.len() - 2];
    } else if part.ends_with(b"\n") {
        part = &part[..part.len() - 1];
    }
    part
}

fn summarize_multipart_request(
    bytes: &[u8],
    content_type: &str,
    uri: &str,
) -> Option<(String, Option<String>)> {
    let boundary = extract_boundary(content_type)?;
    let marker = format!("--{}", boundary).into_bytes();
    let mut cursor = find_subslice(bytes, &marker, 0)?;
    let mut fields = serde_json::Map::new();
    let mut files = Vec::new();
    let mut model = None;

    loop {
        cursor += marker.len();
        if cursor >= bytes.len() || bytes[cursor..].starts_with(b"--") {
            break;
        }
        if bytes[cursor..].starts_with(b"\r\n") {
            cursor += 2;
        } else if bytes[cursor..].starts_with(b"\n") {
            cursor += 1;
        }

        let Some(next_boundary) = find_subslice(bytes, &marker, cursor) else {
            break;
        };
        let part = trim_part_tail(&bytes[cursor..next_boundary]);
        cursor = next_boundary;

        let (headers, body) = if let Some(idx) = find_subslice(part, b"\r\n\r\n", 0) {
            (&part[..idx], &part[idx + 4..])
        } else if let Some(idx) = find_subslice(part, b"\n\n", 0) {
            (&part[..idx], &part[idx + 2..])
        } else {
            continue;
        };

        let headers_text = String::from_utf8_lossy(headers);
        let disposition = headers_text
            .lines()
            .find(|line| {
                line.to_ascii_lowercase()
                    .starts_with("content-disposition:")
            })
            .unwrap_or("");
        let content_type = headers_text
            .lines()
            .find(|line| line.to_ascii_lowercase().starts_with("content-type:"))
            .and_then(|line| {
                line.split_once(':')
                    .map(|(_, value)| value.trim().to_string())
            });
        let Some(name) = extract_quoted_param(disposition, "name") else {
            continue;
        };
        let filename = extract_quoted_param(disposition, "filename");

        if filename.is_some() || content_type.as_deref().unwrap_or("").starts_with("image/") {
            files.push(serde_json::json!({
                "field": name,
                "filename": filename,
                "content_type": content_type,
                "bytes": body.len()
            }));
        } else if let Ok(text) = std::str::from_utf8(body) {
            let value = truncate_for_log(text.trim(), MAX_LOGGED_FIELD_CHARS);
            if name == "model" {
                model = Some(value.clone());
            }
            fields.insert(name, Value::String(value));
        } else {
            fields.insert(
                name,
                Value::String(format!("[binary field: {} bytes]", body.len())),
            );
        }
    }

    let summary = serde_json::json!({
        "content_type": "multipart/form-data",
        "path": uri,
        "fields": fields,
        "files": files,
        "raw_bytes": bytes.len()
    });
    let rendered = serde_json::to_string_pretty(&summary).ok()?;
    Some((rendered, model))
}

fn image_mime_and_dimensions(bytes: &[u8]) -> (String, Option<(u32, u32)>) {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes.len() >= 24 {
        let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        return ("image/png".to_string(), Some((width, height)));
    }

    if bytes.starts_with(b"\xff\xd8") {
        let mut idx = 2;
        while idx + 9 < bytes.len() {
            if bytes[idx] != 0xff {
                idx += 1;
                continue;
            }
            let marker = bytes[idx + 1];
            if marker == 0xd9 || marker == 0xda {
                break;
            }
            if idx + 4 > bytes.len() {
                break;
            }
            let segment_len = u16::from_be_bytes([bytes[idx + 2], bytes[idx + 3]]) as usize;
            if segment_len < 2 || idx + 2 + segment_len > bytes.len() {
                break;
            }
            if matches!(
                marker,
                0xc0 | 0xc1
                    | 0xc2
                    | 0xc3
                    | 0xc5
                    | 0xc6
                    | 0xc7
                    | 0xc9
                    | 0xca
                    | 0xcb
                    | 0xcd
                    | 0xce
                    | 0xcf
            ) && segment_len >= 7
            {
                let height = u16::from_be_bytes([bytes[idx + 5], bytes[idx + 6]]) as u32;
                let width = u16::from_be_bytes([bytes[idx + 7], bytes[idx + 8]]) as u32;
                return ("image/jpeg".to_string(), Some((width, height)));
            }
            idx += 2 + segment_len;
        }
        return ("image/jpeg".to_string(), None);
    }

    ("application/octet-stream".to_string(), None)
}

fn summarize_image_json_response(json: &Value) -> Option<String> {
    let mut summary = json.clone();
    let data = summary.get_mut("data")?.as_array_mut()?;
    for item in data.iter_mut() {
        if let Some(obj) = item.as_object_mut() {
            if let Some(b64) = obj.get("b64_json").and_then(|v| v.as_str()) {
                let decoded = base64::engine::general_purpose::STANDARD.decode(b64).ok();
                let (mime_type, dimensions, bytes) = decoded
                    .as_deref()
                    .map(|bytes| {
                        let (mime, dims) = image_mime_and_dimensions(bytes);
                        (mime, dims, bytes.len())
                    })
                    .unwrap_or_else(|| ("unknown".to_string(), None, b64.len() * 3 / 4));
                obj.remove("b64_json");
                obj.insert(
                    "image".to_string(),
                    serde_json::json!({
                        "encoding": "base64",
                        "redacted": true,
                        "mime_type": mime_type,
                        "bytes": bytes,
                        "dimensions": dimensions.map(|(width, height)| serde_json::json!({
                            "width": width,
                            "height": height
                        }))
                    }),
                );
            } else if let Some(url) = obj.get("url").and_then(|v| v.as_str()) {
                if url.starts_with("data:image/") {
                    obj.insert(
                        "url".to_string(),
                        Value::String(format!("[data URL redacted: {} chars]", url.len())),
                    );
                }
            }
        }
    }
    serde_json::to_string_pretty(&summary).ok()
}

/// Helper function to record User Token usage
fn record_user_token_usage(
    user_token_identity: &Option<UserTokenIdentity>,
    log: &ProxyRequestLog,
    user_agent: Option<String>,
) {
    if let Some(identity) = user_token_identity {
        let _ = crate::modules::user_token_db::record_token_usage_and_ip(
            &identity.token_id,
            log.client_ip.as_deref().unwrap_or("127.0.0.1"),
            log.model.as_deref().unwrap_or("unknown"),
            log.input_tokens.unwrap_or(0) as i32,
            log.output_tokens.unwrap_or(0) as i32,
            log.status as u16,
            user_agent,
        );
    }
}

fn extract_cached_tokens(usage: &Value) -> Option<u32> {
    let c = crate::proxy::pipeline::CanonicalUsage::from_gemini(usage);
    if c.cached_tokens > 0 {
        Some(c.cached_tokens)
    } else {
        None
    }
}

fn extract_input_tokens(usage: &Value) -> Option<u32> {
    let has_field = usage.get("prompt_tokens").is_some()
        || usage.get("input_tokens").is_some()
        || usage.get("total_input_tokens").is_some()
        || usage.get("promptTokenCount").is_some();
    if !has_field {
        return None;
    }
    let c = crate::proxy::pipeline::CanonicalUsage::from_gemini(usage);
    Some(c.total_input_tokens)
}

fn extract_reasoning_tokens(usage: &Value) -> Option<u32> {
    let c = crate::proxy::pipeline::CanonicalUsage::from_gemini(usage);
    if c.reasoning_tokens > 0 {
        Some(c.reasoning_tokens)
    } else {
        None
    }
}

fn extract_output_tokens(usage: &Value) -> Option<u32> {
    let has_field = usage.get("completion_tokens").is_some()
        || usage.get("output_tokens").is_some()
        || usage.get("total_output_tokens").is_some()
        || usage.get("candidatesTokenCount").is_some();
    if !has_field {
        return None;
    }
    let c = crate::proxy::pipeline::CanonicalUsage::from_gemini(usage);
    Some(c.output_tokens)
}

fn build_canonical_consolidated_response(
    thinking_content: String,
    mut thinking_signature: String,
    response_content: String,
    tool_calls: Vec<Value>,
    session_id: Option<&str>,
    log_id: &str,
    timing_obj: serde_json::Map<String, Value>,
    usage_obj: Option<serde_json::Map<String, Value>>,
) -> Value {
    // 权威网关签名回填：若当前签名为空，尝试通过工具调用ID或会话ID从网关状态机(内存 L1 / SQLite L2 DB 思考持久化)恢复
    if thinking_signature.is_empty() {
        for tc in &tool_calls {
            if let Some(call_id) = tc.get("id").and_then(|v| v.as_str()) {
                if !call_id.is_empty() {
                    if let Some(sig) =
                        crate::proxy::SignatureCache::global().get_tool_signature(call_id)
                    {
                        thinking_signature = sig;
                        break;
                    }
                }
            }
        }
        if thinking_signature.is_empty() {
            if let Some(sid) = session_id {
                if let Some(sig) = crate::proxy::SignatureCache::global().get_session_signature(sid)
                {
                    thinking_signature = sig;
                }
            }
        }
    }

    let mut consolidated = serde_json::Map::new();

    // 1. 会话/思考唯一标识
    let candidate_id = session_id.unwrap_or(log_id);
    consolidated.insert(
        "_session_thinking_id".to_string(),
        Value::String(candidate_id.to_string()),
    );

    // 2. 思考文本与权威签名
    if !thinking_content.is_empty() {
        consolidated.insert("thinking".to_string(), Value::String(thinking_content));
    }
    if !thinking_signature.is_empty() {
        consolidated.insert(
            "thinking_signature".to_string(),
            Value::String(thinking_signature),
        );
    }

    // 3. 正文输出
    if !response_content.is_empty() {
        consolidated.insert("content".to_string(), Value::String(response_content));
    }

    // 4. 工具调用列表 (过滤 Null)
    let clean_tool_calls: Vec<Value> = tool_calls.into_iter().filter(|v| !v.is_null()).collect();
    if !clean_tool_calls.is_empty() {
        consolidated.insert("tool_calls".to_string(), Value::Array(clean_tool_calls));
    }

    // 5. 耗时诊断
    if !timing_obj.is_empty() {
        consolidated.insert("_timing".to_string(), Value::Object(timing_obj));
    }

    // 6. 用量统计
    if let Some(usage) = usage_obj {
        if !usage.is_empty() {
            consolidated.insert("usage".to_string(), Value::Object(usage));
        }
    }

    Value::Object(consolidated)
}

fn consolidate_non_streaming_response(
    raw_json: &Value,
    log: &ProxyRequestLog,
    headers_map: &serde_json::Map<String, Value>,
) -> Option<Value> {
    // 若本就已是统一的 consolidated 格式 (例如带有顶层 thinking/tool_calls 且无 choices/candidates)，直接返回 None
    if (raw_json.get("thinking").is_some()
        || raw_json.get("tool_calls").is_some()
        || raw_json.get("_session_thinking_id").is_some())
        && raw_json.get("choices").is_none()
        && raw_json.get("candidates").is_none()
    {
        return None;
    }

    let mut thinking_content = String::new();
    let mut thinking_signature = String::new();
    let mut response_content = String::new();
    let mut tool_calls = Vec::new();

    // 1. OpenAI 格式 (choices)
    if let Some(choices) = raw_json.get("choices").and_then(|c| c.as_array()) {
        for choice in choices {
            if let Some(msg) = choice.get("message") {
                if let Some(rc) = msg.get("reasoning_content").and_then(|v| v.as_str()) {
                    thinking_content.push_str(rc);
                }
                if let Some(th) = msg.get("thinking").and_then(|v| v.as_str()) {
                    thinking_content.push_str(th);
                }
                if let Some(sig) = msg
                    .get("thoughtSignature")
                    .or_else(|| msg.get("thought_signature"))
                    .or_else(|| msg.get("signature"))
                    .and_then(|v| v.as_str())
                {
                    thinking_signature = sig.to_string();
                }
                if let Some(c) = msg.get("content").and_then(|v| v.as_str()) {
                    response_content.push_str(c);
                }
                if let Some(tcs) = msg.get("tool_calls").and_then(|t| t.as_array()) {
                    for tc in tcs {
                        let id = tc.get("id").and_then(|v| v.as_str()).unwrap_or("");
                        let name = tc
                            .get("function")
                            .and_then(|f| f.get("name"))
                            .and_then(|n| n.as_str())
                            .unwrap_or("");
                        let args = tc
                            .get("function")
                            .and_then(|f| f.get("arguments"))
                            .map(|a| {
                                if let Some(s) = a.as_str() {
                                    s.to_string()
                                } else {
                                    a.to_string()
                                }
                            })
                            .unwrap_or_else(|| "{}".to_string());
                        tool_calls.push(serde_json::json!({
                            "id": id,
                            "type": "function",
                            "function": { "name": name, "arguments": args }
                        }));
                    }
                }
            }
        }
    }
    // 2. Gemini 格式 (candidates)
    else if let Some(candidates) = raw_json.get("candidates").and_then(|c| c.as_array()) {
        for cand in candidates {
            if let Some(parts) = cand
                .get("content")
                .and_then(|c| c.get("parts"))
                .and_then(|p| p.as_array())
            {
                for part in parts {
                    if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                        if part
                            .get("thought")
                            .and_then(|t| t.as_bool())
                            .unwrap_or(false)
                        {
                            thinking_content.push_str(text);
                        } else {
                            response_content.push_str(text);
                        }
                    }
                    if let Some(sig) = part
                        .get("thoughtSignature")
                        .or_else(|| part.get("thought_signature"))
                        .or_else(|| part.get("signature"))
                        .or_else(|| {
                            part.get("functionCall")
                                .and_then(|fc| fc.get("thoughtSignature"))
                        })
                        .or_else(|| {
                            part.get("functionCall")
                                .and_then(|fc| fc.get("thought_signature"))
                        })
                        .and_then(|s| s.as_str())
                    {
                        thinking_signature = sig.to_string();
                    }
                    if let Some(fc) = part.get("functionCall") {
                        if let Some(name) = fc.get("name").and_then(|n| n.as_str()) {
                            let call_id = if let Some(id_str) = fc
                                .get("id")
                                .and_then(|i| i.as_str())
                                .filter(|s| !s.is_empty())
                            {
                                id_str.to_string()
                            } else {
                                crate::proxy::thinking_store::synthesize_tool_id(
                                    name,
                                    fc.get("args"),
                                    "root",
                                    tool_calls.len(),
                                )
                            };
                            let args = fc
                                .get("args")
                                .map(|a| a.to_string())
                                .unwrap_or_else(|| "{}".to_string());
                            tool_calls.push(serde_json::json!({
                                "id": call_id,
                                "type": "function",
                                "function": { "name": name, "arguments": args }
                            }));
                        }
                    }
                }
            }
        }
    }
    // 3. Claude 格式 (content 数组)
    else if let Some(content) = raw_json.get("content").and_then(|c| c.as_array()) {
        for block in content {
            match block.get("type").and_then(|t| t.as_str()) {
                Some("thinking") => {
                    if let Some(th) = block.get("thinking").and_then(|t| t.as_str()) {
                        thinking_content.push_str(th);
                    }
                    if let Some(sig) = block
                        .get("signature")
                        .or_else(|| block.get("thought_signature"))
                        .or_else(|| block.get("thoughtSignature"))
                        .and_then(|s| s.as_str())
                    {
                        thinking_signature = sig.to_string();
                    }
                }
                Some("text") => {
                    if let Some(txt) = block.get("text").and_then(|t| t.as_str()) {
                        response_content.push_str(txt);
                    }
                }
                Some("tool_use") => {
                    let id = block.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let args = block
                        .get("input")
                        .map(|a| a.to_string())
                        .unwrap_or_else(|| "{}".to_string());
                    tool_calls.push(serde_json::json!({
                        "id": id,
                        "type": "function",
                        "function": { "name": name, "arguments": args }
                    }));
                }
                _ => {}
            }
        }
    }

    if thinking_content.is_empty()
        && thinking_signature.is_empty()
        && response_content.is_empty()
        && tool_calls.is_empty()
    {
        return None;
    }

    // 提取耗时诊断
    let mut timing_obj = serde_json::Map::new();
    for key in [
        "clean_ms",
        "norm_ms",
        "thinking_ms",
        "ttft_ms",
        "stream_ms",
        "total_ms",
    ] {
        let hdr_key = format!("x-timing-{}", key);
        if let Some(val) = headers_map.get(&hdr_key).and_then(|v| v.as_str()) {
            if let Ok(n) = val.parse::<f64>() {
                let out_key = key.replace("_ms", "_s");
                timing_obj.insert(out_key, serde_json::json!(n / 1000.0));
            }
        }
    }
    if !timing_obj.contains_key("total_s") {
        timing_obj.insert(
            "total_s".to_string(),
            serde_json::json!(log.duration as f64 / 1000.0),
        );
    }

    // 提取 Token 用量与缓存命中率
    let usage_source = raw_json
        .get("usage")
        .or_else(|| raw_json.get("usageMetadata"));
    let mut usage_obj = serde_json::Map::new();
    let input_toks = log
        .input_tokens
        .or_else(|| usage_source.and_then(extract_input_tokens))
        .unwrap_or(0);
    let output_toks = log
        .output_tokens
        .or_else(|| usage_source.and_then(extract_output_tokens))
        .unwrap_or(0);
    let cached_toks = log
        .cached_tokens
        .or_else(|| usage_source.and_then(extract_cached_tokens))
        .unwrap_or(0);
    let reasoning_toks = usage_source.and_then(extract_reasoning_tokens).unwrap_or(0);

    let total_in = if cached_toks > input_toks {
        input_toks + cached_toks
    } else {
        input_toks
    };
    let total_toks = total_in + output_toks;

    usage_obj.insert("input_tokens".to_string(), serde_json::json!(total_in));
    usage_obj.insert("output_tokens".to_string(), serde_json::json!(output_toks));
    usage_obj.insert("total_tokens".to_string(), serde_json::json!(total_toks));
    if cached_toks > 0 {
        usage_obj.insert("cached_tokens".to_string(), serde_json::json!(cached_toks));
        if total_in > 0 {
            let hit_rate = (cached_toks as f64 / total_in as f64 * 100.0)
                .min(100.0)
                .max(0.0);
            usage_obj.insert(
                "cache_hit_rate".to_string(),
                serde_json::json!(format!("{:.1}%", hit_rate)),
            );
        }
    }
    if reasoning_toks > 0 {
        usage_obj.insert(
            "reasoning_tokens".to_string(),
            serde_json::json!(reasoning_toks),
        );
    }

    Some(build_canonical_consolidated_response(
        thinking_content,
        thinking_signature,
        response_content,
        tool_calls,
        log.session_id.as_deref(),
        &log.id,
        timing_obj,
        Some(usage_obj),
    ))
}

pub async fn monitor_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let _logging_enabled = state.monitor.is_enabled();

    let method = request.method().to_string();
    let uri = request.uri().to_string();

    if uri.contains("event_logging") || uri.contains("/api/") || uri.starts_with("/internal/") {
        return next.run(request).await;
    }

    let start = Instant::now();

    // Extract client IP from headers (X-Forwarded-For or X-Real-IP)
    // IMPORTANT: Extract from Request headers, not Response headers (since we want the client's IP)
    // Note: We need to do this BEFORE consuming the request body if possible, or extract it from the original request
    let client_ip = request
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or(s).trim().to_string())
        .or_else(|| {
            request
                .headers()
                .get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        });

    let user_agent = request
        .headers()
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let request_content_type = request
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let mut model = if uri.contains("/v1beta/models/") {
        uri.split("/v1beta/models/")
            .nth(1)
            .and_then(|s| s.split(':').next())
            .map(|s| s.to_string())
    } else {
        None
    };

    let request_headers_json =
        crate::proxy::payload_audit::headers_to_redacted_json(request.headers());

    let request_body_str;

    // [FIX] 从请求 extensions 提取 UserTokenIdentity (由 Auth 中间件注入)
    // 必须在处理 request body 之前提取，因为 into_parts() 后需要保留这个值
    let user_token_identity = request.extensions().get::<UserTokenIdentity>().cloned();

    let request = if method == "POST" {
        let (parts, body) = request.into_parts();
        match axum::body::to_bytes(body, MAX_REQUEST_LOG_SIZE).await {
            Ok(bytes) => {
                request_body_str = if request_content_type.starts_with("multipart/form-data") {
                    if let Some((summary, multipart_model)) =
                        summarize_multipart_request(&bytes, &request_content_type, &uri)
                    {
                        if model.is_none() {
                            model = multipart_model;
                        }
                        Some(summary)
                    } else {
                        Some(format!(
                            "[Multipart Request Data: {} bytes, failed to parse summary]",
                            bytes.len()
                        ))
                    }
                } else if let Ok(s) = std::str::from_utf8(&bytes) {
                    if model.is_none() {
                        model = serde_json::from_slice::<Value>(&bytes).ok().and_then(|v| {
                            v.get("model")
                                .and_then(|m| m.as_str())
                                .map(|s| s.to_string())
                        });
                    }
                    Some(s.to_string())
                } else {
                    Some("[Binary Request Data]".to_string())
                };
                Request::from_parts(parts, Body::from(bytes))
            }
            Err(_) => {
                request_body_str = None;
                Request::from_parts(parts, Body::empty())
            }
        }
    } else {
        request_body_str = None;
        request
    };

    let upstream_holder = UpstreamRequestBodyHolder::new();
    let mut request = request;
    request.extensions_mut().insert(upstream_holder.clone());

    let response = crate::proxy::monitor::CURRENT_UPSTREAM_CAPTURE
        .scope(upstream_holder.clone(), next.run(request))
        .await;
    let upstream_request_body = upstream_holder.take();
    let upstream_request_headers = upstream_holder.take_headers();
    let response_headers_json =
        crate::proxy::payload_audit::headers_to_redacted_json(response.headers());

    // user_token_identity 已在上面从请求 extensions 中提取

    let duration = start.elapsed().as_millis() as u64;
    let status = response.status().as_u16();

    // 过滤健康检查请求，避免每 30 秒探针刷屏淹没真实业务日志 (Issue #3498)
    // 1. 默认仅过滤成功的 GET 健康检查 (2xx)，非 GET 请求或异常状态 (如 503) 依然记录以供排障；
    // 2. 联动面板胶囊开关 (capture_health_logs) 与环境变量 (ABV_LOG_HEALTH_CHECKS)。
    let capture_health_enabled =
        state.monitor.is_capture_health_logs() || should_log_health_checks();
    if is_health_check_path(&uri)
        && method == "GET"
        && response.status().is_success()
        && !capture_health_enabled
    {
        return response;
    }

    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    // Extract account email from X-Account-Email header if present
    let account_email = response
        .headers()
        .get("X-Account-Email")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // Extract mapped model from X-Mapped-Model header if present
    let mapped_model = response
        .headers()
        .get("X-Mapped-Model")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // Determine protocol from URL path
    let protocol = if uri.contains("/v1/messages") {
        Some("anthropic".to_string())
    } else if uri.contains("/v1beta/models") {
        Some("gemini".to_string())
    } else if uri.starts_with("/v1/") {
        Some("openai".to_string())
    } else {
        None
    };

    // Client IP has been extracted at the beginning of the function

    // Extract username from UserTokenIdentity if present
    let username = user_token_identity
        .as_ref()
        .map(|identity| identity.username.clone());
    let is_image_route = uri.contains("/v1/images/");

    // Extract session ID from response headers (e.g. X-Session-Id or X-Antigravity-Session-Id)
    let session_id = response
        .headers()
        .get("X-Session-Id")
        .or_else(|| response.headers().get("X-Antigravity-Session-Id"))
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let monitor = state.monitor.clone();
    let mut log = ProxyRequestLog {
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now().timestamp_millis(),
        method,
        url: uri,
        status,
        duration,
        model,
        mapped_model,
        account_email,
        client_ip,
        error: None,
        request_body: request_body_str,
        upstream_request_body,
        response_body: None,
        request_headers: Some(request_headers_json),
        upstream_request_headers,
        response_headers: Some(response_headers_json),
        input_tokens: None,
        output_tokens: None,
        cached_tokens: None,
        protocol,
        username,
        session_id,
    };

    if content_type.contains("text/event-stream") {
        let (parts, body) = response.into_parts();
        let mut stream = body.into_data_stream();
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        let start_instant = start;

        tokio::spawn(async move {
            let stream_start = std::time::Instant::now();
            let mut all_stream_data = Vec::new();
            let mut last_few_bytes = Vec::new();

            while let Some(chunk_res) = next_chunk_while_receiver_open(&mut stream, &tx).await {
                if let Ok(chunk) = chunk_res {
                    all_stream_data.extend_from_slice(&chunk);

                    if chunk.len() > 8192 {
                        last_few_bytes = chunk.slice(chunk.len() - 8192..).to_vec();
                    } else {
                        last_few_bytes.extend_from_slice(&chunk);
                        if last_few_bytes.len() > 8192 {
                            last_few_bytes.drain(0..last_few_bytes.len() - 8192);
                        }
                    }
                    if tx.send(Ok::<_, axum::Error>(chunk)).await.is_err() {
                        break;
                    }
                } else if let Err(e) = chunk_res {
                    if tx.send(Err(axum::Error::new(e))).await.is_err() {
                        break;
                    }
                }
            }
            drop(stream);

            let stream_ms = stream_start.elapsed().as_micros() as f64 / 1000.0;
            let total_ms = start_instant.elapsed().as_micros() as f64 / 1000.0;
            log.duration = total_ms.round() as u64;

            let mut headers_map: serde_json::Map<String, Value> = log
                .response_headers
                .as_ref()
                .and_then(|h| serde_json::from_str(h).ok())
                .unwrap_or_default();

            headers_map.insert(
                "x-timing-stream-ms".to_string(),
                serde_json::json!(format!("{:.3}", stream_ms)),
            );
            headers_map.insert(
                "x-timing-total-ms".to_string(),
                serde_json::json!(format!("{:.3}", total_ms)),
            );
            log.response_headers = serde_json::to_string(&Value::Object(headers_map.clone())).ok();

            // Parse and consolidate stream data into readable format
            if let Ok(full_response) = std::str::from_utf8(&all_stream_data) {
                let mut thinking_content = String::new();
                let mut response_content = String::new();
                let mut thinking_signature = String::new();
                let mut tool_calls: Vec<Value> = Vec::new();
                let mut cached_tokens: Option<u32> = None;
                let mut reasoning_tokens: Option<u32> = None;

                for line in full_response.lines() {
                    if !line.starts_with("data: ") {
                        continue;
                    }
                    let json_str = line.trim_start_matches("data: ").trim();
                    if json_str == "[DONE]" {
                        continue;
                    }

                    if let Ok(json) = serde_json::from_str::<Value>(json_str) {
                        // OpenAI format: choices[0].delta.content / reasoning_content / tool_calls
                        if let Some(choices) = json.get("choices").and_then(|c| c.as_array()) {
                            for choice in choices {
                                if let Some(delta) = choice.get("delta") {
                                    // Thinking/reasoning content
                                    if let Some(thinking) =
                                        delta.get("reasoning_content").and_then(|v| v.as_str())
                                    {
                                        thinking_content.push_str(thinking);
                                    }
                                    // Thinking signature in OpenAI delta
                                    if let Some(sig) = delta
                                        .get("signature")
                                        .or_else(|| delta.get("thought_signature"))
                                        .and_then(|v| v.as_str())
                                    {
                                        thinking_signature = sig.to_string();
                                    }
                                    // Main response content
                                    if let Some(content) =
                                        delta.get("content").and_then(|v| v.as_str())
                                    {
                                        response_content.push_str(content);
                                    }
                                    // Tool calls
                                    if let Some(delta_tool_calls) =
                                        delta.get("tool_calls").and_then(|t| t.as_array())
                                    {
                                        for tc in delta_tool_calls {
                                            if let Some(index) =
                                                tc.get("index").and_then(|i| i.as_u64())
                                            {
                                                let idx = index as usize;
                                                while tool_calls.len() <= idx {
                                                    tool_calls.push(serde_json::json!({
                                                        "id": "",
                                                        "type": "function",
                                                        "function": { "name": "", "arguments": "" }
                                                    }));
                                                }
                                                let current_tc = &mut tool_calls[idx];
                                                if let Some(id) =
                                                    tc.get("id").and_then(|v| v.as_str())
                                                {
                                                    current_tc["id"] =
                                                        Value::String(id.to_string());
                                                }
                                                if let Some(func) = tc.get("function") {
                                                    if let Some(name) =
                                                        func.get("name").and_then(|v| v.as_str())
                                                    {
                                                        current_tc["function"]["name"] =
                                                            Value::String(name.to_string());
                                                    }
                                                    if let Some(args) = func
                                                        .get("arguments")
                                                        .and_then(|v| v.as_str())
                                                    {
                                                        let old_args = current_tc["function"]
                                                            ["arguments"]
                                                            .as_str()
                                                            .unwrap_or("");
                                                        current_tc["function"]["arguments"] =
                                                            Value::String(format!(
                                                                "{}{}",
                                                                old_args, args
                                                            ));
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // Gemini format: candidates[0].content.parts
                        if let Some(candidates) = json.get("candidates").and_then(|c| c.as_array())
                        {
                            for cand in candidates {
                                if let Some(content) = cand.get("content") {
                                    if let Some(parts) =
                                        content.get("parts").and_then(|p| p.as_array())
                                    {
                                        for part in parts {
                                            if let Some(text) =
                                                part.get("text").and_then(|t| t.as_str())
                                            {
                                                if part
                                                    .get("thought")
                                                    .and_then(|t| t.as_bool())
                                                    .unwrap_or(false)
                                                {
                                                    thinking_content.push_str(text);
                                                } else {
                                                    response_content.push_str(text);
                                                }
                                            }
                                            let sig = part
                                                .get("thoughtSignature")
                                                .or_else(|| part.get("thought_signature"))
                                                .or_else(|| part.get("signature"))
                                                .or_else(|| {
                                                    part.get("functionCall")
                                                        .and_then(|fc| fc.get("thoughtSignature"))
                                                })
                                                .or_else(|| {
                                                    part.get("functionCall")
                                                        .and_then(|fc| fc.get("thought_signature"))
                                                })
                                                .and_then(|s| s.as_str());
                                            if let Some(s) = sig {
                                                thinking_signature = s.to_string();
                                            }
                                            if let Some(fc) = part.get("functionCall") {
                                                if let Some(name) =
                                                    fc.get("name").and_then(|n| n.as_str())
                                                {
                                                    let call_id = if let Some(id_str) = fc
                                                        .get("id")
                                                        .and_then(|i| i.as_str())
                                                        .filter(|s| !s.is_empty())
                                                    {
                                                        id_str.to_string()
                                                    } else {
                                                        crate::proxy::thinking_store::synthesize_tool_id(
                                                            name,
                                                            fc.get("args"),
                                                            "root",
                                                            tool_calls.len(),
                                                        )
                                                    };
                                                    let args = fc
                                                        .get("args")
                                                        .map(|a| a.to_string())
                                                        .unwrap_or_else(|| "{}".to_string());
                                                    tool_calls.push(serde_json::json!({
                                                        "id": call_id,
                                                        "type": "function",
                                                        "function": { "name": name, "arguments": args }
                                                    }));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // Claude/Anthropic format: content_block_start, content_block_delta, etc.
                        let msg_type = json.get("type").and_then(|t| t.as_str());
                        match msg_type {
                            Some("content_block_start") => {
                                if let (Some(index), Some(block)) = (
                                    json.get("index").and_then(|i| i.as_u64()),
                                    json.get("content_block"),
                                ) {
                                    let idx = index as usize;
                                    if block.get("type").and_then(|t| t.as_str())
                                        == Some("tool_use")
                                    {
                                        let id =
                                            block.get("id").and_then(|v| v.as_str()).unwrap_or("");
                                        let name = block
                                            .get("name")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("");
                                        while tool_calls.len() <= idx {
                                            tool_calls.push(Value::Null);
                                        }
                                        tool_calls[idx] = serde_json::json!({
                                            "id": id,
                                            "type": "function",
                                            "function": { "name": name, "arguments": "" }
                                        });
                                    }
                                    if let Some(thinking) =
                                        block.get("thinking").and_then(|v| v.as_str())
                                    {
                                        thinking_content.push_str(thinking);
                                    }
                                    if let Some(sig) = block
                                        .get("signature")
                                        .or_else(|| block.get("thought_signature"))
                                        .or_else(|| block.get("thoughtSignature"))
                                        .and_then(|v| v.as_str())
                                    {
                                        thinking_signature = sig.to_string();
                                    }
                                }
                            }
                            Some("content_block_delta") => {
                                if let (Some(index), Some(delta)) = (
                                    json.get("index").and_then(|i| i.as_u64()),
                                    json.get("delta"),
                                ) {
                                    let idx = index as usize;

                                    // Tool use input delta (Anthropic 官方协议为 partial_json 增量)
                                    let delta_json = delta
                                        .get("partial_json")
                                        .or_else(|| delta.get("input_json_delta"))
                                        .or_else(|| delta.get("text"))
                                        .and_then(|v| v.as_str());

                                    if let Some(delta_str) = delta_json {
                                        if idx < tool_calls.len() && !tool_calls[idx].is_null() {
                                            let old_args = tool_calls[idx]["function"]["arguments"]
                                                .as_str()
                                                .unwrap_or("");
                                            tool_calls[idx]["function"]["arguments"] =
                                                Value::String(format!("{}{}", old_args, delta_str));
                                        }
                                    }
                                    // Legacy/Native thinking block
                                    if let Some(thinking) =
                                        delta.get("thinking").and_then(|v| v.as_str())
                                    {
                                        thinking_content.push_str(thinking);
                                    }
                                    // Thinking signature in delta (Claude signature_delta or direct signature)
                                    if let Some(sig) = delta
                                        .get("signature")
                                        .or_else(|| delta.get("thought_signature"))
                                        .or_else(|| delta.get("thoughtSignature"))
                                        .and_then(|v| v.as_str())
                                    {
                                        thinking_signature = sig.to_string();
                                    }
                                    // Text content
                                    if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                                        response_content.push_str(text);
                                    }
                                }
                            }
                            Some("message_delta") => {
                                if let Some(delta) = json.get("delta") {
                                    if let Some(usage) = delta.get("usage") {
                                        if let Some(output_tokens) =
                                            usage.get("output_tokens").and_then(|v| v.as_u64())
                                        {
                                            log.output_tokens = Some(output_tokens as u32);
                                        }
                                    }
                                }
                            }
                            Some("response.output_text.delta") => {
                                if let Some(text) = json.get("delta").and_then(|v| v.as_str()) {
                                    response_content.push_str(text);
                                }
                            }
                            Some("response.reasoning_summary_text.delta") => {
                                if let Some(text) = json.get("delta").and_then(|v| v.as_str()) {
                                    thinking_content.push_str(text);
                                }
                            }
                            Some("response.output_item.added") => {
                                if let Some(item) = json.get("item") {
                                    if item.get("type").and_then(|t| t.as_str())
                                        == Some("function_call")
                                    {
                                        let name =
                                            item.get("name").and_then(|v| v.as_str()).unwrap_or("");
                                        let id = item
                                            .get("call_id")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("");
                                        tool_calls.push(serde_json::json!({
                                            "id": id,
                                            "type": "function",
                                            "function": { "name": name, "arguments": "" }
                                        }));
                                    }
                                }
                            }
                            Some("response.function_call_arguments.delta") => {
                                if let Some(delta) = json.get("delta").and_then(|v| v.as_str()) {
                                    if let Some(last_tc) = tool_calls.last_mut() {
                                        let old_args =
                                            last_tc["function"]["arguments"].as_str().unwrap_or("");
                                        last_tc["function"]["arguments"] =
                                            Value::String(format!("{}{}", old_args, delta));
                                    }
                                }
                            }
                            _ => {}
                        }

                        // Legacy Claude delta (for older implementations or simplified streams)
                        if msg_type.is_none() {
                            if let Some(delta) = json.get("delta") {
                                // Thinking block
                                if let Some(thinking) =
                                    delta.get("thinking").and_then(|v| v.as_str())
                                {
                                    thinking_content.push_str(thinking);
                                }
                                // Thinking signature
                                if let Some(sig) = delta.get("signature").and_then(|v| v.as_str()) {
                                    thinking_signature = sig.to_string();
                                }
                                // Text content
                                if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                                    response_content.push_str(text);
                                }
                            }
                        }

                        // Token usage extraction
                        if let Some(usage) = json
                            .get("usage")
                            .or(json.get("usageMetadata"))
                            .or(json.get("response").and_then(|r| r.get("usage")))
                            .or(json.get("response").and_then(|r| r.get("usageMetadata")))
                        {
                            log.input_tokens = extract_input_tokens(usage);
                            log.output_tokens = extract_output_tokens(usage);
                            cached_tokens = cached_tokens.or_else(|| extract_cached_tokens(usage));
                            log.cached_tokens = log.cached_tokens.or(cached_tokens);
                            reasoning_tokens =
                                reasoning_tokens.or_else(|| extract_reasoning_tokens(usage));

                            if log.input_tokens.is_none() && log.output_tokens.is_none() {
                                log.output_tokens = usage
                                    .get("total_tokens")
                                    .or(usage.get("totalTokenCount"))
                                    .and_then(|v| v.as_u64())
                                    .map(|v| v as u32);
                            }
                        }
                    }
                }

                // [Timing Diagnostics] 注入耗时诊断元数据 (秒)
                let mut timing_obj = serde_json::Map::new();
                if let Some(clean) = headers_map
                    .get("x-timing-clean-ms")
                    .and_then(|v| v.as_str())
                {
                    if let Ok(n) = clean.parse::<f64>() {
                        timing_obj.insert("clean_s".to_string(), serde_json::json!(n / 1000.0));
                    }
                }
                if let Some(norm) = headers_map.get("x-timing-norm-ms").and_then(|v| v.as_str()) {
                    if let Ok(n) = norm.parse::<f64>() {
                        timing_obj.insert("norm_s".to_string(), serde_json::json!(n / 1000.0));
                    }
                }
                if let Some(th) = headers_map
                    .get("x-timing-thinking-ms")
                    .and_then(|v| v.as_str())
                {
                    if let Ok(n) = th.parse::<f64>() {
                        timing_obj.insert("thinking_s".to_string(), serde_json::json!(n / 1000.0));
                    }
                }
                if let Some(ttft) = headers_map.get("x-timing-ttft-ms").and_then(|v| v.as_str()) {
                    if let Ok(n) = ttft.parse::<f64>() {
                        timing_obj.insert("ttft_s".to_string(), serde_json::json!(n / 1000.0));
                    }
                }
                timing_obj.insert(
                    "stream_s".to_string(),
                    serde_json::json!(stream_ms / 1000.0),
                );
                timing_obj.insert("total_s".to_string(), serde_json::json!(total_ms / 1000.0));

                // 优先从 tail 兜底提取 token（若当前尚无 token 数据）
                if log.input_tokens.is_none() && log.output_tokens.is_none() {
                    if let Ok(full_tail) = std::str::from_utf8(&last_few_bytes) {
                        for line in full_tail.lines().rev() {
                            if line.starts_with("data: ")
                                && (line.contains("\"usage\"")
                                    || line.contains("\"usageMetadata\""))
                            {
                                let json_str = line.trim_start_matches("data: ").trim();
                                if let Ok(json) = serde_json::from_str::<Value>(json_str) {
                                    if let Some(usage) = json
                                        .get("usage")
                                        .or(json.get("usageMetadata"))
                                        .or(json.get("response").and_then(|r| r.get("usage")))
                                        .or(json
                                            .get("response")
                                            .and_then(|r| r.get("usageMetadata")))
                                    {
                                        log.input_tokens = extract_input_tokens(usage);
                                        log.output_tokens = extract_output_tokens(usage);
                                        cached_tokens =
                                            cached_tokens.or_else(|| extract_cached_tokens(usage));
                                        log.cached_tokens = log.cached_tokens.or(cached_tokens);
                                        reasoning_tokens = reasoning_tokens
                                            .or_else(|| extract_reasoning_tokens(usage));
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }

                let has_actual_content = !response_content.is_empty()
                    || !tool_calls.is_empty()
                    || !thinking_content.is_empty();

                let mut usage_obj = serde_json::Map::new();
                if has_actual_content || log.input_tokens.is_some() || log.output_tokens.is_some() {
                    let input_toks = log.input_tokens.unwrap_or(0);
                    let output_toks = log.output_tokens.unwrap_or(0);
                    let cached_toks = cached_tokens.or(log.cached_tokens).unwrap_or(0);
                    let reasoning_toks = reasoning_tokens.unwrap_or(0);

                    let total_in = if cached_toks > input_toks {
                        input_toks + cached_toks
                    } else {
                        input_toks
                    };
                    let total_toks = total_in + output_toks;

                    usage_obj.insert("input_tokens".to_string(), serde_json::json!(total_in));
                    usage_obj.insert("output_tokens".to_string(), serde_json::json!(output_toks));
                    usage_obj.insert("total_tokens".to_string(), serde_json::json!(total_toks));
                    if cached_toks > 0 {
                        usage_obj
                            .insert("cached_tokens".to_string(), serde_json::json!(cached_toks));
                        if total_in > 0 {
                            let hit_rate = (cached_toks as f64 / total_in as f64 * 100.0)
                                .min(100.0)
                                .max(0.0);
                            usage_obj.insert(
                                "cache_hit_rate".to_string(),
                                serde_json::json!(format!("{:.1}%", hit_rate)),
                            );
                        }
                    }
                    if reasoning_toks > 0 {
                        usage_obj.insert(
                            "reasoning_tokens".to_string(),
                            serde_json::json!(reasoning_toks),
                        );
                    }
                }

                // 🌟 构建统一的满血简要版响应报文 (包含网关权威签名回填与确定性 Tool ID)
                let consolidated = build_canonical_consolidated_response(
                    thinking_content,
                    thinking_signature,
                    response_content,
                    tool_calls,
                    log.session_id.as_deref(),
                    &log.id,
                    timing_obj,
                    if usage_obj.is_empty() {
                        None
                    } else {
                        Some(usage_obj)
                    },
                );

                if consolidated
                    .as_object()
                    .map(|m| m.is_empty())
                    .unwrap_or(true)
                {
                    // Fallback: store raw SSE data if parsing failed
                    log.response_body = Some(full_response.to_string());
                } else {
                    log.response_body = Some(
                        serde_json::to_string_pretty(&consolidated)
                            .unwrap_or_else(|_| full_response.to_string()),
                    );
                }
            } else {
                log.response_body = Some(format!(
                    "[Binary Stream Data: {} bytes]",
                    all_stream_data.len()
                ));
            }

            // Fallback token extraction from tail if not already extracted
            if log.input_tokens.is_none() && log.output_tokens.is_none() {
                if let Ok(full_tail) = std::str::from_utf8(&last_few_bytes) {
                    for line in full_tail.lines().rev() {
                        if line.starts_with("data: ")
                            && (line.contains("\"usage\"") || line.contains("\"usageMetadata\""))
                        {
                            let json_str = line.trim_start_matches("data: ").trim();
                            if let Ok(json) = serde_json::from_str::<Value>(json_str) {
                                if let Some(usage) = json
                                    .get("usage")
                                    .or(json.get("usageMetadata"))
                                    .or(json.get("response").and_then(|r| r.get("usage")))
                                    .or(json.get("response").and_then(|r| r.get("usageMetadata")))
                                {
                                    log.input_tokens = extract_input_tokens(usage);
                                    log.output_tokens = extract_output_tokens(usage);
                                    log.cached_tokens =
                                        log.cached_tokens.or_else(|| extract_cached_tokens(usage));
                                    break;
                                }
                            }
                        }
                    }
                }
            }

            if log.status >= 400 {
                log.error = Some("Stream Error or Failed".to_string());
            }

            // Fallback input token estimation prefers the transit (upstream) body
            if log.input_tokens.is_none() {
                let estimated = log
                    .upstream_request_body
                    .as_ref()
                    .or(log.request_body.as_ref())
                    .map(|body| {
                        crate::proxy::mappers::context_manager::estimate_raw_tokens_from_payload(
                            body,
                        )
                    })
                    .unwrap_or(0);
                if estimated > 0 {
                    log.input_tokens = Some(estimated);
                }
            }

            // Record User Token Usage
            record_user_token_usage(&user_token_identity, &log, user_agent.clone());

            monitor.log_request(log).await;
        });

        Response::from_parts(
            parts,
            Body::from_stream(tokio_stream::wrappers::ReceiverStream::new(rx)),
        )
    } else if content_type.contains("application/json") || content_type.contains("text/") {
        let total_ms = start.elapsed().as_micros() as f64 / 1000.0;
        log.duration = total_ms.round() as u64;

        let mut headers_map: serde_json::Map<String, Value> = log
            .response_headers
            .as_ref()
            .and_then(|h| serde_json::from_str(h).ok())
            .unwrap_or_default();

        headers_map.insert(
            "x-timing-total-ms".to_string(),
            serde_json::json!(format!("{:.3}", total_ms)),
        );
        log.response_headers = serde_json::to_string(&Value::Object(headers_map.clone())).ok();

        let (parts, body) = response.into_parts();
        match axum::body::to_bytes(body, MAX_RESPONSE_LOG_SIZE).await {
            Ok(bytes) => {
                if let Ok(s) = std::str::from_utf8(&bytes) {
                    if let Ok(json) = serde_json::from_str::<Value>(&s) {
                        // 支持 OpenAI "usage" 或 Gemini "usageMetadata"
                        if let Some(usage) = json
                            .get("usage")
                            .or(json.get("usageMetadata"))
                            .or(json.get("response").and_then(|r| r.get("usage")))
                            .or(json.get("response").and_then(|r| r.get("usageMetadata")))
                        {
                            log.input_tokens = extract_input_tokens(usage);
                            log.output_tokens = extract_output_tokens(usage);
                            log.cached_tokens =
                                log.cached_tokens.or_else(|| extract_cached_tokens(usage));

                            if log.input_tokens.is_none() && log.output_tokens.is_none() {
                                log.output_tokens = usage
                                    .get("total_tokens")
                                    .or(usage.get("totalTokenCount"))
                                    .and_then(|v| v.as_u64())
                                    .map(|v| v as u32);
                            }
                        }
                    }
                    if is_image_route {
                        log.response_body = serde_json::from_str::<Value>(&s)
                            .ok()
                            .and_then(|json| summarize_image_json_response(&json))
                            .or_else(|| Some(s.to_string()));
                    } else if let Ok(json) = serde_json::from_str::<Value>(&s) {
                        // 🌟 非流式响应入库统一转换为满血简要版 (包含网关权威签名回填与防雪崩确定性 Tool ID)
                        if let Some(canonical) =
                            consolidate_non_streaming_response(&json, &log, &headers_map)
                        {
                            log.response_body = serde_json::to_string_pretty(&canonical)
                                .ok()
                                .or_else(|| Some(s.to_string()));
                        } else {
                            log.response_body = Some(s.to_string());
                        }
                    } else {
                        log.response_body = Some(s.to_string());
                    }
                } else {
                    log.response_body = Some("[Binary Response Data]".to_string());
                }

                if log.status >= 400 {
                    log.error = log.response_body.clone();
                }

                // Fallback input token estimation prefers the transit (upstream) body
                if log.input_tokens.is_none() {
                    let estimated = log
                        .upstream_request_body
                        .as_ref()
                        .or(log.request_body.as_ref())
                        .map(|body| {
                            crate::proxy::mappers::context_manager::estimate_raw_tokens_from_payload(body)
                        })
                        .unwrap_or(0);
                    if estimated > 0 {
                        log.input_tokens = Some(estimated);
                    }
                }

                // Record User Token Usage
                record_user_token_usage(&user_token_identity, &log, user_agent.clone());

                monitor.log_request(log).await;
                Response::from_parts(parts, Body::from(bytes))
            }
            Err(_) => {
                log.response_body = Some("[Response too large (>100MB)]".to_string());

                // Record User Token Usage (even if too large)
                record_user_token_usage(&user_token_identity, &log, user_agent.clone());

                monitor.log_request(log).await;
                Response::from_parts(parts, Body::empty())
            }
        }
    } else {
        log.response_body = Some(format!("[{}]", content_type));

        // Record User Token Usage
        record_user_token_usage(&user_token_identity, &log, user_agent);

        monitor.log_request(log).await;
        response
    }
}

#[cfg(test)]
mod tests {
    use super::next_chunk_while_receiver_open;
    use futures::stream;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use std::task::Poll;

    struct DropFlag(Arc<AtomicBool>);

    impl Drop for DropFlag {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn receiver_close_stops_waiting_and_drops_source_stream() {
        let dropped = Arc::new(AtomicBool::new(false));
        let drop_flag = DropFlag(dropped.clone());
        let (polled_tx, polled_rx) = tokio::sync::oneshot::channel();
        let (tx, rx) = tokio::sync::mpsc::channel::<()>(1);

        let task = tokio::spawn(async move {
            let mut polled_tx = Some(polled_tx);
            let mut source = stream::poll_fn(move |_| {
                let _ = &drop_flag;
                if let Some(polled_tx) = polled_tx.take() {
                    let _ = polled_tx.send(());
                }
                Poll::<Option<()>>::Pending
            });

            assert!(next_chunk_while_receiver_open(&mut source, &tx)
                .await
                .is_none());
        });

        polled_rx.await.expect("source stream was not polled");
        drop(rx);
        tokio::time::timeout(std::time::Duration::from_secs(1), task)
            .await
            .expect("forwarder did not stop after receiver closed")
            .expect("forwarder task panicked");
        assert!(dropped.load(Ordering::SeqCst));
    }

    #[test]
    fn test_extract_tokens_anthropic() {
        use serde_json::json;
        let usage = json!({
            "input_tokens": 1200,
            "cache_read_input_tokens": 8800,
            "cache_creation_input_tokens": 0,
            "output_tokens": 150
        });
        assert_eq!(super::extract_input_tokens(&usage), Some(10000));
        assert_eq!(super::extract_cached_tokens(&usage), Some(8800));
        assert_eq!(super::extract_output_tokens(&usage), Some(150));
    }

    #[test]
    fn test_extract_tokens_openai_chat() {
        use serde_json::json;
        let usage = json!({
            "prompt_tokens": 10000,
            "completion_tokens": 150,
            "total_tokens": 10150,
            "prompt_tokens_details": {
                "cached_tokens": 8800
            }
        });
        assert_eq!(super::extract_input_tokens(&usage), Some(10000));
        assert_eq!(super::extract_cached_tokens(&usage), Some(8800));
        assert_eq!(super::extract_output_tokens(&usage), Some(150));
    }

    #[test]
    fn test_extract_tokens_openai_responses() {
        use serde_json::json;
        let usage = json!({
            "input_tokens": 10000,
            "output_tokens": 150,
            "total_tokens": 10150,
            "input_tokens_details": {
                "cached_tokens": 8800
            }
        });
        assert_eq!(super::extract_input_tokens(&usage), Some(10000));
        assert_eq!(super::extract_cached_tokens(&usage), Some(8800));
        assert_eq!(super::extract_output_tokens(&usage), Some(150));
    }

    #[test]
    fn test_extract_tokens_gemini_raw() {
        use serde_json::json;
        let usage = json!({
            "promptTokenCount": 10000,
            "candidatesTokenCount": 150,
            "totalTokenCount": 10150,
            "cachedContentTokenCount": 8800
        });
        assert_eq!(super::extract_input_tokens(&usage), Some(10000));
        assert_eq!(super::extract_cached_tokens(&usage), Some(8800));
        assert_eq!(super::extract_output_tokens(&usage), Some(150));
    }

    #[test]
    fn test_is_health_check_path() {
        assert!(super::is_health_check_path("/health"));
        assert!(super::is_health_check_path("/health/"));
        assert!(super::is_health_check_path("/healthz"));
        assert!(super::is_health_check_path("/healthz/"));
        assert!(super::is_health_check_path("/api/health"));
        assert!(super::is_health_check_path("/health?probe=k8s"));
        assert!(super::is_health_check_path("/healthz?t=123"));

        assert!(!super::is_health_check_path("/v1/chat/completions"));
        assert!(!super::is_health_check_path("/v1/models"));
        assert!(!super::is_health_check_path("/api/accounts"));
        assert!(!super::is_health_check_path("/healthy"));
    }
}
