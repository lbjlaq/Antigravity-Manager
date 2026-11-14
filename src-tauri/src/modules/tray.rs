use crate::models::quota::ModelQuota;
use crate::modules;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
#[cfg(not(target_os = "android"))]
use tauri::{
    image::Image,
    menu::{CheckMenuItem, MenuItem, PredefinedMenuItem},
    tray::{MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Listener, Manager,
};
#[cfg(all(not(target_os = "android"), not(target_os = "macos")))]
use tauri::{menu::Menu, tray::MouseButton};

static MINI_VIEW_ACTIVE: AtomicBool = AtomicBool::new(false);
static MINI_VIEW_PINNED: AtomicBool = AtomicBool::new(false);
static LAST_BLUR_TIME: AtomicI64 = AtomicI64::new(0);
static TRAY_SELECTED_ACCOUNT_ID: std::sync::RwLock<Option<String>> = std::sync::RwLock::new(None);

pub fn set_tray_selected_account_id(account_id: Option<String>) {
    if let Ok(mut lock) = TRAY_SELECTED_ACCOUNT_ID.write() {
        *lock = account_id;
    }
}

pub fn get_tray_selected_account_id() -> Option<String> {
    TRAY_SELECTED_ACCOUNT_ID
        .read()
        .ok()
        .and_then(|lock| lock.clone())
}

pub fn is_mini_view_active() -> bool {
    MINI_VIEW_ACTIVE.load(Ordering::Relaxed)
}

pub fn is_mini_view_pinned() -> bool {
    MINI_VIEW_PINNED.load(Ordering::Relaxed)
}

pub fn set_mini_view_active(active: bool) {
    MINI_VIEW_ACTIVE.store(active, Ordering::Relaxed);
}

pub fn set_mini_view_pinned(pinned: bool) {
    MINI_VIEW_PINNED.store(pinned, Ordering::Relaxed);
}

pub fn record_blur() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    LAST_BLUR_TIME.store(now, Ordering::Relaxed);
}

pub fn is_recent_blur() -> bool {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let last = LAST_BLUR_TIME.load(Ordering::Relaxed);
    (now - last) < 300
}

pub fn ensure_mini_window(app: &tauri::AppHandle) -> Result<tauri::WebviewWindow, String> {
    #[cfg(target_os = "android")]
    {
        let _ = app;
        return Err("Android 不支持 mini 窗口".to_string());
    }

    #[cfg(not(target_os = "android"))]
    {
        if let Some(window) = app.get_webview_window("mini") {
            return Ok(window);
        }

        let window_config = app
            .config()
            .app
            .windows
            .iter()
            .find(|w| w.label == "mini")
            .cloned()
            .ok_or_else(|| "未在 tauri.conf.json 中找到 label 为 'mini' 的窗口配置".to_string())?;

        let window = tauri::WebviewWindowBuilder::from_config(app, &window_config)
            .map_err(|e| format!("构建 mini 窗口失败: {}", e))?
            .build()
            .map_err(|e| format!("启动 mini 窗口失败: {}", e))?;

        Ok(window)
    }
}

/// 隐藏/收起 Mini 悬浮视窗并清理激活状态，在 macOS 下按需恢复主窗口 ActivationPolicy
pub fn hide_mini_window(app: &tauri::AppHandle) {
    #[cfg(target_os = "android")]
    {
        let _ = app;
    }

    #[cfg(not(target_os = "android"))]
    {
        if let Some(window) = app.get_webview_window("mini") {
            let _ = window.hide();
        }
        set_mini_view_active(false);

        #[cfg(target_os = "macos")]
        {
            if let Some(main) = app.get_webview_window("main") {
                let is_open =
                    main.is_visible().unwrap_or(false) || main.is_minimized().unwrap_or(false);
                if is_open {
                    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
                }
            }
        }
    }
}

