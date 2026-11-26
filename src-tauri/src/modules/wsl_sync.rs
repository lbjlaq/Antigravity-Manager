use std::path::PathBuf;

#[cfg(target_os = "windows")]
use std::collections::HashMap;
#[cfg(target_os = "windows")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "windows")]
use std::sync::{LazyLock, Mutex};
#[cfg(target_os = "windows")]
use std::time::{Duration, Instant};

/// 提取 UNC 路径中的 WSL 分发版名称
/// 支持 \\wsl.localhost\<distro> 和 \\wsl$\<distro>
pub fn extract_wsl_distro(path: &str) -> Option<String> {
    let normalized = path.replace('/', "\\");
    let unc = normalized
        .strip_prefix(r"\\?\UNC\")
        .map(|rest| format!(r"\\{}", rest))
        .unwrap_or(normalized);

    for host in [r"\\wsl.localhost\", r"\\wsl$\"] {
        if let Some(rest) = unc.strip_prefix(host) {
            let distro = rest.split('\\').next().unwrap_or("");
            if !distro.is_empty() {
                return Some(distro.to_string());
            }
        }
    }
    None
}

/// 解析 wsl.exe --list --running --quiet 的原始输出，判断特定分发版是否运行中
pub fn parse_running_wsl_output(stdout: &[u8], target_distro: &str) -> bool {
    if stdout.is_empty() || target_distro.is_empty() {
        return false;
    }

    // 1. 尝试按 UTF-16LE 解析 (Windows 控制台默认 Unicode 输出格式)
    if stdout.len() >= 2 && stdout.len() % 2 == 0 {
        let u16s: Vec<u16> = stdout
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        if let Ok(text) = String::from_utf16(&u16s) {
            let clean = text.trim_start_matches('\u{feff}');
            if clean.lines().any(|line| {
                line.trim_matches(|c: char| c.is_whitespace() || c == '\0')
                    .eq_ignore_ascii_case(target_distro)
            }) {
                return true;
            }
        }
    }

    // 2. 尝试按 UTF-8 解析
    if let Ok(text) = std::str::from_utf8(stdout) {
        let clean = text.trim_start_matches('\u{feff}');
        if clean.lines().any(|line| {
            line.trim_matches(|c: char| c.is_whitespace() || c == '\0')
                .eq_ignore_ascii_case(target_distro)
        }) {
            return true;
        }
    }

    false
}

/// 解析 wsl.exe 输出中所有处于运行中的分发版名称
pub fn extract_distro_names_from_output(stdout: &[u8]) -> Vec<String> {
    let mut names = Vec::new();

    // 1. 尝试按 UTF-16LE 解析
    if stdout.len() >= 2 && stdout.len() % 2 == 0 {
        let u16s: Vec<u16> = stdout
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        if let Ok(text) = String::from_utf16(&u16s) {
            let clean = text.trim_start_matches('\u{feff}');
            for line in clean.lines() {
                let name = line.trim_matches(|c: char| c.is_whitespace() || c == '\0');
                if !name.is_empty() {
                    names.push(name.to_string());
                }
            }
            if !names.is_empty() {
                return names;
            }
        }
    }

    // 2. 尝试按 UTF-8 解析
    if let Ok(text) = std::str::from_utf8(stdout) {
        let clean = text.trim_start_matches('\u{feff}');
        for line in clean.lines() {
            let name = line.trim_matches(|c: char| c.is_whitespace() || c == '\0');
            if !name.is_empty() {
                names.push(name.to_string());
            }
        }
    }

    names
}

#[cfg(target_os = "windows")]
fn run_command_with_timeout(
    cmd: &mut std::process::Command,
    timeout: Duration,
) -> Option<std::process::Output> {
    use std::sync::mpsc;
    use std::thread;

    let mut child = cmd
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;

    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let res = child.wait_with_output();
        let _ = tx.send(res);
    });

    match rx.recv_timeout(timeout) {
        Ok(Ok(output)) => Some(output),
        _ => None,
    }
}

