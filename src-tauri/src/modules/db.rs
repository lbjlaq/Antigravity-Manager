use crate::utils::protobuf;
use rusqlite::Connection;
use std::path::PathBuf;

fn get_antigravity_path(target_ide: Option<&str>) -> Option<PathBuf> {
    if let Ok(config) = crate::modules::config::load_app_config() {
        if let Some(path_str) = config.antigravity_executable {
            let path = PathBuf::from(path_str);
            if path.exists() {
                return Some(path);
            }
        }
    }
    crate::modules::process::get_antigravity_executable_path(target_ide)
}

/// Get Antigravity database path (cross-platform)
pub fn get_db_path(target_ide: Option<&str>) -> Result<PathBuf, String> {
    // Prefer path specified by --user-data-dir argument
    if let Some(user_data_dir) = crate::modules::process::get_user_data_dir_from_process(target_ide) {
        let custom_db_path = user_data_dir.join("User").join("globalStorage").join("state.vscdb");
        if custom_db_path.exists() {
            return Ok(custom_db_path);
        }
    }

    // Check if in portable mode
    if let Some(antigravity_path) = get_antigravity_path(target_ide) {
        if let Some(parent_dir) = antigravity_path.parent() {
            let portable_db_path = PathBuf::from(parent_dir)
                .join("data")
                .join("user-data")
                .join("User")
                .join("globalStorage")
                .join("state.vscdb");

            if portable_db_path.exists() {
                return Ok(portable_db_path);
            }
        }
    }

    let folder_name = if target_ide == Some("ide") { "Antigravity IDE" } else { "Antigravity" };

    // Standard mode: use system default path
    #[cfg(target_os = "macos")]
    {
        let home = dirs::home_dir().ok_or("Failed to get home directory")?;
        Ok(home.join(format!("Library/Application Support/{}/User/globalStorage/state.vscdb", folder_name)))
    }

    #[cfg(target_os = "windows")]
    {
        let appdata =
            std::env::var("APPDATA").map_err(|_| "Failed to get APPDATA environment variable".to_string())?;
        Ok(PathBuf::from(appdata).join(folder_name).join("User\\globalStorage\\state.vscdb"))
    }

    #[cfg(target_os = "linux")]
    {
        let home = dirs::home_dir().ok_or("Failed to get home directory")?;
        Ok(home.join(format!(".config/{}/User/globalStorage/state.vscdb", folder_name)))
    }
}

/// Inject Token and Email into database
pub fn inject_token(
    db_path: &PathBuf,
    access_token: &str,
    refresh_token: &str,
    expiry: i64,
    email: &str,
    mut is_gcp_tos: bool,
    project_id: Option<&str>,
    id_token: Option<&str>,
    oauth_client_key: Option<&str>,
    target_ide: Option<&str>,
) -> Result<String, String> {
    crate::modules::logger::log_info("Starting Token injection...");
    
    // 如果使用的是本项目的内置 Client ID (antigravity_enterprise 实际上是标准版)
    // 则强制关闭 GCP TOS 标志，以确保 IDE 使用标准 Client ID 进行刷新
    if let Some(key) = oauth_client_key {
        if key == "antigravity_enterprise" {
            if is_gcp_tos {
                crate::modules::logger::log_info("[DB] Built-in client detected, forcing Standard mode for injection.");
                is_gcp_tos = false;
            }
        }
    }
    
    // 1. Detect Antigravity version
    let version_result = crate::modules::version::get_antigravity_version(target_ide);
    
    match version_result {
        Ok(ver) => {
            crate::modules::logger::log_info(&format!(
                "Detected Antigravity version: {}",
                ver.short_version
            ));
            
            // 2. Choose injection strategy based on version
            if crate::modules::version::is_new_version(&ver) {
                // >= 1.16.5: Use new format only
                crate::modules::logger::log_info(
                    "Using new format injection (antigravityUnifiedStateSync.oauthToken)",
                );
                inject_new_format(
                    db_path,
                    access_token,
                    refresh_token,
                    expiry,
                    email,
                    is_gcp_tos,
                    project_id,
                    id_token,
                )
            } else {
                // < 1.16.5: Use old format only
                crate::modules::logger::log_info(
                    "Using old format injection (jetskiStateSync.agentManagerInitState)",
                );
                inject_old_format(db_path, access_token, refresh_token, expiry, email)
            }
        }
        Err(e) => {
            // Cannot detect version: Try both formats (fallback)
            crate::modules::logger::log_warn(&format!(
                "Version detection failed, trying both formats for compatibility: {}",
                e
            ));
            
            // Try new format first
            let new_result = inject_new_format(
                db_path,
                access_token,
                refresh_token,
                expiry,
                email,
                is_gcp_tos,
                project_id,
                id_token,
            );
            
            // Try old format
            let old_result = inject_old_format(db_path, access_token, refresh_token, expiry, email);
            
            // Return success if either format succeeded
            if new_result.is_ok() || old_result.is_ok() {
                Ok("Token injection successful (dual format fallback)".to_string())
            } else {
                Err(format!(
                    "Both formats failed - New: {:?}, Old: {:?}",
                    new_result.err(),
                    old_result.err()
                ))
            }
        }
    }
}

