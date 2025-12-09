# -*- coding: utf-8 -*-
import json
import os
import time
import uuid
from pathlib import Path
from datetime import datetime

# Use relative imports
from utils import info, error, warning, get_accounts_file_path, get_app_data_dir
from localization import t
from db_manager import backup_account, restore_account, get_current_account_info
from process_manager import close_antigravity, start_antigravity

def load_accounts():
    """加载账号列表"""
    file_path = get_accounts_file_path()
    if not file_path.exists():
        return {}
    
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            return json.load(f)
    except Exception as e:
        error(t("log.accounts.load.error", error=e))
        return {}

def save_accounts(accounts):
    """保存账号列表"""
    file_path = get_accounts_file_path()
    try:
        with open(file_path, 'w', encoding='utf-8') as f:
            json.dump(accounts, f, ensure_ascii=False, indent=2)
        return True
    except Exception as e:
        error(t("log.accounts.save.error", error=e))
        return False

def add_account_snapshot(name=None, email=None):
    """添加当前状态为新账号，如果邮箱已存在则覆盖"""
    # 0. 自动获取信息
    if not email:
        info(t("log.auto.email"))
        account_info = get_current_account_info()
        if account_info and "email" in account_info:
            email = account_info["email"]
            info(t("log.found.email", email=email))
        else:
            warning(t("log.email.notfound"))
            email = "Unknown"
            
    if not name:
        # 如果没有提供名称，使用邮箱前缀或默认名称
        if email and email != "Unknown":
            name = email.split("@")[0]
        else:
            name = f"Account_{int(time.time())}"
        info(t("log.generated.name", name=name))

    # 1. 检查是否已存在相同邮箱的账号
    accounts = load_accounts()
    existing_account = None
    existing_id = None
    
    for acc_id, acc_data in accounts.items():
        if acc_data.get("email") == email:
            existing_account = acc_data
            existing_id = acc_id
            break
    
    if existing_account:
        info(t("log.existing.backup", email=email))
        # 使用已有的 ID 和备份路径
        account_id = existing_id
        backup_path = Path(existing_account["backup_file"])
        created_at = existing_account.get("created_at", datetime.now().isoformat())
        
        # 如果没有提供新名称，保留原名称
        if not name or name == email.split("@")[0]:
            name = existing_account.get("name", name)
    else:
        info(t("log.create.backup", email=email))
        # 生成新的 ID 和备份路径
        account_id = str(uuid.uuid4())
        backup_filename = f"{account_id}.json"
        backup_dir = get_app_data_dir() / "backups"
        backup_dir.mkdir(exist_ok=True)
        backup_path = backup_dir / backup_filename
        created_at = datetime.now().isoformat()
    
    # 2. 执行备份
    info(t("log.backup.start", name=name))
    if not backup_account(email, str(backup_path)):
        error(t("log.backup.fail"))
        return False
    
    # 3. 更新账号列表
    accounts[account_id] = {
        "id": account_id,
        "name": name,
        "email": email,
        "backup_file": str(backup_path),
        "created_at": created_at,
        "last_used": datetime.now().isoformat()
    }
    
    if save_accounts(accounts):
        if existing_account:
            info(t("log.backup.updated", name=name, email=email))
        else:
            info(t("log.backup.added", name=name, email=email))
        return True
    return False

def delete_account(account_id):
    """删除账号"""
    accounts = load_accounts()
    if account_id not in accounts:
        error(t("log.account.missing"))
        return False
    
    account = accounts[account_id]
    name = account.get("name", "Unknown")
    backup_file = account.get("backup_file")
    
    # 删除备份文件
    if backup_file and os.path.exists(backup_file):
        try:
            os.remove(backup_file)
            info(t("log.backup.deleted", path=backup_file))
        except Exception as e:
            warning(t("log.backup.delete.fail", error=e))
    
    # 从列表中移除
    del accounts[account_id]
    if save_accounts(accounts):
        info(t("log.account.deleted", name=name))
        return True
    return False

def switch_account(account_id):
    """切换到指定账号"""
    accounts = load_accounts()
    if account_id not in accounts:
        error(t("log.account.missing"))
        return False
    
    account = accounts[account_id]
    name = account.get("name", "Unknown")
    backup_file = account.get("backup_file")
    
    if not backup_file or not os.path.exists(backup_file):
        error(t("log.backup.missing", path=backup_file))
        return False
    
    info(t("log.switch.prepare", name=name))
    
    # 1. 关闭进程
    if not close_antigravity():
        # 尝试继续，但给出警告
        warning(t("log.close.fail"))
    
    # 2. 恢复数据
    if restore_account(backup_file):
        # 更新最后使用时间
        accounts[account_id]["last_used"] = datetime.now().isoformat()
        save_accounts(accounts)
        
        # 3. 启动进程
        start_antigravity()
        info(t("log.switch.success", name=name))
        return True
    else:
        error(t("log.restore.fail"))
        return False

def list_accounts_data():
    """获取账号列表数据 (用于显示)"""
    accounts = load_accounts()
    data = list(accounts.values())
    # 按最后使用时间倒序排序
    data.sort(key=lambda x: x.get("last_used", ""), reverse=True)
    return data
