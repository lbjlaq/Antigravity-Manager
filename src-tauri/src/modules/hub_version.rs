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
    /// 官方 Hub 版本号 (如 "2.23.1")
    pub version: String,
    /// 官方构建提交号 (Piper CL, 如 "998881661")
    pub cl: String,
    /// 完整出站 User-Agent 字符串
    pub user_agent: String,
    /// 指纹来源标识
    pub source: HubFingerprintSource,
}

/// 全局缓存的最新 Hub 指纹，初始状态使用纯内存稳态基线，零运行时或 I/O 异常风险
static CACHED_HUB_FINGERPRINT: Lazy<RwLock<HubFingerprint>> = Lazy::new(|| {
    RwLock::new(HubFingerprint {
        version: OFFICIAL_HUB_VERSION.to_string(),
        cl: OFFICIAL_HUB_CL.to_string(),
        user_agent: build_hub_user_agent(OFFICIAL_HUB_VERSION, OFFICIAL_HUB_CL),
        source: HubFingerprintSource::Baseline,
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

/// 出站指纹固定的平台段：统一采用官方 **Windows x64** 指纹。
///
/// 官方三种平台的 Hub 指纹并非完全同构：本次（2026-10-29）实抓的是 Windows 客户端
/// `os_type=windows; arch=amd64`，不同平台的版本号也可能存在发布节奏差异。
/// 固定为实抓来源可保证版本号 + CL + 平台段三者同源，不自行拼接跨平台组合。
/// 修改前请同步校准 `OFFICIAL_HUB_VERSION` / `OFFICIAL_HUB_CL`。
const HUB_FINGERPRINT_OS: &str = "windows";
const HUB_FINGERPRINT_ARCH: &str = "amd64";

/// 构建官方标准的 Antigravity Hub User-Agent 字符串
///
/// 形如: `antigravity/hub/2.32.1 (aidev_client; os_type=windows; arch=amd64; cl=1021296516)`
pub fn build_hub_user_agent(version: &str, cl: &str) -> String {
    format!(
        "antigravity/hub/{} (aidev_client; os_type={}; arch={}; cl={})",
        version, HUB_FINGERPRINT_OS, HUB_FINGERPRINT_ARCH, cl
    )
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

/// 尝试从本地已安装的 Antigravity 客户端探测 Hub 版本
pub fn try_detect_local_hub_version() -> Option<String> {
    if let Ok(local_ver) = crate::modules::version::get_antigravity_version(None) {
        let ver = if !local_ver.short_version.is_empty() {
            local_ver.short_version
        } else {
            local_ver.bundle_version
        };
        if !ver.is_empty() {
            return Some(ver);
        }
    }
    None
}

/// 同步解析 Hub 客户端指纹（不发起任何外部网络 I/O，安全可靠）
/// 优先级: 用户手动自定义 > 本地安装探测 > 稳态基准
pub fn resolve_hub_fingerprint_sync(
    custom_version: Option<&str>,
    custom_cl: Option<&str>,
) -> HubFingerprint {
    let clean_ver = custom_version.map(|s| s.trim()).filter(|s| !s.is_empty());
    let clean_cl = custom_cl.map(|s| s.trim()).filter(|s| !s.is_empty());

    // 1. 如果用户自定义了版本与 CL
    if let (Some(v), Some(c)) = (clean_ver, clean_cl) {
        return HubFingerprint {
            version: v.to_string(),
            cl: c.to_string(),
            user_agent: build_hub_user_agent(v, c),
            source: HubFingerprintSource::Custom,
        };
    }

    let final_cl = clean_cl.unwrap_or(OFFICIAL_HUB_CL).to_string();

    // 2. 如果用户仅自定义了版本
    if let Some(v) = clean_ver {
        return HubFingerprint {
            version: v.to_string(),
            cl: final_cl.clone(),
            user_agent: build_hub_user_agent(v, &final_cl),
            source: HubFingerprintSource::Custom,
        };
    }

    // 3. 尝试本地已安装探测（仅接受 2.x Hub 版本线，IDE 4.x 不可用作 hub 指纹）
    if let Some(local_ver) =
        try_detect_local_hub_version().filter(|v| is_acceptable_local_hub_version(v))
    {
        return HubFingerprint {
            version: local_ver.clone(),
            cl: final_cl.clone(),
            user_agent: build_hub_user_agent(&local_ver, &final_cl),
            source: HubFingerprintSource::Local,
        };
    }

    // 4. 稳态基准兜底（硬编码校准值：版本与 CL 必须成对，不自动跟随远程版本）
    HubFingerprint {
        version: OFFICIAL_HUB_VERSION.to_string(),
        cl: final_cl.clone(),
        user_agent: build_hub_user_agent(OFFICIAL_HUB_VERSION, &final_cl),
        source: HubFingerprintSource::Baseline,
    }
}

/// 异步全流程解析 Hub 客户端指纹（包含官方远程源实时探测）
/// 优先级: 用户手动自定义 > 远程更新源探测 > 本地安装探测 > 稳态基准
pub async fn resolve_hub_fingerprint_async(
    custom_version: Option<&str>,
    custom_cl: Option<&str>,
) -> HubFingerprint {
    resolve_hub_fingerprint_async_with(custom_version, custom_cl, false).await
}

/// 同上，`force_remote = true` 时跳过 6h 发布缓存（UI「强制探测」）
pub async fn resolve_hub_fingerprint_async_with(
    custom_version: Option<&str>,
    custom_cl: Option<&str>,
    force_remote: bool,
) -> HubFingerprint {
    let clean_ver = custom_version.map(|s| s.trim()).filter(|s| !s.is_empty());
    let clean_cl = custom_cl.map(|s| s.trim()).filter(|s| !s.is_empty());

    // 1. 用户自定义版本与 CL
    if let (Some(v), Some(c)) = (clean_ver, clean_cl) {
        return HubFingerprint {
            version: v.to_string(),
            cl: c.to_string(),
            user_agent: build_hub_user_agent(v, c),
            source: HubFingerprintSource::Custom,
        };
    }

    let final_cl = clean_cl.unwrap_or(OFFICIAL_HUB_CL).to_string();

    // 2. 用户仅自定义了版本
    if let Some(v) = clean_ver {
        return HubFingerprint {
            version: v.to_string(),
            cl: final_cl.clone(),
            user_agent: build_hub_user_agent(v, &final_cl),
            source: HubFingerprintSource::Custom,
        };
    }

    // 3. 尝试远程动态探测 (异步非阻塞，超时 5 秒；自带 6h 缓存，force 时强制刷新)
    if let Some(remote_ver) = try_detect_remote_hub_version_with(force_remote).await {
        return HubFingerprint {
            version: remote_ver.clone(),
            cl: final_cl.clone(),
            user_agent: build_hub_user_agent(&remote_ver, &final_cl),
            source: HubFingerprintSource::Remote,
        };
    }

    // 4. 尝试本地已安装探测（仅接受 2.x Hub 版本线，IDE 4.x 不可用作 hub 指纹）
    if let Some(local_ver) =
        try_detect_local_hub_version().filter(|v| is_acceptable_local_hub_version(v))
    {
        return HubFingerprint {
            version: local_ver.clone(),
            cl: final_cl.clone(),
            user_agent: build_hub_user_agent(&local_ver, &final_cl),
            source: HubFingerprintSource::Local,
        };
    }

    // 5. 稳态基准兜底（硬编码校准值：版本与 CL 必须成对，不自动跟随远程版本）
    HubFingerprint {
        version: OFFICIAL_HUB_VERSION.to_string(),
        cl: final_cl.clone(),
        user_agent: build_hub_user_agent(OFFICIAL_HUB_VERSION, &final_cl),
        source: HubFingerprintSource::Baseline,
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
) -> HubFingerprint {
    let resolved = resolve_hub_fingerprint_sync(custom_version, custom_cl);
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
) -> HubFingerprint {
    detect_and_update_hub_fingerprint_with(custom_version, custom_cl, false).await
}

/// 同上，`force_remote = true` 时跳过发布缓存强制拉取（UI「强制探测」按钮）
pub async fn detect_and_update_hub_fingerprint_with(
    custom_version: Option<&str>,
    custom_cl: Option<&str>,
    force_remote: bool,
) -> HubFingerprint {
    let resolved =
        resolve_hub_fingerprint_async_with(custom_version, custom_cl, force_remote).await;
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
    fn test_custom_override_takes_precedence() {
        let fp = resolve_hub_fingerprint_sync(Some("3.0.0"), Some("12345678"));
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
    fn test_baseline_fallback() {
        let fp = resolve_hub_fingerprint_sync(None, None);
        // 如果本地没有安装 Antigravity，则必定回退到 baseline 或检测到 local
        assert!(!fp.version.is_empty());
        assert!(!fp.cl.is_empty());
    }
}
