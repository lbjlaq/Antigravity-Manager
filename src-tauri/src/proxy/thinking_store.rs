//! Server-side full thinking-block store.
//!
//! Captures untruncated thought text + thoughtSignature from upstream Gemini
//! responses, then precisely re-injects them into the next request's `contents`
//! even when the client dropped / truncated thinking.
//!
//! Matching is content-based (visible assistant text + tool ids), not turn
//! index, so OpenAI / Anthropic / Gemini packet shapes can differ.
//!
//! Isolation key = `{tenant}:{client_session_id}`:
//! - tenant is a hash of the caller's API key
//! - client_session_id prefers `X-Session-Id` / body `session_id`

use axum::http::HeaderMap;
use dashmap::DashMap;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

const MIN_SIGNATURE_LENGTH: usize = 50;
pub const SENTINEL_SIGNATURE: &str = "skip_thought_signature_validator";
const MAX_SESSIONS: usize = 2000;
const MAX_TURNS_PER_SESSION: usize = 200;
const MAX_BYTES_PER_SESSION: usize = 32 * 1024 * 1024;
/// Persist last_accessed at most this often. Fill/hydrate is memory-only between writes.
const TOUCH_PERSIST_INTERVAL: Duration = Duration::from_secs(5 * 60);

fn idle_ttl() -> Duration {
    let days = crate::proxy::config::get_thinking_retention_days().max(1) as u64;
    Duration::from_secs(days.saturating_mul(24 * 60 * 60))
}

const PLACEHOLDER_THOUGHTS: &[&str] = &[
    "...",
    "·",
    ".",
    "···",
    "[undefined]",
    "Applying tool decisions and generating response...",
];

/// Server-side auto thinking. Clients usually omit thinking config;
/// if any of `claude` / `flash` / `pro` / `agent` appears in a model id
/// (requested or mapped), the proxy enables thoughts + signature restore.
/// Image / embed / lite are excluded to avoid 400s.
pub fn model_forces_server_thinking(model: &str) -> bool {
    if !crate::proxy::config::is_thinking_store_enabled() {
        return false;
    }
    let m = model.to_lowercase();
    if m.is_empty() {
        return false;
    }
    if m.contains("image") || m.contains("imagen") || m.contains("embed") || m.contains("lite") || m.contains("preview") {
        return false;
    }
    m.contains("claude")
        || m.contains("flash")
        || m.contains("pro")
        || m.contains("agent")
        || m.contains("gemini")
        || m.contains("thinking")
        || m.contains("o1")
        || m.contains("o3")
        || m.contains("deepseek")
}

pub fn any_model_forces_server_thinking(models: &[&str]) -> bool {
    models.iter().copied().any(model_forces_server_thinking)
}

#[derive(Debug, Clone)]
pub struct ThinkingRecord {
    pub fingerprint: String,
    pub thought: String,
    pub signature: Option<String>,
    pub tool_ids: Vec<String>,
    #[allow(dead_code)]
    pub tool_names: Vec<String>,
    pub visible: String,
}

#[derive(Debug)]
struct SessionEntry {
    turns: Vec<Arc<ThinkingRecord>>,
    last_access: Instant,
    last_persist_touch: Instant,
    bytes: usize,
    l2_loaded: bool,
}

impl SessionEntry {
    fn new() -> Self {
        Self {
            turns: Vec::new(),
            last_access: Instant::now(),
            last_persist_touch: Instant::now(),
            bytes: 0,
            l2_loaded: false,
        }
    }
}

pub struct ThinkingStore {
    sessions: DashMap<String, SessionEntry>,
}

impl ThinkingStore {
    fn new() -> Self {
        Self {
            sessions: DashMap::new(),
        }
    }

