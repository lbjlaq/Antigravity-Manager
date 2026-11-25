use crate::models::Account;
use crate::modules::{account, config, logger, quota};
use chrono::{DateTime, Utc};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use tokio::time::{self, Duration};

// Warmup history: key = "email:bucket_or_model:weekly:cycle_id", value = warmup timestamp
static WARMUP_HISTORY: Lazy<Mutex<HashMap<String, i64>>> =
    Lazy::new(|| Mutex::new(load_warmup_history()));

fn get_warmup_history_path() -> Result<PathBuf, String> {
    let data_dir = account::get_data_dir()?;
    Ok(data_dir.join("warmup_history.json"))
}

fn load_warmup_history() -> HashMap<String, i64> {
    match get_warmup_history_path() {
        Ok(path) if path.exists() => match std::fs::read_to_string(&path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => HashMap::new(),
        },
        _ => HashMap::new(),
    }
}

fn save_warmup_history(history: &HashMap<String, i64>) {
    if let Ok(path) = get_warmup_history_path() {
        if let Ok(content) = serde_json::to_string_pretty(history) {
            let _ = std::fs::write(&path, content);
        }
    }
}

pub fn record_warmup_history(key: &str, timestamp: i64) {
    let mut history = WARMUP_HISTORY.lock().unwrap();
    history.insert(key.to_string(), timestamp);
    save_warmup_history(&history);
}

pub fn check_cooldown(key: &str, cooldown_seconds: i64) -> bool {
    let history = WARMUP_HISTORY.lock().unwrap();
    if let Some(&last_ts) = history.get(key) {
        let now = chrono::Utc::now().timestamp();
        now - last_ts < cooldown_seconds
    } else {
        false
    }
}

/// Helper to parse ISO8601 / RFC3339 string to timestamp
fn parse_reset_time_ts(s: &str) -> Option<i64> {
    if s.is_empty() {
        return None;
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.timestamp());
    }
    if let Ok(dt) = DateTime::parse_from_str(s, "%Y/%m/%d %H:%M:%S") {
        return Some(dt.timestamp());
    }
    None
}

/// Select a real upstream model from `pool_models` to ping for a given group, honoring
/// any valid model in `monitored_models` that actually exists on the account.
///
/// Never returns logical group placeholders (such as `"claude"` or `"gemini"`) or retired IDs
/// absent from `pool_models`, avoiding upstream 404 errors during warmup.
fn pick_model_for_group(
    group_name: &str,
    bucket_id: &str,
    monitored_models: &[String],
    pool_models: &[String],
) -> String {
    let active_pool: Vec<&str> = pool_models
        .iter()
        .map(|s| s.trim())
        .filter(|m| {
            !m.is_empty()
                && !m.to_lowercase().contains("-disabled")
                && !m.to_lowercase().ends_with("_disabled")
                && !m.eq_ignore_ascii_case("gemini-flash-worker-agy")
        })
        .collect();

    let find_in_pool = |candidate: &str| -> Option<String> {
        let trimmed = candidate.trim();
        active_pool
            .iter()
            .find(|m| m.eq_ignore_ascii_case(trimmed))
            .map(|m| (*m).to_string())
    };

    let is_3p = bucket_id.to_lowercase().contains("3p")
        || group_name.to_lowercase().contains("claude")
        || group_name.to_lowercase().contains("gpt");

    if is_3p {
        for m in monitored_models {
            let l = m.to_lowercase();
            if l.contains("claude") || l.contains("gpt") {
                if let Some(actual) = find_in_pool(m) {
                    return actual;
                }
            }
        }
        // 3P 兜底：从当前账号真实拥有的激活模型中挑选，绝不发送裸 "claude" 分组占位名
        active_pool
            .iter()
            .find(|m| {
                let l = m.to_lowercase();
                l.contains("claude") || l.contains("gpt")
            })
            .map(|m| (*m).to_string())
            .unwrap_or_default()
    } else {
        for m in monitored_models {
            if m.to_lowercase().contains("gemini") {
                if let Some(actual) = find_in_pool(m) {
                    return actual;
                }
            }
        }
        // Gemini 兜底：取号池最新 flash-low / 无后缀衍生 flash；若无则回退号池首个激活 Gemini 模型
        let pool_set: std::collections::HashSet<String> =
            active_pool.iter().map(|m| (*m).to_string()).collect();
        crate::proxy::common::model_gate::pick_background_model(&pool_set).unwrap_or_else(|| {
            active_pool
                .iter()
                .find(|m| m.to_lowercase().starts_with("gemini-"))
                .map(|m| (*m).to_string())
                .unwrap_or_default()
        })
    }
}

