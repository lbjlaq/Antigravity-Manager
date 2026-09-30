use crate::modules::account::get_data_dir;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tracing::{error, info, warn};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

pub const ERROR_LOG_FILE_PREFIX: &str = "error.log";
const DEFAULT_ERROR_LOG_BUDGET_BYTES: u64 = 500 * 1024 * 1024;
const SLIDING_KEEP_RATIO_NUM: u64 = 7;
const SLIDING_KEEP_RATIO_DEN: u64 = 10;
const EVICT_CHECK_EVERY_BYTES: u64 = 1024 * 1024;
const SLIDING_TMP_NAME: &str = "error.log.sliding.tmp";

static ERROR_LOG_BUDGET_BYTES: AtomicU64 = AtomicU64::new(DEFAULT_ERROR_LOG_BUDGET_BYTES);
static ERROR_LOG_WRITER: OnceLock<Arc<Mutex<SlidingErrorLogWriter>>> = OnceLock::new();

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
        fs::create_dir_all(&log_dir)
            .map_err(|e| format!("Failed to create log directory: {}", e))?;
    }

    Ok(log_dir)
}

/// 仅失败日志的当日文件路径（按天滚动为 `error.log.YYYY-MM-DD`）。
pub fn internal_error_log_path() -> Result<PathBuf, String> {
    Ok(get_log_dir()?.join(ERROR_LOG_FILE_PREFIX))
}

/// Total size of `error.log*` sliding-window files.
pub fn internal_error_log_disk_size() -> Result<u64, String> {
    Ok(sum_error_log_bytes(&get_log_dir()?))
}

pub fn set_internal_error_log_budget_bytes(bytes: u64) {
    let bytes = if bytes == 0 {
        DEFAULT_ERROR_LOG_BUDGET_BYTES
    } else {
        bytes
    };
    ERROR_LOG_BUDGET_BYTES.store(bytes, Ordering::Relaxed);
}

pub fn sync_internal_error_log_budget_from_config() {
    let bytes = crate::modules::config::load_app_config()
        .ok()
        .map(|c| c.proxy.internal_error_log_retention.budget_bytes())
        .unwrap_or(DEFAULT_ERROR_LOG_BUDGET_BYTES);
    set_internal_error_log_budget_bytes(bytes);
}

/// Apply the sliding window: over budget, drop oldest 30% of the budget and keep appending.
pub fn apply_internal_error_log_retention() -> Result<u64, String> {
    if let Some(writer) = ERROR_LOG_WRITER.get() {
        let mut guard = writer.lock().unwrap_or_else(|e| e.into_inner());
        let _ = guard.flush();
        return guard.evict();
    }
    let dir = get_log_dir()?;
    let budget = ERROR_LOG_BUDGET_BYTES.load(Ordering::Relaxed);
    evict_error_logs_in_dir(&dir, budget, None)
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

    sync_internal_error_log_budget_from_config();

    let shared = match SlidingErrorLogWriter::new(log_dir.clone()) {
        Ok(writer) => {
            let shared = Arc::new(Mutex::new(writer));
            let _ = ERROR_LOG_WRITER.set(shared.clone());
            if let Ok(mut guard) = shared.lock() {
                if let Err(e) = guard.evict() {
                    eprintln!("Failed to apply error.log sliding window: {}", e);
                }
            }
            shared
        }
        Err(e) => {
            eprintln!("Failed to open error.log writer: {}", e);
            // Fall back to tracing_appender daily rolling without a live sliding handle.
            let file_appender = tracing_appender::rolling::daily(log_dir, ERROR_LOG_FILE_PREFIX);
            let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
            install_subscriber(non_blocking, _guard);
            let _ = cleanup_legacy_logs(7);
            return;
        }
    };

    let (non_blocking, _guard) = tracing_appender::non_blocking(SharedErrorLogWriter(shared));
    install_subscriber(non_blocking, _guard);

    info!("Log system initialized (console + error.log sliding window)");

    if let Err(e) = cleanup_old_logs(7) {
        warn!("Failed to cleanup leftover log files: {}", e);
    }
}

