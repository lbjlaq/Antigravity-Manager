# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Antigravity Tools is a professional AI account management and protocol proxy system built with Tauri v2. It converts web-based AI sessions (Google/Anthropic) into standardized API endpoints, bridging protocol differences between vendors. The application provides multi-account management, intelligent request routing, and protocol conversion for OpenAI, Anthropic, and Gemini formats.

**Tech Stack**: Tauri v2 (Rust backend) + React 19 + TypeScript + Vite

## Development Commands

### Frontend Development
```bash
# Install dependencies
npm install

# Start development server (frontend only)
npm run dev

# Type check and build frontend
npm run build

# Preview production build
npm run preview
```

### Full Application Development
```bash
# Start Tauri dev mode (backend + frontend with hot reload)
npm run tauri dev

# Build production application
npm run tauri build

# Run Tauri CLI commands directly
npm run tauri -- [command]
```

### Rust Backend Development
```bash
# From project root or src-tauri directory
cargo build
cargo test
cargo clippy
cargo fmt

# Run specific tests
cargo test --lib test_name
cargo test --package antigravity_tools_lib
```

### Testing & Quality
```bash
# TypeScript type checking
npx tsc --noEmit

# Rust formatting check
cargo fmt -- --check

# Rust linting
cargo clippy -- -D warnings
```

## Architecture

### Hybrid Desktop Application Structure

**Tauri Process Model**: This is a dual-process desktop application:
- **Backend Process**: Rust application running Axum HTTP server on localhost (default: `127.0.0.1:8045`)
- **Frontend Process**: React application in webview communicating via Tauri IPC

**Critical**: Frontend → Backend communication uses Tauri's `invoke()` API, NOT direct HTTP calls. The Axum server handles external API proxy requests from third-party applications.

### Backend Architecture (Rust)

#### Core Modules (`src-tauri/src/`)

**`lib.rs`**: Application entry point
- Registers all Tauri commands via `invoke_handler`
- Initializes plugins: dialog, fs, opener, autostart, single-instance
- Sets up system tray and window event handlers
- Auto-starts proxy server based on configuration

**`modules/`**: Core business logic
- `account.rs`: Account CRUD operations, Google OAuth token management
- `quota.rs`: Quota fetching and synchronization with Google APIs
- `config.rs`: Application configuration (JSON-based, stored in user data directory)
- `oauth.rs` + `oauth_server.rs`: OAuth 2.0 authorization flow with temporary callback server
- `process.rs`: Background tasks and session management
- `db.rs`: SQLite database operations (rusqlite)
- `proxy_db.rs`: Proxy service specific database operations
- `migration.rs`: V1 to V2 data migration utilities
- `logger.rs`: Tracing-based logging setup
- `tray.rs`: System tray menu and interactions

**`proxy/`**: API proxy server (Axum-based)
- `server.rs`: Axum HTTP server setup and routing
- `token_manager.rs`: Multi-account token rotation and lifecycle management
- `rate_limit.rs`: Rate limiting and intelligent retry logic
- `handlers/`: Request handlers for each protocol
  - `openai.rs`: OpenAI `/v1/chat/completions` endpoint
  - `claude.rs`: Anthropic `/v1/messages` endpoint
  - `gemini.rs`: Google Gemini native endpoint
  - `mcp.rs`: Model Context Protocol support
  - `audio.rs`: Audio/speech processing
- `mappers/`: Protocol conversion modules
  - `claude/`: Claude request/response mapping
  - `openai/`: OpenAI format conversion
  - `gemini/`: Gemini format conversion
  - `common_utils.rs`: Shared mapping utilities
- `middleware/`: Request/response processing
- `upstream/`: Upstream API client management
- `providers/`: Provider-specific logic
- `config.rs`: Proxy service configuration
- `monitor.rs`: Request monitoring and statistics

**`commands/`**: Tauri command handlers
- `mod.rs`: Account, quota, config, OAuth, migration commands
- `proxy.rs`: Proxy service lifecycle commands (start/stop/status/stats)
- `autostart.rs`: System autostart configuration

**`models/`**: Data structures and types

**`utils/`**: Helper utilities

#### Key Architectural Patterns

**Account Management**: Accounts stored as individual JSON files in `{data_dir}/accounts/{account_id}.json`. Each account contains OAuth tokens, quotas, and metadata.

