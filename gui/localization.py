import json
import locale
from pathlib import Path

from utils import get_app_data_dir, debug


DEFAULT_LANGUAGE = "zh"

LANGUAGE_LABELS = {
    "zh": "简体中文",
    "en": "English",
}


TRANSLATIONS = {
    "zh": {
        "app.title": "Antigravity Manager",
        "app.brand": "Antigravity",
        "app.fullname": "Antigravity Manager",
        "nav.dashboard": "仪表盘",
        "nav.settings": "设置",
        "status.checking": "正在检测状态...",
        "status.running": "Antigravity 正在后台运行中",
        "status.stopped": "Antigravity 服务已停止 (点击启动)",
        "list.title": "账号列表",
        "list.count": "{count} 个备份",
        "list.empty": "暂无备份记录",
        "backup.button": "备份当前",
        "badge.current": "当前",
        "last.used": "上次使用",
        "menu.switch": "切换到此账号",
        "menu.delete": "删除备份",
        "dialog.info": "提示",
        "dialog.ok": "确定",
        "dialog.cancel": "取消",
        "confirm.delete.title": "确认删除",
        "confirm.delete.content": "确定要删除这个账号备份吗？此操作无法撤销。",
        "confirm.delete.confirm": "删除",
        "start.failed": "启动失败",
        "switch.failed": "切换账号失败，请检查日志",
        "switch.error": "发生错误: {error}",
        "delete.failed": "删除账号失败，请检查日志",
        "delete.error": "删除错误: {error}",
        "backup.error": "备份错误: {error}",
        "never": "从未",
        "settings.title": "设置",
        "settings.data": "数据管理",
        "settings.local_dir": "本地数据目录",
        "settings.view_backups": "查看备份文件和数据库",
        "settings.open_folder": "打开文件夹",
        "settings.about": "关于",
        "settings.version": "版本",
        "settings.system": "系统",
        "settings.python": "Python",
        "settings.author": "作者：",
        "settings.wechat": "公众号：",
        "settings.logs": "系统日志",
        "settings.language": "界面语言",
        "language.en": "English",
        "language.zh": "简体中文",
        "cli.title": "🚀 Antigravity 账号管理工具",
        "cli.choose": "请选择操作：",
        "cli.menu.list": "📋 列出所有备份",
        "cli.menu.add": "➕ 添加/更新备份",
        "cli.menu.switch": "🔄 切换/恢复备份",
        "cli.menu.delete": "🗑️  删除备份",
        "cli.menu.start": "▶️  启动 Antigravity",
        "cli.menu.stop": "⏹️  关闭 Antigravity",
        "cli.menu.exit": "🚪 退出",
        "cli.prompt.option": "请输入选项 (0-6): ",
        "cli.no.records": "暂无存档",
        "cli.total": "共有 {count} 个存档:",
        "cli.name": "名称",
        "cli.email": "邮箱",
        "cli.id": "ID",
        "cli.last_used": "最后使用",
        "cli.add.title": "➕ 添加/更新账号备份",
        "cli.prompt.name": "请输入账号名称（留空自动生成）: ",
        "cli.prompt.email": "请输入邮箱（留空自动识别）: ",
        "cli.prompt.continue": "按回车键继续...",
        "cli.add.success": "✅ 操作成功！",
        "cli.add.fail": "❌ 操作失败！",
        "cli.switch.title": "🔄 切换/恢复账号",
        "cli.prompt.switch": "请输入要切换的账号序号: ",
        "cli.invalid.index": "❌ 无效的序号: {value}",
        "cli.delete.title": "🗑️  删除账号备份",
        "cli.prompt.delete": "请输入要删除的账号序号: ",
        "cli.cancelled": "已取消操作",
        "cli.confirm.delete": "⚠️  确定要删除该账号吗？(y/N): ",
        "cli.delete.cancel": "已取消删除",
        "cli.switch.success": "✅ 切换成功！",
        "cli.switch.fail": "❌ 切换失败！",
        "cli.delete.success": "✅ 删除成功！",
        "cli.delete.fail": "❌ 删除失败！",
        "cli.invalid.option": "❌ 无效的选项，请重新选择",
        "cli.exit": "👋 再见！",
        "cli.interactive.added": "存档添加成功",
        "cli.switch.invalid": "无效的 ID 或序号: {value}",
        "cli.delete.invalid": "无效的 ID 或序号: {value}",
        "log.auto.email": "正在尝试从数据库读取账号信息...",
        "log.found.email": "自动获取到邮箱: {email}",
        "log.email.notfound": "无法从数据库自动获取邮箱，将使用 'Unknown'",
        "log.generated.name": "使用自动生成的名称: {name}",
        "log.accounts.load.error": "加载账号列表失败: {error}",
        "log.accounts.save.error": "保存账号列表失败: {error}",
        "log.existing.backup": "检测到邮箱 {email} 已存在备份，将覆盖旧备份",
        "log.create.backup": "创建新账号备份: {email}",
        "log.backup.start": "正在备份当前状态为账号: {name}",
        "log.backup.fail": "备份失败，取消添加账号",
        "log.backup.updated": "账号 {name} ({email}) 备份已更新",
        "log.backup.added": "账号 {name} ({email}) 添加成功",
        "log.account.missing": "账号不存在",
        "log.backup.deleted": "备份文件已删除: {path}",
        "log.backup.delete.fail": "删除备份文件失败: {error}",
        "log.account.deleted": "账号 {name} 已删除",
        "log.backup.missing": "备份文件丢失: {path}",
        "log.switch.prepare": "准备切换到账号: {name}",
        "log.close.fail": "无法关闭 Antigravity，尝试强制恢复...",
        "log.restore.fail": "恢复数据失败",
        "log.switch.success": "切换到账号 {name} 成功",
        "log.close.start": "正在尝试关闭 Antigravity...",
        "log.close.unknown": "未知系统平台: {platform}，将尝试通用方法",
        "log.close.script": "尝试通过 AppleScript 优雅退出 Antigravity...",
        "log.close.script.fail": "AppleScript 退出失败: {error}，将使用其他方式",
        "log.close.taskkill": "尝试通过 taskkill 优雅退出 Antigravity...",
        "log.close.taskkill.fail": "taskkill 退出失败: {error}，将使用其他方式",
        "log.close.request": "已发送退出请求，等待应用响应...",
        "log.close.detected": "发现目标进程: {name} ({pid}) - {path}",
        "log.close.done": "所有 Antigravity 进程已正常关闭",
        "log.close.remaining": "检测到 {count} 个进程仍在运行",
        "log.close.term": "发送终止信号 (SIGTERM)...",
        "log.close.wait": "等待进程退出（最多 {seconds} 秒）...",
        "log.close.force": "发送强制终止信号 (SIGKILL)...",
        "log.close.still": "仍有 {count} 个进程未退出: {processes}",
        "log.close.unable": "无法终止的进程: {processes}",
        "log.close.partial": "部分进程未能关闭，请手动关闭后重试",
        "log.close.error": "关闭 Antigravity 进程时发生错误: {error}",
        "log.start": "正在启动 Antigravity...",
        "log.start.uri": "使用 URI 协议启动...",
        "log.start.uri.sent": "Antigravity URI 启动命令已发送",
        "log.start.uri.fail": "URI 启动失败，尝试使用可执行文件路径...",
        "log.start.path": "使用可执行文件路径启动...",
        "log.start.path.missing": "找不到 Antigravity 可执行文件",
        "log.start.path.hint": "提示：可以尝试使用 URI 协议启动（use_uri=True）",
        "log.start.sent": "Antigravity 启动命令已发送",
        "log.start.error": "启动进程时出错: {error}",
        "log.db.locked": "数据库被锁定: {error}",
        "log.db.locked.hint": "提示: 请确保 Antigravity 应用已完全关闭",
        "log.db.connect.fail": "连接数据库失败: {error}",
        "log.db.unexpected": "连接数据库时发生意外错误: {error}",
        "log.db.missing": "未找到 Antigravity 数据库路径",
        "log.db.path.missing": "数据库文件不存在: {path}",
        "log.db.backup.start": "正在从数据库备份数据: {path}",
        "log.db.field.backup": "备份字段: {field}",
        "log.db.field.missing": "字段不存在: {field}",
        "log.db.backup.success": "备份成功: {path}",
        "log.db.query.error": "数据库查询出错: {error}",
        "log.db.backup.error": "备份过程出错: {error}",
        "log.backupfile.missing": "备份文件不存在: {path}",
        "log.backupfile.readfail": "读取备份文件失败: {error}",
        "log.db.restore.title": "正在恢复数据库: {path}",
        "log.db.field.restore": "恢复字段: {field}",
        "log.db.restore.done": "数据库恢复完成: {path}",
        "log.db.write.error": "数据库写入出错: {error}",
        "log.db.restore.error": "恢复过程出错: {error}",
        "log.db.extract.error": "提取账号信息出错: {error}",
        "log.uri.unsupported": "不支持的操作系统: {platform}",
        "log.uri.fail": "打开 URI 失败: {error}",
        "log.process.stopped": "Antigravity 服务已停止",
    },
    "en": {
        "app.title": "Antigravity Manager",
        "app.brand": "Antigravity",
        "app.fullname": "Antigravity Manager",
        "nav.dashboard": "Dashboard",
        "nav.settings": "Settings",
        "status.checking": "Checking status...",
        "status.running": "Antigravity is running",
        "status.stopped": "Antigravity is stopped (tap to start)",
        "list.title": "Account List",
        "list.count": "{count} backups",
        "list.empty": "No backups yet",
        "backup.button": "Backup current",
        "badge.current": "Current",
        "last.used": "Last used",
        "menu.switch": "Switch to this account",
        "menu.delete": "Delete backup",
        "dialog.info": "Notice",
        "dialog.ok": "OK",
        "dialog.cancel": "Cancel",
        "confirm.delete.title": "Confirm deletion",
        "confirm.delete.content": "Delete this backup? This action cannot be undone.",
        "confirm.delete.confirm": "Delete",
        "start.failed": "Start failed",
        "switch.failed": "Switch failed, check logs",
        "switch.error": "Error occurred: {error}",
        "delete.failed": "Delete failed, check logs",
        "delete.error": "Delete error: {error}",
        "backup.error": "Backup error: {error}",
        "never": "Never",
        "settings.title": "Settings",
        "settings.data": "Data",
        "settings.local_dir": "Local data folder",
        "settings.view_backups": "See backups and database",
        "settings.open_folder": "Open folder",
        "settings.about": "About",
        "settings.version": "Version",
        "settings.system": "System",
        "settings.python": "Python",
        "settings.author": "Author:",
        "settings.wechat": "WeChat:",
        "settings.logs": "System logs",
        "settings.language": "Language",
        "language.en": "English",
        "language.zh": "简体中文",
        "cli.title": "🚀 Antigravity Account Manager",
        "cli.choose": "Choose an action:",
        "cli.menu.list": "📋 List backups",
        "cli.menu.add": "➕ Add/Update backup",
        "cli.menu.switch": "🔄 Switch/Restore backup",
        "cli.menu.delete": "🗑️  Delete backup",
        "cli.menu.start": "▶️  Start Antigravity",
        "cli.menu.stop": "⏹️  Stop Antigravity",
        "cli.menu.exit": "🚪 Exit",
        "cli.prompt.option": "Select option (0-6): ",
        "cli.no.records": "No backups found",
        "cli.total": "Total {count} backups:",
        "cli.name": "Name",
        "cli.email": "Email",
        "cli.id": "ID",
        "cli.last_used": "Last used",
        "cli.add.title": "➕ Add/Update backup",
        "cli.prompt.name": "Enter account name (leave blank to auto): ",
        "cli.prompt.email": "Enter email (leave blank to detect): ",
        "cli.prompt.continue": "Press Enter to continue...",
        "cli.add.success": "✅ Success!",
        "cli.add.fail": "❌ Failed!",
        "cli.switch.title": "🔄 Switch/Restore backup",
        "cli.prompt.switch": "Enter the index to switch: ",
        "cli.invalid.index": "❌ Invalid index: {value}",
        "cli.delete.title": "🗑️  Delete backup",
        "cli.prompt.delete": "Enter the index to delete: ",
        "cli.cancelled": "Cancelled",
        "cli.confirm.delete": "⚠️  Delete this backup? (y/N): ",
        "cli.delete.cancel": "Delete cancelled",
        "cli.switch.success": "✅ Switched!",
        "cli.switch.fail": "❌ Switch failed!",
        "cli.delete.success": "✅ Deleted!",
        "cli.delete.fail": "❌ Delete failed!",
        "cli.invalid.option": "❌ Invalid option, please retry",
        "cli.exit": "👋 Bye!",
        "cli.interactive.added": "Backup added successfully",
        "cli.switch.invalid": "Invalid ID or index: {value}",
        "cli.delete.invalid": "Invalid ID or index: {value}",
        "log.auto.email": "Reading account info from database...",
        "log.found.email": "Detected email: {email}",
        "log.email.notfound": "Could not detect email, using 'Unknown'",
        "log.generated.name": "Generated name: {name}",
        "log.accounts.load.error": "Failed to load accounts: {error}",
        "log.accounts.save.error": "Failed to save accounts: {error}",
        "log.existing.backup": "Email {email} already exists, updating backup",
        "log.create.backup": "Creating new backup for {email}",
        "log.backup.start": "Backing up current state for: {name}",
        "log.backup.fail": "Backup failed, canceling add",
        "log.backup.updated": "Backup updated for {name} ({email})",
        "log.backup.added": "Backup added for {name} ({email})",
        "log.account.missing": "Account not found",
        "log.backup.deleted": "Backup file removed: {path}",
        "log.backup.delete.fail": "Failed to delete backup file: {error}",
        "log.account.deleted": "Account {name} deleted",
        "log.backup.missing": "Backup file missing: {path}",
        "log.switch.prepare": "Preparing to switch to: {name}",
        "log.close.fail": "Unable to close Antigravity, trying forced restore...",
        "log.restore.fail": "Restore failed",
        "log.switch.success": "Switched to {name} successfully",
        "log.close.start": "Attempting to close Antigravity...",
        "log.close.unknown": "Unknown platform: {platform}, using generic strategy",
        "log.close.script": "Trying AppleScript to quit Antigravity...",
        "log.close.script.fail": "AppleScript quit failed: {error}, trying other ways",
        "log.close.taskkill": "Trying taskkill to quit Antigravity...",
        "log.close.taskkill.fail": "taskkill failed: {error}, trying other ways",
        "log.close.request": "Exit request sent, waiting for response...",
        "log.close.detected": "Found process: {name} ({pid}) - {path}",
        "log.close.done": "All Antigravity processes closed",
        "log.close.remaining": "{count} processes still running",
        "log.close.term": "Sending SIGTERM...",
        "log.close.wait": "Waiting for processes (max {seconds}s)...",
        "log.close.force": "Sending SIGKILL...",
        "log.close.still": "Still {count} processes running: {processes}",
        "log.close.unable": "Processes not killed: {processes}",
        "log.close.partial": "Some processes not closed, please close manually",
        "log.close.error": "Error closing Antigravity: {error}",
        "log.start": "Starting Antigravity...",
        "log.start.uri": "Starting via URI...",
        "log.start.uri.sent": "URI launch sent",
        "log.start.uri.fail": "URI launch failed, trying executable...",
        "log.start.path": "Starting via executable path...",
        "log.start.path.missing": "Antigravity executable not found",
        "log.start.path.hint": "Tip: try using URI launch (use_uri=True)",
        "log.start.sent": "Launch command sent",
        "log.start.error": "Error starting process: {error}",
        "log.db.locked": "Database locked: {error}",
        "log.db.locked.hint": "Hint: make sure Antigravity is closed",
        "log.db.connect.fail": "Failed to connect to database: {error}",
        "log.db.unexpected": "Unexpected database error: {error}",
        "log.db.missing": "Antigravity database path not found",
        "log.db.path.missing": "Database file missing: {path}",
        "log.db.backup.start": "Backing up from database: {path}",
        "log.db.field.backup": "Backing up field: {field}",
        "log.db.field.missing": "Field missing: {field}",
        "log.db.backup.success": "Backup saved: {path}",
        "log.db.query.error": "Database query error: {error}",
        "log.db.backup.error": "Backup error: {error}",
        "log.backupfile.missing": "Backup file missing: {path}",
        "log.backupfile.readfail": "Failed to read backup file: {error}",
        "log.db.restore.title": "Restoring database: {path}",
        "log.db.field.restore": "Restored field: {field}",
        "log.db.restore.done": "Database restore complete: {path}",
        "log.db.write.error": "Database write error: {error}",
        "log.db.restore.error": "Restore error: {error}",
        "log.db.extract.error": "Error extracting account info: {error}",
        "log.uri.unsupported": "Unsupported platform: {platform}",
        "log.uri.fail": "Failed to open URI: {error}",
        "log.process.stopped": "Antigravity stopped",
    },
}


