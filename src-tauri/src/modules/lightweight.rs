use tauri::{AppHandle, Manager, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_window_state::{AppHandleExt, StateFlags, WindowExt};

/// 尝试获取或重新构建主窗口。
/// 如果主窗口已被轻量模式销毁，将基于 tauri.conf.json 的配置动态重建，并恢复窗口位置和图标。
pub fn ensure_main_window(app: &AppHandle) -> Result<WebviewWindow, String> {
    if let Some(window) = app.get_webview_window("main") {
        return Ok(window);
    }

    tracing::info!("[Lightweight] 主窗口当前未运行，正在从配置重建 WebviewWindow('main')...");

    // 1. 从应用配置中查找 label 为 "main" 的窗口配置
    let window_config = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == "main")
        .cloned()
        .ok_or_else(|| "未在 tauri.conf.json 中找到 label 为 'main' 的窗口配置".to_string())?;

    // 2. 动态创建窗口
    let window = WebviewWindowBuilder::from_config(app, &window_config)
        .map_err(|e| format!("构建主窗口失败: {}", e))?
        .build()
        .map_err(|e| format!("启动主窗口失败: {}", e))?;

    // 3. 为非 macOS 系统显式设置应用图标（确保 Win32 任务栏图标恢复）
    #[cfg(not(target_os = "macos"))]
    {
        let icon_bytes: &[u8] = include_bytes!("../../icons/icon.png");
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

    // 4. Linux 透明窗口兼容性处理
    #[cfg(target_os = "linux")]
    {
        if !crate::is_wayland_session() {
            if let Ok(gtk_window) = window.gtk_window() {
                use gtk::prelude::WidgetExt;
                if let Some(screen) = gtk_window.screen() {
                    if let Some(visual) = screen.system_visual() {
                        gtk_window.set_visual(Some(&visual));
                    }
                }
            }
        }
    }

    // 5. 恢复窗口状态（记忆的位置和尺寸）
    let _ = window.restore_state(StateFlags::all().difference(StateFlags::VISIBLE));

    tracing::info!("[Lightweight] 主窗口重建完成");
    Ok(window)
}

/// 退出轻量模式：确保主窗口存在，并将其显示、聚焦
pub fn exit_lightweight_mode(app: &AppHandle) -> Result<WebviewWindow, String> {
    let window = ensure_main_window(app)?;

    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();

    #[cfg(target_os = "macos")]
    {
        app.set_activation_policy(tauri::ActivationPolicy::Regular)
            .unwrap_or(());
    }

    Ok(window)
}

/// 进入轻量模式：保存当前窗口位置与尺寸，彻底销毁 WebView 进程以释放 100MB+ 常驻内存
pub fn enter_lightweight_mode(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        tracing::info!("[Lightweight] 正在进入轻量模式，保存窗口状态并销毁 WebView...");

        // 1. 保存窗口状态
        let _ = app.save_window_state(StateFlags::all().difference(StateFlags::VISIBLE));

        // 2. 销毁窗口与渲染器
        let _ = window.destroy();

        // 3. macOS 切换为附属模式，避免在 Dock 栏残留
        #[cfg(target_os = "macos")]
        {
            app.set_activation_policy(tauri::ActivationPolicy::Accessory)
                .unwrap_or(());
        }

        tracing::info!("[Lightweight] WebView 已释放，内存占用已降至最小");
    }

    Ok(())
}
