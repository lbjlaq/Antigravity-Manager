use std::fs;
use std::path::Path;
// codesign 仅 macOS 分支使用（patch_agy_binary 重签名）
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(target_os = "macos")]
use std::process::Command;

/// 探测指定路径是否位于某个 .app Bundle 内部
fn find_enclosing_app_bundle(path: &Path) -> Option<std::path::PathBuf> {
    let mut cur = path.parent();
    while let Some(p) = cur {
        if p.extension().map_or(false, |ext| ext == "app") {
            return Some(p.to_path_buf());
        }
        cur = p.parent();
    }
    None
}

/// 查找备份文件（依次检索：.deep_compact_backups 目录、backups 目录、旧版同级 bak 备份）
fn find_existing_backup_path(path: &Path) -> Option<std::path::PathBuf> {
    if let Some(bundle) = find_enclosing_app_bundle(path) {
        if let Some(parent) = bundle.parent() {
            let bin_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("claude");
            let app_name = bundle
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("claude.app");

            // 候选 1: parent/.deep_compact_backups/claude.app.claude.bak
            let c1 = parent
                .join(".deep_compact_backups")
                .join(format!("{}.{}.bak", app_name, bin_name));
            if c1.exists() {
                return Some(c1);
            }

            // 候选 2: parent/backups/claude.bak 或 claude.deep_compact.bak
            let c2 = parent.join("backups").join(format!("{}.bak", bin_name));
            if c2.exists() {
                return Some(c2);
            }
            let c2_deep = parent
                .join("backups")
                .join(format!("{}.deep_compact.bak", bin_name));
            if c2_deep.exists() {
                return Some(c2_deep);
            }
        }
    }

    // 候选 3: 旧版同级 .deep_compact.bak
    let c3 = std::path::PathBuf::from(format!("{}.deep_compact.bak", path.display()));
    if c3.exists() {
        return Some(c3);
    }
    let c4 = std::path::PathBuf::from(format!("{}.bak", path.display()));
    if c4.exists() {
        return Some(c4);
    }

    None
}

/// 计算备份文件的安全写入路径（若在 .app 内部，存放在 .app 外部同级目录，避免破坏 macOS Bundle 签名密封）
fn get_safe_backup_path(path: &Path) -> std::path::PathBuf {
    if let Some(bundle) = find_enclosing_app_bundle(path) {
        if let Some(parent) = bundle.parent() {
            let backup_dir = parent.join(".deep_compact_backups");
            let _ = fs::create_dir_all(&backup_dir);
            let bin_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("claude");
            let app_name = bundle
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("claude.app");
            return backup_dir.join(format!("{}.{}.bak", app_name, bin_name));
        }
    }
    std::path::PathBuf::from(format!("{}.deep_compact.bak", path.display()))
}

