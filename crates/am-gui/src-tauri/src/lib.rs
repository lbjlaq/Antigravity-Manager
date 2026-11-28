mod tray;
mod updater;

use am_core::config::{AppConfig, UiStateConfig};
use am_core::stats::MasterAnalyticsReport;
use am_core::storage::backup::ImportReport;
use am_core::types::{Account, ActiveHostStatus, QuotaSummary, TargetHost};
use am_core::{AntigravityManager, SystemInfo};
use std::sync::Arc;
use tauri::{Emitter, Manager};
use updater::{UpdateController, UpdateSnapshot};

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

static AUTO_NO_TUN_ON_SWITCH: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static PREFERRED_PROXY_URL: std::sync::OnceLock<std::sync::Mutex<Option<String>>> =
    std::sync::OnceLock::new();

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
    let auto_no_tun =
        should_restart && AUTO_NO_TUN_ON_SWITCH.load(std::sync::atomic::Ordering::Relaxed);

    let switched = state
        .switch_account_with_refresh(&account, host, should_restart && !auto_no_tun)
        .await
        .map_err(|e| e.to_string())?;

    if auto_no_tun {
        let preferred = PREFERRED_PROXY_URL
            .get_or_init(|| std::sync::Mutex::new(None))
            .lock()
            .ok()
            .and_then(|g| g.clone());
        let proxy_url = match preferred.filter(|s| !s.trim().is_empty()) {
            Some(u) => Some(u),
            None => am_core::am_proxy::detect_local_proxies()
                .await
                .into_iter()
                .next()
                .map(|n| n.to_url()),
        };
        if let Some(url) = proxy_url {
            let target_app = match host {
                TargetHost::Ide => am_core::am_proxy::TargetApp::AntigravityIde,
                _ => am_core::am_proxy::TargetApp::AntigravityApp,
            };
            let _ = am_core::am_proxy::launch_with_proxy(target_app, &url, &[]);
        }
    }

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
        let preserved_last_check = if config.update.last_check_time == 0 {
            existing.update.last_check_time
        } else {
            config.update.last_check_time
        };
        let preserved_skipped = config
            .update
            .skipped_version
            .clone()
            .or_else(|| existing.update.skipped_version.clone());

        *existing = config;
        existing.ui_state = preserved_ui;
        existing.update.last_check_time = preserved_last_check;
        existing.update.skipped_version = preserved_skipped;
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
fn get_update_state(controller: tauri::State<'_, UpdateController>) -> UpdateSnapshot {
    controller.snapshot()
}

#[tauri::command]
async fn check_for_updates(
    app: tauri::AppHandle,
    controller: tauri::State<'_, UpdateController>,
) -> Result<am_core::UpdateInfo, String> {
    controller.check_manual(&app).await
}

#[tauri::command]
async fn download_update(
    app: tauri::AppHandle,
    controller: tauri::State<'_, UpdateController>,
    force_native: Option<bool>,
) -> Result<(), String> {
    controller
        .download_update(&app, force_native.unwrap_or(false))
        .await
}

#[tauri::command]
async fn apply_and_restart_update(
    app: tauri::AppHandle,
    controller: tauri::State<'_, UpdateController>,
) -> Result<(), String> {
    controller.apply_and_restart(&app).await
}

#[tauri::command]
fn dismiss_update_banner(
    app: tauri::AppHandle,
    controller: tauri::State<'_, UpdateController>,
) -> UpdateSnapshot {
    controller.dismiss_banner(&app)
}

#[tauri::command]
fn skip_update_version(
    app: tauri::AppHandle,
    controller: tauri::State<'_, UpdateController>,
    version: String,
) -> Result<UpdateSnapshot, String> {
    controller.skip_version(&app, &version)
}

#[derive(serde::Serialize)]
struct ProxyStatusPayload {
    running: bool,
    local_addr: Option<String>,
    upstream: Option<am_core::am_proxy::ProxyNode>,
}

