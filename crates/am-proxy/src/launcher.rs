use crate::types::{ProxyError, TargetApp};
use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Windows GUI 进程派生辅助子进程时强制隐藏控制台窗口，杜绝 conhost / Windows Terminal 闪烁弹窗
#[cfg(target_os = "windows")]
fn hide_console_window(cmd: &mut Command) -> &mut Command {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(CREATE_NO_WINDOW)
}

/// 默认绕过本地回环与内网地址（完整覆盖 localhost / 127.0.0.0/8 / 局域网 / <-loopback>，彻底杜绝 Webview 走代理报错）
pub const DEFAULT_NO_PROXY: &str =
    "localhost,127.0.0.1,::1,127.0.0.0/8,10.0.0.0/8,192.168.0.0/16,172.16.0.0/12,*.local";

/// 生成跨平台通用的代理环境变量集合（同时覆盖 Electron、Node.js 22+ fetch、Go/Rust language_server）
pub fn build_proxy_env(proxy_http_url: &str) -> Vec<(String, String)> {
    vec![
        ("HTTP_PROXY".into(), proxy_http_url.into()),
        ("HTTPS_PROXY".into(), proxy_http_url.into()),
        ("ALL_PROXY".into(), proxy_http_url.into()),
        ("http_proxy".into(), proxy_http_url.into()),
        ("https_proxy".into(), proxy_http_url.into()),
        ("all_proxy".into(), proxy_http_url.into()),
        ("NO_PROXY".into(), DEFAULT_NO_PROXY.into()),
        ("no_proxy".into(), DEFAULT_NO_PROXY.into()),
        ("NODE_USE_ENV_PROXY".into(), "1".into()),
    ]
}

/// 获取 Antigravity IDE 的用户级 `settings.json` 路径（Win / macOS / Linux）
pub fn ide_user_settings_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    #[cfg(target_os = "macos")]
    {
        Some(
            home.join("Library")
                .join("Application Support")
                .join("Antigravity IDE")
                .join("User")
                .join("settings.json"),
        )
    }
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData").join("Roaming"));
        Some(
            appdata
                .join("Antigravity IDE")
                .join("User")
                .join("settings.json"),
        )
    }
    #[cfg(target_os = "linux")]
    {
        let cfg = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));
        Some(
            cfg.join("Antigravity IDE")
                .join("User")
                .join("settings.json"),
        )
    }
}

