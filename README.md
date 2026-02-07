<div align="center">
  <img src="public/icon.png" alt="Antigravity Manager" width="128" height="128" style="border-radius: 24px;">

  # Antigravity Manager

  **Your Personal High-Performance AI Gateway**

  *Seamlessly proxy Gemini & Claude • OpenAI-Compatible • Privacy First*

  [![Version](https://img.shields.io/badge/Version-5.0.2-blue?style=for-the-badge)](https://github.com/GofMan5/Antigravity-Manager/releases)
  [![Tauri](https://img.shields.io/badge/Tauri-v2-orange?style=for-the-badge)](https://tauri.app)
  [![Rust](https://img.shields.io/badge/Backend-Rust-red?style=for-the-badge)](https://www.rust-lang.org)
  [![React](https://img.shields.io/badge/Frontend-React-61DAFB?style=for-the-badge)](https://react.dev)

  [Features](#-features) •
  [Installation](#-installation) •
  [Quick Start](#-quick-start) •
  [Configuration](#-configuration)

</div>

---

## What is Antigravity Manager?

Antigravity Manager is a powerful desktop application that transforms your Google/Anthropic web sessions into standardized API endpoints. It provides:

- **Multi-Account Management** — Add unlimited accounts via OAuth or token import
- **Protocol Translation** — OpenAI, Anthropic, and Gemini API compatibility
- **Smart Load Balancing** — Automatic account rotation based on quotas and health
- **Real-time Monitoring** — Track usage, quotas, and request logs

---

## ✨ Features

### 🎛️ Smart Dashboard
- Real-time quota monitoring across all accounts
- One-click account switching with smart recommendations
- Visual health indicators and subscription tier badges

### 🔐 Account Management
- **OAuth 2.0** — Secure browser-based authorization
- **Token Import** — Batch import from JSON or manual entry
- **Auto-healing** — Automatic token refresh and error recovery

### 🔌 API Proxy
| Protocol | Endpoint | Compatibility |
|----------|----------|---------------|
| OpenAI | `/v1/chat/completions` | ChatGPT, Cursor, Continue |
| Anthropic | `/v1/messages` | Claude Code CLI, Claude Desktop |
| Gemini | `/v1beta/models` | Google AI SDK |

### 🛡️ Reliability Features
- **VALIDATION_REQUIRED Handling** — Temporary account blocking with auto-recovery
- **Circuit Breaker** — Configurable backoff steps for rate limits
- **Quota Protection** — Automatic model-level protection when quota is low
- **Health Scoring** — Prioritize stable accounts automatically

### 🔧 Developer Tools
- **Debug Console** — Real-time log viewer with filtering and export
- **Traffic Monitor** — Request/response inspection with timing
- **Model Mapping** — Custom routing rules and aliases

---

## 📥 Installation

### Windows
Download the latest `.msi` or portable `.zip` from [Releases](https://github.com/GofMan5/Antigravity-Manager/releases).

### macOS
```bash
# Via Homebrew
brew tap GofMan5/antigravity-manager https://github.com/GofMan5/Antigravity-Manager
brew install --cask --no-quarantine antigravity-tools

# Or download .dmg from Releases (Universal: Apple Silicon & Intel)
```

### Linux
```bash
# Arch Linux
curl -sSL https://raw.githubusercontent.com/GofMan5/Antigravity-Manager/main/deploy/arch/install.sh | bash

# Other distros: Download .deb or .AppImage from Releases
```

### Docker
```bash
docker run -d --name antigravity \
  -p 8045:8045 \
  -e API_KEY=sk-your-key \
  -v ~/.antigravity_tools:/root/.antigravity_tools \
  ghcr.io/gofman5/antigravity-manager:latest
```

---

## 🚀 Quick Start

### 1. Add an Account

1. Open **Accounts** → **Add Account**
2. Choose **OAuth** (recommended) or **Token**
3. Complete authorization in your browser
4. Account appears with quota information

### 2. Start the Proxy

1. Go to **API Proxy** tab
2. Click **Start Proxy**
3. Note the endpoint: `http://127.0.0.1:8045`

### 3. Connect Your App

#### Claude Code CLI
```bash
export ANTHROPIC_API_KEY="sk-antigravity"
export ANTHROPIC_BASE_URL="http://127.0.0.1:8045"
claude
```

#### Python (OpenAI SDK)
```python
import openai

client = openai.OpenAI(
    api_key="sk-antigravity",
    base_url="http://127.0.0.1:8045/v1"
)

response = client.chat.completions.create(
    model="gemini-2.5-pro",
    messages=[{"role": "user", "content": "Hello!"}]
)
print(response.choices[0].message.content)
```

#### Cursor / Continue / Other IDEs
- **API Base**: `http://127.0.0.1:8045/v1`
- **API Key**: `sk-antigravity` (or your configured key)
- **Model**: `gemini-2.5-pro`, `claude-sonnet-4`, etc.

---

## ⚙️ Configuration

### Settings Location
- **Windows**: `%APPDATA%\antigravity_tools\`
- **macOS**: `~/Library/Application Support/antigravity_tools/`
- **Linux**: `~/.antigravity_tools/`

### Key Settings

| Setting | Default | Description |
|---------|---------|-------------|
| `validation_block_minutes` | 10 | How long to block account after 403 VALIDATION_REQUIRED |
| `show_proxy_selected_badge` | true | Show "SELECTED" badge on accounts page |
| `debug_console_enabled` | false | Enable built-in debug console |

### Environment Variables (Docker)

| Variable | Description |
|----------|-------------|
| `API_KEY` | Required. Used for API authentication |
| `WEB_PASSWORD` | Optional. Separate password for web UI |
| `ABV_MAX_BODY_SIZE` | Max request body size (default: 100MB) |

---

## 🏗️ Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     External Applications                    │
│              (Claude Code, Cursor, ChatGPT, etc.)           │
└─────────────────────────┬───────────────────────────────────┘
                          │ OpenAI / Anthropic / Gemini API
                          ▼
┌─────────────────────────────────────────────────────────────┐
│                   Antigravity Proxy Server                   │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │   Auth &    │  │   Model     │  │   Account           │  │
│  │   Routing   │──│   Mapper    │──│   Dispatcher        │  │
│  └─────────────┘  └─────────────┘  └─────────────────────┘  │
│                                              │               │
│  ┌─────────────┐  ┌─────────────┐  ┌────────▼────────────┐  │
│  │   Rate      │  │   Health    │  │   Token Manager     │  │
│  │   Limiter   │──│   Scoring   │──│   (Multi-Account)   │  │
│  └─────────────┘  └─────────────┘  └─────────────────────┘  │
└─────────────────────────┬───────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────┐
│                    Upstream APIs                             │
│         Google AI (Gemini) / Anthropic (Claude)             │
└─────────────────────────────────────────────────────────────┘
```

---

## 🤝 Contributing

This is a fork of [lbjlaq/Antigravity-Manager](https://github.com/lbjlaq/Antigravity-Manager).

Contributions welcome! Please:
1. Fork the repository
2. Create a feature branch
3. Submit a Pull Request

---

## 📄 License

[CC-BY-NC-SA-4.0](LICENSE) — Non-commercial use with attribution.

---

<div align="center">
  <sub>Built with ❤️ using Tauri, Rust, and React</sub>
</div>
