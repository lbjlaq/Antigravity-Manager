use crate::proxy::monitor::ProxyRequestLog;
use rusqlite::{params, Connection};
use std::path::PathBuf;

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
    let _ = conn.execute("ALTER TABLE request_logs ADD COLUMN account_email TEXT", []);
    let _ = conn.execute("ALTER TABLE request_logs ADD COLUMN mapped_model TEXT", []);

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_timestamp ON request_logs (timestamp DESC)",
        [],
    )
    .map_err(|e| e.to_string())?;

    // 🆕 Story #8: Run migration for proxy_stats table
    migrate_stats_table()?;

    // 🆕 Story-008-02: Run migration for cache_metrics tables
    migrate_cache_metrics_table()?;

    // 🆕 Story-008-01: Run migration for budget_patterns table
    migrate_budget_patterns_table()?;

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

pub fn get_logs(limit: usize) -> Result<Vec<ProxyRequestLog>, String> {
    let db_path = get_proxy_db_path()?;
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;

    let mut stmt = conn.prepare(
        "SELECT id, timestamp, method, url, status, duration, model, error, request_body, response_body, input_tokens, output_tokens, account_email, mapped_model
         FROM request_logs 
         ORDER BY timestamp DESC 
         LIMIT ?1"
    ).map_err(|e| e.to_string())?;

    let logs_iter = stmt
        .query_map([limit], |row| {
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
        })
        .map_err(|e| e.to_string())?;

    let mut logs = Vec::new();
    for log in logs_iter {
        logs.push(log.map_err(|e| e.to_string())?);
    }
    Ok(logs)
}

// 🆕 Story #8: Updated to load from proxy_stats table
pub fn get_stats() -> Result<crate::proxy::monitor::ProxyStats, String> {
    let db_path = get_proxy_db_path()?;
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;

    // Try to load from proxy_stats table first
    let stats_result = conn.query_row(
        "SELECT total_requests, success_count, error_count,
                thinking_budget_violations, thinking_position_violations,
                thinking_position_violations_user, thinking_position_violations_model
         FROM proxy_stats WHERE id = 1",
        [],
        |row| {
            Ok(crate::proxy::monitor::ProxyStats {
                total_requests: row.get(0)?,
                success_count: row.get(1)?,
                error_count: row.get(2)?,
                thinking_budget_violations: row.get(3)?,
                thinking_position_violations: row.get(4)?,
                thinking_position_violations_user: row.get(5)?,
                thinking_position_violations_model: row.get(6)?,
            })
        },
    );

    match stats_result {
        Ok(stats) => Ok(stats),
        Err(_) => {
            // Fallback to calculating from request_logs if proxy_stats doesn't exist yet
            tracing::warn!("[ProxyDB] proxy_stats table not found, calculating from logs");

            let total_requests: u64 = conn
                .query_row("SELECT COUNT(*) FROM request_logs", [], |row| row.get(0))
                .map_err(|e| e.to_string())?;

            let success_count: u64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM request_logs WHERE status >= 200 AND status < 400",
                    [],
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;

            let error_count: u64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM request_logs WHERE status < 200 OR status >= 400",
                    [],
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;

            Ok(crate::proxy::monitor::ProxyStats {
                total_requests,
                success_count,
                error_count,
                thinking_budget_violations: 0,
                thinking_position_violations: 0,
                thinking_position_violations_user: 0,
                thinking_position_violations_model: 0,
            })
        }
    }
}

