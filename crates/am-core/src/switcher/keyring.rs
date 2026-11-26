use super::SwitchEngine;
use anyhow::{Context, Result};
use std::fs;
use std::time::Duration;

impl SwitchEngine {
    /// 写入系统凭据存储区 (macOS Keychain / Linux Secret Service)
    pub fn write_to_system_keyring(account: &crate::types::Account) -> Result<()> {
        let raw_expiry = account
            .token
            .expiry_timestamp
            .unwrap_or_else(|| chrono::Utc::now().timestamp() + 3600);
        let expiry_secs = if raw_expiry > 10_000_000_000 {
            raw_expiry / 1000
        } else {
            raw_expiry
        };
        let expiry_datetime =
            chrono::DateTime::from_timestamp(expiry_secs, 0).unwrap_or_else(|| chrono::Utc::now());
        let expiry_str = expiry_datetime.to_rfc3339_opts(chrono::SecondsFormat::Micros, true);

        let payload_json = serde_json::json!({
            "token": {
                "access_token": account.token.access_token,
                "token_type": "Bearer",
                "refresh_token": account.token.refresh_token,
                "expiry": expiry_str,
                "expires_in": 3599
            },
            "auth_method": "consumer",
            "id_token_email": account.email,
            "id_token": account.token.id_token
        });

        #[cfg(target_os = "macos")]
        {
            use base64::{engine::general_purpose::STANDARD, Engine as _};
            let encoded_payload = STANDARD.encode(payload_json.to_string());
            let full_keyring_value = format!("go-keyring-base64:{}", encoded_payload);

            // 删除旧凭据
            let _ = std::process::Command::new("security")
                .args([
                    "delete-generic-password",
                    "-s",
                    "gemini",
                    "-a",
                    "antigravity",
                ])
                .output();

            // 通过 `security -i` 的 stdin 管道写入新凭据，避免敏感 Token 暴露在进程命令行参数 (argv / ps) 中
            use std::io::Write;
            let mut child = std::process::Command::new("security")
                .arg("-i")
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .context("Failed to spawn macOS security command")?;

            if let Some(mut stdin) = child.stdin.take() {
                let escaped = full_keyring_value
                    .replace('\\', "\\\\")
                    .replace('"', "\\\"")
                    .replace('\r', "")
                    .replace('\n', "");
                let cmd_line = format!(
                    "add-generic-password -U -s \"gemini\" -a \"antigravity\" -w \"{}\" -A\n",
                    escaped
                );
                let _ = stdin.write_all(cmd_line.as_bytes());
            }

            let output = child
                .wait_with_output()
                .context("Failed to wait for macOS security command")?;

            if !output.status.success() {
                let err = String::from_utf8_lossy(&output.stderr);
                tracing::warn!("[Keyring] macOS security command notice: {}", err.trim());
            }
        }

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::ffi::OsStrExt;
            use std::ptr;

            #[repr(C)]
            struct FILETIME {
                dw_low_date_time: u32,
                dw_high_date_time: u32,
            }

            #[repr(C)]
            struct CREDENTIALW {
                flags: u32,
                cred_type: u32,
                target_name: *const u16,
                comment: *const u16,
                last_written: FILETIME,
                credential_blob_size: u32,
                credential_blob: *const u8,
                persist: u32,
                attribute_count: u32,
                attributes: *const std::ffi::c_void,
                target_alias: *const u16,
                user_name: *const u16,
            }

            #[link(name = "advapi32")]
            extern "system" {
                fn CredWriteW(credential: *const CREDENTIALW, flags: u32) -> i32;
                fn CredDeleteW(target_name: *const u16, type_: u32, flags: u32) -> i32;
            }

            let target = "gemini:antigravity";
            let user = "antigravity";
            let payload_str = payload_json.to_string();
            let secret = payload_str.as_bytes();

            let target_wide: Vec<u16> = std::ffi::OsStr::new(target)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let user_wide: Vec<u16> = std::ffi::OsStr::new(user)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();

            let cred = CREDENTIALW {
                flags: 0,
                cred_type: 1, // CRED_TYPE_GENERIC
                target_name: target_wide.as_ptr(),
                comment: ptr::null(),
                last_written: FILETIME {
                    dw_low_date_time: 0,
                    dw_high_date_time: 0,
                },
                credential_blob_size: secret.len() as u32,
                credential_blob: secret.as_ptr(),
                persist: 2, // CRED_PERSIST_LOCAL_MACHINE
                attribute_count: 0,
                attributes: ptr::null(),
                target_alias: ptr::null(),
                user_name: user_wide.as_ptr(),
            };

            unsafe {
                let _ = CredDeleteW(target_wide.as_ptr(), 1, 0);
                if CredWriteW(&cred, 0) == 0 {
                    let err = std::io::Error::last_os_error();
                    tracing::warn!("[Keyring] Windows CredWriteW failed: {}", err);
                }
            }
        }

