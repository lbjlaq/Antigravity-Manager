use crate::proxy::config::LogRetentionConfig;
use crate::proxy::monitor::ProxyRequestLog;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use rusqlite::{params, Connection, OpenFlags};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};

static LOG_WRITE_LOCK: Mutex<()> = Mutex::new(());
static TOOL_SIGNATURE_DB: OnceLock<Mutex<Option<(PathBuf, Connection)>>> = OnceLock::new();

const THOUGHT_RAW_MAGIC: &[u8] = b"RAW1";
const THOUGHT_GZIP_MAGIC: &[u8] = b"AGZ1";
const MIN_GZIP_THOUGHT: usize = 384;
const SENTINEL_SIGNATURE: &str = "skip_thought_signature_validator";
const MIN_REAL_SIGNATURE: usize = 50;

fn persist_signature(signature: Option<&str>) -> Option<&str> {
    signature.filter(|s| s.len() >= MIN_REAL_SIGNATURE && *s != SENTINEL_SIGNATURE)
}

/// Tool turns match by tool_id at fill time — visible/tool_names are in the request JSON.
/// Only text-only turns keep visible so prefix matching still works after restart.
fn persist_visible<'a>(tool_ids: &[String], visible: &'a str) -> &'a str {
    if tool_ids.is_empty() {
        visible
    } else {
        ""
    }
}

fn pack_thought(s: &str) -> Vec<u8> {
    if s.len() >= MIN_GZIP_THOUGHT {
        let mut enc = GzEncoder::new(Vec::with_capacity(s.len() / 2), Compression::fast());
        if enc.write_all(s.as_bytes()).is_ok() {
            if let Ok(buf) = enc.finish() {
                if buf.len() + THOUGHT_GZIP_MAGIC.len() < s.len() {
                    let mut out = Vec::with_capacity(THOUGHT_GZIP_MAGIC.len() + buf.len());
                    out.extend_from_slice(THOUGHT_GZIP_MAGIC);
                    out.extend_from_slice(&buf);
                    return out;
                }
            }
        }
    }
    let mut out = Vec::with_capacity(THOUGHT_RAW_MAGIC.len() + s.len());
    out.extend_from_slice(THOUGHT_RAW_MAGIC);
    out.extend_from_slice(s.as_bytes());
    out
}

fn unpack_thought(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(THOUGHT_GZIP_MAGIC) {
        let mut decoder = GzDecoder::new(rest);
        let mut s = String::new();
        if decoder.read_to_string(&mut s).is_ok() {
            return s;
        }
    }
    if let Some(rest) = bytes.strip_prefix(THOUGHT_RAW_MAGIC) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    String::from_utf8_lossy(bytes).into_owned()
}

pub fn get_proxy_db_path() -> Result<PathBuf, String> {
    let data_dir = crate::modules::account::get_data_dir()?;
    Ok(data_dir.join("proxy_logs.db"))
}

pub fn get_thinking_db_path() -> Result<PathBuf, String> {
    let data_dir = crate::modules::account::get_data_dir()?;
    Ok(data_dir.join("thinking_store.db"))
}

fn apply_fast_pragmas(conn: &Connection) -> Result<(), String> {
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| e.to_string())?;
    conn.pragma_update(None, "busy_timeout", 5000)
        .map_err(|e| e.to_string())?;
    conn.pragma_update(None, "synchronous", "NORMAL")
        .map_err(|e| e.to_string())?;
    let _ = conn.pragma_update(None, "cache_size", -64000);
    let _ = conn.pragma_update(None, "temp_store", "MEMORY");
    let _ = conn.pragma_update(None, "mmap_size", 268435456);
    Ok(())
}

fn connect_db() -> Result<Connection, String> {
    let db_path = get_proxy_db_path()?;
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    apply_fast_pragmas(&conn)?;
    Ok(conn)
}

fn init_thinking_schema(conn: &Connection) -> Result<(), String> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS thinking_records (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_key TEXT NOT NULL,
            fingerprint TEXT NOT NULL,
            thought TEXT NOT NULL,
            signature TEXT,
            tool_ids TEXT NOT NULL,
            tool_names TEXT NOT NULL,
            visible TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            last_accessed INTEGER
        )",
        [],
    )
    .map_err(|e| e.to_string())?;

    // 动态升级：增加 primary_tool_id 列用于超长会话极速穿透点查与防叠加查重
    let _ = conn.execute(
        "ALTER TABLE thinking_records ADD COLUMN primary_tool_id TEXT",
        [],
    );

    // 1. 覆盖 load_thinking_records 的正向序列扫描 (ORDER BY id ASC)，避免内存二次排序
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_thinking_rec_seq ON thinking_records (session_key, id ASC)",
        [],
    );
    // 2. 覆盖基于 primary_tool_id 的快速穿透点查 (极简 Partial Index，极致纳秒响应)
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_thinking_rec_tool ON thinking_records (session_key, primary_tool_id) WHERE primary_tool_id IS NOT NULL",
        [],
    );
    // 3. 覆盖基于 fingerprint 的指纹点查
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_thinking_rec_fp ON thinking_records (session_key, fingerprint)",
        [],
    );
    // 4. 覆盖最新记录查询 (ORDER BY id DESC)
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_thinking_rec_latest ON thinking_records (session_key, id DESC)",
        [],
    );
    // 5. 覆盖会话创建时间索引
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_thinking_rec_session ON thinking_records (session_key, created_at ASC)",
        [],
    );
    // 6. 覆盖历史清理时间索引
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_thinking_rec_accessed ON thinking_records (last_accessed ASC)",
        [],
    );
    // 7. 覆盖基于 signature 的精准穿透点查 (极简 Partial Index，WHERE signature IS NOT NULL)
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_thinking_rec_sig ON thinking_records (session_key, signature) WHERE signature IS NOT NULL",
        [],
    );
    conn.execute(
        "CREATE TABLE IF NOT EXISTS thinking_sessions (
            session_key TEXT PRIMARY KEY,
            last_accessed INTEGER NOT NULL
        )",
        [],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS thinking_meta (
            k TEXT PRIMARY KEY,
            v TEXT NOT NULL
        )",
        [],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn open_thinking_db_at(db_path: &PathBuf) -> Result<Connection, String> {
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    apply_fast_pragmas(&conn)?;
    init_thinking_schema(&conn)?;
    Ok(conn)
}

fn open_thinking_db() -> Result<Connection, String> {
    let db_path = get_thinking_db_path()?;
    open_thinking_db_at(&db_path)
}

static THINKING_DB: OnceLock<Mutex<Option<(PathBuf, Connection)>>> = OnceLock::new();

pub struct ThinkingDbGuard(MutexGuard<'static, Option<(PathBuf, Connection)>>);

impl std::ops::Deref for ThinkingDbGuard {
    type Target = Connection;
    fn deref(&self) -> &Self::Target {
        &self.0.as_ref().expect("thinking db connection").1
    }
}

impl std::ops::DerefMut for ThinkingDbGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0.as_mut().expect("thinking db connection").1
    }
}

/// Process-lifetime connection to thinking_store.db.
/// Fill/hydrate must not open proxy_logs.db (it can be multi-GB on HDD).
/// Automatically tracks data directory changes and reuses connection with fast pragmas.
fn thinking_db() -> Result<ThinkingDbGuard, String> {
    let db_path = get_thinking_db_path()?;
    let slot = THINKING_DB.get_or_init(|| Mutex::new(None));
    let mut guard = slot.lock().map_err(|e| format!("thinking db lock: {e}"))?;
    if guard.as_ref().map(|(p, _)| p) != Some(&db_path) {
        let conn = open_thinking_db_at(&db_path)?;
        *guard = Some((db_path, conn));
    }
    Ok(ThinkingDbGuard(guard))
}

fn mark_thinking_imported(conn: &Connection) {
    let _ = conn.execute(
        "INSERT OR REPLACE INTO thinking_meta (k, v) VALUES ('imported_from_proxy_logs', '1')",
        [],
    );
}

