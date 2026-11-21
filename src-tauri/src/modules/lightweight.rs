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
        // Fixes #3985: 恢复位置后执行 ensure_window_on_visible_screen 多屏安全边界自愈。
        #[cfg(target_os = "windows")]
        {
            let window_for_restore = window.clone();
            let flags = StateFlags::all().difference(StateFlags::VISIBLE);
            defer_on_main_thread(app, move || {
                let _ = window_for_restore.restore_state(flags);
                ensure_window_on_visible_screen(&window_for_restore);
            });
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = window.restore_state(StateFlags::all().difference(StateFlags::VISIBLE));
            ensure_window_on_visible_screen(&window);
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
            ensure_window_on_visible_screen(&window);
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
        }

        Ok(window)
    }
}

/// 矩形边界（像素坐标与尺寸）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RectBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// 计算多显示器环境下窗口的安全坐标（防止标题栏出界、跨屏漂移或断联屏幕遗留无效坐标）
/// Fixes #3985
pub fn clamp_bounds_to_monitors(window: RectBounds, monitors: &[RectBounds]) -> (i32, i32) {
    if monitors.is_empty() {
        return (window.x, window.y);
    }

    let win_w = window.width as i32;
    let win_h = window.height as i32;
    let win_left = window.x;
    let win_top = window.y;
    let win_right = win_left.saturating_add(win_w);
    let win_bottom = win_top.saturating_add(win_h);

    // 1. 查找与窗口重叠面积最大的显示器
    let mut best_monitor: Option<RectBounds> = None;
    let mut max_overlap_area: i64 = 0;

    for m in monitors {
        let m_w = m.width as i32;
        let m_h = m.height as i32;
        let m_right = m.x.saturating_add(m_w);
        let m_bottom = m.y.saturating_add(m_h);

        let overlap_left = win_left.max(m.x);
        let overlap_right = win_right.min(m_right);
        let overlap_top = win_top.max(m.y);
        let overlap_bottom = win_bottom.min(m_bottom);

        if overlap_right > overlap_left && overlap_bottom > overlap_top {
            let area =
                (overlap_right - overlap_left) as i64 * (overlap_bottom - overlap_top) as i64;
            if area > max_overlap_area {
                max_overlap_area = area;
                best_monitor = Some(*m);
            }
        }
    }

    // 2. 若存在有效重叠的显示器，在此显示器可见区域内进行安全钳制
    if let Some(target) = best_monitor {
        let m_w = target.width as i32;
        let m_h = target.height as i32;
        let mut new_x = window.x;
        let mut new_y = window.y;

        // 垂直保护：标题栏不可顶出屏幕（至少保留在顶部边界），且不可沉入屏幕底部以下
        if new_y < target.y {
            new_y = target.y;
        } else if new_y > target.y + m_h - 40 {
            new_y = (target.y + m_h - 40 - win_h).max(target.y);
        }

        // 水平保护：若窗口尺寸小于等于显示器宽度，尽量全量吸附；否则至少保留 120px 可见
        if win_w <= m_w {
            new_x = new_x.clamp(target.x, target.x + m_w - win_w);
        } else {
            new_x = new_x.clamp(target.x + 120 - win_w, target.x + m_w - 120);
        }

        return (new_x, new_y);
    }

    // 3. 零相交（窗口完全漂移出界、断联副屏残余坐标或 Win32 最小化 -32000 坐标）：优雅居中于首个可用/主显示器
    let fallback = monitors[0];
    let fallback_w = fallback.width as i32;
    let fallback_h = fallback.height as i32;
    let center_x = fallback.x + (fallback_w - win_w).max(0) / 2;
    let center_y = fallback.y + (fallback_h - win_h).max(0) / 2;
    (center_x, center_y)
}

