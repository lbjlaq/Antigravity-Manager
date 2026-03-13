use crate::utils::protobuf;
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
        let custom_db_path = user_data_dir
            .join("User")
            .join("globalStorage")
            .join("state.vscdb");
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
        let appdata = std::env::var("APPDATA")
            .map_err(|_| "Failed to get APPDATA environment variable".to_string())?;
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
    is_gcp_tos: bool,
    project_id: Option<&str>,
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
) -> Result<String, String> {
    let conn = Connection::open(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    // Create OAuthTokenInfo (binary)
    let oauth_info = protobuf::create_oauth_info(access_token, refresh_token, expiry, is_gcp_tos);
    let outer_b64 = protobuf::create_unified_state_entry("oauthTokenInfoSentinelKey", &oauth_info);

    conn.execute(
        "INSERT OR REPLACE INTO ItemTable (key, value) VALUES (?, ?)",
        ["antigravityUnifiedStateSync.oauthToken", &outer_b64],
    )
    .map_err(|e| format!("Failed to write new format: {}", e))?;

    inject_user_status(&conn, email)?;

    if let Some(project_id) = project_id.filter(|pid| !pid.trim().is_empty()) {
        inject_enterprise_project_preference(&conn, project_id)?;
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

    let conn = Connection::open(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

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
    clean_data = protobuf::remove_field(&clean_data, 2)?; // Email
    clean_data = protobuf::remove_field(&clean_data, 6)?; // OAuthTokenInfo

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::protobuf;
    use rusqlite::Connection;
    use std::fs;
    use std::path::PathBuf;

    struct TestDbPath {
        path: PathBuf,
    }

    impl TestDbPath {
        fn new(name: &str) -> Self {
            let unique = format!(
                "{}_{}_{}",
                name,
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            );
            let path = std::env::temp_dir().join(format!("{unique}.sqlite3"));
            let conn = Connection::open(&path).expect("failed to create sqlite db");
            conn.execute(
                "CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
                [],
            )
            .expect("failed to create ItemTable");
            drop(conn);
            Self { path }
        }
    }

    impl Drop for TestDbPath {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
        }
    }

    fn read_item_table_value(db_path: &PathBuf, key: &str) -> String {
        let conn = Connection::open(db_path).expect("failed to open sqlite db");
        conn.query_row("SELECT value FROM ItemTable WHERE key = ?", [key], |row| {
            row.get(0)
        })
        .expect("missing ItemTable value")
    }

    #[test]
    fn inject_new_format_writes_is_gcp_tos_flag() {
        let db = TestDbPath::new("inject_new_format_writes_is_gcp_tos_flag");

        inject_new_format(
            &db.path,
            "access-token",
            "refresh-token",
            1_700_000_000,
            "user@example.com",
            true,
            None,
        )
        .expect("inject_new_format should succeed");

        let oauth_entry = read_item_table_value(&db.path, "antigravityUnifiedStateSync.oauthToken");
        let (sentinel, oauth_payload) = protobuf::decode_unified_state_entry(&oauth_entry)
            .expect("failed to decode oauth entry");

        assert_eq!(sentinel, "oauthTokenInfoSentinelKey");
        assert_eq!(
            protobuf::find_varint_field(&oauth_payload, 6).expect("failed to parse oauth payload"),
            Some(1)
        );
    }

    #[test]
    fn inject_new_format_writes_enterprise_project_preference() {
        let db = TestDbPath::new("inject_new_format_writes_enterprise_project_preference");
        let project_id = "intense-age-490103-c3";

        inject_new_format(
            &db.path,
            "access-token",
            "refresh-token",
            1_700_000_000,
            "user@example.com",
            true,
            Some(project_id),
        )
        .expect("inject_new_format should succeed");

        let project_entry = read_item_table_value(
            &db.path,
            "antigravityUnifiedStateSync.enterprisePreferences",
        );
        let (sentinel, project_payload) = protobuf::decode_unified_state_entry(&project_entry)
            .expect("failed to decode project entry");

        assert_eq!(sentinel, "enterpriseGcpProjectId");
        let stored_project = String::from_utf8(
            protobuf::find_field(&project_payload, 3)
                .expect("failed to parse string value field")
                .expect("missing string value field"),
        )
        .expect("project id should be utf-8");
        assert_eq!(stored_project, project_id);
    }

    #[test]
    fn inject_new_format_writes_minimal_user_status() {
        let db = TestDbPath::new("inject_new_format_writes_minimal_user_status");
        let email = "suozzilinander@gmail.com";

        inject_new_format(
            &db.path,
            "access-token",
            "refresh-token",
            1_700_000_000,
            email,
            true,
            None,
        )
        .expect("inject_new_format should succeed");

        let user_status_entry =
            read_item_table_value(&db.path, "antigravityUnifiedStateSync.userStatus");
        let (sentinel, user_status_payload) = protobuf::decode_unified_state_entry(
            &user_status_entry,
        )
        .expect("failed to decode user status entry");

        assert_eq!(sentinel, "userStatusSentinelKey");
        let stored_name = String::from_utf8(
            protobuf::find_field(&user_status_payload, 3)
                .expect("failed to parse user status name field")
                .expect("missing user status name field"),
        )
        .expect("user status name should be utf-8");
        let stored_email = String::from_utf8(
            protobuf::find_field(&user_status_payload, 7)
                .expect("failed to parse user status email field")
                .expect("missing user status email field"),
        )
        .expect("user status email should be utf-8");

        assert_eq!(stored_name, email);
        assert_eq!(stored_email, email);
    }
}
