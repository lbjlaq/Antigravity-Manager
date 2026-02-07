//! Admin API handlers module
//!
//! This module organizes all admin API handlers into logical submodules.

pub mod accounts;
pub mod import;
pub mod proxy;
pub mod stats;
pub mod system;

// Re-export all handlers for convenient access
pub use accounts::*;
pub use import::*;
pub use proxy::*;
pub use stats::*;
pub use system::*;
