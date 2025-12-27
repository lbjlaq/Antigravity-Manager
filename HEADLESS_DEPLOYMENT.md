# Headless Proxy Server 部署指南

本项目现已支持在无GUI环境的服务器上部署反向代理服务。

## 功能特性

- ✅ **独立运行**: 无需Tauri GUI运行时,可在纯命令行环境部署
- ✅ **优雅关闭**: 支持 `Ctrl+C` 和 `SIGTERM` 信号优雅停止
- ✅ **配置复用**: 使用与桌面版相同的配置文件 (`~/.antigravity_tools/gui_config.json`)
- ✅ **账号管理**: 自动加载已配置的账号信息
- ✅ **完整功能**: 支持所有反向代理功能(OpenAI/Claude/Gemini协议转换)

## 构建说明

### 方式一: 使用自动化脚本 (推荐)

项目根目录提供了自动化编译脚本:

```bash
# 赋予执行权限
chmod +x build-headless.sh

# 运行编译脚本
./build-headless.sh
```

脚本会自动:
- 清理之前的构建
- 使用正确的feature flags编译headless-proxy
- 排除所有GUI依赖(GTK/webkit等)
- 显示编译结果和文件大小

### 方式二: 手动编译

#### 编译 headless-proxy (无GUI依赖)

**重要**: 必须使用 `--features headless --no-default-features` 来排除GUI依赖

```bash
cd src-tauri
cargo build --release --bin headless-proxy --features headless --no-default-features
```

编译后的二进制文件位于: `src-tauri/target/release/headless-proxy`

#### 架构说明

本项目通过Cargo feature flags实现GUI和headless的分离:

- **gui** feature (默认): 包含Tauri及所有GUI依赖
- **headless** feature: 仅包含核心proxy服务,无GUI依赖

**依赖对比**:

| 组件 | GUI模式 | Headless模式 |
|------|---------|-------------|
| Tauri框架 | ✅ | ❌ |
| GTK/webkit | ✅ | ❌ |
| 系统托盘 | ✅ | ❌ |
| **TLS实现** | OpenSSL (系统) | **rustls (静态链接)** |
| Axum服务器 | ✅ | ✅ |
| 配置管理 | ✅ | ✅ |
| 账号管理 | ✅ | ✅ |
| 日志系统 | ✅ | ✅ |

**TLS说明**:
- Headless模式使用 **rustls** 实现TLS，完全静态链接，无需系统OpenSSL库
- 这使得部署更简单，不需要担心不同系统的OpenSSL版本差异
- 性能与安全性与OpenSSL相当，且内存占用更小

## 配置说明

### 1. 准备配置文件

Headless服务器使用与桌面版相同的配置目录:

```
~/.antigravity_tools/
├── gui_config.json      # 主配置文件
├── accounts.db          # 账号数据库
└── logs/                # 日志目录
```

#### 配置文件示例 (`gui_config.json`)

```json
{
  "language": "zh-CN",
  "theme": "dark",
  "auto_refresh": false,
  "refresh_interval": 30,
  "auto_sync": false,
  "sync_interval": 60,
  "proxy": {
    "enabled": true,
    "allow_lan_access": true,
    "port": 8080,
    "api_key": "sk-your-api-key-here",
    "auto_start": true,
    "anthropic_mapping": {
      "claude-3-5-sonnet-20241022": "gemini-2.0-flash-exp"
    },
    "openai_mapping": {
      "gpt-4": "gemini-2.0-flash-exp"
    },
    "custom_mapping": {},
    "request_timeout": 300,
    "upstream_proxy": {
      "enabled": false,
      "url": ""
    }
  },
  "auto_launch": false
}
```

**重要配置项**:
- `proxy.enabled`: 必须设置为 `true`
- `proxy.allow_lan_access`:
  - `false` - 仅本机访问 (127.0.0.1)
  - `true` - 允许局域网访问 (0.0.0.0)
- `proxy.port`: 监听端口
- `proxy.api_key`: API密钥(客户端请求时需要提供)

### 2. 添加账号

使用桌面版应用添加账号,或手动导入。

## 运行说明

### 直接运行

```bash
./target/release/headless-proxy
```

### web-admin

```bash
http://127.0.0.1:8045/admin
```


### 停止服务

按 `Ctrl+C` 或发送 `SIGTERM` 信号:

```bash
kill -TERM <pid>
```

服务器将优雅关闭:

