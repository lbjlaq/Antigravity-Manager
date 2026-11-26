use am_core::config::AppConfig;
use am_core::types::TargetHost;
use am_core::AntigravityManager;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::time::Duration;
use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    window::Color,
    Emitter, Manager, WebviewUrl,
};

pub const TRAY_ID: &str = "main-tray";
const MINI_WIDTH: f64 = 360.0;
const MINI_HEIGHT: f64 = 420.0;
const MINI_IDLE_RECLAIM_SECS: u64 = 90;
const MAIN_IDLE_RECLAIM_SECS: u64 = 180;

static MINI_VIEW_ACTIVE: AtomicBool = AtomicBool::new(false);
static MINI_VIEW_PINNED: AtomicBool = AtomicBool::new(false);
static APP_EXITING: AtomicBool = AtomicBool::new(false);
static TRAY_AVAILABLE: AtomicBool = AtomicBool::new(false);
static LIGHTWEIGHT_ON_READY: AtomicBool = AtomicBool::new(false);
static LAST_BLUR_TIME_MS: AtomicI64 = AtomicI64::new(0);
static MINI_IDLE_GEN: AtomicU64 = AtomicU64::new(0);
static MAIN_IDLE_GEN: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn is_app_exiting() -> bool {
    APP_EXITING.load(Ordering::SeqCst)
}

pub fn mark_app_exiting() {
    APP_EXITING.store(true, Ordering::SeqCst);
}

pub fn is_tray_available() -> bool {
    TRAY_AVAILABLE.load(Ordering::SeqCst)
}

pub fn take_lightweight_on_ready() -> bool {
    LIGHTWEIGHT_ON_READY.swap(false, Ordering::SeqCst)
}

pub fn is_mini_pinned() -> bool {
    MINI_VIEW_PINNED.load(Ordering::Relaxed)
}

pub fn set_mini_pinned_state(app: &tauri::AppHandle, pinned: bool) {
    MINI_VIEW_PINNED.store(pinned, Ordering::Relaxed);
    if pinned {
        cancel_mini_idle_reclaim();
    }
    if let Some(win) = app.get_webview_window("mini") {
        let _ = win.set_always_on_top(pinned);
    }
}

pub fn cancel_mini_idle_reclaim() {
    MINI_IDLE_GEN.fetch_add(1, Ordering::SeqCst);
}

pub fn cancel_main_idle_reclaim() {
    MAIN_IDLE_GEN.fetch_add(1, Ordering::SeqCst);
}

/// 安排延迟回收未置顶的 mini 悬浮窗：短时间二次点击秒开，闲置 90s 后自动销毁释放 WebView 进程
pub fn schedule_mini_idle_reclaim(app: &tauri::AppHandle) {
    let gen = MINI_IDLE_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(MINI_IDLE_RECLAIM_SECS)).await;
        if MINI_IDLE_GEN.load(Ordering::SeqCst) != gen
            || is_mini_pinned()
            || MINI_VIEW_ACTIVE.load(Ordering::Relaxed)
        {
            return;
        }
        if let Some(win) = handle.get_webview_window("mini") {
            let visible = win.is_visible().unwrap_or(false);
            if !visible {
                let _ = win.destroy();
                am_core::trim_process_memory();
            }
        }
    });
}

/// 安排托盘隐身闲置回收：未显式开启轻量模式时，主窗口隐藏在托盘 3 分钟无操作自动释放前端 WebView 进程
pub fn schedule_main_idle_reclaim(app: &tauri::AppHandle) {
    let gen = MAIN_IDLE_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(MAIN_IDLE_RECLAIM_SECS)).await;
        if MAIN_IDLE_GEN.load(Ordering::SeqCst) != gen || !is_tray_available() || is_app_exiting() {
            return;
        }
        let still_hidden = handle
            .get_webview_window("main")
            .map(|m| !m.is_visible().unwrap_or(true))
            .unwrap_or(false);
        if still_hidden {
            enter_lightweight_mode(&handle);
        }
    });
}