_current_language = None


def _settings_path() -> Path:
    return get_app_data_dir() / "settings.json"


def _load_settings() -> dict:
    path = _settings_path()
    if not path.exists():
        return {}
    try:
        with open(path, "r", encoding="utf-8") as f:
            return json.load(f)
    except Exception as e:
        debug(f"Failed to load settings: {e}")
        return {}


def _save_settings(data: dict):
    path = _settings_path()
    try:
        with open(path, "w", encoding="utf-8") as f:
            json.dump(data, f, ensure_ascii=False, indent=2)
    except Exception as e:
        debug(f"Failed to save settings: {e}")


def get_language() -> str:
    global _current_language
    if _current_language:
        return _current_language

    settings = _load_settings()
    lang = settings.get("language")
    if lang in TRANSLATIONS:
        _current_language = lang
        return _current_language

    # First run: default to Chinese and persist
    _current_language = DEFAULT_LANGUAGE
    settings["language"] = _current_language
    _save_settings(settings)
    return _current_language


def set_language(lang: str):
    global _current_language
    if lang not in TRANSLATIONS:
        return False
    _current_language = lang
    settings = _load_settings()
    settings["language"] = lang
    _save_settings(settings)
    return True


def t(key: str, **kwargs) -> str:
    lang = get_language()
    template = TRANSLATIONS.get(lang, {}).get(key)
    if template is None:
        template = TRANSLATIONS.get(DEFAULT_LANGUAGE, {}).get(key, key)
    try:
        return template.format(**kwargs) if kwargs else template
    except Exception:
        return template


def get_language_options():
    return [
        {"code": code, "label": LANGUAGE_LABELS.get(code, code)}
        for code in TRANSLATIONS.keys()
    ]
