// 无 WebView 服务版（headless）系统托盘（Windows）。
// 服务模式没有 tauri 窗口运行时可用，因此直接使用 tray-icon crate：
// Windows 上托盘与菜单回调依赖 Win32 消息循环，所以托盘在独立线程上创建，
// 并由该线程持续泵消息（GetMessageW）；服务主循环（tokio runtime）不受影响。
// 退出走与桌面托盘一致的清理顺序：cloudflared → Admin Server → 代理实例 → 进程退出。

use std::sync::Arc;

use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::TrayIconBuilder;

use crate::commands;
use tracing::{error, info, warn};

pub struct HeadlessTrayContext {
    /// 服务 tokio runtime 的句柄：菜单事件线程用 block_on 执行异步清理
    pub runtime: tokio::runtime::Handle,
    /// WebUI/管理面端口（用于"打开 Web UI"）
    pub port: u16,
    pub proxy_state: commands::proxy::ProxyServiceState,
    pub cf_state: Arc<commands::cloudflared::CloudflaredState>,
}

/// 在独立线程上创建托盘并泵消息。失败只记日志，不影响服务运行。
pub fn spawn_tray(ctx: HeadlessTrayContext) {
    let result = std::thread::Builder::new()
        .name("headless-tray".into())
        .spawn(move || run_tray(ctx));
    if let Err(e) = result {
        error!("Failed to spawn headless tray thread: {}", e);
    }
}

fn menu_texts() -> (&'static str, &'static str, &'static str) {
    // (打开 Web UI, 退出, tooltip)
    let zh = modules::config::load_app_config()
        .map(|c| c.language.starts_with("zh"))
        .unwrap_or(true);
    if zh {
        ("打开 Web UI", "退出", "Antigravity Tools 服务运行中")
    } else {
        ("Open Web UI", "Quit", "Antigravity Tools service running")
    }
}

fn run_tray(ctx: HeadlessTrayContext) {
    let (open_text, quit_text, tooltip) = menu_texts();

    let icon = match load_icon() {
        Ok(icon) => icon,
        Err(e) => {
            error!("Headless tray: failed to load icon: {}", e);
            return;
        }
    };

    // 菜单：打开 Web UI / 退出
    let mut menu = Menu::new();
    let open_item = MenuItem::with_id("open_webui", open_text, true, None);
    let quit_item = MenuItem::with_id("quit", quit_text, true, None);
    if let Err(e) = menu.append_items(&[&open_item, &PredefinedMenuItem::separator(), &quit_item]) {
        error!("Headless tray: failed to build menu: {}", e);
        return;
    }

    let tray = TrayIconBuilder::with_id("headless")
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(true)
        .with_icon(icon)
        .with_tooltip(tooltip)
        .build();

    if let Err(e) = tray {
        error!("Headless tray: failed to create tray icon: {}", e);
        return;
    }
    info!("Headless system tray created.");

    // 全局菜单事件处理器（Windows 上经消息循环在托盘线程回调）
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| match event.id().as_ref() {
        "open_webui" => {
            let url = format!("http://localhost:{}", ctx.port);
            crate::open_in_system_browser(&url);
        }
        "quit" => {
            info!("Headless tray: quit requested, shutting down services...");
            shutdown_services(
                &ctx.runtime,
                &ctx.proxy_state,
                &ctx.cf_state,
            );
            std::process::exit(0);
        }
        _ => {}
    }));

    pump_messages_forever();
}

fn load_icon() -> Result<tray_icon::Icon, String> {
    let icon_bytes: &[u8] = include_bytes!("../../icons/icon.png");
    let img = image::load_from_memory(icon_bytes).map_err(|e| e.to_string())?;
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    tray_icon::Icon::from_rgba(rgba.into_raw(), width, height).map_err(|e| e.to_string())
}

/// 与桌面托盘"退出"菜单一致的清理顺序（见 modules/tray.rs），附带超时保护。
fn shutdown_services(
    runtime: &tokio::runtime::Handle,
    proxy_state: &commands::proxy::ProxyServiceState,
    cf_state: &Arc<commands::cloudflared::CloudflaredState>,
) {
    runtime.block_on(async {
        // 1. 终止 cloudflared 隧道子进程
        let _ = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            cf_state.stop(),
        )
        .await;

        // 2. 停止 Admin Server（关闭 TCP 监听器和所有活动连接）
        if let Ok(mut lock) = tokio::time::timeout(
            std::time::Duration::from_millis(1000),
            proxy_state.admin_server.write(),
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
            proxy_state.instance.write(),
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

        // 4. 给予底层套接字彻底注销的微小缓冲
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    });
}

/// 托盘线程消息泵：GetMessageW 返回 0（WM_QUIT）或 -1（错误）时结束循环。
/// 服务进程退出走 std::process::exit，因此正常情况下此循环与进程同生命周期。
fn pump_messages_forever() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, TranslateMessage, MSG,
    };

    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        loop {
            let ret = GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0);
            if ret <= 0 {
                if ret < 0 {
                    warn!("Headless tray: GetMessageW failed; tray thread exiting");
                }
                break;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
