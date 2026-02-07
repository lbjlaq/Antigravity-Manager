// Proxy Commands Module
// Organized into logical submodules for maintainability

pub mod accounts;
pub mod config;
pub mod external;
pub mod lifecycle;
pub mod logs;
pub mod status;
mod types;

// Re-export types (these don't need #[tauri::command])
pub use types::{AdminServerInstance, ProxyServiceInstance, ProxyServiceState, ProxyStatus};

// Re-export internal helpers (non-command functions)
pub use lifecycle::{ensure_admin_server, internal_start_proxy_service};

// Re-export for internal use by other command modules
pub use accounts::reload_proxy_accounts;
