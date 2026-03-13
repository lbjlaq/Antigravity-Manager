use crate::models::{Account, TokenData};
use crate::modules::{account, db};
use crate::utils::protobuf;
use base64::{engine::general_purpose, Engine as _};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone)]
struct ImportedOAuthState {
    refresh_token: String,
    is_gcp_tos: bool,
    project_id: Option<String>,
}

/// Scan and import V1 data
pub async fn import_from_v1() -> Result<Vec<Account>, String> {
    use crate::modules::oauth;

    let home = dirs::home_dir().ok_or("Failed to get home directory")?;

    // V1 data directory (confirmed cross-platform consistency from utils.py)
    let v1_dir = home.join(".antigravity-agent");

    let mut imported_accounts = Vec::new();

    // Try multiple possible filenames
    let index_files = vec![
        "antigravity_accounts.json", // Directly use string literal
        "accounts.json",
    ];

    let mut found_index = false;

    for index_filename in index_files {
        let v1_accounts_path = v1_dir.join(index_filename);

        if !v1_accounts_path.exists() {
            continue;
        }

        found_index = true;
        crate::modules::logger::log_info(&format!("V1 data discovered: {:?}", v1_accounts_path));

        let content = match fs::read_to_string(&v1_accounts_path) {
            Ok(c) => c,
            Err(e) => {
                crate::modules::logger::log_warn(&format!("Failed to read index: {}", e));
                continue;
            }
        };

        let v1_index: Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(e) => {
                crate::modules::logger::log_warn(&format!("Failed to parse index JSON: {}", e));
                continue;
            }
        };

        // Compatible with two formats: direct map, or contains "accounts" field
        let accounts_map = if let Some(map) = v1_index.as_object() {
            if let Some(accounts) = map.get("accounts").and_then(|v| v.as_object()) {
                accounts
            } else {
                map
            }
        } else {
            continue;
        };

        for (id, acc_info) in accounts_map {
            let email_placeholder = acc_info
                .get("email")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown")
                .to_string();

            // Skip non-account keys (e.g. "current_account_id")
            if !acc_info.is_object() {
                continue;
            }

            let backup_file_str = acc_info.get("backup_file").and_then(|v| v.as_str());
            let data_file_str = acc_info.get("data_file").and_then(|v| v.as_str());

            // Prefer backup_file, then data_file
            let target_file = backup_file_str.or(data_file_str);

            if target_file.is_none() {
                crate::modules::logger::log_warn(&format!(
                    "Account {} ({}) missing data file path",
                    id, email_placeholder
                ));
                continue;
            }

            let mut backup_path = PathBuf::from(target_file.unwrap());

            // If relative path, try joining with v1_dir
            if !backup_path.exists() {
                backup_path = v1_dir.join(backup_path.file_name().unwrap_or_default());
            }

            // Try joining data/ or backups/ subdirectories again
            if !backup_path.exists() {
                let file_name = backup_path.file_name().unwrap_or_default();
                let try_backups = v1_dir.join("backups").join(file_name);
                if try_backups.exists() {
                    backup_path = try_backups;
                } else {
                    let try_accounts = v1_dir.join("accounts").join(file_name);
                    if try_accounts.exists() {
                        backup_path = try_accounts;
                    }
                }
            }

            if !backup_path.exists() {
                crate::modules::logger::log_warn(&format!(
                    "Account {} ({}) backup file not found: {:?}",
                    id, email_placeholder, backup_path
                ));
                continue;
            }

            // Read backup file
            if let Ok(backup_content) = fs::read_to_string(&backup_path) {
                if let Ok(backup_json) = serde_json::from_str::<Value>(&backup_content) {
                    // Compatible with two formats:
                    // 1. V1 backup: jetskiStateSync.agentManagerInitState -> Protobuf
                    // 2. V2/Script data: JSON containing "token" field

                    let mut refresh_token_opt = None;

                    // Try format 2
                    if let Some(token_data) = backup_json.get("token") {
                        if let Some(rt) = token_data.get("refresh_token").and_then(|v| v.as_str()) {
                            refresh_token_opt = Some(rt.to_string());
                        }
                    }

                    // Try format 1
                    if refresh_token_opt.is_none() {
                        if let Some(state_b64) = backup_json
                            .get("jetskiStateSync.agentManagerInitState")
                            .and_then(|v| v.as_str())
                        {
                            // Parse Protobuf
                            if let Ok(blob) = general_purpose::STANDARD.decode(state_b64) {
                                if let Ok(Some(oauth_data)) = protobuf::find_field(&blob, 6) {
                                    if let Ok(Some(refresh_bytes)) =
                                        protobuf::find_field(&oauth_data, 3)
                                    {
                                        if let Ok(rt) = String::from_utf8(refresh_bytes) {
                                            refresh_token_opt = Some(rt);
                                        }
                                    }
                                }
                            }
                        }
                    }

                    if let Some(refresh_token) = refresh_token_opt {
                        crate::modules::logger::log_info(&format!(
                            "Importing account: {}",
                            email_placeholder
                        ));
                        let (email, access_token, expires_in) =
                            match oauth::refresh_access_token(&refresh_token, None).await {
                                Ok(token_resp) => {
                                    match oauth::get_user_info(&token_resp.access_token, None).await
                                    {
                                        Ok(user_info) => (
                                            user_info.email,
                                            token_resp.access_token,
                                            token_resp.expires_in,
                                        ),
                                        Err(_) => (
                                            email_placeholder.clone(),
                                            token_resp.access_token,
                                            token_resp.expires_in,
                                        ),
                                    }
                                }
                                Err(e) => {
                                    crate::modules::logger::log_warn(&format!(
                                        "Token refresh failed (likely expired): {}",
                                        e
                                    ));
                                    (
                                        email_placeholder.clone(),
                                        "imported_access_token".to_string(),
                                        0,
                                    )
                                }
                            };

                        let token_data = TokenData::new(
                            access_token,
                            refresh_token,
                            expires_in,
                            Some(email.clone()),
                            None, // project_id will be fetched on demand
                            None, // session_id
                            true, // V1 tokens are Antigravity Google OAuth tokens
                        );

                        // Name already fetched in get_user_info at line 153, but outside match scope, use None to be safe
                        match account::upsert_account(email.clone(), None, token_data) {
                            Ok(acc) => {
                                crate::modules::logger::log_info(&format!(
                                    "Import successful: {}",
                                    email
                                ));
                                imported_accounts.push(acc);
                            }
                            Err(e) => crate::modules::logger::log_error(&format!(
                                "Import save failed {}: {}",
                                email, e
                            )),
                        }
                    } else {
                        crate::modules::logger::log_warn(&format!(
                            "Account {} data file missing Refresh Token",
                            email_placeholder
                        ));
                    }
                }
            }
        }
    }

    if !found_index {
        return Err("V1 account data file not found".to_string());
    }

    Ok(imported_accounts)
}

