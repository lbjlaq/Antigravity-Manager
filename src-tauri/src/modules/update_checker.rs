use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use crate::modules::logger;
use chrono::Utc;

const GITHUB_API_URL: &str = "https://api.github.com/repos/lbjlaq/Antigravity-Manager/releases/latest";
const GITHUB_RAW_URL: &str = "https://raw.githubusercontent.com/lbjlaq/Antigravity-Manager/main/package.json";
const JSDELIVR_URL: &str = "https://cdn.jsdelivr.net/gh/lbjlaq/Antigravity-Manager@main/package.json";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const DEFAULT_CHECK_INTERVAL_HOURS: u64 = 24;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub has_update: bool,
    pub download_url: String, // previously release_url
    pub release_notes: String,
    pub published_at: String,
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSettings {
    pub auto_check: bool,
    pub last_check_time: u64,
    #[serde(default = "default_check_interval")]
    pub check_interval_hours: u64,
}

fn default_check_interval() -> u64 {
    DEFAULT_CHECK_INTERVAL_HOURS
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self {
            auto_check: true,
            last_check_time: 0,
            check_interval_hours: DEFAULT_CHECK_INTERVAL_HOURS,
        }
    }
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    body: String,
    published_at: String,
}

/// Check for updates with fallback strategy
pub async fn check_for_updates() -> Result<UpdateInfo, String> {
    // 1. Try GitHub API (Preferred: has release notes, specific version mapping)
    match check_github_api().await {
        Ok(info) => return Ok(info),
        Err(e) => {
            logger::log_warn(&format!("GitHub API check failed: {}. Trying fallbacks...", e));
        }
    }

    // 2. Try GitHub Raw (Precision: avoids CDN caching issues)
    match check_static_url(GITHUB_RAW_URL, "GitHub Raw").await {
        Ok(info) => return Ok(info),
        Err(e) => {
            logger::log_warn(&format!("GitHub Raw check failed: {}. Trying next fallback...", e));
        }
    }

    // 3. Try jsDelivr (High Availability: CDN)
    match check_static_url(JSDELIVR_URL, "jsDelivr").await {
        Ok(info) => return Ok(info),
        Err(e) => {
            logger::log_error(&format!("All update checks failed. Last error: {}", e));
            return Err(e);
        }
    }
}

async fn create_client() -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder()
        .user_agent("Antigravity-Manager")
        .timeout(std::time::Duration::from_secs(10));

    // Load config to check for upstream proxy
    if let Ok(config) = crate::modules::config::load_app_config() {
        if config.proxy.upstream_proxy.enabled && !config.proxy.upstream_proxy.url.is_empty() {
            logger::log_info(&format!("Update checker using upstream proxy: {}", config.proxy.upstream_proxy.url));
            match reqwest::Proxy::all(&config.proxy.upstream_proxy.url) {
                Ok(proxy) => {
                    builder = builder.proxy(proxy);
                },
                Err(e) => {
                    logger::log_warn(&format!("Failed to parse proxy URL '{}': {}", config.proxy.upstream_proxy.url, e));
                }
            }
        }
    }

    builder.build().map_err(|e| format!("Failed to create HTTP client: {}", e))
}

async fn check_github_api() -> Result<UpdateInfo, String> {
    let client = create_client().await?;

    logger::log_info("Checking for updates via GitHub API...");

    let response = client
        .get(GITHUB_API_URL)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("GitHub API returned status: {}", response.status()));
    }

    let release: GitHubRelease = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse release info: {}", e))?;

    let latest_version = release.tag_name.trim_start_matches('v').to_string();
    let current_version = CURRENT_VERSION.to_string();
    let has_update = compare_versions(&latest_version, &current_version);

    if has_update {
        logger::log_info(&format!("New version found (API): {} (Current: {})", latest_version, current_version));
    } else {
        logger::log_info(&format!("Up to date (API): {} (Matches {})", current_version, latest_version));
    }

    Ok(UpdateInfo {
        current_version,
        latest_version,
        has_update,
        download_url: release.html_url,
        release_notes: release.body,
        published_at: release.published_at,
        source: Some("GitHub API".to_string()),
    })
}

#[derive(Deserialize)]
struct PackageJson {
    version: String,
}

async fn check_static_url(url: &str, source_name: &str) -> Result<UpdateInfo, String> {
    let client = create_client().await?;

    logger::log_info(&format!("Checking for updates via {}...", source_name));

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("{} returned status: {}", source_name, response.status()));
    }

    let package_json: PackageJson = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse package.json: {}", e))?;

    let latest_version = package_json.version;
    let current_version = CURRENT_VERSION.to_string();
    let has_update = compare_versions(&latest_version, &current_version);

    if has_update {
        logger::log_info(&format!("New version found ({}): {} (Current: {})", source_name, latest_version, current_version));
    } else {
        logger::log_info(&format!("Up to date ({}): {} (Matches {})", source_name, current_version, latest_version));
    }

    // fallback sources generally don't provide release notes or download specific URL, construct generic
    let download_url = "https://github.com/lbjlaq/Antigravity-Manager/releases/latest".to_string();
    let release_notes = format!("New version detected via {}. Please check release page for details.", source_name);

    Ok(UpdateInfo {
        current_version,
        latest_version,
        has_update,
        download_url,
        release_notes,
        published_at: Utc::now().to_rfc3339(), // Approximate time
        source: Some(source_name.to_string()),
    })
}

