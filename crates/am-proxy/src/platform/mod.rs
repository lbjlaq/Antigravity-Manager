use crate::types::{ProxyError, ProxyNode, ProxyProtocol, ProxySource, TargetApp};
use std::path::{Path, PathBuf};

const HOOK_START: &str = "# >>> am-proxy managed block >>>";
const HOOK_END: &str = "# <<< am-proxy managed block <<<";

/// 常见 VPN / 代理客户端进程名关键字（大小写不敏感）
const KNOWN_PROXY_PROCESS_KEYWORDS: &[&str] = &[
    "shadowroc",
    "clash",
    "mihomo",
    "surge",
    "v2ray",
    "xray",
    "sing-box",
    "singbox",
    "stash",
    "loon",
    "quantumul",
    "nekoray",
    "nekobox",
    "flclash",
    "karing",
    "hiddify",
    "shadowsocks",
    "ss-local",
    "privoxy",
    "trojan",
    "hysteria",
    "naive",
    "v2raya",
    "verge",
];

/// 智能扫描本机正在监听 TCP 端口的常见 VPN / 代理进程（无论端口是否为默认值）
pub fn detect_listening_proxy_processes() -> Vec<ProxyNode> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let mut nodes = Vec::new();
        let output = match std::process::Command::new("lsof")
            .args(["-nP", "-iTCP", "-sTCP:LISTEN"])
            .output()
        {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
            _ => return nodes,
        };

        for line in output.lines().skip(1) {
            let cols: Vec<&str> = line.split_whitespace().collect();
            if cols.len() < 9 {
                continue;
            }
            let proc_name = cols[0];
            let lower = proc_name.to_ascii_lowercase();
            if !KNOWN_PROXY_PROCESS_KEYWORDS
                .iter()
                .any(|k| lower.contains(k))
            {
                continue;
            }
            // 通常形如 *:1082 或 127.0.0.1:7890 或 [::1]:7890
            let addr_col = cols[cols.len() - 2];
            if let Some((raw_host, raw_port)) = addr_col.rsplit_once(':') {
                if let Ok(port) = raw_port.parse::<u16>() {
                    let host = if raw_host == "*" || raw_host == "0.0.0.0" || raw_host == "[::]" {
                        "127.0.0.1".to_string()
                    } else {
                        raw_host.trim_matches(['[', ']']).to_string()
                    };
                    nodes.push(ProxyNode {
                        name: format!("{} (进程监听)", proc_name),
                        host,
                        port,
                        protocol: ProxyProtocol::Http,
                        username: None,
                        password: None,
                        latency_ms: None,
                        source: ProxySource::AutoDetected,
                    });
                }
            }
        }
        nodes
    }

    #[cfg(target_os = "windows")]
    {
        use std::collections::HashMap;
        use std::os::windows::process::CommandExt;
        use std::process::Stdio;
        use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        let mut nodes = Vec::new();
        let output = match std::process::Command::new("netstat")
            .args(["-ano", "-p", "tcp"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .output()
        {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
            _ => return nodes,
        };

        // 仅拉取进程名，零多余 OpenProcess 句柄
        let mut sys = System::new();
        sys.refresh_processes_specifics(ProcessesToUpdate::All, ProcessRefreshKind::new());
        let pid_names: HashMap<u32, String> = sys
            .processes()
            .iter()
            .map(|(pid, p)| (pid.as_u32(), p.name().to_string_lossy().to_string()))
            .collect();

        for line in output.lines() {
            let cols: Vec<&str> = line.split_whitespace().collect();
            // TCP  127.0.0.1:7890  0.0.0.0:0  LISTENING  12345
            if cols.len() < 5 || !cols[3].eq_ignore_ascii_case("LISTENING") {
                continue;
            }
            let Ok(pid) = cols[4].parse::<u32>() else {
                continue;
            };
            let Some(proc_name) = pid_names.get(&pid) else {
                continue;
            };
            let lower = proc_name.to_ascii_lowercase();
            if !KNOWN_PROXY_PROCESS_KEYWORDS
                .iter()
                .any(|k| lower.contains(k))
            {
                continue;
            }
            if let Some((raw_host, raw_port)) = cols[1].rsplit_once(':') {
                if let Ok(port) = raw_port.parse::<u16>() {
                    let host = if raw_host == "0.0.0.0" || raw_host == "[::]" || raw_host == "*" {
                        "127.0.0.1".to_string()
                    } else {
                        raw_host.trim_matches(['[', ']']).to_string()
                    };
                    nodes.push(ProxyNode {
                        name: format!("{} (进程监听)", proc_name.trim_end_matches(".exe")),
                        host,
                        port,
                        protocol: ProxyProtocol::Http,
                        username: None,
                        password: None,
                        latency_ms: None,
                        source: ProxySource::AutoDetected,
                    });
                }
            }
        }
        nodes
    }
}

/// 读取当前操作系统的系统代理设置
pub fn detect_system_proxies() -> Vec<ProxyNode> {
    #[cfg(target_os = "macos")]
    {
        detect_macos_system_proxies()
    }
    #[cfg(target_os = "windows")]
    {
        detect_windows_system_proxies()
    }
    #[cfg(target_os = "linux")]
    {
        detect_linux_system_proxies()
    }
}

#[cfg(target_os = "macos")]
fn detect_macos_system_proxies() -> Vec<ProxyNode> {
    let mut nodes = Vec::new();
    let output = match std::process::Command::new("scutil").arg("--proxy").output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => return nodes,
    };

    let get_val = |key: &str| -> Option<String> {
        for line in output.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix(key) {
                if let Some((_, val)) = rest.split_once(':') {
                    return Some(val.trim().to_string());
                }
            }
        }
        None
    };

    if get_val("HTTPSEnable").as_deref() == Some("1") {
        if let (Some(host), Some(port_s)) = (get_val("HTTPSProxy"), get_val("HTTPSPort")) {
            if let Ok(port) = port_s.parse::<u16>() {
                nodes.push(ProxyNode {
                    name: "macOS System HTTPS".into(),
                    host,
                    port,
                    protocol: ProxyProtocol::Http,
                    username: None,
                    password: None,
                    latency_ms: None,
                    source: ProxySource::System,
                });
            }
        }
    }

    if get_val("HTTPEnable").as_deref() == Some("1") {
        if let (Some(host), Some(port_s)) = (get_val("HTTPProxy"), get_val("HTTPPort")) {
            if let Ok(port) = port_s.parse::<u16>() {
                nodes.push(ProxyNode {
                    name: "macOS System HTTP".into(),
                    host,
                    port,
                    protocol: ProxyProtocol::Http,
                    username: None,
                    password: None,
                    latency_ms: None,
                    source: ProxySource::System,
                });
            }
        }
    }

    if get_val("SOCKSEnable").as_deref() == Some("1") {
        if let (Some(host), Some(port_s)) = (get_val("SOCKSProxy"), get_val("SOCKSPort")) {
            if let Ok(port) = port_s.parse::<u16>() {
                nodes.push(ProxyNode {
                    name: "macOS System SOCKS5".into(),
                    host,
                    port,
                    protocol: ProxyProtocol::Socks5,
                    username: None,
                    password: None,
                    latency_ms: None,
                    source: ProxySource::System,
                });
            }
        }
    }

    nodes
}