    pub fn global() -> &'static ThinkingStore {
        static INSTANCE: OnceLock<ThinkingStore> = OnceLock::new();
        INSTANCE.get_or_init(ThinkingStore::new)
    }

    fn maybe_evict(&self, keep_key: &str) {
        if self.sessions.len() <= MAX_SESSIONS {
            return;
        }
        self.sessions
            .retain(|_, e| e.last_access.elapsed() < idle_ttl());
        if self.sessions.len() <= MAX_SESSIONS {
            return;
        }
        if let Some(oldest_key) = self
            .sessions
            .iter()
            .min_by_key(|e| e.last_access)
            .map(|e| e.key().clone())
        {
            if oldest_key != keep_key {
                self.sessions.remove(&oldest_key);
            }
        }
    }

    pub fn record(&self, store_key: &str, rec: ThinkingRecord) {
        if !crate::proxy::config::is_thinking_store_enabled() {
            return;
        }
        if rec.thought.trim().is_empty() && rec.signature.is_none() {
            return;
        }
        // Placeholder "..." / sentinel-only blocks are injected for Gemini protocol
        // compliance. Recording them as new turns made every 300K-context request
        // append N dummies, then prune rewrite the whole SQLite session.
        if !is_capturable_thought(&rec.thought, rec.signature.as_deref()) {
            return;
        }

        let rec_bytes = record_bytes(&rec);

        self.maybe_evict(store_key);

        let persist = {
            let mut entry = self
                .sessions
                .entry(store_key.to_string())
                .or_insert_with(SessionEntry::new);
            entry.last_access = Instant::now();
            entry.l2_loaded = true;

            let merge_last = entry
                .turns
                .last()
                .is_some_and(|last| last.fingerprint == rec.fingerprint);

            if merge_last {
                let (stronger, old_text_bytes) = {
                    let last = entry.turns.last().expect("merge_last");
                    (
                        rec.thought.len() >= last.thought.len()
                            || rec.signature.as_ref().map(|s| s.len()).unwrap_or(0)
                                > last.signature.as_ref().map(|s| s.len()).unwrap_or(0),
                        last.thought.len() + last.visible.len(),
                    )
                };
                if stronger {
                    entry.bytes = entry.bytes.saturating_sub(old_text_bytes);
                    {
                        let last_arc = entry.turns.last_mut().expect("merge_last");
                        *Arc::make_mut(last_arc) = rec;
                    }
                    entry.bytes = entry.bytes.saturating_add(rec_bytes);
                    entry.turns.last().cloned()
                } else {
                    None
                }
            } else {
                entry.turns.push(Arc::new(rec));
                entry.bytes = entry.bytes.saturating_add(rec_bytes);
                while entry.turns.len() > MAX_TURNS_PER_SESSION
                    || entry.bytes > MAX_BYTES_PER_SESSION
                {
                    if let Some(old) = entry.turns.first() {
                        let old_bytes = old.thought.len()
                            + old.signature.as_ref().map(|s| s.len()).unwrap_or(0)
                            + old.visible.len();
                        entry.bytes = entry.bytes.saturating_sub(old_bytes);
                    }
                    if entry.turns.is_empty() {
                        break;
                    }
                    entry.turns.remove(0);
                }
                entry.turns.last().cloned()
            }
        };

        if let Some(saved) = persist {
            let _ = crate::modules::proxy_db::save_thinking_record(
                store_key,
                &saved.fingerprint,
                &saved.thought,
                saved.signature.as_deref(),
                &saved.tool_ids,
                &saved.tool_names,
                &saved.visible,
            );
        }
    }

    /// Refresh in-memory expiry. SQLite last_accessed is debounced so HDD
    /// never sees a write on the fill hot path after the session is warm.
    pub fn touch_session(&self, store_key: &str) {
        if !crate::proxy::config::is_thinking_store_enabled() || store_key.is_empty() {
            return;
        }
        let mut persist = false;
        if let Some(mut entry) = self.sessions.get_mut(store_key) {
            entry.last_access = Instant::now();
            if entry.last_persist_touch.elapsed() >= TOUCH_PERSIST_INTERVAL {
                entry.last_persist_touch = Instant::now();
                persist = true;
            }
        }
        if persist {
            let _ = crate::modules::proxy_db::touch_thinking_session(store_key);
        }
    }

    fn load_turns(&self, store_key: &str) -> Vec<Arc<ThinkingRecord>> {
        if let Some(e) = self.sessions.get(store_key) {
            if e.l2_loaded {
                let turns = e.turns.clone();
                drop(e);
                if let Some(mut entry) = self.sessions.get_mut(store_key) {
                    entry.last_access = Instant::now();
                }
                return turns;
            }
        }

        let persisted = crate::modules::proxy_db::load_thinking_records(store_key).unwrap_or_default();
        let loaded_len = persisted.len();
        let mut entry = self
            .sessions
            .entry(store_key.to_string())
            .or_insert_with(SessionEntry::new);
        if !entry.l2_loaded && entry.turns.is_empty() && !persisted.is_empty() {
            for p in persisted {
                let rec = ThinkingRecord {
                    fingerprint: p.fingerprint,
                    thought: p.thought,
                    signature: p.signature,
                    tool_ids: p.tool_ids,
                    tool_names: p.tool_names,
                    visible: p.visible,
                };
                entry.bytes += record_bytes(&rec);
                entry.turns.push(Arc::new(rec));
            }
            tracing::info!(
                "[ThinkingStore] Restored {} turns from SQLite L2 for session {}",
                entry.turns.len(),
                store_key
            );
        }
        entry.l2_loaded = true;
        entry.last_access = Instant::now();
        entry.last_persist_touch = Instant::now();
        let turns = entry.turns.clone();
        drop(entry);
        if loaded_len > 0 {
            let _ = crate::modules::proxy_db::touch_thinking_session(store_key);
        }
        turns
    }

    /// Capture real thinking from the inbound request without re-appending
    /// history that is already stored. Placeholder blocks are ignored.
    pub fn ingest_from_contents(&self, store_key: &str, contents: &[Value]) {
        if !crate::proxy::config::is_thinking_store_enabled() || store_key.is_empty() {
            return;
        }

        let mut incoming: Vec<ThinkingRecord> = Vec::new();
        for content in contents {
            let role = content.get("role").and_then(|v| v.as_str()).unwrap_or("");
            if role != "model" && role != "assistant" {
                continue;
            }
            let Some(parts) = content.get("parts").and_then(|p| p.as_array()) else {
                continue;
            };
            let mut acc = TurnAccumulator::new();
            for part in parts {
                acc.ingest_part(part);
            }
            if !acc.should_capture() {
                continue;
            }
            incoming.push(acc.into_record());
        }
        if incoming.is_empty() {
            return;
        }

        let existing = self.load_turns(store_key);
        let mut used = vec![false; existing.len()];
        let mut to_append = Vec::new();
        let mut to_upgrade: Vec<(usize, ThinkingRecord)> = Vec::new();

        for rec in incoming {
            if let Some(idx) = match_existing_record(&rec, &existing, &used) {
                used[idx] = true;
                if is_stronger_record(&rec, &existing[idx]) {
                    to_upgrade.push((idx, rec));
                }
            } else {
                to_append.push(rec);
            }
        }

        if !to_upgrade.is_empty() {
            if let Some(mut entry) = self.sessions.get_mut(store_key) {
                for (idx, rec) in &to_upgrade {
                    if *idx >= entry.turns.len() {
                        continue;
                    }
                    let new_bytes = record_bytes(rec);
                    let old_bytes = record_bytes(&entry.turns[*idx]);
                    entry.bytes = entry
                        .bytes
                        .saturating_sub(old_bytes)
                        .saturating_add(new_bytes);
                    *Arc::make_mut(&mut entry.turns[*idx]) = rec.clone();
                }
            }
            if let Some((idx, rec)) = to_upgrade.last() {
                if *idx + 1 == existing.len() {
                    let _ = crate::modules::proxy_db::save_thinking_record(
                        store_key,
                        &rec.fingerprint,
                        &rec.thought,
                        rec.signature.as_deref(),
                        &rec.tool_ids,
                        &rec.tool_names,
                        &rec.visible,
                    );
                }
            }
        }

        for rec in to_append {
            self.record(store_key, rec);
        }
    }

    pub fn restore_gemini_contents(&self, store_key: &str, contents: &mut Vec<Value>) -> usize {
        if !crate::proxy::config::is_thinking_store_enabled() {
            return 0;
        }
        if contents.is_empty() || store_key.is_empty() {
            return 0;
        }

        let records = self.load_turns(store_key);
        if records.is_empty() {
            return 0;
        }

        // 收集所有的 model 轮次元信息
        struct ModelTurnMeta {
            content_idx: usize,
            visible: String,
            norm_visible: String,
            tool_ids: Vec<String>,
            #[allow(dead_code)]
            tool_names: Vec<String>,
            existing_thought: String,
            fp: String,
            matched_record_idx: Option<usize>,
            already_complete: bool,
        }

        let mut model_turns: Vec<ModelTurnMeta> = Vec::new();
        for (c_idx, content) in contents.iter().enumerate() {
            let role = content
                .get("role")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if role != "model" && role != "assistant" {
                continue;
            }
            let Some(parts) = content.get("parts").and_then(|p| p.as_array()) else {
                continue;
            };
            let (visible, tool_ids, tool_names, existing_thought) = inspect_parts(parts);
            let already_complete = !turn_needs_restore(parts, &existing_thought);
            // Agent tool turns match by tool_id (Phase 1). Skip fingerprint /
            // whitespace-normalize until a later phase actually needs them.
            model_turns.push(ModelTurnMeta {
                content_idx: c_idx,
                visible,
                norm_visible: String::new(),
                tool_ids,
                tool_names,
                existing_thought,
                fp: String::new(),
                matched_record_idx: None,
                already_complete,
            });
        }

        if model_turns.is_empty() {
            return 0;
        }

        let mut used = vec![false; records.len()];
        let mut by_tool: HashMap<&str, Vec<usize>> = HashMap::new();
        let mut by_fp: HashMap<&str, Vec<usize>> = HashMap::new();
        for (rec_idx, rec) in records.iter().enumerate() {
            for id in &rec.tool_ids {
                by_tool.entry(id.as_str()).or_default().push(rec_idx);
            }
            by_fp.entry(rec.fingerprint.as_str()).or_default().push(rec_idx);
        }

        // 从尾部往回匹配：最新 model 轮次优先吃最新记录，避免早期短回复抢走后轮思考。
        // JSON 注入位置仍是该轮 parts 头部（Gemini 要求 thought 在 functionCall 之前）。

        // Phase 1: 工具调用 ID 精准锚定（最高优先级：tool_ids 具有全局唯一性）
        for turn in model_turns.iter_mut().rev() {
            if turn.already_complete || turn.tool_ids.is_empty() {
                continue;
            }
            for id in &turn.tool_ids {
                let Some(idxs) = by_tool.get(id.as_str()) else {
                    continue;
                };
                if let Some(&rec_idx) = idxs.iter().rev().find(|&&i| !used[i]) {
                    turn.matched_record_idx = Some(rec_idx);
                    used[rec_idx] = true;
                    break;
                }
            }
        }

        // Phase 2: 完整指纹匹配（硬性隔离：工具轮次与纯文本轮次严禁混用）
        for turn in model_turns.iter_mut().rev() {
            if turn.already_complete || turn.matched_record_idx.is_some() {
                continue;
            }
            if turn.fp.is_empty() {
                turn.fp = fingerprint(&turn.visible, &turn.tool_ids, &turn.tool_names);
            }
            let turn_has_tools = !turn.tool_ids.is_empty() || !turn.tool_names.is_empty();
            let Some(idxs) = by_fp.get(turn.fp.as_str()) else {
                continue;
            };
            if let Some(&rec_idx) = idxs.iter().rev().find(|&&i| {
                if used[i] {
                    return false;
                }
                let rec_has_tools =
                    !records[i].tool_ids.is_empty() || !records[i].tool_names.is_empty();
                rec_has_tools == turn_has_tools
            }) {
                turn.matched_record_idx = Some(rec_idx);
                used[rec_idx] = true;
            }
        }

        // Phase 3: 纯文本前缀 / 正文相似匹配（仅限纯文本轮次）
        // Normalize each record once. The old inner-loop split_whitespace().collect().join()
        // was O(turns * records * visible_len) and stalled 10s+ at ~300K context.
        let needs_phase3 = model_turns.iter().any(|t| {
            !t.already_complete
                && t.matched_record_idx.is_none()
                && t.tool_ids.is_empty()
                && t.tool_names.is_empty()
                && !t.visible.trim().is_empty()
        });
        let rec_norms: Vec<String> = if needs_phase3 {
            records.iter().map(|r| normalize_ws(&r.visible)).collect()
        } else {
            Vec::new()
        };
        if needs_phase3 {
            for turn in model_turns.iter_mut().rev() {
                if turn.already_complete || turn.matched_record_idx.is_some() {
                    continue;
                }
                let turn_has_tools = !turn.tool_ids.is_empty() || !turn.tool_names.is_empty();
                if turn_has_tools || turn.visible.trim().is_empty() {
                    continue;
                }
                if turn.norm_visible.is_empty() {
                    turn.norm_visible = normalize_ws(&turn.visible);
                }
                if turn.norm_visible.is_empty() {
                    continue;
                }
                for (rec_idx, rec) in records.iter().enumerate().rev() {
                    let rec_has_tools = !rec.tool_ids.is_empty() || !rec.tool_names.is_empty();
                    if used[rec_idx] || rec_has_tools || rec_norms[rec_idx].is_empty() {
                        continue;
                    }
                    let norm_rec = &rec_norms[rec_idx];
                    let norm_vis = &turn.norm_visible;
                    if norm_rec == norm_vis
                        || norm_rec.starts_with(norm_vis)
                        || norm_vis.starts_with(norm_rec)
                    {
                        turn.matched_record_idx = Some(rec_idx);
                        used[rec_idx] = true;
                        break;
                    }
                }
            }
        }

        // Phase 4: 尾部优先的逆向兜底匹配（对齐用户的“最新回答在尾部”思路）
        // 仅对对话中【最后一个 model 轮次】进行保底匹配，绝不污染历史早期轮次！
        if let Some(last_turn) = model_turns.last_mut() {
            if !last_turn.already_complete && last_turn.matched_record_idx.is_none() {
                let last_turn_has_tools = !last_turn.tool_ids.is_empty() || !last_turn.tool_names.is_empty();
                if let Some((last_unused_rec_idx, _)) = records
                    .iter()
                    .enumerate()
                    .rfind(|(idx, r)| {
                        if used[*idx] {
                            return false;
                        }
                        let r_has_tools = !r.tool_ids.is_empty() || !r.tool_names.is_empty();
                        r_has_tools == last_turn_has_tools
                    })
                {
                    last_turn.matched_record_idx = Some(last_unused_rec_idx);
                    used[last_unused_rec_idx] = true;
                }
            }
        }

        let mut restored = 0usize;
        for turn in model_turns {
            if turn.already_complete {
                continue;
            }
            let Some(rec_idx) = turn.matched_record_idx else {
                continue;
            };
            let rec = &records[rec_idx];
            if rec.thought.trim().is_empty() && rec.signature.is_none() {
                continue;
            }

            let Some(content) = contents.get_mut(turn.content_idx) else {
                continue;
            };
            let Some(parts) = content.get_mut("parts").and_then(|p| p.as_array_mut()) else {
                continue;
            };

            let has_unvalidated_function_call = parts
                .iter()
                .any(|p| p.get("functionCall").is_some() && !part_has_signature(p));

            let should_replace = is_placeholder_thought(&turn.existing_thought)
                || turn.existing_thought.len() < rec.thought.len()
                || (rec.signature.is_some() && !parts.iter().any(|p| part_has_signature(p)))
                || has_unvalidated_function_call;

            if !should_replace {
                continue;
            }

            parts.retain(|p| p.get("thought").and_then(|t| t.as_bool()) != Some(true));

            let thought_text = if is_placeholder_thought(&rec.thought) || rec.thought.trim().is_empty() {
                "..."
            } else {
                rec.thought.as_str()
            };

            let mut thought_part = json!({
                "text": thought_text,
                "thought": true,
            });
            if let Some(sig) = rec.signature.as_ref().filter(|s| is_real_signature(s)) {
                thought_part["thoughtSignature"] = json!(sig);
                for part in parts.iter_mut() {
                    if part.get("functionCall").is_some() {
                        part["thoughtSignature"] = json!(sig);
                    }
                }
            } else {
                thought_part["thoughtSignature"] = json!(SENTINEL_SIGNATURE);
                for part in parts.iter_mut() {
                    if part.get("functionCall").is_some() && !part_has_signature(part) {
                        part["thoughtSignature"] = json!(SENTINEL_SIGNATURE);
                    }
                }
            }
            parts.insert(0, thought_part);
            restored += 1;
        }

        if restored > 0 {
            tracing::info!(
                "[ThinkingStore] Restored {} full thinking block(s) for session {}",
                restored,
                store_key
            );
        }
        restored
    }

    pub fn end_session(&self, store_key: &str) -> EndSessionResult {
        let removed = self.sessions.remove(store_key);
        let (deleted_turns, deleted_bytes) = removed
            .map(|(_, e)| (e.turns.len(), e.bytes))
            .unwrap_or((0, 0));
        let _ = crate::modules::proxy_db::delete_thinking_records_for_session(store_key);
        EndSessionResult {
            session_id: client_id_from_store_key(store_key).to_string(),
            deleted_turns,
            deleted_bytes,
        }
    }

    /// Drop thinking records that no longer appear in the (possibly compressed) history.
    /// Always keeps the newest 2 turns so the latest unused response thinking is not lost.
    pub fn prune_orphaned_records(&self, store_key: &str, contents: &[Value]) {
        if !crate::proxy::config::is_thinking_store_enabled() || store_key.is_empty() {
            return;
        }

        let mem_turns = self.sessions.get(store_key).map(|e| e.turns.len()).unwrap_or(0);
        if mem_turns == 0 {
            return;
        }
        let live_turn_count = contents.iter().filter(|c| is_model_or_assistant(c)).count();
        // Typical agent path: stored turns ≈ live model turns. Skip inspect/fingerprint/SQLite.
        if mem_turns <= live_turn_count.saturating_add(2) {
            return;
        }

        let mut live_tool_ids = std::collections::HashSet::new();
        let mut live_fps = std::collections::HashSet::new();
        let mut live_visibles: Vec<String> = Vec::new();

        for content in contents {
            if !is_model_or_assistant(content) {
                continue;
            }
            let Some(parts) = content.get("parts").and_then(|p| p.as_array()) else {
                continue;
            };
            let (visible, tool_ids, tool_names, _) = inspect_parts(parts);
            live_fps.insert(fingerprint(&visible, &tool_ids, &tool_names));
            for id in tool_ids {
                live_tool_ids.insert(id);
            }
            let norm = normalize_ws(&visible);
            if !norm.is_empty() {
                live_visibles.push(norm);
            }
        }

        let keep_fps = {
            let Some(mut entry) = self.sessions.get_mut(store_key) else {
                return;
            };
            if entry.turns.len() <= live_turn_count.saturating_add(2) {
                return;
            }

            let rec_norms: Vec<String> = entry
                .turns
                .iter()
                .map(|r| normalize_ws(&r.visible))
                .collect();
            let total = entry.turns.len();
            let keep_tail_start = total.saturating_sub(2);
            let mut keep: Vec<Arc<ThinkingRecord>> = Vec::new();
            for (i, rec) in entry.turns.iter().enumerate() {
                let matched_tool = rec.tool_ids.iter().any(|id| live_tool_ids.contains(id));
                let matched_fp = live_fps.contains(&rec.fingerprint);
                let norm_rec = &rec_norms[i];
                let matched_text = !norm_rec.is_empty()
                    && live_visibles.iter().any(|v| {
                        v == norm_rec || v.starts_with(norm_rec) || norm_rec.starts_with(v)
                    });
                if matched_tool || matched_fp || matched_text || i >= keep_tail_start {
                    keep.push(rec.clone());
                }
            }

            if keep.len() == entry.turns.len() {
                return;
            }

            let dropped = entry.turns.len() - keep.len();
            entry.bytes = keep.iter().map(|r| record_bytes(r)).sum();
            let fps: Vec<String> = keep.iter().map(|r| r.fingerprint.clone()).collect();
            entry.turns = keep;
            tracing::info!(
                "[ThinkingStore] Pruned {} orphaned thinking record(s) after context compression for session {}",
                dropped,
                store_key
            );
            fps
        };

        // Delete orphans by fingerprint. Never DELETE+re-INSERT the kept blobs.
        let _ = crate::modules::proxy_db::delete_thinking_records_except_fingerprints(
            store_key,
            &keep_fps,
        );
    }

    pub fn session_stats(&self, store_key: &str) -> Option<(usize, usize)> {
        self.sessions
            .get(store_key)
            .map(|e| (e.turns.len(), e.bytes))
    }

    #[cfg(test)]
    pub fn clear(&self) {
        self.sessions.clear();
    }
}

