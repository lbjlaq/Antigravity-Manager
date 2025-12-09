import flet as ft
import sys
from pathlib import Path

# Import from local modules
from views.home_view import HomeView
from views.settings_view import SettingsView
from theme import get_palette
from icons import AppIcons
from localization import t, set_language

class SidebarItem(ft.Container):
    def __init__(self, icon, label, selected, on_click, palette):
        super().__init__()
        self.on_click = on_click
        self.border_radius = 6
        self.padding = ft.padding.symmetric(horizontal=10, vertical=8)
        self.bgcolor = ft.Colors.with_opacity(0.1, palette.text_main) if selected else ft.Colors.TRANSPARENT
        self.animate = ft.Animation(200, ft.AnimationCurve.EASE_OUT)
        
        self.content = ft.Row(
            [
                ft.Icon(
                    icon, 
                    size=18, 
                    color=palette.primary if selected else palette.text_main
                ),
                ft.Text(
                    label, 
                    size=13, 
                    weight=ft.FontWeight.W_500 if selected else ft.FontWeight.NORMAL,
                    color=palette.text_main if selected else palette.text_grey
                ),
            ],
            spacing=10,
            vertical_alignment=ft.CrossAxisAlignment.CENTER,
        )

class Sidebar(ft.Container):
    def __init__(self, page, on_nav_change):
        super().__init__()
        self.page = page
        self.on_nav_change = on_nav_change
        self.selected_index = 0
        self.width = 200
        self.padding = ft.padding.only(top=20, left=10, right=10)
        
        self.items = self.get_items()
        
        # Initialize theme without calling update()
        self.palette = get_palette(self.page)
        self.bgcolor = self.palette.sidebar_bg
        self.border = ft.border.only(right=ft.BorderSide(1, self.palette.sidebar_border))
        self.build_menu()

    def get_items(self):
        return [
            {"icon": AppIcons.dashboard, "label": t("nav.dashboard")},
            {"icon": AppIcons.settings, "label": t("nav.settings")},
        ]

    def update_theme(self):
        self.palette = get_palette(self.page)
        self.bgcolor = self.palette.sidebar_bg
        self.border = ft.border.only(right=ft.BorderSide(1, self.palette.sidebar_border))
        self.build_menu()
        self.update()

    def build_menu(self):
        menu_items = []
        for idx, item in enumerate(self.items):
            menu_items.append(
                SidebarItem(
                    icon=item["icon"],
                    label=item["label"],
                    selected=(idx == self.selected_index),
                    on_click=lambda e, i=idx: self.handle_nav(i),
                    palette=self.palette
                )
            )
        
        self.content = ft.Column(
            [
                ft.Container(
                    content=ft.Text(t("app.brand"), size=12, weight=ft.FontWeight.BOLD, color=self.palette.text_grey),
                    padding=ft.padding.only(left=10, bottom=10)
                ),
                ft.Column(menu_items, spacing=2)
            ]
        )

    def handle_nav(self, index):
        self.selected_index = index
        self.build_menu()
        self.update()
        self.on_nav_change(index)

    def update_language(self):
        self.items = self.get_items()
        self.build_menu()
        self.update()

def main(page: ft.Page):
    # 尝试在启动时写入日志，验证路径和权限
    try:
        from utils import info, get_app_data_dir
        app_dir = get_app_data_dir()
        info(f"应用启动，数据目录: {app_dir}")
        info(f"Python 版本: {sys.version}")
        info(f"运行平台: {sys.platform}")
    except Exception as e:
        print(f"启动日志写入失败: {e}")

    page.title = t("app.title")
    page.theme_mode = ft.ThemeMode.SYSTEM
    
    # Window settings optimization
    page.window_width = 1000
    page.window_height = 700
    page.window_min_width = 800
    page.window_min_height = 600
    page.window_resizable = True
    page.padding = 0
    
    # Set window icon (must use .ico format on Windows and page.window.icon property)
    page.window.icon = "icon.ico"
    
    # Note: Window icons cannot be changed at runtime in Flet on macOS
    # To use custom icon, build the app with: flet build macos
    # The icon in assets/icon.icns will be automatically used during build
    
    # Initial palette
    palette = get_palette(page)
    page.bgcolor = palette.bg_page
    
    # Define views
    home_view = HomeView(page)

    def handle_language_change(lang_code):
        if set_language(lang_code):
            sidebar.update_language()
            home_view.update_locale()
            settings_view.update_locale()
            page.update()

    settings_view = SettingsView(page, on_language_change=handle_language_change)
    
    views = {
        0: home_view,
        1: settings_view
    }

    content_area = ft.Container(
        content=home_view,
        expand=True,
        padding=0,
        bgcolor=palette.bg_page
    )

    def change_route(index):
        content_area.content = views[index]
        content_area.update()

    sidebar = Sidebar(page, change_route)

    page.add(
        ft.Row(
            [
                sidebar,
                content_area,
            ],
            expand=True,
            spacing=0,
        )
    )
    
    def theme_changed(e):
        palette = get_palette(page)
        page.bgcolor = palette.bg_page
        content_area.bgcolor = palette.bg_page
        
        sidebar.update_theme()
        home_view.update_theme()
        settings_view.update_theme()
        
        page.update()

    page.on_platform_brightness_change = theme_changed

if __name__ == "__main__":
    # Handle assets path for both development and PyInstaller frozen state
    if getattr(sys, 'frozen', False):
        # Running as compiled executable
        if hasattr(sys, '_MEIPASS'):
            # --onefile mode: assets are extracted to temp dir
            assets_path = str(Path(sys._MEIPASS) / "assets")
        else:
            # --onedir mode: assets are next to the executable
            assets_path = str(Path(sys.executable).parent / "assets")
    else:
        # Running as script
        # gui/main.py -> parent is gui -> parent is root -> assets
        assets_path = str(Path(__file__).parent.parent / "assets")
        
    try:
        ft.app(target=main, assets_dir=assets_path)
    except Exception as e:
        import traceback
        print("CRITICAL ERROR: Application crashed!")
        print(traceback.format_exc())
        input("Press Enter to exit...")
    except KeyboardInterrupt:
        pass
    finally:
        # If the app exits normally, we might still want to pause if debugging
        # input("App exited. Press Enter to close...")
        pass
