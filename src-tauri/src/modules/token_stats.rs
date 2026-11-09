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
    #[serde(default)]
    pub total_reasoning_tokens: u64,
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
    #[serde(default)]
    pub total_reasoning_tokens: u64,
    pub total_tokens: u64,
    pub request_count: u64,
}

/// Summary statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenStatsSummary {
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cached_tokens: u64,
    #[serde(default)]
    pub total_reasoning_tokens: u64,
    pub total_tokens: u64,
    pub total_requests: u64,
    pub unique_accounts: u64,
    #[serde(default)]
    pub avg_ttft_ms: Option<f64>,
}

/// Per-model token statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelTokenStats {
    pub model: String,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cached_tokens: u64,
    #[serde(default)]
    pub total_reasoning_tokens: u64,
    pub total_tokens: u64,
    pub request_count: u64,
    #[serde(default)]
    pub avg_ttft_ms: Option<f64>,
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

    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_token_account_timestamp ON token_usage (account_email, timestamp DESC)",
        [],
    );

    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_token_model ON token_usage (model)",
        [],
    );

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

    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_hourly_account ON token_stats_hourly (account_email, hour_bucket)",
        [],
    );

    add_column_if_missing(
        &conn,
        "token_usage",
        "cached_tokens INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column_if_missing(
        &conn,
        "token_usage",
        "reasoning_tokens INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column_if_missing(&conn, "token_usage", "requested_model TEXT")?;
    add_column_if_missing(&conn, "token_usage", "ttft_ms REAL")?;
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_token_requested_model ON token_usage (requested_model)",
        [],
    );
    add_column_if_missing(
        &conn,
        "token_stats_hourly",
        "total_cached_tokens INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column_if_missing(
        &conn,
        "token_stats_hourly",
        "total_reasoning_tokens INTEGER NOT NULL DEFAULT 0",
    )?;

    // 历史数据平滑迁移：针对旧版本未记录 requested_model 的记录，依据默认路由规则反推并回填实际路由模型
    migrate_legacy_unmapped_models(&conn);

    Ok(())
}

/// 历史数据兼容迁移：针对 requested_model IS NULL 的历史记录，将其原先记录的模型保留至 requested_model，
/// 并通过网关统一路由解析器计算其真实生效的物理/路由模型填入 model 字段。
fn migrate_legacy_unmapped_models(conn: &Connection) {
    let unmapped_models: Vec<String> = {
        let mut stmt = match conn
            .prepare("SELECT DISTINCT model FROM token_usage WHERE requested_model IS NULL")
        {
            Ok(s) => s,
            Err(_) => return,
        };
        let rows = match stmt.query_map([], |row| row.get::<_, String>(0)) {
            Ok(r) => r,
            Err(_) => return,
        };
        rows.filter_map(|r| r.ok()).collect()
    };

    if unmapped_models.is_empty() {
        return;
    }

    let default_custom = crate::proxy::config::default_custom_mapping();
    for original_model in unmapped_models {
        let routed_model = crate::proxy::common::model_mapping::resolve_model_route(
            &original_model,
            &default_custom,
        );

        // 如果原始模型与路由模型不同，说明历史记录存的是客户端原始名，需要迁移成：
        // model = routed_model (实际路由模型), requested_model = original_model (原始请求模型)
        // 如果相同，直接将 requested_model 置为 original_model
        let _ = conn.execute(
            "UPDATE token_usage SET model = ?1, requested_model = ?2 WHERE model = ?2 AND requested_model IS NULL",
            params![routed_model, original_model],
        );
    }
}

