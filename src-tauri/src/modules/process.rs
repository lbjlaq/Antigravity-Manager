use std::process::Command;
use std::thread;
use std::time::Duration;
use sysinfo::System;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

/// Get normalized path of the current running executable
fn get_current_exe_path() -> Option<std::path::PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.canonicalize().ok())
}

/// 判断字符串是否匹配 Antigravity IDE 特征（兼容空格、中划线、下划线、紧凑连写与点号变体）。
///
/// 铁律：只用**命名变体**判定 IDE 身份，绝不使用「同时出现 antigravity 与 vscode/code-oss」这类弱特征组合：
/// 否则任何在命令行里打开 antigravity 工程的 VS Code / 终端都会被误判为 IDE（issue #3762）。
pub fn is_antigravity_ide_str(s: &str) -> bool {
    let lower = s.to_lowercase();
    [
        "antigravity ide",
        "antigravity-ide",
        "antigravity_ide",
        "antigravityide",
        "antigravity.ide",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

/// 判断文件路径是否指向 Antigravity IDE 实例
pub fn is_antigravity_ide_path(p: &std::path::Path) -> bool {
    is_antigravity_ide_str(&p.to_string_lossy())
}

/// 客户端形态判定结果
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ClientKind {
    /// 本程序自身（Antigravity Manager / Tools）或同类工具
    Manager,
    /// Antigravity IDE（VS Code OSS 架构）
    Ide,
    /// Antigravity 经典原生桌面端
    Classic,
    /// 与 Antigravity 无关的进程
    Unrelated,
}

/// 从命令行中提取 Chromium 系 `--user-data-dir` 的值（入参已小写）。
/// 仅用于区分 IDE / 经典版：绝不把工作区、文件或日志路径当作身份特征。
///
/// 注意：`args_str` 由 `process.cmd()` 各参数用单空格 join 而成，原始引号已丢失，
/// 因此路径中的空格不能当作分隔符；取值以「下一个 flag（` --`）」或字符串末尾为界。
fn chromium_user_data_dir(args_str: &str) -> Option<&str> {
    const NEEDLE: &str = "user-data-dir";
    let pos = args_str.find(NEEDLE)?;
    let rest = args_str[pos + NEEDLE.len()..].trim_start_matches(['=', ':', ' ']);
    let rest = rest.trim_start_matches('"');
    let end = rest
        .find("\" --")
        .or_else(|| rest.find(" --"))
        .unwrap_or(rest.len());
    Some(rest[..end].trim_end_matches(['"', ' ']))
}

/// 唯一的进程身份判定入口（全部探测路径共享同一套语义）。
///
/// 设计铁律：
/// 1. **身份只来自内核报告的可执行文件名/路径**（`name` / `exe_path`）。命令行 `args` 永远不能
///    单独制造「这是 Antigravity」的身份 —— 否则任何在命令行里打开 antigravity 工程的编辑器、
///    终端或资源管理器都会被 `close_antigravity` 的 `taskkill /F /T` 误杀。
/// 2. `args` 只用作 **IDE / 经典版判别器**，且优先看 `--user-data-dir` 的值（权限受限或自定义
///    安装目录下的 2.6.0+ IDE 靠它区分），避免工作区路径误伤。
/// 3. manager / tools 自身必须在第 0 步排除。
pub fn classify_process(name: &str, exe_path: &str, argv0: &str, args_str: &str) -> ClientKind {
    if is_manager_or_tools_process(name, exe_path, argv0) {
        return ClientKind::Manager;
    }
    // 1. 身份判据：可执行文件名或内核报告的真实路径
    if !exe_path.contains("antigravity") && !name.contains("antigravity") {
        return ClientKind::Unrelated;
    }
    // 2. IDE / 经典判别：路径优先
    if is_antigravity_ide_str(exe_path) || is_antigravity_ide_str(name) {
        return ClientKind::Ide;
    }
    // 3. 权限受限或自定义安装目录：用 Chromium 家族用户数据目录兜底
    if chromium_user_data_dir(args_str)
        .map(is_antigravity_ide_str)
        .unwrap_or(false)
    {
        return ClientKind::Ide;
    }
    ClientKind::Classic
}

fn kind_matches_target(
    kind: ClientKind,
    target_ide: Option<&str>,
    exe_path: &str,
    ide_exe_paths: &std::collections::HashSet<String>,
) -> bool {
    match kind {
        ClientKind::Ide => target_ide == Some("ide"),
        // 显式配置/扫描到的 IDE 可执行文件绝不当作经典版
        ClientKind::Classic => target_ide != Some("ide") && !ide_exe_paths.contains(exe_path),
        ClientKind::Manager | ClientKind::Unrelated => false,
    }
}

/// Helper to determine if a process or binary is Antigravity Tools / Manager itself
/// (e.g. antigravity-tools, antigravity_tools, antigravity-manager, antigravity_manager, or Linux 15-char comm truncation like antigravity-too / antigravity_too).
pub fn is_manager_or_tools_process(name: &str, exe_path: &str, argv0: &str) -> bool {
    /// 显式产品名/可执行名（含连写、空格与 Linux comm 截断变体）。
    /// 这些模式足够独特，对整条路径判定成立（开发构建常在 `...\Antigravity-Manager\...` 下）。
    fn explicit(lower: &str) -> bool {
        const PATTERNS: [&str; 10] = [
            "antigravity-tools",
            "antigravity_tools",
            "antigravitytools",
            "antigravity tools",
            "antigravity-manager",
            "antigravity_manager",
            "antigravitymanager",
            "antigravity manager",
            "antigravity-too", // Linux TASK_COMM_LEN 16 (15 chars) comm truncation
            "antigravity_too",
        ];
        PATTERNS.iter().any(|pattern| lower.contains(pattern))
    }

    /// 宽松兜底：只认**文件名**（file stem），绝不看父目录。
    /// 否则装在 `D:\Tools\Antigravity\` 或 `C:\Tools\agy\` 的客户端会被误判为 Manager，
    /// 进而导致「客户端永远检测不到 → 切号只写凭据不重启 → 静默失败」。
    fn loose_file_stem(s: &str) -> bool {
        let last_segment = s.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(s);
        let stem = std::path::Path::new(last_segment)
            .file_stem()
            .map(|v| v.to_string_lossy().to_lowercase())
            .unwrap_or_else(|| last_segment.to_lowercase());
        (stem.contains("antigravity") || stem.contains("agy"))
            && (stem.contains("tool") || stem.contains("manager"))
    }

    let check = |s: &str| {
        let lower = s.to_lowercase();
        explicit(&lower) || loose_file_stem(&lower)
    };
    check(name) || check(exe_path) || check(argv0)
}

/// Helper to extract executable paths of Antigravity IDE instances
/// Uses config path as primary, and falls back to cmd() arg scanning (works on macOS/Linux)
fn get_ide_exe_paths(system: &System) -> std::collections::HashSet<String> {
    let mut immune_exe_paths = std::collections::HashSet::new();

    // Primary: load from explicit config setting (most reliable on Windows)
    if let Ok(config) = crate::modules::config::load_app_config() {
        if let Some(ide_path) = config.antigravity_ide_executable {
            if let Ok(canonical) = std::path::PathBuf::from(&ide_path).canonicalize() {
                immune_exe_paths.insert(canonical.to_string_lossy().to_lowercase());
            } else {
                immune_exe_paths.insert(ide_path.to_lowercase());
            }
        }
    }

    // Fallback: scan process cmd() args (works on macOS/Linux, may be empty on Windows)
    for (_pid, process) in system.processes() {
        let args = process.cmd();
        let args_str = args
            .iter()
            .map(|arg| arg.to_string_lossy().to_lowercase())
            .collect::<Vec<String>>()
            .join(" ");

        if is_antigravity_ide_str(&args_str) {
            if let Some(exe_path) = process.exe().and_then(|p| p.to_str()) {
                immune_exe_paths.insert(exe_path.to_lowercase());
            }
        }
    }
    immune_exe_paths
}

/// Check if a process with the given name is running (case-insensitive, strips .exe on Windows).
pub fn is_process_running_by_name(target_name: &str) -> bool {
    let mut system = System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All);
    let target_lower = target_name.to_lowercase();
    for (_pid, process) in system.processes() {
        let mut name = process.name().to_string_lossy().to_lowercase();
        if name.ends_with(".exe") {
            name.truncate(name.len() - 4);
        }
        if name == target_lower {
            return true;
        }
    }
    false
}

/// Helper process discriminator to filter out sub-processes, audio/gpu/renderers, crashpads, and language servers
pub(crate) fn is_helper_process(name: &str, args_str: &str, exe_path: &str) -> bool {
    let name_lower = name.to_lowercase();
    let args_lower = args_str.to_lowercase();
    let exe_lower = exe_path.to_lowercase();

    args_lower.contains("--type=")
        || args_lower.contains("node-ipc")
        || args_lower.contains("nodeipc")
        || args_lower.contains("max-old-space-size")
        || args_lower.contains("node_modules")
        || args_lower.contains("--standalone")
        || args_lower.contains("--subclient_type")
        || args_lower.contains("--override_ide_name")
        || name_lower.contains("helper")
        || name_lower.contains("plugin")
        || name_lower.contains("renderer")
        || name_lower.contains("gpu")
        || name_lower.contains("crashpad")
        || name_lower.contains("utility")
        || name_lower.contains("audio")
        || name_lower.contains("sandbox")
        || name_lower.contains("language_server")
        || args_lower.contains("language_server")
        || exe_lower.contains("crashpad")
        || exe_lower.contains("helper")
        || exe_lower.contains("language_server")
        || is_language_server_process(name, exe_path)
}

/// Sanitize restart arguments to prevent internal engine/language_server arguments
/// (such as --standalone or --override_ide_name) from leaking into IDE relaunch commands.
pub(crate) fn sanitize_restart_args(args: &[String]) -> Vec<String> {
    args.iter()
        .filter(|arg| {
            let lower = arg.trim().to_lowercase();
            !lower.is_empty()
                && !lower.starts_with("--standalone")
                && !lower.starts_with("--override_ide_name")
                && !lower.starts_with("--subclient_type")
                && !lower.contains("language_server")
        })
        .cloned()
        .collect()
}

/// Check if Antigravity is running
pub fn is_antigravity_running(target_ide: Option<&str>) -> bool {
    let mut system = System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All);
    let ide_exe_paths = get_ide_exe_paths(&system);

    let current_exe = get_current_exe_path();
    let current_pid = std::process::id();

    // Load both manual paths from config
    let config = crate::modules::config::load_app_config().ok();
    let manual_path = config
        .as_ref()
        .and_then(|c| c.antigravity_executable.as_ref())
        .and_then(|p| std::path::PathBuf::from(p).canonicalize().ok());
    let ide_manual_path = config
        .as_ref()
        .and_then(|c| c.antigravity_ide_executable.as_ref())
        .and_then(|p| std::path::PathBuf::from(p).canonicalize().ok());

    for (pid, process) in system.processes() {
        let pid_u32 = pid.as_u32();
        if pid_u32 == current_pid {
            continue;
        }

        let name = process.name().to_string_lossy().to_lowercase();
        let exe_path = process
            .exe()
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .to_lowercase();

        // Exclude own path (handles case where manager is mistaken for Antigravity on Linux)
        if let (Some(ref my_path), Some(p_exe)) = (&current_exe, process.exe()) {
            if let Ok(p_path) = p_exe.canonicalize() {
                if my_path == &p_path {
                    continue;
                }
            }
        }

        // Exclude manager / tools processes
        if is_manager_or_tools_process(&name, &exe_path, "") {
            continue;
        }

        // Common helper process exclusion logic
        let args = process.cmd();
        let args_str = args
            .iter()
            .map(|arg| arg.to_string_lossy().to_lowercase())
            .collect::<Vec<String>>()
            .join(" ");

        let is_helper = is_helper_process(&name, &args_str, &exe_path);

        if is_helper {
            continue;
        }

        // Recognition ref 2: If targeting IDE and ide_manual_path is configured, check it first
        if target_ide == Some("ide") {
            if let (Some(ref ide_m_path), Some(p_exe)) = (&ide_manual_path, process.exe()) {
                if let Ok(p_path) = p_exe.canonicalize() {
                    #[cfg(target_os = "macos")]
                    {
                        let m = ide_m_path.to_string_lossy();
                        let p = p_path.to_string_lossy();
                        if let (Some(mi), Some(pi)) = (m.find(".app"), p.find(".app")) {
                            if m[..mi + 4] == p[..pi + 4] {
                                return true;
                            }
                        }
                    }
                    #[cfg(not(target_os = "macos"))]
                    if ide_m_path == &p_path {
                        return true;
                    }
                }
            }
        }

        // Recognition ref 3: Priority check for manual path match (client)
        if target_ide != Some("ide") {
            if let (Some(ref m_path), Some(p_exe)) = (&manual_path, process.exe()) {
                if let Ok(p_path) = p_exe.canonicalize() {
                    // macOS: Check if within the same .app bundle
                    #[cfg(target_os = "macos")]
                    {
                        let m_path_str = m_path.to_string_lossy();
                        let p_path_str = p_path.to_string_lossy();
                        if let (Some(m_idx), Some(p_idx)) =
                            (m_path_str.find(".app"), p_path_str.find(".app"))
                        {
                            if m_path_str[..m_idx + 4] == p_path_str[..p_idx + 4] {
                                return true;
                            }
                        }
                    }

                    #[cfg(not(target_os = "macos"))]
                    if m_path == &p_path {
                        return true;
                    }
                }
            }
        }

        // 3. Strict mode: If the relevant manual path is configured, we strictly enforce it
        // and DO NOT fallback to fuzzy string matching.
        if manual_path.is_some() && target_ide != Some("ide") {
            continue;
        }
        if ide_manual_path.is_some() && target_ide == Some("ide") {
            continue;
        }

        // Check if the process matches target_ide
        let kind = classify_process(&name, &exe_path, "", &args_str);
        let is_ide_match = kind_matches_target(kind, target_ide, &exe_path, &ide_exe_paths);

        if is_ide_match {
            return true;
        }
    }

    false
}

