use crate::types::TargetHost;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

/// Antigravity 宿主版本信息
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AntigravityVersion {
    pub short_version: String,
    pub bundle_version: String,
}

static VERSION_CACHE: LazyLock<Mutex<HashMap<String, (AntigravityVersion, Instant)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

const CACHE_TTL: Duration = Duration::from_secs(60);

/// 清除版本缓存
#[allow(dead_code)]
pub fn clear_version_cache() {
    if let Ok(mut guard) = VERSION_CACHE.lock() {
        guard.clear();
    }
}

/// 语义化版本号比较 (e.g. "2.34.0" vs "2.33.0")
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

/// 从任意字符串中提取第一个语义化版本号 (X.Y.Z)
pub fn extract_semver(raw: &str) -> Option<String> {
    for token in raw.split(|c: char| c.is_whitespace() || c == ',' || c == ';') {
        let t = token.trim_matches(|c: char| c == '"' || c == '\'' || c == '(' || c == ')');
        if t.is_empty() {
            continue;
        }
        let core = t.split('-').next().unwrap_or(t);
        let parts: Vec<&str> = core.split('.').collect();
        if parts.len() >= 3
            && parts[..3]
                .iter()
                .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
        {
            return Some(format!("{}.{}.{}", parts[0], parts[1], parts[2]));
        }
    }
    None
}

/// 检测目标宿主版本（跨平台静态读取，支持内存缓存与指定路径）
pub fn get_antigravity_version_with_path(
    target: TargetHost,
    preferred_path: Option<&Path>,
) -> Result<AntigravityVersion> {
    let cache_key = format!(
        "{:?}:{}",
        target,
        preferred_path
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default()
    );

    if let Ok(guard) = VERSION_CACHE.lock() {
        if let Some((ver, timestamp)) = guard.get(&cache_key) {
            if timestamp.elapsed() < CACHE_TTL {
                return Ok(ver.clone());
            }
        }
    }

    let exe_path = preferred_path
        .map(|p| p.to_path_buf())
        .or_else(|| locate_target_executable(target))
        .ok_or_else(|| anyhow!("Unable to locate executable for target {:?}", target))?;

    let res: Result<AntigravityVersion> = {
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

        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            Err(anyhow!("Unsupported operating system"))
        }
    };

    if let Ok(ref ver) = res {
        if let Ok(mut guard) = VERSION_CACHE.lock() {
            guard.insert(cache_key, (ver.clone(), Instant::now()));
        }
    }

    res
}

/// 获取指定目标的版本
pub fn get_antigravity_version(target: TargetHost) -> Result<AntigravityVersion> {
    get_antigravity_version_with_path(target, None)
}

