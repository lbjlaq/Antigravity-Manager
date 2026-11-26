mod tray;

use am_core::config::{AppConfig, UiStateConfig};
use am_core::stats::MasterAnalyticsReport;
use am_core::storage::backup::ImportReport;
use am_core::types::{Account, ActiveHostStatus, QuotaSummary, TargetHost};
use am_core::{AntigravityManager, SystemInfo};
use std::sync::Arc;
use tauri::{Emitter, Manager};
use tauri_plugin_updater::UpdaterExt;

struct DaemonWakeup(Arc<tokio::sync::Notify>);

#[tauri::command]
async fn get_stats_report(
    state: tauri::State<'_, AntigravityManager>,
    start_ts_ms: Option<i64>,
    end_ts_ms: Option<i64>,
) -> Result<MasterAnalyticsReport, String> {
    let manager = state.inner().clone();
    tokio::task::spawn_blocking(move || {
        manager.get_local_usage_stats_in_range(start_ts_ms, end_ts_ms)
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_active_hosts(
    state: tauri::State<'_, AntigravityManager>,
) -> Result<ActiveHostStatus, String> {
    let manager = state.inner().clone();
    tokio::task::spawn_blocking(move || manager.get_active_hosts())
        .await
        .map_err(|e| format!("Task join error: {}", e))
}

#[tauri::command]
async fn get_accounts(state: tauri::State<'_, AntigravityManager>) -> Result<Vec<Account>, String> {
    let manager = state.inner().clone();
    tokio::task::spawn_blocking(move || manager.list_full_accounts())
        .await
        .map_err(|e| format!("Task join error: {}", e))?
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn switch_account(
    app: tauri::AppHandle,
    state: tauri::State<'_, AntigravityManager>,
    account: String,
    target: String,
    restart: Option<bool>,
) -> Result<Account, String> {
    let host: TargetHost = target.parse().map_err(|e: String| e)?;
    let should_restart = restart.unwrap_or(false);
    let switched = state
        .switch_account_with_refresh(&account, host, should_restart)
        .await
        .map_err(|e| e.to_string())?;
    let _ = app.emit("tray://accounts-changed", ());
    Ok(switched)
}

#[tauri::command]
async fn smart_pick_account(
    app: tauri::AppHandle,
    state: tauri::State<'_, AntigravityManager>,
    target: String,
    strategy: Option<String>,
    prefer: Option<String>,
    tag: Option<String>,
) -> Result<Option<Account>, String> {
    let host: TargetHost = target.parse().map_err(|e: String| e)?;
    let res = state
        .smart_switch_with_refresh(host, strategy.as_deref(), prefer.as_deref(), tag, false)
        .await
        .map_err(|e| e.to_string())?;
    if res.is_some() {
        let _ = app.emit("tray://accounts-changed", ());
    }
    Ok(res)
}

#[tauri::command]
async fn refresh_account_quota(
    state: tauri::State<'_, AntigravityManager>,
    account_id: String,
) -> Result<QuotaSummary, String> {
    state
        .refresh_account_quota(&account_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn refresh_all_quotas(
    state: tauri::State<'_, AntigravityManager>,
) -> Result<Vec<(String, bool)>, String> {
    let raw = state
        .refresh_all_quotas()
        .await
        .map_err(|e| e.to_string())?;
    Ok(raw
        .into_iter()
        .map(|(email, res)| (email, res.is_ok()))
        .collect())
}

#[tauri::command]
fn reorder_accounts(
    app: tauri::AppHandle,
    state: tauri::State<'_, AntigravityManager>,
    account_ids: Vec<String>,
) -> Result<(), String> {
    state
        .reorder_accounts(&account_ids)
        .map_err(|e| e.to_string())?;
    let _ = app.emit("tray://accounts-changed", ());
    Ok(())
}

#[tauri::command]
fn delete_account(
    state: tauri::State<'_, AntigravityManager>,
    account_id: String,
) -> Result<bool, String> {
    state.delete_account(&account_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_accounts(
    state: tauri::State<'_, AntigravityManager>,
    account_ids: Vec<String>,
) -> Result<usize, String> {
    let mut count = 0;
    for id in account_ids {
        if state.delete_account(&id).unwrap_or(false) {
            count += 1;
        }
    }
    Ok(count)
}

#[tauri::command]
fn set_account_tags(
    state: tauri::State<'_, AntigravityManager>,
    account_id: String,
    tags: Vec<String>,
) -> Result<Account, String> {
    state
        .set_account_tags(&account_id, &tags)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn add_account_by_token(
    state: tauri::State<'_, AntigravityManager>,
    refresh_token: String,
    tags: Vec<String>,
) -> Result<Account, String> {
    state
        .add_account_by_token(&refresh_token, tags)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn start_oauth_login(
    state_mgr: tauri::State<'_, AntigravityManager>,
) -> Result<Account, String> {
    state_mgr
        .login_via_oauth_loopback(|auth_url| {
            let _ = open::that(auth_url);
        })
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn regenerate_device_profile(
    state: tauri::State<'_, AntigravityManager>,
    account_id: String,
) -> Result<Account, String> {
    state
        .regenerate_device_profile(&account_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn clear_account_forbidden(
    state: tauri::State<'_, AntigravityManager>,
    account_id: String,
) -> Result<Account, String> {
    state
        .clear_account_forbidden(&account_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn disable_account(
    state: tauri::State<'_, AntigravityManager>,
    account_id: String,
    reason: Option<String>,
) -> Result<Account, String> {
    state
        .disable_account(&account_id, reason.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn enable_account(
    state: tauri::State<'_, AntigravityManager>,
    account_id: String,
) -> Result<Account, String> {
    state.enable_account(&account_id).map_err(|e| e.to_string())
}

#[tauri::command]
async fn export_accounts_backup(
    state: tauri::State<'_, AntigravityManager>,
) -> Result<String, String> {
    let manager = state.inner().clone();
    tokio::task::spawn_blocking(move || manager.export_accounts_json())
        .await
        .map_err(|e| format!("Task join error: {}", e))?
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn import_accounts_backup(
    state: tauri::State<'_, AntigravityManager>,
    backup_json: String,
    overwrite: bool,
) -> Result<ImportReport, String> {
    let manager = state.inner().clone();
    tokio::task::spawn_blocking(move || manager.import_accounts_json(&backup_json, overwrite))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_app_config() -> Result<AppConfig, String> {
    Ok(AppConfig::load())
}

#[tauri::command]
fn save_app_config(
    app: tauri::AppHandle,
    wakeup: tauri::State<'_, DaemonWakeup>,
    config: AppConfig,
) -> Result<(), String> {
    let saved = AppConfig::update(|existing| {
        let preserved_ui = if config.ui_state.is_empty() && !existing.ui_state.is_empty() {
            existing.ui_state.clone()
        } else {
            config.ui_state.clone()
        };
        *existing = config;
        existing.ui_state = preserved_ui;
    })
    .map_err(|e| e.to_string())?;
    tray::sync_tray_config(&app, &saved);
    wakeup.0.notify_one();
    let _ = app.emit("tray://ui-state-changed", ());
    Ok(())
}

#[tauri::command]
fn patch_ui_state(app: tauri::AppHandle, patch: UiStateConfig) -> Result<UiStateConfig, String> {
    let cfg = AppConfig::update(|existing| {
        existing.ui_state.merge_from(patch);
    })
    .map_err(|e| e.to_string())?;
    let _ = app.emit("tray://ui-state-changed", ());
    Ok(cfg.ui_state)
}

#[tauri::command]
fn show_main_window(app: tauri::AppHandle, tab: Option<String>) -> Result<(), String> {
    tray::show_main_window(&app);
    if let Some(target_tab) = tab {
        if !target_tab.is_empty() {
            let _ = app.emit("tray://navigate-tab", target_tab);
        }
    }
    Ok(())
}

#[tauri::command]
fn hide_mini_popover(app: tauri::AppHandle) -> Result<(), String> {
    tray::hide_mini_window(&app);
    Ok(())
}

#[tauri::command]
fn set_mini_pinned(app: tauri::AppHandle, pinned: bool) -> Result<bool, String> {
    tray::set_mini_pinned_state(&app, pinned);
    Ok(tray::is_mini_pinned())
}

#[tauri::command]
fn get_mini_pinned() -> Result<bool, String> {
    Ok(tray::is_mini_pinned())
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    tray::mark_app_exiting();
    app.exit(0);
}

#[tauri::command]
fn toggle_auto_launch(app: tauri::AppHandle, enable: bool) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    let autolaunch = app.autolaunch();
    if enable {
        autolaunch
            .enable()
            .map_err(|e| format!("Failed to enable auto-launch: {}", e))?;
    } else if let Err(e) = autolaunch.disable() {
        let msg = e.to_string();
        if !msg.contains("os error 2")
            && !msg.contains("No such file")
            && !msg.contains("找不到指定的文件")
        {
            return Err(format!("Failed to disable auto-launch: {}", e));
        }
    }
    let _ = AppConfig::update(|cfg| {
        cfg.tray.auto_launch = enable;
    });
    Ok(enable)
}

#[tauri::command]
fn is_auto_launch_enabled(app: tauri::AppHandle) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

#[tauri::command]
fn sync_ui_preferences(
    app: tauri::AppHandle,
    lang: Option<String>,
    theme: Option<String>,
) -> Result<(), String> {
    if let Some(ref l) = lang {
        let _ = AppConfig::update(|cfg| {
            if cfg.tray.language != *l {
                cfg.tray.language = l.clone();
            }
        });
        tray::rebuild_tray_menu(&app, l);
    }
    let _ = app.emit(
        "tray://prefs-synced",
        serde_json::json!({
            "lang": lang,
            "theme": theme,
        }),
    );
    Ok(())
}

#[tauri::command]
async fn get_system_info(
    app: tauri::AppHandle,
    state: tauri::State<'_, AntigravityManager>,
) -> Result<SystemInfo, String> {
    let manager = state.inner().clone();
    let version = app.package_info().version.to_string();
    tokio::task::spawn_blocking(move || manager.get_system_info(&version))
        .await
        .map_err(|e| format!("Task join error: {}", e))
}

#[tauri::command]
fn get_app_version(app: tauri::AppHandle) -> Result<String, String> {
    Ok(app.package_info().version.to_string())
}

#[tauri::command]
fn open_external_url(url: String) -> Result<(), String> {
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("Only http:// and https:// URLs are allowed".to_string());
    }
    open::that(&url).map_err(|e| format!("Failed to open browser URL: {}", e))
}

#[tauri::command]
fn prune_logs(_days: Option<u32>) -> Result<usize, String> {
    // 纯本地只读与安全原则：保留核心摘要，清理临时导出缓存
    Ok(0)
}

#[tauri::command]
async fn check_for_updates(app: tauri::AppHandle) -> Result<am_core::UpdateInfo, String> {
    let current = app.package_info().version.to_string();
    am_core::check_for_updates(&current)
        .await
        .map_err(|e| e.to_string())
}

/// 应用内一键下载、验签、安装并重启更新（基于 tauri-plugin-updater + tauri-plugin-process）
#[tauri::command]
async fn install_update(app: tauri::AppHandle) -> Result<(), String> {
    let updater = app
        .updater()
        .map_err(|e| format!("Updater unavailable: {}", e))?;
    let update = updater
        .check()
        .await
        .map_err(|e| format!("Failed to fetch update manifest: {}", e))?
        .ok_or_else(|| "Already up to date".to_string())?;

    let app_for_progress = app.clone();
    let downloaded_acc = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let downloaded_clone = downloaded_acc.clone();

    update
        .download_and_install(
            move |chunk_len, content_len| {
                let prev = downloaded_clone
                    .fetch_add(chunk_len as u64, std::sync::atomic::Ordering::Relaxed);
                let downloaded = prev + chunk_len as u64;
                let pct = content_len
                    .filter(|&total| total > 0)
                    .map(|total| {
                        ((downloaded as f64 / total as f64) * 100.0)
                            .min(100.0)
                            .round() as u32
                    })
                    .unwrap_or(0);
                let _ = app_for_progress.emit(
                    "updater://progress",
                    serde_json::json!({
                        "downloaded": downloaded,
                        "total": content_len.unwrap_or(0),
                        "pct": pct,
                    }),
                );
            },
            || {},
        )
        .await
        .map_err(|e| format!("Update install failed: {}", e))?;

    // 必须先放行 ExitRequested 托盘常驻拦截，再触发重启
    tray::mark_app_exiting();
    app.restart();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 紧凑 Tokio 运行时：限制工作线程为 2、阻塞池上限 4 且空闲 15s 自动回收，
    // 显著降低多核 Windows / Linux / macOS 下的线程栈与 allocator arena 内存占用
    let compact_rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(4)
        .thread_keep_alive(std::time::Duration::from_secs(15))
        .enable_all()
        .build()
        .expect("failed to build compact tokio runtime");
    tauri::async_runtime::set(compact_rt.handle().clone());

    let manager = AntigravityManager::new().expect("failed to initialize AntigravityManager");
    let daemon = manager.create_daemon_service();
    let daemon_wakeup = daemon.wakeup_handle();
    let daemon_cell = std::sync::Mutex::new(Some(daemon));

    let app = tauri::Builder::default()
        .manage(manager)
        .manage(DaemonWakeup(daemon_wakeup))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(move |app| {
            tray::apply_macos_dock_icon();
            if let Some(icon) = app.default_window_icon().cloned() {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.set_icon(icon);
                }
            }
            let _ = tray::init_tray(app.handle());
            tray::apply_startup_launch_policy(app.handle());
            if let Some(daemon) = daemon_cell.lock().ok().and_then(|mut g| g.take()) {
                tauri::async_runtime::spawn(async move {
                    daemon.run_loop().await;
                });
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            let label = window.label();
            match event {
                tauri::WindowEvent::Focused(false) if label == "mini" => {
                    tray::record_mini_blur();
                    if !tray::is_mini_pinned() {
                        tray::hide_mini_window(window.app_handle());
                    }
                }
                tauri::WindowEvent::CloseRequested { api, .. } if label == "main" => {
                    if !tray::is_app_exiting() {
                        let cfg = AppConfig::load();
                        if tray::is_tray_available() && cfg.tray.enabled && cfg.tray.close_to_tray {
                            api.prevent_close();
                            if cfg.tray.lightweight_mode {
                                tray::enter_lightweight_mode(window.app_handle());
                            } else {
                                let _ = window.hide();
                                #[cfg(target_os = "macos")]
                                {
                                    let _ = window
                                        .app_handle()
                                        .set_activation_policy(tauri::ActivationPolicy::Accessory);
                                }
                                tray::schedule_main_idle_reclaim(window.app_handle());
                            }
                        }
                    }
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_stats_report,
            get_active_hosts,
            get_accounts,
            switch_account,
            smart_pick_account,
            refresh_account_quota,
            refresh_all_quotas,
            reorder_accounts,
            delete_account,
            delete_accounts,
            set_account_tags,
            add_account_by_token,
            start_oauth_login,
            regenerate_device_profile,
            clear_account_forbidden,
            disable_account,
            enable_account,
            export_accounts_backup,
            import_accounts_backup,
            get_app_config,
            save_app_config,
            patch_ui_state,
            show_main_window,
            hide_mini_popover,
            set_mini_pinned,
            get_mini_pinned,
            quit_app,
            toggle_auto_launch,
            is_auto_launch_enabled,
            sync_ui_preferences,
            get_system_info,
            get_app_version,
            open_external_url,
            prune_logs,
            check_for_updates,
            install_update,
        ])
        .build(tauri::generate_context!())
        .expect("error while building antigravity manager application");

    app.run(|app_handle, event| match event {
        tauri::RunEvent::Ready => {
            if tray::take_lightweight_on_ready() {
                tray::enter_lightweight_mode(app_handle);
            }
        }
        // 当轻量模式销毁全部前端 WebView 窗口时阻止主进程自动退出，保持后台托盘常驻
        tauri::RunEvent::ExitRequested { api, .. } => {
            if !tray::is_app_exiting() && tray::is_tray_available() {
                api.prevent_exit();
            }
        }
        #[cfg(target_os = "macos")]
        tauri::RunEvent::Reopen {
            has_visible_windows,
            ..
        } => {
            if !has_visible_windows {
                tray::show_main_window(app_handle);
            }
        }
        _ => {}
    });
}