/// Copy old thinking rows out of proxy_logs.db into thinking_store.db.
/// Never deletes the log DB. Old uncompressed rows stay readable via unpack_thought.
fn migrate_thinking_from_logs() -> Result<(), String> {
    let conn = thinking_db()?;
    let imported: Option<String> = conn
        .query_row(
            "SELECT v FROM thinking_meta WHERE k = 'imported_from_proxy_logs'",
            [],
            |r| r.get(0),
        )
        .ok();
    if imported.as_deref() == Some("1") {
        return Ok(());
    }

    let logs_path = get_proxy_db_path()?;
    if !logs_path.exists() {
        mark_thinking_imported(&conn);
        return Ok(());
    }

    let escaped = logs_path
        .to_string_lossy()
        .replace('\\', "/")
        .replace('\'', "''");
    if conn
        .execute(&format!("ATTACH DATABASE '{}' AS logs", escaped), [])
        .is_err()
    {
        return Ok(());
    }

    let has_table: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM logs.sqlite_master WHERE type='table' AND name='thinking_records'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    if has_table == 0 {
        let _ = conn.execute("DETACH DATABASE logs", []);
        mark_thinking_imported(&conn);
        return Ok(());
    }

    // Copy only rows not already present. Do not gzip/rewrite on import — that
    // would stall HDD by touching every old thought blob at startup.
    let copy_with_accessed = "INSERT INTO thinking_records (session_key, fingerprint, thought, signature, tool_ids, tool_names, visible, created_at, last_accessed)
             SELECT src.session_key, src.fingerprint, src.thought, src.signature, src.tool_ids, src.tool_names, src.visible, src.created_at,
                    COALESCE(src.last_accessed, src.created_at)
             FROM logs.thinking_records src
             WHERE NOT EXISTS (
                SELECT 1 FROM thinking_records t
                WHERE t.session_key = src.session_key
                  AND t.fingerprint = src.fingerprint
                  AND t.created_at = src.created_at
             )";
    let copy_basic = "INSERT INTO thinking_records (session_key, fingerprint, thought, signature, tool_ids, tool_names, visible, created_at, last_accessed)
             SELECT src.session_key, src.fingerprint, src.thought, src.signature, src.tool_ids, src.tool_names, src.visible, src.created_at, src.created_at
             FROM logs.thinking_records src
             WHERE NOT EXISTS (
                SELECT 1 FROM thinking_records t
                WHERE t.session_key = src.session_key
                  AND t.fingerprint = src.fingerprint
                  AND t.created_at = src.created_at
             )";
    let copied = match conn.execute(copy_with_accessed, []) {
        Ok(n) => n,
        Err(_) => {
            match conn.execute(copy_basic, []) {
                Ok(n) => n,
                Err(e) => {
                    let _ = conn.execute("DETACH DATABASE logs", []);
                    tracing::warn!("[ThinkingStore] Import from proxy_logs.db failed (will retry next start): {e}");
                    return Ok(());
                }
            }
        }
    };
    let _ = conn.execute(
        "INSERT OR IGNORE INTO thinking_sessions (session_key, last_accessed)
         SELECT session_key, MAX(created_at) FROM thinking_records GROUP BY session_key",
        [],
    );
    let _ = conn.execute("DETACH DATABASE logs", []);
    mark_thinking_imported(&conn);
    if copied > 0 {
        tracing::info!(
            "[ThinkingStore] Imported {} thinking row(s) from proxy_logs.db (old file kept as backup)",
            copied
        );
    }
    Ok(())
}

pub fn init_db() -> Result<(), String> {
    let conn = Connection::open(get_proxy_db_path()?).map_err(|e| e.to_string())?;
    // Must precede WAL for new databases. Upgrade legacy databases if auto_vacuum is 0.
    let auto_vacuum: i64 = conn
        .pragma_query_value(None, "auto_vacuum", |r| r.get(0))
        .unwrap_or(0);
    if auto_vacuum == 0 {
        let _ = conn.pragma_update(None, "auto_vacuum", "INCREMENTAL");
        let _ = conn.execute("VACUUM", []);
    }
    apply_fast_pragmas(&conn)?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS request_logs (
            id TEXT PRIMARY KEY,
            timestamp INTEGER,
            method TEXT,
            url TEXT,
            status INTEGER,
            duration INTEGER,
            model TEXT,
            error TEXT
        )",
        [],
    )
    .map_err(|e| e.to_string())?;

    // Try to add new columns (ignore errors if they exist)
    let _ = conn.execute("ALTER TABLE request_logs ADD COLUMN request_body TEXT", []);
    let _ = conn.execute(
        "ALTER TABLE request_logs ADD COLUMN upstream_request_body TEXT",
        [],
    );
    let _ = conn.execute("ALTER TABLE request_logs ADD COLUMN response_body TEXT", []);
    let _ = conn.execute(
        "ALTER TABLE request_logs ADD COLUMN input_tokens INTEGER",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE request_logs ADD COLUMN output_tokens INTEGER",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE request_logs ADD COLUMN cached_tokens INTEGER",
        [],
    );
    let _ = conn.execute("ALTER TABLE request_logs ADD COLUMN account_email TEXT", []);
    let _ = conn.execute("ALTER TABLE request_logs ADD COLUMN mapped_model TEXT", []);
    let _ = conn.execute("ALTER TABLE request_logs ADD COLUMN protocol TEXT", []);
    let _ = conn.execute("ALTER TABLE request_logs ADD COLUMN client_ip TEXT", []);
    let _ = conn.execute("ALTER TABLE request_logs ADD COLUMN username TEXT", []);
    let _ = conn.execute(
        "ALTER TABLE request_logs ADD COLUMN request_headers TEXT",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE request_logs ADD COLUMN upstream_request_headers TEXT",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE request_logs ADD COLUMN response_headers TEXT",
        [],
    );
    let _ = conn.execute("ALTER TABLE request_logs ADD COLUMN session_id TEXT", []);

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_timestamp ON request_logs (timestamp DESC)",
        [],
    )
    .map_err(|e| e.to_string())?;

    // Add status index for faster stats queries
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_status ON request_logs (status)",
        [],
    )
    .map_err(|e| e.to_string())?;

    // 高效复合索引：状态与时间戳倒序（针对错误筛选与分页排序，极大提升大数据量下的响应速度）
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_status_timestamp ON request_logs (status, timestamp DESC)",
        [],
    );

    // 复合索引：模型与时间戳倒序（针对模型级日志过滤与排序）
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_model_timestamp ON request_logs (model, timestamp DESC)",
        [],
    );

    // 复合索引：账号邮箱与时间戳倒序（针对多用户/多账号过滤）
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_account_timestamp ON request_logs (account_email, timestamp DESC)",
        [],
    );

    // 复合索引：客户端IP与时间戳倒序（针对安全审计与IP过滤）
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_client_ip_timestamp ON request_logs (client_ip, timestamp DESC)",
        [],
    );

    // 复合索引：用户名与时间戳倒序
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_username_timestamp ON request_logs (username, timestamp DESC)",
        [],
    );

    // 复合索引：会话与时间戳倒序（针对会话粒度运维分析）
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_session_timestamp ON request_logs (session_id, timestamp DESC)",
        [],
    );

    // 单列索引：协议类型
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_protocol ON request_logs (protocol)",
        [],
    );

    // 单列索引：请求方法
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_method ON request_logs (method)",
        [],
    );

    // 持久化工具签名表 (支持代理重启后根据 tool_id 秒级恢复真实加密签名)
    conn.execute(
        "CREATE TABLE IF NOT EXISTS tool_signatures (
            tool_id TEXT PRIMARY KEY,
            signature TEXT NOT NULL,
            created_at INTEGER NOT NULL
        )",
        [],
    )
    .map_err(|e| e.to_string())?;
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_tool_sig_created ON tool_signatures (created_at DESC)",
        [],
    );

    drop(conn);
    migrate_thinking_from_logs()?;

    Ok(())
}

fn map_request_log_row(row: &rusqlite::Row) -> rusqlite::Result<ProxyRequestLog> {
    Ok(ProxyRequestLog {
        id: row.get(0)?,
        timestamp: row.get(1)?,
        method: row.get(2)?,
        url: row.get(3)?,
        status: row.get(4)?,
        duration: row.get(5)?,
        model: row.get(6)?,
        error: row.get(7)?,
        request_body: row.get(8).unwrap_or(None),
        upstream_request_body: row.get(9).unwrap_or(None),
        response_body: row.get(10).unwrap_or(None),
        input_tokens: row.get(11).unwrap_or(None),
        output_tokens: row.get(12).unwrap_or(None),
        cached_tokens: row.get(13).unwrap_or(None),
        account_email: row.get(14).unwrap_or(None),
        mapped_model: row.get(15).unwrap_or(None),
        protocol: row.get(16).unwrap_or(None),
        client_ip: row.get(17).unwrap_or(None),
        username: row.get(18).unwrap_or(None),
        request_headers: row.get(19).unwrap_or(None),
        upstream_request_headers: row.get(20).unwrap_or(None),
        response_headers: row.get(21).unwrap_or(None),
        session_id: row.get(22).unwrap_or(None),
    })
}

pub fn save_tool_signature(tool_id: &str, signature: &str) -> Result<(), String> {
    if tool_id.is_empty() || signature.is_empty() {
        return Ok(());
    }
    let conn = connect_db()?;
    let now = chrono::Utc::now().timestamp_millis();
    conn.execute(
        "INSERT OR REPLACE INTO tool_signatures (tool_id, signature, created_at) VALUES (?1, ?2, ?3)",
        params![tool_id, signature, now],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn load_tool_signature(tool_id: &str) -> Result<Option<String>, String> {
    if tool_id.is_empty() {
        return Ok(None);
    }
    let db_path = get_proxy_db_path()?;
    let mut db = TOOL_SIGNATURE_DB
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|e| format!("tool signature db lock: {e}"))?;
    if db.as_ref().map(|(path, _)| path) != Some(&db_path) {
        let conn = Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| e.to_string())?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        *db = Some((db_path, conn));
    }
    let conn = &db
        .as_ref()
        .ok_or("tool signature db was not initialized")?
        .1;
    // Dropping rows and the cached statement ends the read before releasing the lock.
    let mut stmt = conn
        .prepare_cached("SELECT signature FROM tool_signatures WHERE tool_id = ?1 LIMIT 1")
        .map_err(|e| e.to_string())?;
    let mut rows = stmt.query(params![tool_id]).map_err(|e| e.to_string())?;
    if let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let sig: String = row.get(0).map_err(|e| e.to_string())?;
        Ok(Some(sig))
    } else {
        Ok(None)
    }
}

