use crate::models::Account;
use crate::modules::{db, device, process, version};
use std::fs;
// Command 仅用于 macos/Linux 分支（security / secret-tool / kill），Windows 裁剪不导入以免 unused
#[cfg(not(windows))]
use std::process::Command;

pub trait SystemIntegration: Send + Sync {
    /// 当切换账号时执行的系统层操作（如杀进程、写入文件、注入数据库）
    async fn on_account_switch(
        &self,
        account: &crate::models::Account,
        target_ide: Option<&str>,
    ) -> Result<(), String>;

    /// 更新系统托盘（如果适用）
    fn update_tray(&self);

    /// 发送系统通知
    fn show_notification(&self, title: &str, body: &str);
}

/// 根据目标参数、进程运行态及可执行文件存在性决策最终切换环境
pub fn resolve_effective_target(
    target_ide: Option<&str>,
    classic_running: bool,
    ide_running: bool,
    has_classic_exe: bool,
    ide_exe_path: Option<&str>,
) -> (bool, Option<&'static str>) {
    let is_explicit_ide = target_ide == Some("ide");
    let is_explicit_classic = target_ide == Some("classic");
    let is_explicit_acp = target_ide == Some("acp");

    if is_explicit_ide {
        return (true, Some("ide"));
    }
    if is_explicit_classic {
        return (false, Some("classic"));
    }
    if is_explicit_acp {
        return (false, Some("acp"));
    }

    // target_ide 为 None 或未指定时进行智能环境探查（经典版桌面端优先，严禁仅凭静态 IDE 数据库文件劫持经典版目标）
    let mut is_ide = false;
    if classic_running {
        // 原生经典版正在运行，确定目标为经典版
        is_ide = false;
    } else if ide_running {
        // 经典版未运行，但 IDE 正在运行，推导为 IDE
        is_ide = true;
    } else if has_classic_exe {
        // 原生经典版可执行文件存在，优先保持经典版
        is_ide = false;
    } else if let Some(exe_str) = ide_exe_path {
        // 原生经典版不存在，检查是否存在 IDE 可执行文件
        if process::is_antigravity_ide_str(exe_str) {
            is_ide = true;
        }
    }

    let effective = if is_ide {
        Some("ide")
    } else if is_explicit_classic {
        Some("classic")
    } else {
        None
    };

    (is_ide, effective)
}

/// 桌面版实现：包含完整的进程控制 and UI 同步
pub struct DesktopIntegration {
    pub app_handle: tauri::AppHandle,
}

/// 写入账号凭据：>= 2.0.0 的原生应用走系统 Keyring，旧架构与定制 IDE 走 SQLite 注入。
///
/// 抽成独立函数是为了让「热切号」能在终止 language_server 子进程**之前**完成凭据写入
/// （见 `on_account_switch` 的顺序说明），同时让完整重启路径保持原有的「先杀后写」顺序。
fn apply_account_credentials(
    account: &Account,
    effective_target: Option<&str>,
    is_ide: bool,
    active_exe_path: Option<&std::path::Path>,
) -> Result<(), String> {
    let mut use_keyring = false;

    // 硬防护（issue #3762、issue #3770、issue #3778）：
    // 目标一旦确认为 IDE，执行全链路双写与权威对齐：
    // 1. 注入 SQLite state.vscdb（供 VS Code 界面及扩展前端快速恢复会话与显示）
    // 2. 写入系统 Keyring（服务 gemini:antigravity，供 IDE 2.6.0+ 后台核心 agy 语言服务作为首要真理源读取）
    // 3. 同步写入文件级凭据回退 (~/.gemini/jetski-standalone-oauth-token 与 oauth_creds.json)
    let is_target_ide = is_ide
        || effective_target == Some("ide")
        || (effective_target != Some("classic")
            && active_exe_path
                .map(process::is_antigravity_ide_path)
                .unwrap_or(false));

    if is_target_ide {
        crate::modules::logger::log_info(
            "[Desktop] Target is Antigravity IDE (VS Code architecture); performing dual write (SQLite state.vscdb + system Keyring + file fallback).",
        );

        // 1.1 获取存储路径并写入设备 Profile
        if let Ok(storage_path) = device::get_storage_path(effective_target) {
            if let Some(ref profile) = account.device_profile {
                let _ = device::write_profile(&storage_path, profile);
            }
        }

        // 1.2 注入 SQLite 数据库 (state.vscdb)
        let db_path = db::get_db_path(effective_target)?;
        if db_path.exists() {
            let backup_path = db_path.with_extension("vscdb.backup");
            let _ = fs::copy(&db_path, &backup_path);
        }

        db::inject_token(
            &db_path,
            &account.token.access_token,
            &account.token.refresh_token,
            account.token.expiry_timestamp,
            &account.email,
            account.token.is_gcp_tos,
            account.token.project_id.as_deref(),
            account.token.id_token.as_deref(),
            account.token.oauth_client_key.as_deref(),
            effective_target,
        )?;

        if let Some(ref profile) = account.device_profile {
            let _ = db::write_service_machine_id(&db_path, &profile.mac_machine_id);
        }

        // 1.3 同步写入系统 Keyring (针对 IDE 2.6.0+ agy 后台语言服务鉴权真理源)
        if let Err(keyring_err) = write_to_system_keyring(account) {
            crate::modules::logger::log_warn(&format!(
                "[Desktop] Warning: Failed to write system Keyring for IDE: {}",
                keyring_err
            ));
        }

        // 1.4 同步写入本地文件凭据回退 (~/.gemini/jetski-standalone-oauth-token 与 oauth_creds.json)
        if let Err(file_err) = write_to_file_credentials(account, false) {
            crate::modules::logger::log_warn(&format!(
                "[Desktop] Warning: Failed to write file credentials for IDE: {}",
                file_err
            ));
        }

        return Ok(());
    }

    if !is_target_ide {
        // 经典原生版：自动探测版本号（优先使用预快照路径）
        match version::get_antigravity_version_with_path(
            effective_target,
            active_exe_path.as_deref(),
        ) {
            Ok(ver) => {
                // 如果版本号 >= 2.0.0
                if version::compare_version(&ver.short_version, "2.0.0") != std::cmp::Ordering::Less
                {
                    use_keyring = true;
                    crate::modules::logger::log_info(&format!(
                        "[Desktop] Detected Antigravity version {} >= 2.0.0, using system Keyring.",
                        ver.short_version
                    ));
                } else {
                    crate::modules::logger::log_info(&format!(
                        "[Desktop] Detected Antigravity version {} < 2.0.0, falling back to legacy SQLite injection.",
                        ver.short_version
                    ));
                }
            }
            Err(e) => {
                // 如果探测失败，优先检查本地是否存在可用的 SQLite 数据库 (state.vscdb)
                // 若存在数据库，说明是经典的 VS Code/IDE 架构，优先使用 SQLite 注入，防止无 secret-tool 时报错
                let has_sqlite_db = db::get_db_path(effective_target)
                    .map(|p| p.exists())
                    .unwrap_or(false);

                if has_sqlite_db {
                    use_keyring = false;
                    crate::modules::logger::log_info(&format!(
                        "[Desktop] Failed to detect Antigravity version ({}), but detected existing SQLite database. Falling back to SQLite injection.",
                        e
                    ));
                } else {
                    use_keyring = true;
                    crate::modules::logger::log_warn(&format!(
                        "[Desktop] Failed to detect Antigravity version ({}) and no SQLite database found, defaulting to system Keyring.",
                        e
                    ));
                }
            }
        }
    }

    // ================== Antigravity 经典版/原生应用逻辑 ==================
    // 2.1 写入设备 Profile (若 storage_path 可解析)
    if let Ok(storage_path) = device::get_storage_path(effective_target) {
        if let Some(ref profile) = account.device_profile {
            let _ = device::write_profile(&storage_path, profile);
        }
    }

    // 2.2 双写同步：若经典版存储路径下存在 SQLite state.vscdb，执行数据库 Token 注入
    // 确保原生桌面端与扩展会话状态在所有 Antigravity 变体中保持严格一致 (Fixes #3829)
    let mut sqlite_injected = false;
    let mut sqlite_err_opt = None;
    let db_path_res = db::get_db_path(effective_target);
    match db_path_res {
        Ok(ref db_path) if db_path.exists() => {
            crate::modules::logger::log_info(&format!(
                "[Desktop] Detected existing SQLite DB at {:?} for classic target; performing dual-write.",
                db_path
            ));
            let backup_path = db_path.with_extension("vscdb.backup");
            let _ = fs::copy(db_path, &backup_path);
            match db::inject_token(
                db_path,
                &account.token.access_token,
                &account.token.refresh_token,
                account.token.expiry_timestamp,
                &account.email,
                account.token.is_gcp_tos,
                account.token.project_id.as_deref(),
                account.token.id_token.as_deref(),
                account.token.oauth_client_key.as_deref(),
                effective_target,
            ) {
                Ok(_) => {
                    sqlite_injected = true;
                    if let Some(ref profile) = account.device_profile {
                        let _ = db::write_service_machine_id(db_path, &profile.mac_machine_id);
                    }
                }
                Err(e) => {
                    crate::modules::logger::log_warn(&format!(
                        "[Desktop] Failed to inject token into SQLite DB for classic: {}",
                        e
                    ));
                    sqlite_err_opt = Some(e);
                }
            }
        }
        Ok(ref db_path) => {
            if !use_keyring {
                sqlite_err_opt = Some(format!(
                    "SQLite database file not found at {:?} for legacy Antigravity (< 2.0.0)",
                    db_path
                ));
            }
        }
        Err(e) => {
            if !use_keyring {
                sqlite_err_opt = Some(e);
            }
        }
    }

    // 2.3 写入系统 Keychain/Keyring
    let mut keyring_err_opt = None;
    if use_keyring {
        if let Err(keyring_err) = write_to_system_keyring(account) {
            crate::modules::logger::log_warn(&format!(
                "[Desktop] Keyring write failed ({}) for classic.",
                keyring_err
            ));
            keyring_err_opt = Some(keyring_err);
        }
    }

    // 2.4 同步写入 Standalone / 语言服务原生降级凭据: ~/.gemini/jetski-standalone-oauth-token 与 oauth_creds.json
    // 消除经典款切号后客户端弹出“还需要登录认证”的问题 (Fixes #3829)
    if let Err(file_err) = write_to_file_credentials(account, false) {
        crate::modules::logger::log_warn(&format!(
            "[Desktop] Warning: Failed to write file credentials for classic: {}",
            file_err
        ));
    }

    // 2.5 校验凭据写入状态：
    if use_keyring {
        // 若要求使用 Keyring 且 Keyring 写入失败，但在已成功注入本地 SQLite 数据库时降级允许切号继续；
        // 若两者均未成功则阻断切号流程
        if let Some(keyring_err) = keyring_err_opt {
            if !sqlite_injected {
                return Err(keyring_err);
            }
            crate::modules::logger::log_warn(
                "[Desktop] Keyring write failed, but SQLite DB was synchronized for classic; switch proceeding.",
            );
        }
    } else {
        // 旧版本 (< 2.0.0) 必须成功注入 SQLite
        if !sqlite_injected {
            return Err(sqlite_err_opt.unwrap_or_else(|| {
                "Failed to inject token into SQLite DB for legacy Antigravity (< 2.0.0)".to_string()
            }));
        }
    }

    Ok(())
}

