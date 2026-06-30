// HTTP 会话历史存储
// 为 /v1/responses POST 提供 previous_response_id 链式历史支持
// 这样即使客户端用 HTTP 而不是 WebSocket，也能实现多轮对话

use serde_json::Value;
use std::collections::HashMap;
use crate::proxy::handlers::openai::get_cached_tool_call;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const SESSION_TTL_SECS: u64 = 3600; // 1小时过期

#[derive(Debug, Clone)]
pub struct HttpSessionEntry {
    /// 对话历史：instructions + 所有 input items（包括历史response输出）
    pub input_items: Vec<Value>,
    /// 系统指令
    pub instructions: String,
    /// 模型名
    pub model: String,
    /// 上次访问时间（用于TTL淘汰）
    pub last_accessed: Instant,
}

struct HttpSessionStore {
    sessions: HashMap<String, HttpSessionEntry>,
}

impl HttpSessionStore {
    fn new() -> Self {
        Self {
            sessions: HashMap::new(),
        }
    }

    fn get(&mut self, response_id: &str) -> Option<HttpSessionEntry> {
        let entry = self.sessions.get_mut(response_id)?;
        entry.last_accessed = Instant::now();
        Some(entry.clone())
    }

    fn insert(&mut self, response_id: String, entry: HttpSessionEntry) {
        self.sessions.insert(response_id, entry);
        // 顺便淘汰过期 session（惰性清理）
        self.evict_expired();
    }

    fn evict_expired(&mut self) {
        let ttl = Duration::from_secs(SESSION_TTL_SECS);
        self.sessions.retain(|_, v| v.last_accessed.elapsed() < ttl);
    }
}

static STORE: OnceLock<Mutex<HttpSessionStore>> = OnceLock::new();

fn store() -> &'static Mutex<HttpSessionStore> {
    STORE.get_or_init(|| Mutex::new(HttpSessionStore::new()))
}

/// 根据 previous_response_id 查找历史会话
pub async fn get_session(previous_response_id: &str) -> Option<HttpSessionEntry> {
    store().lock().await.get(previous_response_id)
}

/// 保存新的会话状态（以 response_id 为 key）
pub async fn save_session(response_id: String, entry: HttpSessionEntry) {
    store().lock().await.insert(response_id, entry);
}

/// 追加本轮模型输出项到会话历史中
pub async fn append_outputs(response_id: &str, new_outputs: Vec<Value>) {
    if let Some(entry) = store().lock().await.sessions.get_mut(response_id) {
        entry.input_items.extend(new_outputs);
    }
}

/// 把上一轮的 response output items 转成 input items 追加到历史中
/// 同时把新的 user input items 追加进去
/// 返回合并后的 input items
pub fn merge_history_with_new_input(
    mut history: Vec<Value>,
    response_output: &[Value],
    new_input: &[Value],
    tool_call_cache: &HashMap<String, Value>,
) -> Vec<Value> {
    // 追加上一轮 response output（assistant消息、工具调用等）
    for item in response_output {
        history.push(item.clone());
    }

    // 追加新的 input items
    for item in new_input {
        let t = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if t == "compaction" || t == "compaction_summary" {
            continue;
        }
        history.push(item.clone());
    }

    // 修复工具调用（确保function_call_output前有对应的function_call）
    repair_tool_calls(&mut history, tool_call_cache);

    // 去重
    dedupe_input_items(history)
}

fn repair_tool_calls(
    items: &mut Vec<Value>,
    tool_call_cache: &HashMap<String, Value>,
) {
    let mut call_present = std::collections::HashSet::new();
    for item in items.iter() {
        let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if item_type == "function_call" || item_type == "custom_tool_call" {
            if let Some(call_id) = item.get("call_id").and_then(|v| v.as_str()) {
                call_present.insert(call_id.to_string());
            }
        }
    }

    let mut new_items = Vec::new();
    let mut inserted = std::collections::HashSet::new();
    for item in items.drain(..) {
        let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if item_type == "function_call_output" || item_type == "custom_tool_call_output" {
            if let Some(call_id) = item.get("call_id").and_then(|v| v.as_str()) {
                if !call_id.is_empty() && !call_present.contains(call_id) && !inserted.contains(call_id) {
                    if let Some(cached_call) = tool_call_cache.get(call_id).cloned().or_else(|| get_cached_tool_call(call_id)) {
                        new_items.push(cached_call.clone());
                        inserted.insert(call_id.to_string());
                    }
                }
            }
        }
        new_items.push(item);
    }
    *items = new_items;
}

fn dedupe_input_items(items: Vec<Value>) -> Vec<Value> {
    use std::collections::{HashMap, HashSet};
    let mut referenced_call_ids = HashSet::new();
    for item in &items {
        let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if item_type == "function_call_output" || item_type == "custom_tool_call_output" {
            if let Some(call_id) = item.get("call_id").and_then(|v| v.as_str()) {
                if !call_id.is_empty() {
                    referenced_call_ids.insert(call_id.to_string());
                }
            }
        }
    }

    let mut keep_map: HashMap<String, usize> = HashMap::new();
    for (idx, item) in items.iter().enumerate() {
        let item_id = item.get("id").and_then(|v| v.as_str()).unwrap_or("");
        if item_id.is_empty() {
            continue;
        }
        let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
        let is_referenced = !call_id.is_empty() && referenced_call_ids.contains(call_id);
        if let Some(&existing_idx) = keep_map.get(item_id) {
            let existing_call_id = items[existing_idx].get("call_id").and_then(|v| v.as_str()).unwrap_or("");
            let existing_referenced = !existing_call_id.is_empty() && referenced_call_ids.contains(existing_call_id);
            if is_referenced || !existing_referenced {
                keep_map.insert(item_id.to_string(), idx);
            }
        } else {
            keep_map.insert(item_id.to_string(), idx);
        }
    }

    let mut keep_indices = std::collections::HashSet::new();
    for (_, idx) in keep_map {
        keep_indices.insert(idx);
    }

    let mut filtered = Vec::new();
    for (idx, item) in items.into_iter().enumerate() {
        let item_id = item.get("id").and_then(|v| v.as_str()).unwrap_or("");
        if !item_id.is_empty() && !keep_indices.contains(&idx) {
            continue;
        }
        filtered.push(item);
    }
    filtered
}


