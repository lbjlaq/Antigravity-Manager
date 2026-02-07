// File: src-tauri/src/modules/security_db.rs
//! Security database module for IP blacklist/whitelist management.
//! Uses SQLite with WAL mode for concurrent access safety.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use once_cell::sync::Lazy;

/// Global database connection pool (single connection with mutex for thread safety)
static DB_CONNECTION: Lazy<Mutex<Option<Connection>>> = Lazy::new(|| Mutex::new(None));

/// IP Blacklist entry
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IpBlacklistEntry {
    pub id: i64,
    pub ip_pattern: String,
    pub reason: String,
    pub created_at: i64,
    pub expires_at: Option<i64>,
    pub created_by: String,
    pub hit_count: i64,
}

/// IP Whitelist entry
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IpWhitelistEntry {
    pub id: i64,
    pub ip_pattern: String,
    pub description: String,
    pub created_at: i64,
    pub created_by: String,
}

/// Access log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessLogEntry {
    pub id: i64,
    pub ip_address: String,
    pub path: String,
    pub method: String,
    pub status_code: i32,
    pub blocked: bool,
    pub block_reason: Option<String>,
    pub timestamp: i64,
    pub user_agent: Option<String>,
}

/// Security statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityStats {
    pub total_requests: i64,
    pub blocked_requests: i64,
    pub unique_ips: i64,
    pub blacklist_count: i64,
    pub whitelist_count: i64,
    pub top_blocked_ips: Vec<(String, i64)>,
}

/// Get the security database path
fn get_db_path() -> Result<PathBuf, String> {
    let data_dir = crate::modules::account::get_data_dir()
        .map_err(|e| format!("Failed to get data dir: {}", e))?;
    Ok(data_dir.join("security.db"))
}

/// Connect to the security database
fn connect_db() -> Result<Connection, String> {
    let db_path = get_db_path()?;
    let conn = Connection::open(&db_path)
        .map_err(|e| format!("Failed to open security database: {}", e))?;
    
    // Enable WAL mode for better concurrency
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
        .map_err(|e| format!("Failed to set PRAGMA: {}", e))?;
    
    Ok(conn)
}

/// Initialize the security database schema
pub fn init_db() -> Result<(), String> {
    let conn = connect_db()?;
    
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS ip_blacklist (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ip_pattern TEXT NOT NULL UNIQUE,
            reason TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL,
            expires_at INTEGER,
            created_by TEXT NOT NULL DEFAULT 'system',
            hit_count INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS ip_whitelist (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ip_pattern TEXT NOT NULL UNIQUE,
            description TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL,
            created_by TEXT NOT NULL DEFAULT 'system'
        );

        CREATE TABLE IF NOT EXISTS access_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ip_address TEXT NOT NULL,
            path TEXT NOT NULL,
            method TEXT NOT NULL,
            status_code INTEGER NOT NULL,
            blocked INTEGER NOT NULL DEFAULT 0,
            block_reason TEXT,
            timestamp INTEGER NOT NULL,
            user_agent TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_blacklist_ip ON ip_blacklist(ip_pattern);
        CREATE INDEX IF NOT EXISTS idx_blacklist_expires ON ip_blacklist(expires_at);
        CREATE INDEX IF NOT EXISTS idx_whitelist_ip ON ip_whitelist(ip_pattern);
        CREATE INDEX IF NOT EXISTS idx_access_log_ip ON access_log(ip_address);
        CREATE INDEX IF NOT EXISTS idx_access_log_timestamp ON access_log(timestamp);
        CREATE INDEX IF NOT EXISTS idx_access_log_blocked ON access_log(blocked);
        "#,
    )
    .map_err(|e| format!("Failed to create security tables: {}", e))?;

    tracing::info!("[Security] Database initialized at {:?}", get_db_path()?);
    Ok(())
}

// ============================================================================
// BLACKLIST OPERATIONS
// ============================================================================