/// 解析 Windows `reg query` 标准 3 列输出: `<ValueName>    <REG_TYPE>    <ValueData>`
#[cfg(any(target_os = "windows", test))]
fn parse_reg_query_value<'a>(output: &'a str, key: &str) -> Option<&'a str> {
    output.lines().find_map(|line| {
        let trimmed = line.trim();
        let rest = trimmed.strip_prefix(key)?;
        // 必须紧跟空白，避免 `ProxyServer` 误匹配 `ProxyServerOverride` 等前缀键
        if !rest.starts_with(char::is_whitespace) {
            return None;
        }
        let after_key = rest.trim_start();
        // 跳过 REG_DWORD / REG_SZ 等类型列，保留包含空格在内的完整值
        let (_, val) = after_key.split_once(char::is_whitespace)?;
        let val = val.trim();
        (!val.is_empty()).then_some(val)
    })
}

#[cfg(target_os = "windows")]
fn detect_windows_system_proxies() -> Vec<ProxyNode> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let mut nodes = Vec::new();
    let output = match std::process::Command::new("reg")
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => return nodes,
    };

    // 必须确认 ProxyEnable == 0x1，避免读取到已关闭的残留 ProxyServer
    let enabled = parse_reg_query_value(&output, "ProxyEnable").and_then(|v| {
        let stripped = v.strip_prefix("0x").or_else(|| v.strip_prefix("0X"));
        if let Some(hex) = stripped {
            u32::from_str_radix(hex, 16).ok()
        } else {
            v.parse::<u32>().ok()
        }
    }) == Some(1);
    if !enabled {
        return nodes;
    }

    let Some(raw) = parse_reg_query_value(&output, "ProxyServer") else {
        return nodes;
    };

    // 支持单值 `127.0.0.1:7890` 以及分协议 `http=127.0.0.1:7890;https=127.0.0.1:7890;socks=127.0.0.1:7891`
    let mut push_entry = |name: &str, addr: &str, protocol: ProxyProtocol| {
        let stripped = addr
            .trim()
            .trim_start_matches("http://")
            .trim_start_matches("https://")
            .trim_start_matches("socks5://")
            .trim_start_matches("socks://");
        if let Some((host, port_s)) = stripped.rsplit_once(':') {
            if let Ok(port) = port_s.parse::<u16>() {
                nodes.push(ProxyNode {
                    name: name.into(),
                    host: host.to_string(),
                    port,
                    protocol,
                    username: None,
                    password: None,
                    latency_ms: None,
                    source: ProxySource::System,
                });
            }
        }
    };

    if raw.contains('=') {
        let mut https_addr = None;
        let mut http_addr = None;
        let mut socks_addr = None;
        for part in raw.split(';') {
            if let Some((k, v)) = part.split_once('=') {
                match k.trim().to_ascii_lowercase().as_str() {
                    "https" => https_addr = Some(v.trim()),
                    "http" => http_addr = Some(v.trim()),
                    "socks" | "socks5" => socks_addr = Some(v.trim()),
                    _ => {}
                }
            }
        }
        if let Some(a) = https_addr {
            push_entry("Windows System HTTPS", a, ProxyProtocol::Http);
        }
        if let Some(a) = http_addr {
            push_entry("Windows System HTTP", a, ProxyProtocol::Http);
        }
        if let Some(a) = socks_addr {
            push_entry("Windows System SOCKS5", a, ProxyProtocol::Socks5);
        }
    } else {
        push_entry("Windows System Proxy", raw, ProxyProtocol::Http);
    }

    nodes
}

