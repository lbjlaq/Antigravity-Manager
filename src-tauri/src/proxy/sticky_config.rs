use serde::{Deserialize, Serialize};

/// 调度模式枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SchedulingMode {
    /// 缓存优先 (Cache-first): 尽可能锁定同一账号，限流时优先等待，极大提升 Prompt Caching 命中率
    CacheFirst,
    /// 平衡模式 (Balance): 锁定同一账号，限流时立即切换到备选账号，兼顾成功率和性能
    Balance,
    /// 性能优先 (Performance-first): 纯轮询模式 (Round-robin)，账号负载最均衡，但不利用缓存
    PerformanceFirst,
}

impl Default for SchedulingMode {
    fn default() -> Self {
        Self::Balance
    }
}

/// 粘性绑定维度枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AffinityScope {
    /// 默认：按会话标识符粒度绑定账号（最主流模式，兼顾多对话相互隔离与单会话缓存）
    Session,
    /// 凭据级强粘性：按客户端 API-Key / 来源凭证粒度绑定账号（专治多 Agent 频繁派生瞬态 Session UUID 导致的缓存击穿）
    ApiKey,
}

impl Default for AffinityScope {
    fn default() -> Self {
        Self::Session
    }
}

/// 粘性会话配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StickySessionConfig {
    /// 当前调度模式
    pub mode: SchedulingMode,
    /// 缓存优先模式下的最大等待时间 (秒)
    pub max_wait_seconds: u64,
    /// 请求级 429（无 retryDelay）首轮原地 GraceRetry 等待时间（毫秒，默认 1500）
    #[serde(default = "default_request_429_grace_ms")]
    pub request_429_grace_ms: u64,
    /// 并发分流使用的账号上限 (0 表示不限制，>0 时最多分流至该数量的账号)
    #[serde(default = "default_max_spillover_accounts")]
    pub max_spillover_accounts: usize,
    /// 粘性路由绑定作用域（默认 Session 会话级）
    #[serde(default)]
    pub affinity_scope: AffinityScope,
}

impl StickySessionConfig {
    /// 获取实际生效的粘性作用域（优先从环境变量读取覆盖，对齐 Headless & CLI 规范）
    pub fn resolve_affinity_scope(&self) -> AffinityScope {
        if let Ok(val) = std::env::var("ANTIGRAVITY_SCHEDULING_AFFINITY_SCOPE")
            .or_else(|_| std::env::var("AGY_SCHEDULING_AFFINITY_SCOPE"))
        {
            let val = val.trim().to_ascii_lowercase();
            if val == "api_key" || val == "apikey" || val == "key" {
                return AffinityScope::ApiKey;
            } else if val == "session" || val == "session_id" {
                return AffinityScope::Session;
            }
        }
        self.affinity_scope
    }

    /// 获取实际生效的请求级 429 GraceRetry 等待时间（毫秒）。
    /// 环境变量 `ABV_REQUEST_429_GRACE_MS` 优先，范围 200–10000ms。
    pub fn resolve_request_429_grace_ms(&self) -> u64 {
        std::env::var("ABV_REQUEST_429_GRACE_MS")
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .unwrap_or(self.request_429_grace_ms)
            .clamp(200, 10_000)
    }
}

/// 全局获取当前生效的粘性绑定维度（支持环境变量覆盖，保证与 Headless/CLI 一致）
pub fn get_affinity_scope() -> AffinityScope {
    if let Ok(val) = std::env::var("ANTIGRAVITY_SCHEDULING_AFFINITY_SCOPE")
        .or_else(|_| std::env::var("AGY_SCHEDULING_AFFINITY_SCOPE"))
    {
        let val = val.trim().to_ascii_lowercase();
        if val == "api_key" || val == "apikey" || val == "key" {
            return AffinityScope::ApiKey;
        } else if val == "session" || val == "session_id" {
            return AffinityScope::Session;
        }
    }

    crate::modules::config::load_app_config()
        .map(|cfg| cfg.proxy.scheduling.affinity_scope)
        .unwrap_or_default()
}

fn default_request_429_grace_ms() -> u64 {
    1500
}

fn default_max_spillover_accounts() -> usize {
    0
}

impl Default for StickySessionConfig {
    fn default() -> Self {
        Self {
            mode: SchedulingMode::Balance,
            max_wait_seconds: 60,
            request_429_grace_ms: 1500,
            max_spillover_accounts: 0,
            affinity_scope: AffinityScope::Session,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sticky_session_config_default() {
        let cfg = StickySessionConfig::default();
        assert_eq!(cfg.mode, SchedulingMode::Balance);
        assert_eq!(cfg.affinity_scope, AffinityScope::Session);
        assert_eq!(cfg.request_429_grace_ms, 1500);
        assert_eq!(cfg.resolve_affinity_scope(), AffinityScope::Session);
    }

    #[test]
    fn test_sticky_session_config_deserialize_defaults() {
        let json_str = r#"{"mode": "CacheFirst", "max_wait_seconds": 90}"#;
        let cfg: StickySessionConfig = serde_json::from_str(json_str).unwrap();
        assert_eq!(cfg.mode, SchedulingMode::CacheFirst);
        assert_eq!(cfg.max_wait_seconds, 90);
        assert_eq!(cfg.request_429_grace_ms, 1500);
        assert_eq!(cfg.affinity_scope, AffinityScope::Session);
    }

    #[test]
    fn test_sticky_session_config_deserialize_api_key() {
        let json_str = r#"{"affinity_scope": "api_key"}"#;
        let cfg: StickySessionConfig = serde_json::from_str(json_str).unwrap();
        assert_eq!(cfg.affinity_scope, AffinityScope::ApiKey);
    }
}
