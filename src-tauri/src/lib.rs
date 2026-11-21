pub mod commands;
pub mod constants;
pub mod error;
#[cfg(any(target_os = "linux", test))]
pub mod linux_display_guard;
#[cfg(any(target_os = "linux", test))]
mod linux_graphics;
pub mod models;
pub mod modules;
pub mod utils;

use modules::logger;
use tauri::Manager;
use tracing::{info, warn};

#[derive(Clone, Copy)]
struct AppRuntimeFlags {
    tray_enabled: bool,
}

fn env_flag_enabled(name: &str) -> bool {
    std::env::var(name)
        .map(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
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
            // 智能自适应检测：检查系统中是否存在有效的 AppIndicator / StatusNotifier 动态链接库
            let has_appindicator = [
                "/usr/lib/x86_64-linux-gnu/libayatana-appindicator3.so.1",
                "/usr/lib/x86_64-linux-gnu/libappindicator3.so.1",
                "/usr/lib64/libayatana-appindicator3.so.1",
                "/usr/lib64/libappindicator3.so.1",
                "/usr/lib/libayatana-appindicator3.so.1",
                "/usr/lib/libappindicator3.so.1",
            ]
            .iter()
            .any(|path| std::path::Path::new(path).exists());

            if has_appindicator {
                info!("Linux Wayland session detected with valid AppIndicator libraries. Enabling tray automatically.");
                return true;
            }

            warn!(
                "Linux Wayland session detected without AppIndicator libraries; disabling tray by default to avoid GTK crashes. Install libayatana-appindicator3 or set ANTIGRAVITY_FORCE_TRAY=1 to force-enable."
            );
            return false;
        }
    }

    true
}

/// Login-item launches or shortcut flags pass `--minimized`, `--silent`, `--hidden`, or `-s`.
/// Quiet startup applies only to that launch.
fn apply_login_launch_policy(app: &tauri::App) {
    let launched_by_login = std::env::args().any(|arg| {
        let lower = arg.to_lowercase();
        lower == "--minimized"
            || lower == "--silent"
            || lower == "--hidden"
            || lower == "-s"
            || lower == "/minimized"
            || lower.contains("--minimized")
            || lower.contains("--silent")
            || lower.contains("--hidden")
    });
    if !launched_by_login {
        return;
    }

    let config = modules::load_app_config().unwrap_or_default();
    if !config.quiet_autostart {
        info!("Login launch will show the main window because quiet startup is off");
        return;
    }

    let tray_enabled = app
        .try_state::<AppRuntimeFlags>()
        .map(|flags| flags.tray_enabled)
        .unwrap_or(true);
    if !tray_enabled {
        info!("Quiet startup skipped because the tray is unavailable; showing the main window");
        #[cfg(not(target_os = "android"))]
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.show();
        }
        return;
    }

    modules::startup_quiet::arm();
    if config.lightweight_mode {
        info!("Quiet login launch will enter lightweight mode once the event loop is ready");
        modules::startup_quiet::request_lightweight_on_ready();
    } else {
        info!("Quiet login launch will keep the main window hidden in the tray");
        #[cfg(target_os = "macos")]
        {
            let _ = app
                .handle()
                .set_activation_policy(tauri::ActivationPolicy::Accessory);
        }
    }
}



#[cfg(target_os = "linux")]
fn nvidia_proprietary_loaded() -> bool {
    std::path::Path::new("/dev/nvidia0").exists()
        || std::path::Path::new("/proc/driver/nvidia/version").exists()
        || std::path::Path::new("/sys/module/nvidia").exists()
}

