//! 未知提示词异常拦截与五态处置机制 (Issue #3972 扩展)
//!
//! 当下游客户端（第三方 IDE、CLI、Agent）向网关发送未经适配或无法识别的自定义系统提示词首句时，
//! 为防止直接透传触发上游 Google Cloud Code WAF 启发式规则导致的假 429 RESOURCE_EXHAUSTED，
//! 本模块实现：
//! 1. 快速安全与归一化判定（已由管线归一化或命中白名单/重写规则的提示词零开销放行）；
//! 2. 异常提示词捕获并异步挂起请求（tokio oneshot 机制，带 25 秒防超时保护）；
//! 3. 桌面前端弹窗广播（`proxy://prompt-intercept` 事件）与五态处置支持：
//!    - 在前面添加 (Prepend)：保留原第一句，强制在其前面追加官方 Antigravity 身份声明
//!    - 替换并记住 (ReplaceAndRemember)：将第一句替换为官方声明，并持久化写入 prompt_rewrite.rules
//!    - 放行并记住 (AllowAndRemember)：原样放行，并持久化记录到白名单
//!    - 本次替换 (ReplaceOnce)：仅对本次会话/请求替换，不写盘
//!    - 本次放行 (AllowOnce)：仅对本次会话/请求放行，不写盘
//! 4. 会话级防抖缓存（同一句提示词在会话窗口内只弹一次窗，避免并发请求风暴打扰用户）。

use crate::proxy::mappers::prompt_sanitizer::OFFICIAL_ANTIGRAVITY_IDENTITY;
use dashmap::DashMap;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tauri::Emitter;
use tokio::sync::oneshot;
use uuid::Uuid;

/// 全局 Tauri AppHandle（应用启动时由 lib.rs 注入）
static APP_HANDLE: OnceLock<tauri::AppHandle> = OnceLock::new();

/// 默认处置超时（秒），超时后自动执行默认安全策略（Prepend）
pub const DEFAULT_INTERCEPT_TIMEOUT_SECS: u64 = 25;

/// 会话级决策缓存生存期（秒）
const SESSION_CACHE_TTL_SECS: u64 = 300;

/// 处置动作枚举（六态对齐：前插、替换、放行 各有“本次”与“记住”）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptInterceptAction {
    /// 在前面添加并记住（保留原指令，前插官方身份并写入清理规则）
    PrependAndRemember,
    /// 本次在前面添加（保留原指令，前插官方身份，不写盘；超时默认策略）
    PrependOnce,
    /// 替换并记住（替换为官方声明并写入清理规则）
    ReplaceAndRemember,
    /// 本次替换（替换为官方声明，不写盘）
    ReplaceOnce,
    /// 放行并记住（原样放行并记录到白名单）
    AllowAndRemember,
    /// 本次放行（原样放行，不写盘）
    AllowOnce,
}

impl PromptInterceptAction {
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "prepend_and_remember"
            | "prepend-and-remember"
            | "在前面添加并记住"
            | "在前面插入并记住" => Some(Self::PrependAndRemember),
            "prepend_once"
            | "prepend-once"
            | "prepend"
            | "本次在前面添加"
            | "本次在前面插入"
            | "在前面添加" => Some(Self::PrependOnce),
            "replace_and_remember" | "replace-and-remember" | "替换并记住" => {
                Some(Self::ReplaceAndRemember)
            }
            "replace_once" | "replace-once" | "本次替换" => Some(Self::ReplaceOnce),
            "allow_and_remember" | "allow-and-remember" | "放行并记住" => {
                Some(Self::AllowAndRemember)
            }
            "allow_once" | "allow-once" | "本次放行" => Some(Self::AllowOnce),
            _ => None,
        }
    }
}

/// 发送给前端的拦截事件载荷
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptInterceptEvent {
    pub id: String,
    pub first_sentence: String,
    pub full_prompt_preview: String,
    pub official_identity: String,
    pub client_ip: Option<String>,
    pub model: Option<String>,
    pub timeout_secs: u64,
}

/// 拦截挂起队列记录项
struct InterceptEntry {
    first_sentence: String,
    senders: Vec<oneshot::Sender<PromptInterceptAction>>,
}