/// Import account from custom database path
pub async fn import_from_custom_db_path(path_str: String) -> Result<Account, String> {
    use crate::modules::oauth;

    let path = PathBuf::from(path_str);
    if !path.exists() {
        return Err(format!("File does not exist: {:?}", path));
    }

    let oauth_state = extract_oauth_state_from_file(&path)?;
    let refresh_token = oauth_state.refresh_token.clone();

    // 3. Use Refresh Token to get latest Access Token and user info
    crate::modules::logger::log_info("Getting user info using Refresh Token...");
    let token_resp = oauth::refresh_access_token(&refresh_token, None).await?;
    let user_info = oauth::get_user_info(&token_resp.access_token, None).await?;

    let email = user_info.email;

    crate::modules::logger::log_info(&format!("Successfully retrieved account info: {}", email));

    let token_data = TokenData::new(
        token_resp.access_token,
        refresh_token,
        token_resp.expires_in,
        Some(email.clone()),
        oauth_state.project_id,
        None, // session_id will be generated in token_manager
        oauth_state.is_gcp_tos,
    );

    // 4. Add or update account
    account::upsert_account(email.clone(), user_info.name, token_data)
}

/// Import current logged-in account from default IDE database
pub async fn import_from_db() -> Result<Account, String> {
    let db_path = db::get_db_path()?;
    import_from_custom_db_path(db_path.to_string_lossy().to_string()).await
}