/// 读取 Antigravity IDE `settings.json` 中当前配置的 `http.proxy`
pub fn get_ide_settings_proxy() -> Option<String> {
    let path = ide_user_settings_path()?;
    let raw = std::fs::read_to_string(path).ok()?;
    let val: Value = serde_json::from_str(&raw).ok()?;
    val.get("http.proxy")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// 同步或清除 Antigravity IDE 用户级 `settings.json` 的代理与本地回环绕过配置（零侵入 .app，直启也生效）
pub fn sync_ide_proxy_settings(proxy_url: Option<&str>) -> Result<(), ProxyError> {
    let Some(path) = ide_user_settings_path() else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut obj = if path.exists() {
        let raw = std::fs::read_to_string(&path).unwrap_or_default();
        serde_json::from_str::<serde_json::Map<String, Value>>(&raw).unwrap_or_default()
    } else {
        serde_json::Map::new()
    };

    match proxy_url.map(str::trim).filter(|s| !s.is_empty()) {
        Some(url) => {
            obj.insert("http.proxy".into(), Value::String(url.to_string()));
            obj.insert("http.proxySupport".into(), Value::String("override".into()));
            obj.insert(
                "http.noProxy".into(),
                serde_json::json!(["localhost", "127.0.0.1", "::1", "127.0.0.0/8", "*.local"]),
            );
        }
        None => {
            obj.remove("http.proxy");
            obj.remove("http.proxySupport");
            obj.remove("http.noProxy");
        }
    }

    let formatted = serde_json::to_string_pretty(&Value::Object(obj))
        .map_err(|e| ProxyError::Protocol(e.to_string()))?;
    std::fs::write(&path, formatted)?;
    Ok(())
}

/// 解析目标应用在当前操作系统的可执行文件路径
pub fn resolve_target_binary(target: TargetApp) -> Result<PathBuf, ProxyError> {
    #[cfg(target_os = "macos")]
    {
        let mut candidates: Vec<PathBuf> = match target {
            TargetApp::AntigravityApp => vec![PathBuf::from(
                "/Applications/Antigravity.app/Contents/MacOS/Antigravity",
            )],
            TargetApp::AntigravityIde => vec![PathBuf::from(
                "/Applications/Antigravity IDE.app/Contents/MacOS/Electron",
            )],
            TargetApp::AgyCli => Vec::new(),
        };
        if let Some(home) = dirs::home_dir() {
            match target {
                TargetApp::AntigravityApp => candidates
                    .push(home.join("Applications/Antigravity.app/Contents/MacOS/Antigravity")),
                TargetApp::AntigravityIde => candidates
                    .push(home.join("Applications/Antigravity IDE.app/Contents/MacOS/Electron")),
                TargetApp::AgyCli => {
                    candidates.push(home.join(".local/bin/agy"));
                    candidates.push(PathBuf::from("/usr/local/bin/agy"));
                    candidates.push(PathBuf::from("/opt/homebrew/bin/agy"));
                }
            }
        }
        for p in candidates {
            if p.exists() {
                return Ok(p);
            }
        }
        Err(ProxyError::AppNotFound(target.to_string()))
    }

    #[cfg(target_os = "windows")]
    {
        let local_app = dirs::data_local_dir().unwrap_or_default();
        let prog_files = std::env::var_os("ProgramFiles")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));
        let prog_files_x86 = std::env::var_os("ProgramFiles(x86)")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)"));

        let mut candidates: Vec<PathBuf> = match target {
            TargetApp::AntigravityApp => vec![
                local_app.join(r"Programs\antigravity\Antigravity.exe"),
                local_app.join(r"Programs\Antigravity\Antigravity.exe"),
                prog_files.join(r"Antigravity\Antigravity.exe"),
                prog_files_x86.join(r"Antigravity\Antigravity.exe"),
            ],
            TargetApp::AntigravityIde => vec![
                local_app.join(r"Programs\Antigravity IDE\Antigravity IDE.exe"),
                prog_files.join(r"Antigravity IDE\Antigravity IDE.exe"),
                prog_files_x86.join(r"Antigravity IDE\Antigravity IDE.exe"),
            ],
            TargetApp::AgyCli => {
                let mut list = vec![
                    local_app.join(r"Programs\Antigravity\bin\agy.cmd"),
                    local_app.join(r"Programs\antigravity\bin\agy.cmd"),
                ];
                if let Some(home) = dirs::home_dir() {
                    list.push(home.join(r".local\bin\agy.cmd"));
                    list.push(home.join(r".local\bin\agy.exe"));
                }
                if let Some(path_var) = std::env::var_os("PATH") {
                    for dir in std::env::split_paths(&path_var) {
                        list.push(dir.join("agy.cmd"));
                        list.push(dir.join("agy.exe"));
                    }
                }
                list
            }
        };
        for p in candidates.drain(..) {
            if p.exists() {
                return Ok(p);
            }
        }

        // 兜底：若用户自定义安装到非默认盘符（如 D:\...）且进程正在运行，直接从进程表反查真实 EXE 路径
        if matches!(
            target,
            TargetApp::AntigravityApp | TargetApp::AntigravityIde
        ) {
            use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
            let expected = match target {
                TargetApp::AntigravityApp => "Antigravity.exe",
                TargetApp::AntigravityIde => "Antigravity IDE.exe",
                TargetApp::AgyCli => "",
            };
            let mut sys = System::new();
            sys.refresh_processes_specifics(ProcessesToUpdate::All, ProcessRefreshKind::new());
            let matched_pids: Vec<sysinfo::Pid> = sys
                .processes()
                .iter()
                .filter_map(|(&pid, p)| {
                    p.name()
                        .to_string_lossy()
                        .eq_ignore_ascii_case(expected)
                        .then_some(pid)
                })
                .collect();
            if !matched_pids.is_empty() {
                sys.refresh_processes_specifics(
                    ProcessesToUpdate::Some(&matched_pids),
                    ProcessRefreshKind::new().with_exe(UpdateKind::OnlyIfNotSet),
                );
                for pid in matched_pids {
                    if let Some(exe) = sys.process(pid).and_then(|p| p.exe()) {
                        if exe.exists() {
                            return Ok(exe.to_path_buf());
                        }
                    }
                }
            }
        }

        Err(ProxyError::AppNotFound(target.to_string()))
    }

    #[cfg(target_os = "linux")]
    {
        let candidates: Vec<PathBuf> = match target {
            TargetApp::AntigravityApp => vec![
                PathBuf::from("/usr/bin/antigravity"),
                PathBuf::from("/usr/local/bin/antigravity"),
                PathBuf::from("/opt/Antigravity/antigravity"),
            ],
            TargetApp::AntigravityIde => vec![
                PathBuf::from("/usr/bin/antigravity-ide"),
                PathBuf::from("/usr/local/bin/antigravity-ide"),
                PathBuf::from("/opt/Antigravity IDE/antigravity-ide"),
            ],
            TargetApp::AgyCli => {
                let mut list = Vec::new();
                if let Some(home) = dirs::home_dir() {
                    list.push(home.join(".local/bin/agy"));
                }
                list.push(PathBuf::from("/usr/local/bin/agy"));
                list
            }
        };
        for p in candidates {
            if p.exists() {
                return Ok(p);
            }
        }
        Err(ProxyError::AppNotFound(target.to_string()))
    }
}

