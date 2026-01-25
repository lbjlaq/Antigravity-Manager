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

/// Inject Token into database
pub fn inject_token(
    db_path: &PathBuf,
    access_token: &str,
    refresh_token: &str,
    expiry: i64,
) -> Result<String, String> {
    // 1. Open database
    let conn = Connection::open(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    // Ensure table exists for fresh installs
    conn.execute(
        "CREATE TABLE IF NOT EXISTS ItemTable (key TEXT PRIMARY KEY, value TEXT);",
        [],
    )
    .map_err(|e| format!("Failed to create ItemTable: {}", e))?;

    // 2. Read current data
    let current_data_result: Result<String, _> = conn.query_row(
        "SELECT value FROM ItemTable WHERE key = ?",
        ["jetskiStateSync.agentManagerInitState"],
        |row| row.get(0),
    );

    // 3. Handle data (decode if exists, else empty)
    let blob = match current_data_result {
        Ok(data) => general_purpose::STANDARD
            .decode(&data)
            .map_err(|e| format!("Base64 decoding failed: {}", e))?,
        Err(rusqlite::Error::QueryReturnedNoRows) => Vec::new(),
        Err(e) => return Err(format!("Failed to read data: {}", e)),
    };

    // 4. Remove old Field 6
    let clean_data = if blob.is_empty() {
        Vec::new()
    } else {
        protobuf::remove_field(&blob, 6)?
    };

    // 5. Create new Field 6
    let new_field = protobuf::create_oauth_field(access_token, refresh_token, expiry);

    // 6. Merge data
    let final_data = [clean_data, new_field].concat();
    let final_b64 = general_purpose::STANDARD.encode(&final_data);

    // 7. Write to database
    conn.execute(
        "UPDATE ItemTable SET value = ? WHERE key = ?",
        [&final_b64, "jetskiStateSync.agentManagerInitState"],
    )
    .map_err(|e| format!("Failed to write data: {}", e))?;

    // 8. Inject Onboarding flag
    let onboarding_key = "antigravityOnboarding";
    conn.execute(
        "INSERT OR REPLACE INTO ItemTable (key, value) VALUES (?, ?)",
        [onboarding_key, "true"],
    )
    .map_err(|e| format!("Failed to write Onboarding flag: {}", e))?;

    Ok(format!("Token injection successful!\nDatabase: {:?}", db_path))
}