#[derive(Debug, Clone)]
pub struct PersistedThinkingRecord {
    pub fingerprint: String,
    pub thought: String,
    pub signature: Option<String>,
    pub tool_ids: Vec<String>,
    pub tool_names: Vec<String>,
    pub visible: String,
}

pub fn save_thinking_record(
    session_key: &str,
    fingerprint: &str,
    thought: &str,
    signature: Option<&str>,
    tool_ids: &[String],
    _tool_names: &[String],
    visible: &str,
) -> Result<(), String> {
    if session_key.is_empty() {
        return Ok(());
    }
    let mut conn = thinking_db()?;
    let now = chrono::Utc::now().timestamp_millis();
    let tool_ids_json = serde_json::to_string(tool_ids).unwrap_or_else(|_| "[]".to_string());
    let primary_tool_id = tool_ids.first().map(|s| s.as_str());
    // tool_names / full visible for tool turns are reconstructable from the next
    // request JSON at fill time. Do not write them.
    let visible_persist = persist_visible(tool_ids, visible);
    let packed_thought = pack_thought(thought);
    let signature = persist_signature(signature);

    // 智能防叠加与幂等查重：只允许合并/更新当前会话中的【最新一条】活跃轮次（流式碎片拼接或更长思考补齐）
    // 绝不能回溯更新历史早期轮次！
    let latest_row: Option<(i64, usize, Option<String>, String, Option<String>)> = conn
        .query_row(
            "SELECT id, length(thought), signature, fingerprint, primary_tool_id
             FROM thinking_records
             WHERE session_key = ?1
             ORDER BY id DESC LIMIT 1",
            params![session_key],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .ok();

    let existing_id: Option<(i64, usize, Option<String>)> = match latest_row {
        Some((id, len, sig, ref last_fp, ref last_tool_id)) => {
            let is_match = if let Some(p_id) = primary_tool_id {
                last_tool_id.as_deref() == Some(p_id)
            } else {
                last_fp == fingerprint && last_tool_id.is_none()
            };
            if is_match {
                Some((id, len, sig))
            } else {
                None
            }
        }
        None => None,
    };

    if let Some((id, old_thought_len, old_sig)) = existing_id {
        // 已存在记录：检查是否需要更新（防止将已有实质思考覆盖为占位符，但允许补全更长思考或有效签名）
        let incoming_has_meaningful_thought =
            !crate::proxy::thinking_store::is_placeholder_thought(thought)
                && !thought.trim().is_empty();
        let old_is_dummy = old_thought_len <= 10; // "RAW1..." 或占位符非常短

        let should_update_thought = incoming_has_meaningful_thought || old_is_dummy;
        let effective_sig = signature.or(old_sig.as_deref());

        if should_update_thought {
            let mut stmt = conn
                .prepare_cached(
                    "UPDATE thinking_records
                     SET thought = ?1, signature = ?2, tool_ids = ?3, visible = ?4, created_at = ?5, primary_tool_id = ?6
                     WHERE id = ?7",
                )
                .map_err(|e| e.to_string())?;
            stmt.execute(params![
                packed_thought.as_slice(),
                effective_sig,
                &tool_ids_json,
                visible_persist,
                now,
                primary_tool_id,
                id,
            ])
            .map_err(|e| e.to_string())?;
        } else if signature.is_some() && signature != old_sig.as_deref() {
            // 仅更新签名，保留已有的高质量实质思考
            let mut stmt = conn
                .prepare_cached(
                    "UPDATE thinking_records
                     SET signature = ?1, created_at = ?2
                     WHERE id = ?3",
                )
                .map_err(|e| e.to_string())?;
            stmt.execute(params![signature, now, id])
                .map_err(|e| e.to_string())?;
        }
    } else {
        // 全新轮次：插入新记录（包含 primary_tool_id 列）
        let mut stmt = conn
            .prepare_cached(
                "INSERT INTO thinking_records (session_key, fingerprint, thought, signature, tool_ids, tool_names, visible, created_at, primary_tool_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, '[]', ?6, ?7, ?8)",
            )
            .map_err(|e| e.to_string())?;
        stmt.execute(params![
            session_key,
            fingerprint,
            packed_thought.as_slice(),
            signature,
            &tool_ids_json,
            visible_persist,
            now,
            primary_tool_id,
        ])
        .map_err(|e| e.to_string())?;
    }

    let mut session_stmt = conn
        .prepare_cached(
            "INSERT INTO thinking_sessions (session_key, last_accessed) VALUES (?1, ?2)
             ON CONFLICT(session_key) DO UPDATE SET last_accessed = excluded.last_accessed",
        )
        .map_err(|e| e.to_string())?;
    let _ = session_stmt.execute(params![session_key, now]);

    Ok(())
}

pub fn load_thinking_records(session_key: &str) -> Result<Vec<PersistedThinkingRecord>, String> {
    if session_key.is_empty() {
        return Ok(Vec::new());
    }
    let mut conn = thinking_db()?;
    let mut stmt = conn
        .prepare_cached(
            "SELECT fingerprint, thought, signature, tool_ids, tool_names, visible
             FROM thinking_records
             WHERE session_key = ?1
             ORDER BY id ASC",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![session_key], |row| {
            let fp: String = row.get(0)?;
            let thought_raw: Vec<u8> = row.get(1)?;
            let signature: Option<String> = row.get(2)?;
            let tool_ids_str: String = row.get(3)?;
            let tool_names_str: String = row.get(4)?;
            let visible: String = row.get(5)?;
            Ok((
                fp,
                thought_raw,
                signature,
                tool_ids_str,
                tool_names_str,
                visible,
            ))
        })
        .map_err(|e| e.to_string())?;

    let mut result = Vec::new();
    for row in rows {
        if let Ok((fp, thought_raw, signature, tool_ids_str, tool_names_str, visible)) = row {
            let tool_ids: Vec<String> = serde_json::from_str(&tool_ids_str).unwrap_or_default();
            let tool_names: Vec<String> = serde_json::from_str(&tool_names_str).unwrap_or_default();
            result.push(PersistedThinkingRecord {
                fingerprint: fp,
                thought: unpack_thought(&thought_raw),
                signature: persist_signature(signature.as_deref()).map(str::to_string),
                tool_ids,
                tool_names,
                visible,
            });
        }
    }
    Ok(result)
}

/// 根据 tool_id 精准穿透点查历史思考（利用 primary_tool_id 极速 Partial Index）
pub fn load_thinking_by_tool_id(
    session_key: &str,
    tool_id: &str,
) -> Result<Option<PersistedThinkingRecord>, String> {
    if session_key.is_empty() || tool_id.is_empty() {
        return Ok(None);
    }
    let mut conn = thinking_db()?;

    // 1. Fast Path: 优先通过 primary_tool_id 走专属索引极速点查 (0ms 纳秒级命中)
    let mut stmt = conn
        .prepare_cached(
            "SELECT fingerprint, thought, signature, tool_ids, tool_names, visible
             FROM thinking_records
             WHERE session_key = ?1 AND primary_tool_id = ?2
             ORDER BY id DESC LIMIT 1",
        )
        .map_err(|e| e.to_string())?;

    let mut rows = stmt
        .query(params![session_key, tool_id])
        .map_err(|e| e.to_string())?;

    if let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let fp: String = row.get(0).map_err(|e| e.to_string())?;
        let thought_raw: Vec<u8> = row.get(1).map_err(|e| e.to_string())?;
        let signature: Option<String> = row.get(2).map_err(|e| e.to_string())?;
        let tool_ids_str: String = row.get(3).map_err(|e| e.to_string())?;
        let tool_names_str: String = row.get(4).map_err(|e| e.to_string())?;
        let visible: String = row.get(5).map_err(|e| e.to_string())?;
        let tool_ids: Vec<String> = serde_json::from_str(&tool_ids_str).unwrap_or_default();
        let tool_names: Vec<String> = serde_json::from_str(&tool_names_str).unwrap_or_default();
        return Ok(Some(PersistedThinkingRecord {
            fingerprint: fp,
            thought: unpack_thought(&thought_raw),
            signature: persist_signature(signature.as_deref()).map(str::to_string),
            tool_ids,
            tool_names,
            visible,
        }));
    }

    // 2. Fallback Path: 针对历史旧记录 (tool_ids 列表内模糊包含)
    let pattern = format!("%\"{}\"%", tool_id);
    let mut fallback_stmt = conn
        .prepare_cached(
            "SELECT fingerprint, thought, signature, tool_ids, tool_names, visible
             FROM thinking_records
             WHERE session_key = ?1 AND tool_ids LIKE ?2
             ORDER BY id DESC LIMIT 1",
        )
        .map_err(|e| e.to_string())?;

    let mut fallback_rows = fallback_stmt
        .query(params![session_key, pattern])
        .map_err(|e| e.to_string())?;

    if let Some(row) = fallback_rows.next().map_err(|e| e.to_string())? {
        let fp: String = row.get(0).map_err(|e| e.to_string())?;
        let thought_raw: Vec<u8> = row.get(1).map_err(|e| e.to_string())?;
        let signature: Option<String> = row.get(2).map_err(|e| e.to_string())?;
        let tool_ids_str: String = row.get(3).map_err(|e| e.to_string())?;
        let tool_names_str: String = row.get(4).map_err(|e| e.to_string())?;
        let visible: String = row.get(5).map_err(|e| e.to_string())?;
        let tool_ids: Vec<String> = serde_json::from_str(&tool_ids_str).unwrap_or_default();
        let tool_names: Vec<String> = serde_json::from_str(&tool_names_str).unwrap_or_default();
        Ok(Some(PersistedThinkingRecord {
            fingerprint: fp,
            thought: unpack_thought(&thought_raw),
            signature: persist_signature(signature.as_deref()).map(str::to_string),
            tool_ids,
            tool_names,
            visible,
        }))
    } else {
        Ok(None)
    }
}