/// Record token usage from a request
pub fn record_usage(
    account_email: &str,
    model: &str,
    requested_model: Option<&str>,
    input_tokens: u32,
    output_tokens: u32,
    cached_tokens: u32,
    reasoning_tokens: u32,
    ttft_ms: Option<f64>,
) -> Result<(), String> {
    let conn = connect_db()?;
    let timestamp = chrono::Local::now().timestamp();
    let total_tokens = input_tokens + output_tokens;

    // Insert into raw usage table
    conn.execute(
        "INSERT INTO token_usage (timestamp, account_email, model, requested_model, input_tokens, output_tokens, cached_tokens, reasoning_tokens, total_tokens, ttft_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![timestamp, account_email, model, requested_model, input_tokens, output_tokens, cached_tokens, reasoning_tokens, total_tokens, ttft_ms],
    ).map_err(|e| e.to_string())?;

    let hour_bucket = chrono::Local::now().format("%Y-%m-%d %H:00").to_string();
    conn.execute(
        "INSERT INTO token_stats_hourly (hour_bucket, account_email, total_input_tokens, total_output_tokens, total_cached_tokens, total_reasoning_tokens, total_tokens, request_count)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1)
         ON CONFLICT(hour_bucket, account_email) DO UPDATE SET
            total_input_tokens = total_input_tokens + ?3,
            total_output_tokens = total_output_tokens + ?4,
            total_cached_tokens = total_cached_tokens + ?5,
            total_reasoning_tokens = total_reasoning_tokens + ?6,
            total_tokens = total_tokens + ?7,
            request_count = request_count + 1",
        params![hour_bucket, account_email, input_tokens, output_tokens, cached_tokens, reasoning_tokens, total_tokens],
    ).map_err(|e| e.to_string())?;

    Ok(())
}

/// Attach current weekly usage without loading raw history or persisting derived totals.
pub(crate) fn populate_weekly_usage(accounts: &mut [crate::models::Account]) -> Result<(), String> {
    if !accounts.iter().any(|account| {
        account
            .quota
            .as_ref()
            .is_some_and(|q| q.quota_groups.is_some())
    }) {
        return Ok(());
    }
    let conn = connect_db()?;
    populate_weekly_usage_with_conn(&conn, accounts, chrono::Utc::now().timestamp())
}

fn populate_weekly_usage_with_conn(
    conn: &Connection,
    accounts: &mut [crate::models::Account],
    now: i64,
) -> Result<(), String> {
    let mut stmt = conn
        .prepare(
            "SELECT COALESCE(SUM(input_tokens + output_tokens), 0) FROM token_usage
         WHERE account_email = ?1 AND timestamp >= ?2 AND timestamp < ?3 AND timestamp <= ?4
         AND ((?5 = 0 AND model LIKE 'gemini%')
           OR (?5 = 1 AND (model LIKE 'claude%' OR model LIKE 'gpt%')))",
        )
        .map_err(|e| e.to_string())?;
    for account in accounts {
        let Some(groups) = account
            .quota
            .as_mut()
            .and_then(|quota| quota.quota_groups.as_mut())
        else {
            continue;
        };
        for group in groups {
            let name = group.display_name.to_lowercase();
            for bucket in &mut group.buckets {
                bucket.cycle_tokens = None;
                let Some((start, end)) = bucket.weekly_cycle_bounds(now) else {
                    continue;
                };
                let id = bucket.bucket_id.to_lowercase();
                let third_party =
                    name.contains("claude") || name.contains("gpt") || id.contains("3p");
                if !third_party && !name.contains("gemini") && !id.contains("gemini") {
                    continue;
                }
                bucket.cycle_tokens = Some(
                    stmt.query_row(
                        params![account.email, start, end, now, third_party],
                        |row| row.get(0),
                    )
                    .map_err(|e| e.to_string())?,
                );
            }
        }
    }
    Ok(())
}

