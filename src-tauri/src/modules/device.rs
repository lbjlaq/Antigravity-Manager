use crate::models::DeviceProfile;
use crate::modules::{logger, process};
use chrono::Local;
use rand::{distributions::Alphanumeric, Rng};
use rusqlite::Connection;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

const DATA_DIR: &str = ".antigravity_tools";
const GLOBAL_BASELINE: &str = "device_original.json";

fn get_data_dir() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("无法获取用户主目录")?;
    let data_dir = home.join(DATA_DIR);
    if !data_dir.exists() {
        fs::create_dir_all(&data_dir).map_err(|e| format!("创建数据目录失败: {}", e))?;
    }
    Ok(data_dir)
}

/// 寻找 storage.json 路径（优先自定义/便携路径）
pub fn get_storage_path() -> Result<PathBuf, String> {
    // 1) --user-data-dir 参数
    if let Some(user_data_dir) = process::get_user_data_dir_from_process() {
        let path = user_data_dir
            .join("User")
            .join("globalStorage")
            .join("storage.json");
        if path.exists() {
            return Ok(path);
        }
    }

    // 2) 便携模式（基于可执行文件的 data/user-data）
    if let Some(exe_path) = process::get_antigravity_executable_path() {
        if let Some(parent) = exe_path.parent() {
            let portable = parent
                .join("data")
                .join("user-data")
                .join("User")
                .join("globalStorage")
                .join("storage.json");
            if portable.exists() {
                return Ok(portable);
            }
        }
    }

    // 3) 标准安装位置
    #[cfg(target_os = "macos")]
    {
        let home = dirs::home_dir().ok_or("无法获取 Home 目录")?;
        let path =
            home.join("Library/Application Support/Antigravity/User/globalStorage/storage.json");
        if path.exists() {
            return Ok(path);
        }
    }

    #[cfg(target_os = "windows")]
    {
        let appdata =
            std::env::var("APPDATA").map_err(|_| "无法获取 APPDATA 环境变量".to_string())?;
        let path = PathBuf::from(appdata).join("Antigravity\\User\\globalStorage\\storage.json");
        if path.exists() {
            return Ok(path);
        }
    }

    #[cfg(target_os = "linux")]
    {
        let home = dirs::home_dir().ok_or("无法获取 Home 目录")?;
        let path = home.join(".config/Antigravity/User/globalStorage/storage.json");
        if path.exists() {
            return Ok(path);
        }
    }

    Err("未找到 storage.json，请确认 Antigravity 已运行过并生成配置文件".to_string())
}

/// 获取 storage.json 所在目录
pub fn get_storage_dir() -> Result<PathBuf, String> {
    let path = get_storage_path()?;
    path.parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| "无法获取 storage.json 所在目录".to_string())
}

/// 获取 state.vscdb 路径（与 storage.json 同目录）
pub fn get_state_db_path() -> Result<PathBuf, String> {
    let dir = get_storage_dir()?;
    Ok(dir.join("state.vscdb"))
}

fn get_machine_id_path() -> Result<PathBuf, String> {
    // 1) --user-data-dir 参数
    if let Some(user_data_dir) = process::get_user_data_dir_from_process() {
        return Ok(user_data_dir.join("machineid"));
    }

    // 2) 便携模式（基于可执行文件的 data/user-data）
    if let Some(exe_path) = process::get_antigravity_executable_path() {
        if let Some(parent) = exe_path.parent() {
            return Ok(parent.join("data").join("user-data").join("machineid"));
        }
    }

    // 3) 标准安装位置
    #[cfg(target_os = "macos")]
    {
        let home = dirs::home_dir().ok_or("无法获取 Home 目录")?;
        return Ok(home.join("Library/Application Support/Antigravity/machineid"));
    }

    #[cfg(target_os = "windows")]
    {
        let appdata =
            std::env::var("APPDATA").map_err(|_| "无法获取 APPDATA 环境变量".to_string())?;
        return Ok(PathBuf::from(appdata).join("Antigravity\\machineid"));
    }

    #[cfg(target_os = "linux")]
    {
        let home = dirs::home_dir().ok_or("无法获取 Home 目录")?;
        return Ok(home.join(".config/Antigravity/machineid"));
    }

    #[allow(unreachable_code)]
    Err("无法确定 machineid 路径".to_string())
}