#[cfg(target_os = "linux")]
/// Get PID set of current process and all ancestors.
/// Only ancestors (parents/grandparents) are excluded to prevent accidentally killing
/// the launcher or shell that started the Manager.
/// Child processes spawned by the Manager (e.g., the IDE) must remain killable, so
/// descendants are intentionally NOT included here.
fn get_self_family_pids(system: &sysinfo::System) -> std::collections::HashSet<u32> {
    let current_pid = std::process::id();
    let mut family_pids = std::collections::HashSet::new();
    family_pids.insert(current_pid);

    // Traverse upward to find all ancestors - prevent killing the launcher/shell
    let mut next_pid = current_pid;
    // Prevent infinite loop, max depth 10
    for _ in 0..10 {
        let pid_val = sysinfo::Pid::from_u32(next_pid);
        if let Some(process) = system.process(pid_val) {
            if let Some(parent) = process.parent() {
                let parent_id = parent.as_u32();
                // Avoid cycles or duplicates
                if !family_pids.insert(parent_id) {
                    break;
                }
                next_pid = parent_id;
            } else {
                break;
            }
        } else {
            break;
        }
    }

    family_pids
}

/// Get PIDs of all Antigravity processes (including main and helper processes)
fn get_antigravity_pids(target_ide: Option<&str>) -> Vec<u32> {
    let mut system = System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All);
    let ide_exe_paths = get_ide_exe_paths(&system);

    // Linux: Enable family process tree exclusion
    #[cfg(target_os = "linux")]
    let family_pids = get_self_family_pids(&system);

    let mut pids = Vec::new();
    let current_pid = std::process::id();
    let current_exe = get_current_exe_path();

    // Load both manual paths from config
    let config = crate::modules::config::load_app_config().ok();
    let manual_path = config
        .as_ref()
        .and_then(|c| c.antigravity_executable.as_ref())
        .and_then(|p| std::path::PathBuf::from(p).canonicalize().ok());
    let ide_manual_path = config
        .as_ref()
        .and_then(|c| c.antigravity_ide_executable.as_ref())
        .and_then(|p| std::path::PathBuf::from(p).canonicalize().ok());

    for (pid, process) in system.processes() {
        let pid_u32 = pid.as_u32();

        // Exclude own PID
        if pid_u32 == current_pid {
            continue;
        }

        // Exclude own executable path (hardened against broad name matching)
        if let (Some(ref my_path), Some(p_exe)) = (&current_exe, process.exe()) {
            if let Ok(p_path) = p_exe.canonicalize() {
                if my_path == &p_path {
                    continue;
                }
            }
        }

        let name = process.name().to_string_lossy().to_lowercase();
        let exe_path = process
            .exe()
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .to_lowercase();

        // Exclude manager / tools processes
        if is_manager_or_tools_process(&name, &exe_path, "") {
            continue;
        }

        #[cfg(target_os = "linux")]
        {
            // 1. Exclude family processes (self, children, parents)
            if family_pids.contains(&pid_u32) {
                continue;
            }
        }

        #[cfg(not(target_os = "linux"))]
        {
            // Other platforms: exclude only self
            if pid_u32 == current_pid {
                continue;
            }
        }

        // Recognition ref IDE manual path: If the process exactly matches the configured IDE path,
        // NEVER add it to kill list regardless of target_ide
        if let (Some(ref ide_m_path), Some(p_exe)) = (&ide_manual_path, process.exe()) {
            if let Ok(p_path) = p_exe.canonicalize() {
                #[cfg(target_os = "macos")]
                let matches = {
                    let m = ide_m_path.to_string_lossy();
                    let p = p_path.to_string_lossy();
                    matches!(m.find(".app").zip(p.find(".app")), Some((mi, pi)) if m[..mi + 4] == p[..pi + 4])
                };
                #[cfg(not(target_os = "macos"))]
                let matches = ide_m_path == &p_path;

                if matches && target_ide != Some("ide") {
                    // This is explicitly the IDE we must NOT kill when switching client
                    continue;
                }
                if matches && target_ide == Some("ide") {
                    // This is the IDE we WANT to close
                    pids.push(pid_u32);
                    continue;
                }
            }
        }

        // Recognition ref 3: Check manual config path match (client)
        if let (Some(ref m_path), Some(p_exe)) = (&manual_path, process.exe()) {
            if let Ok(p_path) = p_exe.canonicalize() {
                #[cfg(target_os = "macos")]
                let matches = {
                    let m_path_str = m_path.to_string_lossy();
                    let p_path_str = p_path.to_string_lossy();
                    matches!(m_path_str.find(".app").zip(p_path_str.find(".app")), Some((m_idx, p_idx)) if m_path_str[..m_idx + 4] == p_path_str[..p_idx + 4])
                };
                #[cfg(not(target_os = "macos"))]
                let matches = m_path == &p_path;

                if matches {
                    #[cfg(target_os = "macos")]
                    let is_main = {
                        let args = process.cmd();
                        let is_helper_by_args = args
                            .iter()
                            .any(|arg| arg.to_string_lossy().contains("--type="));
                        let is_helper_by_name = name.contains("helper")
                            || name.contains("plugin")
                            || name.contains("renderer")
                            || name.contains("gpu")
                            || name.contains("crashpad")
                            || name.contains("utility")
                            || name.contains("audio")
                            || name.contains("sandbox");
                        !is_helper_by_args && !is_helper_by_name
                    };
                    #[cfg(not(target_os = "macos"))]
                    let is_main = true;

                    if is_main {
                        if target_ide == Some("ide") {
                            // This is explicitly the client we must NOT kill when switching IDE
                            continue;
                        } else {
                            // This is the client we WANT to close
                            pids.push(pid_u32);
                            continue;
                        }
                    }
                }
            }
        }

        // 4. Strict mode: If the relevant manual path is configured, we strictly enforce it
        // and DO NOT fallback to fuzzy string matching.
        if manual_path.is_some() && target_ide != Some("ide") {
            continue;
        }
        if ide_manual_path.is_some() && target_ide == Some("ide") {
            continue;
        }

        // Get executable path
        let exe_path = process
            .exe()
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .to_lowercase();

        // Common helper process exclusion logic
        let args = process.cmd();
        let args_str = args
            .iter()
            .map(|arg| arg.to_string_lossy().to_lowercase())
            .collect::<Vec<String>>()
            .join(" ");

        let is_helper = is_helper_process(&name, &args_str, &exe_path);

        // Check if the process matches target_ide
        let kind = classify_process(&name, &exe_path, "", &args_str);
        let is_ide_match = kind_matches_target(kind, target_ide, &exe_path, &ide_exe_paths);

        if is_ide_match && !is_helper {
            pids.push(pid_u32);
        }
    }

    if !pids.is_empty() {
        crate::modules::logger::log_info(&format!(
            "Found {} Antigravity ({:?}) processes: {:?}",
            pids.len(),
            target_ide,
            pids
        ));
    }

    pids
}

