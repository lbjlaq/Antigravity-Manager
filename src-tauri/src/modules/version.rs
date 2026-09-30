use crate::modules::process;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::fs;
use std::path::PathBuf;

/// Antigravity 版本信息
#[derive(Debug, Clone)]
pub struct AntigravityVersion {
    pub short_version: String,
    #[allow(dead_code)] // 预留给构建/诊断输出
    pub bundle_version: String,
}

/// 从任意字符串中提取第一个语义化版本号 (X.Y.Z)
fn extract_semver(raw: &str) -> Option<String> {
    for token in raw.split(|c: char| c.is_whitespace() || c == ',' || c == ';') {
        let t = token.trim_matches(|c: char| c == '"' || c == '\'' || c == '(' || c == ')');
        if t.is_empty() {
            continue;
        }
        let mut parts = t.split('.');
        let p1 = parts.next();
        let p2 = parts.next();
        let p3 = parts.next();
        if p1.is_some()
            && p2.is_some()
            && p3.is_some()
            && [p1.unwrap(), p2.unwrap(), p3.unwrap()]
                .iter()
                .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
        {
            return Some(t.to_string());
        }
    }
    None
}

/// 检测 Antigravity 版本（跨平台，支持预快照路径优先）
pub fn get_antigravity_version_with_path(
    target_ide: Option<&str>,
    preferred_path: Option<&std::path::Path>,
) -> Result<AntigravityVersion, String> {
    // 1. 优先使用预捕获路径，若无则探查 Antigravity 可执行文件路径
    let exe_path = preferred_path
        .map(|p| p.to_path_buf())
        .or_else(|| process::get_antigravity_executable_path(target_ide))
        .ok_or("Unable to locate Antigravity executable")?;

    // 2. 根据平台读取版本信息
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

/// 检测 Antigravity 版本（跨平台）
pub fn get_antigravity_version(target_ide: Option<&str>) -> Result<AntigravityVersion, String> {
    get_antigravity_version_with_path(target_ide, None)
}

/// macOS: 从 Info.plist 读取版本
#[cfg(target_os = "macos")]
fn get_version_macos(exe_path: &PathBuf) -> Result<AntigravityVersion, String> {
    use plist::Value;

    // exe_path 可能是 /Applications/Antigravity.app 或内部可执行文件
    // 需要找到 .app 目录
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

    let content =
        fs::read(&info_plist_path).map_err(|e| format!("Failed to read Info.plist: {}", e))?;

    let plist: Value =
        plist::from_bytes(&content).map_err(|e| format!("Failed to parse Info.plist: {}", e))?;

    let dict = plist
        .as_dictionary()
        .ok_or("Info.plist is not a dictionary")?;

    let short_version = dict
        .get("CFBundleShortVersionString")
        .and_then(|v| v.as_string())
        .ok_or("CFBundleShortVersionString not found")?;

    let bundle_version = dict
        .get("CFBundleVersion")
        .and_then(|v| v.as_string())
        .unwrap_or(short_version);

    Ok(AntigravityVersion {
        short_version: short_version.to_string(),
        bundle_version: bundle_version.to_string(),
    })
}

/// Windows: 从可执行文件元数据读取版本（纯原生 Win32 API，不调用 powershell）
#[cfg(target_os = "windows")]
fn get_version_windows(exe_path: &PathBuf) -> Result<AntigravityVersion, String> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "version")]
    extern "system" {
        fn GetFileVersionInfoSizeW(lptstrFilename: *const u16, lpdwHandle: *mut u32) -> u32;
        fn GetFileVersionInfoW(
            lptstrFilename: *const u16,
            dwHandle: u32,
            dwLen: u32,
            lpData: *mut std::ffi::c_void,
        ) -> i32;
        fn VerQueryValueW(
            pBlock: *const std::ffi::c_void,
            lpSubBlock: *const u16,
            lplpBuffer: *mut *mut std::ffi::c_void,
            puLen: *mut u32,
        ) -> i32;
    }

    let mut path_u16: Vec<u16> = exe_path.as_os_str().encode_wide().collect();
    path_u16.push(0);

    let mut dummy: u32 = 0;
    let size = unsafe { GetFileVersionInfoSizeW(path_u16.as_ptr(), &mut dummy) };
    if size == 0 {
        return Err("Failed to get version info size from executable".to_string());
    }

    let mut data = vec![0u8; size as usize];
    let ok =
        unsafe { GetFileVersionInfoW(path_u16.as_ptr(), 0, size, data.as_mut_ptr() as *mut _) };
    if ok == 0 {
        return Err("Failed to read version info from executable".to_string());
    }

    let subblock_root: Vec<u16> = OsStr::new(r"\").encode_wide().chain(Some(0)).collect();
    let mut root_buf: *mut std::ffi::c_void = std::ptr::null_mut();
    let mut root_len: u32 = 0;

    #[repr(C)]
    struct VsFixedFileInfo {
        dw_signature: u32,
        dw_struc_version: u32,
        dw_file_version_ms: u32,
        dw_file_version_ls: u32,
        dw_product_version_ms: u32,
        dw_product_version_ls: u32,
        dw_file_flags_mask: u32,
        dw_file_flags: u32,
        dw_file_os: u32,
        dw_file_type: u32,
        dw_file_subtype: u32,
        dw_file_date_ms: u32,
        dw_file_date_ls: u32,
    }

    if unsafe {
        VerQueryValueW(
            data.as_ptr() as *const _,
            subblock_root.as_ptr(),
            &mut root_buf,
            &mut root_len,
        )
    } != 0
        && !root_buf.is_null()
        && (root_len as usize) >= std::mem::size_of::<VsFixedFileInfo>()
    {
        let ffi = unsafe { &*(root_buf as *const VsFixedFileInfo) };
        let major = (ffi.dw_file_version_ms >> 16) & 0xffff;
        let minor = ffi.dw_file_version_ms & 0xffff;
        let patch = (ffi.dw_file_version_ls >> 16) & 0xffff;
        let build = ffi.dw_file_version_ls & 0xffff;
        let version = if build > 0 {
            format!("{}.{}.{}.{}", major, minor, patch, build)
        } else {
            format!("{}.{}.{}", major, minor, patch)
        };
        return Ok(AntigravityVersion {
            short_version: version.clone(),
            bundle_version: version,
        });
    }

    Err("Version information not found in executable".to_string())
}