struct ProxyState(tokio::sync::Mutex<Option<am_core::am_proxy::RelayServer>>);

#[tauri::command]
async fn proxy_detect_nodes() -> Vec<am_core::am_proxy::ProxyNode> {
    am_core::am_proxy::detect_local_proxies().await
}

#[tauri::command]
async fn proxy_get_status(
    state: tauri::State<'_, ProxyState>,
) -> Result<ProxyStatusPayload, String> {
    let guard = state.0.lock().await;
    if let Some(relay) = guard.as_ref() {
        Ok(ProxyStatusPayload {
            running: true,
            local_addr: Some(relay.local_http_url()),
            upstream: relay.current_upstream().await,
        })
    } else {
        Ok(ProxyStatusPayload {
            running: false,
            local_addr: None,
            upstream: None,
        })
    }
}

#[tauri::command]
async fn proxy_start_or_switch(
    state: tauri::State<'_, ProxyState>,
    node: Option<am_core::am_proxy::ProxyNode>,
    port: Option<u16>,
) -> Result<ProxyStatusPayload, String> {
    let mut guard = state.0.lock().await;
    if let Some(relay) = guard.as_ref() {
        relay.switch_upstream(node).await;
        return Ok(ProxyStatusPayload {
            running: true,
            local_addr: Some(relay.local_http_url()),
            upstream: relay.current_upstream().await,
        });
    }
    let bind_port = port.unwrap_or(am_core::am_proxy::DEFAULT_RELAY_PORT);
    let bind_addr = format!("127.0.0.1:{}", bind_port);
    let relay = am_core::am_proxy::RelayServer::start(&bind_addr, node)
        .await
        .map_err(|e| e.to_string())?;
    let payload = ProxyStatusPayload {
        running: true,
        local_addr: Some(relay.local_http_url()),
        upstream: relay.current_upstream().await,
    };
    *guard = Some(relay);
    Ok(payload)
}

#[tauri::command]
async fn proxy_stop(state: tauri::State<'_, ProxyState>) -> Result<ProxyStatusPayload, String> {
    let mut guard = state.0.lock().await;
    if let Some(relay) = guard.take() {
        relay.shutdown();
    }
    Ok(ProxyStatusPayload {
        running: false,
        local_addr: None,
        upstream: None,
    })
}

#[tauri::command]
async fn proxy_launch_app(
    state: tauri::State<'_, ProxyState>,
    target: String,
    proxy_url: Option<String>,
) -> Result<u32, String> {
    let resolved_url = match proxy_url.filter(|s| !s.trim().is_empty()) {
        Some(u) => u,
        None => {
            let guard = state.0.lock().await;
            guard
                .as_ref()
                .map(|r| r.local_http_url())
                .ok_or_else(|| "请先选择一个本机代理出口".to_string())?
        }
    };
    let target_app = match target.as_str() {
        "app" => am_core::am_proxy::TargetApp::AntigravityApp,
        "ide" => am_core::am_proxy::TargetApp::AntigravityIde,
        "cli" | "agy" => am_core::am_proxy::TargetApp::AgyCli,
        other => return Err(format!("未知目标应用: {}", other)),
    };
    am_core::am_proxy::launch_with_proxy(target_app, &resolved_url, &[]).map_err(|e| e.to_string())
}

