use rusqlite::{params, Connection};
use std::path::PathBuf;
use crate::proxy::monitor::ProxyRequestLog;

pub fn get_proxy_db_path() -> Result<PathBuf, String> {
    let data_dir = crate::modules::account::get_data_dir()?;
    Ok(data_dir.join("proxy_logs.db"))
}

pub fn init_db() -> Result<(), String> {
    let db_path = get_proxy_db_path()?;
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    
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
    ).map_err(|e| e.to_string())?;

    // Try to add new columns (ignore errors if they exist)
    let _ = conn.execute("ALTER TABLE request_logs ADD COLUMN request_body TEXT", []);
    let _ = conn.execute("ALTER TABLE request_logs ADD COLUMN response_body TEXT", []);
    let _ = conn.execute("ALTER TABLE request_logs ADD COLUMN input_tokens INTEGER", []);
    let _ = conn.execute("ALTER TABLE request_logs ADD COLUMN output_tokens INTEGER", []);
    let _ = conn.execute("ALTER TABLE request_logs ADD COLUMN account_email TEXT", []);
    let _ = conn.execute("ALTER TABLE request_logs ADD COLUMN mapped_model TEXT", []);

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_timestamp ON request_logs (timestamp DESC)",
        [],
    ).map_err(|e| e.to_string())?;

    // Add status index for faster stats queries
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_status ON request_logs (status)",
        [],
    ).map_err(|e| e.to_string())?;

    Ok(())
}

pub fn save_log(log: &ProxyRequestLog) -> Result<(), String> {
    let db_path = get_proxy_db_path()?;
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;

    conn.execute(
        "INSERT INTO request_logs (id, timestamp, method, url, status, duration, model, error, request_body, response_body, input_tokens, output_tokens, account_email, mapped_model)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
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
            log.account_email,
            log.mapped_model,
        ],
    ).map_err(|e| e.to_string())?;

    Ok(())
}

/// Get logs summary (without large request_body and response_body fields) with pagination
pub fn get_logs_summary(limit: usize, offset: usize) -> Result<Vec<ProxyRequestLog>, String> {
    let db_path = get_proxy_db_path()?;
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;

    let mut stmt = conn.prepare(
        "SELECT id, timestamp, method, url, status, duration, model, error, 
                NULL as request_body, NULL as response_body,
                input_tokens, output_tokens, account_email, mapped_model
         FROM request_logs 
         ORDER BY timestamp DESC 
         LIMIT ?1 OFFSET ?2"
    ).map_err(|e| e.to_string())?;

    let logs_iter = stmt.query_map([limit, offset], |row| {
        Ok(ProxyRequestLog {
            id: row.get(0)?,
            timestamp: row.get(1)?,
            method: row.get(2)?,
            url: row.get(3)?,
            status: row.get(4)?,
            duration: row.get(5)?,
            model: row.get(6)?,
            mapped_model: row.get(13).unwrap_or(None),
            account_email: row.get(12).unwrap_or(None),
            error: row.get(7)?,
            request_body: None,  // Don't query large fields for list view
            response_body: None, // Don't query large fields for list view
            input_tokens: row.get(10).unwrap_or(None),
            output_tokens: row.get(11).unwrap_or(None),
        })
    }).map_err(|e| e.to_string())?;

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
    let db_path = get_proxy_db_path()?;
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;

    // Optimized: Use single query instead of three separate queries
    let (total_requests, success_count, error_count): (u64, u64, u64) = conn.query_row(
        "SELECT 
            COUNT(*) as total,
            SUM(CASE WHEN status >= 200 AND status < 400 THEN 1 ELSE 0 END) as success,
            SUM(CASE WHEN status < 200 OR status >= 400 THEN 1 ELSE 0 END) as error
         FROM request_logs",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).map_err(|e| e.to_string())?;

    Ok(crate::proxy::monitor::ProxyStats {
        total_requests,
        success_count,
        error_count,
    })
}

/// Get single log detail (with request_body and response_body)
pub fn get_log_detail(log_id: &str) -> Result<ProxyRequestLog, String> {
    let db_path = get_proxy_db_path()?;
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;

    let mut stmt = conn.prepare(
        "SELECT id, timestamp, method, url, status, duration, model, error, 
                request_body, response_body, input_tokens, output_tokens, 
                account_email, mapped_model
         FROM request_logs 
         WHERE id = ?1"
    ).map_err(|e| e.to_string())?;

    stmt.query_row([log_id], |row| {
        Ok(ProxyRequestLog {
            id: row.get(0)?,
            timestamp: row.get(1)?,
            method: row.get(2)?,
            url: row.get(3)?,
            status: row.get(4)?,
            duration: row.get(5)?,
            model: row.get(6)?,
            mapped_model: row.get(13).unwrap_or(None),
            account_email: row.get(12).unwrap_or(None),
            error: row.get(7)?,
            request_body: row.get(8).unwrap_or(None),
            response_body: row.get(9).unwrap_or(None),
            input_tokens: row.get(10).unwrap_or(None),
            output_tokens: row.get(11).unwrap_or(None),
        })
    }).map_err(|e| e.to_string())
}

/// Cleanup old logs (keep last N days)
pub fn cleanup_old_logs(days: i64) -> Result<usize, String> {
    let db_path = get_proxy_db_path()?;
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    
    let cutoff_timestamp = chrono::Utc::now().timestamp() - (days * 24 * 3600);
    
    let deleted = conn.execute(
        "DELETE FROM request_logs WHERE timestamp < ?1",
        [cutoff_timestamp],
    ).map_err(|e| e.to_string())?;
    
    // Execute VACUUM to reclaim disk space
    conn.execute("VACUUM", []).map_err(|e| e.to_string())?;
    
    Ok(deleted)
}

/// Limit maximum log count (keep newest N records)
#[allow(dead_code)]
pub fn limit_max_logs(max_count: usize) -> Result<usize, String> {
    let db_path = get_proxy_db_path()?;
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    
    let deleted = conn.execute(
        "DELETE FROM request_logs WHERE id NOT IN (
            SELECT id FROM request_logs ORDER BY timestamp DESC LIMIT ?1
        )",
        [max_count],
    ).map_err(|e| e.to_string())?;
    
    conn.execute("VACUUM", []).map_err(|e| e.to_string())?;
    
    Ok(deleted)
}