/// 额外清理：终止 `target_ide` 对应的孤儿 `language_server` 进程，防止端口/互斥锁占有阻塞重启。
///
/// 隔离铁律：先按客户端形态分类再杀 —— IDE 与经典版各自独立，绝不互相误杀，
/// 也不会误伤名称含 `language_server` 的无关进程（可执行文件必须属于 Antigravity 安装）。
pub fn sweep_orphan_language_servers(target_ide: Option<&str>) {
    let mut system = System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All);

    for (pid, process) in system.processes() {
        let name = process.name().to_string_lossy().to_lowercase();
        let exe_path = process
            .exe()
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .to_lowercase();
        let args_str = process
            .cmd()
            .iter()
            .map(|arg| arg.to_string_lossy().to_lowercase())
            .collect::<Vec<String>>()
            .join(" ");

        if is_manager_or_tools_process(&name, &exe_path, "") {
            continue;
        }

        if is_language_server_process(&name, &exe_path)
            && matches_target_client(target_ide, &name, &exe_path, "", &args_str)
        {
            let pid_u32 = pid.as_u32();
            crate::modules::logger::log_info(&format!(
                "Sweeping orphan language_server process for {:?} (PID: {}, Path: {})",
                target_ide, pid_u32, exe_path
            ));
            force_kill_pid(pid_u32);
        }
    }
}

/// `language_server` 子进程判定。
///
/// 覆盖面：Windows/Linux 为 `language_server` / `language_server.exe` / `agy` / `agy.exe`，
/// macOS 为 `language_server_macos` / `language_server_macos_arm` / `agy`。
///
/// 注意：这是**窄**判定（仅语言服务），与 [`is_helper_process`] 的"广义 helper"判定区分开 ——
/// 热切号只允许终止语言服务，绝不触碰 renderer / gpu / crashpad 等其它 helper。
pub(crate) fn is_language_server_process(name: &str, exe_path: &str) -> bool {
    let name_lower = name.to_lowercase();
    let exe_lower = exe_path.to_lowercase();
    name_lower.contains("language_server")
        || exe_lower.contains("language_server")
        || name_lower == "agy"
        || name_lower == "agy.exe"
        || exe_lower.ends_with("agy.exe")
        || exe_lower.ends_with("/agy")
}

/// 强制终止单个进程（Windows 用 `taskkill /F`，其余平台用 `kill -9`）。
fn force_kill_pid(pid: u32) {
    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .creation_flags(0x08000000)
            .output();
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = Command::new("kill").args(["-9", &pid.to_string()]).output();
    }
}

/// 收集 `target_ide` 对应 Antigravity 的 `language_server` 子进程 PID（issue #3503 方案 A 的定位器）。
///
/// 定位策略 = 「主进程 → 后代 → 名字/路径命中 language_server」，而**不是** issue 建议的
/// `exe.contains("antigravity")` 路径模糊匹配。原因：
/// 1. IDE 形态下语言服务位于 `Antigravity IDE` 安装根（含空格，且可能在用户自定义目录），
///    纯路径匹配会漏杀 → 表现是"切号看似成功、实际仍是旧账号"这种最危险的静默失败；
/// 2. 以**主进程为根**展开后代，天然按 target 隔离 —— IDE 与经典版各自独立，不会互相误杀，
///    也不会误伤其它 IDE / 语言服务器进程。
///
/// `get_antigravity_pids` 同时返回主进程与 helper，故这里先用 [`is_helper_process`] 过滤出
/// **非 helper 的主进程**作为根：若把 helper 也当根，`language_server` 自身会变成"根"而不是
/// "后代"，就永远定位不到了。
fn language_server_subprocess_pids(system: &System, target_ide: Option<&str>) -> Vec<u32> {
    let roots: Vec<u32> = get_antigravity_pids(target_ide)
        .into_iter()
        .filter(|pid| {
            system
                .process(sysinfo::Pid::from_u32(*pid))
                .map(|process| {
                    let name = process.name().to_string_lossy().to_string();
                    let args = process
                        .cmd()
                        .iter()
                        .map(|s| s.to_string_lossy().to_string())
                        .collect::<Vec<_>>()
                        .join(" ");
                    let exe = process
                        .exe()
                        .map(|e| e.to_string_lossy().to_string())
                        .unwrap_or_default();
                    !is_helper_process(&name, &args, &exe)
                })
                .unwrap_or(false)
        })
        .collect();

    if roots.is_empty() {
        return Vec::new();
    }

    // 广度优先展开全部后代（Antigravity.exe → … → language_server.exe，可能多层嵌套）
    let mut descendants: Vec<u32> = Vec::new();
    let mut frontier: Vec<u32> = roots.clone();
    let mut visited: std::collections::HashSet<u32> = roots.into_iter().collect();

    while let Some(parent) = frontier.pop() {
        for (pid, process) in system.processes() {
            if process.parent().map(|p| p.as_u32()) != Some(parent) {
                continue;
            }
            let pid_u32 = pid.as_u32();
            if visited.insert(pid_u32) {
                descendants.push(pid_u32);
                frontier.push(pid_u32);
            }
        }
    }

    descendants
        .into_iter()
        .filter(|pid_u32| {
            system
                .process(sysinfo::Pid::from_u32(*pid_u32))
                .map(|process| {
                    let name = process.name().to_string_lossy().to_string();
                    let exe = process
                        .exe()
                        .map(|e| e.to_string_lossy().to_string())
                        .unwrap_or_default();
                    is_language_server_process(&name, &exe)
                })
                .unwrap_or(false)
        })
        .collect()
}

/// 热切号：仅终止 `target_ide` 对应 Antigravity 主进程**后代**中的 `language_server` 子进程，
/// 保留主窗口进程。
///
/// 机理（issue #3503 方案 A）：VS Code/Electron 内核自带 supervisor，主窗口检测到语言服务断开后
/// 会在约 2 秒内原地重新拉起子进程并重载 Webview，重新读取最新凭据 —— 因此切号时用户的
/// 未保存缓冲区、终端任务、断点、文件树全部保留，不再被强杀主进程打断。
///
/// 返回实际终止的进程数。**`Ok(0)` 表示未定位到子进程，调用方必须回退到完整重启**
/// （`close_antigravity`），否则会出现"凭据已写入、IDE 仍在使用旧账号"的不一致状态 ——
/// 那比强制重启严重得多。
pub fn kill_language_server_subprocesses(target_ide: Option<&str>) -> Result<usize, String> {
    let mut system = System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All);

    let pids = language_server_subprocess_pids(&system, target_ide);
    for pid_u32 in &pids {
        crate::modules::logger::log_info(&format!(
            "[HotSwitch] Terminating language_server subprocess (PID: {}, target: {:?})",
            pid_u32, target_ide
        ));
        force_kill_pid(*pid_u32);
    }

    Ok(pids.len())
}

/// 等待 `target_ide` 的 `language_server` 子进程被 supervisor 重新拉起（有界轮询）。
///
/// 用于热切号后的确认：只有子进程确实回来了，才说明主窗口仍具备 AI 能力、可以跳过完整重启；
/// 超时未恢复则调用方应降级为完整重启，避免留下"窗口活着但引擎已死"的残状态。
pub fn wait_for_language_server_respawn(target_ide: Option<&str>, timeout_secs: u64) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);

    while std::time::Instant::now() < deadline {
        thread::sleep(Duration::from_millis(500));

        let mut system = System::new();
        system.refresh_processes(sysinfo::ProcessesToUpdate::All);
        if !language_server_subprocess_pids(&system, target_ide).is_empty() {
            return true;
        }
    }

    false
}