impl SystemIntegration for DesktopIntegration {
    async fn on_account_switch(
        &self,
        account: &crate::models::Account,
        target_ide: Option<&str>,
    ) -> Result<(), String> {
        crate::modules::logger::log_info(&format!(
            "[Desktop] Executing system switch for: {} (target_ide: {:?})",
            account.email, target_ide
        ));

        if target_ide == Some("agy") {
            write_to_system_keyring(account)?;
            let _ = write_to_file_credentials(account, true);
            // A successful write (or a file fallback) is not proof that the CLI
            // will read this account from the system credential store.
            let stored = read_from_system_keyring_only()?;
            verify_agy_credentials(&account.token.refresh_token, &stored.refresh_token)?;

            if let Ok(storage_path) = device::get_storage_path(target_ide) {
                if let Some(ref profile) = account.device_profile {
                    let _ = device::write_profile(&storage_path, profile);
                }
            }

            let is_running = process::is_process_running_by_name("agy");
            let msg = if is_running {
                format!(
                    "Credentials for {} saved and verified. Running agy sessions may still use and write back their previous credentials.",
                    account.email
                )
            } else {
                format!(
                    "Credentials for {} saved and verified for the next CLI command.",
                    account.email
                )
            };
            self.show_notification("Antigravity CLI", &msg);
            self.update_tray();

            return Ok(());
        }

        if target_ide == Some("acp") {
            write_acp_credentials(account)?;
            let killed = process::kill_acp_server_processes().unwrap_or(0);
            let msg = if killed > 0 {
                format!(
                    "Credentials for {} saved. Terminated {} agy_acp_server process(es); host IDE will respawn automatically.",
                    account.email, killed
                )
            } else {
                format!(
                    "Credentials for {} saved for Antigravity IDE Plugin (ACP).",
                    account.email
                )
            };
            self.show_notification("Antigravity ACP", &msg);
            self.update_tray();

            return Ok(());
        }

        // 1. 智能决策：判断目标是 Antigravity IDE (VS Code 定制版) 还是 Antigravity 经典版 (原生桌面端)
        let classic_running = process::is_antigravity_running(None);
        let ide_running = process::is_antigravity_running(Some("ide"));
        let classic_exe = process::get_antigravity_executable_path(Some("classic"));
        let ide_exe = process::get_antigravity_executable_path(Some("ide"));
        let ide_exe_str = ide_exe.as_ref().map(|p| p.to_string_lossy().to_string());

        let (is_ide, effective_target) = resolve_effective_target(
            target_ide,
            classic_running,
            ide_running,
            classic_exe.is_some(),
            ide_exe_str.as_deref(),
        );

        if is_ide {
            crate::modules::logger::log_info(
                "[Desktop] Determined target environment is Antigravity IDE, using IDE account switch logic.",
            );
        } else {
            crate::modules::logger::log_info(
                "[Desktop] Determined target environment is Antigravity classic, using classic account switch logic.",
            );
        }

        // 0. 在关闭外部进程前，预先快照捕获正在运行的客户端可执行文件路径与启动参数
        // 彻底防止杀死进程后由于安装在非标准路径而丢失路径导致启动失败 (Unable to start)
        let active_exe_path = process::get_antigravity_executable_path(effective_target);
        let active_args = process::get_args_from_running_process(effective_target);

        // 2. 决定切换模式（热切号 / 完整重启）并处理进程与凭据
        //
        //    · 热切号 —— 仅限原生经典版（Standalone >= 2.0.0，issue #3503 / issue #3637）：
        //      经典款主窗口进程与后台 language_server 解耦，只终止子进程并更新 Keyring 后，
        //      宿主内置 supervisor 会在约 2 秒内原地重建子进程重载新凭据，保留主窗口和未保存内容。
        //      顺序是**先写凭据 → 再杀子进程 →（best-effort）再补写一次**。
        //    · 完整重启 —— IDE 款（is_ide）、客户端未运行、未定位到语言服务子进程或杀死失败：
        //      IDE 款深度依赖 Extension Host 内存上下文与 SQLite (state.vscdb) 单写者锁，单杀子进程
        //      无法使 IDE 窗口界面与主会话感知新账号，且容易导致并发写冲突与回写覆盖；故 IDE 款坚决
        //      保持「先杀主进程 → 干净写入 SQLite → 重新启动客户端」的权威原子流程。
        let running = process::is_antigravity_running(effective_target);
        let mut hot_switch = false;
        let mut credentials_applied = false;

        // 仅原生经典版（!is_ide）且运行中时尝试热切号
        if !is_ide && running {
            apply_account_credentials(
                account,
                effective_target,
                is_ide,
                active_exe_path.as_deref(),
            )?;
            credentials_applied = true;

            match process::kill_language_server_subprocesses(effective_target) {
                Ok(n) if n > 0 => {
                    hot_switch = true;
                    crate::modules::logger::log_info(&format!(
                        "[Desktop] Hot switch: terminated {} language_server subprocess(es); \
                         main window kept alive, supervisor will respawn the child with the new credentials.",
                        n
                    ));

                    // 补写一次凭据（best-effort）：子进程已被终止，此时数据库不存在竞争写者，
                    // 这次写入即为权威值 —— 用于覆盖"写凭据 → 杀子进程"这几十毫秒窗口内
                    // 旧语言服务可能把旧 token 回写进 Keyring 的情况，
                    // 确保 supervisor 约 2s 后重新拉起的子进程读到的一定是新凭据。
                    // 失败只告警，不使整个切号失败：第一次写入已经就位。
                    if let Err(e) = apply_account_credentials(
                        account,
                        effective_target,
                        is_ide,
                        active_exe_path.as_deref(),
                    ) {
                        crate::modules::logger::log_warn(&format!(
                            "[Desktop] Hot switch: post-kill credential re-assert failed ({}); \
                             the pre-kill write is already in place, continuing.",
                            e
                        ));
                    }
                }
                Ok(_) => crate::modules::logger::log_info(
                    "[Desktop] Hot switch unavailable: no language_server subprocess found; falling back to full restart.",
                ),
                Err(e) => crate::modules::logger::log_warn(&format!(
                    "[Desktop] Hot switch failed ({}); falling back to full restart.",
                    e
                )),
            }
        }

        if !hot_switch && running {
            process::close_antigravity(20, effective_target)?;
            // 完整重启路径必须在彻底终止主进程之后重新断言写入凭据，
            // 消除外部客户端退出钩子在关机瞬间回写陈旧 Session Token 的竞态
            apply_account_credentials(
                account,
                effective_target,
                is_ide,
                active_exe_path.as_deref(),
            )?;
            credentials_applied = true;
        }
        if effective_target != target_ide
            && target_ide.is_some()
            && process::is_antigravity_running(target_ide)
        {
            process::close_antigravity(20, target_ide)?;
        }

        // 若此前从未写入凭据（例如客户端未处于运行状态）：此时写入凭据
        if !credentials_applied {
            apply_account_credentials(
                account,
                effective_target,
                is_ide,
                active_exe_path.as_deref(),
            )?;
        }

        // 3. 重启外部进程（优先使用预快照路径与启动参数）
        //    热切号路径下主窗口仍然存活：只需等 supervisor 把 language_server 拉起即可，
        //    绝不能再去启动一次主窗口（否则会变成双实例）。
        if hot_switch {
            if process::wait_for_language_server_respawn(effective_target, 15) {
                crate::modules::logger::log_info(&format!(
                    "[Desktop] Hot switch completed for {}: language_server respawned with the new credentials.",
                    account.email
                ));
                let _ = crate::modules::tray::update_tray_menus(&self.app_handle);
                return Ok(());
            }

            // 子进程迟迟未恢复 → 降级为完整重启，避免留下"窗口活着但 AI 引擎已死"的残状态
            crate::modules::logger::log_warn(
                "[Desktop] Hot switch degraded: language_server did not respawn within timeout; performing a full restart.",
            );
            process::close_antigravity(20, effective_target)?;
            // 彻底杀死主窗口后重新断言写入凭据，消除退出钩子回写旧 token
            let _ = apply_account_credentials(
                account,
                effective_target,
                is_ide,
                active_exe_path.as_deref(),
            );
        }

        if let Err(e) = process::start_antigravity_with_fallback_path(
            effective_target,
            active_exe_path.as_deref(),
            active_args.as_deref(),
        ) {
            // 若切号前外部客户端原本就没有处于运行状态，且启动失败原因是找不到客户端可执行文件
            // （例如纯反代服务模式、未安装 GUI 客户端或无头环境）：
            // 此时凭据和配置已经写入成功，降级处理并记录信息，避免让整个切号操作报错中断。
            if !running && process::is_client_executable_missing(&e) {
                crate::modules::logger::log_info(
                    "[Desktop] Client executable not found and was not running before switch; credentials applied successfully.",
                );
            } else {
                return Err(e);
            }
        }

        // 4. 更新托盘
        let _ = crate::modules::tray::update_tray_menus(&self.app_handle);

        Ok(())
    }