/// Add IP to blacklist
pub fn add_to_blacklist(
    ip_pattern: &str,
    reason: &str,
    expires_at: Option<i64>,
    created_by: &str,
) -> Result<i64, String> {
    if ip_pattern.trim().is_empty() {
        return Err("IP pattern cannot be empty".to_string());
    }

    let conn = connect_db()?;
    let now = chrono::Utc::now().timestamp();

    conn.execute(
        "INSERT OR REPLACE INTO ip_blacklist (ip_pattern, reason, created_at, expires_at, created_by, hit_count)
         VALUES (?1, ?2, ?3, ?4, ?5, 0)",
        params![ip_pattern.trim(), reason, now, expires_at, created_by],
    )
    .map_err(|e| format!("Failed to add to blacklist: {}", e))?;

    let id = conn.last_insert_rowid();
    tracing::info!(
        "[Security] Added to blacklist: {} (reason: {}, expires: {:?})",
        ip_pattern,
        reason,
        expires_at
    );

    Ok(id)
}

/// Remove IP from blacklist
pub fn remove_from_blacklist(ip_pattern: &str) -> Result<bool, String> {
    let conn = connect_db()?;
    let affected = conn
        .execute(
            "DELETE FROM ip_blacklist WHERE ip_pattern = ?1",
            [ip_pattern],
        )
        .map_err(|e| format!("Failed to remove from blacklist: {}", e))?;

    if affected > 0 {
        tracing::info!("[Security] Removed from blacklist: {}", ip_pattern);
    }

    Ok(affected > 0)
}

/// Remove IP from blacklist by ID
pub fn remove_from_blacklist_by_id(id: i64) -> Result<bool, String> {
    let conn = connect_db()?;
    let affected = conn
        .execute("DELETE FROM ip_blacklist WHERE id = ?1", [id])
        .map_err(|e| format!("Failed to remove from blacklist: {}", e))?;

    if affected > 0 {
        tracing::info!("[Security] Removed from blacklist by ID: {}", id);
    }

    Ok(affected > 0)
}

/// Get all blacklist entries (excluding expired)
pub fn get_blacklist() -> Result<Vec<IpBlacklistEntry>, String> {
    let conn = connect_db()?;
    let now = chrono::Utc::now().timestamp();

    // Clean up expired entries first
    let _ = conn.execute(
        "DELETE FROM ip_blacklist WHERE expires_at IS NOT NULL AND expires_at < ?1",
        [now],
    );

    let mut stmt = conn
        .prepare(
            "SELECT id, ip_pattern, reason, created_at, expires_at, created_by, hit_count
             FROM ip_blacklist
             ORDER BY created_at DESC",
        )
        .map_err(|e| format!("Failed to prepare statement: {}", e))?;

    let entries = stmt
        .query_map([], |row| {
            Ok(IpBlacklistEntry {
                id: row.get(0)?,
                ip_pattern: row.get(1)?,
                reason: row.get(2)?,
                created_at: row.get(3)?,
                expires_at: row.get(4)?,
                created_by: row.get(5)?,
                hit_count: row.get(6)?,
            })
        })
        .map_err(|e| format!("Failed to query blacklist: {}", e))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(entries)
}

/// Check if IP is in blacklist (with CIDR support)
pub fn is_ip_in_blacklist(ip: &str) -> Result<bool, String> {
    get_blacklist_entry_for_ip(ip).map(|entry| entry.is_some())
}

/// Get blacklist entry for IP (if exists)
pub fn get_blacklist_entry_for_ip(ip: &str) -> Result<Option<IpBlacklistEntry>, String> {
    let conn = connect_db()?;
    let now = chrono::Utc::now().timestamp();

    // Try exact match first
    let entry_result: Result<IpBlacklistEntry, _> = conn.query_row(
        "SELECT id, ip_pattern, reason, created_at, expires_at, created_by, hit_count
         FROM ip_blacklist
         WHERE ip_pattern = ?1 AND (expires_at IS NULL OR expires_at > ?2)",
        params![ip, now],
        |row| {
            Ok(IpBlacklistEntry {
                id: row.get(0)?,
                ip_pattern: row.get(1)?,
                reason: row.get(2)?,
                created_at: row.get(3)?,
                expires_at: row.get(4)?,
                created_by: row.get(5)?,
                hit_count: row.get(6)?,
            })
        },
    );

    if let Ok(entry) = entry_result {
        // Increment hit count
        let _ = conn.execute(
            "UPDATE ip_blacklist SET hit_count = hit_count + 1 WHERE id = ?1",
            [entry.id],
        );
        return Ok(Some(entry));
    }

    // Check CIDR patterns
    let entries = get_blacklist()?;
    for entry in entries {
        if entry.ip_pattern.contains('/') && cidr_matches(&entry.ip_pattern, ip) {
            // Increment hit count
            let _ = conn.execute(
                "UPDATE ip_blacklist SET hit_count = hit_count + 1 WHERE id = ?1",
                [entry.id],
            );
            return Ok(Some(entry));
        }
    }

    Ok(None)
}