**Token Rotation**: `TokenManager` implements intelligent account selection based on:
- Rate limit tracking (429 errors trigger automatic retry with different account)
- Subscription tier prioritization (Ultra > Pro > Free)
- Quota-based weighting (prefers accounts with higher remaining quota)
- Sticky session support (optional session-to-account binding)

**Protocol Conversion**: Three-layer transformation:
1. **Handler Layer**: Route incoming requests to protocol-specific handler
2. **Mapper Layer**: Transform request format to upstream API format
3. **Upstream Layer**: Execute request with retry and error handling

**Auto-Stream Conversion**: Non-streaming requests automatically converted to streaming at upstream to avoid rate limits, then collected and returned as JSON to client.

**Model Routing**: Custom model mapping with regex support. Allows requests for arbitrary model IDs to be transparently routed to actual provider models.

### Frontend Architecture (React)

#### Structure (`src/`)

**`App.tsx`**: React Router setup with 5 main routes:
- `/` - Dashboard (quota monitoring, account recommendations)
- `/accounts` - Account management (list/grid view, OAuth, import)
- `/api-proxy` - Proxy server controls and configuration
- `/monitor` - Request monitoring and statistics
- `/settings` - Application settings and preferences

**`pages/`**: Route components (Dashboard, Accounts, ApiProxy, Monitor, Settings)

**`components/`**: Reusable UI components
- `layout/`: Layout components (sidebar, navigation)
- `common/`: Shared components (ThemeManager, modals, forms)
- `accounts/`: Account-specific components (account cards, import dialogs)
- `dashboard/`: Dashboard widgets (quota cards, charts)
- `proxy/`: Proxy configuration components

**`stores/`**: Zustand state management
- `useAccountStore.ts`: Account list, current account, quota state
- `useConfigStore.ts`: Application configuration
- `networkMonitorStore.ts`: Network request monitoring

**`services/`**: Tauri command wrappers
- `accountService.ts`: Account operations via Tauri IPC
- `configService.ts`: Configuration loading/saving

**`hooks/`**: Custom React hooks

**`utils/`**: Frontend utilities

**`locales/`**: i18next translations (zh, en)

#### State Management Pattern

**Zustand stores** handle client state. Stores call **service functions** which invoke Tauri commands:
```typescript
// Frontend: useAccountStore.ts calls accountService.ts
const accounts = await listAccounts();

// Service: accountService.ts invokes Tauri command
import { invoke } from '@tauri-apps/api/core';
return invoke('list_accounts');

// Backend: commands/mod.rs handles command
#[tauri::command]
fn list_accounts() -> Result<Vec<Account>, String>
```

**Event-based updates**: Backend emits Tauri events (e.g., `tray://account-switched`) which frontend listens to via `@tauri-apps/api/event` for real-time UI updates.

### Data Flow

**External API Request Flow**:
```
Third-party App (Claude Code, Cursor, etc.)
  → HTTP Request to localhost:8045
  → Axum Router (server.rs)
  → Protocol Handler (openai/claude/gemini.rs)
  → Token Manager (account selection)
  → Request Mapper (protocol conversion)
  → Upstream Client (Google/Anthropic API)
  → Response Mapper (format conversion)
  → Client receives response
```

**UI Interaction Flow**:
```
User Action (React Component)
  → Store Action (Zustand)
  → Service Function
  → Tauri invoke()
  → Rust Command Handler
  → Module Logic
  → Database/File System
  → Response → Store Update → UI Re-render
```

## Important Implementation Details

### OAuth Flow
OAuth authorization uses a temporary HTTP server on a random port. The authorization URL is pre-generated and copyable before clicking. After browser authorization, the callback is handled automatically. If the app isn't running during callback, a manual completion option is available.

### Configuration Storage
- **Application config**: `{data_dir}/config.json`
- **Accounts**: `{data_dir}/accounts/*.json`
- **Database**: `{data_dir}/antigravity.db` (SQLite)
- **Logs**: `{data_dir}/logs/` (rotating file appender)