pub fn clear_logs() -> Result<(), String> {
    let db_path = get_proxy_db_path()?;
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM request_logs", [])
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ========== Story #8: Violation Metrics Database Integration ==========

/// 🆕 Story #8 Step 10: Migrate proxy_stats table
/// Creates proxy_stats table with violation metrics columns
/// Idempotent - safe to call multiple times
pub fn migrate_stats_table() -> Result<(), String> {
    let db_path = get_proxy_db_path()?;
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;

    // Check if table already exists and has violation columns
    let has_violation_columns: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('proxy_stats')
             WHERE name IN ('thinking_budget_violations', 'thinking_position_violations',
                           'thinking_position_violations_user', 'thinking_position_violations_model')",
            [],
            |row| {
                let count: i32 = row.get(0)?;
                Ok(count == 4)
            },
        )
        .unwrap_or(false);

    if has_violation_columns {
        tracing::info!("[ProxyDB] proxy_stats table already migrated with violation columns");
        return Ok(());
    }

    // Create proxy_stats table if not exists
    conn.execute(
        "CREATE TABLE IF NOT EXISTS proxy_stats (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            total_requests INTEGER DEFAULT 0,
            success_count INTEGER DEFAULT 0,
            error_count INTEGER DEFAULT 0,
            thinking_budget_violations INTEGER DEFAULT 0,
            thinking_position_violations INTEGER DEFAULT 0,
            thinking_position_violations_user INTEGER DEFAULT 0,
            thinking_position_violations_model INTEGER DEFAULT 0,
            updated_at INTEGER
        )",
        [],
    )
    .map_err(|e| format!("Failed to create proxy_stats table: {}", e))?;

    // Insert initial row (id=1, single-row table pattern)
    conn.execute("INSERT OR IGNORE INTO proxy_stats (id) VALUES (1)", [])
        .map_err(|e| format!("Failed to insert initial proxy_stats row: {}", e))?;

    tracing::info!("[ProxyDB] Successfully migrated proxy_stats table with violation metrics");
    Ok(())
}

/// 🆕 Story #8 Step 11: Save stats to database
/// Persists ProxyStats including violation counters
pub fn save_stats(stats: &crate::proxy::monitor::ProxyStats) -> Result<(), String> {
    let db_path = get_proxy_db_path()?;
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;

    let now = chrono::Utc::now().timestamp();

    conn.execute(
        "INSERT OR REPLACE INTO proxy_stats (
            id,
            total_requests,
            success_count,
            error_count,
            thinking_budget_violations,
            thinking_position_violations,
            thinking_position_violations_user,
            thinking_position_violations_model,
            updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            1, // Single-row table, always id=1
            stats.total_requests,
            stats.success_count,
            stats.error_count,
            stats.thinking_budget_violations,
            stats.thinking_position_violations,
            stats.thinking_position_violations_user,
            stats.thinking_position_violations_model,
            now,
        ],
    )
    .map_err(|e| format!("Failed to save proxy stats: {}", e))?;

    Ok(())
}

// ========== Story-008-02: Cache Metrics Database Integration ==========

/// Create cache_metrics table for signature cache monitoring
/// Story-008-02 AC5: Dashboard integration requires persistence
pub fn migrate_cache_metrics_table() -> Result<(), String> {
    let db_path = get_proxy_db_path()?;
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;

    // Check if cache_metrics table exists
    let has_cache_metrics: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='cache_metrics'",
            [],
            |row| {
                let count: i32 = row.get(0)?;
                Ok(count > 0)
            },
        )
        .unwrap_or(false);

    if has_cache_metrics {
        tracing::info!("[ProxyDB] cache_metrics table already exists");
        return Ok(());
    }

    // Create cache_metrics table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS cache_metrics (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            hit_count INTEGER DEFAULT 0,
            miss_count INTEGER DEFAULT 0,
            hit_rate REAL DEFAULT 0.0,
            total_cost_saved REAL DEFAULT 0.0,
            savings_percentage REAL DEFAULT 0.0,
            lookup_p50 REAL DEFAULT 0.0,
            lookup_p95 REAL DEFAULT 0.0,
            lookup_p99 REAL DEFAULT 0.0,
            write_p95 REAL DEFAULT 0.0,
            memory_usage INTEGER DEFAULT 0,
            total_operations INTEGER DEFAULT 0,
            degradation_alert INTEGER DEFAULT 0,
            updated_at INTEGER
        )",
        [],
    )
    .map_err(|e| format!("Failed to create cache_metrics table: {}", e))?;

    // Create signature_stats table for top signatures tracking
    conn.execute(
        "CREATE TABLE IF NOT EXISTS signature_stats (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            signature TEXT NOT NULL,
            signature_hash TEXT UNIQUE NOT NULL,
            reuse_count INTEGER DEFAULT 0,
            first_cached INTEGER NOT NULL,
            last_used INTEGER NOT NULL,
            cost_saved REAL DEFAULT 0.0,
            avg_lookup_time REAL DEFAULT 0.0,
            high_value INTEGER DEFAULT 0
        )",
        [],
    )
    .map_err(|e| format!("Failed to create signature_stats table: {}", e))?;

    // Create index on reuse_count for top signatures query
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_reuse_count ON signature_stats (reuse_count DESC)",
        [],
    )
    .map_err(|e| format!("Failed to create reuse_count index: {}", e))?;

    // Insert initial cache_metrics row
    conn.execute("INSERT OR IGNORE INTO cache_metrics (id) VALUES (1)", [])
        .map_err(|e| format!("Failed to insert initial cache_metrics row: {}", e))?;

    tracing::info!("[ProxyDB] Successfully migrated cache_metrics tables");
    Ok(())
}

