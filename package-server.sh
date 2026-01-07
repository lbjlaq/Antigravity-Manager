#!/bin/bash
# Antigravity Manager 打包脚本
# 用于创建可直接部署到服务器的压缩包

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
VERSION=$(grep '"version"' "$SCRIPT_DIR/package.json" | head -1 | sed 's/.*: "\(.*\)".*/\1/')
PACKAGE_NAME="antigravity-server-${VERSION}-linux-amd64"
BUILD_DIR="$SCRIPT_DIR/build/$PACKAGE_NAME"

echo "📦 打包 Antigravity Manager v${VERSION}..."

# 1. 构建前端
echo "🔨 构建前端..."
cd "$SCRIPT_DIR"
npm run build

# 2. 构建后端
echo "🔨 构建后端..."
cd "$SCRIPT_DIR/src-tauri"
cargo build --release --bin antigravity-server --no-default-features --features web-server

# 3. 创建打包目录
echo "📁 创建部署包..."
rm -rf "$BUILD_DIR"
mkdir -p "$BUILD_DIR"

# 复制文件
cp target/release/antigravity-server "$BUILD_DIR/"
cp -r ../dist "$BUILD_DIR/"
cp ../DEPLOYMENT.md "$BUILD_DIR/README.md"

# 创建启动脚本
cat > "$BUILD_DIR/start.sh" << 'EOF'
#!/bin/bash
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"
./antigravity-server --port 8765 --static-dir ./dist --data-dir ~/.antigravity "$@"
EOF
chmod +x "$BUILD_DIR/start.sh"

# 创建 systemd 服务模板
cat > "$BUILD_DIR/antigravity.service" << 'EOF'
[Unit]
Description=Antigravity Manager Web Server
After=network.target

[Service]
Type=simple
User=REPLACE_WITH_YOUR_USERNAME
WorkingDirectory=/opt/antigravity
ExecStart=/opt/antigravity/antigravity-server --port 8765 --static-dir /opt/antigravity/dist --data-dir /home/REPLACE_WITH_YOUR_USERNAME/.antigravity
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
EOF

# 4. 创建压缩包
echo "📦 压缩..."
cd "$SCRIPT_DIR/build"
tar -czvf "${PACKAGE_NAME}.tar.gz" "$PACKAGE_NAME"

echo ""
echo "✅ 打包完成！"
echo "📦 文件位置: $SCRIPT_DIR/build/${PACKAGE_NAME}.tar.gz"
echo ""
echo "📋 部署步骤:"
echo "  1. scp build/${PACKAGE_NAME}.tar.gz user@server:/tmp/"
echo "  2. ssh user@server"
echo "  3. cd /tmp && tar -xzf ${PACKAGE_NAME}.tar.gz"
echo "  4. sudo mv ${PACKAGE_NAME} /opt/antigravity"
echo "  5. /opt/antigravity/start.sh"
echo ""
echo "或使用 systemd:"
echo "  sudo cp /opt/antigravity/antigravity.service /etc/systemd/system/"
echo "  sudo sed -i 's/REPLACE_WITH_YOUR_USERNAME/your-user/g' /etc/systemd/system/antigravity.service"
echo "  sudo systemctl daemon-reload"
echo "  sudo systemctl enable --now antigravity"