#[derive(Debug, Clone)]
pub struct EndSessionResult {
    pub session_id: String,
    pub deleted_turns: usize,
    pub deleted_bytes: usize,
}

#[derive(Debug, Clone)]
pub struct SessionScope {
    pub client_id: String,
    pub store_key: String,
}

impl SessionScope {
    pub fn from_headers(headers: &HeaderMap, fallback: impl Into<String>) -> Self {
        Self::from_request_parts(headers, None, None, fallback)
    }

    pub fn from_headers_and_body(
        headers: &HeaderMap,
        body: Option<&Value>,
        fallback: impl Into<String>,
    ) -> Self {
        Self::from_request_parts(headers, body, None, fallback)
    }

    pub fn from_request_parts(
        headers: &HeaderMap,
        body: Option<&Value>,
        query: Option<&str>,
        fallback: impl Into<String>,
    ) -> Self {
        let fallback = fallback.into();
        let client_id = explicit_session_id_with_query(headers, body, query)
            .unwrap_or(fallback)
            .trim()
            .to_string();
        let client_id = sanitize_session_id(&client_id);
        let tenant = tenant_from_headers(headers);
        let store_key = format!("{}:{}", tenant, client_id);
        Self {
            client_id,
            store_key,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct TurnAccumulator {
    thought: String,
    signature: Option<String>,
    visible: String,
    tool_ids: Vec<String>,
    tool_names: Vec<String>,
}

impl TurnAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn ingest_part(&mut self, part: &Value) {
        let is_thought = part.get("thought").and_then(|v| v.as_bool()).unwrap_or(false);
        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
            if is_thought {
                self.thought.push_str(text);
            } else {
                self.visible.push_str(text);
            }
        }
        if let Some(sig) = part
            .get("thoughtSignature")
            .or_else(|| part.get("thought_signature"))
            .and_then(|s| s.as_str())
        {
            if is_real_signature(sig)
                && self
                    .signature
                    .as_ref()
                    .map(|old| sig.len() > old.len())
                    .unwrap_or(true)
            {
                self.signature = Some(sig.to_string());
            }
        }
        if let Some(fc) = part.get("functionCall") {
            let name = fc
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            if let Some(id_str) = fc.get("id").and_then(|v| v.as_str()) {
                if !self.tool_ids.iter().any(|x| x == id_str) {
                    self.tool_ids.push(id_str.to_string());
                    self.tool_names.push(name);
                }
            } else if !self.tool_names.iter().any(|x| x == &name) {
                self.tool_names.push(name);
            }
        }
    }

    pub fn record_tool_id(&mut self, tool_name: &str, real_id: &str) {
        if !self.tool_ids.iter().any(|x| x == real_id) {
            self.tool_ids.push(real_id.to_string());
        }
        if !self.tool_names.iter().any(|x| x == tool_name) {
            self.tool_names.push(tool_name.to_string());
        }
    }

    pub fn is_empty(&self) -> bool {
        self.thought.trim().is_empty() && self.signature.is_none()
    }

    fn should_capture(&self) -> bool {
        !self.is_empty() && is_capturable_thought(&self.thought, self.signature.as_deref())
    }

    fn into_record(self) -> ThinkingRecord {
        let fp = fingerprint(&self.visible, &self.tool_ids, &self.tool_names);
        ThinkingRecord {
            fingerprint: fp,
            thought: self.thought,
            signature: self.signature,
            tool_ids: self.tool_ids,
            tool_names: self.tool_names,
            visible: self.visible,
        }
    }

    pub fn commit(self, store_key: &str) {
        if store_key.is_empty() || !self.should_capture() {
            return;
        }
        let rec = self.into_record();
        tracing::debug!(
            "[ThinkingStore] Capture thought len={} sig_len={} fp={} sid={}",
            rec.thought.len(),
            rec.signature.as_ref().map(|s| s.len()).unwrap_or(0),
            rec.fingerprint,
            store_key
        );
        ThinkingStore::global().record(store_key, rec);
    }
}

pub fn capture_gemini_contents(store_key: &str, contents: &[Value]) {
    if !crate::proxy::config::is_thinking_store_enabled() || store_key.is_empty() {
        return;
    }
    for content in contents {
        let role = content.get("role").and_then(|v| v.as_str()).unwrap_or("");
        if role != "model" && role != "assistant" {
            continue;
        }
        if let Some(parts) = content.get("parts").and_then(|p| p.as_array()) {
            capture_gemini_parts(store_key, parts);
        }
    }
}

/// Capture client-supplied thinking, restore missing blocks, then prune compressed-away history.
///
/// Client histories usually have no real thinking (Claude/OpenAI). After a session is
/// warm in memory, this path is RAM-only: no SQLite open, no placeholder ingest, no prune
/// rewrite. JSON fill still copies stored thought text into the freshly built request.
pub fn hydrate_gemini_contents(store_key: &str, contents: &mut Vec<Value>) -> usize {
    if store_key.is_empty() {
        return 0;
    }
    let store = ThinkingStore::global();
    store.touch_session(store_key);
    if contents_have_capturable_thought(contents) {
        store.ingest_from_contents(store_key, contents);
    }
    let restored = store.restore_gemini_contents(store_key, contents);
    store.prune_orphaned_records(store_key, contents);
    restored
}

fn is_model_or_assistant(content: &Value) -> bool {
    matches!(
        content.get("role").and_then(|v| v.as_str()),
        Some("model") | Some("assistant")
    )
}

fn contents_have_capturable_thought(contents: &[Value]) -> bool {
    for content in contents {
        if !is_model_or_assistant(content) {
            continue;
        }
        let Some(parts) = content.get("parts").and_then(|p| p.as_array()) else {
            continue;
        };
        for part in parts {
            let is_thought = part.get("thought").and_then(|v| v.as_bool()).unwrap_or(false);
            if !is_thought {
                continue;
            }
            let text = part.get("text").and_then(|t| t.as_str()).unwrap_or("");
            let sig = part
                .get("thoughtSignature")
                .or_else(|| part.get("thought_signature"))
                .and_then(|s| s.as_str());
            if is_capturable_thought(text, sig) {
                return true;
            }
        }
    }
    false
}

pub fn capture_gemini_parts(store_key: &str, parts: &[Value]) {
    let mut acc = TurnAccumulator::new();
    for part in parts {
        acc.ingest_part(part);
    }
    acc.commit(store_key);
}

pub fn capture_gemini_response(store_key: &str, response: &Value) {
    let raw = response.get("response").unwrap_or(response);
    if let Some(parts) = raw
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.as_array())
    {
        capture_gemini_parts(store_key, parts);
    }
}