    fn update_tray(&self) {
        let _ = crate::modules::tray::update_tray_menus(&self.app_handle);
    }

    fn show_notification(&self, title: &str, body: &str) {
        // 使用 tauri-plugin-dialog 或原生通知（此处简化）
        crate::modules::logger::log_info(&format!("[Notification] {}: {}", title, body));
    }
}

/// Resolve the base Gemini home directory ($GEMINI_HOME or ~/.gemini)
pub fn get_gemini_home_dir() -> Result<std::path::PathBuf, String> {
    if let Ok(val) = std::env::var("GEMINI_HOME") {
        let trimmed = val.trim();
        if !trimmed.is_empty() {
            return Ok(std::path::PathBuf::from(trimmed));
        }
    }
    let home = dirs::home_dir().ok_or_else(|| "Failed to get user home directory".to_string())?;
    Ok(home.join(".gemini"))
}

/// 辅助方法：向宿主操作系统的 Keychain/Credentials Manager 写入 Token（支持自定义服务与账号）
pub(crate) fn write_to_system_keyring_with_service_account(
    account: &crate::models::Account,
    service: &str,
    user_account: &str,
) -> Result<(), String> {
    // 1. 构建 Token 的 JSON Payload，并将过期时间戳格式化为符合 RFC3339 的带微秒格式
    let expiry_secs = if account.token.expiry_timestamp > 10_000_000_000 {
        account.token.expiry_timestamp / 1000
    } else {
        account.token.expiry_timestamp
    };
    let expiry_datetime =
        chrono::DateTime::from_timestamp(expiry_secs, 0).unwrap_or_else(|| chrono::Utc::now());
    let expiry_str = expiry_datetime.to_rfc3339_opts(chrono::SecondsFormat::Micros, true);

    #[derive(serde::Serialize)]
    struct KeyringTokenDetails {
        access_token: String,
        token_type: String,
        refresh_token: String,
        expiry: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        expires_in: Option<i64>,
    }

    #[derive(serde::Serialize)]
    struct KeyringPayload {
        token: KeyringTokenDetails,
        auth_method: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        id_token_email: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        id_token: Option<String>,
    }

    let payload_json = serde_json::to_string(&KeyringPayload {
        token: KeyringTokenDetails {
            access_token: account.token.access_token.clone(),
            token_type: "Bearer".to_string(),
            refresh_token: account.token.refresh_token.clone(),
            expiry: expiry_str,
            expires_in: Some(3599),
        },
        auth_method: "consumer".to_string(),
        id_token_email: Some(account.email.clone()),
        id_token: account.token.id_token.clone(),
    })
    .map_err(|e| format!("Failed to serialize keyring JSON: {}", e))?;

    crate::modules::logger::log_info(&format!(
        "[Desktop] Writing token to system credential store for: {} (service: {}, account: {})",
        account.email, service, user_account
    ));

    // 2. 跨平台凭据注入
    #[cfg(target_os = "macos")]
    {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let encoded_payload = STANDARD.encode(&payload_json);
        let full_keyring_value = format!("go-keyring-base64:{}", encoded_payload);

        // 2.1 macOS Keychain Access
        // 删除旧的
        let _ = Command::new("security")
            .args(["delete-generic-password", "-s", service, "-a", user_account])
            .output();

        // 写入新的 (-A 参数允许所有本地应用免密码、无感直接读取凭据)
        let output = Command::new("security")
            .args([
                "add-generic-password",
                "-s",
                service,
                "-a",
                user_account,
                "-w",
                &full_keyring_value,
                "-A",
            ])
            .output()
            .map_err(|e| format!("Failed to execute security command: {}", e))?;

        if !output.status.success() {
            let err_msg = String::from_utf8_lossy(&output.stderr);
            return Err(format!("macOS security command failed: {}", err_msg.trim()));
        }
    }

    #[cfg(target_os = "windows")]
    {
        // 2.2 Windows Credential Manager direct Win32 API calls to write raw UTF-8 bytes
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

        let target = format!("{}:{}", service, user_account);
        let user = user_account;
        let secret = payload_json.as_bytes();

        let target_wide: Vec<u16> = std::ffi::OsStr::new(&target)
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
            // Delete first to ensure we write clean
            let _ = CredDeleteW(target_wide.as_ptr(), 1, 0);

            let res = CredWriteW(&cred, 0);
            if res == 0 {
                let err = std::io::Error::last_os_error();
                return Err(format!("Windows CredWriteW failed: {}", err));
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        // 2.3 Linux Secret Service API
        use std::io::Write;
        use std::sync::mpsc;

        let store_to_collection = |collection_opt: Option<&str>,
                                   payload: &[u8]|
         -> Result<(), String> {
            let mut cmd = Command::new("secret-tool");
            cmd.arg("store");
            if let Some(col) = collection_opt {
                cmd.arg(format!("--collection={}", col));
                cmd.arg(format!(
                    "--label=Password for '{}' on '{}'",
                    user_account, service
                ));
            } else {
                cmd.arg(format!("--label={}", service));
            }
            cmd.args(["service", service, "username", user_account]);
            cmd.stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());

            let mut child = match cmd.spawn() {
                Ok(child) => child,
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::NotFound {
                        return Err(
                            "Linux Secret Service utility 'secret-tool' not found (未检测到 secret-tool 工具)。\n\
                             Please install libsecret-tools to enable Keyring credential storage:\n\
                             • Ubuntu / Debian: sudo apt install -y libsecret-tools\n\
                             • Fedora / RHEL: sudo dnf install -y libsecret\n\
                             • Arch Linux: sudo pacman -S libsecret"
                                .to_string(),
                        );
                    }
                    return Err(format!("Failed to spawn secret-tool: {}", e));
                }
            };

            if let Some(mut stdin) = child.stdin.take() {
                stdin
                    .write_all(payload)
                    .map_err(|e| format!("Failed to write to secret-tool stdin: {}", e))?;
            }

            let child_pid = child.id();
            let (tx, rx) = mpsc::channel::<Result<std::process::Output, std::io::Error>>();
            std::thread::spawn(move || {
                let _ = tx.send(child.wait_with_output());
            });

            let output = match rx.recv_timeout(std::time::Duration::from_secs(10)) {
                Ok(result) => {
                    result.map_err(|e| format!("Failed to wait for secret-tool: {}", e))?
                }
                Err(_) => {
                    let _ = Command::new("kill")
                        .args(["-9", &child_pid.to_string()])
                        .output();
                    crate::modules::logger::log_error(
                        "[Desktop] secret-tool store blocked for >10s — D-Bus session bus unreachable.",
                    );
                    return Err(
                        "Keyring write timed out (10s). The D-Bus session bus is not reachable from this process."
                            .to_string(),
                    );
                }
            };

            if !output.status.success() {
                let err_msg = String::from_utf8_lossy(&output.stderr);
                return Err(format!("Linux secret-tool failed: {}", err_msg.trim()));
            }

            Ok(())
        };

        // 1. 优先尝试写入 'login' 集合
        let login_res = store_to_collection(
            Some("/org/freedesktop/secrets/collection/login"),
            payload_json.as_bytes(),
        );

        // 2. 同时写入默认集合（保证其他依赖 default collection 的系统工具也能读取）
        let default_res = store_to_collection(None, payload_json.as_bytes());

        if user_account == "antigravity" {
            // 尝试优先同步写入本地文件凭据 (~/.gemini/oauth_creds.json)
            let _ = write_to_file_credentials(account, false);
        }

        // 若两者均失败，则返回错误；若至少一个成功，则记录并继续
        if login_res.is_err() && default_res.is_err() {
            return Err(login_res.unwrap_err());
        } else if let Err(e) = login_res {
            crate::modules::logger::log_warn(&format!(
                "[Desktop] Failed to write token to 'login' collection, falling back to default collection: {}",
                e
            ));
        } else {
            crate::modules::logger::log_info(
                "[Desktop] Successfully synced credential to Secret Service 'login' collection.",
            );
        }
    }

    crate::modules::logger::log_info(
        "[Desktop] Successfully wrote token to system credential store.",
    );

    Ok(())
}

