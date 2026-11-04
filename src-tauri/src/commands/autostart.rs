// Autostart 命令
use tauri_plugin_autostart::ManagerExt;

#[tauri::command]
pub async fn toggle_auto_launch(app: tauri::AppHandle, enable: bool) -> Result<(), String> {
    let manager = app.autolaunch();

    if enable {
        manager
            .enable()
            .map_err(|e| format!("启用自动启动失败: {}", e))?;
        crate::modules::logger::log_info("已启用开机自动启动");
    } else {
        match manager.disable() {
            Ok(_) => {
                crate::modules::logger::log_info("已禁用开机自动启动");
            }
            Err(e) => {
                let err_msg = e.to_string();
                // 在 Windows 上，如果注册表项不存在，disable() 会返回 "系统找不到指定的文件" (os error 2)
                // 这种情况应该视为成功，因为目标（禁用）已经达成
                if err_msg.contains("os error 2") || err_msg.contains("找不到指定的文件") {
                    crate::modules::logger::log_info("开机自启项已不存在，视为禁用成功");
                } else {
                    return Err(format!("禁用自动启动失败: {}", e));
                }
            }
        }
    }

    // 同步将开机自启状态持久化到 app_config，确保系统配置与自启项保持原子一致
    if let Ok(mut config) = crate::modules::config::load_app_config() {
        if config.auto_launch != enable {
            config.auto_launch = enable;
            let _ = crate::modules::config::save_app_config(&config);
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn is_auto_launch_enabled(app: tauri::AppHandle) -> Result<bool, String> {
    let manager = app.autolaunch();
    manager.is_enabled().map_err(|e| e.to_string())
}
