use rusqlite::Connection;
use base64::{Engine as _, engine::general_purpose};
use std::path::PathBuf;
use crate::utils::protobuf;

/// 获取 Antigravity 数据库路径（跨平台）
pub fn get_db_path() -> Result<PathBuf, String> {
    #[cfg(target_os = "macos")]
    {
        let home = dirs::home_dir().ok_or("无法获取 Home 目录")?;
        Ok(home.join("Library/Application Support/Antigravity/User/globalStorage/state.vscdb"))
    }
    
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var("APPDATA")
            .map_err(|_| "无法获取 APPDATA 环境变量".to_string())?;
        Ok(PathBuf::from(appdata).join("Antigravity\\User\\globalStorage\\state.vscdb"))
    }
    
    #[cfg(target_os = "linux")]
    {
        let home = dirs::home_dir().ok_or("无法获取 Home 目录")?;
        Ok(home.join(".config/Antigravity/User/globalStorage/state.vscdb"))
    }
}

/// 注入 Token 到数据库
pub fn inject_token(
    db_path: &PathBuf,
    access_token: &str,
    refresh_token: &str,
    expiry: i64,
) -> Result<String, String> {
    // 1. 打开数据库
    let conn = Connection::open(db_path)
        .map_err(|e| format!("打开数据库失败: {}", e))?;

    // 2. 读取当前数据
    let current_data: String = conn
        .query_row(
            "SELECT value FROM ItemTable WHERE key = ?",
            ["jetskiStateSync.agentManagerInitState"],
            |row| row.get(0),
        )
        .map_err(|e| format!("读取数据失败: {}", e))?;

    // 3. Base64 解码
    let blob = general_purpose::STANDARD
        .decode(&current_data)
        .map_err(|e| format!("Base64 解码失败: {}", e))?;

    // 4. 移除旧 Field 6
    let clean_data = protobuf::remove_field(&blob, 6)?;

    // 5. 创建新 Field 6
    let new_field = protobuf::create_oauth_field(access_token, refresh_token, expiry);

    // 6. 合并数据
    let final_data = [clean_data, new_field].concat();
    let final_b64 = general_purpose::STANDARD.encode(&final_data);

    // 7. 写入数据库
    conn.execute(
        "UPDATE ItemTable SET value = ? WHERE key = ?",
        [&final_b64, "jetskiStateSync.agentManagerInitState"],
    )
    .map_err(|e| format!("写入数据失败: {}", e))?;

    // 8. 注入 Onboarding 标记
    let onboarding_key = "antigravityOnboarding";
    conn.execute(
        "INSERT OR REPLACE INTO ItemTable (key, value) VALUES (?, ?)",
        [onboarding_key, "true"],
    )
    .map_err(|e| format!("写入 Onboarding 标记失败: {}", e))?;

    Ok(format!("Token 注入成功！\n数据库: {:?}", db_path))
}