/// 正在挂起的拦截请求注册表：id -> InterceptEntry
static PENDING_INTERCEPTS: Lazy<DashMap<String, InterceptEntry>> = Lazy::new(DashMap::new);

/// 首句与正在挂起 ID 的反向映射（防止短时间相同首句弹多个弹窗）：first_sentence -> id
static SENTENCE_TO_ID: Lazy<DashMap<String, String>> = Lazy::new(DashMap::new);

/// 会话级防抖决策缓存：first_sentence -> (action, expire_at)
static SESSION_CACHE: Lazy<DashMap<String, (PromptInterceptAction, Instant)>> =
    Lazy::new(DashMap::new);

/// 初始化注入 Tauri AppHandle
pub fn init_prompt_intercept(handle: tauri::AppHandle) {
    let _ = APP_HANDLE.set(handle);
}

/// 提取提示词第一句或关键指令声明
pub fn extract_first_sentence(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let mut end_idx = trimmed.len();

    // 优先以首个换行符截断
    if let Some(pos) = trimmed.find('\n') {
        end_idx = end_idx.min(pos);
    }

    // 中文句号截断
    if let Some(pos) = trimmed[..end_idx].find('。') {
        end_idx = pos + '。'.len_utf8();
    }

    // 英文句号后接空格或行尾截断
    let slice = &trimmed[..end_idx];
    for (i, c) in slice.char_indices() {
        if c == '.' {
            let next_char = slice[i + 1..].chars().next();
            if next_char.is_none() || next_char == Some(' ') || next_char == Some('\t') {
                end_idx = i + 1;
                break;
            }
        }
    }

    // 长度上限限制为 300 字符，避免异常超长文本撑爆界面
    if end_idx > 300 {
        end_idx = 300;
    }

    trimmed[..end_idx].trim().to_string()
}

/// 判断提示词是否已属于已知安全状态（无需挂起弹窗）
pub fn is_prompt_safe_or_known(text: &str, allowlist: &[String]) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return true;
    }

    // 1. 已是官方身份或以官方身份起头
    if trimmed.starts_with(OFFICIAL_ANTIGRAVITY_IDENTITY)
        || trimmed.contains(OFFICIAL_ANTIGRAVITY_IDENTITY)
    {
        return true;
    }

    // 2. 检查持久化白名单
    let lower = trimmed.to_lowercase();
    for item in allowlist {
        let item_trim = item.trim().to_lowercase();
        if !item_trim.is_empty() && (lower.starts_with(&item_trim) || lower.contains(&item_trim)) {
            return true;
        }
    }

    false
}

/// 根据用户处置动作对原始文本进行应用转换
pub fn apply_action_to_text(
    action: PromptInterceptAction,
    original_text: &str,
    first_sentence: &str,
) -> String {
    match action {
        PromptInterceptAction::PrependAndRemember | PromptInterceptAction::PrependOnce => {
            // 在原第一句前面强制追加官方声明，保留后续所有指令
            format!(
                "{}\n\n{}",
                OFFICIAL_ANTIGRAVITY_IDENTITY,
                original_text.trim()
            )
        }
        PromptInterceptAction::ReplaceOnce | PromptInterceptAction::ReplaceAndRemember => {
            // 将原首句精准替换为官方声明
            if original_text.trim() == first_sentence.trim() {
                OFFICIAL_ANTIGRAVITY_IDENTITY.to_string()
            } else if let Some(pos) = original_text.find(first_sentence) {
                let mut out = String::with_capacity(
                    original_text.len() + OFFICIAL_ANTIGRAVITY_IDENTITY.len(),
                );
                out.push_str(&original_text[..pos]);
                out.push_str(OFFICIAL_ANTIGRAVITY_IDENTITY);
                out.push_str(&original_text[pos + first_sentence.len()..]);
                out
            } else {
                format!(
                    "{}\n\n{}",
                    OFFICIAL_ANTIGRAVITY_IDENTITY,
                    original_text.trim()
                )
            }
        }
        PromptInterceptAction::AllowOnce | PromptInterceptAction::AllowAndRemember => {
            original_text.to_string()
        }
    }
}

