use std::sync::LazyLock;
use regex::Regex;

/// URL to fetch the latest Antigravity version
const VERSION_URL: &str = "https://antigravity-auto-updater-974169037036.us-central1.run.app";

/// Second fallback: Official Changelog page
const CHANGELOG_URL: &str = "https://antigravity.google/changelog";

/// Fallback version derived from Cargo.toml at compile time
const FALLBACK_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Known stable configuration (for Docker/Headless fallback)
/// Antigravity 1.16.5 uses Electron 39.2.3 which corresponds to Chrome 132.0.6834.160
const KNOWN_STABLE_VERSION: &str = "1.16.5";
const KNOWN_STABLE_ELECTRON: &str = "39.2.3";
const KNOWN_STABLE_CHROME: &str = "132.0.6834.160";

/// Pre-compiled regex for version parsing (X.Y.Z pattern)
static VERSION_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\d+\.\d+\.\d+").expect("Invalid version regex")
});

/// Parse version from response text using pre-compiled regex
/// Matches semver pattern: X.Y.Z (e.g., "1.15.8")
fn parse_version(text: &str) -> Option<String> {
    VERSION_REGEX.find(text).map(|m| m.as_str().to_string())
}

/// Version source for logging
#[derive(Debug, PartialEq)]
enum VersionSource {
    LocalInstallation,
    KnownStableFallback,
    RemoteAPI,
    ChangelogWeb,
    CargoToml,
}

/// Helper struct for version info
struct VersionConfig {
    version: String,
    electron: String,
    chrome: String,
}

/// Fetch version strategy: Local > Known Stable (Docker) > Remote
fn resolve_version_config() -> (VersionConfig, VersionSource) {
    // 1. Try Local Installation (Preferred)
    if let Ok(local_ver) = crate::modules::version::get_antigravity_version() {
        let resolved_version = parse_version(&local_ver.short_version)
            .or_else(|| parse_version(&local_ver.bundle_version))
            .unwrap_or_else(|| {
                tracing::warn!(
                    raw_short = %local_ver.short_version,
                    raw_bundle = %local_ver.bundle_version,
                    fallback = KNOWN_STABLE_VERSION,
                    "Unable to parse semver from local installation version output; using known stable fallback"
                );
                KNOWN_STABLE_VERSION.to_string()
            });

        // Map local version to Electron/Chrome if possible
        // For now, if local version is >= 1.16.5, we assume it's using the new Electron 39 stack
        // Ideally we would maintain a map, but for now we default to the KNOWN_STABLE stack
        // if the version matches or is newer.
        // If older, we might want to fallback to older values, but using new values is generally safer for "updates".
        return (
            VersionConfig {
                version: resolved_version,
                electron: KNOWN_STABLE_ELECTRON.to_string(),
                chrome: KNOWN_STABLE_CHROME.to_string(),
            },
            VersionSource::LocalInstallation,
        );
    }

    // 2. Fallback to Known Stable (Docker / Headless)
    // This provides a valid fingerprint even without the App installed
    (
        VersionConfig {
            version: KNOWN_STABLE_VERSION.to_string(),
            electron: KNOWN_STABLE_ELECTRON.to_string(),
            chrome: KNOWN_STABLE_CHROME.to_string(),
        },
        VersionSource::KnownStableFallback,
    )
}

/// Current resolved Antigravity version (e.g., "1.16.5")
pub static CURRENT_VERSION: LazyLock<String> = LazyLock::new(|| {
    let (config, _) = resolve_version_config();
    config.version
});

/// Global Session ID (generated once per app launch)
pub static SESSION_ID: LazyLock<String> = LazyLock::new(|| {
    uuid::Uuid::new_v4().to_string()
});

/// Shared User-Agent string for all upstream API requests.
/// Format matches official Electron client:
/// "Antigravity/1.16.5 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Antigravity/1.16.5 Chrome/132.0.6834.160 Electron/39.2.3 Safari/537.36"
pub static USER_AGENT: LazyLock<String> = LazyLock::new(|| {
    let (config, source) = resolve_version_config();

    tracing::info!(
        version = %config.version,
        source = ?source,
        "User-Agent initialized"
    );

    // Platform mapping
    let platform_info = match std::env::consts::OS {
        "macos" => "Macintosh; Intel Mac OS X 10_15_7",
        "windows" => "Windows NT 10.0; Win64; x64",
        "linux" => "X11; Linux x86_64",
        _ => "X11; Linux x86_64", // Default to Linux-like
    };

    format!(
        "Mozilla/5.0 ({}) AppleWebKit/537.36 (KHTML, like Gecko) Antigravity/{} Chrome/{} Electron/{} Safari/537.36",
        platform_info,
        config.version,
        config.chrome,
        config.electron
    )
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_version_from_updater_response() {
        let text = "Auto updater is running. Stable Version: 1.15.8-5724687216017408";
        assert_eq!(parse_version(text), Some("1.15.8".to_string()));
    }

    #[test]
    fn test_parse_version_simple() {
        assert_eq!(parse_version("1.15.8"), Some("1.15.8".to_string()));
        assert_eq!(parse_version("Version: 2.0.0"), Some("2.0.0".to_string()));
        assert_eq!(parse_version("v1.2.3"), Some("1.2.3".to_string()));
    }

    #[test]
    fn test_parse_version_invalid() {
        assert_eq!(parse_version("no version here"), None);
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("1.2"), None); // Only X.Y, not X.Y.Z
    }

    #[test]
    fn test_parse_version_with_suffix() {
        // Regex only matches X.Y.Z, suffix is naturally excluded
        let text = "antigravity/1.15.8 windows/amd64";
        assert_eq!(parse_version(text), Some("1.15.8".to_string()));
    }
}
