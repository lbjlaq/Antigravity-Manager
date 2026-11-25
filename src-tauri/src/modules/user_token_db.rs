//! User Token Database Module
//! UserToken 数据库操作模块

#![allow(dead_code)]
// 用户令牌存储，部分接口留作后续扩展

use chrono::{FixedOffset, Timelike, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// 用户令牌结构体
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserToken {
    pub id: String,
    pub token: String,
    pub username: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub expires_type: String, // "day", "week", "month", "never"
    pub expires_at: Option<i64>,
    pub max_ips: i32,                 // 0 = unlimited
    pub curfew_start: Option<String>, // "HH:MM" 宵禁开始时间
    pub curfew_end: Option<String>,   // "HH:MM" 宵禁结束时间
    pub allowed_account_ids: Option<Vec<String>>,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_used_at: Option<i64>,
    pub total_requests: i64,
    pub total_tokens_used: i64,
}

/// 令牌 IP 绑定结构体
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenIpBinding {
    pub id: String,
    pub token_id: String,
    pub ip_address: String,
    pub first_seen_at: i64,
    pub last_seen_at: i64,
    pub request_count: i64,
    pub user_agent: Option<String>,
}

/// 令牌使用日志结构体
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsageLog {
    pub id: String,
    pub token_id: String,
    pub ip_address: String,
    pub model: String,
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub request_time: i64,
    pub status: u16,
}

/// 获取数据库路径
pub fn get_db_path() -> Result<PathBuf, String> {
    let mut path = crate::modules::account::get_data_dir()?;
    path.push("user_tokens.db");
    Ok(path)
}

/// 连接数据库
pub fn connect_db() -> Result<Connection, String> {
    let path = get_db_path()?;
    let conn = Connection::open(&path).map_err(|e| format!("Failed to open database: {}", e))?;
    Ok(conn)
}

/// 初始化数据库
pub fn init_db() -> Result<(), String> {
    let conn = connect_db()?;
    init_db_with_conn(&conn)
}

/// 在指定连接上初始化数据库（便于测试迁移逻辑）
fn init_db_with_conn(conn: &Connection) -> Result<(), String> {
    // 创建 user_tokens 表
    conn.execute(
        "CREATE TABLE IF NOT EXISTS user_tokens (
            id TEXT PRIMARY KEY,
            token TEXT UNIQUE NOT NULL,
            username TEXT NOT NULL,
            description TEXT,
            enabled BOOLEAN NOT NULL DEFAULT 1,
            expires_type TEXT NOT NULL,
            expires_at INTEGER,
            max_ips INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            last_used_at INTEGER,
            total_requests INTEGER NOT NULL DEFAULT 0,
            total_tokens_used INTEGER NOT NULL DEFAULT 0,
            curfew_start TEXT,
            curfew_end TEXT,
            allowed_account_ids TEXT
        )",
        [],
    )
    .map_err(|e| format!("Failed to create user_tokens table: {}", e))?;

    // 尝试添加新列 (用于旧数据库迁移，忽略已存在的错误)
    let _ = conn.execute("ALTER TABLE user_tokens ADD COLUMN expires_type TEXT", []);
    let _ = conn.execute("ALTER TABLE user_tokens ADD COLUMN expires_at INTEGER", []);
    let _ = conn.execute(
        "ALTER TABLE user_tokens ADD COLUMN max_ips INTEGER DEFAULT 0",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE user_tokens ADD COLUMN total_requests INTEGER DEFAULT 0",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE user_tokens ADD COLUMN total_tokens_used INTEGER DEFAULT 0",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE user_tokens ADD COLUMN last_used_at INTEGER",
        [],
    );
    let _ = conn.execute("ALTER TABLE user_tokens ADD COLUMN curfew_start TEXT", []);
    let _ = conn.execute("ALTER TABLE user_tokens ADD COLUMN curfew_end TEXT", []);
    let _ = conn.execute(
        "ALTER TABLE user_tokens ADD COLUMN allowed_account_ids TEXT",
        [],
    );

    // 创建 token_ip_bindings 表
    conn.execute(
        "CREATE TABLE IF NOT EXISTS token_ip_bindings (
            id TEXT PRIMARY KEY,
            token_id TEXT NOT NULL,
            ip_address TEXT NOT NULL,
            first_seen_at INTEGER NOT NULL,
            last_seen_at INTEGER NOT NULL,
            request_count INTEGER NOT NULL DEFAULT 0,
            user_agent TEXT,
            FOREIGN KEY(token_id) REFERENCES user_tokens(id) ON DELETE CASCADE,
            UNIQUE(token_id, ip_address)
        )",
        [],
    )
    .map_err(|e| format!("Failed to create token_ip_bindings table: {}", e))?;

    // 创建 token_usage_logs 表
    conn.execute(
        "CREATE TABLE IF NOT EXISTS token_usage_logs (
            id TEXT PRIMARY KEY,
            token_id TEXT NOT NULL,
            ip_address TEXT,
            model TEXT,
            input_tokens INTEGER,
            output_tokens INTEGER,
            request_time INTEGER NOT NULL,
            status INTEGER,
            FOREIGN KEY(token_id) REFERENCES user_tokens(id) ON DELETE CASCADE
        )",
        [],
    )
    .map_err(|e| format!("Failed to create token_usage_logs table: {}", e))?;

    // 创建索引
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_token_usage_logs_token_id ON token_usage_logs(token_id)",
        [],
    );
    let _ = conn.execute("CREATE INDEX IF NOT EXISTS idx_token_usage_logs_request_time ON token_usage_logs(request_time)", []);
    let _ = conn.execute("CREATE INDEX IF NOT EXISTS idx_token_usage_logs_token_time ON token_usage_logs(token_id, request_time DESC)", []);

    // [FIX Issue #1719] 数据清洗：修复旧版本升级导致的 NULL 字段
    // 这些字段在旧版本中可能不存在，ALTER TABLE 添加后默认为 NULL，导致反序列化失败
    let _ = conn.execute("UPDATE user_tokens SET expires_type = 'never' WHERE expires_type IS NULL OR expires_type = ''", []);
    let _ = conn.execute(
        "UPDATE user_tokens SET max_ips = 0 WHERE max_ips IS NULL",
        [],
    );
    let _ = conn.execute(
        "UPDATE user_tokens SET total_requests = 0 WHERE total_requests IS NULL",
        [],
    );
    let _ = conn.execute(
        "UPDATE user_tokens SET total_tokens_used = 0 WHERE total_tokens_used IS NULL",
        [],
    );
    let _ = conn.execute(
        "UPDATE user_tokens SET enabled = 1 WHERE enabled IS NULL",
        [],
    );

    Ok(())
}