        #[cfg(target_os = "linux")]
        {
            use std::io::Write;
            use std::sync::mpsc;

            let payload_bytes = payload_json.to_string().into_bytes();
            let store_to_collection = |collection_opt: Option<&str>| -> Result<()> {
                let mut cmd = std::process::Command::new("secret-tool");
                cmd.arg("store");
                if let Some(col) = collection_opt {
                    cmd.arg(format!("--collection={}", col));
                    cmd.arg("--label=Password for 'antigravity' on 'gemini'");
                } else {
                    cmd.arg("--label=gemini");
                }
                cmd.args(["service", "gemini", "username", "antigravity"])
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped());

                let mut child = cmd.spawn().context("Failed to spawn secret-tool")?;
                if let Some(mut stdin) = child.stdin.take() {
                    let _ = stdin.write_all(&payload_bytes);
                }
                let child_pid = child.id();
                let (tx, rx) = mpsc::channel();
                std::thread::spawn(move || {
                    let _ = tx.send(child.wait_with_output());
                });

                match rx.recv_timeout(Duration::from_secs(10)) {
                    Ok(Ok(output)) if output.status.success() => Ok(()),
                    Ok(Ok(output)) => {
                        let err = String::from_utf8_lossy(&output.stderr);
                        anyhow::bail!("secret-tool failed: {}", err.trim())
                    }
                    Ok(Err(e)) => Err(e.into()),
                    Err(_) => {
                        let _ = std::process::Command::new("kill")
                            .args(["-9", &child_pid.to_string()])
                            .output();
                        anyhow::bail!("secret-tool store timed out after 10s (D-Bus unreachable)")
                    }
                }
            };

