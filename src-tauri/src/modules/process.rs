use std::process::Command;
use std::thread;
use std::time::Duration;
use sysinfo::System;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

/// 获取当前正在运行的可执行文件规范化路径
fn get_current_exe_path() -> Option<std::path::PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.canonicalize().ok())
}

/// 检查 Antigravity 是否在运行
pub fn is_antigravity_running() -> bool {
    let mut system = System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All);

    let current_exe = get_current_exe_path();
    let current_pid = std::process::id();

    // 识别参考 1: 加载手动配置路径（移至循环外以提升性能）
    let manual_path = crate::modules::config::load_app_config()
        .ok()
        .and_then(|c| c.antigravity_executable)
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

        // 排除自身路径（处理 Linux 下管理器被误认为反重力进程的情况）
        if let (Some(ref my_path), Some(p_exe)) = (&current_exe, process.exe()) {
            if let Ok(p_path) = p_exe.canonicalize() {
                if my_path == &p_path {
                    continue;
                }
            }
        }

        // 识别参考 2: 优先检查是否匹配手动指定的路径
        if let (Some(ref m_path), Some(p_exe)) = (&manual_path, process.exe()) {
            if let Ok(p_path) = p_exe.canonicalize() {
                // macOS 下检查是否都在同一个 .app 包内
                #[cfg(target_os = "macos")]
                {
                    let m_path_str = m_path.to_string_lossy();
                    let p_path_str = p_path.to_string_lossy();
                    if let (Some(m_idx), Some(p_idx)) =
                        (m_path_str.find(".app"), p_path_str.find(".app"))
                    {
                        if m_path_str[..m_idx + 4] == p_path_str[..p_idx + 4] {
                            // 即使路径匹配，也必须通过名称和参数确认不是 Helper
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
                            if !is_helper_by_args && !is_helper_by_name {
                                return true;
                            }
                        }
                    }
                }

                #[cfg(not(target_os = "macos"))]
                if m_path == &p_path {
                    return true;
                }
            }
        }

        // 通用的辅助进程排除逻辑

        // 通用的辅助进程排除逻辑
        let args = process.cmd();
        let args_str = args
            .iter()
            .map(|arg| arg.to_string_lossy().to_lowercase())
            .collect::<Vec<String>>()
            .join(" ");

        let is_helper = args_str.contains("--type=")
            || name.contains("helper")
            || name.contains("plugin")
            || name.contains("renderer")
            || name.contains("gpu")
            || name.contains("crashpad")
            || name.contains("utility")
            || name.contains("audio")
            || name.contains("sandbox")
            || exe_path.contains("crashpad");

        #[cfg(target_os = "macos")]
        {
            if exe_path.contains("antigravity.app") && !is_helper {
                return true;
            }
        }

        #[cfg(target_os = "windows")]
        {
            if name == "antigravity.exe" && !is_helper {
                return true;
            }
        }

        #[cfg(target_os = "linux")]
        {
            if (name.contains("antigravity") || exe_path.contains("/antigravity"))
                && !name.contains("tools")
                && !is_helper
            {
                return true;
            }
        }
    }

    false
}

