use crate::proxy::config::LogRetentionConfig;
use crate::proxy::monitor::ProxyRequestLog;
use rusqlite::{params, Connection};
use std::path::PathBuf;
use std::sync::Mutex;

static LOG_WRITE_LOCK: Mutex<()> = Mutex::new(());

pub fn get_proxy_db_path() -> Result<PathBuf, String> {
    let data_dir = crate::modules::account::get_data_dir()?;
    Ok(data_dir.join("proxy_logs.db"))
}

fn connect_db() -> Result<Connection, String> {
    let db_path = get_proxy_db_path()?;
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;

    // Must precede WAL for new databases. Existing databases are not fully VACUUMed.
    conn.pragma_update(None, "auto_vacuum", "INCREMENTAL")
        .map_err(|e| e.to_string())?;

    // Enable WAL mode for better concurrency
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| e.to_string())?;

    // Set busy timeout to 5000ms to avoid "database is locked" errors
    conn.pragma_update(None, "busy_timeout", 5000)
        .map_err(|e| e.to_string())?;

    // Synchronous NORMAL is faster and safe enough for WAL
    conn.pragma_update(None, "synchronous", "NORMAL")
        .map_err(|e| e.to_string())?;

    Ok(conn)
}

pub fn init_db() -> Result<(), String> {
    // connect_db will initialize WAL mode and other pragmas
    let conn = connect_db()?;

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

    Ok(())
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
    let now = chrono::Utc::now().timestamp_millis();
    let body_cutoff = now - (policy.max_body_age_hours as i64 * 3600 * 1000);
    let age_cutoff = now - (policy.max_age_days as i64 * 24 * 3600 * 1000);
    let bodies_cleared = conn.execute(
        "UPDATE request_logs SET request_body = NULL, response_body = NULL WHERE timestamp < ?1 AND (request_body IS NOT NULL OR response_body IS NOT NULL)",
        [body_cutoff],
    ).map_err(|e| e.to_string())?;
    let mut rows_deleted = conn
        .execute(
            "DELETE FROM request_logs WHERE timestamp < ?1",
            [age_cutoff],
        )
        .map_err(|e| e.to_string())?;
    if policy.max_rows > 0 {
        rows_deleted += conn.execute(
            "DELETE FROM request_logs WHERE id NOT IN (SELECT id FROM request_logs ORDER BY timestamp DESC LIMIT ?1)",
            [policy.max_rows],
        ).map_err(|e| e.to_string())?;
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
            return Err("proxy log checkpoint busy".to_string());
        }
        Ok(())
    };
    checkpoint()?;
    // This pragma yields a row per reclaimed page; drain it to perform all 256 steps.
    let mut vacuum = conn
        .prepare("PRAGMA incremental_vacuum(256)")
        .map_err(|e| e.to_string())?;
    let mut pages = vacuum.query([]).map_err(|e| e.to_string())?;
    while pages.next().map_err(|e| e.to_string())?.is_some() {}
    drop(pages);
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
    if projected_bytes(conn, log_bytes)? <= budget {
        return Ok(());
    }
    reclaim_space(conn)?;
    if projected_bytes(conn, log_bytes)? <= budget {
        return Ok(());
    }
    let auto_vacuum: i64 = conn
        .pragma_query_value(None, "auto_vacuum", |r| r.get(0))
        .map_err(|e| e.to_string())?;
    // Legacy files cannot shrink: even reusing all free pages still needs WAL headroom.
    if auto_vacuum == 0
        && disk_bytes(conn)?
            .saturating_add(log_bytes.saturating_mul(2))
            .saturating_add(64 * 1024)
            > budget
    {
        return Err("legacy proxy log database cannot shrink within budget".to_string());
    }
    let target = budget / 5 * 4;
    // Bounded work per write, oldest bodies first, then oldest summaries. No full-body reads.
    for _ in 0..8 {
        let before = projected_bytes(conn, log_bytes)?;
        let free: u64 = conn
            .pragma_query_value(None, "freelist_count", |r| r.get(0))
            .map_err(|e| e.to_string())?;
        // Reclaim already freed pages before discarding more history.
        if auto_vacuum == 0 || free == 0 {
            let cleared = conn.execute(
            "UPDATE request_logs SET request_body = NULL, response_body = NULL WHERE id IN
             (SELECT id FROM request_logs WHERE request_body IS NOT NULL OR response_body IS NOT NULL ORDER BY timestamp ASC LIMIT 32)", []
        ).map_err(|e| e.to_string())?;
            if cleared == 0 {
                conn.execute("DELETE FROM request_logs WHERE id IN (SELECT id FROM request_logs ORDER BY timestamp ASC LIMIT 32)", [])
                    .map_err(|e| e.to_string())?;
            }
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
    let budget = policy.max_disk_mb.saturating_mul(1024 * 1024);
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
    let body_bytes = [&log.request_body, &log.response_body]
        .iter()
        .filter_map(|s| s.as_ref())
        .map(|s| s.len() as u64)
        .sum::<u64>();
    let mut log_bytes = summary_bytes.saturating_add(body_bytes);
    if log_bytes.saturating_mul(3).saturating_add(64 * 1024) > budget / 5 * 4 {
        log.request_body = None;
        log.response_body = None;
        log_bytes = summary_bytes;
    }
    if log_bytes.saturating_mul(3).saturating_add(64 * 1024) > budget {
        return Err("proxy log summary exceeds disk budget".to_string());
    }
    make_room(conn, budget, log_bytes)?;

    conn.execute(
        "INSERT INTO request_logs (id, timestamp, method, url, status, duration, model, error, request_body, response_body, input_tokens, output_tokens, cached_tokens, account_email, mapped_model, protocol, client_ip, username)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
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
            log.response_body,
            log.input_tokens,
            log.output_tokens,
            log.cached_tokens,
            log.account_email,
            log.mapped_model,
            log.protocol,
            log.client_ip,
            log.username,
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
                NULL as request_body, NULL as response_body,
                input_tokens, output_tokens, cached_tokens, account_email, mapped_model, protocol, client_ip, username
         FROM request_logs 
         ORDER BY timestamp DESC 
         LIMIT ?1 OFFSET ?2",
        )
        .map_err(|e| e.to_string())?;

    let logs_iter = stmt
        .query_map([limit, offset], |row| {
            Ok(ProxyRequestLog {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                method: row.get(2)?,
                url: row.get(3)?,
                status: row.get(4)?,
                duration: row.get(5)?,
                model: row.get(6)?,
                error: row.get(7)?,
                request_body: None,  // Don't query large fields for list view
                response_body: None, // Don't query large fields for list view
                input_tokens: row.get(10).unwrap_or(None),
                output_tokens: row.get(11).unwrap_or(None),
                cached_tokens: row.get(12).unwrap_or(None),
                account_email: row.get(13).unwrap_or(None),
                mapped_model: row.get(14).unwrap_or(None),
                protocol: row.get(15).unwrap_or(None),
                client_ip: row.get(16).unwrap_or(None),
                username: row.get(17).unwrap_or(None),
            })
        })
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
                request_body, response_body, input_tokens, output_tokens,
                cached_tokens, account_email, mapped_model, protocol, client_ip, username
         FROM request_logs
         WHERE id = ?1",
        )
        .map_err(|e| e.to_string())?;

    stmt.query_row([log_id], |row| {
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
            response_body: row.get(9).unwrap_or(None),
            input_tokens: row.get(10).unwrap_or(None),
            output_tokens: row.get(11).unwrap_or(None),
            cached_tokens: row.get(12).unwrap_or(None),
            account_email: row.get(13).unwrap_or(None),
            mapped_model: row.get(14).unwrap_or(None),
            protocol: row.get(15).unwrap_or(None),
            client_ip: row.get(16).unwrap_or(None),
            username: row.get(17).unwrap_or(None),
        })
    })
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod retention_tests {
    use super::apply_retention_with_connection;
    use crate::proxy::config::LogRetentionConfig;
    use rusqlite::Connection;

    #[test]
    fn prompt_log_disk_budget_cleanup_and_live_config_reload() {
        use super::*;
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
        crate::modules::config::save_app_config(&config).unwrap();
        save_log(sample_log("old", 300_000)).unwrap();
        let conn = connect_db().unwrap();
        reclaim_space(&conn).unwrap();
        assert!(disk_bytes(&conn).unwrap() > 1024 * 1024);
        config.proxy.log_retention.max_disk_mb = 1;
        crate::modules::config::save_app_config(&config).unwrap();
        save_log(sample_log("new", 4096)).unwrap();
        assert!(get_log_detail("old").unwrap().response_body.is_none());
        assert_eq!(
            get_log_detail("new").unwrap().response_body,
            Some("错".repeat(4096))
        );
        assert!(disk_bytes(&conn).unwrap() <= 1024 * 1024);
        save_log(sample_log("oversize", 400_000)).unwrap();
        assert!(get_log_detail("oversize").unwrap().response_body.is_none());
        config.proxy.log_retention.max_disk_mb = 0;
        crate::modules::config::save_app_config(&config).unwrap();
        assert!(save_log(sample_log("no-room", 100)).is_err());
        assert!(get_log_detail("no-room").is_err());
    }

    #[test]
    fn prompt_log_legacy_headroom_rejection_preserves_history_on_retries() {
        use super::*;
        use crate::proxy::monitor::prompt_log_tests::TestDataDir;
        let _dir = TestDataDir::new();
        let conn = Connection::open(get_proxy_db_path().unwrap()).unwrap();
        conn.execute_batch("CREATE TABLE request_logs (id TEXT PRIMARY KEY, timestamp INTEGER, method TEXT, url TEXT, status INTEGER, duration INTEGER, model TEXT, error TEXT)").unwrap();
        init_db().unwrap();
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
        use super::*;
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
        assert_eq!(counts, (3, 0));
        assert_eq!(
            get_log_detail("new").unwrap().response_body,
            Some("错".repeat(100))
        );
        assert!(disk_bytes(&conn).unwrap() <= 1024 * 1024);
    }

    #[test]
    fn clears_old_bodies_and_limits_rows() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE request_logs (id TEXT PRIMARY KEY, timestamp INTEGER, request_body TEXT, response_body TEXT)").unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        conn.execute(
            "INSERT INTO request_logs VALUES ('retained-with-old-body', ?1, 'request', 'response')",
            [now - 25 * 3600 * 1000],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO request_logs VALUES ('new-1', ?1, NULL, NULL)",
            [now - 30 * 3600 * 1000],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO request_logs VALUES ('deleted-1', ?1, NULL, NULL)",
            [now - 35 * 3600 * 1000],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO request_logs VALUES ('deleted-2', ?1, NULL, NULL)",
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
        assert_eq!(cleared, 1);
        assert_eq!(deleted, 2);
        let body: Option<String> = conn
            .query_row(
                "SELECT request_body FROM request_logs WHERE id = 'retained-with-old-body'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(body, None);
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
    let conn = connect_db()?;
    conn.execute("DELETE FROM request_logs", [])
        .map_err(|e| e.to_string())?;
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
                NULL as request_body, NULL as response_body,
                input_tokens, output_tokens, cached_tokens, account_email, mapped_model, protocol, client_ip, username
         FROM request_logs
         WHERE (status < 200 OR status >= 400)
         ORDER BY timestamp DESC
         LIMIT ?1 OFFSET ?2"
    } else if filter.is_empty() {
        "SELECT id, timestamp, method, url, status, duration, model, substr(error, 1, 1024),
                NULL as request_body, NULL as response_body,
                input_tokens, output_tokens, cached_tokens, account_email, mapped_model, protocol, client_ip, username
         FROM request_logs
         ORDER BY timestamp DESC
         LIMIT ?1 OFFSET ?2"
    } else {
        "SELECT id, timestamp, method, url, status, duration, model, substr(error, 1, 1024),
                NULL as request_body, NULL as response_body,
                input_tokens, output_tokens, cached_tokens, account_email, mapped_model, protocol, client_ip, username
         FROM request_logs
         WHERE (url LIKE ?3 OR method LIKE ?3 OR model LIKE ?3 OR CAST(status AS TEXT) LIKE ?3 OR account_email LIKE ?3 OR client_ip LIKE ?3)
         ORDER BY timestamp DESC
         LIMIT ?1 OFFSET ?2"
    };

    let logs: Vec<ProxyRequestLog> = if filter.is_empty() && !errors_only {
        let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
        let logs_iter = stmt
            .query_map([limit, offset], |row| {
                Ok(ProxyRequestLog {
                    id: row.get(0)?,
                    timestamp: row.get(1)?,
                    method: row.get(2)?,
                    url: row.get(3)?,
                    status: row.get(4)?,
                    duration: row.get(5)?,
                    model: row.get(6)?,
                    error: row.get(7)?,
                    request_body: None,
                    response_body: None,
                    input_tokens: row.get(10).unwrap_or(None),
                    output_tokens: row.get(11).unwrap_or(None),
                    cached_tokens: row.get(12).unwrap_or(None),
                    account_email: row.get(13).unwrap_or(None),
                    mapped_model: row.get(14).unwrap_or(None),
                    protocol: row.get(15).unwrap_or(None),
                    client_ip: row.get(16).unwrap_or(None),
                    username: row.get(17).unwrap_or(None),
                })
            })
            .map_err(|e| e.to_string())?;
        logs_iter.filter_map(|r| r.ok()).collect()
    } else if errors_only {
        let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
        let logs_iter = stmt
            .query_map([limit, offset], |row| {
                Ok(ProxyRequestLog {
                    id: row.get(0)?,
                    timestamp: row.get(1)?,
                    method: row.get(2)?,
                    url: row.get(3)?,
                    status: row.get(4)?,
                    duration: row.get(5)?,
                    model: row.get(6)?,
                    error: row.get(7)?,
                    request_body: None,
                    response_body: None,
                    input_tokens: row.get(10).unwrap_or(None),
                    output_tokens: row.get(11).unwrap_or(None),
                    cached_tokens: row.get(12).unwrap_or(None),
                    account_email: row.get(13).unwrap_or(None),
                    mapped_model: row.get(14).unwrap_or(None),
                    protocol: row.get(15).unwrap_or(None),
                    client_ip: row.get(16).unwrap_or(None),
                    username: row.get(17).unwrap_or(None),
                })
            })
            .map_err(|e| e.to_string())?;
        logs_iter.filter_map(|r| r.ok()).collect()
    } else {
        let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
        let logs_iter = stmt
            .query_map(rusqlite::params![limit, offset, filter_pattern], |row| {
                Ok(ProxyRequestLog {
                    id: row.get(0)?,
                    timestamp: row.get(1)?,
                    method: row.get(2)?,
                    url: row.get(3)?,
                    status: row.get(4)?,
                    duration: row.get(5)?,
                    model: row.get(6)?,
                    error: row.get(7)?,
                    request_body: None,
                    response_body: None,
                    input_tokens: row.get(10).unwrap_or(None),
                    output_tokens: row.get(11).unwrap_or(None),
                    cached_tokens: row.get(12).unwrap_or(None),
                    account_email: row.get(13).unwrap_or(None),
                    mapped_model: row.get(14).unwrap_or(None),
                    protocol: row.get(15).unwrap_or(None),
                    client_ip: row.get(16).unwrap_or(None),
                    username: row.get(17).unwrap_or(None),
                })
            })
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
                request_body, response_body, input_tokens, output_tokens,
                cached_tokens, account_email, mapped_model, protocol, client_ip, username
         FROM request_logs
         ORDER BY timestamp DESC",
        )
        .map_err(|e| e.to_string())?;

    let logs_iter = stmt
        .query_map([], |row| {
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
                response_body: row.get(9).unwrap_or(None),
                input_tokens: row.get(10).unwrap_or(None),
                output_tokens: row.get(11).unwrap_or(None),
                cached_tokens: row.get(12).unwrap_or(None),
                account_email: row.get(13).unwrap_or(None),
                mapped_model: row.get(14).unwrap_or(None),
                protocol: row.get(15).unwrap_or(None),
                client_ip: row.get(16).unwrap_or(None),
                username: row.get(17).unwrap_or(None),
            })
        })
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
