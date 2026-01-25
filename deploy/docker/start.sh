#!/bin/bash
set -e

cleanup() {
    echo "Shutting down..."
    pkill -f "Xtigervnc :1" 2>/dev/null || true
    pkill -f websockify 2>/dev/null || true
    exit 0
}

trap cleanup SIGTERM SIGINT SIGHUP

VNC_PASSWORD=${VNC_PASSWORD:-password}
RESOLUTION=${RESOLUTION:-1280x800}
NOVNC_PORT=${NOVNC_PORT:-6080}

# Clean up stale X lock files
rm -rf /tmp/.X1-lock /tmp/.X11-unix/X1 2>/dev/null || true
mkdir -p /tmp/.X11-unix
chmod 1777 /tmp/.X11-unix
touch "$HOME/.Xauthority"

mkdir -p "$HOME/.vnc"
echo "$VNC_PASSWORD" | vncpasswd -f > "$HOME/.vnc/passwd"
chmod 600 "$HOME/.vnc/passwd"

echo "Checking for Antigravity Tools..."
if dpkg -s antigravity-tools >/dev/null 2>&1; then
    CURRENT_VERSION=$(dpkg -s antigravity-tools | grep "Version:" | awk '{print "v"$2}')
else
    CURRENT_VERSION="none"
fi

ARCH=$(dpkg --print-architecture)
echo "Detected architecture: $ARCH"

RATE_LIMIT=$(wget -qO- --timeout=10 --header="Accept: application/vnd.github.v3+json" \
    "https://api.github.com/rate_limit" 2>/dev/null | grep -o '"remaining":[0-9]*' | head -1 | cut -d: -f2 || echo "0")

if [ "${RATE_LIMIT:-0}" -gt 5 ]; then
    LATEST_URL=$(wget -qO- --timeout=30 https://api.github.com/repos/lbjlaq/Antigravity-Manager/releases/latest \
        | grep "browser_download_url.*_${ARCH}.deb" \
        | cut -d '"' -f 4 || echo "")

    if [ -n "$LATEST_URL" ]; then
        # Try to extract version, handle both v1.2.3 and 1.2.3 formats
        LATEST_VERSION=$(echo "$LATEST_URL" | grep -oE 'v?[0-9]+\.[0-9]+\.[0-9]+' | head -1)
        # Ensure v prefix
        if [[ "$LATEST_VERSION" != v* ]] && [ -n "$LATEST_VERSION" ]; then
            LATEST_VERSION="v$LATEST_VERSION"
        fi

        # Force update if not installed or version mismatch
        if [ ! -f "/usr/bin/antigravity_tools" ] || [ "$CURRENT_VERSION" != "$LATEST_VERSION" ]; then
            echo "Updating $CURRENT_VERSION -> $LATEST_VERSION"
            wget -q --timeout=60 "$LATEST_URL" -O /tmp/ag.deb
            sudo apt-get update -qq && sudo apt-get install -y /tmp/ag.deb
            rm -f /tmp/ag.deb
            sudo rm -rf /var/lib/apt/lists/*
        else
            echo "Up to date: $CURRENT_VERSION"
        fi
    else
        echo "Could not find latest version, using cached: $CURRENT_VERSION"
    fi
else
    echo "GitHub API rate limit exceeded or network issue, using cached: $CURRENT_VERSION"
fi

echo "Starting VNC server..."
Xtigervnc :1 -auth "$HOME/.Xauthority" -geometry "${RESOLUTION}" -depth 24 \
    -rfbauth "$HOME/.vnc/passwd" -localhost no -SecurityTypes VncAuth \
    -AlwaysShared -AcceptKeyEvents -AcceptPointerEvents -AcceptSetDesktopSize &

# Wait for X server to be ready
timeout 10 bash -c 'until xset q &>/dev/null; do sleep 0.5; done' || echo "Xtigervnc startup timeout"

echo "Starting Openbox..."
openbox-session &

echo "Starting noVNC proxy..."
websockify --web /usr/share/novnc/ --wrap-mode=ignore 6080 localhost:5901 &

echo "Ready: http://localhost:${NOVNC_PORT}/vnc_lite.html"
echo "Starting Antigravity Tools..."

# Run app with exec (replaces shell process, keeps container alive)
exec /usr/bin/antigravity_tools