// ============================================================================
// WHITELIST OPERATIONS
// ============================================================================

/// Add IP to whitelist
pub fn add_to_whitelist(
    ip_pattern: &str,
    description: &str,
    created_by: &str,
) -> Result<i64, String> {
    if ip_pattern.trim().is_empty() {
        return Err("IP pattern cannot be empty".to_string());
    }

    let conn = connect_db()?;
    let now = chrono::Utc::now().timestamp();

    conn.execute(
        "INSERT OR REPLACE INTO ip_whitelist (ip_pattern, description, created_at, created_by)
         VALUES (?1, ?2, ?3, ?4)",
        params![ip_pattern.trim(), description, now, created_by],
    )
    .map_err(|e| format!("Failed to add to whitelist: {}", e))?;

    let id = conn.last_insert_rowid();
    tracing::info!("[Security] Added to whitelist: {}", ip_pattern);

    Ok(id)
}

/// Remove IP from whitelist
pub fn remove_from_whitelist(ip_pattern: &str) -> Result<bool, String> {
    let conn = connect_db()?;
    let affected = conn
        .execute(
            "DELETE FROM ip_whitelist WHERE ip_pattern = ?1",
            [ip_pattern],
        )
        .map_err(|e| format!("Failed to remove from whitelist: {}", e))?;

    if affected > 0 {
        tracing::info!("[Security] Removed from whitelist: {}", ip_pattern);
    }

    Ok(affected > 0)
}

/// Remove IP from whitelist by ID
pub fn remove_from_whitelist_by_id(id: i64) -> Result<bool, String> {
    let conn = connect_db()?;
    let affected = conn
        .execute("DELETE FROM ip_whitelist WHERE id = ?1", [id])
        .map_err(|e| format!("Failed to remove from whitelist: {}", e))?;

    if affected > 0 {
        tracing::info!("[Security] Removed from whitelist by ID: {}", id);
    }

    Ok(affected > 0)
}

/// Get all whitelist entries
pub fn get_whitelist() -> Result<Vec<IpWhitelistEntry>, String> {
    let conn = connect_db()?;

    let mut stmt = conn
        .prepare(
            "SELECT id, ip_pattern, description, created_at, created_by
             FROM ip_whitelist
             ORDER BY created_at DESC",
        )
        .map_err(|e| format!("Failed to prepare statement: {}", e))?;

    let entries = stmt
        .query_map([], |row| {
            Ok(IpWhitelistEntry {
                id: row.get(0)?,
                ip_pattern: row.get(1)?,
                description: row.get(2)?,
                created_at: row.get(3)?,
                created_by: row.get(4)?,
            })
        })
        .map_err(|e| format!("Failed to query whitelist: {}", e))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(entries)
}

/// Check if IP is in whitelist (with CIDR support)
pub fn is_ip_in_whitelist(ip: &str) -> Result<bool, String> {
    let conn = connect_db()?;

    // Try exact match first
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM ip_whitelist WHERE ip_pattern = ?1",
            [ip],
            |row| row.get(0),
        )
        .map_err(|e| format!("Failed to check whitelist: {}", e))?;

    if count > 0 {
        return Ok(true);
    }

    // Check CIDR patterns
    let entries = get_whitelist()?;
    for entry in entries {
        if entry.ip_pattern.contains('/') && cidr_matches(&entry.ip_pattern, ip) {
            return Ok(true);
        }
    }

    Ok(false)
}

// ============================================================================
// ACCESS LOG OPERATIONS
// ============================================================================