/// 规范化允许的账号 ID 列表：去除首尾空白、过滤空项、保序去重；空列表归约为 None（不限制）
pub fn normalize_allowed_account_ids(ids: Option<Vec<String>>) -> Option<Vec<String>> {
    let ids = ids?;
    let mut seen = std::collections::HashSet::new();
    let mut normalized = Vec::new();
    for id in ids {
        let trimmed = id.trim();
        if !trimmed.is_empty() && seen.insert(trimmed.to_string()) {
            normalized.push(trimmed.to_string());
        }
    }
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

/// 从数据库 TEXT 字段解析允许的账号 ID 列表（fail-closed：畸形 JSON 视为空限制而非无限制）
pub fn parse_allowed_account_ids(raw: Option<String>) -> Option<Vec<String>> {
    let raw = raw?;
    if raw.trim().is_empty() {
        return None;
    }
    match serde_json::from_str::<Vec<String>>(&raw) {
        Ok(ids) => Some(ids),
        Err(e) => {
            tracing::error!(
                "Failed to parse allowed_account_ids JSON from DB (fail-closed to empty list): {}",
                e
            );
            Some(Vec::new())
        }
    }
}

fn serialize_allowed_account_ids(ids: &Option<Vec<String>>) -> Result<Option<String>, String> {
    ids.as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| format!("Failed to serialize allowed_account_ids: {}", e))
}

/// 创建新令牌
pub fn create_token(
    username: String,
    expires_type: String,
    description: Option<String>,
    max_ips: i32,
    curfew_start: Option<String>,
    curfew_end: Option<String>,
    custom_expires_at: Option<i64>, // 自定义过期时间戳 (秒)
    allowed_account_ids: Option<Vec<String>>,
) -> Result<UserToken, String> {
    let conn = connect_db()?;
    create_token_with_conn(
        &conn,
        username,
        expires_type,
        description,
        max_ips,
        curfew_start,
        curfew_end,
        custom_expires_at,
        allowed_account_ids,
    )
}

#[allow(clippy::too_many_arguments)]
fn create_token_with_conn(
    conn: &Connection,
    username: String,
    expires_type: String,
    description: Option<String>,
    max_ips: i32,
    curfew_start: Option<String>,
    curfew_end: Option<String>,
    custom_expires_at: Option<i64>,
    allowed_account_ids: Option<Vec<String>>,
) -> Result<UserToken, String> {
    let id = Uuid::new_v4().to_string();
    let token = format!("sk-{}", Uuid::new_v4().to_string().replace("-", ""));
    let now = Utc::now().timestamp();
    let normalized_allowed_accounts = normalize_allowed_account_ids(allowed_account_ids);
    let serialized_allowed_accounts = serialize_allowed_account_ids(&normalized_allowed_accounts)?;

    let expires_at = match expires_type.as_str() {
        "day" => Some(
            Utc::now()
                .checked_add_signed(chrono::Duration::days(1))
                .unwrap()
                .timestamp(),
        ),
        "week" => Some(
            Utc::now()
                .checked_add_signed(chrono::Duration::weeks(1))
                .unwrap()
                .timestamp(),
        ),
        "month" => Some(
            Utc::now()
                .checked_add_signed(chrono::Duration::days(30))
                .unwrap()
                .timestamp(),
        ),
        "custom" => custom_expires_at, // 使用自定义时间戳
        _ => None,                     // "never" or other
    };

    let user_token = UserToken {
        id: id.clone(),
        token: token.clone(),
        username: username.clone(),
        description: description.clone(),
        enabled: true,
        expires_type: expires_type.clone(),
        expires_at,
        max_ips,
        curfew_start: curfew_start.clone(),
        curfew_end: curfew_end.clone(),
        allowed_account_ids: normalized_allowed_accounts,
        created_at: now,
        updated_at: now,
        last_used_at: None,
        total_requests: 0,
        total_tokens_used: 0,
    };

    conn.execute(
        "INSERT INTO user_tokens (
            id, token, username, description, enabled, expires_type, expires_at, max_ips,
            curfew_start, curfew_end, allowed_account_ids,
            created_at, updated_at, total_requests, total_tokens_used
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            user_token.id,
            user_token.token,
            user_token.username,
            user_token.description,
            user_token.enabled,
            user_token.expires_type,
            user_token.expires_at,
            user_token.max_ips,
            user_token.curfew_start,
            user_token.curfew_end,
            serialized_allowed_accounts,
            user_token.created_at,
            user_token.updated_at,
            user_token.total_requests,
            user_token.total_tokens_used,
        ],
    )
    .map_err(|e| format!("Failed to insert user token: {}", e))?;

    Ok(user_token)
}

