use std::sync::Arc;
use reqwest::Client;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use crate::modules::proxy::converter;
use uuid::Uuid;

/// Antigravity API 客户端
pub struct GeminiClient {
    client: Client,
}

impl GeminiClient {
    pub fn new(
        timeout_secs: u64, 
        proxy_config: Option<crate::modules::proxy::config::UpstreamProxyConfig>
    ) -> Self {
        Self {
            client: crate::utils::http::create_client_with_proxy(timeout_secs, proxy_config),
        }
    }
    
    /// 发送流式请求到 Antigravity API (Anthropic 格式)
    pub async fn stream_generate_anthropic(
        &self,
        anthropic_request: &converter::AnthropicChatRequest,
        access_token: &str,
        project_id: &str,
        session_id: &str,
        model_mapping: &std::collections::HashMap<String, String>,
        signature_map: Arc<tokio::sync::Mutex<std::collections::HashMap<String, String>>>, // 新增
    ) -> Result<impl futures::Stream<Item = Result<String, String>>, String> {
         // 使用 Antigravity 内部 API
        let url = "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:streamGenerateContent?alt=sse";
        
        let contents = converter::convert_anthropic_to_gemini_contents_ext(anthropic_request, signature_map.clone()).await;
        let model_name = anthropic_request.model.clone();
        
        // System Instruction
        let system_instruction = if let Some(sys) = &anthropic_request.system {
            serde_json::json!({
                "role": "user",
                "parts": [{"text": sys}]
            })
        } else {
             serde_json::json!({
                "role": "user",
                "parts": [{"text": ""}]
            })
        };

        // Generation Config
        let mut generation_config = serde_json::json!({
            "temperature": anthropic_request.temperature.unwrap_or(1.0),
            "topP": anthropic_request.top_p.unwrap_or(0.95),
            "maxOutputTokens": anthropic_request.max_tokens.unwrap_or(16384), // 平衡型配置
            "candidateCount": 1,
        });

        // 注入 Thinking Config (参考 neovate-code)
        // 只有支持思维链的模型才注入，这里简化为判断是否包含 sonnet 或 thinking 字样
        if model_name.contains("sonnet-3-7") || model_name.contains("thinking") || model_name.contains("claude-3-7") {
             if let Some(config) = generation_config.as_object_mut() {
                config.insert("thinkingConfig".to_string(), serde_json::json!({
                    "includeThoughts": true,
                    "thinkingBudget": 8191, // Google Protocol Limit < 8192, 8191 is max safe value
                }));
            }
        }

        // 映射模型名 (Anthropic 模型名 -> Gemini 模型名，暂时直通或简单映射)
        // Claude Code 可能会传 "claude-3-5-sonnet-20240620" 等
        // 目前策略：尝试匹配 gemini 模型，或者默认使用 gemini-3-pro-low 如果传的是 anthropic 名字
        // 鲁棒模糊映射机制 (参考 CLIProxyAPI 经验)
        let initial_mapped = if let Some(mapped) = model_mapping.get(&model_name) {
            tracing::info!("(Anthropic) 基础映射: {} -> {}", model_name, mapped);
            mapped.as_str()
        } else {
            model_name.as_str()
        };

        let lower_name = initial_mapped.to_lowercase();
        // 最终 API 型号转换：将内部型号转换为 Antigravity Daily API 实际支持的名称
        // 参考 CLIProxyAPI 的 modelName2Alias 函数
        let upstream_model = if lower_name == "gemini-3-flash" {
            "gemini-3-flash-preview"
        } else if lower_name == "gemini-3-pro-high" {
            "gemini-3-pro-preview"
        } else if lower_name.starts_with("gemini-") {
             initial_mapped
        } else if lower_name.contains("thinking") {
             // 如果映射结果本身包含 thinking (如 claude-sonnet-4-5-thinking)，尝试直接透传
             initial_mapped
        } else if lower_name.contains("opus") {
            "gemini-3-pro-preview" // 修正: High -> Preview
        } else {
            initial_mapped
        };

        if upstream_model != initial_mapped {
            tracing::info!("(Anthropic) 模型 API 转换: {} -> {}", initial_mapped, upstream_model);
        }

        let request_body = serde_json::json!({
            "project": project_id,
            "requestId": Uuid::new_v4().to_string(),
            "model": upstream_model,
            "userAgent": "antigravity",
            "request": {
                "contents": contents,
                "systemInstruction": system_instruction,
                "generationConfig": generation_config,
                // ✅ 移除 toolConfig 以避免 MALFORMED_FUNCTION_CALL 错误
                // "toolConfig": {
                //     "functionCallingConfig": {
                //         "mode": "VALIDATED"
                //     }
                // },
                // ✅ 暂时移除 tools 以避免 MALFORMED_FUNCTION_CALL 错误, 强制输出文本
                "sessionId": session_id
            }
        });

        // 记录请求详情以便调试 404
        tracing::debug!(
            "(Anthropic) 发起请求: {} -> {} | Project: {}", 
            model_name, upstream_model, project_id
        );
        // tracing::trace!("(Anthropic) 请求体: {}", serde_json::to_string(&request_body).unwrap_or_default());

        let response = self.client
            .post(url)
            .bearer_auth(access_token)
            .header("Host", "daily-cloudcode-pa.sandbox.googleapis.com")
            .header("User-Agent", "claude-cli/1.0.83 (external, cli)")
            .header("X-App", "cli") // 模拟 Claude CLI
            .header("Anthropic-Beta", "claude-code-20250219,interleaved-thinking-2025-05-14") // 启用 Beta 特性
            .header("X-Stainless-Lang", "js")
            .header("X-Stainless-Package-Version", "0.55.1")
            .header("X-Stainless-Os", "MacOS")
            .header("X-Stainless-Arch", "arm64")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| format!("请求失败: {}", e))?;
        
