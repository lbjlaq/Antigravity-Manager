// File: src-tauri/src/error.rs
//! Unified error handling for Antigravity Manager
//! All Tauri commands should return AppResult<T> for consistent error handling

use serde::Serialize;
use thiserror::Error;

/// Application-wide error type with structured serialization for frontend
#[derive(Error, Debug)]
pub enum AppError {
    // ============================================================================
    // Database Errors
    // ============================================================================
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Database pool error: {0}")]
    DatabasePool(String),

    // ============================================================================
    // Network Errors
    // ============================================================================
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("Connection timeout")]
    Timeout,

    #[error("Rate limited: retry after {retry_after}s")]
    RateLimit { retry_after: u64 },

    // ============================================================================
    // IO Errors
    // ============================================================================
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("File not found: {0}")]
    FileNotFound(String),

    // ============================================================================
    // Tauri Errors
    // ============================================================================
    #[error("Tauri error: {0}")]
    Tauri(#[from] tauri::Error),

    // ============================================================================
    // Validation Errors
    // ============================================================================
    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Invalid input: {field} - {message}")]
    InvalidInput { field: String, message: String },

    // ============================================================================
    // Authentication & Authorization Errors
    // ============================================================================
    #[error("OAuth error: {0}")]
    OAuth(String),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Token expired")]
    TokenExpired,

    // ============================================================================
    // Business Logic Errors
    // ============================================================================
    #[error("Account error: {0}")]
    Account(String),

    #[error("Account not found: {0}")]
    AccountNotFound(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Quota exceeded: {0}")]
    QuotaExceeded(String),

    // ============================================================================
    // Proxy Errors
    // ============================================================================
    #[error("Proxy error: {0}")]
    Proxy(String),

    #[error("Upstream error: {status} - {message}")]
    Upstream { status: u16, message: String },

    #[error("No available accounts")]
    NoAvailableAccounts,

    // ============================================================================
    // Security Errors
    // ============================================================================
    #[error("Security error: {0}")]
    Security(String),

    #[error("IP blocked: {0}")]
    IpBlocked(String),

    // ============================================================================
    // Generic Errors
    // ============================================================================
    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Operation cancelled")]
    Cancelled,

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Unknown error: {0}")]
    Unknown(String),
}

/// Structured JSON serialization for frontend consumption
/// Format: { "type": "ErrorVariant", "message": "...", "details": {...} }
impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;

        let (error_type, message, details) = match self {
            // Database errors
            AppError::Database(e) => ("Database", e.to_string(), None),
            AppError::DatabasePool(msg) => ("DatabasePool", msg.clone(), None),

            // Network errors
            AppError::Network(e) => ("Network", e.to_string(), None),
            AppError::Timeout => ("Timeout", "Connection timeout".to_string(), None),
            AppError::RateLimit { retry_after } => (
                "RateLimit",
                format!("Rate limited: retry after {}s", retry_after),
                Some(serde_json::json!({ "retryAfter": retry_after })),
            ),

            // IO errors
            AppError::Io(e) => ("Io", e.to_string(), None),
            AppError::FileNotFound(path) => (
                "FileNotFound",
                format!("File not found: {}", path),
                Some(serde_json::json!({ "path": path })),
            ),

            // Tauri errors
            AppError::Tauri(e) => ("Tauri", e.to_string(), None),

            // Validation errors
            AppError::Validation(msg) => ("Validation", msg.clone(), None),
            AppError::InvalidInput { field, message } => (
                "InvalidInput",
                format!("Invalid input: {} - {}", field, message),
                Some(serde_json::json!({ "field": field, "message": message })),
            ),

            // Auth errors
            AppError::OAuth(msg) => ("OAuth", msg.clone(), None),
            AppError::Unauthorized(msg) => ("Unauthorized", msg.clone(), None),
            AppError::TokenExpired => ("TokenExpired", "Token expired".to_string(), None),

            // Business logic errors
            AppError::Account(msg) => ("Account", msg.clone(), None),
            AppError::AccountNotFound(id) => (
                "AccountNotFound",
                format!("Account not found: {}", id),
                Some(serde_json::json!({ "accountId": id })),
            ),
            AppError::Config(msg) => ("Config", msg.clone(), None),
            AppError::QuotaExceeded(msg) => ("QuotaExceeded", msg.clone(), None),

            // Proxy errors
            AppError::Proxy(msg) => ("Proxy", msg.clone(), None),
            AppError::Upstream { status, message } => (
                "Upstream",
                format!("Upstream error: {} - {}", status, message),
                Some(serde_json::json!({ "status": status, "message": message })),
            ),
            AppError::NoAvailableAccounts => {
                ("NoAvailableAccounts", "No available accounts".to_string(), None)
            }

            // Security errors
            AppError::Security(msg) => ("Security", msg.clone(), None),
            AppError::IpBlocked(ip) => (
                "IpBlocked",
                format!("IP blocked: {}", ip),
                Some(serde_json::json!({ "ip": ip })),
            ),

            // Generic errors
            AppError::NotFound(msg) => ("NotFound", msg.clone(), None),
            AppError::Cancelled => ("Cancelled", "Operation cancelled".to_string(), None),
            AppError::Internal(msg) => ("Internal", msg.clone(), None),
            AppError::Unknown(msg) => ("Unknown", msg.clone(), None),
        };

        let field_count = if details.is_some() { 3 } else { 2 };
        let mut state = serializer.serialize_struct("AppError", field_count)?;
        state.serialize_field("type", error_type)?;
        state.serialize_field("message", &message)?;
        if let Some(ref d) = details {
            state.serialize_field("details", d)?;
        }
        state.end()
    }
}