#[cfg(target_os = "linux")]
/// 获取当前进程及其所有直系亲属（祖先 + 后代）的 PID 集合
fn get_self_family_pids(system: &sysinfo::System) -> std::collections::HashSet<u32> {
    let current_pid = std::process::id();
    let mut family_pids = std::collections::HashSet::new();
    family_pids.insert(current_pid);

    // 1. 向上查找所有祖先 (Ancestors) - 防止杀掉启动器
    let mut next_pid = current_pid;
    // 防止死循环，设置最大深度 10
    for _ in 0..10 {
        let pid_val = sysinfo::Pid::from_u32(next_pid);
        if let Some(process) = system.process(pid_val) {
            if let Some(parent) = process.parent() {
                let parent_id = parent.as_u32();
                // 避免循环或重复
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

    // 2. 向下查找所有后代 (Descendants)
    // 构建父子关系映射 (Parent -> Children)
    let mut adj: std::collections::HashMap<u32, Vec<u32>> = std::collections::HashMap::new();
    for (pid, process) in system.processes() {
        if let Some(parent) = process.parent() {
            adj.entry(parent.as_u32()).or_default().push(pid.as_u32());
        }
    }

    // BFS 遍历查找所有后代
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(current_pid);

    while let Some(pid) = queue.pop_front() {
        if let Some(children) = adj.get(&pid) {
            for &child in children {
                if family_pids.insert(child) {
                    queue.push_back(child);
                }
            }
        }
    }

    family_pids
}

/// 获取所有 Antigravity 进程的 PID（包括主进程和Helper进程）
fn get_antigravity_pids() -> Vec<u32> {
    let mut system = System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All);

    // Linux 端启用家族进程树排除
    #[cfg(target_os = "linux")]
    let family_pids = get_self_family_pids(&system);

    let mut pids = Vec::new();
    let current_pid = std::process::id();
    let current_exe = get_current_exe_path();

    // 加载手动配置路径作为辅助参考
    let manual_path = crate::modules::config::load_app_config()
        .ok()
        .and_then(|c| c.antigravity_executable)
        .and_then(|p| std::path::PathBuf::from(p).canonicalize().ok());

    for (pid, process) in system.processes() {
        let pid_u32 = pid.as_u32();

        // 排除自身 PID
        if pid_u32 == current_pid {
            continue;
        }

        // 排除自身可执行文件路径（深度加固，防止名称识别过于宽泛）
        if let (Some(ref my_path), Some(p_exe)) = (&current_exe, process.exe()) {
            if let Ok(p_path) = p_exe.canonicalize() {
                if my_path == &p_path {
                    continue;
                }
            }
        }

        let _name = process.name().to_string_lossy().to_lowercase();

        #[cfg(target_os = "linux")]
        {
            // 1. 排除家族进程 (自身、子进程、父进程)
            if family_pids.contains(&pid_u32) {
                continue;
            }
            // 2. 额外保护：如果名字包含 "tools" 且不是子进程，很有可能是管理器本身
            if _name.contains("tools") {
                continue;
            }
        }

        #[cfg(not(target_os = "linux"))]
        {
            // 其他平台仅排除自身
            if pid_u32 == current_pid {
                continue;
            }
        }

        // 识别参考 3: 检查手动配置路径匹配
        if let (Some(ref m_path), Some(p_exe)) = (&manual_path, process.exe()) {
            if let Ok(p_path) = p_exe.canonicalize() {
                #[cfg(target_os = "macos")]
                {
                    let m_path_str = m_path.to_string_lossy();
                    let p_path_str = p_path.to_string_lossy();
                    if let (Some(m_idx), Some(p_idx)) =
                        (m_path_str.find(".app"), p_path_str.find(".app"))
                    {
                        if m_path_str[..m_idx + 4] == p_path_str[..p_idx + 4] {
                            let args = process.cmd();
                            let is_helper_by_args = args
                                .iter()
                                .any(|arg| arg.to_string_lossy().contains("--type="));
                            let is_helper_by_name = _name.contains("helper")
                                || _name.contains("plugin")
                                || _name.contains("renderer")
                                || _name.contains("gpu")
                                || _name.contains("crashpad")
                                || _name.contains("utility")
                                || _name.contains("audio")
                                || _name.contains("sandbox");
                            if !is_helper_by_args && !is_helper_by_name {
                                pids.push(pid_u32);
                                continue;
                            }
                        }
                    }
                }

                #[cfg(not(target_os = "macos"))]
                if m_path == &p_path {
                    pids.push(pid_u32);
                    continue;
                }
            }
        }

        // 获取可执行文件路径
        let exe_path = process
            .exe()
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .to_lowercase();

        // 通用的辅助进程排除逻辑
        let args = process.cmd();
        let args_str = args
            .iter()
            .map(|arg| arg.to_string_lossy().to_lowercase())
            .collect::<Vec<String>>()
            .join(" ");

        let is_helper = args_str.contains("--type=")
            || _name.contains("helper")
            || _name.contains("plugin")
            || _name.contains("renderer")
            || _name.contains("gpu")
            || _name.contains("crashpad")
            || _name.contains("utility")
            || _name.contains("audio")
            || _name.contains("sandbox")
            || exe_path.contains("crashpad");

        #[cfg(target_os = "macos")]
        {
            // 匹配 Antigravity 主程序包内的进程，但排除 Helper/Plugin/Renderer 等辅助进程
            if exe_path.contains("antigravity.app") && !is_helper {
                pids.push(pid_u32);
            }
        }

        #[cfg(target_os = "windows")]
        {
            let name = process.name().to_string_lossy().to_lowercase();
            if name == "antigravity.exe" && !is_helper {
                pids.push(pid_u32);
            }
        }

        #[cfg(target_os = "linux")]
        {
            let name = process.name().to_string_lossy().to_lowercase();
            if (name == "antigravity" || exe_path.contains("/antigravity"))
                && !name.contains("tools")
                && !is_helper
            {
                pids.push(pid_u32);
            }
        }
    }

    if !pids.is_empty() {
        crate::modules::logger::log_info(&format!(
            "找到 {} 个 Antigravity 进程: {:?}",
            pids.len(),
            pids
        ));
    }

    pids
}

/// 关闭 Antigravity 进程
pub fn close_antigravity(timeout_secs: u64) -> Result<(), String> {
    crate::modules::logger::log_info("正在关闭 Antigravity...");

    #[cfg(target_os = "windows")]
    {
        // Windows: 改为使用 PID 进行精准关闭，以支持并存多版本或自定义文件名
        let pids = get_antigravity_pids();
        if !pids.is_empty() {
            crate::modules::logger::log_info(&format!(
                "正在 Windows 上精准关闭 {} 个识别到的进程...",
                pids.len()
            ));
            for pid in pids {
                let _ = Command::new("taskkill")
                    .args(["/F", "/PID", &pid.to_string()])
                    .creation_flags(0x08000000) // CREATE_NO_WINDOW
                    .output();
            }
            // 给一点点时间让系统清理 PID
            thread::sleep(Duration::from_millis(200));
        }
    }

    #[cfg(target_os = "macos")]
    {
        // macOS: 优化关闭策略，避免"窗口意外终止"弹窗
        // 策略：仅向主进程发送 SIGTERM，让其自行协调关闭子进程

        let pids = get_antigravity_pids();
        if !pids.is_empty() {
            // 1. 识别主进程 (PID)
            // 策略：Electron/Tauri 的主进程没有 `--type` 参数，而 Helper 进程都有 `--type=renderer/gpu/utility` 等
            let mut system = System::new();
            system.refresh_processes(sysinfo::ProcessesToUpdate::All);

            let mut main_pid = None;

            // 加载手动配置路径作为最高优先级参考
            let manual_path = crate::modules::config::load_app_config()
                .ok()
                .and_then(|c| c.antigravity_executable)
                .and_then(|p| std::path::PathBuf::from(p).canonicalize().ok());

            crate::modules::logger::log_info("正在分析进程列表以识别主进程:");
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

                    // 1. 优先尝试手动路径匹配
                    if let (Some(ref m_path), Some(p_exe)) = (&manual_path, process.exe()) {
                        if let Ok(p_path) = p_exe.canonicalize() {
                            let m_path_str = m_path.to_string_lossy();
                            let p_path_str = p_path.to_string_lossy();
                            if let (Some(m_idx), Some(p_idx)) =
                                (m_path_str.find(".app"), p_path_str.find(".app"))
                            {
                                if m_path_str[..m_idx + 4] == p_path_str[..p_idx + 4] {
                                    // 深度校验：即便路径匹配，也必须排除 Helper 关键字和参数
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
                                        crate::modules::logger::log_info("   => 识别为主进程 (匹配手动配置路径)");
                                        break;
                                    }
                                }
                            }
                        }
                    }

                    // 2. 特征分析匹配（回退方案）
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
                            crate::modules::logger::log_info("   => 识别为主进程 (Name/Args特征分析)");
                        }
                    } else {
                        crate::modules::logger::log_info("   => 识别为辅助进程 (Helper/Args)");
                    }
                }
            }

            // 阶段 1: 优雅退出 (SIGTERM)
            if let Some(pid) = main_pid {
                crate::modules::logger::log_info(&format!(
                    "决定向主进程 PID: {} 发送 SIGTERM",
                    pid
                ));
                let output = Command::new("kill")
                    .args(["-15", &pid.to_string()])
                    .output();

                if let Ok(result) = output {
                    if !result.status.success() {
                        let error = String::from_utf8_lossy(&result.stderr);
                        crate::modules::logger::log_warn(&format!(
                            "主进程 SIGTERM 失败: {}",
                            error
                        ));
                    }
                }
            } else {
                crate::modules::logger::log_warn(
                    "未识别出明确的主进程，将尝试对所有进程发送 SIGTERM (可能导致弹窗)",
                );
                for pid in &pids {
                    let _ = Command::new("kill")
                        .args(["-15", &pid.to_string()])
                        .output();
                }
            }

            // 等待优雅退出（最多 timeout_secs 的 70%）
            let graceful_timeout = (timeout_secs * 7) / 10;
            let start = std::time::Instant::now();
            while start.elapsed() < Duration::from_secs(graceful_timeout) {
                if !is_antigravity_running() {
                    crate::modules::logger::log_info("所有 Antigravity 进程已优雅关闭");
                    return Ok(());
                }
                thread::sleep(Duration::from_millis(500));
            }

            // 阶段 2: 强制杀死 (SIGKILL) - 针对残留的所有进程 (Helpers)
            if is_antigravity_running() {
                let remaining_pids = get_antigravity_pids();
                if !remaining_pids.is_empty() {
                    crate::modules::logger::log_warn(&format!(
                        "优雅关闭超时，强制杀死 {} 个残留进程 (SIGKILL)",
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
                                        "SIGKILL 进程 {} 失败: {}",
                                        pid, error
                                    ));
                                }
                            }
                        }
                    }
                    thread::sleep(Duration::from_secs(1));
                }

                // 再次检查
                if !is_antigravity_running() {
                    crate::modules::logger::log_info("所有进程已在强制清理后退出");
                    return Ok(());
                }
            } else {
                crate::modules::logger::log_info("所有进程已在 SIGTERM 后退出");
                return Ok(());
            }
        } else {
            // 只有当 pids 为空时才认为没在运行，不要在这里报错，因为可能是已经关闭了
            crate::modules::logger::log_info("Antigravity 未在运行，无需关闭");
            return Ok(());
        }
    }

    #[cfg(target_os = "linux")]
    {
        // Linux: 同样尝试识别主进程并委派退出
        let pids = get_antigravity_pids();
        if !pids.is_empty() {
            let mut system = System::new();
            system.refresh_processes(sysinfo::ProcessesToUpdate::All);

            let mut main_pid = None;

            // 加载手动配置路径作为最高优先级参考
            let manual_path = crate::modules::config::load_app_config()
                .ok()
                .and_then(|c| c.antigravity_executable)
                .and_then(|p| std::path::PathBuf::from(p).canonicalize().ok());

            crate::modules::logger::log_info("正在分析 Linux 进程列表以识别主进程:");
            for pid_u32 in &pids {
                let pid = sysinfo::Pid::from_u32(*pid_u32);
                if let Some(process) = system.process(pid) {
                    let name = process.name().to_string_lossy().to_lowercase();
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

                    // 1. 优先尝试手动路径匹配
                    if let (Some(ref m_path), Some(p_exe)) = (&manual_path, process.exe()) {
                        if let Ok(p_path) = p_exe.canonicalize() {
                            if &p_path == m_path {
                                // 确认不是 Helper
                                let is_helper_by_args = args_str.contains("--type=");
                                let is_helper_by_name = name.contains("helper")
                                    || name.contains("renderer")
                                    || name.contains("gpu")
                                    || name.contains("crashpad")
                                    || name.contains("utility")
                                    || name.contains("audio")
                                    || name.contains("sandbox");
                                if !is_helper_by_args && !is_helper_by_name {
                                    main_pid = Some(pid_u32);
                                    crate::modules::logger::log_info(&format!(
                                        "   => 识别为主进程 (匹配手动配置路径)"
                                    ));
                                    break;
                                }
                            }
                        }
                    }

                    // 2. 特征分析匹配
                    let is_helper_by_args = args_str.contains("--type=");
                    let is_helper_by_name = name.contains("helper")
                        || name.contains("renderer")
                        || name.contains("gpu")
                        || name.contains("crashpad")
                        || name.contains("utility")
                        || name.contains("audio")
                        || name.contains("sandbox")
                        || name.contains("plugin")
                        || name.contains("language_server");

                    if !is_helper_by_args && !is_helper_by_name {
                        if main_pid.is_none() {
                            main_pid = Some(pid_u32);
                            crate::modules::logger::log_info(&format!(
                                "   => 识别为主进程 (特征分析)"
                            ));
                        }
                    } else {
                        crate::modules::logger::log_info(&format!(
                            "   => 识别为辅助进程 (Helper/Args)"
                        ));
                    }
                }
            }

            // 阶段 1: 优雅退出 (SIGTERM)
            if let Some(pid) = main_pid {
                crate::modules::logger::log_info(&format!("尝试优雅关闭主进程 {} (SIGTERM)", pid));
                let _ = Command::new("kill")
                    .args(["-15", &pid.to_string()])
                    .output();
            } else {
                crate::modules::logger::log_warn(
                    "未识别出明确的 Linux 主进程，将对所有关联进程发送 SIGTERM",
                );
                for pid in &pids {
                    let _ = Command::new("kill")
                        .args(["-15", &pid.to_string()])
                        .output();
                }
            }

            // 等待优雅退出
            let graceful_timeout = (timeout_secs * 7) / 10;
            let start = std::time::Instant::now();
            while start.elapsed() < Duration::from_secs(graceful_timeout) {
                if !is_antigravity_running() {
                    crate::modules::logger::log_info("Antigravity 已优雅关闭");
                    return Ok(());
                }
                thread::sleep(Duration::from_millis(500));
            }

            // 阶段 2: 强制杀死 (SIGKILL) - 针对全量残留进程
            if is_antigravity_running() {
                let remaining_pids = get_antigravity_pids();
                if !remaining_pids.is_empty() {
                    crate::modules::logger::log_warn(&format!(
                        "优雅关闭超时，强制杀死 {} 个残留进程 (SIGKILL)",
                        remaining_pids.len()
                    ));
                    for pid in &remaining_pids {
                        let _ = Command::new("kill").args(["-9", &pid.to_string()]).output();
                    }
                    thread::sleep(Duration::from_secs(1));
                }
            }
        } else {
            // pids 为空，说明没有检测到进程，或者都被排除逻辑排除了
            crate::modules::logger::log_info(
                "未找到需要关闭的 Antigravity 进程 (可能已被过滤或未运行)",
            );
        }
    }

    // 最终检查
    if is_antigravity_running() {
        return Err("无法关闭 Antigravity 进程，请手动关闭后重试".to_string());
    }

    crate::modules::logger::log_info("Antigravity 已成功关闭");
    Ok(())
}

