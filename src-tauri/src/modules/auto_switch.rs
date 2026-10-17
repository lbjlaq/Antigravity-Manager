use crate::models::Account;
use crate::models::QuotaData;
use crate::modules::{account, config, logger, process};
use once_cell::sync::Lazy;
use std::sync::Mutex;
use std::time::Instant;

/// 自动热切换防抖/冷却时间：120 秒
const AUTO_SWITCH_COOLDOWN_SECS: u64 = 120;

/// 记录上次自动热切换时间戳，避免因瞬时错误或网络波动引发震荡连续换号
static LAST_CLASSIC_AUTO_SWITCH: Lazy<Mutex<Option<Instant>>> = Lazy::new(|| Mutex::new(None));

/// 提取指定模型大类（Gemini 或 第三方模型/Claude）的有效可用配额百分比 (0-100)
pub fn get_category_percentage(quota: &QuotaData, category: &str) -> Option<i32> {
    let cat = category.to_lowercase();
    let is_claude = cat.contains("claude") || cat.contains("gpt") || cat.contains("third");
    let is_gemini = cat.contains("gemini") || (!is_claude && cat == "gemini");

    // 1. 优先从双窗口 quota_groups 提取
    if let Some(ref groups) = quota.quota_groups {
        for group in groups {
            let name = group.display_name.to_lowercase();
            if name.contains("argon") {
                continue;
            }
            let is_target = if is_claude {
                name.contains("claude") || name.contains("gpt") || name.contains("3p")
            } else if is_gemini {
                name.contains("gemini") || !name.contains("claude")
            } else {
                false
            };

            if is_target {
                // 周配额熔断检查：周额度 <= 5% 视为枯竭
                let weekly_bucket = group.buckets.iter().find(|b| {
                    let win = format!("{} {}", b.window, b.bucket_id).to_lowercase();
                    win.contains("week") || win.contains("7d")
                });
                if let Some(b) = weekly_bucket {
                    if b.remaining_fraction <= 0.05 {
                        return Some(0);
                    }
                }

                // 5H 滚动桶
                let five_hour_bucket = group.buckets.iter().find(|b| {
                    let win = format!("{} {}", b.window, b.bucket_id).to_lowercase();
                    win.contains("5h") || win.contains("hour")
                });
                if let Some(b) = five_hour_bucket {
                    return Some((b.remaining_fraction * 100.0).round() as i32);
                }
            }
        }
    }

    // 2. 回退到单模型 models 列表
    if is_gemini {
        if let Some(m) = quota
            .models
            .iter()
            .find(|m| m.name.to_lowercase().contains("gemini-pro"))
        {
            return Some(m.percentage);
        }
        if let Some(m) = quota
            .models
            .iter()
            .find(|m| m.name.to_lowercase().contains("gemini-flash"))
        {
            return Some(m.percentage);
        }
        if let Some(m) = quota
            .models
            .iter()
            .find(|m| m.name.to_lowercase().contains("gemini"))
        {
            return Some(m.percentage);
        }
    } else if is_claude {
        if let Some(m) = quota
            .models
            .iter()
            .find(|m| m.name.to_lowercase().contains("claude"))
        {
            return Some(m.percentage);
        }
        if let Some(m) = quota
            .models
            .iter()
            .find(|m| m.name.to_lowercase().contains("gpt"))
        {
            return Some(m.percentage);
        }
    }

    None
}

