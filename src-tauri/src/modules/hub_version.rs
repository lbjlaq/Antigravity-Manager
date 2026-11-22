// 官方 Antigravity Hub 客户端指纹探测与解析模块
//
// 提供官方语言服务枢纽 (Antigravity Hub) 版本号与构建提交点 (CL) 的动态检测能力：
// 1. 用户自定义优先 (custom_version / custom_cl)
// 2. 官方远程自动更新源 (antigravity-hub-auto-updater) 实时探测 (异步非阻塞)
// 3. 本地已安装 Antigravity Hub 探测 (macOS Info.plist / Windows PE / Linux)
// 4. 内置稳态基准兜底 (OFFICIAL_HUB_VERSION / OFFICIAL_HUB_CL)

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::sync::RwLock;
use tracing::{debug, info};

use crate::constants::{OFFICIAL_HUB_CL, OFFICIAL_HUB_VERSION};

/// 客户端指纹解析来源
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HubFingerprintSource {
    /// 用户手动配置覆盖
    Custom,
    /// 官方远程更新源探测
    Remote,
    /// 本地已安装客户端探测
    Local,
    /// 内置稳态基准兜底
    Baseline,
}

impl std::fmt::Display for HubFingerprintSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Custom => write!(f, "custom"),
            Self::Remote => write!(f, "remote"),
            Self::Local => write!(f, "local"),
            Self::Baseline => write!(f, "baseline"),
        }
    }
}

/// 官方 Hub 客户端指纹结构体
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubFingerprint {
    /// 官方 Hub 版本号 (如 "2.33.0")
    pub version: String,
    /// 官方构建提交号 (Piper CL, 如 "1023160026")
    pub cl: String,
    /// 完整出站 User-Agent 字符串
    pub user_agent: String,
    /// 指纹来源标识
    pub source: HubFingerprintSource,
    /// 探测到的官方远程最新版本（仅在动态探测时填充）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_remote_version: Option<String>,
    /// 是否检测到官方发布了比当前出站指纹更新的版本
    #[serde(default)]
    pub update_available: bool,
}

/// 全局缓存的最新 Hub 指纹，初始状态使用纯内存稳态基线，零运行时或 I/O 异常风险
static CACHED_HUB_FINGERPRINT: Lazy<RwLock<HubFingerprint>> = Lazy::new(|| {
    RwLock::new(HubFingerprint {
        version: OFFICIAL_HUB_VERSION.to_string(),
        cl: OFFICIAL_HUB_CL.to_string(),
        user_agent: build_hub_user_agent(OFFICIAL_HUB_VERSION, OFFICIAL_HUB_CL),
        source: HubFingerprintSource::Baseline,
        latest_remote_version: None,
        update_available: false,
    })
});

/// 根据系统和架构获取官方更新清单的 URL
pub fn get_official_updater_manifest_url() -> &'static str {
    match std::env::consts::OS {
        "macos" => match std::env::consts::ARCH {
            "aarch64" => {
                "https://antigravity-hub-auto-updater-974169037036.us-central1.run.app/manifest/latest-arm64-mac.yml"
            }
            _ => {
                "https://antigravity-hub-auto-updater-974169037036.us-central1.run.app/manifest/latest-mac.yml"
            }
        },
        "windows" => {
            "https://antigravity-hub-auto-updater-974169037036.us-central1.run.app/manifest/latest.yml"
        }
        "linux" => {
            "https://antigravity-hub-auto-updater-974169037036.us-central1.run.app/manifest/latest-linux.yml"
        }
        _ => {
            "https://antigravity-hub-auto-updater-974169037036.us-central1.run.app/manifest/latest-arm64-mac.yml"
        }
    }
}

/// User-selected UA platform. Windows is the default for backward compatibility.
fn build_hub_user_agent_for_platform(version: &str, cl: &str, linux: bool) -> String {
    let os_type = if linux { "linux" } else { "windows" };
    format!(
        "antigravity/hub/{} (aidev_client; os_type={}; arch=amd64; cl={})",
        version, os_type, cl
    )
}

