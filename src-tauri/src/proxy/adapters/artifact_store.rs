//! Sidecar store for raw tool/search/page payloads.
//!
//! The chat history must not replay huge tool output as ordinary `tool.content`.
//! Large raw payloads are stored here and the model only receives a bounded
//! evidence summary plus an artifact id.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};

const SCHEMA_VERSION: i64 = 1;
const DEFAULT_PERSISTED_TTL: Duration = Duration::from_secs(30 * 24 * 3600);
const DEFAULT_L1_SIZE: usize = 64;
const DEFAULT_L1_TTL: Duration = Duration::from_secs(3600);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredToolArtifact {
    pub artifact_id: String,
    pub call_id: Option<String>,
    pub kind: String,
    pub original_chars: usize,
    pub original_lines: usize,
    /// 是否已**持久化到共享 `tool_artifacts.db`**(而非仅落 proxy 进程内存的降级 fallback)。
    /// 跨进程的 `read_tool_artifact`(`--mcp-serve-webfetch` 进程)只能读 DB,读不到内存档 ——
    /// 故仅当 `persisted=true` 时压缩摘要才告知模型「可调 read_tool_artifact 取回」,否则会给一个
    /// reader 看不到的 id、把回取变成 miss/重跑(MOC-235 review #4)。
    pub persisted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolArtifactRecord {
    pub artifact_id: String,
    pub call_id: Option<String>,
    pub kind: String,
    pub raw_content: String,
    pub original_chars: usize,
    pub original_lines: usize,
    pub created_unix: i64,
    pub last_access_unix: i64,
    pub access_count: u64,
}

#[derive(Debug, Clone)]
struct ArtifactEntry {
    record: ToolArtifactRecord,
    ts: Instant,
    access_count: u64,
}

#[derive(Debug, Default)]
struct ArtifactStoreInner {
    entries: HashMap<String, ArtifactEntry>,
}

#[derive(Debug)]
pub struct ToolArtifactStore {
    max_size: usize,
    ttl: Duration,
    persisted_ttl: Duration,
    inner: Mutex<ArtifactStoreInner>,
    db: Mutex<Option<Connection>>,
}

impl ToolArtifactStore {
    pub fn new(max_size: usize, ttl: Duration) -> Self {
        Self {
            max_size: max_size.max(1),
            ttl,
            persisted_ttl: DEFAULT_PERSISTED_TTL,
            inner: Mutex::new(ArtifactStoreInner {
                entries: HashMap::new(),
            }),
            db: Mutex::new(None),
        }
    }

    pub fn with_db_path(
        max_size: usize,
        ttl: Duration,
        persisted_ttl: Duration,
        db_path: &Path,
    ) -> (Self, Option<String>) {
        let store = Self {
            max_size: max_size.max(1),
            ttl,
            persisted_ttl,
            inner: Mutex::new(ArtifactStoreInner {
                entries: HashMap::new(),
            }),
            db: Mutex::new(None),
        };
        let warn = match init_db(db_path) {
            Ok(conn) => {
                *store.db.lock().expect("artifact store db mutex poisoned") = Some(conn);
                None
            }
            Err(e) => Some(format!(
                "tool_artifacts.db init failed at {}: {e} — falling back to in-memory only",
                db_path.display()
            )),
        };
        (store, warn)
    }

    pub fn save(&self, call_id: Option<&str>, kind: &str, raw_content: &str) -> StoredToolArtifact {
        let now = unix_now();
        let record = ToolArtifactRecord {
            artifact_id: new_artifact_id(),
            call_id: call_id
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned),
            kind: kind.to_owned(),
            raw_content: raw_content.to_owned(),
            original_chars: raw_content.chars().count(),
            original_lines: raw_content.lines().count(),
            created_unix: now,
            last_access_unix: now,
            access_count: 0,
        };
        let mut stored = record.to_stored();

        // persisted = 真的写进了共享 DB。false(无 DB / INSERT 失败)→ 落 proxy 进程内存兜底,
        // 但跨进程 read_tool_artifact 读不到 → 摘要据此不告知可回取(MOC-235 review #4)。
        stored.persisted = match self.persist_save(&record) {
            Ok(persisted) => {
                if !persisted {
                    self.save_in_memory(record);
                }
                persisted
            }
            Err(e) => {
                log_artifact_warning(
                    "TOOL_ARTIFACT_DB_SAVE_FAILED",
                    format!(
                        "save artifact_id={} failed: {e}; falling back to in-memory store",
                        record.artifact_id
                    ),
                );
                self.save_in_memory(record);
                false
            }
        };

        stored
    }

    pub fn get(&self, artifact_id: &str) -> Option<ToolArtifactRecord> {
        match self.get_result(artifact_id) {
            Ok(record) => record,
            Err(e) => {
                log_artifact_warning(
                    "TOOL_ARTIFACT_DB_LOAD_FAILED",
                    format!("load artifact_id={artifact_id} failed: {e}"),
                );
                None
            }
        }
    }

    /// 同 [`get`] 但**保留 DB 读错误**(不吞成 `None`)。跨进程回取(MOC-235)用它区分
    /// 「真不存在 / 已过期」(`Ok(None)`)与「瞬时读失败:锁超时 / 损坏 / 权限」(`Err`)——
    /// 让 read_tool_artifact 对后者提示「稍后用同一 id 重试」, 而非误导模型「重跑原工具」
    /// (重跑正是本功能要消除的成本)。`get` 仍吞错返 `None`(保留旧调用方语义)。
    pub fn get_result(&self, artifact_id: &str) -> Result<Option<ToolArtifactRecord>, String> {
        if artifact_id.trim().is_empty() {
            return Ok(None);
        }
        {
            let mut inner = self.inner.lock().expect("artifact store mutex poisoned");
            let expired = inner
                .entries
                .get(artifact_id)
                .map(|entry| entry.ts.elapsed() > self.ttl)
                .unwrap_or(false);
            if expired {
                inner.entries.remove(artifact_id);
            } else if let Some(entry) = inner.entries.get_mut(artifact_id) {
                entry.access_count += 1;
                entry.record.access_count += 1;
                entry.record.last_access_unix = unix_now();
                return Ok(Some(entry.record.clone()));
            }
        }

        self.persist_load(artifact_id).map_err(|e| e.to_string())
    }

    pub fn clear(&self) {
        self.inner
            .lock()
            .expect("artifact store mutex poisoned")
            .entries
            .clear();
    }

    pub fn clear_all_persisted(&self) -> Result<usize, String> {
        self.clear();
        let mut guard = self.db.lock().expect("artifact store db mutex poisoned");
        let Some(conn) = guard.as_mut() else {
            return Ok(0);
        };
        conn.execute("DELETE FROM tool_artifacts", [])
            .map_err(|e| format!("tool_artifacts.db clear failed: {e}"))
    }

    pub fn evict_expired_persisted(&self) -> Result<usize, String> {
        let mut guard = self.db.lock().expect("artifact store db mutex poisoned");
        let Some(conn) = guard.as_mut() else {
            return Ok(0);
        };
        let cutoff = unix_now().saturating_sub(self.persisted_ttl.as_secs() as i64);
        conn.execute(
            "DELETE FROM tool_artifacts WHERE last_access_unix <= ?1",
            params![cutoff],
        )
        .map_err(|e| format!("tool_artifacts.db evict expired failed: {e}"))
    }

    fn save_in_memory(&self, record: ToolArtifactRecord) {
        let mut inner = self.inner.lock().expect("artifact store mutex poisoned");
        self.evict_expired_locked(&mut inner);
        if inner.entries.len() >= self.max_size && !inner.entries.contains_key(&record.artifact_id)
        {
            self.evict_oldest_locked(&mut inner);
        }
        inner.entries.insert(
            record.artifact_id.clone(),
            ArtifactEntry {
                record,
                ts: Instant::now(),
                access_count: 0,
            },
        );
    }

    /// 把 record 写进共享 DB。`Ok(true)` = 已持久化;`Ok(false)` = 无 DB(未持久化, 由 caller
    /// 落内存兜底);`Err` = INSERT 失败(同样由 caller 落内存)。返回值供 [`save`] 标 `persisted`。
    fn persist_save(&self, record: &ToolArtifactRecord) -> rusqlite::Result<bool> {
        let mut guard = self.db.lock().expect("artifact store db mutex poisoned");
        let Some(conn) = guard.as_mut() else {
            return Ok(false);
        };
        conn.execute(
            "INSERT INTO tool_artifacts \
             (artifact_id, call_id, kind, raw_content, original_chars, original_lines, \
              created_unix, last_access_unix, access_count) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0)",
            params![
                &record.artifact_id,
                record.call_id.as_deref(),
                &record.kind,
                &record.raw_content,
                record.original_chars as i64,
                record.original_lines as i64,
                record.created_unix,
                record.last_access_unix
            ],
        )?;
        Ok(true)
    }

    fn persist_load(&self, artifact_id: &str) -> rusqlite::Result<Option<ToolArtifactRecord>> {
        let mut guard = self.db.lock().expect("artifact store db mutex poisoned");
        let Some(conn) = guard.as_mut() else {
            return Ok(None);
        };
        let cutoff = unix_now().saturating_sub(self.persisted_ttl.as_secs() as i64);
        let record = conn
            .query_row(
                "SELECT artifact_id, call_id, kind, raw_content, original_chars, original_lines, \
                 created_unix, last_access_unix, access_count \
                 FROM tool_artifacts WHERE artifact_id = ?1 AND last_access_unix > ?2",
                params![artifact_id, cutoff],
                |r| {
                    Ok(ToolArtifactRecord {
                        artifact_id: r.get(0)?,
                        call_id: r.get(1)?,
                        kind: r.get(2)?,
                        raw_content: r.get(3)?,
                        original_chars: r.get::<_, i64>(4)? as usize,
                        original_lines: r.get::<_, i64>(5)? as usize,
                        created_unix: r.get(6)?,
                        last_access_unix: r.get(7)?,
                        access_count: r.get::<_, i64>(8)? as u64,
                    })
                },
            )
            .optional()?;

        if record.is_some() {
            let now = unix_now();
            if let Err(e) = conn.execute(
                "UPDATE tool_artifacts SET last_access_unix = ?1, access_count = access_count + 1 \
                 WHERE artifact_id = ?2",
                params![now, artifact_id],
            ) {
                log_artifact_warning(
                    "TOOL_ARTIFACT_DB_TOUCH_FAILED",
                    format!("touch artifact_id={artifact_id} failed: {e}"),
                );
            }
        }
        Ok(record)
    }

    fn evict_expired_locked(&self, inner: &mut ArtifactStoreInner) {
        let ttl = self.ttl;
        inner.entries.retain(|_, entry| entry.ts.elapsed() <= ttl);
    }

    fn evict_oldest_locked(&self, inner: &mut ArtifactStoreInner) {
        let Some(oldest_key) = inner
            .entries
            .iter()
            .min_by_key(|(_, entry)| (entry.access_count, entry.ts))
            .map(|(key, _)| key.clone())
        else {
            return;
        };
        inner.entries.remove(&oldest_key);
    }
}

