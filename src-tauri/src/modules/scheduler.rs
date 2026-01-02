use chrono::{Local, Timelike};
use std::sync::Mutex;
use tokio::time::{self, Duration};

use crate::modules::{config, logger, quota};

// 防止同一分钟内重复触发
static LAST_TRIGGER_TIME: Mutex<Option<String>> = Mutex::new(None);

pub fn start_scheduler() {
    tauri::async_runtime::spawn(async {
        logger::log_info("Smart Scheduler started. Checking for peak usage periods...");
        let mut interval = time::interval(Duration::from_secs(60)); // Check every minute

        loop {
            interval.tick().await;

            let now = Local::now();
            let current_weekday = now.format("%a").to_string().to_lowercase(); // mon, tue, ...
            let current_hm = now.format("%H:%M").to_string();
            let current_minutes = now.hour() * 60 + now.minute();

            // Map chrono weekday short to our config keys if needed,
            // but %a returns Mon, Tue... we need mon, tue.
            // Config keys are: mon, tue, wed, thu, fri, sat, sun.

            // Calculate Next Day
            let next_day = now.checked_add_signed(chrono::Duration::days(1)).unwrap();
            let next_weekday = next_day.format("%a").to_string().to_lowercase();

            if let Ok(app_config) = config::load_app_config() {
                if app_config.scheduled_warmup.enabled {
                    let mut should_trigger = false;

                    // Helper to check schedules
                    let check_schedule = |day_key: &str, is_tomorrow: bool| -> bool {
                        if let Some(ranges) = app_config.scheduled_warmup.schedules.get(day_key) {
                            for range in ranges {
                                if let (Ok(start), Ok(end)) =
                                    (parse_time_str(&range.start), parse_time_str(&range.end))
                                {
                                    let mid = (start + end) / 2;
                                    let mut trigger_min = mid - 300; // -5 hours

                                    // Logic:
                                    // If is_tomorrow=false (Today):
                                    //   We are looking for trigger times that fall TODAY (0..1440).
                                    //   trigger_min is relative to today 00:00.
                                    //   If trigger_min < 0, it fell on Yesterday (ignore).
                                    //   If trigger_min >= 0, check if == current_minutes.

                                    // If is_tomorrow=true (Tomorrow):
                                    //   We are looking for trigger times that wrap back to TODAY.
                                    //   trigger_min is relative to tomorrow 00:00.
                                    //   If trigger_min < 0, it means it is (1440 + trigger_min) minutes from TODAY 00:00.
                                    //   Check if (1440 + trigger_min) == current_minutes.

                                    if !is_tomorrow {
                                        if trigger_min >= 0 && trigger_min as u32 == current_minutes
                                        {
                                            return true;
                                        }
                                    } else {
                                        // Tomorrow's peak triggering today
                                        if trigger_min < 0 {
                                            let trigger_today_min = 1440 + trigger_min;
                                            if trigger_today_min as u32 == current_minutes {
                                                return true;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        false
                    };

                    // Check Today's Peaks
                    if check_schedule(&current_weekday, false) {
                        should_trigger = true;
                    }
                    // Check Tomorrow's Peaks (causing trigger today)
                    if check_schedule(&next_weekday, true) {
                        should_trigger = true;
                    }

                    if should_trigger {
                        let should_run = {
                            let mut last_trigger = LAST_TRIGGER_TIME.lock().unwrap();
                            if *last_trigger != Some(current_hm.clone()) {
                                *last_trigger = Some(current_hm.clone());
                                true
                            } else {
                                false
                            }
                        };

                        if should_run {
                            // Trigger!
                            logger::log_info(&format!(
                                "[Scheduler] Smart Peak Warm-up Triggered at {}!",
                                current_hm
                            ));

                            match quota::warm_up_all_accounts().await {
                                Ok(msg) => logger::log_info(&format!(
                                    "[Scheduler] Warm-up success: {}",
                                    msg
                                )),
                                Err(e) => {
                                    logger::log_error(&format!("[Scheduler] Warm-up failed: {}", e))
                                }
                            }
                        }
                    }
                }
            }
        }
    });
}

fn parse_time_str(s: &str) -> Result<i32, ()> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 2 {
        return Err(());
    }
    let h: i32 = parts[0].parse().map_err(|_| ())?;
    let m: i32 = parts[1].parse().map_err(|_| ())?;
    Ok(h * 60 + m)
}
