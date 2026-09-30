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

/// Select a model to ping for a given group, honoring user's monitored_models preference if available
fn pick_model_for_group(group_name: &str, bucket_id: &str, monitored_models: &[String]) -> String {
    let is_3p = bucket_id.to_lowercase().contains("3p")
        || group_name.to_lowercase().contains("claude")
        || group_name.to_lowercase().contains("gpt");

    if is_3p {
        if let Some(m) = monitored_models.iter().find(|m| {
            let l = m.to_lowercase();
            l.contains("claude") || l.contains("gpt")
        }) {
            return m.clone();
        }
        "claude-sonnet-4-6".to_string()
    } else {
        if let Some(m) = monitored_models.iter().find(|m| {
            let l = m.to_lowercase();
            l.contains("gemini")
        }) {
            return m.clone();
        }
        "gemini-3-flash".to_string()
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

            // 优先执行多账号相控阵智能调度
            if app_config.phase_scheduler.enabled {
                run_phase_scheduler_tick(&app_config.phase_scheduler, &app_handle, &proxy_state).await;
            }

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
                if acc.disabled || acc.proxy_disabled {
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
                                        let model_to_ping = pick_model_for_group(
                                            &group.display_name,
                                            &bucket.bucket_id,
                                            &app_config.scheduled_warmup.monitored_models,
                                        );

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
                            let model_to_ping = pick_model_for_group(
                                &group.display_name,
                                &bucket.bucket_id,
                                &app_config.scheduled_warmup.monitored_models,
                            );

                            let success = quota::warmup_model_directly(
                                &token,
                                &model_to_ping,
                                &pid,
                                &account.email,
                                100,
                                Some(&account.id),
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

/// 运行多账号相控阵错峰调度 Tick
pub async fn run_phase_scheduler_tick(
    phase_config: &crate::models::PhaseSchedulerConfig,
    _app_handle: &Option<tauri::AppHandle>,
    _proxy_state: &crate::commands::proxy::ProxyServiceState,
) {
    let active_ide_id = crate::modules::account::detect_and_get_active_ide_account().map(|a| a.id);
    let Ok(accounts) = account::list_accounts() else {
        return;
    };

    let schedulable_accounts: Vec<_> = accounts
        .into_iter()
        .filter(|a| !a.disabled && !a.proxy_disabled && active_ide_id.as_deref() != Some(&a.id))
        .collect();

    let n = schedulable_accounts.len();
    if n == 0 {
        return;
    }

    let now_local = chrono::Local::now();
    let now_ts = chrono::Utc::now().timestamp();

    if phase_config.mode == "steady" {
        // 平稳错峰模式：5h窗口（300分钟），错峰步长 delta = 300 / n 分钟 (如 5 个账号为 60 分钟)
        let delta_mins = 300 / (n as i64);

        let parts: Vec<&str> = phase_config.work_start_time.split(':').collect();
        let start_hour = parts.first().and_then(|s| s.parse::<u32>().ok()).unwrap_or(9);
        let start_min = parts.get(1).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);

        for (i, acc) in schedulable_accounts.iter().enumerate() {
            let offset_mins = 30 + (i as i64) * delta_mins;
            let warmup_offset_from_start = offset_mins - 300;

            if let Some(base_time) = now_local
                .date_naive()
                .and_hms_opt(start_hour, start_min, 0)
                .and_then(|dt| dt.and_local_timezone(chrono::Local).single())
            {
                let target_warmup_time =
                    base_time + chrono::Duration::minutes(warmup_offset_from_start);
                let diff_secs = (now_local - target_warmup_time).num_seconds();

                let history_key = format!("{}:phase_warmup", acc.email);
                let cooldown_ok = !check_cooldown(&history_key, 18000);

                if diff_secs >= -180 && diff_secs <= 360 && cooldown_ok {
                    logger::log_info(&format!(
                        "⚡ [PhaseScheduler] 触发平稳模式错峰预热: 账号 {} (相位 #{} / 步长 {}m), 目标于开工后 +{}m 满额刷新",
                        acc.email, i, delta_mins, offset_mins
                    ));

                    if let Ok((token, pid)) = quota::get_valid_token_for_warmup(acc).await {
                        let target_model = phase_config
                            .monitored_models
                            .first()
                            .map(|s| s.as_str())
                            .unwrap_or("gemini-3-flash");
                        let res = quota::warmup_model_directly(
                            &token,
                            target_model,
                            &pid,
                            &acc.email,
                            100,
                            Some(&acc.id),
                        )
                        .await;
                        if res {
                            record_warmup_history(&history_key, now_ts);
                        }
                    }
                }
            }
        }
    } else if phase_config.mode == "burst" {
        if phase_config.burst_mode_type == "scheduled" {
            // 预约爆发模式：解析预约开始时间 (如 "20:00")
            let parts: Vec<&str> = phase_config.burst_start_time.split(':').collect();
            let burst_hour = parts.first().and_then(|s| s.parse::<u32>().ok()).unwrap_or(20);
            let burst_min = parts.get(1).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);

            if let Some(burst_time) = now_local
                .date_naive()
                .and_hms_opt(burst_hour, burst_min, 0)
                .and_then(|dt| dt.and_local_timezone(chrono::Local).single())
            {
                // 前置对齐打卡时间点 = 预约爆发时间前 5 小时 (300 分钟)
                let pre_warmup_time = burst_time - chrono::Duration::minutes(300);
                let diff_secs = (now_local - pre_warmup_time).num_seconds();

                if diff_secs >= -180 && diff_secs <= 360 {
                    logger::log_info(&format!(
                        "🚀 [PhaseScheduler] 触发狂暴模式预约前置打卡 (预约于 {:02}:{:02} 爆发 {}h): 提前5小时预热对齐相位...",
                        burst_hour, burst_min, phase_config.burst_duration_hours
                    ));

                    // 对池内前3个账号执行预热打卡埋点
                    for acc in schedulable_accounts.iter().take(3) {
                        let history_key = format!("{}:burst_pre_warmup", acc.email);
                        if !check_cooldown(&history_key, 18000) {
                            if let Ok((token, pid)) = quota::get_valid_token_for_warmup(acc).await {
                                let target_model = phase_config
                                    .monitored_models
                                    .first()
                                    .map(|s| s.as_str())
                                    .unwrap_or("gemini-3-flash");
                                let res = quota::warmup_model_directly(
                                    &token,
                                    target_model,
                                    &pid,
                                    &acc.email,
                                    100,
                                    Some(&acc.id),
                                )
                                .await;
                                if res {
                                    record_warmup_history(&history_key, now_ts);
                                    logger::log_info(&format!(
                                        "✓ [PhaseScheduler] 账号 {} 完成狂暴预约前置对齐预热",
                                        acc.email
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        } else {
            logger::log_info(&format!(
                "🚀 [PhaseScheduler] 即时狂暴模式运行中 (持续 {}h)，全速并发调度最优账号",
                phase_config.burst_duration_hours
            ));
        }
    }
}