/// Get minute-level aggregated stats for a time range
pub fn get_minute_stats(
    minutes: i64,
    step_minutes: i64,
) -> Result<Vec<TokenStatsAggregated>, String> {
    let conn = connect_db()?;
    let step_secs = step_minutes.max(1) * 60;
    let cutoff = if minutes <= 0 {
        0
    } else {
        chrono::Local::now().timestamp() - (minutes * 60)
    };

    let mut stmt = conn
        .prepare(
            "SELECT strftime('%Y-%m-%d %H:%M', datetime(timestamp - (timestamp % ?2), 'unixepoch', 'localtime')) as minute_bucket, 
                SUM(input_tokens) as input, 
                SUM(output_tokens) as output,
                SUM(cached_tokens) as cached,
                SUM(COALESCE(reasoning_tokens, 0)) as reasoning,
                SUM(total_tokens) as total,
                COUNT(*) as count
         FROM token_usage 
         WHERE timestamp >= ?1
         GROUP BY minute_bucket
         ORDER BY minute_bucket ASC",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![cutoff, step_secs], |row| {
            Ok(TokenStatsAggregated {
                period: row.get(0)?,
                total_input_tokens: row.get(1)?,
                total_output_tokens: row.get(2)?,
                total_cached_tokens: row.get(3)?,
                total_reasoning_tokens: row.get(4)?,
                total_tokens: row.get(5)?,
                request_count: row.get(6)?,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row.map_err(|e| e.to_string())?);
    }
    Ok(result)
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
                SUM(total_reasoning_tokens) as reasoning,
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
                total_reasoning_tokens: row.get(4)?,
                total_tokens: row.get(5)?,
                request_count: row.get(6)?,
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
                SUM(total_reasoning_tokens) as reasoning,
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
                total_reasoning_tokens: row.get(4)?,
                total_tokens: row.get(5)?,
                request_count: row.get(6)?,
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
        let cutoff = chrono::Local::now() - chrono::Duration::weeks(weeks);
        cutoff.timestamp()
    };

    let mut stmt = conn
        .prepare(
            "SELECT strftime('%Y-W%W', datetime(timestamp, 'unixepoch', 'localtime')) as week_bucket,
                SUM(input_tokens) as input, 
                SUM(output_tokens) as output,
                SUM(cached_tokens) as cached,
                SUM(reasoning_tokens) as reasoning,
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
                total_reasoning_tokens: row.get(4)?,
                total_tokens: row.get(5)?,
                request_count: row.get(6)?,
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
        let cutoff = chrono::Local::now() - chrono::Duration::hours(hours);
        cutoff.format("%Y-%m-%d %H:00").to_string()
    };

    let mut stmt = conn
        .prepare(
            "SELECT account_email,
                SUM(total_input_tokens) as input, 
                SUM(total_output_tokens) as output,
                SUM(total_cached_tokens) as cached,
                SUM(total_reasoning_tokens) as reasoning,
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
                total_reasoning_tokens: row.get(4)?,
                total_tokens: row.get(5)?,
                request_count: row.get(6)?,
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
        let cutoff = chrono::Local::now() - chrono::Duration::hours(hours);
        cutoff.format("%Y-%m-%d %H:00").to_string()
    };

    let (total_input, total_output, total_cached, total_reasoning, total, requests): (
        u64,
        u64,
        u64,
        u64,
        u64,
        u64,
    ) = conn
        .query_row(
            "SELECT COALESCE(SUM(total_input_tokens), 0),
                COALESCE(SUM(total_output_tokens), 0),
                COALESCE(SUM(total_cached_tokens), 0),
                COALESCE(SUM(total_reasoning_tokens), 0),
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
                    row.get(5)?,
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

    let cutoff_timestamp = if hours <= 0 {
        0
    } else {
        chrono::Local::now().timestamp() - (hours * 3600)
    };

    let avg_ttft_ms: Option<f64> = conn
        .query_row(
            "SELECT ROUND(AVG(ttft_ms), 1) FROM token_usage WHERE timestamp >= ?1 AND ttft_ms IS NOT NULL AND ttft_ms > 0",
            [cutoff_timestamp],
            |row| row.get(0),
        )
        .unwrap_or(None);

    Ok(TokenStatsSummary {
        total_input_tokens: total_input,
        total_output_tokens: total_output,
        total_cached_tokens: total_cached,
        total_reasoning_tokens: total_reasoning,
        total_tokens: total,
        total_requests: requests,
        unique_accounts,
        avg_ttft_ms,
    })
}

fn model_column_expr(view_mode: Option<&str>) -> &'static str {
    match view_mode {
        Some("requested") => "COALESCE(NULLIF(requested_model, ''), model)",
        _ => "model",
    }
}

