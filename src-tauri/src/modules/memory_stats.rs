//! Process memory observability & Linux glibc allocator retention controls (Issue #4054).
//!
//! - On Linux glibc, multi-threaded workloads allocating/freeing 100 KB–2 MB buffers across
//!   Tokio workers and `spawn_blocking` threads suffer from glibc's default arena fan-out
//!   (`8 * nproc`) and dynamic `mmap_threshold` ratchet (up to 32 MB). Bounding arenas, fixing
//!   `M_MMAP_THRESHOLD` / `M_TRIM_THRESHOLD`, and running periodic `malloc_trim(0)` returns
//!   unused pages to the OS for both Rust and C-library (SQLite / BoringSSL) heaps.
//! - Exposes ground-truth process memory metrics (`/proc/self/status` on Linux with `sysinfo`
//!   cross-platform fallback) so operators can monitor `VmRSS` / `RssAnon` directly via HTTP API.

use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};

#[cfg(all(target_os = "linux", target_env = "gnu"))]
const DEFAULT_MALLOC_ARENA_MAX: i32 = 2;
#[cfg(all(target_os = "linux", target_env = "gnu"))]
const DEFAULT_MMAP_THRESHOLD_BYTES: i32 = 128 * 1024;
#[cfg(all(target_os = "linux", target_env = "gnu"))]
const DEFAULT_TRIM_THRESHOLD_BYTES: i32 = 128 * 1024;
const DEFAULT_MALLOC_TRIM_INTERVAL_SECS: u64 = 60;

