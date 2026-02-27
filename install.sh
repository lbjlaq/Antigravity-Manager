#!/usr/bin/env bash
# Antigravity Tools Install Script (Linux + macOS)
# Usage: curl -fsSL https://raw.githubusercontent.com/lbjlaq/Antigravity-Manager/main/install.sh | bash
#
# Environment variables:
#   VERSION     - Install specific version (e.g., "4.1.20"), default: latest
#   DRY_RUN     - Set to "1" to print commands without executing

set -euo pipefail

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

REPO="lbjlaq/Antigravity-Manager"
APP_NAME="Antigravity Tools"
APP_ID="com.lbjlaq.antigravity-tools"
GITHUB_API="https://api.github.com/repos/${REPO}/releases"

# Helper functions
info() { echo -e "${BLUE}[INFO]${NC} $1"; }
success() { echo -e "${GREEN}[OK]${NC} $1"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
error() { echo -e "${RED}[ERROR]${NC} $1" >&2; exit 1; }

run() {
    if [[ "${DRY_RUN:-0}" == "1" ]]; then
        echo -e "${YELLOW}[DRY-RUN]${NC} $*"
    else
        "$@"
    fi
}

# Show help
show_help() {
    cat << EOF
${APP_NAME} Install Script

Usage:
    curl -fsSL https://raw.githubusercontent.com/${REPO}/main/install.sh | bash

    # Install specific version
    curl -fsSL https://raw.githubusercontent.com/${REPO}/main/install.sh | VERSION=4.1.25 bash

Options:
    --help      Show this help message
    --version   Show script version

Environment Variables:
    VERSION     Install specific version (default: latest)
    DRY_RUN     Set to "1" to preview commands without executing

Supported Platforms:
    - Linux x86_64:  .deb (Debian/Ubuntu), .rpm (Fedora/RHEL), .AppImage (Universal)
    - Linux aarch64: .deb (Debian/Ubuntu), .rpm (Fedora/RHEL), .AppImage (Universal)
    - macOS x86_64:  .dmg
    - macOS arm64:   .dmg

EOF
    exit 0
}

# Detect OS and architecture
detect_platform() {
    OS="$(uname -s)"
    ARCH="$(uname -m)"

    case "$OS" in
        Linux)  PLATFORM="linux" ;;
        Darwin) PLATFORM="macos" ;;
        *)      error "Unsupported OS: $OS. Use install.ps1 for Windows." ;;
    esac

    case "$ARCH" in
        x86_64|amd64)   ARCH_LABEL="x86_64"; DEB_ARCH="amd64"; RPM_ARCH="x86_64" ;;
        aarch64|arm64)  ARCH_LABEL="aarch64"; DEB_ARCH="arm64"; RPM_ARCH="aarch64" ;;
        *)              error "Unsupported architecture: $ARCH" ;;
    esac

    info "Detected: $PLATFORM ($ARCH_LABEL)"
}

# Detect Linux package manager
detect_linux_distro() {
    if [[ "$PLATFORM" != "linux" ]]; then
        return
    fi

    if command -v apt-get &>/dev/null; then
        PKG_MANAGER="apt"
        PKG_EXT="deb"
    elif command -v dnf &>/dev/null; then
        PKG_MANAGER="dnf"
        PKG_EXT="rpm"
    elif command -v yum &>/dev/null; then
        PKG_MANAGER="yum"
        PKG_EXT="rpm"
    else
        PKG_MANAGER="appimage"
        PKG_EXT="AppImage"
        warn "No supported package manager found, using AppImage"
    fi

    info "Package manager: $PKG_MANAGER ($PKG_EXT)"
}

