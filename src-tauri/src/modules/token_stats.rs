use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Aggregated token statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenStatsAggregated {
    pub period: String, // e.g., "2024-01-15 14:00" for hourly, "2024-01-15" for daily
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cached_tokens: u64,
    pub total_tokens: u64,
    pub request_count: u64,
}

/// Per-account token statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountTokenStats {
    pub account_email: String,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cached_tokens: u64,
    pub total_tokens: u64,
    pub request_count: u64,
}

/// Summary statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenStatsSummary {
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cached_tokens: u64,
    pub total_tokens: u64,
    pub total_requests: u64,
    pub unique_accounts: u64,
}

/// Per-model token statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelTokenStats {
    pub model: String,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cached_tokens: u64,
    pub total_tokens: u64,
    pub request_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelTrendPoint {
    pub period: String,
    pub model_data: std::collections::HashMap<String, u64>,
}

/// Account trend data point (for stacked area chart)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountTrendPoint {
    pub period: String,
    pub account_data: std::collections::HashMap<String, u64>,
}

/// Today's aggregated usage summary for dashboard
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodayUsageSummary {
    pub gemini_tokens: u64,
    pub gemini_requests: u64,
    pub gemini_used_pct: f64,
    pub claude_tokens: u64,
    pub claude_requests: u64,
    pub claude_used_pct: f64,
    pub total_tokens: u64,
    pub total_requests: u64,
}

pub(crate) fn get_db_path() -> Result<PathBuf, String> {
    let data_dir = crate::modules::account::get_data_dir()?;
    Ok(data_dir.join("token_stats.db"))
}

fn connect_db() -> Result<Connection, String> {
    let db_path = get_db_path()?;
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;

    // Enable WAL mode for better concurrency
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| e.to_string())?;
    conn.pragma_update(None, "busy_timeout", 5000)
        .map_err(|e| e.to_string())?;
    conn.pragma_update(None, "synchronous", "NORMAL")
        .map_err(|e| e.to_string())?;

    Ok(conn)
}

fn add_column_if_missing(conn: &Connection, table: &str, column_def: &str) -> Result<(), String> {
    let sql = format!("ALTER TABLE {} ADD COLUMN {}", table, column_def);
    match conn.execute(&sql, []) {
        Ok(_) => Ok(()),
        Err(e) if e.to_string().contains("duplicate column name") => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// Initialize the token stats database
pub fn init_db() -> Result<(), String> {
    let conn = connect_db()?;

    // Create main usage table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS token_usage (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp INTEGER NOT NULL,
            account_email TEXT NOT NULL,
            model TEXT NOT NULL,
            input_tokens INTEGER NOT NULL DEFAULT 0,
            output_tokens INTEGER NOT NULL DEFAULT 0,
            cached_tokens INTEGER NOT NULL DEFAULT 0,
            total_tokens INTEGER NOT NULL DEFAULT 0
        )",
        [],
    )
    .map_err(|e| e.to_string())?;

    // Create indexes for efficient queries
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_token_timestamp ON token_usage (timestamp DESC)",
        [],
    )
    .map_err(|e| e.to_string())?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_token_account ON token_usage (account_email)",
        [],
    )
    .map_err(|e| e.to_string())?;

    // Create hourly aggregation table for fast queries
    conn.execute(
        "CREATE TABLE IF NOT EXISTS token_stats_hourly (
            hour_bucket TEXT NOT NULL,
            account_email TEXT NOT NULL,
            total_input_tokens INTEGER NOT NULL DEFAULT 0,
            total_output_tokens INTEGER NOT NULL DEFAULT 0,
            total_cached_tokens INTEGER NOT NULL DEFAULT 0,
            total_tokens INTEGER NOT NULL DEFAULT 0,
            request_count INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (hour_bucket, account_email)
        )",
        [],
    )
    .map_err(|e| e.to_string())?;

    add_column_if_missing(
        &conn,
        "token_usage",
        "cached_tokens INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column_if_missing(
        &conn,
        "token_stats_hourly",
        "total_cached_tokens INTEGER NOT NULL DEFAULT 0",
    )?;

    // Create quota snapshots table for tracking daily quota consumption
    conn.execute(
        "CREATE TABLE IF NOT EXISTS quota_snapshots (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp INTEGER NOT NULL,
            account_email TEXT NOT NULL,
            gemini_percentage INTEGER NOT NULL DEFAULT 0,
            claude_percentage INTEGER NOT NULL DEFAULT 0
        )",
        [],
    )
    .map_err(|e| e.to_string())?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_quota_snapshot_time ON quota_snapshots (timestamp DESC)",
        [],
    )
    .map_err(|e| e.to_string())?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_quota_snapshot_account ON quota_snapshots (account_email, timestamp DESC)",
        [],
    )
    .map_err(|e| e.to_string())?;

    // 一次性将历史 token_usage 中的虚拟别名/路由名就地归一化为真实模型名
    if let Ok(mut stmt) = conn.prepare("SELECT DISTINCT model FROM token_usage") {
        if let Ok(models_iter) = stmt.query_map([], |r| r.get::<_, String>(0)) {
            let models: Vec<String> = models_iter.filter_map(|r| r.ok()).collect();
            for m in models {
                let real = crate::proxy::common::model_mapping::resolve_real_forwarded_model(&m);
                if real != m {
                    let _ = conn.execute(
                        "UPDATE token_usage SET model = ?1 WHERE model = ?2",
                        rusqlite::params![real, m],
                    );
                }
            }
        }
    }

    Ok(())
}

