pub mod disclaimer;

use crate::types::TargetHost;
use anyhow::{Context, Result};
use rusqlite::Connection;
use std::fs;
use std::path::PathBuf;
use sysinfo::{ProcessesToUpdate, System};

pub struct SwitchEngine;

impl SwitchEngine {
    /// 执行针对特定单一目标或全部目标的切换
    pub fn switch(account: &crate::types::Account, target: TargetHost, force_restart: bool) -> Result<()> {
        match target {
            TargetHost::App => {
                Self::switch_desktop_target(account, "Antigravity", force_restart)?;
            }
            TargetHost::Ide => {
                Self::switch_desktop_target(account, "Antigravity IDE", force_restart)?;
            }
            TargetHost::Plugin => {
                Self::switch_acp_plugin(account)?;
            }
            TargetHost::Agy => {
                Self::switch_cli_agy(account)?;
            }
            TargetHost::All => {
                let _ = Self::switch_desktop_target(account, "Antigravity", force_restart);
                let _ = Self::switch_desktop_target(account, "Antigravity IDE", force_restart);
                let _ = Self::switch_acp_plugin(account);
                let _ = Self::switch_cli_agy(account);
            }
        }
        Ok(())
    }

    /// 解析指定应用在不同操作系统的配置存储根目录
    pub fn resolve_app_user_storage_dir(app_folder: &str) -> Option<PathBuf> {
        #[cfg(target_os = "macos")]
        {
            dirs::home_dir().map(|h| {
                h.join("Library/Application Support")
                    .join(app_folder)
                    .join("User/globalStorage")
            })
        }

        #[cfg(target_os = "windows")]
        {
            dirs::config_dir().map(|c| c.join(app_folder).join("User/globalStorage"))
        }

        #[cfg(target_os = "linux")]
        {
            dirs::config_dir().map(|c| c.join(app_folder).join("User/globalStorage"))
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            None
        }
    }

    /// 切换 Antigravity / Antigravity IDE 宿主 (深层隔离设备指纹 + SQLite 遥测，并热杀子进程)
    fn switch_desktop_target(
        account: &crate::types::Account,
        app_folder: &str,
        force_restart: bool,
    ) -> Result<()> {
        let user_storage = match Self::resolve_app_user_storage_dir(app_folder) {
            Some(p) if p.exists() => p,
            _ => return Ok(()), // 该宿主未安装或未初始化，安全跳过
        };

        // 1. 深度写入设备指纹到 storage.json
        if let Some(ref profile) = account.device_profile {
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
            // 核心深度细节：写入 storage.serviceMachineId 对齐 devDeviceId，防止底座基于 MAC 重新哈希计算
            root["storage.serviceMachineId"] =
                serde_json::Value::String(profile.dev_device_id.clone());

            let _ = fs::write(storage_json_path, serde_json::to_string_pretty(&root)?);
        }

        // 2. 深度写入 Token 与设备识别符到 state.vscdb
        let db_path = user_storage.join("state.vscdb");
        if db_path.exists() {
            let conn = Connection::open(&db_path)?;
            let json_state = serde_json::json!({
                "state": "signedIn",
                "context": {
                    "project": account.token.project_id.clone().unwrap_or_default(),
                    "showProjectError": false,
                    "errorMessage": "",
                },
                "user": {
                    "email": account.email,
                    "name": account.name.clone().unwrap_or_default()
                }
            });

            let state_str = serde_json::to_string(&json_state)?;
            conn.execute(
                "INSERT OR REPLACE INTO ItemTable (key, value) VALUES ('cloudcode.userState', ?1)",
                [&state_str],
            )?;

            // 写入 Protobuf 编码的 OAuth Token
            let pb_bytes = Self::encode_oauth_token_protobuf(&account.token.access_token, &account.token.refresh_token);
            conn.execute(
                "INSERT OR REPLACE INTO ItemTable (key, value) VALUES ('google.cloud-code.token', ?1)",
                [&pb_bytes],
            )?;

            // 同步写入设备机器码到 SQLite，并重置上一账号的旧遥测会话
            if let Some(ref profile) = account.device_profile {
                let _ = conn.execute(
                    "INSERT OR REPLACE INTO ItemTable (key, value) VALUES ('storage.serviceMachineId', ?1)",
                    [&profile.dev_device_id],
                );
                let _ = conn.execute(
                    "INSERT OR REPLACE INTO ItemTable (key, value) VALUES ('telemetry.serviceMachineId', ?1)",
                    [&profile.dev_device_id],
                );
                // 清理可能串联的会话标记
                let _ = conn.execute(
                    "DELETE FROM ItemTable WHERE key = 'telemetry.currentSessionId'",
                    [],
                );
            }

            // 4. 一键应用免二次确认与静默配置
            let _ = disclaimer::DisclaimerBypass::apply_to_db(&conn);
        }

        // 3. 执行重启或零停机热重载
        if force_restart {
            Self::restart_application(app_folder)?;
        } else {
            Self::hot_reload_by_killing_language_server(app_folder)?;
        }

        Ok(())
    }

    /// 切换 ACP 插件 (VSCode / Cursor 插件守护进程)
    fn switch_acp_plugin(account: &crate::types::Account) -> Result<()> {
        let home = dirs::home_dir().context("No home dir")?;
        let acp_dir = home.join(".gemini/antigravity-acp");
        if !acp_dir.exists() {
            fs::create_dir_all(&acp_dir)?;
        }

        let acp_token_path = acp_dir.join("acp_token.json");
        let token_payload = serde_json::json!({
            "access_token": account.token.access_token,
            "refresh_token": account.token.refresh_token,
            "expiry_timestamp": account.token.expiry_timestamp,
            "email": account.email,
            "project_id": account.token.project_id
        });

        fs::write(acp_token_path, serde_json::to_string_pretty(&token_payload)?)?;
        Self::kill_process_by_name("agy_acp_server");
        Ok(())
    }

