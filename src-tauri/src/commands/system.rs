// File: src-tauri/src/commands/system.rs
//! System utility Tauri commands
//! Window management, file operations, updates, paths

use crate::error::{AppError, AppResult};
use crate::modules;

fn validate_text_file_path(path: &str) -> AppResult<()> {
    if path.trim().is_empty() {
        return Err(AppError::Validation("File path cannot be empty".to_string()));
    }

    let normalized = path.replace('\\', "/").to_ascii_lowercase();

    if normalized.contains("../") || normalized.contains("..\\") || normalized.ends_with("/..") {
        return Err(AppError::Validation(
            "Path traversal is not allowed".to_string(),
        ));
    }

    let forbidden_prefixes = [
        "/etc/",
        "/proc/",
        "/sys/",
        "/dev/",
        "/root/",
        "/var/spool/cron",
        "c:/windows",
        "c:/programdata",
    ];

    if forbidden_prefixes
        .iter()
        .any(|prefix| normalized.starts_with(prefix))
    {
        return Err(AppError::Security(
            "Access to system-sensitive path is denied".to_string(),
        ));
    }

    Ok(())
}

// ============================================================================
// File Operations
// ============================================================================

/// Save text to file (bypasses frontend scope restrictions)
#[tauri::command]
pub async fn save_text_file(path: String, content: String) -> AppResult<()> {
    validate_text_file_path(&path)?;
    std::fs::write(&path, content)
        .map_err(|e| AppError::Io(e))
}

/// Read text from file (bypasses frontend scope restrictions)
#[tauri::command]
pub async fn read_text_file(path: String) -> AppResult<String> {
    validate_text_file_path(&path)?;
    std::fs::read_to_string(&path)
        .map_err(|e| AppError::Io(e))
}

/// Clear log cache
#[tauri::command]
pub async fn clear_log_cache() -> AppResult<()> {
    modules::logger::clear_logs()
        .map_err(AppError::Internal)
}

/// Clear Antigravity application cache
/// Used to fix login failures, version validation errors, etc.
#[tauri::command]
pub async fn clear_antigravity_cache() -> Result<crate::modules::cache::ClearResult, String> {
    crate::modules::cache::clear_antigravity_cache(None)
}

/// Get Antigravity cache paths list (for preview)
#[tauri::command]
pub async fn get_antigravity_cache_paths() -> Result<Vec<String>, String> {
    Ok(crate::modules::cache::get_existing_cache_paths()
        .into_iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect())
}

/// Open data folder in file explorer
#[tauri::command]
pub async fn open_data_folder() -> AppResult<()> {
    let path = modules::account::get_data_dir()
        .map_err(AppError::Account)?;

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&path)
            .spawn()
            .map_err(|e| AppError::Internal(format!("Failed to open folder: {}", e)))?;
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&path)
            .spawn()
            .map_err(|e| AppError::Internal(format!("Failed to open folder: {}", e)))?;
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&path)
            .spawn()
            .map_err(|e| AppError::Internal(format!("Failed to open folder: {}", e)))?;
    }

    Ok(())
}

/// Get data directory absolute path
#[tauri::command]
pub async fn get_data_dir_path() -> AppResult<String> {
    let path = modules::account::get_data_dir()
        .map_err(AppError::Account)?;
    Ok(path.to_string_lossy().to_string())
}

// ============================================================================
// Window Management
// ============================================================================

/// Show main window
#[tauri::command]
pub async fn show_main_window(window: tauri::Window) -> AppResult<()> {
    window.show()
        .map_err(|e| AppError::Tauri(e))
}

/// Set window theme (for Windows title bar button color sync)
#[tauri::command]
pub async fn set_window_theme(window: tauri::Window, theme: String) -> AppResult<()> {
    use tauri::Theme;

    let tauri_theme = match theme.as_str() {
        "dark" => Some(Theme::Dark),
        "light" => Some(Theme::Light),
        _ => None, // system default
    };

    window.set_theme(tauri_theme)
        .map_err(|e| AppError::Tauri(e))
}

// ============================================================================
// Antigravity Path Detection
// ============================================================================

/// Get Antigravity executable path
#[tauri::command]
pub async fn get_antigravity_path(bypass_config: Option<bool>) -> AppResult<String> {
    // Priority: config > detection
    if bypass_config != Some(true) {
        if let Ok(config) = crate::modules::config::load_app_config() {
            if let Some(path) = config.antigravity_executable {
                if std::path::Path::new(&path).exists() {
                    return Ok(path);
                }
            }
        }
    }

    // Real-time detection
    match crate::modules::process::get_antigravity_executable_path() {
        Some(path) => Ok(path.to_string_lossy().to_string()),
        None => Err(AppError::NotFound("Antigravity installation path not found".to_string())),
    }
}

/// Get Antigravity launch arguments from running process
#[tauri::command]
pub async fn get_antigravity_args() -> AppResult<Vec<String>> {
    match crate::modules::process::get_args_from_running_process() {
        Some(args) => Ok(args),
        None => Err(AppError::NotFound("No running Antigravity process found".to_string())),
    }
}

// ============================================================================
// Update Checking
// ============================================================================

pub use crate::modules::update_checker::UpdateInfo;

/// Check for updates from GitHub releases
#[tauri::command]
pub async fn check_for_updates() -> AppResult<UpdateInfo> {
    modules::logger::log_info("Update check triggered by frontend");
    crate::modules::update_checker::check_for_updates()
        .await
        .map_err(AppError::Internal)
}

/// Check if updates should be checked based on settings
#[tauri::command]
pub async fn should_check_updates() -> AppResult<bool> {
    let settings = crate::modules::update_checker::load_update_settings()
        .map_err(AppError::Config)?;
    Ok(crate::modules::update_checker::should_check_for_updates(&settings))
}

/// Update last check timestamp
#[tauri::command]
pub async fn update_last_check_time() -> AppResult<()> {
    crate::modules::update_checker::update_last_check_time()
        .map_err(AppError::Config)
}

/// Get update settings
#[tauri::command]
pub async fn get_update_settings() -> AppResult<crate::modules::update_checker::UpdateSettings> {
    crate::modules::update_checker::load_update_settings()
        .map_err(AppError::Config)
}

/// Save update settings
#[tauri::command]
pub async fn save_update_settings(
    settings: crate::modules::update_checker::UpdateSettings,
) -> AppResult<()> {
    crate::modules::update_checker::save_update_settings(&settings)
        .map_err(AppError::Config)
}
