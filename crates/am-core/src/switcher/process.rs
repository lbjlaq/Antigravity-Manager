use super::{is_antigravity_ide_str, SwitchEngine};
use crate::types::TargetHost;
use anyhow::Result;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

const ACTIVE_HOSTS_CACHE_TTL: Duration = Duration::from_secs(2);
static ACTIVE_HOSTS_CACHE: OnceLock<Mutex<Option<(Instant, crate::types::ActiveHostStatus)>>> =
    OnceLock::new();

impl SwitchEngine {
    /// 精简跨平台进程刷新：仅按需读取 cmd/exe（及可选 memory），避免拉取全量 env/cwd/disk 元数据，
    /// 显著降低 Windows `OpenProcess` 与 Linux `/proc` 遍历开销
    pub(crate) fn refresh_processes_minimal(sys: &mut System, include_memory: bool) {
        let mut kind = ProcessRefreshKind::new()
            .with_cmd(UpdateKind::OnlyIfNotSet)
            .with_exe(UpdateKind::OnlyIfNotSet);
        if include_memory {
            kind = kind.with_memory();
        }
        sys.refresh_processes_specifics(ProcessesToUpdate::All, kind);
    }

    /// 主动失效宿主进程探活缓存（如切号/杀进程后立即重新探测）
    pub fn invalidate_active_hosts_cache() {
        if let Some(lock) = ACTIVE_HOSTS_CACHE.get() {
            if let Ok(mut guard) = lock.lock() {
                *guard = None;
            }
        }
    }
    /// 排除本管理器/CLI/守护进程自身，防止误判或误杀
    pub(super) fn is_manager_or_tools_process(pid: u32, name: &str, exe: &str, cmd: &str) -> bool {
        if pid == std::process::id() {
            return true;
        }
        name.contains("antigravity-tools")
            || exe.contains("antigravity-tools")
            || name.contains("antigravity-manager")
            || exe.contains("antigravity-manager")
            || name.starts_with("am-gui")
            || name.starts_with("am-cli")
            || name.starts_with("am-daemon")
            || exe.contains("/am-gui")
            || exe.contains("\\am-gui")
            || exe.contains("/am-cli")
            || exe.contains("\\am-cli")
            || cmd.contains("antigravity-tools")
    }

    /// 判定是否为子进程/helper（排除出主进程根列表，确保 language_server 只能作为后代被定位）
    pub(super) fn is_helper_process(name: &str, cmd: &str, exe: &str) -> bool {
        cmd.contains("--type=")
            || name.contains("helper")
            || name.contains("crashpad")
            || name.contains("language_server")
            || exe.contains("language_server")
            || name == "agy"
            || name == "agy.exe"
            || exe.ends_with("/agy")
            || exe.ends_with("\\agy.exe")
    }

    /// 跨平台判定是否为 Antigravity 经典版进程（macOS / Windows / Linux），严格隔离 IDE 与管理器自身
    pub(super) fn is_classic_process(pid: u32, name: &str, exe: &str, cmd: &str) -> bool {
        if Self::is_manager_or_tools_process(pid, name, exe, cmd) {
            return false;
        }
        if is_antigravity_ide_str(name)
            || is_antigravity_ide_str(exe)
            || is_antigravity_ide_str(cmd)
        {
            return false;
        }
        exe.contains("antigravity.app")
            || cmd.contains("antigravity.app")
            || name == "antigravity"
            || name == "antigravity.exe"
            || exe.ends_with("/antigravity")
            || exe.ends_with("\\antigravity.exe")
            || exe.contains("\\antigravity\\")
            || exe.contains("/antigravity/")
    }

    /// 跨平台判定是否为 language_server / agy 语言服务子进程
    pub(super) fn is_language_server_process(
        name: &str,
        exe: &str,
        cmd: &str,
        include_agy: bool,
    ) -> bool {
        if name.contains("language_server") || exe.contains("language_server") {
            return true;
        }
        if include_agy {
            return name == "agy"
                || name == "agy.exe"
                || exe.ends_with("/agy")
                || exe.ends_with("\\agy.exe")
                || cmd.contains("--hub");
        }
        false
    }