/// 处理系统托盘点击事件：实现跨平台现代 Popover（类似腾讯 Lemon / Raycast）
/// 默认失焦自动收起（除非用户点击图钉进行固定）
pub fn handle_tray_click(app: &tauri::AppHandle, rect: Option<tauri::Rect>) {
    #[cfg(target_os = "android")]
    {
        let _ = (app, rect);
    }

    #[cfg(not(target_os = "android"))]
    {
        // 1. 最近失焦防抖过滤：若由于点击托盘导致窗口刚失去焦点 (<300ms)，视作自然关闭，避免误重开抖动
        if is_recent_blur() {
            return;
        }

        if is_mini_view_active() {
            hide_mini_window(app);
            return;
        }

        // 2. 唤醒并呈现 mini 悬浮窗（完全独立于主控制台窗口）
        let window = match ensure_mini_window(app) {
            Ok(w) => w,
            Err(e) => {
                tracing::error!("[Tray] 无法获取或重建 mini 悬浮窗: {}", e);
                return;
            }
        };

        const MINI_WIDTH: f64 = 360.0;
        const MINI_HEIGHT: f64 = 360.0;

        let monitor = window.current_monitor().ok().flatten();
        let (mon_x, mon_y, mon_w, mon_h, scale_factor) = if let Some(ref m) = monitor {
            let pos = m.position();
            let sz = m.size();
            let sf = m.scale_factor();
            (
                pos.x as f64,
                pos.y as f64,
                sz.width as f64,
                sz.height as f64,
                sf,
            )
        } else {
            (0.0, 0.0, 1920.0, 1080.0, 1.0)
        };

        let physical_w = MINI_WIDTH * scale_factor;
        let physical_h = MINI_HEIGHT * scale_factor;

        let (mut win_x, mut win_y) = if let Some(r) = rect {
            let (tray_x, tray_y) = match r.position {
                tauri::Position::Physical(p) => (p.x as f64, p.y as f64),
                tauri::Position::Logical(p) => (p.x * scale_factor, p.y * scale_factor),
            };
            let (tray_w, tray_h) = match r.size {
                tauri::Size::Physical(s) => (s.width as f64, s.height as f64),
                tauri::Size::Logical(s) => (s.width * scale_factor, s.height * scale_factor),
            };

            // 水平居中对齐托盘图标
            let x = tray_x + (tray_w / 2.0) - (physical_w / 2.0);

            // 垂直定位：跨平台适配（判断处于屏幕上半部分如 macOS 顶部菜单栏/Linux 顶栏，还是下半部分如 Windows 底部任务栏）
            let y = if tray_y > mon_y + (mon_h / 2.0) {
                // Windows 底部任务栏 / Linux 底部 Dock：窗口向上弹出
                tray_y - physical_h - (4.0 * scale_factor)
            } else {
                // macOS 顶部菜单栏 / Windows 顶部任务栏：窗口向下弹出
                tray_y + tray_h + (4.0 * scale_factor)
            };

            (x, y)
        } else {
            // 无托盘坐标时，默认优雅居中于主屏顶部下方
            (
                mon_x + (mon_w - physical_w) / 2.0,
                mon_y + 40.0 * scale_factor,
            )
        };

        // 四边屏幕边缘安全吸附限制（留出 8px 边距）
        let margin = 8.0 * scale_factor;
        if win_x < mon_x + margin {
            win_x = mon_x + margin;
        }
        if win_x + physical_w > mon_x + mon_w - margin {
            win_x = mon_x + mon_w - physical_w - margin;
        }
        if win_y < mon_y + margin {
            win_y = mon_y + margin;
        }
        if win_y + physical_h > mon_y + mon_h - margin {
            win_y = mon_y + mon_h - physical_h - margin;
        }

        let _ = window.set_decorations(false);
        let _ = window.set_resizable(false);
        let _ = window.set_shadow(true);
        let _ = window.set_always_on_top(is_mini_view_pinned());
        let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize::new(
            MINI_WIDTH,
            MINI_HEIGHT,
        )));
        let _ = window.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(
            win_x as i32,
            win_y as i32,
        )));

        // macOS 专属策略：唤起 Mini 视窗时，始终切换为 Accessory 附属模式。
        // 关键点：当主控制台处于 Regular 模式（在后台、另一个桌面 Space、或已最小化到 Dock）时，
        // 若在 Regular 模式下直接调用 window.set_focus()，AppKit 的 activateIgnoringOtherApps:YES
        // 会将整个应用程序激活并将其所有可见窗口（包括主控制台）强制提至前台或触发跨 Space 跳转。
        // 切至 Accessory 模式后，系统仅将 Mini 独立呈现置顶，绝不干扰或强拉后台的主控制台。
        #[cfg(target_os = "macos")]
        {
            let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
        }

        let _ = window.show();
        let _ = window.set_focus();
        set_mini_view_active(true);
        let _ = app.emit("tray://show-mini-view", ());
    }
}