/// Close Antigravity processes
// timeout_secs 仅用于 macos/Linux 分支（graceful_timeout），Windows 分支不使用参数
#[cfg_attr(target_os = "windows", allow(unused_variables))]
pub fn close_antigravity(timeout_secs: u64, target_ide: Option<&str>) -> Result<(), String> {
    crate::modules::logger::log_info(&format!("Closing Antigravity ({:?})...", target_ide));

    #[cfg(target_os = "windows")]
    {
        // Windows: Precise kill by PID to support multiple versions or custom filenames
        let pids = get_antigravity_pids(target_ide);
        if !pids.is_empty() {
            crate::modules::logger::log_info(&format!(
                "Precisely closing {} identified processes on Windows (taskkill /F /T)...",
                pids.len()
            ));
            for pid in pids {
                let _ = Command::new("taskkill")
                    .args(["/F", "/T", "/PID", &pid.to_string()])
                    .creation_flags(0x08000000) // CREATE_NO_WINDOW
                    .output();
            }
            thread::sleep(Duration::from_millis(300));
        }

        // Extra cleanup: 终止目标客户端对应的孤儿 language_server，防止端口/互斥锁占有阻塞重启。
        sweep_orphan_language_servers(target_ide);
    }

    #[cfg(target_os = "macos")]
    {
        // macOS: Optimize closing strategy to avoid "Window terminated unexpectedly" popups
        // Strategy: SEND SIGTERM to main process only, let it coordinate closing children

        let pids = get_antigravity_pids(target_ide);
        if !pids.is_empty() {
            // 1. Identify main process (PID)
            let mut system = System::new();
            system.refresh_processes(sysinfo::ProcessesToUpdate::All);

            let mut main_pid = None;

            // Load manual configuration path as highest priority reference
            let manual_path = crate::modules::config::load_app_config()
                .ok()
                .and_then(|c| c.antigravity_executable)
                .and_then(|p| std::path::PathBuf::from(p).canonicalize().ok());

            crate::modules::logger::log_info("Analyzing process list to identify main process:");
            for pid_u32 in &pids {
                let pid = sysinfo::Pid::from_u32(*pid_u32);
                if let Some(process) = system.process(pid) {
                    let name = process.name().to_string_lossy();
                    let args = process.cmd();
                    let args_str = args
                        .iter()
                        .map(|arg| arg.to_string_lossy().into_owned())
                        .collect::<Vec<String>>()
                        .join(" ");

                    crate::modules::logger::log_info(&format!(
                        " - PID: {} | Name: {} | Args: {}",
                        pid_u32, name, args_str
                    ));

                    // 1. Priority to manual path matching
                    if let (Some(ref m_path), Some(p_exe)) = (&manual_path, process.exe()) {
                        if let Ok(p_path) = p_exe.canonicalize() {
                            let m_path_str = m_path.to_string_lossy();
                            let p_path_str = p_path.to_string_lossy();
                            if let (Some(m_idx), Some(p_idx)) =
                                (m_path_str.find(".app"), p_path_str.find(".app"))
                            {
                                if m_path_str[..m_idx + 4] == p_path_str[..p_idx + 4] {
                                    // Deep validation: even if path matches, must exclude Helper keywords and arguments
                                    let is_helper_by_args = args_str.contains("--type=");
                                    let is_helper_by_name = name.to_lowercase().contains("helper")
                                        || name.to_lowercase().contains("plugin")
                                        || name.to_lowercase().contains("renderer")
                                        || name.to_lowercase().contains("gpu")
                                        || name.to_lowercase().contains("crashpad")
                                        || name.to_lowercase().contains("utility")
                                        || name.to_lowercase().contains("audio")
                                        || name.to_lowercase().contains("sandbox")
                                        || name.to_lowercase().contains("language_server");

                                    if !is_helper_by_args && !is_helper_by_name {
                                        main_pid = Some(pid_u32);
                                        crate::modules::logger::log_info(&format!(
                                            "   => Identified as main process (manual path match)"
                                        ));
                                        break;
                                    }
                                }
                            }
                        }
                    }

                    // 2. Feature analysis matching (fallback)
                    let is_helper_by_name = name.to_lowercase().contains("helper")
                        || name.to_lowercase().contains("crashpad")
                        || name.to_lowercase().contains("utility")
                        || name.to_lowercase().contains("audio")
                        || name.to_lowercase().contains("sandbox")
                        || name.to_lowercase().contains("language_server")
                        || name.to_lowercase().contains("plugin")
                        || name.to_lowercase().contains("renderer");

                    let is_helper_by_args = args_str.contains("--type=");

                    if !is_helper_by_name && !is_helper_by_args {
                        if main_pid.is_none() {
                            main_pid = Some(pid_u32);
                            crate::modules::logger::log_info(&format!(
                                "   => Identified as main process (Name/Args analysis)"
                            ));
                        }
                    } else {
                        crate::modules::logger::log_info(&format!(
                            "   => Identified as helper process (Helper/Args)"
                        ));
                    }
                }
            }

            // Phase 1: Graceful exit (SIGTERM)
            if let Some(pid) = main_pid {
                crate::modules::logger::log_info(&format!(
                    "Sending SIGTERM to main process PID: {}",
                    pid
                ));
                let _ = Command::new("kill")
                    .args(["-15", &pid.to_string()])
                    .output();
            } else {
                crate::modules::logger::log_warn(
                    "No main process identified, sending SIGTERM to all associated processes",
                );
                for pid in &pids {
                    let _ = Command::new("kill")
                        .args(["-15", &pid.to_string()])
                        .output();
                }
            }

            // Wait for graceful exit (max 70% of timeout_secs)
            let graceful_timeout = (timeout_secs * 7) / 10;
            let start = std::time::Instant::now();
            while start.elapsed() < Duration::from_secs(graceful_timeout) {
                if !is_antigravity_running(target_ide) {
                    sweep_orphan_language_servers(target_ide);
                    crate::modules::logger::log_info("All Antigravity processes gracefully closed");
                    return Ok(());
                }
                thread::sleep(Duration::from_millis(500));
            }

            // Phase 2: Force kill (SIGKILL) - targeting all remaining processes (Helpers)
            if is_antigravity_running(target_ide) {
                let remaining_pids = get_antigravity_pids(target_ide);
                if !remaining_pids.is_empty() {
                    crate::modules::logger::log_warn(&format!(
                        "Graceful exit timeout, force killing {} remaining processes (SIGKILL)",
                        remaining_pids.len()
                    ));
                    for pid in &remaining_pids {
                        let output = Command::new("kill").args(["-9", &pid.to_string()]).output();

                        if let Ok(result) = output {
                            if !result.status.success() {
                                let error = String::from_utf8_lossy(&result.stderr);
                                if !error.contains("No such process") {
                                    // "No matching processes" for killall, "No such process" for kill
                                    crate::modules::logger::log_error(&format!(
                                        "SIGKILL process {} failed: {}",
                                        pid, error
                                    ));
                                }
                            }
                        }
                    }
                    thread::sleep(Duration::from_secs(1));
                }

                // Final check
                if !is_antigravity_running(target_ide) {
                    sweep_orphan_language_servers(target_ide);
                    crate::modules::logger::log_info("All processes exited after forced cleanup");
                    return Ok(());
                }
            } else {
                sweep_orphan_language_servers(target_ide);
                crate::modules::logger::log_info("All processes exited after SIGTERM");
                return Ok(());
            }
        } else {
            // Only consider not running when pids is empty, don't error here as it might already be closed
            sweep_orphan_language_servers(target_ide);
            crate::modules::logger::log_info("Antigravity not running, no need to close");
            return Ok(());
        }
    }

    #[cfg(target_os = "linux")]
    {
        // Linux: precise closing
        let pids = get_antigravity_pids(target_ide);
        if !pids.is_empty() {
            let mut system = System::new();
            system.refresh_processes(sysinfo::ProcessesToUpdate::All);

            let mut main_pid = None;

            for pid_u32 in &pids {
                let pid = sysinfo::Pid::from_u32(*pid_u32);
                if let Some(process) = system.process(pid) {
                    let name = process.name().to_string_lossy();
                    let args = process.cmd();
                    let args_str = args
                        .iter()
                        .map(|arg| arg.to_string_lossy().into_owned())
                        .collect::<Vec<String>>()
                        .join(" ");

                    let is_helper_by_name = name.to_lowercase().contains("helper")
                        || name.to_lowercase().contains("crashpad")
                        || name.to_lowercase().contains("utility")
                        || name.to_lowercase().contains("audio")
                        || name.to_lowercase().contains("sandbox")
                        || name.to_lowercase().contains("plugin")
                        || name.to_lowercase().contains("renderer");

                    let is_helper_by_args = args_str.contains("--type=");

                    if !is_helper_by_name && !is_helper_by_args {
                        main_pid = Some(pid_u32);
                        break;
                    }
                }
            }

            // Phase 1: SIGTERM
            if let Some(pid) = main_pid {
                let _ = Command::new("kill")
                    .args(["-15", &pid.to_string()])
                    .output();
            } else {
                crate::modules::logger::log_warn(
                    "No clear Linux main process identified, sending SIGTERM to all associated processes",
                );
                for pid in &pids {
                    let _ = Command::new("kill")
                        .args(["-15", &pid.to_string()])
                        .output();
                }
            }

            // Wait for graceful exit
            let graceful_timeout = (timeout_secs * 7) / 10;
            let start = std::time::Instant::now();
            while start.elapsed() < Duration::from_secs(graceful_timeout) {
                if !is_antigravity_running(target_ide) {
                    sweep_orphan_language_servers(target_ide);
                    crate::modules::logger::log_info("Antigravity gracefully closed");
                    return Ok(());
                }
                thread::sleep(Duration::from_millis(500));
            }

            // Phase 2: SIGKILL
            if is_antigravity_running(target_ide) {
                let remaining_pids = get_antigravity_pids(target_ide);
                if !remaining_pids.is_empty() {
                    crate::modules::logger::log_warn(&format!(
                        "Graceful exit timeout, force killing {} remaining processes (SIGKILL)",
                        remaining_pids.len()
                    ));
                    for pid in &remaining_pids {
                        let _ = Command::new("kill").args(["-9", &pid.to_string()]).output();
                    }
                    thread::sleep(Duration::from_secs(1));
                }
            }
        } else {
            sweep_orphan_language_servers(target_ide);
            crate::modules::logger::log_info(
                "No Antigravity processes found to close (possibly filtered or not running)",
            );
        }
    }

    // Final check with polling retry window (max 3 seconds, 150ms interval) to tolerate OS cleanup latency
    let final_check_start = std::time::Instant::now();
    let final_check_timeout = Duration::from_secs(3);

    while final_check_start.elapsed() < final_check_timeout {
        if !is_antigravity_running(target_ide) {
            sweep_orphan_language_servers(target_ide);
            crate::modules::logger::log_info("Antigravity closed successfully");
            return Ok(());
        }
        thread::sleep(Duration::from_millis(150));
    }

    // If still running after 3 seconds, perform one last sweep kill on all remaining PIDs
    let remaining_pids = get_antigravity_pids(target_ide);
    if !remaining_pids.is_empty() {
        crate::modules::logger::log_warn(&format!(
            "Still running after timeout, attempting final sweep kill on PIDs: {:?}",
            remaining_pids
        ));
        for pid in &remaining_pids {
            #[cfg(target_os = "windows")]
            let _ = Command::new("taskkill")
                .args(["/F", "/T", "/PID", &pid.to_string()])
                .creation_flags(0x08000000)
                .output();

            #[cfg(not(target_os = "windows"))]
            let _ = Command::new("kill").args(["-9", &pid.to_string()]).output();
        }
        thread::sleep(Duration::from_millis(300));
    }

    if is_antigravity_running(target_ide) {
        return Err(
            "Unable to close Antigravity process, please close manually and retry".to_string(),
        );
    }

    sweep_orphan_language_servers(target_ide);
    crate::modules::logger::log_info("Antigravity closed successfully");
    Ok(())
}

/// Clean AppImage-specific environment variables before spawning external processes on Linux
#[cfg(target_os = "linux")]
pub fn clean_appimage_env(cmd: &mut Command) {
    let appimage_vars = [
        "APPIMAGE",
        "APPDIR",
        "ARGV0",
        "OWD",
        "LD_LIBRARY_PATH",
        "LD_PRELOAD",
        "GTK_PATH",
        "GIO_EXTRA_MODULES",
        "GI_TYPELIB_PATH",
        "QT_PLUGIN_PATH",
        "QT_QPA_PLATFORM_PLUGIN_PATH",
    ];
    for var in &appimage_vars {
        cmd.env_remove(var);
    }

    if let Ok(xdg_data_dirs) = std::env::var("XDG_DATA_DIRS") {
        let filtered_dirs: Vec<&str> = xdg_data_dirs
            .split(':')
            .filter(|dir| !dir.starts_with("/tmp/.mount_"))
            .collect();
        cmd.env("XDG_DATA_DIRS", filtered_dirs.join(":"));
    }
}

/// True when startup failed because no Antigravity client binary or app bundle is installed.
pub fn is_client_executable_missing(err: &str) -> bool {
    let lower = err.to_ascii_lowercase();
    lower.contains("executable not found")
        || lower.contains("unable to find application")
        || lower.contains("no application knows how to open")
        || lower.contains("application not found")
}