#[cfg(target_os = "linux")]
fn configure_linux_graphics() {
    // 确保 GLib 在未设置 G_MESSAGES_DEBUG 时不过滤底层 Gdk/Gtk 的 debug 日志
    if std::env::var("G_MESSAGES_DEBUG").is_err() {
        std::env::set_var("G_MESSAGES_DEBUG", "Gdk,Gtk");
    }

    linux_display_guard::init_display_guard();

    use linux_graphics::{
        desktop_is_gnome, desktop_is_kde, desktop_is_wlroots_family, is_remote_virtual_desktop,
        should_disable_webkit_dmabuf, should_enable_software_compositing, should_force_x11_backend,
    };

    let is_wayland = is_wayland_session();
    let has_x11_display = std::env::var("DISPLAY")
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false);
    let desktop = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_else(|_| std::env::var("XDG_SESSION_DESKTOP").unwrap_or_default());
    let force_wayland = env_flag_enabled("ANTIGRAVITY_FORCE_WAYLAND");
    let force_x11 = env_flag_enabled("ANTIGRAVITY_FORCE_X11");
    let gdk_already_set = std::env::var("GDK_BACKEND").is_ok();

    if should_force_x11_backend(
        gdk_already_set,
        force_x11,
        force_wayland,
        is_wayland,
        has_x11_display,
        &desktop,
    ) {
        // Force X11 backend under GNOME/legacy Wayland to avoid a GTK shm crash.
        std::env::set_var("GDK_BACKEND", "x11");
        warn!(
            "Forcing GDK_BACKEND=x11 for stability on Wayland. Set ANTIGRAVITY_FORCE_WAYLAND=1 to keep Wayland backend."
        );
    } else if is_wayland
        && !gdk_already_set
        && (desktop_is_wlroots_family(&desktop)
            || desktop_is_kde(&desktop)
            || desktop_is_gnome(&desktop))
    {
        info!(
            "Keeping native Wayland GDK backend on {} (avoiding Xwayland WebKitGTK frame freeze).",
            desktop
        );
    }

    let webkit_already_set = std::env::var("WEBKIT_DISABLE_DMABUF_RENDERER").is_ok();
    if should_disable_webkit_dmabuf(
        webkit_already_set,
        is_wayland,
        nvidia_proprietary_loaded(),
        &desktop,
    ) {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        info!(
            "WEBKIT_DISABLE_DMABUF_RENDERER=1 (WebKit DMA-BUF workaround on this Wayland setup). Set it yourself to override."
        );
    }

    // [Fix #3752] 针对 xrdp / VNC 等远程虚拟桌面会话，自动启用 WebKitGTK 软件安全合成与渲染
    // 根除客户端断开时由于显存缓冲区交换失败引发的 XIO Error / Broken pipe 崩溃，普通本地桌面不受影响
    let has_xrdp_env = std::env::var("XRDP_SESSION").is_ok()
        || std::env::var("XRDP_SOCKET_PATH").is_ok()
        || std::env::var("XRDP_PULSE_SINK_SOCKET").is_ok();
    let has_vnc_env = std::env::var("VNCDESKTOP").is_ok() || std::env::var("VNC_PORT").is_ok();
    let force_software = env_flag_enabled("ANTIGRAVITY_FORCE_SOFTWARE_RENDER");
    let force_hardware = env_flag_enabled("ANTIGRAVITY_FORCE_HARDWARE_RENDER");
    let is_remote_desktop =
        is_remote_virtual_desktop(has_xrdp_env, has_vnc_env, force_software, force_hardware);

    let compositing_already_disabled = std::env::var("WEBKIT_DISABLE_COMPOSITING_MODE").is_ok();
    if should_enable_software_compositing(compositing_already_disabled, is_remote_desktop) {
        std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        if std::env::var("LIBGL_ALWAYS_SOFTWARE").is_err() {
            std::env::set_var("LIBGL_ALWAYS_SOFTWARE", "1");
        }
        info!(
            "Remote virtual desktop session detected (xrdp/VNC). Enabling safe software compositing to prevent XIO crashes on disconnect."
        );
    }
}

