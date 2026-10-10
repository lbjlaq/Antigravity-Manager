#!/bin/bash
set -e
ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

echo "=========================================="
echo "🚀 编译打包: 专属纯净增强版 (无中转站)"
echo "=========================================="

echo "1. 构建前端..."
npm run build

echo "2. 构建 Tauri App (多核加速)..."
npm run tauri build -- --bundles app

echo "3. 生成发布 DMG 安装包..."
bash scripts/package_dmg.sh

if [ -f "Antigravity_Tools_4.9.6_ManualFix.dmg" ]; then
    cp -f "Antigravity_Tools_4.9.6_ManualFix.dmg" "Antigravity_Tools_4.9.6_Custom_Pure.dmg"
fi

echo "=========================================="
echo "🎉 专属纯净增强版打包成功！"
echo "DMG 路径: $ROOT_DIR/Antigravity_Tools_4.9.6_Custom_Pure.dmg"
echo "=========================================="
