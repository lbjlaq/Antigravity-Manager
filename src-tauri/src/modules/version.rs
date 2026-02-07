use crate::modules::process;
use std::fs;
use std::path::PathBuf;

/// Antigravity version information
#[derive(Debug, Clone)]
pub struct AntigravityVersion {
    pub short_version: String,
    pub bundle_version: String,
}

/// Detect Antigravity version (cross-platform)
pub fn get_antigravity_version() -> Result<AntigravityVersion, String> {
    // 1. Get Antigravity executable path (reuse existing functionality)
    let exe_path = process::get_antigravity_executable_path()
        .ok_or("Unable to locate Antigravity executable")?;
    
    // 2. Read version info based on platform
    #[cfg(target_os = "macos")]
    {
        get_version_macos(&exe_path)
    }
    
    #[cfg(target_os = "windows")]
    {
        get_version_windows(&exe_path)
    }
    
    #[cfg(target_os = "linux")]
    {
        get_version_linux(&exe_path)
    }
}

/// macOS: Read version from Info.plist
#[cfg(target_os = "macos")]
fn get_version_macos(exe_path: &PathBuf) -> Result<AntigravityVersion, String> {
    use plist::Value;
    
    // exe_path might be /Applications/Antigravity.app or internal executable
    // Need to find the .app directory
    let path_str = exe_path.to_string_lossy();
    let app_path = if let Some(idx) = path_str.find(".app") {
        PathBuf::from(&path_str[..idx + 4])
    } else {
        exe_path.clone()
    };
    
    let info_plist_path = app_path.join("Contents/Info.plist");
    if !info_plist_path.exists() {
        return Err(format!("Info.plist not found: {:?}", info_plist_path));
    }
    
    let content = fs::read(&info_plist_path)
        .map_err(|e| format!("Failed to read Info.plist: {}", e))?;
    
    let plist: Value = plist::from_bytes(&content)
        .map_err(|e| format!("Failed to parse Info.plist: {}", e))?;
    
    let dict = plist.as_dictionary()
        .ok_or("Info.plist is not a dictionary")?;
    
    let short_version = dict.get("CFBundleShortVersionString")
        .and_then(|v| v.as_string())
        .ok_or("CFBundleShortVersionString not found")?;
    
    let bundle_version = dict.get("CFBundleVersion")
        .and_then(|v| v.as_string())
        .unwrap_or(short_version);
    
    Ok(AntigravityVersion {
        short_version: short_version.to_string(),
        bundle_version: bundle_version.to_string(),
    })
}

/// Windows: Read version from executable metadata
#[cfg(target_os = "windows")]
fn get_version_windows(exe_path: &PathBuf) -> Result<AntigravityVersion, String> {
    use std::process::Command;
    
    // Windows: Use PowerShell to read file version info
    let output = Command::new("powershell")
        .args([
            "-Command",
            &format!(
                "(Get-Item '{}').VersionInfo.FileVersion",
                exe_path.display()
            ),
        ])
        .output()
        .map_err(|e| format!("Failed to execute PowerShell: {}", e))?;
    
    if !output.status.success() {
        return Err("Failed to read version from executable".to_string());
    }
    
    let version = String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_string();
    
    if version.is_empty() {
        return Err("Version information not found in executable".to_string());
    }
    
    Ok(AntigravityVersion {
        short_version: version.clone(),
        bundle_version: version,
    })
}

/// Linux: Read from package.json or --version argument
#[cfg(target_os = "linux")]
fn get_version_linux(exe_path: &PathBuf) -> Result<AntigravityVersion, String> {
    use std::process::Command;
    
    // Method 1: Try executing --version
    let output = Command::new(exe_path)
        .arg("--version")
        .output();
    
    if let Ok(result) = output {
        if result.status.success() {
            let version = String::from_utf8_lossy(&result.stdout)
                .trim()
                .to_string();
            if !version.is_empty() {
                return Ok(AntigravityVersion {
                    short_version: version.clone(),
                    bundle_version: version,
                });
            }
        }
    }
    
    // Method 2: Try reading from installation directory's package.json
    if let Some(parent) = exe_path.parent() {
        let package_json = parent.join("resources/app/package.json");
        if package_json.exists() {
            if let Ok(content) = fs::read_to_string(&package_json) {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(version) = json.get("version").and_then(|v| v.as_str()) {
                        return Ok(AntigravityVersion {
                            short_version: version.to_string(),
                            bundle_version: version.to_string(),
                        });
                    }
                }
            }
        }
    }
    
    Err("Unable to determine Antigravity version on Linux".to_string())
}

/// Check if version is new format (>= 1.16.5)
pub fn is_new_version(version: &AntigravityVersion) -> bool {
    compare_version(&version.short_version, "1.16.5") >= std::cmp::Ordering::Equal
}

/// Compare version numbers
fn compare_version(v1: &str, v2: &str) -> std::cmp::Ordering {
    let parts1: Vec<u32> = v1
        .split('.')
        .filter_map(|s| s.parse().ok())
        .collect();
    let parts2: Vec<u32> = v2
        .split('.')
        .filter_map(|s| s.parse().ok())
        .collect();
    
    for i in 0..parts1.len().max(parts2.len()) {
        let p1 = parts1.get(i).unwrap_or(&0);
        let p2 = parts2.get(i).unwrap_or(&0);
        match p1.cmp(p2) {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }
    std::cmp::Ordering::Equal
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_comparison() {
        assert_eq!(compare_version("1.16.5", "1.16.4"), std::cmp::Ordering::Greater);
        assert_eq!(compare_version("1.16.5", "1.16.5"), std::cmp::Ordering::Equal);
        assert_eq!(compare_version("1.16.4", "1.16.5"), std::cmp::Ordering::Less);
        assert_eq!(compare_version("1.17.0", "1.16.5"), std::cmp::Ordering::Greater);
        assert_eq!(compare_version("2.0.0", "1.16.5"), std::cmp::Ordering::Greater);
    }

    #[test]
    fn test_is_new_version() {
        let old = AntigravityVersion {
            short_version: "1.16.4".to_string(),
            bundle_version: "1.16.4".to_string(),
        };
        assert!(!is_new_version(&old));

        let new = AntigravityVersion {
            short_version: "1.16.5".to_string(),
            bundle_version: "1.16.5".to_string(),
        };
        assert!(is_new_version(&new));
        
        let newer = AntigravityVersion {
            short_version: "1.17.0".to_string(),
            bundle_version: "1.17.0".to_string(),
        };
        assert!(is_new_version(&newer));
    }
}
