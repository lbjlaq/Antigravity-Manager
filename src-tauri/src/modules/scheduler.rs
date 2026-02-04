use chrono::Utc;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;
use tokio::time::{self, Duration};
use crate::modules::{config, logger, quota, account};
use crate::models::Account;
use std::path::PathBuf;

// Warmup history: key = "email:model_name:100", value = warmup timestamp
static WARMUP_HISTORY: Lazy<Mutex<HashMap<String, i64>>> = Lazy::new(|| Mutex::new(load_warmup_history()));

fn get_warmup_history_path() -> Result<PathBuf, String> {
    let data_dir = account::get_data_dir()?;
    Ok(data_dir.join("warmup_history.json"))
}

fn load_warmup_history() -> HashMap<String, i64> {
    match get_warmup_history_path() {
        Ok(path) if path.exists() => {
            match std::fs::read_to_string(&path) {
                Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
                Err(_) => HashMap::new(),
            }
        }
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

pub fn start_scheduler(app_handle: Option<tauri::AppHandle>, proxy_state: crate::commands::proxy::ProxyServiceState) {
    tauri::async_runtime::spawn(async move {
        logger::log_info("Smart Warmup Scheduler started. Monitoring quota at 100%...");
        
        // Scan every 10 minutes
        let mut interval = time::interval(Duration::from_secs(600));

        loop {
            interval.tick().await;

            // Load configuration
            let Ok(app_config) = config::load_app_config() else {
                continue;
            };

            if !app_config.scheduled_warmup.enabled {
                continue;
            }
            
            // Get all accounts (no longer filtering by level)
            let Ok(accounts) = account::list_accounts() else {
                continue;
            };

            if accounts.is_empty() {
                continue;
            }

            logger::log_info(&format!(
                "[Scheduler] Scanning {} accounts for 100% quota models...",
                accounts.len()
            ));

            let mut warmup_tasks = Vec::new();
            let mut skipped_cooldown = 0;

            // Scan each model for each account
            for account in &accounts {
                // Skip disabled accounts
                if account.disabled || account.proxy_disabled {
                    continue;
                }

                // Get valid token
                let Ok((token, pid)) = quota::get_valid_token_for_warmup(account).await else {
                    continue;
                };

                // Get fresh quota
                let Ok((fresh_quota, _)) = quota::fetch_quota_with_cache(&token, &account.email, Some(&pid), Some(&account.id)).await else {
                    continue;
                };

                let now_ts = Utc::now().timestamp();

                for model in fresh_quota.models {
                    // Core logic: detect 100% quota
                    if model.percentage == 100 {
                        let model_to_ping = model.name.clone();

                        // Only warmup models configured by user (allowlist)
                        if !app_config.scheduled_warmup.monitored_models.contains(&model_to_ping) {
                            continue;
                        }

                        // Use mapped name as key
                        let history_key = format!("{}:{}:100", account.email, model_to_ping);
                        
                        // Check cooldown: do not repeat warmup within 4 hours
                        {
                            let history = WARMUP_HISTORY.lock().unwrap();
                            if let Some(&last_warmup_ts) = history.get(&history_key) {
                                let cooldown_seconds = 14400;
                                if now_ts - last_warmup_ts < cooldown_seconds {
                                    skipped_cooldown += 1;
                                    continue;
                                }
                            }
                        }

                        warmup_tasks.push((
                            account.id.clone(),
                            account.email.clone(),
                            model_to_ping.clone(),
                            token.clone(),
                            pid.clone(),
                            model.percentage,
                            history_key.clone(),
                        ));

                        logger::log_info(&format!(
                            "[Scheduler] ✓ Scheduled warmup: {} @ {} (quota at 100%)",
                            model_to_ping, account.email
                        ));
                    } else if model.percentage < 100 {
                        // Quota not full, clear history, need to map name first
                        let model_to_ping = model.name.clone();
                        let history_key = format!("{}:{}:100", account.email, model_to_ping);
                        
                        let mut history = WARMUP_HISTORY.lock().unwrap();
                        if history.remove(&history_key).is_some() {
                            save_warmup_history(&history);
                            logger::log_info(&format!(
                                "[Scheduler] Cleared history for {} @ {} (quota: {}%)",
                                model_to_ping, account.email, model.percentage
                            ));
                        }
                    }
                }
            }

            // Execute warmup tasks
            if !warmup_tasks.is_empty() {
                let total = warmup_tasks.len();
                if skipped_cooldown > 0 {
                    logger::log_info(&format!(
                        "[Scheduler] Skipped {} models in cooldown, will warmup {}",
                        skipped_cooldown, total
                    ));
                }
                logger::log_info(&format!(
                    "[Scheduler] 🔥 Triggering {} warmup tasks...",
                    total
                ));

                let handle_for_warmup = app_handle.clone();
                let state_for_warmup = proxy_state.clone();

                tokio::spawn(async move {
                    let mut success = 0;
                    let batch_size = 3;
                    let now_ts = chrono::Utc::now().timestamp();
                    
                    for (batch_idx, batch) in warmup_tasks.chunks(batch_size).enumerate() {
                        let mut handles = Vec::new();
                        
                        for (task_idx, (id, email, model, token, pid, pct, history_key)) in batch.iter().enumerate() {
                            let global_idx = batch_idx * batch_size + task_idx + 1;
                            let id = id.clone();
                            let email = email.clone();
                            let model = model.clone();
                            let token = token.clone();
                            let pid = pid.clone();
                            let pct = *pct;
                            let history_key = history_key.clone();
                            
                            logger::log_info(&format!(
                                "[Warmup {}/{}] {} @ {} ({}%)",
                                global_idx, total, model, email, pct
                            ));
                            
                            let handle = tokio::spawn(async move {
                                let result = quota::warmup_model_directly(&token, &model, &pid, &email, pct, Some(&id)).await;
                                (result, history_key)
                            });
                            handles.push(handle);
                        }
                        
                        for handle in handles {
                            match handle.await {
                                Ok((true, history_key)) => {
                                    success += 1;
                                    record_warmup_history(&history_key, now_ts);
                                }
                                _ => {}
                            }
                        }
                        
                        if batch_idx < (warmup_tasks.len() + batch_size - 1) / batch_size - 1 {
                            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                        }
                    }

                    logger::log_info(&format!(
                        "[Scheduler] ✅ Warmup completed: {}/{} successful",
                        success, total
                    ));

                    // Refresh quota
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    let _ = crate::commands::refresh_all_quotas_internal(&state_for_warmup, handle_for_warmup).await;
                });
            } else if skipped_cooldown > 0 {
                logger::log_info(&format!(
                    "[Scheduler] Scan completed, all 100% models are in cooldown, skipped {}",
                    skipped_cooldown
                ));
            } else {
                logger::log_info("[Scheduler] Scan completed, no models with 100% quota need warmup");
            }

            // Sync to frontend if handle exists
            if let Some(handle) = app_handle.as_ref() {
                let handle_inner = handle.clone();
                let state_inner = proxy_state.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    let _ = crate::commands::refresh_all_quotas_internal(&state_inner, Some(handle_inner)).await;
                    logger::log_info("[Scheduler] Quota data synced to frontend");
                });
            }

            // Regularly clean up history (keep last 24 hours)
            {
                let now_ts = Utc::now().timestamp();
                let mut history = WARMUP_HISTORY.lock().unwrap();
                let cutoff = now_ts - 86400; // 24 hours ago
                history.retain(|_, &mut ts| ts > cutoff);
            }
        }
    });
}