/// Increase file descriptor limit on Unix (macOS/Linux) to prevent "Too many open files" errors
#[cfg(unix)]
fn increase_nofile_limit() {
    unsafe {
        let mut rl = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };

        if libc::getrlimit(libc::RLIMIT_NOFILE, &mut rl) == 0 {
            info!(
                "Current open file limit: soft={}, hard={}",
                rl.rlim_cur, rl.rlim_max
            );

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

/// Windows FFI calls to disable Efficiency Mode (EcoQoS / Power Throttling)
/// to prevent background freezes when minimized/hidden.
#[cfg(target_os = "windows")]
mod windows_api {
    type Bool = i32;
    type Handle = *mut std::ffi::c_void;

    #[repr(C)]
    struct ProcessPowerThrottlingState {
        version: u32,
        control_mask: u32,
        state_mask: u32,
    }

    #[link(name = "Kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> Handle;
        fn SetProcessInformation(
            h_process: Handle,
            process_information_class: u32,
            process_information: *mut std::ffi::c_void,
            process_information_size: u32,
        ) -> Bool;
    }

    pub fn disable_efficiency_mode() {
        unsafe {
            let mut state = ProcessPowerThrottlingState {
                version: 1,        // PROCESS_POWER_THROTTLING_STATE::VERSION
                control_mask: 0x1, // PROCESS_POWER_THROTTLING_CURRENT_EXECUTION_SPEED
                state_mask: 0,
            };
            let process_handle = GetCurrentProcess();
            // ProcessPowerThrottling = 4
            let res = SetProcessInformation(
                process_handle,
                4,
                &mut state as *mut _ as *mut std::ffi::c_void,
                std::mem::size_of::<ProcessPowerThrottlingState>() as u32,
            );
            if res == 0 {
                let err = std::io::Error::last_os_error();
                tracing::warn!(
                    "Failed to disable Windows Power Throttling / EcoQoS: {}",
                    err
                );
            } else {
                tracing::info!(
                    "Successfully disabled Windows Power Throttling / EcoQoS for the process."
                );
            }
        }
    }
}

// Test command
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg(not(target_os = "android"))]
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Record main thread ID on Linux for display disconnection guard
    #[cfg(target_os = "linux")]
    linux_display_guard::record_main_thread_id();

    // Disable Windows background throttling/EcoQoS
    #[cfg(target_os = "windows")]
    windows_api::disable_efficiency_mode();

    // Check for headless mode
    let args: Vec<String> = std::env::args().collect();

    // Check for CLI subcommands (switch, list, current, etc.)
    if let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        match runtime.block_on(modules::cli_handler::try_handle_cli(&args)) {
            Ok(true) => return,
            Ok(false) => {}
            Err(e) => {
                eprintln!("\x1b[31m[Antigravity CLI Error]\x1b[0m {}", e);
                std::process::exit(1);
            }
        }
    }

    // Increase file descriptor limit (Unix: macOS/Linux)
    #[cfg(unix)]
    increase_nofile_limit();

    // Initialize logger
    logger::init_logger();

    #[cfg(target_os = "linux")]
    configure_linux_graphics();

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
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::all()
                        .difference(tauri_plugin_window_state::StateFlags::VISIBLE),
                )
                // Fixes #3993: 插件在 on_window_ready 中会于 WebviewWindow::build 期间同步 restore_state，
                // 此时 Tauri WindowManager/PluginStore 的非可重入 RwLock 仍被持有；Windows 上
                // SetWindowPos 会同步回派 WM_WINDOWPOSCHANGED 并再次取锁，导致 UI 线程死锁。
                // 跳过 main 的初始恢复，改为构建完成后再恢复（见 setup 与 lightweight::ensure_main_window）。
                .skip_initial_state("main")
                .build(),
        )
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // Fixes #3993: Windows 下该回调跑在 UI 线程，且可能仍持有 PluginStore 锁；
            // 经工作线程再投递到主线程，等当前回调返回后再重建窗口。
            #[cfg(target_os = "windows")]
            {
                let app_handle = app.clone();
                modules::lightweight::defer_on_main_thread(app, move || {
                    let _ = modules::lightweight::exit_lightweight_mode(&app_handle);
                });
            }
            #[cfg(not(target_os = "windows"))]
            {
                let _ = modules::lightweight::exit_lightweight_mode(app);
            }
        }))
        .manage(AppRuntimeFlags { tray_enabled })
        .setup(|app| {
            info!("Setup starting...");

            // Initialize log bridge with app handle for debug console
            modules::log_bridge::init_log_bridge(app.handle().clone());

            // 为主窗口显式设置应用图标（强制触发 Win32 WM_SETICON，防止透明/覆盖标题栏窗口在任务栏丢失图标）
            #[cfg(not(target_os = "macos"))]
            if let Some(window) = app.get_webview_window("main") {
                let icon_bytes: &[u8] = include_bytes!("../icons/icon.png");
                if let Ok(img) = image::load_from_memory(icon_bytes) {
                    let rgba = img.to_rgba8();
                    let (width, height) = rgba.dimensions();
                    let _ = window.set_icon(tauri::image::Image::new_owned(
                        rgba.into_raw(),
                        width,
                        height,
                    ));
                }
            }

            // Fixes #3993: 已对 main 设置 skip_initial_state，启动窗口的位置/尺寸恢复改在窗口
            // 构建完成后的 setup 中执行，不再嵌套于构建期间（持有 Tauri 内部 RwLock）。
            #[cfg(not(target_os = "android"))]
            if let Some(window) = app.get_webview_window("main") {
                use tauri_plugin_window_state::{StateFlags, WindowExt};
                let flags = StateFlags::all().difference(StateFlags::VISIBLE);
                #[cfg(target_os = "windows")]
                {
                    modules::lightweight::defer_on_main_thread(app.handle(), move || {
                        let _ = window.restore_state(flags);
                    });
                }
                #[cfg(not(target_os = "windows"))]
                {
                    let _ = window.restore_state(flags);
                }
            }

            // Windows: 异步原生自愈桌面与开始菜单历史快捷方式图标缺失，并刷新外壳（零子进程，不调用 powershell）
            #[cfg(target_os = "windows")]
            {
                std::thread::spawn(|| {
                    crate::utils::win_shortcut::heal_shortcuts_native();
                });
            }

            let runtime_flags = app.state::<AppRuntimeFlags>();
            if runtime_flags.tray_enabled {
                modules::tray::create_tray(app.handle())?;
                info!("Tray created");
            } else {
                info!("Tray disabled for this session");
            }

            // 启动时异步刷新全量配额
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                modules::startup_refresh::spawn_startup_quota_refresh(Some(handle));
            });

            // Start smart scheduler for 7-day weekly reset warmup
            modules::scheduler::start_scheduler(Some(app.handle().clone()));
            info!("Smart scheduler (7-Day Weekly Reset Warmup) initialized.");

            // Start background auto-refresh scheduler (Fixes #3958)
            modules::auto_refresh::start_auto_refresh(Some(app.handle().clone()));
            info!("Background auto-refresh scheduler initialized.");

            apply_login_launch_policy(app);

            ensure_cli_symlinks();

            #[cfg(target_os = "linux")]
            {
                tauri::async_runtime::spawn(async {
                    use tokio::signal::unix::{signal, SignalKind};
                    let mut sigterm = signal(SignalKind::terminate()).ok();
                    let mut sigint = signal(SignalKind::interrupt()).ok();
                    tokio::select! {
                        _ = async {
                            if let Some(ref mut s) = sigterm {
                                s.recv().await;
                            } else {
                                std::future::pending::<()>().await;
                            }
                        } => {},
                        _ = async {
                            if let Some(ref mut s) = sigint {
                                s.recv().await;
                            } else {
                                std::future::pending::<()>().await;
                            }
                        } => {},
                    }
                    tracing::info!("[Linux Display Guard] Received termination signal in background mode, shutting down cleanly.");
                    linux_display_guard::mark_user_initiated_exit();
                    std::process::exit(0);
                });
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            match event {
                tauri::WindowEvent::Focused(focused) => {
                    if window.label() == "main" && *focused {
                        #[cfg(target_os = "macos")]
                        {
                            let _ = window
                                .app_handle()
                                .set_activation_policy(tauri::ActivationPolicy::Regular);
                        }
                    } else if !focused && window.label() == "mini" {
                        modules::tray::record_blur();
                        if !modules::tray::is_mini_view_pinned() && modules::tray::is_mini_view_active() {
                            modules::tray::hide_mini_window(window.app_handle());
                        }
                    }
                }
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    if window.label() == "mini" {
                        api.prevent_close();
                        modules::tray::hide_mini_window(window.app_handle());
                        return;
                    }

                    let tray_enabled = window
                        .app_handle()
                        .try_state::<AppRuntimeFlags>()
                        .map(|flags| flags.tray_enabled)
                        .unwrap_or(true);

                    if tray_enabled {
                        api.prevent_close();

                        let is_lightweight = modules::load_app_config()
                            .map(|c| c.lightweight_mode)
                            .unwrap_or(false);

                        if is_lightweight {
                            let _ = modules::lightweight::enter_lightweight_mode(window.app_handle());
                        } else {
                            let _ = window.hide();
                            #[cfg(target_os = "macos")]
                            {
                                use tauri::Manager;
                                window
                                    .app_handle()
                                    .set_activation_policy(tauri::ActivationPolicy::Accessory)
                                    .unwrap_or(());
                            }
                        }
                    }
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            // Account management commands
            commands::list_accounts,
            commands::add_account,
            commands::batch_import_accounts,
            commands::delete_account,
            commands::delete_accounts,
            commands::release_account_qos_jail,
            commands::remove_account_forbidden_model,
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
            commands::get_active_clients_status,
            // Quota commands
            commands::fetch_account_quota,
            commands::refresh_all_quotas,
            // Config commands
            commands::load_config,
            commands::get_config,
            commands::save_config,
            // Additional commands
            commands::prepare_oauth_url,
            commands::start_oauth_login,
            commands::complete_oauth_login,
            commands::cancel_oauth_login,
            commands::submit_oauth_code,
            commands::list_oauth_clients,
            commands::get_active_oauth_client,
            commands::set_active_oauth_client,
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
            commands::get_internal_error_log_path,
            commands::get_internal_error_log_disk_size,
            commands::set_data_dir,
            commands::migrate_data_dir,
            commands::show_main_window,
            commands::switch_to_main_window,
            commands::show_mini_view,
            commands::hide_mini_view,
            commands::exit_app,
            commands::set_mini_view_pinned,
            commands::is_mini_view_pinned,
            commands::set_mini_view_active,
            commands::update_tray_for_account,
            commands::set_window_theme,
            commands::get_antigravity_path,
            commands::get_antigravity_cli_path,
            commands::get_antigravity_args,
            commands::check_for_updates,
            commands::check_native_update,
            commands::check_homebrew_installation,
            commands::check_appimage_installation,
            commands::brew_upgrade_cask,
            commands::get_update_settings,
            commands::save_update_settings,
            commands::should_check_updates,
            commands::update_last_check_time,
            commands::toggle_proxy_status,
            commands::clear_account_forbidden,
            // Autostart commands
            commands::autostart::toggle_auto_launch,
            commands::autostart::is_auto_launch_enabled,
            // Warmup commands
            commands::warm_up_all_accounts,
            commands::warm_up_account,
            commands::update_account_label,
            commands::update_account_priority,
            commands::update_account_project_id,
            commands::auto_fetch_account_project_id,
            // Debug console commands
            modules::log_bridge::enable_debug_console,
            modules::log_bridge::disable_debug_console,
            modules::log_bridge::is_debug_console_enabled,
            modules::log_bridge::get_debug_console_logs,
            modules::log_bridge::clear_debug_console_logs,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            match event {
                tauri::RunEvent::Ready => {
                    if modules::startup_quiet::take_lightweight_on_ready() {
                        info!("Entering lightweight mode for quiet login launch");
                        let _ = modules::lightweight::enter_lightweight_mode_without_saving(app_handle);
                    }
                }
                // Prevent app from exiting when window is destroyed in lightweight mode
                tauri::RunEvent::ExitRequested { api, .. } => {
                    let tray_enabled = app_handle
                        .try_state::<AppRuntimeFlags>()
                        .map(|flags| flags.tray_enabled)
                        .unwrap_or(true);

                    if tray_enabled {
                        api.prevent_exit();
                    }
                }
                // Handle app exit
                tauri::RunEvent::Exit => {
                    tracing::info!("Application exiting...");
                }
                // Handle macOS dock icon click to reopen window
                #[cfg(target_os = "macos")]
                tauri::RunEvent::Reopen { .. } => {
                    let _ = modules::lightweight::exit_lightweight_mode(app_handle);
                }
                _ => {}
            }
        });
}

#[cfg(unix)]
fn ensure_cli_symlinks() {
    if let (Some(home), Ok(current_exe)) = (dirs::home_dir(), std::env::current_exe()) {
        let local_bin = home.join(".local/bin");
        if !local_bin.exists() {
            let _ = std::fs::create_dir_all(&local_bin);
        }
        for name in &["antigravity-manager", "am", "antigravity-tools", "at"] {
            let link = local_bin.join(name);
            if !link.exists() {
                let _ = std::os::unix::fs::symlink(&current_exe, &link);
            }
        }
    }
}

#[cfg(not(unix))]
fn ensure_cli_symlinks() {}