#[tauri::command]
pub async fn patch_agy_binary(file_path: String) -> Result<String, String> {
    let mut actual_path = file_path.clone();
    if actual_path.ends_with(".app") || actual_path.ends_with(".app/") {
        let app_path = Path::new(&actual_path);
        let inner = app_path.join("Contents/MacOS/agy");
        if inner.exists() {
            actual_path = inner.to_string_lossy().to_string();
        }
    }

    let path = Path::new(&actual_path);
    if !path.exists() {
        return Err("File not found".into());
    }

    let data = fs::read(path).map_err(|e| format!("Failed to read file: {}", e))?;
    let n = data.len();
    let mut patch_offset = None;
    let mut new_inst_bytes = None;
    let mut is_pe_x64 = false;

    // 1. Scan for x86_64 PE (Windows/Linux) pattern
    // Pattern: cmpb $0x0, (%r12) -> 41 80 3c 24 00
    //          jne offset32      -> 0f 85 XX XX XX XX
    //          leaq rip_off, rax -> 48 8d 05 XX XX XX XX
    //          mov $0x18, %ebx   -> bb 18 00 00 00
    let pe_pattern = [0x41, 0x80, 0x3c, 0x24, 0x00, 0x0f, 0x85];
    let mut i = 0;
    while i < n - 25 {
        if data[i..i + 7] == pe_pattern {
            // Validate the rest of the pattern
            // leaq opcode starts after jne (which is 6 bytes: 0f 85 XX XX XX XX)
            let leaq_idx = i + 5 + 6;
            if data[leaq_idx..leaq_idx + 3] == [0x48, 0x8d, 0x05] {
                // mov $0x18, %ebx starts after leaq (which is 7 bytes: 48 8d 05 XX XX XX XX)
                let mov_idx = leaq_idx + 7;
                if data[mov_idx..mov_idx + 2] == [0xbb, 0x18] {
                    // Found the gate!
                    patch_offset = Some(i + 5); // Points to the jne instruction: 0f 85 ...
                                                // Rewrite jne to 6 NOP bytes (0x90) so it falls through unconditionally
                    new_inst_bytes = Some(vec![0x90; 6]);
                    is_pe_x64 = true;
                    break;
                }
            }
        }
        i += 1;
    }

    // 2. Scan for ARM64 eligibility gate pattern if not PE x86_64
    if patch_offset.is_none() {
        for j in (0..n - 20).step_by(4) {
            let inst1 = u32::from_le_bytes(data[j..j + 4].try_into().unwrap());
            let inst2 = u32::from_le_bytes(data[j + 4..j + 8].try_into().unwrap());
            let inst4 = u32::from_le_bytes(data[j + 12..j + 16].try_into().unwrap());
            let inst5 = u32::from_le_bytes(data[j + 16..j + 20].try_into().unwrap());

            // 1. ldrb wA, [xB, #0x58]
            if (inst1 & 0xfffffc00) != 0x39416000 {
                continue;
            }
            let b_reg = (inst1 >> 5) & 0x1f;
            let a_reg = inst1 & 0x1f;

            // 2. tbnz wA, #0, label1
            if (inst2 & 0xffe0001f) != (0x37000000 | a_reg) {
                continue;
            }

            // 3. ldr xC, [xB, #0x38]
            if (inst4 & 0xfffffc00) != 0xf9401c00 || ((inst4 >> 5) & 0x1f) != b_reg {
                continue;
            }
            let c_reg = inst4 & 0x1f;

            // 4. cbz xC, label_send
            if (inst5 & 0xffe0001f) != (0xb4000000 | c_reg) {
                continue;
            }

            // Extract imm19 from cbz
            let imm19_raw = (inst5 >> 5) & 0x7ffff;
            let imm19 = if (imm19_raw & 0x40000) != 0 {
                (imm19_raw as i32) - 0x80000
            } else {
                imm19_raw as i32
            };

            patch_offset = Some(j + 16);
            // Encode unconditional branch: b label_send (0x14000000 | (imm19 & 0x3ffffff))
            let b_inst = 0x14000000 | ((imm19 as u32) & 0x3ffffff);
            new_inst_bytes = Some(b_inst.to_le_bytes().to_vec());
            break;
        }
    }

    if patch_offset.is_none() {
        // Check if already patched for x86_64 PE
        let mut check_idx = 0;
        while check_idx < n - 25 {
            if data[check_idx..check_idx + 7] == pe_pattern {
                let leaq_idx = check_idx + 5 + 6;
                if data[leaq_idx..leaq_idx + 3] == [0x48, 0x8d, 0x05] {
                    let mov_idx = leaq_idx + 7;
                    if data[mov_idx..mov_idx + 2] == [0xbb, 0x18] {
                        if data[check_idx + 5..check_idx + 11] == [0x90; 6] {
                            return Ok("Binary is already patched.".into());
                        }
                    }
                }
            }
            check_idx += 1;
        }

        // Check if already patched for ARM64
        for j in (0..n - 20).step_by(4) {
            let inst1 = u32::from_le_bytes(data[j..j + 4].try_into().unwrap());
            let inst2 = u32::from_le_bytes(data[j + 4..j + 8].try_into().unwrap());
            let inst4 = u32::from_le_bytes(data[j + 12..j + 16].try_into().unwrap());
            let inst5 = u32::from_le_bytes(data[j + 16..j + 20].try_into().unwrap());

            if (inst1 & 0xfffffc00) == 0x39416000 {
                let b_reg = (inst1 >> 5) & 0x1f;
                let a_reg = inst1 & 0x1f;
                if (inst2 & 0xffe0001f) == (0x37000000 | a_reg) {
                    if (inst4 & 0xfffffc00) == 0xf9401c00 && ((inst4 >> 5) & 0x1f) == b_reg {
                        if (inst5 & 0xfc000000) == 0x14000000 {
                            return Ok("Binary is already patched.".into());
                        }
                    }
                }
            }
        }

        return Err("Pattern not found. This version of the CLI might not have the eligibility gate, or the structure has changed.".into());
    }

    let offset = patch_offset.unwrap();
    let patch_bytes = new_inst_bytes.unwrap();
    let orig_pattern = data[offset..offset + patch_bytes.len()].to_vec();

    // Create backup atomically
    let backup_path = format!("{}.bak", actual_path);
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&backup_path)
    {
        Ok(mut backup_file) => {
            let mut src_file = fs::File::open(path)
                .map_err(|e| format!("Failed to open file for backup: {}", e))?;
            if let Err(e) = std::io::copy(&mut src_file, &mut backup_file) {
                let _ = fs::remove_file(&backup_path);
                return Err(format!("Failed to write backup: {}", e));
            }
            if let Err(e) = backup_file.sync_all() {
                let _ = fs::remove_file(&backup_path);
                return Err(format!("Failed to sync backup: {}", e));
            }
            #[cfg(unix)]
            if let Ok(src_meta) = src_file.metadata() {
                let _ = backup_file.set_permissions(src_meta.permissions());
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(format!("Failed to create backup: {}", e)),
    }

    // Apply patch
    use std::io::{Read, Seek, SeekFrom, Write};
    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| format!("Failed to open file for writing: {}", e))?;
    file.seek(SeekFrom::Start(offset as u64))
        .map_err(|e| format!("Seek failed: {}", e))?;

    let mut current_bytes = vec![0u8; orig_pattern.len()];
    file.read_exact(&mut current_bytes)
        .map_err(|e| format!("Failed to read target bytes at offset: {}", e))?;
    if current_bytes != orig_pattern {
        return Err("Existing bytes at offset do not match expected pattern".into());
    }

    file.seek(SeekFrom::Start(offset as u64))
        .map_err(|e| format!("Seek failed: {}", e))?;
    file.write_all(&patch_bytes)
        .map_err(|e| format!("Write failed: {}", e))?;

    // Re-sign on macOS (only if we patched an ARM64 macOS executable)
    #[cfg(target_os = "macos")]
    {
        if !is_pe_x64 {
            let _ = Command::new("/usr/bin/codesign")
                .args(&["--remove-signature", "--", &actual_path])
                .output();
            let output = Command::new("/usr/bin/codesign")
                .args(&["--sign", "-", "--", &actual_path])
                .output();
            match output {
                Ok(out) if out.status.success() => {}
                Ok(out) => {
                    let err_msg = String::from_utf8_lossy(&out.stderr);
                    return Err(format!(
                        "Patch applied, but codesigning failed: {}",
                        err_msg
                    ));
                }
                Err(e) => {
                    return Err(format!(
                        "Patch applied, but codesigning execution failed: {}",
                        e
                    ))
                }
            }
        }
    }

    Ok("Patch applied successfully!".into())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ClaudeInstallationInfo {
    pub version: String,
    pub path: String,
    pub is_patched: bool,
    pub is_patchable: bool,
    pub is_8k: bool,
    pub size_mb: f64,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ClaudePatchStatus {
    pub file_path: String,
    pub is_patched: bool,
    pub is_patchable: bool,
    pub is_8k: bool,
    pub is_legacy: bool,
    pub message: String,
    pub available_installations: Vec<ClaudeInstallationInfo>,
}

/// 递归在目录下查找所有名为 claude.app 的 bundle 目录 (最大深度限制避免无限递归)
fn find_claude_apps_recursively(
    dir: &Path,
    max_depth: usize,
) -> Vec<(std::path::PathBuf, std::path::PathBuf)> {
    let Ok(canonical_base) = dir.canonicalize() else {
        return Vec::new();
    };

    fn scan_dir(
        dir: &Path,
        base_root: &Path,
        max_depth: usize,
    ) -> Vec<(std::path::PathBuf, std::path::PathBuf)> {
        let mut results = Vec::new();
        if max_depth == 0 || !dir.is_dir() {
            return results;
        }

        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                // entry.file_type()?.is_symlink() guard: 拒绝符号链接以防目录穿越与无限环路
                let is_symlink = entry.file_type().map(|ft| ft.is_symlink()).unwrap_or(true);
                if is_symlink {
                    continue;
                }

                let p = entry.path();
                // 确保规范化路径完全包含在 base root 内部
                let Ok(canonical_p) = p.canonicalize() else {
                    continue;
                };
                if !canonical_p.starts_with(base_root) {
                    continue;
                }

                if canonical_p.is_dir() {
                    if canonical_p.file_name().map_or(false, |n| n == "claude.app") {
                        let bin = canonical_p.join("Contents/MacOS/claude");
                        if bin.exists() {
                            results.push((canonical_p, bin));
                        }
                    } else {
                        results.extend(scan_dir(&canonical_p, base_root, max_depth - 1));
                    }
                }
            }
        }

        results
    }

    scan_dir(&canonical_base, &canonical_base, max_depth)
}

