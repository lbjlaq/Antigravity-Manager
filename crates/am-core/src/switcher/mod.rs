use crate::types::{Account, ActiveHostStatus, TargetHost};
use anyhow::{Context, Result};
use rusqlite::Connection;
use std::fs;
use sysinfo::{ProcessRefreshKind, RefreshKind, System};

pub struct SwitchEngine;

impl SwitchEngine {
    /// 执行热切换核心逻辑
    pub fn switch(account: &Account, target: TargetHost, restart: bool) -> Result<()> {
        let targets = match target {
            TargetHost::All => vec![
                TargetHost::App,
                TargetHost::Ide,
                TargetHost::Plugin,
                TargetHost::Agy,
            ],
            single => vec![single],
        };

        for t in targets {
            match t {
                TargetHost::App => Self::switch_desktop_target(account, "Antigravity", restart)?,
                TargetHost::Ide => {
                    Self::switch_desktop_target(account, "Antigravity IDE", restart)?
                }
                TargetHost::Plugin => Self::switch_plugin_target(account)?,
                TargetHost::Agy => Self::switch_cli_target(account)?,
                TargetHost::All => unreachable!(),
            }
        }

        Ok(())
    }

    /// 切换 Antigravity / Antigravity IDE 宿主 (写 state.vscdb + storage.json，并热杀子进程)
    fn switch_desktop_target(account: &Account, app_folder: &str, force_restart: bool) -> Result<()> {
        let home = dirs::home_dir().context("No home dir")?;
        let user_storage = home
            .join("Library/Application Support")
            .join(app_folder)
            .join("User/globalStorage");

        if !user_storage.exists() {
            // 如果该目标未安装/未初始化，则跳过
            return Ok(());
        }

        // 1. 写入设备指纹到 storage.json
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

            let _ = fs::write(storage_json_path, serde_json::to_string_pretty(&root)?);
        }

        // 2. 写入 Token 到 state.vscdb
        let db_path = user_storage.join("state.vscdb");
        if db_path.exists() {
            let conn = Connection::open(&db_path)?;
            // 构造 state 字符串
            let json_state = serde_json::json!({
                "state": "signedIn",
                "context": {
                    "project": account.token.project_id.clone().unwrap_or_default(),
                    "showProjectError": false,
                    "errorMessage": "",
                    "ineligibleMessage": ""
                },
                "token": {
                    "accessToken": account.token.access_token,
                    "refreshToken": account.token.refresh_token,
                    "expiry": account.token.expiry_timestamp.unwrap_or(0) * 1000
                }
            })
            .to_string();

            // 生成 Protobuf 统一主题格式
            let proto_b64 = encode_oauth_token_topic(&json_state);

            conn.execute(
                "INSERT INTO ItemTable (key, value) VALUES ('antigravityUnifiedStateSync.oauthToken', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = ?1",
                [&proto_b64],
            )?;

            // 保持 storage.serviceMachineId 与 devDeviceId 一致
            if let Some(ref profile) = account.device_profile {
                let _ = conn.execute(
                    "INSERT INTO ItemTable (key, value) VALUES ('storage.serviceMachineId', ?1)
                     ON CONFLICT(key) DO UPDATE SET value = ?1",
                    [&profile.dev_device_id],
                );
            }
        }

        // 3. 热切换处理：优先仅终止 language_server 子进程；若要求 restart 则杀主进程重启
        if force_restart {
            kill_processes_by_pattern(app_folder);
        } else {
            // 原地无感热切：只杀后台 language_server 子进程
            let killed = kill_processes_by_pattern("language_server");
            tracing::info!(
                "Hot-switched account for {}. Terminated {} language_server subprocess(es).",
                app_folder,
                killed
            );
        }

        Ok(())
    }

    /// 切换 IDE 插件版 (antigravity-acp)
    fn switch_plugin_target(account: &Account) -> Result<()> {
        let home = dirs::home_dir().context("No home dir")?;
        let acp_dir = home.join(".gemini/antigravity-acp");
        if !acp_dir.exists() {
            fs::create_dir_all(&acp_dir)?;
        }

        let token_path = acp_dir.join("acp_token.json");
        let cred = serde_json::json!({
            "access_token": account.token.access_token,
            "refresh_token": account.token.refresh_token,
            "token_type": "Bearer",
            "expiry": account.token.expiry_timestamp.unwrap_or(0),
            "email": account.email
        });
        fs::write(token_path, serde_json::to_string_pretty(&cred)?)?;

        // 终止后台正在运行的 agy_acp_server
        let killed = kill_processes_by_pattern("agy_acp_server");
        tracing::info!("Plugin switched. Terminated {} agy_acp_server(s).", killed);
        Ok(())
    }

    /// 切换 CLI 终端 (agy)
    fn switch_cli_target(account: &Account) -> Result<()> {
        let home = dirs::home_dir().context("No home dir")?;
        let agy_dir = home.join(".gemini/antigravity");
        if !agy_dir.exists() {
            fs::create_dir_all(&agy_dir)?;
        }
        let cred = serde_json::json!({
            "email": account.email,
            "refresh_token": account.token.refresh_token,
            "access_token": account.token.access_token
        });
        fs::write(
            agy_dir.join("active_account.json"),
            serde_json::to_string_pretty(&cred)?,
        )?;
        Ok(())
    }

    /// 探测各宿主当前活跃运行状态
    pub fn detect_active_hosts() -> ActiveHostStatus {
        let mut sys = System::new_with_specifics(
            RefreshKind::new().with_processes(ProcessRefreshKind::everything()),
        );
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All);

        let mut app_running = false;
        let mut ide_running = false;
        let mut plugin_running = false;

        for process in sys.processes().values() {
            let name = process.name().to_string_lossy();
            if name.contains("Antigravity IDE") {
                ide_running = true;
            } else if name.contains("Antigravity") && !name.contains("IDE") {
                app_running = true;
            } else if name.contains("agy_acp_server") {
                plugin_running = true;
            }
        }

        ActiveHostStatus {
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

/// 简单 Protobuf 打包 helper
fn encode_oauth_token_topic(json_state: &str) -> String {
    let mut buf = Vec::new();
    // Sentinel Key
    let key = b"authStateWithContextSentinelKey";
    let val = json_state.as_bytes();

    // Field 1: key (tag 10: (1 << 3) | 2)
    buf.push(10);
    write_varint(&mut buf, key.len() as u64);
    buf.extend_from_slice(key);

    // Field 2: value (tag 18: (2 << 3) | 2)
    buf.push(18);
    let mut val_buf = Vec::new();
    val_buf.push(10);
    write_varint(&mut val_buf, val.len() as u64);
    val_buf.extend_from_slice(val);

    write_varint(&mut buf, val_buf.len() as u64);
    buf.extend_from_slice(&val_buf);

    let mut outer = Vec::new();
    outer.push(10);
    write_varint(&mut outer, buf.len() as u64);
    outer.extend_from_slice(&buf);

    base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &outer)
}

fn write_varint(buf: &mut Vec<u8>, mut val: u64) {
    while val >= 0x80 {
        buf.push(((val & 0x7F) | 0x80) as u8);
        val >>= 7;
    }
    buf.push(val as u8);
}

fn kill_processes_by_pattern(pattern: &str) -> usize {
    let mut sys = System::new_with_specifics(
        RefreshKind::new().with_processes(ProcessRefreshKind::everything()),
    );
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All);

    let mut count = 0;
    for process in sys.processes().values() {
        let name = process.name().to_string_lossy();
        if name.contains(pattern) {
            let _ = process.kill();
            count += 1;
        }
    }
    count
}
