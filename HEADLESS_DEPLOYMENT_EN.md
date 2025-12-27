# Headless Proxy Server Deployment Guide

This project now supports deploying the reverse proxy service on servers without a GUI environment.

## Features

- **Standalone Operation**: No Tauri GUI runtime required, deployable in pure command-line environments
- **Graceful Shutdown**: Supports `Ctrl+C` and `SIGTERM` signals for graceful stopping
- **Configuration Reuse**: Uses the same configuration file as the desktop version (`~/.antigravity_tools/gui_config.json`)
- **Account Management**: Automatically loads configured account information
- **Full Functionality**: Supports all reverse proxy features (OpenAI/Claude/Gemini protocol conversion)

## Build Instructions

### Method 1: Use Automated Script (Recommended)

An automated build script is provided in the project root directory:

```bash
# Grant execute permission
chmod +x build-headless.sh

# Run the build script
./build-headless.sh
```

The script will automatically:
- Clean previous builds
- Compile headless-proxy with correct feature flags
- Exclude all GUI dependencies (GTK/webkit)
- Display build results and file size

### Method 2: Manual Compilation

#### Compile headless-proxy (No GUI Dependencies)

**Important**: Must use `--features headless --no-default-features` to exclude GUI dependencies

```bash
cd src-tauri
cargo build --release --bin headless-proxy --features headless --no-default-features
```

The compiled binary is located at: `src-tauri/target/release/headless-proxy`

#### Architecture Overview

This project separates GUI and headless modes through Cargo feature flags:

- **gui** feature (default): Includes Tauri and all GUI dependencies
- **headless** feature: Only includes core proxy service, no GUI dependencies

**Dependency Comparison**:

| Component | GUI Mode | Headless Mode |
|-----------|----------|---------------|
| Tauri Framework | Yes | No |
| GTK/webkit | Yes | No |
| System Tray | Yes | No |
| **TLS Implementation** | OpenSSL (system) | **rustls (static linking)** |
| Axum Server | Yes | Yes |
| Configuration Management | Yes | Yes |
| Account Management | Yes | Yes |
| Logging System | Yes | Yes |

**TLS Notes**:
- Headless mode uses **rustls** for TLS, fully statically linked, no system OpenSSL library required
- This simplifies deployment without worrying about OpenSSL version differences across systems
- Performance and security comparable to OpenSSL, with smaller memory footprint

## Configuration

### 1. Prepare Configuration Files

The headless server uses the same configuration directory as the desktop version:

```
~/.antigravity_tools/
├── gui_config.json      # Main configuration file
├── accounts.db          # Account database
└── logs/                # Log directory
```

#### Configuration File Example (`gui_config.json`)

```json
{
  "language": "en",
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

**Important Configuration Options**:
- `proxy.enabled`: Must be set to `true`
- `proxy.allow_lan_access`:
  - `false` - Local access only (127.0.0.1)
  - `true` - Allow LAN access (0.0.0.0)
- `proxy.port`: Listening port
- `proxy.api_key`: API key (required in client requests)

### 2. Add Accounts

Use the desktop application to add accounts, or import manually.

## Running the Service

### Direct Execution

```bash
./target/release/headless-proxy
```

### web-admin

```bash
http://127.0.0.1:8045/admin
```

### Stopping the Service

Press `Ctrl+C` or send a `SIGTERM` signal:

```bash
kill -TERM <pid>
```

The server will shut down gracefully:

```
2025-12-27T10:05:00.000Z INFO  headless_proxy] Shutdown signal received, stopping server...
2025-12-27T10:05:00.010Z INFO  antigravity_tools_lib::proxy::server] Proxy server stopped listening
2025-12-27T10:05:00.020Z INFO  headless_proxy] Server stopped gracefully.
```

## Deployment Methods

### Method 1: Systemd Service (Recommended)

Create a systemd service file `/etc/systemd/system/antigravity-proxy.service`:

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

# Security Hardening (Optional)
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=read-only
ReadWritePaths=/home/your-username/.antigravity_tools

[Install]
WantedBy=multi-user.target
```

Start the service:

```bash
# Copy the binary
sudo mkdir -p /opt/antigravity
sudo cp target/release/headless-proxy /opt/antigravity/
sudo chmod +x /opt/antigravity/headless-proxy

# Enable and start the service
sudo systemctl daemon-reload
sudo systemctl enable antigravity-proxy
sudo systemctl start antigravity-proxy

# Check status
sudo systemctl status antigravity-proxy

# View logs
sudo journalctl -u antigravity-proxy -f
```

### Method 2: Screen/Tmux Session

```bash
# Using screen
screen -S antigravity
./target/release/headless-proxy
# Press Ctrl+A, D to detach

# Reattach
screen -r antigravity

# Or using tmux
tmux new -s antigravity
./target/release/headless-proxy
# Press Ctrl+B, D to detach

# Reattach
tmux attach -t antigravity
```

### Health Check

```bash
curl http://localhost:8080/healthz
# Output: {"status":"ok"}
```

## Troubleshooting

### 1. Service Won't Start

**Error**: `Failed to load app configuration`
- **Cause**: Configuration file doesn't exist or has invalid format
- **Solution**: Check if `~/.antigravity_tools/gui_config.json` exists and is properly formatted

**Error**: `Proxy service is disabled in configuration`
- **Cause**: `proxy.enabled` is set to `false`
- **Solution**: Modify the configuration file to set `proxy.enabled` to `true`

**Error**: `No active accounts found`
- **Cause**: No accounts configured
- **Solution**: Use the desktop version to add accounts, or manually import the account database

**Error**: `Failed to bind to address 0.0.0.0:8045`
- **Cause**: Port is already in use
- **Solution**: Change `proxy.port` in the configuration file or stop the process using that port

### 2. Viewing Logs

Log file location: `~/.antigravity_tools/logs/`

```bash
# View latest logs
tail -f ~/.antigravity_tools/logs/antigravity.log
```

### 3. Permission Issues

If the data directory has incorrect permissions:

```bash
# Fix permissions
chmod 755 ~/.antigravity_tools
chmod 644 ~/.antigravity_tools/gui_config.json
chmod 644 ~/.antigravity_tools/accounts.db
```

## Security Recommendations

1. **API Key**: Always use a strong random API key, avoid default values
2. **LAN Access**: Only enable `allow_lan_access` in trusted network environments
3. **Reverse Proxy**: Recommended to use Nginx/Caddy in production with HTTPS enabled
4. **Firewall**: Configure firewall rules to restrict source IP access


## Performance Optimization

### 1. Resource Limits

Add resource limits to the systemd service:

```ini
[Service]
MemoryMax=512M
CPUQuota=50%
TasksMax=100
```

### 2. Log Rotation

Create `/etc/logrotate.d/antigravity-proxy`:

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

## Monitoring Recommendations

1. **Health Check Endpoint**: Configure monitoring systems to periodically check `/healthz`
2. **Process Monitoring**: Use systemd or monitoring tools to ensure the process is running
3. **Log Monitoring**: Monitor error logs to detect issues early

## Related Files

- Source Code: `src-tauri/src/bin/headless.rs`
- Cargo Config: `src-tauri/Cargo.toml`
- Main Config: `~/.antigravity_tools/gui_config.json`

## Technical Support

For issues, please submit a GitHub Issue or refer to the project documentation.