/// Log an access attempt
pub fn log_access(
    ip_address: &str,
    path: &str,
    method: &str,
    status_code: i32,
    blocked: bool,
    block_reason: Option<&str>,
    user_agent: Option<&str>,
) -> Result<i64, String> {
    let conn = connect_db()?;
    let now = chrono::Utc::now().timestamp();

    conn.execute(
        "INSERT INTO access_log (ip_address, path, method, status_code, blocked, block_reason, timestamp, user_agent)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![ip_address, path, method, status_code, blocked as i32, block_reason, now, user_agent],
    )
    .map_err(|e| format!("Failed to log access: {}", e))?;

    Ok(conn.last_insert_rowid())
}

/// Get access logs with optional filters
pub fn get_access_logs(
    limit: i64,
    offset: i64,
    blocked_only: bool,
    ip_filter: Option<&str>,
) -> Result<Vec<AccessLogEntry>, String> {
    let conn = connect_db()?;

    let mut sql = String::from(
        "SELECT id, ip_address, path, method, status_code, blocked, block_reason, timestamp, user_agent
         FROM access_log WHERE 1=1",
    );

    if blocked_only {
        sql.push_str(" AND blocked = 1");
    }

    if ip_filter.is_some() {
        sql.push_str(" AND ip_address LIKE ?1");
    }

    sql.push_str(" ORDER BY timestamp DESC LIMIT ?2 OFFSET ?3");

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| format!("Failed to prepare statement: {}", e))?;

    let entries: Vec<AccessLogEntry> = if let Some(ip) = ip_filter {
        let pattern = format!("%{}%", ip);
        stmt.query_map(params![pattern, limit, offset], |row| {
            Ok(AccessLogEntry {
                id: row.get(0)?,
                ip_address: row.get(1)?,
                path: row.get(2)?,
                method: row.get(3)?,
                status_code: row.get(4)?,
                blocked: row.get::<_, i32>(5)? != 0,
                block_reason: row.get(6)?,
                timestamp: row.get(7)?,
                user_agent: row.get(8)?,
            })
        })
        .map_err(|e| format!("Failed to query access logs: {}", e))?
        .filter_map(|r| r.ok())
        .collect()
    } else {
        stmt.query_map(params![limit, offset], |row| {
            Ok(AccessLogEntry {
                id: row.get(0)?,
                ip_address: row.get(1)?,
                path: row.get(2)?,
                method: row.get(3)?,
                status_code: row.get(4)?,
                blocked: row.get::<_, i32>(5)? != 0,
                block_reason: row.get(6)?,
                timestamp: row.get(7)?,
                user_agent: row.get(8)?,
            })
        })
        .map_err(|e| format!("Failed to query access logs: {}", e))?
        .filter_map(|r| r.ok())
        .collect()
    };

    Ok(entries)
}

/// Clear old access logs (older than specified days)
pub fn cleanup_old_logs(days: i64) -> Result<i64, String> {
    let conn = connect_db()?;
    let cutoff = chrono::Utc::now().timestamp() - (days * 24 * 60 * 60);

    let affected = conn
        .execute("DELETE FROM access_log WHERE timestamp < ?1", [cutoff])
        .map_err(|e| format!("Failed to cleanup logs: {}", e))?;

    if affected > 0 {
        tracing::info!("[Security] Cleaned up {} old access log entries", affected);
    }

    Ok(affected as i64)
}

/// Clear all access logs
pub fn clear_all_logs() -> Result<i64, String> {
    let conn = connect_db()?;
    let affected = conn
        .execute("DELETE FROM access_log", [])
        .map_err(|e| format!("Failed to clear logs: {}", e))?;

    tracing::info!("[Security] Cleared all {} access log entries", affected);
    Ok(affected as i64)
}

// ============================================================================
// STATISTICS
// ============================================================================