            if let Err(e) = store_to_collection(Some("/org/freedesktop/secrets/collection/login")) {
                tracing::warn!("[Keyring] Linux login collection write notice: {}", e);
            }
            if let Err(e) = store_to_collection(None) {
                tracing::warn!("[Keyring] Linux default collection write notice: {}", e);
            }
        }

        Ok(())
    }

    /// 写入本地文件凭据回退：
    /// - `sync_agy_cli = false`（切换 App / IDE）：仅写入 `jetski-standalone-oauth-token`，严禁触碰 CLI 专属凭据与 Gemini CLI 凭据
    /// - `sync_agy_cli = true`（切换 CLI）：同步写入 `antigravity-cli/antigravity-oauth-token`、`oauth_creds.json`、`google_accounts.json`、`antigravity/active_account.json`
    pub fn write_to_file_credentials(
        account: &crate::types::Account,
        sync_agy_cli: bool,
    ) -> Result<()> {
        let home = dirs::home_dir().context("No home dir")?;
        Self::write_to_file_credentials_in_dir(&home.join(".gemini"), account, sync_agy_cli)
    }

    pub(super) fn write_to_file_credentials_in_dir(
        gemini_dir: &std::path::Path,
        account: &crate::types::Account,
        sync_agy_cli: bool,
    ) -> Result<()> {
        if !gemini_dir.exists() {
            let _ = fs::create_dir_all(gemini_dir);
        }

        let raw_expiry = account
            .token
            .expiry_timestamp
            .unwrap_or_else(|| chrono::Utc::now().timestamp() + 3600);
        let expiry_secs = if raw_expiry > 10_000_000_000 {
            raw_expiry / 1000
        } else {
            raw_expiry
        };
        let expiry_datetime =
            chrono::DateTime::from_timestamp(expiry_secs, 0).unwrap_or_else(|| chrono::Utc::now());
        let expiry_str = expiry_datetime.to_rfc3339_opts(chrono::SecondsFormat::Micros, true);

        // 1. jetski-standalone-oauth-token (经典版与 IDE 语言服务独立降级回退)
        let jetski_payload = serde_json::json!({
            "token": {
                "access_token": account.token.access_token,
                "token_type": "Bearer",
                "refresh_token": account.token.refresh_token,
                "expiry": expiry_str,
                "expires_in": 3599
            },
            "auth_method": "consumer",
            "id_token_email": account.email,
            "id_token": account.token.id_token
        });
        let standalone_token_path = gemini_dir.join("jetski-standalone-oauth-token");
        let _ = crate::storage::atomic_write_secure(
            &standalone_token_path,
            &serde_json::to_string_pretty(&jetski_payload)?,
        );

        // 2. [FIX #3728] 以下文件全部归属 CLI 体系（agy CLI + Gemini CLI），仅在显式切换 CLI 时写入，
        // 严禁切 App / IDE 时顺手覆写破坏多端账号隔离。
        if sync_agy_cli {
            // 2.1 Antigravity CLI (agy) 专属目录 ~/.gemini/antigravity-cli/antigravity-oauth-token
            let agy_cli_dir = gemini_dir.join("antigravity-cli");
            if !agy_cli_dir.exists() {
                let _ = fs::create_dir_all(&agy_cli_dir);
            }
            let agy_token_path = agy_cli_dir.join("antigravity-oauth-token");
            let _ = crate::storage::atomic_write_secure(
                &agy_token_path,
                &serde_json::to_string_pretty(&jetski_payload)?,
            );

            // 2.2 Gemini CLI 凭据 ~/.gemini/oauth_creds.json
            let oauth_creds = serde_json::json!({
                "access_token": account.token.access_token,
                "refresh_token": account.token.refresh_token,
                "token_type": "Bearer",
                "expiry_date": expiry_secs * 1000,
                "id_token": account.token.id_token,
                "scope": "https://www.googleapis.com/auth/userinfo.email openid https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/userinfo.profile"
            });
            let oauth_creds_path = gemini_dir.join("oauth_creds.json");
            let _ = crate::storage::atomic_write_secure(
                &oauth_creds_path,
                &serde_json::to_string_pretty(&oauth_creds)?,
            );

            // 2.3 Gemini CLI 当前活跃账号 ~/.gemini/google_accounts.json
            let google_accounts = serde_json::json!({
                "active": account.email,
                "old": []
            });
            let google_accounts_path = gemini_dir.join("google_accounts.json");
            let _ = crate::storage::atomic_write_secure(
                &google_accounts_path,
                &serde_json::to_string_pretty(&google_accounts)?,
            );

            // 2.4 active_account.json（含明文双 Token，严格走 0600 原子写入）
            let agy_dir = gemini_dir.join("antigravity");
            if !agy_dir.exists() {
                let _ = fs::create_dir_all(&agy_dir);
            }
            let active_payload = serde_json::json!({
                "id": account.id,
                "email": account.email,
                "access_token": account.token.access_token,
                "refresh_token": account.token.refresh_token,
                "project_id": account.token.project_id,
                "updated_at": chrono::Utc::now().timestamp()
            });
            let _ = crate::storage::atomic_write_secure(
                &agy_dir.join("active_account.json"),
                &serde_json::to_string_pretty(&active_payload)?,
            );
        }

        Ok(())
    }

    /// 热切号文件凭据落盘保障（Fixes #4026 对齐主分支）
    ///
    /// 在热切号路径终止 language_server 子进程之前调用，确认
    /// `~/.gemini/jetski-standalone-oauth-token` 已被操作系统真实持久化并可读。
    pub fn ensure_file_credentials_flushed(account: &crate::types::Account) {
        let gemini_dir = match dirs::home_dir() {
            Some(h) => h.join(".gemini"),
            None => return,
        };
        let token_path = gemini_dir.join("jetski-standalone-oauth-token");

        let verified = fs::read_to_string(&token_path)
            .ok()
            .filter(|s| !s.is_empty() && s.contains(&account.email))
            .is_some();

        if !verified {
            tracing::warn!("[Desktop] Hot switch: jetski-standalone-oauth-token not yet flushed; retrying write before kill.");
            let _ = Self::write_to_file_credentials(account, false);
            std::thread::sleep(Duration::from_millis(100));
        } else {
            tracing::info!(
                "[Desktop] Hot switch: jetski-standalone-oauth-token confirmed flushed for {}; safe to terminate language_server.",
                account.email
            );
        }
    }

    /// 清理 2.34.0+ 客户端在会话切换时的陈旧缓存（解决 Issue #4026 根因）
    pub fn clean_stale_session_caches() {
        if let Some(home) = dirs::home_dir() {
            let gemini_dir = home.join(".gemini");
            let _ = fs::remove_file(gemini_dir.join("credential-user-scope"));
            let _ = fs::remove_file(gemini_dir.join("needs-auth-cache.json"));
            let agy_dir = gemini_dir.join("antigravity");
            let _ = fs::remove_file(agy_dir.join("needs-auth-cache.json"));
            let _ = fs::remove_file(agy_dir.join("credential-user-scope"));
        }
    }

    /// 解析 Keyring / OAuth JSON 载荷中的 `(refresh_token, email)`
    pub fn parse_credential_payload(raw: &str) -> Option<(Option<String>, Option<String>)> {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        let json_str = if let Some(b64) = trimmed.strip_prefix("go-keyring-base64:") {
            let bytes = STANDARD.decode(b64.trim()).ok()?;
            String::from_utf8(bytes).ok()?
        } else {
            trimmed.to_string()
        };

        let val: serde_json::Value = serde_json::from_str(&json_str).ok()?;
        let rt = val
            .get("token")
            .and_then(|t| t.get("refresh_token"))
            .and_then(|v| v.as_str())
            .or_else(|| val.get("refresh_token").and_then(|v| v.as_str()))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let email = val
            .get("id_token_email")
            .and_then(|v| v.as_str())
            .or_else(|| val.get("email").and_then(|v| v.as_str()))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        if rt.is_none() && email.is_none() {
            return None;
        }
        Some((rt, email))
    }

    /// 跨平台仅读取系统级钥匙串/凭据管理器中的 `(refresh_token, email)`（不回退文件，对齐主分支 2.0+ 经典款与 CLI 探测）
    pub fn read_system_keyring_credential() -> Option<(Option<String>, Option<String>)> {
        #[cfg(target_os = "macos")]
        {
            let output = std::process::Command::new("security")
                .args([
                    "find-generic-password",
                    "-s",
                    "gemini",
                    "-a",
                    "antigravity",
                    "-w",
                ])
                .output()
                .ok()?;
            if !output.status.success() {
                return None;
            }
            let raw = String::from_utf8_lossy(&output.stdout);
            return Self::parse_credential_payload(&raw);
        }

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::ffi::OsStrExt;
            use std::ptr;

            #[repr(C)]
            struct FILETIME {
                dw_low_date_time: u32,
                dw_high_date_time: u32,
            }

            #[repr(C)]
            struct CREDENTIALW {
                flags: u32,
                cred_type: u32,
                target_name: *const u16,
                comment: *const u16,
                last_written: FILETIME,
                credential_blob_size: u32,
                credential_blob: *mut u8,
                persist: u32,
                attribute_count: u32,
                attributes: *const std::ffi::c_void,
                target_alias: *const u16,
                user_name: *const u16,
            }

            #[link(name = "advapi32")]
            extern "system" {
                fn CredReadW(
                    target_name: *const u16,
                    type_: u32,
                    flags: u32,
                    credential: *mut *mut CREDENTIALW,
                ) -> i32;
                fn CredFree(buffer: *mut std::ffi::c_void);
            }

            let target_wide: Vec<u16> = std::ffi::OsStr::new("gemini:antigravity")
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let mut cred_ptr: *mut CREDENTIALW = ptr::null_mut();
            unsafe {
                if CredReadW(target_wide.as_ptr(), 1, 0, &mut cred_ptr) != 0 && !cred_ptr.is_null()
                {
                    let cred = &*cred_ptr;
                    let slice = std::slice::from_raw_parts(
                        cred.credential_blob,
                        cred.credential_blob_size as usize,
                    );
                    let raw = String::from_utf8_lossy(slice).into_owned();
                    CredFree(cred_ptr as *mut std::ffi::c_void);
                    return Self::parse_credential_payload(&raw);
                }
            }
            return None;
        }

        #[cfg(target_os = "linux")]
        {
            use std::sync::mpsc;

            let child = std::process::Command::new("secret-tool")
                .args(["lookup", "service", "gemini", "username", "antigravity"])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .spawn()
                .ok()?;

            let child_pid = child.id();
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(child.wait_with_output());
            });

            match rx.recv_timeout(Duration::from_secs(5)) {
                Ok(Ok(output)) if output.status.success() => {
                    let raw = String::from_utf8_lossy(&output.stdout);
                    return Self::parse_credential_payload(&raw);
                }
                Ok(_) => return None,
                Err(_) => {
                    let _ = std::process::Command::new("kill")
                        .args(["-9", &child_pid.to_string()])
                        .output();
                    return None;
                }
            }
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            None
        }
    }

    /// 读取 `agy` CLI 独立专属凭据文件 `~/.gemini/antigravity-cli/antigravity-oauth-token`（严格隔离，不混读经典款共享文件）
    pub fn read_cli_exclusive_credential() -> Option<(Option<String>, Option<String>)> {
        let home = dirs::home_dir()?;
        let cli_dir = home.join(".gemini").join("antigravity-cli");
        let candidates = [
            cli_dir.join("antigravity-oauth-token"),
            cli_dir.join("antigravity-oauth-token.antigravity.bak"),
            cli_dir.join("antigravity-oauth-token.bak"),
        ];
        for path in &candidates {
            if path.exists() {
                if let Ok(content) = fs::read_to_string(path) {
                    if let Some(parsed) = Self::parse_credential_payload(&content) {
                        return Some(parsed);
                    }
                }
            }
        }
        None
    }

    /// 读取 ACP 插件专属凭据 `~/.gemini/antigravity-acp/acp_token.json`
    pub fn read_acp_plugin_credential() -> Option<(Option<String>, Option<String>)> {
        let home = dirs::home_dir()?;
        let candidates = [
            home.join(".gemini/antigravity-acp/acp_token.json"),
            home.join(".antigravity/acp-daemon.json"),
        ];
        for path in &candidates {
            if path.exists() {
                if let Ok(content) = fs::read_to_string(path) {
                    if let Some(parsed) = Self::parse_credential_payload(&content) {
                        return Some(parsed);
                    }
                }
            }
        }
        None
    }
}
