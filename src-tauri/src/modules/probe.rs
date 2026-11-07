use crate::models::account::Account;
use crate::modules::{auto_switch, quota};

/// 自动探活选号失败：
/// - `NoCandidates`：按当前策略没有任何可用候选（HTTP 400）；
/// - `AllCandidatesFailed`：候选存在，但全部无法刷新有效凭据（HTTP 502）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeSelectionError {
    NoCandidates,
    AllCandidatesFailed { reasons: Vec<String> },
}

impl std::fmt::Display for ProbeSelectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoCandidates => write!(
                f,
                "当前没有可用于探活的账号（已排除停用/封禁/验证中/403 冻结/不可用反代账号），请先恢复账号或指定具体账号测试"
            ),
            Self::AllCandidatesFailed { reasons } => {
                let last = reasons
                    .last()
                    .map(String::as_str)
                    .unwrap_or("未知错误");
                write!(
                    f,
                    "自动选号失败：{} 个候选账号均无法获取有效凭据（最近错误: {}）",
                    reasons.len(),
                    last
                )
            }
        }
    }
}

impl std::error::Error for ProbeSelectionError {}

/// 一次探活选号结果：选中的账号 + 已刷新凭据 + 项目 ID。
#[derive(Debug, Clone)]
pub struct ProbeAccount {
    pub account: Account,
    pub access_token: String,
    pub project_id: String,
}

fn target_model_percentage(account: &Account, model: &str) -> Option<i32> {
    account
        .quota
        .as_ref()
        .and_then(|q| auto_switch::find_model_percentage(q, model))
}

fn max_model_percentage(account: &Account) -> i32 {
    account
        .quota
        .as_ref()
        .and_then(|q| q.models.iter().map(|m| m.percentage).max())
        .unwrap_or(0)
}

/// 候选过滤 + 排序（纯函数，便于 table-driven 单测）：
/// 1. 只排除确定不可用的账号：disabled / banned / validation_blocked / 403 forbidden；
/// 2. `include_proxy_disabled=false` 时额外排除手动禁用反代的账号（Web 自动调度语义）；
/// 3. 按目标模型配额降序，其次按账号最高配额降序，最后按 account_id 稳定排序。
pub fn rank_probe_candidates<'a>(
    accounts: &'a [Account],
    model: &str,
    include_proxy_disabled: bool,
) -> Vec<&'a Account> {
    let mut candidates: Vec<&Account> = accounts
        .iter()
        .filter(|a| {
            !a.disabled
                && !a.banned
                && !a.validation_blocked
                && !a.quota.as_ref().is_some_and(|q| q.is_forbidden)
                && (include_proxy_disabled || !a.proxy_disabled)
        })
        .collect();

    candidates.sort_by(|a, b| {
        target_model_percentage(b, model)
            .cmp(&target_model_percentage(a, model))
            .then_with(|| max_model_percentage(b).cmp(&max_model_percentage(a)))
            .then_with(|| a.id.cmp(&b.id))
    });
    candidates
}

