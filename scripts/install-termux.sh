#!/bin/sh
# Antigravity Headless Server Installer for Termux (Android ARM64)
set -e

REPO="lbjlaq/Antigravity-Manager"
TARGET_DIR="${PREFIX:-/data/data/com.termux/files/usr}/bin"
CONFIG_DIR="${HOME}/.config/antigravity"

echo "=========================================="
echo " Antigravity Server Termux Installer"
echo "=========================================="

# 1. Architecture check
ARCH=$(uname -m)
if [ "$ARCH" != "aarch64" ]; then
    echo "❌ Error: Only aarch64 (ARM64) architecture is currently supported on Termux."
    echo "Current architecture: $ARCH"
    exit 1
fi

# 2. Dependency check
for cmd in curl tar; do
    if ! command -v $cmd >/dev/null 2>&1; then
        echo "Installing missing dependency: $cmd..."
        pkg install -y $cmd
    fi
done

# 3. Retrieve release asset
echo "🔍 Fetching latest release info..."
LATEST_TAG=$(curl -sSL "https://api.github.com/repos/${REPO}/releases" | grep '"tag_name":' | head -n 1 | cut -d '"' -f 4)

if [ -z "$LATEST_TAG" ]; then
    echo "❌ Failed to detect release tag. Please check your network connection."
    exit 1
fi

echo "📦 Selected version: ${LATEST_TAG}"
ASSET_NAME="antigravity-server-android-arm64.tar.gz"
DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${LATEST_TAG}/${ASSET_NAME}"

TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

echo "⬇️ Downloading ${ASSET_NAME}..."
if ! curl -sSL -f "$DOWNLOAD_URL" -o "${TMP_DIR}/${ASSET_NAME}"; then
    echo "⚠️ Direct download failed from tag ${LATEST_TAG}, checking preview release..."
    PREVIEW_URL="https://github.com/${REPO}/releases/download/preview/${ASSET_NAME}"
    curl -sSL -f "$PREVIEW_URL" -o "${TMP_DIR}/${ASSET_NAME}" || {
        echo "❌ Download failed. Binary might not be published yet for ${LATEST_TAG}."
        exit 1
    }
fi

echo "📂 Extracting to ${TARGET_DIR}..."
mkdir -p "$TARGET_DIR"
mkdir -p "$CONFIG_DIR"
tar -xzf "${TMP_DIR}/${ASSET_NAME}" -C "$TMP_DIR"
install -m 755 "${TMP_DIR}/antigravity-server" "${TARGET_DIR}/antigravity-server"
if [ -d "${TMP_DIR}/dist" ]; then
    # Resolved by the server via <exe dir>/../share/antigravity/dist
    SHARE_DIR="$(dirname "$TARGET_DIR")/share/antigravity"
    mkdir -p "$SHARE_DIR"
    rm -rf "${SHARE_DIR}/dist"
    cp -r "${TMP_DIR}/dist" "${SHARE_DIR}/dist"
fi

echo ""
echo "🎉 Successfully installed antigravity-server!"
echo "------------------------------------------"
echo "To start the server (with Web UI):"
echo "  antigravity-server"
echo ""
echo "To run in pure proxy mode (disable Web UI to save resources):"
echo "  antigravity-server --no-web"
echo ""
echo "Custom port example:"
echo "  antigravity-server -p 8045"
echo ""
echo "Data directory: ~/.antigravity_tools"
echo "=========================================="