/// Save cache metrics to database
/// Story-008-02 AC5: Persist metrics for dashboard
pub fn save_cache_metrics(metrics: &crate::proxy::cache_monitor::CacheMetrics) -> Result<(), String> {
    let db_path = get_proxy_db_path()?;
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;

    let now = chrono::Utc::now().timestamp();

    conn.execute(
        "INSERT OR REPLACE INTO cache_metrics (
            id,
            hit_count,
            miss_count,
            hit_rate,
            total_cost_saved,
            savings_percentage,
            lookup_p50,
            lookup_p95,
            lookup_p99,
            write_p95,
            memory_usage,
            total_operations,
            degradation_alert,
            updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            1, // Single-row table, always id=1
            metrics.hit_count as i64,
            metrics.miss_count as i64,
            metrics.hit_rate,
            metrics.cost_savings.total_saved,
            metrics.cost_savings.savings_percentage,
            metrics.performance.lookup_p50,
            metrics.performance.lookup_p95,
            metrics.performance.lookup_p99,
            metrics.performance.write_p95,
            metrics.performance.memory_usage as i64,
            metrics.performance.total_operations as i64,
            if metrics.performance.degradation_alert { 1 } else { 0 },
            now,
        ],
    )
    .map_err(|e| format!("Failed to save cache metrics: {}", e))?;

    Ok(())
}

/// Load cache metrics from database
/// Story-008-02 AC5: Load persisted metrics for dashboard
pub fn load_cache_metrics() -> Result<crate::proxy::cache_monitor::CacheMetrics, String> {
    let db_path = get_proxy_db_path()?;
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;

    let metrics_result = conn.query_row(
        "SELECT hit_count, miss_count, hit_rate,
                total_cost_saved, savings_percentage,
                lookup_p50, lookup_p95, lookup_p99, write_p95,
                memory_usage, total_operations, degradation_alert,
                updated_at
         FROM cache_metrics WHERE id = 1",
        [],
        |row| {
            use crate::proxy::cache_monitor::{CacheMetrics, CostSavings, PerformanceMetrics};

            let hit_count: i64 = row.get(0)?;
            let miss_count: i64 = row.get(1)?;
            let degradation_flag: i64 = row.get(11)?;
            let updated_timestamp: i64 = row.get(12)?;

            Ok(CacheMetrics {
                hit_count: hit_count as u64,
                miss_count: miss_count as u64,
                hit_rate: row.get(2)?,
                top_signatures: Vec::new(), // Loaded separately
                cost_savings: CostSavings {
                    total_saved: row.get(3)?,
                    savings_percentage: row.get(4)?,
                    per_account: std::collections::HashMap::new(),
                    per_user: std::collections::HashMap::new(),
                    hourly_savings: Vec::new(),
                    daily_savings: Vec::new(),
                },
                performance: PerformanceMetrics {
                    lookup_p50: row.get(5)?,
                    lookup_p95: row.get(6)?,
                    lookup_p99: row.get(7)?,
                    write_p95: row.get(8)?,
                    memory_usage: row.get::<_, i64>(9)? as u64,
                    total_operations: row.get::<_, i64>(10)? as u64,
                    degradation_alert: degradation_flag != 0,
                },
                updated_at: chrono::DateTime::from_timestamp(updated_timestamp, 0)
                    .unwrap_or_else(chrono::Utc::now),
            })
        },
    );

    match metrics_result {
        Ok(metrics) => Ok(metrics),
        Err(_) => {
            // Return default metrics if table doesn't exist yet
            Ok(crate::proxy::cache_monitor::CacheMetrics::default())
        }
    }
}

