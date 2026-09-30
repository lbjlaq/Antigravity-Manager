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

const MIN_SIGNATURE_LENGTH: usize = 32;
pub const SENTINEL_SIGNATURE: &str = "skip_thought_signature_validator";
const MAX_SESSIONS: usize = 2000;
fn max_turns_per_session() -> usize {
    crate::proxy::config::get_thinking_max_memory_turns()
}
const MAX_BYTES_PER_SESSION: usize = 64 * 1024 * 1024;
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
    if m.contains("image")
        || m.contains("imagen")
        || m.contains("embed")
        || m.contains("lite")
        || m.contains("preview")
    {
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

        // Always hydrate L2 before appending. Otherwise the first capture after a
        // process start can mark l2_loaded=true with only the new turn and permanently
        // shadow older SQLite history on subsequent hydrate/restore calls.
        let needs_l2 = self
            .sessions
            .get(store_key)
            .map(|e| !e.l2_loaded)
            .unwrap_or(true);
        if needs_l2 {
            let _ = self.load_turns(store_key);
        }

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
                while entry.turns.len() > max_turns_per_session()
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
            // Trust warm non-empty memory. An empty l2_loaded entry is treated as
            // stale (e.g. first hydrate before any capture) and reloads from SQLite.
            if e.l2_loaded && !e.turns.is_empty() {
                let turns = e.turns.clone();
                drop(e);
                if let Some(mut entry) = self.sessions.get_mut(store_key) {
                    entry.last_access = Instant::now();
                }
                return turns;
            }
        }

        let persisted =
            crate::modules::proxy_db::load_thinking_records(store_key).unwrap_or_default();
        let loaded_len = persisted.len();
        let mut entry = self
            .sessions
            .entry(store_key.to_string())
            .or_insert_with(SessionEntry::new);
        if entry.turns.is_empty() && !persisted.is_empty() {
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
        for (c_idx, content) in contents.iter().enumerate() {
            let role = content.get("role").and_then(|v| v.as_str()).unwrap_or("");
            if role != "model" && role != "assistant" {
                continue;
            }
            let Some(parts) = content.get("parts").and_then(|p| p.as_array()) else {
                continue;
            };
            let preceding_turn = if c_idx > 0 {
                contents.get(c_idx - 1)
            } else {
                None
            };
            let anchor = compute_causal_anchor(preceding_turn);
            let mut acc = TurnAccumulator::with_anchor(&anchor);
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
        self.restore_gemini_contents_with_model(store_key, contents, None)
    }

    pub fn restore_gemini_contents_with_model(
        &self,
        store_key: &str,
        contents: &mut Vec<Value>,
        target_model: Option<&str>,
    ) -> usize {
        if !crate::proxy::config::is_thinking_store_enabled() {
            return 0;
        }
        if contents.is_empty() || store_key.is_empty() {
            return 0;
        }

        let mut records = self.load_turns(store_key);

        // 收集所有的 model 轮次元信息
        struct ModelTurnMeta {
            content_idx: usize,
            visible: String,
            norm_visible: String,
            tool_ids: Vec<String>,
            #[allow(dead_code)]
            tool_names: Vec<String>,
            existing_thought: String,
            existing_sig: Option<String>,
            fp: String,
            matched_record_idx: Option<usize>,
            already_complete: bool,
        }

        let mut model_turns: Vec<ModelTurnMeta> = Vec::new();
        for (c_idx, content) in contents.iter().enumerate() {
            let role = content.get("role").and_then(|v| v.as_str()).unwrap_or("");
            if role != "model" && role != "assistant" {
                continue;
            }
            let Some(parts) = content.get("parts").and_then(|p| p.as_array()) else {
                continue;
            };
            let preceding_turn = if c_idx > 0 {
                contents.get(c_idx - 1)
            } else {
                None
            };
            let anchor = compute_causal_anchor(preceding_turn);
            let (visible, tool_ids, tool_names, existing_thought) =
                inspect_parts_with_anchor(parts, &anchor);
            let existing_sig = parts.iter().find_map(|p| {
                let sig = p
                    .get("thoughtSignature")
                    .or_else(|| p.get("thought_signature"))
                    .and_then(|s| s.as_str())
                    .filter(|s| is_real_signature(s))
                    .map(str::to_string);
                sig.or_else(|| {
                    p.get("functionCall")
                        .and_then(|fc| fc.get("id"))
                        .and_then(|id| id.as_str())
                        .and_then(|id| {
                            crate::proxy::SignatureCache::global().get_tool_signature(store_key, id)
                        })
                        .filter(|s| is_real_signature(s))
                })
            });
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
                existing_sig,
                fp: String::new(),
                matched_record_idx: None,
                already_complete,
            });
        }

        if model_turns.is_empty() {
            return 0;
        }

        let mut used = vec![false; records.len()];
        let mut by_sig: HashMap<String, usize> = HashMap::new();
        let mut by_tool: HashMap<String, Vec<usize>> = HashMap::new();
        let mut by_fp: HashMap<&str, Vec<usize>> = HashMap::new();
        for (rec_idx, rec) in records.iter().enumerate() {
            if let Some(ref sig) = rec.signature.as_ref().filter(|s| is_real_signature(s)) {
                let norm = normalize_signature_for_comparison(sig);
                by_sig.insert(norm.into_owned(), rec_idx);
            }
            for id in &rec.tool_ids {
                let norm = crate::proxy::common::utils::normalize_tool_id(id);
                by_tool.entry(norm.to_string()).or_default().push(rec_idx);
                if norm.as_ref() != id.as_str() {
                    by_tool.entry(id.clone()).or_default().push(rec_idx);
                }
            }
            by_fp
                .entry(rec.fingerprint.as_str())
                .or_default()
                .push(rec_idx);
        }

        // Phase 0: 真实签名直查（最高优先级：客户端历史若自带真实签名，直接精确反向匹配）
        for turn in model_turns.iter_mut() {
            if turn.already_complete || turn.matched_record_idx.is_some() {
                continue;
            }
            if let Some(ref sig) = turn.existing_sig {
                let norm = normalize_signature_for_comparison(sig);
                if let Some(&rec_idx) = by_sig.get(norm.as_ref()) {
                    if !used[rec_idx] {
                        // 防错配保护：工具调用轮次绝不能匹配纯文本记录，纯文本轮次绝不能匹配工具记录！
                        let turn_has_tools =
                            !turn.tool_ids.is_empty() || !turn.tool_names.is_empty();
                        let rec_has_tools = !records[rec_idx].tool_ids.is_empty()
                            || !records[rec_idx].tool_names.is_empty();
                        if turn_has_tools == rec_has_tools {
                            let tool_names_match = if turn.tool_names.is_empty()
                                || records[rec_idx].tool_names.is_empty()
                            {
                                true
                            } else {
                                turn.tool_names == records[rec_idx].tool_names
                            };
                            if tool_names_match {
                                turn.matched_record_idx = Some(rec_idx);
                                used[rec_idx] = true;
                            }
                        }
                    }
                }
            }
        }

        // Phase 1: 工具调用 ID 精准锚定（包括原生唯一 ID 与确定性合成 ID，正向保序匹配）
        for turn in model_turns.iter_mut() {
            if turn.already_complete
                || turn.matched_record_idx.is_some()
                || turn.tool_ids.is_empty()
            {
                continue;
            }
            for id in &turn.tool_ids {
                let norm = crate::proxy::common::utils::normalize_tool_id(id);
                let idxs = by_tool
                    .get(norm.as_ref())
                    .or_else(|| by_tool.get(id.as_str()));
                let Some(idxs) = idxs else {
                    continue;
                };
                if let Some(&rec_idx) = idxs.iter().find(|&&i| {
                    if used[i] {
                        return false;
                    }
                    // 严密防御工具名不匹配：防止 ID 碰撞导致把其他工具的思考与签名挂到当前工具上！
                    if !turn.tool_names.is_empty() && !records[i].tool_names.is_empty() {
                        if turn.tool_names != records[i].tool_names {
                            return false;
                        }
                    }
                    true
                }) {
                    turn.matched_record_idx = Some(rec_idx);
                    used[rec_idx] = true;
                    break;
                }
            }
        }

        // Phase 2: 完整指纹匹配（正向保序匹配，杜绝修剪后逆向滑窗相位错位）
        for turn in model_turns.iter_mut() {
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
            if let Some(&rec_idx) = idxs.iter().find(|&&i| {
                if used[i] {
                    return false;
                }
                let rec_has_tools =
                    !records[i].tool_ids.is_empty() || !records[i].tool_names.is_empty();
                if rec_has_tools != turn_has_tools {
                    return false;
                }
                if !turn.tool_names.is_empty() && !records[i].tool_names.is_empty() {
                    if turn.tool_names != records[i].tool_names {
                        return false;
                    }
                }
                true
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
            for turn in model_turns.iter_mut() {
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
                for (rec_idx, rec) in records.iter().enumerate() {
                    let rec_has_tools = !rec.tool_ids.is_empty() || !rec.tool_names.is_empty();
                    if used[rec_idx] || rec_has_tools || rec_norms[rec_idx].is_empty() {
                        continue;
                    }
                    let norm_rec = &rec_norms[rec_idx];
                    let norm_vis = &turn.norm_visible;
                    let thought_matches_prefix = !rec.thought.trim().is_empty()
                        && norm_vis.starts_with(&normalize_ws(&rec.thought));
                    if norm_rec == norm_vis
                        || norm_rec.starts_with(norm_vis)
                        || norm_vis.starts_with(norm_rec)
                        || (norm_rec.len() >= 10 && norm_vis.ends_with(norm_rec))
                        || (thought_matches_prefix && norm_vis.ends_with(norm_rec))
                    {
                        turn.matched_record_idx = Some(rec_idx);
                        used[rec_idx] = true;
                        break;
                    }
                }
            }
        }

        // Phase 3.5: L2 SQLite 精准穿透回捞 (针对超过内存容量淘汰或冷启动的历史轮次)
        // 核心原则：淘汰轮次绝不盲目降级占位符！优先通过 signature / tool_id / fingerprint 从 SQLite 索引中精准回捞
        for turn in model_turns.iter_mut() {
            if turn.already_complete || turn.matched_record_idx.is_some() {
                continue;
            }

            let mut fetched_rec: Option<ThinkingRecord> = None;

            // 0. 优先按签名精准穿透
            if let Some(ref sig) = turn.existing_sig {
                let mut found =
                    crate::modules::proxy_db::load_thinking_by_signature(store_key, sig);
                if found.as_ref().map(|o| o.is_none()).unwrap_or(true) && is_claude_signature(sig) {
                    let google_sig = ensure_google_claude_thought_signature(sig);
                    if google_sig != *sig {
                        found = crate::modules::proxy_db::load_thinking_by_signature(
                            store_key,
                            &google_sig,
                        );
                    }
                }
                if let Ok(Some(persisted)) = found {
                    let turn_has_tools = !turn.tool_ids.is_empty() || !turn.tool_names.is_empty();
                    let rec_has_tools =
                        !persisted.tool_ids.is_empty() || !persisted.tool_names.is_empty();
                    if turn_has_tools == rec_has_tools {
                        fetched_rec = Some(ThinkingRecord {
                            fingerprint: persisted.fingerprint,
                            thought: persisted.thought,
                            signature: persisted.signature,
                            tool_ids: persisted.tool_ids,
                            tool_names: persisted.tool_names,
                            visible: persisted.visible,
                        });
                    }
                }
            }

            // 1. 工具调用精准穿透点查 (利用 primary_tool_id Partial Index，纳秒级命中)
            if fetched_rec.is_none() && !turn.tool_ids.is_empty() {
                for id in &turn.tool_ids {
                    if let Ok(Some(persisted)) =
                        crate::modules::proxy_db::load_thinking_by_tool_id(store_key, id)
                    {
                        let tool_names_match =
                            if turn.tool_names.is_empty() || persisted.tool_names.is_empty() {
                                true
                            } else {
                                turn.tool_names == persisted.tool_names
                            };
                        if tool_names_match {
                            fetched_rec = Some(ThinkingRecord {
                                fingerprint: persisted.fingerprint,
                                thought: persisted.thought,
                                signature: persisted.signature,
                                tool_ids: persisted.tool_ids,
                                tool_names: persisted.tool_names,
                                visible: persisted.visible,
                            });
                            break;
                        }
                    }
                }
            } else if fetched_rec.is_none() && !turn.visible.trim().is_empty() {
                // 2. 纯文本轮次精准穿透点查 (利用 idx_thinking_rec_fp 索引)
                if turn.fp.is_empty() {
                    turn.fp = fingerprint(&turn.visible, &turn.tool_ids, &turn.tool_names);
                }
                if let Ok(Some(persisted)) =
                    crate::modules::proxy_db::load_thinking_by_fingerprint(store_key, &turn.fp)
                {
                    fetched_rec = Some(ThinkingRecord {
                        fingerprint: persisted.fingerprint,
                        thought: persisted.thought,
                        signature: persisted.signature,
                        tool_ids: persisted.tool_ids,
                        tool_names: persisted.tool_names,
                        visible: persisted.visible,
                    });
                }
            }

            if let Some(rec) = fetched_rec {
                let rec_arc = Arc::new(rec);
                records.push(rec_arc.clone());
                used.push(true);
                let new_idx = records.len() - 1;
                turn.matched_record_idx = Some(new_idx);
                // 同步注册回内存会话实体，确保后续轮次无需重复点查
                if let Some(mut entry) = self.sessions.get_mut(store_key) {
                    entry.bytes = entry.bytes.saturating_add(record_bytes(&rec_arc));
                    entry.turns.push(rec_arc);
                }
            }
        }

        // Phase 4: 尾部优先的逆向兜底匹配（仅限纯文本轮次，绝不跨轮借用工具签名造成下轮突变！）
        if let Some(last_turn) = model_turns.last_mut() {
            if !last_turn.already_complete && last_turn.matched_record_idx.is_none() {
                let last_turn_has_tools =
                    !last_turn.tool_ids.is_empty() || !last_turn.tool_names.is_empty();
                if !last_turn_has_tools {
                    if let Some((last_unused_rec_idx, _)) =
                        records.iter().enumerate().rfind(|(idx, r)| {
                            if used[*idx] {
                                return false;
                            }
                            let r_has_tools = !r.tool_ids.is_empty() || !r.tool_names.is_empty();
                            !r_has_tools
                        })
                    {
                        last_turn.matched_record_idx = Some(last_unused_rec_idx);
                        used[last_unused_rec_idx] = true;
                    }
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

            let has_meaningful_thought =
                !is_placeholder_thought(&rec.thought) && !rec.thought.trim().is_empty();

            let is_claude_target = target_model
                .map(|m| m.to_lowercase().contains("claude"))
                .unwrap_or_else(|| store_key.to_lowercase().contains("claude"));

            if has_meaningful_thought {
                let mut thought_part = json!({
                    "text": rec.thought.as_str(),
                    "thought": true,
                });

                // [DE-DUPLICATION & ELEVATION] 正文残留思考文本切除与去重：
                // 解决关思考时降级到正文的思考文本，在重新开思考时被再次提升复活后，导致正文残留双份复读的问题！
                let rec_thought_trimmed = rec.thought.trim();
                let rec_thought_norm = normalize_ws(&rec.thought);

                let mut cleaned_parts = Vec::with_capacity(parts.len());
                for part in parts.drain(..) {
                    let is_plain_text = part.get("text").is_some()
                        && part.get("functionCall").is_none()
                        && part.get("functionResponse").is_none()
                        && part.get("thought").and_then(|v| v.as_bool()) != Some(true);

                    if is_plain_text {
                        let text = part["text"].as_str().unwrap_or("");
                        let text_trimmed = text.trim();

                        // 0. 若正文带有 <think>...</think> 标签，精准切除标签与思考内容，保留剩余真实正文
                        if let Some((_, rem)) = extract_think_tags(text) {
                            if !rem.is_empty() {
                                cleaned_parts.push(json!({ "text": rem }));
                            }
                            continue;
                        }

                        // 1. 完全相同（之前作为独立降级部件存在）：直接丢弃该部件
                        if text_trimmed == rec_thought_trimmed
                            || (!rec_thought_norm.is_empty()
                                && normalize_ws(text) == rec_thought_norm)
                        {
                            continue;
                        }
                        // 2. 正文以思考文本开头（思考文本与正文被客户端合并为一个部件）：切除前缀
                        if text.starts_with(&rec.thought) {
                            let remainder = &text[rec.thought.len()..];
                            let trimmed_rem =
                                remainder.trim_start_matches(|c| c == '\r' || c == '\n');
                            if !trimmed_rem.is_empty() {
                                cleaned_parts.push(json!({ "text": trimmed_rem }));
                            }
                            continue;
                        } else if text_trimmed.starts_with(rec_thought_trimmed) {
                            let remainder = &text_trimmed[rec_thought_trimmed.len()..];
                            let trimmed_rem =
                                remainder.trim_start_matches(|c| c == '\r' || c == '\n');
                            if !trimmed_rem.is_empty() {
                                cleaned_parts.push(json!({ "text": trimmed_rem }));
                            }
                            continue;
                        }
                    }
                    cleaned_parts.push(part);
                }
                *parts = cleaned_parts;

                if is_claude_target {
                    // Claude：签名放在第一个非思考 part，不挂到思考块。
                    // 库存里认得出的 Claude 签名才回填；Gemini 签名不注入。
                    let fallback = rec
                        .signature
                        .as_deref()
                        .filter(|s| is_real_signature(s) && is_claude_signature(s));
                    let wrapped = fallback.map(|s| ensure_google_claude_thought_signature(s));
                    place_turn_signature_scoped(
                        &mut *parts,
                        wrapped.as_deref(),
                        false,
                        Some(store_key),
                    );
                } else {
                    // Gemini 原生：把本轮捕获到的真实签名归位到「该轮第一个非思考 part」。
                    //
                    // 首位思考块保持纯净思考文本（铁律 I4：思考块绝不携带签名），随后由
                    // `parts.insert(0, thought_part)` 插入 index 0 —— 锚点自然落到 index 1，
                    // 正好复现官方的「思考块 + 带签名正文」排列。
                    let fallback = rec
                        .signature
                        .as_deref()
                        .filter(|s| is_real_signature(s) && is_likely_gemini_signature(s));
                    place_turn_signature_scoped(&mut *parts, fallback, true, Some(store_key));
                }
                parts.insert(0, thought_part);
                restored += 1;
            } else {
                // 【2026-09-27】无实质思考内容（占位/空思考）：绝不注入 "..." 占位思考块！
                // 官方标准形态为「无思考块 + 锚点带签名」（baogao.txt 9/24 轮）。
                // 首个非思考 part 作为锚点正常承接该轮签名。
                if is_claude_target {
                    let fallback = rec
                        .signature
                        .as_deref()
                        .filter(|s| is_real_signature(s) && is_claude_signature(s));
                    let wrapped = fallback.map(|s| ensure_google_claude_thought_signature(s));
                    place_turn_signature_scoped(
                        &mut *parts,
                        wrapped.as_deref(),
                        false,
                        Some(store_key),
                    );
                } else {
                    let fallback = rec
                        .signature
                        .as_deref()
                        .filter(|s| is_real_signature(s) && is_likely_gemini_signature(s));
                    if place_turn_signature_scoped(&mut *parts, fallback, true, Some(store_key))
                        .is_some()
                    {
                        restored += 1;
                    }
                }
            }
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

    /// 精准定向净化指定会话中的异构污染签名（保留思考文本与健康签名）
    pub fn purge_corrupted_signatures(&self, store_key: &str, target_model: &str) -> usize {
        if store_key.is_empty() {
            return 0;
        }
        let is_gemini = target_model.to_lowercase().contains("gemini");
        let is_claude = target_model.to_lowercase().contains("claude");
        if !is_gemini && !is_claude {
            return 0;
        }

        let mut purged_count = 0;

        // 1. 精准净化内存缓存 (RAM)
        if let Some(mut entry) = self.sessions.get_mut(store_key) {
            let mut new_turns = Vec::with_capacity(entry.turns.len());
            for rec in &entry.turns {
                if let Some(ref sig) = rec.signature {
                    let is_foreign = if is_gemini {
                        !is_likely_gemini_signature(sig)
                    } else if is_claude {
                        !is_claude_signature(sig)
                    } else {
                        false
                    };
                    if is_foreign {
                        purged_count += 1;
                        let mut cleaned = (**rec).clone();
                        cleaned.signature = None;
                        new_turns.push(Arc::new(cleaned));
                        continue;
                    }
                }
                new_turns.push(rec.clone());
            }
            entry.turns = new_turns;
        }

        // 2. 精准净化持久化数据库 (SQLite)
        let _ = crate::modules::proxy_db::purge_foreign_signatures_for_session_with_model(
            store_key,
            target_model,
        );

        if purged_count > 0 {
            tracing::warn!(
                "[ThinkingStore] Surgically purged {} foreign signature(s) for session {} targeting {}",
                purged_count, store_key, target_model
            );
        }
        purged_count
    }

    /// Drop thinking records that no longer appear in the (possibly compressed) history.
    /// Always keeps the newest 2 turns so the latest unused response thinking is not lost.
    pub fn prune_orphaned_records(&self, store_key: &str, contents: &[Value]) {
        if !crate::proxy::config::is_thinking_store_enabled() || store_key.is_empty() {
            return;
        }

        let mem_turns = self
            .sessions
            .get(store_key)
            .map(|e| e.turns.len())
            .unwrap_or(0);
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
        let mut live_sigs = std::collections::HashSet::new();
        let mut live_visibles: Vec<String> = Vec::new();

        for (c_idx, content) in contents.iter().enumerate() {
            if !is_model_or_assistant(content) {
                continue;
            }
            let Some(parts) = content.get("parts").and_then(|p| p.as_array()) else {
                continue;
            };
            let preceding_turn = if c_idx > 0 {
                contents.get(c_idx - 1)
            } else {
                None
            };
            let anchor = compute_causal_anchor(preceding_turn);
            let (visible, tool_ids, tool_names, _) = inspect_parts_with_anchor(parts, &anchor);
            live_fps.insert(fingerprint(&visible, &tool_ids, &tool_names));
            for id in tool_ids {
                live_tool_ids.insert(id);
            }
            for part in parts {
                if let Some(sig) = part
                    .get("thoughtSignature")
                    .or_else(|| part.get("thought_signature"))
                    .and_then(|s| s.as_str())
                    .filter(|s| is_real_signature(s))
                {
                    live_sigs.insert(sig.to_string());
                }
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
                let matched_sig = rec
                    .signature
                    .as_deref()
                    .is_some_and(|s| live_sigs.contains(s));
                let matched_tool = rec.tool_ids.iter().any(|id| live_tool_ids.contains(id));
                let matched_fp = live_fps.contains(&rec.fingerprint);
                let norm_rec = &rec_norms[i];
                let matched_text = !norm_rec.is_empty()
                    && live_visibles.iter().any(|v| {
                        v == norm_rec || v.starts_with(norm_rec) || norm_rec.starts_with(v)
                    });
                if matched_sig || matched_tool || matched_fp || matched_text || i >= keep_tail_start
                {
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
            store_key, &keep_fps,
        );
    }

    pub fn session_stats(&self, store_key: &str) -> Option<(usize, usize)> {
        self.sessions
            .get(store_key)
            .map(|e| (e.turns.len(), e.bytes))
    }

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
    /// 带内容锚点的会话号。思维库、签名缓存、上游 `sessionId` 用它。
    pub client_id: String,
    /// `{tenant}:{client_id}`。思维库、签名缓存、上游 `sessionId` 的存储键。
    pub store_key: String,
    /// 不含内容锚点的稳定会话号。没有稳定会话身份时与 `client_id` 相同。
    pub affinity_id: String,
    /// `{tenant}:{affinity_id}`。只决定用哪个 Google 账号。
    pub affinity_key: String,
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
        Self::resolve(headers, body, query, fallback, None)
    }

    /// `tenant_id` 来自中间件已经解析出的用户 Token。没有时再哈希请求头里的凭证。
    pub fn resolve(
        headers: &HeaderMap,
        body: Option<&Value>,
        query: Option<&str>,
        fallback: impl Into<String>,
        tenant_id: Option<&str>,
    ) -> Self {
        let fallback = fallback.into();
        let tenant = tenant_component(headers, tenant_id);
        let winner = stable_session_winner(headers, body, query);
        let affinity_anchor = if winner.is_some() {
            ""
        } else {
            fallback.as_str()
        };
        let affinity_id = derive_winner_session_id(&tenant, winner.as_deref(), affinity_anchor);
        let client_id = derive_winner_session_id(&tenant, winner.as_deref(), &fallback);
        let store_key = format!("{}:{}", tenant, client_id);
        let affinity_key = format!("{}:{}", tenant, affinity_id);
        Self {
            client_id,
            store_key,
            affinity_id,
            affinity_key,
        }
    }

    pub fn purge_signatures(&self, model: &str) {
        ThinkingStore::global().purge_corrupted_signatures(&self.store_key, model);
        crate::proxy::SignatureCache::global().delete_session_signature(&self.store_key);
    }
}

#[derive(Debug, Default, Clone)]
pub struct TurnAccumulator {
    thought: String,
    signature: Option<String>,
    visible: String,
    tool_ids: Vec<String>,
    tool_names: Vec<String>,
    context_anchor: String,
    function_call_count: usize,
}

impl TurnAccumulator {
    pub fn new() -> Self {
        Self::with_anchor("root")
    }

    pub fn with_anchor(anchor: impl Into<String>) -> Self {
        Self {
            thought: String::new(),
            signature: None,
            visible: String::new(),
            tool_ids: Vec::new(),
            tool_names: Vec::new(),
            context_anchor: anchor.into(),
            function_call_count: 0,
        }
    }

    pub fn ingest_part(&mut self, part: &Value) {
        let is_thought = part
            .get("thought")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
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
            let explicit_id = fc
                .get("id")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(|s| crate::proxy::common::utils::normalize_tool_id(s).into_owned());

            let synthetic = synthesize_tool_id(
                &name,
                fc.get("args"),
                &self.context_anchor,
                self.function_call_count,
            );
            self.function_call_count += 1;

            // 核心演进：全面转战因果伪哈希 ID！首位强制存入确定性 synthetic ID，客户端 real_id 紧随其后作为元数据兜底
            if !self.tool_ids.iter().any(|x| x == &synthetic) {
                self.tool_ids.push(synthetic.clone());
            }

            if let Some(ref real_id) = explicit_id {
                if !self.tool_ids.iter().any(|x| x == real_id) {
                    self.tool_ids.push(real_id.clone());
                }
            }

            if !self.tool_names.iter().any(|x| x == &name) {
                self.tool_names.push(name);
            }
        }
    }

    pub fn record_tool_id(&mut self, tool_name: &str, real_id: &str) {
        let norm_id = crate::proxy::common::utils::normalize_tool_id(real_id);
        let id_str = norm_id.to_string();
        if !self.tool_ids.iter().any(|x| x == &id_str) {
            self.tool_ids.push(id_str);
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
    for (c_idx, content) in contents.iter().enumerate() {
        let role = content.get("role").and_then(|v| v.as_str()).unwrap_or("");
        if role != "model" && role != "assistant" {
            continue;
        }
        if let Some(parts) = content.get("parts").and_then(|p| p.as_array()) {
            let preceding_turn = if c_idx > 0 {
                contents.get(c_idx - 1)
            } else {
                None
            };
            let anchor = compute_causal_anchor(preceding_turn);
            capture_gemini_parts_with_anchor(store_key, parts, &anchor);
        }
    }
}

/// Capture client-supplied thinking, restore missing blocks, then prune compressed-away history.
///
/// Client histories usually have no real thinking (Claude/OpenAI). After a session is
/// warm in memory, this path is RAM-only: no SQLite open, no placeholder ingest, no prune
/// rewrite. JSON fill still copies stored thought text into the freshly built request.
pub fn hydrate_gemini_contents(store_key: &str, contents: &mut Vec<Value>) -> usize {
    hydrate_gemini_contents_with_model(store_key, contents, None)
}

pub fn hydrate_gemini_contents_with_model(
    store_key: &str,
    contents: &mut Vec<Value>,
    target_model: Option<&str>,
) -> usize {
    if store_key.is_empty() {
        return 0;
    }
    let store = ThinkingStore::global();
    store.touch_session(store_key);
    // 优先执行拓扑还原，保证历史已有记录对齐为真实真签名
    let restored = store.restore_gemini_contents_with_model(store_key, contents, target_model);
    // 只有在完成还原后，若仍有客户端自带的合法实质思考块，才安全吸纳进库
    if contents_have_capturable_thought(contents) {
        store.ingest_from_contents(store_key, contents);
    }
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
            let is_thought = part
                .get("thought")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
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
    capture_gemini_parts_with_anchor(store_key, parts, "root");
}

pub fn capture_gemini_parts_with_anchor(store_key: &str, parts: &[Value], anchor: &str) {
    let mut acc = TurnAccumulator::with_anchor(anchor);
    for part in parts {
        acc.ingest_part(part);
    }
    acc.commit(store_key);
}

pub fn capture_gemini_response(store_key: &str, response: &Value) {
    capture_gemini_response_with_preceding(store_key, response, None);
}

pub fn capture_gemini_response_with_preceding(
    store_key: &str,
    response: &Value,
    preceding: Option<&Value>,
) {
    let raw = response.get("response").unwrap_or(response);
    if let Some(parts) = raw
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.as_array())
    {
        let anchor = compute_causal_anchor(preceding);
        capture_gemini_parts_with_anchor(store_key, parts, &anchor);
    }
}

/// 四大协议统一思考补齐管线：确保所有 Gemini contents 中的 model 轮次在开启思考时，必须具备合法的思考块与签名
pub fn finalize_gemini_contents_thinking(contents: &mut [Value], is_thinking_enabled: bool) {
    finalize_gemini_contents_thinking_with_model(contents, is_thinking_enabled, None);
}

pub fn finalize_gemini_contents_thinking_with_model(
    contents: &mut [Value],
    is_thinking_enabled: bool,
    target_model: Option<&str>,
) {
    finalize_gemini_contents_thinking_with_session(
        contents,
        is_thinking_enabled,
        target_model,
        None,
    );
}

pub fn finalize_gemini_contents_thinking_with_session(
    contents: &mut [Value],
    is_thinking_enabled: bool,
    target_model: Option<&str>,
    session_id: Option<&str>,
) {
    // 预先计算每一轮的前置因果锚点 (causal anchor)，以便无 ID 的 Gemini 原生工具调用也能合成确定性 ID
    let anchors: Vec<String> = (0..contents.len())
        .map(|i| {
            let preceding = if i > 0 { contents.get(i - 1) } else { None };
            compute_causal_anchor(preceding)
        })
        .collect();

    // 黄金法则 2.2：纯思考轮签名暂存游标
    // 当某一轮只有纯思考块（无正文、无工具调用）时，Gemini 思考块绝不能带签名，
    // 将该轮获取到的有效签名攒在手上 (pending_thought_sig) 预留给下一轮；
    // 若下一轮有新签名返回，则丢弃旧签名采纳新签名；若下一轮无签名，则由攒着的签名补充。
    let mut pending_thought_sig: Option<String> = None;

    let initial_session_sig: Option<String> = session_id.and_then(|sid| {
        crate::proxy::SignatureCache::global()
            .get_session_signature(sid)
            .filter(|s| is_likely_gemini_signature(s))
    });

    // 预先计算最后一个 model / 工具调用轮次的索引
    let last_model_idx = contents.iter().enumerate().rposition(|(_, m)| {
        matches!(
            m.get("role").and_then(|r| r.as_str()),
            Some("model") | Some("assistant")
        ) || m
            .get("parts")
            .and_then(|p| p.as_array())
            .map_or(false, |parts| {
                parts.iter().any(|p| p.get("functionCall").is_some())
            })
    });

    for (msg_idx, msg) in contents.iter_mut().enumerate() {
        let _anchor = &anchors[msg_idx];
        let has_fc = msg
            .get("parts")
            .and_then(|p| p.as_array())
            .map_or(false, |parts| {
                parts.iter().any(|p| p.get("functionCall").is_some())
            });
        if has_fc && msg.get("role").and_then(|r| r.as_str()) == Some("user") {
            msg["role"] = json!("model");
        }
        let is_model = matches!(
            msg.get("role").and_then(|r| r.as_str()),
            Some("model") | Some("assistant")
        ) || has_fc;

        if !is_model {
            // 非 model 轮次（如 user 轮次的 functionResponse）：清洗可能混入的误标签名
            if let Some(parts) = msg.get_mut("parts").and_then(|p| p.as_array_mut()) {
                for part in parts.iter_mut() {
                    if let Some(obj) = part.as_object_mut() {
                        obj.remove("thought_signature");
                        if obj.contains_key("functionResponse") {
                            obj.remove("thoughtSignature");
                        }
                    }
                }
            }
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
                // 铁律：只认 thought: true。functionCall 与纯正文都会携带 thoughtSignature，
                // 绝不能仅凭签名判定为思考块，否则会漏补首位 thought、关思考时误删工具。
                let is_thought = is_thought_part(&part);
                if is_thought {
                    thinking_parts.push(part);
                } else {
                    other_parts.push(part);
                }
            }

            let is_claude_turn = target_model
                .map(|m| m.to_lowercase().contains("claude"))
                .unwrap_or(false);

            // 1. 提取当前轮次已有合法的真实签名 (纯检查当前轮部件自带签名，必须严格校验合法性！)
            // [Pipeline First] 若存在 functionCall 强制优先从 functionCall 提取合法签名，
            // 避免前面紧随的 text 说明正文误抢签名导致真实工具调用凭据丢失或绕过 tool_signatures 穿透查库。
            let turn_real_sig =
                extract_turn_signature(&other_parts, !is_claude_turn, is_claude_turn);

            // 1.1 Gemini 目标下工具调用签名多级检索与穿透回填：
            // ① 原生/传入 tool_id -> 内存 L1 + SQLite L2 (tool_signatures)
            // ② 若未命中（非原生 ID 如 Write-76 或缓存失配），计算 ID 无关的确定性因果伪哈希 ID (synthetic_id) 穿透匹配 SQLite
            // ③ 若找到则回填并反向自愈写入缓存，回填不了则由后序 chosen_sig 统一带上官方哨兵 (SENTINEL_SIGNATURE)
            let cached_tool_sig: Option<String> =
                if !is_claude_turn && turn_real_sig.is_none() {
                    let mut found_sig = None;
                    let mut fc_counter = 0usize;
                    for p in &other_parts {
                        if let Some(fc) = p.get("functionCall") {
                            let id = fc.get("id").and_then(|v| v.as_str()).unwrap_or("");
                            // 1. 先用原生 ID 查内存 L1 与 SQLite L2 (tool_signatures 表)
                            if !id.is_empty() {
                                if let Some(sid) = session_id {
                                    if let Some(sig) = crate::proxy::SignatureCache::global()
                                        .get_tool_signature(sid, id)
                                        .filter(|s| is_likely_gemini_signature(s))
                                    {
                                        found_sig = Some(sig);
                                        break;
                                    }
                                }
                            }

                            // 2. 若未命中（非原生 ID 如 Write-76，或缓存失配）：
                            // 使用 ID 无关的确定性因果伪哈希 ID 穿透匹配 SQLite
                            let name = fc.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
                            let synthetic_id =
                                synthesize_tool_id(name, fc.get("args"), _anchor, fc_counter);
                            fc_counter += 1;

                            // 2.1 先查内存 L1 与 SQLite L2 (tool_signatures 表)
                            if let Some(sid) = session_id {
                                if let Some(sig) = crate::proxy::SignatureCache::global()
                                    .get_tool_signature(sid, &synthetic_id)
                                    .filter(|s| is_likely_gemini_signature(s))
                                {
                                    if !id.is_empty() {
                                        crate::proxy::SignatureCache::global()
                                            .cache_tool_signature(sid, id, sig.clone());
                                    }
                                    found_sig = Some(sig);
                                    break;
                                }
                            }

                            // 2.2 若仍未命中，尝试按 session_key 在 SQLite thinking_records 极速穿透点查 (causal_tool_id)
                            if let Some(sid) = session_id {
                                if let Ok(Some(rec)) =
                                    crate::modules::proxy_db::load_thinking_by_tool_id(
                                        sid,
                                        &synthetic_id,
                                    )
                                {
                                    if let Some(sig) =
                                        rec.signature.filter(|s| is_likely_gemini_signature(s))
                                    {
                                        if !id.is_empty() {
                                            if let Some(sid) = session_id {
                                                crate::proxy::SignatureCache::global()
                                                    .cache_tool_signature(sid, id, sig.clone());
                                            }
                                        }
                                        found_sig = Some(sig);
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    found_sig
                } else {
                    None
                };

            let is_last_model_turn = last_model_idx == Some(msg_idx);
            let cached_session_sig: Option<String> = if !is_claude_turn
                && is_last_model_turn
                && turn_real_sig.is_none()
                && cached_tool_sig.is_none()
            {
                initial_session_sig.clone()
            } else {
                None
            };

            // 2. 签名归位（终审出站门禁 Gatekeeper）：
            // 签名只写在该轮第一个非思考 part 上。Gemini 与 Claude 桌面端都是这个落点。
            if other_parts.is_empty() {
                // 黄金法则 2.2：纯思考轮（无正文、无工具调用）
                // 思考块绝不挂载签名。若本轮持有有效签名，先暂存在手上 (pending_thought_sig) 预留给后续轮次
                let turn_sig = thinking_parts.iter().find_map(|tp| {
                    tp.get("thoughtSignature")
                        .or_else(|| tp.get("thought_signature"))
                        .and_then(|s| s.as_str())
                        .filter(|s| {
                            is_real_signature(s)
                                && (is_likely_gemini_signature(s) || is_claude_signature(s))
                        })
                        .map(str::to_string)
                });
                for tp in thinking_parts.iter_mut() {
                    if let Some(obj) = tp.as_object_mut() {
                        obj.remove("thoughtSignature");
                        obj.remove("thought_signature");
                    }
                }
                if let Some(sig) = turn_sig {
                    pending_thought_sig = Some(sig);
                }
            } else {
                // 第一种情况：经过各种手段（客户端校验采纳 / 网关缓存或库按 tool_id / 会话级最新签名 / 上一轮纯思考暂存），本轮手上有签名：
                // 优先级：
                // ① 客户端自带且严格校验合法的签名 turn_real_sig（有效则采纳并反向入库）
                // ② 签名缓存与 SQLite 库按 tool_id / 伪哈希 ID 精确回填 cached_tool_sig
                // ③ 最新轮次按会话 ID 取回上一轮流式产出的最新签名 cached_session_sig
                // ④ 上一轮纯思考暂存预留的签名 pending_thought_sig
                // ⑤ 兜底保底：若当前包含工具调用且为 Gemini 目标，回填不了则带官方哨兵 (SENTINEL_SIGNATURE) 防 400
                let (chosen_sig, is_fresh) = if let Some(ref sig) = turn_real_sig {
                    (Some(sig.clone()), true)
                } else if let Some(ref sig) = cached_tool_sig {
                    (Some(sig.clone()), true)
                } else if let Some(ref sig) = cached_session_sig {
                    (Some(sig.clone()), true)
                } else if let Some(sig) = pending_thought_sig.take() {
                    (Some(sig), false)
                } else if !is_claude_turn
                    && other_parts.iter().any(|p| p.get("functionCall").is_some())
                {
                    (Some(SENTINEL_SIGNATURE.to_string()), true)
                } else {
                    (None, false)
                };

                if is_fresh {
                    // 若本轮已有独立的新签名，直接丢弃之前纯思考轮暂存的陈旧签名
                    pending_thought_sig = None;
                }

                // 黄金法则 3：
                let placed = place_turn_signature_scoped(
                    &mut other_parts,
                    chosen_sig.as_deref(),
                    !is_claude_turn,
                    session_id,
                );
                if let Some(final_sig) = placed {
                    if final_sig != SENTINEL_SIGNATURE && is_likely_gemini_signature(&final_sig) {
                        if let Some(sid) = session_id {
                            if !crate::proxy::SignatureCache::global()
                                .has_session_signature_after(sid, msg_idx)
                            {
                                crate::proxy::SignatureCache::global()
                                    .cache_session_signature(sid, final_sig, msg_idx);
                            }
                        }
                    }
                }
            }

            // 3. 治理思考块 (thinking_parts)
            // 思考块不挂签名。Claude 保留思考正文（Mac IDE 与 Windows 桌面端都回传）。
            // Gemini 在已有非思考 part 时丢掉思考正文，连续只靠签名。
            if is_thinking_enabled {
                for tp in thinking_parts.iter_mut() {
                    if let Some(obj) = tp.as_object_mut() {
                        obj.remove("thoughtSignature");
                        obj.remove("thought_signature");
                    }
                }
                if is_claude_turn || other_parts.is_empty() {
                    parts.extend(thinking_parts);
                }
            } else {
                // 当思考模式为关时：
                // 1. 绝不主动注入任何占位思考块（如 "..."）；
                // 2. 若含有实质性思考内容的思考块，单次出站降级为普通文本以防丢失语义，摘除 thought: true 标记；
                // 3. 纯占位符则直接剔除，绝不上送 thought: true 结构
                for tp in thinking_parts {
                    let text = tp.get("text").and_then(|t| t.as_str()).unwrap_or("");
                    if is_meaningful_thought(text) {
                        let wrapped = if is_claude_turn {
                            if text.trim_start().starts_with("<think>") {
                                text.to_string()
                            } else {
                                format!("<think>\n{}\n</think>\n\n", text.trim())
                            }
                        } else {
                            text.to_string()
                        };
                        parts.push(json!({ "text": wrapped }));
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

/// 全生态显式会话标识。只返回一个赢家，后面的来源全部丢掉。
pub fn explicit_session_id_with_query(
    headers: &HeaderMap,
    body: Option<&Value>,
    query: Option<&str>,
) -> Option<String> {
    stable_session_winner(headers, body, query)
}

/// 稳定会话号的唯一优先级：
/// 产品会话头（`x-claude-code-session-id` 最先）→ 其它通配会话头 →
/// 通用 `x-session-id` → 别名头 → query → body 稳定字段。
/// `previous_response_id` 是每轮游标，不在名单里。
pub fn stable_session_winner(
    headers: &HeaderMap,
    body: Option<&Value>,
    query: Option<&str>,
) -> Option<String> {
    if let Some(sid) = session_id_from_headers(headers) {
        return Some(sid);
    }
    if let Some(sid) = extract_query_session_id(headers, query) {
        return Some(sid);
    }
    extract_body_session_id(body)
}

pub fn explicit_session_id(headers: &HeaderMap, body: Option<&Value>) -> Option<String> {
    explicit_session_id_with_query(headers, body, None)
}

/// Product-specific `x-**-session-id` / `x-**-sessionid`. Checked before generic `x-session-id`.
const PRODUCT_SESSION_HEADERS: &[&str] = &[
    "x-claude-code-session-id",
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
    headers
        .get(name)
        .and_then(|h| h.to_str().ok())
        .and_then(|v| {
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
    // 1. Product-specific x-**-session-id / x-**-sessionid，名单顺序即优先级。
    for name in PRODUCT_SESSION_HEADERS {
        if let Some(sid) = header_session_value(headers, name) {
            return Some(sid);
        }
    }
    let mut wildcards = std::collections::BTreeMap::new();
    for (name, value) in headers.iter() {
        let key = name.as_str();
        if is_generic_x_session_id(key)
            || PRODUCT_SESSION_HEADERS
                .iter()
                .any(|listed| key.eq_ignore_ascii_case(listed))
        {
            continue;
        }
        if !is_wildcard_session_header(key) {
            continue;
        }
        if let Ok(v) = value.to_str() {
            let v = v.trim();
            if !v.is_empty() {
                let sanitized = sanitize_session_id(v);
                if sanitized != "sid-unknown" {
                    wildcards.insert(key.to_ascii_lowercase(), sanitized);
                }
            }
        }
    }
    if let Some((_, sid)) = wildcards.iter().next() {
        return Some(sid.clone());
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

fn hash16(raw: &str) -> String {
    let hash = format!("{:x}", Sha256::digest(raw.as_bytes()));
    hash[..16].to_string()
}

fn tenant_component(headers: &HeaderMap, tenant_id: Option<&str>) -> String {
    if let Some(id) = tenant_id.map(str::trim).filter(|id| !id.is_empty()) {
        return hash16(id);
    }
    tenant_from_headers(headers)
}

fn tenant_from_headers(headers: &HeaderMap) -> String {
    let raw = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer ").or(Some(s)))
        .or_else(|| headers.get("x-api-key").and_then(|h| h.to_str().ok()))
        .or_else(|| headers.get("x-goog-api-key").and_then(|h| h.to_str().ok()))
        .unwrap_or("anon");
    hash16(raw)
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

/// 从 URL Query、代理跳转 Header (x-forwarded-uri, x-original-uri) 以及 Referer 中提取会话参数
pub fn extract_query_session_id(headers: &HeaderMap, query: Option<&str>) -> Option<String> {
    if let Some(q) = query {
        if let Some(sid) = extract_session_from_query_str(q) {
            return Some(sid);
        }
    }
    for uri_h in ["x-forwarded-uri", "x-original-uri"] {
        if let Some(raw_uri) = headers.get(uri_h).and_then(|h| h.to_str().ok()) {
            if let Some(pos) = raw_uri.find('?') {
                if let Some(sid) = extract_session_from_query_str(&raw_uri[pos + 1..]) {
                    return Some(sid);
                }
            }
        }
    }
    if let Some(referer) = headers.get("referer").and_then(|h| h.to_str().ok()) {
        if let Some(pos) = referer.find('?') {
            if let Some(sid) = extract_session_from_query_str(&referer[pos + 1..]) {
                return Some(sid);
            }
        }
    }
    None
}

/// 从 JSON Body 及 metadata 中提取显式指定的会话字段
pub fn extract_body_session_id(body: Option<&Value>) -> Option<String> {
    let body = body?;
    for field in [
        "session_id",
        "sessionId",
        "conversation_id",
        "chat_id",
        "thread_id",
        "client_session_id",
        "_session_thinking_id",
    ] {
        if let Some(v) = body.get(field).and_then(|v| v.as_str()) {
            let v = v.trim();
            if !v.is_empty() {
                let sanitized = sanitize_session_id(v);
                if !sanitized.is_empty() && sanitized != "sid-unknown" {
                    return Some(sanitized);
                }
            }
        }
    }
    if let Some(req) = body.get("request") {
        for field in ["sessionId", "session_id"] {
            if let Some(v) = req.get(field).and_then(|v| v.as_str()) {
                let v = v.trim();
                if !v.is_empty() {
                    let sanitized = sanitize_session_id(v);
                    if !sanitized.is_empty() && sanitized != "sid-unknown" {
                        return Some(sanitized);
                    }
                }
            }
        }
    }
    if let Some(metadata) = body.get("metadata") {
        for field in [
            "conversation_id",
            "chat_id",
            "session_id",
            "sessionId",
            "thread_id",
        ] {
            if let Some(v) = metadata.get(field).and_then(|v| v.as_str()) {
                let v = v.trim();
                if !v.is_empty() && !v.contains("session-") {
                    let sanitized = sanitize_session_id(v);
                    if !sanitized.is_empty() && sanitized != "sid-unknown" {
                        return Some(sanitized);
                    }
                }
            }
        }
    }
    None
}

/// 只哈希一个稳定会话号。有赢家时账号键省略内容锚点，思维库键保留锚点。
pub fn derive_winner_session_id(tenant: &str, stable_id: Option<&str>, anchor: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"v3|");
    hasher.update(tenant.as_bytes());
    hasher.update([0xff]);
    if let Some(id) = stable_id.map(str::trim).filter(|id| !id.is_empty()) {
        hasher.update(b"sid=");
        hasher.update(id.as_bytes());
        hasher.update([0xfe]);
    }
    let anchor = anchor.trim();
    if !anchor.is_empty() {
        hasher.update(b"anchor=");
        hasher.update(anchor.as_bytes());
    }
    let hash = format!("{:x}", hasher.finalize());
    format!("sess-{}", &hash[..16])
}

fn client_id_from_store_key(store_key: &str) -> &str {
    store_key
        .split_once(':')
        .map(|(_, rest)| rest)
        .unwrap_or(store_key)
}

pub fn is_real_signature(sig: &str) -> bool {
    let s = sig.trim();
    if s == SENTINEL_SIGNATURE {
        return false;
    }
    if s.as_bytes().first() == Some(&0x12) && s.len() >= 16 {
        return true;
    }
    s.len() >= MIN_SIGNATURE_LENGTH
}

/// 判断签名是否符合 Google Gemini 原生 Protobuf 签名特征：
/// 1. 官方跳过验签哨兵 (skip_thought_signature_validator)；
/// 2. 或满足有效长度 (>= MIN_SIGNATURE_LENGTH)，且 Base64 解码后首字节为 Protobuf Tag 2 (0x12)
///    (单层 Base64 通常以 'E' 开头，双层 Base64 包装通常以 'R' 开头)
pub fn is_likely_gemini_signature(sig: &str) -> bool {
    let s = sig.trim();
    if s == SENTINEL_SIGNATURE {
        return true;
    }
    // Claude 签名绝不能被误判为 Gemini 签名
    if is_claude_signature(s) {
        return false;
    }
    // 兼容历史脏数据中被误解码为原始二进制 Protobuf (首字节 0x12) 的签名
    if s.as_bytes().first() == Some(&0x12) && s.len() >= 16 {
        return true;
    }
    if s.len() < MIN_SIGNATURE_LENGTH {
        return false;
    }
    #[cfg(test)]
    if s.starts_with("sig_")
        || s.starts_with("test-")
        || s.starts_with("client-")
        || s.chars().all(|c| c == 's' || c == 't')
    {
        return true;
    }
    if !s.starts_with('E') && !s.starts_with('R') {
        return false;
    }
    use base64::Engine;
    if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(s) {
        if decoded.first() == Some(&0x12) {
            return true;
        }
        // 双层 Base64 包装支持（Google Vertex AI 格式）
        if let Ok(inner_str) = std::str::from_utf8(&decoded) {
            if inner_str.starts_with('E') {
                if let Ok(inner) = base64::engine::general_purpose::STANDARD.decode(inner_str) {
                    if inner.first() == Some(&0x12) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// 判断签名是否属于 Claude 家族的签名
pub fn is_claude_signature(sig: &str) -> bool {
    let s = sig.trim();
    if s.is_empty() || s == SENTINEL_SIGNATURE {
        return false;
    }
    use base64::Engine;
    if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(s) {
        if decoded
            .windows(6)
            .any(|w| w.eq_ignore_ascii_case(b"claude"))
        {
            return true;
        }
        if let Ok(inner) = base64::engine::general_purpose::STANDARD.decode(&decoded) {
            if inner.windows(6).any(|w| w.eq_ignore_ascii_case(b"claude")) {
                return true;
            }
        }
    }
    false
}

/// 将 Claude 签名正规化为发送给 Google Vertex AI 接口所需的格式
/// Google 的 REST API 对 bytes 字段会自动执行 base64_decode，
/// 因此发往 Google 的 thoughtSignature 必须是 ASCII 签名字节的 Base64 编码 (即 "RXU4..." 格式)
pub fn ensure_google_claude_thought_signature(sig: &str) -> String {
    let s = sig.trim();
    if s.is_empty() || s == SENTINEL_SIGNATURE {
        return s.to_string();
    }
    use base64::Engine;
    // 如果已经由 Base64 包装过（即 base64 decode 出来能再解出 b"claude"），无需重复包装
    if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(s) {
        if let Ok(inner) = base64::engine::general_purpose::STANDARD.decode(&decoded) {
            if inner.windows(6).any(|w| w.eq_ignore_ascii_case(b"claude")) {
                return s.to_string();
            }
        }
    }
    // 只有在当前签名确实是原始 Claude 客户端签名（解码一层后包含 b"claude"）时才进行一次 Base64 包装！
    // 严禁对非 Claude 签名或未知字符串无节制再包装，彻底阻断几何级膨胀死循环。
    if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(s) {
        if decoded
            .windows(6)
            .any(|w| w.eq_ignore_ascii_case(b"claude"))
        {
            return base64::engine::general_purpose::STANDARD.encode(s.as_bytes());
        }
    }
    s.to_string()
}

/// 将 Claude 签名还原为客户端（Claude Code / Anthropic SDK）期望的原生格式 (Eu8...)
pub fn ensure_raw_claude_thought_signature(sig: &str) -> String {
    let s = sig.trim();
    if s.is_empty() || s == SENTINEL_SIGNATURE {
        return s.to_string();
    }
    use base64::Engine;
    if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(s) {
        if let Ok(inner) = base64::engine::general_purpose::STANDARD.decode(&decoded) {
            if inner.windows(6).any(|w| w.eq_ignore_ascii_case(b"claude")) {
                if let Ok(raw_s) = String::from_utf8(decoded) {
                    return raw_s;
                }
            }
        }
    }
    s.to_string()
}

/// 用于 ThinkingStore / SignatureCache 内部的比对与哈希：
/// 统一归一化为原始客户端签名形式 (Eu8...)，使 "RXU4..." 与 "Eu8..." 判定为相同签名
pub fn normalize_signature_for_comparison(sig: &str) -> std::borrow::Cow<'_, str> {
    use base64::Engine;
    if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(sig) {
        if let Ok(inner) = base64::engine::general_purpose::STANDARD.decode(&decoded) {
            if inner.windows(6).any(|w| w == b"claude") {
                if let Ok(s) = String::from_utf8(decoded) {
                    return std::borrow::Cow::Owned(s);
                }
            }
        }
    }
    std::borrow::Cow::Borrowed(sig)
}

pub fn signatures_match(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    normalize_signature_for_comparison(a) == normalize_signature_for_comparison(b)
}

/// 判定某个 Part 是否为思考块。
///
/// **铁律：只认 `thought == true`。**
///
/// 绝不能退化为"带 `thoughtSignature` 且无 `functionCall`/`functionResponse`"——
/// 真实 Antigravity 报文会把签名挂在**纯正文 part** 上（官方不变量见
/// `.workbuddy/outputs/correct-assembly-spec.md`）。旧启发式会把可见回答误判为思考块，
/// 进而在回填阶段把正文改写成"内部思考"，并丢掉该轮唯一的签名——这正是
/// "签名剥离 → 跨请求死循环"事故的根因。
#[inline]
pub fn is_thought_part(part: &Value) -> bool {
    part.get("thought")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// 把签名归位到**该轮第一个非思考 part**，其余 part 一律删除签名字段。
///
/// 由官方报文归纳出的四条硬约束：
///
/// 1. 锚点 = 该轮第一个 `thought != true` 的 part，**不是**硬编码的 `parts[0]`
/// 确定 model 轮次中接收签名的目标锚点部件索引。
///
/// 官方规范与 Google Gemini 强校验铁律（现场 400 铁证）：
/// 1. 若本轮包含工具调用 (`functionCall`)，Google Gemini 的工具校验器强制要求签名**必须落在 `functionCall` 部件上**。
///    若存在 `functionCall` 但签名落在了正文文本部件（如客户端占位符 `"..."` 或过程说明）上，
///    上游直接抛出终止性 400 错误：
///    `Function call is missing a thought_signature in functionCall parts. Additional data, function call ...`
///    因此，当存在 `functionCall` 时，锚点**严格必须**为首个 `functionCall` 部件。
/// 2. 若本轮无任何 `functionCall`（纯正文回答轮次），锚点为首个非思考部件（`thought != true` 的纯文本正文）。
/// 3. 若整轮皆为思考块，则无锚点（返回 None）。
pub fn find_turn_anchor(parts: &[Value]) -> Option<usize> {
    find_turn_anchor_with(parts, true)
}

/// `prefer_function_call`：Gemini 为 true，有工具调用时锚点必须是第一个 `functionCall`。
/// Claude 为 false，锚点固定是该轮第一个非思考 part，即使后面还有工具调用。
pub fn find_turn_anchor_with(parts: &[Value], prefer_function_call: bool) -> Option<usize> {
    if prefer_function_call {
        if let Some(fc_pos) = parts.iter().position(|p| p.get("functionCall").is_some()) {
            return Some(fc_pos);
        }
    }
    parts.iter().position(|p| !is_thought_part(p))
}

/// 从单个部件中提取合法签名（若有）。
/// `is_claude`: 若为 true 则校验 Claude 签名形态，若为 false 则允许官方哨兵或 Gemini 签名形态。
pub fn extract_part_signature(part: &Value, is_claude: bool) -> Option<String> {
    let raw = part
        .get("thoughtSignature")
        .or_else(|| part.get("thought_signature"))
        .and_then(|s| s.as_str())?;

    if is_claude {
        if is_real_signature(raw) && is_claude_signature(raw) {
            Some(raw.to_string())
        } else {
            None
        }
    } else {
        if raw == SENTINEL_SIGNATURE || (is_real_signature(raw) && is_likely_gemini_signature(raw))
        {
            Some(raw.to_string())
        } else {
            None
        }
    }
}

/// 从一轮部件切片中提取本轮合法签名：
/// 若 `prefer_function_call` 为 true 且当前切片中存在 `functionCall`，
/// 强制优先从 `functionCall` 部件中提取签名，确保工具调用的合法凭据不被前面的说明文本抢先占有；
/// 若 `functionCall` 未携带签名或为纯正文轮次，则回退查找非思考普通部件自带的签名。
pub fn extract_turn_signature(
    parts: &[Value],
    prefer_function_call: bool,
    is_claude: bool,
) -> Option<String> {
    let has_fc = parts.iter().any(|p| p.get("functionCall").is_some());
    if prefer_function_call && has_fc {
        parts
            .iter()
            .filter(|p| p.get("functionCall").is_some())
            .find_map(|p| extract_part_signature(p, is_claude))
            .or_else(|| {
                parts
                    .iter()
                    .filter(|p| !is_thought_part(p))
                    .find_map(|p| extract_part_signature(p, is_claude))
            })
    } else {
        parts
            .iter()
            .filter(|p| !is_thought_part(p))
            .find_map(|p| extract_part_signature(p, is_claude))
    }
}

/// 官方报文对齐规范：为 model 轮次将签名归位到正确的锚点。
///
/// 由官方报文归纳出的硬约束：
/// 1. 签名只写在「目标锚点部件」上（若有工具调用则落在首个 `functionCall`，纯正文轮落在首个正文 `parts[0]` 或 `parts[1]`）；
/// 2. `thought: true` / `functionResponse` / 其余 part —— 字段必须**缺席**，而不是空串；
/// 3. 锚点自带或本轮携带真实签名时**抢救保留**（覆盖客户端将签名误挂在占位文本或非锚点部件的场景）；
/// 4. 锚点无签名且 `fallback_sig` 也为空时 —— **什么都不写**。官方在"在飞轮"上就是缺席的，
///    缺失签名是被容忍的，**绝不发明哨兵**。
///
/// 返回最终写入锚点的签名（若有）。
pub fn place_turn_signature(parts: &mut Vec<Value>, fallback_sig: Option<&str>) -> Option<String> {
    place_turn_signature_scoped(parts, fallback_sig, true, None)
}

pub fn place_turn_signature_with(
    parts: &mut Vec<Value>,
    fallback_sig: Option<&str>,
    prefer_function_call: bool,
) -> Option<String> {
    place_turn_signature_scoped(parts, fallback_sig, prefer_function_call, None)
}

pub fn place_turn_signature_scoped(
    parts: &mut Vec<Value>,
    fallback_sig: Option<&str>,
    prefer_function_call: bool,
    store_key: Option<&str>,
) -> Option<String> {
    // 1. 签名抢救与提取（必须在任何清空或过滤之前执行）：
    // 必须经过严格校验（is_real_signature && is_likely_gemini_signature），只有合法才接受！
    // 优先抢救 functionCall 自带的真实签名，无工具调用时抢救首个非思考正文部件签名。
    let has_fc = parts.iter().any(|p| p.get("functionCall").is_some());
    let own_sig = extract_turn_signature(parts, prefer_function_call, !prefer_function_call);

    // 2. 占位文本清理：若本轮包含 functionCall，清理混入的无意义占位正文（如 "..."、"·" 等客户端/中间件遗留脏数据）
    if has_fc {
        parts.retain(|p| {
            if p.get("functionCall").is_none() && !is_thought_part(p) {
                if let Some(t) = p.get("text").and_then(|s| s.as_str()) {
                    if is_placeholder_thought(t) {
                        return false;
                    }
                }
            }
            true
        });
    }

    // 3. 锚点确定：有工具调用则必须锚定首个 functionCall；无工具调用则锚定首个非思考正文
    let anchor = find_turn_anchor_with(parts, prefer_function_call)?;

    // 4. 全量清空非目标部件签名，保证非目标 part 的签名字段确实"缺席"
    for (idx, part) in parts.iter_mut().enumerate() {
        if idx == anchor {
            continue;
        }
        if let Some(obj) = part.as_object_mut() {
            // [Pipeline First] 非锚点部件全量清空误挂的签名（如 text / thought / functionResponse）
            // 但如果当前部件本身也是一个携带了有效签名的 functionCall，予以保留，杜绝抹除并发工具的独立签名
            let is_signed_fc = prefer_function_call
                && obj.contains_key("functionCall")
                && obj
                    .get("thoughtSignature")
                    .or_else(|| obj.get("thought_signature"))
                    .and_then(|s| s.as_str())
                    .map_or(false, |s| s == SENTINEL_SIGNATURE || is_real_signature(s));

            if !is_signed_fc {
                obj.remove("thoughtSignature");
                obj.remove("thought_signature");
            }
        }
    }

    // 5. functionResponse 永不携带签名
    if parts[anchor].get("functionResponse").is_some() {
        return None;
    }

    // 6. 锚点自带/抢救优先，缺失才使用回填来源（真实签名或哨兵）
    let sig = own_sig
        .or_else(|| {
            fallback_sig
                .filter(|s| {
                    *s == SENTINEL_SIGNATURE
                        || (is_real_signature(s)
                            && (is_likely_gemini_signature(s) || is_claude_signature(s)))
                })
                .map(str::to_string)
        })
        .or_else(|| {
            if prefer_function_call && has_fc {
                Some(SENTINEL_SIGNATURE.to_string())
            } else {
                None
            }
        })?;

    // 权威防裂化：如果签名是原始二进制 protobuf (首字节 0x12)，必须转为标准 Base64 编码后再发送给 Gemini！
    let final_sig = if sig.as_bytes().first() == Some(&0x12) {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(sig.as_bytes())
    } else {
        sig
    };
    parts[anchor]["thoughtSignature"] = json!(final_sig);

    // 反向入库优化：如果锚点是工具调用且不是哨兵占位，反向更新/修补回签名缓存与 SQLite tool_signatures 库！
    if final_sig != SENTINEL_SIGNATURE {
        if let Some(fc) = parts[anchor].get("functionCall") {
            if let (Some(scope), Some(id)) = (store_key, fc.get("id").and_then(|v| v.as_str())) {
                crate::proxy::SignatureCache::global().cache_tool_signature(
                    scope,
                    id,
                    final_sig.clone(),
                );
            }
        }
    }

    Some(final_sig)
}

pub fn is_placeholder_thought(s: &str) -> bool {
    let t = s.trim();
    t.is_empty()
        || PLACEHOLDER_THOUGHTS.contains(&t)
        || t.chars().all(|c| c == '.' || c == '·' || c == '…')
}

pub fn is_meaningful_thought(thought: &str) -> bool {
    let t = thought.trim();
    if t.is_empty() || is_placeholder_thought(t) {
        return false;
    }
    // 拦截伪思考标签与客户端占位脏数据
    let stripped = t
        .trim_start_matches("<think>")
        .trim_end_matches("</think>")
        .trim_start_matches("Thinking Process:")
        .trim_start_matches("Thinking Process")
        .trim_start_matches("[Thinking]")
        .trim();
    if stripped.is_empty()
        || stripped.eq_ignore_ascii_case("none")
        || stripped.eq_ignore_ascii_case("null")
        || stripped.eq_ignore_ascii_case("undefined")
        || is_placeholder_thought(stripped)
    {
        return false;
    }
    true
}

/// 提取并切除文本中的 `<think>...</think>` 标签内容（用于跨模型自愈与正文思考文本重提升）
/// 支持任意大小写（<think> / <THINK>）、前后正文无缝缝合拼接、以及未闭合标签容错
/// 返回 `Some((thought, remaining_visible))`
pub fn extract_think_tags(text: &str) -> Option<(String, String)> {
    let lower = text.to_lowercase();
    let start_tag = "<think>";
    let end_tag = "</think>";

    if let Some(start_pos) = lower.find(start_tag) {
        let after_start = start_pos + start_tag.len();
        if let Some(end_rel) = lower[after_start..].find(end_tag) {
            let end_pos = after_start + end_rel;
            let thought = text[after_start..end_pos].trim().to_string();
            let before = text[..start_pos].trim();
            let after = text[end_pos + end_tag.len()..].trim();
            let visible = if before.is_empty() {
                after.to_string()
            } else if after.is_empty() {
                before.to_string()
            } else {
                format!("{}\n\n{}", before, after)
            };
            return Some((thought, visible));
        } else {
            // 未闭合标签容错：截断到文本末尾
            let thought = text[after_start..].trim().to_string();
            let before = text[..start_pos].trim();
            return Some((thought, before.to_string()));
        }
    }
    None
}

fn is_capturable_thought(thought: &str, signature: Option<&str>) -> bool {
    // 占位符或纯空白思考绝不可捕获为新的持久化思考记录！
    if is_placeholder_thought(thought) || thought.trim().is_empty() {
        // [DECOUPLE 2026-09-26] 官方新规：任何 model 轮都可能返回签名（正文轮 / 工具轮 / 纯思考轮）。
        // 纯 fc 轮（无思考文本）只要携带真实签名，也必须入库——否则下一轮锚点回填时找不到签名。
        return signature.is_some_and(is_real_signature);
    }
    if signature.is_some_and(is_real_signature) {
        return true;
    }
    is_meaningful_thought(thought)
}

fn record_bytes(rec: &ThinkingRecord) -> usize {
    rec.thought.len() + rec.signature.as_ref().map(|s| s.len()).unwrap_or(0) + rec.visible.len()
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
    if let Some(ref sig) = rec.signature.as_ref().filter(|s| is_real_signature(s)) {
        for (i, ex) in existing.iter().enumerate() {
            if used[i] {
                continue;
            }
            if let Some(ref ex_sig) = ex.signature {
                if signatures_match(sig, ex_sig) {
                    return Some(i);
                }
            }
        }
    }
    if !rec.tool_ids.is_empty() {
        for (i, ex) in existing.iter().enumerate() {
            if used[i] {
                continue;
            }
            if rec
                .tool_ids
                .iter()
                .any(|id| ex.tool_ids.iter().any(|x| x == id))
            {
                return Some(i);
            }
        }
    }
    let rec_has_tools = !rec.tool_ids.is_empty() || !rec.tool_names.is_empty();
    for (i, ex) in existing.iter().enumerate() {
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
    // Sentinel / missing thought signature still needs ThinkingStore or tool-sig alignment.
    let thought_sig_ok = parts.iter().any(|p| {
        p.get("thought").and_then(|v| v.as_bool()).unwrap_or(false)
            && p.get("thoughtSignature")
                .or_else(|| p.get("thought_signature"))
                .and_then(|s| s.as_str())
                .is_some_and(is_real_signature)
    });
    if !thought_sig_ok {
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

fn write_canonical_json(val: &Value, out: &mut Vec<u8>) {
    match val {
        Value::Null => out.extend_from_slice(b"null"),
        Value::Bool(b) => out.extend_from_slice(if *b { b"true" } else { b"false" }),
        Value::Number(n) => out.extend_from_slice(n.to_string().as_bytes()),
        Value::String(s) => {
            if let Ok(json_str) = serde_json::to_string(s) {
                out.extend_from_slice(json_str.as_bytes());
            } else {
                out.extend_from_slice(s.as_bytes());
            }
        }
        Value::Array(arr) => {
            out.push(b'[');
            for (i, elem) in arr.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_canonical_json(elem, out);
            }
            out.push(b']');
        }
        Value::Object(map) => {
            out.push(b'{');
            let mut sorted_keys: Vec<&String> = map.keys().collect();
            sorted_keys.sort();
            for (i, &key) in sorted_keys.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                if let Ok(json_key) = serde_json::to_string(key) {
                    out.extend_from_slice(json_key.as_bytes());
                } else {
                    out.extend_from_slice(key.as_bytes());
                }
                out.push(b':');
                if let Some(v) = map.get(key) {
                    write_canonical_json(v, out);
                }
            }
            out.push(b'}');
        }
    }
}

pub fn canonical_json_hash(val: Option<&Value>) -> String {
    let mut out = Vec::with_capacity(128);
    match val {
        Some(v) => write_canonical_json(v, &mut out),
        None => out.extend_from_slice(b"{}"),
    }
    let mut hasher = Sha256::new();
    hasher.update(&out);
    let hex = format!("{:x}", hasher.finalize());
    hex[..12].to_string()
}

pub fn compute_causal_anchor(turn: Option<&Value>) -> String {
    let Some(content) = turn else {
        return "root".to_string();
    };

    let mut hasher = Sha256::new();
    let role = content.get("role").and_then(|r| r.as_str()).unwrap_or("");
    hasher.update(role.as_bytes());
    hasher.update([0xff]);

    if let Some(parts) = content.get("parts").and_then(|p| p.as_array()) {
        for part in parts {
            if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                hasher.update(b"txt:");
                hasher.update(text.len().to_string().as_bytes());
                hasher.update([0xfe]);
                let prefix_len = text.len().min(48);
                hasher.update(&text.as_bytes()[..prefix_len]);
                hasher.update([0xfd]);
                let suffix_start = text.len().saturating_sub(48);
                hasher.update(&text.as_bytes()[suffix_start..]);
                hasher.update([0xfc]);
            } else if let Some(fr) = part.get("functionResponse") {
                hasher.update(b"fr:");
                let fr_name = fr.get("name").and_then(|n| n.as_str()).unwrap_or("");
                hasher.update(fr_name.as_bytes());
                hasher.update([0xfe]);
                if let Some(resp) = fr.get("response") {
                    let mut resp_bytes = Vec::new();
                    write_canonical_json(resp, &mut resp_bytes);
                    let resp_hash = Sha256::digest(&resp_bytes);
                    hasher.update(&resp_hash[..8]);
                }
                hasher.update([0xfd]);
            } else if let Some(fc) = part.get("functionCall") {
                hasher.update(b"fc:");
                let fc_name = fc.get("name").and_then(|n| n.as_str()).unwrap_or("");
                hasher.update(fc_name.as_bytes());
                hasher.update([0xfe]);
                let args_hash = canonical_json_hash(fc.get("args"));
                hasher.update(args_hash.as_bytes());
                hasher.update([0xfd]);
            }
        }
    }

    let hex = format!("{:x}", hasher.finalize());
    hex[..12].to_string()
}

pub fn synthesize_tool_id(
    tool_name: &str,
    args: Option<&Value>,
    causal_anchor: &str,
    call_index_in_turn: usize,
) -> String {
    let args_hash = canonical_json_hash(args);
    let anchor_clean = if causal_anchor.is_empty() {
        "root"
    } else {
        causal_anchor
    };
    format!(
        "call_{}_{}_{}_{}",
        tool_name, anchor_clean, args_hash, call_index_in_turn
    )
}

fn inspect_parts(parts: &[Value]) -> (String, Vec<String>, Vec<String>, String) {
    inspect_parts_with_anchor(parts, "root")
}

fn inspect_parts_with_anchor(
    parts: &[Value],
    anchor: &str,
) -> (String, Vec<String>, Vec<String>, String) {
    let mut visible = String::new();
    let mut thought = String::new();
    let mut tool_ids = Vec::new();
    let mut tool_names = Vec::new();
    let mut function_call_count = 0usize;
    for part in parts {
        let is_thought = part
            .get("thought")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
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
            let explicit_id = fc
                .get("id")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(|s| crate::proxy::common::utils::normalize_tool_id(s).into_owned());

            let synthetic = synthesize_tool_id(&name, fc.get("args"), anchor, function_call_count);
            function_call_count += 1;

            // 核心演进：全面转战因果伪哈希 ID！首位强制存入确定性 synthetic ID
            if !tool_ids.iter().any(|x| x == &synthetic) {
                tool_ids.push(synthetic);
            }
            if let Some(real_id) = explicit_id {
                if !tool_ids.iter().any(|x| x == &real_id) {
                    tool_ids.push(real_id);
                }
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
        let tool_ids = tool_id.map(|id| vec![id.to_string()]).unwrap_or_default();
        let tool_names = if tool_id.is_some() {
            vec!["shell".to_string()]
        } else {
            Vec::new()
        };
        let fp = fingerprint(visible, &tool_ids, &tool_names);
        let mut hasher = Sha256::new();
        hasher.update(thought.as_bytes());
        hasher.update(visible.as_bytes());
        if let Some(t_id) = tool_id {
            hasher.update(t_id.as_bytes());
        }
        let hash_hex = format!("{:x}", hasher.finalize());
        let sig = format!("sig_{:0>56}", &hash_hex[..40]);
        ThinkingRecord {
            fingerprint: fp,
            thought: thought.to_string(),
            signature: Some(sig),
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
        assert_eq!(
            contents[0]["parts"][0]["text"].as_str().unwrap().len(),
            50_000
        );
        assert_eq!(contents[0]["parts"][0]["thought"], true);
        assert!(contents[0]["parts"][0].get("thoughtSignature").is_none());
        assert_eq!(
            contents[0]["parts"][1]["thoughtSignature"]
                .as_str()
                .unwrap()
                .len(),
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
            contents[0]["parts"][1]["thoughtSignature"]
                .as_str()
                .unwrap()
                .len(),
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
        let key = format!("t:s1-{}", uuid::Uuid::new_v4());
        store.record(&key, rec("short", "same", None));
        store.record(&key, rec("much longer thought", "same", None));
        let stats = store.session_stats(&key).unwrap();
        assert_eq!(stats.0, 1);
        let mut contents = vec![json!({"role":"model","parts":[{"text":"same"}]})];
        store.restore_gemini_contents(&key, &mut contents);
        assert_eq!(contents[0]["parts"][0]["text"], "much longer thought");
        store.end_session(&key);
    }

    #[test]
    fn tool_record_does_not_pollute_earlier_text_turns() {
        let store = ThinkingStore::new();
        // Turn 2 generated thinking + tool call
        store.record(
            "t:s1",
            rec("**Inferring User's Intention**", "", Some("call_54421")),
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
        assert_eq!(
            contents[0]["parts"][0]["text"],
            "你好！我是 JeikCode AI 编程助手。"
        );
        assert!(contents[0]["parts"][0].get("thought").is_none());

        // Turn 1 must NOT have thinking injected
        assert_eq!(contents[1]["parts"].as_array().unwrap().len(), 1);
        assert_eq!(contents[1]["parts"][0]["text"], "当然是真的！😄");
        assert!(contents[1]["parts"][0].get("thought").is_none());

        // Turn 2 MUST have thinking injected and matched with call_54421
        assert_eq!(contents[2]["parts"][0]["thought"], true);
        assert_eq!(
            contents[2]["parts"][0]["text"],
            "**Inferring User's Intention**"
        );
        assert_eq!(contents[2]["parts"][1]["functionCall"]["id"], "call_54421");
    }

    #[test]
    fn test_thinking_with_text_and_parallel_tools() {
        let store = ThinkingStore::new();

        // Test Case 6: 有思考、有正文、有多工具并行出来
        let tool_ids = vec!["call_batch_1".to_string(), "call_batch_2".to_string()];
        let tool_names = vec!["read_file".to_string(), "grep_search".to_string()];
        // 构造符合 Google 原生特征的假签名（Base64 解码后首字节 = protobuf tag 0x12）
        let real_sig = {
            use base64::Engine;
            let mut raw = vec![0x12u8];
            raw.extend_from_slice(&[b'A'; 60]);
            base64::engine::general_purpose::STANDARD.encode(raw)
        };
        let fp = fingerprint("I will read both files in parallel", &tool_ids, &tool_names);
        store.record(
            "t:s2",
            ThinkingRecord {
                fingerprint: fp,
                thought: "Parallel execution planned".to_string(),
                signature: Some(real_sig.clone()),
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
        // Index 0: thought block —— 保持纯净文本，铁律 I4：思考块绝不携带签名
        assert_eq!(parts[0]["thought"], true);
        assert_eq!(parts[0]["text"], "Parallel execution planned");
        assert!(parts[0].get("thoughtSignature").is_none());

        // Index 1: 可见正文 (无签名)
        assert_eq!(parts[1]["text"], "I will read both files in parallel");
        assert!(parts[1].get("thoughtSignature").is_none());

        // Index 2: 锚点为首个 functionCall (call_batch_1) 携带签名
        assert_eq!(parts[2]["functionCall"]["id"], "call_batch_1");
        assert_eq!(
            parts[2]["thoughtSignature"], real_sig,
            "Real signature must be restored onto the functionCall anchor"
        );
        assert_eq!(parts[3]["functionCall"]["id"], "call_batch_2");
        assert!(parts[3].get("thoughtSignature").is_none());
    }

    #[test]
    fn test_sqlite_persistence_and_recovery() {
        let store_key = "test_session_sqlite_recovery_unique";
        let fp = fingerprint(
            "Persisted visible text",
            &["call_persisted_999".to_string()],
            &["bash".to_string()],
        );
        // 构造符合 Google 原生特征的假签名（Base64 解码后首字节 = protobuf tag 0x12）
        let real_sig = {
            use base64::Engine;
            let mut raw = vec![0x12u8];
            raw.extend_from_slice(&[b'A'; 60]);
            base64::engine::general_purpose::STANDARD.encode(raw)
        };
        let rec = ThinkingRecord {
            fingerprint: fp,
            thought: "Thought restored from SQLite".to_string(),
            signature: Some(real_sig.clone()),
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
        assert!(parts[1].get("thoughtSignature").is_none());
        assert_eq!(parts[2]["thoughtSignature"], real_sig);
        assert_eq!(parts[2]["functionCall"]["id"], "call_persisted_999");
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
                store_key,
                fp,
                thought,
                Some(sig),
                ids,
                names,
                visible,
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
        assert_eq!(
            rows.len(),
            3,
            "older 你好 turn must not be overwritten: {rows:?}"
        );
        assert_eq!(rows[0].thought, "thought-1-longer");
        assert_eq!(rows[1].thought, "thought-tool");
        assert_eq!(rows[2].thought, "thought-3");
        let _ = crate::modules::proxy_db::delete_thinking_records_for_session(store_key);
    }

    #[test]
    fn test_explicit_session_id_and_query_extraction() {
        // 1. Query parameter extraction
        let sid = extract_session_from_query_str(
            "model=gemini-2.5-pro&session_id=win_alpha_101&temp=0.7",
        );
        assert_eq!(sid.as_deref(), Some("win_alpha_101"));

        let sid_alias = extract_session_from_query_str("channel=proj_beta");
        assert_eq!(sid_alias.as_deref(), Some("proj_beta"));

        // 2. Header extraction: Claude Code / Cursor / VSCode
        let mut headers = HeaderMap::new();
        headers.insert("x-cursor-session-id", "cursor-tab-99".parse().unwrap());
        let extracted = explicit_session_id_with_query(&headers, None, None);
        assert_eq!(extracted.as_deref(), Some("cursor-tab-99"));

        // 3. Body & Metadata extraction
        let body = json!({
            "metadata": {
                "conversation_id": "meta-conv-888"
            }
        });
        let empty_headers = HeaderMap::new();
        let extracted2 = explicit_session_id_with_query(&empty_headers, Some(&body), None);
        assert_eq!(extracted2.as_deref(), Some("meta-conv-888"));
    }

    #[test]
    fn test_wildcard_client_session_headers() {
        let uuid = "c17b6d3c-e808-4874-8f16-b5dd4b6a2179";

        let mut atom = HeaderMap::new();
        atom.insert("x-atomcode-session-id", uuid.parse().unwrap());
        assert_eq!(
            explicit_session_id_with_query(&atom, None, None).as_deref(),
            Some(uuid)
        );

        let mut jeik = HeaderMap::new();
        jeik.insert("x-jeikcode-sessionid", uuid.parse().unwrap());
        assert_eq!(
            explicit_session_id_with_query(&jeik, None, None).as_deref(),
            Some(uuid)
        );

        let mut multi = HeaderMap::new();
        multi.insert("x-api-key", "secret".parse().unwrap());
        multi.insert("x-atomcode-session-id", uuid.parse().unwrap());
        multi.insert("x-jeikcode-sessionid", uuid.parse().unwrap());
        multi.insert("x-session-id", uuid.parse().unwrap());
        assert_eq!(
            explicit_session_id_with_query(&multi, None, None).as_deref(),
            Some(uuid)
        );

        let mut custom = HeaderMap::new();
        custom.insert("x-windsurf-session-id", "wind-tab-1".parse().unwrap());
        assert_eq!(
            explicit_session_id_with_query(&custom, None, None).as_deref(),
            Some("wind-tab-1")
        );

        let mut ignored = HeaderMap::new();
        ignored.insert("x-request-id", "req-should-not-win".parse().unwrap());
        ignored.insert("x-api-key", "secret".parse().unwrap());
        assert_eq!(explicit_session_id_with_query(&ignored, None, None), None);
    }

    #[test]
    fn product_session_headers_win_over_generic_x_session_id() {
        let mut jeik = HeaderMap::new();
        jeik.insert("x-session-id", "generic-session".parse().unwrap());
        jeik.insert("x-jeikcode-sessionid", "jeik-session".parse().unwrap());
        assert_eq!(
            explicit_session_id_with_query(&jeik, None, None).as_deref(),
            Some("jeik-session")
        );

        let mut atom = HeaderMap::new();
        atom.insert("x-session-id", "generic-session".parse().unwrap());
        atom.insert("x-atomcode-session-id", "atom-session".parse().unwrap());
        assert_eq!(
            explicit_session_id_with_query(&atom, None, None).as_deref(),
            Some("atom-session")
        );

        let mut wildcard = HeaderMap::new();
        wildcard.insert("x-session-id", "generic-session".parse().unwrap());
        wildcard.insert("x-windsurf-session-id", "wind-session".parse().unwrap());
        assert_eq!(
            explicit_session_id_with_query(&wildcard, None, None).as_deref(),
            Some("wind-session")
        );

        let mut only_generic = HeaderMap::new();
        only_generic.insert("x-session-id", "generic-session".parse().unwrap());
        assert_eq!(
            explicit_session_id_with_query(&only_generic, None, None).as_deref(),
            Some("generic-session")
        );

        let mut claude = HeaderMap::new();
        claude.insert("x-session-id", "generic-session".parse().unwrap());
        claude.insert(
            "x-claude-code-session-id",
            "claude-session".parse().unwrap(),
        );
        assert_eq!(
            explicit_session_id_with_query(&claude, None, None).as_deref(),
            Some("claude-session")
        );
    }

    #[test]
    fn test_3d_orthogonal_blended_session_stability_and_isolation() {
        // 1. 同一对话多轮聊天：相同 Headers 与相同的 Anchor -> 100% 相同稳定
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-claude-code-session-id",
            "53696541-0a6e-4be0-801e-2ee7a5601831".parse().unwrap(),
        );
        let scope_turn1 = SessionScope::from_headers(&headers, "sid-main-conversation-root");
        let scope_turn2 = SessionScope::from_headers(&headers, "sid-main-conversation-root");
        assert_eq!(scope_turn1.client_id, scope_turn2.client_id);
        assert_eq!(scope_turn1.store_key, scope_turn2.store_key);

        // 2. 主 Agent 与 Subagent 共用 x-claude-code-session-id，锚点不同：
        //    思维库隔离，账号粘性仍绑定同一会话。
        let scope_subagent = SessionScope::from_headers(&headers, "sid-subagent-distinct-prompt");
        assert_ne!(scope_turn1.client_id, scope_subagent.client_id);
        assert_ne!(scope_turn1.store_key, scope_subagent.store_key);
        assert_eq!(scope_turn1.affinity_key, scope_subagent.affinity_key);
        assert_eq!(scope_turn1.affinity_id, scope_subagent.affinity_id);

        // 2b. 没有稳定会话身份时，锚点同时分开账号粘性，避免匿名并发对话并到同一账号。
        let bare_a = SessionScope::from_headers(&HeaderMap::new(), "anchor-a");
        let bare_b = SessionScope::from_headers(&HeaderMap::new(), "anchor-b");
        assert_ne!(bare_a.affinity_key, bare_b.affinity_key);
        assert_eq!(bare_a.affinity_key, bare_a.store_key);
        assert_ne!(scope_turn1.affinity_key, bare_a.affinity_key);

        // 3. 不同租户多用户并发（不同 Authorization / API Key） -> 绝对隔离！
        let mut headers_user_a = headers.clone();
        headers_user_a.insert("authorization", "Bearer user-token-aaa".parse().unwrap());
        let mut headers_user_b = headers.clone();
        headers_user_b.insert("authorization", "Bearer user-token-bbb".parse().unwrap());
        let scope_user_a =
            SessionScope::from_headers(&headers_user_a, "sid-main-conversation-root");
        let scope_user_b =
            SessionScope::from_headers(&headers_user_b, "sid-main-conversation-root");
        assert_ne!(scope_user_a.store_key, scope_user_b.store_key);
        assert_ne!(scope_user_a.affinity_key, scope_user_b.affinity_key);

        // 4. Header 乱序注入时哈希绝对一致（BTreeMap 保证确定性排序）
        let mut headers_order1 = HeaderMap::new();
        headers_order1.insert("x-atomcode-session-id", "uuid-123".parse().unwrap());
        headers_order1.insert("x-jeikcode-sessionid", "uuid-456".parse().unwrap());

        let mut headers_order2 = HeaderMap::new();
        headers_order2.insert("x-jeikcode-sessionid", "uuid-456".parse().unwrap());
        headers_order2.insert("x-atomcode-session-id", "uuid-123".parse().unwrap());

        let scope_ord1 = SessionScope::from_headers(&headers_order1, "anchor-1");
        let scope_ord2 = SessionScope::from_headers(&headers_order2, "anchor-1");
        assert_eq!(scope_ord1.client_id, scope_ord2.client_id);
        assert_eq!(scope_ord1.affinity_key, scope_ord2.affinity_key);

        // 5. 跨协议相同显式会话头（如 Claude 切到 OpenAI）：相同会话锚点下 store_key 绝对一致共享，不同锚点下强隔离
        let mut claude_headers = HeaderMap::new();
        claude_headers.insert("x-session-id", "conv-uuid-999".parse().unwrap());
        claude_headers.insert("x-api-key", "secret-token".parse().unwrap());

        let mut openai_headers = HeaderMap::new();
        openai_headers.insert("x-session-id", "conv-uuid-999".parse().unwrap());
        openai_headers.insert("authorization", "Bearer secret-token".parse().unwrap());

        let scope_claude = SessionScope::from_headers(&claude_headers, "shared-anchor");
        let scope_openai = SessionScope::from_headers(&openai_headers, "shared-anchor");

        assert_eq!(
            scope_claude.store_key, scope_openai.store_key,
            "Cross-protocol requests in the same session must share the identical store_key"
        );
        assert_eq!(scope_claude.affinity_key, scope_openai.affinity_key);

        // 6. 产品头压过会变的通用 x-session-id。审批文案只拆思维库，不换账号。
        let mut main_headers = HeaderMap::new();
        main_headers.insert("x-claude-code-session-id", "cli-session".parse().unwrap());
        main_headers.insert("x-session-id", "generic-a".parse().unwrap());
        let mut approval_headers = HeaderMap::new();
        approval_headers.insert("x-claude-code-session-id", "cli-session".parse().unwrap());
        approval_headers.insert("x-session-id", "generic-b".parse().unwrap());
        let scope_main = SessionScope::from_headers(&main_headers, "anchor-main");
        let scope_approval = SessionScope::from_headers(&approval_headers, "anchor-approval");
        assert_eq!(scope_main.affinity_key, scope_approval.affinity_key);
        assert_ne!(scope_main.store_key, scope_approval.store_key);

        // 7. previous_response_id 每轮都变，不能当稳定身份。
        let body_turn = json!({ "previous_response_id": "resp-1" });
        let body_next = json!({ "previous_response_id": "resp-2" });
        let resp_a =
            SessionScope::from_headers_and_body(&HeaderMap::new(), Some(&body_turn), "anchor");
        let resp_b =
            SessionScope::from_headers_and_body(&HeaderMap::new(), Some(&body_next), "anchor");
        assert_eq!(resp_a.affinity_key, resp_b.affinity_key);
        assert_eq!(resp_a.store_key, resp_b.store_key);

        let rooted = json!({
            "session_id": "resp-root",
            "previous_response_id": "resp-9"
        });
        let rooted_next = json!({
            "session_id": "resp-root",
            "previous_response_id": "resp-10"
        });
        let root_a =
            SessionScope::from_headers_and_body(&HeaderMap::new(), Some(&rooted), "anchor-a");
        let root_b =
            SessionScope::from_headers_and_body(&HeaderMap::new(), Some(&rooted_next), "anchor-a");
        assert_eq!(root_a.affinity_key, root_b.affinity_key);
        assert_eq!(root_a.store_key, root_b.store_key);
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
            contents[1]["parts"][0]["text"], "think-turn2",
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
        let session_key = format!("t:compress-{}", uuid::Uuid::new_v4());
        let key = &session_key;
        store.record(
            key,
            rec("thought-old-1", "old visible one", Some("call_old_1")),
        );
        store.record(
            key,
            rec("thought-old-2", "old visible two", Some("call_old_2")),
        );
        store.record(
            key,
            rec("thought-old-3", "old visible three", Some("call_old_3")),
        );
        store.record(
            key,
            rec("thought-keep", "kept latest answer", Some("call_keep")),
        );
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
        assert!(
            turns <= 3,
            "orphaned compressed turns should be pruned, got {turns}"
        );
        let parts = contents[0]["parts"].as_array().unwrap();
        assert_eq!(parts[0]["text"], "thought-keep");
        store.end_session(key);
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
        assert_eq!(
            after, 20,
            "placeholder history must not be appended as new turns"
        );
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
        let _ = crate::modules::proxy_db::delete_thinking_records_for_session(key);
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
            assert_eq!(
                parts[0]["thought"], true,
                "thought must stay at parts[0] for turn {i}"
            );
            assert_eq!(parts[0]["text"], format!("THOUGHT-BLOCK-{i}"));
            assert!(parts[0].get("thoughtSignature").is_none());
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

    #[test]
    fn signed_function_call_is_not_mistaken_for_thinking_block() {
        let real_sig = "Ep4MCpsMARFNMg9NDlK9RXXz5Mzq9mniX9KSQBBzbUx3k85w/qDgtcE+28NH+1EvPeULAprqUquvYXGMzUXGy1xJoMnqdkC4vqebuhyd2Xhs0oz+OhqcOTwLhGYOG0KBKQ87Hfw4q/sMCSgf2gz4vFMa6V6kKMJepYlPXKFJJF4ok+W6lUt3PfYln8K9Dh7wB/40iHiZ2BnJd++6hfUwu9Bz1n795S50l0yCj84EaSCDDF334Erxq7Fo";

        // Case 1: thinking enabled, only signed functionCall → must prepend real thought block
        let mut contents = vec![json!({
            "role": "model",
            "parts": [
                {
                    "functionCall": {
                        "name": "read_file",
                        "id": "call_1",
                        "args": { "path": "a.rs" }
                    },
                    "thoughtSignature": real_sig
                }
            ]
        })];
        finalize_gemini_contents_thinking(&mut contents, true);
        let parts = contents[0]["parts"].as_array().unwrap();
        // 【2026-09-27】无思考文本时不再注入 "..." 占位思考块（官方标准形态：
        // 「无思考块 + 锚点带签名」，9/24 轮）。签名落锚点 fc。
        assert_eq!(
            parts.len(),
            1,
            "no placeholder thinking block should be injected"
        );
        assert!(parts[0].get("functionCall").is_some());
        assert_eq!(parts[0]["thoughtSignature"], real_sig);

        // Case 2: thinking disabled → signed functionCall must survive and preserve thoughtSignature for Gemini AST validator
        let mut contents_off = vec![json!({
            "role": "model",
            "parts": [
                {
                    "functionCall": {
                        "name": "bash",
                        "id": "call_9",
                        "args": {}
                    },
                    "thoughtSignature": real_sig
                }
            ]
        })];
        finalize_gemini_contents_thinking(&mut contents_off, false);
        let parts_off = contents_off[0]["parts"].as_array().unwrap();
        assert_eq!(
            parts_off.len(),
            1,
            "functionCall must not be dropped when thinking is off"
        );
        assert!(parts_off[0].get("functionCall").is_some());
        assert_eq!(
            parts_off[0]["thoughtSignature"], real_sig,
            "Gemini native model requires functionCall to retain its signature even when thinking is off"
        );
    }

    #[test]
    fn finalize_upgrades_sentinel_thought_from_tool_real_signature() {
        let real_sig = "Ep4MCpsMARFNMg9NDlK9RXXz5Mzq9mniX9KSQBBzbUx3k85w/qDgtcE+28NH+1EvPeULAprqUquvYXGMzUXGy1xJoMnqdkC4vqebuhyd2Xhs0oz+OhqcOTwLhGYOG0KBKQ87Hfw4q/sMCSgf2gz4vFMa6V6kKMJepYlPXKFJJF4ok+W6lUt3PfYln8K9Dh7wB/40iHiZ2BnJd++6hfUwu9Bz1n795S50l0yCj84EaSCDDF334Erxq7Fo";
        let mut contents = vec![json!({
            "role": "model",
            "parts": [
                {
                    "text": "...",
                    "thought": true,
                    "thoughtSignature": SENTINEL_SIGNATURE
                },
                {
                    "functionCall": {
                        "name": "read_file",
                        "id": "call_upgrade",
                        "args": {}
                    },
                    "thoughtSignature": real_sig
                }
            ]
        })];
        finalize_gemini_contents_thinking(&mut contents, true);
        let parts = contents[0]["parts"].as_array().unwrap();
        assert_eq!(parts[0]["thought"], true);
        assert!(parts[0].get("thoughtSignature").is_none());
        assert_eq!(parts[1]["thoughtSignature"], real_sig);
    }

    #[test]
    fn turn_needs_restore_when_thought_signature_is_sentinel() {
        let parts = vec![
            json!({
                "text": "some real looking text that is not a placeholder",
                "thought": true,
                "thoughtSignature": SENTINEL_SIGNATURE
            }),
            json!({
                "functionCall": { "name": "shell", "id": "call_x", "args": {} },
                "thoughtSignature": SENTINEL_SIGNATURE
            }),
        ];
        assert!(
            turn_needs_restore(&parts, "some real looking text that is not a placeholder"),
            "sentinel thought signature must still request restore"
        );
    }

    #[test]
    fn test_is_meaningful_thought_sanitizer() {
        // Placeholders & empty must fail
        assert!(!is_meaningful_thought(""));
        assert!(!is_meaningful_thought("   "));
        assert!(!is_meaningful_thought("..."));
        assert!(!is_meaningful_thought("···"));
        assert!(!is_meaningful_thought("."));

        // Pseudo-thinking tags & placeholders must fail
        assert!(!is_meaningful_thought("<think></think>"));
        assert!(!is_meaningful_thought("<think>\n\n</think>"));
        assert!(!is_meaningful_thought("Thinking Process:\n"));
        assert!(!is_meaningful_thought("[Thinking]"));
        assert!(!is_meaningful_thought("None"));
        assert!(!is_meaningful_thought("none"));
        assert!(!is_meaningful_thought("null"));
        assert!(!is_meaningful_thought("undefined"));
        assert!(!is_meaningful_thought("[Thinking]\n..."));

        // Real thoughts must pass
        assert!(is_meaningful_thought(
            "Let's analyze the problem step by step."
        ));
        assert!(is_meaningful_thought(
            "<think>First compute the square root of 16, which is 4.</think>"
        ));
        assert!(is_meaningful_thought(
            "Thinking Process:\n1. Check file existence\n2. Open file"
        ));
    }

    #[test]
    fn test_finalize_thinking_disabled_downgrades_meaningful_thought_and_strips_placeholders() {
        let mut contents = vec![
            json!({
                "role": "model",
                "parts": [
                    {
                        "text": "...",
                        "thought": true,
                        "thoughtSignature": SENTINEL_SIGNATURE
                    },
                    {
                        "text": "Hello, how can I help?"
                    }
                ]
            }),
            json!({
                "role": "model",
                "parts": [
                    {
                        "text": "Real thought: solving user query carefully.",
                        "thought": true,
                        "thoughtSignature": "some_sig"
                    },
                    {
                        "text": "Here is the answer."
                    }
                ]
            }),
        ];

        finalize_gemini_contents_thinking(&mut contents, false);

        // Turn 1: placeholder "..." thought is dropped, only visible text survives
        let parts1 = contents[0]["parts"].as_array().unwrap();
        assert_eq!(parts1.len(), 1);
        assert_eq!(parts1[0]["text"], "Hello, how can I help?");
        assert!(parts1[0].get("thought").is_none());

        // Turn 2: meaningful thought is downgraded to text {"text": "Real thought: ..."}
        let parts2 = contents[1]["parts"].as_array().unwrap();
        assert_eq!(parts2.len(), 2);
        assert_eq!(
            parts2[0]["text"],
            "Real thought: solving user query carefully."
        );
        assert!(parts2[0].get("thought").is_none());
        assert!(parts2[0].get("thoughtSignature").is_none());
        assert_eq!(parts2[1]["text"], "Here is the answer.");
    }

    #[test]
    fn test_finalize_thinking_disabled_cleans_user_function_response_signature() {
        let mut contents = vec![json!({
            "role": "user",
            "parts": [
                {
                    "functionResponse": {
                        "name": "calc",
                        "response": { "result": 42 }
                    },
                    "thoughtSignature": "sig_to_be_cleaned"
                }
            ]
        })];

        finalize_gemini_contents_thinking(&mut contents, false);

        let parts = contents[0]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 1);
        assert!(parts[0].get("functionResponse").is_some());
        assert!(
            parts[0].get("thoughtSignature").is_none(),
            "thoughtSignature must be removed from functionResponse when thinking is disabled"
        );
    }

    #[test]
    fn test_sqlite_penetration_fallback_when_memory_missing() {
        let store = ThinkingStore::new();
        let key = "t:sqlite-fallback-test";
        let tool_id = "call_fallback_999";
        let real_sig = "s".repeat(60);

        // 1. 模拟旧轮次已持久化入库 SQLite（但在内存缓存中已被淘汰或未命中）
        crate::modules::proxy_db::save_thinking_record(
            key,
            "fp_fallback",
            "This thinking was retrieved directly from SQLite!",
            Some(&real_sig),
            &[tool_id.to_string()],
            &[],
            "some visible",
        )
        .unwrap();

        // 确保内存缓存是完全清空的，逼迫触发 L2 SQLite 穿透点查
        store.clear();

        // 2. 构造客户端回传的历史请求（缺少思考块，仅占位符）
        let mut contents = vec![json!({
            "role": "model",
            "parts": [
                {
                    "text": "...",
                    "thought": true,
                    "thoughtSignature": SENTINEL_SIGNATURE
                },
                {
                    "functionCall": {
                        "name": "shell",
                        "id": tool_id,
                        "args": {}
                    }
                }
            ]
        })];

        // 3. 执行思考复活：应当穿透到 SQLite 成功捞回！
        let restored = store.restore_gemini_contents(key, &mut contents);
        assert_eq!(restored, 1, "Must penetrate to SQLite and restore 1 turn!");

        let parts = contents[0]["parts"].as_array().unwrap();
        assert_eq!(parts[0]["thought"], true);
        assert_eq!(
            parts[0]["text"],
            "This thinking was retrieved directly from SQLite!"
        );
        assert!(parts[0].get("thoughtSignature").is_none());
        assert_eq!(parts[1]["thoughtSignature"], real_sig);

        let _ = crate::modules::proxy_db::delete_thinking_records_for_session(key);
    }

    #[test]
    fn test_canonical_json_hash_key_order_independence() {
        let a = json!({ "b": 2, "a": 1, "c": { "z": 9, "y": 8 } });
        let b = json!({ "a": 1, "c": { "y": 8, "z": 9 }, "b": 2 });
        assert_eq!(
            canonical_json_hash(Some(&a)),
            canonical_json_hash(Some(&b)),
            "Canonical JSON hash must be independent of key order"
        );
    }

    #[test]
    fn test_compute_causal_anchor_differentiates_user_text_vs_tool_response() {
        let turn1 = json!({ "role": "user", "parts": [{ "text": "Run cargo check" }] });
        let turn2 = json!({
            "role": "user",
            "parts": [{ "functionResponse": { "name": "bash", "response": { "exit_code": 1, "output": "error" } } }]
        });
        let anchor1 = compute_causal_anchor(Some(&turn1));
        let anchor2 = compute_causal_anchor(Some(&turn2));
        assert_ne!(
            anchor1, anchor2,
            "Causal anchors for user text vs tool response must be distinct"
        );
    }

    #[test]
    fn test_gemini_native_consecutive_identical_tools_do_not_overwrite() {
        let key = "t:test_identical_tools";
        let _ = crate::modules::proxy_db::delete_thinking_records_for_session(key);
        let store = ThinkingStore::new();

        // Turn 1: user text -> model tool call
        let mut acc1 = TurnAccumulator::with_anchor("anchor_user_turn_1");
        acc1.ingest_part(&json!({
            "text": "Thought for turn 1",
            "thought": true,
            "thoughtSignature": "s".repeat(60)
        }));
        acc1.ingest_part(&json!({
            "functionCall": { "name": "bash", "args": { "command": "pwd" } }
        }));
        store.record(key, acc1.into_record());

        // Turn 2: tool response -> model tool call (identical tool name and args!)
        let mut acc2 = TurnAccumulator::with_anchor("anchor_fr_turn_2");
        acc2.ingest_part(&json!({
            "text": "Thought for turn 2",
            "thought": true,
            "thoughtSignature": "t".repeat(60)
        }));
        acc2.ingest_part(&json!({
            "functionCall": { "name": "bash", "args": { "command": "pwd" } }
        }));
        store.record(key, acc2.into_record());

        let stats = store.session_stats(key).unwrap();
        assert_eq!(
            stats.0, 2,
            "Must store two distinct turns, not overwrite via collision"
        );

        let _ = crate::modules::proxy_db::delete_thinking_records_for_session(key);
    }

    #[test]
    fn test_prune_orphaned_records_zero_phase_shift() {
        let key = "t:test_phase_shift_prune";
        let _ = crate::modules::proxy_db::delete_thinking_records_for_session(key);
        let store = ThinkingStore::new();

        for i in 0..6 {
            let user_turn = json!({
                "role": "user",
                "parts": [{ "text": format!("user msg {i}") }]
            });
            let anchor = compute_causal_anchor(Some(&user_turn));
            let mut acc = TurnAccumulator::with_anchor(&anchor);
            acc.ingest_part(&json!({
                "text": format!("Thought turn {i}"),
                "thought": true,
                "thoughtSignature": format!("sig_{:0>60}", i)
            }));
            acc.ingest_part(&json!({
                "functionCall": { "name": "bash", "args": { "step": i } }
            }));
            store.record(key, acc.into_record());
        }

        // Client compresses away turns 0 and 1; only turns 2..5 remain
        let mut contents = Vec::new();
        for i in 2..6 {
            contents.push(json!({
                "role": "user",
                "parts": [{ "text": format!("user msg {i}") }]
            }));
            contents.push(json!({
                "role": "model",
                "parts": [
                    { "text": "...", "thought": true, "thoughtSignature": SENTINEL_SIGNATURE },
                    { "functionCall": { "name": "bash", "args": { "step": i } } }
                ]
            }));
        }

        let restored = store.restore_gemini_contents(key, &mut contents);
        assert_eq!(restored, 4, "Must restore 4 compressed turns");

        // Verify ZERO PHASE SHIFT: turn 2 must have "Thought turn 2", NOT "Thought turn 4"
        for (idx, step) in (2..6).enumerate() {
            let content_idx = idx * 2 + 1;
            let parts = contents[content_idx]["parts"].as_array().unwrap();
            assert_eq!(
                parts[0]["text"],
                format!("Thought turn {step}"),
                "Turn {step} must strictly match its own thought without phase shift"
            );
            assert!(
                parts[0].get("thoughtSignature").is_none(),
                "Turn {step} thought block must not carry signature per Iron Law I4"
            );
            assert_eq!(
                parts[1]["thoughtSignature"],
                format!("sig_{:0>60}", step),
                "Turn {step} must strictly match its own thoughtSignature on the functionCall anchor"
            );
        }

        let _ = crate::modules::proxy_db::delete_thinking_records_for_session(key);
    }

    #[test]
    fn test_phase0_signature_direct_matching_recovers_truncated_text() {
        let key = "t:test_phase0_sig";
        let _ = crate::modules::proxy_db::delete_thinking_records_for_session(key);
        let store = ThinkingStore::new();
        let real_sig = "s".repeat(60);

        let mut acc = TurnAccumulator::with_anchor("anchor_1");
        acc.ingest_part(&json!({
            "text": "Detailed multi-step chain of thought",
            "thought": true,
            "thoughtSignature": real_sig
        }));
        acc.ingest_part(&json!({
            "functionCall": { "name": "read_file", "args": { "path": "main.rs" } }
        }));
        store.record(key, acc.into_record());

        let mut contents = vec![
            json!({ "role": "user", "parts": [{ "text": "start" }] }),
            json!({
                "role": "model",
                "parts": [
                    { "text": "...", "thought": true, "thoughtSignature": real_sig },
                    { "functionCall": { "name": "read_file", "args": { "path": "main.rs" } } }
                ]
            }),
        ];

        let restored = store.restore_gemini_contents(key, &mut contents);
        assert_eq!(restored, 1);
        assert_eq!(
            contents[1]["parts"][0]["text"], "Detailed multi-step chain of thought",
            "Phase 0 must recover truncated thought using real signature"
        );

        let _ = crate::modules::proxy_db::delete_thinking_records_for_session(key);
    }

    #[test]
    fn test_sqlite_l2_penetration_by_signature_and_tool() {
        let key = "t:test_sqlite_l2_sig_rescue";
        let real_sig = format!("sig_l2_{}", "x".repeat(53));
        let tool_id = "call_bash_anchor123_hash456_0";

        let _ = crate::modules::proxy_db::save_thinking_record(
            key,
            "fp_l2_sig_test",
            "Rescued thought from SQLite via signature",
            Some(&real_sig),
            &[tool_id.to_string()],
            &["bash".to_string()],
            "",
        );

        let store = ThinkingStore::new();
        // Clear memory to force L2 penetration
        store.clear();

        let mut contents = vec![
            json!({ "role": "user", "parts": [{ "text": "hello" }] }),
            json!({
                "role": "model",
                "parts": [
                    { "text": "...", "thought": true, "thoughtSignature": real_sig },
                    { "functionCall": { "name": "bash", "id": tool_id, "args": {} } }
                ]
            }),
        ];

        let restored = store.restore_gemini_contents(key, &mut contents);
        assert_eq!(
            restored, 1,
            "Must penetrate to SQLite and restore via signature"
        );
        assert_eq!(
            contents[1]["parts"][0]["text"],
            "Rescued thought from SQLite via signature"
        );

        let _ = crate::modules::proxy_db::delete_thinking_records_for_session(key);
    }

    #[test]
    fn test_is_likely_gemini_signature_validation() {
        // 1. 官方哨兵
        assert!(is_likely_gemini_signature(SENTINEL_SIGNATURE));

        // 2. 真实 Gemini 原生签名 (首字节 0x12, 以 'E' 开头)
        let gemini_sig = "Ep4KCpsKAWkUfRMa5ZYMDdlPjxrQTLzVZ6MZeopI88888888888888888888888888888888";
        assert!(is_likely_gemini_signature(gemini_sig));

        // 3. Google Vertex AI 双层 Base64 包装 (以 'R' 开头, 解开是 'E...' 且首字节 0x12)
        use base64::Engine;
        let vertex_wrapped =
            base64::engine::general_purpose::STANDARD.encode(gemini_sig.as_bytes());
        assert!(vertex_wrapped.starts_with('R'));
        assert!(is_likely_gemini_signature(&vertex_wrapped));

        // 4. 异构外部 Claude 签名 (以 '3', 'l', 'A', 'R' 开头, 解码后非 0x12)
        let claude_sig_1 = "3mgp11XmVXq9InniGA4VAKd7c97NqFw+dWZt79Uz/w9znho88gSM76jv2bZmir7wI86Ixpha7eWdGuznAot4PNbe3+V9bgMTIEyUarn4MLAiiFVb830ZlM+H5ukQwXdD2Zv8nUSmmZTYinpLPGha8TORZAfpU1FJEvwyECel5+W7kc9kpTWrd8DqRNBTOz5EDtvoatiZgKv5SqInhGXK74SJ+PRIC6fNXvYG082HR6TsVxvVYaerz8A40rloIVTxRNK43h3Ecs1boxY4PZqBT8Yhl2qn/iZ+4Xt7FNkI0DAuS9iK0HYKMC4yw0OqKx/LeU+WFZlyc6hGm1BkzLY6yG97MH7kmJ0OPlBWgWFaTeL/uXuGJX6QkKObXN+phoq+kkF2vdFt/mdJMbdgfmSCVQ9037hGBhOHm0zN50KLkp1SxuAY1oWc+lDcI4ufWoyn";
        let claude_sig_2 = "ls29VsBy+VBvzrVBmB2gNmOCoaeJkn19qz8jP8jExGpDc0IxRaV1V9/+cQ4O00000000000000000000000000000000";
        let claude_sig_3 = "A1nvLg9Twun3bBCb1BKLmSNA6MRxaLE2GdEocv6bwuhNKfUmBB2YMvvmaVyO00000000000000000000000000000000";
        assert!(!is_likely_gemini_signature(claude_sig_1));
        assert!(!is_likely_gemini_signature(claude_sig_2));
        assert!(!is_likely_gemini_signature(claude_sig_3));

        // 5. 过短签名
        assert!(!is_likely_gemini_signature("short_sig"));
    }

    #[test]
    fn test_is_claude_signature_validation() {
        use base64::Engine;
        // 1. 构建合法的原始 Claude 签名（单层 Base64 解码后包含 b"claude"）
        let inner_claude_payload =
            b"\x12\xb2\x02\n\x92\x01\x08\x12\x10\x02\x18\x02*@claude-opus-4-6-signature-data";
        let raw_claude_sig = base64::engine::general_purpose::STANDARD.encode(inner_claude_payload);
        assert!(is_claude_signature(&raw_claude_sig));

        // 2. 构建 Google Vertex AI 双层包装后的 Claude 签名
        let wrapped_claude_sig =
            base64::engine::general_purpose::STANDARD.encode(raw_claude_sig.as_bytes());
        assert!(is_claude_signature(&wrapped_claude_sig));

        // 3. Gemini 签名绝不是 Claude 签名
        let gemini_sig = "Ep4KCpsKAWkUfRMa5ZYMDdlPjxrQTLzVZ6MZeopI88888888888888888888888888888888";
        assert!(!is_claude_signature(gemini_sig));

        // 4. 空与哨兵
        assert!(!is_claude_signature(""));
        assert!(!is_claude_signature(SENTINEL_SIGNATURE));
    }

    #[test]
    fn test_ensure_google_claude_thought_signature_does_not_inflate() {
        use base64::Engine;
        // 原始 Claude 签名：包装一次
        let inner_claude_payload =
            b"\x12\xb2\x02\n\x92\x01\x08\x12\x10\x02\x18\x02*@claude-opus-4-6-signature-data";
        let raw_claude_sig = base64::engine::general_purpose::STANDARD.encode(inner_claude_payload);
        let wrapped_once = ensure_google_claude_thought_signature(&raw_claude_sig);
        assert_ne!(wrapped_once, raw_claude_sig);

        // 已包装的 Claude 签名：幂等，绝对不再包装！
        let wrapped_twice = ensure_google_claude_thought_signature(&wrapped_once);
        assert_eq!(
            wrapped_once, wrapped_twice,
            "Claude signature must be idempotent, no double wrapping"
        );

        // 非 Claude 签名 (Gemini 签名)：绝不能包装！彻底杜绝几何级膨胀
        let gemini_sig = "Ep4KCpsKAWkUfRMa5ZYMDdlPjxrQTLzVZ6MZeopI88888888888888888888888888888888";
        let gemini_out = ensure_google_claude_thought_signature(gemini_sig);
        assert_eq!(
            gemini_out, gemini_sig,
            "Non-Claude signature must NOT be base64 wrapped"
        );
    }

    #[test]
    fn test_restore_and_finalize_gemini_contents_with_model_claude_target_rejects_gemini_signature()
    {
        let store = ThinkingStore::new();
        let key = "t:cross_model_test_session";
        let gemini_sig = "Ep4KCpsKAWkUfRMa5ZYMDdlPjxrQTLzVZ6MZeopI88888888888888888888888888888888";

        // 模拟上一轮 Gemini 存储的思考记录（带有 Gemini 原生签名）
        store.record(
            key,
            ThinkingRecord {
                fingerprint: "fp_test_cross".to_string(),
                thought: "Thought generated by Gemini".to_string(),
                signature: Some(gemini_sig.to_string()),
                tool_ids: vec![],
                tool_names: vec![],
                visible: "Gemini visible text".to_string(),
            },
        );

        // 客户端在同一 session 下切换模型为 Claude 发起后续对话
        let mut contents = vec![
            json!({ "role": "user", "parts": [{ "text": "Hello" }] }),
            json!({
                "role": "model",
                "parts": [{ "text": "Gemini visible text" }]
            }),
            json!({ "role": "user", "parts": [{ "text": "Next turn" }] }),
        ];

        // 1. 执行 restore（针对目标模型 claude-opus-4-6-thinking）
        let restored = store.restore_gemini_contents_with_model(
            key,
            &mut contents,
            Some("claude-opus-4-6-thinking"),
        );
        assert_eq!(restored, 1);

        // 恢复出的 thinking 块绝不能挂载 Gemini 签名！
        let parts_after_restore = contents[1]["parts"].as_array().unwrap();
        assert!(
            parts_after_restore[0].get("thoughtSignature").is_none(),
            "ThinkingStore must not assign foreign Gemini signature when target is Claude"
        );

        // 2. 执行 finalize（终审节点）
        finalize_gemini_contents_thinking_with_model(
            &mut contents,
            true,
            Some("claude-opus-4-6-thinking"),
        );

        // Claude 历史保留思考正文。签名不挂在思考块上。
        let final_parts = contents[1]["parts"].as_array().unwrap();
        let thought_has_sig = final_parts.iter().any(|p| {
            p.get("thought").and_then(|v| v.as_bool()) == Some(true)
                && p.get("thoughtSignature").is_some()
        });
        assert!(
            !thought_has_sig,
            "Claude thought parts do not carry thoughtSignature"
        );
        assert!(
            final_parts.iter().any(|p| p
                .get("text")
                .and_then(|t| t.as_str())
                .map_or(false, |s| s.contains("Thought generated by Gemini"))),
            "Original thinking content must be safely preserved as text in conversation history"
        );
    }

    #[test]
    fn test_finalize_thinking_disabled_preserves_historical_thinking_and_signatures() {
        let real_sig = "Ep4MCpsMARFNMg9NDlK9RXXz5Mzq9mniX9KSQBBzbUx3k85w/qDgtcE+28NH+1EvPeULAprqUquvYXGMzUXGy1xJoMnqdkC4vqebuhyd2Xhs0oz+OhqcOTwLhGYOG0KBKQ87Hfw4q/sMCSgf2gz4vFMa6V6kKMJepYlPXKFJJF4ok+W6lUt3PfYln8K9Dh7wB/40iHiZ2BnJd++6hfUwu9Bz1n795S50l0yCj84EaSCDDF334Erxq7Fo";
        // 模拟多轮对话：
        // Turn 1: User
        // Turn 2: Model (历史已发生，包含合法思考块与工具调用及签名)
        // Turn 3: User (工具返回)
        // Turn 4: User (最新一轮用户提问，并且在这一轮用户把思考开关关闭了)
        let mut contents = vec![
            json!({
                "role": "user",
                "parts": [{ "text": "Run bash command" }]
            }),
            json!({
                "role": "model",
                "parts": [
                    {
                        "text": "Planning to run ls...",
                        "thought": true
                    },
                    {
                        "functionCall": {
                            "name": "bash",
                            "args": { "command": "ls" }
                        },
                        "thoughtSignature": real_sig
                    }
                ]
            }),
            json!({
                "role": "user",
                "parts": [{
                    "functionResponse": {
                        "name": "bash",
                        "response": { "output": "file1.txt" }
                    }
                }]
            }),
            json!({
                "role": "user",
                "parts": [{ "text": "Now answer without thinking" }]
            }),
        ];

        // 最新一轮关思考 (is_thinking_enabled = false)
        finalize_gemini_contents_thinking_with_model(
            &mut contents,
            false,
            Some("gemini-3.8-flash-tiered"),
        );

        // 验证历史 Turn 2 的思考块与工具调用签名是否被完好保留：
        let history_model_parts = contents[1]["parts"].as_array().expect("parts array");
        assert_eq!(history_model_parts.len(), 2);
        assert_eq!(history_model_parts[0]["text"], "Planning to run ls...");
        assert_eq!(history_model_parts[1]["thoughtSignature"], real_sig, "Historical tool thoughtSignature must NOT be stripped when thinking is disabled in the current turn");
    }

    #[test]
    fn test_finalize_thinking_disabled_keeps_unsigned_function_call_unsigned_for_gemini() {
        let mut contents = vec![json!({
            "role": "model",
            "parts": [
                {
                    "functionCall": {
                        "name": "grep",
                        "id": "call_unsigned",
                        "args": { "pattern": "abc" }
                    }
                }
            ]
        })];

        // 关思考出站，发往 Gemini
        finalize_gemini_contents_thinking_with_model(
            &mut contents,
            false,
            Some("gemini-3.8-flash-tiered"),
        );

        // 官方报文里哨兵出现 0 次；缺失签名被上游容忍（在飞轮即缺席）。
        // 因此无签名的 functionCall 保持「字段缺席」，绝不发明占位符。
        let parts = contents[0]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 1);
        assert!(
            parts[0].get("thoughtSignature").is_none()
                && parts[0].get("thought_signature").is_none(),
            "Unsigned functionCall must stay unsigned when sent to Gemini — never invent a sentinel"
        );
    }

    #[test]
    fn test_finalize_does_not_invent_signature_for_unsigned_function_call() {
        let tool_id = "call_finalize_cache_777";
        let mut contents = vec![json!({
            "role": "model",
            "parts": [
                {
                    "functionCall": {
                        "name": "run_command",
                        "id": tool_id,
                        "args": { "cmd": "cargo test" }
                    }
                    // 注意：未带 thoughtSignature（模拟未在进站流水线查到的工具调用）
                }
            ]
        })];

        // 终审出站，发往 Gemini
        finalize_gemini_contents_thinking_with_model(
            &mut contents,
            true,
            Some("gemini-3.8-flash-tiered"),
        );

        let parts = contents[0]["parts"].as_array().unwrap();
        // 终审门禁：不查库、不发明签名。官方 functionCall 轮是"纯净"的
        // （33 个 model 轮里 thought × functionCall 共现 0 次），因此不得注入占位思考块。
        assert_eq!(
            parts.len(),
            1,
            "finalize must NOT inject a placeholder thought block"
        );
        let fc = parts
            .iter()
            .find(|p| p.get("functionCall").is_some())
            .unwrap();
        assert!(
            fc.get("thoughtSignature").is_none(),
            "Pipeline finalize must not invent a sentinel; absence is tolerated by upstream"
        );
    }

    #[test]
    fn test_finalize_thinking_disabled_strips_tool_signature_for_claude() {
        let real_sig = "s".repeat(60);
        let mut contents = vec![json!({
            "role": "model",
            "parts": [
                {
                    "functionCall": {
                        "name": "grep",
                        "id": "call_123",
                        "args": {}
                    },
                    "thoughtSignature": real_sig
                }
            ]
        })];

        // 关思考出站，发往 Claude
        finalize_gemini_contents_thinking_with_model(
            &mut contents,
            false,
            Some("claude-3-7-sonnet"),
        );

        let parts = contents[0]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(
            parts[0]["thoughtSignature"], real_sig,
            "Claude tool-call signature stays on the functionCall"
        );
    }

    #[test]
    fn test_restore_deduplicates_and_elevates_thought_from_plain_text() {
        let store = ThinkingStore::new();
        let key = "t:re-elevation-dedup-test";
        let tool_id = "call_dedup_001";
        let real_sig = "Ep4MCpsMARFNMg9NDlK9RXXz5Mzq9mniX9KSQBBzbUx3k85w/qDgtcE+28NH+1EvPeULAprqUquvYXGMzUXGy1xJoMnqdkC4vqebuhyd2Xhs0oz+OhqcOTwLhGYOG0KBKQ87Hfw4q/sMCSgf2gz4vFMa6V6kKMJepYlPXKFJJF4ok+W6lUt3PfYln8K9Dh7wB/40iHiZ2BnJd++6hfUwu9Bz1n795S50l0yCj84EaSCDDF334Erxq7Fo";
        let thought_text =
            "I am analyzing the repository carefully and formulating a search query.";
        let visible_answer = "Here is the exact file path you requested.";

        // 1. 模拟旧轮次入库（开思考时发生的思维记录）
        let _ = crate::modules::proxy_db::save_thinking_record(
            key,
            "fp_dedup_001",
            thought_text,
            Some(real_sig),
            &[tool_id.to_string()],
            &["grep".to_string()],
            visible_answer,
        );

        // 2. 模拟客户端发上来的历史：之前关思考时降级的思考文本变成了普通正文部件，残留或与正文拼接在一起
        let mut contents = vec![
            json!({
                "role": "user",
                "parts": [{ "text": "Find the file" }]
            }),
            json!({
                "role": "model",
                "parts": [
                    { "text": thought_text }, // 降级残留的普通文本部件
                    { "text": visible_answer },
                    {
                        "functionCall": {
                            "name": "grep",
                            "id": tool_id,
                            "args": {}
                        }
                    }
                ]
            }),
            json!({
                "role": "user",
                "parts": [{ "functionResponse": { "name": "grep", "response": { "res": "ok" } } }]
            }),
        ];

        // 3. 当前轮次重新开启思考，执行复活与重提升
        let restored = store.restore_gemini_contents_with_model(
            key,
            &mut contents,
            Some("gemini-3.8-flash-tiered"),
        );
        assert_eq!(restored, 1, "Must restore exactly 1 turn");

        let model_parts = contents[1]["parts"].as_array().expect("parts array");
        // 验证：
        // 1. 首位成功提升恢复为真正的思考块 (thought: true)
        assert_eq!(model_parts[0]["thought"], true);
        assert_eq!(model_parts[0]["text"], thought_text);

        // 2. 正文部件中的降级残留部件被精准剔除，只保留真实的可见回答，消灭双份复读！
        assert_eq!(model_parts[1]["text"], visible_answer);
        assert!(model_parts[1].get("thought").is_none());

        // 3. 工具调用挂载真实签名
        assert_eq!(model_parts[2]["functionCall"]["id"], tool_id);
        assert_eq!(model_parts[2]["thoughtSignature"], real_sig);

        let _ = crate::modules::proxy_db::delete_thinking_records_for_session(key);
    }

    #[test]
    fn test_gemini_to_claude_wraps_foreign_thought_in_think_tags_and_strips_tool_sig() {
        let gemini_sig = "Ep4MCpsMARFNMg9NDlK9RXXz5Mzq9mniX9KSQBBzbUx3k85w/qDgtcE+28NH+1EvPeULAprqUquvYXGMzUXGy1xJoMnqdkC4vqebuhyd2Xhs0oz+OhqcOTwLhGYOG0KBKQ87Hfw4q/sMCSgf2gz4vFMa6V6kKMJepYlPXKFJJF4ok+W6lUt3PfYln8K9Dh7wB/40iHiZ2BnJd++6hfUwu9Bz1n795S50l0yCj84EaSCDDF334Erxq7Fo";
        let thought_text = "Step 1: Check database schema. Step 2: Query tables.";
        let visible_answer = "Found 3 matching records in the database.";

        let mut contents = vec![json!({
            "role": "model",
            "parts": [
                {
                    "text": thought_text,
                    "thought": true,
                    "thoughtSignature": gemini_sig
                },
                {
                    "text": visible_answer
                },
                {
                    "functionCall": {
                        "name": "query_db",
                        "id": "call_db_1",
                        "args": {}
                    },
                    "thoughtSignature": gemini_sig
                }
            ]
        })];

        // 目标模型切为 Claude
        finalize_gemini_contents_thinking_with_model(
            &mut contents,
            true,
            Some("claude-3-7-sonnet"),
        );

        let parts = contents[0]["parts"].as_array().expect("parts array");
        assert_eq!(parts[0]["thought"], true);
        assert_eq!(parts[0]["text"], thought_text);
        assert!(parts[0].get("thoughtSignature").is_none());
        assert_eq!(parts[1]["text"], visible_answer);
        assert_eq!(parts[1]["thoughtSignature"], gemini_sig);
        assert_eq!(parts[2]["functionCall"]["id"], "call_db_1");
        assert!(parts[2].get("thoughtSignature").is_none());
    }

    #[test]
    fn test_extract_think_tags_helper() {
        let input =
            "<think>\nThinking line 1\nThinking line 2\n</think>\n\nFinal response text here.";
        let res = extract_think_tags(input);
        assert!(res.is_some());
        let (thought, visible) = res.unwrap();
        assert_eq!(thought, "Thinking line 1\nThinking line 2");
        assert_eq!(visible, "Final response text here.");

        // No tags
        assert!(extract_think_tags("Just normal text").is_none());
    }

    #[test]
    fn test_gemini_native_synthetic_id_bridges_cross_protocol_signature_recovery() {
        let store = ThinkingStore::new();
        let key = "t:cross-proto-synthetic-id-test";
        let client_tool_id = "call_openai_native_456";
        let real_sig = "EmIKYAFpFH0TDqviLY1vZ8EuHqBLLj5xxD+0hchYg2VaoyolUQRP+hSCsKRpSpj+yrQA2H27yVFnF7tlp5OHIUvTdZKKErAqILJzK5FG8RJg42jCaaI2/iwqoBuRd5BDVwBxaQ==";
        let thought_text = "Analyzing directory and listing files.";
        let _visible_answer = "Running bash tool.";

        // 1. 模拟 OpenAI 协议下生成的工具调用（带有 client_tool_id）
        let user_turn = json!({
            "role": "user",
            "parts": [{ "text": "List the files" }]
        });
        let anchor = compute_causal_anchor(Some(&user_turn));
        let mut acc = TurnAccumulator::with_anchor(&anchor);
        acc.ingest_part(&json!({
            "thought": true,
            "text": thought_text,
            "thoughtSignature": real_sig
        }));
        acc.ingest_part(&json!({
            "functionCall": {
                "name": "bash",
                "args": { "command": "ls -la" },
                "id": client_tool_id
            },
            "thoughtSignature": real_sig
        }));
        acc.commit(key);

        // 2. 模拟用户切换到了原生 Gemini 协议发起后续对话：
        // 原生 Gemini 请求的 contents 中天生没有 id 字段（id: None）！
        let mut gemini_contents = vec![
            user_turn,
            json!({
                "role": "model",
                "parts": [
                    {
                        "functionCall": {
                            "name": "bash",
                            "args": { "command": "ls -la" }
                            // 注意：完全无 id 字段！
                        }
                    }
                ]
            }),
            json!({
                "role": "user",
                "parts": [{ "functionResponse": { "name": "bash", "response": { "res": "file.txt" } } }]
            }),
        ];

        // 3. 原生 Gemini 协议执行流水线复活与终审出站
        let restored = store.restore_gemini_contents_with_model(
            key,
            &mut gemini_contents,
            Some("gemini-3.8-flash-tiered"),
        );
        assert_eq!(
            restored, 1,
            "Must match via synthetic context ID even though Gemini request had no tool id"
        );

        finalize_gemini_contents_thinking_with_model(
            &mut gemini_contents,
            true,
            Some("gemini-3.8-flash-tiered"),
        );

        let model_parts = gemini_contents[1]["parts"].as_array().expect("parts");
        // 验证思考块被成功提升复活：
        assert_eq!(model_parts[0]["thought"], true);
        assert_eq!(model_parts[0]["text"], thought_text);

        // 验证跨协议捕获到的真实签名被归位到锚点，而不是被替换成哨兵：
        let fc = &model_parts[1];
        assert_eq!(fc["functionCall"]["name"], "bash");
        assert_eq!(
            fc["thoughtSignature"], real_sig,
            "Captured real signature must be restored onto the anchor, not replaced by a sentinel"
        );
        // 验证伪 ID 纯粹内部使用，绝不外泄给无 ID 协议：
        assert!(
            fc["functionCall"].get("id").is_none(),
            "Synthetic ID must remain internal and not leak to client"
        );

        let _ = crate::modules::proxy_db::delete_thinking_records_for_session(key);
    }
}

/// 官方报文对齐回归测试：五种 part 排列下签名锚点必须与真机一致。
///
/// 依据 3 份官方 Antigravity 报文、23 处真实签名归纳出的不变量
/// （见 `.workbuddy/outputs/correct-assembly-spec.md`）：
///   1. 签名只出现在 model 轮，每轮至多 1 个；
///   2. 锚点 = 该轮第一个 `thought != true` 的 part；
///   3. `thought: true` / `functionResponse` / 非锚点 —— 字段必须「缺席」；
///   4. 缺失签名被上游容忍，绝不发明哨兵。
#[cfg(test)]
mod signature_placement_tests {
    use super::*;

    /// 构造符合 Google 原生特征的假签名（Base64 解码后首字节 = protobuf tag 0x12）
    fn gemini_sig(seed: u8) -> String {
        use base64::Engine;
        let mut raw = vec![0x12u8, seed];
        raw.extend_from_slice(&[b'A'; 60]);
        base64::engine::general_purpose::STANDARD.encode(raw)
    }

    #[test]
    fn is_thought_part_only_accepts_explicit_flag() {
        // 带签名但没有 thought 标志的正文 —— 绝不能判为思考块（旧启发式的核心错误）
        assert!(!is_thought_part(
            &json!({ "text": "answer", "thoughtSignature": "Eabc" })
        ));
        // 带 functionCall 也不算
        assert!(!is_thought_part(
            &json!({ "functionCall": { "name": "x" }, "thoughtSignature": "Eabc" })
        ));
        // 只有显式 thought: true 才算
        assert!(is_thought_part(
            &json!({ "text": "reasoning", "thought": true })
        ));
        // 显式 false 不算
        assert!(!is_thought_part(&json!({ "text": "x", "thought": false })));
    }

    #[test]
    fn arrangement_a_thought_plus_text_keeps_signature_on_text() {
        // 官方 f81eae5c contents[1] 的排列
        let sig = gemini_sig(1);
        let mut parts = vec![
            json!({ "text": "reasoning", "thought": true }),
            json!({ "text": "visible answer", "thoughtSignature": sig }),
        ];
        let placed = place_turn_signature(&mut parts, None);

        assert_eq!(placed.as_deref(), Some(sig.as_str()));
        assert_eq!(parts.len(), 2, "绝不能重排或增删 part");
        assert!(
            parts[0].get("thoughtSignature").is_none(),
            "思考块绝不带签名"
        );
        assert_eq!(parts[1]["thoughtSignature"], sig, "锚点 = parts[1]");
    }

    #[test]
    fn arrangement_b_parallel_calls_signs_only_the_first() {
        // 官方 f81eae5c contents[3]：3 个并发 functionCall，签名只在 parts[0]
        let sig = gemini_sig(2);
        let mut parts = vec![
            json!({ "functionCall": { "id": "a", "name": "run_command" }, "thoughtSignature": sig }),
            json!({ "functionCall": { "id": "b", "name": "view_file" } }),
            json!({ "functionCall": { "id": "c", "name": "view_file" } }),
        ];
        place_turn_signature(&mut parts, None);

        assert_eq!(parts.len(), 3, "绝不插入假思考块");
        assert_eq!(parts[0]["thoughtSignature"], sig);
        assert!(
            parts[1].get("thoughtSignature").is_none(),
            "字段必须缺席而非空串"
        );
        assert!(parts[2].get("thoughtSignature").is_none());
    }

    #[test]
    fn arrangement_c_function_responses_never_signed() {
        let sig = gemini_sig(3);
        let mut parts = vec![
            json!({ "functionResponse": { "id": "a", "name": "x" } }),
            json!({ "functionResponse": { "id": "b", "name": "y" } }),
        ];
        // 即便提供回填来源，也绝不写入 functionResponse
        let placed = place_turn_signature(&mut parts, Some(sig.as_str()));

        assert!(placed.is_none());
        assert!(parts[0].get("thoughtSignature").is_none());
        assert!(parts[1].get("thoughtSignature").is_none());
    }

    #[test]
    fn arrangement_d_pure_text_keeps_its_own_signature() {
        let sig = gemini_sig(4);
        let mut parts = vec![json!({ "text": "hello", "thoughtSignature": sig })];
        let placed = place_turn_signature(&mut parts, None);

        assert_eq!(placed.as_deref(), Some(sig.as_str()));
        assert_eq!(parts[0]["thoughtSignature"], sig);
    }

    #[test]
    fn unsigned_anchor_stays_absent_when_no_source_available() {
        // 官方 baogao.txt contents[17]：在飞 functionCall 就是无签名的
        let mut parts = vec![json!({ "functionCall": { "id": "call_x", "name": "bash" } })];
        let placed = place_turn_signature(&mut parts, None);

        assert!(placed.is_none());
        assert!(
            parts[0].get("thoughtSignature").is_none(),
            "绝不发明哨兵，官方报文里哨兵出现 0 次"
        );
    }

    #[test]
    fn fallback_signature_is_placed_on_anchor() {
        // 跨协议路径：锚点无签名，但库里有真实签名
        let sig = gemini_sig(5);
        let mut parts = vec![json!({ "functionCall": { "id": "call_x", "name": "bash" } })];
        let placed = place_turn_signature(&mut parts, Some(sig.as_str()));

        assert_eq!(placed.as_deref(), Some(sig.as_str()));
        assert_eq!(parts[0]["thoughtSignature"], sig);
    }

    #[test]
    fn sentinel_is_never_accepted_as_a_source() {
        let mut parts = vec![json!({ "functionCall": { "id": "call_x", "name": "bash" } })];
        let placed = place_turn_signature(&mut parts, Some(SENTINEL_SIGNATURE));

        assert!(placed.is_none(), "哨兵不是合法回填来源");
        assert!(parts[0].get("thoughtSignature").is_none());
    }

    #[test]
    fn non_anchor_signatures_are_purged_to_absence() {
        let sig_a = gemini_sig(6);
        let sig_b = gemini_sig(7);
        let mut parts = vec![
            json!({ "functionCall": { "id": "a", "name": "x" }, "thoughtSignature": sig_a }),
            json!({ "functionCall": { "id": "b", "name": "y" }, "thoughtSignature": sig_b }),
            json!({ "text": "trailing", "thought_signature": sig_b }),
        ];
        place_turn_signature(&mut parts, None);

        assert_eq!(parts[0]["thoughtSignature"], sig_a, "锚点保留自己的真签名");
        assert!(parts[1].get("thoughtSignature").is_none());
        assert!(parts[2].get("thoughtSignature").is_none());
        assert!(
            parts[2].get("thought_signature").is_none(),
            "蛇形字段必须被清除"
        );
    }

    #[test]
    fn all_thought_turn_has_no_anchor() {
        let sig = gemini_sig(8);
        let mut parts = vec![
            json!({ "text": "r1", "thought": true }),
            json!({ "text": "r2", "thought": true }),
        ];
        let placed = place_turn_signature(&mut parts, Some(sig.as_str()));

        assert!(placed.is_none(), "整轮皆思考则本轮无锚点");
        assert!(parts[0].get("thoughtSignature").is_none());
        assert!(parts[1].get("thoughtSignature").is_none());
    }

    #[test]
    fn arrangement_f_tool_call_with_placeholder_text_anchors_to_function_call() {
        // 现场 400 铁证重现：客户端发送带有占位文本 "..." 与 tool_use 的 assistant 轮次，
        // 签名最初挂在占位文本 "..." 上。
        // 正确产出：丢弃占位文本 "..."，将真实签名救援并归位到首个 functionCall 上！
        let sig = gemini_sig(10);
        let mut parts = vec![
            json!({ "text": "Thinking deeply...", "thought": true }),
            json!({ "text": "...", "thoughtSignature": sig.clone() }),
            json!({ "functionCall": { "id": "call_2594022", "name": "exec", "args": { "command": "dws auth" } } }),
        ];
        let placed = place_turn_signature(&mut parts, None);

        assert_eq!(placed.as_deref(), Some(sig.as_str()));
        // 占位文本 "..." 必须被清理，parts 仅保留思考块与 functionCall
        assert_eq!(parts.len(), 2, "占位文本部件必须被丢弃");
        assert_eq!(parts[0]["thought"], true);
        assert!(parts[0].get("thoughtSignature").is_none());
        assert_eq!(parts[1]["functionCall"]["name"], "exec");
        assert_eq!(
            parts[1]["thoughtSignature"], sig,
            "签名必须正确归位到 functionCall 锚点"
        );
    }

    #[test]
    fn arrangement_g_tool_call_with_commentary_anchors_to_function_call() {
        // 当 assistant 轮次同时包含真实正文说明与工具调用时，
        // Google Gemini 强校验规范要求签名必须落在 functionCall 部件上，正文不得带签名
        let sig = gemini_sig(11);
        let mut parts = vec![
            json!({ "text": "I will run this tool for you" }),
            json!({ "functionCall": { "id": "call_999", "name": "read_file" } }),
        ];
        let placed = place_turn_signature(&mut parts, Some(sig.as_str()));

        assert_eq!(placed.as_deref(), Some(sig.as_str()));
        assert_eq!(parts.len(), 2, "真实正文与工具调用均予保留");
        assert!(
            parts[0].get("thoughtSignature").is_none(),
            "正文不得携带签名"
        );
        assert_eq!(
            parts[1]["thoughtSignature"], sig,
            "签名必须落在 functionCall 上"
        );
    }

    #[test]
    fn test_extract_turn_signature_prioritizes_function_call_for_gemini() {
        // [Pipeline First] 测试混合轮次中签名提取优先级：
        // Gemini 目标下，若 functionCall 带有签名，必须强制优先提取 functionCall 上的签名，
        // 杜绝前面紧随的说明正文（text）抢走签名导致工具调用凭据失效。
        let text_sig = gemini_sig(101);
        let fc_sig = gemini_sig(102);

        let parts = vec![
            json!({ "text": "Analyzing the request...", "thoughtSignature": text_sig }),
            json!({ "functionCall": { "id": "call_test", "name": "read" }, "thoughtSignature": fc_sig }),
        ];

        // 1. Gemini (prefer_function_call = true) -> 优先提取 functionCall 的签名 fc_sig
        let gemini_extracted = extract_turn_signature(&parts, true, false);
        assert_eq!(gemini_extracted.as_deref(), Some(fc_sig.as_str()));

        // 2. Claude (prefer_function_call = false) -> 提取首个非思考部件的签名 text_sig
        let claude_sig_str = "AQ".to_string() + &"A".repeat(50);
        let claude_parts = vec![
            json!({ "text": "Analyzing...", "thoughtSignature": claude_sig_str }),
            json!({ "functionCall": { "id": "call_test", "name": "read" } }),
        ];
        let claude_extracted = extract_turn_signature(&claude_parts, false, true);
        assert_eq!(claude_extracted.as_deref(), Some(claude_sig_str.as_str()));
    }

    #[test]
    fn test_place_turn_signature_preserves_valid_signatures_on_parallel_tools() {
        // [Pipeline First] 并发工具调用场景：
        // 若第二个 functionCall 自带有效签名，Gemini 目标下绝不能将其粗暴剥离为缺席！
        let sig1 = gemini_sig(201);
        let sig2 = gemini_sig(202);

        let mut parts = vec![
            json!({ "functionCall": { "id": "call_1", "name": "read_file" }, "thoughtSignature": sig1 }),
            json!({ "functionCall": { "id": "call_2", "name": "edit_file" }, "thoughtSignature": sig2 }),
        ];

        place_turn_signature(&mut parts, None);

        assert_eq!(
            parts[0]["thoughtSignature"], sig1,
            "首个工具调用保持自身签名"
        );
        assert_eq!(
            parts[1]["thoughtSignature"], sig2,
            "并发的第二个工具调用有效签名绝不被误删"
        );
    }

    #[test]
    fn test_find_turn_anchor_logic() {
        // 1. 纯思考块 -> 无锚点
        assert_eq!(
            find_turn_anchor(&[json!({ "thought": true, "text": "t" })]),
            None
        );
        // 2. 思考块 + 正文 -> 锚点为正文
        assert_eq!(
            find_turn_anchor(&[
                json!({ "thought": true, "text": "t" }),
                json!({ "text": "answer" })
            ]),
            Some(1)
        );
        // 3. 正文 + 工具 -> 锚点必须优先为工具调用！
        assert_eq!(
            find_turn_anchor(&[
                json!({ "text": "commentary" }),
                json!({ "functionCall": { "name": "bash" } })
            ]),
            Some(1)
        );
        // 4. 并发工具 -> 锚点为首个工具调用
        assert_eq!(
            find_turn_anchor(&[
                json!({ "functionCall": { "name": "a" } }),
                json!({ "functionCall": { "name": "b" } })
            ]),
            Some(0)
        );
    }

    #[test]
    fn test_tool_id_normalization_in_restore() {
        let store = ThinkingStore::new();
        let session_key = "test_norm_tool_id_session";
        let sig = gemini_sig(9);

        // Record saved with canonical call_573077
        // Record saved with canonical call_573077
        store.record(
            session_key,
            ThinkingRecord {
                fingerprint: "fp1".to_string(),
                thought: "I need to read this file".to_string(),
                signature: Some(sig.clone()),
                tool_ids: vec!["call_573077".to_string()],
                tool_names: vec!["default_api:read".to_string()],
                visible: String::new(),
            },
        );

        // Incoming client contents stripped underscore: call573077
        let mut contents = vec![
            json!({
                "role": "user",
                "parts": [{ "text": "read the file" }]
            }),
            json!({
                "role": "model",
                "parts": [{
                    "functionCall": {
                        "name": "default_api:read",
                        "id": "call573077",
                        "args": { "path": "test.txt" }
                    }
                }]
            }),
        ];

        let count = store.restore_gemini_contents_with_model(
            session_key,
            &mut contents,
            Some("gemini-2.5-flash"),
        );
        assert_eq!(count, 1);
        let model_parts = contents[1]["parts"].as_array().unwrap();
        // Signature should be attached to the anchor (functionCall at index 1)
        assert_eq!(model_parts[1]["thoughtSignature"], sig);
    }

    #[test]
    fn test_client_invalid_signature_rejected_and_backfilled_with_reverse_storage() {
        let real_sig = gemini_sig(88);
        let mut parts = vec![json!({
            "functionCall": {
                "id": "call_invalid_test_1",
                "name": "default_api:run_command",
                "args": {}
            },
            "thoughtSignature": "not_a_valid_gemini_signature_12345678901234567890"
        })];

        // place_turn_signature 必须拒绝客户端伪造的无效签名，采纳 fallback_sig 并反向入库
        let placed =
            place_turn_signature_scoped(&mut parts, Some(&real_sig), true, Some("test-store"));
        assert_eq!(placed, Some(real_sig.clone()));
        assert_eq!(parts[0]["thoughtSignature"], real_sig);

        // 验证反向入库已生效，并且不落到别的会话。
        let cached = crate::proxy::SignatureCache::global()
            .get_tool_signature("test-store", "call_invalid_test_1");
        assert_eq!(
            crate::proxy::SignatureCache::global()
                .get_tool_signature("other-store", "call_invalid_test_1"),
            None
        );
        assert_eq!(cached, Some(real_sig));
    }

    #[test]
    fn test_sequential_tool_calling_sentinel_fallback() {
        let sig_tool_a = gemini_sig(77);
        let tool_a_id = format!("call_seq_a_{}", uuid::Uuid::new_v4().simple());
        let tool_b_id = format!("call_seq_b_{}", uuid::Uuid::new_v4().simple());
        let mut contents = vec![
            // Turn 0: User prompt
            json!({
                "role": "user",
                "parts": [{ "text": "start task" }]
            }),
            // Turn 1: Model calls Tool A (带合法签名)
            json!({
                "role": "model",
                "parts": [{
                    "functionCall": {
                        "id": tool_a_id.clone(),
                        "name": "default_api:skills_read",
                        "args": {}
                    },
                    "thoughtSignature": sig_tool_a.clone()
                }]
            }),
            // Turn 2: User returns response for Tool A
            json!({
                "role": "model",
                "parts": [{
                    "functionResponse": {
                        "id": tool_a_id,
                        "name": "default_api:skills_read",
                        "response": { "output": "ok" }
                    }
                }]
            }),
            // Turn 3: Model calls Tool B (无客户端签名，无缓存)
            json!({
                "role": "model",
                "parts": [{
                    "functionCall": {
                        "id": tool_b_id,
                        "name": "default_api:skills_read_resource",
                        "args": {}
                    }
                }]
            }),
        ];

        // 终审把关与脱敏规范化
        finalize_gemini_contents_thinking_with_session(
            &mut contents,
            true,
            Some("gemini-2.5-flash"),
            Some("sess-test-seq-1"),
        );

        // 黄金法则（第二种情况）：当本轮确实没有独立签名时，消除哨兵保持缺省，
        // 绝不跨轮继承其他工具调用的不同真实签名！
        let turn3_parts = contents[3]["parts"].as_array().unwrap();
        assert!(turn3_parts[0].get("thoughtSignature").is_none());
    }

    #[test]
    fn test_pure_thinking_signature_pending_and_handover() {
        let sig_thought = gemini_sig(66);
        let mut contents = vec![
            // Turn 0: User prompt
            json!({
                "role": "user",
                "parts": [{ "text": "start thinking task" }]
            }),
            // Turn 1: 纯思考轮 (只有 thought: true，带有效签名)
            json!({
                "role": "model",
                "parts": [{
                    "text": "Deep thinking analysis...",
                    "thought": true,
                    "thoughtSignature": sig_thought.clone()
                }]
            }),
            // Turn 2: 后续工具调用轮 (无签名)
            json!({
                "role": "model",
                "parts": [{
                    "functionCall": {
                        "id": "call_after_thought",
                        "name": "default_api:run_command",
                        "args": {}
                    }
                }]
            }),
        ];

        finalize_gemini_contents_thinking_with_session(
            &mut contents,
            true,
            Some("gemini-2.5-flash"),
            Some("sess-test-pure-1"),
        );

        // 黄金法则 2.2：纯思考轮的思考块绝不挂载签名！
        let turn1_parts = contents[1]["parts"].as_array().unwrap();
        assert!(turn1_parts[0].get("thoughtSignature").is_none());

        // 黄金法则 2.2：纯思考轮攒在手上的签名，成功交接给下一轮的首个工具调用！
        let turn2_parts = contents[2]["parts"].as_array().unwrap();
        assert_eq!(turn2_parts[0]["thoughtSignature"], sig_thought);
    }

    #[test]
    fn test_tool_call_global_priority_over_commentary_and_other_fc() {
        let real_sig = gemini_sig(99);
        let mut contents = vec![
            // Model 轮包含：思考块 + 进度说明正文 + 2个并发工具调用
            json!({
                "role": "model",
                "parts": [
                    { "text": "let me think", "thought": true },
                    { "text": "Executing commands..." },
                    {
                        "functionCall": { "id": "call_fc_1", "name": "run_command", "args": {} },
                        "thoughtSignature": real_sig.clone()
                    },
                    {
                        "functionCall": { "id": "call_fc_2", "name": "view_file", "args": {} },
                        "thoughtSignature": real_sig.clone()
                    }
                ]
            }),
        ];

        finalize_gemini_contents_thinking_with_session(
            &mut contents,
            true,
            Some("gemini-2.5-flash"),
            Some("sess-test-prio-1"),
        );

        let parts = contents[0]["parts"].as_array().unwrap();
        // 1. 思考块必须排在首位 parts[0]，且无签名
        assert_eq!(parts[0]["thought"], true);
        assert!(parts[0].get("thoughtSignature").is_none());

        // 2. 首个工具调用 fc 必须全局抢夺签名（parts[1] 或首个 fc）
        let fc1 = parts
            .iter()
            .find(|p| {
                p.get("functionCall")
                    .and_then(|fc| fc.get("id"))
                    .and_then(|id| id.as_str())
                    == Some("call_fc_1")
            })
            .unwrap();
        assert_eq!(fc1["thoughtSignature"], real_sig);

        // 3. 伴随正文 text 绝不携带签名
        let text_part = parts
            .iter()
            .find(|p| p.get("text").is_some() && p.get("thought").is_none())
            .unwrap();
        assert!(text_part.get("thoughtSignature").is_none());

        // 4. 并发的其余工具调用绝不携带签名
        let fc2 = parts
            .iter()
            .find(|p| {
                p.get("functionCall")
                    .and_then(|fc| fc.get("id"))
                    .and_then(|id| id.as_str())
                    == Some("call_fc_2")
            })
            .unwrap();
        assert!(fc2.get("thoughtSignature").is_none());
    }
}