/// 查找指定模型在配额中的有效剩余百分比 (0-100)
pub fn find_model_percentage(quota: &QuotaData, model_pattern: &str) -> Option<i32> {
    let pat = model_pattern.to_lowercase();

    // 0. 若为大类分类（gemini 或 claude/third_party），直接按算力池大类聚合计算
    if pat == "gemini" || pat == "claude" || pat == "third_party" || pat == "third" {
        return get_category_percentage(quota, &pat);
    }

    // 1. 双窗口周配额检查 (quota_groups 熔断)
    //    若对应分类的周配额已耗尽 (<= 0.01)，则无论 5h 剩余多少，该模型均视为耗尽 (0%)
    if let Some(ref groups) = quota.quota_groups {
        let is_gemini_pat = pat.contains("gemini");
        let is_claude_pat = pat.contains("claude") || pat.contains("gpt") || pat.contains("3p");

        for group in groups {
            let group_name = group.display_name.to_lowercase();
            let matches_group = (is_gemini_pat && group_name.contains("gemini"))
                || (is_claude_pat
                    && (group_name.contains("claude")
                        || group_name.contains("gpt")
                        || group_name.contains("3p")));

            if matches_group {
                for bucket in &group.buckets {
                    let win = format!("{} {}", bucket.window, bucket.bucket_id).to_lowercase();
                    if win.contains("week") || win.contains("7d") {
                        if bucket.remaining_fraction <= 0.01 {
                            return Some(0);
                        }
                    }
                }
            }
        }
    }

    // 2. 查找模型列表中的百分比
    // 2.1 精确匹配
    if let Some(m) = quota.models.iter().find(|m| m.name.to_lowercase() == pat) {
        return Some(m.percentage);
    }

    // 2.2 子串包含匹配 (如 model_pattern = "gemini-3.8-flash" 匹配 "gemini-3.8-flash-high")
    if let Some(m) = quota
        .models
        .iter()
        .find(|m| m.name.to_lowercase().contains(&pat))
    {
        return Some(m.percentage);
    }

    // 2.3 反向包含匹配 (如 model.name = "gemini-3.8-flash" 包含在 "gemini-3.8-flash-tiered" 中)
    if let Some(m) = quota
        .models
        .iter()
        .find(|m| pat.contains(&m.name.to_lowercase()))
    {
        return Some(m.percentage);
    }

    None
}

/// 判断账号是否已被判定为“配额耗尽/不可用”
///
/// * `threshold`: 触发阈值百分比 (0-99)。当监控模型配额 <= threshold 时判定为耗尽
/// * `monitored_models`: 监控模型列表。若非空，任一监控模型低于阈值即判定为耗尽；若为空，则全部核心模型归零才触发
pub fn is_account_depleted(account: &Account, threshold: i32, monitored_models: &[String]) -> bool {
    // 1. 禁用或封禁
    if account.disabled || account.validation_blocked {
        return true;
    }

    // 2. 无配额数据时不盲目判定为耗尽，避免未初始化时被误切
    let Some(ref q) = account.quota else {
        return false;
    };

    // 3. 403 访问被拒
    if q.is_forbidden {
        return true;
    }

    // 4. 用户指定了监控模型：任一监控模型额度 <= threshold 即触发
    if !monitored_models.is_empty() {
        for model_id in monitored_models {
            if let Some(pct) = find_model_percentage(q, model_id) {
                if pct <= threshold {
                    return true;
                }
            }
        }
        return false;
    }

    // 5. 默认回退逻辑：未指定监控模型时，检查所有 Gemini 核心模型
    let gemini_models: Vec<_> = q
        .models
        .iter()
        .filter(|m| {
            let name = m.name.to_lowercase();
            name.contains("gemini")
        })
        .collect();

    if !gemini_models.is_empty() {
        let has_available_gemini = gemini_models.iter().any(|m| m.percentage > threshold);
        if !has_available_gemini {
            return true;
        }
    } else if !q.models.is_empty() {
        let has_any = q.models.iter().any(|m| m.percentage > threshold);
        if !has_any {
            return true;
        }
    } else if let Some(pct) = get_category_percentage(q, "gemini") {
        if pct <= threshold {
            return true;
        }
    }

    false
}