/// 辅助方法：向宿主操作系统的 Keychain/Credentials Manager 写入 Token
pub(crate) fn write_to_system_keyring(account: &crate::models::Account) -> Result<(), String> {
    write_to_system_keyring_with_service_account(account, "gemini", "antigravity")?;

    // 同步写入 ~/.gemini/ 目录下的文件凭据，兼容 SSH 会话、容器环境和无 Keyring/D-Bus 场景
    if let Err(e) = write_to_file_credentials(account, false) {
        crate::modules::logger::log_warn(&format!("[Desktop] File credential sync warning: {}", e));
    }

    Ok(())
}

/// 向指定目录 ($GEMINI_HOME 或测试目录) 下的 antigravity-acp/ 写入 acp_token.json 以及 settings.json
pub fn write_acp_file_credentials_to_dir(
    gemini_dir: &std::path::Path,
    account: &crate::models::Account,
) -> Result<(), String> {
    let acp_dir = gemini_dir.join("antigravity-acp");
    if !acp_dir.exists() {
        std::fs::create_dir_all(&acp_dir)
            .map_err(|e| format!("Failed to create antigravity-acp directory: {}", e))?;
    }

    let (client_id, client_secret) =
        crate::modules::oauth::get_client_credentials_for_account(&account.token);
    let scopes = vec![
        "https://www.googleapis.com/auth/userinfo.email".to_string(),
        "https://www.googleapis.com/auth/userinfo.profile".to_string(),
        "https://www.googleapis.com/auth/cloud-platform".to_string(),
        "openid".to_string(),
    ];
    let project_id = account.token.project_id.clone().unwrap_or_default();

    #[derive(serde::Serialize, serde::Deserialize)]
    struct AcpTokenJson {
        client_id: String,
        client_secret: String,
        refresh_token: String,
        token_uri: String,
        scopes: Vec<String>,
        project_id: String,
    }

    let token_json = AcpTokenJson {
        client_id,
        client_secret,
        refresh_token: account.token.refresh_token.clone(),
        token_uri: "https://oauth2.googleapis.com/token".to_string(),
        scopes,
        project_id,
    };

    let token_file_path = acp_dir.join("acp_token.json");
    let serialized_token = serde_json::to_string_pretty(&token_json)
        .map_err(|e| format!("Failed to serialize acp_token.json: {}", e))?;
    std::fs::write(&token_file_path, serialized_token)
        .map_err(|e| format!("Failed to write acp_token.json: {}", e))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&token_file_path, std::fs::Permissions::from_mode(0o600));
    }

    // Ensure settings.json has {"auth": {"type": "oauth-personal"}}, preserving any existing settings
    let settings_path = acp_dir.join("settings.json");
    let mut settings_val: serde_json::Value = if settings_path.exists() {
        std::fs::read_to_string(&settings_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    if !settings_val.is_object() {
        settings_val = serde_json::json!({});
    }

    if let Some(obj) = settings_val.as_object_mut() {
        match obj.get_mut("auth") {
            Some(auth_val) if auth_val.is_object() => {
                if let Some(auth_obj) = auth_val.as_object_mut() {
                    auth_obj.insert("type".to_string(), serde_json::json!("oauth-personal"));
                }
            }
            _ => {
                obj.insert(
                    "auth".to_string(),
                    serde_json::json!({ "type": "oauth-personal" }),
                );
            }
        }
    }

    let serialized_settings = serde_json::to_string_pretty(&settings_val)
        .map_err(|e| format!("Failed to serialize settings.json: {}", e))?;
    std::fs::write(&settings_path, serialized_settings)
        .map_err(|e| format!("Failed to write settings.json: {}", e))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&settings_path, std::fs::Permissions::from_mode(0o600));
    }

    Ok(())
}

/// 向 $GEMINI_HOME/antigravity-acp/ 写入 acp_token.json 以及 settings.json
pub fn write_acp_file_credentials(account: &crate::models::Account) -> Result<(), String> {
    let gemini_home = get_gemini_home_dir()?;
    write_acp_file_credentials_to_dir(&gemini_home, account)
}

/// 同步注入 ACP 凭据（文件凭据 + Keyring gemini:antigravity-acp）
pub fn write_acp_credentials(account: &crate::models::Account) -> Result<(), String> {
    write_acp_file_credentials(account)?;
    if let Err(e) =
        write_to_system_keyring_with_service_account(account, "gemini", "antigravity-acp")
    {
        crate::modules::logger::log_warn(&format!(
            "[ACP] System keyring write failed: {}, file credentials active",
            e
        ));
    }
    crate::modules::logger::log_info(&format!(
        "[ACP] Successfully wrote credentials for {}",
        account.email
    ));
    Ok(())
}

/// 从指定目录 ($GEMINI_HOME 或测试目录) 下的 antigravity-acp/acp_token.json 读取 refresh_token
pub fn read_acp_token_file_from_dir(gemini_dir: &std::path::Path) -> Result<String, String> {
    let path = gemini_dir.join("antigravity-acp").join("acp_token.json");
    if !path.exists() {
        return Err("acp_token.json not found".to_string());
    }
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read acp_token.json: {}", e))?;
    let val: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse acp_token.json: {}", e))?;
    let rt = val
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "valid refresh_token not found in acp_token.json".to_string())?;
    Ok(rt.to_string())
}

/// 读取 $GEMINI_HOME/antigravity-acp/acp_token.json 的 refresh_token
pub fn read_acp_token_file() -> Result<String, String> {
    let gemini_home = get_gemini_home_dir()?;
    read_acp_token_file_from_dir(&gemini_home)
}

/// 辅助方法：向指定目录同步写入本地文件凭据
fn write_to_file_credentials_to_dir(
    gemini_dir: &std::path::Path,
    account: &crate::models::Account,
    sync_agy_cli: bool,
) -> Result<(), String> {
    if !gemini_dir.exists() {
        if let Err(e) = std::fs::create_dir_all(gemini_dir) {
            crate::modules::logger::log_warn(&format!(
                "[Desktop] Failed to create .gemini directory: {}",
                e
            ));
            return Err(format!("Failed to create .gemini directory: {}", e));
        }
    }

    let expiry_secs = if account.token.expiry_timestamp > 10_000_000_000 {
        account.token.expiry_timestamp / 1000
    } else {
        account.token.expiry_timestamp
    };
    let expiry_datetime =
        chrono::DateTime::from_timestamp(expiry_secs, 0).unwrap_or_else(|| chrono::Utc::now());
    let expiry_str = expiry_datetime.to_rfc3339_opts(chrono::SecondsFormat::Micros, true);

    #[derive(serde::Serialize)]
    struct AgyTokenDetails {
        access_token: String,
        token_type: String,
        refresh_token: String,
        expiry: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        expires_in: Option<i64>,
    }

    #[derive(serde::Serialize)]
    struct AgyOAuthTokenFile {
        token: AgyTokenDetails,
        auth_method: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        id_token_email: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        id_token: Option<String>,
    }

    let agy_token = AgyOAuthTokenFile {
        token: AgyTokenDetails {
            access_token: account.token.access_token.clone(),
            token_type: "Bearer".to_string(),
            refresh_token: account.token.refresh_token.clone(),
            expiry: expiry_str,
            expires_in: Some(3599),
        },
        auth_method: "consumer".to_string(),
        id_token_email: Some(account.email.clone()),
        id_token: account.token.id_token.clone(),
    };

    // 1. 同步写入 Standalone / IDE 语言服务原生降级凭据: ~/.gemini/jetski-standalone-oauth-token
    // 当系统 Keyring 不可用或超时时，agy 语言服务以此文件作为首要回退
    let standalone_token_path = gemini_dir.join("jetski-standalone-oauth-token");
    if let Ok(standalone_json_str) = serde_json::to_string_pretty(&agy_token) {
        if let Err(e) = std::fs::write(&standalone_token_path, &standalone_json_str) {
            crate::modules::logger::log_warn(&format!(
                "[Desktop] Failed to write jetski-standalone-oauth-token: {}",
                e
            ));
        } else {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(
                    &standalone_token_path,
                    std::fs::Permissions::from_mode(0o600),
                );
            }
            crate::modules::logger::log_info(&format!(
                "[Desktop] Successfully synced fallback credentials to ~/.gemini/jetski-standalone-oauth-token for: {}",
                account.email
            ));
        }
    }

    // 2. 同步写入 Antigravity CLI (agy) 原生文件凭据: ~/.gemini/antigravity-cli/antigravity-oauth-token
    // 兼容 SSH 会话、tmux、Docker 容器以及无 D-Bus 桌面环境
    // [FIX #3728] 仅在切换 agy CLI 时同步写入该专属凭证，严禁经典款切号盲目覆盖破坏多端账号隔离
    if sync_agy_cli {
        let agy_cli_dir = gemini_dir.join("antigravity-cli");
        if !agy_cli_dir.exists() {
            if let Err(e) = std::fs::create_dir_all(&agy_cli_dir) {
                crate::modules::logger::log_warn(&format!(
                    "[Desktop] Failed to create antigravity-cli directory: {}",
                    e
                ));
            }
        }

        let agy_token_path = agy_cli_dir.join("antigravity-oauth-token");
        if let Ok(agy_json_str) = serde_json::to_string_pretty(&agy_token) {
            if let Err(e) = std::fs::write(&agy_token_path, agy_json_str) {
                crate::modules::logger::log_warn(&format!(
                    "[Desktop] Failed to write antigravity-oauth-token: {}",
                    e
                ));
            } else {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(
                        &agy_token_path,
                        std::fs::Permissions::from_mode(0o600),
                    );
                }
                crate::modules::logger::log_info(&format!(
                    "[Desktop] Successfully synced file-based credentials to ~/.gemini/antigravity-cli/antigravity-oauth-token for: {}",
                    account.email
                ));
            }
        }
    }

    // 2. 同步写入 Gemini CLI 凭据 (~/.gemini/oauth_creds.json 以及 ~/.gemini/google_accounts.json)
    let expiry_ms = if account.token.expiry_timestamp > 10_000_000_000 {
        account.token.expiry_timestamp
    } else {
        account.token.expiry_timestamp * 1000
    };

    #[derive(serde::Serialize)]
    struct OAuthCredsFile {
        access_token: String,
        refresh_token: String,
        token_type: String,
        expiry_date: i64,
        #[serde(skip_serializing_if = "Option::is_none")]
        id_token: Option<String>,
        scope: String,
    }

    let creds = OAuthCredsFile {
        access_token: account.token.access_token.clone(),
        refresh_token: account.token.refresh_token.clone(),
        token_type: "Bearer".to_string(),
        expiry_date: expiry_ms,
        id_token: account.token.id_token.clone(),
        scope: "https://www.googleapis.com/auth/userinfo.email openid https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/userinfo.profile".to_string(),
    };

    let creds_path = gemini_dir.join("oauth_creds.json");
    let json_str = serde_json::to_string_pretty(&creds)
        .map_err(|e| format!("Failed to serialize oauth_creds JSON: {}", e))?;

    if let Err(e) = std::fs::write(&creds_path, json_str) {
        crate::modules::logger::log_warn(&format!(
            "[Desktop] Failed to write oauth_creds.json: {}",
            e
        ));
        return Err(format!("Failed to write oauth_creds.json: {}", e));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&creds_path, std::fs::Permissions::from_mode(0o600));
    }

    #[derive(serde::Serialize)]
    struct GoogleAccountsFile {
        active: String,
        old: Vec<String>,
    }

    let accounts_info = GoogleAccountsFile {
        active: account.email.clone(),
        old: vec![],
    };

    let accounts_path = gemini_dir.join("google_accounts.json");
    if let Ok(accounts_json_str) = serde_json::to_string_pretty(&accounts_info) {
        let _ = std::fs::write(&accounts_path, accounts_json_str);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ =
                std::fs::set_permissions(&accounts_path, std::fs::Permissions::from_mode(0o600));
        }
    }

    crate::modules::logger::log_info(&format!(
        "[Desktop] Successfully synced file-based credentials to ~/.gemini/oauth_creds.json for: {}",
        account.email
    ));

    Ok(())
}

