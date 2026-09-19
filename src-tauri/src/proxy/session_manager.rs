use crate::proxy::mappers::claude::models::{ClaudeRequest, MessageContent};
use crate::proxy::mappers::openai::models::{OpenAIContent, OpenAIRequest};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// 会话管理器工具
pub struct SessionManager;

/// 清理用户消息中混入的系统提示词（如 Claude Code / Cursor / AtomCode 的 <system-reminder>、<environment>、[System...] 等）
/// 提取真正的用户输入内容，保证同一对话无论经过多少轮、携带什么动态时间戳，会话指纹都绝对稳定。
pub fn sanitize_user_text_for_fingerprint(raw: &str) -> String {
    let mut s = raw.to_string();

    // 1. 移除 <system-reminder>...</system-reminder> 标签及内部内容
    while let Some(start) = s.find("<system-reminder>") {
        if let Some(end) = s[start..].find("</system-reminder>") {
            s.replace_range(start..start + end + "</system-reminder>".len(), "");
        } else {
            s.truncate(start);
            break;
        }
    }

    // 2. 移除 <environment>...</environment> 标签及内部内容
    while let Some(start) = s.find("<environment>") {
        if let Some(end) = s[start..].find("</environment>") {
            s.replace_range(start..start + end + "</environment>".len(), "");
        } else {
            s.truncate(start);
            break;
        }
    }

    // 3. 移除 [System: ...] 或 [System ...]
    while let Some(start) = s.find("[System") {
        if let Some(end) = s[start..].find(']') {
            s.replace_range(start..start + end + 1, "");
        } else {
            s.truncate(start);
            break;
        }
    }

    s.trim().to_string()
}

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
        // 基于第一条用户消息的 SHA256 哈希作为会话锚点
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

            let clean_text = sanitize_user_text_for_fingerprint(&text);
            if clean_text.len() >= 2 {
                hasher.update(clean_text.as_bytes());
                content_found = true;
                break; // 始终锚定第一条有效用户消息
            }
        }

        if !content_found {
            // 兜底方案1：尝试取第一条 user 消息的原文本（只要有内容）
            for msg in &request.messages {
                if msg.role == "user" {
                    let raw = format!("{:?}", msg.content);
                    if raw.trim().len() >= 2 {
                        hasher.update(raw.trim().as_bytes());
                        content_found = true;
                        break;
                    }
                }
            }
        }

        if !content_found {
            // 兜底方案2：如果没找到任何 user 消息，退化为对最后一条消息进行哈希
            if let Some(last_msg) = request.messages.last() {
                hasher.update(format!("{:?}", last_msg.content).as_bytes());
            }
        }

        // [NEW] 融合多维环境特征 (System Prompt 完整摘要 + 可用 Tools 列表摘要)
        // 彻底解决多窗口首条消息完全相同（如均输入“你好”）的碰撞问题！
        hasher.update([0xff]);
        if let Some(sys) = &request.system {
            let sys_text = match sys {
                crate::proxy::mappers::claude::models::SystemPrompt::String(s) => s.clone(),
                crate::proxy::mappers::claude::models::SystemPrompt::Array(blocks) => blocks
                    .iter()
                    .map(|b| b.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
            };
            let clean_sys = sanitize_user_text_for_fingerprint(&sys_text);
            if !clean_sys.is_empty() {
                hasher.update(clean_sys.as_bytes());
            }
        }

        hasher.update([0xfe]);
        if let Some(tools) = &request.tools {
            for tool in tools {
                if let Some(name) = &tool.name {
                    hasher.update(name.as_bytes());
                    hasher.update([0xfd]);
                }
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
        if let Some(explicit) = request.session_id.as_ref() {
            let trimmed = explicit.trim();
            if !trimmed.is_empty() {
                return crate::proxy::thinking_store::sanitize_session_id(trimmed);
            }
        }
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

                let clean_text = sanitize_user_text_for_fingerprint(&text);
                if clean_text.len() >= 2 {
                    hasher.update(clean_text.as_bytes());
                    content_found = true;
                    break;
                }
            }
        }

        if !content_found {
            for msg in &request.messages {
                if msg.role == "user" {
                    let raw = format!("{:?}", msg.content);
                    if raw.trim().len() >= 2 {
                        hasher.update(raw.trim().as_bytes());
                        content_found = true;
                        break;
                    }
                }
            }
        }

        if !content_found {
            if let Some(last_msg) = request.messages.last() {
                hasher.update(format!("{:?}", last_msg.content).as_bytes());
            }
        }

        // [NEW] 融合多维环境特征 (System/Developer 消息摘要 + Tools 列表摘要)
        hasher.update([0xff]);
        for msg in &request.messages {
            if msg.role == "system" || msg.role == "developer" {
                if let Some(content) = &msg.content {
                    let text = match content {
                        OpenAIContent::String(s) => s.clone(),
                        OpenAIContent::Array(blocks) => blocks
                            .iter()
                            .filter_map(|block| {
                                match block {
                                crate::proxy::mappers::openai::models::OpenAIContentBlock::Text {
                                    text,
                                } => Some(text.as_str()),
                                _ => None,
                            }
                            })
                            .collect::<Vec<_>>()
                            .join(" "),
                    };
                    let clean_sys = sanitize_user_text_for_fingerprint(&text);
                    if !clean_sys.is_empty() {
                        hasher.update(clean_sys.as_bytes());
                        break;
                    }
                }
            }
        }

        hasher.update([0xfe]);
        if let Some(tools) = &request.tools {
            for tool in tools {
                let name = tool
                    .get("function")
                    .and_then(|f| f.get("name"))
                    .and_then(|n| n.as_str())
                    .or_else(|| tool.get("name").and_then(|n| n.as_str()));
                if let Some(name) = name {
                    hasher.update(name.as_bytes());
                    hasher.update([0xfd]);
                }
            }
        }

        let hash = format!("{:x}", hasher.finalize());
        let sid = format!("sid-{}", &hash[..16]);
        tracing::debug!("[SessionManager-OpenAI] Generated fingerprint: {}", sid);
        sid
    }

    /// 根据 Gemini 原生请求 (JSON) 生成稳定的会话指纹
    pub fn extract_gemini_session_id(request: &Value, _model_name: &str) -> String {
        if let Some(explicit) = request.get("session_id").and_then(|v| v.as_str()) {
            let trimmed = explicit.trim();
            if !trimmed.is_empty() {
                return crate::proxy::thinking_store::sanitize_session_id(trimmed);
            }
        }
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
                    let clean_text = sanitize_user_text_for_fingerprint(&combined_text);
                    if clean_text.len() >= 2 {
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

        // [NEW] 融合多维环境特征 (systemInstruction 摘要 + tools 列表摘要)
        hasher.update([0xff]);
        if let Some(sys_inst) = request
            .get("system_instruction")
            .or_else(|| request.get("systemInstruction"))
        {
            if let Some(parts) = sys_inst.get("parts").and_then(|p| p.as_array()) {
                let mut sys_texts = Vec::new();
                for part in parts {
                    if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                        sys_texts.push(text);
                    }
                }
                let clean_sys = sanitize_user_text_for_fingerprint(&sys_texts.join(" "));
                if !clean_sys.is_empty() {
                    hasher.update(clean_sys.as_bytes());
                }
            }
        }

        hasher.update([0xfe]);
        if let Some(tools) = request.get("tools").and_then(|t| t.as_array()) {
            for tool in tools {
                if let Some(decls) = tool.get("functionDeclarations").and_then(|d| d.as_array()) {
                    for decl in decls {
                        if let Some(name) = decl.get("name").and_then(|n| n.as_str()) {
                            hasher.update(name.as_bytes());
                            hasher.update([0xfd]);
                        }
                    }
                }
            }
        }

        let hash = format!("{:x}", hasher.finalize());
        let sid = format!("sid-{}", &hash[..16]);
        tracing::debug!("[SessionManager-Gemini] Generated fingerprint: {}", sid);
        sid
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::mappers::claude::models::{Message, SystemPrompt, Tool};

    #[test]
    fn test_sanitize_user_text_for_fingerprint() {
        let text1 =
            "你好啊\n\n<system-reminder>\nCurrent date: 2026-09-08 (Tue)\n</system-reminder>";
        let text2 =
            "你好啊\n\n<system-reminder>\nCurrent date: 2026-09-09 (Wed)\n</system-reminder>";
        let text3 = "你好啊 [System: Tool execution completed successfully]";

        assert_eq!(sanitize_user_text_for_fingerprint(text1), "你好啊");
        assert_eq!(sanitize_user_text_for_fingerprint(text2), "你好啊");
        assert_eq!(sanitize_user_text_for_fingerprint(text3), "你好啊");
    }

    #[test]
    fn test_same_user_text_different_system_prompt_isolated() {
        let req_project_a = ClaudeRequest {
            model: "claude-3-7-sonnet".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("你好".to_string()),
            }],
            system: Some(SystemPrompt::String(
                "Project A Workspace: /src/backend".to_string(),
            )),
            tools: None,
            stream: false,
            max_tokens: None,
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: None,
            metadata: None,
            output_config: None,
            size: None,
            quality: None,
        };

        let req_project_b = ClaudeRequest {
            model: "claude-3-7-sonnet".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("你好".to_string()),
            }],
            system: Some(SystemPrompt::String(
                "Project B Workspace: /src/frontend".to_string(),
            )),
            tools: None,
            stream: false,
            max_tokens: None,
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: None,
            metadata: None,
            output_config: None,
            size: None,
            quality: None,
        };

        let sid_a = SessionManager::extract_session_id(&req_project_a);
        let sid_b = SessionManager::extract_session_id(&req_project_b);

        // Even though both messages are "你好", different workspace/system instructions yield completely isolated sessions!
        assert_ne!(sid_a, sid_b);
    }

    #[test]
    fn test_same_user_text_different_tools_isolated() {
        let req_with_tools = ClaudeRequest {
            model: "claude-3-7-sonnet".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("你好".to_string()),
            }],
            system: None,
            tools: Some(vec![Tool {
                name: Some("bash".to_string()),
                description: None,
                input_schema: None,
                type_: None,
            }]),
            stream: false,
            max_tokens: None,
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: None,
            metadata: None,
            output_config: None,
            size: None,
            quality: None,
        };

        let req_pure_chat = ClaudeRequest {
            model: "claude-3-7-sonnet".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("你好".to_string()),
            }],
            system: None,
            tools: None,
            stream: false,
            max_tokens: None,
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: None,
            metadata: None,
            output_config: None,
            size: None,
            quality: None,
        };

        let sid_tools = SessionManager::extract_session_id(&req_with_tools);
        let sid_chat = SessionManager::extract_session_id(&req_pure_chat);

        assert_ne!(sid_tools, sid_chat);
    }

    #[test]
    fn test_same_conversation_multi_turn_stability() {
        let req_turn1 = ClaudeRequest {
            model: "claude-3-7-sonnet".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("你好".to_string()),
            }],
            system: Some(SystemPrompt::String("Project A".to_string())),
            tools: None,
            stream: false,
            max_tokens: None,
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: None,
            metadata: None,
            output_config: None,
            size: None,
            quality: None,
        };

        let mut req_turn2 = req_turn1.clone();
        req_turn2.messages.push(Message {
            role: "assistant".to_string(),
            content: MessageContent::String("你好！请问有什么可以帮助你？".to_string()),
        });
        req_turn2.messages.push(Message {
            role: "user".to_string(),
            content: MessageContent::String("写个斐波那契数列".to_string()),
        });

        let sid1 = SessionManager::extract_session_id(&req_turn1);
        let sid2 = SessionManager::extract_session_id(&req_turn2);

        // Within the same conversation, multi-turn session ID remains 100% stable!
        assert_eq!(sid1, sid2);
    }

    #[test]
    fn test_utf8_char_boundary_at_512_bytes_never_panics() {
        // Construct a system prompt where byte index 512 lands exactly inside a 3-byte Chinese character '单' (bytes 511..514)
        let prefix = "a".repeat(511);
        let malicious_sys = format!("{}单清单清单", prefix);
        assert!(
            !malicious_sys.is_char_boundary(512),
            "Byte 512 must be inside '单' to test the regression"
        );

        // 1. Claude Request
        let claude_req = ClaudeRequest {
            model: "claude-3-7-sonnet".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("你好".to_string()),
            }],
            system: Some(SystemPrompt::String(malicious_sys.clone())),
            tools: None,
            stream: false,
            max_tokens: None,
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: None,
            metadata: None,
            output_config: None,
            size: None,
            quality: None,
        };
        let sid_claude = SessionManager::extract_session_id(&claude_req);
        assert!(sid_claude.starts_with("sid-"));

        // 2. OpenAI Request
        let openai_req: OpenAIRequest = serde_json::from_value(serde_json::json!({
            "model": "gpt-4o",
            "messages": [
                { "role": "system", "content": malicious_sys },
                { "role": "user", "content": "你好" }
            ]
        }))
        .unwrap();
        let sid_openai = SessionManager::extract_openai_session_id(&openai_req);
        assert!(sid_openai.starts_with("sid-"));

        // 3. Gemini Native Request
        let gemini_req = serde_json::json!({
            "contents": [{
                "role": "user",
                "parts": [{ "text": "你好" }]
            }],
            "system_instruction": {
                "parts": [{ "text": malicious_sys }]
            }
        });
        let sid_gemini = SessionManager::extract_gemini_session_id(&gemini_req, "gemini-2.5-pro");
        assert!(sid_gemini.starts_with("sid-"));

        // 4. Real User Prompt reported in issue
        let user_prompt =
            "你是一个远程服务器运维专家。\n\n当前纳管的 LXC 容器清单如下....".repeat(20);
        let claude_user_req = ClaudeRequest {
            model: "claude-3-7-sonnet".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::String("你好".to_string()),
            }],
            system: Some(SystemPrompt::String(user_prompt)),
            tools: None,
            stream: false,
            max_tokens: None,
            temperature: None,
            top_p: None,
            top_k: None,
            thinking: None,
            metadata: None,
            output_config: None,
            size: None,
            quality: None,
        };
        let sid_real = SessionManager::extract_session_id(&claude_user_req);
        assert!(sid_real.starts_with("sid-"));
    }
}