#[cfg(target_os = "linux")]
fn detect_linux_system_proxies() -> Vec<ProxyNode> {
    let mut nodes = Vec::new();
    for key in [
        "HTTPS_PROXY",
        "HTTP_PROXY",
        "ALL_PROXY",
        "https_proxy",
        "http_proxy",
    ] {
        if let Ok(val) = std::env::var(key) {
            if let Ok(mut node) = ProxyNode::parse_url(&val, Some(format!("Env {}", key))) {
                node.source = ProxySource::System;
                nodes.push(node);
            }
        }
    }
    nodes
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShellKind {
    Zsh,
    Bash,
    Fish,
    Pwsh,
}

fn detect_shell_rc(home: &Path) -> (ShellKind, PathBuf) {
    #[cfg(target_os = "windows")]
    {
        let pwsh7 = home.join(r"Documents\PowerShell\Microsoft.PowerShell_profile.ps1");
        let win_ps = home.join(r"Documents\WindowsPowerShell\Microsoft.PowerShell_profile.ps1");
        let ps_module_path = std::env::var("PSModulePath").unwrap_or_default();
        let looks_like_ps5 = !ps_module_path.contains(r"PowerShell\7")
            && win_ps.parent().map(|p| p.exists()).unwrap_or(false)
            && !pwsh7.parent().map(|p| p.exists()).unwrap_or(false);
        return (ShellKind::Pwsh, if looks_like_ps5 { win_ps } else { pwsh7 });
    }

    #[cfg(not(target_os = "windows"))]
    {
        let shell_env = std::env::var("SHELL").unwrap_or_default();
        let shell_name = Path::new(&shell_env)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        if shell_name == "fish" {
            return (ShellKind::Fish, home.join(".config/fish/config.fish"));
        }

        if shell_name == "zsh" || (shell_name.is_empty() && cfg!(target_os = "macos")) {
            let zdot = std::env::var_os("ZDOTDIR")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.to_path_buf());
            return (ShellKind::Zsh, zdot.join(".zshrc"));
        }

        let bashrc = home.join(".bashrc");
        let bash_profile = home.join(".bash_profile");
        if cfg!(target_os = "macos") && !bashrc.exists() && bash_profile.exists() {
            return (ShellKind::Bash, bash_profile);
        }
        (ShellKind::Bash, bashrc)
    }
}

