// File: src-tauri/src/commands/stats.rs
//! Token statistics Tauri commands
//! Analytics for token usage across accounts and models

use crate::error::{AppError, AppResult};
use crate::modules::token_stats::{
    TokenStatsAggregated, AccountTokenStats, TokenStatsSummary,
    ModelTokenStats, ModelTrendPoint, AccountTrendPoint,
};

// Re-export types for external use

// ============================================================================
// Time-based Statistics
// ============================================================================

/// Get hourly token statistics
#[tauri::command]
pub async fn get_token_stats_hourly(hours: i64) -> AppResult<Vec<TokenStatsAggregated>> {
    crate::modules::token_stats::get_hourly_stats(hours)
        .map_err(AppError::Account)
}

/// Get daily token statistics
#[tauri::command]
pub async fn get_token_stats_daily(days: i64) -> AppResult<Vec<TokenStatsAggregated>> {
    crate::modules::token_stats::get_daily_stats(days)
        .map_err(AppError::Account)
}

/// Get weekly token statistics
#[tauri::command]
pub async fn get_token_stats_weekly(weeks: i64) -> AppResult<Vec<TokenStatsAggregated>> {
    crate::modules::token_stats::get_weekly_stats(weeks)
        .map_err(AppError::Account)
}

// ============================================================================
// Account-based Statistics
// ============================================================================

/// Get token statistics by account
#[tauri::command]
pub async fn get_token_stats_by_account(hours: i64) -> AppResult<Vec<AccountTokenStats>> {
    crate::modules::token_stats::get_account_stats(hours)
        .map_err(AppError::Account)
}

/// Get summary statistics
#[tauri::command]
pub async fn get_token_stats_summary(hours: i64) -> AppResult<TokenStatsSummary> {
    crate::modules::token_stats::get_summary_stats(hours)
        .map_err(AppError::Account)
}

// ============================================================================
// Model-based Statistics
// ============================================================================

/// Get token statistics by model
#[tauri::command]
pub async fn get_token_stats_by_model(hours: i64) -> AppResult<Vec<ModelTokenStats>> {
    crate::modules::token_stats::get_model_stats(hours)
        .map_err(AppError::Account)
}

/// Get model trend (hourly)
#[tauri::command]
pub async fn get_token_stats_model_trend_hourly(hours: i64) -> AppResult<Vec<ModelTrendPoint>> {
    crate::modules::token_stats::get_model_trend_hourly(hours)
        .map_err(AppError::Account)
}

/// Get model trend (daily)
#[tauri::command]
pub async fn get_token_stats_model_trend_daily(days: i64) -> AppResult<Vec<ModelTrendPoint>> {
    crate::modules::token_stats::get_model_trend_daily(days)
        .map_err(AppError::Account)
}

// ============================================================================
// Account Trend Statistics
// ============================================================================

/// Get account trend (hourly)
#[tauri::command]
pub async fn get_token_stats_account_trend_hourly(hours: i64) -> AppResult<Vec<AccountTrendPoint>> {
    crate::modules::token_stats::get_account_trend_hourly(hours)
        .map_err(AppError::Account)
}

/// Get account trend (daily)
#[tauri::command]
pub async fn get_token_stats_account_trend_daily(days: i64) -> AppResult<Vec<AccountTrendPoint>> {
    crate::modules::token_stats::get_account_trend_daily(days)
        .map_err(AppError::Account)
}