/// 列出所有令牌
pub fn list_tokens() -> Result<Vec<UserToken>, String> {
    let conn = connect_db()?;
    list_tokens_with_conn(&conn)
}

fn list_tokens_with_conn(conn: &Connection) -> Result<Vec<UserToken>, String> {
    let mut stmt = conn
        .prepare("SELECT * FROM user_tokens ORDER BY created_at DESC")
        .map_err(|e| format!("Failed to prepare query: {}", e))?;

    let token_iter = stmt
        .query_map([], |row| {
            Ok(UserToken {
                id: row.get("id")?,
                token: row.get("token")?,
                username: row.get("username")?,
                description: row.get("description")?,
                enabled: row.get("enabled").unwrap_or(true), // 防御性默认值
                expires_type: row.get("expires_type").unwrap_or("never".to_string()), // 防御性默认值
                expires_at: row.get("expires_at").unwrap_or(None),
                max_ips: row.get("max_ips").unwrap_or(0),
                curfew_start: row.get("curfew_start").unwrap_or(None),
                curfew_end: row.get("curfew_end").unwrap_or(None),
                allowed_account_ids: parse_allowed_account_ids(
                    row.get("allowed_account_ids").unwrap_or(None),
                ),
                created_at: row.get("created_at")?,
                updated_at: row.get("updated_at")?,
                last_used_at: row.get("last_used_at").unwrap_or(None),
                total_requests: row.get("total_requests").unwrap_or(0),
                total_tokens_used: row.get("total_tokens_used").unwrap_or(0),
            })
        })
        .map_err(|e| format!("Failed to query tokens: {}", e))?;

    let mut tokens = Vec::new();
    for token in token_iter {
        tokens.push(token.map_err(|e| format!("Failed to parse token row: {}", e))?);
    }

    Ok(tokens)
}

/// 获取单个令牌信息
pub fn get_token_by_id(id: &str) -> Result<Option<UserToken>, String> {
    let conn = connect_db()?;
    get_token_by_id_with_conn(&conn, id)
}

fn get_token_by_id_with_conn(conn: &Connection, id: &str) -> Result<Option<UserToken>, String> {
    let mut stmt = conn
        .prepare("SELECT * FROM user_tokens WHERE id = ?1")
        .map_err(|e| format!("Failed to prepare query: {}", e))?;

    let token = stmt
        .query_row(params![id], |row| {
            Ok(UserToken {
                id: row.get("id")?,
                token: row.get("token")?,
                username: row.get("username")?,
                description: row.get("description")?,
                enabled: row.get("enabled")?,
                expires_type: row.get("expires_type")?,
                expires_at: row.get("expires_at")?,
                max_ips: row.get("max_ips")?,
                curfew_start: row.get("curfew_start").unwrap_or(None),
                curfew_end: row.get("curfew_end").unwrap_or(None),
                allowed_account_ids: parse_allowed_account_ids(
                    row.get("allowed_account_ids").unwrap_or(None),
                ),
                created_at: row.get("created_at")?,
                updated_at: row.get("updated_at")?,
                last_used_at: row.get("last_used_at")?,
                total_requests: row.get("total_requests")?,
                total_tokens_used: row.get("total_tokens_used")?,
            })
        })
        .optional()
        .map_err(|e| format!("Failed to query token: {}", e))?;

    Ok(token)
}

/// 根据 Token 值获取令牌信息
pub fn get_token_by_value(token: &str) -> Result<Option<UserToken>, String> {
    let conn = connect_db()?;
    get_token_by_value_with_conn(&conn, token)
}

fn get_token_by_value_with_conn(
    conn: &Connection,
    token: &str,
) -> Result<Option<UserToken>, String> {
    let mut stmt = conn
        .prepare("SELECT * FROM user_tokens WHERE token = ?1")
        .map_err(|e| format!("Failed to prepare query: {}", e))?;

    let token = stmt
        .query_row(params![token], |row| {
            Ok(UserToken {
                id: row.get("id")?,
                token: row.get("token")?,
                username: row.get("username")?,
                description: row.get("description")?,
                enabled: row.get("enabled")?,
                expires_type: row.get("expires_type")?,
                expires_at: row.get("expires_at")?,
                max_ips: row.get("max_ips")?,
                curfew_start: row.get("curfew_start").unwrap_or(None),
                curfew_end: row.get("curfew_end").unwrap_or(None),
                allowed_account_ids: parse_allowed_account_ids(
                    row.get("allowed_account_ids").unwrap_or(None),
                ),
                created_at: row.get("created_at")?,
                updated_at: row.get("updated_at")?,
                last_used_at: row.get("last_used_at")?,
                total_requests: row.get("total_requests")?,
                total_tokens_used: row.get("total_tokens_used")?,
            })
        })
        .optional()
        .map_err(|e| format!("Failed to query token: {}", e))?;

    Ok(token)
}

/// 更新令牌状态/备注等
pub fn update_token(
    id: &str,
    username: Option<String>,
    description: Option<String>,
    enabled: Option<bool>,
    max_ips: Option<i32>,
    curfew_start: Option<Option<String>>,
    curfew_end: Option<Option<String>>,
    allowed_account_ids: Option<Option<Vec<String>>>,
) -> Result<(), String> {
    let conn = connect_db()?;
    update_token_with_conn(
        &conn,
        id,
        username,
        description,
        enabled,
        max_ips,
        curfew_start,
        curfew_end,
        allowed_account_ids,
    )
}