fn install_subscriber<W>(non_blocking: W, guard: tracing_appender::non_blocking::WorkerGuard)
where
    W: for<'writer> tracing_subscriber::fmt::MakeWriter<'writer> + Send + Sync + 'static,
{
    let console_layer = fmt::Layer::new()
        .with_target(false)
        .with_thread_ids(false)
        .with_level(true)
        .with_timer(LocalTimer);

    let error_file_layer = fmt::Layer::new()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(true)
        .with_level(true)
        .with_timer(LocalTimer)
        .with_filter(LevelFilter::ERROR);

    let filter_layer = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let bridge_layer = crate::modules::log_bridge::TauriLogBridgeLayer::new();

    let _ = tracing_subscriber::registry()
        .with(filter_layer)
        .with(console_layer)
        .with(error_file_layer)
        .with(bridge_layer)
        .try_init();

    std::mem::forget(guard);
}

#[derive(Clone)]
struct SharedErrorLogWriter(Arc<Mutex<SlidingErrorLogWriter>>);

impl Write for SharedErrorLogWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).flush()
    }
}

struct SlidingErrorLogWriter {
    dir: PathBuf,
    date: String,
    file: Option<File>,
    bytes_since_check: u64,
}

impl SlidingErrorLogWriter {
    fn new(dir: PathBuf) -> io::Result<Self> {
        let date = today_stamp();
        let file = open_current(&dir)?;
        Ok(Self {
            dir,
            date,
            file: Some(file),
            bytes_since_check: 0,
        })
    }

    fn roll_date_if_needed(&mut self) -> io::Result<()> {
        let today = today_stamp();
        if today == self.date {
            return Ok(());
        }
        self.file.take();
        let current = self.dir.join(ERROR_LOG_FILE_PREFIX);
        if current.exists() {
            let rolled = self
                .dir
                .join(format!("{}.{}", ERROR_LOG_FILE_PREFIX, self.date));
            if rolled.exists() || fs::rename(&current, &rolled).is_err() {
                let mut dst = OpenOptions::new().create(true).append(true).open(&rolled)?;
                let mut src = File::open(&current)?;
                io::copy(&mut src, &mut dst)?;
                let _ = OpenOptions::new().write(true).truncate(true).open(&current);
            }
        }
        self.date = today;
        self.file = Some(open_current(&self.dir)?);
        Ok(())
    }

    fn evict(&mut self) -> Result<u64, String> {
        let budget = ERROR_LOG_BUDGET_BYTES.load(Ordering::Relaxed);
        evict_error_logs_in_dir(&self.dir, budget, Some(&mut self.file))
    }
}

impl Write for SlidingErrorLogWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.roll_date_if_needed()?;
        let file = match self.file.as_mut() {
            Some(file) => file,
            None => {
                self.file = Some(open_current(&self.dir)?);
                match self.file.as_mut() {
                    Some(file) => file,
                    None => {
                        return Err(io::Error::new(
                            io::ErrorKind::Other,
                            "failed to reopen error.log",
                        ));
                    }
                }
            }
        };
        let n = file.write(buf)?;
        self.bytes_since_check = self.bytes_since_check.saturating_add(n as u64);
        if self.bytes_since_check >= EVICT_CHECK_EVERY_BYTES {
            self.bytes_since_check = 0;
            if let Err(e) = self.evict() {
                eprintln!("error.log sliding window evict failed: {}", e);
            }
        }
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some(file) = self.file.as_mut() {
            file.flush()
        } else {
            Ok(())
        }
    }
}

fn today_stamp() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

fn open_current(dir: &Path) -> io::Result<File> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(ERROR_LOG_FILE_PREFIX))
}

pub(crate) fn is_error_log_filename(name: &str) -> bool {
    name == ERROR_LOG_FILE_PREFIX || name.starts_with("error.log.")
}

fn is_sliding_tmp(name: &str) -> bool {
    name == SLIDING_TMP_NAME
}

fn list_error_log_files(dir: &Path) -> Vec<(PathBuf, u64, String)> {
    let mut files = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return files,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(name) => name.to_string(),
            None => continue,
        };
        if is_sliding_tmp(&name) || !is_error_log_filename(&name) {
            continue;
        }
        let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        files.push((path, size, name));
    }
    files.sort_by(|a, b| error_log_sort_key(&a.2).cmp(&error_log_sort_key(&b.2)));
    files
}

