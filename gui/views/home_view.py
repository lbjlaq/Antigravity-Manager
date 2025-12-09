import flet as ft
import threading
import time
from datetime import datetime
from process_manager import is_process_running, start_antigravity, close_antigravity
from account_manager import add_account_snapshot, list_accounts_data, switch_account, delete_account
from db_manager import get_current_account_info
from theme import get_palette
from icons import AppIcons
from localization import t

RADIUS_CARD = 12
PADDING_PAGE = 20

class HomeView(ft.Container):
    def __init__(self, page: ft.Page):
        super().__init__()
        self.page = page
        self.expand = True
        self.padding = PADDING_PAGE
        
        # Initialize with current palette
        self.palette = get_palette(page)
        self.bgcolor = self.palette.bg_page
        
        # Status Bar Elements
        self.status_bar = ft.Container(
            content=ft.Row(
                [
                    ft.Icon(AppIcons.info, size=16, color=self.palette.primary),
                    ft.Text(t("status.checking"), size=13, weight=ft.FontWeight.W_500, color=self.palette.primary)
                ],
                alignment=ft.MainAxisAlignment.CENTER,
                vertical_alignment=ft.CrossAxisAlignment.CENTER
            ),
            bgcolor=self.palette.bg_light_blue,
            padding=ft.padding.symmetric(vertical=8, horizontal=15),
            border_radius=8,
            animate=ft.Animation(300, ft.AnimationCurve.EASE_OUT),
            on_click=self.toggle_app_status
        )
        
        # List Header Elements
        self.list_title = ft.Text(t("list.title"), size=18, weight=ft.FontWeight.BOLD, color=self.palette.text_main)
        self.stats_badge = ft.Container(
            content=ft.Text(t("list.count", count=0), size=12, color=self.palette.primary, weight=ft.FontWeight.BOLD),
            bgcolor=self.palette.bg_light_blue,
            padding=ft.padding.symmetric(horizontal=8, vertical=2),
            border_radius=10,
        )
        
        # Accounts list
        self.accounts_list = ft.Column(spacing=12, scroll=ft.ScrollMode.HIDDEN)
        self.current_email = None
        self._pending_locale_refresh = False
        
        # Start status monitoring
        self.running = True

    def did_mount(self):
        self.running = True
        self.build_ui()
        self.refresh_data()
        if self._pending_locale_refresh:
            self._pending_locale_refresh = False
            self.update_locale()
        self.monitor_thread = threading.Thread(target=self.monitor_status, daemon=True)
        self.monitor_thread.start()
        
        # 自动备份当前账号
        self.auto_backup()

    def auto_backup(self):
        def task():
            # 延迟一点时间，确保UI已加载
            time.sleep(1)
            if add_account_snapshot():
                self.refresh_data()
        threading.Thread(target=task, daemon=True).start()

    def will_unmount(self):
        self.running = False

    def update_theme(self):
        self.palette = get_palette(self.page)
        self.bgcolor = self.palette.bg_page
        
        # Update static elements
        self.list_title.color = self.palette.text_main
        self.stats_badge.bgcolor = self.palette.bg_light_blue
        self.stats_badge.content.color = self.palette.primary
        
        # Rebuild UI or refresh data to update list items
        self.refresh_data()
        self.update()

    def build_ui(self):
        self.content = ft.Column(
            [
                # 1. Status Notification Bar
                self.status_bar,
                
                ft.Container(height=20),
                
                # 2. List Header with Integrated Stats
                ft.Row(
                    [
                        ft.Row(
                            [
                                self.list_title,
                                self.stats_badge
                            ],
                            vertical_alignment=ft.CrossAxisAlignment.CENTER,
                            spacing=8
                        ),
                        ft.Container(
                            content=ft.Row(
                                [
                                    ft.Icon(AppIcons.add, size=14, color="#FFFFFF"), # Always white on primary
                                    ft.Text(t("backup.button"), size=13, color="#FFFFFF", weight=ft.FontWeight.W_600)
                                ],
                                spacing=4,
                                alignment=ft.MainAxisAlignment.CENTER
                            ),
                            bgcolor=self.palette.primary,
                            padding=ft.padding.symmetric(horizontal=16, vertical=8),
                            border_radius=8,
                            on_click=self.backup_current,
                            shadow=ft.BoxShadow(
                                spread_radius=0,
                                blur_radius=8,
                                color=ft.Colors.with_opacity(0.4, self.palette.primary),
                                offset=ft.Offset(0, 2),
                            )
                        )
                    ],
                    alignment=ft.MainAxisAlignment.SPACE_BETWEEN,
                    vertical_alignment=ft.CrossAxisAlignment.CENTER
                ),
                
                ft.Container(height=15),

                # 3. Account List Container
                ft.Container(
                    content=self.accounts_list,
                    expand=True, # Take up remaining space
                )
            ],
        )

    def refresh_data(self):
        if not self.page:
            return
        # Refresh current email
        info = get_current_account_info()
        if info and "email" in info:
            self.current_email = info["email"]
            
        # Refresh accounts list
        self.accounts_list.controls.clear()
        accounts = list_accounts_data()
        
        # Update stats badge
        self.stats_badge.content.value = t("list.count", count=len(accounts))
        
        if not accounts:
            self.accounts_list.controls.append(
                ft.Container(
                    content=ft.Column(
                        [
                            ft.Icon(AppIcons.document, size=40, color=self.palette.text_grey),
                            ft.Container(height=10),
                            ft.Text(t("list.empty"), color=self.palette.text_grey, size=14),
                        ],
                        horizontal_alignment=ft.CrossAxisAlignment.CENTER
                    ),
                    alignment=ft.alignment.center,
                    padding=40,
                    expand=True
                )
            )
        else:
            for idx, acc in enumerate(accounts):
                is_current = (acc.get('email') == self.current_email)
                self.accounts_list.controls.append(self.create_account_row(acc, is_current))
        
        self.update()

    def format_last_used(self, iso_str):
        if not iso_str:
            return t("never")
        try:
            dt = datetime.fromisoformat(iso_str)
            return dt.strftime("%Y-%m-%d %H:%M")
        except:
            return str(iso_str).split('T')[0]

    def create_account_row(self, acc, is_current):
        return ft.Container(
            content=ft.Row(
                [
                    # Left: Avatar & Info
                    ft.Row(
                        [
                            ft.Container(
                                content=ft.Text(
                                    acc['name'][0].upper() if acc['name'] else "?", 
                                    color="#FFFFFF",
                                    weight=ft.FontWeight.BOLD,
                                    size=16
                                ),
                                width=40,
                                height=40,
                                border_radius=20,
                                bgcolor=self.palette.primary if is_current else self.palette.text_grey,
                                alignment=ft.alignment.center,
                                shadow=ft.BoxShadow(
                                    spread_radius=0,
                                    blur_radius=6,
                                    color=ft.Colors.with_opacity(0.3, self.palette.primary) if is_current else "#00000000",
                                    offset=ft.Offset(0, 2),
                                )
                            ),
                            ft.Column(
                                [
                                    ft.Row(
                                        [
                                            ft.Text(acc['name'], size=15, weight=ft.FontWeight.BOLD, color=self.palette.text_main),
                                            ft.Container(
                                                content=ft.Text(t("badge.current"), size=10, color=self.palette.primary, weight=ft.FontWeight.BOLD),
                                                bgcolor=self.palette.bg_light_blue,
                                                padding=ft.padding.symmetric(horizontal=6, vertical=2),
                                                border_radius=4,
                                                visible=is_current
                                            )
                                        ],
                                        spacing=6,
                                        vertical_alignment=ft.CrossAxisAlignment.CENTER
                                    ),
                                    ft.Text(acc['email'], size=12, color=self.palette.text_grey),
                                ],
                                spacing=2,
                                alignment=ft.MainAxisAlignment.CENTER
                            )
                        ],
                        spacing=12
                    ),
                    
                    # Right: Date & Menu
                    ft.Row(
                        [
                            ft.Column(
                                [
                                    ft.Text(
                                        t("last.used"), 
                                        size=10, 
                                        color=self.palette.text_grey,
                                        text_align=ft.TextAlign.RIGHT
                                    ),
                                    ft.Text(
                                        self.format_last_used(acc.get('last_used')), 
                                        size=12, 
                                        color=self.palette.text_grey,
                                        weight=ft.FontWeight.W_500
                                    ),
                                ],
                                spacing=2,
                                alignment=ft.MainAxisAlignment.CENTER,
                                horizontal_alignment=ft.CrossAxisAlignment.END
                            ),
                            ft.PopupMenuButton(
                                icon=AppIcons.ellipsis,
                                icon_color=self.palette.text_grey,
                                items=[
                                    ft.PopupMenuItem(
                                        text=t("menu.switch"), 
                                        icon=ft.Icons.SWAP_HORIZ,
                                        on_click=lambda e: self.switch_to_account(acc['id'])
                                    ),
                                    ft.PopupMenuItem(
                                        text=t("menu.delete"), 
                                        icon=ft.Icons.DELETE_OUTLINE,
                                        on_click=lambda e: self.delete_acc(acc['id'])
                                    ),
                                ]
                            ),
                        ],
                        vertical_alignment=ft.CrossAxisAlignment.CENTER,
                        spacing=5
                    )
                ],
                alignment=ft.MainAxisAlignment.SPACE_BETWEEN,
                vertical_alignment=ft.CrossAxisAlignment.CENTER
            ),
            padding=ft.padding.symmetric(horizontal=20, vertical=12),
            bgcolor=self.palette.bg_card,
            border_radius=RADIUS_CARD,
            shadow=ft.BoxShadow(
                spread_radius=0,
                blur_radius=10,
                color=self.palette.shadow,
                offset=ft.Offset(0, 2),
            ),
            animate=ft.Animation(200, ft.AnimationCurve.EASE_OUT),
            on_hover=self.on_card_hover
        )

    def on_card_hover(self, e):
        # Only show shadow hover effect in light mode or if shadow is visible
        if self.palette.shadow != "#00000000":
            e.control.shadow.blur_radius = 15 if e.data == "true" else 10
            e.control.shadow.offset = ft.Offset(0, 6) if e.data == "true" else ft.Offset(0, 2)
            e.control.update()

    def monitor_status(self):
        while self.running:
            is_running = is_process_running()
            
            # Update Status Bar
            content_row = self.status_bar.content
            icon = content_row.controls[0]
            text = content_row.controls[1]
            
            if is_running:
                self.status_bar.bgcolor = self.palette.bg_light_green
                icon.name = AppIcons.check_circle
                icon.color = "#34C759"
                text.value = t("status.running")
                text.color = "#34C759"
            else:
                self.status_bar.bgcolor = self.palette.bg_light_red
                icon.name = AppIcons.pause_circle
                icon.color = "#FF3B30"
                text.value = t("status.stopped")
                text.color = "#FF3B30"
            
            self.update()
            time.sleep(2)

    def toggle_app_status(self, e):
        if is_process_running():
            self.stop_app(e)
        else:
            self.start_app(e)

    def show_message(self, message, is_error=False):
        dlg = ft.CupertinoAlertDialog(
            title=ft.Text(t("dialog.info")),
            content=ft.Text(message),
            actions=[
                ft.CupertinoDialogAction(
                    t("dialog.ok"), 
                    is_destructive_action=is_error,
                    on_click=lambda e: self.page.close(dlg)
                )
            ]
        )
        self.page.open(dlg)

    def start_app(self, e):
        if start_antigravity():
            pass
        else:
            self.show_message(t("start.failed"), True)

    def stop_app(self, e):
        def close_task():
            if close_antigravity():
                pass
            else:
                pass
        threading.Thread(target=close_task, daemon=True).start()

    def backup_current(self, e):
        def backup_task():
            try:
                if add_account_snapshot():
                    self.refresh_data()
                else:
                    pass
            except Exception as e:
                import traceback
                error_msg = f"{t('backup.error', error=str(e))}\n{traceback.format_exc()}"
                from utils import error
                error(error_msg)
                self.show_message(t("backup.error", error=str(e)), True)
        threading.Thread(target=backup_task, daemon=True).start()

    def show_confirm_dialog(self, title, content, on_confirm, confirm_text=None, is_destructive=False):
        confirm_text = confirm_text or t("dialog.ok")
        def handle_confirm(e):
            on_confirm()
            self.page.close(dlg)
            
        dlg = ft.CupertinoAlertDialog(
            title=ft.Text(title),
            content=ft.Text(content),
            actions=[
                ft.CupertinoDialogAction(
                    t("dialog.cancel"), 
                    on_click=lambda e: self.page.close(dlg)
                ),
                ft.CupertinoDialogAction(
                    confirm_text, 
                    is_destructive_action=is_destructive,
                    on_click=handle_confirm
                ),
            ]
        )
        self.page.open(dlg)

    def switch_to_account(self, account_id):
        def task():
            try:
                if switch_account(account_id):
                    self.refresh_data()
                    # Optional: show success message
                    # self.show_message("切换账号成功")
                else:
                    self.show_message(t("switch.failed"), True)
            except Exception as e:
                import traceback
                error_msg = f"{t('switch.error', error=str(e))}\n{traceback.format_exc()}"
                from utils import error
                error(error_msg)
                self.show_message(t("switch.error", error=str(e)), True)
        threading.Thread(target=task, daemon=True).start()

    def delete_acc(self, account_id):
        def confirm_delete():
            try:
                if delete_account(account_id):
                    self.refresh_data()
                else:
                    self.show_message(t("delete.failed"), True)
            except Exception as e:
                import traceback
                error_msg = f"{t('delete.error', error=str(e))}\n{traceback.format_exc()}"
                from utils import error
                error(error_msg)
                self.show_message(t("delete.error", error=str(e)), True)
            self.page.update()

        self.show_confirm_dialog(
            title=t("confirm.delete.title"),
            content=t("confirm.delete.content"),
            on_confirm=confirm_delete,
            confirm_text=t("confirm.delete.confirm"),
            is_destructive=True
        )

    def update_locale(self):
        if not self.page:
            self._pending_locale_refresh = True
            return
        status_row = self.status_bar.content
        status_row.controls[1].value = t("status.checking")
        self.list_title.value = t("list.title")
        self.refresh_data()
        self.build_ui()
        self.update()
