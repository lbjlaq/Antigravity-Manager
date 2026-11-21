# Antigravity Manager

> 🚀 Lightweight, cross-platform Google AI account manager for Antigravity with full GUI, Mini Window, and CLI (`am`) support.

[English](README.md) | [简体中文](README_ZH.md)

---

## ✨ Features

- **OAuth 2.0 Direct Auth**: Fast, reliable account authorization with local callback listener or manual code input.
- **Account Hot-Switching**: Seamlessly switch active accounts without losing context. Compatible with Antigravity IDE and Classic clients.
- **Quota & Balance Monitoring**: Real-time quota polling, percentage indicators, model-level allowance inspection, and auto-refresh intervals.
- **Account Management & Multi-Account Pools**: Batch import/export, tagging, label management, priority ordering, and account health validation.
- **Device Fingerprint Isolation**: Independent device fingerprint profile generation and binding per account to prevent correlation.
- **Host Integration (Shared Database)**: Shares `accounts.json` and client state with Antigravity Tools / IDE hosts seamlessly without conflict.
- **Mini Float Window**: Pinned floating view for instant quota checks and quick switching right from your desktop.
- **CLI Support (`am`)**: Complete headless CLI parity (`am list`, `am current`, `am switch`, `am export`, `am import`).

---

## 🛠 CLI Usage (`am`)

`antigravity-manager` comes with a built-in CLI executable named `am`.

```bash
# List all accounts and their quota status
am list

# View the currently active account
am current

# Switch to a target account
am switch <account_id>

# Export accounts
am export -o accounts_backup.json

# View CLI help
am --help
```

---

## 🏗 Architecture & Modules

```
src/
├── components/
│   ├── accounts/          # Account management & switching UI
│   ├── dashboard/         # Quota overview & statistics
│   ├── layout/            # Layout & Mini window
│   ├── navbar/            # Navigation bar
│   └── settings/          # Configuration & auto-switch settings
├── services/              # Account and configuration services
├── stores/                # Zustand state stores
└── utils/                 # Utilities and IPC request wrapper

src-tauri/
├── src/
│   ├── bin/am.rs          # CLI binary (`am`)
│   ├── commands/          # Tauri IPC command handlers
│   ├── models/            # Core account, quota, and config models
│   └── modules/           # Account management, OAuth, device fingerprint, DB sync
```

---

## 📄 License

MIT License.