/// Save signature stats to database
/// Story-008-02 AC2: Persist top signatures for analysis
pub fn save_signature_stats(
    stats: &[crate::proxy::cache_monitor::SignatureStats],
) -> Result<(), String> {
    let db_path = get_proxy_db_path()?;
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;

    for stat in stats {
        // Use simple hash of signature as unique key
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        stat.signature.hash(&mut hasher);
        let sig_hash = format!("{:x}", hasher.finish());

        conn.execute(
            "INSERT OR REPLACE INTO signature_stats (
                signature_hash,
                signature,
                reuse_count,
                first_cached,
                last_used,
                cost_saved,
                avg_lookup_time,
                high_value
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                sig_hash,
                stat.signature,
                stat.reuse_count as i64,
                stat.first_cached.timestamp(),
                stat.last_used.timestamp(),
                stat.cost_saved,
                stat.avg_lookup_time,
                if stat.high_value { 1 } else { 0 },
            ],
        )
        .map_err(|e| format!("Failed to save signature stats: {}", e))?;
    }

    Ok(())
}

/// Load top signature stats from database
/// Story-008-02 AC2: Load persisted signature analytics
pub fn load_top_signatures(limit: usize) -> Result<Vec<crate::proxy::cache_monitor::SignatureStats>, String> {
    let db_path = get_proxy_db_path()?;
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;

    let mut stmt = conn
        .prepare(
            "SELECT signature, reuse_count, first_cached, last_used,
                    cost_saved, avg_lookup_time, high_value
             FROM signature_stats
             ORDER BY reuse_count DESC
             LIMIT ?1",
        )
        .map_err(|e| e.to_string())?;

    let stats_iter = stmt
        .query_map([limit], |row| {
            use crate::proxy::cache_monitor::SignatureStats;

            let first_cached_ts: i64 = row.get(2)?;
            let last_used_ts: i64 = row.get(3)?;
            let high_value_flag: i64 = row.get(6)?;

            Ok(SignatureStats {
                signature: row.get(0)?,
                reuse_count: row.get::<_, i64>(1)? as u64,
                first_cached: chrono::DateTime::from_timestamp(first_cached_ts, 0)
                    .unwrap_or_else(chrono::Utc::now),
                last_used: chrono::DateTime::from_timestamp(last_used_ts, 0)
                    .unwrap_or_else(chrono::Utc::now),
                cost_saved: row.get(4)?,
                avg_lookup_time: row.get(5)?,
                high_value: high_value_flag != 0,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut stats = Vec::new();
    for stat in stats_iter {
        stats.push(stat.map_err(|e| e.to_string())?);
    }
    Ok(stats)
}

// ========== Story-008-01: Budget Patterns Database Integration ==========

/// Create budget_patterns table for adaptive budget optimization
/// Story-008-01 AC3: Historical pattern storage
pub fn migrate_budget_patterns_table() -> Result<(), String> {
    let db_path = get_proxy_db_path()?;
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;

    // Check if budget_patterns table exists
    let has_budget_patterns: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='budget_patterns'",
            [],
            |row| {
                let count: i32 = row.get(0)?;
                Ok(count > 0)
            },
        )
        .unwrap_or(false);

    if has_budget_patterns {
        tracing::info!("[ProxyDB] budget_patterns table already exists");
        return Ok(());
    }

    // Create budget_patterns table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS budget_patterns (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            prompt_hash TEXT UNIQUE NOT NULL,
            complexity_level TEXT NOT NULL,
            avg_budget INTEGER NOT NULL,
            usage_count INTEGER DEFAULT 1,
            total_quality_score REAL DEFAULT 0.0,
            last_used INTEGER NOT NULL,
            created_at INTEGER NOT NULL
        )",
        [],
    )
    .map_err(|e| format!("Failed to create budget_patterns table: {}", e))?;

    // Create index on prompt_hash for fast lookups
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_budget_patterns_hash ON budget_patterns(prompt_hash)",
        [],
    )
    .map_err(|e| format!("Failed to create prompt_hash index: {}", e))?;

    // Create index on last_used for cleanup queries
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_budget_patterns_last_used ON budget_patterns(last_used DESC)",
        [],
    )
    .map_err(|e| format!("Failed to create last_used index: {}", e))?;

    tracing::info!("[ProxyDB] Successfully migrated budget_patterns table");
    Ok(())
}

