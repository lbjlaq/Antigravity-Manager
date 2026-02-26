mod models;
mod modules;
mod commands;
mod utils;
mod proxy;  // Proxy service module
pub mod error;
pub mod constants;

use tauri::Manager;
use modules::logger;
use tracing::{info, warn, error};
use std::sync::Arc;

#[derive(Clone, Copy)]
struct AppRuntimeFlags {
    tray_enabled: bool,
}

fn env_flag_enabled(name: &str) -> bool {
    std::env::var(name)
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
}

#[cfg(target_os = "linux")]
fn is_wayland_session() -> bool {
    std::env::var("WAYLAND_DISPLAY")
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
        || std::env::var("XDG_SESSION_TYPE")
            .map(|v| v.eq_ignore_ascii_case("wayland"))
            .unwrap_or(false)
}

fn should_enable_tray() -> bool {
    if env_flag_enabled("ANTIGRAVITY_DISABLE_TRAY") {
        info!("Tray disabled by ANTIGRAVITY_DISABLE_TRAY");
        return false;
    }

    #[cfg(target_os = "linux")]
    {
        if is_wayland_session() && !env_flag_enabled("ANTIGRAVITY_FORCE_TRAY") {
            warn!(
                "Linux Wayland session detected; disabling tray by default to avoid GTK/AppIndicator crashes. Set ANTIGRAVITY_FORCE_TRAY=1 to force-enable."
            );
            return false;
        }
    }

    true
}

#[cfg(target_os = "linux")]
fn configure_linux_gdk_backend() {
    if std::env::var("GDK_BACKEND").is_ok() {
        return;
    }

    let is_wayland = is_wayland_session();
    let has_x11_display = std::env::var("DISPLAY")
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false);
    let force_wayland = env_flag_enabled("ANTIGRAVITY_FORCE_WAYLAND");
    let force_x11 = env_flag_enabled("ANTIGRAVITY_FORCE_X11");

    if force_x11 || (is_wayland && has_x11_display && !force_wayland) {
        // Force X11 backend under Wayland sessions to avoid a GTK Wayland shm crash.
        std::env::set_var("GDK_BACKEND", "x11");
        warn!(
            "Forcing GDK_BACKEND=x11 for stability on Wayland. Set ANTIGRAVITY_FORCE_WAYLAND=1 to keep Wayland backend."
        );
    }
}

/// Increase file descriptor limit for macOS to prevent "Too many open files" errors
#[cfg(target_os = "macos")]
fn increase_nofile_limit() {
    unsafe {
        let mut rl = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };

        if libc::getrlimit(libc::RLIMIT_NOFILE, &mut rl) == 0 {
            info!("Current open file limit: soft={}, hard={}", rl.rlim_cur, rl.rlim_max);

            // Attempt to increase to 4096 or maximum hard limit
            let target = 4096.min(rl.rlim_max);
            if rl.rlim_cur < target {
                rl.rlim_cur = target;
                if libc::setrlimit(libc::RLIMIT_NOFILE, &rl) == 0 {
                    info!("Successfully increased hard file limit to {}", target);
                } else {
                    warn!("Failed to increase file descriptor limit");
                }
            }
        }
    }
}

