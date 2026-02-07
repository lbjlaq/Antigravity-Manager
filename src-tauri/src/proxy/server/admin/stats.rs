//! Token statistics admin handlers
//!
//! Handles all token usage statistics endpoints.

use axum::{http::StatusCode, response::IntoResponse, Json};

use crate::modules::{logger, token_stats};
use crate::proxy::server::types::{ErrorResponse, StatsPeriodQuery};

// ============================================================================
// Token Statistics
// ============================================================================

pub async fn get_token_stats_hourly(
    axum::extract::Query(p): axum::extract::Query<StatsPeriodQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let hours = p.hours.unwrap_or(24);
    let res = tokio::task::spawn_blocking(move || token_stats::get_hourly_stats(hours)).await;

    match res {
        Ok(Ok(stats)) => Ok(Json(stats)),
        Ok(Err(e)) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: e }),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}

pub async fn get_token_stats_daily(
    axum::extract::Query(p): axum::extract::Query<StatsPeriodQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let days = p.days.unwrap_or(7);
    let res = tokio::task::spawn_blocking(move || token_stats::get_daily_stats(days)).await;

    match res {
        Ok(Ok(stats)) => Ok(Json(stats)),
        Ok(Err(e)) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: e }),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}

pub async fn get_token_stats_weekly(
    axum::extract::Query(p): axum::extract::Query<StatsPeriodQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let weeks = p.weeks.unwrap_or(4);
    let res = tokio::task::spawn_blocking(move || token_stats::get_weekly_stats(weeks)).await;

    match res {
        Ok(Ok(stats)) => Ok(Json(stats)),
        Ok(Err(e)) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: e }),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}

pub async fn get_token_stats_by_account(
    axum::extract::Query(p): axum::extract::Query<StatsPeriodQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let hours = p.hours.unwrap_or(168);
    let res = tokio::task::spawn_blocking(move || token_stats::get_account_stats(hours)).await;

    match res {
        Ok(Ok(stats)) => Ok(Json(stats)),
        Ok(Err(e)) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: e }),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}

pub async fn get_token_stats_summary(
    axum::extract::Query(p): axum::extract::Query<StatsPeriodQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let hours = p.hours.unwrap_or(168);
    let res = tokio::task::spawn_blocking(move || token_stats::get_summary_stats(hours)).await;

    match res {
        Ok(Ok(stats)) => Ok(Json(stats)),
        Ok(Err(e)) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: e }),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}

pub async fn get_token_stats_by_model(
    axum::extract::Query(p): axum::extract::Query<StatsPeriodQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let hours = p.hours.unwrap_or(168);
    let res = tokio::task::spawn_blocking(move || token_stats::get_model_stats(hours)).await;

    match res {
        Ok(Ok(stats)) => Ok(Json(stats)),
        Ok(Err(e)) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: e }),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}

pub async fn get_token_stats_model_trend_hourly(
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let res =
        tokio::task::spawn_blocking(|| token_stats::get_model_trend_hourly(24)).await;

    match res {
        Ok(Ok(stats)) => Ok(Json(stats)),
        Ok(Err(e)) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: e }),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}

pub async fn get_token_stats_model_trend_daily(
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let res =
        tokio::task::spawn_blocking(|| token_stats::get_model_trend_daily(7)).await;

    match res {
        Ok(Ok(stats)) => Ok(Json(stats)),
        Ok(Err(e)) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: e }),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}

pub async fn get_token_stats_account_trend_hourly(
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let res =
        tokio::task::spawn_blocking(|| token_stats::get_account_trend_hourly(24)).await;

    match res {
        Ok(Ok(stats)) => Ok(Json(stats)),
        Ok(Err(e)) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: e }),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}

pub async fn get_token_stats_account_trend_daily(
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let res =
        tokio::task::spawn_blocking(|| token_stats::get_account_trend_daily(7)).await;

    match res {
        Ok(Ok(stats)) => Ok(Json(stats)),
        Ok(Err(e)) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: e }),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}

pub async fn clear_token_stats() -> impl IntoResponse {
    let res = tokio::task::spawn_blocking(|| {
        // Clear databases (brute force)
        if let Ok(path) = token_stats::get_db_path() {
            let _ = std::fs::remove_file(path);
        }
        let _ = token_stats::init_db();
    })
    .await;

    match res {
        Ok(_) => {
            logger::log_info("[API] Cleared all token statistics");
            StatusCode::OK
        }
        Err(e) => {
            logger::log_error(&format!("[API] Failed to clear token stats: {}", e));
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}