/// Linux: 从 package.json 或 --version 参数读取
#[cfg(target_os = "linux")]
fn get_version_linux(exe_path: &PathBuf) -> Result<AntigravityVersion, String> {
    use std::process::Command;

    // 方法1 (优先): 尝试从安装目录的 package.json 读取，避免执行可执行文件意外拉起 GUI
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

    // 方法2 (兜底): 尝试执行 --version (仅在无法从 package.json 获取时执行)
    let output = Command::new(exe_path).arg("--version").output();

    if let Ok(result) = output {
        if result.status.success() {
            let raw_version = String::from_utf8_lossy(&result.stdout).trim().to_string();
            if !raw_version.is_empty() {
                let version = extract_semver(&raw_version).unwrap_or_else(|| {
                    raw_version
                        .lines()
                        .next()
                        .unwrap_or_default()
                        .trim()
                        .to_string()
                });
                return Ok(AntigravityVersion {
                    short_version: version.clone(),
                    bundle_version: raw_version,
                });
            }
        }
    }

    Err("Unable to determine Antigravity version on Linux".to_string())
}

/// 判断是否为新版本 (>= 1.16.5)
pub fn is_new_version(version: &AntigravityVersion) -> bool {
    compare_version(&version.short_version, "1.16.5") >= std::cmp::Ordering::Equal
}

/// 比较版本号
pub fn compare_version(v1: &str, v2: &str) -> std::cmp::Ordering {
    let parts1: Vec<u32> = v1.split('.').filter_map(|s| s.parse().ok()).collect();
    let parts2: Vec<u32> = v2.split('.').filter_map(|s| s.parse().ok()).collect();

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
        assert_eq!(
            compare_version("1.16.5", "1.16.4"),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            compare_version("1.16.5", "1.16.5"),
            std::cmp::Ordering::Equal
        );
        assert_eq!(
            compare_version("1.16.4", "1.16.5"),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            compare_version("1.17.0", "1.16.5"),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            compare_version("2.0.0", "1.16.5"),
            std::cmp::Ordering::Greater
        );
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

    #[test]
    fn test_extract_semver_from_messy_output() {
        let raw = "1.107.0\n1504c8cc4b34dbfbb4a97ebe954b3da2b5634516\nx64";
        assert_eq!(extract_semver(raw), Some("1.107.0".to_string()));
    }
}
