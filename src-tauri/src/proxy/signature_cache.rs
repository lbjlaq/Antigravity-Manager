use dashmap::DashMap;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{
    sync_channel, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError,
};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

// Node.js proxy uses 2 hours TTL
const SIGNATURE_TTL: Duration = Duration::from_secs(2 * 60 * 60);
const MIN_SIGNATURE_LENGTH: usize = 32;

// Different cache limits for different layers
const FAMILY_CACHE_LIMIT: usize = 200; // Layer 2: Model family mappings
const SESSION_CACHE_LIMIT: usize = 1000; // Layer 3: Session-based signatures (largest)
const TOOL_DELETE_QUEUE_CAP: usize = 1024;
const SESSION_FLOOR_TTL: Duration = Duration::from_secs(3600);
const TOOL_TOUCH_INTERVAL: Duration = Duration::from_secs(24 * 3600);

#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub struct SignatureCacheStats {
    pub resolve_calls: u64,
    pub l1_hits: u64,
    pub l2_hits: u64,
    pub misses: u64,
    pub batch_calls: u64,
    pub batch_ids: u64,
    pub saves_enqueued: u64,
    pub saves_dropped: u64,
    pub touches_enqueued: u64,
    pub resolve_ns: u64,
    pub l1_entries: usize,
    pub pending_deletes: usize,
    pub session_floors: usize,
}

struct SignatureCacheMetrics {
    resolve_calls: AtomicU64,
    l1_hits: AtomicU64,
    l2_hits: AtomicU64,
    misses: AtomicU64,
    batch_calls: AtomicU64,
    batch_ids: AtomicU64,
    saves_enqueued: AtomicU64,
    saves_dropped: AtomicU64,
    touches_enqueued: AtomicU64,
    resolve_ns: AtomicU64,
}

impl SignatureCacheMetrics {
    const fn new() -> Self {
        Self {
            resolve_calls: AtomicU64::new(0),
            l1_hits: AtomicU64::new(0),
            l2_hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            batch_calls: AtomicU64::new(0),
            batch_ids: AtomicU64::new(0),
            saves_enqueued: AtomicU64::new(0),
            saves_dropped: AtomicU64::new(0),
            touches_enqueued: AtomicU64::new(0),
            resolve_ns: AtomicU64::new(0),
        }
    }

    fn record_resolve(&self, l1_hit: bool, l2_hit: bool, elapsed: Duration) {
        self.resolve_calls.fetch_add(1, Ordering::Relaxed);
        if l1_hit {
            self.l1_hits.fetch_add(1, Ordering::Relaxed);
        } else if l2_hit {
            self.l2_hits.fetch_add(1, Ordering::Relaxed);
        } else {
            self.misses.fetch_add(1, Ordering::Relaxed);
        }
        self.resolve_ns.fetch_add(
            elapsed.as_nanos().min(u64::MAX as u128) as u64,
            Ordering::Relaxed,
        );
    }
}

static SIGNATURE_CACHE_METRICS: SignatureCacheMetrics = SignatureCacheMetrics::new();

