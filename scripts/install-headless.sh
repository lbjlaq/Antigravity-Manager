#!/usr/bin/env bash
# Antigravity Headless 一键安装脚本 (Linux 服务器)
#
# 用法（在线安装）：
#   curl -fsSL https://raw.githubusercontent.com/lbjlaq/Antigravity-Manager/main/scripts/install-headless.sh | bash
#
# 本脚本只做：下载官方 headless 包 -> 校验 -> 解压 -> 放置到安装目录 -> 打印启动说明。
# 不注册系统服务、不修改系统配置。服务生命周期由使用者自行管理。
#
# 环境变量：
#   VERSION       指定版本（如 5.0.4），默认取最新 release
#   INSTALL_DIR   安装目录，默认 $HOME/.local/antigravity-server
#   DRY_RUN       设为 1 时只打印将要执行的操作
#   ASSET_DIR     本地资产目录（测试用），含 headless 包，设置后跳过网络下载
#   REPO          仓库（默认 lbjlaq/Antigravity-Manager；fork 测试时可覆盖）
#   GITHUB_MIRROR 下载镜像前缀（如 https://gh-proxy.org/），加速 GitHub 下载
set -euo pipefail

REPO="${REPO:-lbjlaq/Antigravity-Manager}"
GITHUB_API="${GITHUB_API:-https://api.github.com/repos/${REPO}/releases}"
SCRIPT_VERSION="1.0.0"

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; BLUE='\033[0;34m'; NC='\033[0m'
info()    { echo -e "${BLUE}[INFO]${NC} $*"; }
success() { echo -e "${GREEN}[OK]${NC} $*"; }
warn()    { echo -e "${YELLOW}[WARN]${NC} $*" >&2; }
die()     { echo -e "${RED}[ERROR]${NC} $*" >&2; exit 1; }

run() {
  if [[ "${DRY_RUN:-0}" == "1" ]]; then
    echo -e "${YELLOW}[DRY-RUN]${NC} $*"
  else
    "$@"
  fi
}

# GNU tar 在路径含盘符（Windows Git Bash）时会误判远程主机，--force-local 规避；
# BusyBox/Alpine 等非 GNU tar 不支持该选项，仅检测为 GNU tar 时附加。
tar_local() {
  if tar --version 2>/dev/null | grep -qi 'GNU tar'; then
    tar --force-local "$@"
  else
    tar "$@"
  fi
}

show_help() {
  cat <<EOF
Antigravity Headless 安装脚本 (Linux) v${SCRIPT_VERSION}

用法：
  bash scripts/install-headless.sh [选项]
  curl -fsSL <raw-url>/scripts/install-headless.sh | bash

选项：
  --help      显示本帮助
  --version   显示脚本版本

环境变量：
  VERSION       指定版本（默认最新 release）
  INSTALL_DIR   安装目录（默认 \$HOME/.local/antigravity-server）
  DRY_RUN=1     只打印操作，不实际执行
  ASSET_DIR     本地资产目录（测试用，跳过网络）
  REPO          仓库（默认 lbjlaq/Antigravity-Manager）
  GITHUB_MIRROR 下载镜像前缀（如 https://gh-proxy.org/），加速下载

说明：本脚本只下载/校验/解压/放置文件并打印启动说明，不注册任何系统服务。
EOF
}

for arg in "$@"; do
  case "$arg" in
    --help|-h) show_help; exit 0 ;;
    --version|-v) echo "scripts/install-headless.sh v${SCRIPT_VERSION}"; exit 0 ;;
    *) die "未知参数: $arg（使用 --help 查看用法）" ;;
  esac
done

INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/antigravity-server}"

check_deps() {
  local missing=()
  for cmd in curl tar find mkdir cp rm chmod mv; do
    command -v "$cmd" >/dev/null 2>&1 || missing+=("$cmd")
  done
  [[ ${#missing[@]} -eq 0 ]] || die "缺少必需命令: ${missing[*]}"
}

detect_platform() {
  local os arch
  os="${TARGET_OS:-$(uname -s)}"
  arch="${TARGET_ARCH:-$(uname -m)}"
  [[ "$os" == "Linux" ]] || die "本脚本仅支持 Linux，当前系统: $os"
  case "$arch" in
    x86_64|amd64) ARCH_TAG="x86_64" ;;
    aarch64|arm64) ARCH_TAG="aarch64" ;;
    *) die "不支持的架构: $arch（仅支持 x86_64 / aarch64）" ;;
  esac
  info "检测到平台: Linux ($ARCH_TAG)"
}