/// 计算候选账号的优选评分（分值越高越优先）
pub fn calculate_candidate_score(account: &Account, monitored_models: &[String]) -> i64 {
    let mut score: i64 = 0;

    // 1. 订阅等级梯队 (Tier Priority)
    if let Some(ref q) = account.quota {
        if let Some(ref tier) = q.subscription_tier {
            match tier.to_uppercase().as_str() {
                "ULTRA" => score += 2000,
                "PRO" => score += 1000,
                _ => {}
            }
        }
    }

    // 2. 监控模型配额加权评分
    if let Some(ref q) = account.quota {
        if !monitored_models.is_empty() {
            let mut monitored_sum = 0i64;
            let mut count = 0i64;
            for model_id in monitored_models {
                if let Some(pct) = find_model_percentage(q, model_id) {
                    monitored_sum += pct as i64;
                    count += 1;
                }
            }
            if count > 0 {
                score += monitored_sum / count;
            } else {
                let max_pct = q
                    .models
                    .iter()
                    .map(|m| m.percentage)
                    .max()
                    .or_else(|| get_category_percentage(q, "gemini"))
                    .unwrap_or(0);
                score += max_pct as i64;
            }
        } else {
            let max_pct = q
                .models
                .iter()
                .map(|m| m.percentage)
                .max()
                .or_else(|| get_category_percentage(q, "gemini"))
                .unwrap_or(0);
            score += max_pct as i64;
        }
    }

    // 3. LRU 破局机制：优先选择最久未使用的账号
    let last_used = account.last_used;
    let now = chrono::Utc::now().timestamp();
    let idle_hours = ((now - last_used).max(0) / 3600).min(100);
    score += idle_hours;

    score
}

/// 从所有账号中挑选当前最适合经典款切换的候选账号
pub fn find_best_candidate(
    current_id: &str,
    accounts: &[Account],
    threshold: i32,
    monitored_models: &[String],
) -> Option<Account> {
    accounts
        .iter()
        .filter(|acc| {
            acc.id != current_id
                && !acc.disabled
                && !acc.validation_blocked
                && !is_account_depleted(acc, threshold, monitored_models)
        })
        .max_by_key(|acc| calculate_candidate_score(acc, monitored_models))
        .cloned()
}