/// 四大协议统一思考补齐管线：确保所有 Gemini contents 中的 model 轮次在开启思考时，必须具备合法的思考块与签名
pub fn finalize_gemini_contents_thinking(
    contents: &mut [Value],
    is_thinking_enabled: bool,
) {
    for msg in contents.iter_mut() {
        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
        if role != "model" && role != "assistant" {
            continue;
        }
        if let Some(parts) = msg.get_mut("parts").and_then(|p| p.as_array_mut()) {
            let mut thinking_parts = Vec::new();
            let mut other_parts = Vec::new();

            for mut part in parts.drain(..) {
                if let Some(obj) = part.as_object_mut() {
                    // 统一清洗向 Google 发送的非标准蛇形字段
                    obj.remove("thought_signature");
                }
                let is_thought = part.get("thought").and_then(|v| v.as_bool()).unwrap_or(false)
                    || part.get("thoughtSignature").is_some();
                if is_thought {
                    thinking_parts.push(part);
                } else {
                    other_parts.push(part);
                }
            }

            if is_thinking_enabled {
                if thinking_parts.is_empty() {
                    // 优先继承本轮工具调用身上的真实加密签名
                    let turn_sig = other_parts
                        .iter()
                        .find_map(|p| {
                            if p.get("functionCall").is_some() {
                                p.get("thoughtSignature").and_then(|s| s.as_str())
                            } else {
                                None
                            }
                        })
                        .unwrap_or(SENTINEL_SIGNATURE);

                    thinking_parts.push(json!({
                        "text": "...",
                        "thought": true,
                        "thoughtSignature": turn_sig,
                    }));
                }

                // 为所有缺失签名的工具调用打上保底哨兵
                for part in other_parts.iter_mut() {
                    if part.get("functionCall").is_some()
                        && part.get("thoughtSignature").is_none()
                    {
                        part["thoughtSignature"] = json!(SENTINEL_SIGNATURE);
                    }
                }

                // 思考块始终强制排在最前面，其他部件紧随其后
                parts.extend(thinking_parts);
            } else {
                // 当思考模式为关时，不应补充或保留任何思考块，清洗所有 functionCall 上的 thoughtSignature
                for part in other_parts.iter_mut() {
                    if let Some(obj) = part.as_object_mut() {
                        obj.remove("thoughtSignature");
                    }
                }
            }

            parts.extend(other_parts);
        }
    }
}


/// 从 URL Query 字符串中提取 session / conversation 标识符
pub fn extract_session_from_query_str(query: &str) -> Option<String> {
    for (k, v) in url::form_urlencoded::parse(query.as_bytes()) {
        let key = k.to_ascii_lowercase();
        if matches!(
            key.as_str(),
            "session_id" | "sid" | "cid" | "conversation_id" | "chat_id" | "thread_id" | "channel"
        ) {
            let trimmed = v.trim();
            if !trimmed.is_empty() {
                let sanitized = sanitize_session_id(trimmed);
                if !sanitized.is_empty() && sanitized != "sid-unknown" {
                    return Some(sanitized);
                }
            }
        }
    }
    None
}

/// 全生态显式会话标识解析（包含 URL Query、Header 扩展与 Body 扩展）
pub fn explicit_session_id_with_query(
    headers: &HeaderMap,
    body: Option<&Value>,
    query: Option<&str>,
) -> Option<String> {
    // 1. 显式 URL Query 参数（最高优先级：用户配置 Base URL 直接挂载 ?session_id=win1）
    if let Some(q) = query {
        if let Some(sid) = extract_session_from_query_str(q) {
            return Some(sid);
        }
    }

    // 2. 从反代请求头中抓取 URL Query (x-forwarded-uri, x-original-uri)
    for uri_h in ["x-forwarded-uri", "x-original-uri"] {
        if let Some(raw_uri) = headers.get(uri_h).and_then(|h| h.to_str().ok()) {
            if let Some(pos) = raw_uri.find('?') {
                if let Some(sid) = extract_session_from_query_str(&raw_uri[pos + 1..]) {
                    return Some(sid);
                }
            }
        }
    }

    // 3. 从 Web 客户端 Referer 中嗅探 Query
    if let Some(referer) = headers.get("referer").and_then(|h| h.to_str().ok()) {
        if let Some(pos) = referer.find('?') {
            if let Some(sid) = extract_session_from_query_str(&referer[pos + 1..]) {
                return Some(sid);
            }
        }
    }

    // 4. 全生态 HTTP Headers：先精确名单，再通配 x-*-session-id / x-*-sessionid
    if let Some(sid) = session_id_from_headers(headers) {
        return Some(sid);
    }

    // 5. JSON Body 及 Metadata 深度提取
    if let Some(body) = body {
        for field in [
            "session_id",
            "conversation_id",
            "chat_id",
            "thread_id",
            "client_session_id",
            "previous_response_id",
        ] {
            if let Some(v) = body.get(field).and_then(|v| v.as_str()) {
                let v = v.trim();
                if !v.is_empty() {
                    return Some(sanitize_session_id(v));
                }
            }
        }
        if let Some(metadata) = body.get("metadata") {
            for field in [
                "conversation_id",
                "chat_id",
                "session_id",
                "thread_id",
                "user_id",
            ] {
                if let Some(v) = metadata.get(field).and_then(|v| v.as_str()) {
                    let v = v.trim();
                    if !v.is_empty() && !v.contains("session-") {
                        return Some(sanitize_session_id(v));
                    }
                }
            }
        }
    }

    None
}