pub fn record_mini_blur() {
    LAST_BLUR_TIME_MS.store(now_ms(), Ordering::Relaxed);
}

fn is_recent_blur() -> bool {
    (now_ms() - LAST_BLUR_TIME_MS.load(Ordering::Relaxed)) < 280
}

/// 获取或按需重建主窗口（当轻量模式已销毁主窗口 WebView 进程时，从 tauri.conf.json 动态恢复）
pub fn ensure_main_window(app: &tauri::AppHandle) -> Result<tauri::WebviewWindow, String> {
    if let Some(win) = app.get_webview_window("main") {
        return Ok(win);
    }
    let win_cfg = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == "main")
        .cloned()
        .ok_or_else(|| "Missing 'main' window config in tauri.conf.json".to_string())?;

    let win = tauri::WebviewWindowBuilder::from_config(app, &win_cfg)
        .map_err(|e| format!("Failed to configure main window: {}", e))?
        .build()
        .map_err(|e| format!("Failed to rebuild main window: {}", e))?;
    if let Some(icon) = app.default_window_icon().cloned() {
        let _ = win.set_icon(icon);
    }
    Ok(win)
}

/// 进入轻量模式：直接销毁前端 WebView 渲染进程以彻底释放内存，仅保留极轻量的 Rust 守护与托盘
pub fn enter_lightweight_mode(app: &tauri::AppHandle) {
    cancel_main_idle_reclaim();
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.destroy();
    }
    if !is_mini_pinned() {
        cancel_mini_idle_reclaim();
        if let Some(mini) = app.get_webview_window("mini") {
            let _ = mini.destroy();
        }
        MINI_VIEW_ACTIVE.store(false, Ordering::Relaxed);
    }
    #[cfg(target_os = "macos")]
    {
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    }
    am_core::trim_process_memory();
}

/// 应用启动窗口可见性策略：
/// - 手动打开软件：始终显示主窗口
/// - 开机自启（带 `--minimized` / `--silent` / `--hidden` / `-s`）：
///   - 若系统无托盘或用户关闭了托盘 / 免打扰启动 -> 回退显示主窗口
///   - 若开启免打扰启动 -> 不弹主窗口留在托盘；同时开启轻量模式时在 Ready 阶段直接释放前端进程
pub fn apply_startup_launch_policy(app: &tauri::AppHandle) {
    let launched_by_autostart = std::env::args().any(|arg| {
        let lower = arg.to_ascii_lowercase();
        lower == "--minimized"
            || lower == "--silent"
            || lower == "--hidden"
            || lower == "-s"
            || lower == "/minimized"
            || lower.contains("--minimized")
            || lower.contains("--silent")
            || lower.contains("--hidden")
    });

    if !launched_by_autostart {
        show_main_window(app);
        return;
    }

    let cfg = AppConfig::load();
    if !cfg.tray.quiet_autostart || !is_tray_available() {
        show_main_window(app);
        return;
    }

    // 免打扰开机自启生效：保持主窗口隐藏驻留托盘
    if cfg.tray.lightweight_mode {
        LIGHTWEIGHT_ON_READY.store(true, Ordering::SeqCst);
    } else {
        #[cfg(target_os = "macos")]
        {
            let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
        }
        schedule_main_idle_reclaim(app);
    }
}

/// 按需懒创建 mini 悬浮窗（不在启动时预创建，彻底避免白占第二个 WebView 进程内存）
pub fn ensure_mini_window(app: &tauri::AppHandle) -> Result<tauri::WebviewWindow, String> {
    cancel_mini_idle_reclaim();
    if let Some(win) = app.get_webview_window("mini") {
        let _ = win.set_background_color(Some(Color(0, 0, 0, 0)));
        return Ok(win);
    }

    #[allow(unused_mut)]
    let mut builder =
        tauri::WebviewWindowBuilder::new(app, "mini", WebviewUrl::App("index.html?mini=1".into()))
            .title("Antigravity Mini")
            .inner_size(MINI_WIDTH, MINI_HEIGHT)
            .decorations(false)
            .resizable(false)
            .shadow(true)
            .transparent(true)
            .always_on_top(false)
            .visible(false)
            .skip_taskbar(true)
            .disable_drag_drop_handler();

    #[cfg(target_os = "windows")]
    {
        builder = builder.additional_browser_args(
            "--disable-background-networking --disable-component-update --disable-features=Translate --js-flags=--max-old-space-size=128",
        );
    }

    let win = builder
        .build()
        .map_err(|e| format!("Failed to build mini window: {}", e))?;
    let _ = win.set_background_color(Some(Color(0, 0, 0, 0)));
    Ok(win)
}