/// Compare two semantic versions (e.g., "3.3.30" vs "3.3.29")
fn compare_versions(latest: &str, current: &str) -> bool {
    let parse_version = |v: &str| -> Vec<u32> {
        v.split('.')
            .filter_map(|s| s.parse::<u32>().ok())
            .collect()
    };

    let latest_parts = parse_version(latest);
    let current_parts = parse_version(current);

    for i in 0..latest_parts.len().max(current_parts.len()) {
        let latest_part = latest_parts.get(i).unwrap_or(&0);
        let current_part = current_parts.get(i).unwrap_or(&0);

        if latest_part > current_part {
            return true;
        } else if latest_part < current_part {
            return false; // e.g. local: 3.3.30, remote: 3.3.30 => false
        }
    }

    false
}

/// Check if enough time has passed since last check
pub fn should_check_for_updates(settings: &UpdateSettings) -> bool {
    if !settings.auto_check {
        return false;
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let elapsed_hours = (now - settings.last_check_time) / 3600;
    let interval = if settings.check_interval_hours > 0 {
        settings.check_interval_hours
    } else {
        DEFAULT_CHECK_INTERVAL_HOURS
    };
    elapsed_hours >= interval
}

/// Load update settings from config file
pub fn load_update_settings() -> Result<UpdateSettings, String> {
    let data_dir = crate::modules::account::get_data_dir()
        .map_err(|e| format!("Failed to get data dir: {}", e))?;
    let settings_path = data_dir.join("update_settings.json");

    if !settings_path.exists() {
        return Ok(UpdateSettings::default());
    }

    let content = std::fs::read_to_string(&settings_path)
        .map_err(|e| format!("Failed to read settings file: {}", e))?;

    serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse settings: {}", e))
}

/// Save update settings to config file
pub fn save_update_settings(settings: &UpdateSettings) -> Result<(), String> {
    let data_dir = crate::modules::account::get_data_dir()
        .map_err(|e| format!("Failed to get data dir: {}", e))?;
    let settings_path = data_dir.join("update_settings.json");

    let content = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;

    std::fs::write(&settings_path, content)
        .map_err(|e| format!("Failed to write settings file: {}", e))
}

/// Update last check time
pub fn update_last_check_time() -> Result<(), String> {
    let mut settings = load_update_settings()?;
    settings.last_check_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    save_update_settings(&settings)
}

/// Detect if the app was installed via Homebrew Cask (macOS only)
pub fn is_homebrew_installed() -> bool {
    #[cfg(target_os = "macos")]
    {
        let caskroom_paths = [
            "/opt/homebrew/Caskroom/antigravity-tools",
            "/usr/local/Caskroom/antigravity-tools",
        ];

        for path in &caskroom_paths {
            if std::path::Path::new(path).exists() {
                logger::log_info(&format!("Detected Homebrew Cask installation at: {}", path));
                return true;
            }
        }
    }

    false
}

/// Execute `brew upgrade --cask antigravity-tools` with timeout (macOS only)
#[cfg(not(target_os = "macos"))]
pub async fn brew_upgrade_cask() -> Result<String, String> {
    Err("brew_not_supported".to_string())
}

#[cfg(target_os = "macos")]
pub async fn brew_upgrade_cask() -> Result<String, String> {
    logger::log_info("Starting Homebrew Cask upgrade for antigravity-tools...");

    // Find brew binary
    let brew_path = if std::path::Path::new("/opt/homebrew/bin/brew").exists() {
        "/opt/homebrew/bin/brew"
    } else if std::path::Path::new("/usr/local/bin/brew").exists() {
        "/usr/local/bin/brew"
    } else {
        return Err("brew_not_found".to_string());
    };

    // 3 min timeout to prevent hanging
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(180),
        tokio::process::Command::new(brew_path)
            .args(["upgrade", "--cask", "antigravity-tools"])
            .output()
    ).await;

    let output = match result {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => {
            logger::log_error(&format!("Failed to execute brew upgrade: {}", e));
            return Err("brew_exec_failed".to_string());
        }
        Err(_) => {
            logger::log_error("Homebrew upgrade timed out after 3 minutes");
            return Err("brew_timeout".to_string());
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if output.status.success() {
        logger::log_info(&format!("Homebrew upgrade succeeded: {}", stdout));
        Ok(stdout)
    } else {
        logger::log_error(&format!("brew upgrade failed - stdout: {} stderr: {}", stdout, stderr));
        // Return structured error key for frontend i18n
        if stderr.contains("already installed") || stdout.contains("already installed") {
            Err("brew_already_latest".to_string())
        } else {
            Err("brew_upgrade_failed".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compare_versions() {
        assert!(compare_versions("3.3.36", "3.3.35"));
        assert!(compare_versions("3.4.0", "3.3.35"));
        assert!(compare_versions("4.0.3", "3.3.35"));
        assert!(!compare_versions("3.3.34", "3.3.35"));
        assert!(!compare_versions("3.3.35", "3.3.35"));
    }

    #[test]
    fn test_should_check_for_updates() {
        let mut settings = UpdateSettings::default();
        assert!(should_check_for_updates(&settings));

        settings.last_check_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert!(!should_check_for_updates(&settings));

        settings.auto_check = false;
        assert!(!should_check_for_updates(&settings));
    }
}
