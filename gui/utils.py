# -*- coding: utf-8 -*-
import os
import sys
import platform
from pathlib import Path
from datetime import datetime

# -------------------------------------------------------------------------
# 日志工具
# -------------------------------------------------------------------------


def _t(key, **kwargs):
    try:
        from localization import t

        return t(key, **kwargs)
    except Exception:
        try:
            return key.format(**kwargs)
        except Exception:
            return key

def get_log_file_path():
    """获取日志文件路径"""
    try:
        log_dir = get_app_data_dir()
        return log_dir / "app.log"
    except:
        return None

def _log_to_file(message):
    """写入日志到文件"""
    try:
        log_file = get_log_file_path()
        if log_file:
            timestamp = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
            with open(log_file, "a", encoding="utf-8") as f:
                f.write(f"[{timestamp}] {message}\n")
    except:
        pass

def _print_with_color(color_code, symbol, message):
    """带颜色的打印函数，同时写入文件"""
    formatted_msg = f"{symbol} {message}"
    # 在无控制台模式下，sys.stdout 可能为 None，直接打印会报错
    if sys.stdout:
        try:
            print(f"\033[{color_code}m{formatted_msg}\033[0m")
        except:
            pass
    _log_to_file(formatted_msg)

def info(message):
    """打印信息日志 (绿色)"""
    _print_with_color("32", "INFO", message)

def warning(message):
    """打印警告日志 (黄色)"""
    _print_with_color("33", "WARN", message)

def error(message):
    """打印错误日志 (红色)"""
    _print_with_color("31", "ERR ", message)

def debug(message):
    """打印调试日志 (灰色)"""
    # 只有在设置了DEBUG环境变量时才打印
    if os.environ.get("DEBUG"):
        _print_with_color("90", "DBUG", message)
    else:
        # 在打包应用中，我们也希望记录调试信息到文件，方便排查
        _log_to_file(f"DBUG {message}")

# -------------------------------------------------------------------------
# 路径工具
# -------------------------------------------------------------------------

def get_app_data_dir():
    """获取应用数据目录 (~/.antigravity-agent)"""
    home = Path.home()
    config_dir = home / ".antigravity-agent"
    if not config_dir.exists():
        config_dir.mkdir(parents=True, exist_ok=True)
    return config_dir

def get_accounts_file_path():
    """获取账号存储文件路径"""
    return get_app_data_dir() / "antigravity_accounts.json"

def get_antigravity_db_paths():
    """获取 Antigravity 数据库可能的路径"""
    system = platform.system()
    paths = []
    home = Path.home()

    if system == "Darwin":  # macOS
        # 标准路径: ~/Library/Application Support/Antigravity/User/globalStorage/state.vscdb
        paths.append(home / "Library/Application Support/Antigravity/User/globalStorage/state.vscdb")
        # 备用路径 (旧版本可能的位置)
        paths.append(home / "Library/Application Support/Antigravity/state.vscdb")
    elif system == "Windows":
        # 标准路径: %APPDATA%/Antigravity/state.vscdb
        appdata = os.environ.get("APPDATA")
        if appdata:
            base_path = Path(appdata) / "Antigravity"
            # 参考 cursor_reset.py 的路径结构
            paths.append(base_path / "User/globalStorage/state.vscdb")
            paths.append(base_path / "User/state.vscdb")
            paths.append(base_path / "state.vscdb")
    elif system == "Linux":
        # 标准路径: ~/.config/Antigravity/state.vscdb
        paths.append(home / ".config/Antigravity/state.vscdb")
    
    return paths

def get_antigravity_executable_path():
    """获取 Antigravity 可执行文件路径"""
    system = platform.system()
    
    if system == "Darwin":
        return Path("/Applications/Antigravity.app/Contents/MacOS/Antigravity")
    elif system == "Windows":
        # 参考 cursor_reset.py 的查找逻辑
        local_app_data = Path(os.environ.get("LOCALAPPDATA", ""))
        program_files = Path(os.environ.get("ProgramFiles", "C:\\Program Files"))
        program_files_x86 = Path(os.environ.get("ProgramFiles(x86)", "C:\\Program Files (x86)"))
        
        possible_paths = [
            local_app_data / "Programs/Antigravity/Antigravity.exe",
            program_files / "Antigravity/Antigravity.exe",
            program_files_x86 / "Antigravity/Antigravity.exe"
        ]
        
        for path in possible_paths:
            if path.exists():
                return path
                
        # Fallback to default if nothing found (though likely won't exist)
        return local_app_data / "Programs/Antigravity/Antigravity.exe"
        
    elif system == "Linux":
        return Path("/usr/share/antigravity/antigravity")
    
    return None

def open_uri(uri):
    """跨平台打开 URI 协议
    
    Args:
        uri: 要打开的 URI，例如 "antigravity://oauth-success"
        
    Returns:
        bool: 是否成功启动
    """
    import subprocess
    system = platform.system()
    
    try:
        if system == "Darwin":
            # macOS: 使用 open 命令
            subprocess.Popen(["open", uri])
        elif system == "Windows":
            # Windows: 使用 start 命令
            # CREATE_NO_WINDOW = 0x08000000
            subprocess.Popen(["cmd", "/c", "start", "", uri], shell=False, creationflags=0x08000000)
        elif system == "Linux":
            # Linux: 使用 xdg-open
            subprocess.Popen(["xdg-open", uri])
        else:
            error(_t("log.uri.unsupported", platform=system))
            return False
        
        return True
    except Exception as e:
        error(_t("log.uri.fail", error=e))
        return False