/// Save budget pattern to database
/// Story-008-01 AC4: Feedback loop persistence
pub fn save_budget_pattern(
    pattern: &crate::proxy::budget_optimizer::BudgetPattern,
) -> Result<(), String> {
    let db_path = get_proxy_db_path()?;
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;

    conn.execute(
        "INSERT OR REPLACE INTO budget_patterns 
         (prompt_hash, complexity_level, avg_budget, usage_count, total_quality_score, last_used, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            pattern.prompt_hash,
            pattern.complexity_level.to_string(),
            pattern.avg_budget as i64,
            pattern.usage_count as i64,
            pattern.total_quality_score,
            pattern.last_used,
            pattern.created_at,
        ],
    )
    .map_err(|e| format!("Failed to save budget pattern: {}", e))?;

    Ok(())
}

/// Load all budget patterns from database
/// Story-008-01 AC3: Pattern persistence and retrieval
pub fn load_budget_patterns() -> Result<Vec<crate::proxy::budget_optimizer::BudgetPattern>, String> {
    let db_path = get_proxy_db_path()?;
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;

    let mut stmt = conn
        .prepare(
            "SELECT prompt_hash, complexity_level, avg_budget, usage_count, total_quality_score, last_used, created_at
             FROM budget_patterns
             ORDER BY usage_count DESC",
        )
        .map_err(|e| e.to_string())?;

    let patterns_iter = stmt
        .query_map([], |row| {
            let complexity_str: String = row.get(1)?;
            let complexity_level = match complexity_str.as_str() {
                "Simple" => crate::proxy::budget_optimizer::ComplexityLevel::Simple,
                "Moderate" => crate::proxy::budget_optimizer::ComplexityLevel::Moderate,
                "Complex" => crate::proxy::budget_optimizer::ComplexityLevel::Complex,
                "Deep" => crate::proxy::budget_optimizer::ComplexityLevel::Deep,
                _ => crate::proxy::budget_optimizer::ComplexityLevel::Moderate, // Default fallback
            };

            Ok(crate::proxy::budget_optimizer::BudgetPattern {
                prompt_hash: row.get(0)?,
                complexity_level,
                avg_budget: row.get::<_, i64>(2)? as u32,
                usage_count: row.get::<_, i64>(3)? as u32,
                total_quality_score: row.get(4)?,
                last_used: row.get(5)?,
                created_at: row.get(6)?,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut patterns = Vec::new();
    for pattern in patterns_iter {
        patterns.push(pattern.map_err(|e| e.to_string())?);
    }
    Ok(patterns)
}