/// 检查并执行提示词拦截与处置（异步挂起）
pub async fn check_and_intercept_text(
    raw_text: &str,
    model: Option<&str>,
    client_ip: Option<&str>,
) -> (String, bool) {
    let trimmed = raw_text.trim();
    if trimmed.is_empty() {
        return (raw_text.to_string(), false);
    }

    // 获取当前配置中的白名单与拦截开关
    let (enabled, allowlist) = {
        if let Ok(app_config) = crate::modules::config::load_app_config() {
            let pr = &app_config.proxy.prompt_rewrite;
            (pr.intercept_unknown_prompts, pr.allowlist.clone())
        } else {
            (true, Vec::new())
        }
    };

    if !enabled || is_prompt_safe_or_known(trimmed, &allowlist) {
        return (raw_text.to_string(), false);
    }

    let first_sentence = extract_first_sentence(trimmed);
    if first_sentence.is_empty() {
        return (raw_text.to_string(), false);
    }

    // 检查会话缓存中是否已有近期处置记录
    let now = Instant::now();
    if let Some(entry) = SESSION_CACHE.get(&first_sentence) {
        let (cached_action, expire_at) = *entry.value();
        if now < expire_at {
            tracing::info!(
                "[PromptIntercept] Hit session cache for unknown prompt: {:?} -> {:?}",
                first_sentence,
                cached_action
            );
            let transformed = apply_action_to_text(cached_action, raw_text, &first_sentence);
            return (transformed, true);
        }
    }

    // 检查是否具备桌面宿主 GUI 交互句柄
    let app_handle = APP_HANDLE.get();
    if app_handle.is_none() {
        // 无桌面窗口（如独立 CLI 或 Docker 容器环境），默认自动前置添加官方身份（单次不写盘），避免 WAF 拦截
        tracing::info!(
            "[PromptIntercept] Headless mode without UI handle, auto-prepending official identity (once)"
        );
        let transformed = apply_action_to_text(
            PromptInterceptAction::PrependOnce,
            raw_text,
            &first_sentence,
        );
        return (transformed, true);
    }

    let app = app_handle.unwrap();

    // 建立请求挂起与等待通道
    let (tx, rx) = oneshot::channel::<PromptInterceptAction>();
    let intercept_id = if let Some(existing_id) = SENTENCE_TO_ID.get(&first_sentence) {
        // 已有相同首句的弹窗正在等待处理，将当前请求的 sender 合并到既有队列
        let id = existing_id.value().clone();
        if let Some(mut entry) = PENDING_INTERCEPTS.get_mut(&id) {
            entry.senders.push(tx);
        }
        id
    } else {
        // 创建新的拦截事件
        let new_id = Uuid::new_v4().to_string();
        PENDING_INTERCEPTS.insert(
            new_id.clone(),
            InterceptEntry {
                first_sentence: first_sentence.clone(),
                senders: vec![tx],
            },
        );
        SENTENCE_TO_ID.insert(first_sentence.clone(), new_id.clone());

        // 广播弹窗事件给桌面前端
        let preview_len = raw_text.len().min(500);
        let event_payload = PromptInterceptEvent {
            id: new_id.clone(),
            first_sentence: first_sentence.clone(),
            full_prompt_preview: raw_text[..preview_len].to_string(),
            official_identity: OFFICIAL_ANTIGRAVITY_IDENTITY.to_string(),
            client_ip: client_ip.map(|s| s.to_string()),
            model: model.map(|s| s.to_string()),
            timeout_secs: DEFAULT_INTERCEPT_TIMEOUT_SECS,
        };

        if let Err(e) = app.emit("proxy://prompt-intercept", &event_payload) {
            tracing::warn!("[PromptIntercept] Failed to emit intercept event: {}", e);
        }

        // 唤起并置顶主窗口
        let _ = app.emit("tray://show-window", ());

        new_id
    };

    // 异步等待用户选择，超时默认执行 PrependOnce（本次在前面添加，不保留）
    let action = match tokio::time::timeout(Duration::from_secs(DEFAULT_INTERCEPT_TIMEOUT_SECS), rx)
        .await
    {
        Ok(Ok(act)) => act,
        Ok(Err(_)) => {
            tracing::warn!("[PromptIntercept] Intercept channel dropped, fallback to PrependOnce");
            PromptInterceptAction::PrependOnce
        }
        Err(_) => {
            tracing::warn!(
                    "[PromptIntercept] Intercept id={} timed out after {}s, defaulting to PrependOnce (not saved)",
                    intercept_id,
                    DEFAULT_INTERCEPT_TIMEOUT_SECS
                );
            PromptInterceptAction::PrependOnce
        }
    };

    // 清理反向映射
    SENTENCE_TO_ID.remove(&first_sentence);
    PENDING_INTERCEPTS.remove(&intercept_id);

    // 写入会话级缓存（5分钟 TTL）
    SESSION_CACHE.insert(
        first_sentence.clone(),
        (
            action,
            Instant::now() + Duration::from_secs(SESSION_CACHE_TTL_SECS),
        ),
    );

    let transformed = apply_action_to_text(action, raw_text, &first_sentence);
    (transformed, true)
}