/// 根据 signature 精准穿透点查历史思考（利用 idx_thinking_rec_sig 索引）
pub fn load_thinking_by_signature(
    session_key: &str,
    signature: &str,
) -> Result<Option<PersistedThinkingRecord>, String> {
    if session_key.is_empty() || signature.is_empty() {
        return Ok(None);
    }
    let mut conn = thinking_db()?;
    let mut stmt = conn
        .prepare_cached(
            "SELECT fingerprint, thought, signature, tool_ids, tool_names, visible
             FROM thinking_records
             WHERE session_key = ?1 AND signature = ?2
             ORDER BY id DESC LIMIT 1",
        )
        .map_err(|e| e.to_string())?;

    let mut rows = stmt
        .query(params![session_key, signature])
        .map_err(|e| e.to_string())?;

    if let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let fp: String = row.get(0).map_err(|e| e.to_string())?;
        let thought_raw: Vec<u8> = row.get(1).map_err(|e| e.to_string())?;
        let signature: Option<String> = row.get(2).map_err(|e| e.to_string())?;
        let tool_ids_str: String = row.get(3).map_err(|e| e.to_string())?;
        let tool_names_str: String = row.get(4).map_err(|e| e.to_string())?;
        let visible: String = row.get(5).map_err(|e| e.to_string())?;
        let tool_ids: Vec<String> = serde_json::from_str(&tool_ids_str).unwrap_or_default();
        let tool_names: Vec<String> = serde_json::from_str(&tool_names_str).unwrap_or_default();
        Ok(Some(PersistedThinkingRecord {
            fingerprint: fp,
            thought: unpack_thought(&thought_raw),
            signature: persist_signature(signature.as_deref()).map(str::to_string),
            tool_ids,
            tool_names,
            visible,
        }))
    } else {
        Ok(None)
    }
}

/// 根据 fingerprint 精准穿透点查纯文本历史思考（利用 idx_thinking_rec_fp 索引）
pub fn load_thinking_by_fingerprint(
    session_key: &str,
    fingerprint: &str,
) -> Result<Option<PersistedThinkingRecord>, String> {
    if session_key.is_empty() || fingerprint.is_empty() {
        return Ok(None);
    }
    let mut conn = thinking_db()?;
    let mut stmt = conn
        .prepare_cached(
            "SELECT fingerprint, thought, signature, tool_ids, tool_names, visible
             FROM thinking_records
             WHERE session_key = ?1 AND fingerprint = ?2
             ORDER BY id DESC LIMIT 1",
        )
        .map_err(|e| e.to_string())?;

    let mut rows = stmt
        .query(params![session_key, fingerprint])
        .map_err(|e| e.to_string())?;

    if let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let fp: String = row.get(0).map_err(|e| e.to_string())?;
        let thought_raw: Vec<u8> = row.get(1).map_err(|e| e.to_string())?;
        let signature: Option<String> = row.get(2).map_err(|e| e.to_string())?;
        let tool_ids_str: String = row.get(3).map_err(|e| e.to_string())?;
        let tool_names_str: String = row.get(4).map_err(|e| e.to_string())?;
        let visible: String = row.get(5).map_err(|e| e.to_string())?;
        let tool_ids: Vec<String> = serde_json::from_str(&tool_ids_str).unwrap_or_default();
        let tool_names: Vec<String> = serde_json::from_str(&tool_names_str).unwrap_or_default();
        Ok(Some(PersistedThinkingRecord {
            fingerprint: fp,
            thought: unpack_thought(&thought_raw),
            signature: persist_signature(signature.as_deref()).map(str::to_string),
            tool_ids,
            tool_names,
            visible,
        }))
    } else {
        Ok(None)
    }
}

pub fn touch_thinking_session(session_key: &str) -> Result<usize, String> {
    if session_key.is_empty() {
        return Ok(0);
    }
    let conn = thinking_db()?;
    let now = chrono::Utc::now().timestamp_millis();
    // Touch a 1-row session table. Never UPDATE thinking_records here — that
    // rewrites every thought/visible TEXT blob for the session.
    conn.execute(
        "INSERT INTO thinking_sessions (session_key, last_accessed) VALUES (?1, ?2)
         ON CONFLICT(session_key) DO UPDATE SET last_accessed = excluded.last_accessed",
        params![session_key, now],
    )
    .map_err(|e| e.to_string())
}

pub fn delete_thinking_records_except_fingerprints(
    session_key: &str,
    keep_fps: &[String],
) -> Result<usize, String> {
    if session_key.is_empty() || keep_fps.is_empty() {
        return Ok(0);
    }
    let conn = thinking_db()?;
    let fps_json = serde_json::to_string(keep_fps).unwrap_or_else(|_| "[]".to_string());
    conn.execute(
        "DELETE FROM thinking_records
         WHERE session_key = ?1
         AND fingerprint NOT IN (SELECT value FROM json_each(?2))",
        params![session_key, fps_json],
    )
    .map_err(|e| e.to_string())
}

pub fn delete_thinking_records_for_session(session_key: &str) -> Result<usize, String> {
    let conn = thinking_db()?;
    let _ = conn.execute(
        "DELETE FROM thinking_sessions WHERE session_key = ?1",
        params![session_key],
    );
    conn.execute(
        "DELETE FROM thinking_records WHERE session_key = ?1",
        params![session_key],
    )
    .map_err(|e| e.to_string())
}

/// 全量清空思考块数据库 (仅清空 thinking_records / thinking_sessions / tool_signatures，绝不触碰 request_logs 日志)
pub fn clear_all_thinking_data() -> Result<usize, String> {
    let mut total_deleted = 0;
    // 1. 清空 thinking_store.db 中的记录与会话
    let conn = thinking_db()?;
    let deleted = conn
        .execute("DELETE FROM thinking_records", [])
        .map_err(|e| e.to_string())?;
    total_deleted += deleted;
    let _ = conn.execute("DELETE FROM thinking_sessions", []);
    let _ = conn.execute("VACUUM", []);

    // 2. 清空 proxy_logs.db 中残留的历史工具签名表与陈旧思考表 (绝不触碰 request_logs)
    if let Ok(log_conn) = connect_db() {
        let _ = log_conn.execute("DELETE FROM tool_signatures", []);
        let _ = log_conn.execute("DELETE FROM thinking_records", []);
        let _ = log_conn.execute("DELETE FROM thinking_sessions", []);
    }

    Ok(total_deleted)
}

pub fn cleanup_old_thinking_records(days: i64) -> Result<usize, String> {
    let cutoff = chrono::Utc::now().timestamp_millis() - (days * 24 * 3600 * 1000);
    let deleted_tools = connect_db()
        .ok()
        .and_then(|conn| {
            conn.execute(
                "DELETE FROM tool_signatures WHERE created_at < ?1",
                params![cutoff],
            )
            .ok()
        })
        .unwrap_or(0);
    let conn = thinking_db()?;
    let deleted_records = conn
        .execute(
            "DELETE FROM thinking_records WHERE session_key IN (
                SELECT session_key FROM thinking_sessions WHERE last_accessed < ?1
             ) OR (
                session_key NOT IN (SELECT session_key FROM thinking_sessions)
                AND COALESCE(last_accessed, created_at) < ?1
             )",
            params![cutoff],
        )
        .unwrap_or(0);
    let _ = conn.execute(
        "DELETE FROM thinking_sessions WHERE last_accessed < ?1",
        params![cutoff],
    );
    Ok(deleted_tools + deleted_records)
}

pub fn apply_retention(policy: &LogRetentionConfig) -> Result<(usize, usize), String> {
    let _guard = LOG_WRITE_LOCK.lock().map_err(|e| e.to_string())?;
    let conn = connect_db()?;
    apply_retention_with_connection(&conn, policy)
}