#[tauri::command]
async fn proxy_install_shell_hook(
    state: tauri::State<'_, ProxyState>,
    enable: bool,
    proxy_url: Option<String>,
) -> Result<String, String> {
    if !enable {
        am_core::am_proxy::uninstall_shell_hook().map_err(|e| e.to_string())?;
        return Ok("已移除终端 agy 代理 Hook".into());
    }
    let resolved_url = match proxy_url.filter(|s| !s.trim().is_empty()) {
        Some(u) => u,
        None => {
            let guard = state.0.lock().await;
            guard
                .as_ref()
                .map(|r| r.local_http_url())
                .ok_or_else(|| "请先选择一个本机代理出口".to_string())?
        }
    };
    let path = am_core::am_proxy::install_shell_hook(&resolved_url).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

#[tauri::command]
async fn proxy_create_macos_wrapper(
    state: tauri::State<'_, ProxyState>,
    target: String,
    proxy_url: Option<String>,
) -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        let resolved_url = match proxy_url.filter(|s| !s.trim().is_empty()) {
            Some(u) => u,
            None => {
                let guard = state.0.lock().await;
                guard
                    .as_ref()
                    .map(|r| r.local_http_url())
                    .ok_or_else(|| "请先选择一个本机代理出口".to_string())?
            }
        };
        let target_app = match target.as_str() {
            "app" => am_core::am_proxy::TargetApp::AntigravityApp,
            "ide" => am_core::am_proxy::TargetApp::AntigravityIde,
            other => return Err(format!("不支持的目标: {}", other)),
        };
        let desktop = std::env::var_os("HOME")
            .map(|h| std::path::PathBuf::from(h).join("Desktop"))
            .ok_or_else(|| "无法定位桌面目录".to_string())?;
        let path = am_core::am_proxy::platform::create_macos_wrapper_app(
            target_app,
            &resolved_url,
            &desktop,
        )
        .map_err(|e| e.to_string())?;
        Ok(path.display().to_string())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (state, target, proxy_url);
        Err("桌面 Wrapper .app 仅适用于 macOS".into())
    }
}

#[derive(serde::Serialize)]
struct ProxyIntegrationsPayload {
    app_wrapper: bool,
    ide_wrapper: bool,
    shell_hook: bool,
    ide_settings_proxy: Option<String>,
    auto_no_tun_on_switch: bool,
}

#[tauri::command]
async fn proxy_get_integrations() -> ProxyIntegrationsPayload {
    let shell_hook = am_core::am_proxy::platform::is_shell_hook_installed();
    let ide_settings_proxy = am_core::am_proxy::get_ide_settings_proxy();
    #[cfg(target_os = "macos")]
    let (app_wrapper, ide_wrapper) = {
        let desktop = std::env::var_os("HOME")
            .map(|h| std::path::PathBuf::from(h).join("Desktop"))
            .unwrap_or_default();
        (
            desktop.join("Antigravity (No-TUN).app").exists(),
            desktop.join("Antigravity IDE (No-TUN).app").exists(),
        )
    };
    #[cfg(not(target_os = "macos"))]
    let (app_wrapper, ide_wrapper) = (false, false);

    ProxyIntegrationsPayload {
        app_wrapper,
        ide_wrapper,
        shell_hook,
        ide_settings_proxy,
        auto_no_tun_on_switch: AUTO_NO_TUN_ON_SWITCH.load(std::sync::atomic::Ordering::Relaxed),
    }
}

#[tauri::command]
async fn proxy_set_auto_no_tun(enabled: bool, proxy_url: Option<String>) {
    AUTO_NO_TUN_ON_SWITCH.store(enabled, std::sync::atomic::Ordering::Relaxed);
    if let Ok(mut guard) = PREFERRED_PROXY_URL
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
    {
        *guard = proxy_url.filter(|s| !s.trim().is_empty());
    }
}