/// 以外部注入模式启动目标应用（零侵入原应用包，100% 免疫官方更新）
pub fn launch_with_proxy(
    target: TargetApp,
    proxy_http_url: &str,
    extra_args: &[String],
) -> Result<u32, ProxyError> {
    let bin = resolve_target_binary(target)?;

    if target == TargetApp::AntigravityIde {
        // 清理历史写入的 http.proxySupport=override，避免 VS Code proxy-agent 拦截内置 Webview 本地回环流量
        let _ = sync_ide_proxy_settings(None);
    }

    if matches!(
        target,
        TargetApp::AntigravityApp | TargetApp::AntigravityIde
    ) {
        graceful_terminate_root_host(target);
    }

    #[cfg(target_os = "windows")]
    let mut cmd = {
        let is_batch_shim = bin
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"))
            .unwrap_or(false);
        if is_batch_shim {
            let mut c = Command::new("cmd");
            hide_console_window(&mut c);
            c.arg("/C").arg(&bin);
            c
        } else {
            Command::new(&bin)
        }
    };
    #[cfg(not(target_os = "windows"))]
    let mut cmd = Command::new(&bin);
    for (k, v) in build_proxy_env(proxy_http_url) {
        cmd.env(k, v);
    }
    cmd.args(extra_args);
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    let child = cmd.spawn()?;
    Ok(child.id())
}

