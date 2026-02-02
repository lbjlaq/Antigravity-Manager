//! Route definitions for the Axum server
//!
//! This module defines all API routes and builds the router.

use axum::{
    routing::{any, delete, get, post},
    Router,
};

use crate::proxy::server::admin;
use crate::proxy::server::oauth;
use crate::proxy::server::types::AppState;

/// Build the admin API routes (requires authentication)
pub fn build_admin_routes() -> Router<AppState> {
    Router::new()
        // Health check
        .route("/health", get(health_check))
        // Account management
        .route("/accounts", get(admin::list_accounts).post(admin::add_account))
        .route("/accounts/current", get(admin::get_current_account))
        .route("/accounts/switch", post(admin::switch_account))
        .route("/accounts/refresh", post(admin::refresh_all_quotas))
        .route("/accounts/:accountId", delete(admin::delete_account))
        .route("/accounts/:accountId/bind-device", post(admin::bind_device))
        .route("/accounts/:accountId/device-profiles", get(admin::get_device_profiles))
        .route("/accounts/:accountId/device-versions", get(admin::list_device_versions))
        .route("/accounts/device-preview", post(admin::preview_generate_profile))
        .route(
            "/accounts/:accountId/bind-device-profile",
            post(admin::bind_device_profile_with_profile),
        )
        .route("/accounts/restore-original", post(admin::restore_original_device))
        .route(
            "/accounts/:accountId/device-versions/:versionId/restore",
            post(admin::restore_device_version),
        )
        .route(
            "/accounts/:accountId/device-versions/:versionId",
            delete(admin::delete_device_version),
        )
        // Import
        .route("/accounts/import/v1", post(admin::import_v1_accounts))
        .route("/accounts/import/db", post(admin::import_from_db))
        .route("/accounts/import/db-custom", post(admin::import_custom_db))
        .route("/accounts/sync/db", post(admin::sync_account_from_db))
        // Statistics (legacy paths)
        .route("/stats/summary", get(admin::get_token_stats_summary))
        .route("/stats/hourly", get(admin::get_token_stats_hourly))
        .route("/stats/daily", get(admin::get_token_stats_daily))
        .route("/stats/weekly", get(admin::get_token_stats_weekly))
        .route("/stats/accounts", get(admin::get_token_stats_by_account))
        .route("/stats/models", get(admin::get_token_stats_by_model))
        // Configuration
        .route("/config", get(admin::get_config).post(admin::save_config))
        // CLI sync
        .route("/proxy/cli/status", post(admin::get_cli_sync_status))
        .route("/proxy/cli/sync", post(admin::execute_cli_sync))
        .route("/proxy/cli/restore", post(admin::execute_cli_restore))
        .route("/proxy/cli/config", post(admin::get_cli_config_content))
        // Proxy control
        .route("/proxy/status", get(admin::get_proxy_status))
        .route("/proxy/start", post(admin::start_proxy_service))
        .route("/proxy/stop", post(admin::stop_proxy_service))
        .route("/proxy/mapping", post(admin::update_model_mapping))
        .route("/proxy/api-key/generate", post(admin::generate_api_key))
        .route("/proxy/session-bindings/clear", post(admin::clear_proxy_session_bindings))
        .route("/proxy/rate-limits", delete(admin::clear_all_rate_limits))
        .route("/proxy/rate-limits/:accountId", delete(admin::clear_rate_limit))
        // [FIX #820] Preferred account
        .route(
            "/proxy/preferred-account",
            get(admin::get_preferred_account).post(admin::set_preferred_account),
        )
        // OAuth (Admin endpoints)
        .route("/accounts/oauth/prepare", post(admin::prepare_oauth_url))
        .route("/accounts/oauth/start", post(admin::start_oauth_login))
        .route("/accounts/oauth/complete", post(admin::complete_oauth_login))
        .route("/accounts/oauth/cancel", post(admin::cancel_oauth_login))
        .route("/accounts/oauth/submit-code", post(admin::submit_oauth_code))
        // z.ai
        .route("/zai/models/fetch", post(admin::fetch_zai_models))
        // Monitor
        .route("/proxy/monitor/toggle", post(admin::set_proxy_monitor_enabled))
        // Cloudflared
        .route("/proxy/cloudflared/status", get(admin::cloudflared_get_status))
        .route("/proxy/cloudflared/install", post(admin::cloudflared_install))
        .route("/proxy/cloudflared/start", post(admin::cloudflared_start))
        .route("/proxy/cloudflared/stop", post(admin::cloudflared_stop))
        // System
        .route("/system/open-folder", post(admin::open_folder))
        .route("/proxy/stats", get(admin::get_proxy_stats))
        // Logs
        .route("/logs", get(admin::get_proxy_logs_filtered))
        .route("/logs/count", get(admin::get_proxy_logs_count_filtered))
        .route("/logs/clear", post(admin::clear_proxy_logs))
        .route("/logs/:logId", get(admin::get_proxy_log_detail))
        // Token stats (new paths)
        .route("/stats/token/clear", post(admin::clear_token_stats))
        .route("/stats/token/hourly", get(admin::get_token_stats_hourly))
        .route("/stats/token/daily", get(admin::get_token_stats_daily))
        .route("/stats/token/weekly", get(admin::get_token_stats_weekly))
        .route("/stats/token/by-account", get(admin::get_token_stats_by_account))
        .route("/stats/token/summary", get(admin::get_token_stats_summary))
        .route("/stats/token/by-model", get(admin::get_token_stats_by_model))
        .route(
            "/stats/token/model-trend/hourly",
            get(admin::get_token_stats_model_trend_hourly),
        )
        .route(
            "/stats/token/model-trend/daily",
            get(admin::get_token_stats_model_trend_daily),
        )
        .route(
            "/stats/token/account-trend/hourly",
            get(admin::get_token_stats_account_trend_hourly),
        )
        .route(
            "/stats/token/account-trend/daily",
            get(admin::get_token_stats_account_trend_daily),
        )
        // Account bulk operations
        .route("/accounts/bulk-delete", post(admin::delete_accounts))
        .route("/accounts/export", post(admin::export_accounts))
        .route("/accounts/reorder", post(admin::reorder_accounts))
        .route("/accounts/:accountId/quota", get(admin::fetch_account_quota))
        .route("/accounts/:accountId/toggle-proxy", post(admin::toggle_proxy_status))
        // Warmup
        .route("/accounts/warmup", post(admin::warm_up_all_accounts))
        .route("/accounts/:accountId/warmup", post(admin::warm_up_account))
        // System paths
        .route("/system/data-dir", get(admin::get_data_dir_path))
        .route("/system/save-file", post(admin::save_text_file))
        .route("/system/updates/settings", get(admin::get_update_settings))
        .route("/system/updates/check-status", get(admin::should_check_updates))
        .route("/system/updates/check", post(admin::check_for_updates))
        .route("/system/updates/touch", post(admin::update_last_check_time))
        .route("/system/updates/save", post(admin::save_update_settings))
        .route("/system/autostart/status", get(admin::is_auto_launch_enabled))
        .route("/system/autostart/toggle", post(admin::toggle_auto_launch))
        .route(
            "/system/http-api/settings",
            get(admin::get_http_api_settings).post(admin::save_http_api_settings),
        )
        .route("/system/antigravity/path", get(admin::get_antigravity_path))
        .route("/system/antigravity/args", get(admin::get_antigravity_args))
        // OAuth (Web) - Admin interface
        .route("/auth/url", get(oauth::prepare_oauth_url_web))
}