    /// 切换 CLI 命令行 (agy)
    fn switch_cli_agy(account: &crate::types::Account) -> Result<()> {
        let home = dirs::home_dir().context("No home dir")?;
        let agy_dir = home.join(".gemini/antigravity");
        if !agy_dir.exists() {
            fs::create_dir_all(&agy_dir)?;
        }

        let active_account_path = agy_dir.join("active_account.json");
        let payload = serde_json::json!({
            "id": account.id,
            "email": account.email,
            "access_token": account.token.access_token,
            "refresh_token": account.token.refresh_token,
            "project_id": account.token.project_id,
            "updated_at": chrono::Utc::now().timestamp()
        });

        fs::write(active_account_path, serde_json::to_string_pretty(&payload)?)?;
        Ok(())
    }

    /// 编码 Google Cloud Code 识别的标准 Protobuf Token 结构
    fn encode_oauth_token_protobuf(access_token: &str, refresh_token: &str) -> Vec<u8> {
        let mut buf = Vec::new();
        // Field 1: access_token
        let acc_bytes = access_token.as_bytes();
        buf.push(0x0a);
        Self::encode_varint(acc_bytes.len() as u64, &mut buf);
        buf.extend_from_slice(acc_bytes);

        // Field 2: refresh_token
        let ref_bytes = refresh_token.as_bytes();
        buf.push(0x12);
        Self::encode_varint(ref_bytes.len() as u64, &mut buf);
        buf.extend_from_slice(ref_bytes);

        buf
    }

    fn encode_varint(mut val: u64, buf: &mut Vec<u8>) {
        while val >= 0x80 {
            buf.push((val as u8 & 0x7F) | 0x80);
            val >>= 7;
        }
        buf.push(val as u8);
    }

    /// 跨平台宿主探测与精准杀掉 language_server 触发零感知热重载
    pub fn hot_reload_by_killing_language_server(app_hint: &str) -> Result<()> {
        let mut sys = System::new_all();
        sys.refresh_processes(ProcessesToUpdate::All);

        let ls_names = ["language_server", "language_server.exe"];
        let app_lower = app_hint.to_lowercase();

        for (_pid, process) in sys.processes() {
            let proc_name = process.name().to_string_lossy().to_string();
            let matches_ls = ls_names.iter().any(|name| proc_name.eq_ignore_ascii_case(name));

            if matches_ls {
                let cmdline = process
                    .cmd()
                    .iter()
                    .map(|s| s.to_string_lossy())
                    .collect::<Vec<_>>()
                    .join(" ");

                // 关联当前宿主路径（如 Antigravity 或 Antigravity IDE）
                if cmdline.to_lowercase().contains(&app_lower) || cmdline.contains("cloudcode") {
                    let _ = process.kill();
                }
            }
        }
        Ok(())
    }

    /// 跨平台通过进程名关闭进程
    pub fn kill_process_by_name(name: &str) {
        let mut sys = System::new_all();
        sys.refresh_processes(ProcessesToUpdate::All);

        for (_pid, process) in sys.processes() {
            let proc_name = process.name().to_string_lossy();
            if proc_name.eq_ignore_ascii_case(name)
                || proc_name.eq_ignore_ascii_case(&format!("{}.exe", name))
            {
                let _ = process.kill();
            }
        }
    }

    /// 重启整个宿主应用
    pub fn restart_application(app_folder: &str) -> Result<()> {
        Self::kill_process_by_name(app_folder);
        std::thread::sleep(std::time::Duration::from_millis(500));

        #[cfg(target_os = "macos")]
        {
            let app_name = if app_folder.contains("IDE") {
                "Antigravity IDE.app"
            } else {
                "Antigravity.app"
            };
            let _ = std::process::Command::new("open")
                .arg("-a")
                .arg(app_name)
                .spawn();
        }

        #[cfg(target_os = "windows")]
        {
            if let Some(local_app) = dirs::data_local_dir() {
                let exe = local_app.join("Programs").join(app_folder).join(format!("{}.exe", app_folder));
                if exe.exists() {
                    let _ = std::process::Command::new(exe).spawn();
                }
            }
        }

        Ok(())
    }

    /// 探测各宿主在系统中的实时激活与运行状态 (跨平台)
    pub fn detect_active_hosts() -> crate::types::ActiveHostStatus {
        let mut sys = System::new_all();
        sys.refresh_processes(ProcessesToUpdate::All);

        let mut app_running = false;
        let mut ide_running = false;
        let mut plugin_running = false;

        let app_patterns = ["Antigravity", "Antigravity.exe", "antigravity"];
        let ide_patterns = ["Antigravity IDE", "Antigravity IDE.exe", "antigravity-ide"];

        for (_, process) in sys.processes() {
            let name = process.name().to_string_lossy();
            if ide_patterns.iter().any(|p| name.eq_ignore_ascii_case(p)) {
                ide_running = true;
            } else if app_patterns.iter().any(|p| name.eq_ignore_ascii_case(p)) {
                app_running = true;
            }

            if name.eq_ignore_ascii_case("agy_acp_server")
                || name.eq_ignore_ascii_case("agy_acp_server.exe")
            {
                plugin_running = true;
            }
        }

        crate::types::ActiveHostStatus {
            app_account: None,
            app_running,
            ide_account: None,
            ide_running,
            plugin_account: None,
            plugin_running,
            agy_account: None,
        }
    }
}