impl ToolArtifactRecord {
    fn to_stored(&self) -> StoredToolArtifact {
        StoredToolArtifact {
            artifact_id: self.artifact_id.clone(),
            call_id: self.call_id.clone(),
            kind: self.kind.clone(),
            original_chars: self.original_chars,
            original_lines: self.original_lines,
            persisted: false, // 由 save() 按 persist_save 结果覆盖
        }
    }
}

fn init_db(db_path: &Path) -> rusqlite::Result<Connection> {
    if let Some(parent) = db_path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            log_artifact_warning(
                "TOOL_ARTIFACT_DB_PARENT_DIR_FAILED",
                format!("create_dir_all({}) failed: {e}", parent.display()),
            );
        }
    }
    let conn = Connection::open(db_path)?;
    if let Err(e) = conn.pragma_update(None, "journal_mode", "WAL") {
        log_artifact_warning(
            "TOOL_ARTIFACT_DB_PRAGMA_FAILED",
            format!("pragma journal_mode=WAL failed: {e}"),
        );
    }
    if let Err(e) = conn.pragma_update(None, "synchronous", "NORMAL") {
        log_artifact_warning(
            "TOOL_ARTIFACT_DB_PRAGMA_FAILED",
            format!("pragma synchronous=NORMAL failed: {e}"),
        );
    }
    // 跨进程并发(MOC-235): proxy 进程写(INSERT/touch)+ `--mcp-serve-webfetch` 进程读
    // (read_tool_artifact 回取)同时访问本 db。WAL 允许多读单写, 但两进程并发写(proxy
    // persist_save 与 MCP get() 的 last_access touch)会撞 SQLITE_BUSY。设 busy_timeout 让其
    // 短暂自旋重试而非立即失败(touch 失败本就 graceful, 但 INSERT 重试能少丢 artifact)。
    if let Err(e) = conn.busy_timeout(Duration::from_secs(5)) {
        log_artifact_warning(
            "TOOL_ARTIFACT_DB_BUSY_TIMEOUT_FAILED",
            format!("busy_timeout failed: {e}"),
        );
    }
    create_schema(&conn)?;
    Ok(conn)
}

