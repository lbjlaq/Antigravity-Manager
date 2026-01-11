//! Platform detection utilities for Antigravity compliance
//!
//! This module provides compile-time platform and architecture detection
//! to generate correct User-Agent strings for anti-detection compliance.
//!
//! # Antigravity v1.13.3 Compliance
//! - User-Agent format: `antigravity/{version} {platform}/{arch}`
//! - Platform: darwin (macOS) | windows | linux
//! - Architecture: arm64 (aarch64) | x86_64

/// Antigravity version for User-Agent and metadata
///
/// Must match the version expected by Google's Antigravity service
pub const ANTIGRAVITY_VERSION: &str = "1.13.3";

/// Detect current operating system platform using compile-time cfg macros
///
/// Returns platform identifier matching Antigravity v1.13.3 expectations:
/// - `"darwin"` for macOS
/// - `"windows"` for Windows
/// - `"linux"` for Linux and other Unix-like systems (default)
///
/// # Examples
/// ```
/// let platform = get_platform();
/// assert!(platform == "darwin" || platform == "windows" || platform == "linux");
/// ```
#[inline]
pub fn get_platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        // Default to linux for other Unix-like systems
        "linux"
    }
}

/// Detect CPU architecture using compile-time cfg macros
///
/// Returns architecture identifier matching Antigravity v1.13.3 expectations:
/// - `"arm64"` for ARM64/aarch64 (Apple Silicon, ARM servers)
/// - `"x86_64"` for x86-64 (Intel/AMD, default)
///
/// # Examples
/// ```
/// let arch = get_architecture();
/// assert!(arch == "arm64" || arch == "x86_64");
/// ```
#[inline]
pub fn get_architecture() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else {
        // Default to x86_64 for unknown architectures
        "x86_64"
    }
}

/// Build User-Agent string for Antigravity v1.13.3 API compliance
///
/// Generates User-Agent in the format: `antigravity/{version} {platform}/{arch}`
///
/// This is critical for anti-detection compliance. The User-Agent must match
/// the actual platform/architecture of the running system to avoid detection
/// by Google's security systems.
///
/// # Returns
/// Formatted User-Agent string, e.g.:
/// - `"antigravity/1.13.3 darwin/arm64"` (macOS on Apple Silicon)
/// - `"antigravity/1.13.3 windows/x86_64"` (Windows on Intel/AMD)
/// - `"antigravity/1.13.3 linux/x86_64"` (Linux on Intel/AMD)
///
/// # Examples
/// ```
/// let ua = build_user_agent();
/// assert!(ua.starts_with("antigravity/1.13.3 "));
/// ```
pub fn build_user_agent() -> String {
    let platform = get_platform();
    let arch = get_architecture();

    let ua = format!("antigravity/{} {}/{}", ANTIGRAVITY_VERSION, platform, arch);

    tracing::debug!(
        "[Platform-Detection] User-Agent: '{}' (platform: {}, arch: {})",
        ua,
        platform,
        arch
    );

    ua
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_platform_returns_valid_value() {
        let platform = get_platform();
        assert!(
            platform == "darwin" || platform == "windows" || platform == "linux",
            "Platform must be one of: darwin, windows, linux. Got: {}",
            platform
        );
    }

    #[test]
    fn test_get_architecture_returns_valid_value() {
        let arch = get_architecture();
        assert!(
            arch == "arm64" || arch == "x86_64",
            "Architecture must be one of: arm64, x86_64. Got: {}",
            arch
        );
    }

    #[test]
    fn test_build_user_agent_format() {
        let ua = build_user_agent();

        // Must start with version
        assert!(
            ua.starts_with("antigravity/1.13.3 "),
            "User-Agent must start with 'antigravity/1.13.3 '. Got: {}",
            ua
        );

        // Must contain valid platform
        assert!(
            ua.contains("darwin/") || ua.contains("windows/") || ua.contains("linux/"),
            "User-Agent must contain valid platform. Got: {}",
            ua
        );

        // Must end with valid architecture
        assert!(
            ua.ends_with("/arm64") || ua.ends_with("/x86_64"),
            "User-Agent must end with valid architecture. Got: {}",
            ua
        );
    }

    #[test]
    fn test_build_user_agent_no_extra_spaces() {
        let ua = build_user_agent();

        // Should only have one space (between version and platform/arch)
        let space_count = ua.matches(' ').count();
        assert_eq!(
            space_count, 1,
            "User-Agent should have exactly 1 space. Got {} spaces in: {}",
            space_count, ua
        );
    }

    #[test]
    fn test_build_user_agent_deterministic() {
        // Calling multiple times should return same result
        let ua1 = build_user_agent();
        let ua2 = build_user_agent();
        let ua3 = build_user_agent();

        assert_eq!(ua1, ua2, "User-Agent should be deterministic");
        assert_eq!(ua2, ua3, "User-Agent should be deterministic");
    }

    #[test]
    fn test_antigravity_version_constant() {
        assert_eq!(
            ANTIGRAVITY_VERSION, "1.13.3",
            "ANTIGRAVITY_VERSION must be '1.13.3' for compliance"
        );
    }

    #[test]
    fn test_platform_architecture_combinations_valid() {
        let platform = get_platform();
        let arch = get_architecture();

        // All platform/arch combinations should be valid for Antigravity
        match (platform, arch) {
            ("darwin", "arm64") => {}   // macOS on Apple Silicon
            ("darwin", "x86_64") => {}  // macOS on Intel
            ("windows", "x86_64") => {} // Windows on Intel/AMD
            ("windows", "arm64") => {}  // Windows on ARM
            ("linux", "x86_64") => {}   // Linux on Intel/AMD
            ("linux", "arm64") => {}    // Linux on ARM
            _ => panic!("Invalid platform/arch combination: {}/{}", platform, arch),
        }
    }
}