#[allow(clippy::too_many_arguments)]
fn update_token_with_conn(
    conn: &Connection,
    id: &str,
    username: Option<String>,
    description: Option<String>,
    enabled: Option<bool>,
    max_ips: Option<i32>,
    curfew_start: Option<Option<String>>,
    curfew_end: Option<Option<String>>,
    allowed_account_ids: Option<Option<Vec<String>>>,
) -> Result<(), String> {
    let now = Utc::now().timestamp();

    let mut query = "UPDATE user_tokens SET updated_at = ?1".to_string();
    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(now)];
    let mut param_idx = 2;

    if let Some(user) = username {
        query.push_str(&format!(", username = ?{}", param_idx));
        params_vec.push(Box::new(user));
        param_idx += 1;
    }

    if let Some(desc) = description {
        query.push_str(&format!(", description = ?{}", param_idx));
        params_vec.push(Box::new(desc));
        param_idx += 1;
    }

    if let Some(en) = enabled {
        query.push_str(&format!(", enabled = ?{}", param_idx));
        params_vec.push(Box::new(en));
        param_idx += 1;
    }

    if let Some(ips) = max_ips {
        query.push_str(&format!(", max_ips = ?{}", param_idx));
        params_vec.push(Box::new(ips));
        param_idx += 1;
    }

    if let Some(start) = curfew_start {
        query.push_str(&format!(", curfew_start = ?{}", param_idx));
        params_vec.push(Box::new(start));
        param_idx += 1;
    }

    if let Some(end) = curfew_end {
        query.push_str(&format!(", curfew_end = ?{}", param_idx));
        params_vec.push(Box::new(end));
        param_idx += 1;
    }

    if let Some(maybe_allowed) = allowed_account_ids {
        let normalized = normalize_allowed_account_ids(maybe_allowed);
        let serialized = serialize_allowed_account_ids(&normalized)?;
        query.push_str(&format!(", allowed_account_ids = ?{}", param_idx));
        params_vec.push(Box::new(serialized));
        param_idx += 1;
    }

    query.push_str(&format!(" WHERE id = ?{}", param_idx));
    params_vec.push(Box::new(id.to_string()));

    // 将 Vec<Box<dyn ToSql>> 转换为 &[&dyn ToSql]
    let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();

    conn.execute(&query, params_refs.as_slice())
        .map_err(|e| format!("Failed to update user token: {}", e))?;

    Ok(())
}

/// 续期令牌
pub fn renew_token(id: &str, expires_type: &str) -> Result<(), String> {
    let conn = connect_db()?;
    let now = Utc::now().timestamp();

    let expires_at = match expires_type {
        "day" => Some(
            Utc::now()
                .checked_add_signed(chrono::Duration::days(1))
                .unwrap()
                .timestamp(),
        ),
        "week" => Some(
            Utc::now()
                .checked_add_signed(chrono::Duration::weeks(1))
                .unwrap()
                .timestamp(),
        ),
        "month" => Some(
            Utc::now()
                .checked_add_signed(chrono::Duration::days(30))
                .unwrap()
                .timestamp(),
        ),
        _ => None, // "never" or other
    };

    conn.execute(
        "UPDATE user_tokens SET expires_type = ?1, expires_at = ?2, updated_at = ?3, enabled = 1 WHERE id = ?4",
        params![expires_type, expires_at, now, id],
    ).map_err(|e| format!("Failed to renew token: {}", e))?;

    Ok(())
}

/// 删除令牌
pub fn delete_token(id: &str) -> Result<(), String> {
    let conn = connect_db()?;
    conn.execute("DELETE FROM user_tokens WHERE id = ?1", params![id])
        .map_err(|e| format!("Failed to delete token: {}", e))?;
    Ok(())
}

/// 获取令牌的所有 IP 绑定
pub fn get_token_ips(token_id: &str) -> Result<Vec<TokenIpBinding>, String> {
    let conn = connect_db()?;
    let mut stmt = conn
        .prepare("SELECT * FROM token_ip_bindings WHERE token_id = ?1 ORDER BY last_seen_at DESC")
        .map_err(|e| format!("Failed to prepare query: {}", e))?;

    let iter = stmt
        .query_map(params![token_id], |row| {
            Ok(TokenIpBinding {
                id: row.get("id")?,
                token_id: row.get("token_id")?,
                ip_address: row.get("ip_address")?,
                first_seen_at: row.get("first_seen_at")?,
                last_seen_at: row.get("last_seen_at")?,
                request_count: row.get("request_count")?,
                user_agent: row.get("user_agent")?,
            })
        })
        .map_err(|e| format!("Failed to query token IPs: {}", e))?;

    let mut bindings = Vec::new();
    for b in iter {
        bindings.push(b.map_err(|e| format!("Failed to parse binding row: {}", e))?);
    }

    Ok(bindings)
}

