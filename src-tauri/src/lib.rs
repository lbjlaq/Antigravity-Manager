mod models;
mod modules;
mod commands;
mod utils;
mod proxy;  // Proxy service module
pub mod shared;  // Shared infrastructure (db pool, etc.)
pub mod error;
pub mod constants;

use tauri::Manager;
use modules::logger;
use tracing::{info, warn, error};
use std::sync::Arc;

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

    // Initialize token stats database
    if let Err(e) = modules::token_stats::init_db() {
        error!("Failed to initialize token stats database: {}", e);
    }

    // Initialize security database
    if let Err(e) = modules::security_db::init_db() {
        error!("Failed to initialize security database: {}", e);
    }
    
    if is_headless {
        info!("Starting in HEADLESS mode...");
        
        let rt = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");
        rt.block_on(async {
            // Initialize states manually
            let proxy_state = commands::proxy::ProxyServiceState::new();
            let cf_state = Arc::new(commands::cloudflared::CloudflaredState::new());

            // Load config
            match modules::config::load_app_config() {
                Ok(mut config) => {
                    let mut modified = false;

                    // Force LAN access in headless/docker mode so it binds to 0.0.0.0
                    if !config.proxy.allow_lan_access {
                        config.proxy.allow_lan_access = true;
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
                            if config.proxy.api_key != key {
                                config.proxy.api_key = key;
                                modified = true;
                            }
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
                            if config.proxy.admin_password.as_deref() != Some(pwd.as_str()) {
                                config.proxy.admin_password = Some(pwd);
                                modified = true;
                            }
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
                            let needs_update = !matches!(
                                (&config.proxy.auth_mode, &m),
                                (
                                    crate::proxy::ProxyAuthMode::Off,
                                    crate::proxy::ProxyAuthMode::Off
                                ) | (
                                    crate::proxy::ProxyAuthMode::Strict,
                                    crate::proxy::ProxyAuthMode::Strict
                                ) | (
                                    crate::proxy::ProxyAuthMode::AllExceptHealth,
                                    crate::proxy::ProxyAuthMode::AllExceptHealth
                                ) | (
                                    crate::proxy::ProxyAuthMode::Auto,
                                    crate::proxy::ProxyAuthMode::Auto
                                )
                            );
                            if needs_update {
                                config.proxy.auth_mode = m;
                                modified = true;
                            }
                        }
                    }

                    if modified {
                        if let Err(e) = modules::config::save_app_config(&config) {
                            error!("Failed to persist headless config overrides: {}", e);
                        } else {
                            info!("Persisted headless config overrides to gui_config.json");
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
                    
                    // Start smart scheduler
                    modules::scheduler::start_scheduler(None, proxy_state.clone());
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

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_window_state::Builder::default().build());

        // Single instance plugin - only in release builds (controlled by feature flag)
        #[cfg(feature = "single-instance")]
        let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            let _ = app.get_webview_window("main")
                .map(|window| {
                    let _ = window.show();
                    let _ = window.set_focus();
                    #[cfg(target_os = "macos")]
                    app.set_activation_policy(tauri::ActivationPolicy::Regular).unwrap_or(());
                });
        }));

        builder.manage(commands::proxy::ProxyServiceState::new())
        .manage(commands::cloudflared::CloudflaredState::new())
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
                if let Some(window) = app.get_webview_window("main") {
                    // Access GTK window and disable transparency at the GTK level
                    if let Ok(gtk_window) = window.gtk_window() {
                        use gtk::prelude::WidgetExt;
                        // Remove the visual's alpha channel to disable transparency
                        if let Some(screen) = gtk_window.screen() {
                            // Use non-composited visual if available
                            if let Some(visual) = screen.system_visual() {
                                gtk_window.set_visual(Some(&visual));
                            }
                        }
                        info!("Linux: Applied transparent window workaround");
                    }
                }
            }

            modules::tray::create_tray(app.handle())?;
            info!("Tray created");
            
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
            
            // Start smart scheduler
            let scheduler_state = app.handle().state::<commands::proxy::ProxyServiceState>();
            modules::scheduler::start_scheduler(Some(app.handle().clone()), scheduler_state.inner().clone());
            
            // [PHASE 1] 已整合至 Axum 端口 (8045)，不再单独启动 19527 端口
            info!("Management API integrated into main proxy server (port 8045)");
            
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                #[cfg(target_os = "macos")]
                {
                    use tauri::Manager;
                    window.app_handle().set_activation_policy(tauri::ActivationPolicy::Accessory).unwrap_or(());
                }
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            // Account management commands
            commands::account::list_accounts,
            commands::account::add_account,
            commands::account::delete_account,
            commands::account::delete_accounts,
            commands::account::reorder_accounts,
            commands::account::switch_account,
            commands::account::get_current_account,
            commands::account::toggle_proxy_status,
            commands::account::export_accounts,
            // Device fingerprint
            commands::device::get_device_profiles,
            commands::device::bind_device_profile,
            commands::device::bind_device_profile_with_profile,
            commands::device::preview_generate_profile,
            commands::device::apply_device_profile,
            commands::device::restore_original_device,
            commands::device::list_device_versions,
            commands::device::restore_device_version,
            commands::device::delete_device_version,
            commands::device::open_device_folder,
            // Quota commands
            commands::quota::fetch_account_quota,
            commands::quota::refresh_all_quotas,
            commands::quota::warm_up_all_accounts,
            commands::quota::warm_up_account,
            // Config commands
            commands::config::load_config,
            commands::config::save_config,
            commands::config::get_http_api_settings,
            commands::config::save_http_api_settings,
            // OAuth commands
            commands::oauth::prepare_oauth_url,
            commands::oauth::start_oauth_login,
            commands::oauth::complete_oauth_login,
            commands::oauth::cancel_oauth_login,
            commands::oauth::submit_oauth_code,
            // Import commands
            commands::import::import_v1_accounts,
            commands::import::import_from_db,
            commands::import::import_custom_db,
            commands::import::sync_account_from_db,
            // System commands
            commands::system::save_text_file,
            commands::system::read_text_file,
            commands::system::clear_log_cache,
            commands::system::clear_antigravity_cache,
            commands::system::get_antigravity_cache_paths,
            commands::system::open_data_folder,
            commands::system::get_data_dir_path,
            commands::system::show_main_window,
            commands::system::set_window_theme,
            commands::system::get_antigravity_path,
            commands::system::get_antigravity_args,
            commands::system::check_for_updates,
            commands::system::get_update_settings,
            commands::system::save_update_settings,
            commands::system::should_check_updates,
            commands::system::update_last_check_time,
            // Token stats commands
            commands::stats::get_token_stats_hourly,
            commands::stats::get_token_stats_daily,
            commands::stats::get_token_stats_weekly,
            commands::stats::get_token_stats_by_account,
            commands::stats::get_token_stats_summary,
            commands::stats::get_token_stats_by_model,
            commands::stats::get_token_stats_model_trend_hourly,
            commands::stats::get_token_stats_model_trend_daily,
            commands::stats::get_token_stats_account_trend_hourly,
            commands::stats::get_token_stats_account_trend_daily,
            // Proxy service commands
            commands::proxy::lifecycle::start_proxy_service,
            commands::proxy::lifecycle::stop_proxy_service,
            commands::proxy::status::get_proxy_status,
            commands::proxy::status::get_proxy_stats,
            commands::proxy::status::get_proxy_logs,
            commands::proxy::logs::get_proxy_logs_paginated,
            commands::proxy::logs::get_proxy_log_detail,
            commands::proxy::logs::get_proxy_logs_count,
            commands::proxy::logs::export_proxy_logs,
            commands::proxy::logs::export_proxy_logs_json,
            commands::proxy::logs::get_proxy_logs_count_filtered,
            commands::proxy::logs::get_proxy_logs_filtered,
            commands::proxy::status::set_proxy_monitor_enabled,
            commands::proxy::status::clear_proxy_logs,
            commands::proxy::config::generate_api_key,
            commands::proxy::accounts::reload_proxy_accounts,
            commands::proxy::config::update_model_mapping,
            commands::proxy::external::fetch_zai_models,
            commands::proxy::config::get_proxy_scheduling_config,
            commands::proxy::config::update_proxy_scheduling_config,
            commands::proxy::accounts::clear_proxy_session_bindings,
            commands::proxy::accounts::set_preferred_account,
            commands::proxy::accounts::get_preferred_account,
            commands::proxy::accounts::clear_proxy_rate_limit,
            commands::proxy::accounts::clear_all_proxy_rate_limits,
            // Autostart commands
            commands::autostart::toggle_auto_launch,
            commands::autostart::is_auto_launch_enabled,
            // CLI sync commands
            proxy::cli_sync::get_cli_sync_status,
            proxy::cli_sync::execute_cli_sync,
            proxy::cli_sync::execute_cli_restore,
            proxy::cli_sync::get_cli_config_content,
            // OpenCode sync commands
            proxy::opencode_sync::get_opencode_sync_status,
            proxy::opencode_sync::execute_opencode_sync,
            proxy::opencode_sync::execute_opencode_restore,
            proxy::opencode_sync::get_opencode_config_content,
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
            // Security commands (IP blacklist/whitelist)
            commands::security::security_init_db,
            commands::security::security_get_blacklist,
            commands::security::security_add_to_blacklist,
            commands::security::security_remove_from_blacklist,
            commands::security::security_remove_from_blacklist_by_id,
            commands::security::security_is_ip_blacklisted,
            commands::security::security_get_whitelist,
            commands::security::security_add_to_whitelist,
            commands::security::security_remove_from_whitelist,
            commands::security::security_remove_from_whitelist_by_id,
            commands::security::security_is_ip_whitelisted,
            commands::security::security_get_access_logs,
            commands::security::security_cleanup_logs,
            commands::security::security_clear_all_logs,
            commands::security::security_get_stats,
            commands::security::security_clear_blacklist,
            commands::security::security_clear_whitelist,
            commands::security::security_get_ip_token_stats,
            commands::security::get_security_config,
            commands::security::update_security_config,
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