fn all_shell_rc_candidates(home: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    #[cfg(not(target_os = "windows"))]
    {
        if let Some(zdot) = std::env::var_os("ZDOTDIR").filter(|v| !v.is_empty()) {
            paths.push(PathBuf::from(zdot).join(".zshrc"));
        }
        paths.push(home.join(".zshrc"));
        paths.push(home.join(".bashrc"));
        paths.push(home.join(".bash_profile"));
        paths.push(home.join(".config/fish/config.fish"));
    }
    #[cfg(target_os = "windows")]
    {
        paths.push(home.join(r"Documents\PowerShell\Microsoft.PowerShell_profile.ps1"));
        paths.push(home.join(r"Documents\WindowsPowerShell\Microsoft.PowerShell_profile.ps1"));
    }
    paths
}

/// 检测用户在受管块之外是否已有自定义 `agy` 别名或函数，避免静默覆盖
fn has_custom_agy_definition(cleaned_content: &str, shell: ShellKind) -> bool {
    for raw_line in cleaned_content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let matched = match shell {
            ShellKind::Fish => {
                line.starts_with("function agy ")
                    || line == "function agy"
                    || line.starts_with("alias agy=")
                    || line.starts_with("alias agy ")
            }
            ShellKind::Pwsh => {
                let lower = line.to_ascii_lowercase();
                lower.starts_with("function agy ")
                    || lower.starts_with("function agy{")
                    || lower.starts_with("set-alias agy ")
                    || lower.starts_with("set-alias -name agy ")
                    || lower.starts_with("sal agy ")
            }
            ShellKind::Zsh | ShellKind::Bash => {
                line.starts_with("alias agy=")
                    || line.starts_with("function agy ")
                    || line.starts_with("function agy(")
                    || line.starts_with("agy()")
                    || line.starts_with("agy ()")
            }
        };
        if matched {
            return true;
        }
    }
    false
}

/// 为 `agy` CLI 安装无感 Shell Hook（幂等写入，防冲突，不破坏用户原有配置）
pub fn install_shell_hook(proxy_http_url: &str) -> Result<PathBuf, ProxyError> {
    let home = dirs::home_dir()
        .ok_or_else(|| ProxyError::UnsupportedPlatform("Cannot resolve home directory".into()))?;

    let (shell, rc_path) = detect_shell_rc(&home);
    let existing = std::fs::read_to_string(&rc_path).unwrap_or_default();
    let already_managed = existing.contains(HOOK_START);
    let cleaned = strip_hook_block(&existing);

    if !already_managed && has_custom_agy_definition(&cleaned, shell) {
        return Err(ProxyError::Protocol(format!(
            "Skipped shell hook in {} because a custom `agy` alias/function already exists",
            rc_path.display()
        )));
    }

    let no_proxy = crate::launcher::DEFAULT_NO_PROXY;
    let hook_body = match shell {
        ShellKind::Fish => format!(
            "{start}\nfunction agy --wraps agy\n  env HTTP_PROXY=\"{url}\" HTTPS_PROXY=\"{url}\" ALL_PROXY=\"{url}\" http_proxy=\"{url}\" https_proxy=\"{url}\" all_proxy=\"{url}\" NO_PROXY=\"{no_proxy}\" no_proxy=\"{no_proxy}\" NODE_USE_ENV_PROXY=1 command agy $argv\nend\n{end}\n",
            start = HOOK_START,
            end = HOOK_END,
            url = proxy_http_url,
            no_proxy = no_proxy
        ),
        ShellKind::Pwsh => format!(
            "{start}\r\nfunction agy {{\r\n  $env:HTTP_PROXY=\"{url}\"; $env:HTTPS_PROXY=\"{url}\"; $env:ALL_PROXY=\"{url}\"; $env:http_proxy=\"{url}\"; $env:https_proxy=\"{url}\"; $env:all_proxy=\"{url}\"; $env:NO_PROXY=\"{no_proxy}\"; $env:no_proxy=\"{no_proxy}\"; $env:NODE_USE_ENV_PROXY=\"1\"\r\n  $bin = (Get-Command agy -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1).Source\r\n  if ($bin) {{ & $bin @args }}\r\n}}\r\n{end}\r\n",
            start = HOOK_START,
            end = HOOK_END,
            url = proxy_http_url,
            no_proxy = no_proxy
        ),
        ShellKind::Zsh | ShellKind::Bash => format!(
            "{start}\nagy() {{\n  HTTP_PROXY=\"{url}\" HTTPS_PROXY=\"{url}\" ALL_PROXY=\"{url}\" http_proxy=\"{url}\" https_proxy=\"{url}\" all_proxy=\"{url}\" NO_PROXY=\"{no_proxy}\" no_proxy=\"{no_proxy}\" NODE_USE_ENV_PROXY=1 command agy \"$@\"\n}}\n{end}\n",
            start = HOOK_START,
            end = HOOK_END,
            url = proxy_http_url,
            no_proxy = no_proxy
        ),
    };

    if let Some(parent) = rc_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let final_content = if cleaned.trim().is_empty() {
        hook_body
    } else {
        format!("{}\n\n{}", cleaned.trim_end(), hook_body)
    };
    std::fs::write(&rc_path, final_content)?;
    Ok(rc_path)
}