/// UUID v4 格式正则验证（与官方 Antigravity 一致）
fn is_valid_uuid(value: &str) -> bool {
    use once_cell::sync::Lazy;
    use regex::Regex;
    static UUID_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"(?i)^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$").unwrap()
    });
    UUID_RE.is_match(value.trim())
}

/// 验证并规范化 serviceMachineId（必须是有效 UUID）
fn validate_service_machine_id(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if is_valid_uuid(trimmed) {
        Some(trimmed.to_string())
    } else {
        None
    }
}

fn read_machine_id_file() -> Option<String> {
    let path = get_machine_id_path().ok()?;
    let content = fs::read_to_string(&path).ok()?;
    validate_service_machine_id(&content)
}

fn write_machine_id_file(service_id: &str) -> Result<(), String> {
    let path = get_machine_id_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建 machineid 目录失败: {}", e))?;
    }
    fs::write(&path, service_id).map_err(|e| format!("写入 machineid 失败: {}", e))?;
    Ok(())
}

fn read_state_service_machine_id_value() -> Option<String> {
    let db_path = get_state_db_path().ok()?;
    if !db_path.exists() {
        return None;
    }
    let conn = Connection::open(&db_path).ok()?;
    let value: Result<String, _> = conn.query_row(
        "SELECT value FROM ItemTable WHERE key = 'storage.serviceMachineId'",
        [],
        |row| row.get(0),
    );
    value.ok().and_then(|v| validate_service_machine_id(&v))
}

fn generate_service_machine_id() -> String {
    Uuid::new_v4().to_string()
}

/// 获取 serviceMachineId（官方优先级：数据库 → 文件 → 生成）
pub fn get_service_machine_id() -> String {
    // 1. 优先从数据库读取
    if let Some(id) = read_state_service_machine_id_value() {
        return id;
    }
    // 2. 其次从 machineid 文件读取
    if let Some(id) = read_machine_id_file() {
        // 同步到数据库
        let _ = sync_state_service_machine_id_value(&id);
        return id;
    }
    // 3. 生成新的并写入
    let new_id = generate_service_machine_id();
    if let Err(e) = write_machine_id_file(&new_id) {
        logger::log_warn(&format!("写入 machineid 失败: {}", e));
    }
    let _ = sync_state_service_machine_id_value(&new_id);
    new_id
}

/// 确保 profile 有有效的 serviceMachineId（使用 UUID 格式验证，每个账号独立）
pub fn ensure_service_machine_id(profile: &mut DeviceProfile) -> bool {
    match validate_service_machine_id(&profile.service_machine_id) {
        Some(value) => {
            if value != profile.service_machine_id {
                profile.service_machine_id = value;
                return true;
            }
            false
        }
        None => {
            // 直接生成新的 UUID（每个账号独立）
            profile.service_machine_id = generate_service_machine_id();
            true
        }
    }
}

/// 备份 storage.json，返回备份文件路径
#[allow(dead_code)]
pub fn backup_storage(storage_path: &Path) -> Result<PathBuf, String> {
    if !storage_path.exists() {
        return Err(format!("storage.json 不存在: {:?}", storage_path));
    }
    let dir = storage_path
        .parent()
        .ok_or_else(|| "无法获取 storage.json 的父目录".to_string())?;
    let backup_path = dir.join(format!(
        "storage.json.backup_{}",
        Local::now().format("%Y%m%d_%H%M%S")
    ));
    fs::copy(storage_path, &backup_path).map_err(|e| format!("备份 storage.json 失败: {}", e))?;
    Ok(backup_path)
}