```
2025-12-27T10:05:00.000Z INFO  headless_proxy] Shutdown signal received, stopping server...
2025-12-27T10:05:00.010Z INFO  antigravity_tools_lib::proxy::server] 反代服务器停止监听
2025-12-27T10:05:00.020Z INFO  headless_proxy] Server stopped gracefully.
```

## 部署方式

### 方式1: Systemd 服务 (推荐)

创建 systemd 服务文件 `/etc/systemd/system/antigravity-proxy.service`:

```ini
[Unit]
Description=Antigravity Headless Proxy Server
After=network.target

[Service]
Type=simple
User=your-username
WorkingDirectory=/opt/antigravity
ExecStart=/opt/antigravity/headless-proxy
Restart=on-failure
RestartSec=5s

# 安全加固 (可选)
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=read-only
ReadWritePaths=/home/your-username/.antigravity_tools

[Install]
WantedBy=multi-user.target
```

启动服务:

```bash
# 复制二进制文件
sudo mkdir -p /opt/antigravity
sudo cp target/release/headless-proxy /opt/antigravity/
sudo chmod +x /opt/antigravity/headless-proxy

# 启用并启动服务
sudo systemctl daemon-reload
sudo systemctl enable antigravity-proxy
sudo systemctl start antigravity-proxy

# 查看状态
sudo systemctl status antigravity-proxy

# 查看日志
sudo journalctl -u antigravity-proxy -f
```

### 方式2: Screen/Tmux 会话

```bash
# 使用 screen
screen -S antigravity
./target/release/headless-proxy
# 按 Ctrl+A, D 分离会话

# 重新连接
screen -r antigravity

# 或使用 tmux
tmux new -s antigravity
./target/release/headless-proxy
# 按 Ctrl+B, D 分离会话

# 重新连接
tmux attach -t antigravity
```

### 健康检查

```bash
curl http://localhost:8080/healthz
# 输出: {"status":"ok"}
```

## 故障排查

### 1. 服务无法启动

**错误**: `Failed to load app configuration`
- **原因**: 配置文件不存在或格式错误
- **解决**: 检查 `~/.antigravity_tools/gui_config.json` 是否存在且格式正确

**错误**: `Proxy service is disabled in configuration`
- **原因**: `proxy.enabled` 设置为 `false`
- **解决**: 修改配置文件将 `proxy.enabled` 设置为 `true`

**错误**: `No active accounts found`
- **原因**: 没有配置任何账号
- **解决**: 使用桌面版添加账号,或手动导入账号数据库

**错误**: `地址 0.0.0.0:8045 绑定失败`
- **原因**: 端口已被占用
- **解决**: 修改配置文件中的 `proxy.port` 或停止占用端口的进程

### 2. 日志查看

日志文件位置: `~/.antigravity_tools/logs/`

```bash
# 查看最新日志
tail -f ~/.antigravity_tools/logs/antigravity.log
```

### 3. 权限问题

如果数据目录权限不正确:

```bash
# 修复权限
chmod 755 ~/.antigravity_tools
chmod 644 ~/.antigravity_tools/gui_config.json
chmod 644 ~/.antigravity_tools/accounts.db
```

## 安全建议

1. **API密钥**: 务必使用强随机API密钥,避免使用默认值
2. **局域网访问**: 仅在可信网络环境启用 `allow_lan_access`
3. **反向代理**: 建议在生产环境使用Nginx/Caddy等反向代理,并启用HTTPS
4. **防火墙**: 配置防火墙规则限制访问来源IP

## 性能优化

### 1. 资源限制

在 systemd 服务中添加资源限制:

```ini
[Service]
MemoryMax=512M
CPUQuota=50%
TasksMax=100
```

### 2. 日志轮转

创建 `/etc/logrotate.d/antigravity-proxy`:

```
/home/*/.antigravity_tools/logs/*.log {
    daily
    rotate 7
    compress
    delaycompress
    missingok
    notifempty
    create 0644 user group
}
```

## 监控建议

1. **健康检查端点**: 配置监控系统定期检查 `/healthz`
2. **进程监控**: 使用 systemd 或监控工具确保进程运行
3. **日志监控**: 监控错误日志,及时发现问题

## 相关文件

- 源代码: `src-tauri/src/bin/headless.rs`
- Cargo配置: `src-tauri/Cargo.toml`
- 主配置: `~/.antigravity_tools/gui_config.json`

## 技术支持

如有问题,请提交 GitHub Issue 或查看项目文档。