is_valid_version() { [[ "$1" =~ ^[0-9]+\.[0-9]+\.[0-9]+ ]]; }

get_version() {
  if [[ -n "${VERSION:-}" ]]; then
    RELEASE_VERSION="${VERSION#[vV]}"
    is_valid_version "$RELEASE_VERSION" || die "VERSION 格式错误: $VERSION（应为 x.y.z，如 5.0.4）"
    info "使用指定版本: v$RELEASE_VERSION"
    return
  fi
  info "正在获取最新版本..."
  local response final_url
  if response=$(curl -fsSL --max-time 15 -H "User-Agent: Antigravity-Headless-Installer" "${GITHUB_API}/latest" 2>/dev/null); then
    RELEASE_VERSION=$(printf '%s' "$response" | grep '"tag_name"' | head -n1 | sed -E 's/.*"tag_name"[[:space:]]*:[[:space:]]*"v?([^"]+)".*/\1/' | tr -d '[:space:]\r\n')
    if is_valid_version "${RELEASE_VERSION:-}"; then
      info "最新版本: v$RELEASE_VERSION"
      return
    fi
    warn "API 返回版本格式异常，尝试重定向方式..."
    RELEASE_VERSION=""
  else
    warn "GitHub API 直连失败（可能限流/网络受限）..."
    # 仅在 API 直连失败时，若配置了镜像则通过镜像重试 API
    if [[ -n "${GITHUB_MIRROR:-}" ]]; then
      local mirror_api="${GITHUB_MIRROR%/}/${GITHUB_API}/latest"
      info "尝试通过镜像获取版本: $mirror_api"
      if response=$(curl -fsSL --max-time 15 -H "User-Agent: Antigravity-Headless-Installer" "$mirror_api" 2>/dev/null); then
        RELEASE_VERSION=$(printf '%s' "$response" | grep '"tag_name"' | head -n1 | sed -E 's/.*"tag_name"[[:space:]]*:[[:space:]]*"v?([^"]+)".*/\1/' | tr -d '[:space:]\r\n')
        if is_valid_version "${RELEASE_VERSION:-}"; then
          info "最新版本（镜像 API）: v$RELEASE_VERSION"
          return
        fi
      fi
      warn "镜像 API 未返回有效版本，尝试重定向方式..."
    else
      warn "尝试重定向方式..."
    fi
  fi
  final_url=$(curl -fsSL --max-time 15 -o /dev/null -w '%{url_effective}' "https://github.com/${REPO}/releases/latest" 2>/dev/null | tr -d '[:space:]\r\n' || true)
  if [[ -n "$final_url" ]]; then
    RELEASE_VERSION=$(printf '%s' "$final_url" | sed -E 's|.*/tag/v?([0-9][^/]*).*|\1|')
  fi
  is_valid_version "${RELEASE_VERSION:-}" || die "无法确定版本，请显式指定: VERSION=x.y.z bash scripts/install-headless.sh"
  info "最新版本（重定向）: v$RELEASE_VERSION"
}

download_file() {
  local url="$1" dest="$2"
  if [[ -n "${ASSET_DIR:-}" ]]; then
    local local_file="${ASSET_DIR}/$(basename "$dest")"
    [[ -f "$local_file" ]] || die "ASSET_DIR 中缺少文件: $local_file"
    info "使用本地资产: $local_file"
    cp "$local_file" "$dest"
    return
  fi
  local fetch_url="$url"
  if [[ -n "${GITHUB_MIRROR:-}" ]]; then
    # 镜像前缀 + 完整原始 URL（gh-proxy.org 用法: https://gh-proxy.org/https://github.com/...）
    fetch_url="${GITHUB_MIRROR%/}/${url}"
  fi
  info "下载: $fetch_url"
  run curl -fSL --retry 3 --max-time 600 -o "$dest" "$fetch_url" || die "下载失败: $fetch_url"
  if [[ "${DRY_RUN:-0}" != "1" ]]; then
    [[ -f "$dest" ]] || die "下载后文件不存在: $dest"
  fi
}

verify_checksum() {
  local archive="$1" checksum_file="$2"
  if [[ "${DRY_RUN:-0}" == "1" ]]; then
    echo -e "${YELLOW}[DRY-RUN]${NC} 校验 $archive 的 SHA256"
    return
  fi
  local expected actual
  expected=$(awk '{print $1}' "$checksum_file" | tr '[:upper:]' '[:lower:]')
  if command -v sha256sum >/dev/null 2>&1; then
    actual=$(sha256sum "$archive" | awk '{print $1}')
  elif command -v shasum >/dev/null 2>&1; then
    actual=$(shasum -a 256 "$archive" | awk '{print $1}')
  else
    warn "未找到 sha256sum 或 shasum，跳过校验"
    return
  fi
  if [[ "$expected" != "$actual" ]]; then
    die "SHA256 校验失败: 期望 $expected，实际 $actual"
  fi
  success "SHA256 校验通过"
}