// Test command
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Check for headless mode
    let args: Vec<String> = std::env::args().collect();
    let is_headless = args.iter().any(|arg| arg == "--headless");

    // Increase file descriptor limit (macOS only)
    #[cfg(target_os = "macos")]
    increase_nofile_limit();

    // Initialize logger
    logger::init_logger();

    #[cfg(target_os = "linux")]
    configure_linux_gdk_backend();

    // Initialize token stats database
    if let Err(e) = modules::token_stats::init_db() {
        error!("Failed to initialize token stats database: {}", e);
    }

    // Initialize security database
    if let Err(e) = modules::security_db::init_db() {
        error!("Failed to initialize security database: {}", e);
    }
    
    // Initialize user token database
    if let Err(e) = modules::user_token_db::init_db() {
        error!("Failed to initialize user token database: {}", e);
    }

    if is_headless {
        info!("Starting in HEADLESS mode...");

        let rt = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");
        rt.block_on(async {
            // Initialize states manually
            // [FIX] Initialize log bridge for headless mode
            // Pass a dummy app handle or None since we don't have a Tauri app handle in headless mode
            // Actually log_bridge relies on AppHandle to emit events.
            // In headless mode, we don't emit events, but we still need the buffer.
            // We need to modify log_bridge to handle missing AppHandle gracefully, which it already does (Option).
            // But init_log_bridge requires AppHandle.
            // We'll skip passing AppHandle for now and just leverage the global buffer capability.
            // Since init_log_bridge takes AppHandle, we might need a separate init for headless or just not call init and rely on lazy init of buffer?
            // Checking log_bridge code again...
            // "static LOG_BUFFER: OnceLock<...> = OnceLock::new();" -> lazy init.
            // So we just need to ensure the tracing layer is added.
            // And `logger::init_logger()` adds the layer?
            // Let's check `modules::logger`.

            let proxy_state = commands::proxy::ProxyServiceState::new();
            let cf_state = Arc::new(commands::cloudflared::CloudflaredState::new());

            // Load config
            match modules::config::load_app_config() {
                Ok(mut config) => {
                    let mut modified = false;
                    // Headless/docker 默认允许 LAN 访问（绑定 0.0.0.0）
                    // 若设置 ABV_BIND_LOCAL_ONLY，则仅绑定 127.0.0.1
                    let bind_local_only = std::env::var("ABV_BIND_LOCAL_ONLY")
                        .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes" | "on"))
                        .unwrap_or(false);
                    if bind_local_only {
                        config.proxy.allow_lan_access = false;
                        modified = true;
                    } else {
                        config.proxy.allow_lan_access = true;
                    }

                    // [FIX] Force auth mode to AllExceptHealth in headless mode if it's Off or Auto
                    // This ensures Web UI login validation works properly
                    if matches!(config.proxy.auth_mode, crate::proxy::ProxyAuthMode::Off | crate::proxy::ProxyAuthMode::Auto) {
                        info!("Headless mode: Forcing auth_mode to AllExceptHealth for Web UI security");
                        config.proxy.auth_mode = crate::proxy::ProxyAuthMode::AllExceptHealth;
                        modified = true;
                    }

                    // [NEW] 支持通过环境变量注入 API Key
                    // 优先级：ABV_API_KEY > API_KEY > 配置文件
                    let env_key = std::env::var("ABV_API_KEY")
                        .or_else(|_| std::env::var("API_KEY"))
                        .ok();

                    if let Some(key) = env_key {
                        if !key.trim().is_empty() {
                            info!("Using API Key from environment variable");
                            config.proxy.api_key = key;
                            modified = true;
                        }
                    }

                    // [NEW] 支持通过环境变量注入 Web UI 密码
                    // 优先级：ABV_WEB_PASSWORD > WEB_PASSWORD > 配置文件
                    let env_web_password = std::env::var("ABV_WEB_PASSWORD")
                        .or_else(|_| std::env::var("WEB_PASSWORD"))
                        .ok();

                    if let Some(pwd) = env_web_password {
                        if !pwd.trim().is_empty() {
                            info!("Using Web UI Password from environment variable");
                            config.proxy.admin_password = Some(pwd);
                            modified = true;
                        }
                    }

                    // [NEW] 支持通过环境变量注入鉴权模式
                    // 优先级：ABV_AUTH_MODE > AUTH_MODE > 配置文件
                    let env_auth_mode = std::env::var("ABV_AUTH_MODE")
                        .or_else(|_| std::env::var("AUTH_MODE"))
                        .ok();

                    if let Some(mode_str) = env_auth_mode {
                        let mode = match mode_str.to_lowercase().as_str() {
                            "off" => Some(crate::proxy::ProxyAuthMode::Off),
                            "strict" => Some(crate::proxy::ProxyAuthMode::Strict),
                            "all_except_health" => Some(crate::proxy::ProxyAuthMode::AllExceptHealth),
                            "auto" => Some(crate::proxy::ProxyAuthMode::Auto),
                            _ => {
                                warn!("Invalid AUTH_MODE: {}, ignoring", mode_str);
                                None
                            }
                        };
                        if let Some(m) = mode {
                            info!("Using Auth Mode from environment variable: {:?}", m);
                            config.proxy.auth_mode = m;
                            modified = true;
                        }
                    }

                    info!("--------------------------------------------------");
                    info!("🚀 Headless mode proxy service starting...");
                    info!("📍 Port: {}", config.proxy.port);
                    info!("🔑 Current API Key: {}", config.proxy.api_key);
                    if let Some(ref pwd) = config.proxy.admin_password {
                        info!("🔐 Web UI Password: {}", pwd);
                    } else {
                        info!("🔐 Web UI Password: (Same as API Key)");
                    }
                    info!("💡 Tips: You can use these keys to login to Web UI and access AI APIs.");
                    info!("💡 Search docker logs or grep gui_config.json to find them.");
                    info!("--------------------------------------------------");

                    // [FIX #1460] Persist environment overrides to ensure they are visible in Web UI/load_config
                    if modified {
                        if let Err(e) = modules::config::save_app_config(&config) {
                            error!("Failed to persist environment overrides: {}", e);
                        } else {
                            info!("Environment overrides persisted to gui_config.json");
                        }
                    }

                    // Start proxy service
                    if let Err(e) = commands::proxy::internal_start_proxy_service(
                        config.proxy,
                        &proxy_state,
                        crate::modules::integration::SystemManager::Headless,
                        cf_state.clone(),
                    ).await {
                        error!("Failed to start proxy service in headless mode: {}", e);
                        std::process::exit(1);
                    }

                    info!("Headless proxy service is running.");

                    // [DISABLED] Start smart scheduler (Automatic warmup disabled as per user request)
                    // modules::scheduler::start_scheduler(None, proxy_state.clone());
                    info!("Smart scheduler (Automatic Warmup) is DISABLED.");
                    info!("Smart scheduler started in headless mode.");
                }
                Err(e) => {
                    error!("Failed to load config for headless mode: {}", e);
                    std::process::exit(1);
                }
            }

            // Wait for Ctrl-C
            tokio::signal::ctrl_c().await.ok();
            info!("Headless mode shutting down");
        });
        return;
    }

    let tray_enabled = should_enable_tray();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            let _ = app.get_webview_window("main")
                .map(|window| {
                    let _ = window.show();
                    let _ = window.set_focus();
                    #[cfg(target_os = "macos")]
                    app.set_activation_policy(tauri::ActivationPolicy::Regular).unwrap_or(());
                });
        }))
        .manage(commands::proxy::ProxyServiceState::new())
        .manage(commands::cloudflared::CloudflaredState::new())
        .manage(AppRuntimeFlags { tray_enabled })
        .setup(|app| {
            info!("Setup starting...");

            // Initialize log bridge with app handle for debug console
            modules::log_bridge::init_log_bridge(app.handle().clone());

            // Linux: Workaround for transparent window crash/freeze
            // The transparent window feature is unstable on Linux with WebKitGTK
            // We disable the visual alpha channel to prevent softbuffer-related crashes
            #[cfg(target_os = "linux")]
            {
                use tauri::Manager;
                if is_wayland_session() {
                    info!("Linux Wayland session detected; skipping transparent window workaround");
                } else if let Some(window) = app.get_webview_window("main") {
                    // Access GTK window and disable transparency at the GTK level
                    if let Ok(gtk_window) = window.gtk_window() {
                        use gtk::prelude::WidgetExt;
                        // Remove the visual's alpha channel to disable transparency
                        if let Some(screen) = gtk_window.screen() {
                            // Use non-composited visual if available
                            if let Some(visual) = screen.system_visual() {
                                gtk_window.set_visual(Some(&visual));
                            }
                            info!("Linux: Applied transparent window workaround");
                        }
                    }
                }
            }

            let runtime_flags = app.state::<AppRuntimeFlags>();
            if runtime_flags.tray_enabled {
                modules::tray::create_tray(app.handle())?;
                info!("Tray created");
            } else {
                info!("Tray disabled for this session");
            }

            // 立即启动管理服务器 (8045)，以便 Web 端能访问
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                // Load config
                if let Ok(config) = modules::config::load_app_config() {
                    let state = handle.state::<commands::proxy::ProxyServiceState>();
                    let cf_state = handle.state::<commands::cloudflared::CloudflaredState>();
                    let integration = crate::modules::integration::SystemManager::Desktop(handle.clone());

                    // 1. 确保管理后台开启
                    if let Err(e) = commands::proxy::ensure_admin_server(
                        config.proxy.clone(),
                        &state,
                        integration.clone(),
                        Arc::new(cf_state.inner().clone()),
                    ).await {
                        error!("Failed to start admin server: {}", e);
                    } else {
                        info!("Admin server (port {}) started successfully", config.proxy.port);
                    }

                    // 2. 自动启动转发逻辑
                    if config.proxy.auto_start {
                        if let Err(e) = commands::proxy::internal_start_proxy_service(
                            config.proxy,
                            &state,
                            integration,
                            Arc::new(cf_state.inner().clone()),
                        ).await {
                            error!("Failed to auto-start proxy service: {}", e);
                        } else {
                            info!("Proxy service auto-started successfully");
                        }
                    }
                }
            });

            // [DISABLED] Start smart scheduler (Automatic warmup disabled as per user request)
            // let scheduler_state = app.handle().state::<commands::proxy::ProxyServiceState>();
            // modules::scheduler::start_scheduler(Some(app.handle().clone()), scheduler_state.inner().clone());
            info!("Smart scheduler (Automatic Warmup) is DISABLED.");

            // [PHASE 1] 已整合至 Axum 端口 (8045)，不再单独启动 19527 端口
            info!("Management API integrated into main proxy server (port 8045)");

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let tray_enabled = window
                    .app_handle()
                    .try_state::<AppRuntimeFlags>()
                    .map(|flags| flags.tray_enabled)
                    .unwrap_or(true);

                if tray_enabled {
                    let _ = window.hide();
                    #[cfg(target_os = "macos")]
                    {
                        use tauri::Manager;
                        window
                            .app_handle()
                            .set_activation_policy(tauri::ActivationPolicy::Accessory)
                            .unwrap_or(());
                    }
                    api.prevent_close();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            // Account management commands
            commands::list_accounts,
            commands::add_account,
            commands::delete_account,
            commands::delete_accounts,
            commands::reorder_accounts,
            commands::switch_account,
            commands::export_accounts,
            // Device fingerprint
            commands::get_device_profiles,
            commands::bind_device_profile,
            commands::bind_device_profile_with_profile,
            commands::preview_generate_profile,
            commands::apply_device_profile,
            commands::restore_original_device,
            commands::list_device_versions,
            commands::restore_device_version,
            commands::delete_device_version,
            commands::open_device_folder,
            commands::get_current_account,
            // Quota commands
            commands::fetch_account_quota,
            commands::refresh_all_quotas,
            // Config commands
            commands::load_config,
            commands::save_config,
            // Additional commands
            commands::prepare_oauth_url,
            commands::start_oauth_login,
            commands::complete_oauth_login,
            commands::cancel_oauth_login,
            commands::submit_oauth_code,
            commands::import_v1_accounts,
            commands::import_from_db,
            commands::import_custom_db,
            commands::sync_account_from_db,
            commands::save_text_file,
            commands::read_text_file,
            commands::clear_log_cache,
            commands::clear_antigravity_cache,
            commands::get_antigravity_cache_paths,
            commands::open_data_folder,
            commands::get_data_dir_path,
            commands::show_main_window,
            commands::set_window_theme,
            commands::get_antigravity_path,
            commands::get_antigravity_args,
            commands::check_for_updates,
            commands::check_homebrew_installation,
            commands::brew_upgrade_cask,
            commands::get_update_settings,
            commands::save_update_settings,
            commands::should_check_updates,
            commands::update_last_check_time,
            commands::toggle_proxy_status,
            // Proxy service commands
            commands::proxy::start_proxy_service,
            commands::proxy::stop_proxy_service,
            commands::proxy::get_proxy_status,
            commands::proxy::get_proxy_stats,
            commands::proxy::get_proxy_logs,
            commands::proxy::get_proxy_logs_paginated,
            commands::proxy::get_proxy_log_detail,
            commands::proxy::get_proxy_logs_count,
            commands::proxy::export_proxy_logs,
            commands::proxy::export_proxy_logs_json,
            commands::proxy::get_proxy_logs_count_filtered,
            commands::proxy::get_proxy_logs_filtered,
            commands::proxy::set_proxy_monitor_enabled,
            commands::proxy::clear_proxy_logs,
            commands::proxy::generate_api_key,
            commands::proxy::reload_proxy_accounts,
            commands::proxy::update_model_mapping,
            commands::proxy::check_proxy_health,
            commands::proxy::get_proxy_pool_config,
            commands::proxy::fetch_zai_models,
            commands::proxy::get_proxy_scheduling_config,
            commands::proxy::update_proxy_scheduling_config,
            commands::proxy::clear_proxy_session_bindings,
            commands::proxy::set_preferred_account,
            commands::proxy::get_preferred_account,
            commands::proxy::clear_proxy_rate_limit,
            commands::proxy::clear_all_proxy_rate_limits,
            commands::proxy::check_proxy_health,
            // Proxy Pool Binding commands
            commands::proxy_pool::bind_account_proxy,
            commands::proxy_pool::unbind_account_proxy,
            commands::proxy_pool::get_account_proxy_binding,
            commands::proxy_pool::get_all_account_bindings,
            // Autostart commands
            commands::autostart::toggle_auto_launch,
            commands::autostart::is_auto_launch_enabled,
            // Warmup commands
            commands::warm_up_all_accounts,
            commands::warm_up_account,
            commands::update_account_label,
            // HTTP API settings commands
            commands::get_http_api_settings,
            commands::save_http_api_settings,
            // Token 统计命令
            commands::get_token_stats_hourly,
            commands::get_token_stats_daily,
            commands::get_token_stats_weekly,
            commands::get_token_stats_by_account,
            commands::get_token_stats_summary,
            commands::get_token_stats_by_model,
            commands::get_token_stats_model_trend_hourly,
            commands::get_token_stats_model_trend_daily,
            commands::get_token_stats_account_trend_hourly,
            commands::get_token_stats_account_trend_daily,
            proxy::cli_sync::get_cli_sync_status,
            proxy::cli_sync::execute_cli_sync,
            proxy::cli_sync::execute_cli_restore,
            proxy::cli_sync::get_cli_config_content,
            proxy::opencode_sync::get_opencode_sync_status,
            proxy::opencode_sync::execute_opencode_sync,
            proxy::opencode_sync::execute_opencode_restore,
            proxy::opencode_sync::get_opencode_config_content,
            proxy::opencode_sync::execute_opencode_clear,
            proxy::droid_sync::get_droid_sync_status,
            proxy::droid_sync::execute_droid_sync,
            proxy::droid_sync::execute_droid_restore,
            proxy::droid_sync::get_droid_config_content,
            // Security/IP monitoring commands
            commands::security::get_ip_access_logs,
            commands::security::get_ip_stats,
            commands::security::get_ip_token_stats,
            commands::security::clear_ip_access_logs,
            commands::security::get_ip_blacklist,
            commands::security::add_ip_to_blacklist,
            commands::security::remove_ip_from_blacklist,
            commands::security::clear_ip_blacklist,
            commands::security::check_ip_in_blacklist,
            commands::security::get_ip_whitelist,
            commands::security::add_ip_to_whitelist,
            commands::security::remove_ip_from_whitelist,
            commands::security::clear_ip_whitelist,
            commands::security::check_ip_in_whitelist,
            commands::security::get_security_config,
            commands::security::update_security_config,
            // Cloudflared commands
            commands::cloudflared::cloudflared_check,
            commands::cloudflared::cloudflared_install,
            commands::cloudflared::cloudflared_start,
            commands::cloudflared::cloudflared_stop,
            commands::cloudflared::cloudflared_get_status,
            // Debug console commands
            modules::log_bridge::enable_debug_console,
            modules::log_bridge::disable_debug_console,
            modules::log_bridge::is_debug_console_enabled,
            modules::log_bridge::get_debug_console_logs,
            modules::log_bridge::clear_debug_console_logs,
            // User Token commands
            commands::user_token::list_user_tokens,
            commands::user_token::create_user_token,
            commands::user_token::update_user_token,
            commands::user_token::delete_user_token,
            commands::user_token::renew_user_token,
            commands::user_token::get_token_ip_bindings,
            commands::user_token::get_user_token_summary,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            match event {
                // Handle app exit - cleanup background tasks
                tauri::RunEvent::Exit => {
                    tracing::info!("Application exiting, cleaning up background tasks...");
                    if let Some(state) = app_handle.try_state::<crate::commands::proxy::ProxyServiceState>() {
                        tauri::async_runtime::block_on(async {
                            // Use timeout-based read() instead of try_read() to handle lock contention
                            match tokio::time::timeout(
                                std::time::Duration::from_secs(3),
                                state.instance.read()
                            ).await {
                                Ok(guard) => {
                                    if let Some(instance) = guard.as_ref() {
                                        // Use graceful_shutdown with 2s timeout for task cleanup
                                        instance.token_manager
                                            .graceful_shutdown(std::time::Duration::from_secs(2))
                                            .await;
                                    }
                                }
                                Err(_) => {
                                    tracing::warn!("Lock acquisition timed out after 3s, forcing exit");
                                }
                            }
                        });
                    }
                }
                // Handle macOS dock icon click to reopen window
                #[cfg(target_os = "macos")]
                tauri::RunEvent::Reopen { .. } => {
                    if let Some(window) = app_handle.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.unminimize();
                        let _ = window.set_focus();
                        app_handle.set_activation_policy(tauri::ActivationPolicy::Regular).unwrap_or(());
                    }
                }
                _ => {}
            }
        });
}