/// 辅助方法：同步写入本地文件凭据 (~/.gemini/antigravity-cli/antigravity-oauth-token 以及 ~/.gemini/oauth_creds.json)
/// 用于在 SSH 会话、容器环境或无系统 Keyring / D-Bus 的场景下保障 CLI/工具的凭据兼容性
fn write_to_file_credentials(
    account: &crate::models::Account,
    sync_agy_cli: bool,
) -> Result<(), String> {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return Err("Failed to resolve user home directory".to_string()),
    };
    write_to_file_credentials_to_dir(&home.join(".gemini"), account, sync_agy_cli)
}

/// 辅助方法：从指定目录的本地文件凭据读取 Token 作为跨平台回退
fn read_from_file_credentials_from_dir(
    gemini_dir: &std::path::Path,
) -> Result<crate::modules::migration::ImportedOAuthState, String> {
    // 1. 优先尝试从 Antigravity CLI (agy) 原生文件凭据或 Standalone 回退凭据读取
    let candidate_paths = [
        gemini_dir
            .join("antigravity-cli")
            .join("antigravity-oauth-token"),
        gemini_dir.join("jetski-standalone-oauth-token"),
    ];
    for path in &candidate_paths {
        if path.exists() {
            if let Ok(content) = fs::read_to_string(path) {
                if let Ok(state) = parse_keyring_payload(&content) {
                    return Ok(state);
                }
            }
        }
    }

    // 2. 回退到 oauth_creds.json
    let creds_path = gemini_dir.join("oauth_creds.json");
    if !creds_path.exists() {
        return Err(
            "No file-based credentials found (~/.gemini/antigravity-cli/antigravity-oauth-token or ~/.gemini/oauth_creds.json)"
                .to_string(),
        );
    }
    let content = fs::read_to_string(&creds_path)
        .map_err(|e| format!("Failed to read oauth_creds.json: {}", e))?;
    let json: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse oauth_creds.json: {}", e))?;
    let refresh_token = json
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Refresh token not found in oauth_creds.json".to_string())?
        .to_string();
    Ok(crate::modules::migration::ImportedOAuthState {
        refresh_token,
        is_gcp_tos: true,
        project_id: None,
    })
}

/// 辅助方法：从本地文件凭据读取 Token 作为跨平台回退
fn read_from_file_credentials() -> Result<crate::modules::migration::ImportedOAuthState, String> {
    let home =
        dirs::home_dir().ok_or_else(|| "Failed to resolve user home directory".to_string())?;
    read_from_file_credentials_from_dir(&home.join(".gemini"))
}

/// 读取 Antigravity CLI (agy) 原生与备份文件凭据，严格限制仅扫描 Antigravity
pub fn read_antigravity_cli_credentials() -> Vec<crate::modules::migration::ImportedOAuthState> {
    let mut states = Vec::new();
    let Some(home) = dirs::home_dir() else {
        return states;
    };

    let cli_dir = home.join(".gemini").join("antigravity-cli");
    let candidate_files = [
        cli_dir.join("antigravity-oauth-token"),
        cli_dir.join("antigravity-oauth-token.antigravity.bak"),
        cli_dir.join("antigravity-oauth-token.bak"),
        home.join(".gemini").join("jetski-standalone-oauth-token"),
    ];

    let mut seen_tokens = std::collections::HashSet::new();
    for path in &candidate_files {
        if path.exists() {
            if let Ok(content) = fs::read_to_string(path) {
                if let Ok(state) = parse_keyring_payload(&content) {
                    if !state.refresh_token.is_empty()
                        && seen_tokens.insert(state.refresh_token.clone())
                    {
                        crate::modules::logger::log_info(&format!(
                            "[CLI-Scan] Discovered Antigravity CLI OAuth state in {:?}",
                            path
                        ));
                        states.push(state);
                    }
                }
            }
        }
    }

    states
}

/// 辅助方法：从宿主操作系统的 Keychain/Credentials Manager 读取 Token
pub fn read_from_system_keyring() -> Result<crate::modules::migration::ImportedOAuthState, String> {
    read_system_credentials(true)
}

/// Read only the system store: stale fallback files must not confirm an agy switch.
pub(crate) fn read_from_system_keyring_only(
) -> Result<crate::modules::migration::ImportedOAuthState, String> {
    read_system_credentials(false)
}

fn verify_agy_credentials(expected: &str, stored: &str) -> Result<(), String> {
    if expected.is_empty() || expected != stored {
        return Err(
            "Stored agy credentials do not match the selected account; the switch was not confirmed."
                .to_string(),
        );
    }
    Ok(())
}

pub fn read_from_system_keyring_target(
    service: &str,
    user_account: &str,
) -> Result<crate::modules::migration::ImportedOAuthState, String> {
    #[cfg(target_os = "macos")]
    {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let output = Command::new("security")
            .args([
                "find-generic-password",
                "-s",
                service,
                "-a",
                user_account,
                "-w",
            ])
            .output()
            .map_err(|e| format!("Failed to execute security command: {}", e))?;

        if !output.status.success() {
            return Err("No credential found in macOS Keychain".to_string());
        }

        let secret_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let payload_str = if secret_str.starts_with("go-keyring-base64:") {
            let b64_part = &secret_str["go-keyring-base64:".len()..];
            let decoded = STANDARD
                .decode(b64_part)
                .map_err(|e| format!("Base64 decode failed: {}", e))?;
            String::from_utf8(decoded).map_err(|e| format!("UTF-8 decode failed: {}", e))?
        } else {
            secret_str
        };

        return parse_keyring_payload(&payload_str);
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

        let target = format!("{}:{}", service, user_account);
        let target_wide: Vec<u16> = std::ffi::OsStr::new(&target)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let mut cred_ptr: *mut CREDENTIALW = ptr::null_mut();
        unsafe {
            let res = CredReadW(target_wide.as_ptr(), 1, 0, &mut cred_ptr);
            if res == 0 || cred_ptr.is_null() {
                return Err("No credential found in Windows Credential Manager".to_string());
            }

            let cred = &*cred_ptr;
            let blob = std::slice::from_raw_parts(
                cred.credential_blob,
                cred.credential_blob_size as usize,
            );
            let payload_str = String::from_utf8_lossy(blob).to_string();
            CredFree(cred_ptr as *mut std::ffi::c_void);

            return parse_keyring_payload(&payload_str);
        }
    }

    #[cfg(target_os = "linux")]
    {
        let output = match Command::new("secret-tool")
            .args(["lookup", "service", service, "username", user_account])
            .output()
        {
            Ok(out) => out,
            Err(e) => {
                if e.kind() == std::io::ErrorKind::NotFound {
                    return Err(
                        "Linux Secret Service utility 'secret-tool' not found (未检测到 secret-tool 工具)。\n\
                         Please install libsecret-tools to enable Keyring storage:\n\
                         • Ubuntu / Debian: sudo apt install -y libsecret-tools\n\
                         • Fedora / RHEL: sudo dnf install -y libsecret\n\
                         • Arch Linux: sudo pacman -S libsecret"
                            .to_string(),
                    );
                }
                return Err(format!("Failed to execute secret-tool: {}", e));
            }
        };

        if !output.status.success() {
            return Err("No credential found in Linux secret-tool".to_string());
        }

        let payload_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        return parse_keyring_payload(&payload_str);
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = service;
        let _ = user_account;
        Err("Keyring not supported on this operating system".to_string())
    }
}