/// 检测特定 WSL 分发版当前是否处于 running 状态。
/// 包含负向缓存与 2 秒硬超时保护，严禁阻塞主线程或访问 stopped 的 UNC 路径。
#[cfg(target_os = "windows")]
pub fn is_wsl_distro_running(distro: &str) -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    if distro.is_empty() {
        return false;
    }

    static NEGATIVE_CACHE: LazyLock<Mutex<HashMap<String, Instant>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));

    if let Ok(mut cache) = NEGATIVE_CACHE.try_lock() {
        cache.retain(|_, at| at.elapsed() < Duration::from_secs(5));
        if cache.contains_key(distro) {
            return false;
        }
    }

    static PROBE_RUNNING: AtomicBool = AtomicBool::new(false);
    if PROBE_RUNNING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return false;
    }

    struct ProbeGuard;
    impl Drop for ProbeGuard {
        fn drop(&mut self) {
            PROBE_RUNNING.store(false, Ordering::Release);
        }
    }
    let _guard = ProbeGuard;

    let mut cmd = std::process::Command::new("wsl.exe");
    cmd.args(["--list", "--running", "--quiet"]);
    cmd.creation_flags(CREATE_NO_WINDOW);

    let output = run_command_with_timeout(&mut cmd, Duration::from_secs(2));

    let running = match output {
        Some(out) if out.status.success() => parse_running_wsl_output(&out.stdout, distro),
        _ => false,
    };

    if !running {
        if let Ok(mut cache) = NEGATIVE_CACHE.lock() {
            cache.insert(distro.to_string(), Instant::now());
        }
    }

    running
}

#[cfg(not(target_os = "windows"))]
pub fn is_wsl_distro_running(_distro: &str) -> bool {
    false
}

/// 获取当前所有处于运行中的 WSL 分发版名称
#[cfg(target_os = "windows")]
pub fn get_running_wsl_distros() -> Vec<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let mut cmd = std::process::Command::new("wsl.exe");
    cmd.args(["--list", "--running", "--quiet"]);
    cmd.creation_flags(CREATE_NO_WINDOW);

    let Some(out) = run_command_with_timeout(&mut cmd, Duration::from_secs(2)) else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }

    extract_distro_names_from_output(&out.stdout)
}

#[cfg(not(target_os = "windows"))]
pub fn get_running_wsl_distros() -> Vec<String> {
    Vec::new()
}

fn collect_distro_user_homes(prefixes: &[PathBuf]) -> Vec<PathBuf> {
    for home_base in prefixes {
        let mut homes = Vec::new();
        if let Ok(entries) = std::fs::read_dir(home_base) {
            for entry in entries.flatten() {
                let user_dir = entry.path();
                if user_dir.is_dir() {
                    let file_name = entry.file_name().to_string_lossy().to_string();
                    if !file_name.starts_with('.') && file_name != "lost+found" {
                        homes.push(user_dir);
                    }
                }
            }
        }
        // 同一个分发版只需通过一种 UNC 前缀写入一次，避免对同一 state.vscdb 重复注入并覆盖备份
        if !homes.is_empty() {
            return homes;
        }
    }
    Vec::new()
}

/// 解析有效的 WSL 用户主目录路径（供凭据写入）
pub fn resolve_wsl_home_dirs(custom_home: Option<&str>) -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    // 1. 优先使用用户自定义的 WSL 主目录路径
    if let Some(custom) = custom_home {
        let trimmed = custom.trim();
        if !trimmed.is_empty() {
            let p = PathBuf::from(trimmed);
            if let Some(distro) = extract_wsl_distro(trimmed) {
                if is_wsl_distro_running(&distro) && p.exists() {
                    candidates.push(p);
                    return candidates;
                }
            } else if p.exists() {
                candidates.push(p);
                return candidates;
            }
        }
    }

    // 2. 自动枚举当前处于运行中的 WSL 分发版
    for distro in get_running_wsl_distros() {
        let prefixes = [
            PathBuf::from(format!(r"\\wsl.localhost\{}\home", distro)),
            PathBuf::from(format!(r"\\wsl$\{}\home", distro)),
        ];
        candidates.extend(collect_distro_user_homes(&prefixes));
    }

    candidates.sort();
    candidates.dedup();
    candidates
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn collect_unique_wsl_distros(homes: &[PathBuf]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut distros = Vec::new();
    for home in homes {
        if let Some(distro) = extract_wsl_distro(&home.to_string_lossy()) {
            if seen.insert(distro.to_ascii_lowercase()) {
                distros.push(distro);
            }
        }
    }
    distros
}

