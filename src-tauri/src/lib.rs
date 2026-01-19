mod models;
mod modules;
mod commands;
mod utils;
mod proxy;  // Proxy service module
pub mod error;

use tauri::Manager;
use modules::logger;
use tracing::{info, warn, error};

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
    // Increase file descriptor limit (macOS only)
    #[cfg(target_os = "macos")]
    increase_nofile_limit();

    // Initialize logger
    logger::init_logger();

    // Initialize token stats database
    if let Err(e) = modules::token_stats::init_db() {
        error!("Failed to initialize token stats database: {}", e);
    }
    
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
        .setup(|app| {
            info!("Setup starting...");

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
            
            // Auto-start proxy service
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                // Load config
                if let Ok(config) = modules::config::load_app_config() {
                    if config.proxy.auto_start {
                        let state = handle.state::<commands::proxy::ProxyServiceState>();
                        // Attempt to start service
                        if let Err(e) = commands::proxy::start_proxy_service(
                            config.proxy,
                            state,
                            handle.clone(),
                        ).await {
                            error!("Failed to auto-start proxy service: {}", e);
                        } else {
                            info!("Proxy service auto-started successfully");
                        }
                    }
                }
            });
            
            // Start smart scheduler
            modules::scheduler::start_scheduler(app.handle().clone());
            
            // Start HTTP API server (for external calls, e.g. VS Code plugin)
            match modules::http_api::load_settings() {
                Ok(settings) if settings.enabled => {
                    modules::http_api::spawn_server(settings.port);
                    info!("HTTP API server started on port {}", settings.port);
                }
                Ok(_) => {
                    info!("HTTP API server is disabled in settings");
                }
                Err(e) => {
                    // Use default port if loading fails
                    error!("Failed to load HTTP API settings: {}, using default port", e);
                    modules::http_api::spawn_server(modules::http_api::DEFAULT_PORT);
                    info!("HTTP API server started on port {}", modules::http_api::DEFAULT_PORT);
                }
            }
            
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
            commands::list_accounts,
            commands::add_account,
            commands::delete_account,
            commands::delete_accounts,
            commands::reorder_accounts,
            commands::switch_account,
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
            commands::import_v1_accounts,
            commands::import_from_db,
            commands::import_custom_db,
            commands::sync_account_from_db,
            commands::save_text_file,
            commands::read_text_file,
            commands::clear_log_cache,
            commands::open_data_folder,
            commands::get_data_dir_path,
            commands::show_main_window,
            commands::get_antigravity_path,
            commands::get_antigravity_args,
            commands::check_for_updates,
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
            commands::proxy::fetch_zai_models,
            commands::proxy::get_proxy_scheduling_config,
            commands::proxy::update_proxy_scheduling_config,
            commands::proxy::clear_proxy_session_bindings,
            commands::proxy::set_preferred_account,
            commands::proxy::get_preferred_account,
            // Autostart commands
            commands::autostart::toggle_auto_launch,
            commands::autostart::is_auto_launch_enabled,
            // Warmup commands
            commands::warm_up_all_accounts,
            commands::warm_up_account,
            // HTTP API settings commands
            commands::get_http_api_settings,
            commands::save_http_api_settings,
            // Token 统计命令
            commands::get_token_stats_hourly,
            commands::get_token_stats_daily,
            commands::get_token_stats_weekly,
            commands::get_token_stats_by_account,
            commands::get_token_stats_summary,
            proxy::cli_sync::get_cli_sync_status,
            proxy::cli_sync::execute_cli_sync,
            proxy::cli_sync::execute_cli_restore,
            proxy::cli_sync::get_cli_config_content,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            // Handle macOS dock icon click to reopen window
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                if let Some(window) = app_handle.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                    app_handle.set_activation_policy(tauri::ActivationPolicy::Regular).unwrap_or(());
                }
            }
            // Suppress unused variable warnings on non-macOS platforms
            #[cfg(not(target_os = "macos"))]
            let _ = (app_handle, event);
        });
}