/// 启动 Antigravity
pub fn start_antigravity() -> Result<(), String> {
    crate::modules::logger::log_info("正在启动 Antigravity...");

    // 优先从配置项加载手动指定的路径和参数
    let config = crate::modules::config::load_app_config().ok();
    let manual_path = config
        .as_ref()
        .and_then(|c| c.antigravity_executable.clone());
    let args = config.and_then(|c| c.antigravity_args.clone());

    if let Some(mut path_str) = manual_path {
        let mut path = std::path::PathBuf::from(&path_str);

        #[cfg(target_os = "macos")]
        {
            // 容错处理：如果指定的路径位于 .app 内部（比如误选了 Helper），则自动修正为 .app 目录
            if let Some(app_idx) = path_str.find(".app") {
                let corrected_app = &path_str[..app_idx + 4];
                if corrected_app != path_str {
                    crate::modules::logger::log_info(&format!(
                        "检测到 macOS 路径位于 .app 内部，自动修正为: {}",
                        corrected_app
                    ));
                    path_str = corrected_app.to_string();
                    path = std::path::PathBuf::from(&path_str);
                }
            }
        }

        if path.exists() {
            crate::modules::logger::log_info(&format!("使用手动配置路径启动: {}", path_str));

            #[cfg(target_os = "macos")]
            {
                // macOS 下如果是 .app 目录，用 open
                if path_str.ends_with(".app") || path.is_dir() {
                    let mut cmd = Command::new("open");
                    cmd.arg("-a").arg(&path_str);

                    // 添加启动参数
                    if let Some(ref args) = args {
                        for arg in args {
                            cmd.arg(arg);
                        }
                    }

                    cmd.spawn().map_err(|e| format!("启动失败 (open): {}", e))?;
                } else {
                    let mut cmd = Command::new(&path_str);

                    // 添加启动参数
                    if let Some(ref args) = args {
                        for arg in args {
                            cmd.arg(arg);
                        }
                    }

                    cmd.spawn()
                        .map_err(|e| format!("启动失败 (direct): {}", e))?;
                }
            }

            #[cfg(not(target_os = "macos"))]
            {
                let mut cmd = Command::new(&path_str);

                // 添加启动参数
                if let Some(ref args) = args {
                    for arg in args {
                        cmd.arg(arg);
                    }
                }

                cmd.spawn().map_err(|e| format!("启动失败: {}", e))?;
            }

            crate::modules::logger::log_info(&format!(
                "Antigravity 启动命令已发送 (手动路径: {}, 参数: {:?})",
                path_str, args
            ));
            return Ok(());
        } else {
            crate::modules::logger::log_warn(&format!(
                "手动配置路径不存在: {}，将回退到自动检测",
                path_str
            ));
        }
    }

    #[cfg(target_os = "macos")]
    {
        // 改进：使用 output() 等待 open 命令完成，以捕获"应用未找到"错误
        let mut cmd = Command::new("open");
        cmd.args(["-a", "Antigravity"]);

        // 添加启动参数
        if let Some(ref args) = args {
            for arg in args {
                cmd.arg(arg);
            }
        }

        let output = cmd
            .output()
            .map_err(|e| format!("无法执行 open 命令: {}", e))?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "启动失败 (open exited with {}): {}",
                output.status, error
            ));
        }
    }

    #[cfg(target_os = "windows")]
    {
        // 尝试通过注册表或默认路径启动
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "start", "antigravity://"]);

        // 添加启动参数
        if let Some(ref args) = args {
            for arg in args {
                cmd.arg(arg);
            }
        }

        let result = cmd.spawn();

        if result.is_err() {
            return Err("启动失败，请手动打开 Antigravity".to_string());
        }
    }

    #[cfg(target_os = "linux")]
    {
        let mut cmd = Command::new("antigravity");

        // 添加启动参数
        if let Some(ref args) = args {
            for arg in args {
                cmd.arg(arg);
            }
        }

        cmd.spawn().map_err(|e| format!("启动失败: {}", e))?;
    }

    crate::modules::logger::log_info(&format!(
        "Antigravity 启动命令已发送 (默认检测, 参数: {:?})",
        args
    ));
    Ok(())
}

