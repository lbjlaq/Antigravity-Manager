//! 签名缓存环境自适应预设（SQLite-only）。
//!
//! 启动时根据 CPU / 内存 / 部署形态选择 Low / Standard / High / Server，
//! 只影响内存与批处理预算；SQLite 参数保持内部默认，不暴露给用户。

use serde::Serialize;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, Serialize)]
pub struct CacheProfile {
    pub name: &'static str,
    pub l1_limit: usize,
    pub read_pool_size: usize,
    pub batch_cap: usize,
    pub queue_cap: usize,
    pub retention_days: i64,
    pub max_rows: usize,
    pub total_memory_mb: u64,
    pub cores: usize,
    pub deployment: &'static str,
}

fn profile_from_name(
    name: &'static str,
    total_memory_mb: u64,
    cores: usize,
    deployment: &'static str,
) -> CacheProfile {
    let (l1_limit, read_pool_size, batch_cap, queue_cap, retention_days, max_rows) = match name {
        "low" => (5_000, 2, 128, 2_048, 5, 20_000),
        "high" => (50_000, 8, 1_024, 32_768, 10, 200_000),
        "server" => (100_000, 8, 1_024, 32_768, 10, 500_000),
        _ => (10_000, 4, 512, 8_192, 10, 10_000),
    };
    CacheProfile {
        name,
        l1_limit,
        read_pool_size,
        batch_cap,
        queue_cap,
        retention_days,
        max_rows,
        total_memory_mb,
        cores,
        deployment,
    }
}

fn resolve_profile() -> CacheProfile {
    let deployment = match std::env::var("ANTIGRAVITY_DEPLOYMENT")
        .unwrap_or_default()
        .to_lowercase()
        .as_str()
    {
        "server" | "headless" => "server",
        _ => "desktop",
    };
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let total_memory_mb = {
        let mut sys = sysinfo::System::new();
        sys.refresh_memory();
        (sys.total_memory() / 1024 / 1024).max(1)
    };

    #[cfg(test)]
    let profile = {
        // 测试保持单读连接，避免现有连接复用断言失效。
        let mut profile = profile_from_name("standard", total_memory_mb, cores, deployment);
        profile.read_pool_size = 1;
        profile
    };
    #[cfg(not(test))]
    let profile = {
        if let Ok(raw) = std::env::var("ANTIGRAVITY_CACHE_PROFILE") {
            match raw.to_lowercase().as_str() {
                "low" => return profile_from_name("low", total_memory_mb, cores, deployment),
                "standard" => {
                    return profile_from_name("standard", total_memory_mb, cores, deployment);
                }
                "high" => return profile_from_name("high", total_memory_mb, cores, deployment),
                "server" => {
                    return profile_from_name("server", total_memory_mb, cores, deployment);
                }
                _ => {}
            }
        }

        let name = if deployment == "server" || (total_memory_mb >= 64 * 1024 && cores >= 16) {
            "server"
        } else if total_memory_mb >= 32 * 1024 && cores >= 16 {
            "high"
        } else if total_memory_mb >= 8 * 1024 && cores >= 4 {
            "standard"
        } else {
            "low"
        };
        profile_from_name(name, total_memory_mb, cores, deployment)
    };
    profile
}

static PROFILE: OnceLock<CacheProfile> = OnceLock::new();

pub fn cache_profile() -> CacheProfile {
    *PROFILE.get_or_init(resolve_profile)
}