fn apply_retention_with_connection(
    conn: &Connection,
    policy: &LogRetentionConfig,
) -> Result<(usize, usize), String> {
    // 请求体不再按时间强制清空，完全由容量上限与行数滑动窗口整体托管，保留完整报文
    let bodies_cleared = 0;

    // 注意：已移除基于 max_age_days 的按天整行删除逻辑，改为条数上限与空间上限滑动窗口淘汰
    let mut rows_deleted = 0;
    if policy.max_rows > 0 {
        rows_deleted += conn.execute(
            "DELETE FROM request_logs WHERE id NOT IN (SELECT id FROM request_logs ORDER BY timestamp DESC LIMIT ?1)",
            [policy.max_rows],
        ).map_err(|e| e.to_string())?;
    }

    // 按空间上限执行 30% 滑动窗口尾部淘汰
    let budget = policy.budget_bytes();
    if budget > 0 && disk_bytes(conn).unwrap_or(0) > budget {
        let (evicted, _) = evict_sliding_window(conn, budget)?;
        rows_deleted += evicted;
    }

    reclaim_space(conn)?;
    Ok((bodies_cleared, rows_deleted))
}

fn reclaim_space(conn: &Connection) -> Result<(), String> {
    let checkpoint = || -> Result<(), String> {
        let busy: i64 = conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))
            .map_err(|e| e.to_string())?;
        if busy != 0 {
            tracing::warn!("proxy log checkpoint busy");
        }
        Ok(())
    };
    checkpoint()?;

    let auto_vacuum: i64 = conn
        .pragma_query_value(None, "auto_vacuum", |r| r.get(0))
        .unwrap_or(0);

    if auto_vacuum == 2 {
        // Draining all free pages incrementally in batches
        for _ in 0..50 {
            let free: u64 = conn
                .pragma_query_value(None, "freelist_count", |r| r.get(0))
                .unwrap_or(0);
            if free == 0 {
                break;
            }
            let step = free.min(1000);
            let mut vacuum = conn
                .prepare(&format!("PRAGMA incremental_vacuum({})", step))
                .map_err(|e| e.to_string())?;
            let mut pages = vacuum.query([]).map_err(|e| e.to_string())?;
            while pages.next().map_err(|e| e.to_string())?.is_some() {}
            drop(pages);
        }
    } else {
        // Non-incremental or legacy database: full VACUUM to shrink disk size
        let _ = conn.execute("VACUUM", []);
    }

    checkpoint()
}

fn disk_bytes(conn: &Connection) -> Result<u64, String> {
    let path = conn.path().ok_or("proxy log database has no file path")?;
    [PathBuf::from(path), PathBuf::from(format!("{path}-wal"))]
        .iter()
        .try_fold(0u64, |total, path| match std::fs::metadata(path) {
            Ok(metadata) => Ok(total.saturating_add(metadata.len())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(total),
            Err(e) => Err(e.to_string()),
        })
}

pub fn get_proxy_db_disk_bytes() -> Result<u64, String> {
    let conn = connect_db()?;
    disk_bytes(&conn)
}

/// 滑动窗口尾部淘汰机制：
/// 当日志数据库达到或即将超过预算上限时，自动清理最尾部（最早）的日志，
/// 一次性挤出最大存储空间的 30%（即让体积回落到 <= 70% 预算内），
/// 并记录日志，随后返回清理的记录数与释放字节数。
pub fn evict_sliding_window(conn: &Connection, budget: u64) -> Result<(usize, u64), String> {
    if budget == 0 {
        return Ok((0, 0));
    }
    let before_bytes = disk_bytes(conn)?;
    // 一次挤出最大空间的 30% (即目标保留 <= 70% 的最大上限)
    let evict_quota = (budget as f64 * 0.30) as u64;
    let target_bytes = budget.saturating_sub(evict_quota);

    if before_bytes <= target_bytes {
        return Ok((0, 0));
    }

    let mut total_deleted: usize = 0;
    // 循环按批次从最尾部（最早记录，timestamp ASC）清理
    for _ in 0..100 {
        let deleted = conn
            .execute(
                "DELETE FROM request_logs WHERE id IN (
                SELECT id FROM request_logs ORDER BY timestamp ASC LIMIT 250
            )",
                [],
            )
            .map_err(|e| e.to_string())?;

        if deleted == 0 {
            break;
        }
        total_deleted += deleted;
        reclaim_space(conn)?;

        let current_bytes = disk_bytes(conn)?;
        if current_bytes <= target_bytes {
            break;
        }
    }

    let after_bytes = disk_bytes(conn)?;
    let freed_bytes = before_bytes.saturating_sub(after_bytes);

    if total_deleted > 0 {
        tracing::info!(
            "[ProxyLog Sliding Window] Disk budget reached ({:.2} GB limit). Evicted {} tail records, freed {:.2} MB (target 30% quota: {:.2} MB). Current size: {:.2} MB.",
            budget as f64 / 1_073_741_824.0,
            total_deleted,
            freed_bytes as f64 / 1_048_576.0,
            evict_quota as f64 / 1_048_576.0,
            after_bytes as f64 / 1_048_576.0
        );
    }

    Ok((total_deleted, freed_bytes))
}

fn projected_bytes(conn: &Connection, log_bytes: u64) -> Result<u64, String> {
    let free: u64 = conn
        .pragma_query_value(None, "freelist_count", |r| r.get(0))
        .map_err(|e| e.to_string())?;
    let page_size: u64 = conn
        .pragma_query_value(None, "page_size", |r| r.get(0))
        .map_err(|e| e.to_string())?;
    // Free pages avoid database growth, but still need WAL frames during the transaction.
    Ok(disk_bytes(conn)?
        .saturating_add(log_bytes.saturating_mul(2))
        .saturating_add(log_bytes.saturating_sub(free.saturating_mul(page_size)))
        .saturating_add(64 * 1024))
}

fn make_room(conn: &Connection, budget: u64, log_bytes: u64) -> Result<(), String> {
    if budget == 0 {
        return Err("proxy log disk budget is 0".to_string());
    }
    if projected_bytes(conn, log_bytes)? <= budget {
        return Ok(());
    }
    reclaim_space(conn)?;
    if projected_bytes(conn, log_bytes)? <= budget {
        return Ok(());
    }

    let auto_vacuum: i64 = conn
        .pragma_query_value(None, "auto_vacuum", |r| r.get(0))
        .unwrap_or(0);
    // Legacy files cannot shrink: even reusing all free pages still needs WAL headroom.
    if auto_vacuum == 0
        && disk_bytes(conn)?
            .saturating_add(log_bytes.saturating_mul(2))
            .saturating_add(64 * 1024)
            > budget
    {
        return Err("legacy proxy log database cannot shrink within budget".to_string());
    }

    // 优先触发 30% 滑动窗口机制清理最尾部历史日志
    let (evicted, _) = evict_sliding_window(conn, budget)?;
    if evicted > 0 {
        reclaim_space(conn)?;
    }

    if projected_bytes(conn, log_bytes)? <= budget {
        return Ok(());
    }

    let target = budget.saturating_mul(7) / 10;
    // Bounded work per write, oldest bodies first, then oldest summaries. No full-body reads.
    for _ in 0..8 {
        let before = projected_bytes(conn, log_bytes)?;
        let cleared = conn.execute(
            "UPDATE request_logs SET request_body = NULL, upstream_request_body = NULL, response_body = NULL,
             request_headers = NULL, upstream_request_headers = NULL, response_headers = NULL WHERE id IN
             (SELECT id FROM request_logs WHERE request_body IS NOT NULL OR upstream_request_body IS NOT NULL OR response_body IS NOT NULL ORDER BY timestamp ASC LIMIT 64)", []
        ).map_err(|e| e.to_string())?;
        if cleared == 0 {
            conn.execute("DELETE FROM request_logs WHERE id IN (SELECT id FROM request_logs ORDER BY timestamp ASC LIMIT 64)", [])
                .map_err(|e| e.to_string())?;
        }
        reclaim_space(conn)?;
        let after = projected_bytes(conn, log_bytes)?;
        if after <= target {
            return Ok(());
        }
        if after >= before {
            break;
        }
    }
    if projected_bytes(conn, log_bytes)? <= budget {
        Ok(())
    } else {
        Err("proxy log disk budget exhausted".to_string())
    }
}

pub fn save_log(log: ProxyRequestLog) -> Result<(), String> {
    let _guard = LOG_WRITE_LOCK.lock().map_err(|e| e.to_string())?;
    // Read the file for every admitted write, including after a runtime budget change.
    let policy = crate::modules::config::load_app_config()?
        .proxy
        .log_retention;
    let conn = connect_db()?;
    save_log_with_connection(&conn, log, &policy)
}