/// 检查并执行经典款配额耗尽自动热切换
pub async fn check_and_auto_switch_classic(
    proxy_state: &crate::commands::proxy::ProxyServiceState,
    app_handle: Option<&tauri::AppHandle>,
) -> Result<Option<String>, String> {
    // 1. 配置检查
    let Ok(cfg) = config::load_app_config() else {
        return Ok(None);
    };

    let switch_config = if cfg.classic_auto_switch.enabled {
        cfg.classic_auto_switch
    } else if cfg.auto_switch_classic_depleted {
        // 兼容旧版单一布尔字段
        crate::models::config::ClassicAutoSwitchConfig {
            enabled: true,
            threshold_percentage: 0,
            monitored_models: vec![
                "gemini-3.8-flash".to_string(),
                "claude".to_string(),
                "gemini-3-pro-high".to_string(),
            ],
        }
    } else {
        return Ok(None);
    };

    let threshold = switch_config.threshold_percentage as i32;
    let monitored_models = &switch_config.monitored_models;

    // 2. 冷却时间防震荡检查
    {
        let last_switch = LAST_CLASSIC_AUTO_SWITCH.lock().unwrap();
        if let Some(last_time) = *last_switch {
            if last_time.elapsed().as_secs() < AUTO_SWITCH_COOLDOWN_SECS {
                logger::log_info(
                    "[AutoSwitch] Classic auto-switch cooldown active, skipping check.",
                );
                return Ok(None);
            }
        }
    }

    // 3. 仅当经典款正在运行（运行态）时执行热切换
    let classic_running = process::is_antigravity_running(None);
    if !classic_running {
        return Ok(None);
    }

    // 4. 获取经典款当前生效的账号 ID
    let status = account::detect_active_clients_status()?;
    let Some(current_classic_id) = status.classic_account_id else {
        return Ok(None);
    };

    let accounts = account::list_accounts()?;
    let current_acc = accounts.iter().find(|a| a.id == current_classic_id);

    // 5. 判断当前账号是否低于触发阈值
    let is_depleted = match current_acc {
        Some(acc) => is_account_depleted(acc, threshold, monitored_models),
        None => true, // 当前绑定的账号在本地索引不存在，视为异常需切号
    };

    if !is_depleted {
        return Ok(None);
    }

    let current_email = current_acc
        .map(|a| a.email.clone())
        .unwrap_or_else(|| current_classic_id.clone());
    logger::log_warn(&format!(
        "[AutoSwitch] Current classic account {} fell below threshold (<= {}%). Searching for replacement...",
        current_email, threshold
    ));

    // 6. 挑选最佳候选账号（确保候选账号在监控模型上高于阈值）
    let Some(candidate) =
        find_best_candidate(&current_classic_id, &accounts, threshold, monitored_models)
    else {
        logger::log_warn(
            "[AutoSwitch] All available accounts in pool are depleted! Circuit breaker engaged.",
        );
        return Ok(None);
    };

    logger::log_info(&format!(
        "[AutoSwitch] Auto-switching classic edition from {} to candidate {} (ID: {})",
        current_email, candidate.email, candidate.id
    ));

    // 7. 执行经典款热切号（走 SystemManager 接入完整热切流程）
    let integration = match app_handle {
        Some(h) => crate::modules::integration::SystemManager::Desktop(h.clone()),
        None => crate::modules::integration::SystemManager::Headless,
    };

    account::switch_account(&candidate.id, Some("classic"), &integration).await?;

    // 8. 记录冷却锁
    {
        let mut last_switch = LAST_CLASSIC_AUTO_SWITCH.lock().unwrap();
        *last_switch = Some(Instant::now());
    }

    // 同步反代（如果已启动）
    let instance_lock = proxy_state.instance.read().await;
    if let Some(instance) = instance_lock.as_ref() {
        let _ = instance.token_manager.reload_all_accounts().await;
    }

    // 9. 界面与系统通知提醒
    if let Some(handle) = app_handle {
        use tauri::Emitter;
        let _ = handle.emit("accounts://switched", ());
        let _ = crate::modules::tray::update_tray_menus(handle);

        let msg = format!(
            "经典款当前账号配额低于预设阈值 ({}%)，已自动为您热切换至 {}",
            threshold, candidate.email
        );
        integration.show_notification("Antigravity 经典款自动热切换", &msg);
    }

    Ok(Some(candidate.email))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::quota::{ModelQuota, QuotaBucket, QuotaData, QuotaGroup};
    use crate::models::{Account, TokenData};

    fn make_test_account(
        id: &str,
        email: &str,
        flash_pct: i32,
        claude_pct: i32,
        tier: Option<&str>,
    ) -> Account {
        let mut q = QuotaData::new();
        q.subscription_tier = tier.map(String::from);
        q.models.push(ModelQuota {
            name: "gemini-3.8-flash".to_string(),
            percentage: flash_pct,
            reset_time: "".to_string(),
            display_name: None,
            supports_images: None,
            supports_thinking: None,
            thinking_budget: None,
            recommended: None,
            max_tokens: None,
            max_output_tokens: None,
            model: None,
            supported_mime_types: None,
        });
        q.models.push(ModelQuota {
            name: "claude-sonnet-4-6".to_string(),
            percentage: claude_pct,
            reset_time: "".to_string(),
            display_name: None,
            supports_images: None,
            supports_thinking: None,
            thinking_budget: None,
            recommended: None,
            max_tokens: None,
            max_output_tokens: None,
            model: None,
            supported_mime_types: None,
        });

        let mut acc = Account::new(
            id.to_string(),
            email.to_string(),
            TokenData::new(
                "test_access".to_string(),
                "test_refresh".to_string(),
                3600,
                Some(email.to_string()),
                None,
                None,
                false,
                None,
            ),
        );
        acc.quota = Some(q);
        acc.last_used = 1000;
        acc
    }

    #[test]
    fn test_is_account_depleted_with_custom_threshold() {
        let acc = make_test_account("acc1", "a1@test.com", 15, 80, None);
        let monitored = vec!["gemini-3.8-flash".to_string()];

        // 阈值 20%: 15% <= 20% -> 判定为耗尽
        assert!(is_account_depleted(&acc, 20, &monitored));

        // 阈值 10%: 15% > 10% -> 未耗尽
        assert!(!is_account_depleted(&acc, 10, &monitored));
    }

    #[test]
    fn test_is_account_depleted_specific_model_trigger() {
        let acc = make_test_account("acc1", "a1@test.com", 90, 5, None);

        // 如果用户只监控 claude，5% <= 10% -> 触发耗尽
        let monitored_claude = vec!["claude".to_string()];
        assert!(is_account_depleted(&acc, 10, &monitored_claude));

        // 如果用户只监控 flash，90% > 10% -> 不触发
        let monitored_flash = vec!["gemini-3.8-flash".to_string()];
        assert!(!is_account_depleted(&acc, 10, &monitored_flash));
    }

    #[test]
    fn test_find_best_candidate_satisfies_threshold() {
        let current = make_test_account("current", "cur@test.com", 5, 5, None);
        // candidate 1 has low claude (8%), so with threshold 10% it's depleted for claude
        let cand1 = make_test_account("cand1", "c1@test.com", 80, 8, Some("PRO"));
        // candidate 2 has both high (60%, 70%)
        let cand2 = make_test_account("cand2", "c2@test.com", 60, 70, Some("FREE"));

        let monitored = vec!["claude".to_string()];
        let accounts = vec![current, cand1, cand2.clone()];

        let best = find_best_candidate("current", &accounts, 10, &monitored);
        assert!(best.is_some());
        assert_eq!(best.unwrap().id, "cand2");
    }

    #[test]
    fn test_is_account_depleted_category_gemini_and_claude() {
        let acc = make_test_account("acc1", "a1@test.com", 8, 90, None);

        // 监控 gemini (此时 flash 为 8%)，阈值 10% -> 触发耗尽
        assert!(is_account_depleted(&acc, 10, &["gemini".to_string()]));

        // 监控 claude (此时 claude 为 90%)，阈值 10% -> 未耗尽
        assert!(!is_account_depleted(&acc, 10, &["claude".to_string()]));

        // 两者都监控，任一低于阈值即触发 -> 触发耗尽
        assert!(is_account_depleted(
            &acc,
            10,
            &["gemini".to_string(), "claude".to_string()]
        ));
    }

    #[test]
    fn test_is_account_depleted_with_empty_models_quota_groups_fallback() {
        // 当 q.models 为空时，通过 q.quota_groups 回退判定
        let mut q = QuotaData::new();
        q.quota_groups = Some(vec![QuotaGroup {
            display_name: "Gemini Models".to_string(),
            description: None,
            buckets: vec![
                QuotaBucket {
                    bucket_id: "gemini-5h".to_string(),
                    window: "5h".to_string(),
                    remaining_fraction: 0.05, // 5% <= 10% 耗尽阈值
                    reset_time: "".to_string(),
                    observed_at: None,
                    cycle_tokens: None,
                    display_name: None,
                    description: None,
                },
                QuotaBucket {
                    bucket_id: "gemini-weekly".to_string(),
                    window: "weekly".to_string(),
                    remaining_fraction: 0.80,
                    reset_time: "".to_string(),
                    observed_at: None,
                    cycle_tokens: None,
                    display_name: None,
                    description: None,
                },
            ],
        }]);

        let mut acc = Account::new(
            "acc_empty_models".to_string(),
            "empty@test.com".to_string(),
            TokenData::new(
                "test_access".to_string(),
                "test_refresh".to_string(),
                3600,
                Some("empty@test.com".to_string()),
                None,
                None,
                false,
                None,
            ),
        );
        acc.quota = Some(q);

        // 默认监控空列表时使用 gemini 类别，5% <= 10% -> 耗尽
        assert!(is_account_depleted(&acc, 10, &[]));

        // 监控 gemini，5% <= 10% -> 耗尽
        assert!(is_account_depleted(&acc, 10, &["gemini".to_string()]));

        // 阈值降至 3% 时，5% > 3% -> 未耗尽
        assert!(!is_account_depleted(&acc, 3, &["gemini".to_string()]));
    }
}
