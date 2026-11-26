use serde::{Deserialize, Serialize};

/// 智能切号策略
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SwitchStrategy {
    /// 剩余配额最高优先 (默认推荐)
    MaxQuota,
    /// 临近重置优先 (有可用余量且重置时间最近的优先，最大化利用快刷新的额度)
    ExpiringSoon,
    /// 订阅等级优先 (ULTRA > PRO > ENTERPRISE > FREE，同等级按余量优先)
    TierFirst,
    /// 轮询/最久未被使用优先 (保障负载均衡与24h额度恢复)
    LeastUsed,
    /// 优先级权重最高优先
    Priority,
    /// 随机选择
    Random,
}

impl std::str::FromStr for SwitchStrategy {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "max-quota" | "max_quota" | "quota" | "max" => Ok(SwitchStrategy::MaxQuota),
            "expiring-soon" | "expiring_soon" | "expiring" | "reset-soon" => {
                Ok(SwitchStrategy::ExpiringSoon)
            }
            "tier-first" | "tier_first" | "tier" => Ok(SwitchStrategy::TierFirst),
            "least-used" | "least_used" | "balanced" | "rr" | "round-robin" | "idle" | "lru" => {
                Ok(SwitchStrategy::LeastUsed)
            }
            "priority" | "prio" => Ok(SwitchStrategy::Priority),
            "random" | "rand" => Ok(SwitchStrategy::Random),
            _ => Err(format!(
                "Unknown strategy: '{}'. Available: max-quota, expiring-soon, tier-first, least-used, priority, random",
                s
            )),
        }
    }
}

/// 偏好的模型配额池
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelPoolPreference {
    /// 综合综合最佳 (Claude 与 Gemini 加权或取最大可用)
    Balanced,
    /// Claude 池优先 (如 Sonnet 3.7 / 3.5)
    Claude,
    /// Gemini 池优先 (如 Gemini 2.5 Pro / Flash)
    Gemini,
}

impl std::str::FromStr for ModelPoolPreference {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "balanced" | "any" | "all" => Ok(ModelPoolPreference::Balanced),
            "claude" | "sonnet" => Ok(ModelPoolPreference::Claude),
            "gemini" | "pro" | "flash" => Ok(ModelPoolPreference::Gemini),
            _ => Err(format!(
                "Unknown model preference: '{}'. Available: balanced, claude, gemini",
                s
            )),
        }
    }
}

/// 智能选号过滤与排序配置
#[derive(Debug, Clone, Default)]
pub struct AccountSelectionFilter {
    /// 切换策略
    pub strategy: Option<SwitchStrategy>,
    /// 模型池偏好
    pub prefer_pool: Option<ModelPoolPreference>,
    /// 必须包含的标签
    pub required_tag: Option<String>,
    /// 是否必须为健康未禁用账号 (默认 true)
    pub healthy_only: bool,
    /// 最低保留配额阈值 (0.0 ~ 100.0, 低于此百分比的账号不选)
    pub min_quota_percentage: Option<f64>,
}

/// 账号排序规则
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountSortOrder {
    QuotaDesc,
    QuotaAsc,
    PriorityDesc,
    LastUsedAsc,
    LastUsedDesc,
    CreatedAtDesc,
    EmailAsc,
}

impl std::str::FromStr for AccountSortOrder {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "quota" | "quota-desc" => Ok(AccountSortOrder::QuotaDesc),
            "quota-asc" => Ok(AccountSortOrder::QuotaAsc),
            "priority" | "priority-desc" | "prio" => Ok(AccountSortOrder::PriorityDesc),
            "last-used" | "least-used" | "idle" => Ok(AccountSortOrder::LastUsedAsc),
            "last-used-desc" | "recent" => Ok(AccountSortOrder::LastUsedDesc),
            "created" | "created-desc" => Ok(AccountSortOrder::CreatedAtDesc),
            "email" | "name" => Ok(AccountSortOrder::EmailAsc),
            _ => Err(format!(
                "Unknown sort order: '{}'. Available: quota, priority, last-used, recent, created, email",
                s
            )),
        }
    }
}
