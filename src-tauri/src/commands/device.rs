// File: src-tauri/src/commands/device.rs
//! Device fingerprint management Tauri commands
//! Operations for device profile capture, generation, and binding

use crate::error::{AppError, AppResult};
use crate::models::DeviceProfile;
use crate::modules;
use tauri_plugin_opener::OpenerExt;

// ============================================================================
// Device Profile Commands
// ============================================================================

/// Get device profiles for an account (current storage.json + bound profiles)
#[tauri::command]
pub async fn get_device_profiles(
    account_id: String,
) -> AppResult<modules::account::DeviceProfiles> {
    modules::get_device_profiles(&account_id)
        .map_err(AppError::Account)
}

/// Bind device profile (capture: current state; generate: new fingerprint)
#[tauri::command]
pub async fn bind_device_profile(
    account_id: String,
    mode: String,
) -> AppResult<DeviceProfile> {
    modules::bind_device_profile(&account_id, &mode)
        .map_err(AppError::Account)
}

/// Preview generate a fingerprint (without persisting)
#[tauri::command]
pub async fn preview_generate_profile() -> AppResult<DeviceProfile> {
    Ok(crate::modules::device::generate_profile())
}

/// Bind with a specific profile directly
#[tauri::command]
pub async fn bind_device_profile_with_profile(
    account_id: String,
    profile: DeviceProfile,
) -> AppResult<DeviceProfile> {
    modules::bind_device_profile_with_profile(&account_id, profile, Some("generated".to_string()))
        .map_err(AppError::Account)
}

/// Apply bound profile to storage.json
#[tauri::command]
pub async fn apply_device_profile(
    account_id: String,
) -> AppResult<DeviceProfile> {
    modules::apply_device_profile(&account_id)
        .map_err(AppError::Account)
}

/// Restore original device state (earliest backup)
#[tauri::command]
pub async fn restore_original_device() -> AppResult<String> {
    modules::restore_original_device()
        .map_err(AppError::Account)
}

/// List device profile versions for an account
#[tauri::command]
pub async fn list_device_versions(
    account_id: String,
) -> AppResult<modules::account::DeviceProfiles> {
    modules::list_device_versions(&account_id)
        .map_err(AppError::Account)
}

/// Restore a specific device version
#[tauri::command]
pub async fn restore_device_version(
    account_id: String,
    version_id: String,
) -> AppResult<DeviceProfile> {
    modules::restore_device_version(&account_id, &version_id)
        .map_err(AppError::Account)
}

/// Delete a device version (baseline cannot be deleted)
#[tauri::command]
pub async fn delete_device_version(
    account_id: String, 
    version_id: String
) -> AppResult<()> {
    modules::delete_device_version(&account_id, &version_id)
        .map_err(AppError::Account)
}

/// Open device storage folder in file explorer
#[tauri::command]
pub async fn open_device_folder(app: tauri::AppHandle) -> AppResult<()> {
    let dir = modules::device::get_storage_dir()
        .map_err(AppError::Account)?;
    let dir_str = dir
        .to_str()
        .ok_or_else(|| AppError::Validation("Cannot parse storage directory path".to_string()))?
        .to_string();
    
    app.opener()
        .open_path(dir_str, None::<&str>)
        .map_err(|e| AppError::Internal(format!("Failed to open folder: {}", e)))
}