/// 检查并对 Gemini Canonical IR Payload 执行提示词拦截
pub async fn check_and_intercept_payload(
    body: &mut Value,
    model: Option<&str>,
    client_ip: Option<&str>,
) -> bool {
    // 检查 systemInstruction
    if let Some(parts) = body
        .get_mut("systemInstruction")
        .and_then(|si| si.get_mut("parts"))
        .and_then(Value::as_array_mut)
    {
        for part in parts.iter_mut() {
            if let Some(text) = part.get("text").and_then(Value::as_str) {
                let (transformed, modified) =
                    check_and_intercept_text(text, model, client_ip).await;
                if modified {
                    if let Some(obj) = part.as_object_mut() {
                        obj.insert("text".to_string(), Value::String(transformed));
                    }
                    return true;
                }
            }
        }
    }

    // 检查 contents 中第一轮是否含有非空系统提示词
    if let Some(contents) = body.get_mut("contents").and_then(Value::as_array_mut) {
        if let Some(first_turn) = contents.first_mut() {
            if let Some(parts) = first_turn.get_mut("parts").and_then(Value::as_array_mut) {
                if let Some(first_part) = parts.first_mut() {
                    if let Some(text) = first_part.get("text").and_then(Value::as_str) {
                        let trimmed = text.trim();
                        // 仅当首个 part 具有明确身份指令特征（以 You are / I want you to 等起头）才拦截
                        if trimmed.to_lowercase().starts_with("you are ")
                            || trimmed.to_lowercase().starts_with("you're ")
                            || trimmed.to_lowercase().starts_with("i want you to ")
                        {
                            let (transformed, modified) =
                                check_and_intercept_text(text, model, client_ip).await;
                            if modified {
                                if let Some(obj) = first_part.as_object_mut() {
                                    obj.insert("text".to_string(), Value::String(transformed));
                                }
                                return true;
                            }
                        }
                    }
                }
            }
        }
    }

    false
}