/// 从 App Bundle 或路径中解析版本号
fn extract_bundle_version(app_dir: &Path) -> String {
    let plist_path = app_dir.join("Contents/Info.plist");
    if plist_path.exists() {
        if let Ok(content) = fs::read_to_string(&plist_path) {
            let re_short = regex::Regex::new(
                r#"<key>CFBundleShortVersionString</key>\s*<string>([^<]+)</string>"#,
            )
            .ok();
            if let Some(re) = re_short {
                if let Some(caps) = re.captures(&content) {
                    return caps.get(1).unwrap().as_str().to_string();
                }
            }
            let re_ver =
                regex::Regex::new(r#"<key>CFBundleVersion</key>\s*<string>([^<]+)</string>"#).ok();
            if let Some(re) = re_ver {
                if let Some(caps) = re.captures(&content) {
                    return caps.get(1).unwrap().as_str().to_string();
                }
            }
        }
    }

    // 从路径字符串中正则提取类似 2.1.xxx 的版本号
    let path_str = app_dir.to_string_lossy();
    let re_path = regex::Regex::new(r"2\.\d+\.\d+").unwrap();
    if let Some(m) = re_path.find(&path_str) {
        return m.as_str().to_string();
    }

    app_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Unknown")
        .to_string()
}

/// 解析版本号为语义化元组 (major, minor, patch) 以便正确数值比较
fn parse_semver(ver: &str) -> (u32, u32, u32) {
    let clean = ver.trim_start_matches("Sandbox-").trim_start_matches('v');
    let parts: Vec<&str> = clean.split('.').collect();
    let maj = parts.get(0).and_then(|s| s.parse().ok()).unwrap_or(0);
    let min = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let pat = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    (maj, min, pat)
}

/// 扫描系统中所有已安装的 Claude 运行时或客户端二进制
fn scan_all_claude_installations() -> Vec<ClaudeInstallationInfo> {
    let mut results: Vec<(bool, (u32, u32, u32), ClaudeInstallationInfo)> = Vec::new();

    #[cfg(target_os = "macos")]
    {
        let mut candidates = Vec::new();

        if let Some(home) = dirs::home_dir() {
            // 1. 递归扫描当前用户目录下的 Claude-3p/claude-code (正式生产路径，优先级最高)
            let base_dir = home.join("Library/Application Support/Claude-3p/claude-code");
            if base_dir.exists() {
                for (app_dir, bin) in find_claude_apps_recursively(&base_dir, 4) {
                    let ver_name = extract_bundle_version(&app_dir);
                    candidates.push((ver_name, app_dir, bin, true));
                }
            }

            // 1.1 扫描沙盒测试目录 deep_compact_test_sandbox (沙盒测试路径，优先级垫底)
            let sandbox_versions = home.join("Documents/deep_compact_test_sandbox/versions");
            if sandbox_versions.exists() {
                for (app_dir, bin) in find_claude_apps_recursively(&sandbox_versions, 3) {
                    let raw_ver = extract_bundle_version(&app_dir);
                    let ver_name = format!("Sandbox-{}", raw_ver);
                    candidates.push((ver_name, app_dir, bin, false));
                }
            }
        }

        // 2. 扫描系统 Applications（仅保留具备核心功能的实际存在实例，排除无有效修剪函数的空壳）
        let standard_claude = std::path::PathBuf::from("/Applications/Claude.app");
        let standard_bin = standard_claude.join("Contents/MacOS/Claude");
        if standard_bin.exists() {
            if let Ok(m) = fs::metadata(&standard_bin) {
                if m.len() > 1024 * 1024 {
                    candidates.push((
                        "Desktop-App".to_string(),
                        standard_claude,
                        standard_bin,
                        true,
                    ));
                }
            }
        }

        // 3. 去重并提取补丁状态
        let patched_re = regex::bytes::Regex::new(
            r"function\s+[a-zA-Z0-9_$]+\([a-zA-Z0-9_$]+,[a-zA-Z0-9_$]+,[a-zA-Z0-9_$]+\)\{(return 0;|.*?s>=(8000|35000).*?\})"
        ).ok();

        let origin_re = regex::bytes::Regex::new(
            r"function\s+([a-zA-Z0-9_$]+)\(([a-zA-Z0-9_$]+),([a-zA-Z0-9_$]+),([a-zA-Z0-9_$]+)\)\{let\s+[a-zA-Z0-9_$]+=0,[a-zA-Z0-9_$]+=0;for\(let\s+[a-zA-Z0-9_$]+=[a-zA-Z0-9_$]+-1;[a-zA-Z0-9_$]+>=0;[a-zA-Z0-9_$]+--\)if\([a-zA-Z0-9_$]+\+=[a-zA-Z0-9_$]+\[[a-zA-Z0-9_$]+\],[a-zA-Z0-9_$]+\+\+,[a-zA-Z0-9_$]+>=[a-zA-Z0-9_$]+\)break;if\([a-zA-Z0-9_$]+>=[a-zA-Z0-9_$]+-1\)return\s+Math\.max\(1,Math\.floor\([a-zA-Z0-9_$]+/2\)\);return\s+[a-zA-Z0-9_$]+\}"
        ).ok();

        for (ver, app_path, bin_path, is_prod) in candidates {
            if let Ok(meta) = fs::metadata(&bin_path) {
                let sz_mb = meta.len() as f64 / (1024.0 * 1024.0);
                let (is_patched, is_patchable, is_8k) = if let Ok(data) = fs::read(&bin_path) {
                    let patched = patched_re.as_ref().map_or(false, |r| r.is_match(&data));
                    let is_8k = if patched {
                        data.windows(8).any(|w| w == b"s>=8000)")
                    } else {
                        false
                    };
                    let patchable = if patched {
                        true
                    } else {
                        origin_re.as_ref().map_or(false, |r| r.is_match(&data))
                    };
                    (patched, patchable, is_8k)
                } else {
                    (false, false, false)
                };

                results.push((
                    is_prod,
                    parse_semver(&ver),
                    ClaudeInstallationInfo {
                        version: ver,
                        path: app_path.to_string_lossy().to_string(),
                        is_patched,
                        is_patchable,
                        is_8k,
                        size_mb: (sz_mb * 10.0).round() / 10.0,
                    },
                ));
            }
        }

        // 排序规则：生产环境在前(true > false)，版本号高的在前，最后解包
        results.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    }

    results.into_iter().map(|item| item.2).collect()
}

/// 辅助函数：解析 Claude 客户端可执行文件路径
fn resolve_claude_binary_path(custom_path: Option<String>) -> Result<std::path::PathBuf, String> {
    if let Some(p) = custom_path {
        let trimmed = p.trim().trim_matches('"').trim_matches('\'').trim();
        if !trimmed.is_empty() {
            let path = std::path::PathBuf::from(trimmed);
            // 兼容用户传入 .app 目录或任意上层目录
            if path.is_dir() {
                let inner = path.join("Contents/MacOS/claude");
                if inner.exists() {
                    return Ok(inner);
                }
                let inner_cap = path.join("Contents/MacOS/Claude");
                if inner_cap.exists() {
                    return Ok(inner_cap);
                }
                // 递归探测当前目录及其子目录下的 claude 可执行文件
                if let Ok(entries) = fs::read_dir(&path) {
                    for e in entries.flatten() {
                        let sub = e.path();
                        if sub.is_file()
                            && sub
                                .file_name()
                                .map_or(false, |n| n == "claude" || n == "Claude")
                        {
                            return Ok(sub);
                        }
                    }
                }
            }
            if path.exists() {
                return Ok(path);
            }
            return Err(format!("指定路径不存在: {}", trimmed));
        }
    }

    let installations = scan_all_claude_installations();
    // 列表已按优先级和最高版本排序，直接取第一个即可命中生产最新版
    if let Some(first) = installations.into_iter().next() {
        let app_path = std::path::PathBuf::from(first.path);
        let inner = app_path.join("Contents/MacOS/claude");
        if inner.exists() {
            return Ok(inner);
        }
        let inner_cap = app_path.join("Contents/MacOS/Claude");
        if inner_cap.exists() {
            return Ok(inner_cap);
        }
        return Ok(app_path);
    }

    Err("未在系统中自动发现 Claude Desktop 实例，请手动选择或输入路径".into())
}

/// 列出系统中所有发现的 Claude Desktop 实例 (暴露给前端选择器)
#[tauri::command]
pub async fn list_claude_installations() -> Result<Vec<ClaudeInstallationInfo>, String> {
    Ok(scan_all_claude_installations())
}

/// 检查 Claude 客户端的深度归档补丁状态 (外置纯只读检查)
#[tauri::command]
pub async fn check_claude_cowork_patch(
    file_path: Option<String>,
) -> Result<ClaudePatchStatus, String> {
    let all_installs = scan_all_claude_installations();
    let path = resolve_claude_binary_path(file_path)?;
    let data = fs::read(&path).map_err(|e| format!("读取文件失败: {}", e))?;

    // 1. 检查是否已经注入过补丁 (支持 return 0; 或 8k/35k 精准上下文预算模式)
    let patched_re = regex::bytes::Regex::new(
        r"function\s+[a-zA-Z0-9_$]+\([a-zA-Z0-9_$]+,[a-zA-Z0-9_$]+,[a-zA-Z0-9_$]+\)\{(return 0;|.*?s>=(8000|35000).*?\})"
    ).map_err(|e| e.to_string())?;

    if patched_re.is_match(&data) {
        let is_8k = data.windows(8).any(|w| w == b"s>=8000)");
        let desc = if is_8k {
            "已成功注入 8k 深度归档补丁 (保留最新 8k 活跃消息上下文，超出历史 100% 浓缩归档，压缩率与净空大幅提升)"
        } else {
            "检测到旧版补丁 (35k/return 0)，建议点击「一键注入补丁」平滑升级为 8k 深度归档以获得超 60% 压缩率"
        };
        return Ok(ClaudePatchStatus {
            file_path: path.to_string_lossy().to_string(),
            is_patched: true,
            is_patchable: true,
            is_8k,
            is_legacy: !is_8k,
            message: desc.into(),
            available_installations: all_installs,
        });
    }

    // 2. 检查是否匹配官方原生修剪算法特征 (Structural AST 模式)
    let origin_re = regex::bytes::Regex::new(
        r"function\s+([a-zA-Z0-9_$]+)\(([a-zA-Z0-9_$]+),([a-zA-Z0-9_$]+),([a-zA-Z0-9_$]+)\)\{let\s+[a-zA-Z0-9_$]+=0,[a-zA-Z0-9_$]+=0;for\(let\s+[a-zA-Z0-9_$]+=[a-zA-Z0-9_$]+-1;[a-zA-Z0-9_$]+>=0;[a-zA-Z0-9_$]+--\)if\([a-zA-Z0-9_$]+\+=[a-zA-Z0-9_$]+\[[a-zA-Z0-9_$]+\],[a-zA-Z0-9_$]+\+\+,[a-zA-Z0-9_$]+>=[a-zA-Z0-9_$]+\)break;if\([a-zA-Z0-9_$]+>=[a-zA-Z0-9_$]+-1\)return\s+Math\.max\(1,Math\.floor\([a-zA-Z0-9_$]+/2\)\);return\s+[a-zA-Z0-9_$]+\}"
    ).map_err(|e| e.to_string())?;

    if origin_re.is_match(&data) {
        return Ok(ClaudePatchStatus {
            file_path: path.to_string_lossy().to_string(),
            is_patched: false,
            is_patchable: true,
            is_8k: false,
            is_legacy: false,
            message: "检测到官方原生修剪算法，可安全注入 135 字节等长微创补丁".into(),
            available_installations: all_installs,
        });
    }

    Ok(ClaudePatchStatus {
        file_path: path.to_string_lossy().to_string(),
        is_patched: false,
        is_patchable: false,
        is_8k: false,
        is_legacy: false,
        message: "未匹配到目标修剪特征，当前版本结构可能已变更".into(),
        available_installations: all_installs,
    })
}

/// 应用 Claude Cowork 深度归档微创等长补丁 (外置独立工具命令)
#[tauri::command]
pub async fn apply_claude_cowork_patch(file_path: Option<String>) -> Result<String, String> {
    let path = resolve_claude_binary_path(file_path)?;
    let actual_path = path.to_string_lossy().to_string();

    let data = fs::read(&path).map_err(|e| format!("读取文件失败: {}", e))?;

    // 1. 检查是否已打 8k 深度归档补丁
    let patched_8k_re = regex::bytes::Regex::new(
        r"function\s+[a-zA-Z0-9_$]+\([a-zA-Z0-9_$]+,[a-zA-Z0-9_$]+,[a-zA-Z0-9_$]+\)\{(return 0;|.*?s>=8000.*?\})"
    ).map_err(|e| e.to_string())?;
    if patched_8k_re.is_match(&data) {
        return Ok("该文件已处于 8k 深度归档补丁生效状态，无需重复注入".into());
    }

    // 2. 匹配原生特征并提取变量名
    let origin_re = regex::bytes::Regex::new(
        r"function\s+([a-zA-Z0-9_$]+)\(([a-zA-Z0-9_$]+),([a-zA-Z0-9_$]+),([a-zA-Z0-9_$]+)\)\{let\s+[a-zA-Z0-9_$]+=0,[a-zA-Z0-9_$]+=0;for\(let\s+[a-zA-Z0-9_$]+=[a-zA-Z0-9_$]+-1;[a-zA-Z0-9_$]+>=0;[a-zA-Z0-9_$]+--\)if\([a-zA-Z0-9_$]+\+=[a-zA-Z0-9_$]+\[[a-zA-Z0-9_$]+\],[a-zA-Z0-9_$]+\+\+,[a-zA-Z0-9_$]+>=[a-zA-Z0-9_$]+\)break;if\([a-zA-Z0-9_$]+>=[a-zA-Z0-9_$]+-1\)return\s+Math\.max\(1,Math\.floor\([a-zA-Z0-9_$]+/2\)\);return\s+[a-zA-Z0-9_$]+\}"
    ).map_err(|e| e.to_string())?;

    let mut data = data;
    if !origin_re.is_match(&data) {
        // 若当前文件打了旧版 35k 补丁，尝试自动从备份还原原始二进制以完成 8k 升级
        if let Some(backup_path) = find_existing_backup_path(&path) {
            if let Ok(orig_data) = fs::read(&backup_path) {
                if origin_re.is_match(&orig_data) {
                    data = orig_data;
                } else {
                    return Err("未找到修剪算法特征，无法应用补丁".into());
                }
            } else {
                return Err("未找到修剪算法特征，且无法读取备份文件".into());
            }
        } else {
            return Err("未找到修剪算法特征，无法应用补丁".into());
        }
    }

    let Some(caps) = origin_re.captures(&data) else {
        return Err("未找到修剪算法特征，无法应用补丁".into());
    };

    let matched_match = caps.get(0).unwrap();
    let offset = matched_match.start();
    let matched_len = matched_match.end() - offset;
    let orig_pattern = matched_match.as_bytes().to_vec();

    let fn_name = std::str::from_utf8(caps.get(1).unwrap().as_bytes()).unwrap();
    let p1 = std::str::from_utf8(caps.get(2).unwrap().as_bytes()).unwrap();
    let p2 = std::str::from_utf8(caps.get(3).unwrap().as_bytes()).unwrap();
    let p3 = std::str::from_utf8(caps.get(4).unwrap().as_bytes()).unwrap();

    // 构造严格等长替换字节流 (精准设置为 8k 活跃消息预算，提升压缩率并保持严格等长与 0 偏移漂移)
    let prefix = format!("function {}({},{},{}){{let s=0,g=0;for(let h={}-1;h>=0;h--)if(s+={}[h],g++,s>=8000)break;return g;}}/*", fn_name, p1, p2, p3, p2, p1);
    let suffix = "*/";
    if prefix.len() + suffix.len() > matched_len {
        return Err("构造补丁长度超限".into());
    }
    let spaces_needed = matched_len - prefix.len() - suffix.len();
    let mut replacement = prefix.into_bytes();
    replacement.extend(vec![b' '; spaces_needed]);
    replacement.extend_from_slice(suffix.as_bytes());

    // 3. 创建 .bak 备份文件（使用安全目录，不污染 Bundle 内部）
    let backup_path = get_safe_backup_path(&path);
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&backup_path)
    {
        Ok(mut backup_file) => {
            let mut src_file =
                fs::File::open(&path).map_err(|e| format!("打开原始文件失败: {}", e))?;
            if let Err(e) = std::io::copy(&mut src_file, &mut backup_file) {
                let _ = fs::remove_file(&backup_path);
                return Err(format!("写入备份文件失败: {}", e));
            }
            if let Err(e) = backup_file.sync_all() {
                let _ = fs::remove_file(&backup_path);
                return Err(format!("同步备份文件失败: {}", e));
            }
            #[cfg(unix)]
            if let Ok(src_meta) = src_file.metadata() {
                let _ = backup_file.set_permissions(src_meta.permissions());
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            // 备份文件已原子存在，保留初始纯净副本，无需重复创建
        }
        Err(e) => return Err(format!("创建备份文件失败: {}", e)),
    }

    // 4. 原子写入与替换 (Atomic Write-and-Replace)
    let parent_dir = path
        .parent()
        .ok_or_else(|| "无法获取目标文件所在目录".to_string())?;

    let mut patched_data = data;
    if &patched_data[offset..offset + matched_len] != orig_pattern.as_slice() {
        return Err("目标偏移字节与原始模式不匹配，终止写入以防破坏二进制".into());
    }
    patched_data[offset..offset + matched_len].copy_from_slice(&replacement);

    let temp_file_path = parent_dir.join(format!(".claude_patch_{}.tmp", uuid::Uuid::new_v4()));
    let write_res = (|| -> Result<(), String> {
        use std::io::Write;
        let mut temp_file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_file_path)
            .map_err(|e| format!("创建临时文件失败: {}", e))?;

        temp_file
            .write_all(&patched_data)
            .map_err(|e| format!("写入临时文件失败: {}", e))?;
        temp_file
            .flush()
            .map_err(|e| format!("刷新临时文件缓冲区失败: {}", e))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(metadata) = temp_file.metadata() {
                let mut perms = metadata.permissions();
                perms.set_mode(0o755);
                let _ = temp_file.set_permissions(perms);
            }
        }

        temp_file
            .sync_all()
            .map_err(|e| format!("同步临时文件到磁盘失败: {}", e))?;
        drop(temp_file);

        fs::rename(&temp_file_path, &path).map_err(|e| format!("原子替换目标文件失败: {}", e))?;
        Ok(())
    })();

    if let Err(e) = write_res {
        let _ = fs::remove_file(&temp_file_path);
        return Err(e);
    }

    // 5. macOS ad-hoc 代码重签名（若属于 App Bundle，需连带进行 Deep 重签名以满足系统 Gatekeeper 规范）
    #[cfg(target_os = "macos")]
    {
        let rollback = || {
            let _ = fs::copy(&backup_path, &path);
            #[cfg(unix)]
            {
                if let Ok(metadata) = fs::metadata(&path) {
                    let mut perms = metadata.permissions();
                    perms.set_mode(0o755);
                    let _ = fs::set_permissions(&path, perms);
                }
            }
        };

        // 5.1 签名核心可执行二进制
        let output = std::process::Command::new("/usr/bin/codesign")
            .args(["--force", "--sign", "-", "--", &actual_path])
            .output();
        match output {
            Ok(out) if out.status.success() => {}
            Ok(out) => {
                rollback();
                let err_msg = String::from_utf8_lossy(&out.stderr);
                return Err(format!(
                    "补丁已写入，但二进制 codesign 重签名失败（已自动回滚备份以防 AMFI 崩溃）: {}",
                    err_msg
                ));
            }
            Err(e) => {
                rollback();
                return Err(format!("执行 codesign 命令失败（已自动回滚备份）: {}", e));
            }
        }

        // 5.2 若位于 .app Bundle 内，对整个 App 进行 Deep 签名
        if let Some(bundle_path) = find_enclosing_app_bundle(&path) {
            let bundle_str = bundle_path.to_string_lossy().to_string();
            let bundle_output = std::process::Command::new("/usr/bin/codesign")
                .args(["--force", "--deep", "--sign", "-", "--", &bundle_str])
                .output();
            match bundle_output {
                Ok(out) if out.status.success() => {}
                Ok(out) => {
                    rollback();
                    let err_msg = String::from_utf8_lossy(&out.stderr);
                    return Err(format!(
                        "二进制已签名，但 App Bundle deep 重签名失败（已自动回滚备份）: {}",
                        err_msg
                    ));
                }
                Err(e) => {
                    rollback();
                    return Err(format!(
                        "执行 App Bundle codesign 命令失败（已自动回滚备份）: {}",
                        e
                    ));
                }
            }
        }
    }

    Ok(format!(
        "成功为 Claude 注入深度归档微创补丁并完成重签名！目标路径: {}",
        actual_path
    ))
}