/// New format injection (>= 1.16.5)
fn inject_new_format(
    db_path: &PathBuf,
    access_token: &str,
    refresh_token: &str,
    expiry: i64,
    email: &str,
    is_gcp_tos: bool,
    project_id: Option<&str>,
    id_token: Option<&str>,
) -> Result<String, String> {
    let conn = Connection::open(db_path).map_err(|e| format!("Failed to open database: {}", e))?;
    
    // Create OAuthTokenInfo (binary)
    let oauth_info = protobuf::create_oauth_info(
        access_token,
        refresh_token,
        expiry,
        is_gcp_tos,
        id_token,
        Some(email),
    );
    let outer_b64 = protobuf::create_unified_state_entry("oauthTokenInfoSentinelKey", &oauth_info);
    
    conn.execute(
        "INSERT OR REPLACE INTO ItemTable (key, value) VALUES (?, ?)",
        ["antigravityUnifiedStateSync.oauthToken", &outer_b64],
    )
    .map_err(|e| format!("Failed to write new format: {}", e))?;
    
    inject_user_status(&conn, email)?;

    if let Some(project_id) = project_id.map(str::trim).filter(|pid| !pid.is_empty()) {
        inject_enterprise_project_preference(&conn, project_id)?;
    } else {
        clear_enterprise_project_preference(&conn)?;
    }

    // Inject Onboarding flag
    conn.execute(
        "INSERT OR REPLACE INTO ItemTable (key, value) VALUES (?, ?)",
        ["antigravityOnboarding", "true"],
    )
    .map_err(|e| format!("Failed to write onboarding flag: {}", e))?;
    
    Ok("Token injection successful (new format)".to_string())
}

fn inject_user_status(conn: &Connection, email: &str) -> Result<(), String> {
    let payload = protobuf::create_minimal_user_status_payload(email);
    let entry_b64 = protobuf::create_unified_state_entry("userStatusSentinelKey", &payload);

    conn.execute(
        "INSERT OR REPLACE INTO ItemTable (key, value) VALUES (?, ?)",
        ["antigravityUnifiedStateSync.userStatus", &entry_b64],
    )
    .map_err(|e| format!("Failed to write user status: {}", e))?;

    Ok(())
}

fn inject_enterprise_project_preference(conn: &Connection, project_id: &str) -> Result<(), String> {
    let payload = protobuf::create_string_value_payload(project_id);
    let entry_b64 = protobuf::create_unified_state_entry("enterpriseGcpProjectId", &payload);

    conn.execute(
        "INSERT OR REPLACE INTO ItemTable (key, value) VALUES (?, ?)",
        [
            "antigravityUnifiedStateSync.enterprisePreferences",
            &entry_b64,
        ],
    )
    .map_err(|e| format!("Failed to write enterprise preferences: {}", e))?;

    Ok(())
}

fn clear_enterprise_project_preference(conn: &Connection) -> Result<(), String> {
    conn.execute(
        "DELETE FROM ItemTable WHERE key = ?",
        ["antigravityUnifiedStateSync.enterprisePreferences"],
    )
    .map_err(|e| format!("Failed to clear enterprise preferences: {}", e))?;

    Ok(())
}

/// Old format injection (< 1.16.5)
fn inject_old_format(
    db_path: &PathBuf,
    access_token: &str,
    refresh_token: &str,
    expiry: i64,
    email: &str,
) -> Result<String, String> {
    use base64::{engine::general_purpose, Engine as _};
    use rusqlite::Error as SqliteError;
    
    let conn = Connection::open(db_path)
        .map_err(|e| format!("Failed to open database: {}", e))?;
    
    // Read current data
    let current_data: String = conn
        .query_row(
            "SELECT value FROM ItemTable WHERE key = ?",
            ["jetskiStateSync.agentManagerInitState"],
            |row| row.get(0),
        )
        .map_err(|e| match e {
            SqliteError::QueryReturnedNoRows => {
                "Old format key does not exist, possibly new version Antigravity".to_string()
            }
            _ => format!("Failed to read data: {}", e),
        })?;
    
    // Base64 decode
    let blob = general_purpose::STANDARD
        .decode(&current_data)
        .map_err(|e| format!("Base64 decoding failed: {}", e))?;
    
    // Remove old fields
    let mut clean_data = protobuf::remove_field(&blob, 1)?; // UserID
    clean_data = protobuf::remove_field(&clean_data, 2)?;   // Email
    clean_data = protobuf::remove_field(&clean_data, 6)?;   // OAuthTokenInfo
    
    // Create new fields
    let new_email_field = protobuf::create_email_field(email);
    let new_oauth_field = protobuf::create_oauth_field(access_token, refresh_token, expiry);
    
    // Merge data
    // We intentionally do NOT re-inject Field 1 (UserID) to force the client 
    // to re-authenticate the session with the new token.
    let final_data = [clean_data, new_email_field, new_oauth_field].concat();
    let final_b64 = general_purpose::STANDARD.encode(&final_data);
    
    // Write to database
    conn.execute(
        "UPDATE ItemTable SET value = ? WHERE key = ?",
        [&final_b64, "jetskiStateSync.agentManagerInitState"],
    )
    .map_err(|e| format!("Failed to write data: {}", e))?;
    
    // Inject Onboarding flag
    conn.execute(
        "INSERT OR REPLACE INTO ItemTable (key, value) VALUES (?, ?)",
        ["antigravityOnboarding", "true"],
    )
    .map_err(|e| format!("Failed to write onboarding flag: {}", e))?;
    
    Ok("Token injection successful (old format)".to_string())
}

/// 注入 Service Machine ID 到数据库，解决 VS Code 缓存指纹不匹配导致 Token 失效的问题
pub fn write_service_machine_id(db_path: &std::path::Path, service_machine_id: &str) -> Result<(), String> {
    let conn = Connection::open(db_path).map_err(|e| format!("Failed to open database: {}", e))?;
    
    conn.execute(
        "INSERT OR REPLACE INTO ItemTable (key, value) VALUES (?, ?)",
        ["telemetry.serviceMachineId", service_machine_id],
    )
    .map_err(|e| format!("Failed to write serviceMachineId: {}", e))?;

    crate::modules::logger::log_info(&format!(
        "Successfully injected serviceMachineId: {}",
        service_machine_id
    ));
    
    Ok(())
}