/// 前端用户点击处置按钮调用的命令处理函数
pub async fn resolve_prompt_intercept(id: String, action_str: String) -> Result<(), String> {
    let action = PromptInterceptAction::from_str_loose(&action_str)
        .ok_or_else(|| format!("Unknown prompt intercept action: {}", action_str))?;

    let entry = PENDING_INTERCEPTS
        .remove(&id)
        .map(|(_, v)| v)
        .ok_or_else(|| format!("No active prompt intercept found with id: {}", id))?;

    SENTENCE_TO_ID.remove(&entry.first_sentence);

    // 1. 若选择了“记住”，持久化写入配置
    match action {
        PromptInterceptAction::PrependAndRemember => {
            if let Ok(mut app_config) = crate::modules::config::load_app_config() {
                let rule_id = format!("custom-rule-{}", &id[..8.min(id.len())]);
                let pattern = format!("{}*", entry.first_sentence.trim());
                // 前插规则：匹配首句并替换为官方身份 + 原首句换行
                let replacement = format!(
                    "{}\n\n{}",
                    OFFICIAL_ANTIGRAVITY_IDENTITY,
                    entry.first_sentence.trim()
                );
                let new_rule = crate::proxy::config::PromptRewriteRule::new(
                    rule_id,
                    pattern,
                    replacement,
                    true,
                );
                app_config.proxy.prompt_rewrite.rules.push(new_rule);
                if let Err(e) = crate::modules::config::save_app_config(&app_config) {
                    tracing::warn!(
                        "[PromptIntercept] Failed to save prepend rewrite rule: {}",
                        e
                    );
                } else if let Some(app) = APP_HANDLE.get() {
                    let _ = app.emit("config://updated", ());
                }
            }
        }
        PromptInterceptAction::ReplaceAndRemember => {
            if let Ok(mut app_config) = crate::modules::config::load_app_config() {
                let rule_id = format!("custom-rule-{}", &id[..8.min(id.len())]);
                let pattern = format!("{}*", entry.first_sentence.trim());
                let new_rule = crate::proxy::config::PromptRewriteRule::new(
                    rule_id,
                    pattern,
                    OFFICIAL_ANTIGRAVITY_IDENTITY,
                    true,
                );
                app_config.proxy.prompt_rewrite.rules.push(new_rule);
                if let Err(e) = crate::modules::config::save_app_config(&app_config) {
                    tracing::warn!("[PromptIntercept] Failed to save rewrite rule: {}", e);
                } else if let Some(app) = APP_HANDLE.get() {
                    let _ = app.emit("config://updated", ());
                }
            }
        }
        PromptInterceptAction::AllowAndRemember => {
            if let Ok(mut app_config) = crate::modules::config::load_app_config() {
                let pr = &mut app_config.proxy.prompt_rewrite;
                if !pr.allowlist.contains(&entry.first_sentence) {
                    pr.allowlist.push(entry.first_sentence.clone());
                }
                if let Err(e) = crate::modules::config::save_app_config(&app_config) {
                    tracing::warn!("[PromptIntercept] Failed to save allowlist: {}", e);
                } else if let Some(app) = APP_HANDLE.get() {
                    let _ = app.emit("config://updated", ());
                }
            }
        }
        PromptInterceptAction::PrependOnce
        | PromptInterceptAction::ReplaceOnce
        | PromptInterceptAction::AllowOnce => {
            // 仅在内存 session_cache 生效，不写盘
        }
    }

    // 2. 写入会话级防抖缓存
    SESSION_CACHE.insert(
        entry.first_sentence.clone(),
        (
            action,
            Instant::now() + Duration::from_secs(SESSION_CACHE_TTL_SECS),
        ),
    );

    // 3. 释放所有等待此首句的请求通道
    for sender in entry.senders {
        let _ = sender.send(action);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_first_sentence() {
        let text1 = "You are Cline, an autonomous coding agent that helps with software development.\n\n# Rules\nRule 1: Always check tests.";
        assert_eq!(
            extract_first_sentence(text1),
            "You are Cline, an autonomous coding agent that helps with software development."
        );

        let text2 = "你是专业开发助手。负责全栈架构。";
        assert_eq!(extract_first_sentence(text2), "你是专业开发助手。");

        let text3 = "Single line without terminal dot";
        assert_eq!(
            extract_first_sentence(text3),
            "Single line without terminal dot"
        );
    }

    #[test]
    fn test_is_prompt_safe_or_known() {
        let official = format!(
            "{}\n\nSome custom directives.",
            OFFICIAL_ANTIGRAVITY_IDENTITY
        );
        assert!(is_prompt_safe_or_known(&official, &[]));

        let unknown = "You are an unknown custom agent for testing.";
        assert!(!is_prompt_safe_or_known(unknown, &[]));

        let allowlist = vec!["You are an unknown custom agent".to_string()];
        assert!(is_prompt_safe_or_known(unknown, &allowlist));
    }

    #[test]
    fn test_apply_action_prepend() {
        let raw = "You are Cline, an autonomous coding agent.\n\n# Rules\n1. Check.";
        let first = "You are Cline, an autonomous coding agent.";
        let out1 = apply_action_to_text(PromptInterceptAction::PrependOnce, raw, first);
        assert!(out1.starts_with(OFFICIAL_ANTIGRAVITY_IDENTITY));
        assert!(out1.contains("You are Cline, an autonomous coding agent."));
        assert!(out1.contains("# Rules"));

        let out2 = apply_action_to_text(PromptInterceptAction::PrependAndRemember, raw, first);
        assert!(out2.starts_with(OFFICIAL_ANTIGRAVITY_IDENTITY));
        assert!(out2.contains("You are Cline, an autonomous coding agent."));
        assert!(out2.contains("# Rules"));
    }

    #[test]
    fn test_apply_action_replace() {
        let raw = "You are Cline, an autonomous coding agent.\n\n# Rules\n1. Check.";
        let first = "You are Cline, an autonomous coding agent.";
        let out = apply_action_to_text(PromptInterceptAction::ReplaceOnce, raw, first);
        assert!(out.starts_with(OFFICIAL_ANTIGRAVITY_IDENTITY));
        assert!(!out.contains("You are Cline, an autonomous coding agent."));
        assert!(out.contains("# Rules"));
    }

    #[test]
    fn test_apply_action_allow() {
        let raw = "You are Cline, an autonomous coding agent.\n\n# Rules\n1. Check.";
        let first = "You are Cline, an autonomous coding agent.";
        let out = apply_action_to_text(PromptInterceptAction::AllowOnce, raw, first);
        assert_eq!(out, raw);
    }

    #[test]
    fn test_prompt_intercept_action_parse() {
        assert_eq!(
            PromptInterceptAction::from_str_loose("本次在前面添加"),
            Some(PromptInterceptAction::PrependOnce)
        );
        assert_eq!(
            PromptInterceptAction::from_str_loose("在前面添加并记住"),
            Some(PromptInterceptAction::PrependAndRemember)
        );
        assert_eq!(
            PromptInterceptAction::from_str_loose("prepend"),
            Some(PromptInterceptAction::PrependOnce)
        );
        assert_eq!(
            PromptInterceptAction::from_str_loose("prepend_once"),
            Some(PromptInterceptAction::PrependOnce)
        );
        assert_eq!(
            PromptInterceptAction::from_str_loose("prepend_and_remember"),
            Some(PromptInterceptAction::PrependAndRemember)
        );
        assert_eq!(
            PromptInterceptAction::from_str_loose("替换并记住"),
            Some(PromptInterceptAction::ReplaceAndRemember)
        );
        assert_eq!(
            PromptInterceptAction::from_str_loose("本次替换"),
            Some(PromptInterceptAction::ReplaceOnce)
        );
        assert_eq!(
            PromptInterceptAction::from_str_loose("放行并记住"),
            Some(PromptInterceptAction::AllowAndRemember)
        );
        assert_eq!(
            PromptInterceptAction::from_str_loose("本次放行"),
            Some(PromptInterceptAction::AllowOnce)
        );
    }

    #[tokio::test]
    async fn test_check_and_intercept_payload_with_official_prompt_skips() {
        let mut payload = serde_json::json!({
            "systemInstruction": {
                "parts": [
                    { "text": format!("{}\n\nFollow user instructions.", OFFICIAL_ANTIGRAVITY_IDENTITY) }
                ]
            }
        });
        let modified = check_and_intercept_payload(&mut payload, None, None).await;
        assert!(!modified);
    }

    #[tokio::test]
    async fn test_check_and_intercept_payload_with_unknown_prompt_in_headless_prepends() {
        let mut payload = serde_json::json!({
            "systemInstruction": {
                "parts": [
                    { "text": "You are Cline, an autonomous coding agent.\n\n# Rules\n1. Check." }
                ]
            }
        });
        // 单元测试环境中未初始化 APP_HANDLE，自动走无 GUI 兜底逻辑（Prepend）
        let modified = check_and_intercept_payload(&mut payload, None, None).await;
        assert!(modified);
        let text = payload["systemInstruction"]["parts"][0]["text"]
            .as_str()
            .unwrap();
        assert!(text.starts_with(OFFICIAL_ANTIGRAVITY_IDENTITY));
        assert!(text.contains("You are Cline, an autonomous coding agent."));
        assert!(text.contains("# Rules"));
    }
}