# Get latest or specific version
get_version() {
    if [[ -n "${VERSION:-}" ]]; then
        RELEASE_VERSION="$VERSION"
        info "Using specified version: v$RELEASE_VERSION"
        return
    fi

    info "Fetching latest version..."

    # Method 1: Try GitHub API
    local response
    if response=$(curl -fsSL -H "User-Agent: Antigravity-Installer" "${GITHUB_API}/latest" 2>/dev/null); then
        RELEASE_VERSION=$(echo "$response" | grep '"tag_name"' | sed -E 's/.*"v([^"]+)".*/\1/')
        if [[ -n "$RELEASE_VERSION" ]]; then
            info "Latest version: v$RELEASE_VERSION"
            return
        fi
    fi

    # Method 2: Fallback - parse from redirect URL (no rate limit)
    info "API rate limited, using fallback method..."
    local redirect_url
    redirect_url=$(curl -fsSI "https://github.com/${REPO}/releases/latest" 2>/dev/null | grep -i "^location:" | tr -d '\r' | awk '{print $2}')

    if [[ -n "$redirect_url" ]]; then
        RELEASE_VERSION=$(echo "$redirect_url" | sed -E 's|.*/tag/v||')
    fi

    if [[ -z "${RELEASE_VERSION:-}" ]]; then
        error "Failed to fetch latest version. Try specifying VERSION=x.x.x"
    fi

    info "Latest version: v$RELEASE_VERSION"
}

# Build download URL based on platform and package manager
build_download_url() {
    local base_url="https://github.com/${REPO}/releases/download/v${RELEASE_VERSION}"

    case "$PLATFORM" in
        linux)
            case "$PKG_EXT" in
                deb)
                    # Antigravity.Tools_4.1.25_amd64.deb or _arm64.deb
                    DOWNLOAD_URL="${base_url}/Antigravity.Tools_${RELEASE_VERSION}_${DEB_ARCH}.deb"
                    FILENAME="Antigravity.Tools_${RELEASE_VERSION}_${DEB_ARCH}.deb"
                    ;;
                rpm)
                    # Antigravity.Tools-4.1.25-1.x86_64.rpm or -1.aarch64.rpm
                    DOWNLOAD_URL="${base_url}/Antigravity.Tools-${RELEASE_VERSION}-1.${RPM_ARCH}.rpm"
                    FILENAME="Antigravity.Tools-${RELEASE_VERSION}-1.${RPM_ARCH}.rpm"
                    ;;
                AppImage)
                    # Antigravity.Tools_4.1.25_amd64.AppImage or _aarch64.AppImage
                    local appimage_arch
                    if [[ "$ARCH_LABEL" == "x86_64" ]]; then
                        appimage_arch="amd64"
                    else
                        appimage_arch="aarch64"
                    fi
                    DOWNLOAD_URL="${base_url}/Antigravity.Tools_${RELEASE_VERSION}_${appimage_arch}.AppImage"
                    FILENAME="Antigravity.Tools_${RELEASE_VERSION}_${appimage_arch}.AppImage"
                    ;;
            esac
            ;;
        macos)
            # Prefer universal DMG, fallback to arch-specific
            DOWNLOAD_URL="${base_url}/Antigravity.Tools_${RELEASE_VERSION}_universal.dmg"
            FILENAME="Antigravity.Tools_${RELEASE_VERSION}_universal.dmg"
            ;;
    esac

    info "Download URL: $DOWNLOAD_URL"
}

# Download installer
download_installer() {
    TEMP_DIR=$(mktemp -d)
    DOWNLOAD_PATH="${TEMP_DIR}/${FILENAME}"

    info "Downloading ${APP_NAME} v${RELEASE_VERSION}..."
    run curl -fSL --progress-bar -o "$DOWNLOAD_PATH" "$DOWNLOAD_URL"

    if [[ "${DRY_RUN:-0}" != "1" ]] && [[ ! -f "$DOWNLOAD_PATH" ]]; then
        error "Download failed. Check your network or try a different version."
    fi

    success "Downloaded to $DOWNLOAD_PATH"
}

