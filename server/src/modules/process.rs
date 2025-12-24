use sysinfo::System;
use std::process::Command;
use std::thread;
use std::time::Duration;

/// 检查 Antigravity 是否在运行
pub fn is_antigravity_running() -> bool {
    let mut system = System::new();
    // 关键修复：必须刷新进程列表，否则获取的是空列表
    system.refresh_processes(sysinfo::ProcessesToUpdate::All);

    let current_pid = std::process::id();

    for (pid, process) in system.processes() {
        if pid.as_u32() == current_pid {
            continue;
        }

        #[allow(unused_variables)]
        let name = process.name().to_string_lossy().to_lowercase();
        let exe_path = process.exe()
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .to_lowercase();

        #[cfg(target_os = "macos")]
        {
            if exe_path.contains("antigravity.app") {
                return true;
            }
        }

        #[cfg(target_os = "windows")]
        {
            // 严格匹配进程名，避免 false positive (如 esbuild.exe 在 antigravity 目录下)
            if name == "antigravity.exe" {
                 crate::modules::logger::log_info(&format!("检测到 Antigravity 进程: {} (PID: {}) Path: {}", name, pid, exe_path));
                 return true;
            }
        }

        #[cfg(target_os = "linux")]
        {
            if name == "antigravity" || exe_path.contains("antigravity") {
                return true;
            }
        }
    }

    false
}

/// 获取所有 Antigravity 进程的 PID（包括主进程和Helper进程）
fn get_antigravity_pids() -> Vec<u32> {
    let mut system = System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All);
    
    let current_pid = std::process::id();
    let mut pids = Vec::new();
    
    for (pid, process) in system.processes() {
        // 排除当前进程
        if pid.as_u32() == current_pid {
            continue;
        }
        
        let exe_path = process.exe()
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .to_lowercase();
        
        #[cfg(target_os = "macos")]
        {
            // 匹配所有 Antigravity 相关进程（主进程 + Helper 进程）
            if exe_path.contains("antigravity.app") {
                pids.push(pid.as_u32());
            }
        }
        
        #[cfg(target_os = "windows")]
        {
            let name = process.name().to_string_lossy().to_lowercase();
            if name.starts_with("antigravity") && name.ends_with(".exe") {
                pids.push(pid.as_u32());
            }
        }
        
        #[cfg(target_os = "linux")]
        {
            let name = process.name().to_string_lossy().to_lowercase();
            if name == "antigravity" || exe_path.contains("antigravity") {
                pids.push(pid.as_u32());
            }
        }
    }
    
    if !pids.is_empty() {
        crate::modules::logger::log_info(&format!("找到 {} 个 Antigravity 进程: {:?}", pids.len(), pids));
    }
    
    pids
}

