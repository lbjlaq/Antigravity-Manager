/// 官方标准 Antigravity Hub 出站指纹常量与动态指纹检测器
/// 对齐原生 /Applications/Antigravity.app
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const OFFICIAL_HUB_VERSION: &str = "2.33.0";
pub const OFFICIAL_HUB_CL: &str = "1023160026";
pub const HUB_FINGERPRINT_OS: &str = "windows";
pub const HUB_FINGERPRINT_ARCH: &str = "amd64";

pub const HUB_RELEASES_URL: &str =
    "https://antigravity-hub-auto-updater-974169037036.us-central1.run.app/releases";

/// 指纹来源
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FingerprintSource {
    /// 本地已安装原生客户端动态提取
    Local,
    /// 稳态基准兜底
    Baseline,
}

impl std::fmt::Display for FingerprintSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Local => write!(f, "local"),
            Self::Baseline => write!(f, "baseline"),
        }
    }
}

/// 运行时解析出的 Hub 指纹信息
#[derive(Debug, Clone)]
pub struct HubFingerprint {
    pub version: String,
    pub cl: String,
    pub user_agent: String,
    pub source: FingerprintSource,
    pub local_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RemoteRelease {
    pub version: String,
    #[serde(default)]
    pub execution_id: Option<String>,
}

static DETECTED_FINGERPRINT: OnceLock<HubFingerprint> = OnceLock::new();

/// 构建标准的 Antigravity Hub User-Agent 字符串
pub fn build_hub_user_agent(version: &str, cl: &str) -> String {
    format!(
        "antigravity/hub/{} (aidev_client; os_type={}; arch={}; cl={})",
        version, HUB_FINGERPRINT_OS, HUB_FINGERPRINT_ARCH, cl
    )
}

/// 尝试从本地已安装的 Antigravity.app 提取动态指纹
/// 必须 (version, cl) 两者严格成对提取成功才使用，否则优雅回退基准
pub fn detect_dynamic_fingerprint() -> HubFingerprint {
    if let Some((ver, cl, path)) = try_detect_local() {
        let ua = build_hub_user_agent(&ver, &cl);
        HubFingerprint {
            version: ver,
            cl,
            user_agent: ua,
            source: FingerprintSource::Local,
            local_path: Some(path),
        }
    } else {
        HubFingerprint {
            version: OFFICIAL_HUB_VERSION.to_string(),
            cl: OFFICIAL_HUB_CL.to_string(),
            user_agent: build_hub_user_agent(OFFICIAL_HUB_VERSION, OFFICIAL_HUB_CL),
            source: FingerprintSource::Baseline,
            local_path: None,
        }
    }
}

/// 获取当前激活的 Hub 指纹（进程生命周期内缓存单例）
pub fn get_active_fingerprint() -> &'static HubFingerprint {
    DETECTED_FINGERPRINT.get_or_init(detect_dynamic_fingerprint)
}

/// 生成当前出站 User-Agent
pub fn get_official_user_agent() -> String {
    get_active_fingerprint().user_agent.clone()
}

/// 异步探测官方云端最新 Hub 发布信息 (不影响主流程)
pub async fn check_remote_hub_release() -> Option<RemoteRelease> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(4))
        .build()
        .ok()?;
    let resp = client.get(HUB_RELEASES_URL).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let list: Vec<RemoteRelease> = resp.json().await.ok()?;
    list.into_iter().find(|r| is_acceptable_hub_version(&r.version))
}

/// 检查版本号是否为合法的 2.x Hub 版本线
pub fn is_acceptable_hub_version(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    parts.len() == 3
        && parts[0] == "2"
        && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

/// 探测本地已安装的语言服务与版本
fn try_detect_local() -> Option<(String, String, PathBuf)> {
    let mut candidate_dirs = Vec::new();

    #[cfg(target_os = "macos")]
    {
        candidate_dirs.push(PathBuf::from("/Applications/Antigravity.app"));
        if let Some(home) = dirs::home_dir() {
            candidate_dirs.push(home.join("Applications/Antigravity.app"));
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(local_app_data) = dirs::data_local_dir() {
            candidate_dirs.push(local_app_data.join("Programs/Antigravity"));
            candidate_dirs.push(local_app_data.join("Antigravity"));
        }
        candidate_dirs.push(PathBuf::from(r"C:\Program Files\Antigravity"));
    }

    #[cfg(target_os = "linux")]
    {
        candidate_dirs.push(PathBuf::from("/opt/Antigravity"));
        candidate_dirs.push(PathBuf::from("/usr/share/antigravity"));
        if let Some(home) = dirs::home_dir() {
            candidate_dirs.push(home.join(".local/share/antigravity"));
        }
    }

    for app_dir in candidate_dirs {
        if !app_dir.exists() {
            continue;
        }

        let version = detect_version_from_dir(&app_dir);
        let version = match version {
            Some(v) if is_acceptable_hub_version(&v) => v,
            _ => continue,
        };

        let ls_candidates = [
            app_dir.join("Contents/Resources/bin/language_server"),
            app_dir.join("Contents/Resources/bin/language_server.exe"),
            app_dir.join("resources/bin/language_server.exe"),
            app_dir.join("resources/bin/language_server"),
            app_dir.join("bin/language_server"),
            app_dir.join("bin/language_server.exe"),
        ];

        for ls_path in ls_candidates {
            if ls_path.is_file() {
                if let Some(cl) = extract_cl_from_binary(&ls_path) {
                    return Some((version, cl, ls_path));
                }
            }
        }
    }

    None
}

/// 从应用安装目录获取版本号
fn detect_version_from_dir(app_dir: &Path) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let plist_path = app_dir.join("Contents/Info.plist");
        if plist_path.is_file() {
            if let Ok(content) = std::fs::read_to_string(&plist_path) {
                if let Some(idx) = content.find("CFBundleShortVersionString") {
                    let rest = &content[idx..];
                    if let Some(s_idx) = rest.find("<string>") {
                        let after_s = &rest[s_idx + 8..];
                        if let Some(e_idx) = after_s.find("</string>") {
                            let ver = after_s[..e_idx].trim().to_string();
                            return Some(ver);
                        }
                    }
                }
            }
        }
    }

    let pkg_json = app_dir.join("resources/app/package.json");
    if pkg_json.is_file() {
        if let Ok(content) = std::fs::read_to_string(pkg_json) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(v) = val.get("version").and_then(|v| v.as_str()) {
                    return Some(v.to_string());
                }
            }
        }
    }

    None
}

/// 流式扫描二进制文件提取 Google Blaze/Piper CL 变更号
pub fn extract_cl_from_binary(path: &Path) -> Option<String> {
    use std::io::Read;

    let mut file = std::fs::File::open(path).ok()?;
    let chunk_size = 4 * 1024 * 1024; // 4MB
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