/// 还原 Claude Cowork 原始修剪逻辑 (外置独立工具命令)
#[tauri::command]
pub async fn revert_claude_cowork_patch(file_path: Option<String>) -> Result<String, String> {
    let path = resolve_claude_binary_path(file_path)?;
    let actual_path = path.to_string_lossy().to_string();

    if let Some(backup_path) = find_existing_backup_path(&path) {
        fs::copy(&backup_path, &path).map_err(|e| format!("从备份文件恢复失败: {}", e))?;

        #[cfg(unix)]
        {
            if let Ok(metadata) = fs::metadata(&path) {
                let mut perms = metadata.permissions();
                perms.set_mode(0o755);
                let _ = fs::set_permissions(&path, perms);
            }
        }

        #[cfg(target_os = "macos")]
        {
            // 备份文件本身保留了官方开发者证书与原版签名，严禁执行 ad-hoc 覆盖以避免剥离官方证书与 Keychain 授权
            let _ = std::process::Command::new("/usr/bin/codesign")
                .args(["--verify", "--verbose=2", "--", &actual_path])
                .output();
        }
        return Ok("已成功从备份还原原生二进制！".into());
    }

    Err("未找到备份文件，无法执行一键还原".into())
}

/// 内部辅助函数：检查 Claude Desktop 进程是否正在运行
fn is_claude_running_internal(file_path: Option<&str>) -> bool {
    #[cfg(target_os = "macos")]
    {
        // 1. 优先通过 AppleScript 检查 Claude.app 实例状态
        let out = std::process::Command::new("/usr/bin/osascript")
            .args(["-e", "application \"Claude\" is running"])
            .output();
        if let Ok(output) = out {
            if output.status.success() {
                let s = String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .to_lowercase();
                if s == "true" {
                    return true;
                }
            }
        }

        // 2. 深度扫描系统进程列表，匹配 Claude 路径及子进程
        let mut sys = sysinfo::System::new();
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All);
        let target_bundle = file_path
            .map(std::path::PathBuf::from)
            .and_then(|p| find_enclosing_app_bundle(&p))
            .map(|p| p.to_string_lossy().to_string().to_lowercase());

        for (_pid, proc_) in sys.processes() {
            let exe = proc_
                .exe()
                .map(|e| e.to_string_lossy().to_string().to_lowercase())
                .unwrap_or_default();

            if exe.contains("/applications/claude.app/")
                || exe.contains("claude.app/contents/macos/")
            {
                return true;
            }
            if let Some(ref tb) = target_bundle {
                if exe.contains(tb) {
                    return true;
                }
            }
        }
        false
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = file_path;
        false
    }
}