/// Preserve the previous Windows default for existing callers.
pub fn build_hub_user_agent(version: &str, cl: &str) -> String {
    build_hub_user_agent_for_platform(version, cl, false)
}

/// 官方最新发布是否比当前**硬编码基线**更新。
///
/// 仅用于启动时的小提示，**绝不改变实际出站指纹**：
/// CL 无法从任何公开端点获取（详见 README/注释），版本号与 CL 必须成对人工校准，
/// 因此自动跟随版本号只会在不提示的情况下制造不一致。
pub fn is_update_available(latest_version: &str) -> bool {
    crate::constants::compare_semver(latest_version, OFFICIAL_HUB_VERSION)
        == std::cmp::Ordering::Greater
}

/// 官方 Hub 发布清单（下载页 https://antigravity.google/download 自身也在此拉取）
/// 返回降序 JSON 数组: [{"version":"2.32.1","execution_id":"6410674700681216"}, ...]
const HUB_RELEASES_URL: &str =
    "https://antigravity-hub-auto-updater-974169037036.us-central1.run.app/releases";

/// 远程发布缓存 TTL：官方 Hub 版本迭代约数周一次，且本进程只在**启动时**自动探测一次；
/// 6 小时足以覆盖配置保存 / 服务重启等重复触发，又不会让手动「强制探测」拿到过旧数据
/// （手动路径 `fetch_remote_hub_release_forced()` 会 bypass 缓存）。
const REMOTE_RELEASE_TTL: std::time::Duration = std::time::Duration::from_secs(6 * 3600);

/// 官方 Hub 单条发布记录
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct HubRelease {
    pub version: String,
    pub execution_id: String,
}

static REMOTE_RELEASE_CACHE: Lazy<RwLock<Option<(std::time::Instant, HubRelease)>>> =
    Lazy::new(|| RwLock::new(None));

/// 校验 Hub 版本号形态 X.Y.Z（官方 releases 恒为三段纯数字）
fn is_valid_hub_version(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

/// 本地探测读取的是 Antigravity **IDE** 版本线（4.x），与 Hub 版本线（2.x）不同源；
/// 直接采用会拼出 `antigravity/hub/4.3.0` 这类非法指纹，故只接受 2.x。
fn is_acceptable_local_hub_version(v: &str) -> bool {
    is_valid_hub_version(v) && v.split('.').next() == Some("2")
}

/// 从 `/releases` JSON 文本解析最新发布（官方为降序，取第一个合法项）
pub fn parse_hub_releases(json: &str) -> Option<HubRelease> {
    let list: Vec<HubRelease> = serde_json::from_str(json).ok()?;
    list.into_iter().find(|r| {
        is_valid_hub_version(&r.version)
            && !r.execution_id.is_empty()
            && r.execution_id.chars().all(|c| c.is_ascii_digit())
    })
}

/// 远程探测最新 Hub 发布（进程内 6h 缓存，失败不缓存以便下次重试）
pub async fn fetch_remote_hub_release() -> Option<HubRelease> {
    fetch_remote_hub_release_with(false).await
}

/// 跳过进程内缓存强制拉取（用于 UI 的「强制探测」按钮）
pub async fn fetch_remote_hub_release_forced() -> Option<HubRelease> {
    fetch_remote_hub_release_with(true).await
}

async fn fetch_remote_hub_release_with(force: bool) -> Option<HubRelease> {
    let cached = REMOTE_RELEASE_CACHE
        .read()
        .ok()
        .and_then(|guard| guard.as_ref().cloned());

    if !force {
        if let Some((cached_at, release)) = cached.as_ref() {
            if cached_at.elapsed() < REMOTE_RELEASE_TTL {
                return Some(release.clone());
            }
        }
    }

    if let Some(release) = fetch_remote_hub_release_once().await {
        if let Ok(mut guard) = REMOTE_RELEASE_CACHE.write() {
            *guard = Some((std::time::Instant::now(), release.clone()));
        }
        // 仅记录到日志；出站指纹永远使用硬编码校准值（版本+CL 必须成对）
        debug!(
            version = %release.version,
            execution_id = %release.execution_id,
            "Remote Hub release detected"
        );
        return Some(release);
    }

    // stale-if-error：刷新失败时优先复用上一次成功结果（即使已过期），绝不降级到常量
    if let Some((_, release)) = cached {
        debug!(
            version = %release.version,
            "Hub /releases refresh failed; reusing cached release"
        );
        return Some(release);
    }
    None
}

/// 单次拉取 `/releases`，任何失败（断网/超时/非 200/结构非法）都返回 None
async fn fetch_remote_hub_release_once() -> Option<HubRelease> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .ok()?;

    let resp = match client.get(HUB_RELEASES_URL).send().await {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => {
            debug!(status = %r.status(), "Hub /releases returned non-success status");
            return None;
        }
        Err(e) => {
            debug!("Hub /releases unreachable: {}", e);
            return None;
        }
    };

    parse_hub_releases(&resp.text().await.ok()?)
}