/// Get current Refresh Token from database (common logic)
pub fn extract_refresh_token_from_file(db_path: &PathBuf) -> Result<String, String> {
    extract_oauth_state_from_file(db_path).map(|state| state.refresh_token)
}

fn extract_enterprise_project_id_from_conn(
    conn: &rusqlite::Connection,
) -> Result<Option<String>, String> {
    let entry_b64: Option<String> = conn
        .query_row(
            "SELECT value FROM ItemTable WHERE key = ?",
            ["antigravityUnifiedStateSync.enterprisePreferences"],
            |row| row.get(0),
        )
        .ok();

    let Some(entry_b64) = entry_b64 else {
        return Ok(None);
    };

    let (sentinel_key, payload) = protobuf::decode_unified_state_entry(&entry_b64)?;
    if sentinel_key != "enterpriseGcpProjectId" {
        return Ok(None);
    }

    let Some(project_bytes) = protobuf::find_field(&payload, 3)? else {
        return Ok(None);
    };

    let project_id = String::from_utf8(project_bytes)
        .map_err(|_| "enterpriseGcpProjectId is not UTF-8 encoded".to_string())?;
    if project_id.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(project_id))
    }
}

fn extract_oauth_state_from_file(db_path: &PathBuf) -> Result<ImportedOAuthState, String> {
    use base64::{engine::general_purpose, Engine as _};

    if !db_path.exists() {
        return Err(format!("Database file not found: {:?}", db_path));
    }

    // Connect to database
    let conn = rusqlite::Connection::open(db_path)
        .map_err(|e| format!("Failed to open database: {}", e))?;

    // 1. 尝试新版格式 (>= 1.16.5)
    // 键: antigravityUnifiedStateSync.oauthToken
    // 结构: Outer(F1) -> Inner(F2) -> Inner2(F1) -> Base64 -> OAuthInfo
    let new_format_data: Option<String> = conn
        .query_row(
            "SELECT value FROM ItemTable WHERE key = ?",
            ["antigravityUnifiedStateSync.oauthToken"],
            |row| row.get(0),
        )
        .ok();

    if let Some(outer_b64) = new_format_data {
        crate::modules::logger::log_info(
            "Detected new format database (antigravityUnifiedStateSync.oauthToken)",
        );
        let (sentinel_key, oauth_info_blob) = protobuf::decode_unified_state_entry(&outer_b64)?;
        if sentinel_key != "oauthTokenInfoSentinelKey" {
            return Err(format!("Unexpected OAuth sentinel key: {}", sentinel_key));
        }

        // 解析 OAuthInfo (Field 3) -> Refresh Token
        let refresh_bytes = protobuf::find_field(&oauth_info_blob, 3)
            .map_err(|e| format!("Parsing OAuthInfo Field 3 failed: {}", e))?
            .ok_or("Refresh Token not found in OAuthInfo (Field 3)")?;

        let refresh_token = String::from_utf8(refresh_bytes)
            .map_err(|_| "Refresh Token is not UTF-8 encoded".to_string())?;
        let is_gcp_tos = protobuf::find_varint_field(&oauth_info_blob, 6)?.unwrap_or(1) != 0;
        let project_id = extract_enterprise_project_id_from_conn(&conn)?;

        return Ok(ImportedOAuthState {
            refresh_token,
            is_gcp_tos,
            project_id,
        });
    }

    // 2. 尝试旧版格式 (< 1.16.5)
    crate::modules::logger::log_info(
        "Falling back to old format database (jetskiStateSync.agentManagerInitState)",
    );
    let current_data: String = conn
        .query_row(
            "SELECT value FROM ItemTable WHERE key = ?",
            ["jetskiStateSync.agentManagerInitState"],
            |row| row.get(0),
        )
        .map_err(|_| "Login state data not found in either format".to_string())?;

    // Base64 decode
    let blob = general_purpose::STANDARD
        .decode(&current_data)
        .map_err(|e| format!("Base64 decoding failed: {}", e))?;

    // 1. Find oauthTokenInfo (Field 6)
    let oauth_data = protobuf::find_field(&blob, 6)
        .map_err(|e| format!("Protobuf parsing failed: {}", e))?
        .ok_or("OAuth data not found (Field 6)")?;

    // 2. Extract refresh_token (Field 3)
    let refresh_bytes = protobuf::find_field(&oauth_data, 3)
        .map_err(|e| format!("OAuth data parsing failed: {}", e))?
        .ok_or("Refresh Token not included in data (Field 3)")?;

    let refresh_token = String::from_utf8(refresh_bytes)
        .map_err(|_| "Refresh Token is not UTF-8 encoded".to_string())?;

    Ok(ImportedOAuthState {
        refresh_token,
        is_gcp_tos: true,
        project_id: extract_enterprise_project_id_from_conn(&conn)?,
    })
}