/// Start smart weekly scheduler
pub fn start_scheduler(
    app_handle: Option<tauri::AppHandle>,
    proxy_state: crate::commands::proxy::ProxyServiceState,
) {
    tauri::async_runtime::spawn(async move {
        logger::log_info(
            "[Scheduler] Weekly Reset Warmup Scheduler started. Monitoring 7-day quota windows...",
        );

        // Scan every 5 minutes (300s) to check for accounts reaching weekly reset time
        let mut interval = time::interval(Duration::from_secs(300));

        loop {
            interval.tick().await;

            // Load configuration
            let Ok(app_config) = config::load_app_config() else {
                continue;
            };

            // Must be enabled by user in Settings
            if !app_config.scheduled_warmup.enabled {
                continue;
            }

            let Ok(accounts) = account::list_accounts() else {
                continue;
            };

            if accounts.is_empty() {
                continue;
            }

            let now_ts = Utc::now().timestamp();
            let mut tasks_to_run = Vec::new();

            for acc in &accounts {
                if acc.disabled || acc.proxy_disabled || acc.banned {
                    continue;
                }

                let Ok((token, pid)) = quota::get_valid_token_for_warmup(acc).await else {
                    continue;
                };

                let Ok((fresh_quota, _)) =
                    quota::fetch_quota_with_cache(&token, &acc.email, Some(&pid), Some(&acc.id))
                        .await
                else {
                    continue;
                };

                if fresh_quota.is_forbidden {
                    continue;
                }

                // Check quota_groups for WEEKLY buckets
                if let Some(groups) = &fresh_quota.quota_groups {
                    for group in groups {
                        for bucket in &group.buckets {
                            let win_lower = bucket.window.to_lowercase();
                            let bid_lower = bucket.bucket_id.to_lowercase();
                            let is_weekly = win_lower.contains("week")
                                || bid_lower.contains("week")
                                || win_lower.contains("7d")
                                || bid_lower.contains("7d");
                            if !is_weekly {
                                continue;
                            }

                            // If fraction is 1.0 (100% full)
                            if bucket.remaining_fraction >= 0.999 {
                                let reset_ts_opt = parse_reset_time_ts(&bucket.reset_time);
                                let should_warmup = match reset_ts_opt {
                                    Some(reset_ts) => {
                                        // Periodic reset: current time has passed reset_time (with 1 min buffer)
                                        now_ts >= reset_ts - 60
                                    }
                                    None => {
                                        // Cold start: reset_time is empty (e.g. Gemini weekly bucket not yet activated this week)
                                        // Triggering a warmup activates the 7-day timer upstream.
                                        true
                                    }
                                };

                                if should_warmup {
                                    let history_key = match reset_ts_opt {
                                        Some(reset_ts) => format!(
                                            "{}:{}:weekly:{}",
                                            acc.email, bucket.bucket_id, reset_ts
                                        ),
                                        None => format!(
                                            "{}:{}:weekly:initial",
                                            acc.email, bucket.bucket_id
                                        ),
                                    };

                                    // 6-day cooldown for the same cycle
                                    if !check_cooldown(&history_key, 6 * 86400) {
                                        let pool_models: Vec<String> = fresh_quota
                                            .models
                                            .iter()
                                            .map(|m| m.name.clone())
                                            .collect();
                                        let model_to_ping = pick_model_for_group(
                                            &group.display_name,
                                            &bucket.bucket_id,
                                            &app_config.scheduled_warmup.monitored_models,
                                            &pool_models,
                                        );
                                        if model_to_ping.is_empty() {
                                            continue;
                                        }

                                        tasks_to_run.push((
                                            acc.id.clone(),
                                            acc.email.clone(),
                                            model_to_ping,
                                            token.clone(),
                                            pid.clone(),
                                            history_key,
                                        ));
                                    }
                                }
                            }
                        }
                    }
                } else {
                    // Fallback to models if quota_groups is not populated
                    for model in &fresh_quota.models {
                        if model.percentage == 100 {
                            if !app_config
                                .scheduled_warmup
                                .monitored_models
                                .contains(&model.name)
                            {
                                continue;
                            }
                            if let Some(reset_ts) = parse_reset_time_ts(&model.reset_time) {
                                if now_ts >= reset_ts - 60 {
                                    let history_key =
                                        format!("{}:{}:weekly:{}", acc.email, model.name, reset_ts);
                                    if !check_cooldown(&history_key, 6 * 86400) {
                                        tasks_to_run.push((
                                            acc.id.clone(),
                                            acc.email.clone(),
                                            model.name.clone(),
                                            token.clone(),
                                            pid.clone(),
                                            history_key,
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Execute weekly warmup tasks
            if !tasks_to_run.is_empty() {
                logger::log_info(&format!(
                    "[Scheduler] 🎯 Reached weekly reset for {} account targets. Triggering warmup...",
                    tasks_to_run.len()
                ));

                let handle_for_warmup = app_handle.clone();
                let state_for_warmup = proxy_state.clone();

                tokio::spawn(async move {
                    for (acc_id, email, model, token, pid, history_key) in tasks_to_run {
                        logger::log_info(&format!(
                            "[WeeklyWarmup] 🚀 Triggering weekly warmup for {} @ {}",
                            model, email
                        ));

                        let success = quota::warmup_model_directly(
                            &token,
                            &model,
                            &pid,
                            &email,
                            100,
                            Some(&acc_id),
                            false,
                        )
                        .await;

                        if success {
                            let now = Utc::now().timestamp();
                            record_warmup_history(&history_key, now);
                            logger::log_info(&format!(
                                "[WeeklyWarmup] ✅ Successfully started weekly timer for {} @ {}",
                                model, email
                            ));
                        }
                        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    }

                    // Refresh UI
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    let _ = crate::commands::refresh_all_quotas_internal(
                        &state_for_warmup,
                        handle_for_warmup,
                    )
                    .await;
                });
            }

            // Regularly clean up history (keep last 30 days)
            {
                let now_ts = Utc::now().timestamp();
                let mut history = WARMUP_HISTORY.lock().unwrap();
                let cutoff = now_ts - 30 * 86400;
                history.retain(|_, &mut ts| ts > cutoff);
            }
        }
    });
}

/// Trigger immediate smart warmup check for a single account (e.g. on manual trigger / recovered event)
pub async fn trigger_warmup_for_account(account: &Account) {
    let Ok((token, pid)) = quota::get_valid_token_for_warmup(account).await else {
        return;
    };

    let Ok((fresh_quota, _)) =
        quota::fetch_quota_with_cache(&token, &account.email, Some(&pid), Some(&account.id)).await
    else {
        return;
    };

    if fresh_quota.is_forbidden {
        return;
    }

    let Ok(app_config) = config::load_app_config() else {
        return;
    };

    if !app_config.scheduled_warmup.enabled {
        return;
    }

    let now_ts = Utc::now().timestamp();
    if let Some(groups) = fresh_quota.quota_groups {
        for group in groups {
            for bucket in group.buckets {
                let win_lower = bucket.window.to_lowercase();
                let bid_lower = bucket.bucket_id.to_lowercase();
                let is_weekly = win_lower.contains("week")
                    || bid_lower.contains("week")
                    || win_lower.contains("7d")
                    || bid_lower.contains("7d");
                if !is_weekly {
                    continue;
                }

                if bucket.remaining_fraction >= 0.999 {
                    let reset_ts_opt = parse_reset_time_ts(&bucket.reset_time);
                    let should_warmup = match reset_ts_opt {
                        Some(reset_ts) => now_ts >= reset_ts - 60,
                        None => true, // Cold-start for uninitialized weekly timer (Gemini)
                    };

                    if should_warmup {
                        let history_key = match reset_ts_opt {
                            Some(reset_ts) => {
                                format!(
                                    "{}:{}:weekly:{}",
                                    account.email, bucket.bucket_id, reset_ts
                                )
                            }
                            None => {
                                format!("{}:{}:weekly:initial", account.email, bucket.bucket_id)
                            }
                        };

                        if !check_cooldown(&history_key, 6 * 86400) {
                            let pool_models: Vec<String> =
                                fresh_quota.models.iter().map(|m| m.name.clone()).collect();
                            let model_to_ping = pick_model_for_group(
                                &group.display_name,
                                &bucket.bucket_id,
                                &app_config.scheduled_warmup.monitored_models,
                                &pool_models,
                            );
                            if model_to_ping.is_empty() {
                                continue;
                            }

                            let success = quota::warmup_model_directly(
                                &token,
                                &model_to_ping,
                                &pid,
                                &account.email,
                                100,
                                Some(&account.id),
                                false,
                            )
                            .await;

                            if success {
                                record_warmup_history(&history_key, now_ts);
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pick_model_for_group_never_returns_bare_group_placeholders() {
        let monitored = vec!["claude".to_string(), "gemini".to_string()];
        let pool = vec![
            "gemini-3.8-flash-high".to_string(),
            "gemini-3.8-flash-low".to_string(),
            "claude-sonnet-4-6".to_string(),
            "claude-opus-4-6-thinking".to_string(),
        ];

        let claude_model =
            pick_model_for_group("Claude and GPT models", "3p-weekly", &monitored, &pool);
        assert_ne!(claude_model, "claude");
        assert!(pool.contains(&claude_model));

        let gemini_model =
            pick_model_for_group("Gemini Models", "gemini-weekly", &monitored, &pool);
        assert_ne!(gemini_model, "gemini");
        assert_eq!(gemini_model, "gemini-3.8-flash-low");
    }

    #[test]
    fn test_pick_model_for_group_honors_present_monitored_and_skips_retired() {
        let pool = vec![
            "gemini-4-neon-high".to_string(),
            "claude-opus-4-6-thinking".to_string(),
        ];

        // Present monitored model is honored
        let explicit = pick_model_for_group(
            "Claude and GPT models",
            "3p-weekly",
            &["claude-opus-4-6-thinking".to_string()],
            &pool,
        );
        assert_eq!(explicit, "claude-opus-4-6-thinking");

        // Retired monitored model not in pool falls back to an active pool model
        let fallback_gemini = pick_model_for_group(
            "Gemini Models",
            "gemini-weekly",
            &["gemini-3.1-pro-high".to_string()],
            &pool,
        );
        assert_eq!(fallback_gemini, "gemini-4-neon-high");
    }
}