/// 从 storage.json 读取当前设备指纹
#[allow(dead_code)]
pub fn read_profile(storage_path: &Path) -> Result<DeviceProfile, String> {
    let content =
        fs::read_to_string(storage_path).map_err(|e| format!("读取 storage.json 失败: {}", e))?;
    let json: Value =
        serde_json::from_str(&content).map_err(|e| format!("解析 storage.json 失败: {}", e))?;

    // 支持嵌套 telemetry 或扁平 telemetry.xxx
    let get_field = |key: &str| -> Option<String> {
        if let Some(obj) = json.get("telemetry").and_then(|v| v.as_object()) {
            if let Some(v) = obj.get(key).and_then(|v| v.as_str()) {
                return Some(v.to_string());
            }
        }
        if let Some(v) = json
            .get(format!("telemetry.{key}"))
            .and_then(|v| v.as_str())
        {
            return Some(v.to_string());
        }
        None
    };

    // serviceMachineId 使用官方优先级：数据库 → 文件 → 生成（不从 storage.json 读取）
    let service_machine_id = get_service_machine_id();

    Ok(DeviceProfile {
        machine_id: get_field("machineId").ok_or("缺少 telemetry.machineId")?,
        mac_machine_id: get_field("macMachineId").ok_or("缺少 telemetry.macMachineId")?,
        dev_device_id: get_field("devDeviceId").ok_or("缺少 telemetry.devDeviceId")?,
        sqm_id: get_field("sqmId").ok_or("缺少 telemetry.sqmId")?,
        service_machine_id,
    })
}

/// 将设备指纹写入 storage.json
pub fn write_profile(storage_path: &Path, profile: &DeviceProfile) -> Result<(), String> {
    if !storage_path.exists() {
        return Err(format!("storage.json 不存在: {:?}", storage_path));
    }

    let content =
        fs::read_to_string(storage_path).map_err(|e| format!("读取 storage.json 失败: {}", e))?;
    let mut json: Value =
        serde_json::from_str(&content).map_err(|e| format!("解析 storage.json 失败: {}", e))?;

    // 确保 telemetry 是对象
    if !json.get("telemetry").map_or(false, |v| v.is_object()) {
        if json.as_object_mut().is_some() {
            json["telemetry"] = serde_json::json!({});
        } else {
            return Err("storage.json 顶层不是对象，无法写入 telemetry".to_string());
        }
    }

    if let Some(telemetry) = json.get_mut("telemetry").and_then(|v| v.as_object_mut()) {
        telemetry.insert(
            "machineId".to_string(),
            Value::String(profile.machine_id.clone()),
        );
        telemetry.insert(
            "macMachineId".to_string(),
            Value::String(profile.mac_machine_id.clone()),
        );
        telemetry.insert(
            "devDeviceId".to_string(),
            Value::String(profile.dev_device_id.clone()),
        );
        telemetry.insert("sqmId".to_string(), Value::String(profile.sqm_id.clone()));
    } else {
        return Err("telemetry 字段不是对象，写入失败".to_string());
    }

    // 同时写入扁平键，兼容旧格式
    if let Some(map) = json.as_object_mut() {
        map.insert(
            "telemetry.machineId".to_string(),
            Value::String(profile.machine_id.clone()),
        );
        map.insert(
            "telemetry.macMachineId".to_string(),
            Value::String(profile.mac_machine_id.clone()),
        );
        map.insert(
            "telemetry.devDeviceId".to_string(),
            Value::String(profile.dev_device_id.clone()),
        );
        map.insert(
            "telemetry.sqmId".to_string(),
            Value::String(profile.sqm_id.clone()),
        );
    }

    // serviceMachineId 使用官方优先级获取或验证（不写入 storage.json）
    let service_machine_id = match validate_service_machine_id(&profile.service_machine_id) {
        Some(value) => value,
        None => {
            let generated = get_service_machine_id();
            logger::log_warn("serviceMachineId 无效，已从官方来源获取或生成新值");
            generated
        }
    };

    let updated = serde_json::to_string_pretty(&json)
        .map_err(|e| format!("序列化 storage.json 失败: {}", e))?;
    fs::write(storage_path, updated).map_err(|e| format!("写入 storage.json 失败: {}", e))?;
    logger::log_info("已写入设备指纹到 storage.json");

    if let Err(e) = write_machine_id_file(&service_machine_id) {
        logger::log_warn(&format!("写入 machineid 失败: {}", e));
    }

    // 同步 state.vscdb 的 ItemTable.storage.serviceMachineId
    let _ = sync_state_service_machine_id_value(&service_machine_id);
    Ok(())
}