/// 检查 `agy` CLI 的 Shell Hook 是否已安装
pub fn is_shell_hook_installed() -> bool {
    let Some(home) = dirs::home_dir() else {
        return false;
    };
    all_shell_rc_candidates(&home).into_iter().any(|p| {
        std::fs::read_to_string(p)
            .map(|c| c.contains(HOOK_START))
            .unwrap_or(false)
    })
}

/// 卸载 `agy` CLI 的 Shell Hook
pub fn uninstall_shell_hook() -> Result<(), ProxyError> {
    let home = dirs::home_dir()
        .ok_or_else(|| ProxyError::UnsupportedPlatform("Cannot resolve home directory".into()))?;
    for p in all_shell_rc_candidates(&home) {
        if p.exists() {
            let content = std::fs::read_to_string(&p)?;
            let cleaned = strip_hook_block(&content);
            if cleaned != content {
                std::fs::write(&p, cleaned)?;
            }
        }
    }
    Ok(())
}

fn strip_hook_block(content: &str) -> String {
    let mut out = Vec::new();
    let mut in_block = false;
    for line in content.lines() {
        if line.trim() == HOOK_START {
            in_block = true;
            continue;
        }
        if line.trim() == HOOK_END {
            in_block = false;
            continue;
        }
        if !in_block {
            out.push(line);
        }
    }
    out.join("\n")
}