/// 从运行中的进程获取 Antigravity 可执行文件路径和启动参数
///
/// 这是最可靠的方法，可以找到任意位置的安装和启动参数
fn get_process_info() -> (Option<std::path::PathBuf>, Option<Vec<String>>) {
    let mut system = System::new_all();
    system.refresh_all();

    let current_exe = get_current_exe_path();
    let current_pid = std::process::id();

    for (pid, process) in system.processes() {
        let pid_u32 = pid.as_u32();
        if pid_u32 == current_pid {
            continue;
        }

        // 排除管理器自身进程
        if let (Some(ref my_path), Some(p_exe)) = (&current_exe, process.exe()) {
            if let Ok(p_path) = p_exe.canonicalize() {
                if my_path == &p_path {
                    continue;
                }
            }
        }

        let name = process.name().to_string_lossy().to_lowercase();

        // 获取可执行文件路径和命令行参数
        if let Some(exe) = process.exe() {
            let mut args = process.cmd().iter();
            let exe_path = args
                .next()
                .map_or(exe.to_string_lossy(), |arg| arg.to_string_lossy())
                .to_lowercase();

            // 从命令行参数中提取真正的参数（跳过可执行文件路径）
            let args = args
                .map(|arg| arg.to_string_lossy().to_lowercase())
                .collect::<Vec<String>>();

            let args_str = args.join(" ");

            // 通用的辅助进程排除逻辑
            let is_helper = args_str.contains("--type=")
                || args_str.contains("node-ipc")
                || args_str.contains("nodeipc")
                || args_str.contains("max-old-space-size")
                || args_str.contains("node_modules")
                || name.contains("helper")
                || name.contains("plugin")
                || name.contains("renderer")
                || name.contains("gpu")
                || name.contains("crashpad")
                || name.contains("utility")
                || name.contains("audio")
                || name.contains("sandbox")
                || exe_path.contains("crashpad");

            let path = Some(exe.to_path_buf());
            let args = Some(args);
            #[cfg(target_os = "macos")]
            {
                // macOS: 排除辅助进程,只匹配主程序,并检查 Frameworks
                if exe_path.contains("antigravity.app")
                    && !is_helper
                    && !exe_path.contains("frameworks")
                {
                    // 尝试提取 .app 路径以便更好地支持 open 命令
                    if let Some(app_idx) = exe_path.find(".app") {
                        let app_path_str = &exe.to_string_lossy()[..app_idx + 4];
                        let path = Some(std::path::PathBuf::from(app_path_str));
                        return (path, args);
                    }
                    return (path, args);
                }
            }

            #[cfg(target_os = "windows")]
            {
                // Windows: 严格匹配进程名且排除辅助进程
                if name == "antigravity.exe" && !is_helper {
                    return (path, args);
                }
            }

            #[cfg(target_os = "linux")]
            {
                // Linux: 检查进程名或路径包含 antigravity，排除辅助进程和管理器
                if (name == "antigravity" || exe_path.contains("/antigravity"))
                    && !name.contains("tools")
                    && !is_helper
                {
                    return (path, args);
                }
            }
        }
    }
    (None, None)
}