/// 记录/更新令牌使用情况 (同时处理 user_tokens 和 token_ip_bindings)
pub fn record_token_usage_and_ip(
    token_id: &str,
    ip: &str,
    model: &str,
    input_tokens: i32,
    output_tokens: i32,
    status: u16,
    user_agent: Option<String>,
) -> Result<(), String> {
    let mut conn = connect_db()?;
    let tx = conn
        .transaction()
        .map_err(|e| format!("Failed to create transaction: {}", e))?;
    let now = Utc::now().timestamp();

    // 1. 更新 user_tokens 主表
    tx.execute(
        "UPDATE user_tokens SET 
            last_used_at = ?1, 
            total_requests = total_requests + 1, 
            total_tokens_used = total_tokens_used + ?2 
        WHERE id = ?3",
        params![now, input_tokens + output_tokens, token_id],
    )
    .map_err(|e| format!("Failed to update user_tokens stats: {}", e))?;

    // 2. 更新或插入 token_ip_bindings 表
    let binding_exists: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM token_ip_bindings WHERE token_id = ?1 AND ip_address = ?2)",
        params![token_id, ip],
        |row| row.get(0),
    ).unwrap_or(false);

    if binding_exists {
        tx.execute(
            "UPDATE token_ip_bindings SET 
                last_seen_at = ?1, 
                request_count = request_count + 1,
                user_agent = COALESCE(?2, user_agent)
            WHERE token_id = ?3 AND ip_address = ?4",
            params![now, user_agent, token_id, ip],
        )
        .map_err(|e| format!("Failed to update ip binding: {}", e))?;
    } else {
        let binding_id = Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO token_ip_bindings (
                id, token_id, ip_address, first_seen_at, last_seen_at, request_count, user_agent
            ) VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6)",
            params![binding_id, token_id, ip, now, now, user_agent],
        )
        .map_err(|e| format!("Failed to insert ip binding: {}", e))?;
    }

    // 3. 插入 token_usage_logs 表
    let log_id = Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO token_usage_logs (
            id, token_id, ip_address, model, input_tokens, output_tokens, request_time, status
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            log_id,
            token_id,
            ip,
            model,
            input_tokens,
            output_tokens,
            now,
            status
        ],
    )
    .map_err(|e| format!("Failed to insert usage log: {}", e))?;

    tx.commit()
        .map_err(|e| format!("Failed to commit transaction: {}", e))?;

    Ok(())
}

/// 检查 Token 是否有效 (包含过期时间检查和 IP 限制检查)
/// 返回: (是否有效, 拒绝原因)
pub fn validate_token(token_str: &str, ip: &str) -> Result<(bool, Option<String>), String> {
    let token_opt = get_token_by_value(token_str)?;

    if let Some(token) = token_opt {
        // 1. 检查过期时间
        if token.expires_type != "never" {
            if let Some(expires_at) = token.expires_at {
                if expires_at < Utc::now().timestamp() {
                    return Ok((
                        false,
                        Some(
                            "Your token has expired. Please contact the administrator to renew it."
                                .to_string(),
                        ),
                    ));
                }
            }
        }

        // 2. 检查 IP 限制
        if token.max_ips > 0 {
            let conn = connect_db()?;

            // 检查当前 IP 是否已绑定
            let is_bound: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM token_ip_bindings WHERE token_id = ?1 AND ip_address = ?2)",
                params![token.id, ip],
                |row| row.get(0)
            ).unwrap_or(false);

            if !is_bound {
                // 如果未绑定，检查是否达到上限
                let current_ip_count: i32 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM token_ip_bindings WHERE token_id = ?1",
                        params![token.id],
                        |row| row.get(0),
                    )
                    .unwrap_or(0);

                if current_ip_count >= token.max_ips {
                    return Ok((false, Some(format!("IP limit reached ({}/{}). Please contact the administrator to increase the limit.", current_ip_count, token.max_ips))));
                }
            }
        }

        // 3. 检查宵禁时间 (Curfew)
        // 逻辑：如果当前北京时间在 start 和 end 之间，则拒绝
        // 格式：HH:MM
        // 使用固定 UTC+8 (北京时间)，不依赖服务器本地时区
        if let (Some(start_str), Some(end_str)) = (&token.curfew_start, &token.curfew_end) {
            if !start_str.is_empty() && !end_str.is_empty() {
                let beijing_offset = FixedOffset::east_opt(8 * 3600).unwrap();
                let now_beijing = Utc::now().with_timezone(&beijing_offset);
                let current_time_str =
                    format!("{:02}:{:02}", now_beijing.hour(), now_beijing.minute());

                // 跨午夜处理: start > end (e.g. 23:00 to 06:00)
                // 正常: start < end (e.g. 09:00 to 18:00)
                let is_curfew = if start_str > end_str {
                    current_time_str >= *start_str || current_time_str < *end_str
                } else {
                    current_time_str >= *start_str && current_time_str < *end_str
                };

                if is_curfew {
                    return Ok((false, Some(format!("Service is not available between {} and {} Beijing Time (Curfew enabled). Current Beijing time: {}", start_str, end_str, current_time_str))));
                }
            }
        }

        // 一切正常，Token 有效
        Ok((true, None))
    } else {
        Ok((
            false,
            Some("Invalid token. Please check your API key.".to_string()),
        ))
    }
}