#[derive(Default)]
struct KeyModelQuotas {
    claude: Option<i32>,
    gemini_pro: Option<i32>,
    gemini_flash: Option<i32>,
    gemini_image: Option<i32>,
}

/// 智能提取核心模型的剩余配额百分比（首选精准旗舰型号，未匹配时回退通配分类）
fn extract_key_model_quotas(models: &[ModelQuota]) -> KeyModelQuotas {
    let mut claude: Option<i32> = None;
    let mut gemini_pro: Option<i32> = None;
    let mut gemini_flash: Option<i32> = None;
    let mut gemini_image: Option<i32> = None;

    // 1. 首选匹配当前主力核心型号
    for m in models {
        let name = m.name.trim().to_lowercase();
        if claude.is_none()
            && (name == "claude-sonnet-4-6"
                || name == "claude-opus-4-6-thinking"
                || name == "claude-sonnet-4-5")
        {
            claude = Some(m.percentage);
        }
        if gemini_pro.is_none()
            && (name == "gemini-pro-agent"
                || name == "gemini-3.1-pro-high"
                || name == "gemini-3-pro-high"
                || name == "gemini-3.1-pro")
        {
            gemini_pro = Some(m.percentage);
        }
        if gemini_flash.is_none()
            && (name == "gemini-3.8-flash-high"
                || name == "gemini-3.8-flash"
                || name == "gemini-3-flash-agent"
                || name == "gemini-3-flash"
                || name == "gemini-3.5-flash")
        {
            gemini_flash = Some(m.percentage);
        }
        if gemini_image.is_none()
            && (name == "gemini-3.1-flash-image" || name == "gemini-3-pro-image")
        {
            gemini_image = Some(m.percentage);
        }
    }

    // 2. 类别通配回退匹配（应对未来模型版本迭代）
    for m in models {
        let name = m.name.trim().to_lowercase();
        let is_gemini = name.starts_with("gemini-");
        let is_image = (is_gemini && name.contains("image"))
            || name.starts_with("image")
            || name.starts_with("imagen");

        if gemini_image.is_none() && is_image {
            gemini_image = Some(m.percentage);
        } else if is_gemini && name.contains("flash") {
            if gemini_flash.is_none() {
                gemini_flash = Some(m.percentage);
            }
        } else if is_gemini && name.contains("pro") {
            if gemini_pro.is_none() {
                gemini_pro = Some(m.percentage);
            }
        } else if claude.is_none()
            && (name.contains("claude")
                || name.contains("sonnet")
                || name.contains("opus")
                || name.contains("haiku"))
        {
            claude = Some(m.percentage);
        }
    }

    KeyModelQuotas {
        claude,
        gemini_pro,
        gemini_flash,
        gemini_image,
    }
}