/// Get security statistics
pub fn get_stats() -> Result<SecurityStats, String> {
    let conn = connect_db()?;

    let total_requests: i64 = conn
        .query_row("SELECT COUNT(*) FROM access_log", [], |row| row.get(0))
        .unwrap_or(0);

    let blocked_requests: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM access_log WHERE blocked = 1",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let unique_ips: i64 = conn
        .query_row(
            "SELECT COUNT(DISTINCT ip_address) FROM access_log",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let blacklist_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM ip_blacklist", [], |row| row.get(0))
        .unwrap_or(0);

    let whitelist_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM ip_whitelist", [], |row| row.get(0))
        .unwrap_or(0);

    // Top 10 blocked IPs
    let mut stmt = conn
        .prepare(
            "SELECT ip_address, COUNT(*) as cnt FROM access_log
             WHERE blocked = 1
             GROUP BY ip_address
             ORDER BY cnt DESC
             LIMIT 10",
        )
        .map_err(|e| format!("Failed to prepare statement: {}", e))?;

    let top_blocked_ips: Vec<(String, i64)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|e| format!("Failed to query top blocked: {}", e))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(SecurityStats {
        total_requests,
        blocked_requests,
        unique_ips,
        blacklist_count,
        whitelist_count,
        top_blocked_ips,
    })
}

// ============================================================================
// CIDR MATCHING UTILITIES
// ============================================================================

/// Check if an IP matches a CIDR pattern
fn cidr_matches(cidr: &str, ip: &str) -> bool {
    let parts: Vec<&str> = cidr.split('/').collect();
    if parts.len() != 2 {
        return false;
    }

    let network = parts[0];
    let prefix_len: u8 = match parts[1].parse() {
        Ok(p) => p,
        Err(_) => return false,
    };

    if prefix_len > 32 {
        return false;
    }

    let network_octets = match parse_ipv4(network) {
        Some(o) => o,
        None => return false,
    };

    let ip_octets = match parse_ipv4(ip) {
        Some(o) => o,
        None => return false,
    };

    let network_int = octets_to_u32(&network_octets);
    let ip_int = octets_to_u32(&ip_octets);

    let mask = if prefix_len == 0 {
        0
    } else {
        !0u32 << (32 - prefix_len)
    };

    (network_int & mask) == (ip_int & mask)
}

/// Parse IPv4 address string to octets
fn parse_ipv4(ip: &str) -> Option<[u8; 4]> {
    let parts: Vec<&str> = ip.split('.').collect();
    if parts.len() != 4 {
        return None;
    }

    let mut octets = [0u8; 4];
    for (i, part) in parts.iter().enumerate() {
        match part.parse::<u8>() {
            Ok(n) => octets[i] = n,
            Err(_) => return None,
        }
    }

    Some(octets)
}

/// Convert octets to u32
fn octets_to_u32(octets: &[u8; 4]) -> u32 {
    ((octets[0] as u32) << 24)
        | ((octets[1] as u32) << 16)
        | ((octets[2] as u32) << 8)
        | (octets[3] as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cidr_matches() {
        // /24 subnet
        assert!(cidr_matches("192.168.1.0/24", "192.168.1.100"));
        assert!(cidr_matches("192.168.1.0/24", "192.168.1.1"));
        assert!(cidr_matches("192.168.1.0/24", "192.168.1.255"));
        assert!(!cidr_matches("192.168.1.0/24", "192.168.2.1"));

        // /16 subnet
        assert!(cidr_matches("10.0.0.0/16", "10.0.1.1"));
        assert!(cidr_matches("10.0.0.0/16", "10.0.255.255"));
        assert!(!cidr_matches("10.0.0.0/16", "10.1.0.1"));

        // /8 subnet
        assert!(cidr_matches("10.0.0.0/8", "10.255.255.255"));
        assert!(!cidr_matches("10.0.0.0/8", "11.0.0.1"));

        // /32 exact match
        assert!(cidr_matches("192.168.1.1/32", "192.168.1.1"));
        assert!(!cidr_matches("192.168.1.1/32", "192.168.1.2"));

        // /0 matches all
        assert!(cidr_matches("0.0.0.0/0", "1.2.3.4"));
        assert!(cidr_matches("0.0.0.0/0", "255.255.255.255"));
    }

    #[test]
    fn test_parse_ipv4() {
        assert_eq!(parse_ipv4("192.168.1.1"), Some([192, 168, 1, 1]));
        assert_eq!(parse_ipv4("0.0.0.0"), Some([0, 0, 0, 0]));
        assert_eq!(parse_ipv4("255.255.255.255"), Some([255, 255, 255, 255]));
        assert_eq!(parse_ipv4("invalid"), None);
        assert_eq!(parse_ipv4("192.168.1"), None);
        assert_eq!(parse_ipv4("192.168.1.256"), None);
    }
}