pub fn explicit_session_id(headers: &HeaderMap, body: Option<&Value>) -> Option<String> {
    explicit_session_id_with_query(headers, body, None)
}

/// Product-specific `x-**-session-id` / `x-**-sessionid`. Checked before generic `x-session-id`.
const PRODUCT_SESSION_HEADERS: &[&str] = &[
    "x-jeikcode-sessionid",
    "x-jeikcode-session-id",
    "x-atomcode-session-id",
    "x-atomcode-sessionid",
    "x-antigravity-session-id",
    "x-client-session-id",
    "x-cursor-session-id",
    "cursor-session-id",
    "x-vscode-session-id",
    "anthropic-session-id",
];

const GENERIC_SESSION_HEADER: &str = "x-session-id";

/// Non-session-named aliases. Lowest header priority after `x-session-id`.
const ALIAS_SESSION_HEADERS: &[&str] = &[
    "x-conversation-id",
    "conversation-id",
    "x-chat-id",
    "chat-id",
    "x-thread-id",
    "thread-id",
];

fn header_session_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers.get(name).and_then(|h| h.to_str().ok()).and_then(|v| {
        let v = v.trim();
        if v.is_empty() {
            None
        } else {
            Some(sanitize_session_id(v))
        }
    })
}

fn is_generic_x_session_id(name: &str) -> bool {
    name.eq_ignore_ascii_case(GENERIC_SESSION_HEADER)
}

/// 兼容 AtomCode / JeikCode / Cursor 等客户端自定义会话头：
/// 优先 `x-*-session-id` / `x-*-sessionid`，其次通用 `x-session-id`。
fn is_wildcard_session_header(name: &str) -> bool {
    let key = name.trim().to_ascii_lowercase().replace('_', "-");
    if key == "mcp-session-id" {
        return false;
    }
    if key.ends_with("-request-id")
        || key.ends_with("-trace-id")
        || key.ends_with("-correlation-id")
        || key == "x-request-id"
        || key == "request-id"
    {
        return false;
    }
    let compact = key.replace('-', "");
    compact.contains("session") && compact.ends_with("id")
}

fn session_id_from_headers(headers: &HeaderMap) -> Option<String> {
    // 1. Product-specific x-**-session-id / x-**-sessionid
    for name in PRODUCT_SESSION_HEADERS {
        if let Some(sid) = header_session_value(headers, name) {
            return Some(sid);
        }
    }
    for (name, value) in headers.iter() {
        if is_generic_x_session_id(name.as_str()) {
            continue;
        }
        if !is_wildcard_session_header(name.as_str()) {
            continue;
        }
        if let Ok(v) = value.to_str() {
            let v = v.trim();
            if !v.is_empty() {
                return Some(sanitize_session_id(v));
            }
        }
    }

    // 2. Generic x-session-id
    if let Some(sid) = header_session_value(headers, GENERIC_SESSION_HEADER) {
        return Some(sid);
    }

    // 3. Other conversation/chat/thread aliases
    for name in ALIAS_SESSION_HEADERS {
        if let Some(sid) = header_session_value(headers, name) {
            return Some(sid);
        }
    }
    None
}

fn tenant_from_headers(headers: &HeaderMap) -> String {
    let raw = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer ").or(Some(s)))
        .or_else(|| headers.get("x-api-key").and_then(|h| h.to_str().ok()))
        .or_else(|| headers.get("x-goog-api-key").and_then(|h| h.to_str().ok()))
        .unwrap_or("anon");
    let hash = format!("{:x}", Sha256::digest(raw.as_bytes()));
    hash[..16].to_string()
}

pub fn sanitize_session_id(raw: &str) -> String {
    let mut out = String::new();
    for ch in raw.chars().take(128) {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':') {
            out.push(ch);
        }
    }
    if out.is_empty() {
        "sid-unknown".to_string()
    } else {
        out
    }
}

fn client_id_from_store_key(store_key: &str) -> &str {
    store_key.split_once(':').map(|(_, rest)| rest).unwrap_or(store_key)
}

fn is_real_signature(sig: &str) -> bool {
    sig.len() >= MIN_SIGNATURE_LENGTH && sig != SENTINEL_SIGNATURE
}

pub fn is_placeholder_thought(s: &str) -> bool {
    let t = s.trim();
    t.is_empty()
        || PLACEHOLDER_THOUGHTS.contains(&t)
        || t.chars().all(|c| c == '.' || c == '·' || c == '…')
}

fn is_capturable_thought(thought: &str, signature: Option<&str>) -> bool {
    if signature.is_some_and(is_real_signature) {
        return true;
    }
    !is_placeholder_thought(thought)
}

fn record_bytes(rec: &ThinkingRecord) -> usize {
    rec.thought.len()
        + rec.signature.as_ref().map(|s| s.len()).unwrap_or(0)
        + rec.visible.len()
}

fn is_stronger_record(new: &ThinkingRecord, old: &ThinkingRecord) -> bool {
    new.thought.len() > old.thought.len()
        || new.signature.as_ref().map(|s| s.len()).unwrap_or(0)
            > old.signature.as_ref().map(|s| s.len()).unwrap_or(0)
}

fn match_existing_record(
    rec: &ThinkingRecord,
    existing: &[Arc<ThinkingRecord>],
    used: &[bool],
) -> Option<usize> {
    if !rec.tool_ids.is_empty() {
        for (i, ex) in existing.iter().enumerate().rev() {
            if used[i] {
                continue;
            }
            if rec.tool_ids.iter().any(|id| ex.tool_ids.iter().any(|x| x == id)) {
                return Some(i);
            }
        }
    }
    let rec_has_tools = !rec.tool_ids.is_empty() || !rec.tool_names.is_empty();
    for (i, ex) in existing.iter().enumerate().rev() {
        if used[i] {
            continue;
        }
        if ex.fingerprint != rec.fingerprint {
            continue;
        }
        let ex_has_tools = !ex.tool_ids.is_empty() || !ex.tool_names.is_empty();
        if rec_has_tools == ex_has_tools {
            return Some(i);
        }
    }
    None
}

fn normalize_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut need_space = false;
    for word in s.split_whitespace() {
        if need_space {
            out.push(' ');
        }
        out.push_str(word);
        need_space = true;
    }
    out
}

fn hash_normalized_ws(hasher: &mut impl Digest, s: &str) {
    let mut need_space = false;
    for word in s.split_whitespace() {
        if need_space {
            hasher.update(b" ");
        }
        hasher.update(word.as_bytes());
        need_space = true;
    }
}

fn turn_needs_restore(parts: &[Value], existing_thought: &str) -> bool {
    if is_placeholder_thought(existing_thought) {
        return true;
    }
    let mut saw_function_call = false;
    for part in parts {
        if part.get("functionCall").is_some() {
            saw_function_call = true;
            if !part_has_signature(part) {
                return true;
            }
        }
    }
    if saw_function_call {
        return false;
    }
    !parts.iter().any(part_has_signature)
}

fn part_has_signature(part: &Value) -> bool {
    part.get("thoughtSignature")
        .or_else(|| part.get("thought_signature"))
        .and_then(|s| s.as_str())
        .is_some_and(is_real_signature)
}

fn inspect_parts(parts: &[Value]) -> (String, Vec<String>, Vec<String>, String) {
    let mut visible = String::new();
    let mut thought = String::new();
    let mut tool_ids = Vec::new();
    let mut tool_names = Vec::new();
    for part in parts {
        let is_thought = part.get("thought").and_then(|v| v.as_bool()).unwrap_or(false);
        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
            if is_thought {
                thought.push_str(text);
            } else {
                visible.push_str(text);
            }
        }
        if let Some(fc) = part.get("functionCall") {
            let name = fc
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let id = fc
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if !id.is_empty() {
                tool_ids.push(id);
            }
            tool_names.push(name);
        }
    }
    (visible, tool_ids, tool_names, thought)
}