fn create_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS tool_artifacts (
            artifact_id TEXT PRIMARY KEY,
            call_id TEXT,
            kind TEXT NOT NULL,
            raw_content TEXT NOT NULL,
            original_chars INTEGER NOT NULL,
            original_lines INTEGER NOT NULL,
            created_unix INTEGER NOT NULL,
            last_access_unix INTEGER NOT NULL,
            access_count INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_tool_artifacts_last_access \
            ON tool_artifacts(last_access_unix);
        CREATE INDEX IF NOT EXISTS idx_tool_artifacts_call_id \
            ON tool_artifacts(call_id);
        CREATE TABLE IF NOT EXISTS tool_artifacts_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        ",
    )?;
    conn.execute(
        "INSERT OR REPLACE INTO tool_artifacts_meta (key, value) VALUES ('schema_version', ?1)",
        params![SCHEMA_VERSION.to_string()],
    )?;
    Ok(())
}

fn new_artifact_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("tool_artifact_{nanos:x}_{seq:x}")
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn log_artifact_warning(error_id: &'static str, detail: String) {
    tracing::warn!(error_id, detail = %detail, "tool_artifacts.db");
    eprintln!("warning: [{error_id}] {detail}");
}

pub fn global_tool_artifact_store() -> &'static ToolArtifactStore {
    static STORE: OnceLock<ToolArtifactStore> = OnceLock::new();
    STORE.get_or_init(|| {
        #[cfg(test)]
        {
            ToolArtifactStore::new(DEFAULT_L1_SIZE, DEFAULT_L1_TTL)
        }
        #[cfg(not(test))]
        {
            let db_path = dirs::home_dir()
                .map(|p| p.join(".codex-app-transfer").join("tool_artifacts.db"));
            match db_path {
                Some(path) => {
                    let (store, warn) = ToolArtifactStore::with_db_path(
                        DEFAULT_L1_SIZE,
                        DEFAULT_L1_TTL,
                        DEFAULT_PERSISTED_TTL,
                        &path,
                    );
                    if let Some(msg) = warn {
                        log_artifact_warning("TOOL_ARTIFACT_DB_INIT_FAILED", msg);
                    }
                    if let Err(e) = store.evict_expired_persisted() {
                        log_artifact_warning("TOOL_ARTIFACT_DB_EVICT_FAILED", e);
                    }
                    store
                }
                None => ToolArtifactStore::new(DEFAULT_L1_SIZE, DEFAULT_L1_TTL),
            }
        }
    })
}

