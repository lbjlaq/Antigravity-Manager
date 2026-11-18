use tauri::{AppHandle, Manager, WebviewWindow, WebviewWindowBuilder};
#[cfg(not(target_os = "android"))]
use tauri_plugin_window_state::{AppHandleExt, StateFlags, WindowExt};

/// Queue `f` onto the Win32 event loop from a worker thread so it cannot
/// run inline inside the current tao/Win32 callback.
///
/// `AppHandle::run_on_main_thread` in tauri-runtime-wry 2.10 executes the
/// task immediately when the caller is already on the UI thread, which
/// would not release a WindowManager/PluginStore `RwLock` still held by
/// that callback (Fixes #3993).
#[cfg(target_os = "windows")]
pub fn defer_on_main_thread<F>(app: &tauri::AppHandle, f: F)
where
    F: FnOnce() + Send + 'static,
{
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("agm-defer-main".into())
        .spawn(move || {
            let _ = app.run_on_main_thread(f);
        });
}

/// 尝试获取或重新构建主窗口。
/// 如果主窗口已被轻量模式销毁，将基于 tauri.conf.json 的配置动态重建，并恢复窗口位置和图标。
pub fn ensure_main_window(app: &AppHandle) -> Result<WebviewWindow, String> {
    #[cfg(target_os = "android")]
    {
        let _ = app;
        return Err("Android 不支持动态主窗口构建".to_string());
    }

    #[cfg(not(target_os = "android"))]
    {
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
        // main 已设置 skip_initial_state，插件不再于构建期间自动恢复，需在此手动恢复。
        // Fixes #3993: Windows 上 SetWindowPos 会同步回派 WM_WINDOWPOSCHANGED 并重入 Tauri 的
        // 非可重入 RwLock。build() 返回后通过工作线程再投递到主线程，避免在当前 UI 回调内联执行。
        #[cfg(target_os = "windows")]
        {
            let window_for_restore = window.clone();
            let flags = StateFlags::all().difference(StateFlags::VISIBLE);
            defer_on_main_thread(app, move || {
                let _ = window_for_restore.restore_state(flags);
            });
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = window.restore_state(StateFlags::all().difference(StateFlags::VISIBLE));
        }

        tracing::info!("[Lightweight] 主窗口重建完成");
        Ok(window)
    }
}

/// 退出轻量模式：确保主窗口存在，并将其显示、聚焦
pub fn exit_lightweight_mode(app: &AppHandle) -> Result<WebviewWindow, String> {
    #[cfg(target_os = "android")]
    {
        let _ = app;
        return Err("Android 不支持轻量窗口模式".to_string());
    }

    #[cfg(not(target_os = "android"))]
    {
        crate::modules::startup_quiet::clear();
        #[cfg(not(target_os = "macos"))]
        let already_running = app.get_webview_window("main").is_some();
        let window = ensure_main_window(app)?;

        #[cfg(target_os = "macos")]
        {
            app.set_activation_policy(tauri::ActivationPolicy::Regular)
                .unwrap_or(());
        }

        // macOS 上 WKWebView 无 Win32 编译着色器导致的黑屏闪烁，但若保持隐藏会导致 WebKit 冻结事件循环且激活策略过渡异常；
        // 因此 macOS 下始终立即呈现与置顶窗口，非 macOS（Windows）则在冷重构时等待 React 首帧渲染彻底杜绝黑屏闪现 (Fixes #3744)
        #[cfg(target_os = "macos")]
        {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
        }

        #[cfg(not(target_os = "macos"))]
        if already_running {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
        }

        Ok(window)
    }
}

/// 进入轻量模式：保存当前窗口位置与尺寸，彻底销毁 WebView 进程以释放 100MB+ 常驻内存
pub fn enter_lightweight_mode(app: &AppHandle) -> Result<(), String> {
    release_main_window(app, true)
}

/// 开机免打扰进入轻量模式。此时窗口还是本次启动的初始尺寸，不能覆盖上次记住的位置。
pub fn enter_lightweight_mode_without_saving(app: &AppHandle) -> Result<(), String> {
    release_main_window(app, false)
}

fn release_main_window(app: &AppHandle, save_state: bool) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        let _ = (app, save_state);
        Ok(())
    }

    #[cfg(not(target_os = "android"))]
    {
        if let Some(window) = app.get_webview_window("main") {
            tracing::info!(
                "[Lightweight] 正在进入轻量模式，销毁 WebView（保存窗口状态: {}）...",
                save_state
            );

            if save_state {
                let _ = app.save_window_state(StateFlags::all().difference(StateFlags::VISIBLE));
            }

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
}