fn error_log_sort_key(name: &str) -> (u8, String) {
    if name == ERROR_LOG_FILE_PREFIX {
        (1, String::new())
    } else if let Some(suffix) = name.strip_prefix("error.log.") {
        (0, suffix.to_string())
    } else {
        (0, name.to_string())
    }
}

fn sum_error_log_bytes(dir: &Path) -> u64 {
    list_error_log_files(dir)
        .into_iter()
        .map(|(_, size, _)| size)
        .sum()
}

/// Evict oldest `error.log*` until total size is at most 70% of `budget`.
/// When `current_open` is provided, the live `error.log` handle is closed before trimming
/// and reopened for append afterwards.
pub(crate) fn evict_error_logs_in_dir(
    dir: &Path,
    budget: u64,
    mut current_open: Option<&mut Option<File>>,
) -> Result<u64, String> {
    if budget == 0 {
        return Ok(0);
    }
    let files = list_error_log_files(dir);
    let mut total: u64 = files.iter().map(|(_, size, _)| *size).sum();
    if total <= budget {
        return Ok(0);
    }

    let target = budget.saturating_mul(SLIDING_KEEP_RATIO_NUM) / SLIDING_KEEP_RATIO_DEN;
    let current_path = dir.join(ERROR_LOG_FILE_PREFIX);
    let mut freed = 0u64;

    for (path, size, name) in &files {
        if total <= target {
            break;
        }
        if name == ERROR_LOG_FILE_PREFIX {
            continue;
        }
        match fs::remove_file(path) {
            Ok(()) => {
                total = total.saturating_sub(*size);
                freed += *size;
            }
            Err(e) => {
                warn!("Failed to delete old error log {:?}: {}", path, e);
            }
        }
    }

    if total > target {
        let current_size = fs::metadata(&current_path).map(|m| m.len()).unwrap_or(0);
        let other = total.saturating_sub(current_size);
        let keep = if other >= target {
            current_size.min(64 * 1024)
        } else {
            target.saturating_sub(other)
        };
        if current_size > keep {
            if let Some(slot) = current_open.as_mut() {
                slot.take();
            }
            match trim_file_keep_newest_bytes(&current_path, keep) {
                Ok(trimmed) => {
                    freed += trimmed;
                }
                Err(e) => {
                    if let Some(slot) = current_open.as_mut() {
                        **slot = open_current(dir).ok();
                    }
                    return Err(e);
                }
            }
            if let Some(slot) = current_open.as_mut() {
                **slot = Some(
                    open_current(dir).map_err(|e| format!("Failed to reopen error.log: {}", e))?,
                );
            }
        }
    }

    if freed > 0 {
        info!(
            "Internal error log sliding window: freed {:.2} MB (budget {} MB, keep 70%)",
            freed as f64 / 1024.0 / 1024.0,
            budget / (1024 * 1024)
        );
    }

    Ok(freed)
}

fn trim_file_keep_newest_bytes(path: &Path, keep_bytes: u64) -> Result<u64, String> {
    if !path.exists() {
        return Ok(0);
    }
    let len = fs::metadata(path)
        .map_err(|e| format!("Failed to stat error.log: {}", e))?
        .len();
    if len <= keep_bytes {
        return Ok(0);
    }

    let mut skip = len.saturating_sub(keep_bytes);
    {
        let mut f = File::open(path).map_err(|e| format!("Failed to open error.log: {}", e))?;
        f.seek(SeekFrom::Start(skip))
            .map_err(|e| format!("Failed to seek error.log: {}", e))?;
        let mut probe = [0u8; 8192];
        let n = f
            .read(&mut probe)
            .map_err(|e| format!("Failed to read error.log: {}", e))?;
        if let Some(i) = probe[..n].iter().position(|&b| b == b'\n') {
            skip = skip.saturating_add(i as u64 + 1);
        }
    }
    if skip >= len {
        skip = len.saturating_sub(keep_bytes);
    }

    let tmp = path.with_file_name(SLIDING_TMP_NAME);
    {
        let mut src = File::open(path).map_err(|e| format!("Failed to open error.log: {}", e))?;
        src.seek(SeekFrom::Start(skip))
            .map_err(|e| format!("Failed to seek error.log: {}", e))?;
        let mut dst =
            File::create(&tmp).map_err(|e| format!("Failed to create sliding tmp: {}", e))?;
        io::copy(&mut src, &mut dst)
            .map_err(|e| format!("Failed to copy error.log tail: {}", e))?;
        let _ = dst.sync_all();
    }

    if let Err(e) = fs::remove_file(path) {
        let _ = fs::remove_file(&tmp);
        return Err(format!("Failed to replace error.log: {}", e));
    }
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(format!("Failed to rename sliding tmp: {}", e));
    }
    Ok(skip)
}