/// 在 macOS 非打包开发态 (`cargo run`) 及切换 ActivationPolicy::Regular 后显式注入高清 Dock 图标，
/// 使用 56 KB Retina PNG 替代 1.4 MB .icns，显著降低二进制常驻体积
#[cfg(target_os = "macos")]
pub fn apply_macos_dock_icon() {
    use objc2::{AllocAnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    if let Some(mtm) = MainThreadMarker::new() {
        let png_bytes: &[u8] = include_bytes!("../icons/128x128@2x.png");
        let ns_data = NSData::with_bytes(png_bytes);
        if let Some(ns_image) = NSImage::initWithData(NSImage::alloc(), &ns_data) {
            let ns_app = NSApplication::sharedApplication(mtm);
            unsafe {
                ns_app.setApplicationIconImage(Some(&ns_image));
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn apply_macos_dock_icon() {}

pub fn hide_mini_window(app: &tauri::AppHandle) {
    let cfg = AppConfig::load();
    let main_open = app
        .get_webview_window("main")
        .map(|m| m.is_visible().unwrap_or(false) || m.is_minimized().unwrap_or(false))
        .unwrap_or(false);

    if let Some(win) = app.get_webview_window("mini") {
        if cfg.tray.lightweight_mode && !main_open && !is_mini_pinned() {
            cancel_mini_idle_reclaim();
            let _ = win.destroy();
            am_core::trim_process_memory();
        } else {
            let _ = win.hide();
            if !is_mini_pinned() {
                schedule_mini_idle_reclaim(app);
            }
        }
    }
    MINI_VIEW_ACTIVE.store(false, Ordering::Relaxed);

    #[cfg(target_os = "macos")]
    {
        if main_open {
            let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
            apply_macos_dock_icon();
        }
    }
}

pub fn show_main_window(app: &tauri::AppHandle) {
    cancel_main_idle_reclaim();
    #[cfg(target_os = "macos")]
    {
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
        apply_macos_dock_icon();
    }
    if !is_mini_pinned() {
        if let Some(mini) = app.get_webview_window("mini") {
            let _ = mini.hide();
            schedule_mini_idle_reclaim(app);
        }
        MINI_VIEW_ACTIVE.store(false, Ordering::Relaxed);
    }
    if let Ok(main) = ensure_main_window(app) {
        let _ = main.unminimize();
        let _ = main.show();
        let _ = main.set_focus();
    }
}

pub fn toggle_main_window(app: &tauri::AppHandle) {
    if let Some(main) = app.get_webview_window("main") {
        let visible = main.is_visible().unwrap_or(false);
        let focused = main.is_focused().unwrap_or(false);
        if visible && focused {
            let cfg = AppConfig::load();
            if cfg.tray.enabled && cfg.tray.close_to_tray && cfg.tray.lightweight_mode {
                enter_lightweight_mode(app);
            } else {
                let _ = main.hide();
                #[cfg(target_os = "macos")]
                if cfg.tray.enabled && cfg.tray.close_to_tray {
                    let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
                }
                if cfg.tray.enabled && cfg.tray.close_to_tray {
                    schedule_main_idle_reclaim(app);
                }
            }
            return;
        }
    }
    show_main_window(app);
}

pub fn toggle_mini_popover(app: &tauri::AppHandle, rect: Option<tauri::Rect>) {
    if is_recent_blur() && !is_mini_pinned() {
        return;
    }

    if MINI_VIEW_ACTIVE.load(Ordering::Relaxed) {
        if let Some(win) = app.get_webview_window("mini") {
            if win.is_visible().unwrap_or(false) {
                hide_mini_window(app);
                return;
            }
        }
    }

    let window = match ensure_mini_window(app) {
        Ok(w) => w,
        Err(_) => return,
    };

    let mut target_monitor = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| app.primary_monitor().ok().flatten());

    if let (Some(r), Ok(monitors)) = (rect, app.available_monitors()) {
        if let tauri::Position::Physical(p) = r.position {
            for m in monitors {
                let mp = m.position();
                let ms = m.size();
                if p.x >= mp.x
                    && p.x < mp.x + ms.width as i32
                    && p.y >= mp.y
                    && p.y < mp.y + ms.height as i32
                {
                    target_monitor = Some(m);
                    break;
                }
            }
        }
    }

    let (mon_x, mon_y, mon_w, mon_h, scale_factor) = if let Some(ref m) = target_monitor {
        let pos = m.position();
        let sz = m.size();
        (
            pos.x as f64,
            pos.y as f64,
            sz.width as f64,
            sz.height as f64,
            m.scale_factor(),
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

        let x = tray_x + (tray_w / 2.0) - (physical_w / 2.0);
        let y = if tray_y > mon_y + (mon_h / 2.0) {
            tray_y - physical_h - (6.0 * scale_factor)
        } else {
            tray_y + tray_h + (6.0 * scale_factor)
        };
        (x, y)
    } else {
        (
            mon_x + (mon_w - physical_w) - (24.0 * scale_factor),
            mon_y + (36.0 * scale_factor),
        )
    };

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
    let _ = window.set_background_color(Some(Color(0, 0, 0, 0)));
    let _ = window.set_always_on_top(true);
    let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize::new(
        MINI_WIDTH,
        MINI_HEIGHT,
    )));
    let _ = window.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(
        win_x as i32,
        win_y as i32,
    )));

    #[cfg(target_os = "macos")]
    {
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    }

    let _ = window.show();
    let _ = window.set_focus();
    MINI_VIEW_ACTIVE.store(true, Ordering::Relaxed);
    let _ = app.emit("tray://mini-opened", ());
}

struct TrayMenuTexts {
    open_main: &'static str,
    toggle_mini: &'static str,
    smart_ide: &'static str,
    refresh_all: &'static str,
    quit: &'static str,
    switched_title: &'static str,
    switched_prefix: &'static str,
}

fn tray_menu_texts(lang: &str) -> TrayMenuTexts {
    match lang {
        "zh" | "zh-CN" => TrayMenuTexts {
            open_main: "打开主工作台",
            toggle_mini: "打开悬浮小窗",
            smart_ide: "⚡ IDE 智能一键换号",
            refresh_all: "🔄 刷新全量账号配额",
            quit: "退出 Antigravity Manager",
            switched_title: "Antigravity IDE 已换号",
            switched_prefix: "已智能切换至",
        },
        "zh-TW" => TrayMenuTexts {
            open_main: "開啟主工作台",
            toggle_mini: "開啟懸浮小窗",
            smart_ide: "⚡ IDE 智慧一鍵換號",
            refresh_all: "🔄 重新整理全量帳號配額",
            quit: "結束 Antigravity Manager",
            switched_title: "Antigravity IDE 已換號",
            switched_prefix: "已智慧切換至",
        },
        "ja" => TrayMenuTexts {
            open_main: "メイン画面を開く",
            toggle_mini: "ミニウィンドウを表示",
            smart_ide: "⚡ IDE スマート切替",
            refresh_all: "🔄 全アカウントのクォータを更新",
            quit: "Antigravity Manager を終了",
            switched_title: "Antigravity IDE アカウント切替",
            switched_prefix: "切り替え完了:",
        },
        "ko" => TrayMenuTexts {
            open_main: "메인 스튜디오 열기",
            toggle_mini: "미니 팝오버 열기",
            smart_ide: "⚡ IDE 스마트 계정 전환",
            refresh_all: "🔄 전체 할당량 새로고침",
            quit: "Antigravity Manager 종료",
            switched_title: "Antigravity IDE 계정 전환됨",
            switched_prefix: "전환 완료:",
        },
        "ru" => TrayMenuTexts {
            open_main: "Открыть главное окно",
            toggle_mini: "Открыть мини-окно",
            smart_ide: "⚡ Умная смена аккаунта IDE",
            refresh_all: "🔄 Обновить все квоты",
            quit: "Выйти из Antigravity Manager",
            switched_title: "Аккаунт Antigravity IDE изменен",
            switched_prefix: "Переключено на",
        },
        "es" => TrayMenuTexts {
            open_main: "Abrir ventana principal",
            toggle_mini: "Abrir ventana flotante",
            smart_ide: "⚡ Cambio inteligente IDE",
            refresh_all: "🔄 Actualizar todas las cuotas",
            quit: "Salir de Antigravity Manager",
            switched_title: "Cuenta de Antigravity IDE cambiada",
            switched_prefix: "Cambiado a",
        },
        "pt" => TrayMenuTexts {
            open_main: "Abrir janela principal",
            toggle_mini: "Abrir janela flutuante",
            smart_ide: "⚡ Troca inteligente IDE",
            refresh_all: "🔄 Atualizar todas as cotas",
            quit: "Sair do Antigravity Manager",
            switched_title: "Conta do Antigravity IDE alterada",
            switched_prefix: "Alternado para",
        },
        "tr" => TrayMenuTexts {
            open_main: "Ana Pencereyi Aç",
            toggle_mini: "Mini Pencereyi Aç",
            smart_ide: "⚡ IDE Akıllı Hesap Değiştir",
            refresh_all: "🔄 Tüm Kotaları Yenile",
            quit: "Antigravity Manager'dan Çık",
            switched_title: "Antigravity IDE Hesabı Değiştirildi",
            switched_prefix: "Geçiş yapıldı:",
        },
        "vi" => TrayMenuTexts {
            open_main: "Mở cửa sổ chính",
            toggle_mini: "Mở cửa sổ nổi",
            smart_ide: "⚡ Chuyển tài khoản IDE thông minh",
            refresh_all: "🔄 Làm mới toàn bộ hạn mức",
            quit: "Thoát Antigravity Manager",
            switched_title: "Đã chuyển tài khoản Antigravity IDE",
            switched_prefix: "Đã chuyển sang",
        },
        "ar" => TrayMenuTexts {
            open_main: "فتح النافذة الرئيسية",
            toggle_mini: "فتح النافذة المصغرة",
            smart_ide: "⚡ تبديل ذكي لحساب IDE",
            refresh_all: "🔄 تحديث جميع الحصص",
            quit: "إنهاء Antigravity Manager",
            switched_title: "تم تبديل حساب Antigravity IDE",
            switched_prefix: "تم التبديل إلى",
        },
        "my" => TrayMenuTexts {
            open_main: "Buka Tetingkap Utama",
            toggle_mini: "Buka Tetingkap Mini",
            smart_ide: "⚡ Tukar Pintar IDE",
            refresh_all: "🔄 Segar Semula Semua Kuota",
            quit: "Keluar Antigravity Manager",
            switched_title: "Akaun Antigravity IDE Ditukar",
            switched_prefix: "Ditukar kepada",
        },
        _ => TrayMenuTexts {
            open_main: "Open Main Studio",
            toggle_mini: "Open Mini Popover",
            smart_ide: "⚡ Smart Switch IDE Account",
            refresh_all: "🔄 Refresh All Quotas",
            quit: "Quit Antigravity Manager",
            switched_title: "Antigravity IDE Switched",
            switched_prefix: "Switched to",
        },
    }
}

fn build_tray_menu(app: &tauri::AppHandle, lang: &str) -> tauri::Result<Menu<tauri::Wry>> {
    let t = tray_menu_texts(lang);
    let open_main = MenuItem::with_id(app, "open_main", t.open_main, true, None::<&str>)?;
    let toggle_mini = MenuItem::with_id(app, "toggle_mini", t.toggle_mini, true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let smart_ide = MenuItem::with_id(app, "smart_ide", t.smart_ide, true, None::<&str>)?;
    let refresh_all = MenuItem::with_id(app, "refresh_all", t.refresh_all, true, None::<&str>)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", t.quit, true, None::<&str>)?;

    Menu::with_items(
        app,
        &[
            &open_main,
            &toggle_mini,
            &sep1,
            &smart_ide,
            &refresh_all,
            &sep2,
            &quit,
        ],
    )
}

pub fn rebuild_tray_menu(app: &tauri::AppHandle, lang: &str) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        if let Ok(menu) = build_tray_menu(app, lang) {
            let _ = tray.set_menu(Some(menu));
        }
    }
}