#[tauri::command]
async fn proxy_sync_ide_settings(proxy_url: Option<String>) -> Result<(), String> {
    am_core::am_proxy::sync_ide_proxy_settings(proxy_url.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
async fn proxy_runtime_status(
) -> std::collections::HashMap<String, am_core::am_proxy::AppRuntimeStatus> {
    let mut map = std::collections::HashMap::new();
    map.insert(
        "antigravity".to_string(),
        am_core::am_proxy::get_app_runtime_status(am_core::am_proxy::TargetApp::AntigravityApp),
    );
    map.insert(
        "ide".to_string(),
        am_core::am_proxy::get_app_runtime_status(am_core::am_proxy::TargetApp::AntigravityIde),
    );
    map
}

#[derive(serde::Serialize)]
struct ContextPluginStatus {
    ide_installed: bool,
    mcp_installed: bool,
    cli_installed: bool,
}

fn context_plugin_dirs() -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(std::path::PathBuf::from)?;
    let ide_dir = home
        .join(".antigravity-ide")
        .join("extensions")
        .join("am-monitor");
    let mcp_dir = home
        .join(".gemini")
        .join("config")
        .join("plugins")
        .join("am-monitor");
    Some((ide_dir, mcp_dir))
}

#[tauri::command]
fn context_plugin_status() -> ContextPluginStatus {
    let cli_installed = am_core::cli_statusline::is_cli_statusline_installed();
    if let Some((ide_dir, mcp_dir)) = context_plugin_dirs() {
        ContextPluginStatus {
            ide_installed: ide_dir.join("package.json").exists(),
            mcp_installed: mcp_dir.join("plugin.json").exists()
                && mcp_dir
                    .join("sidecars")
                    .join("panel")
                    .join("sidecar.json")
                    .exists(),
            cli_installed,
        }
    } else {
        ContextPluginStatus {
            ide_installed: false,
            mcp_installed: false,
            cli_installed,
        }
    }
}

#[tauri::command]
fn context_plugin_install(target: String) -> Result<ContextPluginStatus, String> {
    let (ide_dir, mcp_dir) =
        context_plugin_dirs().ok_or_else(|| "Cannot resolve home directory".to_string())?;
    match target.as_str() {
        "ide" => {
            if let Some(parent) = ide_dir.parent() {
                let legacy = parent.join("antigravity-context-ide");
                if legacy.exists() {
                    let _ = std::fs::remove_dir_all(&legacy);
                }
            }
            std::fs::create_dir_all(ide_dir.join("assets")).map_err(|e| e.to_string())?;
            std::fs::write(
                ide_dir.join("assets").join("logo.png"),
                include_bytes!("../../../../plugins/am-monitor/assets/logo.png"),
            )
            .map_err(|e| e.to_string())?;
            std::fs::write(
                ide_dir.join("package.json"),
                include_str!("../../../../plugins/am-monitor/ide/package.json"),
            )
            .map_err(|e| e.to_string())?;
            std::fs::write(
                ide_dir.join("extension.js"),
                include_str!("../../../../plugins/am-monitor/ide/extension.js"),
            )
            .map_err(|e| e.to_string())?;
            std::fs::write(
                ide_dir.join("telemetry.mjs"),
                include_str!("../../../../plugins/am-monitor/telemetry.mjs"),
            )
            .map_err(|e| e.to_string())?;
            std::fs::write(
                ide_dir.join("ui.html"),
                include_str!("../../../../plugins/am-monitor/ui.html"),
            )
            .map_err(|e| e.to_string())?;
        }
        "mcp" => {
            let panel_dir = mcp_dir.join("sidecars").join("panel");
            std::fs::create_dir_all(&panel_dir).map_err(|e| e.to_string())?;
            std::fs::create_dir_all(mcp_dir.join("assets")).map_err(|e| e.to_string())?;
            std::fs::write(
                mcp_dir.join("assets").join("logo.png"),
                include_bytes!("../../../../plugins/am-monitor/assets/logo.png"),
            )
            .map_err(|e| e.to_string())?;
            std::fs::write(
                mcp_dir.join("plugin.json"),
                include_str!("../../../../plugins/am-monitor/plugin.json"),
            )
            .map_err(|e| e.to_string())?;
            std::fs::write(
                mcp_dir.join("telemetry.mjs"),
                include_str!("../../../../plugins/am-monitor/telemetry.mjs"),
            )
            .map_err(|e| e.to_string())?;
            std::fs::write(
                mcp_dir.join("ui.html"),
                include_str!("../../../../plugins/am-monitor/ui.html"),
            )
            .map_err(|e| e.to_string())?;
            std::fs::write(
                panel_dir.join("sidecar.json"),
                include_str!("../../../../plugins/am-monitor/sidecars/panel/sidecar.json"),
            )
            .map_err(|e| e.to_string())?;
            std::fs::write(
                panel_dir.join("main.mjs"),
                include_str!("../../../../plugins/am-monitor/sidecars/panel/main.mjs"),
            )
            .map_err(|e| e.to_string())?;
        }
        "cli" => {
            am_core::cli_statusline::install_cli_statusline(None).map_err(|e| e.to_string())?;
        }
        _ => return Err(format!("Unknown plugin target: {}", target)),
    }
    Ok(context_plugin_status())
}

#[tauri::command]
fn context_plugin_uninstall(target: String) -> Result<ContextPluginStatus, String> {
    if target == "cli" {
        am_core::cli_statusline::uninstall_cli_statusline().map_err(|e| e.to_string())?;
        return Ok(context_plugin_status());
    }
    let (ide_dir, mcp_dir) =
        context_plugin_dirs().ok_or_else(|| "Cannot resolve home directory".to_string())?;
    let dir = match target.as_str() {
        "ide" => ide_dir,
        "mcp" => mcp_dir,
        _ => return Err(format!("Unknown plugin target: {}", target)),
    };
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    Ok(context_plugin_status())
}

#[tauri::command]
async fn proxy_remove_macos_wrapper(target: String) -> Result<bool, String> {
    #[cfg(target_os = "macos")]
    {
        let target_app = match target.as_str() {
            "app" => am_core::am_proxy::TargetApp::AntigravityApp,
            "ide" => am_core::am_proxy::TargetApp::AntigravityIde,
            other => return Err(format!("不支持的目标: {}", other)),
        };
        let desktop = std::env::var_os("HOME")
            .map(|h| std::path::PathBuf::from(h).join("Desktop"))
            .ok_or_else(|| "无法定位桌面目录".to_string())?;
        am_core::am_proxy::platform::remove_macos_wrapper_app(target_app, &desktop)
            .map_err(|e| e.to_string())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = target;
        Ok(false)
    }
}

/// 兼容旧命令：一次性下载、验签、安装并重启更新
#[tauri::command]
async fn install_update(
    app: tauri::AppHandle,
    controller: tauri::State<'_, UpdateController>,
    force_native: Option<bool>,
) -> Result<(), String> {
    controller
        .download_update(&app, force_native.unwrap_or(false))
        .await?;
    controller.apply_and_restart(&app).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 跨平台原子单实例锁（macOS / Windows / Linux 通用；进程异常退出时内核自动释放端口）
    let _single_instance_guard = match std::net::TcpListener::bind("127.0.0.1:49271") {
        Ok(listener) => listener,
        Err(_) => {
            eprintln!("Antigravity Manager is already running (single-instance lock active).");
            return;
        }
    };

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
        .manage(UpdateController::new())
        .manage(ProxyState(tokio::sync::Mutex::new(None)))
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
            updater::spawn_auto_check_scheduler(app.handle().clone());
            if let Some(daemon) = daemon_cell.lock().ok().and_then(|mut g| g.take()) {
                tauri::async_runtime::spawn(async move {
                    daemon.run_loop().await;
                });
            }
            tauri::async_runtime::spawn(async move {});
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
            get_update_state,
            check_for_updates,
            download_update,
            apply_and_restart_update,
            dismiss_update_banner,
            skip_update_version,
            install_update,
            proxy_detect_nodes,
            proxy_get_status,
            proxy_runtime_status,
            proxy_set_auto_no_tun,
            context_plugin_status,
            context_plugin_install,
            context_plugin_uninstall,
            proxy_start_or_switch,
            proxy_stop,
            proxy_launch_app,
            proxy_install_shell_hook,
            proxy_create_macos_wrapper,
            proxy_remove_macos_wrapper,
            proxy_get_integrations,
            proxy_sync_ide_settings,
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