fn save_log_with_connection(
    conn: &Connection,
    mut log: ProxyRequestLog,
    policy: &LogRetentionConfig,
) -> Result<(), String> {
    conn.busy_timeout(std::time::Duration::from_millis(250))
        .map_err(|e| e.to_string())?;
    log.error = log
        .error
        .as_ref()
        .map(|error| error.chars().take(1024).collect());
    let budget = policy.budget_bytes();
    let summary_bytes = [&log.id, &log.method, &log.url]
        .iter()
        .map(|s| s.len() as u64)
        .sum::<u64>()
        + [
            &log.model,
            &log.mapped_model,
            &log.account_email,
            &log.client_ip,
            &log.error,
            &log.protocol,
            &log.username,
        ]
        .iter()
        .filter_map(|s| s.as_ref())
        .map(|s| s.len() as u64)
        .sum::<u64>()
        + 1024;
    let body_bytes = [
        &log.request_body,
        &log.upstream_request_body,
        &log.response_body,
        &log.request_headers,
        &log.upstream_request_headers,
        &log.response_headers,
    ]
    .iter()
    .filter_map(|s| s.as_ref())
    .map(|s| s.len() as u64)
    .sum::<u64>();
    let mut log_bytes = summary_bytes.saturating_add(body_bytes);
    if log_bytes.saturating_mul(3).saturating_add(64 * 1024) > budget / 5 * 4 {
        log.request_body = None;
        log.upstream_request_body = None;
        log.response_body = None;
        log.request_headers = None;
        log.upstream_request_headers = None;
        log.response_headers = None;
        log_bytes = summary_bytes;
    }
    if log_bytes.saturating_mul(3).saturating_add(64 * 1024) > budget {
        return Err("proxy log summary exceeds disk budget".to_string());
    }
    make_room(conn, budget, log_bytes)?;

    conn.execute(
        "INSERT INTO request_logs (id, timestamp, method, url, status, duration, model, error, request_body, upstream_request_body, response_body, input_tokens, output_tokens, cached_tokens, account_email, mapped_model, protocol, client_ip, username, request_headers, upstream_request_headers, response_headers, session_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23)",
        params![
            log.id,
            log.timestamp,
            log.method,
            log.url,
            log.status,
            log.duration,
            log.model,
            log.error,
            log.request_body,
            log.upstream_request_body,
            log.response_body,
            log.input_tokens,
            log.output_tokens,
            log.cached_tokens,
            log.account_email,
            log.mapped_model,
            log.protocol,
            log.client_ip,
            log.username,
            log.request_headers,
            log.upstream_request_headers,
            log.response_headers,
            log.session_id,
        ],
    ).map_err(|e| e.to_string())?;

    Ok(())
}

/// Get logs summary (without large request_body and response_body fields) with pagination
pub fn get_logs_summary(limit: usize, offset: usize) -> Result<Vec<ProxyRequestLog>, String> {
    let conn = connect_db()?;

    let mut stmt = conn
        .prepare(
            "SELECT id, timestamp, method, url, status, duration, model, substr(error, 1, 1024),
                NULL as request_body, NULL as upstream_request_body, NULL as response_body,
                input_tokens, output_tokens, cached_tokens, account_email, mapped_model, protocol, client_ip, username,
                NULL as request_headers, NULL as upstream_request_headers, NULL as response_headers,
                session_id
         FROM request_logs 
         ORDER BY timestamp DESC 
         LIMIT ?1 OFFSET ?2",
        )
        .map_err(|e| e.to_string())?;

    let logs_iter = stmt
        .query_map([limit, offset], map_request_log_row)
        .map_err(|e| e.to_string())?;

    let mut logs = Vec::new();
    for log in logs_iter {
        logs.push(log.map_err(|e| e.to_string())?);
    }
    Ok(logs)
}

/// Get logs (backward compatible, calls get_logs_summary)
pub fn get_logs(limit: usize) -> Result<Vec<ProxyRequestLog>, String> {
    get_logs_summary(limit, 0)
}

