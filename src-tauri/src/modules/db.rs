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

/// Get all possible Antigravity database candidate paths
pub fn get_all_candidate_db_paths(target_ide: Option<&str>) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Some(user_data_dir) = crate::modules::process::get_user_data_dir_from_process(target_ide)
    {
        paths.push(
            user_data_dir
                .join("User")
                .join("globalStorage")
                .join("state.vscdb"),
        );
    }

    if let Some(antigravity_path) = get_antigravity_path(target_ide) {
        if let Some(parent_dir) = antigravity_path.parent() {
            paths.push(
                PathBuf::from(parent_dir)
                    .join("data")
                    .join("user-data")
                    .join("User")
                    .join("globalStorage")
                    .join("state.vscdb"),
            );
        }
    }

    let folder_names: &[&str] = if target_ide == Some("ide") {
        &["Antigravity IDE", "Antigravity"]
    } else if target_ide == Some("code") || target_ide == Some("cursor") {
        &["Antigravity", "Antigravity IDE"]
    } else if target_ide == Some("classic") {
        &["Antigravity"]
    } else {
        &["Antigravity", "Antigravity IDE"]
    };

    #[cfg(target_os = "macos")]
    if let Some(home) = dirs::home_dir() {
        for folder_name in folder_names {
            paths.push(home.join(format!(
                "Library/Application Support/{}/User/globalStorage/state.vscdb",
                folder_name
            )));
        }
    }

    #[cfg(target_os = "windows")]
    if let Ok(appdata) = std::env::var("APPDATA") {
        for folder_name in folder_names {
            paths.push(
                PathBuf::from(&appdata)
                    .join(folder_name)
                    .join("User\\globalStorage\\state.vscdb"),
            );
        }
    }

    #[cfg(target_os = "linux")]
    if let Some(home) = dirs::home_dir() {
        for folder_name in folder_names {
            paths.push(home.join(format!(
                ".config/{}/User/globalStorage/state.vscdb",
                folder_name
            )));
        }
    }

    paths
}

/// Get Antigravity database path (cross-platform)
pub fn get_db_path(target_ide: Option<&str>) -> Result<PathBuf, String> {
    let candidates = get_all_candidate_db_paths(target_ide);
    for path in &candidates {
        if path.exists() {
            return Ok(path.clone());
        }
    }
    candidates
        .into_iter()
        .next()
        .ok_or_else(|| "Failed to locate database path".to_string())
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
                crate::modules::logger::log_info(
                    "[DB] Built-in client detected, forcing Standard mode for injection.",
                );
                is_gcp_tos = false;
            }
        }
    }

    crate::modules::logger::log_info(
        "Skipping version detection, using new format injection directly (antigravityUnifiedStateSync.oauthToken)",
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

    // 忙等待：热切号场景下 Antigravity 仍在运行，数据库中可能存在并发写者；
    // 显式设置 busy_timeout，让 SQLite 在瞬时锁竞争时等待重试，而不是立刻抛 SQLITE_BUSY
    // （rusqlite 默认 busy_timeout = 0，写入会被瞬时锁直接拒绝）。
    conn.busy_timeout(std::time::Duration::from_millis(2000))
        .map_err(|e| format!("Failed to set busy_timeout: {}", e))?;

    // Create OAuthTokenInfo (binary)
    let oauth_info = protobuf::create_oauth_info(
        access_token,
        refresh_token,
        expiry,
        is_gcp_tos,
        id_token,
        Some(email),
    );

    use base64::{engine::general_purpose, Engine as _};
    use rusqlite::OptionalExtension;

    let current_topic = conn
        .query_row(
            "SELECT value FROM ItemTable WHERE key = ?",
            ["antigravityUnifiedStateSync.oauthToken"],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|e| format!("Failed to read oauthToken: {}", e))?
        .map(|val| general_purpose::STANDARD.decode(val).unwrap_or_default())
        .unwrap_or_default();

    let mut topic =
        protobuf::remove_unified_topic_entry(&current_topic, "oauthTokenInfoSentinelKey")?;
    topic.extend(protobuf::create_unified_topic_entry(
        "oauthTokenInfoSentinelKey",
        &oauth_info,
    ));

    let topic_b64 = general_purpose::STANDARD.encode(&topic);

    conn.execute(
        "INSERT OR REPLACE INTO ItemTable (key, value) VALUES (?, ?)",
        ["antigravityUnifiedStateSync.oauthToken", &topic_b64],
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

    // Fix for missing history: Delete the old format state to prevent the IDE from reading a stale UserID
    // which causes history fetching to fail.
    let _ = conn.execute(
        "DELETE FROM ItemTable WHERE key = ?",
        ["jetskiStateSync.agentManagerInitState"],
    );

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

/// 注入 Service Machine ID 到数据库，解决 VS Code 缓存指纹不匹配导致 Token 失效的问题
pub fn write_service_machine_id(
    db_path: &std::path::Path,
    service_machine_id: &str,
) -> Result<(), String> {
    let conn = Connection::open(db_path).map_err(|e| format!("Failed to open database: {}", e))?;

    // 同 `inject_new_format`：热切号时应用仍在运行，忙等待避免瞬时锁竞争导致写入失败
    conn.busy_timeout(std::time::Duration::from_millis(2000))
        .map_err(|e| format!("Failed to set busy_timeout: {}", e))?;

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