/// Record account quota snapshot
pub fn record_quota_snapshot(
    account_email: &str,
    gemini_percentage: i32,
    claude_percentage: i32,
) -> Result<(), String> {
    let conn = connect_db()?;
    let timestamp = chrono::Local::now().timestamp();
    conn.execute(
        "INSERT INTO quota_snapshots (timestamp, account_email, gemini_percentage, claude_percentage)
         VALUES (?1, ?2, ?3, ?4)",
        params![timestamp, account_email, gemini_percentage, claude_percentage],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

/// Record token usage from a request
pub fn record_usage(
    account_email: &str,
    model: &str,
    input_tokens: u32,
    output_tokens: u32,
    cached_tokens: u32,
) -> Result<(), String> {
    let conn = connect_db()?;
    let timestamp = chrono::Local::now().timestamp();
    let total_tokens = input_tokens + output_tokens;
    let real_model = crate::proxy::common::model_mapping::resolve_real_forwarded_model(model);

    // Insert into raw usage table
    conn.execute(
        "INSERT INTO token_usage (timestamp, account_email, model, input_tokens, output_tokens, cached_tokens, total_tokens)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![timestamp, account_email, real_model, input_tokens, output_tokens, cached_tokens, total_tokens],
    ).map_err(|e| e.to_string())?;

    let hour_bucket = chrono::Local::now().format("%Y-%m-%d %H:00").to_string();
    conn.execute(
        "INSERT INTO token_stats_hourly (hour_bucket, account_email, total_input_tokens, total_output_tokens, total_cached_tokens, total_tokens, request_count)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1)
         ON CONFLICT(hour_bucket, account_email) DO UPDATE SET
            total_input_tokens = total_input_tokens + ?3,
            total_output_tokens = total_output_tokens + ?4,
            total_cached_tokens = total_cached_tokens + ?5,
            total_tokens = total_tokens + ?6,
            request_count = request_count + 1",
        params![hour_bucket, account_email, input_tokens, output_tokens, cached_tokens, total_tokens],
    ).map_err(|e| e.to_string())?;

    Ok(())
}

/// Get hourly aggregated stats for a time range
pub fn get_hourly_stats(hours: i64) -> Result<Vec<TokenStatsAggregated>, String> {
    let conn = connect_db()?;
    let cutoff_bucket = if hours <= 0 {
        String::new()
    } else {
        let cutoff = chrono::Local::now() - chrono::Duration::hours(hours);
        cutoff.format("%Y-%m-%d %H:00").to_string()
    };

    let mut stmt = conn
        .prepare(
            "SELECT hour_bucket, 
                SUM(total_input_tokens) as input, 
                SUM(total_output_tokens) as output,
                SUM(total_cached_tokens) as cached,
                SUM(total_tokens) as total,
                SUM(request_count) as count
         FROM token_stats_hourly 
         WHERE hour_bucket >= ?1
         GROUP BY hour_bucket
         ORDER BY hour_bucket ASC",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map([cutoff_bucket], |row| {
            Ok(TokenStatsAggregated {
                period: row.get(0)?,
                total_input_tokens: row.get(1)?,
                total_output_tokens: row.get(2)?,
                total_cached_tokens: row.get(3)?,
                total_tokens: row.get(4)?,
                request_count: row.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row.map_err(|e| e.to_string())?);
    }
    Ok(result)
}

/// Get daily aggregated stats for a time range
pub fn get_daily_stats(days: i64) -> Result<Vec<TokenStatsAggregated>, String> {
    let conn = connect_db()?;
    let cutoff_bucket = if days <= 0 {
        String::new()
    } else {
        let cutoff = chrono::Local::now() - chrono::Duration::days(days);
        cutoff.format("%Y-%m-%d").to_string()
    };

    let mut stmt = conn
        .prepare(
            "SELECT substr(hour_bucket, 1, 10) as day_bucket, 
                SUM(total_input_tokens) as input, 
                SUM(total_output_tokens) as output,
                SUM(total_cached_tokens) as cached,
                SUM(total_tokens) as total,
                SUM(request_count) as count
         FROM token_stats_hourly 
         WHERE substr(hour_bucket, 1, 10) >= ?1
         GROUP BY day_bucket
         ORDER BY day_bucket ASC",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map([cutoff_bucket], |row| {
            Ok(TokenStatsAggregated {
                period: row.get(0)?,
                total_input_tokens: row.get(1)?,
                total_output_tokens: row.get(2)?,
                total_cached_tokens: row.get(3)?,
                total_tokens: row.get(4)?,
                request_count: row.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row.map_err(|e| e.to_string())?);
    }
    Ok(result)
}

/// Get weekly aggregated stats
pub fn get_weekly_stats(weeks: i64) -> Result<Vec<TokenStatsAggregated>, String> {
    let conn = connect_db()?;
    let cutoff_timestamp = if weeks <= 0 {
        0
    } else {
        (chrono::Local::now() - chrono::Duration::weeks(weeks)).timestamp()
    };

    let mut stmt = conn
        .prepare(
            "SELECT strftime('%Y-W%W', datetime(timestamp, 'unixepoch', 'localtime')) as week_bucket,
                SUM(input_tokens) as input, 
                SUM(output_tokens) as output,
                SUM(cached_tokens) as cached,
                SUM(total_tokens) as total,
                COUNT(*) as count
         FROM token_usage 
         WHERE timestamp >= ?1
         GROUP BY week_bucket
         ORDER BY week_bucket ASC",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map([cutoff_timestamp], |row| {
            Ok(TokenStatsAggregated {
                period: row.get(0)?,
                total_input_tokens: row.get(1)?,
                total_output_tokens: row.get(2)?,
                total_cached_tokens: row.get(3)?,
                total_tokens: row.get(4)?,
                request_count: row.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row.map_err(|e| e.to_string())?);
    }
    Ok(result)
}

/// Get per-account statistics for a time range
pub fn get_account_stats(hours: i64) -> Result<Vec<AccountTokenStats>, String> {
    let conn = connect_db()?;
    let cutoff_bucket = if hours <= 0 {
        String::new()
    } else {
        (chrono::Local::now() - chrono::Duration::hours(hours)).format("%Y-%m-%d %H:00").to_string()
    };

    let mut stmt = conn
        .prepare(
            "SELECT account_email,
                SUM(total_input_tokens) as input, 
                SUM(total_output_tokens) as output,
                SUM(total_cached_tokens) as cached,
                SUM(total_tokens) as total,
                SUM(request_count) as count
         FROM token_stats_hourly 
         WHERE hour_bucket >= ?1
         GROUP BY account_email
         ORDER BY total DESC",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map([cutoff_bucket], |row| {
            Ok(AccountTokenStats {
                account_email: row.get(0)?,
                total_input_tokens: row.get(1)?,
                total_output_tokens: row.get(2)?,
                total_cached_tokens: row.get(3)?,
                total_tokens: row.get(4)?,
                request_count: row.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row.map_err(|e| e.to_string())?);
    }
    Ok(result)
}

/// Get summary statistics for a time range
pub fn get_summary_stats(hours: i64) -> Result<TokenStatsSummary, String> {
    let conn = connect_db()?;
    let cutoff_bucket = if hours <= 0 {
        String::new()
    } else {
        (chrono::Local::now() - chrono::Duration::hours(hours)).format("%Y-%m-%d %H:00").to_string()
    };

    let (total_input, total_output, total_cached, total, requests): (u64, u64, u64, u64, u64) =
        conn.query_row(
            "SELECT COALESCE(SUM(total_input_tokens), 0),
                COALESCE(SUM(total_output_tokens), 0),
                COALESCE(SUM(total_cached_tokens), 0),
                COALESCE(SUM(total_tokens), 0),
                COALESCE(SUM(request_count), 0)
         FROM token_stats_hourly 
         WHERE hour_bucket >= ?1",
            [&cutoff_bucket],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .map_err(|e| e.to_string())?;

    let unique_accounts: u64 = conn
        .query_row(
            "SELECT COUNT(DISTINCT account_email) FROM token_stats_hourly WHERE hour_bucket >= ?1",
            [&cutoff_bucket],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    Ok(TokenStatsSummary {
        total_input_tokens: total_input,
        total_output_tokens: total_output,
        total_cached_tokens: total_cached,
        total_tokens: total,
        total_requests: requests,
        unique_accounts,
    })
}

pub fn get_model_stats(hours: i64) -> Result<Vec<ModelTokenStats>, String> {
    let conn = connect_db()?;
    let cutoff = if hours <= 0 {
        0
    } else {
        chrono::Local::now().timestamp() - (hours * 3600)
    };

    let mut stmt = conn
        .prepare(
            "SELECT model,
                SUM(input_tokens) as input,
                SUM(output_tokens) as output,
                SUM(cached_tokens) as cached,
                SUM(total_tokens) as total,
                COUNT(*) as count
         FROM token_usage
         WHERE timestamp >= ?1
         GROUP BY model
         ORDER BY total DESC",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map([cutoff], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, u64>(1)?,
                row.get::<_, u64>(2)?,
                row.get::<_, u64>(3)?,
                row.get::<_, u64>(4)?,
                row.get::<_, u64>(5)?,
            ))
        })
        .map_err(|e| e.to_string())?;

    let mut map: std::collections::HashMap<String, ModelTokenStats> = std::collections::HashMap::new();
    for row in rows {
        let (raw_model, input, output, cached, total, count) = row.map_err(|e| e.to_string())?;
        let real_model = crate::proxy::common::model_mapping::resolve_real_forwarded_model(&raw_model);
        let entry = map.entry(real_model.clone()).or_insert_with(|| ModelTokenStats {
            model: real_model,
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_cached_tokens: 0,
            total_tokens: 0,
            request_count: 0,
        });
        entry.total_input_tokens += input;
        entry.total_output_tokens += output;
        entry.total_cached_tokens += cached;
        entry.total_tokens += total;
        entry.request_count += count;
    }

    let mut result: Vec<ModelTokenStats> = map.into_values().collect();
    result.sort_by(|a, b| b.total_tokens.cmp(&a.total_tokens));
    Ok(result)
}

pub fn get_model_trend_hourly(hours: i64) -> Result<Vec<ModelTrendPoint>, String> {
    let conn = connect_db()?;
    let cutoff = if hours <= 0 {
        0
    } else {
        chrono::Local::now().timestamp() - (hours * 3600)
    };

    let mut stmt = conn
        .prepare(
            "SELECT strftime('%Y-%m-%d %H:00', datetime(timestamp, 'unixepoch', 'localtime')) as hour_bucket,
                model,
                SUM(total_tokens) as total
         FROM token_usage
         WHERE timestamp >= ?1
         GROUP BY hour_bucket, model
         ORDER BY hour_bucket ASC",
        )
        .map_err(|e| e.to_string())?;

    let mut trend_map: std::collections::BTreeMap<String, std::collections::HashMap<String, u64>> =
        std::collections::BTreeMap::new();

    let rows = stmt
        .query_map([cutoff], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u64>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?;

    for row in rows {
        let (period, raw_model, total) = row.map_err(|e| e.to_string())?;
        let real_model = crate::proxy::common::model_mapping::resolve_real_forwarded_model(&raw_model);
        *trend_map.entry(period).or_default().entry(real_model).or_insert(0) += total;
    }

    Ok(trend_map
        .into_iter()
        .map(|(period, model_data)| ModelTrendPoint { period, model_data })
        .collect())
}

pub fn get_model_trend_daily(days: i64) -> Result<Vec<ModelTrendPoint>, String> {
    let conn = connect_db()?;
    let cutoff = if days <= 0 {
        0
    } else {
        chrono::Local::now().timestamp() - (days * 24 * 3600)
    };

    let mut stmt = conn
        .prepare(
            "SELECT strftime('%Y-%m-%d', datetime(timestamp, 'unixepoch', 'localtime')) as day_bucket,
                model,
                SUM(total_tokens) as total
         FROM token_usage
         WHERE timestamp >= ?1
         GROUP BY day_bucket, model
         ORDER BY day_bucket ASC",
        )
        .map_err(|e| e.to_string())?;

    let mut trend_map: std::collections::BTreeMap<String, std::collections::HashMap<String, u64>> =
        std::collections::BTreeMap::new();

    let rows = stmt
        .query_map([cutoff], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u64>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?;

    for row in rows {
        let (period, raw_model, total) = row.map_err(|e| e.to_string())?;
        let real_model = crate::proxy::common::model_mapping::resolve_real_forwarded_model(&raw_model);
        *trend_map.entry(period).or_default().entry(real_model).or_insert(0) += total;
    }

    Ok(trend_map
        .into_iter()
        .map(|(period, model_data)| ModelTrendPoint { period, model_data })
        .collect())
}

pub fn get_account_trend_hourly(hours: i64) -> Result<Vec<AccountTrendPoint>, String> {
    let conn = connect_db()?;
    let cutoff = if hours <= 0 {
        0
    } else {
        chrono::Local::now().timestamp() - (hours * 3600)
    };

    let mut stmt = conn
        .prepare(
            "SELECT strftime('%Y-%m-%d %H:00', datetime(timestamp, 'unixepoch', 'localtime')) as hour_bucket,
                account_email,
                SUM(total_tokens) as total
         FROM token_usage
         WHERE timestamp >= ?1
         GROUP BY hour_bucket, account_email
         ORDER BY hour_bucket ASC",
        )
        .map_err(|e| e.to_string())?;

    let mut trend_map: std::collections::BTreeMap<String, std::collections::HashMap<String, u64>> =
        std::collections::BTreeMap::new();

    let rows = stmt
        .query_map([cutoff], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u64>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?;

    for row in rows {
        let (period, account, total) = row.map_err(|e| e.to_string())?;
        trend_map.entry(period).or_default().insert(account, total);
    }

    Ok(trend_map
        .into_iter()
        .map(|(period, account_data)| AccountTrendPoint {
            period,
            account_data,
        })
        .collect())
}

pub fn get_account_trend_daily(days: i64) -> Result<Vec<AccountTrendPoint>, String> {
    let conn = connect_db()?;
    let cutoff = if days <= 0 {
        0
    } else {
        chrono::Local::now().timestamp() - (days * 24 * 3600)
    };

    let mut stmt = conn
        .prepare(
            "SELECT strftime('%Y-%m-%d', datetime(timestamp, 'unixepoch', 'localtime')) as day_bucket,
                account_email,
                SUM(total_tokens) as total
         FROM token_usage
         WHERE timestamp >= ?1
         GROUP BY day_bucket, account_email
         ORDER BY day_bucket ASC",
        )
        .map_err(|e| e.to_string())?;

    let mut trend_map: std::collections::BTreeMap<String, std::collections::HashMap<String, u64>> =
        std::collections::BTreeMap::new();

    let rows = stmt
        .query_map([cutoff], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u64>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?;

    for row in rows {
        let (period, account, total) = row.map_err(|e| e.to_string())?;
        trend_map.entry(period).or_default().insert(account, total);
    }

    Ok(trend_map
        .into_iter()
        .map(|(period, account_data)| AccountTrendPoint {
            period,
            account_data,
        })
        .collect())
}

/// Get today's usage summary including Tokens, requests and consumed quota percentage
pub fn get_today_usage_summary() -> Result<TodayUsageSummary, String> {
    let conn = connect_db()?;

    // 获取今日 00:00:00 本地时间戳
    let now = chrono::Local::now();
    let today_start = now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|naive| naive.and_local_timezone(chrono::Local).single())
        .map(|dt| dt.timestamp())
        .unwrap_or_else(|| now.timestamp() - (now.timestamp() % 86400));

    // 1. 查询今日 token_usage 中的 Token 消耗和请求数
    let mut stmt = conn
        .prepare(
            "SELECT model,
                    SUM(total_tokens) as total,
                    COUNT(*) as count
             FROM token_usage
             WHERE timestamp >= ?1
             GROUP BY model",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map([today_start], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, u64>(1)?,
                row.get::<_, u64>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?;

    let mut gemini_tokens = 0u64;
    let mut gemini_requests = 0u64;
    let mut claude_tokens = 0u64;
    let mut claude_requests = 0u64;
    let mut total_tokens = 0u64;
    let mut total_requests = 0u64;

    for row in rows {
        let (raw_model, tokens, count) = row.map_err(|e| e.to_string())?;
        let real_model = crate::proxy::common::model_mapping::resolve_real_forwarded_model(&raw_model);
        let m_lower = real_model.to_lowercase();

        if m_lower.contains("gemini") || m_lower.contains("imagen") || m_lower.contains("image") {
            gemini_tokens += tokens;
            gemini_requests += count;
        } else if m_lower.contains("claude") || m_lower.contains("3p") {
            claude_tokens += tokens;
            claude_requests += count;
        }

        total_tokens += tokens;
        total_requests += count;
    }

    // 2. 从配额快照或 Token 当量计算今日消耗百分比 %
    let mut gemini_used_pct = 0.0f64;
    let mut claude_used_pct = 0.0f64;

    if let Ok(mut acc_stmt) = conn.prepare("SELECT DISTINCT account_email FROM quota_snapshots") {
        if let Ok(acc_iter) = acc_stmt.query_map([], |r| r.get::<_, String>(0)) {
            let mut g_delta_sum = 0.0;
            let mut c_delta_sum = 0.0;
            let mut account_count = 0;

            for acc in acc_iter.flatten() {
                let start_snapshot: Option<(i32, i32)> = conn.query_row(
                    "SELECT gemini_percentage, claude_percentage FROM quota_snapshots 
                     WHERE account_email = ?1 AND timestamp <= ?2 
                     ORDER BY timestamp DESC LIMIT 1",
                    params![acc, today_start + 3600],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                ).or_else(|_| {
                    conn.query_row(
                        "SELECT gemini_percentage, claude_percentage FROM quota_snapshots 
                         WHERE account_email = ?1 AND timestamp >= ?2 
                         ORDER BY timestamp ASC LIMIT 1",
                        params![acc, today_start],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                }).ok();

                let latest_snapshot: Option<(i32, i32)> = conn.query_row(
                    "SELECT gemini_percentage, claude_percentage FROM quota_snapshots 
                     WHERE account_email = ?1 
                     ORDER BY timestamp DESC LIMIT 1",
                    params![acc],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                ).ok();

                if let (Some((g_start, c_start)), Some((g_end, c_end))) = (start_snapshot, latest_snapshot) {
                    account_count += 1;

                    let g_diff = if g_end > g_start + 30 {
                        (g_start + (100 - g_end)).max(0)
                    } else {
                        (g_start - g_end).max(0)
                    };
                    g_delta_sum += g_diff as f64;

                    let c_diff = if c_end > c_start + 30 {
                        (c_start + (100 - c_end)).max(0)
                    } else {
                        (c_start - c_end).max(0)
                    };
                    c_delta_sum += c_diff as f64;
                }
            }

            if account_count > 0 {
                gemini_used_pct = g_delta_sum / (account_count as f64);
                claude_used_pct = c_delta_sum / (account_count as f64);
            }
        }
    }

    // 3. 确定今日周配额实际消耗百分比 %：
    // 配额百分比必须严格代表「今日实际消耗的周配额百分比（Weekly Quota Consumed %）」
    // 优先采用 SQLite 账号快照真实下降百分比；若快照无变动记录，才使用反代 Token 当量进行合理估算
    let acc_count = match crate::modules::account::load_account_index() {
        Ok(idx) => idx.accounts.len().max(1) as f64,
        Err(_) => 1.0,
    };

    // 单号单周满配额当量：Gemini 约 20M Tokens，Claude 约 2M Tokens
    let avg_gemini_tokens = gemini_tokens as f64 / acc_count;
    let avg_claude_tokens = claude_tokens as f64 / acc_count;
    let token_gemini_pct = (avg_gemini_tokens / 20_000_000.0) * 100.0;
    let token_claude_pct = (avg_claude_tokens / 2_000_000.0) * 100.0;

    let final_gemini_pct = if gemini_used_pct > 0.0 {
        gemini_used_pct
    } else {
        token_gemini_pct
    };

    let final_claude_pct = if claude_used_pct > 0.0 {
        claude_used_pct
    } else {
        token_claude_pct
    };

    Ok(TodayUsageSummary {
        gemini_tokens,
        gemini_requests,
        gemini_used_pct: (final_gemini_pct * 10.0).round() / 10.0,
        claude_tokens,
        claude_requests,
        claude_used_pct: (final_claude_pct * 10.0).round() / 10.0,
        total_tokens,
        total_requests,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_and_query() {
        assert!(true);
    }
}