static TUNING_APPLIED: AtomicBool = AtomicBool::new(false);
static TRIM_COUNT: AtomicU64 = AtomicU64::new(0);
static LAST_TRIM_AT_MS: AtomicI64 = AtomicI64::new(0);
static LAST_TRIM_RELEASED: AtomicI64 = AtomicI64::new(-1); // -1 = never, 0 = false, 1 = true

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProcessMemoryStats {
    pub pid: u32,
    pub platform: &'static str,
    pub rss_bytes: u64,
    pub rss_anon_bytes: Option<u64>,
    pub rss_file_bytes: Option<u64>,
    pub rss_shmem_bytes: Option<u64>,
    pub vm_swap_bytes: Option<u64>,
    pub virtual_bytes: u64,
    pub threads: Option<u64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AllocatorStats {
    pub malloc_trim_supported: bool,
    pub tuning_applied: bool,
    pub malloc_arena_max_env: Option<String>,
    pub trim_interval_secs: u64,
    pub trim_count: u64,
    pub last_trim_at_ms: Option<i64>,
    pub last_trim_released: Option<bool>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ProcStatusMemory {
    pub vm_rss_bytes: Option<u64>,
    pub rss_anon_bytes: Option<u64>,
    pub rss_file_bytes: Option<u64>,
    pub rss_shmem_bytes: Option<u64>,
    pub vm_size_bytes: Option<u64>,
    pub vm_swap_bytes: Option<u64>,
    pub threads: Option<u64>,
}

fn parse_kib_field(value: &str) -> Option<u64> {
    let mut parts = value.split_whitespace();
    let num: u64 = parts.next()?.parse().ok()?;
    let unit = parts.next().unwrap_or("kB");
    match unit {
        "kB" | "KB" | "KiB" => num.checked_mul(1024),
        "mB" | "MB" | "MiB" => num.checked_mul(1024 * 1024),
        "B" => Some(num),
        _ => num.checked_mul(1024),
    }
}

/// Parse Linux `/proc/self/status` memory and thread counters.
pub fn parse_proc_status_memory(content: &str) -> Option<ProcStatusMemory> {
    let mut out = ProcStatusMemory::default();
    let mut saw_any = false;
    for line in content.lines() {
        let Some((key, raw_val)) = line.split_once(':') else {
            continue;
        };
        let val = raw_val.trim();
        match key.trim() {
            "VmRSS" => {
                out.vm_rss_bytes = parse_kib_field(val);
                saw_any |= out.vm_rss_bytes.is_some();
            }
            "RssAnon" => {
                out.rss_anon_bytes = parse_kib_field(val);
                saw_any |= out.rss_anon_bytes.is_some();
            }
            "RssFile" => {
                out.rss_file_bytes = parse_kib_field(val);
                saw_any |= out.rss_file_bytes.is_some();
            }
            "RssShmem" => {
                out.rss_shmem_bytes = parse_kib_field(val);
                saw_any |= out.rss_shmem_bytes.is_some();
            }
            "VmSize" => {
                out.vm_size_bytes = parse_kib_field(val);
                saw_any |= out.vm_size_bytes.is_some();
            }
            "VmSwap" => {
                out.vm_swap_bytes = parse_kib_field(val);
                saw_any |= out.vm_swap_bytes.is_some();
            }
            "Threads" => {
                out.threads = val.split_whitespace().next().and_then(|s| s.parse().ok());
                saw_any |= out.threads.is_some();
            }
            _ => {}
        }
    }
    saw_any.then_some(out)
}

pub fn trim_interval_secs() -> u64 {
    std::env::var("ANTIGRAVITY_MALLOC_TRIM_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_MALLOC_TRIM_INTERVAL_SECS)
}

fn tuning_disabled_by_env() -> bool {
    std::env::var("ANTIGRAVITY_DISABLE_MALLOC_TUNING")
        .map(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
mod glibc {
    pub const M_TRIM_THRESHOLD: libc::c_int = -1;
    pub const M_MMAP_THRESHOLD: libc::c_int = -3;
    pub const M_ARENA_MAX: libc::c_int = -8;

    extern "C" {
        pub fn mallopt(param: libc::c_int, value: libc::c_int) -> libc::c_int;
        pub fn malloc_trim(pad: libc::size_t) -> libc::c_int;
    }
}

/// Configure Linux glibc malloc parameters early in process startup (before Tokio worker pool).
///
/// Setting `M_MMAP_THRESHOLD` explicitly disables glibc's dynamic 32 MB threshold ratchet so
/// transient >=128 KiB request/response buffers are `mmap`/`munmap`-ed directly instead of
/// fragmenting per-thread arenas. Respects user-provided `MALLOC_*` env overrides and
/// `ANTIGRAVITY_DISABLE_MALLOC_TUNING=1`.
pub fn configure_process_allocator() {
    if tuning_disabled_by_env() {
        return;
    }

    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        let mut applied = false;
        unsafe {
            if std::env::var_os("MALLOC_ARENA_MAX").is_none()
                && glibc::mallopt(glibc::M_ARENA_MAX, DEFAULT_MALLOC_ARENA_MAX) == 1
            {
                applied = true;
            }
            if std::env::var_os("MALLOC_MMAP_THRESHOLD_").is_none()
                && glibc::mallopt(glibc::M_MMAP_THRESHOLD, DEFAULT_MMAP_THRESHOLD_BYTES) == 1
            {
                applied = true;
            }
            if std::env::var_os("MALLOC_TRIM_THRESHOLD_").is_none()
                && glibc::mallopt(glibc::M_TRIM_THRESHOLD, DEFAULT_TRIM_THRESHOLD_BYTES) == 1
            {
                applied = true;
            }
        }
        if applied {
            TUNING_APPLIED.store(true, Ordering::Relaxed);
        }
    }
}

/// Ask the C/system allocator to release free heap pages back to the OS.
/// Returns `true` if memory was actually released back to the OS (Linux glibc only).
pub fn trim_process_heap() -> bool {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        let released = unsafe { glibc::malloc_trim(0) } != 0;
        let now_ms = chrono::Utc::now().timestamp_millis();
        TRIM_COUNT.fetch_add(1, Ordering::Relaxed);
        LAST_TRIM_AT_MS.store(now_ms, Ordering::Relaxed);
        LAST_TRIM_RELEASED.store(if released { 1 } else { 0 }, Ordering::Relaxed);
        released
    }
    #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
    {
        false
    }
}

/// Run `trim_process_heap()` if the configured interval has elapsed.
/// Setting `ANTIGRAVITY_MALLOC_TRIM_INTERVAL_SECS=0` disables periodic trimming.
pub fn maybe_periodic_trim() -> bool {
    let interval_secs = trim_interval_secs();
    if interval_secs == 0 {
        return false;
    }
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let last_ms = LAST_TRIM_AT_MS.load(Ordering::Relaxed);
        let interval_ms = (interval_secs as i64).saturating_mul(1000);
        if last_ms == 0 || now_ms.saturating_sub(last_ms) >= interval_ms {
            return trim_process_heap();
        }
        false
    }
    #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
    {
        false
    }
}

pub fn collect_allocator_stats() -> AllocatorStats {
    let last_ms = LAST_TRIM_AT_MS.load(Ordering::Relaxed);
    let last_rel = LAST_TRIM_RELEASED.load(Ordering::Relaxed);
    AllocatorStats {
        malloc_trim_supported: cfg!(all(target_os = "linux", target_env = "gnu")),
        tuning_applied: TUNING_APPLIED.load(Ordering::Relaxed),
        malloc_arena_max_env: std::env::var("MALLOC_ARENA_MAX").ok(),
        trim_interval_secs: trim_interval_secs(),
        trim_count: TRIM_COUNT.load(Ordering::Relaxed),
        last_trim_at_ms: (last_ms > 0).then_some(last_ms),
        last_trim_released: match last_rel {
            1 => Some(true),
            0 => Some(false),
            _ => None,
        },
    }
}

/// Collect current process RSS / anonymous RSS / virtual memory / thread count.
pub fn collect_process_memory_stats() -> ProcessMemoryStats {
    let pid_u32 = std::process::id();

    #[cfg(target_os = "linux")]
    {
        if let Ok(content) = std::fs::read_to_string("/proc/self/status") {
            if let Some(parsed) = parse_proc_status_memory(&content) {
                return ProcessMemoryStats {
                    pid: pid_u32,
                    platform: std::env::consts::OS,
                    rss_bytes: parsed.vm_rss_bytes.unwrap_or(0),
                    rss_anon_bytes: parsed.rss_anon_bytes,
                    rss_file_bytes: parsed.rss_file_bytes,
                    rss_shmem_bytes: parsed.rss_shmem_bytes,
                    vm_swap_bytes: parsed.vm_swap_bytes,
                    virtual_bytes: parsed.vm_size_bytes.unwrap_or(0),
                    threads: parsed.threads,
                };
            }
        }
    }

    let pid = sysinfo::Pid::from_u32(pid_u32);
    let mut sys = sysinfo::System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]));
    let (rss_bytes, virtual_bytes) = sys
        .process(pid)
        .map(|p| (p.memory(), p.virtual_memory()))
        .unwrap_or((0, 0));

    ProcessMemoryStats {
        pid: pid_u32,
        platform: std::env::consts::OS,
        rss_bytes,
        rss_anon_bytes: None,
        rss_file_bytes: None,
        rss_shmem_bytes: None,
        vm_swap_bytes: None,
        virtual_bytes,
        threads: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_proc_status_memory_linux_sample() {
        let sample = "\
Name:\tantigravity-too
State:\tS (sleeping)
Tgid:\t1
Pid:\t1
VmPeak:\t12345678 kB
VmSize:\t10485760 kB
VmRSS:\t 9600000 kB
RssAnon:\t 9450000 kB
RssFile:\t  135168 kB
RssShmem:\t   14832 kB
VmSwap:\t    2048 kB
Threads:\t28
";
        let parsed = parse_proc_status_memory(sample).expect("should parse /proc/self/status");
        assert_eq!(parsed.vm_size_bytes, Some(10_485_760 * 1024));
        assert_eq!(parsed.vm_rss_bytes, Some(9_600_000 * 1024));
        assert_eq!(parsed.rss_anon_bytes, Some(9_450_000 * 1024));
        assert_eq!(parsed.rss_file_bytes, Some(135_168 * 1024));
        assert_eq!(parsed.rss_shmem_bytes, Some(14_832 * 1024));
        assert_eq!(parsed.vm_swap_bytes, Some(2_048 * 1024));
        assert_eq!(parsed.threads, Some(28));
    }

    #[test]
    fn test_parse_proc_status_memory_empty_returns_none() {
        assert_eq!(parse_proc_status_memory("Name:\tfoo\nState:\tS\n"), None);
    }

    #[test]
    fn test_collect_process_memory_stats_reports_current_pid_and_nonzero_rss() {
        let stats = collect_process_memory_stats();
        assert_eq!(stats.pid, std::process::id());
        assert!(stats.rss_bytes > 0, "current process RSS should be > 0");
    }
}