/// 同步 serviceMachineId 到 machineid 文件和数据库（不操作 storage.json）
#[allow(dead_code)]
pub fn sync_service_machine_id(_storage_path: &Path, service_id: &str) -> Result<(), String> {
    let service_id = validate_service_machine_id(service_id).ok_or("serviceMachineId 无效（非 UUID 格式）")?;

    if let Err(e) = write_machine_id_file(&service_id) {
        logger::log_warn(&format!("写入 machineid 失败: {}", e));
    }

    sync_state_service_machine_id_value(&service_id)?;
    logger::log_info("已同步 serviceMachineId 到 machineid 文件和数据库");
    Ok(())
}

/// 确保 serviceMachineId 已同步（使用官方优先级：数据库 → 文件 → 生成）
#[allow(dead_code)]
pub fn sync_service_machine_id_from_storage(_storage_path: &Path) -> Result<(), String> {
    // 使用官方优先级获取 serviceMachineId，不再从 storage.json 读取
    let service_id = get_service_machine_id();
    
    // 确保已写入 machineid 文件
    if let Err(e) = write_machine_id_file(&service_id) {
        logger::log_warn(&format!("写入 machineid 失败: {}", e));
    }
    
    // 确保已写入数据库
    sync_state_service_machine_id_value(&service_id)?;
    
    logger::log_info("已确保 serviceMachineId 同步到 machineid 文件和数据库");
    Ok(())
}

fn sync_state_service_machine_id_value(service_id: &str) -> Result<(), String> {
    let db_path = get_state_db_path()?;
    if !db_path.exists() {
        logger::log_warn(&format!(
            "state.vscdb 不存在，跳过 serviceMachineId 同步: {:?}",
            db_path
        ));
        return Ok(());
    }

    let conn = Connection::open(&db_path).map_err(|e| format!("打开 state.vscdb 失败: {}", e))?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS ItemTable (key TEXT PRIMARY KEY, value TEXT);",
        [],
    )
    .map_err(|e| format!("创建 ItemTable 失败: {}", e))?;
    conn.execute(
        "INSERT OR REPLACE INTO ItemTable (key, value) VALUES ('storage.serviceMachineId', ?1);",
        [service_id],
    )
    .map_err(|e| format!("写入 storage.serviceMachineId 失败: {}", e))?;
    logger::log_info("已同步 storage.serviceMachineId 至 state.vscdb");
    Ok(())
}

/// 全局原始指纹（所有账号共享）的存取
pub fn load_global_original() -> Option<DeviceProfile> {
    if let Ok(dir) = get_data_dir() {
        let path = dir.join(GLOBAL_BASELINE);
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(mut profile) = serde_json::from_str::<DeviceProfile>(&content) {
                    // 如果原始备份里没有 serviceMachineId（旧版本备份），尝试从当前系统获取补充
                    // 假设当前系统还没被乱改，或者至少比随机生成要好
                    if !is_valid_uuid(&profile.service_machine_id) {
                        let sys_id = get_service_machine_id();
                        logger::log_info(&format!("原始备份缺少 serviceMachineId，已从系统补充: {}", sys_id));
                        profile.service_machine_id = sys_id;
                        
                        // 回写更新原始备份文件，确保以后能永久记住这个 ID
                        let _ = save_global_original_force(&profile);
                    }
                    return Some(profile);
                }
            }
        }
    }
    None
}

