use crate::modules;
use tauri::{
    image::Image,
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
    Emitter, Listener, Manager,
};

pub fn create_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    // 1. Load config to get language settings
    let config = modules::load_app_config().unwrap_or_default();
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

    // 3. Define menu items (using translated texts)
    // Status area
    let loading_text = format!("{}: ...", texts.current);
    let quota_text = format!("{}: --", texts.quota);
    let info_user = MenuItem::with_id(app, "info_user", &loading_text, false, None::<&str>)?;
    let info_quota = MenuItem::with_id(app, "info_quota", &quota_text, false, None::<&str>)?;

    // Quick actions area
    let switch_next =
        MenuItem::with_id(app, "switch_next", &texts.switch_next, true, None::<&str>)?;
    let refresh_curr = MenuItem::with_id(
        app,
        "refresh_curr",
        &texts.refresh_current,
        true,
        None::<&str>,
    )?;

    // System functions
    let show_i = MenuItem::with_id(app, "show", &texts.show_window, true, None::<&str>)?;
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

    // 4. Build menu
    let menu = Menu::with_items(
        app,
        &[
            &info_user,
            &info_quota,
            &sep1,
            &switch_next,
            &refresh_curr,
            &sep2,
            &show_i,
            &lightweight_i,
            &sep3,
            &quit_i,
        ],
    )?;

    // 5. Build tray icon
    let _ = TrayIconBuilder::with_id("main")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .icon(icon)
        .icon_as_template(cfg!(target_os = "macos"))
        .on_menu_event(move |app, event| {
            let app_handle = app.clone();
            match event.id().as_ref() {
                "show" => {
                    let _ = modules::lightweight::exit_lightweight_mode(&app_handle);
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
                    let app_handle = app.clone();
                    tauri::async_runtime::spawn(async move {
                        tracing::info!("[Tray] 退出网关触发，开始全面清理服务与端口...");

                        let state = app_handle.state::<crate::commands::proxy::ProxyServiceState>();
                        let cf_state =
                            app_handle.state::<crate::commands::cloudflared::CloudflaredState>();

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
                                    inst.token_manager
                                        .graceful_shutdown(std::time::Duration::from_millis(400)),
                                )
                                .await;
                                inst.axum_server.set_running(false).await;
                                inst.axum_server.stop();
                            }
                        }

                        // 4. 给予底层套接字彻底注销的微小缓冲，然后干净退出进程
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        std::process::exit(0);
                    });
                }
                "refresh_curr" => {
                    // Execute refresh asynchronously
                    tauri::async_runtime::spawn(async move {
                        if let Ok(Some(account_id)) = modules::get_current_account_id() {
                            // Notify frontend to start
                            let _ = app_handle.emit("tray://refresh-current", ());

                            // Execute refresh logic
                            if let Ok(mut account) = modules::load_account(&account_id) {
                                // Use shared logic from modules::account
                                match modules::account::fetch_quota_with_retry(&mut account).await {
                                    Ok(quota) => {
                                        // Save
                                        let _ = modules::update_account_quota(&account.id, quota);
                                        // Update tray display
                                        update_tray_menus(&app_handle);
                                    }
                                    Err(e) => {
                                        // Error handling, log only
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
                        // 1. Get all accounts
                        if let Ok(accounts) = modules::list_accounts() {
                            if accounts.is_empty() {
                                return;
                            }

                            let current_id = modules::get_current_account_id().unwrap_or(None);
                            let next_account = if let Some(curr) = current_id {
                                let idx = accounts.iter().position(|a| a.id == curr).unwrap_or(0);
                                let next_idx = (idx + 1) % accounts.len();
                                &accounts[next_idx]
                            } else {
                                &accounts[0]
                            };

                            // 2. Switch
                            let integration = crate::modules::integration::DesktopIntegration {
                                app_handle: app_handle.clone(),
                            };
                            if let Ok(_) =
                                modules::switch_account(&next_account.id, None, &integration).await
                            {
                                // 3. Notify frontend
                                let _ = app_handle
                                    .emit("tray://account-switched", next_account.id.clone());
                                // 4. Update tray
                                update_tray_menus(&app_handle);
                            }
                        }
                    });
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                ..
            } = event
            {
                let app = tray.app_handle();
                let _ = modules::lightweight::exit_lightweight_mode(app);
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

/// Helper function to update tray menu
pub fn update_tray_menus(app: &tauri::AppHandle) {
    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        // Read config to get language
        let config = modules::load_app_config().unwrap_or_default();
        let texts = modules::i18n::get_tray_texts(&config.language);

        // Get current account info
        let current = modules::get_current_account_id().unwrap_or(None);

        let mut menu_lines = Vec::new();
        let mut user_text = format!("{}: {}", texts.current, texts.no_account);

        if let Some(id) = current {
            if let Ok(account) = modules::load_account(&id) {
                user_text = format!("{}: {}", texts.current, account.email);

                if let Some(q) = account.quota {
                    if q.is_forbidden {
                        menu_lines.push(format!("🚫 {}", texts.forbidden));
                    } else {
                        // Extract the 3 specified models
                        let mut gemini_high = 0;
                        let mut gemini_image = 0;
                        let mut claude = 0;

                        // Use strict matching, consistent with frontend
                        for m in q.models {
                            let name = m.name.to_lowercase();
                            if name == "gemini-3.1-pro-high" || name == "gemini-3-pro-high" {
                                gemini_high = m.percentage;
                            }
                            if name == "gemini-3.1-flash-image" || name == "gemini-3-pro-image" {
                                gemini_image = m.percentage;
                            }
                            if name == "claude-sonnet-4-6" || name == "claude-sonnet-4-5" {
                                claude = m.percentage;
                            }
                        }

                        menu_lines.push(format!("Gemini High: {}%", gemini_high));
                        menu_lines.push(format!("Gemini Image: {}%", gemini_image));
                        menu_lines.push(format!("Claude 4.5: {}%", claude));
                    }
                } else {
                    menu_lines.push(texts.unknown_quota.clone());
                }
            } else {
                user_text = format!("{}: Error", texts.current);
                menu_lines.push(format!("{}: --", texts.quota));
            }
        } else {
            menu_lines.push(texts.unknown_quota.clone());
        };

        // Rebuild menu items
        let info_user = MenuItem::with_id(&app_clone, "info_user", &user_text, false, None::<&str>);

        // Dynamically create quota items
        let mut quota_items = Vec::new();
        for (i, line) in menu_lines.iter().enumerate() {
            let item = MenuItem::with_id(
                &app_clone,
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
            &app_clone,
            "switch_next",
            &texts.switch_next,
            true,
            None::<&str>,
        );
        let refresh_curr = MenuItem::with_id(
            &app_clone,
            "refresh_curr",
            &texts.refresh_current,
            true,
            None::<&str>,
        );

        let show_i = MenuItem::with_id(&app_clone, "show", &texts.show_window, true, None::<&str>);
        let lightweight_i = CheckMenuItem::with_id(
            &app_clone,
            "toggle_lightweight",
            &texts.lightweight_mode,
            true,
            config.lightweight_mode,
            None::<&str>,
        );
        let quit_i = MenuItem::with_id(&app_clone, "quit", &texts.quit, true, None::<&str>);

        if let (Ok(i_u), Ok(s_n), Ok(r_c), Ok(s), Ok(l), Ok(q)) = (
            info_user,
            switch_next,
            refresh_curr,
            show_i,
            lightweight_i,
            quit_i,
        ) {
            let sep1 = PredefinedMenuItem::separator(&app_clone).ok();
            let sep2 = PredefinedMenuItem::separator(&app_clone).ok();
            let sep3 = PredefinedMenuItem::separator(&app_clone).ok();

            let mut items: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> = vec![&i_u];
            // Add dynamic quota items
            for item in &quota_items {
                items.push(item);
            }

            if let Some(ref s) = sep1 {
                items.push(s);
            }
            items.push(&s_n);
            items.push(&r_c);
            if let Some(ref s) = sep2 {
                items.push(s);
            }
            items.push(&s);
            items.push(&l);
            if let Some(ref s) = sep3 {
                items.push(s);
            }
            items.push(&q);

            if let Ok(menu) = Menu::with_items(&app_clone, &items) {
                if let Some(tray) = app_clone.tray_by_id("main") {
                    let _ = tray.set_menu(Some(menu));
                }
            }
        }
    });
}
