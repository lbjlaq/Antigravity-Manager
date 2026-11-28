use crate::types::{ProxyError, ProxyNode, ProxyProtocol, ProxySource};
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
        Vec::new()
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

#[cfg(target_os = "windows")]
fn detect_windows_system_proxies() -> Vec<ProxyNode> {
    let mut nodes = Vec::new();
    let output = match crate::hide_console(std::process::Command::new("reg"))
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings",
        ])
        .output()
    {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => return nodes,
    };

    // 必须确认 ProxyEnable == 0x1，避免读取到已关闭的残留 ProxyServer
    let enabled = output.lines().any(|line| {
        line.contains("ProxyEnable")
            && line
                .split_whitespace()
                .last()
                .and_then(|v| u32::from_str_radix(v.trim_start_matches("0x"), 16).ok())
                == Some(1)
    });
    if !enabled {
        return nodes;
    }

    let raw_server = output.lines().find_map(|line| {
        if line.contains("ProxyServer") {
            line.split_whitespace().last().map(str::trim)
        } else {
            None
        }
    });
    let Some(raw) = raw_server else {
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