/// 从更新清单 YAML 文本中解析 version 字段 (纯函数)
/// 仅作为 `/releases` 不可用时的兜底（旧清单 Windows/Linux 已 404，只剩 arm64 mac 有效）
pub fn parse_hub_version_from_manifest(text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("version:") {
            let ver = rest.trim().trim_matches(|c| c == '\'' || c == '"');
            if !ver.is_empty()
                && ver
                    .chars()
                    .all(|c| c.is_ascii_digit() || c == '.' || c == '-')
            {
                debug!(remote_version = %ver, "Successfully parsed remote Hub version");
                return Some(ver.to_string());
            }
        }
    }
    None
}

/// 尝试从官方远程源探测最新 Hub 版本号
/// 首选 `/releases` JSON（全平台权威，下载页自身也在此拉取）；失败时回退旧清单 YAML
pub async fn try_detect_remote_hub_version() -> Option<String> {
    try_detect_remote_hub_version_with(false).await
}

async fn try_detect_remote_hub_version_with(force: bool) -> Option<String> {
    if let Some(release) = fetch_remote_hub_release_with(force).await {
        return Some(release.version);
    }

    let url = get_official_updater_manifest_url();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .ok()?;

    let resp = match client.get(url).send().await {
        Ok(r) => r,
        Err(e) => {
            debug!("Remote Hub updater manifest unreachable: {}", e);
            return None;
        }
    };

    if !resp.status().is_success() {
        return None;
    }

    parse_hub_version_from_manifest(&resp.text().await.ok()?)
}

/// 尝试从本地安装的 `language_server` 二进制文件中提取 Piper 构建变更号 (CL)
/// Google Blaze/Piper 构建在二进制中嵌入了 `changelist <CL> with baseline` 元数据
pub fn extract_cl_from_binary(path: &std::path::Path) -> Option<String> {
    use std::io::Read;

    let mut file = std::fs::File::open(path).ok()?;
    // 以 4MB 块流式扫描，保留 128 字节交叠，防止跨块截断
    let chunk_size = 4 * 1024 * 1024;
    let overlap = 128;
    let mut prev = Vec::new();
    let mut buf = vec![0u8; chunk_size];

    loop {
        let n = file.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }

        let mut data = Vec::with_capacity(prev.len() + n);
        data.extend_from_slice(&prev);
        data.extend_from_slice(&buf[..n]);

        let needle = b"changelist ";
        let mut pos = 0;
        while let Some(idx) = data[pos..].windows(needle.len()).position(|w| w == needle) {
            let match_pos = pos + idx;
            let rest = &data[match_pos + needle.len()..];
            let mut digits = Vec::new();
            for &b in rest {
                if b.is_ascii_digit() {
                    digits.push(b as char);
                } else {
                    break;
                }
            }
            if digits.len() >= 8 && digits.len() <= 12 {
                return Some(digits.into_iter().collect());
            }
            pos = match_pos + needle.len();
        }

        if n < chunk_size {
            break;
        }
        prev = buf[n.saturating_sub(overlap)..n].to_vec();
    }

    None
}

