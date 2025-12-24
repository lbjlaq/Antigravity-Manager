use tracing::{info, warn, error};
use tracing_subscriber;
use std::fs;
use std::path::PathBuf;
use crate::modules::account::get_data_dir;

pub fn get_log_dir() -> Result<PathBuf, String> {
    let data_dir = get_data_dir()?;
    let log_dir = data_dir.join("logs");
    
    if !log_dir.exists() {
        fs::create_dir_all(&log_dir).map_err(|e| format!("创建日志目录失败: {}", e))?;
    }
    
    Ok(log_dir)
}

/// 初始化日志系统
pub fn init_logger() {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_thread_ids(false)
        .with_level(true)
        .init();
    
    // 简单的文件日志模拟 (因缺少 tracing-appender)
    if let Ok(log_dir) = get_log_dir() {
        let log_file = log_dir.join("app.log");
        let _ = fs::write(log_file, format!("Log init at {}\n", chrono::Local::now()));
    }
    
    info!("日志系统已初始化");
}

/// 清理日志缓存
pub fn clear_logs() -> Result<(), String> {
    let log_dir = get_log_dir()?;
    if log_dir.exists() {
        fs::remove_dir_all(&log_dir).map_err(|e| format!("清理日志目录失败: {}", e))?;
        fs::create_dir_all(&log_dir).map_err(|e| format!("重建日志目录失败: {}", e))?;
        
        // 重建后立即写入一条初始日志，确保文件存在
        let log_file = log_dir.join("app.log");
        let _ = fs::write(log_file, format!("Log cleared at {}\n", chrono::Local::now()));
    }
    Ok(())
}

fn append_log(level: &str, message: &str) {
    if let Ok(log_dir) = get_log_dir() {
        let log_file = log_dir.join("app.log");
        // 使用 append 模式打开文件，如果文件不存在则创建
        if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(log_file) {
            use std::io::Write;
            let time = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
            let _ = writeln!(file, "[{}] [{}] {}", time, level, message);
        }
    }
}

/// 记录信息日志
pub fn log_info(message: &str) {
    info!("{}", message);
    append_log("INFO", message);
}

/// 记录警告日志
pub fn log_warn(message: &str) {
    warn!("{}", message);
    append_log("WARN", message);
}

/// 记录错误日志
pub fn log_error(message: &str) {
    error!("{}", message);
    append_log("ERROR", message);
}