        if !response.status().is_success() {
            let status = response.status();
            // 强制捕获上游详细错误正文，辅助诊断 404/403 (quota, project, etc.)
            let error_details = response.text().await.unwrap_or_else(|_| "无法读取错误详情".to_string());
            tracing::error!(
                "(Anthropic) 请求失败! 状态码: {}, 映射模型: {} (源: {}), 项目: {}, 错误详情: {}",
                status, upstream_model, model_name, project_id, error_details
            );
            return Err(format!("上游服务错误 ({}): {}", status, error_details));
        }

        // 处理流式响应并转换为 Anthropic SSE 格式
        // 注意：Anthropic SSE 格式复杂:
        // event: message_start { "type": "message_start", "message": { ... } }
        // event: content_block_start { "type": "content_block_start", "index": 0, "content_block": { "type": "text", "text": "" } }
        // event: content_block_delta { "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": "Hello" } }
        // event: message_delta { "type": "message_delta", "delta": { "stop_reason": "end_turn", ... } }
        // event: message_stop { "type": "message_stop" }
        
        // 为了简化，我们需要在这里构建流。
        // 因为 Client 层的 stream_generate 返回的是 Result<String, String> 的流，通常是指 payload data。
        // 为了保持一致性，我们在这里返回 "raw" SSE event data 字符串，
        // Server 层负责封装成 event: type \n data: ... 格式？
        // 或者 Server 层直接透传。
        // 观察 server.rs: handle_stream_request 中 Sse::new(sse_stream)，其中 sse_stream yield Event.
        // client 返回 Result<String, ...>，server 把它包装进 data()。
        
        // 问题：Anthropic SSE 需要 `event: name` 字段，而 OpenAI 只需要 `data:`。
        // Axum SSE default event is "message".
        // 所以我们可能需要让 client 返回 (EventType, Data) tuple? 
        // 或者让 client 返回封装好的 Event struct?
        // 为保持最小修改，我们让 client 返回 String，但是这个 String 包含了 Event 的信息？
        // 不行，Server 会再次封装。
        
        // 方案：让 stream_generate_anthropic 返回 `impl Stream<Item = Result<(String, String), String>>`
        // tuple: (event_type, json_data)
        
