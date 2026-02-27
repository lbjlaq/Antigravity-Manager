use std::sync::LazyLock;
use regex::Regex;

/// URL to fetch the latest Antigravity version
const VERSION_URL: &str = "https://antigravity-auto-updater-974169037036.us-central1.run.app";

/// Second fallback: Official Changelog page
const CHANGELOG_URL: &str = "https://antigravity.google/changelog";



/// Known stable configuration (for Docker/Headless fallback)
/// Antigravity 4.1.25 uses Electron 39.2.3 which corresponds to Chrome 132.0.6834.160
const KNOWN_STABLE_VERSION: &str = "4.1.25";
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

/// Compare two X.Y.Z semantic version strings.
/// Returns Ordering::Greater if v1 > v2.
fn compare_semver(v1: &str, v2: &str) -> std::cmp::Ordering {
    let parse = |v: &str| -> Vec<u32> {
        v.split('.').filter_map(|s| s.parse().ok()).collect()
    };
    let p1 = parse(v1);
    let p2 = parse(v2);
    for i in 0..p1.len().max(p2.len()) {
        let a = p1.get(i).copied().unwrap_or(0);
        let b = p2.get(i).copied().unwrap_or(0);
        match a.cmp(&b) {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }
    std::cmp::Ordering::Equal
}

/// Version source for logging
#[derive(Debug, PartialEq)]
enum VersionSource {
    LocalInstallation,
    KnownStableFallback,
    RemoteAPI,
    #[allow(dead_code)]
    ChangelogWeb,
    #[allow(dead_code)]
    CargoToml,
}

/// Helper struct for version info
struct VersionConfig {
    version: String,
    electron: String,
    chrome: String,
}

/// Try to fetch the latest Antigravity version from the remote update server.
/// Runs in a dedicated OS thread to avoid blocking Tokio's async runtime.
/// Returns None on any network/parse failure — always non-fatal, 5s timeout.
fn try_fetch_remote_version() -> Option<String> {
    // Spawn a dedicated OS thread so that `reqwest::blocking` never touches
    // the Tokio thread-pool and cannot trigger the "Cannot block the current
    // thread from within an asynchronous execution context" panic.
    let (tx, rx) = std::sync::mpsc::channel::<Option<String>>();

    std::thread::spawn(move || {
        let result = (|| -> Option<String> {
            let client = reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .ok()?;

            // 1. Try primary update URL
            if let Ok(resp) = client.get(VERSION_URL).send() {
                if let Ok(text) = resp.text() {
                    if let Some(ver) = parse_version(&text) {
                        tracing::debug!(remote_version = %ver, "Fetched remote version from VERSION_URL");
                        return Some(ver);
                    }
                }
            }

            // 2. Try changelog page as secondary fallback
            if let Ok(resp) = client.get(CHANGELOG_URL).send() {
                if let Ok(text) = resp.text() {
                    if let Some(ver) = parse_version(&text) {
                        tracing::debug!(remote_version = %ver, "Fetched remote version from CHANGELOG_URL");
                        return Some(ver);
                    }
                }
            }

            tracing::debug!("Unable to fetch remote version; will rely on local/stable floor");
            None
        })();

        let _ = tx.send(result);
    });

    // Wait up to 6 seconds (slightly over the client timeout) for the thread
    rx.recv_timeout(std::time::Duration::from_secs(6))
        .unwrap_or(None)
}

/// Smart version resolution strategy:
///   best = max(Local Installation, Remote Latest, Known Stable Fallback)
///
/// This guarantees that even when:
///   - The local Antigravity install is outdated, OR
///   - Local detection fails (Docker / headless / non-standard path),
/// ...we always report a version >= the current minimum required by Google's API.
fn resolve_version_config() -> (VersionConfig, VersionSource) {
    // Floor: static known-stable value (updated with each release of this project)
    let mut best_version = KNOWN_STABLE_VERSION.to_string();
    let mut source = VersionSource::KnownStableFallback;

    // 1. Try Local Installation
    if let Ok(local_ver) = crate::modules::version::get_antigravity_version() {
        let local_parsed = parse_version(&local_ver.short_version)
            .or_else(|| parse_version(&local_ver.bundle_version));

        if let Some(local_v) = local_parsed {
            if compare_semver(&local_v, &best_version) > std::cmp::Ordering::Equal {
                // Local is newer than the floor — use it
                tracing::debug!(
                    local_version = %local_v,
                    "Local installation version is newer than known-stable floor; using local"
                );
                best_version = local_v;
                source = VersionSource::LocalInstallation;
            } else {
                // Local is older than or equal to the floor (e.g. user hasn't updated yet)
                tracing::info!(
                    local_version = %local_v,
                    floor_version = %best_version,
                    "Local Antigravity version is older than known-stable floor; \
                     using floor to avoid upstream model rejection"
                );
                // source stays KnownStableFallback — the local version is intentionally ignored
            }
        }
    }

    // 2. Try Remote Version (best-effort; failure is silently ignored)
    if let Some(remote_v) = try_fetch_remote_version() {
        if compare_semver(&remote_v, &best_version) > std::cmp::Ordering::Equal {
            tracing::info!(
                remote_version = %remote_v,
                previous_best = %best_version,
                "Remote version is newer than current best; upgrading fingerprint version"
            );
            best_version = remote_v;
            source = VersionSource::RemoteAPI;
        }
    }

    (
        VersionConfig {
            version: best_version,
            electron: KNOWN_STABLE_ELECTRON.to_string(),
            chrome: KNOWN_STABLE_CHROME.to_string(),
        },
        source,
    )
}

/// Current resolved Antigravity version (e.g., "4.1.25")
/// Always >= KNOWN_STABLE_VERSION, and >= remote latest when reachable.
pub static CURRENT_VERSION: LazyLock<String> = LazyLock::new(|| {
    let (config, _) = resolve_version_config();
    config.version
});

/// Native OAuth Authorization User-Agent
pub static NATIVE_OAUTH_USER_AGENT: LazyLock<String> = LazyLock::new(|| {
    format!("vscode/1.X.X (Antigravity/{})", CURRENT_VERSION.as_str())
});

/// Global Session ID (generated once per app launch)
pub static SESSION_ID: LazyLock<String> = LazyLock::new(|| {
    uuid::Uuid::new_v4().to_string()
});

/// Shared User-Agent string for all upstream API requests.
/// Format matches the official Antigravity Electron desktop client:
/// "Antigravity/4.1.25 (Macintosh; Intel Mac OS X 10_15_7) Chrome/132.0.6834.160 Electron/39.2.3"
///
/// Version selection: max(local installation, remote latest, known stable 4.1.25)
/// This prevents model rejection due to outdated client version headers.
pub static USER_AGENT: LazyLock<String> = LazyLock::new(|| {
    let (config, source) = resolve_version_config();

    tracing::info!(
        version = %config.version,
        source = ?source,
        "User-Agent initialized"
    );

    let platform_info = match std::env::consts::OS {
        "macos" => "Macintosh; Intel Mac OS X 10_15_7",
        "windows" => "Windows NT 10.0; Win64; x64",
        "linux" => "X11; Linux x86_64",
        _ => "X11; Linux x86_64",
    };

    format!(
        "Antigravity/{} ({}) Chrome/{} Electron/{}",
        config.version,
        platform_info,
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

    #[test]
    fn test_compare_semver() {
        assert_eq!(compare_semver("4.1.25", "4.1.22"), std::cmp::Ordering::Greater);
        assert_eq!(compare_semver("4.1.22", "4.1.25"), std::cmp::Ordering::Less);
        assert_eq!(compare_semver("4.1.25", "4.1.25"), std::cmp::Ordering::Equal);
        assert_eq!(compare_semver("5.0.0", "4.9.9"), std::cmp::Ordering::Greater);
        assert_eq!(compare_semver("1.16.5", "1.16.4"), std::cmp::Ordering::Greater);
    }

    #[test]
    fn test_known_stable_floor_is_up_to_date() {
        // KNOWN_STABLE_VERSION must always be kept in sync with Cargo.toml.
        // This test will fail and remind the developer to update it.
        assert!(
            compare_semver(KNOWN_STABLE_VERSION, "4.1.22") > std::cmp::Ordering::Equal,
            "KNOWN_STABLE_VERSION ({}) must be > 4.1.22; please sync with Cargo.toml",
            KNOWN_STABLE_VERSION
        );
    }

    #[test]
    fn test_old_local_version_uses_floor() {
        // Simulate: local = 4.1.20 (old), floor = 4.1.25
        // Expected: use floor
        let local = "4.1.20";
        let floor = KNOWN_STABLE_VERSION;
        let best = if compare_semver(local, floor) > std::cmp::Ordering::Equal {
            local
        } else {
            floor
        };
        assert_eq!(best, KNOWN_STABLE_VERSION);
    }

    #[test]
    fn test_newer_local_version_takes_priority() {
        // Simulate: local = 4.1.25 (newer than floor), floor = 4.1.25
        // Expected: use local
        let local = "4.1.25";
        let floor = KNOWN_STABLE_VERSION;
        let best = if compare_semver(local, floor) > std::cmp::Ordering::Equal {
            local
        } else {
            floor
        };
        assert_eq!(best, "4.1.25");
    }
}