pub fn create_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    #[cfg(target_os = "android")]
    {
        let _ = app;
        return Ok(());
    }

    #[cfg(not(target_os = "android"))]
    {
        // 1. Load config to get language settings
        #[cfg(not(target_os = "macos"))]
        let config = modules::load_app_config().unwrap_or_default();
        #[cfg(not(target_os = "macos"))]
        let texts = modules::i18n::get_tray_texts(&config.language);

        // 2. Load icon (macOS uses Template Image `tray-icon.png`, Windows/Linux uses full-color `icon.png`)
        #[cfg(target_os = "macos")]
        let icon_bytes: &[u8] = include_bytes!("../../icons/tray-icon.png");
        #[cfg(not(target_os = "macos"))]
        let icon_bytes: &[u8] = include_bytes!("../../icons/icon.png");

        let img = image::load_from_memory(icon_bytes)
            .map_err(|e| {
                tauri::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))
            })?
            .to_rgba8();
        let (width, height) = img.dimensions();
        let icon = Image::new_owned(img.into_raw(), width, height);

        // 3. Define streamlined secondary menu items
        #[cfg(not(target_os = "macos"))]
        let menu = {
            let loading_text = format!("{}: ...", texts.current);
            let info_user =
                MenuItem::with_id(app, "info_user", &loading_text, false, None::<&str>)?;

            let switch_next =
                MenuItem::with_id(app, "switch_next", &texts.switch_next, true, None::<&str>)?;
            let refresh_curr = MenuItem::with_id(
                app,
                "refresh_curr",
                &texts.refresh_current,
                true,
                None::<&str>,
            )?;

            let show_i = MenuItem::with_id(app, "show", &texts.show_window, true, None::<&str>)?;
            let mini_i = MenuItem::with_id(app, "mini_view", &texts.mini_view, true, None::<&str>)?;
            let lightweight_i = CheckMenuItem::with_id(
                app,
                "toggle_lightweight",
                &texts.lightweight_mode,
                true,
                config.lightweight_mode,
                None::<&str>,
            )?;
            let quit_i = MenuItem::with_id(app, "quit", &texts.quit, true, None::<&str>)?;

            let sep1 = PredefinedMenuItem::separator(app)?;
            let sep2 = PredefinedMenuItem::separator(app)?;
            let sep3 = PredefinedMenuItem::separator(app)?;

            Menu::with_items(
                app,
                &[
                    &info_user,
                    &sep1,
                    &switch_next,
                    &refresh_curr,
                    &sep2,
                    &show_i,
                    &mini_i,
                    &lightweight_i,
                    &sep3,
                    &quit_i,
                ],
            )?
        };

        // 4. Build tray icon
        #[cfg(target_os = "macos")]
        let builder = TrayIconBuilder::with_id("main")
            .icon(icon)
            .tooltip("Antigravity Manager")
            .icon_as_template(true);

        #[cfg(not(target_os = "macos"))]
        let builder = TrayIconBuilder::with_id("main")
            .menu(&menu)
            .icon(icon)
            .tooltip("Antigravity Manager")
            .icon_as_template(false)
            .show_menu_on_left_click(false);

        let _ = builder
            .on_tray_icon_event(|tray, event| match event {
                TrayIconEvent::Click {
                    #[cfg(not(target_os = "macos"))]
                    button,
                    #[cfg(target_os = "macos")]
                        button: _,
                    button_state,
                    rect,
                    ..
                } => {
                    #[cfg(target_os = "macos")]
                    let should_handle = button_state == MouseButtonState::Up;
                    #[cfg(not(target_os = "macos"))]
                    let should_handle =
                        button == MouseButton::Left && button_state == MouseButtonState::Up;

                    if should_handle {
                        let app = tray.app_handle();
                        let click_action = crate::modules::load_app_config()
                            .map(|c| c.tray_click_action)
                            .unwrap_or_else(|_| "mini".to_string());

                        if click_action == "main" {
                            hide_mini_window(app);
                            let _ = crate::modules::lightweight::exit_lightweight_mode(app);
                            let _ = app.emit("tray://show-window", ());
                        } else {
                            handle_tray_click(app, Some(rect));
                        }
                    }
                }
                _ => {}
            })
            .on_menu_event(move |app, event| {
                let app_handle = app.clone();
                match event.id().as_ref() {
                    "show" => {
                        let _ = modules::lightweight::exit_lightweight_mode(&app_handle);
                        set_mini_view_active(false);
                        let _ = app_handle.emit("tray://show-window", ());
                    }
                    "mini_view" => {
                        handle_tray_click(&app_handle, None);
                    }
                    "toggle_lightweight" => {
                        tauri::async_runtime::spawn(async move {
                            if let Ok(mut config) = modules::load_app_config() {
                                config.lightweight_mode = !config.lightweight_mode;
                                if let Err(e) = modules::save_app_config(&config) {
                                    modules::logger::log_error(&format!(
                                        "Failed to toggle lightweight mode: {}",
                                        e
                                    ));
                                } else {
                                    modules::logger::log_info(&format!(
                                        "Lightweight mode toggled to {}",
                                        config.lightweight_mode
                                    ));
                                    let _ = app_handle.emit("config://updated", ());
                                }
                            }
                        });
                    }
                    "quit" => {
                        #[cfg(target_os = "linux")]
                        crate::linux_display_guard::mark_user_initiated_exit();

                        let app_handle = app.clone();
                        tauri::async_runtime::spawn(async move {
                            tracing::info!("[Tray] 退出网关触发，开始全面清理服务与端口...");

                            let state =
                                app_handle.state::<crate::commands::proxy::ProxyServiceState>();
                            let cf_state = app_handle
                                .state::<crate::commands::cloudflared::CloudflaredState>();

                            // 1. 终止 cloudflared 隧道子进程
                            let _ = tokio::time::timeout(
                                std::time::Duration::from_millis(500),
                                cf_state.stop(),
                            )
                            .await;

                            // 2. 停止 Admin Server（关闭 TCP 监听器和所有活动连接）
                            if let Ok(mut lock) = tokio::time::timeout(
                                std::time::Duration::from_millis(1000),
                                state.admin_server.write(),
                            )
                            .await
                            {
                                if let Some(admin) = lock.take() {
                                    admin.stop().await;
                                }
                            }

                            // 3. 停止业务代理实例及后台任务
                            if let Ok(mut lock) = tokio::time::timeout(
                                std::time::Duration::from_millis(1000),
                                state.instance.write(),
                            )
                            .await
                            {
                                if let Some(inst) = lock.take() {
                                    let _ = tokio::time::timeout(
                                        std::time::Duration::from_millis(500),
                                        inst.token_manager.graceful_shutdown(
                                            std::time::Duration::from_millis(400),
                                        ),
                                    )
                                    .await;
                                    inst.axum_server.set_running(false).await;
                                    inst.axum_server.stop();
                                }
                            }

                            // 4. 给予底层套接字彻底注销的微小缓冲，然后干净退出进程
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                            // `process::exit` 会绕过 Tauri 事件循环，签名缓存必须在退出前主动 flush。
                            crate::proxy::signature_cache::flush_on_exit();
                            std::process::exit(0);
                        });
                    }
                    "refresh_curr" => {
                        tauri::async_runtime::spawn(async move {
                            if let Ok(Some(account_id)) = modules::get_current_account_id() {
                                let _ = app_handle.emit("tray://refresh-current", ());
                                if let Ok(mut account) = modules::load_account(&account_id) {
                                    match modules::account::fetch_quota_with_retry(&mut account)
                                        .await
                                    {
                                        Ok(quota) => {
                                            let _ =
                                                modules::update_account_quota(&account.id, quota);
                                            update_tray_menus(&app_handle);
                                        }
                                        Err(e) => {
                                            modules::logger::log_error(&format!(
                                                "Tray refresh failed: {}",
                                                e
                                            ));
                                        }
                                    }
                                }
                            }
                        });
                    }
                    "switch_next" => {
                        tauri::async_runtime::spawn(async move {
                            if let Ok(accounts) = modules::list_accounts() {
                                if accounts.is_empty() {
                                    return;
                                }

                                // 托盘「切换下一个」同样必须跳过封禁/禁用/验证中的账号，
                                // 否则会把官方客户端切到一个确定不可用的账号上。
                                let candidates: Vec<_> = accounts
                                    .iter()
                                    .filter(|a| {
                                        !a.disabled
                                            && !a.proxy_disabled
                                            && !a.banned
                                            && !a.validation_blocked
                                            && !a.quota.as_ref().map_or(false, |q| q.is_forbidden)
                                    })
                                    .collect();
                                if candidates.is_empty() {
                                    return;
                                }

                                let current_id = modules::get_current_account_id().unwrap_or(None);
                                let next_account = if let Some(curr) = current_id {
                                    match candidates.iter().position(|a| a.id == curr) {
                                        Some(idx) => &candidates[(idx + 1) % candidates.len()],
                                        None => &candidates[0],
                                    }
                                } else {
                                    &candidates[0]
                                };

                                let integration = crate::modules::integration::DesktopIntegration {
                                    app_handle: app_handle.clone(),
                                };
                                if let Ok(_) =
                                    modules::switch_account(&next_account.id, None, &integration)
                                        .await
                                {
                                    let _ = app_handle
                                        .emit("tray://account-switched", next_account.id.clone());
                                    update_tray_menus(&app_handle);
                                }
                            }
                        });
                    }
                    _ => {}
                }
            })
            .build(app)?;

        // Update status once on initialization
        let handle = app.clone();
        tauri::async_runtime::spawn(async move {
            update_tray_menus(&handle);
        });

        // Listen for config update events
        let handle = app.clone();
        app.listen("config://updated", move |_event| {
            modules::logger::log_info("Configuration updated, refreshing tray menu");
            update_tray_menus(&handle);
        });

        Ok(())
    }
}