/// 在 macOS 上生成零侵入、抗更新的外部 Wrapper `.app` 启动器（自动复用原版高清图标）
#[cfg(target_os = "macos")]
pub fn create_macos_wrapper_app(
    target: TargetApp,
    proxy_http_url: &str,
    dest_dir: &Path,
) -> Result<PathBuf, ProxyError> {
    use std::os::unix::fs::PermissionsExt;

    let (app_name, source_bundle, orig_res_dir, icon_candidates): (&str, &str, &str, &[&str]) =
        match target {
            TargetApp::AntigravityApp => (
                "Antigravity (No-TUN).app",
                "/Applications/Antigravity.app",
                "/Applications/Antigravity.app/Contents/Resources",
                &["Antigravity.icns", "icon.icns", "app.icns", "electron.icns"],
            ),
            TargetApp::AntigravityIde => (
                "Antigravity IDE (No-TUN).app",
                "/Applications/Antigravity IDE.app",
                "/Applications/Antigravity IDE.app/Contents/Resources",
                &[
                    "Antigravity IDE.icns",
                    "icon.icns",
                    "Code.icns",
                    "electron.icns",
                ],
            ),
            TargetApp::AgyCli => {
                return Err(ProxyError::UnsupportedPlatform(
                    "Wrapper .app is only for GUI apps".into(),
                ))
            }
        };

    let bundle_dir = dest_dir.join(app_name);
    let macos_dir = bundle_dir.join("Contents/MacOS");
    let res_dir = bundle_dir.join("Contents/Resources");
    std::fs::create_dir_all(&macos_dir)?;
    std::fs::create_dir_all(&res_dir)?;

    let launcher_script = macos_dir.join("launcher");
    let no_proxy = crate::launcher::DEFAULT_NO_PROXY;
    let script = format!(
        "#!/bin/sh\nexec /usr/bin/open -a \"{bundle}\" \\\n  --env HTTP_PROXY=\"{url}\" \\\n  --env HTTPS_PROXY=\"{url}\" \\\n  --env ALL_PROXY=\"{url}\" \\\n  --env http_proxy=\"{url}\" \\\n  --env https_proxy=\"{url}\" \\\n  --env all_proxy=\"{url}\" \\\n  --env NO_PROXY=\"{no_proxy}\" \\\n  --env no_proxy=\"{no_proxy}\" \\\n  --env NODE_USE_ENV_PROXY=1 \\\n  --args \"$@\"\n",
        bundle = source_bundle,
        url = proxy_http_url,
        no_proxy = no_proxy
    );
    std::fs::write(&launcher_script, script)?;
    std::fs::set_permissions(&launcher_script, std::fs::Permissions::from_mode(0o755))?;

    // 自动查找原版应用中的真实 .icns 图标
    let mut copied_icon = false;
    for candidate in icon_candidates {
        let src = Path::new(orig_res_dir).join(candidate);
        if src.exists() {
            if std::fs::copy(&src, res_dir.join("app.icns")).is_ok() {
                copied_icon = true;
                break;
            }
        }
    }
    if !copied_icon {
        if let Ok(entries) = std::fs::read_dir(orig_res_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().and_then(|e| e.to_str()) == Some("icns") {
                    if std::fs::copy(&p, res_dir.join("app.icns")).is_ok() {
                        break;
                    }
                }
            }
        }
    }

    let bundle_id = match target {
        TargetApp::AntigravityApp => "com.antigravity.manager.notun-app",
        TargetApp::AntigravityIde => "com.antigravity.manager.notun-ide",
        TargetApp::AgyCli => "com.antigravity.manager.notun-cli",
    };

    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key>
  <string>launcher</string>
  <key>CFBundleIdentifier</key>
  <string>{}</string>
  <key>CFBundleName</key>
  <string>{}</string>
  <key>CFBundleIconFile</key>
  <string>app.icns</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
</dict>
</plist>
"#,
        bundle_id,
        app_name.trim_end_matches(".app")
    );
    std::fs::write(bundle_dir.join("Contents/Info.plist"), plist)?;
    // 刷新目录时间戳，触发 macOS Finder 立即加载图标
    let _ = std::process::Command::new("touch")
        .arg(&bundle_dir)
        .status();
    Ok(bundle_dir)
}