/// 强制保存原始指纹（覆盖）
fn save_global_original_force(profile: &DeviceProfile) -> Result<(), String> {
    let dir = get_data_dir()?;
    let path = dir.join(GLOBAL_BASELINE);
    let content = serde_json::to_string_pretty(profile).map_err(|e| format!("序列化原始指纹失败: {}", e))?;
    fs::write(&path, content).map_err(|e| format!("更新原始指纹失败: {}", e))
}

pub fn save_global_original(profile: &DeviceProfile) -> Result<(), String> {
    let dir = get_data_dir()?;
    let path = dir.join(GLOBAL_BASELINE);
    if path.exists() {
        return Ok(()); // 已存在则不覆盖
    }
    let content =
        serde_json::to_string_pretty(profile).map_err(|e| format!("序列化原始指纹失败: {}", e))?;
    fs::write(&path, content).map_err(|e| format!("写入原始指纹失败: {}", e))
}

/// 罗列当前目录下的 storage.json 备份（按时间降序）
#[allow(dead_code)]
pub fn list_backups(storage_path: &Path) -> Result<Vec<PathBuf>, String> {
    let dir = storage_path
        .parent()
        .ok_or_else(|| "无法获取 storage.json 的父目录".to_string())?;
    let mut backups = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with("storage.json.backup_") {
                    backups.push(path);
                }
            }
        }
    }
    // 按修改时间排序（新到旧）
    backups.sort_by(|a, b| {
        let ma = fs::metadata(a).and_then(|m| m.modified()).ok();
        let mb = fs::metadata(b).and_then(|m| m.modified()).ok();
        mb.cmp(&ma)
    });
    Ok(backups)
}

/// 将备份还原到 storage.json，优先 oldest=true 时用最早备份，否则用最新备份
#[allow(dead_code)]
pub fn restore_backup(storage_path: &Path, use_oldest: bool) -> Result<PathBuf, String> {
    let backups = list_backups(storage_path)?;
    if backups.is_empty() {
        return Err("未找到任何 storage.json 备份".to_string());
    }
    let target = if use_oldest {
        backups.last().unwrap().clone()
    } else {
        backups.first().unwrap().clone()
    };
    // 先备份当前
    let _ = backup_storage(storage_path)?;
    fs::copy(&target, storage_path).map_err(|e| format!("恢复备份失败: {}", e))?;
    logger::log_info(&format!("已恢复 storage.json: {:?}", target));
    Ok(target)
}

/// 生成一组新的设备指纹（符合 Cursor/VSCode 风格）
pub fn generate_profile() -> DeviceProfile {
    DeviceProfile {
        machine_id: format!("auth0|user_{}", random_hex(32)),
        mac_machine_id: new_standard_machine_id(),
        dev_device_id: Uuid::new_v4().to_string(),
        sqm_id: format!("{{{}}}", Uuid::new_v4().to_string().to_uppercase()),
        service_machine_id: generate_service_machine_id(),
    }
}

fn random_hex(length: usize) -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(length)
        .map(char::from)
        .collect::<String>()
        .to_lowercase()
}

fn new_standard_machine_id() -> String {
    // xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx (y in 8..b)
    let mut rng = rand::thread_rng();
    let mut id = String::with_capacity(36);
    for ch in "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".chars() {
        if ch == '-' || ch == '4' {
            id.push(ch);
        } else if ch == 'x' {
            id.push_str(&format!("{:x}", rng.gen_range(0..16)));
        } else if ch == 'y' {
            id.push_str(&format!("{:x}", rng.gen_range(8..12)));
        }
    }
    id
}