/// 获取 IP 关联的用户名 (用于 IP 管理页面)
/// 返回最近一次使用该 IP 的 Token 所属的用户名
pub fn get_username_for_ip(ip: &str) -> Result<Option<String>, String> {
    let conn = connect_db()?;
    let result: Option<String> = conn
        .query_row(
            "SELECT t.username
         FROM token_ip_bindings b 
         JOIN user_tokens t ON b.token_id = t.id 
         WHERE b.ip_address = ?1 
         ORDER BY b.last_seen_at DESC 
         LIMIT 1",
            params![ip],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| format!("Failed to query username by ip: {}", e))?;

    Ok(result)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserTokenDetailedSummary {
    pub total_requests: i64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_tokens: i64,
    pub success_requests: i64,
    pub failed_requests: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserTokenTrendPoint {
    pub timestamp_label: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
    pub requests: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserTokenModelStat {
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
    pub requests: i64,
    pub percentage: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserTokenDetailedStats {
    pub token: UserToken,
    pub summary: UserTokenDetailedSummary,
    pub trend: Vec<UserTokenTrendPoint>,
    pub models: Vec<UserTokenModelStat>,
    pub recent_ips: Vec<TokenIpBinding>,
}

/// 查询单个 User Token 的详细统计数据
pub fn get_token_detailed_stats(
    token_id: &str,
    time_range: &str,
) -> Result<UserTokenDetailedStats, String> {
    let token = get_token_by_id(token_id)?
        .ok_or_else(|| format!("User token with id {} not found", token_id))?;

    let conn = connect_db()?;
    let now = Utc::now().timestamp();

    let (start_time, time_format) = match time_range {
        "24h" => (now - 86400, "%m-%d %H:00"),
        "7d" => (now - 7 * 86400, "%m-%d"),
        "30d" => (now - 30 * 86400, "%m-%d"),
        _ => (0, "%Y-%m-%d"),
    };

    // 1. 查询概览指标
    let summary: UserTokenDetailedSummary = conn
        .query_row(
            "SELECT 
                COUNT(*),
                COALESCE(SUM(input_tokens), 0),
                COALESCE(SUM(output_tokens), 0),
                COALESCE(SUM(input_tokens + output_tokens), 0),
                COALESCE(SUM(CASE WHEN status >= 200 AND status < 400 THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN status >= 400 THEN 1 ELSE 0 END), 0)
            FROM token_usage_logs
            WHERE token_id = ?1 AND request_time >= ?2",
            params![token_id, start_time],
            |row| {
                Ok(UserTokenDetailedSummary {
                    total_requests: row.get(0)?,
                    total_input_tokens: row.get(1)?,
                    total_output_tokens: row.get(2)?,
                    total_tokens: row.get(3)?,
                    success_requests: row.get(4)?,
                    failed_requests: row.get(5)?,
                })
            },
        )
        .map_err(|e| format!("Failed to query token summary: {}", e))?;

    // 2. 查询时间走势
    let mut trend_stmt = conn
        .prepare(
            "SELECT 
                strftime(?1, datetime(request_time, 'unixepoch', 'localtime')) as time_group,
                COALESCE(SUM(input_tokens), 0),
                COALESCE(SUM(output_tokens), 0),
                COALESCE(SUM(input_tokens + output_tokens), 0),
                COUNT(*)
            FROM token_usage_logs
            WHERE token_id = ?2 AND request_time >= ?3
            GROUP BY time_group
            ORDER BY time_group ASC",
        )
        .map_err(|e| format!("Failed to prepare trend query: {}", e))?;

    let trend_rows = trend_stmt
        .query_map(params![time_format, token_id, start_time], |row| {
            Ok(UserTokenTrendPoint {
                timestamp_label: row.get(0)?,
                input_tokens: row.get(1)?,
                output_tokens: row.get(2)?,
                total_tokens: row.get(3)?,
                requests: row.get(4)?,
            })
        })
        .map_err(|e| format!("Failed to execute trend query: {}", e))?;

    let mut trend = Vec::new();
    for row in trend_rows {
        if let Ok(point) = row {
            trend.push(point);
        }
    }

    // 3. 查询模型分布
    let mut model_stmt = conn
        .prepare(
            "SELECT 
                COALESCE(NULLIF(model, ''), 'unknown') as model_name,
                COALESCE(SUM(input_tokens), 0),
                COALESCE(SUM(output_tokens), 0),
                COALESCE(SUM(input_tokens + output_tokens), 0) as total_toks,
                COUNT(*)
            FROM token_usage_logs
            WHERE token_id = ?1 AND request_time >= ?2
            GROUP BY model_name
            ORDER BY total_toks DESC",
        )
        .map_err(|e| format!("Failed to prepare model query: {}", e))?;

    let model_rows = model_stmt
        .query_map(params![token_id, start_time], |row| {
            let model: String = row.get(0)?;
            let input_tokens: i64 = row.get(1)?;
            let output_tokens: i64 = row.get(2)?;
            let total_tokens: i64 = row.get(3)?;
            let requests: i64 = row.get(4)?;
            Ok((model, input_tokens, output_tokens, total_tokens, requests))
        })
        .map_err(|e| format!("Failed to execute model query: {}", e))?;

    let mut models_raw = Vec::new();
    let mut grand_total_tokens: i64 = 0;
    for row in model_rows {
        if let Ok((model, input_tokens, output_tokens, total_tokens, requests)) = row {
            grand_total_tokens += total_tokens;
            models_raw.push((model, input_tokens, output_tokens, total_tokens, requests));
        }
    }

    let models = models_raw
        .into_iter()
        .map(
            |(model, input_tokens, output_tokens, total_tokens, requests)| {
                let percentage = if grand_total_tokens > 0 {
                    (total_tokens as f64 / grand_total_tokens as f64) * 100.0
                } else {
                    0.0
                };
                UserTokenModelStat {
                    model,
                    input_tokens,
                    output_tokens,
                    total_tokens,
                    requests,
                    percentage: (percentage * 10.0).round() / 10.0,
                }
            },
        )
        .collect();

    // 4. 获取前 5 个最近 IP 绑定
    let all_ips = get_token_ips(token_id).unwrap_or_default();
    let recent_ips = all_ips.into_iter().take(5).collect();

    Ok(UserTokenDetailedStats {
        token,
        summary,
        trend,
        models,
        recent_ips,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserTokenRankItem {
    pub id: String,
    pub token: String,
    pub username: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub expires_type: String,
    pub expires_at: Option<i64>,
    pub max_ips: i32,
    pub total_tokens: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub requests: i64,
    pub percentage: f64,
}

/// 查询用户 Token 的消耗排行榜
pub fn get_user_tokens_ranking(time_range: &str) -> Result<Vec<UserTokenRankItem>, String> {
    let conn = connect_db()?;
    let now = Utc::now().timestamp();

    let start_time = match time_range {
        "minute" => now - 3600,
        "hourly" | "24h" => now - 86400,
        "daily" | "7d" => now - 7 * 86400,
        "weekly" | "30d" => now - 30 * 86400,
        _ => 0,
    };

    let mut stmt = conn
        .prepare(
            "SELECT 
                t.id,
                t.token,
                t.username,
                t.description,
                t.enabled,
                t.expires_type,
                t.expires_at,
                t.max_ips,
                t.total_tokens_used,
                t.total_requests,
                COALESCE(SUM(l.input_tokens + l.output_tokens), 0) as period_tokens,
                COALESCE(SUM(l.input_tokens), 0) as period_in,
                COALESCE(SUM(l.output_tokens), 0) as period_out,
                COUNT(l.id) as period_requests
            FROM user_tokens t
            LEFT JOIN token_usage_logs l ON t.id = l.token_id AND l.request_time >= ?1
            GROUP BY t.id
            ORDER BY period_tokens DESC, t.total_tokens_used DESC",
        )
        .map_err(|e| format!("Failed to prepare ranking query: {}", e))?;

    let rows = stmt
        .query_map(params![start_time], |row| {
            let id: String = row.get(0)?;
            let token: String = row.get(1)?;
            let username: String = row.get(2)?;
            let description: Option<String> = row.get(3)?;
            let enabled: bool = row.get(4)?;
            let expires_type: String = row.get(5)?;
            let expires_at: Option<i64> = row.get(6)?;
            let max_ips: i32 = row.get(7)?;
            let legacy_tokens: i64 = row.get(8)?;
            let legacy_requests: i64 = row.get(9)?;
            let period_tokens: i64 = row.get(10)?;
            let period_in: i64 = row.get(11)?;
            let period_out: i64 = row.get(12)?;
            let period_requests: i64 = row.get(13)?;

            // 若选全部历史且日志尚未覆盖历史总额，回退到 legacy 累计值
            let (total_tokens, input_tokens, output_tokens, requests) =
                if start_time == 0 && period_tokens == 0 && legacy_tokens > 0 {
                    (legacy_tokens, legacy_tokens, 0, legacy_requests)
                } else {
                    (period_tokens, period_in, period_out, period_requests)
                };

            Ok(UserTokenRankItem {
                id,
                token,
                username,
                description,
                enabled,
                expires_type,
                expires_at,
                max_ips,
                total_tokens,
                input_tokens,
                output_tokens,
                requests,
                percentage: 0.0,
            })
        })
        .map_err(|e| format!("Failed to execute ranking query: {}", e))?;

    let mut list = Vec::new();
    let mut grand_total: i64 = 0;
    for r in rows {
        if let Ok(item) = r {
            grand_total += item.total_tokens;
            list.push(item);
        }
    }

    for item in &mut list {
        if grand_total > 0 {
            item.percentage =
                ((item.total_tokens as f64 / grand_total as f64) * 1000.0).round() / 10.0;
        }
    }

    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_query_token() {
        let _ = init_db(); // Ensure DB is initialized

        // Use a random username to avoid collisions in existing DB runs during dev
        let username = format!("TestUser_{}", Uuid::new_v4());
        let token_res = create_token(
            username.clone(),
            "day".to_string(),
            Some("Test token".to_string()),
            0,
            None,
            None,
            None,
            None,
        );
        assert!(token_res.is_ok());

        let token = token_res.unwrap();
        assert_eq!(token.username, username);
        assert!(token.token.starts_with("sk-"));

        let fetched = get_token_by_id(&token.id);
        assert!(fetched.is_ok());
        assert_eq!(fetched.unwrap().unwrap().username, username);
    }

    #[test]
    fn test_never_expire_token_validation() {
        let _ = init_db();
        let username = format!("NeverExpireUser_{}", Uuid::new_v4());
        let token_res = create_token(
            username.clone(),
            "never".to_string(),
            Some("Never expire test token".to_string()),
            0,
            None,
            None,
            None,
            None,
        );
        assert!(token_res.is_ok());
        let token = token_res.unwrap();
        let (valid, reason) =
            validate_token(&token.token, "127.0.0.1").expect("validation must succeed");
        assert!(
            valid,
            "Token with expires_type never must be valid, reason: {:?}",
            reason
        );
    }

    #[test]
    fn test_get_token_detailed_stats() {
        let _ = init_db();
        let username = format!("StatsUser_{}", Uuid::new_v4());
        let token_res = create_token(
            username.clone(),
            "month".to_string(),
            Some("Stats token".to_string()),
            0,
            None,
            None,
            None,
            None,
        );
        assert!(token_res.is_ok());
        let token = token_res.unwrap();

        // 记录一条使用用量
        let _ = record_token_usage_and_ip(
            &token.id,
            "127.0.0.1",
            "gemini-2.5-flash",
            100,
            200,
            200,
            Some("test-agent".to_string()),
        );

        let stats_res = get_token_detailed_stats(&token.id, "7d");
        assert!(stats_res.is_ok());
        let stats = stats_res.unwrap();
        assert_eq!(stats.summary.total_requests, 1);
        assert_eq!(stats.summary.total_input_tokens, 100);
        assert_eq!(stats.summary.total_output_tokens, 200);
        assert_eq!(stats.summary.total_tokens, 300);
        assert_eq!(stats.models.len(), 1);
        assert_eq!(stats.models[0].model, "gemini-2.5-flash");
        assert_eq!(stats.recent_ips.len(), 1);
        assert_eq!(stats.recent_ips[0].ip_address, "127.0.0.1");
    }

    #[test]
    fn test_allowed_account_ids_create_update_and_clear() {
        let conn = Connection::open_in_memory().unwrap();
        init_db_with_conn(&conn).unwrap();

        // 1. Create unlimited token
        let unlimited = create_token_with_conn(
            &conn,
            "unlimited_user".to_string(),
            "never".to_string(),
            None,
            0,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        assert_eq!(unlimited.allowed_account_ids, None);

        // 2. Create restricted token (with whitespace, empties, and duplicates)
        let restricted = create_token_with_conn(
            &conn,
            "restricted_user".to_string(),
            "never".to_string(),
            None,
            0,
            None,
            None,
            None,
            Some(vec![
                " acc-1 ".to_string(),
                "".to_string(),
                "acc-2".to_string(),
                "acc-1".to_string(),
                "   ".to_string(),
            ]),
        )
        .unwrap();
        assert_eq!(
            restricted.allowed_account_ids,
            Some(vec!["acc-1".to_string(), "acc-2".to_string()])
        );

        let fetched = get_token_by_value_with_conn(&conn, &restricted.token)
            .unwrap()
            .unwrap();
        assert_eq!(
            fetched.allowed_account_ids,
            Some(vec!["acc-1".to_string(), "acc-2".to_string()])
        );

        // 3. Update with None (omit) -> no change
        update_token_with_conn(
            &conn,
            &restricted.id,
            None,
            Some("updated desc".to_string()),
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let after_omit = get_token_by_id_with_conn(&conn, &restricted.id)
            .unwrap()
            .unwrap();
        assert_eq!(
            after_omit.allowed_account_ids,
            Some(vec!["acc-1".to_string(), "acc-2".to_string()])
        );

        // 4. Update with Some(Some(vec!["acc-3"])) -> replace
        update_token_with_conn(
            &conn,
            &restricted.id,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(Some(vec![" acc-3 ".to_string()])),
        )
        .unwrap();
        let after_replace = get_token_by_id_with_conn(&conn, &restricted.id)
            .unwrap()
            .unwrap();
        assert_eq!(
            after_replace.allowed_account_ids,
            Some(vec!["acc-3".to_string()])
        );

        // 5. Update with Some(None) -> clear restriction
        update_token_with_conn(
            &conn,
            &restricted.id,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(None),
        )
        .unwrap();
        let after_clear_null = get_token_by_id_with_conn(&conn, &restricted.id)
            .unwrap()
            .unwrap();
        assert_eq!(after_clear_null.allowed_account_ids, None);

        // 6. Set again, then clear via Some(Some(vec![]))
        update_token_with_conn(
            &conn,
            &restricted.id,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(Some(vec!["acc-4".to_string()])),
        )
        .unwrap();
        update_token_with_conn(
            &conn,
            &restricted.id,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(Some(vec![])),
        )
        .unwrap();
        let after_clear_empty = get_token_by_id_with_conn(&conn, &restricted.id)
            .unwrap()
            .unwrap();
        assert_eq!(after_clear_empty.allowed_account_ids, None);
    }

    #[test]
    fn test_legacy_db_migration_adds_allowed_account_ids_column() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute(
            "CREATE TABLE user_tokens (
                id TEXT PRIMARY KEY,
                token TEXT UNIQUE NOT NULL,
                username TEXT NOT NULL,
                description TEXT,
                enabled BOOLEAN NOT NULL DEFAULT 1,
                expires_type TEXT NOT NULL,
                expires_at INTEGER,
                max_ips INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                last_used_at INTEGER,
                total_requests INTEGER NOT NULL DEFAULT 0,
                total_tokens_used INTEGER NOT NULL DEFAULT 0,
                curfew_start TEXT,
                curfew_end TEXT
            )",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO user_tokens (id, token, username, expires_type, created_at, updated_at)
             VALUES ('legacy-1', 'sk-legacy1', 'legacy_user', 'never', 100, 100)",
            [],
        )
        .unwrap();

        init_db_with_conn(&conn).unwrap();

        let fetched = get_token_by_id_with_conn(&conn, "legacy-1")
            .unwrap()
            .unwrap();
        assert_eq!(fetched.allowed_account_ids, None);

        let all = list_tokens_with_conn(&conn).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].allowed_account_ids, None);
    }

    #[test]
    fn test_parse_allowed_account_ids_fail_closed() {
        assert_eq!(parse_allowed_account_ids(None), None);
        assert_eq!(parse_allowed_account_ids(Some("".to_string())), None);
        assert_eq!(parse_allowed_account_ids(Some("   ".to_string())), None);
        assert_eq!(
            parse_allowed_account_ids(Some("[]".to_string())),
            Some(vec![])
        );
        assert_eq!(
            parse_allowed_account_ids(Some(r#"["acc-1","acc-2"]"#.to_string())),
            Some(vec!["acc-1".to_string(), "acc-2".to_string()])
        );
        // Malformed JSON must fail-closed to Some(vec![]) rather than None
        assert_eq!(
            parse_allowed_account_ids(Some("{not-valid-json".to_string())),
            Some(vec![])
        );
        assert_eq!(
            parse_allowed_account_ids(Some("12345".to_string())),
            Some(vec![])
        );
    }
}