pub fn fingerprint(visible: &str, tool_ids: &[String], tool_names: &[String]) -> String {
    let mut hasher = Sha256::new();
    hash_normalized_ws(&mut hasher, visible);
    hasher.update([0xff]);
    for id in tool_ids {
        hasher.update(id.as_bytes());
        hasher.update([0xfe]);
    }
    hasher.update([0xfd]);
    for name in tool_names {
        hasher.update(name.as_bytes());
        hasher.update([0xfc]);
    }
    let hex = format!("{:x}", hasher.finalize());
    hex[..16].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(thought: &str, visible: &str, tool_id: Option<&str>) -> ThinkingRecord {
        let tool_ids = tool_id
            .map(|id| vec![id.to_string()])
            .unwrap_or_default();
        let tool_names = if tool_id.is_some() {
            vec!["shell".to_string()]
        } else {
            Vec::new()
        };
        let fp = fingerprint(visible, &tool_ids, &tool_names);
        ThinkingRecord {
            fingerprint: fp,
            thought: thought.to_string(),
            signature: Some("s".repeat(60)),
            tool_ids,
            tool_names,
            visible: visible.to_string(),
        }
    }

    #[test]
    fn stores_full_thought_without_truncation() {
        let store = ThinkingStore::new();
        let long = "T".repeat(50_000);
        store.record("t:s1", rec(&long, "hello world", None));
        let mut contents = vec![json!({
            "role": "model",
            "parts": [{ "text": "hello world" }]
        })];
        let n = store.restore_gemini_contents("t:s1", &mut contents);
        assert_eq!(n, 1);
        assert_eq!(contents[0]["parts"][0]["text"].as_str().unwrap().len(), 50_000);
        assert_eq!(contents[0]["parts"][0]["thought"], true);
        assert_eq!(
            contents[0]["parts"][0]["thoughtSignature"].as_str().unwrap().len(),
            60
        );
    }

    #[test]
    fn matches_by_visible_text_not_index() {
        let store = ThinkingStore::new();
        store.record("t:s1", rec("think-A", "answer A", None));
        store.record("t:s1", rec("think-B", "answer B", None));

        // Client dropped turn A, only sends B (different packet shape / rewind)
        let mut contents = vec![json!({
            "role": "model",
            "parts": [{ "text": "answer B" }]
        })];
        store.restore_gemini_contents("t:s1", &mut contents);
        assert_eq!(contents[0]["parts"][0]["text"], "think-B");
    }

    #[test]
    fn matches_tool_id_when_text_missing() {
        let store = ThinkingStore::new();
        store.record("t:s1", rec("plan", "", Some("call_1")));
        let mut contents = vec![json!({
            "role": "model",
            "parts": [{
                "functionCall": { "name": "shell", "id": "call_1", "args": {} }
            }]
        })];
        store.restore_gemini_contents("t:s1", &mut contents);
        assert_eq!(contents[0]["parts"][0]["thought"], true);
        assert_eq!(contents[0]["parts"][0]["text"], "plan");
        assert_eq!(
            contents[0]["parts"][1]["thoughtSignature"].as_str().unwrap().len(),
            60
        );
    }

    #[test]
    fn replaces_placeholder_dots() {
        let store = ThinkingStore::new();
        store.record("t:s1", rec("full chain", "final", None));
        let mut contents = vec![json!({
            "role": "model",
            "parts": [
                { "text": "...", "thought": true },
                { "text": "final" }
            ]
        })];
        store.restore_gemini_contents("t:s1", &mut contents);
        assert_eq!(contents[0]["parts"][0]["text"], "full chain");
    }

    #[test]
    fn tenant_isolation_and_end_session() {
        let store = ThinkingStore::new();
        store.record("aaa:chat", rec("secret-a", "hi", None));
        store.record("bbb:chat", rec("secret-b", "hi", None));

        let mut a = vec![json!({"role":"model","parts":[{"text":"hi"}]})];
        store.restore_gemini_contents("aaa:chat", &mut a);
        assert_eq!(a[0]["parts"][0]["text"], "secret-a");

        let result = store.end_session("aaa:chat");
        assert_eq!(result.deleted_turns, 1);
        let mut a2 = vec![json!({"role":"model","parts":[{"text":"hi"}]})];
        assert_eq!(store.restore_gemini_contents("aaa:chat", &mut a2), 0);

        let mut b = vec![json!({"role":"model","parts":[{"text":"hi"}]})];
        store.restore_gemini_contents("bbb:chat", &mut b);
        assert_eq!(b[0]["parts"][0]["text"], "secret-b");
    }

    #[test]
    fn sanitize_rejects_junk() {
        assert_eq!(sanitize_session_id("abc/../x"), "abc..x");
        assert_eq!(sanitize_session_id(""), "sid-unknown");
    }

    #[test]
    fn keyword_forces_thinking_without_client_flag() {
        assert!(model_forces_server_thinking("gemini-3-flash"));
        assert!(model_forces_server_thinking("gemini-3-pro"));
        assert!(model_forces_server_thinking("gemini-3-flash-agent"));
        assert!(model_forces_server_thinking("gemini-pro-agent"));
        assert!(model_forces_server_thinking("claude-sonnet-4-6"));
        assert!(!model_forces_server_thinking("gpt-4o"));
        assert!(!model_forces_server_thinking("gemini-3-pro-image"));
        assert!(!model_forces_server_thinking("gemini-3.1-flash-lite"));
        assert!(!model_forces_server_thinking("gemini-3-pro-preview"));
    }

    #[test]
    fn same_fingerprint_updates_in_place() {
        let store = ThinkingStore::new();
        store.record("t:s1", rec("short", "same", None));
        store.record("t:s1", rec("much longer thought", "same", None));
        let stats = store.session_stats("t:s1").unwrap();
        assert_eq!(stats.0, 1);
        let mut contents = vec![json!({"role":"model","parts":[{"text":"same"}]})];
        store.restore_gemini_contents("t:s1", &mut contents);
        assert_eq!(contents[0]["parts"][0]["text"], "much longer thought");
    }

    #[test]
    fn tool_record_does_not_pollute_earlier_text_turns() {
        let store = ThinkingStore::new();
        // Turn 2 generated thinking + tool call
        store.record(
            "t:s1",
            rec(
                "**Inferring User's Intention**",
                "",
                Some("call_54421"),
            ),
        );

        // Turn 0: "你好！" (pure text, no thought)
        // Turn 1: "当然是真的！" (pure text, no thought)
        // Turn 2: tool call "call_54421"
        let mut contents = vec![
            json!({
                "role": "model",
                "parts": [{ "text": "你好！我是 JeikCode AI 编程助手。" }]
            }),
            json!({
                "role": "model",
                "parts": [{ "text": "当然是真的！😄" }]
            }),
            json!({
                "role": "model",
                "parts": [{
                    "functionCall": { "name": "shell", "id": "call_54421", "args": {} }
                }]
            }),
        ];

        let restored = store.restore_gemini_contents("t:s1", &mut contents);
        assert_eq!(restored, 1);

        // Turn 0 must NOT have thinking injected
        assert_eq!(contents[0]["parts"].as_array().unwrap().len(), 1);
        assert_eq!(contents[0]["parts"][0]["text"], "你好！我是 JeikCode AI 编程助手。");
        assert!(contents[0]["parts"][0].get("thought").is_none());

        // Turn 1 must NOT have thinking injected
        assert_eq!(contents[1]["parts"].as_array().unwrap().len(), 1);
        assert_eq!(contents[1]["parts"][0]["text"], "当然是真的！😄");
        assert!(contents[1]["parts"][0].get("thought").is_none());

        // Turn 2 MUST have thinking injected and matched with call_54421
        assert_eq!(contents[2]["parts"][0]["thought"], true);
        assert_eq!(contents[2]["parts"][0]["text"], "**Inferring User's Intention**");
        assert_eq!(
            contents[2]["parts"][1]["functionCall"]["id"],
            "call_54421"
        );
    }

    #[test]
    fn test_thinking_with_text_and_parallel_tools() {
        let store = ThinkingStore::new();

        // Test Case 6: 有思考、有正文、有多工具并行出来
        let tool_ids = vec!["call_batch_1".to_string(), "call_batch_2".to_string()];
        let tool_names = vec!["read_file".to_string(), "grep_search".to_string()];
        let fp = fingerprint("I will read both files in parallel", &tool_ids, &tool_names);
        store.record(
            "t:s2",
            ThinkingRecord {
                fingerprint: fp,
                thought: "Parallel execution planned".to_string(),
                signature: Some("sig_parallel_12345678901234567890123456789012345678901234567890".to_string()),
                tool_ids: tool_ids.clone(),
                tool_names: tool_names.clone(),
                visible: "I will read both files in parallel".to_string(),
            },
        );

        let mut contents = vec![json!({
            "role": "model",
            "parts": [
                { "text": "I will read both files in parallel" },
                { "functionCall": { "name": "read_file", "id": "call_batch_1", "args": {} } },
                { "functionCall": { "name": "grep_search", "id": "call_batch_2", "args": {} } }
            ]
        })];

        let restored = store.restore_gemini_contents("t:s2", &mut contents);
        assert_eq!(restored, 1);

        let parts = contents[0]["parts"].as_array().unwrap();
        // Index 0: thought block
        assert_eq!(parts[0]["thought"], true);
        assert_eq!(parts[0]["text"], "Parallel execution planned");
        assert_eq!(parts[0]["thoughtSignature"], "sig_parallel_12345678901234567890123456789012345678901234567890");

        // Index 1: visible text preserved
        assert_eq!(parts[1]["text"], "I will read both files in parallel");

        // Index 2: tool 1 has signature
        assert_eq!(parts[2]["functionCall"]["id"], "call_batch_1");
        assert_eq!(parts[2]["thoughtSignature"], "sig_parallel_12345678901234567890123456789012345678901234567890");

        // Index 3: tool 2 has signature
        assert_eq!(parts[3]["functionCall"]["id"], "call_batch_2");
        assert_eq!(parts[3]["thoughtSignature"], "sig_parallel_12345678901234567890123456789012345678901234567890");
    }

    #[test]
    fn test_sqlite_persistence_and_recovery() {
        let store_key = "test_session_sqlite_recovery_unique";
        let fp = fingerprint("Persisted visible text", &["call_persisted_999".to_string()], &["bash".to_string()]);
        let rec = ThinkingRecord {
            fingerprint: fp,
            thought: "Thought restored from SQLite".to_string(),
            signature: Some("sig_persisted_1234567890123456789012345678901234567890".to_string()),
            tool_ids: vec!["call_persisted_999".to_string()],
            tool_names: vec!["bash".to_string()],
            visible: "Persisted visible text".to_string(),
        };

        let db_res = crate::modules::proxy_db::save_thinking_record(
            store_key,
            &rec.fingerprint,
            &rec.thought,
            rec.signature.as_deref(),
            &rec.tool_ids,
            &rec.tool_names,
            &rec.visible,
        );
        if let Err(e) = db_res {
            eprintln!("Skipping DB test if DB not initialized: {}", e);
            return;
        }

        let store = ThinkingStore::new();

        let mut contents = vec![json!({
            "role": "model",
            "parts": [
                { "text": "Persisted visible text" },
                { "functionCall": { "name": "bash", "id": "call_persisted_999", "args": {} } }
            ]
        })];

        let restored = store.restore_gemini_contents(store_key, &mut contents);
        assert_eq!(restored, 1);

        let parts = contents[0]["parts"].as_array().unwrap();
        assert_eq!(parts[0]["thought"], true);
        assert_eq!(parts[0]["text"], "Thought restored from SQLite");
        assert_eq!(parts[0]["thoughtSignature"], "sig_persisted_1234567890123456789012345678901234567890");
        assert_eq!(parts[2]["thoughtSignature"], "sig_persisted_1234567890123456789012345678901234567890");
    }

    #[test]
    fn sqlite_merges_latest_chunk_but_keeps_older_same_fingerprint_turn() {
        let store_key = "test_session_fp_hello_isolation";
        if crate::modules::proxy_db::delete_thinking_records_for_session(store_key).is_err() {
            return;
        }

        let sig = "sig_hello_12345678901234567890123456789012345678901234567890";
        let fp_hello = fingerprint("你好", &[], &[]);
        let fp_other = fingerprint("other", &["call_x".to_string()], &["bash".to_string()]);

        let save = |fp: &str, thought: &str, visible: &str, ids: &[String], names: &[String]| {
            crate::modules::proxy_db::save_thinking_record(
                store_key, fp, thought, Some(sig), ids, names, visible,
            )
        };

        assert!(save(&fp_hello, "thought-1", "你好", &[], &[]).is_ok());
        assert!(save(&fp_hello, "thought-1-longer", "你好", &[], &[]).is_ok());
        assert!(save(
            &fp_other,
            "thought-tool",
            "other",
            &["call_x".to_string()],
            &["bash".to_string()]
        )
        .is_ok());
        assert!(save(&fp_hello, "thought-3", "你好", &[], &[]).is_ok());

        let rows = crate::modules::proxy_db::load_thinking_records(store_key).unwrap_or_default();
        assert_eq!(rows.len(), 3, "older 你好 turn must not be overwritten: {rows:?}");
        assert_eq!(rows[0].thought, "thought-1-longer");
        assert_eq!(rows[1].thought, "thought-tool");
        assert_eq!(rows[2].thought, "thought-3");
        let _ = crate::modules::proxy_db::delete_thinking_records_for_session(store_key);
    }

    #[test]
    fn test_explicit_session_id_and_query_extraction() {
        // 1. Query parameter extraction
        let sid = extract_session_from_query_str("model=gemini-2.5-pro&session_id=win_alpha_101&temp=0.7");
        assert_eq!(sid.as_deref(), Some("win_alpha_101"));

        let sid_alias = extract_session_from_query_str("channel=proj_beta");
        assert_eq!(sid_alias.as_deref(), Some("proj_beta"));

        // 2. Header extraction: Claude Code / Cursor / VSCode
        let mut headers = HeaderMap::new();
        headers.insert("x-cursor-session-id", "cursor-tab-99".parse().unwrap());
        let scope = SessionScope::from_headers(&headers, "fallback_id");
        assert_eq!(scope.client_id, "cursor-tab-99");

        // 3. Body & Metadata extraction
        let body = json!({
            "metadata": {
                "conversation_id": "meta-conv-888"
            }
        });
        let empty_headers = HeaderMap::new();
        let scope2 = SessionScope::from_headers_and_body(&empty_headers, Some(&body), "fallback_id");
        assert_eq!(scope2.client_id, "meta-conv-888");
    }

    #[test]
    fn test_wildcard_client_session_headers() {
        let uuid = "c17b6d3c-e808-4874-8f16-b5dd4b6a2179";

        let mut atom = HeaderMap::new();
        atom.insert("x-atomcode-session-id", uuid.parse().unwrap());
        assert_eq!(SessionScope::from_headers(&atom, "fallback").client_id, uuid);

        let mut jeik = HeaderMap::new();
        jeik.insert("x-jeikcode-sessionid", uuid.parse().unwrap());
        assert_eq!(SessionScope::from_headers(&jeik, "fallback").client_id, uuid);

        let mut multi = HeaderMap::new();
        multi.insert("x-api-key", "secret".parse().unwrap());
        multi.insert("x-atomcode-session-id", uuid.parse().unwrap());
        multi.insert("x-jeikcode-sessionid", uuid.parse().unwrap());
        multi.insert("x-session-id", uuid.parse().unwrap());
        assert_eq!(SessionScope::from_headers(&multi, "fallback").client_id, uuid);

        let mut custom = HeaderMap::new();
        custom.insert("x-windsurf-session-id", "wind-tab-1".parse().unwrap());
        assert_eq!(
            SessionScope::from_headers(&custom, "fallback").client_id,
            "wind-tab-1"
        );

        let mut ignored = HeaderMap::new();
        ignored.insert("x-request-id", "req-should-not-win".parse().unwrap());
        ignored.insert("x-api-key", "secret".parse().unwrap());
        assert_eq!(
            SessionScope::from_headers(&ignored, "fallback_id").client_id,
            "fallback_id"
        );
    }

    #[test]
    fn product_session_headers_win_over_generic_x_session_id() {
        let mut jeik = HeaderMap::new();
        jeik.insert("x-session-id", "generic-session".parse().unwrap());
        jeik.insert("x-jeikcode-sessionid", "jeik-session".parse().unwrap());
        assert_eq!(
            SessionScope::from_headers(&jeik, "fallback").client_id,
            "jeik-session"
        );

        let mut atom = HeaderMap::new();
        atom.insert("x-session-id", "generic-session".parse().unwrap());
        atom.insert("x-atomcode-session-id", "atom-session".parse().unwrap());
        assert_eq!(
            SessionScope::from_headers(&atom, "fallback").client_id,
            "atom-session"
        );

        let mut wildcard = HeaderMap::new();
        wildcard.insert("x-session-id", "generic-session".parse().unwrap());
        wildcard.insert("x-windsurf-session-id", "wind-session".parse().unwrap());
        assert_eq!(
            SessionScope::from_headers(&wildcard, "fallback").client_id,
            "wind-session"
        );

        let mut only_generic = HeaderMap::new();
        only_generic.insert("x-session-id", "generic-session".parse().unwrap());
        assert_eq!(
            SessionScope::from_headers(&only_generic, "fallback").client_id,
            "generic-session"
        );
    }

    #[test]
    fn tail_first_match_uses_latest_record_for_latest_incomplete_turn() {
        let store = ThinkingStore::new();
        let key = "t:tail-hello";
        store.record(key, rec("think-turn1", "你好", None));
        store.record(key, rec("think-turn2", "你好", None));

        let sig = "s".repeat(60);
        let mut contents = vec![
            json!({
                "role": "model",
                "parts": [
                    { "text": "think-turn1", "thought": true, "thoughtSignature": sig },
                    { "text": "你好" }
                ]
            }),
            json!({
                "role": "model",
                "parts": [{ "text": "你好" }]
            }),
        ];
        let restored = store.restore_gemini_contents(key, &mut contents);
        assert_eq!(restored, 1);
        assert_eq!(contents[0]["parts"][0]["text"], "think-turn1");
        assert_eq!(contents[1]["parts"][0]["thought"], true);
        assert_eq!(
            contents[1]["parts"][0]["text"],
            "think-turn2",
            "latest incomplete turn must take the latest matching record, not the first 你好"
        );
    }

    #[test]
    fn concurrent_sessions_do_not_mix_thinking() {
        use std::thread;

        let store = Arc::new(ThinkingStore::new());
        let handles: Vec<_> = (0..48)
            .map(|i| {
                let store = Arc::clone(&store);
                thread::spawn(move || {
                    let key = format!("t:conc-{i}");
                    let thought = format!("thought-{i}");
                    let visible = format!("hello-{i}");
                    store.record(&key, rec(&thought, &visible, None));
                    let mut contents = vec![json!({
                        "role": "model",
                        "parts": [{ "text": visible }]
                    })];
                    let n = store.restore_gemini_contents(&key, &mut contents);
                    assert_eq!(n, 1);
                    assert_eq!(contents[0]["parts"][0]["text"], thought);
                })
            })
            .collect();
        for h in handles {
            h.join().expect("session thread panicked");
        }
    }

    #[test]
    fn capture_from_client_history_and_prune_compressed_turns() {
        let store = ThinkingStore::new();
        let key = "t:compress-session";
        store.record(key, rec("thought-old-1", "old visible one", Some("call_old_1")));
        store.record(key, rec("thought-old-2", "old visible two", Some("call_old_2")));
        store.record(key, rec("thought-old-3", "old visible three", Some("call_old_3")));
        store.record(key, rec("thought-keep", "kept latest answer", Some("call_keep")));
        store.record(key, rec("thought-tail", "newest unused", None));

        // Client /compact dropped the first three turns; only the latest kept turn remains.
        let mut contents = vec![json!({
            "role": "model",
            "parts": [
                { "text": "kept latest answer" },
                { "functionCall": { "name": "shell", "id": "call_keep", "args": {} } }
            ]
        })];

        let restored = store.restore_gemini_contents(key, &mut contents);
        assert_eq!(restored, 1);
        store.prune_orphaned_records(key, &contents);
        let (turns, _) = store.session_stats(key).unwrap();
        assert!(turns <= 3, "orphaned compressed turns should be pruned, got {turns}");
        let parts = contents[0]["parts"].as_array().unwrap();
        assert_eq!(parts[0]["text"], "thought-keep");
    }

    #[test]
    fn fingerprint_matches_legacy_whitespace_join() {
        let visible = "  hello\n\tworld  foo";
        let ids: Vec<String> = vec!["call_1".to_string()];
        let names: Vec<String> = vec!["shell".to_string()];
        let mut hasher = Sha256::new();
        let norm: String = visible.split_whitespace().collect::<Vec<_>>().join(" ");
        hasher.update(norm.as_bytes());
        hasher.update([0xff]);
        hasher.update(ids[0].as_bytes());
        hasher.update([0xfe]);
        hasher.update([0xfd]);
        hasher.update(names[0].as_bytes());
        hasher.update([0xfc]);
        let hex = format!("{:x}", hasher.finalize());
        assert_eq!(fingerprint(visible, &ids, &names), hex[..16].to_string());
    }

    #[test]
    fn placeholder_thoughts_are_not_recorded() {
        let store = ThinkingStore::new();
        let key = "t:placeholder-skip";
        store.record(
            key,
            ThinkingRecord {
                fingerprint: fingerprint("visible answer", &[], &[]),
                thought: "...".to_string(),
                signature: None,
                tool_ids: vec![],
                tool_names: vec![],
                visible: "visible answer".to_string(),
            },
        );
        store.record(
            key,
            ThinkingRecord {
                fingerprint: fingerprint("visible answer", &[], &[]),
                thought: "...".to_string(),
                signature: Some(SENTINEL_SIGNATURE.to_string()),
                tool_ids: vec![],
                tool_names: vec![],
                visible: "visible answer".to_string(),
            },
        );
        assert!(
            store.session_stats(key).is_none(),
            "placeholder / sentinel-only thoughts must not create store turns"
        );
    }

    #[test]
    fn ingest_placeholders_does_not_duplicate_history() {
        let store = ThinkingStore::new();
        let key = "t:ingest-no-dup";
        for i in 0..20 {
            store.record(
                key,
                rec(
                    &format!("thought-{i}"),
                    &format!("answer {i}"),
                    Some(&format!("call_{i}")),
                ),
            );
        }
        let (before, _) = store.session_stats(key).unwrap();
        assert_eq!(before, 20);

        let contents: Vec<Value> = (0..20)
            .map(|i| {
                json!({
                    "role": "model",
                    "parts": [
                        { "text": "...", "thought": true, "thoughtSignature": SENTINEL_SIGNATURE },
                        { "text": format!("answer {i}") },
                        { "functionCall": { "name": "shell", "id": format!("call_{i}"), "args": {} } }
                    ]
                })
            })
            .collect();

        store.ingest_from_contents(key, &contents);
        let (after, _) = store.session_stats(key).unwrap();
        assert_eq!(after, 20, "placeholder history must not be appended as new turns");
        let _ = crate::modules::proxy_db::delete_thinking_records_for_session(key);
    }

    #[test]
    fn ingest_real_client_thinking_is_idempotent() {
        let store = ThinkingStore::new();
        let key = "t:ingest-real";
        let sig = "s".repeat(60);
        let contents = vec![json!({
            "role": "model",
            "parts": [
                { "text": "full chain of thought here", "thought": true, "thoughtSignature": sig },
                { "text": "hello world" }
            ]
        })];
        store.ingest_from_contents(key, &contents);
        store.ingest_from_contents(key, &contents);
        let (turns, _) = store.session_stats(key).unwrap();
        assert_eq!(turns, 1);
        let _ = crate::modules::proxy_db::delete_thinking_records_for_session(key);
    }

    #[test]
    fn restore_large_visible_text_is_linear() {
        let store = ThinkingStore::new();
        let key = "t:big-visible";
        let blob = "word ".repeat(8_000); // ~40KB per turn
        for i in 0..30 {
            let visible = format!("head-{i} {blob}");
            store.record(key, rec(&format!("thought-{i}"), &visible, None));
        }
        // Truncated visibles force Phase 3 prefix matching (the old quadratic path).
        let mut contents: Vec<Value> = (0..30)
            .map(|i| {
                json!({
                    "role": "model",
                    "parts": [{ "text": format!("head-{i}") }]
                })
            })
            .collect();
        let start = Instant::now();
        let n = store.restore_gemini_contents(key, &mut contents);
        let elapsed = start.elapsed();
        assert_eq!(n, 30);
        assert!(
            elapsed.as_millis() < 800,
            "restore of ~1.2MB visible text took {elapsed:?}; matching must not be quadratic"
        );
        let _ = crate::modules::proxy_db::delete_thinking_records_for_session(key);
    }

    #[test]
    fn placeholder_history_fills_in_order_without_scramble() {
        let store = ThinkingStore::new();
        let key = "t:fill-order";
        for i in 0..12 {
            store.record(
                key,
                rec(
                    &format!("THOUGHT-BLOCK-{i}"),
                    &format!("answer {i}"),
                    Some(&format!("call_{i}")),
                ),
            );
        }
        let mut contents: Vec<Value> = (0..12)
            .map(|i| {
                json!({
                    "role": "model",
                    "parts": [
                        { "text": "...", "thought": true, "thoughtSignature": SENTINEL_SIGNATURE },
                        { "text": format!("answer {i}") },
                        { "functionCall": { "name": "shell", "id": format!("call_{i}"), "args": { "n": i } } }
                    ]
                })
            })
            .collect();

        assert!(!contents_have_capturable_thought(&contents));
        let n = store.restore_gemini_contents(key, &mut contents);
        assert_eq!(n, 12);
        for i in 0..12 {
            let parts = contents[i]["parts"].as_array().unwrap();
            assert_eq!(parts[0]["thought"], true, "thought must stay at parts[0] for turn {i}");
            assert_eq!(parts[0]["text"], format!("THOUGHT-BLOCK-{i}"));
            assert_eq!(parts[0]["thoughtSignature"].as_str().unwrap().len(), 60);
            assert_eq!(parts[1]["text"], format!("answer {i}"));
            assert_eq!(parts[2]["functionCall"]["id"], format!("call_{i}"));
            assert_eq!(parts[2]["functionCall"]["args"]["n"], i);
            assert_eq!(parts[2]["thoughtSignature"].as_str().unwrap().len(), 60);
        }
        store.prune_orphaned_records(key, &contents);
        let (turns, _) = store.session_stats(key).unwrap();
        assert_eq!(turns, 12, "placeholder fill must not prune live tool turns");
        let _ = crate::modules::proxy_db::delete_thinking_records_for_session(key);
    }
}