fn read_system_credentials(
    allow_file_fallback: bool,
) -> Result<crate::modules::migration::ImportedOAuthState, String> {
    match read_from_system_keyring_target("gemini", "antigravity") {
        Ok(state) => Ok(state),
        Err(e) => {
            if allow_file_fallback {
                if let Ok(file_state) = read_from_file_credentials() {
                    return Ok(file_state);
                }
            }
            Err(e)
        }
    }
}

fn parse_keyring_payload(
    payload_str: &str,
) -> Result<crate::modules::migration::ImportedOAuthState, String> {
    let json: serde_json::Value = serde_json::from_str(payload_str)
        .map_err(|e| format!("Failed to parse keyring payload JSON: {}", e))?;

    let refresh_token = json
        .get("token")
        .and_then(|t| t.get("refresh_token"))
        .and_then(|v| v.as_str())
        .or_else(|| json.get("refresh_token").and_then(|v| v.as_str()))
        .ok_or_else(|| "Refresh Token not found in keyring payload".to_string())?
        .to_string();

    Ok(crate::modules::migration::ImportedOAuthState {
        refresh_token,
        is_gcp_tos: true,
        project_id: None,
    })
}

/// Headless/Docker 实现：仅执行数据层操作，忽略 UI 和进程控制
pub struct HeadlessIntegration;

impl SystemIntegration for HeadlessIntegration {
    async fn on_account_switch(
        &self,
        account: &crate::models::Account,
        _target_ide: Option<&str>,
    ) -> Result<(), String> {
        if _target_ide == Some("agy") {
            return Err(
                "Switching to the agy CLI is not supported in headless mode (no host keyring access)."
                    .to_string(),
            );
        }

        if _target_ide == Some("acp") {
            write_acp_file_credentials(account)?;
            let _ =
                write_to_system_keyring_with_service_account(account, "gemini", "antigravity-acp");
            crate::modules::logger::log_info(&format!(
                "[Headless] ACP Credentials written for {}",
                account.email
            ));
            return Ok(());
        }

        crate::modules::logger::log_info(&format!(
            "[Headless] Account switched in memory: {}",
            account.email
        ));
        // Docker 模式下通常不直接控制宿主机的 VS Code 进程
        // 如果需要同步配置 to 某个 volume，可以在此处添加逻辑
        Ok(())
    }

    fn update_tray(&self) {
        // No-op
    }

    fn show_notification(&self, title: &str, body: &str) {
        crate::modules::logger::log_info(&format!("[Log Notification] {}: {}", title, body));
    }
}

/// CLI 模式实现：支持向宿主操作系统的 Keychain/Keyring 写入凭据（用于终端命令行切号）
pub struct CliIntegration;

impl SystemIntegration for CliIntegration {
    async fn on_account_switch(
        &self,
        account: &crate::models::Account,
        target_ide: Option<&str>,
    ) -> Result<(), String> {
        if target_ide == Some("agy") {
            write_to_system_keyring(account)?;
            let _ = write_to_file_credentials(account, true);
            let stored = read_from_system_keyring_only()?;
            verify_agy_credentials(&account.token.refresh_token, &stored.refresh_token)?;

            if let Ok(storage_path) = device::get_storage_path(target_ide) {
                if let Some(ref profile) = account.device_profile {
                    let _ = device::write_profile(&storage_path, profile);
                }
            }
            return Ok(());
        }

        if target_ide == Some("acp") {
            write_acp_credentials(account)?;
            let killed = process::kill_acp_server_processes().unwrap_or(0);
            if killed > 0 {
                crate::modules::logger::log_info(&format!(
                    "[CLI] Terminated {} agy_acp_server process(es).",
                    killed
                ));
            }
            return Ok(());
        }

        let classic_running = process::is_antigravity_running(None);
        let ide_running = process::is_antigravity_running(Some("ide"));
        let classic_exe = process::get_antigravity_executable_path(Some("classic"));
        let ide_exe = process::get_antigravity_executable_path(Some("ide"));
        let ide_exe_str = ide_exe.as_ref().map(|p| p.to_string_lossy().to_string());

        let (_is_ide, effective_target) = resolve_effective_target(
            target_ide,
            classic_running,
            ide_running,
            classic_exe.is_some(),
            ide_exe_str.as_deref(),
        );

        let _ = write_to_system_keyring(account);
        if let Ok(db_path) = db::get_db_path(effective_target) {
            if db_path.exists() {
                let _ = db::inject_token(
                    &db_path,
                    &account.token.access_token,
                    &account.token.refresh_token,
                    account.token.expiry_timestamp,
                    &account.email,
                    account.token.is_gcp_tos,
                    account.token.project_id.as_deref(),
                    account.token.id_token.as_deref(),
                    account.token.oauth_client_key.as_deref(),
                    effective_target,
                );
            }
        }
        let _ = write_to_file_credentials(account, false);
        Ok(())
    }

    fn update_tray(&self) {}
    fn show_notification(&self, title: &str, body: &str) {
        crate::modules::logger::log_info(&format!("[CLI Notification] {}: {}", title, body));
    }
}

/// 系统集成管理器：替代 Arc<dyn SystemIntegration> 以解决 async trait 的 dyn 兼容性问题
#[derive(Clone)]
pub enum SystemManager {
    Desktop(tauri::AppHandle),
    Headless,
    Cli,
}

impl SystemManager {
    pub async fn on_account_switch(
        &self,
        account: &Account,
        target_ide: Option<&str>,
    ) -> Result<(), String> {
        match self {
            SystemManager::Desktop(handle) => {
                let integration = DesktopIntegration {
                    app_handle: handle.clone(),
                };
                integration.on_account_switch(account, target_ide).await
            }
            SystemManager::Headless => {
                let integration = HeadlessIntegration;
                integration.on_account_switch(account, target_ide).await
            }
            SystemManager::Cli => {
                let integration = CliIntegration;
                integration.on_account_switch(account, target_ide).await
            }
        }
    }

    pub fn update_tray(&self) {
        if let SystemManager::Desktop(handle) = self {
            let integration = DesktopIntegration {
                app_handle: handle.clone(),
            };
            integration.update_tray();
        }
    }

    pub fn show_notification(&self, title: &str, body: &str) {
        match self {
            SystemManager::Desktop(handle) => {
                let integration = DesktopIntegration {
                    app_handle: handle.clone(),
                };
                integration.show_notification(title, body);
            }
            SystemManager::Headless => {
                let integration = HeadlessIntegration;
                integration.show_notification(title, body);
            }
            SystemManager::Cli => {
                let integration = CliIntegration;
                integration.show_notification(title, body);
            }
        }
    }
}

impl SystemIntegration for SystemManager {
    async fn on_account_switch(
        &self,
        account: &crate::models::Account,
        target_ide: Option<&str>,
    ) -> Result<(), String> {
        self.on_account_switch(account, target_ide).await
    }

    fn update_tray(&self) {
        self.update_tray();
    }

    fn show_notification(&self, title: &str, body: &str) {
        self.show_notification(title, body);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agy_credentials_require_an_exact_nonempty_readback() {
        assert!(verify_agy_credentials("selected-token", "selected-token").is_ok());
        assert!(verify_agy_credentials("", "").is_err());
        let error = verify_agy_credentials("selected-token", "other-token").unwrap_err();
        assert!(!error.contains("selected-token"));
        assert!(!error.contains("other-token"));
    }

    #[test]
    fn test_parse_keyring_payload_nested_token() {
        let payload = r#"{
            "token": {
                "access_token": "ya29.test",
                "token_type": "Bearer",
                "refresh_token": "1//test_refresh_token_123",
                "expiry": "2026-09-19T10:00:00.000000Z"
            },
            "auth_method": "consumer"
        }"#;
        let state = parse_keyring_payload(payload).expect("Failed to parse nested keyring payload");
        assert_eq!(state.refresh_token, "1//test_refresh_token_123");
        assert!(state.is_gcp_tos);
    }