pub fn get_stats() -> Result<crate::proxy::monitor::ProxyStats, String> {
    let conn = connect_db()?;

    // Optimized: Use single query instead of three separate queries
    // Use COALESCE to handle NULL values when table is empty (SUM returns NULL for empty set)
    let (total_requests, success_count, error_count): (u64, u64, u64) = conn
        .query_row(
            "SELECT
            COUNT(*) as total,
            COALESCE(SUM(CASE WHEN status >= 200 AND status < 400 THEN 1 ELSE 0 END), 0) as success,
            COALESCE(SUM(CASE WHEN status < 200 OR status >= 400 THEN 1 ELSE 0 END), 0) as error
         FROM request_logs",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|e| e.to_string())?;

    Ok(crate::proxy::monitor::ProxyStats {
        total_requests,
        success_count,
        error_count,
    })
}

/// Get single log detail (with request_body and response_body)
pub fn get_log_detail(log_id: &str) -> Result<ProxyRequestLog, String> {
    let conn = connect_db()?;

    let mut stmt = conn
        .prepare(
            "SELECT id, timestamp, method, url, status, duration, model, error,
                request_body, upstream_request_body, response_body, input_tokens, output_tokens,
                cached_tokens, account_email, mapped_model, protocol, client_ip, username,
                request_headers, upstream_request_headers, response_headers,
                session_id
         FROM request_logs
         WHERE id = ?1",
        )
        .map_err(|e| e.to_string())?;

    stmt.query_row([log_id], map_request_log_row)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod thinking_pack_tests {
    use super::*;

    #[test]
    fn pack_roundtrip_short_and_long() {
        let short = "hello thought";
        assert_eq!(unpack_thought(&pack_thought(short)), short);
        assert!(pack_thought(short).starts_with(THOUGHT_RAW_MAGIC));

        let long = "word ".repeat(2000);
        let packed = pack_thought(&long);
        assert!(
            packed.starts_with(THOUGHT_GZIP_MAGIC),
            "long thought should gzip"
        );
        assert!(packed.len() < long.len());
        assert_eq!(unpack_thought(&packed), long);
    }

    #[test]
    fn unpack_legacy_utf8() {
        assert_eq!(unpack_thought(b"plain old thought"), "plain old thought");
    }

    #[test]
    fn persist_visible_drops_tool_turns() {
        assert_eq!(
            persist_visible(&["call_1".to_string()], "I will run the tool"),
            ""
        );
        assert_eq!(persist_visible(&[], "hello"), "hello");
    }
}

#[cfg(test)]
mod tool_signature_tests {
    use super::*;
    use crate::proxy::monitor::prompt_log_tests::TestDataDir;

    #[test]
    fn tool_signature_misses_reuse_readonly_connection() {
        let _dir = TestDataDir::new();
        assert!(load_tool_signature("missing").is_err());
        assert!(!get_proxy_db_path().unwrap().exists());
        init_db().unwrap();
        let writer = connect_db().unwrap();
        assert_eq!(
            writer
                .pragma_query_value::<i64, _>(None, "auto_vacuum", |r| r.get(0))
                .unwrap(),
            2
        );
        let before: i64 = writer
            .pragma_query_value(None, "data_version", |r| r.get(0))
            .unwrap();
        assert_eq!(load_tool_signature("missing").unwrap(), None);
        {
            let db = TOOL_SIGNATURE_DB.get().unwrap().lock().unwrap();
            let conn = &db.as_ref().unwrap().1;
            assert!(conn.is_readonly(rusqlite::DatabaseName::Main).unwrap());
            // A connection-local setting detects accidental reopening on a miss.
            conn.pragma_update(None, "cache_size", -1234).unwrap();
        }
        for _ in 0..32 {
            assert_eq!(load_tool_signature("missing").unwrap(), None);
        }
        {
            let db = TOOL_SIGNATURE_DB.get().unwrap().lock().unwrap();
            let conn = &db.as_ref().unwrap().1;
            assert_eq!(
                conn.pragma_query_value::<i64, _>(None, "cache_size", |r| r.get(0))
                    .unwrap(),
                -1234
            );
            assert!(conn.is_autocommit());
            assert_eq!(conn.total_changes(), 0);
        }
        let after: i64 = writer
            .pragma_query_value(None, "data_version", |r| r.get(0))
            .unwrap();
        assert_eq!(after, before);
        TOOL_SIGNATURE_DB.get().unwrap().lock().unwrap().take();
    }

    #[test]
    fn tool_signature_reads_follow_writes_and_data_dir_changes() {
        let _dir = TestDataDir::new();
        init_db().unwrap();
        let signature = "s".repeat(60);
        assert_eq!(load_tool_signature("tool").unwrap(), None);
        save_tool_signature("tool", &signature).unwrap();
        assert_eq!(
            load_tool_signature("tool").unwrap(),
            Some(signature.clone())
        );
        let replacement = "r".repeat(60);
        save_tool_signature("tool", &replacement).unwrap();
        assert_eq!(
            load_tool_signature("tool").unwrap(),
            Some(replacement.clone())
        );
        {
            let _other_dir = TestDataDir::new();
            assert!(load_tool_signature("tool").is_err());
            init_db().unwrap();
            assert_eq!(load_tool_signature("tool").unwrap(), None);
            save_tool_signature("tool", &signature).unwrap();
            assert_eq!(load_tool_signature("tool").unwrap(), Some(signature));
            TOOL_SIGNATURE_DB.get().unwrap().lock().unwrap().take();
        }
        assert_eq!(load_tool_signature("tool").unwrap(), Some(replacement));
        TOOL_SIGNATURE_DB.get().unwrap().lock().unwrap().take();
    }
}

#[cfg(test)]
mod thinking_sqlite_tests {
    use super::*;
    use crate::proxy::monitor::prompt_log_tests::TestDataDir;

    #[test]
    fn test_thinking_record_deduplication_and_penetration_lookup() {
        let _dir = TestDataDir::new();

        let session_key = "test_tenant:sess-123456";
        let tool_id = "call_abc999";
        let real_sig = "s".repeat(60);

        // 1. 首次写入：实质思考 + tool_id
        save_thinking_record(
            session_key,
            "fp_turn1",
            "This is deep analytical thinking about rust code",
            Some(&real_sig),
            &[tool_id.to_string()],
            &[],
            "visible",
        )
        .unwrap();

        // 2. 二次写入相同 tool_id（例如客户端再次回传包含占位符的相同轮次）：绝不叠加新行，绝不将实质思考覆盖为占位符！
        save_thinking_record(
            session_key,
            "fp_turn1",
            "...",
            Some(&real_sig),
            &[tool_id.to_string()],
            &[],
            "visible",
        )
        .unwrap();

        // 3. 验证 SQLite 中仅存 1 行，且保留高质量思考
        let all = load_thinking_records(session_key).unwrap();
        assert_eq!(all.len(), 1, "Duplicate tool saves must be deduplicated!");
        assert_eq!(
            all[0].thought,
            "This is deep analytical thinking about rust code"
        );
        assert_eq!(all[0].signature, Some(real_sig.clone()));

        // 4. 精准穿透点查 tool_id
        let loaded = load_thinking_by_tool_id(session_key, tool_id).unwrap();
        assert!(loaded.is_some());
        let rec = loaded.unwrap();
        assert_eq!(
            rec.thought,
            "This is deep analytical thinking about rust code"
        );
        assert_eq!(rec.signature, Some(real_sig));

        // 5. 不存在的 tool_id 应当正确返回 None
        let missing = load_thinking_by_tool_id(session_key, "call_nonexistent").unwrap();
        assert!(missing.is_none());

        // 6. 纯文本指纹点查测试
        let text_fp = "fp_pure_text_1";
        save_thinking_record(
            session_key,
            text_fp,
            "Pure text reasoning",
            None,
            &[],
            &[],
            "pure text visible",
        )
        .unwrap();
        let loaded_text = load_thinking_by_fingerprint(session_key, text_fp).unwrap();
        assert!(loaded_text.is_some());
        assert_eq!(loaded_text.unwrap().thought, "Pure text reasoning");
    }
}

#[cfg(test)]
mod retention_tests {
    use super::*;
    use crate::proxy::config::LogRetentionConfig;
    use rusqlite::Connection;

    #[test]
    fn prompt_log_disk_budget_cleanup_and_live_config_reload() {
        use crate::proxy::monitor::prompt_log_tests::{sample_log, TestDataDir};
        let _dir = TestDataDir::new();
        init_db().unwrap();
        let mut config = crate::modules::config::load_app_config().unwrap();
        assert_eq!(
            serde_json::from_str::<LogRetentionConfig>("{}")
                .unwrap()
                .max_disk_mb,
            1024
        );
        config.proxy.log_retention.max_disk_mb = 8;
        config.proxy.log_retention.max_storage_gb = 0.0;
        crate::modules::config::save_app_config(&config).unwrap();
        save_log(sample_log("old", 300_000)).unwrap();
        let conn = connect_db().unwrap();
        reclaim_space(&conn).unwrap();
        assert!(disk_bytes(&conn).unwrap() > 1024 * 1024);
        config.proxy.log_retention.max_disk_mb = 1;
        config.proxy.log_retention.max_storage_gb = 0.0;
        crate::modules::config::save_app_config(&config).unwrap();
        save_log(sample_log("new", 4096)).unwrap();
        assert!(get_log_detail("old").is_err());
        assert_eq!(
            get_log_detail("new").unwrap().response_body,
            Some("错".repeat(4096))
        );
        assert!(disk_bytes(&conn).unwrap() <= 1024 * 1024);
        save_log(sample_log("oversize", 400_000)).unwrap();
        assert!(get_log_detail("oversize").unwrap().response_body.is_none());
        let zero_budget_policy = LogRetentionConfig {
            max_disk_mb: 0,
            max_storage_gb: 0.0,
            ..config.proxy.log_retention
        };
        assert!(
            save_log_with_connection(&conn, sample_log("no-room", 100), &zero_budget_policy)
                .is_err()
        );
        assert!(get_log_detail("no-room").is_err());
    }

    #[test]
    fn prompt_log_legacy_headroom_rejection_preserves_history_on_retries() {
        use crate::proxy::monitor::prompt_log_tests::TestDataDir;
        let _dir = TestDataDir::new();
        let conn = Connection::open(get_proxy_db_path().unwrap()).unwrap();
        conn.execute_batch("CREATE TABLE request_logs (id TEXT PRIMARY KEY, timestamp INTEGER, method TEXT, url TEXT, status INTEGER, duration INTEGER, model TEXT, error TEXT, response_body TEXT)").unwrap();
        assert_eq!(
            conn.pragma_query_value::<i64, _>(None, "auto_vacuum", |r| r.get(0))
                .unwrap(),
            0
        );
        conn.execute_batch(
            "WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i + 1 FROM n WHERE i < 128)
             INSERT INTO request_logs (id, timestamp, response_body)
             SELECT CAST(i AS TEXT), i, zeroblob(8192) FROM n;",
        )
        .unwrap();
        reclaim_space(&conn).unwrap();
        let before = disk_bytes(&conn).unwrap();
        let budget = 2 * 1024 * 1024;
        let log_bytes = 500_000;
        assert!(before < budget);
        assert!(before + 2 * log_bytes + 64 * 1024 > budget);
        assert!(3 * log_bytes + 64 * 1024 <= budget / 5 * 4);

        for _ in 0..6 {
            assert!(make_room(&conn, budget, log_bytes).is_err());
            let counts: (i64, i64) = conn
                .query_row(
                    "SELECT COUNT(*), COUNT(response_body) FROM request_logs",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            assert_eq!(counts, (128, 128));
            assert_eq!(disk_bytes(&conn).unwrap(), before);
        }
    }

    #[test]
    fn prompt_log_reclaims_free_pages_before_deleting_summaries() {
        use crate::proxy::monitor::prompt_log_tests::{sample_log, TestDataDir};
        let _dir = TestDataDir::new();
        init_db().unwrap();
        let conn = connect_db().unwrap();
        conn.execute_batch(
            "INSERT INTO request_logs (id, timestamp, response_body) VALUES
             ('old-1', 1, zeroblob(2097152)), ('old-2', 2, zeroblob(2097152)),
             ('old-3', 3, zeroblob(2097152));",
        )
        .unwrap();
        reclaim_space(&conn).unwrap();
        assert!(disk_bytes(&conn).unwrap() > 6 * 1024 * 1024);
        let policy = LogRetentionConfig {
            max_disk_mb: 1,
            max_storage_gb: 0.0,
            ..LogRetentionConfig::default()
        };

        save_log_with_connection(&conn, sample_log("new", 100), &policy).unwrap();

        let counts: (i64, i64) = conn
            .query_row(
                "SELECT COUNT(*), COUNT(response_body) FROM request_logs WHERE id != 'new'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(counts, (0, 0));
        assert_eq!(
            get_log_detail("new").unwrap().response_body,
            Some("错".repeat(100))
        );
        assert!(disk_bytes(&conn).unwrap() <= 1024 * 1024);
    }

    #[test]
    fn clears_old_bodies_and_limits_rows() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE request_logs (id TEXT PRIMARY KEY, timestamp INTEGER, request_body TEXT, upstream_request_body TEXT, response_body TEXT)").unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        conn.execute(
            "INSERT INTO request_logs VALUES ('retained-with-old-body', ?1, 'request', NULL, 'response')",
            [now - 25 * 3600 * 1000],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO request_logs VALUES ('new-1', ?1, NULL, NULL, NULL)",
            [now - 30 * 3600 * 1000],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO request_logs VALUES ('deleted-1', ?1, NULL, NULL, NULL)",
            [now - 35 * 3600 * 1000],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO request_logs VALUES ('deleted-2', ?1, NULL, NULL, NULL)",
            [now - 40 * 3600 * 1000],
        )
        .unwrap();
        let policy = LogRetentionConfig {
            max_body_age_hours: 24,
            max_age_days: 30,
            max_rows: 2,
            ..LogRetentionConfig::default()
        };
        let (cleared, deleted) = apply_retention_with_connection(&conn, &policy).unwrap();
        assert_eq!(cleared, 0);
        assert_eq!(deleted, 2);
        let body: Option<String> = conn
            .query_row(
                "SELECT request_body FROM request_logs WHERE id = 'retained-with-old-body'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(body, Some("request".to_string()));
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM request_logs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }
}

/// Cleanup old logs (keep last N days)
pub fn cleanup_old_logs(days: i64) -> Result<usize, String> {
    let conn = connect_db()?;

    // Note: Request log timestamp is stored in milliseconds (chrono::Utc::now().timestamp_millis())
    let cutoff_timestamp_ms = chrono::Utc::now().timestamp_millis() - (days * 24 * 3600 * 1000);

    let deleted = conn
        .execute(
            "DELETE FROM request_logs WHERE timestamp < ?1",
            [cutoff_timestamp_ms],
        )
        .map_err(|e| e.to_string())?;

    // Only execute VACUUM when substantial rows were deleted to avoid saturating disk I/O on startup
    if deleted >= 500 {
        if let Err(e) = conn.execute("VACUUM", []) {
            tracing::warn!("VACUUM failed after log cleanup: {}", e);
        }
    }

    Ok(deleted)
}

/// Limit maximum log count (keep newest N records)
#[allow(dead_code)]
pub fn limit_max_logs(max_count: usize) -> Result<usize, String> {
    let conn = connect_db()?;

    let deleted = conn
        .execute(
            "DELETE FROM request_logs WHERE id NOT IN (
            SELECT id FROM request_logs ORDER BY timestamp DESC LIMIT ?1
        )",
            [max_count],
        )
        .map_err(|e| e.to_string())?;

    // Only execute VACUUM when substantial rows were deleted
    if deleted >= 500 {
        if let Err(e) = conn.execute("VACUUM", []) {
            tracing::warn!("VACUUM failed after limit_max_logs: {}", e);
        }
    }

    Ok(deleted)
}

pub fn clear_logs() -> Result<(), String> {
    let _guard = LOG_WRITE_LOCK.lock().map_err(|e| e.to_string())?;
    let conn = connect_db()?;
    conn.execute("DELETE FROM request_logs", [])
        .map_err(|e| e.to_string())?;
    // Full vacuum to reclaim all disk space immediately
    let _ = conn.execute("VACUUM", []);
    let _ = conn.pragma_update(None, "wal_checkpoint", "TRUNCATE");
    Ok(())
}

/// Get total count of logs in database
pub fn get_logs_count() -> Result<u64, String> {
    let conn = connect_db()?;

    let count: u64 = conn
        .query_row("SELECT COUNT(*) FROM request_logs", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;

    Ok(count)
}

/// Get count of logs matching search filter
/// filter: search text to match in url, method, model, or status
/// errors_only: if true, only count logs with status < 200 or >= 400
pub fn get_logs_count_filtered(filter: &str, errors_only: bool) -> Result<u64, String> {
    let conn = connect_db()?;

    let filter_pattern = format!("%{}%", filter);

    let sql = if errors_only {
        "SELECT COUNT(*) FROM request_logs WHERE (status < 200 OR status >= 400)"
    } else if filter.is_empty() {
        "SELECT COUNT(*) FROM request_logs"
    } else {
        "SELECT COUNT(*) FROM request_logs WHERE
            (url LIKE ?1 OR method LIKE ?1 OR model LIKE ?1 OR CAST(status AS TEXT) LIKE ?1 OR account_email LIKE ?1)"
    };

    let count: u64 = if filter.is_empty() && !errors_only {
        conn.query_row(sql, [], |row| row.get(0))
    } else if errors_only {
        conn.query_row(sql, [], |row| row.get(0))
    } else {
        conn.query_row(sql, [&filter_pattern], |row| row.get(0))
    }
    .map_err(|e| e.to_string())?;

    Ok(count)
}

/// Get logs with search filter and pagination
/// filter: search text to match in url, method, model, or status
/// errors_only: if true, only return logs with status < 200 or >= 400
pub fn get_logs_filtered(
    filter: &str,
    errors_only: bool,
    limit: usize,
    offset: usize,
) -> Result<Vec<ProxyRequestLog>, String> {
    let conn = connect_db()?;

    let filter_pattern = format!("%{}%", filter);

    let sql = if errors_only {
        "SELECT id, timestamp, method, url, status, duration, model, substr(error, 1, 1024),
                NULL as request_body, NULL as upstream_request_body, NULL as response_body,
                input_tokens, output_tokens, cached_tokens, account_email, mapped_model, protocol, client_ip, username,
                NULL as request_headers, NULL as upstream_request_headers, NULL as response_headers,
                session_id
         FROM request_logs
         WHERE (status < 200 OR status >= 400)
         ORDER BY timestamp DESC
         LIMIT ?1 OFFSET ?2"
    } else if filter.is_empty() {
        "SELECT id, timestamp, method, url, status, duration, model, substr(error, 1, 1024),
                NULL as request_body, NULL as upstream_request_body, NULL as response_body,
                input_tokens, output_tokens, cached_tokens, account_email, mapped_model, protocol, client_ip, username,
                NULL as request_headers, NULL as upstream_request_headers, NULL as response_headers,
                session_id
         FROM request_logs
         ORDER BY timestamp DESC
         LIMIT ?1 OFFSET ?2"
    } else {
        "SELECT id, timestamp, method, url, status, duration, model, substr(error, 1, 1024),
                NULL as request_body, NULL as upstream_request_body, NULL as response_body,
                input_tokens, output_tokens, cached_tokens, account_email, mapped_model, protocol, client_ip, username,
                NULL as request_headers, NULL as upstream_request_headers, NULL as response_headers,
                session_id
         FROM request_logs
         WHERE (url LIKE ?3 OR method LIKE ?3 OR model LIKE ?3 OR CAST(status AS TEXT) LIKE ?3 OR account_email LIKE ?3 OR client_ip LIKE ?3 OR session_id LIKE ?3)
         ORDER BY timestamp DESC
         LIMIT ?1 OFFSET ?2"
    };

    let logs: Vec<ProxyRequestLog> = if filter.is_empty() && !errors_only {
        let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
        let logs_iter = stmt
            .query_map([limit, offset], map_request_log_row)
            .map_err(|e| e.to_string())?;
        logs_iter.filter_map(|r| r.ok()).collect()
    } else if errors_only {
        let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
        let logs_iter = stmt
            .query_map([limit, offset], map_request_log_row)
            .map_err(|e| e.to_string())?;
        logs_iter.filter_map(|r| r.ok()).collect()
    } else {
        let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
        let logs_iter = stmt
            .query_map(
                rusqlite::params![limit, offset, filter_pattern],
                map_request_log_row,
            )
            .map_err(|e| e.to_string())?;
        logs_iter.filter_map(|r| r.ok()).collect()
    };

    Ok(logs)
}

/// Get all logs with full details for export
pub fn get_all_logs_for_export() -> Result<Vec<ProxyRequestLog>, String> {
    let conn = connect_db()?;

    let mut stmt = conn
        .prepare(
            "SELECT id, timestamp, method, url, status, duration, model, error,
                request_body, upstream_request_body, response_body, input_tokens, output_tokens,
                cached_tokens, account_email, mapped_model, protocol, client_ip, username,
                request_headers, upstream_request_headers, response_headers,
                session_id
         FROM request_logs
         ORDER BY timestamp DESC",
        )
        .map_err(|e| e.to_string())?;

    let logs_iter = stmt
        .query_map([], map_request_log_row)
        .map_err(|e| e.to_string())?;

    let mut logs = Vec::new();
    for log in logs_iter {
        logs.push(log.map_err(|e| e.to_string())?);
    }
    Ok(logs)
}

// ... existing code ...

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IpTokenStats {
    pub client_ip: String,
    pub total_tokens: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub request_count: i64,
    pub username: Option<String>,
}

/// Get token usage grouped by IP
pub fn get_token_usage_by_ip(limit: usize, hours: i64) -> Result<Vec<IpTokenStats>, String> {
    let conn = connect_db()?;

    // Fix: Database stores timestamp in milliseconds, but we were calculating 'since' in seconds
    // Convert 'hours' to milliseconds
    let since = chrono::Utc::now().timestamp_millis() - (hours * 3600 * 1000);

    // [FIX] 不再从 request_logs 表获取 username，因为该字段可能为空
    // 先获取 IP 统计数据，然后再单独查询每个 IP 的用户名
    let mut stmt = conn
        .prepare(
            "SELECT
            client_ip,
            COALESCE(SUM(input_tokens), 0) + COALESCE(SUM(output_tokens), 0) as total,
            COALESCE(SUM(input_tokens), 0) as input,
            COALESCE(SUM(output_tokens), 0) as output,
            COUNT(*) as cnt
         FROM request_logs
         WHERE timestamp >= ?1 AND client_ip IS NOT NULL AND client_ip != ''
         GROUP BY client_ip
         ORDER BY total DESC
         LIMIT ?2",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![since, limit], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .map_err(|e| e.to_string())?;

    let mut stats = Vec::new();
    for row in rows {
        let (client_ip, total_tokens, input_tokens, output_tokens, request_count) =
            row.map_err(|e| e.to_string())?;

        // 从 user_token_db 获取该 IP 关联的用户名
        // 这比从 request_logs 获取更可靠，因为 token_ip_bindings 表在每次 User Token 使用时都会更新
        let username =
            crate::modules::user_token_db::get_username_for_ip(&client_ip).unwrap_or(None);

        stats.push(IpTokenStats {
            client_ip,
            total_tokens,
            input_tokens,
            output_tokens,
            request_count,
            username,
        });
    }

    Ok(stats)
}