pub fn init_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let cfg = AppConfig::load();
    let menu = build_tray_menu(app, &cfg.tray.language)?;
    let icon = Image::from_bytes(include_bytes!("../icons/tray-icon.png"))
        .ok()
        .or_else(|| app.default_window_icon().cloned())
        .ok_or_else(|| tauri::Error::AssetNotFound("tray-icon.png".into()))?;

    let left_is_menu = cfg.tray.left_click_action.eq_ignore_ascii_case("menu");

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("Antigravity Manager")
        .menu(&menu)
        .show_menu_on_left_click(left_is_menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open_main" => {
                show_main_window(app);
            }
            "toggle_mini" => {
                toggle_mini_popover(app, None);
            }
            "smart_ide" => {
                let app_handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Some(mgr) = app_handle.try_state::<AntigravityManager>() {
                        if let Ok(Some(acc)) = mgr
                            .smart_switch_with_refresh(TargetHost::Ide, None, None, None, false)
                            .await
                        {
                            let lang = AppConfig::load().tray.language;
                            let t = tray_menu_texts(&lang);
                            am_core::CoreDaemonService::send_desktop_notification(
                                t.switched_title,
                                &format!("{} {}", t.switched_prefix, acc.email),
                            );
                            let _ = app_handle.emit("tray://accounts-changed", ());
                        }
                    }
                });
            }
            "refresh_all" => {
                let app_handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Some(mgr) = app_handle.try_state::<AntigravityManager>() {
                        let _ = mgr.refresh_all_quotas().await;
                        let _ = app_handle.emit("tray://accounts-changed", ());
                    }
                });
            }
            "quit" => {
                mark_app_exiting();
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                rect,
                ..
            } = event
            {
                let app = tray.app_handle();
                let cfg = AppConfig::load();
                match cfg.tray.left_click_action.to_ascii_lowercase().as_str() {
                    "main" => toggle_main_window(app),
                    "menu" => {}
                    _ => toggle_mini_popover(app, Some(rect)),
                }
            }
        });

    #[cfg(target_os = "macos")]
    {
        builder = builder.icon_as_template(true);
    }

    let tray = builder.build(app)?;
    if !cfg.tray.enabled {
        let _ = tray.set_visible(false);
        TRAY_AVAILABLE.store(false, Ordering::SeqCst);
    } else {
        TRAY_AVAILABLE.store(true, Ordering::SeqCst);
    }

    Ok(())
}

/// 当用户在设置页实时修改托盘开关或点击行为时，即时同步到底层系统托盘
pub fn sync_tray_config(app: &tauri::AppHandle, cfg: &AppConfig) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let visible_ok = tray.set_visible(cfg.tray.enabled).is_ok();
        let left_is_menu = cfg.tray.left_click_action.eq_ignore_ascii_case("menu");
        let _ = tray.set_show_menu_on_left_click(left_is_menu);
        if let Ok(menu) = build_tray_menu(app, &cfg.tray.language) {
            let _ = tray.set_menu(Some(menu));
        }
        TRAY_AVAILABLE.store(cfg.tray.enabled && visible_ok, Ordering::SeqCst);
    } else {
        TRAY_AVAILABLE.store(false, Ordering::SeqCst);
    }
    if !cfg.tray.enabled {
        hide_mini_window(app);
        show_main_window(app);
    }
}
