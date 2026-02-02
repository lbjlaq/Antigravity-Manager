//! Account management module.
//!
//! This module provides comprehensive account management functionality
//! including storage, CRUD operations, device profile management,
//! quota tracking, and account switching.
//!
//! # Module Structure
//!
//! - `storage` - File system operations for accounts
//! - `crud` - Create, update, delete, reorder operations
//! - `device` - Device profile binding and management
//! - `quota` - Quota fetching and protection logic
//! - `switch` - Account switching logic

mod crud;
mod device;
mod quota;
mod storage;
mod switch;

// Re-export public API
pub use crud::{
    add_account, delete_account, delete_accounts, export_accounts_by_ids, reorder_accounts, upsert_account,
};
pub use device::{
    apply_device_profile, bind_device_profile, bind_device_profile_with_profile,
    delete_device_version, get_device_profiles, list_device_versions, restore_device_version,
    restore_original_device, DeviceProfiles,
};
pub use quota::{fetch_quota_with_retry, refresh_all_quotas_logic, toggle_proxy_status, update_account_quota, RefreshStats};
pub use storage::{
    get_accounts_dir, get_current_account, get_current_account_id, get_data_dir, list_accounts,
    load_account, load_account_async, load_account_index, save_account, save_account_index,
    set_current_account_id,
};
pub use switch::switch_account;
