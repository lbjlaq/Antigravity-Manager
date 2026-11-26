use super::{disclaimer, protobuf, SwitchEngine};
use anyhow::Result;
use rusqlite::Connection;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use sysinfo::System;

impl SwitchEngine {
    /// 返回宿主目录名候选（兼容 Linux 大小写敏感、Windows/macOS 不同打包命名）
    fn host_folder_candidates(app_folder: &str) -> Vec<&str> {
        let mut names = vec![app_folder];
        let defaults: &[&str] = if app_folder.contains("IDE") {
            &[
                "Antigravity IDE",
                "antigravity-ide",
                "antigravity_ide",
                "AntigravityIDE",
            ]
        } else {
            &["Antigravity", "antigravity"]
        };
        for name in defaults {
            if !names.contains(name) {
                names.push(name);
            }
        }
        names
    }

    /// 解析指定应用在不同操作系统的配置存储根目录（优先尊重 Settings 自定义路径，自动兼容 Win/Linux/macOS 目录命名变体）
    pub fn resolve_app_user_storage_dir(app_folder: &str) -> Option<PathBuf> {
        let is_ide = app_folder.contains("IDE");
        let cfg = crate::config::AppConfig::load();
        let custom = if is_ide {
            cfg.custom_paths.ide.as_deref()
        } else {
            cfg.custom_paths.app.as_deref()
        };
        if let Some(custom_dir) = custom.map(str::trim).filter(|s| !s.is_empty()) {
            return Some(PathBuf::from(custom_dir));
        }

        let base_dir: PathBuf = {
            #[cfg(target_os = "macos")]
            {
                dirs::home_dir()?.join("Library/Application Support")
            }
            #[cfg(target_os = "windows")]
            {
                dirs::config_dir().or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))?
            }
            #[cfg(target_os = "linux")]
            {
                dirs::config_dir().or_else(|| dirs::home_dir().map(|h| h.join(".config")))?
            }
            #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
            {
                return None;
            }
        };

        let candidates: Vec<PathBuf> = Self::host_folder_candidates(app_folder)
            .into_iter()
            .map(|folder| base_dir.join(folder).join("User").join("globalStorage"))
            .collect();

        candidates
            .iter()
            .find(|p| p.join("state.vscdb").exists())
            .or_else(|| candidates.iter().find(|p| p.exists()))
            .cloned()
            .or_else(|| candidates.into_iter().next())
    }

    /// 注入 VS Code 体系 state.vscdb（采用现代化 Protobuf Sentinel 编码与 busy_timeout 防锁）
    pub fn inject_sqlite_state(db_path: &Path, account: &crate::types::Account) -> Result<()> {
        if !db_path.exists() {
            return Ok(());
        }

        let conn = Connection::open(db_path)?;
        conn.busy_timeout(Duration::from_millis(2000))?;

        // 1. 注入 antigravityUnifiedStateSync.oauthToken
        use base64::{engine::general_purpose, Engine as _};
        let raw_expiry = account
            .token
            .expiry_timestamp
            .unwrap_or_else(|| chrono::Utc::now().timestamp() + 3600);
        let expiry_secs = if raw_expiry > 10_000_000_000 {
            raw_expiry / 1000
        } else {
            raw_expiry
        };

        let oauth_info = protobuf::create_oauth_info(
            &account.token.access_token,
            &account.token.refresh_token,
            expiry_secs,
            account.token.is_gcp_tos.unwrap_or(false),
            account.token.id_token.as_deref(),
            Some(&account.email),
        );

        let current_topic: Vec<u8> = conn
            .query_row(
                "SELECT value FROM ItemTable WHERE key = 'antigravityUnifiedStateSync.oauthToken'",
                [],
                |row| row.get::<_, String>(0),
            )
            .ok()
            .and_then(|val| general_purpose::STANDARD.decode(val).ok())
            .unwrap_or_default();

        let mut topic =
            protobuf::remove_unified_topic_entry(&current_topic, "oauthTokenInfoSentinelKey")
                .unwrap_or_default();
        topic = protobuf::remove_unified_topic_entry(&topic, "authStateWithContextSentinelKey")
            .unwrap_or(topic);

        // 1.1 关键修复 (Fixes #4026 对齐主分支): 同步注入 authStateWithContextSentinelKey (JSON 状态对象)
        // Antigravity 前端通过该键读取当前会话状态。若切号时仅更新 protobuf 的 oauthTokenInfoSentinelKey，
        // 前端会沿用旧账号的 JSON 状态与 Token，在重新校验时跌入 signedOut 并重定向至 /onboarding?login=true。
        let expiry_ms = if expiry_secs > 10_000_000_000 {
            expiry_secs
        } else {
            expiry_secs * 1000
        };
        let project_str = account
            .token
            .project_id
            .as_deref()
            .unwrap_or("aicode-consumers");
        let auth_state_json = serde_json::json!({
            "state": "signedIn",
            "context": {
                "project": project_str,
                "showProjectError": false,
                "errorMessage": "",
                "ineligibleMessage": ""
            },
            "token": {
                "accessToken": account.token.access_token,
                "refreshToken": account.token.refresh_token,
                "expiry": expiry_ms
            }
        });
        if let Ok(auth_state_bytes) = serde_json::to_vec(&auth_state_json) {
            topic.extend(protobuf::create_unified_raw_topic_entry(
                "authStateWithContextSentinelKey",
                &auth_state_bytes,
            ));
        }

        // 1.2 注入 oauthTokenInfoSentinelKey (Protobuf 结构体)
        topic.extend(protobuf::create_unified_topic_entry(
            "oauthTokenInfoSentinelKey",
            &oauth_info,
        ));

        let topic_b64 = general_purpose::STANDARD.encode(&topic);
        conn.execute(
            "INSERT OR REPLACE INTO ItemTable (key, value) VALUES ('antigravityUnifiedStateSync.oauthToken', ?1)",
            [&topic_b64],
        )?;

        // 2. 注入 antigravityUnifiedStateSync.userStatus
        let user_status_payload = protobuf::create_minimal_user_status_payload(&account.email);
        let user_status_b64 =
            protobuf::create_unified_state_entry("userStatusSentinelKey", &user_status_payload);
        conn.execute(
            "INSERT OR REPLACE INTO ItemTable (key, value) VALUES ('antigravityUnifiedStateSync.userStatus', ?1)",
            [&user_status_b64],
        )?;

        // 3. 同步更新 cloudcode.userState，保持扩展层用户邮箱与登录态完全对齐
        let cloudcode_state = serde_json::json!({
            "state": "signedIn",
            "context": {
                "project": account.token.project_id.clone().unwrap_or_default(),
                "showProjectError": false,
                "errorMessage": ""
            },
            "user": {
                "email": account.email,
                "name": account.name.clone().unwrap_or_else(|| account.email.clone())
            }
        });
        let state_str = serde_json::to_string(&cloudcode_state)?;
        let _ = conn.execute(
            "INSERT OR REPLACE INTO ItemTable (key, value) VALUES ('cloudcode.userState', ?1)",
            [&state_str],
        );

        // 4. 企业 project_id 偏好同步（无 project_id 时主动清理，避免残留上一账号企业配置）
        if let Some(project_id) = account
            .token
            .project_id
            .as_deref()
            .map(str::trim)
            .filter(|pid| !pid.is_empty())
        {
            let pref_payload = protobuf::encode_string_field(1, project_id);
            let pref_b64 =
                protobuf::create_unified_state_entry("enterpriseGcpProjectId", &pref_payload);
            let _ = conn.execute(
                "INSERT OR REPLACE INTO ItemTable (key, value) VALUES ('antigravityUnifiedStateSync.enterprisePreferences', ?1)",
                [&pref_b64],
            );
        } else {
            let _ = conn.execute(
                "DELETE FROM ItemTable WHERE key = 'antigravityUnifiedStateSync.enterprisePreferences'",
                [],
            );
        }

        // 5. 设备指纹与会话隔离
        if let Some(ref profile) = account.device_profile {
            let _ = conn.execute(
                "INSERT OR REPLACE INTO ItemTable (key, value) VALUES ('storage.serviceMachineId', ?1)",
                [&profile.dev_device_id],
            );
            let _ = conn.execute(
                "INSERT OR REPLACE INTO ItemTable (key, value) VALUES ('telemetry.serviceMachineId', ?1)",
                [&profile.dev_device_id],
            );
            let _ = conn.execute(
                "DELETE FROM ItemTable WHERE key = 'telemetry.currentSessionId'",
                [],
            );
        }

        // 6. 免责声明、安全条款确认与旧会话历史标识清理
        let _ = disclaimer::DisclaimerBypass::apply_to_db(&conn);
        let _ = disclaimer::DisclaimerBypass::apply_onboarding_to_db(
            &conn,
            &account.email,
            account.token.is_gcp_tos.unwrap_or(false),
        );

        Ok(())
    }

    /// 从客户端 `state.vscdb` 的 Protobuf 状态同步表 (`antigravityUnifiedStateSync.oauthToken`) 提取当前生效的真实 `refresh_token`
    pub fn extract_refresh_token_from_sqlite(app_folder: &str) -> Option<String> {
        use base64::{engine::general_purpose, Engine as _};

        let user_storage = Self::resolve_app_user_storage_dir(app_folder)?;
        let db_path = user_storage.join("state.vscdb");
        if !db_path.exists() {
            return None;
        }

        let conn = rusqlite::Connection::open_with_flags(
            &db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .ok()?;

        // 1. 优先解析新版格式 (>= 1.16.5): antigravityUnifiedStateSync.oauthToken -> Field 3 (refresh_token)
        //    若 oauthTokenInfoSentinelKey 未命中，回退读取同 topic 下的 authStateWithContextSentinelKey JSON token.refreshToken
        if let Ok(outer_b64) = conn.query_row::<String, _, _>(
            "SELECT value FROM ItemTable WHERE key = 'antigravityUnifiedStateSync.oauthToken'",
            [],
            |r| r.get(0),
        ) {
            if let Ok((sentinel_key, oauth_info_blob)) =
                protobuf::decode_unified_state_entry_with_target(
                    &outer_b64,
                    Some("oauthTokenInfoSentinelKey"),
                )
            {
                if sentinel_key == "oauthTokenInfoSentinelKey" {
                    if let Ok(Some(rt_bytes)) = protobuf::find_field(&oauth_info_blob, 3) {
                        if let Ok(rt) = String::from_utf8(rt_bytes) {
                            let trimmed = rt.trim();
                            if !trimmed.is_empty() {
                                return Some(trimmed.to_string());
                            }
                        }
                    }
                }
            }
            if let Ok((sentinel_key, auth_state_bytes)) =
                protobuf::decode_unified_state_entry_with_target(
                    &outer_b64,
                    Some("authStateWithContextSentinelKey"),
                )
            {
                if sentinel_key == "authStateWithContextSentinelKey" {
                    if let Ok(val) = serde_json::from_slice::<serde_json::Value>(&auth_state_bytes)
                    {
                        if let Some(rt) = val
                            .get("token")
                            .and_then(|t| t.get("refreshToken"))
                            .and_then(|v| v.as_str())
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                        {
                            return Some(rt.to_string());
                        }
                    }
                }
            }
        }

        // 2. 兼容旧版格式 (< 1.16.5): jetskiStateSync.agentManagerInitState -> Field 6 -> Field 3
        if let Ok(legacy_b64) = conn.query_row::<String, _, _>(
            "SELECT value FROM ItemTable WHERE key = 'jetskiStateSync.agentManagerInitState'",
            [],
            |r| r.get(0),
        ) {
            if let Ok(blob) = general_purpose::STANDARD.decode(legacy_b64.trim()) {
                if let Ok(Some(oauth_data)) = protobuf::find_field(&blob, 6) {
                    if let Ok(Some(rt_bytes)) = protobuf::find_field(&oauth_data, 3) {
                        if let Ok(rt) = String::from_utf8(rt_bytes) {
                            let trimmed = rt.trim();
                            if !trimmed.is_empty() {
                                return Some(trimmed.to_string());
                            }
                        }
                    }
                }
            }
        }

        None
    }

    /// 从客户端的 SQLite 数据库 (state.vscdb) 中读取当前登录账号的真实邮箱
    pub fn read_account_from_sqlite(app_folder: &str) -> Option<String> {
        let user_storage = Self::resolve_app_user_storage_dir(app_folder)?;
        let db_path = user_storage.join("state.vscdb");
        if !db_path.exists() {
            return None;
        }

        let conn = rusqlite::Connection::open_with_flags(
            &db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .ok()?;

        // 1. 优先从 antigravityUnifiedStateSync.userStatus 解码真实 email (Field 3 / Field 7)
        if let Ok(us_b64) = conn.query_row::<String, _, _>(
            "SELECT value FROM ItemTable WHERE key = 'antigravityUnifiedStateSync.userStatus'",
            [],
            |r| r.get(0),
        ) {
            if let Ok((_, us_blob)) = protobuf::decode_unified_state_entry_with_target(
                &us_b64,
                Some("userStatusSentinelKey"),
            ) {
                for fnum in [7u32, 3u32] {
                    if let Ok(Some(bytes)) = protobuf::find_field(&us_blob, fnum) {
                        if let Ok(s) = String::from_utf8(bytes) {
                            if s.contains('@') {
                                return Some(s);
                            }
                        }
                    }
                }
            }
        }

        // 2. 次选从 cloudcode.userState JSON 中提取真实登录 user.email
        let user_state: Option<String> = conn
            .query_row(
                "SELECT value FROM ItemTable WHERE key = 'cloudcode.userState'",
                [],
                |r| r.get(0),
            )
            .ok();

        if let Some(raw) = user_state {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) {
                if let Some(email) = val
                    .get("user")
                    .and_then(|u| u.get("email"))
                    .and_then(|e| e.as_str())
                {
                    if !email.is_empty() {
                        return Some(email.to_string());
                    }
                }
            }
        }

        // 3. 次选从 onboarding 用户名读取
        let onboarding_user: Option<String> = conn
            .query_row(
                "SELECT value FROM ItemTable WHERE key = 'agy.onboarding.lastLoginUsername' OR key = 'jetski.onboarding.lastLoginUsername'",
                [],
                |r| r.get(0),
            )
            .ok();

        if let Some(user) = onboarding_user {
            let trimmed = user.trim();
            if !trimmed.is_empty() && trimmed.contains('@') {
                return Some(trimmed.to_string());
            }
        }

        // 4. 兜底从 antigravityUnifiedStateSync.oauthToken 的 id_token JWT 解析 email
        if let Ok(outer_b64) = conn.query_row::<String, _, _>(
            "SELECT value FROM ItemTable WHERE key = 'antigravityUnifiedStateSync.oauthToken'",
            [],
            |r| r.get(0),
        ) {
            if let Ok((sentinel_key, oauth_info_blob)) =
                protobuf::decode_unified_state_entry_with_target(
                    &outer_b64,
                    Some("oauthTokenInfoSentinelKey"),
                )
            {
                if sentinel_key == "oauthTokenInfoSentinelKey" {
                    if let Ok(Some(id_token_bytes)) = protobuf::find_field(&oauth_info_blob, 5) {
                        if let Ok(id_token) = String::from_utf8(id_token_bytes) {
                            if let Some(email) = Self::extract_email_from_jwt(&id_token) {
                                return Some(email);
                            }
                        }
                    }
                }
            }
        }

        None
    }

    /// 收集指定宿主的标准与自定义 `app_storage.json` 候选路径（无 OS 进程表扫描，轻量极速）
    fn collect_standard_app_storage_paths(app_folder: &str) -> Vec<PathBuf> {
        let mut candidate_paths = Vec::new();
        let is_ide = app_folder.contains("IDE");

        // 1) 用户在 Settings 中配置的自定义存储目录（如 .../User/globalStorage 或自定义 profile 根目录）
        let cfg = crate::config::AppConfig::load();
        let custom = if is_ide {
            cfg.custom_paths.ide.as_deref()
        } else {
            cfg.custom_paths.app.as_deref()
        };
        if let Some(custom_dir) = custom.map(str::trim).filter(|s| !s.is_empty()) {
            let p = PathBuf::from(custom_dir);
            candidate_paths.push(p.join("app_storage.json"));
            if let Some(parent) = p.parent() {
                candidate_paths.push(parent.join("app_storage.json"));
                if let Some(grandparent) = parent.parent() {
                    candidate_paths.push(grandparent.join("app_storage.json"));
                }
            }
        }

        // 2) 各操作系统标准安装目录
        let folder_names = Self::host_folder_candidates(app_folder);

        #[cfg(target_os = "macos")]
        if let Some(home) = dirs::home_dir() {
            for folder in &folder_names {
                candidate_paths.push(
                    home.join("Library/Application Support")
                        .join(folder)
                        .join("app_storage.json"),
                );
            }
        }

        #[cfg(target_os = "windows")]
        {
            if let Some(app_data) =
                dirs::data_dir().or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
            {
                for folder in &folder_names {
                    candidate_paths.push(app_data.join(folder).join("app_storage.json"));
                }
            }
            if let Some(local_data) = dirs::data_local_dir() {
                for folder in &folder_names {
                    candidate_paths.push(local_data.join(folder).join("app_storage.json"));
                }
            }
        }

        #[cfg(target_os = "linux")]
        if let Some(config_dir) =
            dirs::config_dir().or_else(|| dirs::home_dir().map(|h| h.join(".config")))
        {
            for folder in &folder_names {
                candidate_paths.push(config_dir.join(folder).join("app_storage.json"));
            }
        }

        candidate_paths
    }

    fn read_email_from_app_storage_file(path: &Path) -> Option<String> {
        if !path.exists() {
            return None;
        }
        let content = fs::read_to_string(path).ok()?;
        let data: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(&content).ok()?;
        for key in [
            "agy.onboarding.lastLoginUsername",
            "jetski.onboarding.lastLoginUsername",
        ] {
            if let Some(email) = data
                .get(key)
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty() && s.contains('@'))
            {
                return Some(email.to_string());
            }
        }
        None
    }

    /// 从客户端专属 `app_storage.json` 读取当前宿主最近登录邮箱（App 与 IDE 目录完全隔离，互不串号）
    pub fn read_app_storage_account(
        app_folder: &str,
        check_running_process: bool,
    ) -> Option<String> {
        let mut visited = std::collections::HashSet::new();
        for path in Self::collect_standard_app_storage_paths(app_folder) {
            if !visited.insert(path.clone()) {
                continue;
            }
            if let Some(email) = Self::read_email_from_app_storage_file(&path) {
                return Some(email);
            }
        }

        // 若标准/自定义路径未命中且宿主正在运行，按需回退探测 --user-data-dir / Portable 模式路径
        if check_running_process {
            let is_ide = app_folder.contains("IDE");
            let mut sys = System::new();
            Self::refresh_processes_minimal(&mut sys, false);
            let roots = if is_ide {
                Self::get_ide_main_pids(&sys)
            } else {
                Self::get_classic_main_pids(&sys)
            };
            for pid_u32 in roots {
                if let Some(process) = sys.process(sysinfo::Pid::from_u32(pid_u32)) {
                    if let Some(user_data_dir) = Self::extract_user_data_dir_from_cmd(process.cmd())
                    {
                        let p = user_data_dir.join("app_storage.json");
                        if visited.insert(p.clone()) {
                            if let Some(email) = Self::read_email_from_app_storage_file(&p) {
                                return Some(email);
                            }
                        }
                    }
                    if let Some(exe_parent) = process.exe().and_then(|p| p.parent()) {
                        let p = exe_parent.join("data/user-data/app_storage.json");
                        if visited.insert(p.clone()) {
                            if let Some(email) = Self::read_email_from_app_storage_file(&p) {
                                return Some(email);
                            }
                        }
                    }
                }
            }
        }

        None
    }

    /// 写入设备指纹到 storage.json
    pub fn write_device_profile_storage(
        app_folder: &str,
        profile: &crate::types::DeviceProfile,
    ) -> Result<()> {
        if let Some(user_storage) = Self::resolve_app_user_storage_dir(app_folder) {
            if user_storage.exists() {
                let storage_json_path = user_storage.join("storage.json");
                let mut root: serde_json::Value = if storage_json_path.exists() {
                    let s = fs::read_to_string(&storage_json_path).unwrap_or_default();
                    serde_json::from_str(&s).unwrap_or(serde_json::json!({}))
                } else {
                    serde_json::json!({})
                };

                root["telemetry.machineId"] = serde_json::Value::String(profile.machine_id.clone());
                root["telemetry.macMachineId"] =
                    serde_json::Value::String(profile.mac_machine_id.clone());
                root["telemetry.devDeviceId"] =
                    serde_json::Value::String(profile.dev_device_id.clone());
                root["telemetry.sqmId"] = serde_json::Value::String(profile.sqm_id.clone());
                root["storage.serviceMachineId"] =
                    serde_json::Value::String(profile.dev_device_id.clone());

                let _ = fs::write(storage_json_path, serde_json::to_string_pretty(&root)?);
            }
        }
        Ok(())
    }

    /// 同步 Electron 经典版/IDE 专属全局持久化配置 (app_storage.json) 中的条款与协议状态，
    /// 支持 --user-data-dir、Portable 模式与标准路径，彻底消除切号后 Terms of Service 弹窗 (Fixes #4026)
    pub fn sync_classic_app_storage_onboarding(app_folder: &str, email: &str, is_gcp_tos: bool) {
        let mut candidate_paths = Vec::new();
        let is_ide = app_folder.contains("IDE");

        // 1) 从运行中进程提取 --user-data-dir 与 Portable 模式路径
        let mut sys = System::new();
        Self::refresh_processes_minimal(&mut sys, false);
        let roots = if is_ide {
            Self::get_ide_main_pids(&sys)
        } else {
            Self::get_classic_main_pids(&sys)
        };
        for pid_u32 in roots {
            if let Some(process) = sys.process(sysinfo::Pid::from_u32(pid_u32)) {
                if let Some(user_data_dir) = Self::extract_user_data_dir_from_cmd(process.cmd()) {
                    candidate_paths.push(user_data_dir.join("app_storage.json"));
                }
                if let Some(exe_parent) = process.exe().and_then(|p| p.parent()) {
                    candidate_paths.push(exe_parent.join("data/user-data/app_storage.json"));
                }
            }
        }

        // 2) 各操作系统标准与自定义安装目录
        candidate_paths.extend(Self::collect_standard_app_storage_paths(app_folder));

        let tos_str = if is_gcp_tos { "true" } else { "false" };
        let mut visited = std::collections::HashSet::new();

        for path in candidate_paths {
            if !visited.insert(path.clone()) || !path.exists() {
                continue;
            }

            let mut data: serde_json::Map<String, serde_json::Value> =
                match fs::read_to_string(&path) {
                    Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
                    Err(_) => continue,
                };

            data.insert(
                "agy.onboarding.lastLoginUsername".to_string(),
                serde_json::json!(email),
            );
            data.insert(
                "agy.onboarding.lastLoginIsGcpTos".to_string(),
                serde_json::json!(tos_str),
            );
            data.insert(
                "jetski.onboarding.lastLoginUsername".to_string(),
                serde_json::json!(email),
            );
            data.insert(
                "jetski.onboarding.lastLoginIsGcpTos".to_string(),
                serde_json::json!(tos_str),
            );
            data.insert(
                "ide-install-wizard-shown".to_string(),
                serde_json::json!("true"),
            );

            if let Ok(new_content) = serde_json::to_string_pretty(&data) {
                let _ = fs::write(&path, new_content);
                tracing::info!(
                    "[Desktop] Synchronized onboarding state in {:?} for {}",
                    path,
                    email
                );
            }
        }
    }

    /// 统一全链路凭据写入（设备指纹 + app_storage.json + state.vscdb + Keyring + 文件凭据回退）
    /// 用于热切前预写、杀子进程后二次权威补写，以及完整重启杀主进程后重新断言。
    pub(super) fn apply_host_credentials(
        app_folder: &str,
        account: &crate::types::Account,
        require_sqlite: bool,
    ) -> Result<()> {
        if let Some(ref profile) = account.device_profile {
            let _ = Self::write_device_profile_storage(app_folder, profile);
        }

        Self::sync_classic_app_storage_onboarding(
            app_folder,
            &account.email,
            account.token.is_gcp_tos.unwrap_or(false),
        );

        if let Some(user_storage) = Self::resolve_app_user_storage_dir(app_folder) {
            let db_path = user_storage.join("state.vscdb");
            if db_path.exists() {
                if require_sqlite {
                    Self::inject_sqlite_state(&db_path, account)?;
                } else if let Err(e) = Self::inject_sqlite_state(&db_path, account) {
                    tracing::warn!(
                        "[Desktop] SQLite dual-write warning for {}: {}",
                        app_folder,
                        e
                    );
                }
            }
        }

        Self::write_to_system_keyring(account)?;
        Self::write_to_file_credentials(account, false)?;
        Ok(())
    }
}