        // 但是 rust 静态类型要求返回类型一致。现有 stream_generate 返回 `Result<String, String>` (implicit item).
        // 我们可以返回 `Result<AnthropicEvent, String>` enum?
        // 或者简单点，Server 端分别处理。
        
        // 在 client.rs 里我们只负责解析 Gemini 响应。
        // 转换逻辑：Gemini chunk -> Anthropic events (multiple).
        // 一个 Gemini chunk 可能包含 text，对应 content_block_delta。
        // 初始 chunk 对应 message_start + content_block_start。
        // 结束 chunk 对应 message_delta + message_stop。
        
        // 这是个复杂逻辑。
        // 为了不把 client.rs 搞得太乱，建议把 "Gemini Stream -> Anthropic Stream" 的转换逻辑放到 converter.rs 或新的 proxy/anthropic.rs 中？
        // 这里仅负责发起请求，拿到 Gemini 的 ByteStream，然后 map 转换。
        
        let _msg_id = format!("msg_{}", Uuid::new_v4());
        let _created_model = model_name.clone();

        let stream = response.bytes_stream()
            .eventsource()
            .flat_map(move |result| {
                 let sig_map = Arc::clone(&signature_map);
                 match result {
                    Ok(event) => {
                        let data = event.data;
                         if data == "[DONE]" {
                             return futures::stream::iter(vec![Ok("[DONE]".to_string())]);
                         }
                         
                        // Parse Gemini JSON
                        let json: serde_json::Value = match serde_json::from_str(&data) {
                            Ok(j) => j,
                            Err(e) => return futures::stream::iter(vec![Err(format!("解析 Gemini 流失败: {}", e))]),
                        };
                         
                        // 解析 Gemini JSON
                        let candidates = if let Some(c) = json.get("candidates") {
                             c
                        } else if let Some(r) = json.get("response") {
                             r.get("candidates").unwrap_or(&serde_json::Value::Null)
                        } else {
                             &serde_json::Value::Null
                        };

                        let text = candidates.get(0)
                            .and_then(|c| c.get("content"))
                            .and_then(|c| c.get("parts"))
                            .and_then(|p| p.get(0))
                            .and_then(|p| p.get("text"))
                            .and_then(|t| t.as_str())
                            .unwrap_or("");
                        
                        // ✅ 优化日志逻辑
                        if text.is_empty() {
                            // 检查是否有 thoughtSignature (Gemini 思考过程)
                            let has_thought = candidates.get(0)
                                .and_then(|c| c.get("content"))
                                .and_then(|c| c.get("parts"))
                                .and_then(|p| p.get(0))
                                .and_then(|p| p.get("thoughtSignature"))
                                .is_some();
                            
                            // 检查 finishReason
                            let reason = candidates.get(0)
                                .and_then(|c| c.get("finishReason"))
                                .and_then(|f| f.as_str())
                                .unwrap_or("UNKNOWN");

                            if has_thought {
                                tracing::debug!("(Anthropic) 收到 thoughtSignature (思考过程), 原始: {}", serde_json::to_string(&candidates).unwrap_or_default());
                            } else if reason == "MALFORMED_FUNCTION_CALL" {
                                tracing::warn!("(Anthropic) Gemini 工具调用失败 (MALFORMED_FUNCTION_CALL), 请尝试禁用工具或简化 Prompt");
                            } else if reason == "STOP" {
                                // STOP 但 text 为空,可能是只有 thoughtSignature 但没被解析出来,或者是真正的空响应
                                tracing::debug!("(Anthropic) 收到空文本 (STOP), 可能是 metadata, 原始: {}", serde_json::to_string(&candidates).unwrap_or_default());
                            } else {
                                // 其他情况才警告
                                tracing::warn!(
                                    "(Anthropic) Gemini 返回空文本, 原因: {}, 原始 candidates: {}", 
                                    reason,
                                    serde_json::to_string(candidates).unwrap_or_else(|_| "无法序列化".to_string())
                                );
                            }
                        }
                            
                        let gemini_finish_reason = candidates.get(0)
                            .and_then(|c| c.get("finishReason"))
                            .and_then(|f| f.as_str());

                        let finish_reason = match gemini_finish_reason {
                            Some("STOP") => Some("stop"),
                            Some("MAX_TOKENS") => Some("length"),
                            Some("SAFETY") => Some("content_filter"), 
                            _ => None
                        };
                        
                        // 提取思维链信息
                        let part = candidates.get(0)
                            .and_then(|c| c.get("content"))
                            .and_then(|c| c.get("parts"))
                            .and_then(|p| p.get(0));
                            
                        let is_thought = part.and_then(|p| p.get("thought")).and_then(|t| t.as_bool()).unwrap_or(false);
                        let thought_signature = part.and_then(|p| p.get("thoughtSignature")).and_then(|s| s.as_str());
                        
                        // 捕获 thoughtSignature 并存入 map (参考 endsock gist)
                        if let Some(sig) = thought_signature {
                            let mut map = futures::executor::block_on(sig_map.lock());
                            if let Some(resp_id) = json.get("responseId").and_then(|s| s.as_str()) {
                                map.insert(resp_id.to_string(), sig.to_string());
                            } else {
                                map.insert("latest".to_string(), sig.to_string());
                            }
                            tracing::debug!("(Anthropic) 捕获到 thoughtSignature 并已暂存");
                        }

                        // ✅ 方案更新：只有在有实际内容或结束原因时才发送 chunk
                        // 内容包括：text, is_thought==true, 或者有 thoughtSignature
                        let has_content = !text.is_empty() || is_thought || thought_signature.is_some();
                        
                        if !has_content {
                            if let Some(reason) = finish_reason.as_deref() {
                                if reason == "length" || reason == "stop" { 
                                     // 关键：如果没内容就结束了 (MAX_TOKENS 或 STOP)，视为失败，抛出错误触发重试
                                    tracing::warn!("(Anthropic) 检测到空响应且原因为 {}, 触发重试...", reason);
                                    return futures::stream::iter(vec![Err(format!("Gemini 返回空内容 ({})", reason))]);
                                }
                            }
                            
                            if finish_reason.is_none() {
                                tracing::debug!("(Anthropic) 跳过无内容的 chunk (无 text/thought/reason)");
                                return futures::stream::iter(vec![]);
                            }
                        }

                        let chunk = serde_json::json!({
                            "id": json.get("responseId").and_then(|s| s.as_str()).unwrap_or("chatcmpl-stream"), 
                            "object": "chat.completion.chunk",
                            "created": chrono::Utc::now().timestamp(),
                            "model": model_name,
                            "choices": [{
                                "index": 0,
                                "delta": { 
                                    "content": text,
                                    "thought": is_thought,
                                    "thoughtSignature": thought_signature
                                },
                                "finish_reason": finish_reason
                            }]
                        });
                        
                        return futures::stream::iter(vec![Ok(chunk.to_string())]);
                     },
                     Err(e) => return futures::stream::iter(vec![Err(format!("流错误: {}", e))]),
                 }
            });
            