/// 移除 macOS 桌面上的外部 Wrapper `.app` 启动器
#[cfg(target_os = "macos")]
pub fn remove_macos_wrapper_app(target: TargetApp, dest_dir: &Path) -> Result<bool, ProxyError> {
    let app_name = match target {
        TargetApp::AntigravityApp => "Antigravity (No-TUN).app",
        TargetApp::AntigravityIde => "Antigravity IDE (No-TUN).app",
        TargetApp::AgyCli => return Ok(false),
    };
    let bundle_dir = dest_dir.join(app_name);
    if bundle_dir.exists() {
        std::fs::remove_dir_all(&bundle_dir)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// 跨平台定位用户桌面目录（Windows 支持 OneDrive 重定向桌面路径）
fn resolve_desktop_dir() -> Result<PathBuf, ProxyError> {
    dirs::desktop_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Desktop")))
        .ok_or_else(|| ProxyError::UnsupportedPlatform("Cannot resolve Desktop directory".into()))
}

#[cfg(target_os = "windows")]
fn windows_shortcut_names(target: TargetApp) -> Result<(&'static str, &'static str), ProxyError> {
    match target {
        TargetApp::AntigravityApp => Ok(("Antigravity (No-TUN).lnk", "antigravity-notun.vbs")),
        TargetApp::AntigravityIde => {
            Ok(("Antigravity IDE (No-TUN).lnk", "antigravity-ide-notun.vbs"))
        }
        TargetApp::AgyCli => Err(ProxyError::UnsupportedPlatform(
            "Desktop shortcut is only for GUI apps".into(),
        )),
    }
}

#[cfg(target_os = "windows")]
fn windows_vbs_launcher_path(vbs_name: &str) -> Result<PathBuf, ProxyError> {
    let base = dirs::data_local_dir()
        .or_else(dirs::home_dir)
        .ok_or_else(|| ProxyError::UnsupportedPlatform("Cannot resolve LocalAppData".into()))?;
    Ok(base
        .join("Antigravity Manager")
        .join("launchers")
        .join(vbs_name))
}

#[cfg(target_os = "linux")]
fn linux_desktop_entry_name(target: TargetApp) -> Result<&'static str, ProxyError> {
    match target {
        TargetApp::AntigravityApp => Ok("Antigravity (No-TUN).desktop"),
        TargetApp::AntigravityIde => Ok("Antigravity IDE (No-TUN).desktop"),
        TargetApp::AgyCli => Err(ProxyError::UnsupportedPlatform(
            "Desktop shortcut is only for GUI apps".into(),
        )),
    }
}

/// 跨平台在桌面创建免 TUN 外部代理启动图标（macOS `.app` / Windows 无黑窗 `.lnk` / Linux `.desktop`）
pub fn create_desktop_shortcut(
    target: TargetApp,
    proxy_http_url: &str,
) -> Result<PathBuf, ProxyError> {
    let desktop = resolve_desktop_dir()?;
    std::fs::create_dir_all(&desktop)?;

    #[cfg(target_os = "macos")]
    {
        create_macos_wrapper_app(target, proxy_http_url, &desktop)
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        use std::process::Stdio;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        let (lnk_name, vbs_name) = windows_shortcut_names(target)?;
        let bin = crate::launcher::resolve_target_binary(target)?;
        let vbs_path = windows_vbs_launcher_path(vbs_name)?;
        if let Some(parent) = vbs_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let esc_vbs = |s: &str| s.replace('"', "\"\"");
        let bin_str = bin.to_string_lossy();
        let no_proxy = crate::launcher::DEFAULT_NO_PROXY;
        // 通过 GUI 子系统 wscript.exe 运行，注入 9 个代理环境变量并静默启动原版 EXE（零黑窗）
        let vbs_content = format!(
            "Set sh = CreateObject(\"WScript.Shell\")\r\n\
Set env = sh.Environment(\"Process\")\r\n\
env(\"HTTP_PROXY\") = \"{url}\"\r\n\
env(\"HTTPS_PROXY\") = \"{url}\"\r\n\
env(\"ALL_PROXY\") = \"{url}\"\r\n\
env(\"http_proxy\") = \"{url}\"\r\n\
env(\"https_proxy\") = \"{url}\"\r\n\
env(\"all_proxy\") = \"{url}\"\r\n\
env(\"NO_PROXY\") = \"{no_proxy}\"\r\n\
env(\"no_proxy\") = \"{no_proxy}\"\r\n\
env(\"NODE_USE_ENV_PROXY\") = \"1\"\r\n\
sh.Run \"\"\"{bin}\"\"\", 1, False\r\n",
            url = esc_vbs(proxy_http_url),
            no_proxy = esc_vbs(no_proxy),
            bin = esc_vbs(&bin_str),
        );
        std::fs::write(&vbs_path, vbs_content)?;

        let lnk_path = desktop.join(lnk_name);
        let esc_ps = |s: &str| s.replace('\'', "''");
        let vbs_ps = esc_ps(&vbs_path.to_string_lossy());
        let lnk_ps = esc_ps(&lnk_path.to_string_lossy());
        let icon_ps = esc_ps(&format!("{},0", bin_str));
        let workdir_ps = esc_ps(
            &bin.parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default(),
        );
        let ps_script = format!(
            "$ws = New-Object -ComObject WScript.Shell; \
             $s = $ws.CreateShortcut('{lnk}'); \
             $s.TargetPath = \"$env:SystemRoot\\System32\\wscript.exe\"; \
             $s.Arguments = '//B //Nologo \"{vbs}\"'; \
             $s.IconLocation = '{icon}'; \
             if ('{wd}' -ne '') {{ $s.WorkingDirectory = '{wd}' }}; \
             $s.Save()",
            lnk = lnk_ps,
            vbs = vbs_ps,
            icon = icon_ps,
            wd = workdir_ps,
        );

        let status = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-WindowStyle",
                "Hidden",
                "-Command",
                &ps_script,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .status()?;

        if !status.success() || !lnk_path.exists() {
            return Err(ProxyError::Protocol(
                "Failed to create Windows desktop shortcut".into(),
            ));
        }
        Ok(lnk_path)
    }

    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::PermissionsExt;

        let entry_name = linux_desktop_entry_name(target)?;
        let bin = crate::launcher::resolve_target_binary(target)?;
        let no_proxy = crate::launcher::DEFAULT_NO_PROXY;
        let display_name = entry_name.trim_end_matches(".desktop");
        let entry_path = desktop.join(entry_name);
        let content = format!(
            "[Desktop Entry]\n\
Type=Application\n\
Name={name}\n\
Exec=env HTTP_PROXY=\"{url}\" HTTPS_PROXY=\"{url}\" ALL_PROXY=\"{url}\" http_proxy=\"{url}\" https_proxy=\"{url}\" all_proxy=\"{url}\" NO_PROXY=\"{no_proxy}\" no_proxy=\"{no_proxy}\" NODE_USE_ENV_PROXY=1 \"{bin}\" %U\n\
Terminal=false\n\
Categories=Development;\n",
            name = display_name,
            url = proxy_http_url,
            no_proxy = no_proxy,
            bin = bin.to_string_lossy(),
        );
        std::fs::write(&entry_path, content)?;
        std::fs::set_permissions(&entry_path, std::fs::Permissions::from_mode(0o755))?;
        Ok(entry_path)
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = (target, proxy_http_url);
        Err(ProxyError::UnsupportedPlatform(
            "Unsupported platform for desktop shortcut".into(),
        ))
    }
}

