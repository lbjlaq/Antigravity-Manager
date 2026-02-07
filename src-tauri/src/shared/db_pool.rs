// File: src-tauri/src/shared/db_pool.rs
//! SQLite connection pooling using r2d2
//! Eliminates "Too many open files" errors and improves performance

use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

use r2d2::{Pool, PooledConnection as R2D2PooledConnection};
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::OpenFlags;

use crate::error::{AppError, AppResult};

// ============================================================================
// Type Aliases
// ============================================================================

/// SQLite connection pool type
pub type DbPool = Pool<SqliteConnectionManager>;

/// Pooled connection type
pub type PooledConnection = R2D2PooledConnection<SqliteConnectionManager>;

// ============================================================================
// Global Pool Registry
// ============================================================================

/// Registry of database pools by path
static POOL_REGISTRY: OnceLock<dashmap::DashMap<String, DbPool>> = OnceLock::new();

fn get_registry() -> &'static dashmap::DashMap<String, DbPool> {
    POOL_REGISTRY.get_or_init(dashmap::DashMap::new)
}

// ============================================================================
// Pool Configuration
// ============================================================================

/// Configuration for database pool
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Maximum number of connections in the pool
    pub max_size: u32,
    /// Minimum number of idle connections
    pub min_idle: Option<u32>,
    /// Connection timeout
    pub connection_timeout: Duration,
    /// Maximum lifetime of a connection
    pub max_lifetime: Option<Duration>,
    /// Idle timeout for connections
    pub idle_timeout: Option<Duration>,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_size: 10,
            min_idle: Some(2),
            connection_timeout: Duration::from_secs(30),
            max_lifetime: Some(Duration::from_secs(3600)), // 1 hour
            idle_timeout: Some(Duration::from_secs(600)),  // 10 minutes
        }
    }
}

// ============================================================================
// Pool Creation
// ============================================================================

/// Create a new connection pool for the given database path
pub fn create_pool(db_path: &PathBuf, config: PoolConfig) -> AppResult<DbPool> {
    let manager = SqliteConnectionManager::file(db_path)
        .with_flags(
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .with_init(|conn| {
            // Enable WAL mode for better concurrent access
            conn.execute_batch(
                "PRAGMA journal_mode = WAL;
                 PRAGMA synchronous = NORMAL;
                 PRAGMA foreign_keys = ON;
                 PRAGMA busy_timeout = 5000;",
            )?;
            Ok(())
        });

    let pool = Pool::builder()
        .max_size(config.max_size)
        .min_idle(config.min_idle)
        .connection_timeout(config.connection_timeout)
        .max_lifetime(config.max_lifetime)
        .idle_timeout(config.idle_timeout)
        .build(manager)
        .map_err(|e| AppError::DatabasePool(format!("Failed to create pool: {}", e)))?;

    Ok(pool)
}

/// Get or create a pool for the given database path
pub fn get_pool(db_path: &PathBuf) -> AppResult<DbPool> {
    let key = db_path.to_string_lossy().to_string();
    let registry = get_registry();

    if let Some(pool) = registry.get(&key) {
        return Ok(pool.clone());
    }

    let pool = create_pool(db_path, PoolConfig::default())?;
    registry.insert(key, pool.clone());
    Ok(pool)
}

/// Get a connection from the pool for the given database path
pub fn get_connection(db_path: &PathBuf) -> AppResult<PooledConnection> {
    let pool = get_pool(db_path)?;
    pool.get()
        .map_err(|e| AppError::DatabasePool(format!("Failed to get connection: {}", e)))
}

/// Execute a function with a pooled connection
pub fn with_connection<F, T>(db_path: &PathBuf, f: F) -> AppResult<T>
where
    F: FnOnce(&PooledConnection) -> AppResult<T>,
{
    let conn = get_connection(db_path)?;
    f(&conn)
}

/// Execute a function with a mutable pooled connection
pub fn with_connection_mut<F, T>(db_path: &PathBuf, f: F) -> AppResult<T>
where
    F: FnOnce(&mut PooledConnection) -> AppResult<T>,
{
    let mut conn = get_connection(db_path)?;
    f(&mut conn)
}

// ============================================================================
// Pool Management
// ============================================================================

/// Get pool statistics
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolStats {
    pub path: String,
    pub connections: u32,
    pub idle_connections: u32,
    pub max_size: u32,
}

/// Get statistics for all pools
pub fn get_all_pool_stats() -> Vec<PoolStats> {
    let registry = get_registry();
    registry
        .iter()
        .map(|entry| {
            let pool = entry.value();
            let state = pool.state();
            PoolStats {
                path: entry.key().clone(),
                connections: state.connections,
                idle_connections: state.idle_connections,
                max_size: pool.max_size(),
            }
        })
        .collect()
}

/// Close all pools (for graceful shutdown)
pub fn close_all_pools() {
    let registry = get_registry();
    registry.clear();
    tracing::info!("All database pools closed");
}

/// Remove a specific pool from registry
pub fn remove_pool(db_path: &PathBuf) {
    let key = db_path.to_string_lossy().to_string();
    get_registry().remove(&key);
}

// ============================================================================
// Convenience Functions for Common Databases
// ============================================================================

/// Get connection to the main application database
pub fn get_app_db_connection() -> AppResult<PooledConnection> {
    let data_dir = crate::modules::account::get_data_dir()
        .map_err(|e| AppError::Config(format!("Failed to get data dir: {}", e)))?;
    let db_path = data_dir.join("antigravity.db");
    get_connection(&db_path)
}

/// Get connection to the token stats database
pub fn get_stats_db_connection() -> AppResult<PooledConnection> {
    let data_dir = crate::modules::account::get_data_dir()
        .map_err(|e| AppError::Config(format!("Failed to get data dir: {}", e)))?;
    let db_path = data_dir.join("token_stats.db");
    get_connection(&db_path)
}

/// Get connection to the proxy logs database
pub fn get_proxy_logs_db_connection() -> AppResult<PooledConnection> {
    let data_dir = crate::modules::account::get_data_dir()
        .map_err(|e| AppError::Config(format!("Failed to get data dir: {}", e)))?;
    let db_path = data_dir.join("proxy_logs.db");
    get_connection(&db_path)
}

/// Get connection to the security database
pub fn get_security_db_connection() -> AppResult<PooledConnection> {
    let data_dir = crate::modules::account::get_data_dir()
        .map_err(|e| AppError::Config(format!("Failed to get data dir: {}", e)))?;
    let db_path = data_dir.join("security.db");
    get_connection(&db_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::temp_dir;

    #[test]
    fn test_pool_creation() {
        let temp_path = temp_dir().join("test_pool.db");
        let pool = create_pool(&temp_path, PoolConfig::default());
        assert!(pool.is_ok());
        
        // Cleanup
        let _ = std::fs::remove_file(&temp_path);
    }

    #[test]
    fn test_with_connection() {
        let temp_path = temp_dir().join("test_with_conn.db");
        
        let result = with_connection(&temp_path, |conn| {
            conn.execute("CREATE TABLE IF NOT EXISTS test (id INTEGER)", [])
                .map_err(AppError::Database)?;
            Ok(42)
        });
        
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
        
        // Cleanup
        remove_pool(&temp_path);
        let _ = std::fs::remove_file(&temp_path);
    }
}
