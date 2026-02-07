//! Process management utilities for Antigravity application.
//!
//! This module provides cross-platform process detection, lifecycle management,
//! and path resolution for the Antigravity application.
//!
//! # Module Structure
//!
//! - `detection` - Process detection and identification
//! - `lifecycle` - Start/stop process management
//! - `paths` - Executable path resolution
//! - `helpers` - Helper process identification utilities

mod detection;
mod helpers;
mod lifecycle;
mod paths;

// Re-export public API
pub use detection::is_antigravity_running;
pub use lifecycle::{close_antigravity, start_antigravity};
pub use paths::{
    get_antigravity_executable_path, get_args_from_running_process,
    get_user_data_dir_from_process,
};