/// 探查宿主可执行文件或 .app 包路径
pub fn locate_target_executable(target: TargetHost) -> Option<PathBuf> {
    let is_ide = matches!(target, TargetHost::Ide);

    #[cfg(target_os = "macos")]
    {
        let app_names: &[&str] = if is_ide {
            &["Antigravity IDE", "antigravity-ide", "antigravity_ide"]
        } else {
            &["Antigravity"]
        };

        for name in app_names {
            let p = PathBuf::from(format!("/Applications/{}.app", name));
            if p.exists() {
                return Some(p);
            }
            if let Some(home) = dirs::home_dir() {
                let user_p = home.join(format!("Applications/{}.app", name));
                if user_p.exists() {
                    return Some(user_p);
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        let folder_names: &[&str] = if is_ide {
            &["Antigravity IDE", "antigravity-ide", "antigravity_ide"]
        } else {
            &["Antigravity"]
        };

        let mut base_dirs = Vec::new();
        if let Some(local_app) = dirs::data_local_dir() {
            base_dirs.push(local_app.join("Programs"));
        }
        for env_key in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(pf) = std::env::var_os(env_key) {
                base_dirs.push(PathBuf::from(pf));
            }
        }

        for base in base_dirs {
            for folder in folder_names {
                let exe = base.join(folder).join(format!("{}.exe", folder));
                if exe.exists() {
                    return Some(exe);
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        let mut standard_roots: Vec<PathBuf> = if is_ide {
            vec![
                PathBuf::from("/opt/Antigravity IDE"),
                PathBuf::from("/opt/antigravity-ide"),
                PathBuf::from("/usr/share/antigravity-ide"),
                PathBuf::from("/usr/lib/antigravity-ide"),
                PathBuf::from("/usr/bin/antigravity-ide"),
            ]
        } else {
            vec![
                PathBuf::from("/opt/Antigravity"),
                PathBuf::from("/opt/antigravity"),
                PathBuf::from("/usr/share/antigravity"),
                PathBuf::from("/usr/lib/antigravity"),
                PathBuf::from("/usr/bin/antigravity"),
            ]
        };
        if let Some(home) = dirs::home_dir() {
            if is_ide {
                standard_roots.push(home.join(".local/share/antigravity-ide"));
            } else {
                standard_roots.push(home.join(".local/share/antigravity"));
            }
        }

        for p in standard_roots {
            if p.exists() {
                return Some(p);
            }
        }
    }

    None
}

/// macOS: 从 Info.plist 静态读取版本
#[cfg(target_os = "macos")]
fn get_version_macos(exe_path: &Path) -> Result<AntigravityVersion> {
    let path_str = exe_path.to_string_lossy();
    let app_path = if let Some(idx) = path_str.find(".app") {
        PathBuf::from(&path_str[..idx + 4])
    } else {
        exe_path.to_path_buf()
    };

    let info_plist_path = app_path.join("Contents/Info.plist");
    if !info_plist_path.exists() {
        return Err(anyhow!("Info.plist not found at {:?}", info_plist_path));
    }

    let content =
        std::fs::read(&info_plist_path).map_err(|e| anyhow!("Failed to read Info.plist: {}", e))?;

    let plist: plist::Value =
        plist::from_bytes(&content).map_err(|e| anyhow!("Failed to parse Info.plist: {}", e))?;

    let dict = plist
        .as_dictionary()
        .ok_or_else(|| anyhow!("Info.plist root is not a dictionary"))?;

    let short_version = dict
        .get("CFBundleShortVersionString")
        .and_then(|v| v.as_string())
        .ok_or_else(|| anyhow!("CFBundleShortVersionString not found in Info.plist"))?;

    let bundle_version = dict
        .get("CFBundleVersion")
        .and_then(|v| v.as_string())
        .unwrap_or(short_version);

    Ok(AntigravityVersion {
        short_version: short_version.to_string(),
        bundle_version: bundle_version.to_string(),
    })
}

/// Windows: 从 PE 资源中读取 FileVersion
#[cfg(target_os = "windows")]
fn get_version_windows(exe_path: &Path) -> Result<AntigravityVersion> {
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
        return Err(anyhow!("Failed to get version info size from executable"));
    }

    let mut data = vec![0u8; size as usize];
    let ok =
        unsafe { GetFileVersionInfoW(path_u16.as_ptr(), 0, size, data.as_mut_ptr() as *mut _) };
    if ok == 0 {
        return Err(anyhow!("Failed to read version info from executable"));
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

    Err(anyhow!("Version information not found in executable"))
}

/// Linux: 从 package.json / product.json 静态读取版本，严禁直接执行 GUI 二进制
#[cfg(target_os = "linux")]
fn get_version_linux(exe_path: &Path) -> Result<AntigravityVersion> {
    let real_exe = std::fs::canonicalize(exe_path).unwrap_or_else(|_| exe_path.to_path_buf());
    let mut candidate_json_paths = Vec::new();

    let mut add_parent_candidates = |p: &Path| {
        candidate_json_paths.push(p.join("resources/app/package.json"));
        candidate_json_paths.push(p.join("resources/app/product.json"));
        candidate_json_paths.push(p.join("package.json"));
        candidate_json_paths.push(p.join("product.json"));
    };

    if real_exe.is_dir() {
        add_parent_candidates(&real_exe);
    }
    if exe_path.is_dir() && exe_path != real_exe.as_path() {
        add_parent_candidates(exe_path);
    }
    if let Some(parent) = real_exe.parent() {
        add_parent_candidates(parent);
    }
    if let Some(parent) = exe_path.parent() {
        add_parent_candidates(parent);
    }

    for json_path in &candidate_json_paths {
        if json_path.exists() {
            if let Ok(content) = std::fs::read_to_string(json_path) {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(version) = json.get("version").and_then(|v| v.as_str()) {
                        if let Some(semver) = extract_semver(version) {
                            return Ok(AntigravityVersion {
                                short_version: semver,
                                bundle_version: version.to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    // 严禁在此处调用 Command::new(exe_path).arg("--version")！
    // 桌面 GUI 应用在 Linux 下不支持 --version 退出，再次执行会触发 single-instance IPC 夺焦
    Err(anyhow!(
        "Unable to statically determine Antigravity version on Linux"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_comparison() {
        assert_eq!(
            compare_version("2.34.0", "2.33.0"),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            compare_version("2.33.0", "2.33.0"),
            std::cmp::Ordering::Equal
        );
        assert_eq!(
            compare_version("2.33.0", "2.34.0"),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            compare_version("2.0.0", "1.16.5"),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            compare_version("2.7.0", "2.0.0"),
            std::cmp::Ordering::Greater
        );
    }

    #[test]
    fn test_extract_semver() {
        assert_eq!(
            extract_semver("version: 2.33.0-beta.1"),
            Some("2.33.0".to_string())
        );
        assert_eq!(extract_semver("2.7.0"), Some("2.7.0".to_string()));
    }
}
