# -*- coding: utf-8 -*-
import argparse
import sys
import os

# 将 gui 目录添加到 sys.path，以便内部模块可以相互导入 (例如 account_manager 导入 utils)
sys.path.append(os.path.join(os.path.dirname(__file__), "gui"))

# 支持直接运行和作为模块导入
# 支持直接运行和作为模块导入
try:
    from gui.utils import info, error, warning
    from gui.account_manager import (
        list_accounts_data,
        add_account_snapshot,
        switch_account,
        delete_account
    )
    from gui.process_manager import start_antigravity, close_antigravity
    from gui.localization import t, set_language, get_language
except ImportError as e:
    print(f"Import Error: {e}")
    sys.exit(1)


def initialize_language(lang=None):
    if lang:
        set_language(lang)
    else:
        get_language()

def show_menu():
    """显示主菜单"""
    print("\n" + "="*50)
    print(t("cli.title"))
    print("="*50)
    print(f"\n{t('cli.choose')}")
    print(f"  1. {t('cli.menu.list')}")
    print(f"  2. {t('cli.menu.add')}")
    print(f"  3. {t('cli.menu.switch')}")
    print(f"  4. {t('cli.menu.delete')}")
    print(f"  5. {t('cli.menu.start')}")
    print(f"  6. {t('cli.menu.stop')}")
    print(f"  0. {t('cli.menu.exit')}")
    print("-"*50)

def list_accounts():
    """列出所有账号"""
    accounts = list_accounts_data()
    if not accounts:
        info(t("cli.no.records"))
        return []
    else:
        print("\n" + "="*50)
        info(t("cli.total", count=len(accounts)))
        print("="*50)
        for idx, acc in enumerate(accounts, 1):
            print(f"\n{idx}. {acc['name']}")
            print(f"   📧 {t('cli.email')}: {acc['email']}")
            print(f"   🆔 {t('cli.id')}: {acc['id']}")
            print(f"   ⏰ {t('cli.last_used')}: {acc['last_used']}")
            print("-" * 50)
        return accounts

def add_account():
    """添加账号备份"""
    print("\n" + "="*50)
    print(t("cli.add.title"))
    print("="*50)
    
    name = input(f"\n{t('cli.prompt.name')}").strip()
    email = input(t("cli.prompt.email")).strip()
    
    name = name if name else None
    email = email if email else None
    
    print()
    if add_account_snapshot(name, email):
        info(t("cli.add.success"))
    else:
        error(t("cli.add.fail"))

def switch_account_interactive():
    """交互式切换账号"""
    accounts = list_accounts()
    if not accounts:
        return
    
    print("\n" + "="*50)
    print(t("cli.switch.title"))
    print("="*50)
    
    choice = input(f"\n{t('cli.prompt.switch')}").strip()
    
    if not choice:
        warning(t("cli.cancelled"))
        return
    
    real_id = resolve_id(choice)
    if not real_id:
        error(t("cli.invalid.index", value=choice))
        return
    
    print()
    if switch_account(real_id):
        info(t("cli.switch.success"))
    else:
        error(t("cli.switch.fail"))

def delete_account_interactive():
    """交互式删除账号"""
    accounts = list_accounts()
    if not accounts:
        return
    
    print("\n" + "="*50)
    print(t("cli.delete.title"))
    print("="*50)
    
    choice = input(f"\n{t('cli.prompt.delete')}").strip()
    
    if not choice:
        warning(t("cli.cancelled"))
        return
    
    real_id = resolve_id(choice)
    if not real_id:
        error(t("cli.invalid.index", value=choice))
        return
    
    # 确认删除
    confirm = input(f"\n{t('cli.confirm.delete')}").strip().lower()
    if confirm != 'y':
        warning(t("cli.delete.cancel"))
        return
    
    print()
    if delete_account(real_id):
        info(t("cli.delete.success"))
    else:
        error(t("cli.delete.fail"))