pub fn clear_logs() -> Result<(), String> {
    let db_path = get_proxy_db_path()?;
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM request_logs", []).map_err(|e| e.to_string())?;
    Ok(())
}

/// Get total count of logs in database
pub fn get_logs_count() -> Result<u64, String> {
    let db_path = get_proxy_db_path()?;
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    
    let count: u64 = conn.query_row(
        "SELECT COUNT(*) FROM request_logs",
        [],
        |row| row.get(0),
    ).map_err(|e| e.to_string())?;
    
    Ok(count)
}

/// Get count of logs matching search filter
/// filter: search text to match in url, method, model, or status
/// errors_only: if true, only count logs with status < 200 or >= 400
pub fn get_logs_count_filtered(filter: &str, errors_only: bool) -> Result<u64, String> {
    let db_path = get_proxy_db_path()?;
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    
    let filter_pattern = format!("%{}%", filter);
    
    let sql = if errors_only {
        "SELECT COUNT(*) FROM request_logs WHERE (status < 200 OR status >= 400)"
    } else if filter.is_empty() {
        "SELECT COUNT(*) FROM request_logs"
    } else {
        "SELECT COUNT(*) FROM request_logs WHERE 
            (url LIKE ?1 OR method LIKE ?1 OR model LIKE ?1 OR CAST(status AS TEXT) LIKE ?1)"
    };
    
    let count: u64 = if filter.is_empty() && !errors_only {
        conn.query_row(sql, [], |row| row.get(0))
    } else if errors_only {
        conn.query_row(sql, [], |row| row.get(0))
    } else {
        conn.query_row(sql, [&filter_pattern], |row| row.get(0))
    }.map_err(|e| e.to_string())?;
    
    Ok(count)
}

/// Get logs with search filter and pagination
/// filter: search text to match in url, method, model, or status
/// errors_only: if true, only return logs with status < 200 or >= 400
pub fn get_logs_filtered(filter: &str, errors_only: bool, limit: usize, offset: usize) -> Result<Vec<ProxyRequestLog>, String> {
    let db_path = get_proxy_db_path()?;
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;

    let filter_pattern = format!("%{}%", filter);
    
    let sql = if errors_only {
        "SELECT id, timestamp, method, url, status, duration, model, error, 
                NULL as request_body, NULL as response_body,
                input_tokens, output_tokens, account_email, mapped_model
         FROM request_logs 
         WHERE (status < 200 OR status >= 400)
         ORDER BY timestamp DESC 
         LIMIT ?1 OFFSET ?2"
    } else if filter.is_empty() {
        "SELECT id, timestamp, method, url, status, duration, model, error, 
                NULL as request_body, NULL as response_body,
                input_tokens, output_tokens, account_email, mapped_model
         FROM request_logs 
         ORDER BY timestamp DESC 
         LIMIT ?1 OFFSET ?2"
    } else {
        "SELECT id, timestamp, method, url, status, duration, model, error, 
                NULL as request_body, NULL as response_body,
                input_tokens, output_tokens, account_email, mapped_model
         FROM request_logs 
         WHERE (url LIKE ?3 OR method LIKE ?3 OR model LIKE ?3 OR CAST(status AS TEXT) LIKE ?3)
         ORDER BY timestamp DESC 
         LIMIT ?1 OFFSET ?2"
    };

    let logs: Vec<ProxyRequestLog> = if filter.is_empty() && !errors_only {
        let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
        let logs_iter = stmt.query_map([limit, offset], |row| {
            Ok(ProxyRequestLog {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                method: row.get(2)?,
                url: row.get(3)?,
                status: row.get(4)?,
                duration: row.get(5)?,
                model: row.get(6)?,
                mapped_model: row.get(13).unwrap_or(None),
                account_email: row.get(12).unwrap_or(None),
                error: row.get(7)?,
                request_body: None,
                response_body: None,
                input_tokens: row.get(10).unwrap_or(None),
                output_tokens: row.get(11).unwrap_or(None),
            })
        }).map_err(|e| e.to_string())?;
        logs_iter.filter_map(|r| r.ok()).collect()
    } else if errors_only {
        let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
        let logs_iter = stmt.query_map([limit, offset], |row| {
            Ok(ProxyRequestLog {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                method: row.get(2)?,
                url: row.get(3)?,
                status: row.get(4)?,
                duration: row.get(5)?,
                model: row.get(6)?,
                mapped_model: row.get(13).unwrap_or(None),
                account_email: row.get(12).unwrap_or(None),
                error: row.get(7)?,
                request_body: None,
                response_body: None,
                input_tokens: row.get(10).unwrap_or(None),
                output_tokens: row.get(11).unwrap_or(None),
            })
        }).map_err(|e| e.to_string())?;
        logs_iter.filter_map(|r| r.ok()).collect()
    } else {
        let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
        let logs_iter = stmt.query_map(rusqlite::params![limit, offset, filter_pattern], |row| {
            Ok(ProxyRequestLog {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                method: row.get(2)?,
                url: row.get(3)?,
                status: row.get(4)?,
                duration: row.get(5)?,
                model: row.get(6)?,
                mapped_model: row.get(13).unwrap_or(None),
                account_email: row.get(12).unwrap_or(None),
                error: row.get(7)?,
                request_body: None,
                response_body: None,
                input_tokens: row.get(10).unwrap_or(None),
                output_tokens: row.get(11).unwrap_or(None),
            })
        }).map_err(|e| e.to_string())?;
        logs_iter.filter_map(|r| r.ok()).collect()
    };

    Ok(logs)
}

