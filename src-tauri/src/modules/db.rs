use crate::utils::protobuf;
use base64::{engine::general_purpose, Engine as _};
use rusqlite::Connection;
use std::path::PathBuf;

fn get_antigravity_path() -> Option<PathBuf> {
    if let Ok(config) = crate::modules::config::load_app_config() {
        if let Some(path_str) = config.antigravity_executable {
            let path = PathBuf::from(path_str);
            if path.exists() {
                return Some(path);
            }
        }
    }
    crate::modules::process::get_antigravity_executable_path()
}

/// Get Antigravity database path (cross-platform)
pub fn get_db_path() -> Result<PathBuf, String> {
    // Prefer path specified by --user-data-dir argument
    if let Some(user_data_dir) = crate::modules::process::get_user_data_dir_from_process() {
        let custom_db_path = user_data_dir.join("User").join("globalStorage").join("state.vscdb");
        if custom_db_path.exists() {
            return Ok(custom_db_path);
        }
    }

    // Check if in portable mode
    if let Some(antigravity_path) = get_antigravity_path() {
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

    // Standard mode: use system default path
    #[cfg(target_os = "macos")]
    {
        let home = dirs::home_dir().ok_or("Failed to get home directory")?;
        Ok(home.join("Library/Application Support/Antigravity/User/globalStorage/state.vscdb"))
    }

    #[cfg(target_os = "windows")]
    {
        let appdata =
            std::env::var("APPDATA").map_err(|_| "Failed to get APPDATA environment variable".to_string())?;
        Ok(PathBuf::from(appdata).join("Antigravity\\User\\globalStorage\\state.vscdb"))
    }

    #[cfg(target_os = "linux")]
    {
        let home = dirs::home_dir().ok_or("Failed to get home directory")?;
        Ok(home.join(".config/Antigravity/User/globalStorage/state.vscdb"))
    }
}

/// Inject Token and Email into database
pub fn inject_token(
    db_path: &PathBuf,
    access_token: &str,
    refresh_token: &str,
    expiry: i64,
    email: &str,
) -> Result<String, String> {
    crate::modules::logger::log_info("Starting Token injection...");
    
    // 1. Detect Antigravity version
    let version_result = crate::modules::version::get_antigravity_version();
    
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
                    "Using new format injection (antigravityUnifiedStateSync.oauthToken)"
                );
                inject_new_format(db_path, access_token, refresh_token, expiry)
            } else {
                // < 1.16.5: Use old format only
                crate::modules::logger::log_info(
                    "Using old format injection (jetskiStateSync.agentManagerInitState)"
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
            let new_result = inject_new_format(db_path, access_token, refresh_token, expiry);
            
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
) -> Result<String, String> {
    let conn = Connection::open(db_path)
        .map_err(|e| format!("Failed to open database: {}", e))?;
    
    // Create OAuthTokenInfo (binary)
    let oauth_info = protobuf::create_oauth_info(access_token, refresh_token, expiry);
    let oauth_info_b64 = general_purpose::STANDARD.encode(&oauth_info);
    
    // InnerMessage2: field 1 = base64(oauth_info)
    let inner2 = protobuf::encode_string_field(1, &oauth_info_b64);
    
    // InnerMessage: field 1 = sentinel key, field 2 = inner2
    let inner1 = protobuf::encode_string_field(1, "oauthTokenInfoSentinelKey");
    let inner = [inner1, protobuf::encode_len_delim_field(2, &inner2)].concat();
    
    // OuterMessage: field 1 = inner
    let outer = protobuf::encode_len_delim_field(1, &inner);
    let outer_b64 = general_purpose::STANDARD.encode(&outer);
    
    conn.execute(
        "INSERT OR REPLACE INTO ItemTable (key, value) VALUES (?, ?)",
        ["antigravityUnifiedStateSync.oauthToken", &outer_b64],
    )
    .map_err(|e| format!("Failed to write new format: {}", e))?;
    
    // Inject Onboarding flag
    conn.execute(
        "INSERT OR REPLACE INTO ItemTable (key, value) VALUES (?, ?)",
        ["antigravityOnboarding", "true"],
    )
    .map_err(|e| format!("Failed to write onboarding flag: {}", e))?;
    
    Ok("Token injection successful (new format)".to_string())
}

/// Old format injection (< 1.16.5)
fn inject_old_format(
    db_path: &PathBuf,
    access_token: &str,
    refresh_token: &str,
    expiry: i64,
    email: &str,
) -> Result<String, String> {
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