pub fn get_model_stats(
    hours: i64,
    view_mode: Option<&str>,
) -> Result<Vec<ModelTokenStats>, String> {
    let conn = connect_db()?;
    let cutoff = if hours <= 0 {
        0
    } else {
        chrono::Local::now().timestamp() - (hours * 3600)
    };

    let model_expr = model_column_expr(view_mode);
    let sql = format!(
        "SELECT {col} as agg_model,
                SUM(input_tokens) as input,
                SUM(output_tokens) as output,
                SUM(cached_tokens) as cached,
                SUM(reasoning_tokens) as reasoning,
                SUM(total_tokens) as total,
                COUNT(*) as count,
                ROUND(AVG(CASE WHEN ttft_ms > 0 THEN ttft_ms ELSE NULL END), 1) as avg_ttft
         FROM token_usage
         WHERE timestamp >= ?1
         GROUP BY agg_model
         ORDER BY total DESC",
        col = model_expr
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map([cutoff], |row| {
            Ok(ModelTokenStats {
                model: row.get(0)?,
                total_input_tokens: row.get(1)?,
                total_output_tokens: row.get(2)?,
                total_cached_tokens: row.get(3)?,
                total_reasoning_tokens: row.get(4)?,
                total_tokens: row.get(5)?,
                request_count: row.get(6)?,
                avg_ttft_ms: row.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row.map_err(|e| e.to_string())?);
    }
    Ok(result)
}

pub fn get_model_trend_minute(
    minutes: i64,
    step_minutes: i64,
    view_mode: Option<&str>,
) -> Result<Vec<ModelTrendPoint>, String> {
    let conn = connect_db()?;
    let step_secs = step_minutes.max(1) * 60;
    let cutoff = if minutes <= 0 {
        0
    } else {
        chrono::Local::now().timestamp() - (minutes * 60)
    };

    let model_expr = model_column_expr(view_mode);
    let sql = format!(
        "SELECT strftime('%Y-%m-%d %H:%M', datetime(timestamp - (timestamp % ?2), 'unixepoch', 'localtime')) as minute_bucket,
                {col} as agg_model,
                SUM(total_tokens) as total
         FROM token_usage
         WHERE timestamp >= ?1
         GROUP BY minute_bucket, agg_model
         ORDER BY minute_bucket ASC",
        col = model_expr
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

    let mut trend_map: std::collections::BTreeMap<String, std::collections::HashMap<String, u64>> =
        std::collections::BTreeMap::new();

    let rows = stmt
        .query_map(params![cutoff, step_secs], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u64>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?;

    for row in rows {
        let (period, model, total) = row.map_err(|e| e.to_string())?;
        trend_map.entry(period).or_default().insert(model, total);
    }

    Ok(trend_map
        .into_iter()
        .map(|(period, model_data)| ModelTrendPoint { period, model_data })
        .collect())
}

pub fn get_model_trend_hourly(
    hours: i64,
    view_mode: Option<&str>,
) -> Result<Vec<ModelTrendPoint>, String> {
    let conn = connect_db()?;
    let cutoff = if hours <= 0 {
        0
    } else {
        chrono::Local::now().timestamp() - (hours * 3600)
    };

    let model_expr = model_column_expr(view_mode);
    let sql = format!(
        "SELECT strftime('%Y-%m-%d %H:00', datetime(timestamp, 'unixepoch', 'localtime')) as hour_bucket,
                {col} as agg_model,
                SUM(total_tokens) as total
         FROM token_usage
         WHERE timestamp >= ?1
         GROUP BY hour_bucket, agg_model
         ORDER BY hour_bucket ASC",
        col = model_expr
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

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
        let (period, model, total) = row.map_err(|e| e.to_string())?;
        trend_map.entry(period).or_default().insert(model, total);
    }

    Ok(trend_map
        .into_iter()
        .map(|(period, model_data)| ModelTrendPoint { period, model_data })
        .collect())
}

pub fn get_model_trend_daily(
    days: i64,
    view_mode: Option<&str>,
) -> Result<Vec<ModelTrendPoint>, String> {
    let conn = connect_db()?;
    let cutoff = if days <= 0 {
        0
    } else {
        chrono::Local::now().timestamp() - (days * 24 * 3600)
    };

    let model_expr = model_column_expr(view_mode);
    let sql = format!(
        "SELECT strftime('%Y-%m-%d', datetime(timestamp, 'unixepoch', 'localtime')) as day_bucket,
                {col} as agg_model,
                SUM(total_tokens) as total
         FROM token_usage
         WHERE timestamp >= ?1
         GROUP BY day_bucket, agg_model
         ORDER BY day_bucket ASC",
        col = model_expr
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

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
        let (period, model, total) = row.map_err(|e| e.to_string())?;
        trend_map.entry(period).or_default().insert(model, total);
    }

    Ok(trend_map
        .into_iter()
        .map(|(period, model_data)| ModelTrendPoint { period, model_data })
        .collect())
}

pub fn get_account_trend_minute(
    minutes: i64,
    step_minutes: i64,
) -> Result<Vec<AccountTrendPoint>, String> {
    let conn = connect_db()?;
    let step_secs = step_minutes.max(1) * 60;
    let cutoff = if minutes <= 0 {
        0
    } else {
        chrono::Local::now().timestamp() - (minutes * 60)
    };

    let mut stmt = conn
        .prepare(
            "SELECT strftime('%Y-%m-%d %H:%M', datetime(timestamp - (timestamp % ?2), 'unixepoch', 'localtime')) as minute_bucket,
                account_email,
                SUM(total_tokens) as total
         FROM token_usage
         WHERE timestamp >= ?1
         GROUP BY minute_bucket, account_email
         ORDER BY minute_bucket ASC",
        )
        .map_err(|e| e.to_string())?;

    let mut trend_map: std::collections::BTreeMap<String, std::collections::HashMap<String, u64>> =
        std::collections::BTreeMap::new();

    let rows = stmt
        .query_map(params![cutoff, step_secs], |row| {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn weekly_snapshot(observed: i64, reset: i64, fraction: f64) -> crate::models::QuotaData {
        let iso = |seconds| {
            chrono::DateTime::from_timestamp(seconds, 0)
                .unwrap()
                .to_rfc3339()
        };
        serde_json::from_value(serde_json::json!({
            "models": [
                {"name": "claude-sonnet-4-6", "percentage": 100, "reset_time": iso(reset)},
                {"name": "gemini-pro", "percentage": 100, "reset_time": iso(reset)}
            ],
            "last_updated": observed,
            "quota_groups": [
                {"display_name": "Gemini Models", "buckets": [
                    {"bucket_id": "gemini-weekly", "window": "weekly", "remaining_fraction": fraction,
                     "reset_time": iso(reset), "observed_at": observed * 1000},
                    {"bucket_id": "gemini-5h", "window": "5h", "remaining_fraction": 1,
                     "reset_time": iso(reset), "observed_at": observed * 1000}
                ]},
                {"display_name": "Claude and GPT models", "buckets": [
                    {"bucket_id": "3p-weekly", "window": "weekly", "remaining_fraction": 0.5,
                     "reset_time": iso(reset + 100), "observed_at": observed * 1000}
                ]}
            ]
        })).unwrap()
    }

    #[test]
    fn weekly_usage_tracks_accounts_groups_and_official_reset_cycles() {
        let now = 1_700_000_000;
        let reset = now + 100;
        let start = reset - 7 * 86400;
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE token_usage (timestamp INTEGER, account_email TEXT,
            model TEXT, input_tokens INTEGER, output_tokens INTEGER, cached_tokens INTEGER);",
        )
        .unwrap();
        for (email, model, timestamp, input, output) in [
            ("a", "gemini-pro", start - 1, 900, 0),
            ("a", "gemini-pro", start, 11, 2),
            ("a", "claude-sonnet", start, 900, 0),
            ("a", "claude-sonnet", start + 100, 20, 0),
            ("a", "gpt-oss", now, 30, 0),
            ("a", "custom-alias", now, 900, 0),
            ("b", "gemini-pro", start - 51, 900, 0),
            ("b", "gemini-pro", start - 50, 7, 3),
            ("b", "gemini-pro", now, 900, 0),
            ("a", "gemini-pro", reset, 40, 0),
        ] {
            conn.execute(
                "INSERT INTO token_usage VALUES (?1, ?2, ?3, ?4, ?5, 99)",
                params![timestamp, email, model, input, output],
            )
            .unwrap();
        }
        let mut accounts: Vec<_> = [("a", reset), ("b", reset - 50)]
            .into_iter()
            .map(|(email, account_reset)| {
                let token = crate::models::TokenData::new(
                    String::new(),
                    String::new(),
                    0,
                    None,
                    None,
                    None,
                    false,
                    None,
                );
                let mut account = crate::models::Account::new(email.into(), email.into(), token);
                account.update_quota(weekly_snapshot(now, account_reset, 0.0));
                account
            })
            .collect();
        let buckets = |account: &crate::models::Account| {
            account
                .quota
                .as_ref()
                .unwrap()
                .quota_groups
                .as_ref()
                .unwrap()
                .iter()
                .flat_map(|group| group.buckets.iter().map(|bucket| bucket.cycle_tokens))
                .collect::<Vec<_>>()
        };
        populate_weekly_usage_with_conn(&conn, &mut accounts, now).unwrap();
        assert_eq!(buckets(&accounts[0]), vec![Some(13), None, Some(50)]);
        assert_eq!(buckets(&accounts[1]), vec![Some(910), None, Some(0)]);

        // Legacy persisted boundaries are ignored and omitted on the next serialization.
        let mut legacy = serde_json::to_value(&accounts[0]).unwrap();
        legacy["quota"]["quota_groups"][0]["buckets"][0]["cycle_start"] = now.into();
        accounts[0] = serde_json::from_value(legacy).unwrap();
        assert!(
            serde_json::to_value(&accounts[0]).unwrap()["quota"]["quota_groups"][0]["buckets"][0]
                .get("cycle_start")
                .is_none()
        );
        populate_weekly_usage_with_conn(&conn, &mut accounts, now).unwrap();
        assert_eq!(buckets(&accounts[0]), vec![Some(13), None, Some(50)]);

        // Small and large replenishments, refreshes and reloads keep the official interval.
        for (offset, fraction) in [(1, 0.000001), (2, 1.0), (3, 0.9)] {
            accounts[0].update_quota(weekly_snapshot(now + offset, reset, fraction));
            accounts[0] =
                serde_json::from_value(serde_json::to_value(&accounts[0]).unwrap()).unwrap();
            populate_weekly_usage_with_conn(&conn, &mut accounts, now + offset).unwrap();
            assert_eq!(buckets(&accounts[0]), vec![Some(13), None, Some(50)]);
            assert_eq!(buckets(&accounts[1]), vec![Some(910), None, Some(0)]);
        }
        // An older positive observation cannot move the boundary.
        accounts[0].update_quota(weekly_snapshot(now, reset + 10, 1.0));
        populate_weekly_usage_with_conn(&conn, &mut accounts, now + 3).unwrap();
        assert_eq!(buckets(&accounts[0]), vec![Some(13), None, Some(50)]);

        // A new official reset switches cycles and includes its exact new start.
        accounts[0].update_quota(weekly_snapshot(reset, reset + 7 * 86400, 1.0));
        populate_weekly_usage_with_conn(&conn, &mut accounts, reset).unwrap();
        assert_eq!(buckets(&accounts[0]), vec![Some(40), None, None]);
        let first = &accounts[0]
            .quota
            .as_ref()
            .unwrap()
            .quota_groups
            .as_ref()
            .unwrap()[0]
            .buckets[0];
        assert_eq!(
            first.weekly_cycle_bounds(reset),
            Some((reset, reset + 7 * 86400))
        );

        // Invalid and expired periods are unavailable, never fabricated zero totals.
        accounts[0]
            .quota
            .as_mut()
            .unwrap()
            .quota_groups
            .as_mut()
            .unwrap()[0]
            .buckets[0]
            .reset_time = "invalid".into();
        populate_weekly_usage_with_conn(&conn, &mut accounts, reset + 101).unwrap();
        assert_eq!(buckets(&accounts[0]), vec![None, None, Some(0)]);
        assert_eq!(buckets(&accounts[1]), vec![None, None, None]);
    }

    #[test]
    fn test_record_and_query() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE token_usage (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp INTEGER NOT NULL,
                account_email TEXT NOT NULL,
                model TEXT NOT NULL,
                requested_model TEXT,
                input_tokens INTEGER NOT NULL DEFAULT 0,
                output_tokens INTEGER NOT NULL DEFAULT 0,
                cached_tokens INTEGER NOT NULL DEFAULT 0,
                reasoning_tokens INTEGER NOT NULL DEFAULT 0,
                total_tokens INTEGER NOT NULL DEFAULT 0
            );",
        )
        .unwrap();

        // 模拟：请求 claude-3-7-sonnet，实际路由到了 fable-5
        conn.execute(
            "INSERT INTO token_usage (timestamp, account_email, model, requested_model, input_tokens, output_tokens, cached_tokens, reasoning_tokens, total_tokens)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![1_700_000_000, "user@example.com", "fable-5", "claude-3-7-sonnet", 100, 200, 0, 0, 300],
        ).unwrap();

        // 验证按 routed 聚合（默认）
        let expr_routed = model_column_expr(None);
        let sql_routed = format!("SELECT {expr_routed} as agg_model, SUM(total_tokens) FROM token_usage GROUP BY agg_model");
        let (m_routed, total_routed): (String, i64) = conn
            .query_row(&sql_routed, [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!(m_routed, "fable-5");
        assert_eq!(total_routed, 300);

        // 验证按 requested 聚合
        let expr_requested = model_column_expr(Some("requested"));
        let sql_requested = format!("SELECT {expr_requested} as agg_model, SUM(total_tokens) FROM token_usage GROUP BY agg_model");
        let (m_req, total_req): (String, i64) = conn
            .query_row(&sql_requested, [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!(m_req, "claude-3-7-sonnet");
        assert_eq!(total_req, 300);

        // 验证历史旧数据迁移逻辑 (针对 requested_model 为 NULL 的老数据)
        conn.execute(
            "INSERT INTO token_usage (timestamp, account_email, model, requested_model, input_tokens, output_tokens, cached_tokens, reasoning_tokens, total_tokens)
             VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?6, ?7, ?8)",
            params![1_700_000_100, "user@example.com", "claude-haiku-4-5-20251001", 50, 50, 0, 0, 100],
        ).unwrap();

        migrate_legacy_unmapped_models(&conn);

        // 迁移后，根据 default_custom_mapping，claude-haiku-4-5-20251001 实际路由为 gemini-3.8-flash-low，而 requested_model 为原始请求名
        let (m_migrated, req_migrated): (String, Option<String>) = conn
            .query_row(
                "SELECT model, requested_model FROM token_usage WHERE timestamp = 1700000100",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();

        assert_eq!(m_migrated, "gemini-3.8-flash-low");
        assert_eq!(req_migrated.as_deref(), Some("claude-haiku-4-5-20251001"));
    }
}