/// Cleanup leftover non-error log files older than the given days.
pub fn cleanup_old_logs(days_to_keep: u64) -> Result<(), String> {
    cleanup_legacy_logs(days_to_keep)?;
    apply_internal_error_log_retention().map(|_| ())
}

fn cleanup_legacy_logs(days_to_keep: u64) -> Result<(), String> {
    use std::time::{SystemTime, UNIX_EPOCH};

    let log_dir = get_log_dir()?;
    if !log_dir.exists() {
        return Ok(());
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("Failed to get system time: {}", e))?
        .as_secs();
    let cutoff_time = now.saturating_sub(days_to_keep * 24 * 60 * 60);

    let entries =
        fs::read_dir(&log_dir).map_err(|e| format!("Failed to read log directory: {}", e))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if is_error_log_filename(name) || is_sliding_tmp(name) {
            continue;
        }
        let modified_secs = fs::metadata(&path)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if modified_secs < cutoff_time {
            if let Err(e) = fs::remove_file(&path) {
                warn!("Failed to delete leftover log file {:?}: {}", path, e);
            } else {
                info!(
                    "Deleted leftover log file (expired): {:?}",
                    path.file_name()
                );
            }
        }
    }

    Ok(())
}

/// Clear log cache (using truncation mode to keep file handles valid)
pub fn clear_logs() -> Result<(), String> {
    let log_dir = get_log_dir()?;
    if log_dir.exists() {
        let entries =
            fs::read_dir(&log_dir).map_err(|e| format!("Failed to read log directory: {}", e))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let _ = fs::OpenOptions::new().write(true).truncate(true).open(path);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn write_file(dir: &Path, name: &str, bytes: usize) {
        fs::write(dir.join(name), vec![b'x'; bytes]).expect("write log fixture");
    }

    #[test]
    fn evicts_oldest_rolled_files_to_70_percent() {
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), "error.log.2026-01-01", 100);
        write_file(dir.path(), "error.log.2026-01-02", 100);
        write_file(dir.path(), "error.log.2026-01-03", 100);
        write_file(dir.path(), "error.log", 50);

        let freed = evict_error_logs_in_dir(dir.path(), 200, None).unwrap();
        assert!(freed > 0);
        assert!(sum_error_log_bytes(dir.path()) <= 140);
        assert!(dir.path().join("error.log").exists());
        assert!(!dir.path().join("error.log.2026-01-01").exists());
    }

    #[test]
    fn trims_current_file_keeping_newest_lines() {
        let dir = tempfile::tempdir().unwrap();
        let mut body = String::new();
        for i in 0..20 {
            body.push_str(&format!("line-{i:02}\n"));
        }
        let path = dir.path().join("error.log");
        fs::write(&path, body.as_bytes()).unwrap();

        trim_file_keep_newest_bytes(&path, 40).unwrap();
        let after = fs::read_to_string(&path).unwrap();
        assert!(after.len() as u64 <= 56);
        assert!(after.contains("line-19"));
        assert!(!after.contains("line-00"));
        assert!(!after.starts_with("ine-"), "trim should align to newline");
    }

    #[test]
    fn evicts_oversized_current_file_to_70_percent() {
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), "error.log", 200);
        let _ = evict_error_logs_in_dir(dir.path(), 100, None).unwrap();
        assert!(sum_error_log_bytes(dir.path()) <= 70);
    }

    #[test]
    fn ignores_non_error_log_files() {
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), "app.log", 500);
        write_file(dir.path(), "error.log", 50);
        let _ = evict_error_logs_in_dir(dir.path(), 10, None).unwrap();
        assert!(dir.path().join("app.log").exists());
    }
}