main() {
  check_deps
  detect_platform
  get_version

  local base="https://github.com/${REPO}/releases/download/v${RELEASE_VERSION}"
  local archive="antigravity-server-linux-${ARCH_TAG}.tar.gz"
  local checksum="${archive}.sha256"

  TMP_DIR=$(mktemp -d)
  trap 'rm -rf "$TMP_DIR"' EXIT

  download_file "${base}/${archive}" "${TMP_DIR}/${archive}"
  download_file "${base}/${checksum}" "${TMP_DIR}/${checksum}"
  verify_checksum "${TMP_DIR}/${archive}" "${TMP_DIR}/${checksum}"

  local src="${TMP_DIR}/src"
  mkdir -p "$src"
  run tar_local -xzf "${TMP_DIR}/${archive}" -C "$src" || die "解压失败"
  if [[ "${DRY_RUN:-0}" == "1" ]]; then
    # DRY_RUN 下不真正解压，造占位以便后续流程继续（不会落到 INSTALL_DIR）
    mkdir -p "$src/dist"
    : > "$src/dist/index.html"
    : > "$src/antigravity-server"
  fi
  [[ -f "$src/dist/index.html" ]] || die "包中缺少 dist/index.html"
  [[ -f "$src/antigravity-server" ]] || die "包中缺少 antigravity-server"

  run mkdir -p "$INSTALL_DIR"
  # 先写 .new 再原子替换，避免覆盖正在运行的二进制时报 Text file busy
  local bin_new="${INSTALL_DIR}/antigravity-server.new"
  if command -v install >/dev/null 2>&1; then
    run install -m 755 "$src/antigravity-server" "$bin_new"
  else
    run cp "$src/antigravity-server" "$bin_new"
    run chmod 755 "$bin_new"
  fi
  run mv -f "$bin_new" "${INSTALL_DIR}/antigravity-server"
  local dist_new="${INSTALL_DIR}/dist.new"
  run rm -rf "$dist_new"
  run cp -r "$src/dist" "$dist_new"
  run rm -rf "${INSTALL_DIR}/dist"
  run mv "$dist_new" "${INSTALL_DIR}/dist"
  if [[ "${DRY_RUN:-0}" != "1" ]]; then
    printf '%s\n' "$RELEASE_VERSION" > "${INSTALL_DIR}/VERSION"
  fi

  echo ""
  success "文件已安装到: $INSTALL_DIR"
  echo ""
  echo "===== 启动说明 ====="
  echo "1) WebUI + API 模式（默认监听 0.0.0.0:8045）："
  echo "   cd '$INSTALL_DIR'"
  echo "   ABV_API_KEY='your-key' ABV_DATA_DIR='$INSTALL_DIR/data' ./antigravity-server -p 8045"
  echo ""
  echo "2) 纯代理模式："
  echo "   ABV_API_KEY='your-key' ./antigravity-server --no-web -p 8045"
  echo ""
  echo "3) 运行依赖（重要）："
  echo "   该二进制链接了 GTK/WebKit，需系统库。Debian/Ubuntu 安装："
  echo "   sudo apt-get install -y libgtk-3-0 libwebkit2gtk-4.1-0 \\"
  echo "     libjavascriptcoregtk-4.1-0 libsoup-3.0-0 libgdk-pixbuf-2.0-0"
  echo "   校验: ldd '$INSTALL_DIR/antigravity-server' | grep 'not found' 应为空"
  echo ""
  echo "4) 安全提示："
  echo "   - headless 默认绑定 0.0.0.0，生产环境务必设置 ABV_API_KEY"
  echo "   - 仅本机访问请加: ABV_BIND_LOCAL_ONLY=1"
  echo "   - 强制指定 WebUI 资源: ABV_DIST_PATH='$INSTALL_DIR/dist'"
  echo "   - 接口: OpenAI 兼容 /v1 ；Anthropic /v1/messages"
  echo ""
  echo "5) 服务生命周期（systemd 等）请自行管理，本脚本不注册服务。"
  echo "   许可证: CC BY-NC-SA 4.0，仅限非商业用途。"
  echo "=================="
}

main "$@"