    /// 从进程命令行参数中提取 --user-data-dir 路径（支持 `--user-data-dir=PATH` 与 `--user-data-dir PATH`）
    pub(super) fn extract_user_data_dir_from_cmd(args: &[std::ffi::OsString]) -> Option<PathBuf> {
        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            let s = arg.to_string_lossy();
            if let Some(val) = s.strip_prefix("--user-data-dir=") {
                if !val.trim().is_empty() {
                    return Some(PathBuf::from(val.trim()));
                }
            } else if s == "--user-data-dir" {
                if let Some(next) = iter.next() {
                    let val = next.to_string_lossy();
                    if !val.trim().is_empty() {
                        return Some(PathBuf::from(val.trim()));
                    }
                }
            }
        }
        None
    }

    /// 强制终止单个进程（Windows 用 taskkill /F /PID，Unix 用 kill -9）
    pub(super) fn force_kill_pid(pid: u32) {
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            let _ = std::process::Command::new("taskkill")
                .args(["/F", "/PID", &pid.to_string()])
                .creation_flags(0x08000000) // CREATE_NO_WINDOW
                .output();
        }

        #[cfg(not(target_os = "windows"))]
        {
            let _ = std::process::Command::new("kill")
                .args(["-9", &pid.to_string()])
                .output();
        }
    }

    /// 判定经典版是否正在运行
    pub fn is_classic_running() -> bool {
        let mut sys = System::new();
        Self::refresh_processes_minimal(&mut sys, false);

        for (pid, process) in sys.processes() {
            let name = process.name().to_string_lossy().to_lowercase();
            let exe = process
                .exe()
                .map(|p| p.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let cmd = process
                .cmd()
                .iter()
                .map(|s| s.to_string_lossy().to_lowercase())
                .collect::<Vec<_>>()
                .join(" ");

            if Self::is_classic_process(pid.as_u32(), &name, &exe, &cmd) {
                return true;
            }
        }
        false
    }

    /// 判定 Antigravity IDE 是否正在运行
    pub fn is_ide_running() -> bool {
        let mut sys = System::new();
        Self::refresh_processes_minimal(&mut sys, false);

        for (pid, process) in sys.processes() {
            let name = process.name().to_string_lossy().to_lowercase();
            let exe = process
                .exe()
                .map(|p| p.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let cmd = process
                .cmd()
                .iter()
                .map(|s| s.to_string_lossy().to_lowercase())
                .collect::<Vec<_>>()
                .join(" ");

            if Self::is_manager_or_tools_process(pid.as_u32(), &name, &exe, &cmd) {
                continue;
            }

            if is_antigravity_ide_str(&name)
                || is_antigravity_ide_str(&exe)
                || is_antigravity_ide_str(&cmd)
            {
                return true;
            }
        }
        false
    }

    /// 获取 Antigravity 经典版主进程根 PID（跨平台，排除 helper）
    pub(super) fn get_classic_main_pids(sys: &System) -> Vec<u32> {
        let mut roots = Vec::new();
        for (pid, process) in sys.processes() {
            let pid_u32 = pid.as_u32();
            let name = process.name().to_string_lossy().to_lowercase();
            let exe = process
                .exe()
                .map(|p| p.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let cmd = process
                .cmd()
                .iter()
                .map(|s| s.to_string_lossy().to_lowercase())
                .collect::<Vec<_>>()
                .join(" ");

            if Self::is_classic_process(pid_u32, &name, &exe, &cmd)
                && !Self::is_helper_process(&name, &cmd, &exe)
            {
                roots.push(pid_u32);
            }
        }
        roots
    }

    /// 获取 Antigravity IDE 主进程根 PID（跨平台，排除 helper 与管理器自身）
    pub(super) fn get_ide_main_pids(sys: &System) -> Vec<u32> {
        let mut roots = Vec::new();
        for (pid, process) in sys.processes() {
            let pid_u32 = pid.as_u32();
            let name = process.name().to_string_lossy().to_lowercase();
            let exe = process
                .exe()
                .map(|p| p.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let cmd = process
                .cmd()
                .iter()
                .map(|s| s.to_string_lossy().to_lowercase())
                .collect::<Vec<_>>()
                .join(" ");

            if Self::is_manager_or_tools_process(pid_u32, &name, &exe, &cmd) {
                continue;
            }

            if (is_antigravity_ide_str(&name)
                || is_antigravity_ide_str(&exe)
                || is_antigravity_ide_str(&cmd))
                && !Self::is_helper_process(&name, &cmd, &exe)
            {
                roots.push(pid_u32);
            }
        }
        roots
    }

    /// 广度优先展开进程后代 PID
    pub(super) fn expand_descendants(sys: &System, roots: &[u32]) -> Vec<u32> {
        let mut descendants = Vec::new();
        let mut frontier = roots.to_vec();
        let mut visited: std::collections::HashSet<u32> = roots.iter().cloned().collect();

        while let Some(parent) = frontier.pop() {
            for (pid, process) in sys.processes() {
                if process.parent().map(|p| p.as_u32()) == Some(parent) {
                    let pid_u32 = pid.as_u32();
                    if visited.insert(pid_u32) {
                        descendants.push(pid_u32);
                        frontier.push(pid_u32);
                    }
                }
            }
        }
        descendants
    }

    /// 热切：终止经典版旗下 language_server 并返回杀死的 PID 列表
    pub fn kill_classic_language_server_and_get_pids() -> Result<Vec<u32>> {
        let mut sys = System::new();
        Self::refresh_processes_minimal(&mut sys, false);

        let roots = Self::get_classic_main_pids(&sys);
        let descendants = Self::expand_descendants(&sys, &roots);

        let mut killed = Vec::new();
        for pid_u32 in descendants {
            if let Some(process) = sys.process(sysinfo::Pid::from_u32(pid_u32)) {
                let name = process.name().to_string_lossy().to_lowercase();
                let exe = process
                    .exe()
                    .map(|p| p.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                let cmd = process
                    .cmd()
                    .iter()
                    .map(|s| s.to_string_lossy().to_lowercase())
                    .collect::<Vec<_>>()
                    .join(" ");

                if Self::is_language_server_process(&name, &exe, &cmd, false) {
                    Self::force_kill_pid(pid_u32);
                    killed.push(pid_u32);
                }
            }
        }

        // 补救扫盲：若进程树断裂未展开，通过精准命令行特征匹配
        if killed.is_empty() {
            for (pid, process) in sys.processes() {
                let pid_u32 = pid.as_u32();
                let name = process.name().to_string_lossy().to_lowercase();
                let exe = process
                    .exe()
                    .map(|p| p.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                let cmd = process
                    .cmd()
                    .iter()
                    .map(|s| s.to_string_lossy().to_lowercase())
                    .collect::<Vec<_>>()
                    .join(" ");

                if Self::is_manager_or_tools_process(pid_u32, &name, &exe, &cmd)
                    || is_antigravity_ide_str(&exe)
                    || is_antigravity_ide_str(&cmd)
                {
                    continue;
                }

                if Self::is_language_server_process(&name, &exe, &cmd, false)
                    && (cmd.contains("--override_ide_name antigravity")
                        || cmd.contains("antigravity.app")
                        || exe.contains("antigravity"))
                {
                    Self::force_kill_pid(pid_u32);
                    killed.push(pid_u32);
                }
            }
        }

        Ok(killed)
    }

    /// 热切：终止经典版旗下 language_server
    pub fn kill_classic_language_server() -> Result<usize> {
        Self::kill_classic_language_server_and_get_pids().map(|pids| pids.len())
    }

    /// 热切：终止 IDE 旗下的 language_server 或 agy 并返回杀死的 PID 列表
    pub fn kill_ide_language_server_and_get_pids() -> Result<Vec<u32>> {
        let mut sys = System::new();
        Self::refresh_processes_minimal(&mut sys, false);

        let roots = Self::get_ide_main_pids(&sys);
        let descendants = Self::expand_descendants(&sys, &roots);

        let mut killed = Vec::new();
        for pid_u32 in descendants {
            if let Some(process) = sys.process(sysinfo::Pid::from_u32(pid_u32)) {
                let name = process.name().to_string_lossy().to_lowercase();
                let exe = process
                    .exe()
                    .map(|p| p.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                let cmd = process
                    .cmd()
                    .iter()
                    .map(|s| s.to_string_lossy().to_lowercase())
                    .collect::<Vec<_>>()
                    .join(" ");

                if Self::is_language_server_process(&name, &exe, &cmd, true) {
                    Self::force_kill_pid(pid_u32);
                    killed.push(pid_u32);
                }
            }
        }
        Ok(killed)
    }

    /// 热切：终止 IDE 旗下的 language_server 或 agy
    pub fn kill_ide_language_server() -> Result<usize> {
        Self::kill_ide_language_server_and_get_pids().map(|pids| pids.len())
    }

    /// 等待经典版 language_server 重新拉起 (敏捷 100ms 快速轮询 + 排除旧 PID)
    pub fn wait_for_classic_language_server_respawn_excluding(
        timeout_secs: u64,
        killed_pids: &[u32],
    ) -> bool {
        let deadline = Instant::now() + Duration::from_secs(timeout_secs);
        let mut sys = System::new();
        Self::refresh_processes_minimal(&mut sys, false);
        let roots = Self::get_classic_main_pids(&sys);

        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
            Self::refresh_processes_minimal(&mut sys, false);

            let descendants = Self::expand_descendants(&sys, &roots);
            for pid_u32 in &descendants {
                if killed_pids.contains(pid_u32) {
                    continue;
                }
                if let Some(process) = sys.process(sysinfo::Pid::from_u32(*pid_u32)) {
                    let name = process.name().to_string_lossy().to_lowercase();
                    let exe = process
                        .exe()
                        .map(|p| p.to_string_lossy().to_lowercase())
                        .unwrap_or_default();
                    let cmd = process
                        .cmd()
                        .iter()
                        .map(|s| s.to_string_lossy().to_lowercase())
                        .collect::<Vec<_>>()
                        .join(" ");
                    if Self::is_language_server_process(&name, &exe, &cmd, false) {
                        return true;
                    }
                }
            }

            // 兜底命令行特征扫描新 PID
            for (pid, process) in sys.processes() {
                let pid_u32 = pid.as_u32();
                if killed_pids.contains(&pid_u32) {
                    continue;
                }
                let name = process.name().to_string_lossy().to_lowercase();
                let exe = process
                    .exe()
                    .map(|p| p.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                let cmd = process
                    .cmd()
                    .iter()
                    .map(|s| s.to_string_lossy().to_lowercase())
                    .collect::<Vec<_>>()
                    .join(" ");

                if Self::is_manager_or_tools_process(pid_u32, &name, &exe, &cmd)
                    || is_antigravity_ide_str(&exe)
                    || is_antigravity_ide_str(&cmd)
                {
                    continue;
                }

                if Self::is_language_server_process(&name, &exe, &cmd, false)
                    && (cmd.contains("--override_ide_name antigravity")
                        || cmd.contains("antigravity.app")
                        || exe.contains("antigravity"))
                {
                    return true;
                }
            }
        }
        false
    }

    /// 等待经典版 language_server 重新拉起 (有界轮询 15s)
    pub fn wait_for_classic_language_server_respawn(timeout_secs: u64) -> bool {
        Self::wait_for_classic_language_server_respawn_excluding(timeout_secs, &[])
    }

    /// 等待 IDE 语言服务/agy 重新拉起 (敏捷 100ms 快速轮询 + 预缓存主进程 roots + 排除旧 PID)
    pub fn wait_for_ide_language_server_respawn_excluding(
        timeout_secs: u64,
        killed_pids: &[u32],
    ) -> bool {
        let deadline = Instant::now() + Duration::from_secs(timeout_secs);
        let mut sys = System::new();
        Self::refresh_processes_minimal(&mut sys, false);
        let roots = Self::get_ide_main_pids(&sys);

        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
            Self::refresh_processes_minimal(&mut sys, false);

            let current_roots = if !roots.is_empty() {
                roots.clone()
            } else {
                Self::get_ide_main_pids(&sys)
            };
            let descendants = Self::expand_descendants(&sys, &current_roots);

            for pid_u32 in descendants {
                if killed_pids.contains(&pid_u32) {
                    continue;
                }
                if let Some(process) = sys.process(sysinfo::Pid::from_u32(pid_u32)) {
                    let name = process.name().to_string_lossy().to_lowercase();
                    let exe = process
                        .exe()
                        .map(|p| p.to_string_lossy().to_lowercase())
                        .unwrap_or_default();
                    let cmd = process
                        .cmd()
                        .iter()
                        .map(|s| s.to_string_lossy().to_lowercase())
                        .collect::<Vec<_>>()
                        .join(" ");

                    if Self::is_language_server_process(&name, &exe, &cmd, true) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// 等待 IDE 语言服务/agy 重新拉起 (有界轮询 15s)
    pub fn wait_for_ide_language_server_respawn(timeout_secs: u64) -> bool {
        Self::wait_for_ide_language_server_respawn_excluding(timeout_secs, &[])
    }

    /// 关闭经典版所有进程
    pub fn kill_classic_app() {
        let mut sys = System::new();
        Self::refresh_processes_minimal(&mut sys, false);

        for (pid, process) in sys.processes() {
            let pid_u32 = pid.as_u32();
            let name = process.name().to_string_lossy().to_lowercase();
            let exe = process
                .exe()
                .map(|p| p.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let cmd = process
                .cmd()
                .iter()
                .map(|s| s.to_string_lossy().to_lowercase())
                .collect::<Vec<_>>()
                .join(" ");

            if Self::is_classic_process(pid_u32, &name, &exe, &cmd) {
                Self::force_kill_pid(pid_u32);
            }
        }
        Self::invalidate_active_hosts_cache();
    }

    /// 关闭 IDE 所有进程
    pub fn kill_ide_app() {
        let mut sys = System::new();
        Self::refresh_processes_minimal(&mut sys, false);

        for (pid, process) in sys.processes() {
            let pid_u32 = pid.as_u32();
            let name = process.name().to_string_lossy().to_lowercase();
            let exe = process
                .exe()
                .map(|p| p.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let cmd = process
                .cmd()
                .iter()
                .map(|s| s.to_string_lossy().to_lowercase())
                .collect::<Vec<_>>()
                .join(" ");

            if Self::is_manager_or_tools_process(pid_u32, &name, &exe, &cmd) {
                continue;
            }

            if is_antigravity_ide_str(&name)
                || is_antigravity_ide_str(&exe)
                || is_antigravity_ide_str(&cmd)
            {
                Self::force_kill_pid(pid_u32);
            }
        }
        Self::invalidate_active_hosts_cache();
    }

    /// 根据特征匹配关闭单个进程（如 agy_acp_server）
    pub fn kill_process_by_pattern(pattern: &str) {
        let mut sys = System::new();
        Self::refresh_processes_minimal(&mut sys, false);

        let pattern_lower = pattern.to_lowercase();
        for (pid, process) in sys.processes() {
            let pid_u32 = pid.as_u32();
            let name = process.name().to_string_lossy().to_lowercase();
            let exe = process
                .exe()
                .map(|p| p.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let cmd = process
                .cmd()
                .iter()
                .map(|s| s.to_string_lossy().to_lowercase())
                .collect::<Vec<_>>()
                .join(" ");

            if Self::is_manager_or_tools_process(pid_u32, &name, &exe, &cmd) {
                continue;
            }

            if name.contains(&pattern_lower)
                || exe.contains(&pattern_lower)
                || cmd.contains(&pattern_lower)
            {
                let _ = process.kill();
            }
        }
        Self::invalidate_active_hosts_cache();
    }

    /// 跨平台优雅激活/唤起目标应用窗口至前台 (冷启动或前台聚焦，自动回收子进程防止 `<defunct>` 僵尸)
    pub fn activate_application(app_name: &str) {
        #[cfg(target_os = "macos")]
        {
            let is_ide = app_name.contains("IDE");
            let target_bundle = if is_ide {
                "Antigravity IDE.app"
            } else {
                "Antigravity.app"
            };

            let mut cmd = std::process::Command::new("open");
            cmd.env_remove("ELECTRON_RUN_AS_NODE");
            cmd.arg("-a").arg(target_bundle);
            crate::runtime::spawn_and_reap(cmd);

            let target_app = if is_ide {
                "Antigravity IDE"
            } else {
                "Antigravity"
            };
            let script = format!("tell application \"{}\" to activate", target_app);
            let mut osa = std::process::Command::new("osascript");
            osa.arg("-e").arg(&script);
            crate::runtime::spawn_and_reap(osa);
        }

        #[cfg(target_os = "windows")]
        {
            if let Some(local_app) = dirs::data_local_dir() {
                let exe = local_app
                    .join("Programs")
                    .join(app_name)
                    .join(format!("{}.exe", app_name));
                if exe.exists() {
                    let _ = std::process::Command::new(exe)
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn();
                }
            }
        }

        #[cfg(target_os = "linux")]
        {
            // 使用位置参数 `$1` 安全后台脱钩，1ms 内同步回收 `sh`，由 init 接管长期运行的 GUI 进程（零僵尸、零线程滞留）
            let bin = app_name.to_lowercase();
            let _ = std::process::Command::new("sh")
                .args(["-c", "\"$1\" >/dev/null 2>&1 &", "--", &bin])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
    }

    /// 判定是否为终端独立运行的 `agy` CLI 进程（严格排除 Gemini.app / Antigravity.app / IDE 内嵌的 Helper 与 Hub 语言服务）
    pub(super) fn is_standalone_agy_cli_process(name: &str, exe: &str, cmd: &str) -> bool {
        let is_agy_bin = name == "agy"
            || name == "agy.exe"
            || exe.ends_with("/agy")
            || exe.ends_with("\\agy.exe");
        if !is_agy_bin {
            return false;
        }
        // 排除 macOS/Windows/Linux 桌面应用包内嵌的 helper / hub 子进程（例如 /Applications/Gemini.app/Contents/Helpers/agy --hub）
        if exe.contains(".app/contents/")
            || exe.contains("/helpers/")
            || exe.contains("\\helpers\\")
            || exe.contains("/resources/")
            || exe.contains("\\resources\\")
            || is_antigravity_ide_str(exe)
            || is_antigravity_ide_str(cmd)
            || cmd.contains("--hub")
            || cmd.contains("--use_host_auth")
            || cmd.contains("--host_bridge_url")
            || cmd.contains("--lsp")
            || cmd.contains("--language-server")
        {
            return false;
        }
        true
    }

    /// 从已刷新的单份 `System` 快照中单遍探测四个宿主的运行状态（避免重复扫描 OS 进程表）
    pub(crate) fn detect_active_hosts_from_sys(sys: &System) -> crate::types::ActiveHostStatus {
        let mut app_running = false;
        let mut ide_running = false;
        let mut plugin_running = false;
        let mut agy_running = false;

        for (pid, process) in sys.processes() {
            if app_running && ide_running && plugin_running && agy_running {
                break;
            }

            let pid_u32 = pid.as_u32();
            let name = process.name().to_string_lossy().to_lowercase();
            let exe = process
                .exe()
                .map(|p| p.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let cmd = process
                .cmd()
                .iter()
                .map(|s| s.to_string_lossy().to_lowercase())
                .collect::<Vec<_>>()
                .join(" ");

            if !plugin_running
                && (name.contains("agy_acp_server") || cmd.contains("agy_acp_server"))
            {
                plugin_running = true;
            }

            if !agy_running && Self::is_standalone_agy_cli_process(&name, &exe, &cmd) {
                agy_running = true;
            }

            if (!app_running || !ide_running)
                && !Self::is_manager_or_tools_process(pid_u32, &name, &exe, &cmd)
            {
                if !ide_running
                    && (is_antigravity_ide_str(&name)
                        || is_antigravity_ide_str(&exe)
                        || is_antigravity_ide_str(&cmd))
                {
                    ide_running = true;
                } else if !app_running && Self::is_classic_process(pid_u32, &name, &exe, &cmd) {
                    app_running = true;
                }
            }
        }

        let app_version = crate::version::get_antigravity_version(TargetHost::App)
            .ok()
            .map(|v| v.short_version);
        let ide_version = crate::version::get_antigravity_version(TargetHost::Ide)
            .ok()
            .map(|v| v.short_version);

        crate::types::ActiveHostStatus {
            app_account: None,
            app_running,
            app_version,
            ide_account: None,
            ide_running,
            ide_version,
            plugin_account: None,
            plugin_running,
            agy_account: None,
            agy_running,
        }
    }

    /// 探测各宿主在系统中的实时激活与运行状态 (跨平台：单遍精简扫描 + 2s 短时节流缓存)
    pub fn detect_active_hosts() -> crate::types::ActiveHostStatus {
        let cache = ACTIVE_HOSTS_CACHE.get_or_init(|| Mutex::new(None));
        if let Ok(guard) = cache.lock() {
            if let Some((ts, ref cached)) = *guard {
                if ts.elapsed() < ACTIVE_HOSTS_CACHE_TTL {
                    return cached.clone();
                }
            }
        }

        let mut sys = System::new();
        Self::refresh_processes_minimal(&mut sys, false);
        let status = Self::detect_active_hosts_from_sys(&sys);

        if let Ok(mut guard) = cache.lock() {
            *guard = Some((Instant::now(), status.clone()));
        }
        status
    }
}