/// Get current Refresh Token from default database (backwards compatibility)
pub fn get_refresh_token_from_db() -> Result<String, String> {
    let db_path = db::get_db_path()?;
    extract_refresh_token_from_file(&db_path)
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn write_new_format_state(
        db_path: &PathBuf,
        refresh_token: &str,
        is_gcp_tos_field_present: bool,
        project_id: &str,
    ) {
        let conn = Connection::open(db_path).expect("failed to open sqlite db");
        let oauth_payload = protobuf::create_oauth_info(
            "access-token",
            refresh_token,
            1_700_000_000,
            is_gcp_tos_field_present,
        );
        let oauth_entry =
            protobuf::create_unified_state_entry("oauthTokenInfoSentinelKey", &oauth_payload);
        conn.execute(
            "INSERT INTO ItemTable (key, value) VALUES (?, ?)",
            ["antigravityUnifiedStateSync.oauthToken", &oauth_entry],
        )
        .expect("failed to insert oauth entry");

        let project_payload = protobuf::create_string_value_payload(project_id);
        let project_entry =
            protobuf::create_unified_state_entry("enterpriseGcpProjectId", &project_payload);
        conn.execute(
            "INSERT INTO ItemTable (key, value) VALUES (?, ?)",
            [
                "antigravityUnifiedStateSync.enterprisePreferences",
                &project_entry,
            ],
        )
        .expect("failed to insert project entry");
    }

    #[test]
    fn extract_oauth_state_reads_project_and_gcp_tos_from_new_format() {
        let db = TestDbPath::new("extract_oauth_state_reads_project_and_gcp_tos_from_new_format");
        write_new_format_state(&db.path, "refresh-token", true, "intense-age-490103-c3");

        let state = extract_oauth_state_from_file(&db.path).expect("failed to extract oauth state");

        assert_eq!(state.refresh_token, "refresh-token");
        assert!(state.is_gcp_tos);
        assert_eq!(state.project_id.as_deref(), Some("intense-age-490103-c3"));
    }

    #[test]
    fn extract_oauth_state_defaults_gcp_tos_when_field_is_missing() {
        let db = TestDbPath::new("extract_oauth_state_defaults_gcp_tos_when_field_is_missing");
        write_new_format_state(
            &db.path,
            "refresh-token",
            false,
            "project-5be25e6a-00f4-408d-b71",
        );

        let state = extract_oauth_state_from_file(&db.path).expect("failed to extract oauth state");

        assert_eq!(state.refresh_token, "refresh-token");
        assert!(state.is_gcp_tos);
        assert_eq!(
            state.project_id.as_deref(),
            Some("project-5be25e6a-00f4-408d-b71")
        );
    }
}