/// 按 `artifact_id` 取回被压缩外置的工具输出**全文**(不截断)。供 `--mcp-serve-webfetch`
/// 进程的 `read_tool_artifact` 工具跨进程读 —— 全文存在共享 `tool_artifacts.db`(WAL),
/// 由 proxy 侧 [`build_bounded_tool_output_summary`](request.rs)压缩时 `save` 落盘。
/// 返回:`Ok(Some(全文))` 命中 / `Ok(None)` 真不存在或已过期 / `Err` 瞬时读失败(锁超时
/// / 损坏)—— 调用方据此对「真没有」与「临时错误」给不同提示(后者建议重试同一调用)。MOC-235。
pub fn read_tool_artifact_raw(artifact_id: &str) -> Result<Option<String>, String> {
    global_tool_artifact_store()
        .get_result(artifact_id)
        .map(|opt| opt.map(|record| record.raw_content))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_memory_store_round_trips_raw_payload() {
        let store = ToolArtifactStore::new(8, Duration::from_secs(60));
        let stored = store.save(Some("call_a"), "command_output", "raw output");
        let record = store
            .get(&stored.artifact_id)
            .expect("artifact should exist");

        assert_eq!(record.call_id.as_deref(), Some("call_a"));
        assert_eq!(record.kind, "command_output");
        assert_eq!(record.raw_content, "raw output");
    }

    #[test]
    fn read_tool_artifact_raw_round_trips_via_global() {
        // MOC-235: 通用回取走 global store(test 下为内存档),save 后按 artifact_id 取回不截断全文。
        let stored = global_tool_artifact_store().save(
            Some("call_moc235"),
            "command_output",
            "FULL UNTRUNCATED PAYLOAD moc235",
        );
        assert_eq!(
            read_tool_artifact_raw(&stored.artifact_id)
                .expect("read should not error")
                .as_deref(),
            Some("FULL UNTRUNCATED PAYLOAD moc235")
        );
        // 真不存在 / 空 id → Ok(None)(非 Err);Err 仅留给瞬时 DB 读失败。
        assert!(read_tool_artifact_raw("nonexistent_moc235")
            .expect("miss is not an error")
            .is_none());
        assert!(read_tool_artifact_raw("   ")
            .expect("blank id is not an error")
            .is_none());
    }

    #[test]
    fn save_marks_persisted_only_when_db_backed() {
        // MOC-235 review #4: persisted 必须如实反映「是否进了共享 DB」—— 跨进程 reader 只能读 DB,
        // 摘要据此决定要不要告知模型可回取(内存档不告知, 否则给一个 reader 看不到的 id)。
        let mem = ToolArtifactStore::new(8, Duration::from_secs(60));
        assert!(
            !mem.save(Some("c"), "command_output", "raw").persisted,
            "无 DB 的内存档不应标 persisted"
        );

        let dir = tempfile::tempdir().unwrap();
        let (db, _warn) = ToolArtifactStore::with_db_path(
            8,
            Duration::from_secs(60),
            DEFAULT_PERSISTED_TTL,
            &dir.path().join("a.db"),
        );
        assert!(
            db.save(Some("c"), "command_output", "raw").persisted,
            "写入共享 DB 成功应标 persisted"
        );
    }

    #[test]
    fn sqlite_store_round_trips_raw_payload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tool_artifacts.db");
        let (store, warn) = ToolArtifactStore::with_db_path(
            8,
            Duration::from_secs(60),
            DEFAULT_PERSISTED_TTL,
            &path,
        );
        assert!(warn.is_none());

        let stored = store.save(Some("call_b"), "web_or_search", "large web payload");
        drop(store);

        let (store2, warn2) = ToolArtifactStore::with_db_path(
            8,
            Duration::from_secs(60),
            DEFAULT_PERSISTED_TTL,
            &path,
        );
        assert!(warn2.is_none());
        let record = store2
            .get(&stored.artifact_id)
            .expect("artifact should load");

        assert_eq!(record.call_id.as_deref(), Some("call_b"));
        assert_eq!(record.kind, "web_or_search");
        assert_eq!(record.raw_content, "large web payload");
    }
}
