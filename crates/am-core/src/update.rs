use crate::config::UpdateConfig;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 同仓库独立发布 Tag（与主分支 releases/latest 完全隔离）
pub const RELEASE_TAG: &str = "antigravity-manager";
pub const HOMEBREW_CASK_NAME: &str = "antigravity-manager";
pub const UPDATER_JSON_URL: &str =
    "https://github.com/lbjlaq/Antigravity-Manager/releases/download/antigravity-manager/updater.json";
const GITHUB_TAG_RELEASE_API: &str =
    "https://api.github.com/repos/lbjlaq/Antigravity-Manager/releases/tags/antigravity-manager";
const GITHUB_TAG_RELEASE_PAGE: &str =
    "https://github.com/lbjlaq/Antigravity-Manager/releases/tag/antigravity-manager";

/// 当前安装来源通道，用于决定热更新方式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum InstallChannel {
    /// 支持 Tauri 原生应用内签名更新（macOS DMG/.app、Windows NSIS、Linux AppImage）
    #[default]
    Native,
    /// macOS Homebrew Cask 安装，优先走 `brew upgrade --cask antigravity-manager`
    Homebrew,
    /// Linux DEB/RPM 等非 AppImage 包管理器安装，引导手动下载以避免 ENOEXEC
    Manual,
}

/// 正式版在线更新检查结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub has_update: bool,
    pub download_url: String,
    pub release_notes: String,
    pub published_at: String,
    #[serde(default)]
    pub install_channel: InstallChannel,
}

#[derive(Debug, Deserialize)]
struct UpdaterManifest {
    version: String,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    pub_date: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    #[serde(default)]
    name: Option<String>,
    html_url: String,
    body: Option<String>,
    published_at: Option<String>,
}

/// 检测当前运行实例的安装来源（macOS Homebrew Cask / Linux AppImage vs DEB/RPM / Windows 原生）
pub fn detect_install_channel() -> InstallChannel {
    #[cfg(target_os = "macos")]
    {
        if is_homebrew_installed() {
            return InstallChannel::Homebrew;
        }
        InstallChannel::Native
    }

    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("APPIMAGE").is_some() {
            InstallChannel::Native
        } else {
            InstallChannel::Manual
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        InstallChannel::Native
    }
}