**Data directory paths**:
- macOS: `~/Library/Application Support/com.lbjlaq.antigravity-tools/`
- Windows: `%APPDATA%\com.lbjlaq.antigravity-tools\`
- Linux: `~/.config/com.lbjlaq.antigravity-tools/`

### Proxy Service Lifecycle
The proxy server is a long-running Axum HTTP server managed by Tauri command handlers. It can be started/stopped independently of the main application window. Configuration supports:
- Custom host/port binding
- API key authentication
- Rate limiting and retry strategies
- Model mapping (including regex patterns)
- Upstream proxy configuration
- Request timeout (30-3600 seconds)

### Model Routing Intelligence
The system supports three routing strategies:
1. **Direct mapping**: Simple string-to-string model ID translation
2. **Regex mapping**: Pattern-based routing for flexible model selection
3. **Tiered routing**: Automatic account selection based on subscription tier and quota reset frequency

### Error Recovery
Rate limiting (429) and authentication errors (401) trigger automatic retry with account rotation. The frontend displays 403 errors prominently to indicate blocked accounts.

## Code Style and Conventions

### Rust
- Use `tracing` macros (`info!`, `debug!`, `error!`) for logging, not `println!`
- Error handling: Return `Result<T, String>` from Tauri commands with descriptive error messages
- Async operations use Tokio runtime (already configured by Tauri)
- Module organization: public interfaces in `mod.rs`, implementation in separate files

### TypeScript/React
- React 19 patterns (hooks, functional components)
- Zustand for state management (no Redux/Context API)
- Tauri IPC via `invoke()` from `@tauri-apps/api/core`
- i18next for internationalization (both zh and en translations required)
- DaisyUI + Tailwind CSS for styling
- Lucide React for icons

### File Naming
- Rust: snake_case (e.g., `account_service.rs`)
- TypeScript: PascalCase for components (e.g., `Dashboard.tsx`), camelCase for utilities
- Configuration files: lowercase with hyphens

## Common Development Patterns

### Adding a New Tauri Command
1. Define command in `src-tauri/src/commands/mod.rs` or relevant command file
2. Add to `invoke_handler!` macro in `src-tauri/src/lib.rs`
3. Create service wrapper in `src/services/` if needed
4. Call from store or component using `invoke()`

### Adding a New API Endpoint to Proxy
1. Add route in `src-tauri/src/proxy/server.rs`
2. Create handler in `src-tauri/src/proxy/handlers/`
3. Implement protocol mapper in `src-tauri/src/proxy/mappers/`
4. Update `TokenManager` if account selection logic changes

### Adding UI Configuration
1. Update config structure in `src-tauri/src/models/` and `src-tauri/src/modules/config.rs`
2. Add UI controls in `src/pages/Settings.tsx` or `src/pages/ApiProxy.tsx`
3. Update store in `src/stores/useConfigStore.ts`
4. Handle save via `save_config` command

## Building and Distribution

### macOS
- **Intel/Apple Silicon**: Universal binary built with `tauri build --target universal-apple-darwin`
- **Distribution**: `.dmg` installer (requires code signing for distribution)
- **Homebrew**: Cask formula in `Casks/antigravity-tools.rb`

### Windows
- **Build**: `.msi` installer and portable `.zip`
- **Requirements**: Windows 10+ with WebView2

### Linux
- **Build**: `.deb` package and `AppImage`
- **Distribution**: AppImage is portable, .deb for Debian-based distros

### Build Command
```bash
npm run tauri build
# Output in src-tauri/target/release/bundle/
```

## Security Considerations

- **Token storage**: Account files contain sensitive OAuth tokens, stored in user-only accessible directories
- **API authentication**: Proxy server supports API key authentication (configurable)
- **CORS**: Configured in proxy server for local development tools
- **Rate limiting**: Prevents abuse and manages upstream API quotas
- **Input validation**: All Tauri commands validate input before file system operations

## Troubleshooting

### Proxy Server Won't Start
- Check port availability (default 8045)
- Verify accounts are loaded and valid
- Check logs in `{data_dir}/logs/`
- Ensure no other Antigravity instance is running (single-instance plugin active)

### Account Authorization Fails
- Verify OAuth callback server started successfully
- Check browser console for callback errors
- Ensure authorization link is from current session (links expire)
- Try manual completion if auto-completion fails

### Build Failures
- **Rust**: Ensure Rust toolchain is up to date (`rustup update`)
- **Node**: Verify Node.js version compatibility (v18+)
- **Tauri**: Check Tauri CLI version matches dependencies (`npm run tauri --version`)

## Version and Release Notes

Current version: **v3.3.20** (2026-01-09)

Major features:
- Request timeout configuration up to 3600 seconds
- Auto-stream conversion to eliminate 429 errors
- Tiered routing with subscription-aware account prioritization
- Multi-protocol support (OpenAI, Anthropic, Gemini)
- Imagen 3 support with advanced quality controls
