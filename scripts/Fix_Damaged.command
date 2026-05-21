#!/bin/bash

# 获取当前脚本所在目录
DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
APP_PATH="$DIR/Antigravity Tools.app"

# 定义颜色
RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m' # No Color

echo -e "${GREEN}==============================================${NC}"
echo -e "${GREEN}   Antigravity Tools - 快速修复助手${NC}"
echo -e "${GREEN}==============================================${NC}"
echo ""

if [ -d "$APP_PATH" ]; then
    echo "📍 正在尝试修复应用: $APP_PATH"
    echo "🔑 请输入您的开机密码以授予权限 (输入时不会显示)..."
    echo ""
    
    # 尝试移除隔离属性
    sudo xattr -rd com.apple.quarantine "$APP_PATH"
    
    if [ $? -eq 0 ]; then
        echo ""
        echo -e "${GREEN}✅ 修复成功!${NC}"
        echo "您现在可以像往常一样打开应用了。"
        
        # 尝试通过 AppleScript 弹窗提示成功
        osascript -e 'display notification "修复成功，现在可以打开应用了" with title "Antigravity Tools" sound name "Glass"'
    else
        echo ""
        echo -e "${RED}❌ 修复失败${NC}"
        echo "请检查密码是否输入正确，或稍后重试。"
    fi
else
    echo -e "${RED}⚠️  未找到应用文件${NC}"
    echo "请确保将此修复脚本和 'Antigravity Tools.app' 放在同一个文件夹内 (通常是 /Applications)。"
fi

echo ""
echo "按任意键退出..."
read -n 1 -s -r -p ""
