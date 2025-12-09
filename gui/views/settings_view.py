import flet as ft
import sys
import os
import platform
from pathlib import Path
from theme import get_palette
from icons import AppIcons
from localization import t, get_language, get_language_options

RADIUS_CARD = 12
PADDING_PAGE = 20

class SettingsView(ft.Container):
    def __init__(self, page: ft.Page, on_language_change=None):
        super().__init__()
        self.page = page
        self.on_language_change = on_language_change
        self.expand = True
        self.padding = PADDING_PAGE
        
        # Initialize with current palette
        self.palette = get_palette(page)
        self.bgcolor = self.palette.bg_page
        
        self.log_view = ft.ListView(
            expand=True,
            spacing=5,
            padding=10,
            auto_scroll=True,
        )

        self.language_dropdown = ft.Dropdown(
            value=get_language(),
            options=[
                ft.dropdown.Option(opt["code"], t(f"language.{opt['code']}"))
                for opt in get_language_options()
            ],
            width=180,
            on_change=self.handle_language_change,
        )
        
        # Redirect stdout to capture logs
        self.original_stdout = sys.stdout
        sys.stdout = self.LogRedirector(self.log_view)
        
        self.build_ui()

    def did_mount(self):
        pass

    def will_unmount(self):
        # Keep stdout redirected so we capture logs even when not on this view
        pass

    def update_theme(self):
        self.palette = get_palette(self.page)
        self.bgcolor = self.palette.bg_page
        self.build_ui() # Rebuild UI to update colors
        self.update()

    def update_locale(self):
        self.refresh_language_dropdown()
        self.build_ui()
        self.update()

    def refresh_language_dropdown(self):
        current = self.language_dropdown.value or get_language()
        self.language_dropdown.options = [
            ft.dropdown.Option(opt["code"], t(f"language.{opt['code']}"))
            for opt in get_language_options()
        ]
        self.language_dropdown.value = current

    def build_ui(self):
        self.content = ft.Column(
            [
                ft.Text(t("settings.title"), size=28, weight=ft.FontWeight.BOLD, color=self.palette.text_main),
                ft.Container(height=20),
                
                # Top Row: Data Management + About (side by side)
                ft.Row(
                    [
                        # Data Management Section
                        ft.Container(
                            content=ft.Column(
                                [
                                    ft.Text(t("settings.data"), size=13, weight=ft.FontWeight.BOLD, color=self.palette.text_grey),
                                    ft.Container(height=10),
                                    ft.Container(
                                        content=ft.Column(
                                            [
                                                ft.Row(
                                                    [
                                                        ft.Container(
                                                            content=ft.Icon(AppIcons.folder, size=24, color=self.palette.primary),
                                                            bgcolor=self.palette.bg_light_blue,
                                                            padding=8,
                                                            border_radius=8
                                                        ),
                                                        ft.Column(
                                                            [
                                                                ft.Text(t("settings.local_dir"), size=15, weight=ft.FontWeight.W_600, color=self.palette.text_main),
                                                                ft.Text(t("settings.view_backups"), size=12, color=self.palette.text_grey),
                                                            ],
                                                            spacing=2,
                                                            alignment=ft.MainAxisAlignment.CENTER
                                                        )
                                                    ],
                                                    spacing=15
                                                ),
                                                ft.Container(height=20),
                                                ft.Row(
                                                    [
                                                        ft.Text(t("settings.language"), size=13, color=self.palette.text_main, weight=ft.FontWeight.W_600),
                                                        self.language_dropdown,
                                                    ],
                                                    alignment=ft.MainAxisAlignment.SPACE_BETWEEN,
                                                    vertical_alignment=ft.CrossAxisAlignment.CENTER,
                                                ),
                                                ft.Container(height=12),
                                                ft.Container(
                                                    content=ft.Text(t("settings.open_folder"), size=13, color=self.palette.primary, weight=ft.FontWeight.BOLD),
                                                    padding=ft.padding.symmetric(horizontal=20, vertical=10),
                                                    border_radius=8,
                                                    bgcolor=self.palette.bg_light_blue,
                                                    on_click=self.open_data_folder,
                                                    alignment=ft.alignment.center
                                                ),
                                            ],
                                            spacing=0,
                                            alignment=ft.MainAxisAlignment.SPACE_BETWEEN
                                        ),
                                        padding=20,
                                        bgcolor=self.palette.bg_card,
                                        border_radius=RADIUS_CARD,
                                        height=210,
                                        shadow=ft.BoxShadow(
                                            spread_radius=0,
                                            blur_radius=10,
                                            color=self.palette.shadow,
                                            offset=ft.Offset(0, 4),
                                        ),
                                    ),
                                ],
                                spacing=0
                            ),
                            expand=True
                        ),
                        
                        # About Section
                        ft.Container(
                            content=ft.Column(
                                [
                                    ft.Text(t("settings.about"), size=13, weight=ft.FontWeight.BOLD, color=self.palette.text_grey),
                                    ft.Container(height=10),
                                    ft.Container(
                                        content=ft.Column(
                                            [
                                                ft.Row(
                                                    [
                                                        ft.Icon(ft.Icons.INFO_OUTLINE, size=20, color=self.palette.primary),
                                                        ft.Text(t("app.fullname"), size=15, weight=ft.FontWeight.BOLD, color=self.palette.text_main),
                                                    ],
                                                    spacing=10
                                                ),
                                                ft.Container(height=15),
                                                ft.Row(
                                                    [
                                                        ft.Text(t("settings.author"), size=13, color=self.palette.text_grey, weight=ft.FontWeight.W_500),
                                                        ft.Text("Ctrler", size=13, color=self.palette.text_main),
                                                    ],
                                                    spacing=5
                                                ),
                                                ft.Container(height=8),
                                                ft.Row(
                                                    [
                                                        ft.Text(t("settings.wechat"), size=13, color=self.palette.text_grey, weight=ft.FontWeight.W_500),
                                                        ft.Text("Ctrler", size=13, color=self.palette.text_main),
                                                    ],
                                                    spacing=5
                                                ),
                                            ],
                                            spacing=0,
                                            alignment=ft.MainAxisAlignment.SPACE_BETWEEN
                                        ),
                                        padding=20,
                                        bgcolor=self.palette.bg_card,
                                        border_radius=RADIUS_CARD,
                                        height=170,
                                        shadow=ft.BoxShadow(
                                            spread_radius=0,
                                            blur_radius=10,
                                            color=self.palette.shadow,
                                            offset=ft.Offset(0, 4),
                                        ),
                                    ),
                                ],
                                spacing=0
                            ),
                            expand=True
                        ),
                    ],
                    spacing=15,
                ),
                
                ft.Container(height=20),
                
                # Logs Section (takes up remaining space)
                ft.Text(t("settings.logs"), size=13, weight=ft.FontWeight.BOLD, color=self.palette.text_grey),
                ft.Container(height=10),
                ft.Container(
                    content=self.log_view,
                    bgcolor="#1E1E1E", # Console always dark
                    border_radius=RADIUS_CARD,
                    expand=True,  # This will take up all remaining vertical space
                    padding=15,
                    shadow=ft.BoxShadow(
                        spread_radius=0,
                        blur_radius=10,
                        color=self.palette.shadow,
                        offset=ft.Offset(0, 4),
                    )
                )
            ],
            horizontal_alignment=ft.CrossAxisAlignment.START
        )

    def open_data_folder(self, e):
        path_to_open = os.path.expanduser("~/.antigravity-agent")
        if not os.path.exists(path_to_open):
            path_to_open = os.getcwd()
        
        path_to_open = os.path.normpath(path_to_open)
             
        if platform.system() == "Darwin":
            os.system(f"open '{path_to_open}'")
        elif platform.system() == "Windows":
            try:
                os.startfile(path_to_open)
            except Exception as e:
                print(f"Failed to open folder: {e}")
        else:
            os.system(f"xdg-open '{path_to_open}'")

    def handle_language_change(self, e):
        lang = e.control.value
        if lang and self.on_language_change:
            self.on_language_change(lang)

    class LogRedirector:
        def __init__(self, log_view):
            self.log_view = log_view
            self.terminal = sys.stdout

        def write(self, message):
            if self.terminal:
                try:
                    self.terminal.write(message)
                except:
                    pass
            if not message.strip():
                return
                
            # Simple ANSI color parsing
            text_color = "#FFFFFF" # Default log color
            clean_message = message.strip()
            
            if "\033[32m" in message: # Green (INFO)
                text_color = "#34C759"
                clean_message = clean_message.replace("\033[32m", "").replace("\033[0m", "")
            elif "\033[33m" in message: # Yellow (WARN)
                text_color = "#FFCC00"
                clean_message = clean_message.replace("\033[33m", "").replace("\033[0m", "")
            elif "\033[31m" in message: # Red (ERR)
                text_color = "#FF3B30"
                clean_message = clean_message.replace("\033[31m", "").replace("\033[0m", "")
            elif "\033[90m" in message: # Grey (DEBUG)
                text_color = "#8E8E93"
                clean_message = clean_message.replace("\033[90m", "").replace("\033[0m", "")
            
            # Remove any remaining ANSI codes if simple parsing missed them
            if "\033[" in clean_message:
                import re
                ansi_escape = re.compile(r'\x1B(?:[@-Z\\-_]|\[[0-?]*[ -/]*[@-~])')
                clean_message = ansi_escape.sub('', clean_message)

            self.log_view.controls.append(
                ft.Text(
                    clean_message, 
                    font_family="Monaco, Menlo, Courier New, monospace", 
                    size=12,
                    color=text_color,
                    selectable=True
                )
            )
            
            # Only try to update if the control is attached to a page
            if self.log_view.page:
                try:
                    self.log_view.update()
                except:
                    pass

        def flush(self):
            self.terminal.flush()