/// Build the proxy API routes (AI endpoints)
pub fn build_proxy_routes() -> Router<AppState> {
    use crate::proxy::handlers;

    Router::new()
        // OpenAI Protocol
        .route("/v1/models", get(handlers::openai::handle_list_models))
        .route(
            "/v1/chat/completions",
            post(handlers::openai::handle_chat_completions),
        )
        .route(
            "/v1/completions",
            post(handlers::openai::handle_completions),
        )
        .route("/v1/responses", post(handlers::openai::handle_completions)) // Codex CLI compat
        .route(
            "/v1/images/generations",
            post(handlers::openai::handle_images_generations),
        )
        .route(
            "/v1/images/edits",
            post(handlers::openai::handle_images_edits),
        )
        .route(
            "/v1/audio/transcriptions",
            post(handlers::audio::handle_audio_transcription),
        )
        // Claude Protocol
        .route("/v1/messages", post(handlers::claude::handle_messages))
        .route(
            "/v1/messages/count_tokens",
            post(handlers::claude::handle_count_tokens),
        )
        .route(
            "/v1/models/claude",
            get(handlers::claude::handle_list_models),
        )
        // z.ai MCP (optional reverse-proxy)
        .route(
            "/mcp/web_search_prime/mcp",
            any(handlers::mcp::handle_web_search_prime),
        )
        .route(
            "/mcp/web_reader/mcp",
            any(handlers::mcp::handle_web_reader),
        )
        .route(
            "/mcp/zai-mcp-server/mcp",
            any(handlers::mcp::handle_zai_mcp_server),
        )
        // Gemini Protocol (Native)
        .route("/v1beta/models", get(handlers::gemini::handle_list_models))
        .route(
            "/v1beta/models/:model",
            get(handlers::gemini::handle_get_model).post(handlers::gemini::handle_generate),
        )
        .route(
            "/v1beta/models/:model/countTokens",
            post(handlers::gemini::handle_count_tokens),
        )
        // Common endpoints
        .route("/v1/models/detect", post(handlers::common::handle_detect_model))
        .route("/internal/warmup", post(handlers::warmup::handle_warmup))
        // Telemetry intercept
        .route("/v1/api/event_logging/batch", post(silent_ok))
        .route("/v1/api/event_logging", post(silent_ok))
}

/// Health check handler
pub async fn health_check() -> axum::response::Response {
    axum::Json(serde_json::json!({
        "status": "ok"
    }))
    .into_response()
}

/// Silent OK handler (for telemetry intercept)
pub async fn silent_ok() -> axum::response::Response {
    axum::http::StatusCode::OK.into_response()
}

use axum::response::IntoResponse;
