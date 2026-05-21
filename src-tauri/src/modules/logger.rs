use tracing::{info, warn, error};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
use std::fs;
use std::path::PathBuf;
use crate::modules::account::get_data_dir;

// Custom local timezone time formatter
struct LocalTimer;

impl tracing_subscriber::fmt::time::FormatTime for LocalTimer {
    fn format_time(&self, w: &mut tracing_subscriber::fmt::format::Writer<'_>) -> std::fmt::Result {
        let now = chrono::Local::now();
        write!(w, "{}", now.to_rfc3339())
    }
}

pub fn get_log_dir() -> Result<PathBuf, String> {
    let data_dir = get_data_dir()?;
    let log_dir = data_dir.join("logs");
    
    if !log_dir.exists() {
        fs::create_dir_all(&log_dir).map_err(|e| format!("Failed to create log directory: {}", e))?;
    }
    
    Ok(log_dir)
}

/// Initialize the log system
pub fn init_logger() {
    // Capture log macro logs
    let _ = tracing_log::LogTracer::init();
    
    let log_dir = match get_log_dir() {
        Ok(dir) => dir,
        Err(e) => {
            eprintln!("Failed to initialize log directory: {}", e);
            return;
        }
    };
    
    // 1. Set up file Appender (using tracing-appender for rolling logs)
    // Using a daily rolling strategy here
    let file_appender = tracing_appender::rolling::daily(log_dir, "app.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    
    // 2. Console output layer (using local timezone)
    let console_layer = fmt::Layer::new()
        .with_target(false)
        .with_thread_ids(false)
        .with_level(true)
        .with_timer(LocalTimer);
        
    // 3. File output layer (disable ANSI formatting, use local timezone)
    let file_layer = fmt::Layer::new()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(true)
        .with_level(true)
        .with_timer(LocalTimer);

    // 4. Set filtering layer (default to INFO level to reduce log size)
    let filter_layer = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    // 6. Log bridge layer
    let bridge_layer = crate::modules::log_bridge::TauriLogBridgeLayer::new();

    // 5. Initialize global subscriber (use try_init to avoid crash on repeated initialization)
    let _ = tracing_subscriber::registry()
        .with(filter_layer)
        .with(console_layer)
        .with(file_layer)
        .with(bridge_layer)
        .try_init();

    // Leak _guard to ensure its lifetime lasts until program exit
    // Recommended practice when using tracing_appender::non_blocking (if manual flushing is not needed)
    std::mem::forget(_guard);
    
    info!("Log system initialized (Console + File persistence)");
    
    // Auto-cleanup logs older than 7 days
    if let Err(e) = cleanup_old_logs(7) {
        warn!("Failed to cleanup old logs: {}", e);
    }
}

/// Cleanup log files older than specified days OR if total size exceeds limit
pub fn cleanup_old_logs(days_to_keep: u64) -> Result<(), String> {
    use std::time::{SystemTime, UNIX_EPOCH};
    
    let log_dir = get_log_dir()?;
    if !log_dir.exists() {
        return Ok(());
    }

    // Constants for size-based cleanup
    const MAX_TOTAL_SIZE_BYTES: u64 = 1024 * 1024 * 1024; // 1GB
    const TARGET_SIZE_BYTES: u64 = 512 * 1024 * 1024;    // 512MB
    
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("Failed to get system time: {}", e))?
        .as_secs();
    
    let cutoff_time = now.saturating_sub(days_to_keep * 24 * 60 * 60);
    
    let mut entries_info = Vec::new();
    let entries = fs::read_dir(&log_dir)
        .map_err(|e| format!("Failed to read log directory: {}", e))?;
    
    for entry in entries {
        if let Ok(entry) = entry {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            
            if let Ok(metadata) = fs::metadata(&path) {
                let modified = metadata.modified().unwrap_or(SystemTime::now());
                let modified_secs = modified
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                
                let size = metadata.len();
                entries_info.push((path, size, modified_secs));
            }
        }
    }

    let mut deleted_count = 0;
    let mut total_size_freed = 0u64;

    // 1. First pass: Delete files older than cutoff_time
    let mut remaining_entries = Vec::new();
    for (path, size, modified_secs) in entries_info {
        if modified_secs < cutoff_time {
            if let Err(e) = fs::remove_file(&path) {
                warn!("Failed to delete old log file {:?}: {}", path, e);
                remaining_entries.push((path, size, modified_secs));
            } else {
                deleted_count += 1;
                total_size_freed += size;
                info!("Deleted old log file (expired): {:?}", path.file_name());
            }
        } else {
            remaining_entries.push((path, size, modified_secs));
        }
    }

    // 2. Second pass: If total size still exceeds limit, delete oldest files
    let mut current_total_size: u64 = remaining_entries.iter().map(|(_, size, _)| *size).sum();
    
    if current_total_size > MAX_TOTAL_SIZE_BYTES {
        info!("Log directory size ({} MB) exceeds limit (1024 MB), starting size-based cleanup...", current_total_size / 1024 / 1024);
        
        // Sort remaining entries by modification time (oldest first)
        remaining_entries.sort_by_key(|(_, _, modified)| *modified);
        
        for (path, size, _) in remaining_entries {
            if current_total_size <= TARGET_SIZE_BYTES {
                break;
            }
            
            // Try to delete. Skip if it's the most recent file and it fails (might be active)
            if let Err(e) = fs::remove_file(&path) {
                warn!("Failed to delete log file during size cleanup {:?}: {}", path, e);
            } else {
                deleted_count += 1;
                total_size_freed += size;
                current_total_size -= size;
                info!("Deleted log file (size limit): {:?}", path.file_name());
            }
        }
    }
    
    if deleted_count > 0 {
        let size_mb = total_size_freed as f64 / 1024.0 / 1024.0;
        info!(
            "Log cleanup completed: deleted {} files, freed {:.2} MB space",
            deleted_count, size_mb
        );
    }
    
    Ok(())
}

/// Clear log cache (using truncation mode to keep file handles valid)
pub fn clear_logs() -> Result<(), String> {
    let log_dir = get_log_dir()?;
    if log_dir.exists() {
        // Iterate through all files in directory and truncate instead of deleting directory
        let entries = fs::read_dir(&log_dir).map_err(|e| format!("Failed to read log directory: {}", e))?;
        for entry in entries {
            if let Ok(entry) = entry {
                let path = entry.path();
                if path.is_file() {
                    // Open file in truncation mode, set size to 0
                    let _ = fs::OpenOptions::new()
                        .write(true)
                        .truncate(true)
                        .open(path);
                }
            }
        }
    }
    Ok(())
}

/// Log info message (backward compatibility)
pub fn log_info(message: &str) {
    info!("{}", message);
}

/// Log warn message (backward compatibility)
pub fn log_warn(message: &str) {
    warn!("{}", message);
}

/// Log error message (backward compatibility)
pub fn log_error(message: &str) {
    error!("{}", message);
}
