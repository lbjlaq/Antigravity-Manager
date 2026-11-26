use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// 同仓库独立发布 Tag（与主分支 releases/latest 完全隔离）
pub const RELEASE_TAG: &str = "antigravity-manager";
pub const UPDATER_JSON_URL: &str =
    "https://github.com/lbjlaq/Antigravity-Manager/releases/download/antigravity-manager/updater.json";
const GITHUB_TAG_RELEASE_API: &str =
    "https://api.github.com/repos/lbjlaq/Antigravity-Manager/releases/tags/antigravity-manager";
const GITHUB_TAG_RELEASE_PAGE: &str =
    "https://github.com/lbjlaq/Antigravity-Manager/releases/tag/antigravity-manager";

/// 正式版在线更新检查结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub has_update: bool,
    pub download_url: String,
    pub release_notes: String,
    pub published_at: String,
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

/// 检查同仓库独立 Tag (`antigravity-manager`) 最新正式版
/// 优先拉取 updater.json，若尚未发布则降级查询 GitHub Release Tag API
pub async fn check_for_updates(current_version: &str) -> Result<UpdateInfo> {
    let current_clean = current_version
        .trim()
        .trim_start_matches('v')
        .trim_start_matches('V')
        .to_string();

    let client = crate::runtime::shared_http_client();
    let ua = format!("Antigravity-Manager/{}", current_clean);

    // 1. 优先读取独立 Tag 下的 updater.json
    if let Ok(res) = client
        .get(UPDATER_JSON_URL)
        .timeout(std::time::Duration::from_secs(12))
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
                });
            }
        }
    }

    // 2. 降级查询 GitHub Releases Tag API
    let res = client
        .get(GITHUB_TAG_RELEASE_API)
        .timeout(std::time::Duration::from_secs(12))
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
}
