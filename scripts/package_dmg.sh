#!/bin/bash

# Configuration
APP_NAME="Antigravity Tools"
VERSION=$(grep '"version":' package.json | head -n 1 | awk -F: '{ print $2 }' | sed 's/[", ]//g')
DMG_NAME="Antigravity_Tools_${VERSION}_ManualFix.dmg"
SRC_APP_PATH="src-tauri/target/release/bundle/macos/${APP_NAME}.app"
DIST_DIR="dist_dmg"

echo "📦 开始打包 DMG (带修复脚本)..."
echo "版本: $VERSION"

# 1. 检查构建是否存在
if [ ! -d "$SRC_APP_PATH" ]; then
    echo "❌ 错误: 未找到构建好的 App。"
    echo "请先运行: npm run tauri build"
    exit 1
fi

# 2. 准备临时目录
rm -rf "$DIST_DIR"
mkdir -p "$DIST_DIR"

# 3. 复制文件
echo "Checking source app..."
cp -R "$SRC_APP_PATH" "$DIST_DIR/"
echo "Copying fix script..."
cp "scripts/Fix_Damaged.command" "$DIST_DIR/"
chmod +x "$DIST_DIR/Fix_Damaged.command"

# 4. 创建 /Applications 软连接
ln -s /Applications "$DIST_DIR/Applications"

# 5. 打包 DMG
echo "Creating DMG..."
rm -f "$DMG_NAME"
hdiutil create -volname "${APP_NAME}" -srcfolder "$DIST_DIR" -ov -format UDZO "$DMG_NAME"

# 6. 清理
rm -rf "$DIST_DIR"

echo "✅ 打包完成!"
echo "文件位置: $PWD/$DMG_NAME"