/// 从候选池中按优先级逐个刷新凭据，返回第一个成功账号。
pub async fn pick_auto_probe_account(
    accounts: &[Account],
    model: &str,
    include_proxy_disabled: bool,
) -> Result<ProbeAccount, ProbeSelectionError> {
    let candidates = rank_probe_candidates(accounts, model, include_proxy_disabled);
    if candidates.is_empty() {
        return Err(ProbeSelectionError::NoCandidates);
    }

    let mut reasons = Vec::new();
    for account in candidates {
        match quota::get_valid_token_for_warmup(account).await {
            Ok((access_token, project_id)) => {
                return Ok(ProbeAccount {
                    account: account.clone(),
                    access_token,
                    project_id,
                });
            }
            Err(err) => reasons.push(format!("{}: {}", account.email, err)),
        }
    }

    Err(ProbeSelectionError::AllCandidatesFailed { reasons })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::account::Account;
    use crate::models::token::TokenData;
    use serde_json::json;

    fn test_account(
        id: &str,
        models: &[(&str, i32)],
        proxy_disabled: bool,
        disabled: bool,
        banned: bool,
        validation_blocked: bool,
        forbidden: bool,
    ) -> Account {
        let email = format!("{id}@example.com");
        let mut account = Account::new(
            id.to_string(),
            email.clone(),
            TokenData::new(
                "access-token".to_string(),
                "refresh-token".to_string(),
                3600,
                Some(email),
                Some("project-id".to_string()),
                None,
                false,
                None,
            ),
        );
        account.proxy_disabled = proxy_disabled;
        account.disabled = disabled;
        account.banned = banned;
        account.validation_blocked = validation_blocked;
        account.quota = Some(
            serde_json::from_value(json!({
                "models": models
                    .iter()
                    .map(|(name, percentage)| json!({
                        "name": name,
                        "percentage": percentage,
                        "reset_time": ""
                    }))
                    .collect::<Vec<_>>(),
                "last_updated": 0
            }))
            .unwrap(),
        );
        if forbidden {
            if let Some(quota) = account.quota.as_mut() {
                quota.is_forbidden = true;
            }
        }
        account
    }

    #[test]
    fn test_rank_probe_candidates_filters_unavailable_accounts() {
        let accounts = vec![
            test_account(
                "healthy",
                &[("gemini-3.8-flash-high", 100)],
                false,
                false,
                false,
                false,
                false,
            ),
            test_account(
                "proxy-disabled",
                &[("gemini-3.8-flash-high", 100)],
                true,
                false,
                false,
                false,
                false,
            ),
            test_account(
                "disabled",
                &[("gemini-3.8-flash-high", 100)],
                false,
                true,
                false,
                false,
                false,
            ),
            test_account(
                "banned",
                &[("gemini-3.8-flash-high", 100)],
                false,
                false,
                true,
                false,
                false,
            ),
            test_account(
                "validation",
                &[("gemini-3.8-flash-high", 100)],
                false,
                false,
                false,
                true,
                false,
            ),
            test_account(
                "forbidden",
                &[("gemini-3.8-flash-high", 100)],
                false,
                false,
                false,
                false,
                true,
            ),
        ];

        let web_ranked = rank_probe_candidates(&accounts, "gemini-3.8-flash-high", false);
        assert_eq!(web_ranked.len(), 1);
        assert_eq!(web_ranked[0].id, "healthy");

        let desktop_ranked = rank_probe_candidates(&accounts, "gemini-3.8-flash-high", true);
        assert_eq!(desktop_ranked.len(), 2);
        assert!(desktop_ranked.iter().any(|a| a.id == "proxy-disabled"));
        assert!(!desktop_ranked.iter().any(|a| a.id == "banned"));
    }

    #[test]
    fn test_rank_probe_candidates_prioritizes_target_model_quota() {
        let accounts = vec![
            test_account(
                "gemini-heavy",
                &[("gemini-3.8-flash-high", 100), ("claude-sonnet-4-6", 0)],
                false,
                false,
                false,
                false,
                false,
            ),
            test_account(
                "claude-heavy",
                &[("gemini-3.8-flash-high", 0), ("claude-sonnet-4-6", 100)],
                false,
                false,
                false,
                false,
                false,
            ),
        ];

        let ranked = rank_probe_candidates(&accounts, "claude-sonnet-4-6", false);
        assert_eq!(ranked[0].id, "claude-heavy");
        assert_eq!(ranked[1].id, "gemini-heavy");
    }

    #[test]
    fn test_rank_probe_candidates_falls_back_to_max_quota_and_stable_id() {
        let accounts = vec![
            test_account(
                "b",
                &[("unknown-model", 50)],
                false,
                false,
                false,
                false,
                false,
            ),
            test_account(
                "a",
                &[("unknown-model", 50)],
                false,
                false,
                false,
                false,
                false,
            ),
            test_account(
                "high",
                &[("unknown-model", 100)],
                false,
                false,
                false,
                false,
                false,
            ),
        ];

        let ranked = rank_probe_candidates(&accounts, "unknown-model", false);
        assert_eq!(ranked[0].id, "high");
        assert_eq!(ranked[1].id, "a");
        assert_eq!(ranked[2].id, "b");
    }
}