pub fn signature_cache_stats() -> SignatureCacheStats {
    let pending_deletes = PENDING_SESSION_DELETES
        .get()
        .map(|map| map.iter().filter(|e| *e.value() > 0).count())
        .unwrap_or(0);
    let session_floors = SESSION_MIN_PERSIST_SEQ
        .get()
        .and_then(|slot| slot.lock().ok())
        .map(|map| map.len())
        .unwrap_or(0);
    SignatureCacheStats {
        resolve_calls: SIGNATURE_CACHE_METRICS
            .resolve_calls
            .load(Ordering::Relaxed),
        l1_hits: SIGNATURE_CACHE_METRICS.l1_hits.load(Ordering::Relaxed),
        l2_hits: SIGNATURE_CACHE_METRICS.l2_hits.load(Ordering::Relaxed),
        misses: SIGNATURE_CACHE_METRICS.misses.load(Ordering::Relaxed),
        batch_calls: SIGNATURE_CACHE_METRICS.batch_calls.load(Ordering::Relaxed),
        batch_ids: SIGNATURE_CACHE_METRICS.batch_ids.load(Ordering::Relaxed),
        saves_enqueued: SIGNATURE_CACHE_METRICS
            .saves_enqueued
            .load(Ordering::Relaxed),
        saves_dropped: SIGNATURE_CACHE_METRICS
            .saves_dropped
            .load(Ordering::Relaxed),
        touches_enqueued: SIGNATURE_CACHE_METRICS
            .touches_enqueued
            .load(Ordering::Relaxed),
        resolve_ns: SIGNATURE_CACHE_METRICS.resolve_ns.load(Ordering::Relaxed),
        l1_entries: SignatureCache::global().tool_signatures.len(),
        pending_deletes,
        session_floors,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PersistMode {
    #[cfg(test)]
    MemoryOnly,
    #[allow(dead_code)]
    Sync,
    Async,
}

#[derive(Clone, Debug)]
enum ToolSigCacheValue {
    Present(String),
    Missing { global_epoch: u64, key_epoch: u64 },
}

enum ToolSigPersistCmd {
    Save {
        key: String,
        signature: String,
        seq: u64,
    },
    DeleteSession {
        session_id: String,
    },
    Touch {
        key: String,
        seq: u64,
        at: i64,
    },
    Flush {
        ack: SyncSender<()>,
    },
}

static PERSIST_SEQ: AtomicU64 = AtomicU64::new(1);
static GLOBAL_MIN_PERSIST_SEQ: AtomicU64 = AtomicU64::new(0);
static SESSION_MIN_PERSIST_SEQ: OnceLock<Mutex<HashMap<String, (u64, Instant)>>> = OnceLock::new();
static TOOL_TOUCH_THROTTLE: OnceLock<DashMap<String, Instant>> = OnceLock::new();
static DIRTY_SAVES: OnceLock<Mutex<HashMap<String, (String, u64)>>> = OnceLock::new();
static PERSIST_RETRY_AFTER: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
const DIRTY_SAVE_CAP: usize = 50_000;
static PENDING_SESSION_DELETES: OnceLock<DashMap<String, usize>> = OnceLock::new();
struct PersistSenders {
    save: SyncSender<ToolSigPersistCmd>,
    delete: SyncSender<ToolSigPersistCmd>,
}

static TOOL_SIG_PERSIST_TX: OnceLock<PersistSenders> = OnceLock::new();

#[inline]
fn next_persist_seq() -> u64 {
    PERSIST_SEQ.fetch_add(1, Ordering::AcqRel)
}

fn matches_scoped_session(scoped_key: &str, session_id: &str) -> bool {
    let scoped_prefix = format!("{session_id}\u{1f}");
    let scoped_suffix_prefix = format!(":{session_id}\u{1f}");
    scoped_key.starts_with(&scoped_prefix) || scoped_key.contains(&scoped_suffix_prefix)
}

fn mark_pending_session_delete(session_id: &str) {
    let map = PENDING_SESSION_DELETES.get_or_init(DashMap::new);
    *map.entry(session_id.to_string()).or_insert(0) += 1;
    if map.len() > 4096 {
        // 只清理已归零的条目；非零计数必须保留，避免与并发 mark 竞争丢计数。
        map.retain(|_, count| *count > 0);
    }
}

fn complete_pending_session_delete(session_id: &str) {
    if let Some(map) = PENDING_SESSION_DELETES.get() {
        if let Some(mut count) = map.get_mut(session_id) {
            // 不在这里 remove：remove 与并发 mark 之间存在丢计数窗口。
            // 归零条目由 mark 的低频 retain 清理。
            *count = (*count).saturating_sub(1);
        }
    }
}

fn touch_is_due(scoped_key: &str) -> bool {
    TOOL_TOUCH_THROTTLE
        .get()
        .and_then(|map| {
            map.get(scoped_key)
                .map(|last| last.elapsed() >= TOOL_TOUCH_INTERVAL)
        })
        .unwrap_or(true)
}

fn mark_touch(scoped_key: &str) {
    let map = TOOL_TOUCH_THROTTLE.get_or_init(DashMap::new);
    map.insert(scoped_key.to_string(), Instant::now());
    if map.len() > crate::proxy::cache_profile().l1_limit * 2 {
        map.retain(|_, at| at.elapsed() < TOOL_TOUCH_INTERVAL);
    }
}

fn flush_touch_batch(batch: &mut HashMap<String, (u64, i64)>) {
    if batch.is_empty() {
        return;
    }
    let entries: Vec<(String, i64)> = batch.drain().map(|(k, (_, at))| (k, at)).collect();
    if let Err(e) = crate::modules::proxy_db::touch_tool_signatures_batch(&entries) {
        tracing::debug!("[SignatureCache] Background tool signature touch skipped: {e}");
    }
}

fn mark_dirty_save(key: String, signature: String, seq: u64) {
    let slot = DIRTY_SAVES.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut map) = slot.lock() {
        if map.len() < DIRTY_SAVE_CAP || map.contains_key(&key) {
            map.insert(key, (signature, seq));
        }
    }
}

fn drain_dirty_saves(batch: &mut HashMap<String, (String, u64)>) {
    let Some(slot) = DIRTY_SAVES.get() else {
        return;
    };
    let Ok(mut map) = slot.lock() else {
        return;
    };
    for (key, (sig, seq)) in map.drain() {
        if is_session_delete_pending_for_scoped_key(&key)
            || !SignatureCache::is_persist_entry_current(&key, seq)
        {
            continue;
        }
        let should_insert = batch
            .get(&key)
            .map(|(_, existing_seq)| seq >= *existing_seq)
            .unwrap_or(true);
        if should_insert {
            batch.insert(key, (sig, seq));
        }
    }
}

fn is_session_delete_pending_for_store(store_key: &str) -> bool {
    let Some(map) = PENDING_SESSION_DELETES.get() else {
        return false;
    };
    if map.get(store_key).is_some_and(|v| *v > 0) {
        return true;
    }
    let mut rest = store_key;
    while let Some((_, tail)) = rest.split_once(':') {
        if map.get(tail).is_some_and(|v| *v > 0) {
            return true;
        }
        rest = tail;
    }
    false
}

fn is_session_delete_pending_for_scoped_key(scoped_key: &str) -> bool {
    let store_key = scoped_key
        .split_once('\u{1f}')
        .map(|(prefix, _)| prefix)
        .unwrap_or(scoped_key);
    is_session_delete_pending_for_store(store_key)
}

fn persist_backoff_active() -> bool {
    PERSIST_RETRY_AFTER
        .get()
        .and_then(|slot| slot.lock().ok())
        .and_then(|until| *until)
        .map(|until| Instant::now() < until)
        .unwrap_or(false)
}

fn set_persist_backoff(active: bool) {
    let slot = PERSIST_RETRY_AFTER.get_or_init(|| Mutex::new(None));
    if let Ok(mut until) = slot.lock() {
        *until = if active {
            Some(Instant::now() + Duration::from_secs(1))
        } else {
            None
        };
    }
}

fn flush_tool_sig_batch(batch: &mut HashMap<String, (String, u64)>) {
    if batch.is_empty() {
        return;
    }
    let entries: Vec<(String, String, u64)> =
        batch.drain().map(|(k, (sig, seq))| (k, sig, seq)).collect();
    if let Err(e) = crate::modules::proxy_db::save_tool_signatures_batch_if_current(&entries) {
        // 写失败不能直接丢弃：回填 dirty，worker 空闲时重试。
        for (key, signature, seq) in entries {
            mark_dirty_save(key, signature, seq);
        }
        set_persist_backoff(true);
        tracing::debug!("[SignatureCache] Background tool signature batch save deferred: {e}");
    } else {
        set_persist_backoff(false);
    }
}

fn handle_delete_session(
    session_id: &str,
    batch: &mut HashMap<String, (String, u64)>,
    touch_batch: &mut HashMap<String, (u64, i64)>,
) {
    touch_batch.retain(|k, _| !matches_scoped_session(k, session_id));
    batch.retain(|k, (_, seq)| {
        !matches_scoped_session(k, session_id) || SignatureCache::is_persist_entry_current(k, *seq)
    });
    flush_touch_batch(touch_batch);
    flush_tool_sig_batch(batch);
    match crate::modules::proxy_db::delete_tool_signatures_for_session(session_id) {
        Ok(_) => complete_pending_session_delete(session_id),
        Err(e) => {
            // 删除失败时保持 pending，宁可暂时读不到，也不要让旧签名复活。
            tracing::warn!(
                "[SignatureCache] Session delete failed for {}: {}; keeping pending guard",
                session_id,
                e
            );
        }
    }
}

fn run_tool_sig_persist_worker(
    save_rx: Receiver<ToolSigPersistCmd>,
    delete_rx: Receiver<ToolSigPersistCmd>,
) {
    let mut batch: HashMap<String, (String, u64)> = HashMap::new();
    let mut touch_batch: HashMap<String, (u64, i64)> = HashMap::new();
    let batch_cap = crate::proxy::cache_profile().batch_cap.max(1);
    loop {
        // 删除/清空优先：每个周期先排空高优先级通道
        while let Ok(cmd) = delete_rx.try_recv() {
            if let ToolSigPersistCmd::DeleteSession { session_id } = cmd {
                handle_delete_session(&session_id, &mut batch, &mut touch_batch);
            }
        }

        let first_cmd = match save_rx.recv_timeout(Duration::from_millis(5)) {
            Ok(cmd) => cmd,
            Err(RecvTimeoutError::Timeout) => {
                if !persist_backoff_active() {
                    drain_dirty_saves(&mut batch);
                    flush_touch_batch(&mut touch_batch);
                    flush_tool_sig_batch(&mut batch);
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        };

        let mut cur = Some(first_cmd);
        while let Some(cmd) = cur.take() {
            match cmd {
                ToolSigPersistCmd::Save {
                    key,
                    signature,
                    seq,
                } => {
                    if !is_session_delete_pending_for_scoped_key(&key)
                        && SignatureCache::is_persist_entry_current(&key, seq)
                    {
                        let should_insert = batch
                            .get(&key)
                            .map(|(_, existing_seq)| seq >= *existing_seq)
                            .unwrap_or(true);
                        if should_insert {
                            batch.insert(key, (signature, seq));
                        }
                        if batch.len() >= batch_cap {
                            flush_tool_sig_batch(&mut batch);
                        }
                    }
                }
                ToolSigPersistCmd::Touch { key, seq, at } => {
                    if !is_session_delete_pending_for_scoped_key(&key)
                        && SignatureCache::is_persist_entry_current(&key, seq)
                    {
                        let should_insert = touch_batch
                            .get(&key)
                            .map(|(existing_seq, _)| seq >= *existing_seq)
                            .unwrap_or(true);
                        if should_insert {
                            touch_batch.insert(key, (seq, at));
                        }
                        if touch_batch.len() >= batch_cap {
                            flush_touch_batch(&mut touch_batch);
                        }
                    }
                }
                ToolSigPersistCmd::DeleteSession { session_id } => {
                    handle_delete_session(&session_id, &mut batch, &mut touch_batch);
                }
                ToolSigPersistCmd::Flush { ack } => {
                    drain_dirty_saves(&mut batch);
                    flush_touch_batch(&mut touch_batch);
                    flush_tool_sig_batch(&mut batch);
                    let _ = ack.send(());
                }
            }

            while let Ok(cmd) = delete_rx.try_recv() {
                if let ToolSigPersistCmd::DeleteSession { session_id } = cmd {
                    handle_delete_session(&session_id, &mut batch, &mut touch_batch);
                }
            }

            match save_rx.try_recv() {
                Ok(next) => cur = Some(next),
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
        drain_dirty_saves(&mut batch);
        flush_touch_batch(&mut touch_batch);
        flush_tool_sig_batch(&mut batch);
    }
}

fn persist_senders() -> &'static PersistSenders {
    TOOL_SIG_PERSIST_TX.get_or_init(|| {
        let (save_tx, save_rx) = sync_channel(crate::proxy::cache_profile().queue_cap.max(1));
        let (delete_tx, delete_rx) = sync_channel(TOOL_DELETE_QUEUE_CAP);
        let _ = std::thread::Builder::new()
            .name("ag-tool-sig-writer".to_string())
            .spawn(move || run_tool_sig_persist_worker(save_rx, delete_rx));
        PersistSenders {
            save: save_tx,
            delete: delete_tx,
        }
    })
}

fn tool_sig_persist_sender() -> &'static SyncSender<ToolSigPersistCmd> {
    &persist_senders().save
}

fn tool_sig_delete_sender() -> &'static SyncSender<ToolSigPersistCmd> {
    &persist_senders().delete
}

/// Cache entry with timestamp for TTL
#[derive(Clone, Debug)]
struct CacheEntry<T> {
    data: T,
    timestamp: SystemTime,
}

/// Specialized entry for session-based signatures to track message count
#[derive(Clone, Debug)]
struct SessionSignatureEntry {
    signature: String,
    message_count: usize,
}

impl<T> CacheEntry<T> {
    fn new(data: T) -> Self {
        Self {
            data,
            timestamp: SystemTime::now(),
        }
    }

    fn is_expired(&self) -> bool {
        self.timestamp.elapsed().unwrap_or(Duration::ZERO) > SIGNATURE_TTL
    }
}

/// Triple-layer signature cache to handle:
/// 1. Signature recovery for tool calls (when clients strip them)
/// 2. Cross-model compatibility checks (preventing Claude signatures on Gemini models)
/// 3. Session-based signature tracking (preventing cross-session pollution)
pub struct SignatureCache {
    /// Layer 1: Scoped Tool Use ID -> Thinking Signature (with epoch-guarded negative cache)
    /// Key: "{store_key}\x1f{normalized_tool_use_id}"
    /// Value: Present(signature) or Missing { write_epoch }
    tool_signatures: DashMap<String, CacheEntry<ToolSigCacheValue>>,

    /// Layer 2: Signature -> Model Family
    /// Key: thought signature string
    /// Value: Model family identifier (e.g., "claude-3-5-sonnet", "gemini-2.0-flash")
    thinking_families: Mutex<HashMap<String, CacheEntry<String>>>,

    /// Layer 3: Session ID -> Map of Message Count -> Thinking Signature (NEW)
    /// Key: session fingerprint (e.g., "sid-a1b2c3d4...")
    /// Value: A map of message count to thought signature
    /// This prevents signature pollution between different conversations and preserves history
    session_signatures: Mutex<HashMap<String, CacheEntry<HashMap<usize, SessionSignatureEntry>>>>,

    /// Layer 4: Session ID -> Assistant Reasoning Text History (NEW v4.2.0)
    /// Key: session fingerprint
    /// Value: A vector of reasoning contents (index corresponds to assistant turn index)
    session_reasonings: Mutex<HashMap<String, CacheEntry<Vec<String>>>>,

    /// L2 SQLite persistence strategy: Async background batching in production, isolated in tests.
    persist_mode: PersistMode,
}

impl SignatureCache {
    #[allow(dead_code)]
    fn new() -> Self {
        Self::with_persist_mode(PersistMode::Sync)
    }

    fn with_persist_mode(persist_mode: PersistMode) -> Self {
        Self {
            tool_signatures: DashMap::new(),
            thinking_families: Mutex::new(HashMap::new()),
            session_signatures: Mutex::new(HashMap::new()),
            session_reasonings: Mutex::new(HashMap::new()),
            persist_mode,
        }
    }

    /// Global singleton instance
    pub fn global() -> &'static SignatureCache {
        static INSTANCE: OnceLock<SignatureCache> = OnceLock::new();
        INSTANCE.get_or_init(|| {
            #[cfg(test)]
            {
                SignatureCache::with_persist_mode(PersistMode::MemoryOnly)
            }
            #[cfg(not(test))]
            {
                SignatureCache::with_persist_mode(PersistMode::Async)
            }
        })
    }

    /// Store a tool call signature
    fn scoped_tool_key(store_key: &str, tool_id: &str) -> Option<String> {
        let store_key = store_key.trim();
        let tool_id = tool_id.trim();
        if store_key.is_empty() || tool_id.is_empty() {
            return None;
        }
        Some(format!("{store_key}\u{1f}{tool_id}"))
    }

    pub(crate) fn bump_global_persist_floor() -> u64 {
        let seq = next_persist_seq();
        GLOBAL_MIN_PERSIST_SEQ.fetch_max(seq, Ordering::AcqRel);
        if let Some(slot) = SESSION_MIN_PERSIST_SEQ.get() {
            if let Ok(mut map) = slot.lock() {
                map.clear();
            }
        }
        seq
    }

    pub(crate) fn bump_session_persist_floor(session_id: &str) -> u64 {
        let clean = session_id.trim();
        if clean.is_empty() {
            return GLOBAL_MIN_PERSIST_SEQ.load(Ordering::Acquire);
        }
        let seq = next_persist_seq();
        let slot = SESSION_MIN_PERSIST_SEQ.get_or_init(|| Mutex::new(HashMap::new()));
        if let Ok(mut map) = slot.lock() {
            let entry = map.entry(clean.to_string()).or_insert((0, Instant::now()));
            if seq > entry.0 {
                entry.0 = seq;
            }
            entry.1 = Instant::now();
            if map.len() > 512 {
                let min_global = GLOBAL_MIN_PERSIST_SEQ.load(Ordering::Acquire);
                map.retain(|_, (v, at)| *v > min_global && at.elapsed() < SESSION_FLOOR_TTL);
            }
        }
        seq
    }

    pub(crate) fn is_persist_entry_current(scoped_key: &str, seq: u64) -> bool {
        if seq <= GLOBAL_MIN_PERSIST_SEQ.load(Ordering::Acquire) {
            return false;
        }
        let Some(slot) = SESSION_MIN_PERSIST_SEQ.get() else {
            return true;
        };
        let Ok(map) = slot.lock() else {
            return true;
        };
        let store_key = scoped_key
            .split_once('\u{1f}')
            .map(|(prefix, _)| prefix)
            .unwrap_or(scoped_key);
        if let Some(&(min_seq, _)) = map.get(store_key) {
            if seq <= min_seq {
                return false;
            }
        }
        let mut rest = store_key;
        while let Some((_, tail)) = rest.split_once(':') {
            if let Some(&(min_seq, _)) = map.get(tail) {
                if seq <= min_seq {
                    return false;
                }
            }
            rest = tail;
        }
        true
    }

    #[cfg(test)]
    pub(crate) fn allocate_persist_seq_for_test() -> u64 {
        next_persist_seq()
    }

    fn enqueue_touch(&self, scoped_key: &str, seq: u64) {
        // 先做无锁/分片节流判断，绝大多数命中不会触碰 session floor / pending 全局状态。
        if !touch_is_due(scoped_key)
            || is_session_delete_pending_for_scoped_key(scoped_key)
            || !Self::is_persist_entry_current(scoped_key, seq)
        {
            return;
        }
        let at = chrono::Utc::now().timestamp_millis();
        match self.persist_mode {
            PersistMode::Async => {
                if tool_sig_persist_sender()
                    .try_send(ToolSigPersistCmd::Touch {
                        key: scoped_key.to_string(),
                        seq,
                        at,
                    })
                    .is_ok()
                {
                    mark_touch(scoped_key);
                    SIGNATURE_CACHE_METRICS
                        .touches_enqueued
                        .fetch_add(1, Ordering::Relaxed);
                }
            }
            PersistMode::Sync => {
                if crate::modules::proxy_db::touch_tool_signature(scoped_key, at).is_ok() {
                    mark_touch(scoped_key);
                    SIGNATURE_CACHE_METRICS
                        .touches_enqueued
                        .fetch_add(1, Ordering::Relaxed);
                }
            }
            #[cfg(test)]
            PersistMode::MemoryOnly => {}
        }
    }

    pub fn cache_tool_signature(&self, store_key: &str, tool_use_id: &str, signature: String) {
        let Some(signature) = crate::modules::proxy_db::normalize_and_heal_signature(&signature)
        else {
            return;
        };

        let norm_id = crate::proxy::common::utils::normalize_tool_id(tool_use_id);
        let Some(id_str) = Self::scoped_tool_key(store_key, norm_id.as_ref()) else {
            return;
        };

        // 1. 先检查并更新内存 L1 缓存：若已持有相同的未过期有效签名，直接刷新 TTL 返回，避免重复写库；
        //    若需写入，则在持有 L1 锁期间分配单调递增的 persist_seq，保证与并发会话作废严格定序。
        let mut touch_identical = false;
        let persist_seq = {
            let seq = next_persist_seq();
            if let Some(mut existing) = self.tool_signatures.get_mut(&id_str) {
                if !existing.is_expired() {
                    if let ToolSigCacheValue::Present(ref cur_sig) = existing.data {
                        if cur_sig == &signature {
                            existing.timestamp = SystemTime::now();
                            touch_identical = true;
                        }
                    }
                }
            }

            if !touch_identical {
                tracing::debug!("[SignatureCache] Caching tool signature for id: {}", id_str);
                self.tool_signatures.insert(
                    id_str.clone(),
                    CacheEntry::new(ToolSigCacheValue::Present(signature.clone())),
                );
            }

            // Clean up expired entries (and evict oldest if still above limit)
            let l1_limit = crate::proxy::cache_profile().l1_limit.max(1);
            if self.tool_signatures.len() > l1_limit {
                let before = self.tool_signatures.len();
                self.tool_signatures.retain(|_, v| !v.is_expired());
                if self.tool_signatures.len() > l1_limit {
                    let target = l1_limit * 4 / 5;
                    let remove_count = self.tool_signatures.len().saturating_sub(target);
                    let mut by_age: Vec<(String, SystemTime)> = self
                        .tool_signatures
                        .iter()
                        .map(|e| (e.key().clone(), e.value().timestamp))
                        .collect();
                    by_age.sort_unstable_by_key(|(_, ts)| *ts);
                    for (k, _) in by_age.into_iter().take(remove_count) {
                        self.tool_signatures.remove(&k);
                    }
                }
                let after = self.tool_signatures.len();
                if before != after {
                    tracing::debug!(
                        "[SignatureCache] Tool cache cleanup: {} -> {} entries",
                        before,
                        after
                    );
                }
            }
            seq
        };

        if is_session_delete_pending_for_store(store_key) {
            // 删除与写入并发时，删除可能已 clear L1；若本线程随后插入，需自清理，
            // 避免 pending 期间残留的 L1 Present 被后续召回直接命中。
            self.tool_signatures.remove(&id_str);
            return;
        }

        if touch_identical {
            self.enqueue_touch(&id_str, persist_seq);
            return;
        }

        // 2. 仅对新增或变更的签名持久化到 SQLite L2 数据库 (生产环境走非阻塞批量队列，杜绝阻塞请求与 SSE 流)
        match self.persist_mode {
            PersistMode::Async => {
                if is_session_delete_pending_for_store(store_key)
                    || !Self::is_persist_entry_current(&id_str, persist_seq)
                {
                    return;
                }
                let cmd = ToolSigPersistCmd::Save {
                    key: id_str,
                    signature,
                    seq: persist_seq,
                };
                if let Err(err) = tool_sig_persist_sender().try_send(cmd) {
                    SIGNATURE_CACHE_METRICS
                        .saves_dropped
                        .fetch_add(1, Ordering::Relaxed);
                    let cmd = match err {
                        TrySendError::Full(cmd) | TrySendError::Disconnected(cmd) => cmd,
                    };
                    if let ToolSigPersistCmd::Save {
                        key,
                        signature,
                        seq,
                    } = cmd
                    {
                        mark_dirty_save(key, signature, seq);
                    }
                    tracing::debug!(
                        "[SignatureCache] Persist queue full, keeping tool signature in L1 only"
                    );
                } else {
                    SIGNATURE_CACHE_METRICS
                        .saves_enqueued
                        .fetch_add(1, Ordering::Relaxed);
                }
            }
            PersistMode::Sync => {
                if Self::is_persist_entry_current(&id_str, persist_seq) {
                    let _ = crate::modules::proxy_db::save_tool_signature(&id_str, &signature);
                    SIGNATURE_CACHE_METRICS
                        .saves_enqueued
                        .fetch_add(1, Ordering::Relaxed);
                }
            }
            #[cfg(test)]
            PersistMode::MemoryOnly => {}
        }
    }

    fn candidate_tool_keys(store_key: &str, tool_use_id: &str) -> Vec<String> {
        let norm_id = crate::proxy::common::utils::normalize_tool_id(tool_use_id);
        let mut candidate_ids = Vec::with_capacity(2);
        if let Some(scoped) = Self::scoped_tool_key(store_key, norm_id.as_ref()) {
            candidate_ids.push(scoped);
        }
        if norm_id.as_ref() != tool_use_id {
            if let Some(scoped) = Self::scoped_tool_key(store_key, tool_use_id) {
                candidate_ids.push(scoped);
            }
        }
        candidate_ids
    }

    /// Retrieve a signature for a tool_use_id inside one session store.
    pub fn get_tool_signature(&self, store_key: &str, tool_use_id: &str) -> Option<String> {
        let started = Instant::now();
        if self.persist_mode == PersistMode::Async && is_session_delete_pending_for_store(store_key)
        {
            SIGNATURE_CACHE_METRICS.record_resolve(false, false, started.elapsed());
            return None;
        }
        let candidate_ids = Self::candidate_tool_keys(store_key, tool_use_id);
        if candidate_ids.is_empty() {
            SIGNATURE_CACHE_METRICS.record_resolve(false, false, started.elapsed());
            return None;
        }

        for candidate in &candidate_ids {
            let (cur_global_epoch, cur_key_epoch) =
                crate::modules::proxy_db::current_tool_sig_epochs(candidate);

            // 1. 先查内存 L1 缓存（含按 key 精度隔离的写代数负缓存，避免历史无签名轮次反复穿透 SQLite）
            if let Some((is_expired, data)) = self
                .tool_signatures
                .get(candidate)
                .map(|e| (e.is_expired(), e.data.clone()))
            {
                if !is_expired {
                    match data {
                        ToolSigCacheValue::Present(sig) => {
                            tracing::debug!(
                                "[SignatureCache] Hit tool signature for id: {}",
                                candidate
                            );
                            self.enqueue_touch(candidate, next_persist_seq());
                            SIGNATURE_CACHE_METRICS.record_resolve(true, false, started.elapsed());
                            return Some(sig);
                        }
                        ToolSigCacheValue::Missing {
                            global_epoch,
                            key_epoch,
                        } if global_epoch == cur_global_epoch && key_epoch == cur_key_epoch => {
                            continue;
                        }
                        ToolSigCacheValue::Missing { .. } => {}
                    }
                }
            }

            #[cfg(test)]
            if self.persist_mode == PersistMode::MemoryOnly {
                continue;
            }

            if self.persist_mode == PersistMode::Async
                && is_session_delete_pending_for_store(store_key)
            {
                continue;
            }

            // 2. 内存未命中（如代理重启过），从 SQLite L2 数据库恢复
            match crate::modules::proxy_db::load_tool_signature(candidate) {
                Ok(Some(sig)) => {
                    self.tool_signatures.insert(
                        candidate.clone(),
                        CacheEntry::new(ToolSigCacheValue::Present(sig.clone())),
                    );
                    tracing::info!(
                        "[SignatureCache] Restored tool signature from SQLite for id: {}",
                        candidate
                    );
                    self.enqueue_touch(candidate, next_persist_seq());
                    SIGNATURE_CACHE_METRICS.record_resolve(false, true, started.elapsed());
                    return Some(sig);
                }
                Ok(None) => {
                    let occupied_by_present =
                        self.tool_signatures.get(candidate).is_some_and(|e| {
                            !e.is_expired() && matches!(e.data, ToolSigCacheValue::Present(_))
                        });
                    if !occupied_by_present {
                        self.tool_signatures.insert(
                            candidate.clone(),
                            CacheEntry::new(ToolSigCacheValue::Missing {
                                global_epoch: cur_global_epoch,
                                key_epoch: cur_key_epoch,
                            }),
                        );
                    }
                }
                Err(_) => {}
            }
        }

        SIGNATURE_CACHE_METRICS.record_resolve(false, false, started.elapsed());
        None
    }

    /// 请求级批量召回：一次收集所有 functionCall ID，避免逐工具点查 SQLite。
    pub fn get_tool_signatures_batch(
        &self,
        store_key: &str,
        tool_ids: &[String],
    ) -> HashMap<String, String> {
        let mut result = HashMap::new();
        if tool_ids.is_empty() {
            return result;
        }
        if self.persist_mode == PersistMode::Async && is_session_delete_pending_for_store(store_key)
        {
            return result;
        }
        let batch_started = Instant::now();
        SIGNATURE_CACHE_METRICS
            .batch_calls
            .fetch_add(1, Ordering::Relaxed);
        SIGNATURE_CACHE_METRICS
            .batch_ids
            .fetch_add(tool_ids.len() as u64, Ordering::Relaxed);

        let mut pending: Vec<(String, Vec<String>)> = Vec::new();
        let mut all_candidates: Vec<String> = Vec::new();
        let skip_l2 = self.persist_mode == PersistMode::Async
            && is_session_delete_pending_for_store(store_key);

        for tool_id in tool_ids {
            let candidates = Self::candidate_tool_keys(store_key, tool_id);
            if candidates.is_empty() {
                continue;
            }
            let mut found = false;
            let mut misses: Vec<String> = Vec::new();
            for candidate in &candidates {
                let mut negative_valid = false;
                let (cur_global_epoch, cur_key_epoch) =
                    crate::modules::proxy_db::current_tool_sig_epochs(candidate);
                if let Some((is_expired, data)) = self
                    .tool_signatures
                    .get(candidate)
                    .map(|e| (e.is_expired(), e.data.clone()))
                {
                    if !is_expired {
                        match data {
                            ToolSigCacheValue::Present(sig) => {
                                result.insert(tool_id.clone(), sig);
                                self.enqueue_touch(candidate, next_persist_seq());
                                SIGNATURE_CACHE_METRICS
                                    .l1_hits
                                    .fetch_add(1, Ordering::Relaxed);
                                found = true;
                                break;
                            }
                            ToolSigCacheValue::Missing {
                                global_epoch,
                                key_epoch,
                            } if global_epoch == cur_global_epoch && key_epoch == cur_key_epoch => {
                                negative_valid = true;
                            }
                            ToolSigCacheValue::Missing { .. } => {}
                        }
                    }
                }
                if !negative_valid {
                    misses.push(candidate.clone());
                }
            }
            if found {
                continue;
            }
            #[cfg(test)]
            if self.persist_mode == PersistMode::MemoryOnly {
                continue;
            }
            if skip_l2 || misses.is_empty() {
                continue;
            }
            all_candidates.extend(misses.iter().cloned());
            pending.push((tool_id.clone(), misses));
        }

        if pending.is_empty() || all_candidates.is_empty() {
            return result;
        }

        let loaded = match crate::modules::proxy_db::load_tool_signatures_batch(&all_candidates) {
            Ok(loaded) => loaded,
            Err(e) => {
                // 读取失败绝不能写负缓存，否则会把“数据库暂时不可用”污染成 2 小时 miss。
                tracing::debug!("[SignatureCache] Batch signature load failed: {e}");
                return result;
            }
        };
        for (tool_id, candidates) in pending {
            let mut hit = None;
            for candidate in &candidates {
                if let Some(sig) = loaded.get(candidate).cloned() {
                    self.tool_signatures.insert(
                        candidate.clone(),
                        CacheEntry::new(ToolSigCacheValue::Present(sig.clone())),
                    );
                    hit = Some((candidate.clone(), sig));
                    break;
                }
            }
            if let Some((candidate, sig)) = hit {
                self.enqueue_touch(&candidate, next_persist_seq());
                SIGNATURE_CACHE_METRICS
                    .l2_hits
                    .fetch_add(1, Ordering::Relaxed);
                result.insert(tool_id, sig);
            } else {
                SIGNATURE_CACHE_METRICS
                    .misses
                    .fetch_add(1, Ordering::Relaxed);
                for candidate in &candidates {
                    let (cur_global_epoch, cur_key_epoch) =
                        crate::modules::proxy_db::current_tool_sig_epochs(candidate);
                    let occupied_by_present =
                        self.tool_signatures.get(candidate).is_some_and(|e| {
                            !e.is_expired() && matches!(e.data, ToolSigCacheValue::Present(_))
                        });
                    if !occupied_by_present {
                        self.tool_signatures.insert(
                            candidate.clone(),
                            CacheEntry::new(ToolSigCacheValue::Missing {
                                global_epoch: cur_global_epoch,
                                key_epoch: cur_key_epoch,
                            }),
                        );
                    }
                }
            }
        }
        SIGNATURE_CACHE_METRICS.resolve_ns.fetch_add(
            batch_started.elapsed().as_nanos().min(u64::MAX as u128) as u64,
            Ordering::Relaxed,
        );
        result
    }

    /// Store model family for a signature
    pub fn cache_thinking_family(&self, signature: String, family: String) {
        if signature.len() < MIN_SIGNATURE_LENGTH {
            return;
        }

        if let Ok(mut cache) = self.thinking_families.lock() {
            tracing::debug!(
                "[SignatureCache] Caching thinking family for sig (len={}): {}",
                signature.len(),
                family
            );
            let norm = crate::proxy::thinking_store::normalize_signature_for_comparison(&signature)
                .into_owned();
            cache.insert(signature.clone(), CacheEntry::new(family.clone()));
            if norm != signature {
                cache.insert(norm, CacheEntry::new(family));
            }

            if cache.len() > FAMILY_CACHE_LIMIT {
                let before = cache.len();
                cache.retain(|_, v| !v.is_expired());
                let after = cache.len();
                if before != after {
                    tracing::debug!(
                        "[SignatureCache] Family cache cleanup: {} -> {} entries",
                        before,
                        after
                    );
                }
            }
        }
    }

    /// Get model family for a signature
    pub fn get_signature_family(&self, signature: &str) -> Option<String> {
        if let Ok(cache) = self.thinking_families.lock() {
            if let Some(entry) = cache.get(signature) {
                if !entry.is_expired() {
                    return Some(entry.data.clone());
                } else {
                    tracing::debug!("[SignatureCache] Signature family entry expired");
                }
            }
            let alt = crate::proxy::thinking_store::normalize_signature_for_comparison(signature);
            if alt.as_ref() != signature {
                if let Some(entry) = cache.get(alt.as_ref()) {
                    if !entry.is_expired() {
                        return Some(entry.data.clone());
                    }
                }
            }
        }
        None
    }

    // ===== Layer 3: Session-based Signature Storage =====

    /// Store the thinking signature for a session at a specific message count.
    /// This is the preferred method for tracking signatures across tool loops.
    ///
    /// # Arguments
    /// * `session_id` - Session fingerprint (e.g., "sid-a1b2c3d4...")
    /// * `signature` - The thought signature to store
    /// * `message_count` - The current message count of the conversation (to detect Rewind)
    pub fn cache_session_signature(
        &self,
        session_id: &str,
        signature: String,
        message_count: usize,
    ) {
        // 自愈防裂化：若签名被误传为原始 Protobuf 二进制 (首字节 0x12)，自动纠正编码为标准 Base64
        let signature = if signature.as_bytes().first() == Some(&0x12) {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.encode(signature.as_bytes())
        } else {
            signature
        };

        if signature.len() < MIN_SIGNATURE_LENGTH {
            return;
        }

        if let Ok(mut cache) = self.session_signatures.lock() {
            let entry = cache
                .entry(session_id.to_string())
                .or_insert_with(|| CacheEntry::new(HashMap::new()));

            // Update timestamp to refresh TTL
            entry.timestamp = SystemTime::now();

            // Detect if a rewind happened (e.g. if we have cached signatures with message_count
            // greater than the current message_count, those should be cleared since that future is gone).
            entry.data.retain(|&mc, _| {
                if mc > message_count {
                    tracing::info!(
                        "[SignatureCache] Rewind detected for {} at count {}: removing future signature at count {}.",
                        session_id,
                        message_count,
                        mc
                    );
                    false
                } else {
                    true
                }
            });

            let should_store = match entry.data.get(&message_count) {
                None => true,
                Some(existing) => {
                    // Same message count: only update if the new signature is longer (more complete)
                    signature.len() > existing.signature.len()
                }
            };

            if should_store {
                tracing::debug!(
                    "[SignatureCache] Session {} (msg_count={}) -> storing signature (len={})",
                    session_id,
                    message_count,
                    signature.len()
                );
                entry.data.insert(
                    message_count,
                    SessionSignatureEntry {
                        signature,
                        message_count,
                    },
                );
            }

            // Cleanup when limit is reached (Session cache has largest limit)
            if cache.len() > SESSION_CACHE_LIMIT {
                let before = cache.len();
                cache.retain(|_, v| !v.is_expired());
                let after = cache.len();
                if before != after {
                    tracing::info!(
                        "[SignatureCache] Session cache cleanup: {} -> {} entries (limit: {})",
                        before,
                        after,
                        SESSION_CACHE_LIMIT
                    );
                }
            }
        }
    }

    /// Retrieve the latest thinking signature for a session.
    /// Returns None if not found or expired.
    pub fn get_session_signature(&self, session_id: &str) -> Option<String> {
        if let Ok(cache) = self.session_signatures.lock() {
            // 1. 精确匹配
            if let Some(entry) = cache.get(session_id) {
                if !entry.is_expired() {
                    // Find the signature with the maximum message_count (the latest one)
                    if let Some(sig_entry) = entry.data.values().max_by_key(|e| e.message_count) {
                        tracing::debug!(
                            "[SignatureCache] Session {} (latest, msg_count={}) -> HIT (len={})",
                            session_id,
                            sig_entry.message_count,
                            sig_entry.signature.len()
                        );
                        return Some(sig_entry.signature.clone());
                    }
                } else {
                    tracing::debug!("[SignatureCache] Session {} -> EXPIRED", session_id);
                }
            }
        }
        None
    }

    /// Check if the session cache already holds a signature for a turn after `message_count`.
    pub fn has_session_signature_after(&self, session_id: &str, message_count: usize) -> bool {
        if let Ok(cache) = self.session_signatures.lock() {
            if let Some(entry) = cache.get(session_id) {
                if !entry.is_expired() {
                    return entry.data.keys().any(|&mc| mc > message_count);
                }
            }
        }
        false
    }

    /// Retrieve the thinking signature for a session at a specific message count.
    /// Returns None if not found or expired.
    pub fn get_session_signature_at(
        &self,
        session_id: &str,
        message_count: usize,
    ) -> Option<String> {
        if let Ok(cache) = self.session_signatures.lock() {
            if let Some(entry) = cache.get(session_id) {
                if !entry.is_expired() {
                    if let Some(sig_entry) = entry.data.get(&message_count) {
                        tracing::debug!(
                            "[SignatureCache] Session {} (msg_count={}) -> HIT (len={})",
                            session_id,
                            message_count,
                            sig_entry.signature.len()
                        );
                        return Some(sig_entry.signature.clone());
                    }
                }
            }
        }
        None
    }

    /// Store reasoning text for a specific assistant turn in a session
    pub fn cache_session_reasoning(&self, session_id: &str, reasoning: String, turn_index: usize) {
        if reasoning.trim().is_empty() {
            return;
        }

        if let Ok(mut cache) = self.session_reasonings.lock() {
            let entry = cache
                .entry(session_id.to_string())
                .or_insert_with(|| CacheEntry::new(Vec::new()));

            // Update timestamp to refresh TTL
            entry.timestamp = std::time::SystemTime::now();

            if turn_index >= entry.data.len() {
                entry.data.resize(turn_index + 1, String::new());
            }

            // Only update if the new reasoning is longer to prevent overwriting with partial content
            let old_len = entry.data[turn_index].len();
            if reasoning.len() > old_len {
                tracing::debug!(
                    "[SignatureCache] Session {} (turn={}) -> caching reasoning text (len: {} -> {})",
                    session_id,
                    turn_index,
                    old_len,
                    reasoning.len()
                );
                entry.data[turn_index] = reasoning;
            }

            // Session cache cleanup if limit exceeded
            if cache.len() > SESSION_CACHE_LIMIT {
                let before = cache.len();
                cache.retain(|_, v| !v.is_expired());
                let after = cache.len();
                if before != after {
                    tracing::debug!(
                        "[SignatureCache] Session reasoning cache cleanup: {} -> {} entries",
                        before,
                        after
                    );
                }
            }
        }
    }

    /// Retrieve reasoning text for a specific assistant turn in a session
    pub fn get_session_reasoning(&self, session_id: &str, turn_index: usize) -> Option<String> {
        if let Ok(cache) = self.session_reasonings.lock() {
            if let Some(entry) = cache.get(session_id) {
                if !entry.is_expired() && turn_index < entry.data.len() {
                    let text = &entry.data[turn_index];
                    if !text.trim().is_empty() {
                        tracing::debug!(
                            "[SignatureCache] Session {} (turn={}) -> Hit reasoning text cache (len: {})",
                            session_id,
                            turn_index,
                            text.len()
                        );
                        return Some(text.clone());
                    }
                }
            }
        }
        None
    }

    /// 删除指定会话的缓存签名（同步清理会话级签名、工具调用签名及底层 SQLite 持久化，保留会话推理文本）
    #[allow(dead_code)] // 预留给重试剥离、管理接口或调试使用
    pub fn delete_session_signature(&self, session_id: &str) {
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return;
        }
        let suffix = format!(":{}", session_id);
        if let Ok(mut cache) = self.session_signatures.lock() {
            let before = cache.len();
            cache.retain(|k, _| k != session_id && !k.ends_with(&suffix));
            if cache.len() != before {
                tracing::debug!(
                    "[SignatureCache] Deleted session signature for: {}",
                    session_id
                );
            }
        }
        let mut delete_enqueued = false;
        if self.persist_mode == PersistMode::Async {
            // 先 mark pending，再抬水位：避免并发 Save 在“抬水位但尚未 pending”的窗口内插入
            mark_pending_session_delete(session_id);
        }
        let _ = Self::bump_session_persist_floor(session_id);
        self.tool_signatures
            .retain(|k, _| !matches_scoped_session(k, session_id));
        if self.persist_mode == PersistMode::Async {
            delete_enqueued = tool_sig_delete_sender()
                .try_send(ToolSigPersistCmd::DeleteSession {
                    session_id: session_id.to_string(),
                })
                .is_ok();
        }
        match self.persist_mode {
            PersistMode::Async => {
                if !delete_enqueued {
                    match crate::modules::proxy_db::delete_tool_signatures_for_session(session_id)
                    {
                        Ok(_) => complete_pending_session_delete(session_id),
                        Err(e) => tracing::warn!(
                            "[SignatureCache] Fallback session delete failed for {}: {}; keeping pending guard",
                            session_id,
                            e
                        ),
                    }
                }
            }
            PersistMode::Sync => {
                let _ = crate::modules::proxy_db::delete_tool_signatures_for_session(session_id);
            }
            #[cfg(test)]
            PersistMode::MemoryOnly => {}
        }
    }

    /// 彻底作废特定会话的全部签名缓存（包括 session_signatures, session_reasonings, tool_signatures 及 SQLite 持久化）
    pub fn invalidate_session(&self, session_id: &str) {
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return;
        }
        self.delete_session_signature(session_id);
        let suffix = format!(":{}", session_id);
        if let Ok(mut cache) = self.session_reasonings.lock() {
            cache.retain(|k, _| k != session_id && !k.ends_with(&suffix));
        }
    }

    /// Clear all caches (for testing or manual reset)
    #[allow(dead_code)] // Used in tests
    pub fn clear(&self) {
        let _ = Self::bump_global_persist_floor();
        self.tool_signatures.clear();
        if let Ok(mut cache) = self.thinking_families.lock() {
            cache.clear();
        }
        if let Ok(mut cache) = self.session_signatures.lock() {
            cache.clear();
        }
        if let Ok(mut cache) = self.session_reasonings.lock() {
            cache.clear();
        }
        match self.persist_mode {
            PersistMode::Async | PersistMode::Sync => {
                let _ = crate::modules::proxy_db::clear_tool_signatures();
            }
            #[cfg(test)]
            PersistMode::MemoryOnly => {}
        }
    }

    /// 退出/维护时尽力刷盘：把队列和 dirty 集合中的签名写入 SQLite。
    pub fn flush_persist(&self, timeout: Duration) {
        if self.persist_mode != PersistMode::Async {
            return;
        }
        let (ack_tx, ack_rx) = sync_channel(1);
        if tool_sig_persist_sender()
            .try_send(ToolSigPersistCmd::Flush { ack: ack_tx })
            .is_ok()
        {
            let _ = ack_rx.recv_timeout(timeout);
        }
    }

    #[cfg(test)]
    fn flush_pending_persist(&self) {
        if self.persist_mode != PersistMode::Async {
            return;
        }
        let (ack_tx, ack_rx) = sync_channel(1);
        if tool_sig_persist_sender()
            .send(ToolSigPersistCmd::Flush { ack: ack_tx })
            .is_ok()
        {
            let _ = ack_rx.recv_timeout(Duration::from_secs(5));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_signature_cache() {
        let _dir = crate::proxy::monitor::prompt_log_tests::TestDataDir::new();
        crate::modules::proxy_db::init_db().unwrap();
        let cache = SignatureCache::new();
        let sig = "x".repeat(60); // Valid length

        cache.cache_tool_signature("scope", "tool_1", sig.clone());
        assert_eq!(cache.get_tool_signature("scope", "tool_1"), Some(sig));
        assert_eq!(cache.get_tool_signature("scope", "tool_2"), None);
        assert_eq!(cache.get_tool_signature("other", "tool_1"), None);
    }

    #[test]
    fn test_min_length() {
        let _dir = crate::proxy::monitor::prompt_log_tests::TestDataDir::new();
        let cache = SignatureCache::with_persist_mode(PersistMode::MemoryOnly);
        cache.cache_tool_signature("scope", "tool_short", "short".to_string());
        assert_eq!(cache.get_tool_signature("scope", "tool_short"), None);
    }

    #[test]
    fn test_thinking_family() {
        let _dir = crate::proxy::monitor::prompt_log_tests::TestDataDir::new();
        let cache = SignatureCache::with_persist_mode(PersistMode::MemoryOnly);
        let sig = "y".repeat(60);

        cache.cache_thinking_family(sig.clone(), "claude".to_string());
        assert_eq!(cache.get_signature_family(&sig), Some("claude".to_string()));
    }

    #[test]
    fn test_session_signature() {
        let _dir = crate::proxy::monitor::prompt_log_tests::TestDataDir::new();
        let cache = SignatureCache::with_persist_mode(PersistMode::MemoryOnly);
        let sig1 = "a".repeat(60);
        let sig2 = "b".repeat(80); // Longer, should replace
        let sig3 = "c".repeat(20); // Too short (< 32), should be ignored

        // Initially empty
        assert!(cache.get_session_signature("sid-test123").is_none());

        // Store first signature
        cache.cache_session_signature("sid-test123", sig1.clone(), 5);
        assert_eq!(
            cache.get_session_signature("sid-test123"),
            Some(sig1.clone())
        );

        // Longer signature should replace (same msg count)
        cache.cache_session_signature("sid-test123", sig2.clone(), 5);
        assert_eq!(
            cache.get_session_signature("sid-test123"),
            Some(sig2.clone())
        );

        // Shorter valid signature should NOT replace (same msg count)
        cache.cache_session_signature("sid-test123", sig1.clone(), 5);
        assert_eq!(
            cache.get_session_signature("sid-test123"),
            Some(sig2.clone())
        );

        // Rewind: Shorter signature MUST replace if message count is lower
        cache.cache_session_signature("sid-test123", sig1.clone(), 3);
        assert_eq!(
            cache.get_session_signature("sid-test123"),
            Some(sig1.clone())
        );

        // Too short signature should be ignored entirely (even if rewind)
        cache.cache_session_signature("sid-test123", sig3, 1);
        assert_eq!(cache.get_session_signature("sid-test123"), Some(sig1));

        // Different session should be isolated
        assert!(cache.get_session_signature("sid-other").is_none());
    }

    #[test]
    fn test_clear_all_caches() {
        let _dir = crate::proxy::monitor::prompt_log_tests::TestDataDir::new();
        crate::modules::proxy_db::init_db().unwrap();
        let cache = SignatureCache::new();
        let sig = "x".repeat(60);

        cache.cache_tool_signature("scope", "tool_1", sig.clone());
        cache.cache_thinking_family(sig.clone(), "model".to_string());
        cache.cache_session_signature("sid-1", sig.clone(), 1);

        assert!(cache.get_tool_signature("scope", "tool_1").is_some());
        assert!(cache.get_signature_family(&sig).is_some());
        assert!(cache.get_session_signature("sid-1").is_some());

        cache.clear();

        assert!(cache.get_tool_signature("scope", "tool_1").is_none());
        assert!(cache.get_signature_family(&sig).is_none());
        assert!(cache.get_session_signature("sid-1").is_none());
    }

    #[test]
    fn test_tool_signature_sqlite_recovery() {
        let _dir = crate::proxy::monitor::prompt_log_tests::TestDataDir::new();
        crate::modules::proxy_db::init_db().unwrap();
        let tool_id = "call_sig_sqlite_recovery_unique";
        let sig = "s".repeat(60);

        // 1. Direct save to SQLite
        let scoped = format!("scope\u{1f}{tool_id}");
        crate::modules::proxy_db::save_tool_signature(&scoped, &sig).unwrap();

        // 2. New in-memory cache (simulating proxy restart)
        let cache = SignatureCache::new();

        // 3. get_tool_signature should fall back to SQLite and populate L1 cache
        let recovered = cache.get_tool_signature("scope", tool_id);
        assert_eq!(recovered, Some(sig.clone()));

        // 4. Second lookup should hit L1
        assert_eq!(cache.get_tool_signature("scope", tool_id), Some(sig));
    }

    #[test]
    fn test_tool_signature_normalization_match() {
        let _dir = crate::proxy::monitor::prompt_log_tests::TestDataDir::new();
        crate::modules::proxy_db::init_db().unwrap();
        let cache = SignatureCache::new();
        let sig = "s".repeat(60);

        // Case A: Upstream generated call_573077, client requests with call573077
        cache.cache_tool_signature("scope", "call_573077", sig.clone());
        assert_eq!(
            cache.get_tool_signature("scope", "call573077"),
            Some(sig.clone())
        );
        assert_eq!(
            cache.get_tool_signature("scope", "call_573077"),
            Some(sig.clone())
        );

        // Case B: Client stored with call888888, lookup with call_888888
        let sig2 = "t".repeat(60);
        cache.cache_tool_signature("scope", "call888888", sig2.clone());
        assert_eq!(
            cache.get_tool_signature("scope", "call_888888"),
            Some(sig2.clone())
        );
        assert_eq!(cache.get_tool_signature("scope", "call888888"), Some(sig2));
    }

    #[test]
    fn test_invalidate_session() {
        let _dir = crate::proxy::monitor::prompt_log_tests::TestDataDir::new();
        crate::modules::proxy_db::init_db().unwrap();
        let cache = SignatureCache::new();
        let sig = "s".repeat(60);

        // 1. 设置 sess-1 (带和不带租户前缀) 以及 sess-2
        cache.cache_tool_signature("sess-1", "tool_a", sig.clone());
        cache.cache_tool_signature("tenant:sess-1", "tool_b", sig.clone());
        cache.cache_tool_signature("sess-2", "tool_c", sig.clone());

        cache.cache_session_signature("sess-1", sig.clone(), 1);
        cache.cache_session_signature("tenant:sess-1", sig.clone(), 1);
        cache.cache_session_signature("sess-2", sig.clone(), 1);

        cache.cache_session_reasoning("sess-1", "reasoning 1".to_string(), 0);
        cache.cache_session_reasoning("tenant:sess-1", "reasoning tenant 1".to_string(), 0);
        cache.cache_session_reasoning("sess-2", "reasoning 2".to_string(), 0);

        // 验证已写入
        assert!(cache.get_tool_signature("sess-1", "tool_a").is_some());
        assert!(cache
            .get_tool_signature("tenant:sess-1", "tool_b")
            .is_some());
        assert!(cache.get_tool_signature("sess-2", "tool_c").is_some());

        assert!(cache.get_session_signature("sess-1").is_some());
        assert!(cache.get_session_signature("tenant:sess-1").is_some());
        assert!(cache.get_session_signature("sess-2").is_some());

        assert!(cache.get_session_reasoning("sess-1", 0).is_some());
        assert!(cache.get_session_reasoning("tenant:sess-1", 0).is_some());
        assert!(cache.get_session_reasoning("sess-2", 0).is_some());

        // 2. 清理 sess-1
        cache.invalidate_session("sess-1");

        // 3. 验证 sess-1 及其租户变体全被清除，但 sess-2 保持完好
        assert!(cache.get_tool_signature("sess-1", "tool_a").is_none());
        assert!(cache
            .get_tool_signature("tenant:sess-1", "tool_b")
            .is_none());
        assert!(cache.get_tool_signature("sess-2", "tool_c").is_some());

        assert!(cache.get_session_signature("sess-1").is_none());
        assert!(cache.get_session_signature("tenant:sess-1").is_none());
        assert!(cache.get_session_signature("sess-2").is_some());

        assert!(cache.get_session_reasoning("sess-1", 0).is_none());
        assert!(cache.get_session_reasoning("tenant:sess-1", 0).is_none());
        assert!(cache.get_session_reasoning("sess-2", 0).is_some());
    }

    #[test]
    fn test_batch_recall_is_session_scoped() {
        let _dir = crate::proxy::monitor::prompt_log_tests::TestDataDir::new();
        crate::modules::proxy_db::init_db().unwrap();
        let cache = SignatureCache::new();
        let sig_a = "a".repeat(60);
        let sig_b = "b".repeat(60);

        cache.cache_tool_signature("tenantA:sidA", "call_shared", sig_a.clone());
        cache.cache_tool_signature("tenantB:sidB", "call_shared", sig_b.clone());

        let ids = vec!["call_shared".to_string()];
        let a = cache.get_tool_signatures_batch("tenantA:sidA", &ids);
        let b = cache.get_tool_signatures_batch("tenantB:sidB", &ids);
        assert_eq!(a.get("call_shared"), Some(&sig_a));
        assert_eq!(b.get("call_shared"), Some(&sig_b));
        assert_ne!(a.get("call_shared"), b.get("call_shared"));
    }

    #[test]
    fn test_duplicate_tool_signature_skips_redundant_sqlite_writes() {
        let _dir = crate::proxy::monitor::prompt_log_tests::TestDataDir::new();
        crate::modules::proxy_db::init_db().unwrap();

        let cache = SignatureCache::new();
        let sig_v1 = "s".repeat(60);
        let sig_v2 = "u".repeat(60);

        // 1. Initial write populates both L1 and SQLite L2
        cache.cache_tool_signature("sess-dedup", "call_repeat_1", sig_v1.clone());
        let scoped_key = "sess-dedup\u{1f}call_repeat_1";
        assert_eq!(
            crate::modules::proxy_db::load_tool_signature(scoped_key).unwrap(),
            Some(sig_v1.clone())
        );

        // Pin created_at in SQLite to a sentinel value so any redundant SQLite write is caught
        let conn =
            rusqlite::Connection::open(crate::modules::proxy_db::get_proxy_db_path().unwrap())
                .unwrap();
        let sentinel_ts: i64 = chrono::Utc::now().timestamp_millis() - 60_000;
        conn.execute(
            "UPDATE tool_signatures SET created_at = ?1 WHERE tool_id = ?2",
            rusqlite::params![sentinel_ts, scoped_key],
        )
        .unwrap();

        // 2. Re-caching the identical signature 250 times (simulating multi-turn history replay)
        // must hit L1 and perform ZERO SQLite writes.
        for _ in 0..250 {
            cache.cache_tool_signature("sess-dedup", "call_repeat_1", sig_v1.clone());
        }
        let ts_after: i64 = conn
            .query_row(
                "SELECT created_at FROM tool_signatures WHERE tool_id = ?1",
                rusqlite::params![scoped_key],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            ts_after, sentinel_ts,
            "Redundant tool signatures must not rewrite SQLite"
        );

        // 3. Updating to a different signature MUST persist to SQLite
        cache.cache_tool_signature("sess-dedup", "call_repeat_1", sig_v2.clone());
        assert_eq!(
            crate::modules::proxy_db::load_tool_signature(scoped_key).unwrap(),
            Some(sig_v2)
        );
    }

    #[test]
    fn test_negative_cache_invalidates_on_out_of_band_sqlite_write() {
        let _dir = crate::proxy::monitor::prompt_log_tests::TestDataDir::new();
        crate::modules::proxy_db::init_db().unwrap();

        let cache = SignatureCache::new();
        let sig = "n".repeat(60);

        // 1. First miss populates per-key epoch-guarded negative cache
        assert_eq!(cache.get_tool_signature("sess-neg", "call_missing"), None);
        // 2. Writing an unrelated tool_id must NOT invalidate call_missing's negative cache
        let (global_before, key_before) =
            crate::modules::proxy_db::current_tool_sig_epochs("sess-neg\u{1f}call_missing");
        crate::modules::proxy_db::save_tool_signature("sess-neg\u{1f}call_other", &sig).unwrap();
        let (global_after_other, key_after_other) =
            crate::modules::proxy_db::current_tool_sig_epochs("sess-neg\u{1f}call_missing");
        assert_eq!(global_after_other, global_before);
        assert_eq!(key_after_other, key_before);
        assert_eq!(cache.get_tool_signature("sess-neg", "call_missing"), None);

        // 3. Direct SQLite save to call_missing bumps its per-key epoch, invalidating negative cache
        crate::modules::proxy_db::save_tool_signature("sess-neg\u{1f}call_missing", &sig).unwrap();
        assert_eq!(
            cache.get_tool_signature("sess-neg", "call_missing"),
            Some(sig)
        );
    }

    #[test]
    fn test_async_background_batch_writer_and_session_invalidation() {
        let _dir = crate::proxy::monitor::prompt_log_tests::TestDataDir::new();
        crate::modules::proxy_db::init_db().unwrap();

        let cache = SignatureCache::with_persist_mode(PersistMode::Async);
        let sig = "a".repeat(60);

        for i in 0..150 {
            cache.cache_tool_signature("sess-async-keep", &format!("call_{i}"), sig.clone());
        }
        for i in 0..50 {
            cache.cache_tool_signature("sess-async-drop", &format!("call_{i}"), sig.clone());
        }
        cache.cache_tool_signature("tenant:sess-retry-strip", "call_retry", sig.clone());
        cache.cache_session_signature("tenant:sess-retry-strip", sig.clone(), 2);
        cache.cache_session_reasoning(
            "tenant:sess-retry-strip",
            "preserved reasoning".to_string(),
            0,
        );

        // Simulate a pre-invalidation Save sequence that is delayed and arrives AFTER invalidation
        let stale_seq = SignatureCache::allocate_persist_seq_for_test();

        // Invalidate sess-async-drop and strip sess-retry-strip via delete_session_signature
        cache.invalidate_session("sess-async-drop");
        cache.delete_session_signature("sess-retry-strip");
        assert!(cache
            .get_tool_signature("sess-async-drop", "call_0")
            .is_none());
        assert!(cache
            .get_tool_signature("tenant:sess-retry-strip", "call_retry")
            .is_none());
        assert!(cache
            .get_session_signature("tenant:sess-retry-strip")
            .is_none());
        assert_eq!(
            cache.get_session_reasoning("tenant:sess-retry-strip", 0),
            Some("preserved reasoning".to_string())
        );

        // Even if an older Save command is enqueued or flushed AFTER DeleteSession completed,
        // the per-session monotonic floor must reject it and prevent resurrection.
        let _ = tool_sig_persist_sender().try_send(ToolSigPersistCmd::Save {
            key: "sess-async-drop\u{1f}call_resurrect".to_string(),
            signature: sig.clone(),
            seq: stale_seq,
        });
        let _ = tool_sig_persist_sender().try_send(ToolSigPersistCmd::Save {
            key: "tenant:sess-retry-strip\u{1f}call_retry".to_string(),
            signature: sig.clone(),
            seq: stale_seq,
        });

        cache.flush_pending_persist();

        // Verify surviving session was batch-persisted to SQLite and dropped/stripped sessions are absent
        let fresh_cache = SignatureCache::new();
        assert_eq!(
            fresh_cache.get_tool_signature("sess-async-keep", "call_0"),
            Some(sig.clone())
        );
        assert_eq!(
            fresh_cache.get_tool_signature("sess-async-keep", "call_149"),
            Some(sig.clone())
        );
        assert_eq!(
            fresh_cache.get_tool_signature("sess-async-drop", "call_0"),
            None
        );
        assert_eq!(
            fresh_cache.get_tool_signature("sess-async-drop", "call_resurrect"),
            None
        );
        assert_eq!(
            fresh_cache.get_tool_signature("tenant:sess-retry-strip", "call_retry"),
            None
        );

        // Enqueue new async writes, then call clear_all_thinking_data() directly (without cache.clear() first);
        // clear_all_thinking_data() must bump global persist floor itself so pre-clear writes cannot resurrect.
        for i in 0..40 {
            cache.cache_tool_signature("sess-async-clear", &format!("call_{i}"), sig.clone());
        }
        crate::modules::proxy_db::clear_all_thinking_data().unwrap();
        cache.clear();
        cache.flush_pending_persist();

        let post_clear_cache = SignatureCache::new();
        assert_eq!(
            post_clear_cache.get_tool_signature("sess-async-clear", "call_0"),
            None
        );
        assert_eq!(
            post_clear_cache.get_tool_signature("sess-async-keep", "call_0"),
            None
        );
    }
}