/// 从运行中的进程获取 Antigravity 可执行文件路径
///
/// 这是最可靠的方法，可以找到任意位置的安装
pub fn get_path_from_running_process() -> Option<std::path::PathBuf> {
    let (path, _) = get_process_info();
    path
}

/// 从运行中的进程获取 Antigravity 启动参数
pub fn get_args_from_running_process() -> Option<Vec<String>> {
    let (_, args) = get_process_info();
    args
}

/// 获取 --user-data-dir 参数值（如果存在）
pub fn get_user_data_dir_from_process() -> Option<std::path::PathBuf> {
    // 优先从配置中获取启动参数
    if let Ok(config) = crate::modules::config::load_app_config() {
        if let Some(args) = config.antigravity_args {
            // 检查配置中的参数
            for i in 0..args.len() {
                if args[i] == "--user-data-dir" && i + 1 < args.len() {
                    // 下一个参数是路径
                    let path = std::path::PathBuf::from(&args[i + 1]);
                    if path.exists() {
                        return Some(path);
                    }
                } else if args[i].starts_with("--user-data-dir=") {
                    // 参数和值在同一个字符串中，如 --user-data-dir=/path/to/data
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

    // 如果配置中没有，从运行中的进程获取参数
    if let Some(args) = get_args_from_running_process() {
        for i in 0..args.len() {
            if args[i] == "--user-data-dir" && i + 1 < args.len() {
                // 下一个参数是路径
                let path = std::path::PathBuf::from(&args[i + 1]);
                if path.exists() {
                    return Some(path);
                }
            } else if args[i].starts_with("--user-data-dir=") {
                // 参数和值在同一个字符串中，如 --user-data-dir=/path/to/data
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

/// 获取 Antigravity 可执行文件路径（跨平台）
///
/// 查找策略（优先级从高到低）：
/// 1. 从运行中的进程获取路径（最可靠，支持任意安装位置）
/// 2. 遍历标准安装位置
/// 3. 返回 None
pub fn get_antigravity_executable_path() -> Option<std::path::PathBuf> {
    // 策略1: 从运行进程获取（支持任意位置）
    if let Some(path) = get_path_from_running_process() {
        return Some(path);
    }

    // 策略2: 检查标准安装位置
    check_standard_locations()
}

/// 检查标准安装位置
fn check_standard_locations() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let path = std::path::PathBuf::from("/Applications/Antigravity.app");
        if path.exists() {
            return Some(path);
        }
    }

    #[cfg(target_os = "windows")]
    {
        use std::env;

        // 获取环境变量
        let local_appdata = env::var("LOCALAPPDATA").ok();
        let program_files =
            env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".to_string());
        let program_files_x86 =
            env::var("ProgramFiles(x86)").unwrap_or_else(|_| "C:\\Program Files (x86)".to_string());

        let mut possible_paths = Vec::new();

        // 用户安装位置（优先）
        if let Some(local) = local_appdata {
            possible_paths.push(
                std::path::PathBuf::from(&local)
                    .join("Programs")
                    .join("Antigravity")
                    .join("Antigravity.exe"),
            );
        }

        // 系统安装位置
        possible_paths.push(
            std::path::PathBuf::from(&program_files)
                .join("Antigravity")
                .join("Antigravity.exe"),
        );

        // 32位兼容位置
        possible_paths.push(
            std::path::PathBuf::from(&program_files_x86)
                .join("Antigravity")
                .join("Antigravity.exe"),
        );

        // 返回第一个存在的路径
        for path in possible_paths {
            if path.exists() {
                return Some(path);
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        let possible_paths = vec![
            std::path::PathBuf::from("/usr/bin/antigravity"),
            std::path::PathBuf::from("/opt/Antigravity/antigravity"),
            std::path::PathBuf::from("/usr/share/antigravity/antigravity"),
        ];

        // 用户本地安装
        if let Some(home) = dirs::home_dir() {
            let user_local = home.join(".local/bin/antigravity");
            if user_local.exists() {
                return Some(user_local);
            }
        }

        for path in possible_paths {
            if path.exists() {
                return Some(path);
            }
        }
    }

    None
}
