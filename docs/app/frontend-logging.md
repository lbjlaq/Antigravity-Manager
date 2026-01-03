# Frontend logging (UI troubleshooting)

The app UI is rendered by a frontend bundle, but most data (accounts, config, proxy controls) comes from the desktop backend.

If the UI shows a blank/white screen or buttons stop responding, collect these signals before reporting an issue:

## 1) Confirm you opened the desktop window (not the browser dev server)
- The browser URL `http://localhost:1420/` is the Vite dev server UI and does not load real backend data.
- Use the desktop window launched by `npm run tauri dev` (or the packaged app) to see real data.

## 2) Open the desktop developer tools
- In the desktop window, open Developer Tools and check:
  - **Console** for runtime errors
  - **Network** for failed `invoke` calls

If you see an error, copy:
- the full error message
- the call name (e.g. `save_config`, `start_proxy`, `get_proxy_runtime_status`)

## 3) Check backend logs
The backend logs are written by the Rust/Tauri process.

When running from source:
- Start the app in a terminal (`npm run tauri dev`) and capture the terminal output.

When using the packaged app:
- Check the app’s log output under your OS’s standard app log locations (varies by platform).

## 4) Privacy notes
- Do not share your `gui_config.json`, `accounts/*.json`, or any API keys.
- When sharing logs, redact tokens and any local file paths you consider sensitive.