/// 尝试从本地已安装的 Antigravity 客户端探测 Hub 语言服务路径
fn find_local_language_server_path() -> Option<std::path::PathBuf> {
    if let Some(exe_path) = crate::modules::process::get_antigravity_executable_path(None) {
        let path_str = exe_path.to_string_lossy();
        let app_dir = if let Some(idx) = path_str.find(".app") {
            std::path::PathBuf::from(&path_str[..idx + 4])
        } else {
            exe_path.clone()
        };

        let mut candidates = vec![
            app_dir.join("Contents/Resources/bin/language_server"),
            app_dir.join("Contents/Resources/bin/language_server.exe"),
            app_dir.join("resources/bin/language_server.exe"),
            app_dir.join("resources/bin/language_server"),
            app_dir.join("bin/language_server"),
            app_dir.join("bin/language_server.exe"),
        ];

        // 也检查可执行文件的上层目录
        let mut parent = exe_path.as_path();
        for _ in 0..4 {
            if let Some(p) = parent.parent() {
                candidates.push(p.join("Contents/Resources/bin/language_server"));
                candidates.push(p.join("resources/bin/language_server.exe"));
                candidates.push(p.join("resources/bin/language_server"));
                candidates.push(p.join("bin/language_server"));
                parent = p;
            } else {
                break;
            }
        }

        for candidate in candidates {
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// 尝试从本地已安装的 Antigravity 客户端提取同源且完整的 (版本号, CL) 配对
/// 必须两者同时提取成功才返回，确保绝不拼装出非法出站 UA
pub fn try_detect_local_hub_fingerprint() -> Option<(String, String)> {
    let local_ver = try_detect_local_hub_version()?;
    let ls_path = find_local_language_server_path()?;
    let cl = extract_cl_from_binary(&ls_path)?;
    Some((local_ver, cl))
}

/// 尝试从本地已安装的 Antigravity 客户端探测 Hub 版本（仅接受 2.x Hub 版本线，过滤 IDE 4.x 版本）
pub fn try_detect_local_hub_version() -> Option<String> {
    if let Ok(local_ver) = crate::modules::version::get_antigravity_version(None) {
        let ver = if !local_ver.short_version.is_empty() {
            local_ver.short_version
        } else {
            local_ver.bundle_version
        };
        if is_acceptable_local_hub_version(&ver) {
            return Some(ver);
        }
    }
    None
}

/// 同步解析 Hub 客户端指纹（不发起任何外部网络 I/O，安全可靠）
/// 出站指纹必须严格保证「版本号 + CL + 平台段三者同源」！
/// 优先级: 用户手动显式双字段自定义 > 稳态基准兜底
pub fn resolve_hub_fingerprint_sync(
    custom_version: Option<&str>,
    custom_cl: Option<&str>,
    hub_platform: Option<&str>,
) -> HubFingerprint {
    let linux = hub_platform == Some("linux");
    let clean_ver = custom_version.map(|s| s.trim()).filter(|s| !s.is_empty());
    let clean_cl = custom_cl.map(|s| s.trim()).filter(|s| !s.is_empty());

    // 1. 如果用户显式且同时自定义了版本与 CL，尊重用户配置
    if let (Some(v), Some(c)) = (clean_ver, clean_cl) {
        return HubFingerprint {
            version: v.to_string(),
            cl: c.to_string(),
            user_agent: build_hub_user_agent_for_platform(v, c, linux),
            source: HubFingerprintSource::Custom,
            latest_remote_version: None,
            update_available: false,
        };
    }

    if clean_ver.is_some() || clean_cl.is_some() {
        tracing::warn!(
            custom_version = ?clean_ver,
            custom_cl = ?clean_cl,
            "Custom Hub fingerprint requires both version and CL to avoid upstream signature mismatch; falling back to calibrated baseline"
        );
    }

    // 2. 稳态基准兜底（硬编码校准值：版本与 CL 必须严格成对人工校准同源）
    // Google Piper monorepo 的 CL 严格单调递增，版本号与 CL 必须人工校准成对（如 2.33.0 对应 1023160026）。
    // 严禁将本地已安装旧版或远程未校准版本与基准 CL 错配拼接；错配的弗兰肯斯坦指纹
    // 会被 Google Cloud Code WAF 判定为非正版客户端模拟（触发 TOS_VIOLATION 封禁）。
    HubFingerprint {
        version: OFFICIAL_HUB_VERSION.to_string(),
        cl: OFFICIAL_HUB_CL.to_string(),
        user_agent: build_hub_user_agent_for_platform(OFFICIAL_HUB_VERSION, OFFICIAL_HUB_CL, linux),
        source: HubFingerprintSource::Baseline,
        latest_remote_version: None,
        update_available: false,
    }
}

/// 异步全流程解析 Hub 客户端指纹
/// 同样遵循版本与 CL 严格同源校准原则，防止远程源或本地旧版污染出站 UA
pub async fn resolve_hub_fingerprint_async(
    custom_version: Option<&str>,
    custom_cl: Option<&str>,
    hub_platform: Option<&str>,
) -> HubFingerprint {
    resolve_hub_fingerprint_async_with(custom_version, custom_cl, hub_platform, false).await
}

/// 同上，`force_remote` 为 true 时跳过缓存拉取远程发布源信息
pub async fn resolve_hub_fingerprint_async_with(
    custom_version: Option<&str>,
    custom_cl: Option<&str>,
    hub_platform: Option<&str>,
    force_remote: bool,
) -> HubFingerprint {
    let linux = hub_platform == Some("linux");
    // 异步探测官方最新远程发布版本，用于状态感知与更新提醒
    let remote_release = fetch_remote_hub_release_with(force_remote).await;
    let latest_remote_ver = remote_release.map(|r| r.version);

    // 1. 如果用户显式定义了自定义指纹
    let clean_ver = custom_version.map(|s| s.trim()).filter(|s| !s.is_empty());
    let clean_cl = custom_cl.map(|s| s.trim()).filter(|s| !s.is_empty());
    if let (Some(v), Some(c)) = (clean_ver, clean_cl) {
        let update_avail = latest_remote_ver.as_deref().map_or(false, |latest| {
            crate::constants::compare_semver(latest, v) == std::cmp::Ordering::Greater
        });
        return HubFingerprint {
            version: v.to_string(),
            cl: c.to_string(),
            user_agent: build_hub_user_agent_for_platform(v, c, linux),
            source: HubFingerprintSource::Custom,
            latest_remote_version: latest_remote_ver,
            update_available: update_avail,
        };
    }

    // Keep the existing local version/CL detection unchanged for both selected platforms.
    if let Some((local_ver, local_cl)) = try_detect_local_hub_fingerprint() {
        let update_avail = latest_remote_ver.as_deref().map_or(false, |latest| {
            crate::constants::compare_semver(latest, &local_ver) == std::cmp::Ordering::Greater
        });
        return HubFingerprint {
            version: local_ver.clone(),
            cl: local_cl.clone(),
            user_agent: build_hub_user_agent_for_platform(&local_ver, &local_cl, linux),
            source: HubFingerprintSource::Local,
            latest_remote_version: latest_remote_ver,
            update_available: update_avail,
        };
    }

    // 3. 稳态基准兜底
    let update_avail = latest_remote_ver
        .as_deref()
        .map_or(false, is_update_available);

    HubFingerprint {
        version: OFFICIAL_HUB_VERSION.to_string(),
        cl: OFFICIAL_HUB_CL.to_string(),
        user_agent: build_hub_user_agent_for_platform(OFFICIAL_HUB_VERSION, OFFICIAL_HUB_CL, linux),
        source: HubFingerprintSource::Baseline,
        latest_remote_version: latest_remote_ver,
        update_available: update_avail,
    }
}

/// 获取当前缓存的 Hub 客户端指纹（带毒化恢复保障）
pub fn get_current_hub_fingerprint() -> HubFingerprint {
    let guard = match CACHED_HUB_FINGERPRINT.read() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard.clone()
}

/// 获取当前生效的 Hub User-Agent
pub fn get_current_hub_user_agent() -> String {
    get_current_hub_fingerprint().user_agent
}

/// 同步更新 Hub 客户端指纹（适用于启动配置或配置保存）
pub fn update_hub_fingerprint(
    custom_version: Option<&str>,
    custom_cl: Option<&str>,
    hub_platform: Option<&str>,
) -> HubFingerprint {
    let resolved = resolve_hub_fingerprint_sync(custom_version, custom_cl, hub_platform);
    info!(
        version = %resolved.version,
        cl = %resolved.cl,
        source = %resolved.source,
        "Hub client fingerprint updated (sync)"
    );
    let mut lock = match CACHED_HUB_FINGERPRINT.write() {
        Ok(l) => l,
        Err(poisoned) => poisoned.into_inner(),
    };
    *lock = resolved.clone();
    resolved
}

/// 异步探测并更新 Hub 客户端指纹（适用于启动自动探测或后台刷新，走 6h 发布缓存）
pub async fn detect_and_update_hub_fingerprint(
    custom_version: Option<&str>,
    custom_cl: Option<&str>,
    hub_platform: Option<&str>,
) -> HubFingerprint {
    detect_and_update_hub_fingerprint_with(custom_version, custom_cl, hub_platform, false).await
}

/// 同上，`force_remote = true` 时跳过发布缓存强制拉取（UI「强制探测」按钮）
pub async fn detect_and_update_hub_fingerprint_with(
    custom_version: Option<&str>,
    custom_cl: Option<&str>,
    hub_platform: Option<&str>,
    force_remote: bool,
) -> HubFingerprint {
    let resolved =
        resolve_hub_fingerprint_async_with(custom_version, custom_cl, hub_platform, force_remote).await;
    info!(
        version = %resolved.version,
        cl = %resolved.cl,
        source = %resolved.source,
        "Hub client fingerprint detected and updated (async)"
    );
    let mut lock = match CACHED_HUB_FINGERPRINT.write() {
        Ok(l) => l,
        Err(poisoned) => poisoned.into_inner(),
    };
    *lock = resolved.clone();
    resolved
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_hub_user_agent() {
        let ua = build_hub_user_agent(OFFICIAL_HUB_VERSION, OFFICIAL_HUB_CL);
        // 平台段固定为官方 Windows x64 指纹，与宿主机操作系统无关
        assert_eq!(
            ua,
            format!(
                "antigravity/hub/{} (aidev_client; os_type=windows; arch=amd64; cl={})",
                OFFICIAL_HUB_VERSION, OFFICIAL_HUB_CL
            )
        );
    }

    #[test]
    fn test_selected_platform_changes_only_user_agent_os_type() {
        let linux = resolve_hub_fingerprint_sync(Some("2.33.0"), Some("1023160026"), Some("linux"));
        let windows = resolve_hub_fingerprint_sync(Some("2.33.0"), Some("1023160026"), None);
        assert_eq!(linux.version, windows.version);
        assert_eq!(linux.cl, windows.cl);
        assert_eq!(linux.source, windows.source);
        assert!(linux.user_agent.contains("os_type=linux; arch=amd64"));
        assert!(windows.user_agent.contains("os_type=windows; arch=amd64"));

        let linux_baseline = resolve_hub_fingerprint_sync(None, None, Some("linux"));
        let windows_baseline = resolve_hub_fingerprint_sync(None, None, None);
        assert_eq!(linux_baseline.version, windows_baseline.version);
        assert_eq!(linux_baseline.cl, windows_baseline.cl);
        assert_eq!(linux_baseline.source, windows_baseline.source);
        assert!(linux_baseline.user_agent.contains("os_type=linux; arch=amd64"));
        assert!(windows_baseline.user_agent.contains("os_type=windows; arch=amd64"));
    }

    #[test]
    fn test_custom_override_takes_precedence() {
        let fp = resolve_hub_fingerprint_sync(Some("3.0.0"), Some("12345678"), None);
        assert_eq!(fp.version, "3.0.0");
        assert_eq!(fp.cl, "12345678");
        assert_eq!(fp.source, HubFingerprintSource::Custom);
        assert!(!fp.user_agent.contains(OFFICIAL_HUB_VERSION));
        assert!(fp.user_agent.contains("3.0.0"));
    }

    #[test]
    fn test_parse_hub_releases_prefers_first_valid() {
        // 官方返回降序数组，取第一条合法项
        let sample = r#"[{"version":"2.28.0","execution_id":"6410674700681216"},{"version":"2.27.0","execution_id":"4933976025071616"}]"#;
        let release = parse_hub_releases(sample).expect("must parse official payload");
        assert_eq!(release.version, "2.28.0");
        assert_eq!(release.execution_id, "6410674700681216");

        // 非法项（版本非 X.Y.Z / execution_id 非纯数字）必须被跳过
        let noisy = r#"[{"version":"v2.28","execution_id":"1"},{"version":"2.27.0","execution_id":"abc"},{"version":"2.27.0","execution_id":"99"}]"#;
        assert_eq!(parse_hub_releases(noisy).unwrap().version, "2.27.0");
        assert_eq!(parse_hub_releases("not json"), None);
    }

    #[test]
    fn test_local_hub_version_line_guard() {
        // Hub 版本线：2.x 可用
        assert!(is_acceptable_local_hub_version("2.28.0"));
        assert!(is_acceptable_local_hub_version("2.30.1"));
        // IDE 版本线 4.x 绝不能被当作 hub 版本
        assert!(!is_acceptable_local_hub_version("4.3.0"));
        assert!(!is_acceptable_local_hub_version("2.28"));
    }

    #[test]
    fn test_parse_hub_version_from_manifest() {
        let sample_yaml = r#"
version: 2.23.1
files:
  - url: Antigravity-2.23.1-arm64-mac.zip
    sha512: abcdef
"#;
        let ver = parse_hub_version_from_manifest(sample_yaml);
        assert_eq!(ver, Some("2.23.1".to_string()));
    }

    #[test]
    fn test_unpaired_custom_falls_back_to_calibrated_baseline() {
        // 仅传版本不传 CL，必须回退到基线，绝不产生弗兰肯斯坦指纹
        let fp = resolve_hub_fingerprint_sync(Some("2.23.1"), None, None);
        assert_eq!(fp.version, OFFICIAL_HUB_VERSION);
        assert_eq!(fp.cl, OFFICIAL_HUB_CL);
        assert_eq!(fp.source, HubFingerprintSource::Baseline);

        // 仅传 CL 不传版本，同理必须回退到基线
        let fp_cl_only = resolve_hub_fingerprint_sync(None, Some("99999999"), None);
        assert_eq!(fp_cl_only.version, OFFICIAL_HUB_VERSION);
        assert_eq!(fp_cl_only.cl, OFFICIAL_HUB_CL);
        assert_eq!(fp_cl_only.source, HubFingerprintSource::Baseline);
    }

    #[test]
    fn test_extract_cl_from_binary_stream() {
        use std::io::Write;
        let temp_dir = std::env::temp_dir();
        let test_bin_path = temp_dir.join(format!("test_ls_binary_{}", uuid::Uuid::new_v4()));
        let mut file = std::fs::File::create(&test_bin_path).unwrap();
        // 构造带有 Blaze/Piper 类似构建字符串的二进制片段
        let dummy_data = b"Some random prefix data \x00\x01\x02 changelist 1023160026 with baseline 1022452901 in a mint client \x00 trailing";
        file.write_all(dummy_data).unwrap();
        drop(file);

        let cl = extract_cl_from_binary(&test_bin_path);
        let _ = std::fs::remove_file(&test_bin_path);
        assert_eq!(cl, Some("1023160026".to_string()));
    }

    #[test]
    fn test_baseline_fallback() {
        let fp = resolve_hub_fingerprint_sync(None, None, None);
        assert_eq!(fp.version, OFFICIAL_HUB_VERSION);
        assert_eq!(fp.cl, OFFICIAL_HUB_CL);
        assert_eq!(fp.source, HubFingerprintSource::Baseline);
        assert_eq!(
            fp.user_agent,
            format!(
                "antigravity/hub/{} (aidev_client; os_type=windows; arch=amd64; cl={})",
                OFFICIAL_HUB_VERSION, OFFICIAL_HUB_CL
            )
        );
    }
}
