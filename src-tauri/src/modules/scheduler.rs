use chrono::Utc;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;
use tokio::time::{self, Duration};
use tauri::Manager;
use crate::modules::{config, logger, quota, account};
use crate::models::Account;
use std::path::PathBuf;

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

pub fn start_scheduler(app_handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        logger::log_info("Smart Warmup Scheduler started. Monitoring quota at 100%...");
        
        let mut interval = time::interval(Duration::from_secs(600));

        loop {
            interval.tick().await;

            let Ok(app_config) = config::load_app_config() else {
                continue;
            };

            if !app_config.scheduled_warmup.enabled {
                continue;
            }
            
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

            for account in &accounts {
                let Ok((token, pid)) = quota::get_valid_token_for_warmup(account).await else {
                    continue;
                };

                let Ok((fresh_quota, _)) = quota::fetch_quota_with_cache(&token, &account.email, Some(&pid)).await else {
                    continue;
                };

                let now_ts = Utc::now().timestamp();

                for model in fresh_quota.models {
                    let model_to_ping = if model.name == "gemini-2.5-flash" {
                        "gemini-3-flash".to_string()
                    } else {
                        model.name.clone()
                    };
                    
                    if model.percentage == 100 {
                        if !app_config.scheduled_warmup.monitored_models.contains(&model_to_ping) {
                            continue;
                        }

                        let history_key = format!("{}:{}:100", account.email, model_to_ping);
                        
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

            if !warmup_tasks.is_empty() {
                let total = warmup_tasks.len();
                if skipped_cooldown > 0 {
                    logger::log_info(&format!(
                        "[Scheduler] 已跳过 {} 个冷却期内的模型，将预热 {} 个",
                        skipped_cooldown, total
                    ));
                }
                logger::log_info(&format!(
                    "[Scheduler] 🔥 Triggering {} warmup tasks...",
                    total
                ));

                let handle_for_warmup = app_handle.clone();
                tokio::spawn(async move {
                    let mut success = 0;
                    let batch_size = 3;
                    let now_ts = chrono::Utc::now().timestamp();
                    
                    for (batch_idx, batch) in warmup_tasks.chunks(batch_size).enumerate() {
                        let mut handles = Vec::new();
                        
                        for (task_idx, (email, model, token, pid, pct, history_key)) in batch.iter().enumerate() {
                            let global_idx = batch_idx * batch_size + task_idx + 1;
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
                                let result = quota::warmup_model_directly(&token, &model, &pid, &email, pct).await;
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

                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    let state = handle_for_warmup.state::<crate::commands::proxy::ProxyServiceState>();
                    let _ = crate::commands::refresh_all_quotas(state).await;
                });
            } else if skipped_cooldown > 0 {
                logger::log_info(&format!(
                    "[Scheduler] 扫描完成，所有100%模型均在冷却期内，已跳过 {} 个",
                    skipped_cooldown
                ));
            } else {
                logger::log_info("[Scheduler] 扫描完成，无100%额度的模型需要预热");
            }

            let handle_inner = app_handle.clone();
            tokio::spawn(async move {
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                let state = handle_inner.state::<crate::commands::proxy::ProxyServiceState>();
                let _ = crate::commands::refresh_all_quotas(state).await;
                logger::log_info("[Scheduler] Quota data synced to frontend");
            });

            {
                let now_ts = Utc::now().timestamp();
                let mut history = WARMUP_HISTORY.lock().unwrap();
                let cutoff = now_ts - 86400;
                history.retain(|_, &mut ts| ts > cutoff);
            }
        }
    });
}

#[allow(dead_code)]
pub async fn trigger_warmup_for_account(account: &Account) {
    let Ok((token, pid)) = quota::get_valid_token_for_warmup(account).await else {
        return;
    };

    let Ok((fresh_quota, _)) = quota::fetch_quota_with_cache(&token, &account.email, Some(&pid)).await else {
        return;
    };

    let now_ts = Utc::now().timestamp();
    let mut tasks_to_run = Vec::new();

    for model in fresh_quota.models {
        let model_to_ping = if model.name == "gemini-2.5-flash" {
            "gemini-3-flash".to_string()
        } else {
            model.name.clone()
        };

        let history_key = format!("{}:{}:100", account.email, model_to_ping);
        
        if model.percentage == 100 {
            {
                let mut history = WARMUP_HISTORY.lock().unwrap();
                
                if let Some(&last_warmup_ts) = history.get(&history_key) {
                    let cooldown_seconds = 14400;
                    if now_ts - last_warmup_ts < cooldown_seconds {
                        continue;
                    }
                }
                
                history.insert(history_key.clone(), now_ts);
                save_warmup_history(&history);
            }

            let Ok(app_config) = config::load_app_config() else {
                continue;
            };

            if app_config.scheduled_warmup.monitored_models.contains(&model_to_ping) {
                tasks_to_run.push((model_to_ping, model.percentage));
            }
        } else if model.percentage < 100 {
            let mut history = WARMUP_HISTORY.lock().unwrap();
            history.remove(&history_key);
        }
    }

    if !tasks_to_run.is_empty() {
        for (model, pct) in tasks_to_run {
            logger::log_info(&format!(
                "[Scheduler] 🔥 Triggering individual warmup: {} @ {} (Sync)",
                model, account.email
            ));
            quota::warmup_model_directly(&token, &model, &pid, &account.email, pct).await;
        }
    }
}