/// Get all logs with full details for export
pub fn get_all_logs_for_export() -> Result<Vec<ProxyRequestLog>, String> {
    let db_path = get_proxy_db_path()?;
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;

    let mut stmt = conn.prepare(
        "SELECT id, timestamp, method, url, status, duration, model, error, 
                request_body, response_body, input_tokens, output_tokens, 
                account_email, mapped_model
         FROM request_logs 
         ORDER BY timestamp DESC"
    ).map_err(|e| e.to_string())?;

    let logs_iter = stmt.query_map([], |row| {
        Ok(ProxyRequestLog {
            id: row.get(0)?,
            timestamp: row.get(1)?,
            method: row.get(2)?,
            url: row.get(3)?,
            status: row.get(4)?,
            duration: row.get(5)?,
            model: row.get(6)?,
            mapped_model: row.get(13).unwrap_or(None),
            account_email: row.get(12).unwrap_or(None),
            error: row.get(7)?,
            request_body: row.get(8).unwrap_or(None),
            response_body: row.get(9).unwrap_or(None),
            input_tokens: row.get(10).unwrap_or(None),
            output_tokens: row.get(11).unwrap_or(None),
        })
    }).map_err(|e| e.to_string())?;

    let mut logs = Vec::new();
    for log in logs_iter {
        logs.push(log.map_err(|e| e.to_string())?);
    }
    Ok(logs)
}

/// Get logs by ID list with full details for export
pub fn get_logs_by_ids(ids: &[String]) -> Result<Vec<ProxyRequestLog>, String> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    
    let db_path = get_proxy_db_path()?;
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    
    // Build placeholders for IN clause
    let placeholders: Vec<String> = ids.iter().enumerate().map(|(i, _)| format!("?{}", i + 1)).collect();
    let sql = format!(
        "SELECT id, timestamp, method, url, status, duration, model, error, 
                request_body, response_body, input_tokens, output_tokens, 
                account_email, mapped_model
         FROM request_logs 
         WHERE id IN ({})
         ORDER BY timestamp DESC",
        placeholders.join(", ")
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    
    // Convert ids to params
    let params: Vec<&dyn rusqlite::ToSql> = ids.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
    
    let logs_iter = stmt.query_map(params.as_slice(), |row| {
        Ok(ProxyRequestLog {
            id: row.get(0)?,
            timestamp: row.get(1)?,
            method: row.get(2)?,
            url: row.get(3)?,
            status: row.get(4)?,
            duration: row.get(5)?,
            model: row.get(6)?,
            mapped_model: row.get(13).unwrap_or(None),
            account_email: row.get(12).unwrap_or(None),
            error: row.get(7)?,
            request_body: row.get(8).unwrap_or(None),
            response_body: row.get(9).unwrap_or(None),
            input_tokens: row.get(10).unwrap_or(None),
            output_tokens: row.get(11).unwrap_or(None),
        })
    }).map_err(|e| e.to_string())?;

    let mut logs = Vec::new();
    for log in logs_iter {
        logs.push(log.map_err(|e| e.to_string())?);
    }
    Ok(logs)
}
