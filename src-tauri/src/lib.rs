mod models;
mod modules;
mod commands;
mod utils;
mod proxy;  // 反代服务模块
pub mod error;

use tauri::Manager;
use modules::logger;
use tracing::{info, error};

// 测试命令
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 初始化日志
    logger::init_logger();
    
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
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
            modules::tray::create_tray(app.handle())?;
            info!("Tray created");
            
            // 自动启动反代服务
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                // 加载配置
                if let Ok(config) = modules::config::load_app_config() {
                    if config.proxy.auto_start {
                        let state = handle.state::<commands::proxy::ProxyServiceState>();
                        // 尝试启动服务
                        if let Err(e) = commands::proxy::start_proxy_service(
                            config.proxy,
                            state,
                            handle.clone(),
                        ).await {
                            error!("自动启动反代服务失败: {}", e);
                        } else {
                            info!("反代服务自动启动成功");
                        }
                    }
                }
            });
            
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
            // 账号管理命令
            commands::list_accounts,
            commands::add_account,
            commands::delete_account,
            commands::delete_accounts,
            commands::reorder_accounts,
            commands::switch_account,
            commands::get_current_account,
            // 配额命令
            commands::fetch_account_quota,
            commands::refresh_all_quotas,
            // 配置命令
            commands::load_config,
            commands::save_config,
            // 新增命令
            commands::prepare_oauth_url,
            commands::start_oauth_login,
            commands::complete_oauth_login,
            commands::cancel_oauth_login,
            commands::import_v1_accounts,
            commands::import_from_db,
            commands::import_custom_db,
            commands::sync_account_from_db,
            commands::save_text_file,
            commands::clear_log_cache,
            commands::open_data_folder,
            commands::get_data_dir_path,
            commands::show_main_window,
            commands::get_antigravity_path,
            commands::get_antigravity_args,
            commands::check_for_updates,
            commands::toggle_proxy_status,
            // 反代服务命令
            commands::proxy::start_proxy_service,
            commands::proxy::stop_proxy_service,
            commands::proxy::get_proxy_status,
            commands::proxy::get_proxy_stats,
            commands::proxy::get_proxy_logs,
            commands::proxy::set_proxy_monitor_enabled,
            commands::proxy::clear_proxy_logs,
            commands::proxy::generate_api_key,
            commands::proxy::reload_proxy_accounts,
            commands::proxy::update_model_mapping,
            commands::proxy::fetch_zai_models,
            commands::proxy::get_proxy_scheduling_config,
            commands::proxy::update_proxy_scheduling_config,
            commands::proxy::clear_proxy_session_bindings,
            // Autostart 命令
            commands::autostart::toggle_auto_launch,
            commands::autostart::is_auto_launch_enabled,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