/// 仅定位目标宿主的主进程根 PID 发送 SIGTERM 并等待优雅退出（绝不粗暴 pkill 子进程破坏 Webview）
fn graceful_terminate_root_host(target: TargetApp) {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let Ok(out) = Command::new("ps").args(["-axo", "pid=,command="]).output() else {
            return;
        };
        let stdout = String::from_utf8_lossy(&out.stdout);
        let mut root_pids = Vec::new();
        for line in stdout.lines() {
            let trimmed = line.trim();
            let Some((pid_str, cmd_str)) = trimmed.split_once(char::is_whitespace) else {
                continue;
            };
            let cmd = cmd_str.trim();
            // 严格跳过所有 Helper / Renderer / Utility / Node Worker 进程
            if cmd.contains("--type=")
                || cmd.contains("--crashpad")
                || cmd.contains("Helper")
                || cmd.contains("sidecar_sdk")
                || cmd.contains("--ms-enable-electron-run-as-node")
            {
                continue;
            }
            let matched = match target {
                TargetApp::AntigravityApp => {
                    cmd.contains("Antigravity.app/Contents/MacOS/Antigravity")
                        || cmd.ends_with("/antigravity")
                }
                TargetApp::AntigravityIde => {
                    cmd.contains("Antigravity IDE.app/Contents/MacOS/Electron")
                        || cmd.ends_with("/antigravity-ide")
                }
                TargetApp::AgyCli => false,
            };
            if matched {
                if let Ok(pid) = pid_str.trim().parse::<u32>() {
                    root_pids.push(pid);
                }
            }
        }

        if root_pids.is_empty() {
            return;
        }

        for &pid in &root_pids {
            let _ = Command::new("kill")
                .args(["-TERM", &pid.to_string()])
                .status();
        }

        for _ in 0..28 {
            let any_alive = root_pids.iter().any(|&pid| {
                Command::new("kill")
                    .args(["-0", &pid.to_string()])
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false)
            });
            if !any_alive {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(80));
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }

    #[cfg(target_os = "windows")]
    {
        let img = match target {
            TargetApp::AntigravityApp => "Antigravity.exe",
            TargetApp::AntigravityIde => "Antigravity IDE.exe",
            TargetApp::AgyCli => return,
        };
        let script = format!(
            "$procs = Get-Process -Name '{}' -ErrorAction SilentlyContinue; foreach ($p in $procs) {{ if ($p.MainWindowHandle -ne 0) {{ [void]$p.CloseMainWindow(); [void]$p.WaitForExit(2200) }} }}",
            img.trim_end_matches(".exe")
        );
        let mut cmd = Command::new("powershell");
        hide_console_window(&mut cmd);
        let _ = cmd
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-WindowStyle",
                "Hidden",
                "-Command",
                &script,
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

/// 目标应用当前运行状态与免 TUN 代理注入检测结果
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct AppRuntimeStatus {
    pub running: bool,
    pub proxy_active: bool,
    pub proxy_url: Option<String>,
}

/// 检测目标应用是否正在运行，以及是否处于免 TUN 代理注入状态
pub fn get_app_runtime_status(target: TargetApp) -> AppRuntimeStatus {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let needle = match target {
            TargetApp::AntigravityApp => {
                if cfg!(target_os = "macos") {
                    "Antigravity.app/Contents/MacOS/Antigravity"
                } else {
                    "antigravity"
                }
            }
            TargetApp::AntigravityIde => {
                if cfg!(target_os = "macos") {
                    "Antigravity IDE.app/Contents/MacOS/Electron"
                } else {
                    "antigravity-ide"
                }
            }
            TargetApp::AgyCli => return AppRuntimeStatus::default(),
        };

        let Ok(out) = Command::new("ps").args(["-axo", "pid=,command="]).output() else {
            return AppRuntimeStatus::default();
        };
        let text = String::from_utf8_lossy(&out.stdout);
        let mut pids = Vec::new();
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.contains(needle) && !trimmed.contains("--type=") {
                // Linux 下排除 antigravity-ide / antigravity-manager / am-gui 误匹配经典版 Antigravity
                #[cfg(target_os = "linux")]
                if target == TargetApp::AntigravityApp {
                    let lower = trimmed.to_ascii_lowercase();
                    if lower.contains("antigravity-ide")
                        || lower.contains("antigravity ide")
                        || lower.contains("antigravity-manager")
                        || lower.contains("am-gui")
                    {
                        continue;
                    }
                }
                if let Some(pid_str) = trimmed.split_whitespace().next() {
                    if let Ok(pid) = pid_str.parse::<u32>() {
                        pids.push(pid);
                    }
                }
            }
        }
        if pids.is_empty() {
            return AppRuntimeStatus::default();
        }

        for pid in pids {
            #[cfg(target_os = "linux")]
            if let Ok(raw) = std::fs::read(format!("/proc/{}/environ", pid)) {
                for entry in raw.split(|&b| b == 0) {
                    let s = String::from_utf8_lossy(entry);
                    if let Some(val) = s
                        .strip_prefix("HTTP_PROXY=")
                        .or_else(|| s.strip_prefix("HTTPS_PROXY="))
                        .or_else(|| s.strip_prefix("http_proxy="))
                    {
                        if !val.trim().is_empty() {
                            return AppRuntimeStatus {
                                running: true,
                                proxy_active: true,
                                proxy_url: Some(val.trim().to_string()),
                            };
                        }
                    }
                }
            }

            #[cfg(target_os = "macos")]
            if let Ok(env_out) = Command::new("ps")
                .args(["eww", "-p", &pid.to_string()])
                .output()
            {
                let env_text = String::from_utf8_lossy(&env_out.stdout);
                for token in env_text.split_whitespace() {
                    if let Some(val) = token
                        .strip_prefix("HTTP_PROXY=")
                        .or_else(|| token.strip_prefix("HTTPS_PROXY="))
                        .or_else(|| token.strip_prefix("http_proxy="))
                    {
                        if !val.trim().is_empty() {
                            return AppRuntimeStatus {
                                running: true,
                                proxy_active: true,
                                proxy_url: Some(val.trim().to_string()),
                            };
                        }
                    }
                }
            }
        }

        return AppRuntimeStatus {
            running: true,
            proxy_active: false,
            proxy_url: None,
        };
    }

    #[cfg(target_os = "windows")]
    {
        use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

        let target_names: &[&str] = match target {
            TargetApp::AntigravityApp => &["Antigravity.exe", "Antigravity"],
            TargetApp::AntigravityIde => &["Antigravity IDE.exe", "Antigravity IDE"],
            TargetApp::AgyCli => return AppRuntimeStatus::default(),
        };

        // 第一阶段：使用 NtQuerySystemInformation 轻量获取全量进程名（零子进程、零控制台弹窗、零多余 OpenProcess）
        let mut sys = System::new();
        sys.refresh_processes_specifics(ProcessesToUpdate::All, ProcessRefreshKind::new());

        let matching_pids: Vec<sysinfo::Pid> = sys
            .processes()
            .iter()
            .filter_map(|(&pid, proc_)| {
                let name = proc_.name().to_string_lossy();
                if target_names
                    .iter()
                    .any(|expected| name.eq_ignore_ascii_case(expected))
                {
                    Some(pid)
                } else {
                    None
                }
            })
            .collect();

        if matching_pids.is_empty() {
            return AppRuntimeStatus::default();
        }

        // 第二阶段：仅对命中的宿主进程按需拉取 cmd + environ，探测免 TUN 代理注入状态
        sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&matching_pids),
            ProcessRefreshKind::new()
                .with_cmd(UpdateKind::OnlyIfNotSet)
                .with_environ(UpdateKind::OnlyIfNotSet),
        );

        for pid in matching_pids {
            let Some(proc_) = sys.process(pid) else {
                continue;
            };
            let is_helper = proc_
                .cmd()
                .iter()
                .any(|arg| arg.to_string_lossy().contains("--type="));
            if is_helper {
                continue;
            }
            for entry in proc_.environ() {
                let s = entry.to_string_lossy();
                if let Some((key, val)) = s.split_once('=') {
                    if (key.eq_ignore_ascii_case("HTTP_PROXY")
                        || key.eq_ignore_ascii_case("HTTPS_PROXY"))
                        && !val.trim().is_empty()
                    {
                        return AppRuntimeStatus {
                            running: true,
                            proxy_active: true,
                            proxy_url: Some(val.trim().to_string()),
                        };
                    }
                }
            }
        }

        return AppRuntimeStatus {
            running: true,
            proxy_active: false,
            proxy_url: None,
        };
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    AppRuntimeStatus::default()
}
