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
        "token_stats_hourly",
        "total_cached_tokens INTEGER NOT NULL DEFAULT 0",
    )?;

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

    // Insert into raw usage table
    conn.execute(
        "INSERT INTO token_usage (timestamp, account_email, model, input_tokens, output_tokens, cached_tokens, total_tokens)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![timestamp, account_email, model, input_tokens, output_tokens, cached_tokens, total_tokens],
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

/// Get hourly aggregated stats for a time range
pub fn get_hourly_stats(hours: i64) -> Result<Vec<TokenStatsAggregated>, String> {
    let conn = connect_db()?;
    let cutoff = chrono::Local::now() - chrono::Duration::hours(hours);
    let cutoff_bucket = cutoff.format("%Y-%m-%d %H:00").to_string();

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
    let cutoff = chrono::Local::now() - chrono::Duration::days(days);
    let cutoff_bucket = cutoff.format("%Y-%m-%d").to_string();

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
    let cutoff = chrono::Local::now() - chrono::Duration::weeks(weeks);
    let cutoff_timestamp = cutoff.timestamp();

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
    let cutoff = chrono::Local::now() - chrono::Duration::hours(hours);
    let cutoff_bucket = cutoff.format("%Y-%m-%d %H:00").to_string();

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
    let cutoff = chrono::Local::now() - chrono::Duration::hours(hours);
    let cutoff_bucket = cutoff.format("%Y-%m-%d %H:00").to_string();

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
    let cutoff = chrono::Local::now().timestamp() - (hours * 3600);

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
            Ok(ModelTokenStats {
                model: row.get(0)?,
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

pub fn get_model_trend_hourly(hours: i64) -> Result<Vec<ModelTrendPoint>, String> {
    let conn = connect_db()?;
    let cutoff = chrono::Local::now().timestamp() - (hours * 3600);

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
        let (period, model, total) = row.map_err(|e| e.to_string())?;
        trend_map.entry(period).or_default().insert(model, total);
    }

    Ok(trend_map
        .into_iter()
        .map(|(period, model_data)| ModelTrendPoint { period, model_data })
        .collect())
}

pub fn get_model_trend_daily(days: i64) -> Result<Vec<ModelTrendPoint>, String> {
    let conn = connect_db()?;
    let cutoff = chrono::Local::now().timestamp() - (days * 24 * 3600);

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
        let (period, model, total) = row.map_err(|e| e.to_string())?;
        trend_map.entry(period).or_default().insert(model, total);
    }

    Ok(trend_map
        .into_iter()
        .map(|(period, model_data)| ModelTrendPoint { period, model_data })
        .collect())
}

pub fn get_account_trend_hourly(hours: i64) -> Result<Vec<AccountTrendPoint>, String> {
    let conn = connect_db()?;
    let cutoff = chrono::Local::now().timestamp() - (hours * 3600);

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
    let cutoff = chrono::Local::now().timestamp() - (days * 24 * 3600);

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
            "models": [], "last_updated": observed,
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
        // This would need a test database setup
        // For now, just verify the module compiles
        assert!(true);
    }
}