# Install on Linux
install_linux() {
    info "Installing ${APP_NAME}..."

    case "$PKG_MANAGER" in
        apt)
            run sudo dpkg -i "$DOWNLOAD_PATH"
            run sudo apt-get install -f -y  # Fix dependencies if needed
            ;;
        dnf)
            run sudo dnf install -y "$DOWNLOAD_PATH"
            ;;
        yum)
            run sudo yum install -y "$DOWNLOAD_PATH"
            ;;
        appimage)
            local install_dir="${HOME}/.local/bin"
            run mkdir -p "$install_dir"
            run chmod +x "$DOWNLOAD_PATH"
            run cp "$DOWNLOAD_PATH" "${install_dir}/antigravity-tools"

            if [[ ":$PATH:" != *":${install_dir}:"* ]]; then
                warn "Add ${install_dir} to your PATH to run antigravity-tools from anywhere"

                local shell_name rc_file export_line
                shell_name="$(basename "${SHELL:-/bin/bash}")"
                case "$shell_name" in
                    zsh)  rc_file="$HOME/.zshrc" ;;
                    fish) rc_file="$HOME/.config/fish/config.fish" ;;
                    *)    rc_file="$HOME/.bashrc" ;;
                esac

                export_line="export PATH=\"${install_dir}:\$PATH\""
                [[ "$shell_name" == "fish" ]] && export_line="fish_add_path ${install_dir}"

                if [[ -f "$rc_file" ]] && grep -qF "$install_dir" "$rc_file" 2>/dev/null; then
                    info "PATH entry already in $rc_file"
                else
                    run echo "$export_line" >> "$rc_file"
                    info "Added ${install_dir} to PATH in $rc_file"
                    warn "Run: source $rc_file  (or restart terminal)"
                fi
            fi
            ;;
    esac

    success "${APP_NAME} installed successfully!"
}

# Install on macOS
install_macos() {
    info "Installing ${APP_NAME}..."

    if [[ "${DRY_RUN:-0}" == "1" ]]; then
        echo -e "${YELLOW}[DRY-RUN]${NC} hdiutil attach $DOWNLOAD_PATH -nobrowse -noautoopen"
        echo -e "${YELLOW}[DRY-RUN]${NC} cp -R <mount>/${APP_NAME}.app /Applications/"
        echo -e "${YELLOW}[DRY-RUN]${NC} hdiutil detach <mount>"
        echo -e "${YELLOW}[DRY-RUN]${NC} sudo xattr -rd com.apple.quarantine /Applications/${APP_NAME}.app"
        return
    fi

    # Mount DMG
    local mount_output mount_point
    mount_output=$(hdiutil attach "$DOWNLOAD_PATH" -nobrowse -noautoopen 2>&1)
    mount_point=$(echo "$mount_output" | grep -o '/Volumes/.*' | head -n1)

    if [[ -z "$mount_point" ]]; then
        error "Failed to mount DMG. Output: $mount_output"
    fi

    # Copy app to /Applications
    if [[ -d "/Applications/${APP_NAME}.app" ]]; then
        info "Removing existing installation..."
        rm -rf "/Applications/${APP_NAME}.app"
    fi
    cp -R "${mount_point}/${APP_NAME}.app" /Applications/

    # Unmount DMG
    hdiutil detach "$mount_point" -quiet 2>/dev/null || true

    # Remove quarantine attribute to avoid "app is damaged" error
    info "Removing quarantine attribute..."
    sudo xattr -rd com.apple.quarantine "/Applications/${APP_NAME}.app" 2>/dev/null || true

    success "${APP_NAME} installed to /Applications!"
}

# Cleanup
cleanup() {
    if [[ -n "${TEMP_DIR:-}" ]] && [[ -d "$TEMP_DIR" ]]; then
        rm -rf "$TEMP_DIR"
    fi
}

# Main
main() {
    for arg in "$@"; do
        case "$arg" in
            --help|-h)    show_help ;;
            --version|-v) echo "install.sh v1.0.0"; exit 0 ;;
        esac
    done

    echo ""
    echo -e "${BLUE}========================================${NC}"
    echo -e "${BLUE}    ${APP_NAME} Installer${NC}"
    echo -e "${BLUE}========================================${NC}"
    echo ""

    trap cleanup EXIT

    detect_platform
    detect_linux_distro
    get_version
    build_download_url
    download_installer

    case "$PLATFORM" in
        linux) install_linux ;;
        macos) install_macos ;;
    esac

    echo ""
    success "Installation complete!"
    echo ""
    info "Launch '${APP_NAME}' from your application menu or launcher."
    echo ""
}

main "$@"
