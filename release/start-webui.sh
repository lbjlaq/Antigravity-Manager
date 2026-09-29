#!/usr/bin/env bash
# Antigravity Tools - one-click Web UI launcher (headless service, no WebView)
# Optional env overrides: WEB_PASSWORD / API_KEY / PORT / ABV_BIND_LOCAL_ONLY
cd "$(dirname "$0")" || exit 1

if [ ! -x ./antigravity-tools ]; then
    echo "[ERROR] antigravity-tools not found next to this script."
    exit 1
fi

export PORT="${PORT:-8045}"
# Bind to 127.0.0.1 only by default (set ABV_BIND_LOCAL_ONLY=0 to allow LAN access)
export ABV_BIND_LOCAL_ONLY="${ABV_BIND_LOCAL_ONLY:-1}"

if [ -f ./dist/index.html ]; then
    export ABV_DIST_PATH="$(pwd)/dist"
else
    echo "[WARN] dist/ not found - Web UI assets unavailable, API proxy still works."
    unset ABV_DIST_PATH
fi

echo "=============================================="
echo " Antigravity Tools - Web UI service"
echo " URL : http://localhost:${PORT}  (browser opens automatically)"
echo " Data: ~/.antigravity_tools"
echo " Stop: press Ctrl+C"
echo "=============================================="

./antigravity-tools --headless --open