/// 构造 WSL 内重启 Antigravity 语言服务子进程的 `wsl.exe` 参数。
///
/// 不依赖 `killall`（最小化 WSL 发行版常未预装 `psmisc`），改用 `procps` 标准 `pkill -TERM -f`，
/// 仅精确匹配 Antigravity 安装/服务端（及 VS Code Remote 中的 Antigravity/Gemini 扩展）路径下的
/// `language_server*` 与 `agy`，避免误杀宿主内其他 LSP。
pub fn build_wsl_language_server_reload_args(distro: &str) -> Vec<String> {
    vec![
        "-d".to_string(),
        distro.to_string(),
        "--".to_string(),
        "sh".to_string(),
        "-c".to_string(),
        "pkill -TERM -f '([Aa]ntigravity|\\.[a]ntigravity-server|\\.[v]scode-server.*([Aa]ntigravity|[Gg]emini)).*(language_server|[/ ]agy($|[[:space:]]))' 2>/dev/null || true".to_string(),
    ]
}

/// 将账号凭据同步至 WSL 环境的核心执行体
fn sync_account_to_wsl_inner(account: &crate::models::Account, custom_home: Option<&str>) {
    let target_homes = resolve_wsl_home_dirs(custom_home);
    if target_homes.is_empty() {
        crate::modules::logger::log_info(
            "[WSL Sync] WSL account sync is enabled, but no running WSL user home was found.",
        );
        return;
    }

    for wsl_home in target_homes {
        crate::modules::logger::log_info(&format!(
            "[WSL Sync] Syncing account credentials to WSL home: {}",
            wsl_home.display()
        ));

        // 1. 同步写入 WSL 下的 .gemini 凭据
        let wsl_gemini_dir = wsl_home.join(".gemini");
        if let Err(e) = crate::modules::integration::write_to_file_credentials_to_dir(
            &wsl_gemini_dir,
            account,
            false,
        ) {
            crate::modules::logger::log_warn(&format!(
                "[WSL Sync] Failed to write file credentials to {}: {}",
                wsl_gemini_dir.display(),
                e
            ));
        } else {
            crate::modules::logger::log_info(&format!(
                "[WSL Sync] Successfully synced .gemini credentials to {}",
                wsl_gemini_dir.display()
            ));
        }

        // 2. 检查并注入 WSL 远程 VS Code / Antigravity Server 的 state.vscdb
        let candidate_dbs = [
            wsl_home.join(".vscode-server/data/User/globalStorage/state.vscdb"),
            wsl_home.join(".antigravity-server/data/User/globalStorage/state.vscdb"),
            wsl_home.join(".config/Antigravity/User/globalStorage/state.vscdb"),
            wsl_home.join(".config/Antigravity IDE/User/globalStorage/state.vscdb"),
        ];

        for db_path in &candidate_dbs {
            if db_path.exists() {
                let backup = db_path.with_extension("vscdb.backup");
                let _ = std::fs::copy(db_path, backup);

                if let Err(e) = crate::modules::db::inject_token(
                    db_path,
                    &account.token.access_token,
                    &account.token.refresh_token,
                    account.token.expiry_timestamp,
                    &account.email,
                    account.token.is_gcp_tos,
                    account.token.project_id.as_deref(),
                    account.token.id_token.as_deref(),
                    account.token.oauth_client_key.as_deref(),
                    Some("ide"),
                ) {
                    crate::modules::logger::log_warn(&format!(
                        "[WSL Sync] Failed to inject token into WSL database {}: {}",
                        db_path.display(),
                        e
                    ));
                } else {
                    crate::modules::logger::log_info(&format!(
                        "[WSL Sync] Successfully injected token into WSL database: {}",
                        db_path.display()
                    ));
                }
            }
        }
    }

    // 3. 全部主目录凭据落盘后，按分发版去重向 WSL 运行态中的 language_server 发送重载信号
    #[cfg(target_os = "windows")]
    for distro in collect_unique_wsl_distros(&target_homes) {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let args = build_wsl_language_server_reload_args(&distro);
        let mut cmd = std::process::Command::new("wsl.exe");
        cmd.args(&args);
        cmd.creation_flags(CREATE_NO_WINDOW);
        let _ = run_command_with_timeout(&mut cmd, Duration::from_millis(1500));
    }
}