def interactive_mode():
    """交互式菜单模式"""
    while True:
        show_menu()
        choice = input(t("cli.prompt.option")).strip()
        
        if choice == "1":
            list_accounts()
            input(f"\n{t('cli.prompt.continue')}")
            
        elif choice == "2":
            add_account()
            input(f"\n{t('cli.prompt.continue')}")
            
        elif choice == "3":
            switch_account_interactive()
            input(f"\n{t('cli.prompt.continue')}")
            
        elif choice == "4":
            delete_account_interactive()
            input(f"\n{t('cli.prompt.continue')}")
            
        elif choice == "5":
            print()
            start_antigravity()
            input(f"\n{t('cli.prompt.continue')}")
            
        elif choice == "6":
            print()
            close_antigravity()
            input(f"\n{t('cli.prompt.continue')}")
            
        elif choice == "0":
            print(f"\n{t('cli.exit')}")
            sys.exit(0)
            
        else:
            error(t("cli.invalid.option"))
            input(f"\n{t('cli.prompt.continue')}")

def cli_mode():
    """命令行模式"""
    initialize_language()
    parser = argparse.ArgumentParser(description=t("cli.title"))
    parser.add_argument("--lang", "-l", choices=["zh", "en"], help="Language code")
    subparsers = parser.add_subparsers(dest="command", help=t("cli.choose"))

    # List
    subparsers.add_parser("list", help=t("cli.menu.list"))

    # Add
    add_parser = subparsers.add_parser("add", help=t("cli.menu.add"))
    add_parser.add_argument("--name", "-n", help=t("cli.name"))
    add_parser.add_argument("--email", "-e", help=t("cli.email"))

    # Switch
    switch_parser = subparsers.add_parser("switch", help=t("cli.menu.switch"))
    switch_parser.add_argument("--id", "-i", required=True, help=t("cli.id"))

    # Delete
    del_parser = subparsers.add_parser("delete", help=t("cli.menu.delete"))
    del_parser.add_argument("--id", "-i", required=True, help=t("cli.id"))
    
    # Process Control
    subparsers.add_parser("start", help=t("cli.menu.start"))
    subparsers.add_parser("stop", help=t("cli.menu.stop"))

    args = parser.parse_args()

    initialize_language(args.lang)

    if args.command == "list":
        list_accounts()

    elif args.command == "add":
        if add_account_snapshot(args.name, args.email):
            info(t("cli.interactive.added"))
        else:
            sys.exit(1)

    elif args.command == "switch":
        real_id = resolve_id(args.id)
        if not real_id:
            error(t("cli.switch.invalid", value=args.id))
            sys.exit(1)
            
        if switch_account(real_id):
            info(t("cli.switch.success"))
        else:
            sys.exit(1)

    elif args.command == "delete":
        real_id = resolve_id(args.id)
        if not real_id:
            error(t("cli.delete.invalid", value=args.id))
            sys.exit(1)

        if delete_account(real_id):
            info(t("cli.delete.success"))
        else:
            sys.exit(1)
            
    elif args.command == "start":
        start_antigravity()
        
    elif args.command == "stop":
        close_antigravity()

    else:
        # 没有参数时，进入交互式模式
        interactive_mode()

def main():
    """主入口"""
    # 如果没有命令行参数，进入交互式模式
    if len(sys.argv) == 1:
        initialize_language()
        interactive_mode()
    else:
        cli_mode()

def resolve_id(input_id):
    """解析 ID，支持 UUID 或 序号"""
    accounts = list_accounts_data()
    
    # 1. 尝试作为序号处理
    if input_id.isdigit():
        idx = int(input_id)
        if 1 <= idx <= len(accounts):
            return accounts[idx-1]['id']
            
    # 2. 尝试作为 UUID 匹配
    for acc in accounts:
        if acc['id'] == input_id:
            return input_id
            
    return None

if __name__ == "__main__":
    main()