/// Start Antigravity with optional snapshot path & args fallback
#[allow(unused_mut)]
pub fn start_antigravity_with_fallback_path(
    target_ide: Option<&str>,
    preferred_path: Option<&std::path::Path>,
    preferred_args: Option<&[String]>,
) -> Result<(), String> {
    crate::modules::logger::log_info(&format!(
        "Starting Antigravity ({:?}, preferred_path: {:?})...",
        target_ide, preferred_path
    ));

    // Prefer manually specified path and args from configuration
    let config = crate::modules::config::load_app_config().ok();
    let manual_path = if target_ide == Some("ide") {
        config
            .as_ref()
            .and_then(|c| c.antigravity_ide_executable.clone())
    } else {
        config
            .as_ref()
            .and_then(|c| c.antigravity_executable.clone())
    };
    let raw_args = config
        .and_then(|c| c.antigravity_args.clone())
        .or_else(|| preferred_args.map(|a| a.to_vec()));
    let args = raw_args.map(|a| sanitize_restart_args(&a));

    if let Some(mut path_str) = manual_path {
        let mut path = std::path::PathBuf::from(&path_str);

        #[cfg(target_os = "macos")]
        {
            // Fault tolerance: If path is inside .app bundle (e.g. misselected Helper), auto-correct to .app directory
            if let Some(app_idx) = path_str.find(".app") {
                let corrected_app = &path_str[..app_idx + 4];
                if corrected_app != path_str {
                    crate::modules::logger::log_info(&format!(
                        "Detected macOS path inside .app bundle, auto-correcting to: {}",
                        corrected_app
                    ));
                    path_str = corrected_app.to_string();
                    path = std::path::PathBuf::from(&path_str);
                }
            }
        }

        if path.exists() {
            crate::modules::logger::log_info(&format!(
                "Starting with manual configuration path: {}",
                path_str
            ));

            #[cfg(target_os = "macos")]
            {
                // macOS: if .app directory, use open
                if path_str.ends_with(".app") || path.is_dir() {
                    let mut cmd = Command::new("open");
                    cmd.arg("-a").arg(&path_str);

                    // Add startup arguments (must be after --args for macOS open)
                    if let Some(ref args) = args {
                        let valid_args: Vec<_> =
                            args.iter().filter(|a| !a.trim().is_empty()).collect();
                        if !valid_args.is_empty() {
                            cmd.arg("--args");
                            for arg in valid_args {
                                cmd.arg(arg);
                            }
                        }
                    }

                    cmd.spawn()
                        .map_err(|e| format!("Startup failed (open): {}", e))?;
                } else {
                    let mut cmd = Command::new(&path_str);

                    // Add startup arguments
                    if let Some(ref args) = args {
                        for arg in args {
                            cmd.arg(arg);
                        }
                    }

                    cmd.spawn()
                        .map_err(|e| format!("Startup failed (direct): {}", e))?;
                }
            }

            #[cfg(not(target_os = "macos"))]
            {
                let mut cmd = Command::new(&path_str);

                if let Some(parent) = path.parent() {
                    cmd.current_dir(parent);
                }

                // Add startup arguments
                if let Some(ref args) = args {
                    for arg in args {
                        cmd.arg(arg);
                    }
                }

                #[cfg(target_os = "linux")]
                clean_appimage_env(&mut cmd);

                cmd.spawn().map_err(|e| format!("Startup failed: {}", e))?;
            }

            crate::modules::logger::log_info(&format!(
                "Antigravity startup command sent (manual path: {}, args: {:?})",
                path_str, args
            ));
            return Ok(());
        } else {
            crate::modules::logger::log_warn(&format!(
                "Manual configuration path does not exist: {}, falling back to auto-detection",
                path_str
            ));
        }
    }

    // 次优：如果切换前捕获到了运行中进程的真实有效路径，优先使用它以防止非标准安装路径丢失
    if let Some(pref_path) = preferred_path {
        if pref_path.exists() {
            crate::modules::logger::log_info(&format!(
                "Starting with preferred snapshot process path: {:?}",
                pref_path
            ));

            #[cfg(target_os = "macos")]
            {
                let path_str = pref_path.to_string_lossy();
                let mut cmd = Command::new("open");
                if let Some(app_idx) = path_str.find(".app") {
                    cmd.arg("-a").arg(&path_str[..app_idx + 4]);
                } else {
                    cmd.arg("-a").arg(&*path_str);
                }
                if let Some(ref args) = args {
                    let valid_args: Vec<_> = args.iter().filter(|a| !a.trim().is_empty()).collect();
                    if !valid_args.is_empty() {
                        cmd.arg("--args");
                        for arg in valid_args {
                            cmd.arg(arg);
                        }
                    }
                }
                let output = cmd
                    .output()
                    .map_err(|e| format!("Execute open command failed: {}", e))?;
                if !output.status.success() {
                    let err_msg = String::from_utf8_lossy(&output.stderr);
                    return Err(format!("Startup failed: {}", err_msg.trim()));
                }
                crate::modules::logger::log_info(
                    "Antigravity startup command sent (macOS open snapshot path)",
                );
                return Ok(());
            }

            #[cfg(not(target_os = "macos"))]
            {
                let mut cmd = Command::new(pref_path);

                if let Some(parent) = pref_path.parent() {
                    cmd.current_dir(parent);
                }

                if let Some(ref args) = args {
                    for arg in args {
                        cmd.arg(arg);
                    }
                }
                #[cfg(target_os = "linux")]
                clean_appimage_env(&mut cmd);

                cmd.spawn().map_err(|e| {
                    format!(
                        "Startup failed (preferred snapshot path {:?}): {}",
                        pref_path, e
                    )
                })?;
                crate::modules::logger::log_info(&format!(
                    "Antigravity startup command sent (snapshot path: {:?})",
                    pref_path
                ));
                return Ok(());
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        // Improvement: Use output() to wait for open command completion and capture "app not found" error
        let detected = get_antigravity_executable_path(target_ide);

        let app_target = if let Some(ref d) = detected {
            d.to_string_lossy().to_string()
        } else if target_ide == Some("ide") {
            "Antigravity IDE".to_string()
        } else {
            "Antigravity".to_string()
        };

        let mut cmd = Command::new("open");
        if app_target.ends_with(".app") {
            cmd.arg(&app_target);
        } else {
            cmd.args(["-a", &app_target]);
        }

        // Add startup arguments (must be after --args for macOS open)
        if let Some(ref args) = args {
            let valid_args: Vec<_> = args.iter().filter(|a| !a.trim().is_empty()).collect();
            if !valid_args.is_empty() {
                cmd.arg("--args");
                for arg in valid_args {
                    cmd.arg(arg);
                }
            }
        }

        let output = cmd
            .output()
            .map_err(|e| format!("Execute open command failed: {}", e))?;
        if !output.status.success() {
            let err_msg = String::from_utf8_lossy(&output.stderr);
            let err_msg = err_msg.trim();
            if is_client_executable_missing(err_msg) {
                return Err("Unable to start Antigravity: executable not found".to_string());
            }
            return Err(format!("Unable to start Antigravity: {}", err_msg));
        }

        crate::modules::logger::log_info("Antigravity startup command sent (macOS open)");
        return Ok(());
    }

    #[cfg(not(target_os = "macos"))]
    {
        // Windows/Linux Auto-detection and Startup
        let detected = get_antigravity_executable_path(target_ide);

        if let Some(detected_path) = detected {
            let mut cmd = Command::new(&detected_path);

            if let Some(parent) = detected_path.parent() {
                cmd.current_dir(parent);
            }

            // Add startup arguments
            if let Some(ref args) = args {
                for arg in args {
                    cmd.arg(arg);
                }
            }

            #[cfg(target_os = "linux")]
            clean_appimage_env(&mut cmd);

            cmd.spawn().map_err(|e| {
                format!("Startup failed (detected path {:?}): {}", detected_path, e)
            })?;

            crate::modules::logger::log_info(&format!(
                "Antigravity startup command sent (detected path: {:?})",
                detected_path
            ));
            Ok(())
        } else {
            Err("Unable to start Antigravity: executable not found".to_string())
        }
    }
}

/// Start Antigravity (wrapper using default discovery)
pub fn start_antigravity(target_ide: Option<&str>) -> Result<(), String> {
    start_antigravity_with_fallback_path(target_ide, None, None)
}

/// 判断进程特征是否属于目标客户端：IDE 需命中 IDE 命名；
/// 经典版需含 antigravity 且所有特征都不带 IDE 命名，避免 IDE 实例被误判为经典版。
/// 可执行文件名/路径是身份的唯一来源，命令行仅作为 IDE / 经典判别器。
fn matches_target_client(
    target_ide: Option<&str>,
    name: &str,
    exe_path: &str,
    argv0: &str,
    args_str: &str,
) -> bool {
    if is_language_server_process(name, exe_path) {
        let name_lower = name.to_lowercase();
        let exe_lower = exe_path.to_lowercase();
        let is_agy = name_lower == "agy"
            || name_lower == "agy.exe"
            || exe_lower.ends_with("/agy")
            || exe_lower.ends_with("\\agy")
            || exe_lower.ends_with("/agy.exe")
            || exe_lower.ends_with("\\agy.exe");

        if is_agy {
            // agy 作为语言服务 hub 仅由 Antigravity IDE (2.6.0+) 启动（带 --hub 或 --app_data_dir=antigravity）
            if args_str.contains("--hub") || args_str.contains("antigravity") {
                return target_ide == Some("ide");
            }
        }
    }

    match classify_process(name, exe_path, argv0, args_str) {
        ClientKind::Manager | ClientKind::Unrelated => false,
        ClientKind::Ide => target_ide == Some("ide"),
        ClientKind::Classic => target_ide != Some("ide"),
    }
}

/// 沿路径组件向上定位最外层 `.app` 包目录，按组件而非字节偏移切分，兼容非 ASCII 路径
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn app_bundle_root(exe: &std::path::Path) -> Option<std::path::PathBuf> {
    exe.ancestors()
        .filter(|p| {
            p.extension()
                .is_some_and(|ext| ext.to_string_lossy().eq_ignore_ascii_case("app"))
        })
        .last()
        .map(std::path::Path::to_path_buf)
}

fn get_process_info(target_ide: Option<&str>) -> (Option<std::path::PathBuf>, Option<Vec<String>>) {
    let mut system = System::new_all();
    system.refresh_all();

    let current_exe = get_current_exe_path();
    let current_pid = std::process::id();

    for (pid, process) in system.processes() {
        let pid_u32 = pid.as_u32();
        if pid_u32 == current_pid {
            continue;
        }

        // Exclude manager process itself
        if let (Some(ref my_path), Some(p_exe)) = (&current_exe, process.exe()) {
            if let Ok(p_path) = p_exe.canonicalize() {
                if my_path == &p_path {
                    continue;
                }
            }
        }

        let name = process.name().to_string_lossy().to_lowercase();

        // Get executable path and command line arguments
        if let Some(exe) = process.exe() {
            let mut args = process.cmd().iter();
            // 身份判定以内核报告的真实可执行路径为准；argv[0] 可被启动方任意改写，仅作辅助特征
            let exe_path = exe.to_string_lossy().to_lowercase();
            let argv0 = args
                .next()
                .map(|arg| arg.to_string_lossy().to_lowercase())
                .unwrap_or_default();

            if is_manager_or_tools_process(&name, &exe_path, &argv0) {
                continue;
            }

            // Extract actual arguments from command line (skipping exe path)
            // 保留命令行参数原始大小写，防止 Linux / macOS 区分大小写的文件系统下工作区路径被破坏
            let raw_args = args
                .map(|arg| arg.to_string_lossy().to_string())
                .collect::<Vec<String>>();

            let args_lower_str = raw_args
                .iter()
                .map(|arg| arg.to_lowercase())
                .collect::<Vec<String>>()
                .join(" ");

            // Common helper process exclusion logic (strictly excludes language_server and sub-processes)
            let is_helper = is_helper_process(&name, &args_lower_str, &exe_path)
                || is_helper_process(&name, &args_lower_str, &argv0);

            // Sanitize snapshot arguments to prevent engine parameters like --standalone from leaking into relaunch
            let clean_args = sanitize_restart_args(&raw_args);
            let path = Some(exe.to_path_buf());
            let args = Some(clean_args);

            // Is the process a match for target_ide?
            let is_ide_match =
                matches_target_client(target_ide, &name, &exe_path, &argv0, &args_lower_str);

            if is_ide_match && !is_helper {
                #[cfg(target_os = "macos")]
                {
                    if !exe_path.contains("frameworks") {
                        if let Some(app_root) = app_bundle_root(exe) {
                            return (Some(app_root), args);
                        }
                    }
                    return (path, args);
                }

                #[cfg(target_os = "windows")]
                {
                    return (path, args);
                }

                #[cfg(target_os = "linux")]
                {
                    return (path, args);
                }
            }
        }
    }
    (None, None)
}

/// Get Antigravity executable path from running processes
///
/// Most reliable method to find installation anywhere
pub fn get_path_from_running_process(target_ide: Option<&str>) -> Option<std::path::PathBuf> {
    let (path, _) = get_process_info(target_ide);
    path
}

/// Get Antigravity startup arguments from running processes
pub fn get_args_from_running_process(target_ide: Option<&str>) -> Option<Vec<String>> {
    let (_, args) = get_process_info(target_ide);
    args
}

/// Get --user-data-dir argument value (if exists)
pub fn get_user_data_dir_from_process(target_ide: Option<&str>) -> Option<std::path::PathBuf> {
    // Prefer getting startup arguments from config
    if let Ok(config) = crate::modules::config::load_app_config() {
        if let Some(args) = config.antigravity_args {
            // Check arguments in config
            for i in 0..args.len() {
                if args[i] == "--user-data-dir" && i + 1 < args.len() {
                    // Next argument is the path
                    let path = std::path::PathBuf::from(&args[i + 1]);
                    if path.exists() {
                        return Some(path);
                    }
                } else if args[i].starts_with("--user-data-dir=") {
                    // Argument and value in same string, e.g. --user-data-dir=/path/to/data
                    let parts: Vec<&str> = args[i].splitn(2, '=').collect();
                    if parts.len() == 2 {
                        let path_str = parts[1];
                        let path = std::path::PathBuf::from(path_str);
                        if path.exists() {
                            return Some(path);
                        }
                    }
                }
            }
        }
    }

    // If not in config, get arguments from running process
    if let Some(args) = get_args_from_running_process(target_ide) {
        for i in 0..args.len() {
            if args[i] == "--user-data-dir" && i + 1 < args.len() {
                // Next argument is the path
                let path = std::path::PathBuf::from(&args[i + 1]);
                if path.exists() {
                    return Some(path);
                }
            } else if args[i].starts_with("--user-data-dir=") {
                // Argument and value in same string, e.g. --user-data-dir=/path/to/data
                let parts: Vec<&str> = args[i].splitn(2, '=').collect();
                if parts.len() == 2 {
                    let path_str = parts[1];
                    let path = std::path::PathBuf::from(path_str);
                    if path.exists() {
                        return Some(path);
                    }
                }
            }
        }
    }

    None
}

/// Get Antigravity executable path (cross-platform)
///
/// Search strategy (highest to lowest priority):
/// 1. Get path from running process (most reliable, supports any location)
/// 2. Iterate standard installation locations
/// 3. Return None
pub fn get_antigravity_executable_path(target_ide: Option<&str>) -> Option<std::path::PathBuf> {
    // Strategy 1: Get from running process (supports any location)
    if let Some(path) = get_path_from_running_process(target_ide) {
        return Some(path);
    }

    // Strategy 2: Check config paths (supports user-configured locations)
    if let Ok(config) = crate::modules::config::load_app_config() {
        match target_ide {
            Some("ide") => {
                if let Some(ref p) = config.antigravity_ide_executable {
                    let path = std::path::PathBuf::from(p);
                    if path.exists() {
                        return Some(path);
                    }
                }
            }
            _ => {
                // Try antigravity_executable first (closest match for target_ide=None)
                if let Some(ref p) = config.antigravity_executable {
                    let path = std::path::PathBuf::from(p);
                    if path.exists() {
                        return Some(path);
                    }
                }
            }
        }
    }

    // Strategy 3: Check standard installation locations
    check_standard_locations(target_ide)
}

/// Check standard installation locations and system PATH
fn check_standard_locations(target_ide: Option<&str>) -> Option<std::path::PathBuf> {
    let folder_names: &[&str] = if target_ide == Some("ide") {
        &["Antigravity IDE", "antigravity-ide", "antigravity_ide"]
    } else {
        &["Antigravity"]
    };

    #[cfg(target_os = "macos")]
    {
        for folder_name in folder_names {
            let mut paths = vec![std::path::PathBuf::from(format!(
                "/Applications/{}.app",
                folder_name
            ))];
            if let Some(home) = dirs::home_dir() {
                paths.push(home.join(format!("Applications/{}.app", folder_name)));
            }

            for path in paths {
                if path.exists() {
                    return Some(path);
                }
            }
        }

        // PATH 探测 (macOS)
        let exe_names: &[&str] = if target_ide == Some("ide") {
            &["antigravity-ide", "antigravity_ide"]
        } else {
            &["antigravity"]
        };
        if let Ok(path_var) = std::env::var("PATH") {
            for p in std::env::split_paths(&path_var) {
                for exe in exe_names {
                    let p_cmd = p.join(exe);
                    if p_cmd.exists() {
                        if target_ide != Some("ide") && is_antigravity_ide_path(&p_cmd) {
                            continue;
                        }
                        return Some(p_cmd);
                    }
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        use std::env;

        // Get environment variables
        let local_appdata = env::var("LOCALAPPDATA").ok();
        let program_files =
            env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".to_string());
        let program_files_x86 =
            env::var("ProgramFiles(x86)").unwrap_or_else(|_| "C:\\Program Files (x86)".to_string());
        let program_w6432 = env::var("ProgramW6432").ok();

        for folder_name in folder_names {
            let exe_names: &[&str] = if is_antigravity_ide_str(folder_name) {
                &[
                    "Antigravity IDE.exe",
                    "antigravity-ide.exe",
                    "antigravity_ide.exe",
                    "Antigravity.exe",
                ]
            } else {
                &["Antigravity.exe", "antigravity.exe"]
            };

            for exe_name in exe_names {
                let mut possible_paths = Vec::new();

                // User installation location (preferred)
                if let Some(local) = &local_appdata {
                    possible_paths.push(
                        std::path::PathBuf::from(local)
                            .join("Programs")
                            .join(folder_name)
                            .join(exe_name),
                    );
                    possible_paths.push(
                        std::path::PathBuf::from(local)
                            .join(folder_name)
                            .join(exe_name),
                    );
                }

                // System installation location
                possible_paths.push(
                    std::path::PathBuf::from(&program_files)
                        .join(folder_name)
                        .join(exe_name),
                );

                // 32-bit compatibility location
                possible_paths.push(
                    std::path::PathBuf::from(&program_files_x86)
                        .join(folder_name)
                        .join(exe_name),
                );

                // Explicit 64-bit location
                if let Some(ref w64) = program_w6432 {
                    possible_paths.push(
                        std::path::PathBuf::from(w64)
                            .join(folder_name)
                            .join(exe_name),
                    );
                }

                // Return the first existing path
                for path in possible_paths {
                    if path.exists() {
                        return Some(path);
                    }
                }
            }
        }

        // PATH 探测 (Windows)
        let path_exe_names: &[&str] = if target_ide == Some("ide") {
            &[
                "Antigravity IDE.exe",
                "antigravity-ide.exe",
                "antigravity_ide.exe",
            ]
        } else {
            &["Antigravity.exe", "antigravity.exe"]
        };
        if let Ok(path_var) = std::env::var("PATH") {
            for p in std::env::split_paths(&path_var) {
                for exe in path_exe_names {
                    let p_cmd = p.join(exe);
                    if p_cmd.exists() {
                        if target_ide != Some("ide") && is_antigravity_ide_path(&p_cmd) {
                            continue;
                        }
                        return Some(p_cmd);
                    }
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        for folder_name in folder_names {
            let exe_names = if is_antigravity_ide_str(folder_name) {
                vec!["antigravity-ide", "antigravity_ide", "Antigravity IDE"]
            } else {
                vec!["antigravity", "Antigravity"]
            };

            for exe_name in &exe_names {
                // PATH 探测 (Linux)
                if let Ok(path_var) = std::env::var("PATH") {
                    for p in std::env::split_paths(&path_var) {
                        let p_cmd = p.join(exe_name);
                        if p_cmd.exists() {
                            if target_ide != Some("ide") && is_antigravity_ide_path(&p_cmd) {
                                continue;
                            }
                            if is_manager_or_tools_process("", &p_cmd.to_string_lossy(), "") {
                                continue;
                            }
                            return Some(p_cmd);
                        }
                    }
                }

                let possible_paths = vec![
                    std::path::PathBuf::from(format!("/usr/bin/{}", exe_name)),
                    std::path::PathBuf::from(format!("/usr/local/bin/{}", exe_name)),
                    std::path::PathBuf::from(format!("/opt/{}/{}", folder_name, exe_name)),
                    std::path::PathBuf::from(format!("/opt/{}/{}", exe_name, exe_name)),
                    std::path::PathBuf::from(format!("/usr/share/{}/{}", folder_name, exe_name)),
                    std::path::PathBuf::from(format!("/usr/lib64/{}/{}", folder_name, exe_name)),
                    std::path::PathBuf::from(format!("/usr/lib/{}/{}", folder_name, exe_name)),
                    std::path::PathBuf::from(format!("/var/lib/flatpak/exports/bin/{}", exe_name)),
                    std::path::PathBuf::from(format!("/snap/bin/{}", exe_name)),
                ];

                // User local installation
                if let Some(home) = dirs::home_dir() {
                    let user_paths = vec![
                        home.join(format!(".local/bin/{}", exe_name)),
                        home.join(format!(".local/share/flatpak/exports/bin/{}", exe_name)),
                    ];
                    for path in user_paths {
                        if path.exists()
                            && !is_manager_or_tools_process("", &path.to_string_lossy(), "")
                        {
                            return Some(path);
                        }
                    }
                }

                for path in possible_paths {
                    if path.exists()
                        && !is_manager_or_tools_process("", &path.to_string_lossy(), "")
                    {
                        return Some(path);
                    }
                }
            }
        }
    }

    None
}

/// 获取 Antigravity CLI (agy) 的安装/可执行文件路径
pub fn get_antigravity_cli_executable_path() -> Option<std::path::PathBuf> {
    // 1. 优先从配置查询
    if let Ok(config) = crate::modules::config::load_app_config() {
        if let Some(ref p) = config.antigravity_cli_executable {
            let path = std::path::PathBuf::from(p);
            if path.exists() {
                return Some(path);
            }
        }
    }

    // 2. 检查标准用户本地目录 ~/.local/bin/agy 或 ~/.local/bin/agy.exe
    if let Some(home) = dirs::home_dir() {
        let local_bin = home.join(".local").join("bin");
        let path = if cfg!(target_os = "windows") {
            local_bin.join("agy.exe")
        } else {
            local_bin.join("agy")
        };
        if path.exists() {
            return Some(path);
        }
    }

    // 3. 在系统环境变量 PATH 中查找
    let cmd = if cfg!(target_os = "windows") {
        "agy.exe"
    } else {
        "agy"
    };
    if let Ok(path_var) = std::env::var("PATH") {
        for p in std::env::split_paths(&path_var) {
            let p_cmd = p.join(cmd);
            if p_cmd.exists() {
                return Some(p_cmd);
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_target_client_prefers_ide_markers_from_executable_path() {
        // argv[0] 被改写为经典版名称，但真实 exe 位于 IDE 包内
        assert!(matches_target_client(
            Some("ide"),
            "electron",
            "/applications/antigravity ide.app/contents/macos/electron",
            "antigravity",
            ""
        ));
        assert!(!matches_target_client(
            None,
            "electron",
            "/applications/antigravity ide.app/contents/macos/electron",
            "antigravity",
            ""
        ));

        assert!(matches_target_client(
            None,
            "electron",
            "/applications/antigravity.app/contents/macos/electron",
            "/applications/antigravity.app/contents/macos/electron",
            ""
        ));
        assert!(!matches_target_client(
            Some("ide"),
            "electron",
            "/applications/antigravity.app/contents/macos/electron",
            "/applications/antigravity.app/contents/macos/electron",
            ""
        ));
    }

    #[test]
    fn test_matches_target_client_for_agy_language_server() {
        assert!(matches_target_client(
            Some("ide"),
            "agy",
            "/Users/me/.gemini/bin/agy",
            "agy",
            "--hub --hub-port=52542 --app_data_dir=antigravity"
        ));
        assert!(matches_target_client(
            Some("ide"),
            "agy.exe",
            "C:\\Users\\me\\.gemini\\bin\\agy.exe",
            "agy.exe",
            "--hub --hub-port=52542 --app_data_dir=antigravity"
        ));
        // Classic mode should not match IDE language server agy
        assert!(!matches_target_client(
            None,
            "agy",
            "/Users/me/.gemini/bin/agy",
            "agy",
            "--hub --hub-port=52542 --app_data_dir=antigravity"
        ));
        // Standalone CLI execution without --hub / antigravity should not match IDE
        assert!(!matches_target_client(
            Some("ide"),
            "agy",
            "/Users/me/.gemini/bin/agy",
            "agy",
            "version"
        ));
    }

    #[test]
    fn app_bundle_root_splits_by_component() {
        use std::path::{Path, PathBuf};
        assert_eq!(
            app_bundle_root(Path::new(
                "/Applications/Antigravity IDE.app/Contents/MacOS/Electron"
            )),
            Some(PathBuf::from("/Applications/Antigravity IDE.app"))
        );
        // 非 ASCII 目录与嵌套 .app：取最外层包，且不会因字节偏移 panic
        assert_eq!(
            app_bundle_root(Path::new(
                "/Users/用户/应用/Antigravity.APP/Contents/Frameworks/Helper.app/Contents/MacOS/Helper"
            )),
            Some(PathBuf::from("/Users/用户/应用/Antigravity.APP"))
        );
        assert_eq!(app_bundle_root(Path::new("/usr/bin/antigravity")), None);
    }

    #[test]
    fn test_is_client_executable_missing() {
        assert!(is_client_executable_missing(
            "Unable to start Antigravity: executable not found"
        ));
        assert!(is_client_executable_missing(
            "Unable to start Antigravity: Unable to find application named 'Antigravity'"
        ));
        assert!(!is_client_executable_missing(
            "Unable to start Antigravity: Operation not permitted"
        ));
        assert!(!is_client_executable_missing(
            "Startup failed (detected path): Access is denied"
        ));
    }

    #[test]
    fn test_is_helper_process_detection() {
        // Normal main processes
        assert!(!is_helper_process(
            "Antigravity",
            "/Applications/Antigravity.app/Contents/MacOS/Antigravity",
            "/Applications/Antigravity.app/Contents/MacOS/Antigravity"
        ));
        assert!(!is_helper_process(
            "Antigravity.exe",
            "C:\\Program Files\\Antigravity\\Antigravity.exe",
            "C:\\Program Files\\Antigravity\\Antigravity.exe"
        ));

        // Language server / engine processes (must be detected as helper)
        assert!(is_helper_process(
            "language_server",
            "--standalone --override_ide_name antigravity --subclient_type hub",
            "/Applications/Antigravity.app/Contents/Resources/bin/language_server"
        ));
        assert!(is_helper_process(
            "language_server.exe",
            "--standalone",
            "C:\\Antigravity\\resources\\bin\\language_server.exe"
        ));
        assert!(is_helper_process(
            "Antigravity",
            "--type=utility --utility-sub-type=audio.mojom.AudioService",
            "/Applications/Antigravity.app/Contents/Frameworks/Antigravity Helper.app/Contents/MacOS/Antigravity Helper"
        ));
        assert!(is_helper_process(
            "Antigravity",
            "--type=renderer",
            "/Applications/Antigravity.app/Contents/Frameworks/Antigravity Helper (Renderer).app"
        ));
        assert!(is_helper_process(
            "crashpad_handler",
            "",
            "/Applications/Antigravity.app/Contents/Frameworks/Electron Framework.framework/Helpers/chrome_crashpad_handler"
        ));
        assert!(is_helper_process(
            "agy",
            "--hub --hub-port=52542 --app_data_dir=antigravity",
            "/Users/me/.gemini/bin/agy"
        ));
        assert!(is_helper_process(
            "agy.exe",
            "--hub --hub-port=52542 --app_data_dir=antigravity",
            "C:\\Users\\me\\.gemini\\bin\\agy.exe"
        ));
    }

    #[test]
    fn test_is_language_server_process_narrow_detection() {
        // 热切号的定位器必须"窄"：只认语言服务，绝不把 renderer / gpu / crashpad 等 helper 当目标
        assert!(is_language_server_process(
            "language_server.exe",
            "C:\\Users\\me\\AppData\\Local\\Programs\\Antigravity IDE\\resources\\bin\\language_server.exe"
        ));
        assert!(is_language_server_process(
            "language_server",
            "/Applications/Antigravity.app/Contents/Resources/bin/language_server"
        ));
        // macOS 进程名可能被截断 → 路径命中即可
        assert!(is_language_server_process(
            "language_server",
            "/Applications/Antigravity IDE.app/Contents/Resources/bin/language_server_macos_arm"
        ));
        // 大小写不敏感
        assert!(is_language_server_process(
            "LANGUAGE_SERVER.EXE",
            "C:\\Antigravity\\bin\\Language_Server.exe"
        ));
        // Antigravity IDE 2.6.0+ agy / agy.exe
        assert!(is_language_server_process(
            "agy.exe",
            "C:\\Users\\me\\.gemini\\bin\\agy.exe"
        ));
        assert!(is_language_server_process(
            "agy",
            "/Users/me/.gemini/bin/agy"
        ));

        // 反例：其它 Antigravity 进程一律不得命中（否则热切会误杀渲染/GPU/崩溃处理器）
        for (name, exe) in [
            ("Antigravity.exe", "C:\\Antigravity\\Antigravity.exe"),
            (
                "Antigravity",
                "/Applications/Antigravity.app/Contents/MacOS/Antigravity",
            ),
            (
                "Antigravity Helper",
                "/Applications/Antigravity.app/Contents/Frameworks/Antigravity Helper.app/Contents/MacOS/Antigravity Helper",
            ),
            (
                "Antigravity Helper (Renderer)",
                "/Applications/Antigravity.app/Contents/Frameworks/Antigravity Helper (Renderer).app",
            ),
            (
                "crashpad_handler",
                "/Applications/Antigravity.app/Contents/Frameworks/Electron Framework.framework/Helpers/chrome_crashpad_handler",
            ),
            ("node", "/usr/local/bin/node"),
        ] {
            assert!(
                !is_language_server_process(name, exe),
                "{name} 不应被判为 language_server"
            );
        }
    }

    #[test]
    fn test_sanitize_restart_args() {
        let dirty_args = vec![
            "--standalone".to_string(),
            "--override_ide_name".to_string(),
            "antigravity".to_string(),
            "--subclient_type".to_string(),
            "hub".to_string(),
            "--user-data-dir=/tmp/test".to_string(),
            "/path/to/project".to_string(),
        ];

        let cleaned = sanitize_restart_args(&dirty_args);
        assert!(!cleaned.contains(&"--standalone".to_string()));
        assert!(!cleaned.contains(&"--override_ide_name".to_string()));
        assert!(!cleaned.contains(&"--subclient_type".to_string()));
        assert!(cleaned.contains(&"--user-data-dir=/tmp/test".to_string()));
        assert!(cleaned.contains(&"/path/to/project".to_string()));
    }

    #[test]
    fn test_sanitize_restart_args_preserves_case() {
        let mixed_case_args = vec![
            "--standalone".to_string(),
            "/Users/Developer/MyProject".to_string(),
            "-R".to_string(),
        ];
        let cleaned = sanitize_restart_args(&mixed_case_args);
        assert_eq!(cleaned, vec!["/Users/Developer/MyProject", "-R"]);
    }

    #[test]
    fn test_check_standard_locations_isolation() {
        // [FIX #3253] 经典版与默认探测绝不能匹配 Antigravity IDE
        if let Some(path) = check_standard_locations(Some("classic")) {
            let path_str = path.to_string_lossy();
            assert!(
                !is_antigravity_ide_str(&path_str),
                "Classic search must never return Antigravity IDE path: {:?}",
                path
            );
        }

        if let Some(path) = check_standard_locations(None) {
            let path_str = path.to_string_lossy();
            assert!(
                !is_antigravity_ide_str(&path_str),
                "Default search must never fall back to Antigravity IDE path: {:?}",
                path
            );
        }
    }

    #[test]
    fn test_is_manager_or_tools_process_ignores_parent_dirs() {
        // 装在 D:\Tools\Antigravity / C:\Tools\agy 的客户端绝不能被当作 Manager，
        // 否则客户端永远检测不到，切号会变成「只写凭据不重启」的静默失败。
        assert!(!is_manager_or_tools_process(
            "antigravity.exe",
            r"d:\tools\antigravity\antigravity.exe",
            ""
        ));
        assert!(!is_manager_or_tools_process(
            "language_server.exe",
            r"c:\tools\antigravity\resources\bin\language_server.exe",
            ""
        ));
        assert!(!is_manager_or_tools_process(
            "agy.exe",
            r"c:\tools\agy\agy.exe",
            ""
        ));

        // Manager 自身的空格产品名与路径变体仍然命中
        assert!(is_manager_or_tools_process(
            "antigravity tools.exe",
            r"c:\users\me\appdata\local\programs\antigravity tools\antigravity tools.exe",
            ""
        ));
        assert!(is_manager_or_tools_process(
            "antigravity-manager.exe",
            r"e:\code\antigravity-manager\src-tauri\target\debug\antigravity-manager.exe",
            ""
        ));
        // 自定义命名（仅文件名命中宽松条款）仍然排除
        assert!(is_manager_or_tools_process(
            "antigravity-toolbox.exe",
            "",
            ""
        ));
    }

    #[test]
    fn test_is_manager_or_tools_process_filtering() {
        // Antigravity Tools / Manager filenames and paths must be detected as manager
        assert!(is_manager_or_tools_process("antigravity-tools", "", ""));
        assert!(is_manager_or_tools_process("antigravity_tools", "", ""));
        assert!(is_manager_or_tools_process("antigravity-too", "", "")); // Linux comm truncation
        assert!(is_manager_or_tools_process("antigravity_too", "", ""));
        assert!(is_manager_or_tools_process(
            "",
            "/home/hy/.local/bin/antigravity_tools_4.7.13",
            ""
        ));
        assert!(is_manager_or_tools_process(
            "",
            "/tmp/.mount_antigro7E3N3/usr/bin/antigravity-tools",
            ""
        ));
        assert!(is_manager_or_tools_process(
            "antigravity-tools.exe",
            "C:\\Program Files\\Antigravity Tools\\antigravity-tools.exe",
            ""
        ));
        assert!(is_manager_or_tools_process(
            "Antigravity Tools",
            "/Applications/Antigravity Tools.app/Contents/MacOS/antigravity-tools",
            ""
        ));

        // Real Antigravity IDE and classic client must NOT be flagged as manager
        assert!(!is_manager_or_tools_process(
            "antigravity",
            "/usr/bin/antigravity",
            ""
        ));
        assert!(!is_manager_or_tools_process(
            "antigravity-ide",
            "/usr/bin/antigravity-ide",
            ""
        ));
        assert!(!is_manager_or_tools_process(
            "Antigravity",
            "/Applications/Antigravity.app/Contents/MacOS/Antigravity",
            ""
        ));
        assert!(!is_manager_or_tools_process(
            "Antigravity IDE",
            "/Applications/Antigravity IDE.app/Contents/MacOS/Electron",
            ""
        ));
        assert!(!is_manager_or_tools_process(
            "Antigravity.exe",
            "C:\\Users\\user\\AppData\\Local\\Programs\\Antigravity\\Antigravity.exe",
            ""
        ));
        assert!(!is_manager_or_tools_process(
            "Antigravity IDE.exe",
            "C:\\Users\\user\\AppData\\Local\\Programs\\Antigravity IDE\\Antigravity IDE.exe",
            ""
        ));
    }

    #[test]
    fn test_matches_target_client_rejects_manager_and_tools() {
        let manager_exe = "/home/hy/.local/bin/antigravity_tools_4.7.13";
        assert!(!matches_target_client(
            None,
            "antigravity_too",
            manager_exe,
            manager_exe,
            ""
        ));
        assert!(!matches_target_client(
            Some("classic"),
            "antigravity_too",
            manager_exe,
            manager_exe,
            ""
        ));
        assert!(!matches_target_client(
            Some("ide"),
            "antigravity_too",
            manager_exe,
            manager_exe,
            ""
        ));
    }

    #[test]
    fn test_is_antigravity_ide_str_uses_naming_variants_only() {
        for s in [
            "Antigravity IDE",
            "antigravity-ide",
            "antigravity_ide",
            "AntigravityIDE",
            "antigravityide.exe",
            "antigravity.ide",
            r"C:\Users\user\AppData\Local\Programs\Antigravity IDE\Antigravity.exe",
        ] {
            assert!(is_antigravity_ide_str(s), "{s} 应判定为 IDE");
        }
        for s in [
            "Antigravity",
            "antigravity.exe",
            r"C:\Program Files\Antigravity\Antigravity.exe",
            // 弱特征组合不得构成 IDE 身份（否则打开 antigravity 工程的 VS Code 会被误杀）
            r"C:\Users\me\AppData\Local\Programs\Microsoft VS Code\Code.exe --folder-uri e:/work/antigravity-pro/.vscode",
        ] {
            assert!(!is_antigravity_ide_str(s), "{s} 不应判定为 IDE");
        }
    }

    #[test]
    fn test_classify_process_never_trusts_args_as_identity() {
        // 打开本仓库的 VS Code / 终端 / 资源管理器：命令行含 antigravity，但可执行文件不是 Antigravity。
        // 这类进程绝不得进入关闭列表（close_antigravity 会 taskkill /F /T）。
        assert_eq!(
            classify_process(
                "code.exe",
                r"c:\users\me\appdata\local\programs\microsoft vs code\code.exe",
                "",
                "--folder-uri file:///e%3a/code/antigravity-manager"
            ),
            ClientKind::Unrelated
        );
        assert_eq!(
            classify_process(
                "windowsterminal.exe",
                r"c:\program files\windowsterminal\windowsterminal.exe",
                "",
                "-p powershell cd e:/code/antigravity-manager"
            ),
            ClientKind::Unrelated
        );
        assert_eq!(
            classify_process(
                "explorer.exe",
                r"c:\windows\explorer.exe",
                "",
                "e:/code/antigravity-manager"
            ),
            ClientKind::Unrelated
        );
    }

    #[test]
    fn test_classify_process_excludes_manager_webview() {
        // Tauri WebView2：--user-data-dir 含应用 identifier，但可执行文件不是 Antigravity，
        // 绝不得被判为经典版（否则切号会杀掉自己的界面）。
        assert_eq!(
            classify_process(
                "msedgewebview2.exe",
                r"c:\program files (x86)\microsoft\edgewebview\application\142.0\msedgewebview2.exe",
                "",
                r"--embedded-browser-webview=1 --webview-exe-name=antigravity tools.exe --user-data-dir=c:\users\me\appdata\local\com.lbjlaq.antigravity-tools\ebwebview"
            ),
            ClientKind::Unrelated
        );
        // Manager 自身可执行文件
        assert_eq!(
            classify_process(
                "antigravity tools.exe",
                r"c:\program files\antigravity tools\antigravity tools.exe",
                "",
                ""
            ),
            ClientKind::Manager
        );
    }

    #[test]
    fn test_classify_process_ide_vs_classic() {
        assert_eq!(
            classify_process(
                "antigravity.exe",
                r"c:\users\me\appdata\local\programs\antigravity\antigravity.exe",
                "",
                ""
            ),
            ClientKind::Classic
        );
        assert_eq!(
            classify_process(
                "antigravity.exe",
                r"c:\users\me\appdata\local\programs\antigravity ide\antigravity.exe",
                "",
                ""
            ),
            ClientKind::Ide
        );
        // 自定义安装目录：靠 --user-data-dir 兜底（issue #3762 的 2.6.0+ IDE）
        assert_eq!(
            classify_process(
                "antigravity.exe",
                r"d:\apps\ag\antigravity.exe",
                "",
                r"--user-data-dir=c:\users\me\appdata\roaming\antigravity ide"
            ),
            ClientKind::Ide
        );
        // 经典版的 user-data-dir 不得被误判为 IDE
        assert_eq!(
            classify_process(
                "antigravity.exe",
                r"d:\apps\ag\antigravity.exe",
                "",
                r"--user-data-dir=c:\users\me\appdata\roaming\antigravity"
            ),
            ClientKind::Classic
        );
        // 工作区路径里出现 IDE 字样不得构成 IDE 身份
        assert_eq!(
            classify_process(
                "antigravity.exe",
                r"d:\apps\ag\antigravity.exe",
                "",
                r"--user-data-dir=c:\users\me\appdata\roaming\antigravity --folder-uri=e:/work/antigravity-ide-demo"
            ),
            ClientKind::Classic
        );
    }
}