/// 将账号凭据有界阻塞同步至 WSL 环境（用于桌面端重启重连 WSL 前确保新凭据已落盘）
pub fn sync_account_to_wsl_blocking(
    account: &crate::models::Account,
    timeout: std::time::Duration,
) {
    let Ok(config) = crate::modules::config::load_app_config() else {
        return;
    };

    if !config.wsl_sync_enabled {
        return;
    }

    let account_clone = account.clone();
    let custom_home = config.wsl_custom_home.clone();
    let (tx, rx) = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        sync_account_to_wsl_inner(&account_clone, custom_home.as_deref());
        let _ = tx.send(());
    });

    if rx.recv_timeout(timeout).is_err() {
        crate::modules::logger::log_warn(&format!(
            "[WSL Sync] Timed out waiting {:?} for WSL credential sync before IDE relaunch; continuing host switch.",
            timeout
        ));
    }
}

/// 将账号凭据异步同步至 WSL 环境（尽力而为，非阻塞）
pub fn sync_account_to_wsl(account: &crate::models::Account) {
    let Ok(config) = crate::modules::config::load_app_config() else {
        return;
    };

    if !config.wsl_sync_enabled {
        return;
    }

    let account_clone = account.clone();
    let custom_home = config.wsl_custom_home.clone();

    // 在独立后台线程中执行跨环境文件写入，彻底与宿主机主切号流程隔离
    std::thread::spawn(move || {
        sync_account_to_wsl_inner(&account_clone, custom_home.as_deref());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_wsl_distro_cases() {
        assert_eq!(
            extract_wsl_distro(r"\\wsl.localhost\Ubuntu\home\user\.gemini"),
            Some("Ubuntu".to_string())
        );
        assert_eq!(
            extract_wsl_distro(r"\\wsl$\Debian\home\alice"),
            Some("Debian".to_string())
        );
        assert_eq!(
            extract_wsl_distro(r"\\?\UNC\wsl.localhost\Ubuntu-24.04\home\bob"),
            Some("Ubuntu-24.04".to_string())
        );
        assert_eq!(
            extract_wsl_distro("//wsl.localhost/Arch/home/user"),
            Some("Arch".to_string())
        );
        assert_eq!(extract_wsl_distro(r"C:\Users\User\.gemini"), None);
    }

    #[test]
    fn test_parse_running_wsl_output_utf16() {
        // 构造 UTF-16LE 字节流模拟 Windows 下 wsl.exe 输出
        let sample = "\u{feff}Ubuntu\r\nDebian\r\n";
        let utf16_bytes: Vec<u8> = sample
            .encode_utf16()
            .flat_map(|u| u.to_le_bytes())
            .collect();

        assert!(parse_running_wsl_output(&utf16_bytes, "Ubuntu"));
        assert!(parse_running_wsl_output(&utf16_bytes, "ubuntu"));
        assert!(parse_running_wsl_output(&utf16_bytes, "Debian"));
        assert!(!parse_running_wsl_output(&utf16_bytes, "Fedora"));
    }

    #[test]
    fn test_parse_running_wsl_output_utf8() {
        let sample = "Ubuntu\nDebian\n";
        assert!(parse_running_wsl_output(sample.as_bytes(), "Ubuntu"));
        assert!(parse_running_wsl_output(sample.as_bytes(), "debian"));
        assert!(!parse_running_wsl_output(sample.as_bytes(), "Arch"));
    }

    #[test]
    fn test_extract_distro_names() {
        let sample = "\u{feff}Ubuntu-22.04\r\nopenSUSE\r\n";
        let utf16_bytes: Vec<u8> = sample
            .encode_utf16()
            .flat_map(|u| u.to_le_bytes())
            .collect();
        let names = extract_distro_names_from_output(&utf16_bytes);
        assert_eq!(names, vec!["Ubuntu-22.04", "openSUSE"]);
    }

    #[test]
    fn test_build_wsl_language_server_reload_args() {
        let args = build_wsl_language_server_reload_args("Ubuntu-24.04");
        assert_eq!(&args[..4], &["-d", "Ubuntu-24.04", "--", "sh"]);
        assert_eq!(args[4], "-c");
        let shell_cmd = &args[5];
        assert!(shell_cmd.contains("pkill -TERM -f"));
        assert!(shell_cmd.contains("[Aa]ntigravity"));
        assert!(shell_cmd.contains("language_server"));
        assert!(shell_cmd.contains("agy"));
        assert!(!shell_cmd.contains("killall"));
        // Avoid matching the parent `sh -c ...` cmdline itself under procps `pkill -f`.
        assert!(!shell_cmd.contains("(antigravity"));

        let start = shell_cmd.find('\'').expect("opening quote") + 1;
        let end = shell_cmd.rfind('\'').expect("closing quote");
        let pattern = &shell_cmd[start..end];
        let re = regex::Regex::new(pattern).expect("pkill ERE pattern should compile");

        assert!(re.is_match("/home/u/.antigravity-server/bin/1.0/language_server_linux_x64"));
        assert!(re.is_match("/home/u/.config/Antigravity/bin/language_server"));
        assert!(re.is_match("/home/u/.antigravity-server/bin/agy --serve"));
        assert!(re.is_match("/home/u/.antigravity-server/bin/agy"));
        assert!(re.is_match(
            "/home/u/.vscode-server/extensions/google.geminicodeassist-2.0/language_server"
        ));

        assert!(!re.is_match(shell_cmd));
        assert!(!re.is_match("/home/u/.vscode-server/extensions/elixir-ls/language_server.sh"));
        assert!(!re.is_match("/home/u/antigravity-docs/pedagogy"));
    }

    #[test]
    fn test_collect_distro_user_homes_stops_after_first_matching_prefix() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let first_prefix = tmp.path().join("wsl_localhost_home");
        let second_prefix = tmp.path().join("wsl_dollar_home");

        std::fs::create_dir_all(first_prefix.join("alice")).unwrap();
        std::fs::create_dir_all(first_prefix.join(".cache")).unwrap();
        std::fs::create_dir_all(first_prefix.join("lost+found")).unwrap();
        std::fs::create_dir_all(second_prefix.join("alice")).unwrap();

        let homes = collect_distro_user_homes(&[first_prefix.clone(), second_prefix.clone()]);
        assert_eq!(homes, vec![first_prefix.join("alice")]);

        // Falls back to the second prefix when the first prefix has no valid user dirs.
        let empty_prefix = tmp.path().join("empty_home");
        std::fs::create_dir_all(empty_prefix.join(".hidden")).unwrap();
        let fallback = collect_distro_user_homes(&[empty_prefix, second_prefix.clone()]);
        assert_eq!(fallback, vec![second_prefix.join("alice")]);
    }

    #[test]
    fn test_collect_unique_wsl_distros_deduplicates_case_insensitively() {
        let homes = vec![
            PathBuf::from(r"\\wsl.localhost\Ubuntu\home\alice"),
            PathBuf::from(r"\\wsl.localhost\ubuntu\home\bob"),
            PathBuf::from(r"\\wsl$\Debian\home\carol"),
            PathBuf::from(r"C:\Users\local"),
        ];
        assert_eq!(
            collect_unique_wsl_distros(&homes),
            vec!["Ubuntu".to_string(), "Debian".to_string()]
        );
    }
}