        Ok(stream)
    }

    /// 发送流式请求到 Antigravity API
    /// 注意：需要将 OpenAI 格式转换为 Antigravity 专用格式
    pub async fn stream_generate(
        &self,
        openai_request: &converter::OpenAIChatRequest,
        access_token: &str,
        project_id: &str,
        session_id: &str,  // 新增 sessionId
    ) -> Result<impl futures::Stream<Item = Result<String, String>>, String> {
        // 使用 Antigravity 内部 API
        let url = "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:streamGenerateContent?alt=sse";
        
        // 1. 分离 System Message 和 User/Assistant Messages
        let (system_messages, chat_messages): (Vec<_>, Vec<_>) = openai_request.messages.iter()
            .partition(|m| m.role == "system");
            
        let has_system = !system_messages.is_empty();
        let system_text = if has_system {
            system_messages.iter()
                .map(|m| m.content.text()) // 获取完整文本，不截断
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            "".to_string()
        };

        // 2. 这里的 contents 只包含非 system 消息
        // converting Vec<&OpenAIMessage> to Vec<OpenAIMessage> for the converter (which expects &Vec<OpenAIMessage>... wait, converter takes &Vec<OpenAIMessage>)
        // so we need to construct a new Vec.
        let chat_messages_owned: Vec<converter::OpenAIMessage> = chat_messages.into_iter().cloned().collect();
        let contents = converter::convert_openai_to_gemini_contents(&chat_messages_owned);
        
        // 解析模型后缀配置 (e.g. gemini-3-pro-image-16x9-4k)
        let model_name = openai_request.model.clone(); // Clone for closure
        
        let model_suffix_ar = if model_name.contains("-16x9") { Some("16:9") }
            else if model_name.contains("-9x16") { Some("9:16") }
            else if model_name.contains("-4x3") { Some("4:3") }
            else if model_name.contains("-3x4") { Some("3:4") }
            else if model_name.contains("-1x1") { Some("1:1") }
            else { None };

        let model_suffix_4k = model_name.contains("-4k") || model_name.contains("-hd");

        // 解析 extra params 中的图片配置
        let extra_ar = openai_request.extra.as_ref()
            .and_then(|m| m.get("aspectRatio").or(m.get("aspect_ratio")))
            .and_then(|v| v.as_str());
            
        let extra_size = openai_request.extra.as_ref()
             .and_then(|m| m.get("imageSize").or(m.get("image_size")))
             .and_then(|v| v.as_str());

        // 解析图片配置 (Extra 参数优先 > explicit size > 后缀 > 默认)
        let aspect_ratio = if let Some(ar) = extra_ar {
             ar
        } else {
             match openai_request.size.as_deref() {
                Some("1024x1792") => "9:16",
                Some("1792x1024") => "16:9",
                Some("768x1024") => "3:4",
                Some("1024x768") => "4:3",
                Some("1024x1024") => "1:1",
                Some(_) => "1:1", // Fallback for unknown sizes
                None => model_suffix_ar.unwrap_or("1:1"),
            }
        };

        let is_hd = match openai_request.quality.as_deref() {
            Some("hd") => true,
            Some(_) => false,
            None => model_suffix_4k,
        };
        
        // 构造 generationConfig
        let mut generation_config = serde_json::json!({
            "temperature": openai_request.temperature.unwrap_or(1.0),
            "topP": openai_request.top_p.unwrap_or(0.95),
            "maxOutputTokens": openai_request.max_tokens.unwrap_or(8096),
            "candidateCount": 1
        });

        // 如果是画图模型，注入 imageConfig
        if openai_request.model.contains("gemini-3-pro-image") {
             if let Some(config) = generation_config.as_object_mut() {
                let mut image_config = serde_json::Map::new();
                image_config.insert("aspectRatio".to_string(), serde_json::json!(aspect_ratio));
                
                // 支持直接传 4K 或 hd
                if is_hd || extra_size == Some("4K") || extra_size == Some("hd") {
                    image_config.insert("imageSize".to_string(), serde_json::json!("4K"));
                }
                
                config.insert("imageConfig".to_string(), serde_json::Value::Object(image_config));
            }
        }

        // 如果是图片模型，上游模型名必须是 "gemini-3-pro-image"，不能带后缀
        let upstream_model = if openai_request.model.contains("gemini-3-pro-image") {
            "gemini-3-pro-image".to_string()
        } else {
            openai_request.model.clone()
        };

        let request_body = serde_json::json!({
            "project": project_id,
            "requestId": Uuid::new_v4().to_string(),
            "model": upstream_model,
            "userAgent": "antigravity",
            "request": {
                "contents": contents,
                "systemInstruction": {
                    "role": "user",
                    "parts": [{"text": system_text}]
                },
                "generationConfig": generation_config,
                "toolConfig": {
                    "functionCallingConfig": {
                        "mode": "VALIDATED"
                    }
                },
                "sessionId": session_id
            }
        });
        
        let response = self.client
            .post(url)
            .bearer_auth(access_token)
            .header("Host", "daily-cloudcode-pa.sandbox.googleapis.com")
            .header("User-Agent", "antigravity/1.11.3 windows/amd64")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| format!("请求失败: {}", e))?;
        
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("API 返回错误 {}: {}", status, body));
        }
        
        // 将响应体转换为 OpenAI 格式的 SSE 数据 (不带 data: 前缀)
        let stream = response.bytes_stream()
            .eventsource()
            .map(move |result| {
                match result {
                    Ok(event) => {
                        let data = event.data;
                        if data == "[DONE]" {
                            return Ok("[DONE]".to_string());
                        }
                        
                        // 解析 Gemini JSON
                        let json: serde_json::Value = serde_json::from_str(&data)
                            .map_err(|e| format!("解析 Gemini 流失败: {}", e))?;
                            
                        // 兼容某些 wrap 在 response 字段下的情况
                        let candidates = if let Some(c) = json.get("candidates") {
                            c
                        } else if let Some(r) = json.get("response") {
                            r.get("candidates").unwrap_or(&serde_json::Value::Null)
                        } else {
                            &serde_json::Value::Null
                        };

                        // 提取文本
                        let text = candidates.get(0)
                            .and_then(|c| c.get("content"))
                            .and_then(|c| c.get("parts"))
                            .and_then(|p| p.get(0))
                            .and_then(|p| p.get("text"))
                            .and_then(|t| t.as_str())
                            .unwrap_or("");

                        // 提取结束原因 (Gemini finishReason)
                        let gemini_finish_reason = candidates.get(0)
                            .and_then(|c| c.get("finishReason"))
                            .and_then(|f| f.as_str());

                        let finish_reason = match gemini_finish_reason {
                            Some("STOP") => Some("stop"),
                            Some("MAX_TOKENS") => Some("length"),
                            Some("SAFETY") => Some("content_filter"),
                            Some("RECITATION") => Some("content_filter"),
                            _ => None
                        };
                        
                        // 构造 OpenAI Chunk (仅 payload)
                        // 注意：如果 text 为空且 finish_reason 为空，这可能是一个 keep-alive 或元数据包
                        // OpenAI 允许 delta.content 为空字符串
                        
                        let chunk = serde_json::json!({
                            "id": "chatcmpl-stream",
                            "object": "chat.completion.chunk",
                            "created": chrono::Utc::now().timestamp(),
                            "model": model_name,
                            "choices": [{
                                "index": 0,
                                "delta": {
                                    "content": text
                                },
                                "finish_reason": finish_reason
                            }]
                        });
                        
                        // 注意：这里不要加 data: 前缀，因为 server.rs 中的 Sse 包装器会自动加
                        Ok(chunk.to_string())
                    }
                    Err(e) => Err(format!("流错误: {}", e)),
                }
            });
        
        Ok(stream)
    }
    
    /// 发送非流式请求到 Antigravity API
    pub async fn generate(
        &self,
        openai_request: &converter::OpenAIChatRequest,
        access_token: &str,
        project_id: &str,
        session_id: &str,  // 新增 sessionId
    ) -> Result<serde_json::Value, String> {
        // 使用 Antigravity 内部 API（非流式）
        let url = "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:generateContent";
        
        // 1. 分离 System Message 和 User/Assistant Messages
        let (system_messages, chat_messages): (Vec<_>, Vec<_>) = openai_request.messages.iter()
            .partition(|m| m.role == "system");
            
        let has_system = !system_messages.is_empty();
        let system_text = if has_system {
            system_messages.iter()
                .map(|m| m.content.text())
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            "".to_string()
        };

        // 2. 构造过滤后的 contents
        let chat_messages_owned: Vec<converter::OpenAIMessage> = chat_messages.into_iter().cloned().collect();
        let contents = converter::convert_openai_to_gemini_contents(&chat_messages_owned);
        
        // 构造 Antigravity 专用请求体
        // 解析模型后缀配置
        let model_name = &openai_request.model;
        let model_suffix_ar = if model_name.contains("-16x9") { Some("16:9") }
            else if model_name.contains("-9x16") { Some("9:16") }
            else if model_name.contains("-4x3") { Some("4:3") }
            else if model_name.contains("-3x4") { Some("3:4") }
            else if model_name.contains("-1x1") { Some("1:1") }
            else { None };

        let model_suffix_4k = model_name.contains("-4k") || model_name.contains("-hd");

        // 解析 extra params
        let extra_ar = openai_request.extra.as_ref()
            .and_then(|m| m.get("aspectRatio").or(m.get("aspect_ratio")))
            .and_then(|v| v.as_str());
            
        let extra_size = openai_request.extra.as_ref()
             .and_then(|m| m.get("imageSize").or(m.get("image_size")))
             .and_then(|v| v.as_str());

        // 解析图片配置
        let aspect_ratio = if let Some(ar) = extra_ar {
             ar
        } else {
             match openai_request.size.as_deref() {
                Some("1024x1792") => "9:16",
                Some("1792x1024") => "16:9",
                Some("768x1024") => "3:4",
                Some("1024x768") => "4:3",
                Some("1024x1024") => "1:1",
                Some(_) => "1:1",
                None => model_suffix_ar.unwrap_or("1:1"),
            }
        };

        let is_hd = match openai_request.quality.as_deref() {
            Some("hd") => true,
            Some(_) => false,
            None => model_suffix_4k,
        };
        
        // 构造 generationConfig
        let mut generation_config = serde_json::json!({
            "temperature": openai_request.temperature.unwrap_or(1.0),
            "topP": openai_request.top_p.unwrap_or(0.95),
            "maxOutputTokens": openai_request.max_tokens.unwrap_or(8096),
            "candidateCount": 1
        });

        // 如果是画图模型，注入 imageConfig
        if openai_request.model.contains("gemini-3-pro-image") {
             if let Some(config) = generation_config.as_object_mut() {
                let mut image_config = serde_json::Map::new();
                image_config.insert("aspectRatio".to_string(), serde_json::json!(aspect_ratio));
                
                 if is_hd || extra_size == Some("4K") || extra_size == Some("hd") {
                    image_config.insert("imageSize".to_string(), serde_json::json!("4K"));
                }
                config.insert("imageConfig".to_string(), serde_json::Value::Object(image_config));
            }
        }

        // 如果是图片模型，上游模型名必须是 "gemini-3-pro-image"，不能带后缀
        let upstream_model = if openai_request.model.contains("gemini-3-pro-image") {
            "gemini-3-pro-image".to_string()
        } else {
            openai_request.model.clone()
        };

        // 构造 Antigravity 专用请求体
        let request_body = serde_json::json!({
            "project": project_id,
            "requestId": Uuid::new_v4().to_string(),
            "model": upstream_model,
            "userAgent": "antigravity",
            "request": {
                "contents": contents,
                "systemInstruction": {
                    "role": "user",
                    "parts": [{"text": system_text}]
                },
                "generationConfig": generation_config,
                "toolConfig": {
                    "functionCallingConfig": {
                        "mode": "VALIDATED"
                    }
                },
                "sessionId": session_id
            }
        });
        
        let response = self.client
            .post(url)
            .bearer_auth(access_token)
            .header("Host", "daily-cloudcode-pa.sandbox.googleapis.com")
            .header("User-Agent", "antigravity/1.11.3 windows/amd64")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| format!("请求失败: {}", e))?;
        
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("API 返回错误 {}: {}", status, body));
        }
        
        tracing::debug!("(generate) 开始读取响应文本...");
        let text = response.text().await
            .map_err(|e| {
                tracing::error!("(generate) 读取响应文本失败: {}", e);
                format!("读取响应文本失败: {}", e)
            })?;
        
        tracing::debug!("(generate) 响应文本长度: {} 字符", text.len());
        tracing::debug!("(generate) 开始解析 JSON...");
            
        serde_json::from_str(&text)
            .map_err(|e| {
                let preview = if text.len() > 500 {
                    format!("{}...", &text[..500])
                } else {
                    text.clone()
                };
                tracing::error!("(generate) 解析响应失败. 错误: {}. 响应预览: {}", e, preview);
                format!("解析响应失败: {}. 响应长度: {} 字符", e, text.len())
            })
    }
}
