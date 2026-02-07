// File: src-tauri/src/commands/mod.rs
//! Tauri command handlers
//! 
//! This module organizes all IPC commands by domain.
//! Each submodule contains related commands for a specific feature area.

// ============================================================================
// Submodules
// ============================================================================

// Core account management
pub mod account;

// Device fingerprint operations
pub mod device;

// OAuth authentication flow
pub mod oauth;

// Import and migration
pub mod import;

// Quota management and warmup
pub mod quota;

// Configuration management
pub mod config;

// Token statistics
pub mod stats;

// System utilities (files, windows, updates)
pub mod system;

// Proxy service control
pub mod proxy;

// Autostart management
pub mod autostart;

// Cloudflared tunnel management
pub mod cloudflared;

// Security (IP blacklist/whitelist)
pub mod security;

// ============================================================================
// Re-exports for internal use (refresh_all_quotas_internal used by scheduler)
// ============================================================================

pub use quota::refresh_all_quotas_internal;