    #[test]
    fn test_parse_keyring_payload_flat_token() {
        let payload = r#"{
            "access_token": "ya29.test",
            "refresh_token": "1//test_refresh_token_flat"
        }"#;
        let state = parse_keyring_payload(payload).expect("Failed to parse flat keyring payload");
        assert_eq!(state.refresh_token, "1//test_refresh_token_flat");
    }

    #[test]
    fn test_parse_keyring_payload_missing_token() {
        let payload = r#"{ "auth_method": "consumer" }"#;
        let res = parse_keyring_payload(payload);
        assert!(res.is_err());
    }

    #[test]
    fn test_resolve_effective_target_explicit_classic() {
        // 显式指定 classic，即便 IDE 正在运行或只有 IDE exe，也必须严格判定为经典版
        let (is_ide, effective) = resolve_effective_target(
            Some("classic"),
            false,
            true,
            false,
            Some("/Applications/Antigravity IDE.app"),
        );
        assert!(!is_ide);
        assert_eq!(effective, Some("classic"));
    }

    #[test]
    fn test_resolve_effective_target_explicit_ide() {
        // 显式指定 ide，必须判定为 ide
        let (is_ide, effective) = resolve_effective_target(Some("ide"), true, false, true, None);
        assert!(is_ide);
        assert_eq!(effective, Some("ide"));
    }

    #[test]
    fn test_resolve_effective_target_explicit_acp() {
        // 显式指定 acp，必须判定为 acp
        let (is_ide, effective) = resolve_effective_target(
            Some("acp"),
            false,
            true,
            false,
            Some("/Applications/Antigravity IDE.app"),
        );
        assert!(!is_ide);
        assert_eq!(effective, Some("acp"));
    }

    #[test]
    fn test_resolve_effective_target_autodetect_classic_running() {
        // target_ide 为 None，经典版正在运行，必须优先保持经典版
        let (is_ide, effective) = resolve_effective_target(
            None,
            true,
            true,
            true,
            Some("/Applications/Antigravity IDE.app"),
        );
        assert!(!is_ide);
        assert_eq!(effective, None);
    }

    #[test]
    fn test_resolve_effective_target_autodetect_ide_running_only() {
        // target_ide 为 None，仅 IDE 正在运行，推导为 IDE
        let (is_ide, effective) = resolve_effective_target(
            None,
            false,
            true,
            true,
            Some("/Applications/Antigravity IDE.app"),
        );
        assert!(is_ide);
        assert_eq!(effective, Some("ide"));
    }

    #[test]
    fn test_resolve_effective_target_autodetect_classic_exe_exists() {
        // target_ide 为 None，两者均未运行，但经典版 exe 存在，优先经典版
        let (is_ide, effective) = resolve_effective_target(
            None,
            false,
            false,
            true,
            Some("/Applications/Antigravity IDE.app"),
        );
        assert!(!is_ide);
        assert_eq!(effective, None);
    }

    #[test]
    fn test_resolve_effective_target_autodetect_fallback_ide_exe() {
        // target_ide 为 None，两者均未运行，无经典版但有 IDE exe，推导为 IDE
        let (is_ide, effective) = resolve_effective_target(
            None,
            false,
            false,
            false,
            Some("/Applications/Antigravity IDE.app"),
        );
        assert!(is_ide);
        assert_eq!(effective, Some("ide"));

        // 测试下划线命名 antigravity_ide
        let (is_ide_underscore, effective_underscore) = resolve_effective_target(
            None,
            false,
            false,
            false,
            Some("/usr/local/bin/antigravity_ide"),
        );
        assert!(is_ide_underscore);
        assert_eq!(effective_underscore, Some("ide"));
    }

    #[test]
    fn test_resolve_effective_target_autodetect_default_fallback() {
        // target_ide 为 None，均未运行且均未检测到 exe，默认保底经典版
        let (is_ide, effective) = resolve_effective_target(None, false, false, false, None);
        assert!(!is_ide);
        assert_eq!(effective, None);
    }

    #[test]
    fn test_resolve_effective_target_detects_windows_ide_260_install_path() {
        // [FIX #3762] Windows 2.6.0 IDE 安装在 AppData\Local\Programs\Antigravity IDE，
        // 可执行文件名仍为 Antigravity.exe，必须靠安装目录的 IDE 命名推导为 IDE 目标。
        let (is_ide, effective) = resolve_effective_target(
            None,
            false,
            false,
            false,
            Some(r"C:\Users\user\AppData\Local\Programs\Antigravity IDE\Antigravity.exe"),
        );
        assert!(is_ide);
        assert_eq!(effective, Some("ide"));

        // 经典版同路径不得被推导为 IDE
        let (is_ide_classic, effective_classic) = resolve_effective_target(
            None,
            false,
            false,
            false,
            Some(r"C:\Users\user\AppData\Local\Programs\Antigravity\Antigravity.exe"),
        );
        assert!(!is_ide_classic);
        assert_eq!(effective_classic, None);
    }

    #[test]
    fn test_classic_credential_file_sync_writes_jetski_and_oauth_creds() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let gemini_dir = temp_dir.path().join(".gemini");

        let token = crate::models::TokenData::new(
            "ya29.classic_access_token".to_string(),
            "1//classic_refresh_token".to_string(),
            3600,
            Some("classic-user@example.com".to_string()),
            Some("proj-classic".to_string()),
            None,
            true,
            Some("id-token-classic".to_string()),
        );
        let account = crate::models::Account::new(
            "classic-acc-1".to_string(),
            "classic-user@example.com".to_string(),
            token,
        );

        // 模拟经典款切号写入凭据 (sync_agy_cli = false)
        let res = write_to_file_credentials_to_dir(&gemini_dir, &account, false);
        assert!(res.is_ok());

        // 1. 验证 jetski-standalone-oauth-token 写入成功且内容正确 (Fixes #3829)
        let standalone_path = gemini_dir.join("jetski-standalone-oauth-token");
        assert!(standalone_path.exists());
        let standalone_content = std::fs::read_to_string(&standalone_path).unwrap();
        let standalone_json: serde_json::Value = serde_json::from_str(&standalone_content).unwrap();
        assert_eq!(
            standalone_json["token"]["access_token"],
            "ya29.classic_access_token"
        );
        assert_eq!(
            standalone_json["token"]["refresh_token"],
            "1//classic_refresh_token"
        );
        assert_eq!(standalone_json["token"]["token_type"], "Bearer");
        assert_eq!(standalone_json["auth_method"], "consumer");
        assert_eq!(
            standalone_json["id_token_email"],
            "classic-user@example.com"
        );
        assert_eq!(standalone_json["id_token"], "id-token-classic");

        // 2. 验证 read_from_file_credentials_from_dir 能够以此文件作为原生回退读取
        let readback = read_from_file_credentials_from_dir(&gemini_dir).unwrap();
        assert_eq!(readback.refresh_token, "1//classic_refresh_token");

        // 3. 验证 oauth_creds.json 与 google_accounts.json 正确生成
        let oauth_path = gemini_dir.join("oauth_creds.json");
        assert!(oauth_path.exists());
        let oauth_content = std::fs::read_to_string(&oauth_path).unwrap();
        let oauth_json: serde_json::Value = serde_json::from_str(&oauth_content).unwrap();
        assert_eq!(oauth_json["refresh_token"], "1//classic_refresh_token");

        let accounts_path = gemini_dir.join("google_accounts.json");
        assert!(accounts_path.exists());
        let accounts_content = std::fs::read_to_string(&accounts_path).unwrap();
        let accounts_json: serde_json::Value = serde_json::from_str(&accounts_content).unwrap();
        assert_eq!(accounts_json["active"], "classic-user@example.com");

        // 4. 验证严格隔离：sync_agy_cli = false 时不得向 antigravity-cli 目录写入凭据
        let agy_cli_token_path = gemini_dir
            .join("antigravity-cli")
            .join("antigravity-oauth-token");
        assert!(
            !agy_cli_token_path.exists(),
            "Classic switch must NOT write antigravity-cli credentials"
        );
    }

    #[test]
    fn test_agy_cli_credential_sync_writes_dedicated_cli_token() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let gemini_dir = temp_dir.path().join(".gemini");

        let token = crate::models::TokenData::new(
            "ya29.agy_access_token".to_string(),
            "1//agy_refresh_token".to_string(),
            3600,
            Some("agy-user@example.com".to_string()),
            None,
            None,
            true,
            None,
        );
        let account = crate::models::Account::new(
            "agy-acc-1".to_string(),
            "agy-user@example.com".to_string(),
            token,
        );

        // 切换 agy CLI 时同步写入 (sync_agy_cli = true)
        let res = write_to_file_credentials_to_dir(&gemini_dir, &account, true);
        assert!(res.is_ok());

        let agy_cli_token_path = gemini_dir
            .join("antigravity-cli")
            .join("antigravity-oauth-token");
        assert!(agy_cli_token_path.exists());
        let content = std::fs::read_to_string(&agy_cli_token_path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(json["token"]["refresh_token"], "1//agy_refresh_token");
    }

    #[test]
    fn test_classic_sqlite_dual_write_preserves_and_updates_state_db() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let db_path = temp_dir.path().join("state.vscdb");

        // 初始化模拟 SQLite 数据库，包含初始旧数据
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute(
                "CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value TEXT)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO ItemTable (key, value) VALUES ('antigravityAuthStatus', 'old_auth_data')",
                [],
            )
            .unwrap();
        }

        let backup_path = db_path.with_extension("vscdb.backup");
        let _ = std::fs::copy(&db_path, &backup_path);
        assert!(backup_path.exists());

        // 注入新凭证与机器 ID
        let inject_res = crate::modules::db::inject_token(
            &db_path,
            "ya29.new_classic_token",
            "1//new_classic_refresh",
            1893456000,
            "new-classic@example.com",
            true,
            None,
            None,
            None,
            Some("classic"),
        );
        assert!(inject_res.is_ok());

        let machine_res =
            crate::modules::db::write_service_machine_id(&db_path, "test-mac-machine-id");
        assert!(machine_res.is_ok());

        // 验证主数据库已正确写入新 Token、用户状态、引导状态与机器指纹
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let oauth_token_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM ItemTable WHERE key = 'antigravityUnifiedStateSync.oauthToken'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(oauth_token_count, 1);

        let user_status_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM ItemTable WHERE key = 'antigravityUnifiedStateSync.userStatus'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(user_status_count, 1);

        let onboarding_val: String = conn
            .query_row(
                "SELECT value FROM ItemTable WHERE key = 'antigravityOnboarding'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(onboarding_val, "true");

        let machine_val: String = conn
            .query_row(
                "SELECT value FROM ItemTable WHERE key = 'telemetry.serviceMachineId'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(machine_val, "test-mac-machine-id");

        // 验证备份数据库保留了原始内容且未被污染
        let backup_conn = rusqlite::Connection::open(&backup_path).unwrap();
        let backup_oauth_count: i64 = backup_conn
            .query_row(
                "SELECT COUNT(*) FROM ItemTable WHERE key = 'antigravityUnifiedStateSync.oauthToken'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            backup_oauth_count, 0,
            "Backup DB must not contain newly injected oauthToken"
        );

        let backup_machine_count: i64 = backup_conn
            .query_row(
                "SELECT COUNT(*) FROM ItemTable WHERE key = 'telemetry.serviceMachineId'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            backup_machine_count, 0,
            "Backup DB must not contain newly injected machine ID"
        );

        let backup_old_auth: String = backup_conn
            .query_row(
                "SELECT value FROM ItemTable WHERE key = 'antigravityAuthStatus'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(backup_old_auth, "old_auth_data");
    }

    #[test]
    fn test_read_from_file_credentials_priority_hierarchy() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let gemini_dir = temp_dir.path().join(".gemini");
        std::fs::create_dir_all(gemini_dir.join("antigravity-cli")).unwrap();

        let agy_account = crate::models::Account::new(
            "agy-1".to_string(),
            "agy@example.com".to_string(),
            crate::models::TokenData::new(
                "ya29.agy_acc".to_string(),
                "1//agy_refresh".to_string(),
                3600,
                Some("agy@example.com".to_string()),
                None,
                None,
                true,
                None,
            ),
        );
        let standalone_account = crate::models::Account::new(
            "standalone-1".to_string(),
            "standalone@example.com".to_string(),
            crate::models::TokenData::new(
                "ya29.standalone_acc".to_string(),
                "1//standalone_refresh".to_string(),
                3600,
                Some("standalone@example.com".to_string()),
                None,
                None,
                true,
                None,
            ),
        );

        // 1. 同时写入 CLI 凭据与 Standalone 凭据，验证 CLI 凭据优先级最高
        let _ = write_to_file_credentials_to_dir(&gemini_dir, &agy_account, true);
        // 覆盖 standalone 凭据为不同值
        let _ = write_to_file_credentials_to_dir(&gemini_dir, &standalone_account, false);
        // 此时 antigravity-cli/antigravity-oauth-token 为 agy_refresh，jetski-standalone-oauth-token 为 standalone_refresh
        let cred = read_from_file_credentials_from_dir(&gemini_dir).unwrap();
        assert_eq!(
            cred.refresh_token, "1//agy_refresh",
            "CLI credentials must have highest priority over standalone"
        );

        // 2. 移除 CLI 凭据后，验证 jetski-standalone-oauth-token 优于 oauth_creds.json
        std::fs::remove_file(
            gemini_dir
                .join("antigravity-cli")
                .join("antigravity-oauth-token"),
        )
        .unwrap();
        let cred_standalone = read_from_file_credentials_from_dir(&gemini_dir).unwrap();
        assert_eq!(
            cred_standalone.refresh_token, "1//standalone_refresh",
            "Standalone oauth token must take priority over oauth_creds.json"
        );

        // 3. 移除 standalone 凭据后，验证回退到 oauth_creds.json
        std::fs::remove_file(gemini_dir.join("jetski-standalone-oauth-token")).unwrap();
        let cred_oauth = read_from_file_credentials_from_dir(&gemini_dir).unwrap();
        assert_eq!(
            cred_oauth.refresh_token, "1//standalone_refresh",
            "Should fallback to oauth_creds.json when standalone token missing"
        );

        // 4. 全部移除后，验证返回明确的错误
        std::fs::remove_file(gemini_dir.join("oauth_creds.json")).unwrap();
        let cred_err = read_from_file_credentials_from_dir(&gemini_dir);
        assert!(
            cred_err.is_err(),
            "Must return error when no credential files exist"
        );
    }

    #[test]
    fn test_classic_sqlite_injection_failure_handling() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let corrupt_db_path = temp_dir.path().join("corrupted_state.vscdb");

        // 写入损坏的非 SQLite 文件
        std::fs::write(&corrupt_db_path, b"not a valid sqlite database header").unwrap();

        let inject_res = crate::modules::db::inject_token(
            &corrupt_db_path,
            "ya29.token",
            "1//refresh",
            1893456000,
            "corrupt@example.com",
            true,
            None,
            None,
            None,
            Some("classic"),
        );

        // 必须优雅返回 Err 而不是 panic
        assert!(
            inject_res.is_err(),
            "Injecting token into corrupted DB must return Err gracefully"
        );
    }

    #[test]
    fn test_write_and_read_acp_file_credentials() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let gemini_dir = temp_dir.path();

        let mut token = crate::models::TokenData::new(
            "ya29.test-access-token".to_string(),
            "1//test-refresh-token".to_string(),
            3600,
            Some("test-acp@example.com".to_string()),
            Some("test-gcp-project".to_string()),
            None,
            false,
            None,
        );
        token.oauth_client_key = Some("antigravity_personal".to_string());

        let account = crate::models::Account {
            id: "acp-acc-1".to_string(),
            email: "test-acp@example.com".to_string(),
            name: Some("Test ACP".to_string()),
            priority: None,
            token,
            device_profile: None,
            device_history: None,
            quota: None,
            disabled: false,
            disabled_reason: None,
            disabled_at: None,
            proxy_disabled: false,
            proxy_disabled_reason: None,
            proxy_disabled_at: None,
            protected_models: None,
            forbidden_models: None,
            live_limited_models: None,
            custom_label: None,
            qos_status: None,
            qos_reset_seconds: None,
            validation_blocked: false,
            validation_blocked_until: None,
            validation_blocked_reason: None,
            validation_url: None,
            banned: false,
            banned_code: None,
            banned_reason: None,
            banned_at: None,
            ban_appeal_url: None,
            created_at: 1000,
            last_used: 2000,
        };

        // Pre-create settings.json with custom non-auth settings and existing auth properties to test preservation
        let acp_dir = gemini_dir.join("antigravity-acp");
        std::fs::create_dir_all(&acp_dir).unwrap();
        let initial_settings = serde_json::json!({
            "existing_flag": true,
            "auth": {
                "type": "custom-old",
                "keep_field": "val123"
            }
        });
        std::fs::write(
            acp_dir.join("settings.json"),
            serde_json::to_string(&initial_settings).unwrap(),
        )
        .unwrap();

        // 1. Write ACP file credentials
        write_acp_file_credentials_to_dir(gemini_dir, &account).unwrap();

        // 2. Validate acp_token.json contents
        let token_path = acp_dir.join("acp_token.json");
        assert!(token_path.exists());
        let token_raw = std::fs::read_to_string(&token_path).unwrap();
        let token_json: serde_json::Value = serde_json::from_str(&token_raw).unwrap();
        assert_eq!(
            token_json.get("refresh_token").and_then(|v| v.as_str()),
            Some("1//test-refresh-token")
        );
        assert_eq!(
            token_json.get("token_uri").and_then(|v| v.as_str()),
            Some("https://oauth2.googleapis.com/token")
        );
        assert_eq!(
            token_json.get("project_id").and_then(|v| v.as_str()),
            Some("test-gcp-project")
        );
        let scopes = token_json.get("scopes").and_then(|v| v.as_array()).unwrap();
        assert!(scopes.iter().any(|s| s.as_str() == Some("openid")));

        // 3. Validate settings.json non-destructive preservation
        let settings_raw = std::fs::read_to_string(acp_dir.join("settings.json")).unwrap();
        let settings_json: serde_json::Value = serde_json::from_str(&settings_raw).unwrap();
        assert_eq!(
            settings_json
                .get("auth")
                .and_then(|a| a.get("type"))
                .and_then(|v| v.as_str()),
            Some("oauth-personal")
        );
        assert_eq!(
            settings_json
                .get("auth")
                .and_then(|a| a.get("keep_field"))
                .and_then(|v| v.as_str()),
            Some("val123")
        );
        assert_eq!(
            settings_json.get("existing_flag").and_then(|v| v.as_bool()),
            Some(true)
        );

        // 4. Read back refresh token
        let rt = read_acp_token_file_from_dir(gemini_dir).unwrap();
        assert_eq!(rt, "1//test-refresh-token");

        // 5. Blank/whitespace refresh token must error
        std::fs::write(
            &token_path,
            r#"{"refresh_token": "   ", "token_uri": "https://oauth2.googleapis.com/token"}"#,
        )
        .unwrap();
        assert!(read_acp_token_file_from_dir(gemini_dir).is_err());
    }
}