/// 检测是否通过 macOS Homebrew Cask 安装了 `antigravity-manager`
pub fn is_homebrew_installed() -> bool {
    #[cfg(target_os = "macos")]
    {
        let caskroom_paths = [
            "/opt/homebrew/Caskroom/antigravity-manager",
            "/usr/local/Caskroom/antigravity-manager",
        ];
        caskroom_paths
            .iter()
            .any(|path| std::path::Path::new(path).exists())
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

fn normalize_proxy_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.contains("://") {
        Some(trimmed.to_string())
    } else {
        Some(format!("http://{}", trimmed))
    }
}

/// 从常见系统代理环境变量解析代理地址（供更新检查与 Tauri 原生更新器共用）
pub fn detect_system_proxy() -> Option<String> {
    for var in [
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
        "HTTP_PROXY",
        "http_proxy",
    ] {
        if let Ok(val) = std::env::var(var) {
            if let Some(normalized) = normalize_proxy_url(&val) {
                return Some(normalized);
            }
        }
    }
    None
}

pub fn current_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 判断是否达到自动检查更新条件（遵循 `auto_check` 开关与冷却小时数）
pub fn should_auto_check(cfg: &UpdateConfig) -> bool {
    should_auto_check_at(cfg, current_unix_secs())
}

fn should_auto_check_at(cfg: &UpdateConfig, now_secs: u64) -> bool {
    if !cfg.auto_check {
        return false;
    }
    if cfg.last_check_time == 0 {
        return true;
    }
    let interval_secs = cfg.check_interval_hours.max(1).saturating_mul(3600);
    now_secs.saturating_sub(cfg.last_check_time) >= interval_secs
}

/// 判断目标版本是否已被用户选择「跳过此版本」
pub fn is_version_skipped(cfg: &UpdateConfig, latest_version: &str) -> bool {
    let Some(skipped) = cfg.skipped_version.as_deref() else {
        return false;
    };
    fn clean(s: &str) -> &str {
        s.trim()
            .trim_start_matches("am-")
            .trim_start_matches('v')
            .trim_start_matches('V')
    }
    let s = clean(skipped);
    !s.is_empty() && s.eq_ignore_ascii_case(clean(latest_version))
}

/// 通过 macOS Homebrew Cask 升级 `antigravity-manager`（带 180 秒超时保护）
#[cfg(not(target_os = "macos"))]
pub async fn brew_upgrade_cask() -> Result<String> {
    anyhow::bail!("Homebrew Cask upgrade is only supported on macOS")
}

#[cfg(target_os = "macos")]
pub async fn brew_upgrade_cask() -> Result<String> {
    let brew_path = if std::path::Path::new("/opt/homebrew/bin/brew").exists() {
        "/opt/homebrew/bin/brew"
    } else if std::path::Path::new("/usr/local/bin/brew").exists() {
        "/usr/local/bin/brew"
    } else {
        "brew"
    };

    let output = tokio::time::timeout(
        Duration::from_secs(180),
        tokio::process::Command::new(brew_path)
            .args(["upgrade", "--cask", HOMEBREW_CASK_NAME])
            .output(),
    )
    .await
    .map_err(|_| anyhow::anyhow!("Homebrew upgrade timed out after 180s"))?
    .context("Failed to execute brew upgrade command")?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if output.status.success() {
        Ok(stdout)
    } else {
        let detail = if !stderr.is_empty() { stderr } else { stdout };
        anyhow::bail!("brew upgrade --cask failed: {}", detail)
    }
}

/// 从任意标题或 Tag 字符串（如 `"Antigravity Manager v0.1.0"`、`"am-v0.2.3-beta.1"`）中提取 SemVer 版本号
fn extract_release_semver(raw: &str) -> String {
    let trimmed = raw.trim();
    for token in trimmed.split_whitespace() {
        let candidate = token
            .trim_matches(|c: char| matches!(c, '(' | ')' | '[' | ']' | ',' | ':'))
            .trim_start_matches("am-")
            .trim_start_matches('v')
            .trim_start_matches('V');
        let mut parts = candidate.split('-').next().unwrap_or("").split('.');
        let valid = matches!(
            (parts.next(), parts.next(), parts.next()),
            (Some(a), Some(b), Some(c))
                if !a.is_empty()
                    && !b.is_empty()
                    && !c.is_empty()
                    && a.chars().all(|ch| ch.is_ascii_digit())
                    && b.chars().all(|ch| ch.is_ascii_digit())
                    && c.chars().all(|ch| ch.is_ascii_digit())
        );
        if valid {
            return candidate.to_string();
        }
    }
    trimmed
        .trim_start_matches("am-")
        .trim_start_matches('v')
        .trim_start_matches('V')
        .to_string()
}

/// 比较两个 SemVer 版本号（仅比较主.次.修订数字部分），若 latest > current 返回 true
pub fn is_newer_version(current: &str, latest: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> {
        let extracted = extract_release_semver(s);
        extracted
            .split('-')
            .next()
            .unwrap_or("")
            .split('.')
            .map(|p| p.parse::<u64>().unwrap_or(0))
            .collect()
    };

    let c = parse(current);
    let l = parse(latest);
    let len = c.len().max(l.len());
    for i in 0..len {
        let cv = *c.get(i).unwrap_or(&0);
        let lv = *l.get(i).unwrap_or(&0);
        if lv > cv {
            return true;
        }
        if lv < cv {
            return false;
        }
    }
    false
}

fn build_update_http_client() -> reqwest::Client {
    if let Some(proxy_url) = detect_system_proxy() {
        if let Ok(proxy) = reqwest::Proxy::all(&proxy_url) {
            if let Ok(client) = reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .connect_timeout(Duration::from_secs(10))
                .proxy(proxy)
                .build()
            {
                return client;
            }
        }
    }
    crate::runtime::shared_http_client().clone()
}

/// 检查同仓库独立 Tag (`antigravity-manager`) 最新正式版
/// 优先拉取 updater.json，若尚未发布则降级查询 GitHub Release Tag API
pub async fn check_for_updates(current_version: &str) -> Result<UpdateInfo> {
    let current_clean = current_version
        .trim()
        .trim_start_matches('v')
        .trim_start_matches('V')
        .to_string();

    let client = build_update_http_client();
    let ua = format!("Antigravity-Manager/{}", current_clean);
    let install_channel = detect_install_channel();

    // 1. 优先读取独立 Tag 下的 updater.json
    if let Ok(res) = client
        .get(UPDATER_JSON_URL)
        .timeout(Duration::from_secs(12))
        .header(reqwest::header::USER_AGENT, &ua)
        .send()
        .await
    {
        if res.status().is_success() {
            if let Ok(manifest) = res.json::<UpdaterManifest>().await {
                let latest_clean = manifest
                    .version
                    .trim()
                    .trim_start_matches('v')
                    .trim_start_matches('V')
                    .to_string();
                let has_update = is_newer_version(&current_clean, &latest_clean);
                return Ok(UpdateInfo {
                    current_version: current_clean,
                    latest_version: latest_clean,
                    has_update,
                    download_url: GITHUB_TAG_RELEASE_PAGE.to_string(),
                    release_notes: manifest.notes.unwrap_or_default(),
                    published_at: manifest.pub_date.unwrap_or_default(),
                    install_channel,
                });
            }
        }
    }

    // 2. 降级查询 GitHub Releases Tag API
    let res = client
        .get(GITHUB_TAG_RELEASE_API)
        .timeout(Duration::from_secs(12))
        .header(reqwest::header::USER_AGENT, &ua)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .context("Failed to reach GitHub Releases Tag API")?;

    if !res.status().is_success() {
        anyhow::bail!(
            "Release tag '{}' not found or unavailable (HTTP {})",
            RELEASE_TAG,
            res.status()
        );
    }

    let release: GitHubRelease = res
        .json()
        .await
        .context("Failed to parse GitHub Release JSON")?;

    let raw_ver = release
        .name
        .as_deref()
        .filter(|n| n.chars().any(|c| c.is_ascii_digit()))
        .unwrap_or(&release.tag_name);
    let latest_clean = extract_release_semver(raw_ver);

    let has_update = is_newer_version(&current_clean, &latest_clean);

    Ok(UpdateInfo {
        current_version: current_clean,
        latest_version: latest_clean,
        has_update,
        download_url: release.html_url,
        release_notes: release.body.unwrap_or_default(),
        published_at: release.published_at.unwrap_or_default(),
        install_channel,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_semver_from_release_titles_and_tags() {
        assert_eq!(
            extract_release_semver("Antigravity Manager v0.1.0"),
            "0.1.0"
        );
        assert_eq!(extract_release_semver("am-v1.2.3"), "1.2.3");
        assert_eq!(extract_release_semver("v0.2.0-beta.1"), "0.2.0-beta.1");
        assert_eq!(
            extract_release_semver("Antigravity Manager (v2.3.4)"),
            "2.3.4"
        );
    }

    #[test]
    fn compares_semver_versions_correctly() {
        assert!(is_newer_version("0.0.1", "Antigravity Manager v0.1.0"));
        assert!(is_newer_version("0.1.0", "am-v0.1.1"));
        assert!(!is_newer_version("0.1.0", "Antigravity Manager v0.1.0"));
        assert!(!is_newer_version("0.2.0", "0.1.9"));
    }

    #[test]
    fn evaluates_auto_check_schedule_and_skip_version() {
        let mut cfg = UpdateConfig::default();
        assert!(should_auto_check_at(&cfg, 100_000));

        cfg.last_check_time = 100_000;
        assert!(!should_auto_check_at(&cfg, 100_000 + 3600));
        assert!(should_auto_check_at(&cfg, 100_000 + 24 * 3600));

        cfg.auto_check = false;
        assert!(!should_auto_check_at(&cfg, 100_000 + 48 * 3600));

        cfg.skipped_version = Some("v0.0.2".to_string());
        assert!(is_version_skipped(&cfg, "0.0.2"));
        assert!(is_version_skipped(&cfg, "am-v0.0.2"));
        assert!(!is_version_skipped(&cfg, "0.0.3"));
    }

    #[test]
    fn normalizes_proxy_urls() {
        assert_eq!(normalize_proxy_url(""), None);
        assert_eq!(
            normalize_proxy_url("127.0.0.1:7890"),
            Some("http://127.0.0.1:7890".to_string())
        );
        assert_eq!(
            normalize_proxy_url("http://127.0.0.1:7890"),
            Some("http://127.0.0.1:7890".to_string())
        );
    }
}