/// Helper function to update tray menu
pub fn update_tray_menus(app: &tauri::AppHandle) {
    #[cfg(target_os = "android")]
    {
        let _ = app;
    }

    #[cfg(not(target_os = "android"))]
    {
        #[cfg(target_os = "linux")]
        if crate::linux_display_guard::is_display_disconnected() {
            return;
        }

        let app_clone = app.clone();
        tauri::async_runtime::spawn(async move {
            // Read config to get language
            let config = modules::load_app_config().unwrap_or_default();
            let texts = modules::i18n::get_tray_texts(&config.language);

            // Get current account info (prioritizing user-selected account in MiniView/Tab, fallback to active client account)
            let selected_id = get_tray_selected_account_id();
            let current = selected_id.or_else(|| modules::get_current_account_id().unwrap_or(None));

            let mut menu_lines = Vec::new();
            let mut user_text = format!("{}: {}", texts.current, texts.no_account);
            let mut tooltip_text = format!(
                "Antigravity Manager\n{}: {}",
                texts.current, texts.no_account
            );
            #[allow(unused_mut)]
            let mut macos_title: Option<String> = None;

            if let Some(id) = current {
                if let Ok(account) = modules::load_account(&id) {
                    user_text = format!("{}: {}", texts.current, account.email);

                    let email_display = if account.email.chars().count() > 28 {
                        let truncated: String = account.email.chars().take(25).collect();
                        format!("{}...", truncated)
                    } else {
                        account.email.clone()
                    };

                    if let Some(q) = account.quota {
                        if q.is_forbidden {
                            let forbidden_msg = format!("🚫 {}", texts.forbidden);
                            menu_lines.push(forbidden_msg.clone());
                            tooltip_text = format!(
                                "Antigravity Manager ({})\n{}",
                                email_display, forbidden_msg
                            );
                            macos_title = Some("🚫".to_string());
                        } else {
                            let key_quotas = extract_key_model_quotas(&q.models);

                            if let Some(c) = key_quotas.claude {
                                menu_lines.push(format!("Claude: {}%", c));
                            }
                            if let Some(gp) = key_quotas.gemini_pro {
                                menu_lines.push(format!("Gemini Pro: {}%", gp));
                            }
                            if let Some(gi) = key_quotas.gemini_image {
                                menu_lines.push(format!("Gemini Image: {}%", gi));
                            }
                            if let Some(gf) = key_quotas.gemini_flash {
                                menu_lines.push(format!("Gemini Flash: {}%", gf));
                            }

                            if menu_lines.is_empty() {
                                menu_lines.push(texts.unknown_quota.clone());
                            }

                            // Formulate concise tooltip (strictly within Windows 128-char limit)
                            let mut tooltip_parts = Vec::new();
                            if let Some(c) = key_quotas.claude {
                                tooltip_parts.push(format!("Claude: {}%", c));
                            }
                            if let Some(gp) = key_quotas.gemini_pro {
                                tooltip_parts.push(format!("Gemini: {}%", gp));
                            }
                            if let Some(gi) = key_quotas.gemini_image {
                                tooltip_parts.push(format!("Image: {}%", gi));
                            }

                            let quota_summary = if tooltip_parts.is_empty() {
                                texts.unknown_quota.clone()
                            } else {
                                tooltip_parts.join(" | ")
                            };

                            tooltip_text = format!(
                                "Antigravity Manager ({})\n{}",
                                email_display, quota_summary
                            );

                            // macOS menu bar title (concise for status bar space)
                            macos_title = match (key_quotas.claude, key_quotas.gemini_pro) {
                                (Some(c), Some(g)) => Some(format!("C:{}% G:{}%", c, g)),
                                (Some(c), None) => Some(format!("C:{}%", c)),
                                (None, Some(g)) => Some(format!("G:{}%", g)),
                                _ => None,
                            };
                        }
                    } else {
                        menu_lines.push(texts.unknown_quota.clone());
                        tooltip_text = format!(
                            "Antigravity Manager ({})\n{}",
                            email_display, texts.unknown_quota
                        );
                    }
                } else {
                    user_text = format!("{}: Error", texts.current);
                    menu_lines.push(format!("{}: --", texts.quota));
                    tooltip_text = format!("Antigravity Manager\n{}: Error", texts.current);
                }
            } else {
                menu_lines.push(texts.unknown_quota.clone());
            };

            // [Fix #3757] 统一将所有 GTK/AppKit 系统托盘和菜单项创建调度到操作系统原生主线程执行
            // 彻底杜绝在 Linux Tokio 工作线程上跨线程直接调用 GTK 导致并发死锁或断言崩溃 _exit(1)
            let app_handle_for_main = app_clone.clone();
            let _ = app_clone.run_on_main_thread(move || {
                #[cfg(target_os = "linux")]
                if crate::linux_display_guard::is_display_disconnected() {
                    return;
                }

                // Always update tray tooltip and macOS title (regardless of menu rebuild status)
                if let Some(tray) = app_handle_for_main.tray_by_id("main") {
                    let _ = tray.set_tooltip(Some(tooltip_text));
                    #[cfg(target_os = "macos")]
                    let _ = tray.set_title(macos_title);
                }

                // Rebuild menu items (dynamic quotas + shortcuts) safely on the UI main thread
                let info_user = MenuItem::with_id(
                    &app_handle_for_main,
                    "info_user",
                    &user_text,
                    false,
                    None::<&str>,
                );

                let mut quota_items = Vec::new();
                for (i, line) in menu_lines.iter().enumerate() {
                    let item = MenuItem::with_id(
                        &app_handle_for_main,
                        format!("info_quota_{}", i),
                        line,
                        false,
                        None::<&str>,
                    );
                    if let Ok(item) = item {
                        quota_items.push(item);
                    }
                }

                let switch_next = MenuItem::with_id(
                    &app_handle_for_main,
                    "switch_next",
                    &texts.switch_next,
                    true,
                    None::<&str>,
                );
                let refresh_curr = MenuItem::with_id(
                    &app_handle_for_main,
                    "refresh_curr",
                    &texts.refresh_current,
                    true,
                    None::<&str>,
                );

                let show_i = MenuItem::with_id(
                    &app_handle_for_main,
                    "show",
                    &texts.show_window,
                    true,
                    None::<&str>,
                );
                let mini_i = MenuItem::with_id(
                    &app_handle_for_main,
                    "mini_view",
                    &texts.mini_view,
                    true,
                    None::<&str>,
                );
                let lightweight_i = CheckMenuItem::with_id(
                    &app_handle_for_main,
                    "toggle_lightweight",
                    &texts.lightweight_mode,
                    true,
                    config.lightweight_mode,
                    None::<&str>,
                );
                let quit_i = MenuItem::with_id(
                    &app_handle_for_main,
                    "quit",
                    &texts.quit,
                    true,
                    None::<&str>,
                );

                if let (Ok(i_u), Ok(s_n), Ok(r_c), Ok(s), Ok(m), Ok(l), Ok(q)) = (
                    info_user,
                    switch_next,
                    refresh_curr,
                    show_i,
                    mini_i,
                    lightweight_i,
                    quit_i,
                ) {
                    let sep1 = PredefinedMenuItem::separator(&app_handle_for_main).ok();
                    let sep2 = PredefinedMenuItem::separator(&app_handle_for_main).ok();
                    let sep3 = PredefinedMenuItem::separator(&app_handle_for_main).ok();

                    let mut items: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> = vec![&i_u];
                    for item in &quota_items {
                        items.push(item);
                    }

                    if let Some(ref s1) = sep1 {
                        items.push(s1);
                    }
                    items.push(&s_n);
                    items.push(&r_c);
                    if let Some(ref s2) = sep2 {
                        items.push(s2);
                    }
                    items.push(&s);
                    items.push(&m);
                    items.push(&l);
                    if let Some(ref s3) = sep3 {
                        items.push(s3);
                    }
                    items.push(&q);

                    #[cfg(not(target_os = "macos"))]
                    if let Ok(menu) = Menu::with_items(&app_handle_for_main, &items) {
                        if let Some(tray) = app_handle_for_main.tray_by_id("main") {
                            let _ = tray.set_menu(Some(menu));
                        }
                    }
                }
            });
        });
    }
}