/// Convert AppError to Tauri IPC error for command returns
/// Note: We implement this via Into to avoid conflict with blanket From<T: Serialize>
impl AppError {
    /// Convert to InvokeError for Tauri command returns
    pub fn into_invoke_error(self) -> tauri::ipc::InvokeError {
        // Serialize to JSON for structured frontend parsing
        let json = serde_json::to_string(&self)
            .unwrap_or_else(|_| format!(r#"{{"type":"Internal","message":"{}"}}"#, self));
        tauri::ipc::InvokeError::from(json)
    }
}

// ============================================================================
// Convenience From implementations for common error types
// ============================================================================

impl From<String> for AppError {
    fn from(s: String) -> Self {
        AppError::Unknown(s)
    }
}

impl From<&str> for AppError {
    fn from(s: &str) -> Self {
        AppError::Unknown(s.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::Validation(format!("JSON error: {}", e))
    }
}

impl From<std::string::FromUtf8Error> for AppError {
    fn from(e: std::string::FromUtf8Error) -> Self {
        AppError::Validation(format!("UTF-8 error: {}", e))
    }
}

impl From<url::ParseError> for AppError {
    fn from(e: url::ParseError) -> Self {
        AppError::Validation(format!("URL parse error: {}", e))
    }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        AppError::Internal(e.to_string())
    }
}

// ============================================================================
// Result type alias for convenience
// ============================================================================

/// Convenience type alias for Results using AppError
pub type AppResult<T> = Result<T, AppError>;

// ============================================================================
// Helper methods
// ============================================================================

impl AppError {
    /// Create a validation error
    pub fn validation(msg: impl Into<String>) -> Self {
        AppError::Validation(msg.into())
    }

    /// Create an account error
    pub fn account(msg: impl Into<String>) -> Self {
        AppError::Account(msg.into())
    }

    /// Create a not found error
    pub fn not_found(msg: impl Into<String>) -> Self {
        AppError::NotFound(msg.into())
    }

    /// Create a proxy error
    pub fn proxy(msg: impl Into<String>) -> Self {
        AppError::Proxy(msg.into())
    }

    /// Create an internal error
    pub fn internal(msg: impl Into<String>) -> Self {
        AppError::Internal(msg.into())
    }

    /// Check if this error is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            AppError::Network(_)
                | AppError::Timeout
                | AppError::RateLimit { .. }
                | AppError::DatabasePool(_)
        )
    }

    /// Get retry delay in seconds if applicable
    pub fn retry_after(&self) -> Option<u64> {
        match self {
            AppError::RateLimit { retry_after } => Some(*retry_after),
            AppError::Timeout => Some(5),
            _ => None,
        }
    }
}

// ============================================================================
// Extension trait for converting Result<T, String> to AppResult<T>
// ============================================================================

/// Extension trait to convert legacy Result<T, String> to AppResult<T>
pub trait ResultExt<T> {
    /// Convert Result<T, String> to AppResult<T> with Account error type
    fn map_account_err(self) -> AppResult<T>;

    /// Convert Result<T, String> to AppResult<T> with Config error type
    fn map_config_err(self) -> AppResult<T>;

    /// Convert Result<T, String> to AppResult<T> with Proxy error type
    fn map_proxy_err(self) -> AppResult<T>;

    /// Convert Result<T, String> to AppResult<T> with Validation error type
    fn map_validation_err(self) -> AppResult<T>;
}

impl<T> ResultExt<T> for Result<T, String> {
    fn map_account_err(self) -> AppResult<T> {
        self.map_err(AppError::Account)
    }

    fn map_config_err(self) -> AppResult<T> {
        self.map_err(AppError::Config)
    }

    fn map_proxy_err(self) -> AppResult<T> {
        self.map_err(AppError::Proxy)
    }

    fn map_validation_err(self) -> AppResult<T> {
        self.map_err(AppError::Validation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_serialization() {
        let error = AppError::RateLimit { retry_after: 60 };
        let json = serde_json::to_string(&error).unwrap();
        assert!(json.contains("\"type\":\"RateLimit\""));
        assert!(json.contains("\"retryAfter\":60"));
    }

    #[test]
    fn test_is_retryable() {
        assert!(AppError::Timeout.is_retryable());
        assert!(AppError::RateLimit { retry_after: 30 }.is_retryable());
        assert!(!AppError::Validation("test".into()).is_retryable());
    }
}