/// 检查 Claude Desktop 客户端是否正在运行
#[tauri::command]
pub async fn is_claude_desktop_running(file_path: Option<String>) -> Result<bool, String> {
    tokio::task::spawn_blocking(move || is_claude_running_internal(file_path.as_deref()))
        .await
        .map_err(|e| format!("检查 Claude 运行状态任务失败: {}", e))
}

/// 优雅退出并清理 Claude Desktop 进程
#[tauri::command]
pub async fn close_claude_desktop(file_path: Option<String>) -> Result<bool, String> {
    tokio::task::spawn_blocking(move || {
        #[cfg(target_os = "macos")]
        {
            // 1. 优先通过 AppleScript 优雅退出 Claude
            let _ = std::process::Command::new("/usr/bin/osascript")
                .args(["-e", "tell application \"Claude\" to quit"])
                .output();

            // 最多等待 3 秒等待进程优雅退出
            let start = std::time::Instant::now();
            while start.elapsed() < std::time::Duration::from_millis(3000) {
                if !is_claude_running_internal(file_path.as_deref()) {
                    return Ok(true);
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }

            // 2. 超时未完全退出的，定位残留 PIDs 并发送 SIGTERM/SIGKILL 强制终止
            let mut sys = sysinfo::System::new();
            sys.refresh_processes(sysinfo::ProcessesToUpdate::All);
            let target_bundle = file_path
                .as_ref()
                .map(|p| std::path::PathBuf::from(p))
                .and_then(|p| find_enclosing_app_bundle(&p))
                .map(|p| p.to_string_lossy().to_string().to_lowercase());

            let mut pids_to_kill = Vec::new();
            for (pid, proc_) in sys.processes() {
                let exe = proc_
                    .exe()
                    .map(|e| e.to_string_lossy().to_string().to_lowercase())
                    .unwrap_or_default();
                let is_claude = exe.contains("/applications/claude.app/")
                    || exe.contains("claude.app/contents/macos/")
                    || target_bundle.as_ref().map_or(false, |tb| exe.contains(tb));

                if is_claude {
                    pids_to_kill.push(*pid);
                }
            }

            for pid in pids_to_kill {
                let _ = std::process::Command::new("/bin/kill")
                    .args(["-9", &pid.to_string()])
                    .output();
            }

            std::thread::sleep(std::time::Duration::from_millis(500));
            Ok(true)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = file_path;
            Ok(true)
        }
    })
    .await
    .map_err(|e| format!("退出 Claude 失败: {}", e))?
}

/// 重新启动 Claude Desktop 客户端
#[tauri::command]
pub async fn launch_claude_desktop(file_path: Option<String>) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        #[cfg(target_os = "macos")]
        {
            let mut opened = false;
            if let Some(ref fp) = file_path {
                let path = std::path::PathBuf::from(fp);
                if let Some(bundle) = find_enclosing_app_bundle(&path) {
                    if bundle.exists() {
                        let _ = std::process::Command::new("/usr/bin/open")
                            .arg(&bundle)
                            .output();
                        opened = true;
                    }
                }
            }
            if !opened {
                let _ = std::process::Command::new("/usr/bin/open")
                    .args(["-a", "Claude"])
                    .output();
            }
            Ok(())
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = file_path;
            Ok(())
        }
    })
    .await
    .map_err(|e| format!("启动 Claude 失败: {}", e))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_claude_cowork_patch_lifecycle() {
        let sample_path = "/tmp/claude_versions/darwin-287/package/claude";
        if !std::path::Path::new(sample_path).exists() {
            println!("Sample not found, skipping: {}", sample_path);
            return;
        }

        // 1. 检查初始状态（应为未打补丁但可打补丁）
        let status = check_claude_cowork_patch(Some(sample_path.to_string()))
            .await
            .unwrap();
        assert!(status.is_patchable);

        // 2. 应用补丁
        let apply_res = apply_claude_cowork_patch(Some(sample_path.to_string()))
            .await
            .unwrap();
        assert!(apply_res.contains("成功") || apply_res.contains("已处于"));

        // 3. 再次检查状态（应为已打补丁）
        let status_patched = check_claude_cowork_patch(Some(sample_path.to_string()))
            .await
            .unwrap();
        assert!(status_patched.is_patched);

        // 4. 执行一键还原
        let revert_res = revert_claude_cowork_patch(Some(sample_path.to_string()))
            .await
            .unwrap();
        assert!(revert_res.contains("成功从备份还原"));

        // 5. 还原后检查状态（应回到未打补丁）
        let status_reverted = check_claude_cowork_patch(Some(sample_path.to_string()))
            .await
            .unwrap();
        assert!(!status_reverted.is_patched);
        assert!(status_reverted.is_patchable);
    }

    #[tokio::test]
    async fn test_antigravity_tools_claude_discovery_and_patching() {
        println!(
            "\n================================================================================"
        );
        println!("🚀 [Antigravity Tools 原生命令集成测试: 自动发现与沙盒多版本调度]");
        println!(
            "================================================================================"
        );

        // 1. 测试自动发现
        println!("🔍 步骤 1: 调用 list_claude_installations 探测本机已安装 Claude 实例...");
        let installs = list_claude_installations()
            .await
            .expect("Failed to list installations");
        println!("   └─ 发现系统实例总数: {}", installs.len());
        for (i, inst) in installs.iter().enumerate() {
            println!(
                "      [{}] 版本: {:<12} | 路径: {} (已打补丁: {}, 可打补丁: {})",
                i + 1,
                inst.version,
                inst.path,
                inst.is_patched,
                inst.is_patchable
            );
        }
        assert!(
            !installs.is_empty(),
            "必须能发现系统中已安装的 Claude 实例！"
        );

        // 2. 测试对沙盒中跨度达数十个版本的样本（远古 v2.1.110、早期 v2.1.160、最新 v2.1.287）执行集成修补
        let test_targets = [
            (
                "远古基线",
                "/Users/daniel/Documents/deep_compact_test_sandbox/versions/2.1.110/claude.app",
            ),
            (
                "早期演进",
                "/Users/daniel/Documents/deep_compact_test_sandbox/versions/2.1.160/claude.app",
            ),
            (
                "官方最新",
                "/Users/daniel/Documents/deep_compact_test_sandbox/versions/2.1.287/claude.app",
            ),
        ];

        for (stage, app_path) in test_targets {
            println!("\n--------------------------------------------------------------------------------");
            println!(
                "🎯 步骤 2: 针对【{}】沙盒实例进行端到端验证: {}",
                stage, app_path
            );

            // 2.1 检查状态 (验证对 .app 目录路径的自动内省支持)
            let status_init = check_claude_cowork_patch(Some(app_path.to_string()))
                .await
                .expect("Check failed");
            if status_init.is_patched {
                let _ = revert_claude_cowork_patch(Some(app_path.to_string())).await;
            }

            let status = check_claude_cowork_patch(Some(app_path.to_string()))
                .await
                .expect("Check failed");
            println!("   ├─ 路径解析:   {}", status.file_path);
            assert!(status.file_path.ends_with("Contents/MacOS/claude"));
            println!(
                "   ├─ 初始补丁状态: is_patched={}, is_patchable={}",
                status.is_patched, status.is_patchable
            );
            println!("   ├─ 状态描述:   {}", status.message);
            assert!(status.is_patchable, "目标版本必须可打补丁！");

            // 2.2 应用微创等长补丁
            let apply_res = apply_claude_cowork_patch(Some(app_path.to_string()))
                .await
                .expect("Apply failed");
            println!("   ├─ 应用补丁结果: {}", apply_res);

            // 2.3 验证打补丁后状态
            let status_after = check_claude_cowork_patch(Some(app_path.to_string()))
                .await
                .expect("Re-check failed");
            assert!(status_after.is_patched, "打完补丁后必须为已打补丁状态！");
            println!("   ├─ 验证状态变更: 成功识别为已打补丁 (return 0;)");

            // 2.4 一键安全还原
            let revert_res = revert_claude_cowork_patch(Some(app_path.to_string()))
                .await
                .expect("Revert failed");
            println!("   ├─ 一键还原结果: {}", revert_res);

            // 2.5 还原后再检查
            let status_restored = check_claude_cowork_patch(Some(app_path.to_string()))
                .await
                .expect("Restore-check failed");
            assert!(
                !status_restored.is_patched,
                "还原后必须恢复为未打补丁状态！"
            );
            assert!(status_restored.is_patchable, "还原后必须恢复为可打补丁！");
            println!("   └─ ✅ 端到端生命周期无损验证通过！");
        }
        println!(
            "\n================================================================================\n"
        );
    }

    #[test]
    fn test_patch_pattern_matches_8k_and_35k_and_origin() {
        let origin_code = b"function testFn(p1,p2,p3){let s=0,g=0;for(let h=p2-1;h>=0;h--)if(s+=p1[h],g++,s>=p3)break;if(g>=p2-1)return Math.max(1,Math.floor(p2/2));return g}";
        let patched_8k_code = b"function testFn(p1,p2,p3){let s=0,g=0;for(let h=p2-1;h>=0;h--)if(s+=p1[h],g++,s>=8000)break;return g;}/*                    */";
        let patched_35k_code = b"function testFn(p1,p2,p3){let s=0,g=0;for(let h=p2-1;h>=0;h--)if(s+=p1[h],g++,s>=35000)break;return g;}/*                   */";
        let patched_ret0_code = b"function testFn(p1,p2,p3){return 0;}/*                                                                                     */";

        let origin_re = regex::bytes::Regex::new(
            r"function\s+([a-zA-Z0-9_$]+)\(([a-zA-Z0-9_$]+),([a-zA-Z0-9_$]+),([a-zA-Z0-9_$]+)\)\{let\s+[a-zA-Z0-9_$]+=0,[a-zA-Z0-9_$]+=0;for\(let\s+[a-zA-Z0-9_$]+=[a-zA-Z0-9_$]+-1;[a-zA-Z0-9_$]+>=0;[a-zA-Z0-9_$]+--\)if\([a-zA-Z0-9_$]+\+=[a-zA-Z0-9_$]+\[[a-zA-Z0-9_$]+\],[a-zA-Z0-9_$]+\+\+,[a-zA-Z0-9_$]+>=[a-zA-Z0-9_$]+\)break;if\([a-zA-Z0-9_$]+>=[a-zA-Z0-9_$]+-1\)return\s+Math\.max\(1,Math\.floor\([a-zA-Z0-9_$]+/2\)\);return\s+[a-zA-Z0-9_$]+\}"
        ).unwrap();

        let patched_re = regex::bytes::Regex::new(
            r"function\s+[a-zA-Z0-9_$]+\([a-zA-Z0-9_$]+,[a-zA-Z0-9_$]+,[a-zA-Z0-9_$]+\)\{(return 0;|.*?s>=(8000|35000).*?\})"
        ).unwrap();

        assert!(origin_re.is_match(origin_code));
        assert!(!origin_re.is_match(patched_8k_code));

        assert!(patched_re.is_match(patched_8k_code));
        assert!(patched_re.is_match(patched_35k_code));
        assert!(patched_re.is_match(patched_ret0_code));
        assert!(!patched_re.is_match(origin_code));
    }
}
