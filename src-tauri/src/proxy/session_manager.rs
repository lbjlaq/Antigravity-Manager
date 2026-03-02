use crate::proxy::mappers::claude::models::{ClaudeRequest, MessageContent};
use crate::proxy::mappers::openai::models::{OpenAIContent, OpenAIRequest};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// 会话管理器工具
pub struct SessionManager;

impl SessionManager {
    /// 根据 Claude 请求生成稳定的会话指纹 (Session Fingerprint)
    ///
    /// 设计理念:
    /// - 只哈希第一条用户消息内容,不混入模型名称或时间戳
    /// - 确保同一对话的所有轮次使用相同的 session_id
    /// - 最大化 prompt caching 的命中率
    ///
    /// 优先级:
    /// 1. metadata.user_id (客户端显式提供)
    /// 2. 第一条用户消息的 SHA256 哈希
    pub fn extract_session_id(request: &ClaudeRequest) -> String {
        // 1. 优先使用 metadata 中的 user_id
        if let Some(metadata) = &request.metadata {
            if let Some(user_id) = &metadata.user_id {
                if !user_id.is_empty() && !user_id.contains("session-") {
                    tracing::debug!("[SessionManager] Using explicit user_id: {}", user_id);
                    return user_id.clone();
                }
            }
        }

        // 2. 备选方案：基于第一条用户消息的 SHA256 哈希
        let mut hasher = Sha256::new();

        let mut content_found = false;
        for msg in &request.messages {
            if msg.role != "user" {
                continue;
            }

            let text = match &msg.content {
                MessageContent::String(s) => s.clone(),
                MessageContent::Array(blocks) => blocks
                    .iter()
                    .filter_map(|block| match block {
                        crate::proxy::mappers::claude::models::ContentBlock::Text { text } => {
                            Some(text.as_str())
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
            };

            let clean_text = text.trim();
            // [FIX #1732] 降低准入门槛 (10 -> 3)，确保即使是短消息也会生成稳定的会话锚点
            // 同时排除包含系统标志的消息，防止因为协议注入导致的 ID 漂移
            if clean_text.len() >= 3
                && !clean_text.contains("<system-reminder>")
                && !clean_text.contains("[System")
            {
                hasher.update(clean_text.as_bytes());
                content_found = true;
                break; // 始终锚定第一条有效用户消息
            }
        }

        if !content_found {
            // 如果没找到有意义的内容，退化为对最后一条消息进行哈希
            if let Some(last_msg) = request.messages.last() {
                hasher.update(format!("{:?}", last_msg.content).as_bytes());
            }
        }

        let hash = format!("{:x}", hasher.finalize());
        let sid = format!("sid-{}", &hash[..16]);

        tracing::debug!(
            "[SessionManager] Generated session_id: {} (content_found: {}, model: {})",
            sid,
            content_found,
            request.model
        );
        sid
    }

    /// 根据 OpenAI 请求生成稳定的会话指纹
    pub fn extract_openai_session_id(request: &OpenAIRequest) -> String {
        let mut hasher = Sha256::new();

        let mut content_found = false;
        for msg in &request.messages {
            if msg.role != "user" {
                continue;
            }
            if let Some(content) = &msg.content {
                let text = match content {
                    OpenAIContent::String(s) => s.clone(),
                    OpenAIContent::Array(blocks) => blocks
                        .iter()
                        .filter_map(|block| match block {
                            crate::proxy::mappers::openai::models::OpenAIContentBlock::Text {
                                text,
                            } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(" "),
                };

                let clean_text = text.trim();
                if clean_text.len() > 10 && !clean_text.contains("<system-reminder>") {
                    hasher.update(clean_text.as_bytes());
                    content_found = true;
                    break;
                }
            }
        }

        if !content_found {
            if let Some(last_msg) = request.messages.last() {
                hasher.update(format!("{:?}", last_msg.content).as_bytes());
            }
        }

        let hash = format!("{:x}", hasher.finalize());
        let sid = format!("sid-{}", &hash[..16]);
        tracing::debug!("[SessionManager-OpenAI] Generated fingerprint: {}", sid);
        sid
    }

    /// 根据 Gemini 原生请求 (JSON) 生成稳定的会话指纹
    pub fn extract_gemini_session_id(request: &Value, _model_name: &str) -> String {
        let mut hasher = Sha256::new();

        let mut content_found = false;
        if let Some(contents) = request.get("contents").and_then(|v| v.as_array()) {
            for content in contents {
                if content.get("role").and_then(|v| v.as_str()) != Some("user") {
                    continue;
                }

                if let Some(parts) = content.get("parts").and_then(|v| v.as_array()) {
                    let mut text_parts = Vec::new();
                    for part in parts {
                        if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                            text_parts.push(text);
                        }
                    }

                    let combined_text = text_parts.join(" ");
                    let clean_text = combined_text.trim();
                    if clean_text.len() > 10 && !clean_text.contains("<system-reminder>") {
                        hasher.update(clean_text.as_bytes());
                        content_found = true;
                        break;
                    }
                }
            }
        }

        if !content_found {
            // 兜底：对整个 Body 的首个 user part 进行摘要
            hasher.update(request.to_string().as_bytes());
        }

        let hash = format!("{:x}", hasher.finalize());
        let sid = format!("sid-{}", &hash[..16]);
        tracing::debug!("[SessionManager-Gemini] Generated fingerprint: {}", sid);
        sid
    }
}
