#!/bin/bash

# Headless Proxy 编译脚本
# 此脚本用于编译无GUI依赖的headless-proxy二进制文件

set -e  # 遇到错误时退出

echo "======================================"
echo "编译 Antigravity Headless Proxy"
echo "======================================"
echo ""

# 检查当前目录
if [ ! -f "src-tauri/Cargo.toml" ]; then
    echo "错误: 请在项目根目录运行此脚本"
    exit 1
fi

# 进入 src-tauri 目录
cd src-tauri

echo "清理之前的构建..."
cargo clean

echo ""
echo "编译 headless-proxy (无GUI依赖, 使用rustls-tls)..."
echo "命令: cargo build --release --bin headless-proxy --features headless --no-default-features"
echo ""
echo "说明:"
echo "  - 使用 rustls 替代 OpenSSL (静态链接, 无需系统依赖)"
echo "  - 排除所有 GUI 依赖 (GTK/webkit)"
echo "  - 仅包含核心 proxy 功能"
echo ""

cargo build --release --bin headless-proxy --features headless --no-default-features

if [ $? -eq 0 ]; then
    echo ""
    echo "======================================"
    echo "✅ 编译成功！"
    echo "======================================"
    echo ""
    echo "二进制文件位置: target/release/headless-proxy"
    echo ""
    echo "文件大小:"
    ls -lh target/release/headless-proxy
    echo ""
    echo "验证 TLS 实现:"
    ldd target/release/headless-proxy 2>/dev/null | grep -i ssl && echo "⚠️  仍然链接 OpenSSL" || echo "✅ 使用 rustls (静态链接)"
    echo ""
    echo "部署说明请查看: ../HEADLESS_DEPLOYMENT.md"
else
    echo ""
    echo "======================================"
    echo "❌ 编译失败"
    echo "======================================"
    exit 1
fi
