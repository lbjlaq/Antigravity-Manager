pub mod disclaimer;
mod ide_db;
mod keyring;
mod process;
pub mod protobuf;

use crate::types::TargetHost;
use anyhow::{Context, Result};
use std::fs;
use std::time::Duration;

/// 辅助判断字符串是否属于 Antigravity IDE (定制版 VS Code 架构)
pub fn is_antigravity_ide_str(s: &str) -> bool {
    let lower = s.to_lowercase();
    [
        "antigravity ide",
        "antigravity-ide",
        "antigravity_ide",
        "antigravityide",
        "antigravity.ide",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

pub struct SwitchEngine;

impl SwitchEngine {
    /// 执行针对特定单一目标或全部目标的切换 (各宿主完全解耦、独立调度)
    pub fn switch(
        account: &crate::types::Account,
        target: TargetHost,
        force_restart: bool,
    ) -> Result<()> {
        match target {
            TargetHost::App => {
                Self::switch_classic_app(account, force_restart)?;
            }
            TargetHost::Ide => {
                Self::switch_ide(account, force_restart)?;
            }
            TargetHost::Plugin => {
                Self::switch_acp_plugin(account)?;
            }
            TargetHost::Agy => {
                Self::switch_cli_agy(account)?;
            }
            TargetHost::All => {
                let _ = Self::switch_classic_app(account, force_restart);
                let _ = Self::switch_ide(account, force_restart);
                let _ = Self::switch_acp_plugin(account);
                let _ = Self::switch_cli_agy(account);
            }
        }
        Ok(())
    }

    /// 切换 Antigravity 原生独立客户端
    pub fn switch_classic_app(account: &crate::types::Account, force_restart: bool) -> Result<()> {
        tracing::info!(
            "[Classic] Switching Antigravity Classic to {}",
            account.email
        );

        // 1. 全链路凭据预写：设备指纹 + app_storage.json + state.vscdb + Keyring + 文件凭据回退
        Self::apply_host_credentials("Antigravity", account, false)?;

        // 2. 杀子进程前严格落盘保障 (Fixes #4026)
        Self::ensure_file_credentials_flushed(account);

        // 3. 清理旧会话 scope 缓存，防止 2.34.0+ 重启 language_server 时校验 scope 冲突
        Self::clean_stale_session_caches();

        // 4. 探查运行状态与版本
        let running = Self::is_classic_running();
        let version_res = crate::version::get_antigravity_version(TargetHost::App);

        let allow_hot_switch = if force_restart {
            false
        } else {
            match version_res.as_ref() {
                Ok(ver) => {
                    if crate::version::compare_version(&ver.short_version, "2.34.0")
                        != std::cmp::Ordering::Less
                    {
                        tracing::info!(
                            "[Classic] Detected version {} >= 2.34.0; attempting non-intrusive hot switch with cache clean.",
                            ver.short_version
                        );
                        true
                    } else {
                        true
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "[Classic] Version detection notice ({}): keeping hot switch enabled.",
                        e
                    );
                    true
                }
            }
        };

        if running && allow_hot_switch {
            // 热切换：仅终止属于经典版的 language_server 子进程
            let killed_pids = match Self::kill_classic_language_server_and_get_pids() {
                Ok(pids) if !pids.is_empty() => pids,
                _ => Vec::new(),
            };

            if !killed_pids.is_empty() {
                tracing::info!(
                    "[Classic] Hot switch: terminated {} language_server subprocess(es) (PIDs: {:?}); supervisor will respawn child.",
                    killed_pids.len(),
                    killed_pids
                );
                // 杀后全量补写凭据（best-effort）：消除几十毫秒竞态中旧子进程回写 Keyring/SQLite 的可能
                std::thread::sleep(Duration::from_millis(100));
                if let Err(e) = Self::apply_host_credentials("Antigravity", account, false) {
                    tracing::warn!(
                        "[Classic] Hot switch: post-kill credential re-assert warning ({}); pre-kill write remains in place.",
                        e
                    );
                }
                Self::clean_stale_session_caches();

                // 敏捷 100ms 快速轮询，第一时间感知 supervisor 重新拉起的 language_server (Fixes #4026 对齐主分支)
                if Self::wait_for_classic_language_server_respawn_excluding(15, &killed_pids) {
                    tracing::info!("[Classic] Hot switch verified: language_server respawned successfully with new account.");
                    return Ok(());
                }

                tracing::warn!("[Classic] Language server did not respawn within timeout; falling back to full restart.");
                Self::kill_classic_app();
                std::thread::sleep(Duration::from_millis(500));
                let _ = Self::apply_host_credentials("Antigravity", account, false);
                Self::clean_stale_session_caches();
                Self::activate_application("Antigravity");
            } else {
                tracing::warn!(
                    "[Classic] No language_server subprocess found; performing full restart."
                );
                Self::kill_classic_app();
                std::thread::sleep(Duration::from_millis(500));
                let _ = Self::apply_host_credentials("Antigravity", account, false);
                Self::clean_stale_session_caches();
                Self::activate_application("Antigravity");
            }
        } else if running && !allow_hot_switch {
            // 完整重启路径
            Self::kill_classic_app();
            std::thread::sleep(Duration::from_millis(500));
            let _ = Self::apply_host_credentials("Antigravity", account, false);
            Self::clean_stale_session_caches();
            Self::activate_application("Antigravity");
        } else {
            // 未运行，冷启动
            Self::activate_application("Antigravity");
        }

        Ok(())
    }

    // =========================================================================
    // 宿主 2: Antigravity IDE (VS Code 深度定制版架构)
    // =========================================================================

    /// 切换 Antigravity IDE
    pub fn switch_ide(account: &crate::types::Account, force_restart: bool) -> Result<()> {
        tracing::info!("[IDE] Switching Antigravity IDE to {}", account.email);

        // 1. 全链路凭据预写：设备指纹 + app_storage.json + state.vscdb + Keyring + 文件凭据回退
        Self::apply_host_credentials("Antigravity IDE", account, true)?;
        Self::ensure_file_credentials_flushed(account);
        Self::clean_stale_session_caches();

        let running = Self::is_ide_running();

        if running && !force_restart {
            // 热切换：仅终止属于 Antigravity IDE 的后代语言服务/agy 进程，严禁伤害主窗口！
            let killed_pids = match Self::kill_ide_language_server_and_get_pids() {
                Ok(pids) if !pids.is_empty() => pids,
                _ => Vec::new(),
            };

            if !killed_pids.is_empty() {
                tracing::info!(
                    "[IDE] Hot switch: terminated {} language_server/agy subprocess(es) (PIDs: {:?}); supervisor will respawn.",
                    killed_pids.len(),
                    killed_pids
                );
                // 杀后全量补写凭据（best-effort）：防止旧子进程退出前向 SQLite/Keyring 回写旧 Token
                std::thread::sleep(Duration::from_millis(100));
                if let Err(e) = Self::apply_host_credentials("Antigravity IDE", account, false) {
                    tracing::warn!(
                        "[IDE] Hot switch: post-kill credential re-assert warning ({}); pre-kill write remains in place.",
                        e
                    );
                }
                Self::clean_stale_session_caches();

                // 敏捷 100ms 轮询并排除旧 PID，确保感知到全新拉起的子进程
                if Self::wait_for_ide_language_server_respawn_excluding(15, &killed_pids) {
                    tracing::info!("[IDE] Hot switch verified: language_server/agy respawned successfully with new account.");
                    return Ok(());
                }

                // 超时降级为完整重启，避免留下“窗口存活但 AI 引擎已死”的残状态
                tracing::warn!("[IDE] Language server did not respawn within timeout; falling back to full restart.");
                Self::kill_ide_app();
                std::thread::sleep(Duration::from_millis(500));
                let _ = Self::apply_host_credentials("Antigravity IDE", account, false);
                Self::clean_stale_session_caches();
                Self::activate_application("Antigravity IDE");
            } else {
                // 0 命中回退完整重启，严禁静默 no-op
                tracing::warn!("[IDE] No active background language_server/agy subprocess found; falling back to full restart.");
                Self::kill_ide_app();
                std::thread::sleep(Duration::from_millis(500));
                let _ = Self::apply_host_credentials("Antigravity IDE", account, false);
                Self::clean_stale_session_caches();
                Self::activate_application("Antigravity IDE");
            }
        } else if running && force_restart {
            Self::kill_ide_app();
            std::thread::sleep(Duration::from_millis(500));
            let _ = Self::apply_host_credentials("Antigravity IDE", account, false);
            Self::clean_stale_session_caches();
            Self::activate_application("Antigravity IDE");
        } else {
            Self::activate_application("Antigravity IDE");
        }

        Ok(())
    }

    // =========================================================================
    // 宿主 3: Antigravity CLI (agy 命令行工具)
    // =========================================================================

    /// 切换 CLI 命令行 (agy)
    pub fn switch_cli_agy(account: &crate::types::Account) -> Result<()> {
        tracing::info!("[CLI] Switching Antigravity CLI to {}", account.email);

        // 1. 同步写入 CLI 专属文件凭据（antigravity-cli/antigravity-oauth-token、oauth_creds.json、google_accounts.json、active_account.json）
        Self::write_to_file_credentials(account, true)?;

        // 2. 写入系统 Keyring
        let _ = Self::write_to_system_keyring(account);

        Ok(())
    }

    // =========================================================================
    // 宿主 4: Antigravity ACP 插件 (VSCode / Cursor 插件守护进程)
    // =========================================================================

    /// 切换 ACP 插件 (VSCode / Cursor 插件守护进程)
    pub fn switch_acp_plugin(account: &crate::types::Account) -> Result<()> {
        tracing::info!("[ACP] Switching ACP Plugin to {}", account.email);

        let home = dirs::home_dir().context("No home dir")?;
        let acp_dir = home.join(".gemini/antigravity-acp");
        if !acp_dir.exists() {
            fs::create_dir_all(&acp_dir)?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&acp_dir, fs::Permissions::from_mode(0o700));
        }

        let token_path = acp_dir.join("acp_token.json");
        let project_id = account.token.project_id.clone().unwrap_or_default();

        let token_payload = serde_json::json!({
            "client_id": "1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com",
            "client_secret": "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",
            "refresh_token": account.token.refresh_token,
            "token_uri": "https://oauth2.googleapis.com/token",
            "scopes": [
                "https://www.googleapis.com/auth/cloud-platform",
                "https://www.googleapis.com/auth/userinfo.email",
                "https://www.googleapis.com/auth/aicode"
            ],
            "project_id": project_id
        });

        let token_str = serde_json::to_string(&token_payload)?;
        crate::storage::atomic_write_secure(&token_path, &token_str)?;

        // 维护 settings.json 中的 auth.type = "oauth-personal"
        let settings_path = acp_dir.join("settings.json");
        let mut settings_obj: serde_json::Value = if settings_path.exists() {
            let s = fs::read_to_string(&settings_path).unwrap_or_default();
            serde_json::from_str(&s).unwrap_or(serde_json::json!({}))
        } else {
            serde_json::json!({})
        };

        if let Some(map) = settings_obj.as_object_mut() {
            let auth_entry = map
                .entry("auth".to_string())
                .or_insert_with(|| serde_json::json!({}));
            if let Some(auth_map) = auth_entry.as_object_mut() {
                auth_map.insert(
                    "type".to_string(),
                    serde_json::Value::String("oauth-personal".to_string()),
                );
            }
        }
        let _ = crate::storage::atomic_write_secure(
            &settings_path,
            &serde_json::to_string_pretty(&settings_obj)?,
        );

        // macOS: agy_acp_server 从系统 Keychain 读取明文 JSON (service="gemini", account="antigravity-acp")
        // 使用 `security -i` 通过 stdin 管道写入，严禁在 argv 中传递 `-w <token>` 造成 `ps` 泄露
        #[cfg(target_os = "macos")]
        {
            use std::io::Write;
            let _ = std::process::Command::new("security")
                .args([
                    "delete-generic-password",
                    "-s",
                    "gemini",
                    "-a",
                    "antigravity-acp",
                ])
                .output();

            if let Ok(mut child) = std::process::Command::new("security")
                .arg("-i")
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
            {
                if let Some(mut stdin) = child.stdin.take() {
                    let escaped = token_str
                        .replace('\\', "\\\\")
                        .replace('"', "\\\"")
                        .replace('\r', "")
                        .replace('\n', "");
                    let cmd_line = format!(
                        "add-generic-password -U -s \"gemini\" -a \"antigravity-acp\" -w \"{}\" -A\n",
                        escaped
                    );
                    let _ = stdin.write_all(cmd_line.as_bytes());
                }
                let _ = child.wait();
            }
        }

        // 杀掉 agy_acp_server 守护进程触发重载
        Self::kill_process_by_pattern("agy_acp_server");

        Ok(())
    }

    // =========================================================================
    // 精准进程判定、展开与热重载
    // =========================================================================
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Account, TokenInfo};
    use base64::{engine::general_purpose, Engine as _};
    use rusqlite::Connection;
    use std::ffi::OsString;
    use std::path::PathBuf;

    fn make_test_account(email: &str, project_id: Option<&str>, is_gcp_tos: bool) -> Account {
        let json = serde_json::json!({
            "id": "acc-test",
            "email": email,
            "name": "Test User",
            "token": TokenInfo {
                access_token: "ya29.test-access".to_string(),
                refresh_token: "1//test-refresh".to_string(),
                expires_in: Some(3600),
                token_type: Some("Bearer".to_string()),
                expiry_timestamp: Some(1_900_000_000),
                project_id: project_id.map(str::to_string),
                id_token: Some("eyJ.test.id".to_string()),
                oauth_client_key: None,
                is_gcp_tos: Some(is_gcp_tos),
            }
        });
        serde_json::from_value(json).expect("valid test account")
    }

    #[test]
    fn inject_sqlite_state_syncs_auth_state_onboarding_and_clears_enterprise_pref() {
        let db_path =
            std::env::temp_dir().join(format!("am-core-switcher-{}.vscdb", uuid::Uuid::new_v4()));
        {
            let conn = Connection::open(&db_path).expect("create temp db");
            conn.execute(
                "CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value TEXT)",
                [],
            )
            .expect("create ItemTable");
            conn.execute(
                "INSERT INTO ItemTable (key, value) VALUES ('jetskiStateSync.agentManagerInitState', 'stale-user')",
                [],
            )
            .expect("insert stale init state");
        }

        // 1. 写入带企业 project_id 与 GCP TOS 的账号
        let corp_account = make_test_account("corp@company.com", Some("corp-proj-123"), true);
        SwitchEngine::inject_sqlite_state(&db_path, &corp_account).expect("inject corp account");

        {
            let conn = Connection::open(&db_path).expect("reopen db");
            let oauth_b64: String = conn
                .query_row(
                    "SELECT value FROM ItemTable WHERE key = 'antigravityUnifiedStateSync.oauthToken'",
                    [],
                    |r| r.get(0),
                )
                .expect("oauthToken exists");
            let topic = general_purpose::STANDARD
                .decode(&oauth_b64)
                .expect("valid base64");
            let topic_lossy = String::from_utf8_lossy(&topic);
            assert!(topic_lossy.contains("authStateWithContextSentinelKey"));
            assert!(topic_lossy.contains("oauthTokenInfoSentinelKey"));
            assert!(topic_lossy.contains("\"state\":\"signedIn\""));
            assert!(topic_lossy.contains("corp-proj-123"));

            let agy_user: String = conn
                .query_row(
                    "SELECT value FROM ItemTable WHERE key = 'agy.onboarding.lastLoginUsername'",
                    [],
                    |r| r.get(0),
                )
                .expect("agy onboarding username");
            assert_eq!(agy_user, "corp@company.com");

            let agy_tos: String = conn
                .query_row(
                    "SELECT value FROM ItemTable WHERE key = 'agy.onboarding.lastLoginIsGcpTos'",
                    [],
                    |r| r.get(0),
                )
                .expect("agy onboarding tos");
            assert_eq!(agy_tos, "true");

            let stale_count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM ItemTable WHERE key = 'jetskiStateSync.agentManagerInitState'",
                    [],
                    |r| r.get(0),
                )
                .expect("count stale init state");
            assert_eq!(stale_count, 0);

            let ent_count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM ItemTable WHERE key = 'antigravityUnifiedStateSync.enterprisePreferences'",
                    [],
                    |r| r.get(0),
                )
                .expect("count enterprise pref");
            assert_eq!(ent_count, 1);
        }

        // 2. 切换至无 project_id 的个人账号，验证企业偏好被清理且条款键更新
        let personal_account = make_test_account("user@gmail.com", None, false);
        SwitchEngine::inject_sqlite_state(&db_path, &personal_account)
            .expect("inject personal account");

        {
            let conn = Connection::open(&db_path).expect("reopen db");
            let ent_count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM ItemTable WHERE key = 'antigravityUnifiedStateSync.enterprisePreferences'",
                    [],
                    |r| r.get(0),
                )
                .expect("count enterprise pref after personal switch");
            assert_eq!(ent_count, 0);

            let jetski_user: String = conn
                .query_row(
                    "SELECT value FROM ItemTable WHERE key = 'jetski.onboarding.lastLoginUsername'",
                    [],
                    |r| r.get(0),
                )
                .expect("jetski onboarding username");
            assert_eq!(jetski_user, "user@gmail.com");

            let jetski_tos: String = conn
                .query_row(
                    "SELECT value FROM ItemTable WHERE key = 'jetski.onboarding.lastLoginIsGcpTos'",
                    [],
                    |r| r.get(0),
                )
                .expect("jetski onboarding tos");
            assert_eq!(jetski_tos, "false");
        }

        let _ = fs::remove_file(&db_path);
    }

    #[test]
    fn process_matchers_isolate_classic_ide_helpers_and_language_server() {
        let fake_pid = std::process::id().wrapping_add(1000);

        // macOS / Windows / Linux 经典版识别
        assert!(SwitchEngine::is_classic_process(
            fake_pid,
            "antigravity",
            "/applications/antigravity.app/contents/macos/antigravity",
            "/applications/antigravity.app/contents/macos/antigravity"
        ));
        assert!(SwitchEngine::is_classic_process(
            fake_pid,
            "antigravity.exe",
            "c:\\users\\me\\appdata\\local\\programs\\antigravity\\antigravity.exe",
            "antigravity.exe"
        ));
        assert!(SwitchEngine::is_classic_process(
            fake_pid,
            "antigravity",
            "/opt/antigravity/antigravity",
            "/opt/antigravity/antigravity"
        ));

        // 严格排除 IDE 与管理器自身
        assert!(!SwitchEngine::is_classic_process(
            fake_pid,
            "antigravity ide",
            "/applications/antigravity ide.app/contents/macos/antigravity ide",
            ""
        ));
        assert!(!SwitchEngine::is_classic_process(
            fake_pid,
            "am-gui",
            "/usr/local/bin/am-gui",
            "am-gui"
        ));
        assert!(!SwitchEngine::is_classic_process(
            std::process::id(),
            "antigravity",
            "/applications/antigravity.app/contents/macos/antigravity",
            ""
        ));

        // Helper 与语言服务精准分类
        assert!(SwitchEngine::is_helper_process(
            "antigravity helper",
            "--type=renderer",
            ""
        ));
        assert!(SwitchEngine::is_helper_process(
            "language_server_macos_arm",
            "",
            ""
        ));
        assert!(!SwitchEngine::is_helper_process(
            "antigravity",
            "/applications/antigravity.app/contents/macos/antigravity",
            ""
        ));

        assert!(SwitchEngine::is_language_server_process(
            "language_server_macos_arm",
            "",
            "",
            false
        ));
        assert!(!SwitchEngine::is_language_server_process(
            "agy",
            "/usr/bin/agy",
            "",
            false
        ));
        assert!(SwitchEngine::is_language_server_process(
            "agy",
            "/usr/bin/agy",
            "",
            true
        ));
    }

    #[test]
    fn extract_user_data_dir_supports_equals_and_separate_flag() {
        let args_eq = vec![
            OsString::from("Antigravity"),
            OsString::from("--user-data-dir=/tmp/custom-agy-profile"),
        ];
        assert_eq!(
            SwitchEngine::extract_user_data_dir_from_cmd(&args_eq),
            Some(PathBuf::from("/tmp/custom-agy-profile"))
        );

        let args_sep = vec![
            OsString::from("Antigravity"),
            OsString::from("--user-data-dir"),
            OsString::from("/tmp/another-profile"),
        ];
        assert_eq!(
            SwitchEngine::extract_user_data_dir_from_cmd(&args_sep),
            Some(PathBuf::from("/tmp/another-profile"))
        );
    }

    #[test]
    fn file_credentials_strictly_isolate_cli_from_desktop_switches() {
        let tmp_dir =
            std::env::temp_dir().join(format!("am-core-cli-iso-{}", uuid::Uuid::new_v4()));
        let acc = make_test_account("desktop@example.com", Some("proj-1"), false);

        // 1. 切 App / IDE (sync_agy_cli = false) 仅写入 jetski-standalone-oauth-token，严禁触碰任何 CLI 凭据文件
        SwitchEngine::write_to_file_credentials_in_dir(&tmp_dir, &acc, false)
            .expect("write desktop credentials");
        assert!(tmp_dir.join("jetski-standalone-oauth-token").exists());
        assert!(!tmp_dir.join("oauth_creds.json").exists());
        assert!(!tmp_dir.join("google_accounts.json").exists());
        assert!(!tmp_dir
            .join("antigravity-cli/antigravity-oauth-token")
            .exists());
        assert!(!tmp_dir.join("antigravity/active_account.json").exists());

        // 2. 切 CLI (sync_agy_cli = true) 完整写入 CLI 专属文件凭据
        SwitchEngine::write_to_file_credentials_in_dir(&tmp_dir, &acc, true)
            .expect("write cli credentials");
        assert!(tmp_dir
            .join("antigravity-cli/antigravity-oauth-token")
            .exists());
        assert!(tmp_dir.join("oauth_creds.json").exists());
        assert!(tmp_dir.join("google_accounts.json").exists());
        assert!(tmp_dir.join("antigravity/active_account.json").exists());

        let _ = fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn standalone_agy_cli_matcher_excludes_embedded_app_helpers() {
        // Gemini.app / Antigravity IDE 内嵌的 helper 不得误报为独立 CLI 运行中
        assert!(!SwitchEngine::is_standalone_agy_cli_process(
            "agy",
            "/applications/gemini.app/contents/helpers/agy",
            "/applications/gemini.app/contents/helpers/agy --hub --use_host_auth --host_bridge_url=http://127.0.0.1:62484"
        ));
        assert!(!SwitchEngine::is_standalone_agy_cli_process(
            "agy",
            "/applications/antigravity ide.app/contents/resources/bin/agy",
            "agy --lsp"
        ));

        // 终端独立运行的 agy CLI 正常识别
        assert!(SwitchEngine::is_standalone_agy_cli_process(
            "agy",
            "/usr/local/bin/agy",
            "agy chat"
        ));
        assert!(SwitchEngine::is_standalone_agy_cli_process(
            "agy.exe",
            "c:\\users\\me\\bin\\agy.exe",
            "agy.exe"
        ));
    }
}
