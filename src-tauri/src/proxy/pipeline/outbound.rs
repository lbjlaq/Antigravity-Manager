use super::events::CanonicalStreamEvent;
use super::policy::ProxyProtocol;
use super::usage::CanonicalUsage;
use serde_json::{json, Value};

/// 统一出站萃取对象
#[derive(Debug, Clone, Default)]
pub struct CanonicalEgressPayload {
    pub text: String,
    pub thought: Option<String>,
    pub signature: Option<String>,
    pub tool_calls: Vec<Value>,
    pub usage: CanonicalUsage,
    pub finish_reason: Option<String>,
}

/// 统一出站思考与响应管线（OutboundThinkingPipeline）
/// 负责将上游响应统一收敛为领域对象，触发极简反向入库与审计，并向各大协议差异化散开
pub struct OutboundThinkingPipeline;

impl OutboundThinkingPipeline {
    /// 从 Google Gemini 完整响应中萃取收拢领域对象
    pub fn extract_payload(gemini_resp: &Value) -> CanonicalEgressPayload {
        let mut text = String::new();
        let mut thought = None;
        let mut signature = None;
        let mut tool_calls = Vec::new();

        let raw = gemini_resp.get("response").unwrap_or(gemini_resp);

        if let Some(cands) = raw.get("candidates").and_then(|c| c.as_array()) {
            if let Some(first) = cands.first() {
                if let Some(parts) = first
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
                            if let Some(t) = part.get("text").and_then(|s| s.as_str()) {
                                thought = Some(t.to_string());
                            }
                            if let Some(sig) = part
                                .get("thoughtSignature")
                                .or_else(|| part.get("thought_signature"))
                                .and_then(|s| s.as_str())
                            {
                                signature = Some(sig.to_string());
                            }
                        } else if let Some(fc) = part.get("functionCall") {
                            tool_calls.push(fc.clone());
                            if signature.is_none() {
                                if let Some(sig) = part
                                    .get("thoughtSignature")
                                    .or_else(|| part.get("thought_signature"))
                                    .and_then(|s| s.as_str())
                                {
                                    signature = Some(sig.to_string());
                                }
                            }
                        } else if let Some(t) = part.get("text").and_then(|s| s.as_str()) {
                            text.push_str(t);
                        }
                    }
                }
            }
        }

        let usage = CanonicalUsage::from_gemini(raw);
        let finish_reason = raw
            .get("candidates")
            .and_then(|c| c.get(0))
            .and_then(|cand| cand.get("finishReason"))
            .and_then(|f| f.as_str())
            .map(|s| s.to_string());

        CanonicalEgressPayload {
            text,
            thought,
            signature,
            tool_calls,
            usage,
            finish_reason,
        }
    }

    /// 执行反向入库（极简原则：仅落库实质思考与有效加密签名）
    pub fn auto_ingest(store_key: &str, payload: &CanonicalEgressPayload, message_count: usize) {
        if let Some(ref sig) = payload.signature {
            if sig != crate::proxy::thinking_store::SENTINEL_SIGNATURE && sig.len() >= 50 {
                // 缓存会话签名
                crate::proxy::SignatureCache::global().cache_session_signature(
                    store_key,
                    sig.clone(),
                    message_count,
                );

                // 关联工具调用签名
                for tc in &payload.tool_calls {
                    if let Some(id) = tc.get("id").and_then(|i| i.as_str()) {
                        crate::proxy::SignatureCache::global()
                            .cache_tool_signature(id, sig.clone());
                    }
                }
            }
        }

        // 入库 ThinkingStore
        let has_meaningful_thought = payload
            .thought
            .as_deref()
            .map(crate::proxy::thinking_store::is_meaningful_thought)
            .unwrap_or(false);
        let has_real_signature = payload
            .signature
            .as_deref()
            .map(crate::proxy::thinking_store::is_real_signature)
            .unwrap_or(false);

        if has_meaningful_thought || has_real_signature {
            let tool_ids: Vec<String> = payload
                .tool_calls
                .iter()
                .filter_map(|tc| tc.get("id").and_then(|i| i.as_str()).map(str::to_string))
                .collect();

            let th = payload.thought.clone().unwrap_or_default();
            let fp = crate::proxy::thinking_store::fingerprint(&payload.text, &tool_ids, &[]);

            crate::proxy::thinking_store::ThinkingStore::global().record(
                store_key,
                crate::proxy::thinking_store::ThinkingRecord {
                    fingerprint: fp,
                    thought: th,
                    signature: payload.signature.clone(),
                    tool_ids,
                    tool_names: Vec::new(),
                    visible: payload.text.clone(),
                },
            );
        }
    }

    /// 散开为 OpenAI Chat 协议响应（彻底屏蔽签名，注入标准 usage）
    pub fn diffuse_to_openai_chat(
        payload: &CanonicalEgressPayload,
        model: &str,
        request_id: &str,
    ) -> Value {
        let mut message_obj = json!({
            "role": "assistant",
            "content": if payload.text.is_empty() { Value::Null } else { json!(payload.text) },
        });

        // Chat 协议仅接受 reasoning_content，绝不输出签名！
        if let Some(ref th) = payload.thought {
            message_obj["reasoning_content"] = json!(th);
        }

        if !payload.tool_calls.is_empty() {
            let mut formatted_tcs = Vec::new();
            for (idx, tc) in payload.tool_calls.iter().enumerate() {
                let name = tc.get("name").and_then(|n| n.as_str()).unwrap_or("unknown");
                let args = tc.get("args").cloned().unwrap_or(json!({}));
                let id = tc
                    .get("id")
                    .and_then(|i| i.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("call_{}_{}", name, idx));
                formatted_tcs.push(json!({
                    "id": id,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string())
                    }
                }));
            }
            message_obj["tool_calls"] = json!(formatted_tcs);
        }

        json!({
            "id": request_id,
            "object": "chat.completion",
            "created": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs(),
            "model": model,
            "choices": [
                {
                    "index": 0,
                    "message": message_obj,
                    "finish_reason": payload.finish_reason.as_deref().unwrap_or("stop")
                }
            ],
            "usage": payload.usage.to_openai_chat_usage()
        })
    }

    /// 散开为 Anthropic Claude 协议响应（输出 content_block::thinking 与签名）
    pub fn diffuse_to_claude(
        payload: &CanonicalEgressPayload,
        model: &str,
        request_id: &str,
        scaling_enabled: bool,
        context_limit: u32,
    ) -> Value {
        let mut content = Vec::new();

        // 注入思考块与签名
        if let Some(ref th) = payload.thought {
            let mut th_block = json!({
                "type": "thinking",
                "thinking": th,
            });
            if let Some(ref sig) = payload.signature {
                th_block["signature"] = json!(sig);
            }
            content.push(th_block);
        }

        if !payload.text.is_empty() {
            content.push(json!({
                "type": "text",
                "text": payload.text
            }));
        }

        for (idx, tc) in payload.tool_calls.iter().enumerate() {
            let name = tc.get("name").and_then(|n| n.as_str()).unwrap_or("unknown");
            let args = tc.get("args").cloned().unwrap_or(json!({}));
            let id = tc
                .get("id")
                .and_then(|i| i.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("call_{}_{}", name, idx));
            content.push(json!({
                "type": "tool_use",
                "id": id,
                "name": name,
                "input": args
            }));
        }

        json!({
            "id": request_id,
            "type": "message",
            "role": "assistant",
            "model": model,
            "content": content,
            "stop_reason": payload.finish_reason.as_deref().unwrap_or("end_turn"),
            "stop_sequence": Value::Null,
            "usage": payload.usage.to_claude_usage(scaling_enabled, context_limit)
        })
    }

    /// 散开流式增量事件（Streaming SSE Event Diffuser）
    pub fn diffuse_stream_event(
        event: &CanonicalStreamEvent,
        protocol: ProxyProtocol,
    ) -> Option<String> {
        match event {
            CanonicalStreamEvent::ThoughtDelta(th) => match protocol {
                ProxyProtocol::OpenAIChat => Some(format!(
                    "data: {}\n\n",
                    json!({
                        "choices": [{
                            "index": 0,
                            "delta": { "reasoning_content": th }
                        }]
                    })
                )),
                ProxyProtocol::AnthropicClaude => Some(format!(
                    "event: content_block_delta\ndata: {}\n\n",
                    json!({
                        "type": "content_block_delta",
                        "index": 0,
                        "delta": { "type": "thinking_delta", "thinking": th }
                    })
                )),
                _ => None,
            },
            CanonicalStreamEvent::ThoughtSignature(sig) => {
                // OpenAI Chat 协议严格丢弃签名，不向客户端输出
                if !protocol.emits_signature_to_client() {
                    return None;
                }
                match protocol {
                    ProxyProtocol::AnthropicClaude => Some(format!(
                        "event: content_block_delta\ndata: {}\n\n",
                        json!({
                            "type": "content_block_delta",
                            "index": 0,
                            "delta": { "type": "signature_delta", "signature": sig }
                        })
                    )),
                    _ => None,
                }
            }
            CanonicalStreamEvent::TextDelta(txt) => match protocol {
                ProxyProtocol::OpenAIChat => Some(format!(
                    "data: {}\n\n",
                    json!({
                        "choices": [{
                            "index": 0,
                            "delta": { "content": txt }
                        }]
                    })
                )),
                ProxyProtocol::AnthropicClaude => Some(format!(
                    "event: content_block_delta\ndata: {}\n\n",
                    json!({
                        "type": "content_block_delta",
                        "index": 1,
                        "delta": { "type": "text_delta", "text": txt }
                    })
                )),
                _ => None,
            },
            CanonicalStreamEvent::UsageUpdate(u) => match protocol {
                ProxyProtocol::OpenAIChat => Some(format!(
                    "data: {}\n\n",
                    json!({
                        "choices": [],
                        "usage": u.to_openai_chat_usage()
                    })
                )),
                _ => None,
            },
            CanonicalStreamEvent::Finish { reason, usage } => match protocol {
                ProxyProtocol::OpenAIChat => {
                    let mut obj = json!({
                        "choices": [{
                            "index": 0,
                            "delta": {},
                            "finish_reason": reason
                        }]
                    });
                    if let Some(u) = usage {
                        obj["usage"] = u.to_openai_chat_usage();
                    }
                    Some(format!("data: {}\n\ndata: [DONE]\n\n", obj))
                }
                ProxyProtocol::AnthropicClaude => {
                    Some("event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_string())
                }
                _ => None,
            },
            _ => None,
        }
    }
}
