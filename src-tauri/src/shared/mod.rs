// File: src-tauri/src/shared/mod.rs
//! Shared infrastructure modules for Antigravity Manager
//! Contains cross-cutting concerns: database pooling, utilities, etc.

pub mod db_pool;

pub use db_pool::{DbPool, PooledConnection, get_pool, with_connection};