/// 跨平台移除桌面免 TUN 代理启动图标
pub fn remove_desktop_shortcut(target: TargetApp) -> Result<bool, ProxyError> {
    let desktop = resolve_desktop_dir()?;

    #[cfg(target_os = "macos")]
    {
        remove_macos_wrapper_app(target, &desktop)
    }

    #[cfg(target_os = "windows")]
    {
        let (lnk_name, vbs_name) = match windows_shortcut_names(target) {
            Ok(v) => v,
            Err(_) => return Ok(false),
        };
        let mut removed = false;
        let lnk_path = desktop.join(lnk_name);
        if lnk_path.exists() {
            std::fs::remove_file(&lnk_path)?;
            removed = true;
        }
        if let Ok(vbs_path) = windows_vbs_launcher_path(vbs_name) {
            if vbs_path.exists() {
                let _ = std::fs::remove_file(&vbs_path);
            }
        }
        Ok(removed)
    }

    #[cfg(target_os = "linux")]
    {
        let entry_name = match linux_desktop_entry_name(target) {
            Ok(v) => v,
            Err(_) => return Ok(false),
        };
        let entry_path = desktop.join(entry_name);
        if entry_path.exists() {
            std::fs::remove_file(&entry_path)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = target;
        Ok(false)
    }
}

/// 跨平台检测桌面免 TUN 代理启动图标是否已创建
pub fn is_desktop_shortcut_installed(target: TargetApp) -> bool {
    let Ok(desktop) = resolve_desktop_dir() else {
        return false;
    };

    #[cfg(target_os = "macos")]
    {
        let app_name = match target {
            TargetApp::AntigravityApp => "Antigravity (No-TUN).app",
            TargetApp::AntigravityIde => "Antigravity IDE (No-TUN).app",
            TargetApp::AgyCli => return false,
        };
        desktop.join(app_name).exists()
    }

    #[cfg(target_os = "windows")]
    {
        let Ok((lnk_name, _)) = windows_shortcut_names(target) else {
            return false;
        };
        desktop.join(lnk_name).exists()
    }

    #[cfg(target_os = "linux")]
    {
        let Ok(entry_name) = linux_desktop_entry_name(target) else {
            return false;
        };
        desktop.join(entry_name).exists()
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = target;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::parse_reg_query_value;

    #[test]
    fn test_parse_reg_query_value_exact_and_spaces() {
        let sample = r#"
HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Internet Settings
    ProxyEnable    REG_DWORD    0x1
    ProxyServerOverride    REG_SZ    <local>
    ProxyServer    REG_SZ    http=127.0.0.1:7890; https=127.0.0.1:7891
"#;
        assert_eq!(parse_reg_query_value(sample, "ProxyEnable"), Some("0x1"));
        assert_eq!(
            parse_reg_query_value(sample, "ProxyServer"),
            Some("http=127.0.0.1:7890; https=127.0.0.1:7891")
        );
        assert_eq!(parse_reg_query_value(sample, "NonExistent"), None);
    }
}