/// Trigger immediate smart warmup check for a single account
pub async fn trigger_warmup_for_account(account: &Account) {
    if account.disabled || account.proxy_disabled {
        return;
    }

    // Get valid token
    let Ok((token, pid)) = quota::get_valid_token_for_warmup(account).await else {
        return;
    };

    // Get quota info (prefer cache as refresh command likely just updated disk/cache)
    let Ok((fresh_quota, _)) = quota::fetch_quota_with_cache(&token, &account.email, Some(&pid), Some(&account.id)).await else {
        return;
    };

    // Load config once at the beginning
    let Ok(app_config) = config::load_app_config() else {
        logger::log_warn("[Scheduler] Failed to load app config, skipping warmup check");
        return;
    };

    let now_ts = Utc::now().timestamp();
    let mut tasks_to_run = Vec::new();

    for model in fresh_quota.models {
        let model_name = model.name.clone();
        let history_key = format!("{}:{}:100", account.email, model_name);

        if model.percentage == 100 {
            // First check if model is in user's monitored list
            if !app_config.scheduled_warmup.monitored_models.contains(&model_name) {
                continue;
            }

            // Then check cooldown history
            {
                let history = WARMUP_HISTORY.lock().unwrap();

                // 4 hour cooldown (Pro account resets every 5h, 1h margin)
                if let Some(&last_warmup_ts) = history.get(&history_key) {
                    let cooldown_seconds = 14400;
                    if now_ts - last_warmup_ts < cooldown_seconds {
                        // Still in cooldown, skip
                        continue;
                    }
                }
            }
            // Note: Don't write history here - only write after successful warmup

            tasks_to_run.push((model_name, model.percentage, history_key));
        } else if model.percentage < 100 {
            // Quota not full, clear history, allow warmup next time it's 100%
            let mut history = WARMUP_HISTORY.lock().unwrap();
            if history.remove(&history_key).is_some() {
                save_warmup_history(&history);
            }
        }
    }

    // Execute warmup and record history only on success
    if !tasks_to_run.is_empty() {
        logger::log_info(&format!(
            "[Scheduler] Found {} models ready for warmup on {}",
            tasks_to_run.len(), account.email
        ));

        for (model, pct, history_key) in tasks_to_run {
            logger::log_info(&format!(
                "[Scheduler] 🔥 Triggering individual warmup: {} @ {} (Sync)",
                model, account.email
            ));

            let success = quota::warmup_model_directly(&token, &model, &pid, &account.email, pct, Some(&account.id)).await;

            // Only record history if warmup was successful
            if success {
                record_warmup_history(&history_key, now_ts);
            }
        }
    }
}