/// 确保窗口坐标位于有效的多显示器可视区域内，自愈漂移与出界
#[cfg(not(target_os = "android"))]
pub fn ensure_window_on_visible_screen(window: &WebviewWindow) {
    if window.is_minimized().unwrap_or(false) {
        return;
    }

    let outer_pos = match window.outer_position() {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("[Lightweight] 获取窗口位置失败: {}", e);
            return;
        }
    };
    let outer_sz = match window.outer_size() {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("[Lightweight] 获取窗口尺寸失败: {}", e);
            return;
        }
    };

    if outer_sz.width == 0 || outer_sz.height == 0 {
        return;
    }

    let monitors = match window.available_monitors() {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!("[Lightweight] 获取可用显示器列表失败: {}", e);
            return;
        }
    };

    if monitors.is_empty() {
        return;
    }

    let mon_bounds: Vec<RectBounds> = monitors
        .iter()
        .map(|m| {
            let pos = m.position();
            let sz = m.size();
            RectBounds {
                x: pos.x,
                y: pos.y,
                width: sz.width,
                height: sz.height,
            }
        })
        .collect();

    let current = RectBounds {
        x: outer_pos.x,
        y: outer_pos.y,
        width: outer_sz.width,
        height: outer_sz.height,
    };

    let (safe_x, safe_y) = clamp_bounds_to_monitors(current, &mon_bounds);

    if safe_x != outer_pos.x || safe_y != outer_pos.y {
        tracing::info!(
            "[Lightweight] 窗口坐标漂移自愈 (Fixes #3985): ({}, {}) -> ({}, {})",
            outer_pos.x,
            outer_pos.y,
            safe_x,
            safe_y
        );
        let _ = window.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(
            safe_x, safe_y,
        )));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_window_on_primary_monitor_stays_untouched() {
        let win = RectBounds {
            x: 100,
            y: 100,
            width: 1024,
            height: 700,
        };
        let monitors = vec![RectBounds {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }];
        let (x, y) = clamp_bounds_to_monitors(win, &monitors);
        assert_eq!((x, y), (100, 100));
    }

    #[test]
    fn test_window_on_secondary_monitor_right() {
        let win = RectBounds {
            x: 2100,
            y: 150,
            width: 1024,
            height: 700,
        };
        let monitors = vec![
            RectBounds {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            RectBounds {
                x: 1920,
                y: 0,
                width: 1920,
                height: 1080,
            },
        ];
        let (x, y) = clamp_bounds_to_monitors(win, &monitors);
        // Stays on secondary monitor! (2100 is between 1920 and 1920 + 1920 - 1024 = 2816)
        assert_eq!((x, y), (2100, 150));
    }

    #[test]
    fn test_window_on_secondary_monitor_negative_left() {
        let win = RectBounds {
            x: -1500,
            y: 200,
            width: 1024,
            height: 700,
        };
        let monitors = vec![
            RectBounds {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            RectBounds {
                x: -1920,
                y: 0,
                width: 1920,
                height: 1080,
            },
        ];
        let (x, y) = clamp_bounds_to_monitors(win, &monitors);
        // Stays on left secondary monitor!
        assert_eq!((x, y), (-1500, 200));
    }

    #[test]
    fn test_window_titlebar_off_screen_top_clamped() {
        let win = RectBounds {
            x: 2100,
            y: -50,
            width: 1024,
            height: 700,
        };
        let monitors = vec![
            RectBounds {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            RectBounds {
                x: 1920,
                y: 0,
                width: 1920,
                height: 1080,
            },
        ];
        let (x, y) = clamp_bounds_to_monitors(win, &monitors);
        // Title bar (y: -50) was off-screen, clamped down to target monitor y (0)
        assert_eq!((x, y), (2100, 0));
    }

    #[test]
    fn test_window_completely_offscreen_heals_to_primary() {
        let win = RectBounds {
            x: 5000,
            y: 5000,
            width: 1024,
            height: 700,
        };
        let monitors = vec![RectBounds {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }];
        let (x, y) = clamp_bounds_to_monitors(win, &monitors);
        // Centered on primary monitor: (1920 - 1024) / 2 = 448, (1080 - 700) / 2 = 190
        assert_eq!((x, y), (448, 190));
    }

    #[test]
    fn test_window_minimized_glitch_negative_32000() {
        let win = RectBounds {
            x: -32000,
            y: -32000,
            width: 1024,
            height: 700,
        };
        let monitors = vec![RectBounds {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }];
        let (x, y) = clamp_bounds_to_monitors(win, &monitors);
        assert_eq!((x, y), (448, 190));
    }
}
