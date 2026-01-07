# Antigravity Manager - 服务器部署教程

本指南介绍如何在服务器上部署 Antigravity Manager 的 Web 服务端版本。

## 🔧 系统要求

- **操作系统**: Linux (Ubuntu 20.04+, Debian 11+, CentOS 8+)
- **内存**: 512MB+
- **磁盘**: 100MB+
- **网络**: 服务器需要能访问 Google API

## 📦 安装步骤

### 1. 安装依赖

```bash
# Ubuntu/Debian
sudo apt update
sudo apt install -y curl build-essential pkg-config libssl-dev

# CentOS/RHEL
sudo yum groupinstall -y "Development Tools"
sudo yum install -y openssl-devel
```

### 2. 安装 Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
```

### 3. 安装 Node.js (用于构建前端)

```bash
# 使用 nvm 安装 (推荐)
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.39.0/install.sh | bash
source ~/.bashrc
nvm install 20
nvm use 20
```

### 4. 克隆代码库

```bash
git clone <your-repo-url> antigravity-manager
cd antigravity-manager
```

### 5. 构建前端

```bash
npm install
npm run build
```

### 6. 构建后端

```bash
cd src-tauri
cargo build --release --bin antigravity-server --no-default-features --features web-server
```

## 🚀 启动服务

### 基本启动

```bash
cd src-tauri
./target/release/antigravity-server \
    --port 8765 \
    --static-dir ../dist \
    --data-dir ~/.antigravity
```

### 命令行参数

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `-p, --port` | 8765 | API 服务端口 |
| `-h, --host` | 0.0.0.0 | 绑定地址 |
| `-s, --static-dir` | ./dist | 前端静态文件目录 |
| `-d, --data-dir` | ~/.antigravity | 数据存储目录 |

### 后台运行 (推荐)

使用 `nohup`:
```bash
cd /path/to/antigravity-manager/src-tauri
nohup ./target/release/antigravity-server \
    --port 8765 \
    --static-dir ../dist \
    --data-dir ~/.antigravity \
    > /var/log/antigravity.log 2>&1 &
```

使用 `systemd` (推荐生产环境):
```bash
# 创建 systemd 服务文件
sudo tee /etc/systemd/system/antigravity.service << 'EOF'
[Unit]
Description=Antigravity Manager Web Server
After=network.target

[Service]
Type=simple
User=your-username
WorkingDirectory=/path/to/antigravity-manager/src-tauri
ExecStart=/path/to/antigravity-manager/src-tauri/target/release/antigravity-server \
    --port 8765 \
    --static-dir /path/to/antigravity-manager/dist \
    --data-dir /home/your-username/.antigravity
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
EOF

# 启用并启动服务
sudo systemctl daemon-reload
sudo systemctl enable antigravity
sudo systemctl start antigravity

# 查看状态
sudo systemctl status antigravity
```

## 🌐 访问服务

浏览器访问: `http://<服务器IP>:8765`

## 🔑 添加账号 (OAuth 登录)

由于服务在远程，OAuth 回调无法自动处理，请使用以下方法：

### 方法一：手动粘贴回调 URL（推荐）

1. 打开 `http://<服务器IP>:8765`
2. 点击"添加账号" → OAuth 标签页
3. 点击"开始 OAuth" 并复制 OAuth 链接
4. 在本地浏览器打开该链接，完成 Google 认证
5. 认证后浏览器会跳转到 `http://localhost:9004/callback?code=xxx`
6. **页面会显示"无法访问"，这是正常的**
7. 复制地址栏中的完整 URL
8. 回到服务器页面，在 OAuth 界面底部的输入框粘贴该 URL
9. 点击"确认"完成登录

### 方法二：使用 Refresh Token

1. 通过其他方式获取 Google Refresh Token
2. 在"添加账号" → Token 标签页粘贴

## 🔒 安全建议

### 配置反向代理 (Nginx)

```nginx
server {
    listen 80;
    server_name your-domain.com;

    location / {
        proxy_pass http://127.0.0.1:8765;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        
        # SSE 支持
        proxy_buffering off;
        proxy_cache off;
        proxy_read_timeout 86400s;
    }
}
```

### 添加 HTTPS (Let's Encrypt)

```bash
sudo apt install certbot python3-certbot-nginx
sudo certbot --nginx -d your-domain.com
```

### 防火墙配置

```bash
# 仅开放必要端口
sudo ufw allow 22/tcp    # SSH
sudo ufw allow 80/tcp    # HTTP
sudo ufw allow 443/tcp   # HTTPS
sudo ufw enable
```

## 📋 常见问题

### Q: 构建时报错 "openssl not found"
```bash
sudo apt install libssl-dev  # Debian/Ubuntu
sudo yum install openssl-devel  # CentOS
```

### Q: 启动时报错 "Address already in use"
```bash
# 检查端口占用
lsof -i :8765
# 或更换端口
./target/release/antigravity-server --port 9000 ...
```

### Q: OAuth 登录失败
确保：
1. 服务器能访问 Google API (`curl https://oauth2.googleapis.com`)
2. 正确复制了完整的回调 URL（包含 `?code=...`）

## 🔄 更新部署

```bash
cd antigravity-manager
git pull
npm install && npm run build
cd src-tauri
cargo build --release --bin antigravity-server --no-default-features --features web-server
sudo systemctl restart antigravity  # 如果使用 systemd
```