/// 关闭 Antigravity 进程
pub fn close_antigravity(timeout_secs: u64) -> Result<(), String> {
    crate::modules::logger::log_info("正在关闭 Antigravity...");

    #[cfg(target_os = "windows")]
    {
        // Windows: 直接执行静默强杀 (Quiet Force Kill)
        // 模拟 cursor-free-vip 的逻辑：不尝试优雅关闭，直接使用 /F /IM 原子性强杀
        // 这被证明是处理 Antigravity 及其子进程最快且最干净的方式，避免了死锁和弹窗
        let _ = Command::new("taskkill")
            .args(["/F", "/IM", "Antigravity.exe"])
            .output();
            
        // 给一点点时间让系统清理 PID
        thread::sleep(Duration::from_millis(200));
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
            
            crate::modules::logger::log_info("正在分析进程列表以识别主进程:");
            for pid_u32 in &pids {
                let pid = sysinfo::Pid::from_u32(*pid_u32);
                if let Some(process) = system.process(pid) {
                    let name = process.name().to_string_lossy();
                    let args = process.cmd();
                    // sysinfo 0.31 returns &[OsString], so we need to convert to String
                    let args_str = args.iter()
                        .map(|arg| arg.to_string_lossy().into_owned())
                        .collect::<Vec<String>>()
                        .join(" ");
                    
                    crate::modules::logger::log_info(&format!(" - PID: {} | Name: {} | Args: {}", pid_u32, name, args_str));
                    
                    // 主进程通常没有 --type 参数，或者 args 很少
                    // 注意：开发环境下 (cargo tauri dev) 可能会有 cargo 相关的进程，需要小心
                    // 但这里 pids 列表已经通过 exe_path 过滤过了，应该是 Antigravity 的相关进程
                    
                    let is_helper_by_name = name.to_lowercase().contains("helper") 
                        || name.to_lowercase().contains("crashpad")
                        || name.to_lowercase().contains("language_server");
                        
                    let is_helper_by_args = args_str.contains("--type=");
                    
                    if !is_helper_by_name && !is_helper_by_args {
                        if main_pid.is_none() {
                            main_pid = Some(pid_u32);
                            crate::modules::logger::log_info(&format!("   => 识别为主进程 (Name/Args排除匹配)"));
                        } else {
                            crate::modules::logger::log_warn(&format!("   => 发现多个疑似主进程，保留第一个"));
                        }
                    } else {
                         crate::modules::logger::log_info(&format!("   => 识别为辅助进程 (Helper/Args)"));
                    }
                }
            }
            
            // 阶段 1: 优雅退出 (SIGTERM)
            if let Some(pid) = main_pid {
                crate::modules::logger::log_info(&format!("决定向主进程 PID: {} 发送 SIGTERM", pid));
                let output = Command::new("kill")
                    .args(["-15", &pid.to_string()])
                    .output();
                    
                if let Ok(result) = output {
                    if !result.status.success() {
                        let error = String::from_utf8_lossy(&result.stderr);
                        crate::modules::logger::log_warn(&format!("主进程 SIGTERM 失败: {}", error));
                    }
                }
            } else {
                crate::modules::logger::log_warn("未识别出明确的主进程，将尝试对所有进程发送 SIGTERM (可能导致弹窗)");
                for pid in &pids {
                    let _ = Command::new("kill").args(["-15", &pid.to_string()]).output();
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
                    crate::modules::logger::log_warn(&format!("优雅关闭超时，强制杀死 {} 个残留进程 (SIGKILL)", remaining_pids.len()));
                    for pid in &remaining_pids {
                        let output = Command::new("kill")
                            .args(["-9", &pid.to_string()])
                            .output();
                        
                        if let Ok(result) = output {
                            if !result.status.success() {
                                let error = String::from_utf8_lossy(&result.stderr);
                                if !error.contains("No such process") { // "No matching processes" for killall, "No such process" for kill
                                    crate::modules::logger::log_error(&format!("SIGKILL 进程 {} 失败: {}", pid, error));
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
            crate::modules::logger::log_warn("未找到 Antigravity 进程");
        }
    }

    #[cfg(target_os = "linux")]
    {
        // Linux: 使用 PID 精确控制
        let pids = get_antigravity_pids();
        if !pids.is_empty() {
            let pid = pids[0]; // 使用第一个找到的进程
            crate::modules::logger::log_info(&format!("尝试优雅关闭进程 {} (SIGTERM)", pid));
            let _ = Command::new("kill")
                .args(["-15", &pid.to_string()])
                .output();
            
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
            
            // 强制杀死
            if is_antigravity_running() {
                crate::modules::logger::log_warn(&format!("优雅关闭超时，强制杀死进程 {} (SIGKILL)", pid));
                let _ = Command::new("kill")
                    .args(["-9", &pid.to_string()])
                    .output();
                thread::sleep(Duration::from_secs(1));
            }
        } else {
            crate::modules::logger::log_warn("未找到 Antigravity 进程 PID，尝试使用 pkill");
            let _ = Command::new("pkill")
                .args(["-9", "antigravity"])
                .output();
            thread::sleep(Duration::from_secs(1));
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

    #[cfg(target_os = "macos")]
    {
        // 改进：使用 output() 等待 open 命令完成，以捕获"应用未找到"错误
        let output = Command::new("open")
            .args(["-a", "Antigravity"])
            .output()
            .map_err(|e| format!("无法执行 open 命令: {}", e))?;
            
        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(format!("启动失败 (open exited with {}): {}", output.status, error));
        }
    }

    #[cfg(target_os = "windows")]
    {
        // 尝试通过注册表或默认路径启动
        let result = Command::new("cmd")
            .args(["/C", "start", "antigravity://"])
            .spawn();
        
        if result.is_err() {
            return Err("启动失败，请手动打开 Antigravity".to_string());
        }
    }

    #[cfg(target_os = "linux")]
    {
        Command::new("antigravity")
            .spawn()
            .map_err(|e| format!("启动失败: {}", e))?;
    }

    crate::modules::logger::log_info("Antigravity 启动命令已发送");
    Ok(())
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

/// 从运行中的进程获取 Antigravity 可执行文件路径
/// 
/// 这是最可靠的方法，可以找到任意位置的安装
fn get_path_from_running_process() -> Option<std::path::PathBuf> {
    let mut system = System::new_all();
    system.refresh_all();
    
    for process in system.processes().values() {
        #[allow(unused_variables)]
        let name = process.name().to_string_lossy().to_lowercase();
        
        // 获取可执行文件路径
        if let Some(exe) = process.exe() {
            let exe_path = exe.to_str().unwrap_or("").to_lowercase();
            
            #[cfg(target_os = "macos")]
            {
                // macOS: 检查 Antigravity.app
                if exe_path.contains("antigravity.app") {
                    return Some(exe.to_path_buf());
                }
            }
            
            #[cfg(target_os = "windows")]
            {
                // Windows: 严格匹配进程名
                if name == "antigravity.exe" {
                    return Some(exe.to_path_buf());
                }
            }
            
            #[cfg(target_os = "linux")]
            {
                // Linux: 检查进程名或路径包含 antigravity
                if name.contains("antigravity") || exe_path.contains("antigravity") {
                    return Some(exe.to_path_buf());
                }
            }
        }
    }
    None
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
        let program_files = env::var("ProgramFiles")
            .unwrap_or_else(|_| "C:\\Program Files".to_string());
        let program_files_x86 = env::var("ProgramFiles(x86)")
            .unwrap_or_else(|_| "C:\\Program Files (x86)".to_string());
        
        let mut possible_paths = Vec::new();
        
        // 用户安装位置（优先）
        if let Some(local) = local_appdata {
            possible_paths.push(
                std::path::PathBuf::from(&local)
                    .join("Programs")
                    .join("Antigravity")
                    .join("Antigravity.exe")
            );
        }
        
        // 系统安装位置
        possible_paths.push(
            std::path::PathBuf::from(&program_files)
                .join("Antigravity")
                .join("Antigravity.exe")
        );
        
        // 32位兼容位置
        possible_paths.push(
            std::path::PathBuf::from(&program_files_x86)
                .join("Antigravity")
                .join("Antigravity.exe")
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
