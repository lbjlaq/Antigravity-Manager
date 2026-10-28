use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Windows 常见 CLI 安装路径扫描
#[cfg(target_os = "windows")]
fn scan_windows_cli_paths(cmd: &str) -> Option<PathBuf> {
    let mut common_paths: Vec<PathBuf> = Vec::new();

    // 常见 Windows 安装路径，按优先级排序（仅加入可推导出的绝对路径，避免空/相对路径误判）
    if let Some(app_data) = std::env::var_os("APPDATA") {
        let npm_base = PathBuf::from(app_data).join("npm");
        common_paths.push(npm_base.join(format!("{}.cmd", cmd)));
        common_paths.push(npm_base.join(cmd));
    }

    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        let pnpm_base = PathBuf::from(&local_app_data).join("pnpm");
        common_paths.push(pnpm_base.join(format!("{}.cmd", cmd)));
        common_paths.push(pnpm_base.join(cmd));

        let yarn_base = PathBuf::from(local_app_data).join("Yarn").join("bin");
        common_paths.push(yarn_base.join(format!("{}.cmd", cmd)));
        common_paths.push(yarn_base.join(cmd));
    }

    if let Some(home) = dirs::home_dir() {
        let bun_base = home.join(".bun").join("bin");
        common_paths.push(bun_base.join(format!("{}.exe", cmd)));
        common_paths.push(bun_base.join(cmd));

        let local_bin = home.join(".local").join("bin");
        common_paths.push(local_bin.join(format!("{}.exe", cmd)));
        common_paths.push(local_bin.join(format!("{}.cmd", cmd)));
        common_paths.push(local_bin.join(cmd));

        let cargo_bin = home.join(".cargo").join("bin");
        common_paths.push(cargo_bin.join(format!("{}.exe", cmd)));
        common_paths.push(cargo_bin.join(format!("{}.cmd", cmd)));
        common_paths.push(cargo_bin.join(cmd));

        let grok_bin = home.join(".grok").join("bin");
        common_paths.push(grok_bin.join(format!("{}.exe", cmd)));
        common_paths.push(grok_bin.join(format!("{}.cmd", cmd)));
        common_paths.push(grok_bin.join(cmd));

        let grokbuild_bin = home.join(".grokbuild").join("bin");
        common_paths.push(grokbuild_bin.join(format!("{}.exe", cmd)));
        common_paths.push(grokbuild_bin.join(format!("{}.cmd", cmd)));
        common_paths.push(grokbuild_bin.join(cmd));
    }

    for path in common_paths {
        if is_safe_path(&path) {
            tracing::debug!(
                "[CLI-Sync] Detected {} via Windows explicit path: {:?}",
                cmd,
                path
            );
            return Some(path);
        }
    }

    // 扫描 NVM Windows 目录
    if let Ok(nvm_home) = std::env::var("NVM_HOME") {
        let nvm_path = PathBuf::from(nvm_home);
        if nvm_path.is_dir() {
            // NVM Windows 结构: %NVM_HOME%\v{version}\{cmd}.cmd
            if let Ok(entries) = fs::read_dir(&nvm_path) {
                for entry in entries.flatten() {
                    let cmd_path = entry.path().join(format!("{}.cmd", cmd));
                    if is_safe_path(&cmd_path) {
                        tracing::debug!("[CLI-Sync] Detected {} via NVM_HOME: {:?}", cmd, cmd_path);
                        return Some(cmd_path);
                    }
                    // 也检查 .exe 版本
                    let exe_path = entry.path().join(format!("{}.exe", cmd));
                    if is_safe_path(&exe_path) {
                        tracing::debug!("[CLI-Sync] Detected {} via NVM_HOME: {:?}", cmd, exe_path);
                        return Some(exe_path);
                    }
                }
            }
        }
    }

    None
}

/// 解析 where 命令输出获取第一个有效路径
#[cfg(target_os = "windows")]
fn parse_where_output(output: &[u8]) -> Option<PathBuf> {
    let stdout = String::from_utf8_lossy(output);
    for line in stdout.lines() {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            let path = PathBuf::from(trimmed);
            if is_safe_path(&path) {
                return Some(path);
            }
        }
    }
    None
}

/// 检测备用 CLI 别名命令是否存在并返回其路径
fn detect_fallback_binary(name: &str) -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let mut c = Command::new("where");
        c.arg(name);
        c.creation_flags(CREATE_NO_WINDOW);
        if let Ok(out) = c.output() {
            if out.status.success() {
                return parse_where_output(&out.stdout);
            }
        }
        None
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Ok(out) = Command::new("which").arg(name).output() {
            if out.status.success() {
                return Some(PathBuf::from(name));
            }
        }
        None
    }
}

/// 检查路径是否是 .cmd/.bat 文件
#[cfg(target_os = "windows")]
fn is_cmd_file(path: &PathBuf) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"))
        .unwrap_or(false)
}

/// 验证路径是否安全（防止命令注入）
#[cfg(target_os = "windows")]
fn is_safe_path(path: &PathBuf) -> bool {
    // 检查路径是否存在且是文件
    if !path.exists() || !path.is_file() {
        return false;
    }

    // 必须为绝对路径，避免执行相对路径文件
    if !path.is_absolute() {
        return false;
    }

    // 检查路径是否包含危险字符
    let path_str = path.to_string_lossy();
    let dangerous_chars = ['&', '|', ';', '<', '>', '(', ')', '`', '$', '^', '%', '!'];
    if path_str.chars().any(|c| dangerous_chars.contains(&c)) {
        tracing::warn!(
            "[CLI-Sync] Path contains dangerous characters: {}",
            path_str
        );
        return false;
    }

    true
}

/// 执行版本命令（Windows 特殊处理 .cmd/.bat）
#[cfg(target_os = "windows")]
fn run_version_command(executable_path: &PathBuf) -> Option<String> {
    // 安全校验：验证路径不包含危险字符
    if !is_safe_path(executable_path) {
        return None;
    }

    let output = if is_cmd_file(executable_path) {
        // 使用引号包裹路径防止注入，使用 /S 开关确保安全解析
        let quoted_path = format!("\"{}\"", executable_path.to_string_lossy());
        Command::new("cmd.exe")
            .arg("/C")
            .arg(&quoted_path)
            .arg("--version")
            .creation_flags(CREATE_NO_WINDOW)
            .output()
    } else {
        let mut cmd = Command::new(executable_path);
        cmd.arg("--version");
        cmd.creation_flags(CREATE_NO_WINDOW);
        cmd.output()
    };

    match output {
        Ok(out) if out.status.success() => {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            // 使用正则提取版本号（更精确）
            extract_version(&s)
        }
        _ => None,
    }
}

/// 提取版本号（使用更精确的 semver 匹配）
fn extract_version(s: &str) -> Option<String> {
    // 匹配 semver 格式: x.y.z 或 x.y
    let re = regex::Regex::new(r"(\d+\.\d+(?:\.\d+)?)").ok()?;
    re.captures(s)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_string())
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum CliApp {
    Claude,
    Codex,
    Gemini,
    OpenCode,
    JeikCode,
    GrokBuild,
    DeepSeekHarness,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct CliConfigFile {
    pub name: String,
    pub path: PathBuf,
}

impl CliApp {
    pub fn as_str(&self) -> &'static str {
        match self {
            CliApp::Claude => "claude",
            CliApp::Codex => "codex",
            CliApp::Gemini => "gemini",
            CliApp::OpenCode => "opencode",
            CliApp::JeikCode => "jeikcode",
            CliApp::GrokBuild => "grok",
            CliApp::DeepSeekHarness => "dsh",
        }
    }

    pub fn config_files(&self) -> Vec<CliConfigFile> {
        let home = match dirs::home_dir() {
            Some(p) => p,
            None => return vec![],
        };
        match self {
            CliApp::Claude => vec![
                CliConfigFile {
                    name: ".claude.json".to_string(),
                    path: home.join(".claude.json"),
                },
                CliConfigFile {
                    name: "settings.json".to_string(),
                    path: home.join(".claude").join("settings.json"),
                },
            ],
            CliApp::Codex => {
                let mut files = Vec::new();
                let has_chatgpt = home.join(".chatgpt").exists();
                let has_codex = home.join(".codex").exists();
                let dirs_to_sync = if has_chatgpt && has_codex {
                    vec![home.join(".chatgpt"), home.join(".codex")]
                } else if has_chatgpt {
                    vec![home.join(".chatgpt")]
                } else {
                    vec![home.join(".codex")]
                };

                for dir in dirs_to_sync {
                    files.push(CliConfigFile {
                        name: "auth.json".to_string(),
                        path: dir.join("auth.json"),
                    });
                    files.push(CliConfigFile {
                        name: "config.toml".to_string(),
                        path: dir.join("config.toml"),
                    });
                }
                files
            }
            CliApp::Gemini => {
                let mut files = vec![
                    CliConfigFile {
                        name: ".env".to_string(),
                        path: home.join(".gemini").join(".env"),
                    },
                    CliConfigFile {
                        name: "settings.json".to_string(),
                        path: home.join(".gemini").join("settings.json"),
                    },
                    CliConfigFile {
                        name: "antigravity-cli/settings.json".to_string(),
                        path: home
                            .join(".gemini")
                            .join("antigravity-cli")
                            .join("settings.json"),
                    },
                    CliConfigFile {
                        name: "config.json".to_string(),
                        path: home.join(".gemini").join("config.json"),
                    },
                ];
                #[cfg(target_os = "windows")]
                files.push(CliConfigFile {
                    name: "agy.cmd".to_string(),
                    path: home.join(".gemini").join("agy.cmd"),
                });
                #[cfg(not(target_os = "windows"))]
                files.push(CliConfigFile {
                    name: "agy.sh".to_string(),
                    path: home.join(".gemini").join("agy.sh"),
                });
                files
            }
            CliApp::OpenCode => vec![CliConfigFile {
                name: "config.json".to_string(),
                path: home.join(".opencode").join("config.json"),
            }],
            CliApp::JeikCode => {
                let jeikcode_dir = if let Ok(custom_home) = std::env::var("JEIKCODE_HOME") {
                    PathBuf::from(custom_home)
                } else {
                    home.join(".jeikcode")
                };
                vec![CliConfigFile {
                    name: "config.toml".to_string(),
                    path: jeikcode_dir.join("config.toml"),
                }]
            }
            CliApp::GrokBuild => {
                let grok_dir = if let Ok(custom) =
                    std::env::var("GROK_HOME").or_else(|_| std::env::var("GROKBUILD_HOME"))
                {
                    PathBuf::from(custom)
                } else if home.join(".grok").exists() || !home.join(".grokbuild").exists() {
                    home.join(".grok")
                } else {
                    home.join(".grokbuild")
                };
                vec![CliConfigFile {
                    name: "config.toml".to_string(),
                    path: grok_dir.join("config.toml"),
                }]
            }
            CliApp::DeepSeekHarness => {
                let dsh_dir = home.join(".dsh");
                vec![
                    CliConfigFile {
                        name: "profiles/desktop/cordis.patch.yml".to_string(),
                        path: dsh_dir
                            .join("profiles")
                            .join("desktop")
                            .join("cordis.patch.yml"),
                    },
                    CliConfigFile {
                        name: "profiles/web/cordis.patch.yml".to_string(),
                        path: dsh_dir
                            .join("profiles")
                            .join("web")
                            .join("cordis.patch.yml"),
                    },
                    CliConfigFile {
                        name: ".credentials.yaml".to_string(),
                        path: dsh_dir.join(".credentials.yaml"),
                    },
                ]
            }
        }
    }

    pub fn default_url(&self) -> &'static str {
        match self {
            CliApp::Claude => "https://api.anthropic.com",
            CliApp::Codex => "https://api.openai.com/v1",
            CliApp::Gemini => "https://generativelanguage.googleapis.com",
            CliApp::OpenCode => "https://api.openai.com/v1",
            CliApp::JeikCode => "http://127.0.0.1:8046/v1",
            CliApp::GrokBuild => "https://api.x.ai/v1",
            CliApp::DeepSeekHarness => "https://api.deepseek.com/v1",
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CliStatus {
    pub installed: bool,
    pub version: Option<String>,
    pub is_synced: bool,
    pub has_backup: bool,
    pub current_base_url: Option<String>,
    pub current_model: Option<String>,
    pub files: Vec<String>, // 返回关联的文件名列表供前端展示
}

/// 检测 CLI 是否安装并获取版本
pub fn check_cli_installed(app: &CliApp) -> (bool, Option<String>) {
    let candidate_cmds: Vec<&str> = match app {
        CliApp::Gemini => vec!["agy_cli", "agy", "gemini"],
        CliApp::JeikCode => vec!["jeikcode", "atomcode"],
        CliApp::GrokBuild => vec!["grok", "grokbuild"],
        CliApp::DeepSeekHarness => vec!["dsh", "deepseek-harness", "deepseek"],
        _ => vec![app.as_str()],
    };

    let mut installed = false;
    let mut executable_path = PathBuf::from(candidate_cmds[0]);

    for cmd in candidate_cmds {
        // 1. 优先使用 which/where 检测 (遵循 PATH)
        let which_output = if cfg!(target_os = "windows") {
            let mut c = Command::new("where");
            c.arg(cmd);
            #[cfg(target_os = "windows")]
            c.creation_flags(CREATE_NO_WINDOW);
            c.output()
        } else {
            Command::new("which").arg(cmd).output()
        };

        if let Ok(out) = &which_output {
            if out.status.success() {
                installed = true;
                #[cfg(target_os = "windows")]
                {
                    if let Some(found_path) = parse_where_output(&out.stdout) {
                        executable_path = found_path;
                    }
                }
                #[cfg(not(target_os = "windows"))]
                {
                    let trimmed = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if !trimmed.is_empty() {
                        executable_path = PathBuf::from(trimmed);
                    } else {
                        executable_path = PathBuf::from(cmd);
                    }
                }
                break;
            }
        }

        #[cfg(target_os = "windows")]
        if let Some(found_path) = scan_windows_cli_paths(cmd) {
            installed = true;
            executable_path = found_path;
            break;
        }

        // [FIX #765] macOS 增强检测: 如果 which 失败,显式搜索常用二进制路径
        // 解决 Tauri 进程 PATH 可能不完整导致检测不到已安装 CLI 的问题
        if !cfg!(target_os = "windows") {
            let home = dirs::home_dir().unwrap_or_default();
            let mut common_paths = vec![
                home.join(".local/bin"),
                home.join(".bun/bin"),
                home.join(".bun/install/global/node_modules/.bin"),
                home.join(".npm-global/bin"),
                home.join(".volta/bin"),
                home.join("bin"),
                PathBuf::from("/opt/homebrew/bin"),
                PathBuf::from("/usr/local/bin"),
                PathBuf::from("/usr/bin"),
            ];

            // 增强：扫描 nvm 目录下的所有 node 版本
            let nvm_base = home.join(".nvm/versions/node");
            if nvm_base.exists() {
                if let Ok(entries) = std::fs::read_dir(&nvm_base) {
                    for entry in entries.flatten() {
                        let bin_path = entry.path().join("bin");
                        if bin_path.exists() {
                            common_paths.push(bin_path);
                        }
                    }
                }
            }

            let mut found_explicit = false;
            for path in common_paths {
                let full_path = path.join(cmd);
                if full_path.exists() {
                    tracing::debug!(
                        "[CLI-Sync] Detected {} via explicit path: {:?}",
                        cmd,
                        full_path
                    );
                    installed = true;
                    executable_path = full_path;
                    found_explicit = true;
                    break;
                }
            }
            if found_explicit {
                break;
            }
        }
    }

    // 如果是 JeikCode 且常规检测未命中，尝试检测 atomcode 别名或配置文件是否存在
    if !installed && app == &CliApp::JeikCode {
        if let Some(p) = detect_fallback_binary("atomcode") {
            executable_path = p;
            installed = true;
        } else if let Some(home) = dirs::home_dir() {
            let jeikcode_dir = if let Ok(custom_home) = std::env::var("JEIKCODE_HOME") {
                PathBuf::from(custom_home)
            } else {
                home.join(".jeikcode")
            };
            if jeikcode_dir.join("config.toml").exists() || jeikcode_dir.exists() {
                installed = true;
            }
        }
    }

    // 如果是 GrokBuild 且常规检测未命中，尝试检测 grokbuild 别名或配置文件是否存在
    if !installed && app == &CliApp::GrokBuild {
        if let Some(p) = detect_fallback_binary("grokbuild") {
            executable_path = p;
            installed = true;
        } else if let Some(home) = dirs::home_dir() {
            let grok_dir = home.join(".grok");
            let grokbuild_dir = home.join(".grokbuild");
            if grok_dir.join("config.toml").exists()
                || grokbuild_dir.join("config.toml").exists()
                || grok_dir.exists()
                || grokbuild_dir.exists()
            {
                installed = true;
            }
        }
    }

    // 如果是 DeepSeek Harness 且常规检测未命中，尝试检测应用目录或 ~/.dsh 目录
    if !installed && app == &CliApp::DeepSeekHarness {
        #[cfg(target_os = "macos")]
        {
            let mac_dsh = PathBuf::from(
                "/Applications/DeepSeek Harness.app/Contents/Resources/runtime/cli/bin/dsh",
            );
            if mac_dsh.exists() {
                executable_path = mac_dsh;
                installed = true;
            } else if PathBuf::from("/Applications/DeepSeek Harness.app").exists() {
                installed = true;
            }
        }
        #[cfg(target_os = "windows")]
        if let Some(home) = dirs::home_dir() {
            let win_paths = vec![
                home.join(".dsh").join("bin").join("dsh.cmd"),
                home.join(".dsh").join("bin").join("dsh.exe"),
            ];
            for p in win_paths {
                if is_safe_path(&p) {
                    executable_path = p;
                    installed = true;
                    break;
                }
            }
        }
        if !installed {
            if let Some(home) = dirs::home_dir() {
                let dsh_dir = home.join(".dsh");
                if dsh_dir.exists() {
                    installed = true;
                }
            }
        }
    }

    if !installed {
        return (false, None);
    }

    // 2. 获取版本（Windows 使用特殊处理 .cmd/.bat）
    #[cfg(target_os = "windows")]
    let version = run_version_command(&executable_path);

    #[cfg(not(target_os = "windows"))]
    let version = {
        let mut ver_cmd = Command::new(&executable_path);
        ver_cmd.arg("--version");
        let version_output = ver_cmd.output();
        match version_output {
            Ok(out) if out.status.success() => {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                let cleaned = s
                    .split(|c: char| !c.is_numeric() && c != '.')
                    .filter(|part| !part.is_empty())
                    .last()
                    .map(|p| p.trim())
                    .unwrap_or(&s)
                    .to_string();
                Some(cleaned)
            }
            _ => None,
        }
    };

    (true, version)
}

/// 读取当前配置并检测同步状态
pub fn get_sync_status(
    app: &CliApp,
    proxy_url: &str,
) -> (bool, bool, Option<String>, Option<String>) {
    let files = app.config_files();
    if files.is_empty() {
        return (false, false, None, None);
    }

    let mut all_synced = true;
    let mut has_backup = false;
    let mut current_base_url = None;
    let mut current_model = None;

    for file in &files {
        // 使用更简单的命名规则: original_name + .antigravity.bak
        let backup_path = file
            .path
            .with_file_name(format!("{}.antigravity.bak", file.name));

        if backup_path.exists() {
            has_backup = true;
        }

        // 如果物理文件不存在
        if !file.path.exists() {
            // Gemini 的 settings.json/config.json/agy.sh/agy.cmd 只要有一个存在即可，或者都不存在（视为未同步）
            if app == &CliApp::Gemini
                && (file.name == "settings.json"
                    || file.name == "config.json"
                    || file.name == "antigravity-cli/settings.json"
                    || file.name == "agy.sh"
                    || file.name == "agy.cmd")
            {
                continue;
            }
            all_synced = false;
            continue;
        }

        let content = match fs::read_to_string(&file.path) {
            Ok(c) => c,
            Err(_) => {
                all_synced = false;
                continue;
            }
        };

        match app {
            CliApp::Claude => {
                if file.name == "settings.json" {
                    let json: Value = serde_json::from_str(&content).unwrap_or_default();
                    let url = json
                        .get("env")
                        .and_then(|e| e.get("ANTHROPIC_BASE_URL"))
                        .and_then(|v| v.as_str());
                    if let Some(u) = url {
                        current_base_url = Some(u.to_string());
                        if u.trim_end_matches('/') != proxy_url.trim_end_matches('/') {
                            all_synced = false;
                        }
                    } else {
                        all_synced = false;
                    }
                } else if file.name == ".claude.json" {
                    let json: Value = serde_json::from_str(&content).unwrap_or_default();
                    if json.get("hasCompletedOnboarding") != Some(&Value::Bool(true)) {
                        all_synced = false;
                    }
                }
            }
            CliApp::Codex => {
                if file.name == "config.toml" {
                    // 正则匹配 base_url
                    let re =
                        regex::Regex::new(r#"(?m)^\s*base_url\s*=\s*['"]([^'"]+)['"]"#).unwrap();
                    if let Some(caps) = re.captures(&content) {
                        let url = &caps[1];
                        current_base_url = Some(url.to_string());
                        if url.trim_end_matches('/') != proxy_url.trim_end_matches('/') {
                            all_synced = false;
                        }
                    } else {
                        all_synced = false;
                    }
                }
            }
            CliApp::Gemini => {
                if file.name == ".env" {
                    let re = regex::Regex::new(r#"(?m)^GOOGLE_GEMINI_BASE_URL=(.*)$"#).unwrap();
                    if let Some(caps) = re.captures(&content) {
                        let url = caps[1].trim();
                        current_base_url = Some(url.to_string());
                        if url.trim_end_matches('/') != proxy_url.trim_end_matches('/') {
                            all_synced = false;
                        }
                    } else {
                        all_synced = false;
                    }

                    if current_model.is_none() {
                        let re_m = regex::Regex::new(r#"(?m)^GOOGLE_GEMINI_MODEL=(.*)$"#).unwrap();
                        if let Some(caps) = re_m.captures(&content) {
                            let m = caps[1].trim();
                            if !m.is_empty() {
                                current_model = Some(m.to_string());
                            }
                        }
                    }
                } else if file.name == "settings.json"
                    || file.name == "antigravity-cli/settings.json"
                {
                    if current_model.is_none() {
                        let json: Value = serde_json::from_str(&content).unwrap_or_default();
                        if let Some(m) = json.get("model").and_then(|v| v.as_str()) {
                            if !m.is_empty() {
                                current_model = Some(m.to_string());
                            }
                        }
                    }
                } else if (file.name == "agy.sh" || file.name == "agy.cmd")
                    && !content.contains(proxy_url.trim_end_matches('/'))
                {
                    all_synced = false;
                }
            }
            CliApp::OpenCode => {
                if file.name == "config.json" {
                    let json: Value = serde_json::from_str(&content).unwrap_or_default();
                    let url = json
                        .get("providers")
                        .and_then(|p| p.get("openai"))
                        .and_then(|o| o.get("baseURL"))
                        .and_then(|v| v.as_str());
                    if let Some(u) = url {
                        current_base_url = Some(u.to_string());
                        if u.trim_end_matches('/') != proxy_url.trim_end_matches('/') {
                            all_synced = false;
                        }
                    } else {
                        all_synced = false;
                    }
                }
            }
            CliApp::JeikCode => {
                if file.name == "config.toml" {
                    use toml_edit::DocumentMut;
                    if let Ok(doc) = content.parse::<DocumentMut>() {
                        let normalized_proxy = proxy_url
                            .trim_end_matches('/')
                            .trim_end_matches("/v1")
                            .trim_end_matches('/');

                        let mut is_ag_synced = false;
                        if let Some(accounts) =
                            doc.get("provider_accounts").and_then(|i| i.as_table())
                        {
                            // 优先检查 antigravity-manager 账号
                            if let Some(ag_item) = accounts.get("antigravity-manager") {
                                if let Some(base_url) =
                                    ag_item.get("base_url").and_then(|v| v.as_str())
                                {
                                    current_base_url = Some(base_url.to_string());
                                    let norm = base_url
                                        .trim_end_matches('/')
                                        .trim_end_matches("/v1")
                                        .trim_end_matches('/');
                                    let provider_type = ag_item
                                        .get("provider")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");
                                    if norm == normalized_proxy && provider_type == "anthropic" {
                                        is_ag_synced = true;
                                    }
                                }
                            }

                            // 若尚未找到，但有其他同 URL 的账号，记录其 URL
                            if current_base_url.is_none() {
                                for (_, acc_item) in accounts.iter() {
                                    if let Some(base_url) =
                                        acc_item.get("base_url").and_then(|v| v.as_str())
                                    {
                                        let norm = base_url
                                            .trim_end_matches('/')
                                            .trim_end_matches("/v1")
                                            .trim_end_matches('/');
                                        if norm == normalized_proxy {
                                            current_base_url = Some(base_url.to_string());
                                            break;
                                        }
                                    }
                                }
                            }
                        }

                        // 验证是否已同步最核心模型
                        let has_core_models =
                            if let Some(models) = doc.get("models").and_then(|i| i.as_table()) {
                                models.contains_key("claude-sonnet-4-6")
                            } else {
                                false
                            };

                        if is_ag_synced && has_core_models {
                            // 同步完全正常
                        } else {
                            all_synced = false;
                        }
                    } else {
                        all_synced = false;
                    }
                }
            }
            CliApp::GrokBuild => {
                if file.name == "config.toml" {
                    use toml_edit::DocumentMut;
                    if let Ok(doc) = content.parse::<DocumentMut>() {
                        let normalized_proxy = proxy_url
                            .trim_end_matches('/')
                            .trim_end_matches("/v1")
                            .trim_end_matches('/');

                        let mut matched = false;
                        if let Some(models) = doc.get("model").and_then(|i| i.as_table()) {
                            for (_, m_item) in models.iter() {
                                if let Some(b_url) = m_item.get("base_url").and_then(|v| v.as_str())
                                {
                                    current_base_url = Some(b_url.to_string());
                                    let norm = b_url
                                        .trim_end_matches('/')
                                        .trim_end_matches("/v1")
                                        .trim_end_matches('/');
                                    if norm == normalized_proxy {
                                        matched = true;
                                        break;
                                    }
                                }
                            }
                        }

                        if !matched {
                            all_synced = false;
                        }
                    } else {
                        all_synced = false;
                    }
                }
            }
            CliApp::DeepSeekHarness => {
                if file.name.ends_with("cordis.patch.yml") {
                    let normalized_proxy = proxy_url
                        .trim_end_matches('/')
                        .trim_end_matches("/v1")
                        .trim_end_matches('/');

                    let has_ag_provider = content.contains("provider: antigravity")
                        || content.contains("antigravity:");

                    let mut matched = false;
                    let re_url =
                        regex::Regex::new(r#"(?m)^\s*baseURL:\s*['"]?([^'"\s#]+)['"]?"#).ok();
                    if let Some(re) = re_url {
                        for cap in re.captures_iter(&content) {
                            if let Some(m) = cap.get(1) {
                                let found_url = m.as_str();
                                let norm_found = found_url
                                    .trim_end_matches('/')
                                    .trim_end_matches("/v1")
                                    .trim_end_matches('/');
                                if norm_found == normalized_proxy {
                                    current_base_url = Some(found_url.to_string());
                                    matched = true;
                                    break;
                                } else if current_base_url.is_none() {
                                    current_base_url = Some(found_url.to_string());
                                }
                            }
                        }
                    }

                    if current_model.is_none() {
                        let re_model = regex::Regex::new(
                            r#"(?s)id:\s*agent-default-model[\s\S]*?config:[\s\S]*?model:\s*['"]?([a-zA-Z0-9_.-]+)['"]?"#,
                        )
                        .ok();
                        if let Some(re) = re_model {
                            if let Some(cap) = re.captures(&content) {
                                if let Some(m) = cap.get(1) {
                                    current_model = Some(m.as_str().to_string());
                                }
                            }
                        }
                    }

                    if !matched || !has_ag_provider {
                        all_synced = false;
                    }
                }
            }
        }
    }

    if app == &CliApp::Gemini && current_model.is_none() {
        if let Some(home) = dirs::home_dir() {
            let agy_script = home.join(".local/bin/agy");
            if let Ok(script) = fs::read_to_string(&agy_script) {
                let re = regex::Regex::new(
                    r#"(?m)^\s*export\s+GOOGLE_GEMINI_MODEL\s*=\s*['"]?([^'"\r\n]+)['"]?"#,
                )
                .unwrap();
                if let Some(caps) = re.captures(&script) {
                    current_model = Some(caps[1].trim().to_string());
                } else {
                    let re_raw =
                        regex::Regex::new(r#"(?m)^\s*RAW_MODEL\s*=\s*['"]?([^'"\r\n]+)['"]?"#)
                            .unwrap();
                    if let Some(caps) = re_raw.captures(&script) {
                        current_model = Some(caps[1].trim().to_string());
                    }
                }
            }
        }
    }

    (all_synced, has_backup, current_base_url, current_model)
}

/// 为 Antigravity CLI 创建或更新 Shell Wrapper 脚本
/// 将真实二进制安全更名为 agy_cli，并在原 agy 位置写入自动注入环境变量并 exec agy_cli 的包装脚本
pub fn sync_agy_binary_wrapper(
    proxy_url: &str,
    api_key: &str,
    model: Option<&str>,
) -> Result<(), String> {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return Ok(()),
    };

    // 检查常见安装路径 ~/.local/bin
    let local_bin = home.join(".local/bin");
    let agy_path = local_bin.join("agy");
    let agy_cli_path = local_bin.join("agy_cli");

    if !local_bin.exists() {
        let _ = fs::create_dir_all(&local_bin);
    }

    let target_model = match model {
        Some(m) if !m.is_empty() => m,
        _ => "gemini-3.8-flash-high",
    };

    // 隔离历史 Google OAuth token，彻底阻断打向 daily-cloudcode-pa.googleapis.com
    let token_file = home.join(".gemini/antigravity-cli/antigravity-oauth-token");
    if token_file.exists() {
        let bak = home.join(".gemini/antigravity-cli/antigravity-oauth-token.antigravity.bak");
        let _ = fs::rename(&token_file, &bak);
    }

    let wrapper_content = format!(
        "#!/bin/zsh\n\
         # Generated by Antigravity-Manager (Native Resilience & Auto-Continue)\n\
         export GEMINI_API_KEY=\"{}\"\n\
         export GOOGLE_GEMINI_BASE_URL=\"{}\"\n\
         export GOOGLE_GEMINI_MODEL=\"{}\"\n\
         export DO_NOT_TRACK=1\n\
         export GEMINI_TELEMETRY_ENABLED=false\n\n\
         # 智能拆分 base model 与 thinking effort (如 gemini-3.8-flash-low -> --model gemini-3.8-flash --effort low)\n\
         RAW_MODEL=\"{}\"\n\
         MODEL_ARGS=()\n\
         if [[ ! \"$*\" =~ \"--model\" ]]; then\n\
             EFFORT=\"${{RAW_MODEL##*-}}\"\n\
             BASE_MODEL=\"${{RAW_MODEL%-*}}\"\n\
             case \"$EFFORT\" in\n\
                 high|medium|low|xhigh|max)\n\
                     MODEL_ARGS=(--model \"$BASE_MODEL\" --effort \"$EFFORT\")\n\
                     ;;\n\
                 *)\n\
                     MODEL_ARGS=(--model \"$RAW_MODEL\")\n\
                     ;;\n\
             esac\n\
         fi\n\n\
         AGY_BIN=\"$(dirname \"$0\")/agy_cli\"\n\
         while true; do\n\
             \"$AGY_BIN\" \"${{MODEL_ARGS[@]}}\" \"$@\"\n\
             EXIT_CODE=$?\n\
             if [ $EXIT_CODE -eq 0 ]; then\n\
                 exit 0\n\
             fi\n\
             if command -v antigravity-tools >/dev/null 2>&1; then\n\
                 echo \"\\n\\033[33m[Antigravity] 检测到会话异常/配额用尽，正在自动顺延切号...\\033[0m\"\n\
                 antigravity-tools switch --target agy\n\
                 if [ $? -eq 0 ]; then\n\
                     echo \"\\033[32m[Antigravity] 账号切换成功，正在以 --continue 恢复会话...\\033[0m\\n\"\n\
                     set -- \"--continue\"\n\
                     continue\n\
                 fi\n\
             fi\n\
             exit $EXIT_CODE\n\
         done\n",
        api_key, proxy_url, target_model, target_model
    );

    // 情况 1: 如果已经存在 agy_cli（已经被包装过）
    if agy_cli_path.exists() {
        let _ = fs::write(&agy_path, &wrapper_content);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&agy_path, fs::Permissions::from_mode(0o755));
        }
        return Ok(());
    }

    // 情况 2: 如果当前 agy 存在且是真实二进制文件（非脚本）
    if agy_path.exists() {
        if let Ok(bytes) = fs::read(&agy_path) {
            let is_script = bytes.starts_with(b"#!");
            if !is_script {
                // 是二进制文件，重命名为 agy_cli
                if let Err(e) = fs::rename(&agy_path, &agy_cli_path) {
                    tracing::warn!("[CLI-Sync] Failed to rename agy to agy_cli: {}", e);
                    return Ok(());
                }
                // 写入 wrapper 脚本到 agy_path
                let _ = fs::write(&agy_path, &wrapper_content);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = fs::set_permissions(&agy_path, fs::Permissions::from_mode(0o755));
                }
                tracing::info!(
                    "[CLI-Sync] Successfully converted agy binary to agy_cli and installed wrapper"
                );
            } else {
                // 是脚本，直接更新内容
                let _ = fs::write(&agy_path, &wrapper_content);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = fs::set_permissions(&agy_path, fs::Permissions::from_mode(0o755));
                }
            }
        }
    }

    Ok(())
}

/// 恢复 Antigravity CLI 二进制文件：如果存在 agy_cli 且 agy 是脚本，则将 agy_cli 恢复重命名为 agy
pub fn restore_agy_binary_wrapper() -> Result<(), String> {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return Ok(()),
    };

    let local_bin = home.join(".local/bin");
    let agy_path = local_bin.join("agy");
    let agy_cli_path = local_bin.join("agy_cli");

    if agy_cli_path.exists() {
        let should_restore = if agy_path.exists() {
            if let Ok(content) = fs::read_to_string(&agy_path) {
                content.contains("agy_cli") || content.contains("Antigravity-Manager")
            } else {
                false
            }
        } else {
            true
        };

        if should_restore {
            let _ = fs::remove_file(&agy_path);
            if let Err(e) = fs::rename(&agy_cli_path, &agy_path) {
                return Err(format!("恢复 agy 二进制失败: {}", e));
            }
            tracing::info!("[CLI-Sync] Restored agy_cli back to agy");
        }
    }

    Ok(())
}

/// 执行同步逻辑
pub fn sync_config(
    app: &CliApp,
    proxy_url: &str,
    api_key: &str,
    model: Option<&str>,
) -> Result<(), String> {
    let files = app.config_files();

    for file in &files {
        // Gemini 兼容性逻辑：优先使用 settings.json
        if app == &CliApp::Gemini && file.name == "config.json" && !file.path.exists() {
            let settings_path = file.path.with_file_name("settings.json");
            if settings_path.exists() {
                continue;
            }
        }

        if let Some(parent) = file.path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("无法创建目录: {}", e))?;
        }

        // [New Feature] 自动备份：如果文件存在且没有备份，创建 .antigravity.bak 备份
        // 这样可以保留用户最初的配置，后续多次同步不会覆盖这个备份
        if file.path.exists() {
            let backup_path = file
                .path
                .with_file_name(format!("{}.antigravity.bak", file.name));
            if !backup_path.exists() {
                if let Err(e) = fs::copy(&file.path, &backup_path) {
                    tracing::warn!("Failed to create backup for {}: {}", file.name, e);
                } else {
                    tracing::info!("Created backup for {}: {:?}", file.name, backup_path);
                }
            }
        }

        let mut content = if file.path.exists() {
            fs::read_to_string(&file.path).unwrap_or_default()
        } else {
            String::new()
        };

        match app {
            CliApp::Claude => {
                if file.name == ".claude.json" {
                    let mut json: Value =
                        serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}));
                    if let Some(obj) = json.as_object_mut() {
                        obj.insert("hasCompletedOnboarding".to_string(), Value::Bool(true));
                    }
                    content = serde_json::to_string_pretty(&json).unwrap();
                } else if file.name == "settings.json" {
                    let mut json: serde_json::Value =
                        serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}));
                    if json.as_object().is_none() {
                        json = serde_json::json!({});
                    }
                    let env = json
                        .as_object_mut()
                        .unwrap()
                        .entry("env")
                        .or_insert(serde_json::json!({}));
                    if let Some(env_obj) = env.as_object_mut() {
                        env_obj.insert(
                            "ANTHROPIC_BASE_URL".to_string(),
                            Value::String(proxy_url.to_string()),
                        );
                        if !api_key.is_empty() {
                            if proxy_url.contains("apikey.fun") || proxy_url.contains("apikey.fan")
                            {
                                env_obj.insert(
                                    "ANTHROPIC_AUTH_TOKEN".to_string(),
                                    Value::String(api_key.to_string()),
                                );
                                env_obj.insert(
                                    "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC".to_string(),
                                    Value::String("1".to_string()),
                                );
                                env_obj.insert(
                                    "CLAUDE_CODE_ATTRIBUTION_HEADER".to_string(),
                                    Value::String("0".to_string()),
                                );
                                env_obj.remove("ANTHROPIC_API_KEY");
                            } else {
                                env_obj.insert(
                                    "ANTHROPIC_API_KEY".to_string(),
                                    Value::String(api_key.to_string()),
                                );
                                // [FIX] 避免冲突：如果存在则移除 ANTHROPIC_AUTH_TOKEN
                                env_obj.remove("ANTHROPIC_AUTH_TOKEN");
                            }

                            // [FIX] 清理可能来自其他 Provider 的模型覆盖设置
                            env_obj.remove("ANTHROPIC_MODEL");
                            env_obj.remove("ANTHROPIC_DEFAULT_HAIKU_MODEL");
                            env_obj.remove("ANTHROPIC_DEFAULT_OPUS_MODEL");
                            env_obj.remove("ANTHROPIC_DEFAULT_SONNET_MODEL");
                        } else {
                            // 如果 API Key 为空，则移除该键，避免设置为空字符串
                            env_obj.remove("ANTHROPIC_API_KEY");
                            env_obj.remove("ANTHROPIC_AUTH_TOKEN");
                        }
                    }

                    if let Some(m) = model {
                        // 注意：Claude Code 的官方配置中，当前选定模型放在根节点的 model 字段
                        json.as_object_mut()
                            .unwrap()
                            .insert("model".to_string(), Value::String(m.to_string()));
                    }
                    content = serde_json::to_string_pretty(&json).unwrap();
                }
            }
            CliApp::Codex => {
                if file.name == "auth.json" {
                    let mut json: Value =
                        serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}));
                    if let Some(obj) = json.as_object_mut() {
                        obj.insert(
                            "OPENAI_API_KEY".to_string(),
                            Value::String(api_key.to_string()),
                        );
                        if proxy_url.contains("apikey.fun") || proxy_url.contains("apikey.fan") {
                            obj.remove("OPENAI_BASE_URL");
                        } else {
                            // Codex 的 auth.json 似乎也支持 OPENAI_BASE_URL，但 ccs 没写，我们也同步写一下
                            obj.insert(
                                "OPENAI_BASE_URL".to_string(),
                                Value::String(proxy_url.to_string()),
                            );
                        }
                    }
                    content = serde_json::to_string_pretty(&json).unwrap();
                } else if file.name == "config.toml" {
                    use toml_edit::{value, DocumentMut};
                    let mut doc = content
                        .parse::<DocumentMut>()
                        .unwrap_or_else(|_| DocumentMut::new());

                    // 必须使用 custom 提供商，Codex 不支持原生的 codex provider
                    let provider_key = "custom";
                    let is_apikey_fun =
                        proxy_url.contains("apikey.fun") || proxy_url.contains("apikey.fan");
                    let display_name = if is_apikey_fun {
                        "APIKEY.FUN"
                    } else {
                        "Custom Node"
                    };

                    // 优先设置 Root Keys 确保位于顶部
                    doc.insert("model_provider", value(provider_key));

                    if is_apikey_fun {
                        doc.insert("model", value("gpt-5.5"));
                        doc.insert("review_model", value("gpt-5.5"));
                        doc.insert("model_reasoning_effort", value("high"));
                        doc.insert("disable_response_storage", value(true));
                        doc.insert("network_access", value("enabled"));
                        doc.insert("windows_wsl_setup_acknowledged", value(true));
                        doc.insert("model_context_window", value(270000));
                        doc.insert("model_auto_compact_token_limit", value(270000));
                        doc.insert("effective_context_window_percent", value(95));
                    } else {
                        if let Some(m) = model {
                            doc.insert("model", value(m));
                            let cw = if m.contains("gemini") {
                                1_024_000
                            } else if m.contains("claude") {
                                256_000
                            } else {
                                128_000
                            };
                            doc.insert("model_context_window", value(cw));
                            doc.insert("model_auto_compact_token_limit", value(cw));
                        }
                    }

                    // 移除可能的根级别旧配置
                    doc.remove("openai_api_key");
                    doc.remove("openai_base_url");

                    // 设置层级 [model_providers.custom]
                    let providers = doc
                        .entry("model_providers")
                        .or_insert(toml_edit::Item::Table(toml_edit::Table::new()));
                    if let Some(p_table) = providers.as_table_mut() {
                        let custom = p_table
                            .entry(provider_key)
                            .or_insert(toml_edit::Item::Table(toml_edit::Table::new()));
                        if let Some(c_table) = custom.as_table_mut() {
                            c_table.insert("name", value(display_name));
                            c_table.insert("wire_api", value("responses"));
                            c_table.insert("requires_openai_auth", value(true));
                            c_table.insert("base_url", value(proxy_url.to_string()));
                            if let Some(m) = model {
                                c_table.insert("model", value(m));
                            }
                        }
                    }

                    if is_apikey_fun {
                        let features = doc
                            .entry("features")
                            .or_insert(toml_edit::Item::Table(toml_edit::Table::new()));
                        if let Some(f_table) = features.as_table_mut() {
                            f_table.insert("goals", value(true));
                        }
                    }
                    content = doc.to_string();
                }
            }
            CliApp::Gemini => {
                if file.name == ".env" {
                    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
                    let mut found_url = false;
                    let mut found_key = false;
                    let mut found_dnt = false;
                    let mut found_telemetry = false;
                    for line in lines.iter_mut() {
                        if line.starts_with("GOOGLE_GEMINI_BASE_URL=") {
                            *line = format!("GOOGLE_GEMINI_BASE_URL={}", proxy_url);
                            found_url = true;
                        } else if line.trim().starts_with("GEMINI_API_KEY=") {
                            *line = format!("GEMINI_API_KEY={}", api_key);
                            found_key = true;
                        } else if line.trim().starts_with("DO_NOT_TRACK=") {
                            *line = "DO_NOT_TRACK=1".to_string();
                            found_dnt = true;
                        } else if line.trim().starts_with("GEMINI_TELEMETRY_ENABLED=") {
                            *line = "GEMINI_TELEMETRY_ENABLED=false".to_string();
                            found_telemetry = true;
                        }
                    }
                    if !found_url {
                        lines.push(format!("GOOGLE_GEMINI_BASE_URL={}", proxy_url));
                    }
                    if !found_key {
                        lines.push(format!("GEMINI_API_KEY={}", api_key));
                    }
                    if !found_dnt {
                        lines.push("DO_NOT_TRACK=1".to_string());
                    }
                    if !found_telemetry {
                        lines.push("GEMINI_TELEMETRY_ENABLED=false".to_string());
                    }
                    if let Some(m) = model {
                        let mut found_model = false;
                        for line in lines.iter_mut() {
                            if line.starts_with("GOOGLE_GEMINI_MODEL=") {
                                *line = format!("GOOGLE_GEMINI_MODEL={}", m);
                                found_model = true;
                            }
                        }
                        if !found_model {
                            lines.push(format!("GOOGLE_GEMINI_MODEL={}", m));
                        }
                    }
                    content = lines.join("\n");
                } else if file.name == "agy.sh" {
                    let target_model = match model {
                        Some(m) if !m.is_empty() => m,
                        _ => "gemini-3.8-flash-high",
                    };
                    content = format!(
                        "#!/bin/zsh\n\
                         # Generated by Antigravity-Manager\n\
                         export GEMINI_API_KEY=\"{}\"\n\
                         export GOOGLE_GEMINI_BASE_URL=\"{}\"\n\
                         export GOOGLE_GEMINI_MODEL=\"{}\"\n\
                         export DO_NOT_TRACK=1\n\
                         export GEMINI_TELEMETRY_ENABLED=false\n\n\
                         MODEL_ARG=()\n\
                         if [[ ! \"$*\" =~ \"--model\" ]]; then\n\
                             MODEL_ARG=(--model \"{}\")\n\
                         fi\n\n\
                         exec agy \"${{MODEL_ARG[@]}}\" \"$@\"\n",
                        api_key, proxy_url, target_model, target_model
                    );
                } else if file.name == "agy.cmd" {
                    let target_model = match model {
                        Some(m) if !m.is_empty() => m,
                        _ => "gemini-3.8-flash-high",
                    };
                    content = format!(
                        "@echo off\r\n\
                         REM Generated by Antigravity-Manager\r\n\
                         set \"GEMINI_API_KEY={}\"\r\n\
                         set \"GOOGLE_GEMINI_BASE_URL={}\"\r\n\
                         set \"GOOGLE_GEMINI_MODEL={}\"\r\n\
                         set \"DO_NOT_TRACK=1\"\r\n\
                         set \"GEMINI_TELEMETRY_ENABLED=false\"\r\n\r\n\
                         agy --model {} %*\r\n",
                        api_key, proxy_url, target_model, target_model
                    );
                } else if file.name == "settings.json"
                    || file.name == "config.json"
                    || file.name == "antigravity-cli/settings.json"
                {
                    let mut json: Value =
                        serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}));
                    if json.as_object().is_none() {
                        json = serde_json::json!({});
                    }

                    // Antigravity CLI 关键配置：声明 modelProvider 为 gemini 才能脱离 Google OAuth 走 Base URL
                    if let Some(obj) = json.as_object_mut() {
                        obj.insert(
                            "modelProvider".to_string(),
                            Value::String("gemini".to_string()),
                        );
                        if let Some(m) = model {
                            if !m.is_empty() {
                                obj.insert("model".to_string(), Value::String(m.to_string()));
                            }
                        }
                    }

                    let sec = json
                        .as_object_mut()
                        .unwrap()
                        .entry("security")
                        .or_insert(serde_json::json!({}));
                    let auth = sec
                        .as_object_mut()
                        .unwrap()
                        .entry("auth")
                        .or_insert(serde_json::json!({}));
                    if let Some(auth_obj) = auth.as_object_mut() {
                        auth_obj.insert(
                            "selectedType".to_string(),
                            Value::String("gemini-api-key".to_string()),
                        );
                    }
                    content = serde_json::to_string_pretty(&json).unwrap();
                }
            }
            CliApp::OpenCode => {
                if file.name == "config.json" {
                    let mut json: Value =
                        serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}));
                    if json.as_object().is_none() {
                        json = serde_json::json!({});
                    }
                    let providers = json
                        .as_object_mut()
                        .unwrap()
                        .entry("providers")
                        .or_insert(serde_json::json!({}));
                    let openai = providers
                        .as_object_mut()
                        .unwrap()
                        .entry("openai")
                        .or_insert(serde_json::json!({}));
                    if let Some(openai_obj) = openai.as_object_mut() {
                        openai_obj
                            .insert("baseURL".to_string(), Value::String(proxy_url.to_string()));
                        if !api_key.is_empty() {
                            openai_obj
                                .insert("apiKey".to_string(), Value::String(api_key.to_string()));
                        }
                    }
                    content = serde_json::to_string_pretty(&json).unwrap();
                }
            }
            CliApp::JeikCode => {
                if file.name == "config.toml" {
                    content = sync_jeikcode_toml_content(&content, proxy_url, api_key, model)?;
                }
            }
            CliApp::GrokBuild => {
                if file.name == "config.toml" {
                    use toml_edit::{value, DocumentMut, Item, Table};
                    let mut doc = content
                        .parse::<DocumentMut>()
                        .unwrap_or_else(|_| DocumentMut::new());

                    let normalized_proxy_url = {
                        let trimmed = proxy_url.trim().trim_end_matches('/');
                        if trimmed.ends_with("/v1") {
                            trimmed.to_string()
                        } else {
                            format!("{}/v1", trimmed)
                        }
                    };

                    let core_models = get_core_gateway_models();

                    let default_model_id = match model {
                        Some(m) if !m.is_empty() => m,
                        _ => "gemini-3.8-flash-high",
                    };

                    // 1. 设置 [models] default 为真实选定的网关模型
                    let models_sec = doc.entry("models").or_insert(Item::Table(Table::new()));
                    if let Some(m_table) = models_sec.as_table_mut() {
                        m_table.insert("default", value(default_model_id));
                    }

                    // 2. 注入所有网关真实模型到 [model."<id>"]
                    let model_sec = doc.entry("model").or_insert(Item::Table(Table::new()));
                    if let Some(m_table) = model_sec.as_table_mut() {
                        for m in core_models {
                            let entry = m_table.entry(m.id).or_insert(Item::Table(Table::new()));
                            if let Some(t) = entry.as_table_mut() {
                                t.insert("model", value(m.id));
                                t.insert("base_url", value(&normalized_proxy_url));
                                t.insert("name", value(m.id));
                                t.insert("api_backend", value("responses"));
                                t.insert("context_window", value(m.context_window));
                                t.insert("image_input", value(true));
                                if !api_key.is_empty() {
                                    t.insert("api_key", value(api_key));
                                }
                            }
                        }
                    }

                    content = doc.to_string();
                }
            }
            CliApp::DeepSeekHarness => {
                if file.name.ends_with("cordis.patch.yml") {
                    let target_model = match model {
                        Some(m) if !m.is_empty() => m,
                        _ => "gemini-3.8-flash-high",
                    };
                    let normalized_proxy_url = {
                        let trimmed = proxy_url.trim().trim_end_matches('/');
                        if trimmed.ends_with("/v1") {
                            trimmed.to_string()
                        } else {
                            format!("{}/v1", trimmed)
                        }
                    };
                    content = sync_dsh_cordis_patch_content(
                        &content,
                        &normalized_proxy_url,
                        target_model,
                    );
                } else if file.name == ".credentials.yaml" {
                    content = sync_dsh_credentials_content(&content, api_key);
                }
            }
        }

        // 使用临时文件原子写入
        let tmp_path = file.path.with_extension("tmp");
        fs::write(&tmp_path, &content).map_err(|e| format!("写入临时文件失败: {}", e))?;
        fs::rename(&tmp_path, &file.path).map_err(|e| format!("重命名配置文件失败: {}", e))?;

        #[cfg(unix)]
        if file.name.ends_with(".sh") {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(metadata) = fs::metadata(&file.path) {
                let mut perms = metadata.permissions();
                perms.set_mode(0o755);
                let _ = fs::set_permissions(&file.path, perms);
            }
        }
    }

    if app == &CliApp::Gemini {
        let _ = sync_agy_binary_wrapper(proxy_url, api_key, model);
    }

    Ok(())
}

// Tauri Commands

#[tauri::command]
pub async fn get_cli_sync_status(app_type: CliApp, proxy_url: String) -> Result<CliStatus, String> {
    tokio::task::spawn_blocking(move || {
        let (installed, version) = check_cli_installed(&app_type);
        let (is_synced, has_backup, current_base_url, current_model) = if installed {
            get_sync_status(&app_type, &proxy_url)
        } else {
            (false, false, None, None)
        };

        Ok(CliStatus {
            installed,
            version,
            is_synced,
            has_backup,
            current_base_url,
            current_model,
            files: app_type
                .config_files()
                .into_iter()
                .map(|f| f.name)
                .collect(),
        })
    })
    .await
    .unwrap_or_else(|_| Err("Task panicked".to_string()))
}

#[tauri::command]
pub async fn execute_cli_sync(
    app_type: CliApp,
    proxy_url: String,
    api_key: String,
    model: Option<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        sync_config(&app_type, &proxy_url, &api_key, model.as_deref())
    })
    .await
    .unwrap_or_else(|_| Err("Task panicked".to_string()))
}

#[tauri::command]
pub async fn execute_cli_restore(app_type: CliApp) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let files = app_type.config_files();
        let mut restored_count = 0;

        if app_type == CliApp::Gemini {
            let _ = restore_agy_binary_wrapper();
        }

        // 尝试从备份恢复
        for file in &files {
            let backup_path = file
                .path
                .with_file_name(format!("{}.antigravity.bak", file.name));
            if backup_path.exists() {
                // 还原：覆盖原文件
                if let Err(e) = fs::rename(&backup_path, &file.path) {
                    return Err(format!("恢复备份失败 {}: {}", file.name, e));
                }
                restored_count += 1;
            }
        }

        if restored_count > 0 {
            // 如果成功恢复了至少一个备份，就认为是恢复成功
            return Ok(());
        }

        // 如果没有备份，则执行原来的逻辑：恢复为默认配置
        let default_url = app_type.default_url();
        // 恢复默认时清空 API Key，让用户重新授权或使用官方 Key
        sync_config(&app_type, default_url, "", None)
    })
    .await
    .unwrap_or_else(|_| Err("Task panicked".to_string()))
}

#[tauri::command]
pub async fn get_cli_config_content(
    app_type: CliApp,
    file_name: Option<String>,
) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let files = app_type.config_files();
        let file = if let Some(name) = file_name {
            files
                .into_iter()
                .find(|f| f.name == name)
                .ok_or("找不到指定的文件".to_string())?
        } else {
            files
                .into_iter()
                .next()
                .ok_or("找不到配置文件".to_string())?
        };

        if !file.path.exists() {
            if file.name == "agy.sh" {
                if let Some(home) = dirs::home_dir() {
                    let wrapper = home.join(".local/bin/agy");
                    if wrapper.exists() {
                        if let Ok(c) = fs::read_to_string(&wrapper) {
                            return Ok(c);
                        }
                    }
                }
                return Ok("#!/bin/zsh\n\
                     # Generated by Antigravity-Manager\n\
                     export GEMINI_API_KEY=\"sk-...\"\n\
                     export GOOGLE_GEMINI_BASE_URL=\"http://127.0.0.1:8045\"\n\
                     export GOOGLE_GEMINI_MODEL=\"gemini-3.8-flash-high\"\n\
                     export DO_NOT_TRACK=1\n\
                     export GEMINI_TELEMETRY_ENABLED=false\n\n\
                     MODEL_ARGS=()\n\
                     if [[ ! \"$*\" =~ \"--model\" ]]; then\n\
                         MODEL_ARGS=(--model \"gemini-3.8-flash\" --effort \"high\")\n\
                     fi\n\n\
                     exec agy \"${MODEL_ARGS[@]}\" \"$@\"\n"
                    .to_string());
            } else if file.name == "agy.cmd" {
                return Ok("@echo off\r\n\
                     REM Generated by Antigravity-Manager\r\n\
                     set \"GEMINI_API_KEY=sk-...\"\r\n\
                     set \"GOOGLE_GEMINI_BASE_URL=http://127.0.0.1:8045\"\r\n\
                     set \"GOOGLE_GEMINI_MODEL=gemini-3.8-flash-high\"\r\n\
                     set \"DO_NOT_TRACK=1\"\r\n\
                     set \"GEMINI_TELEMETRY_ENABLED=false\"\r\n\r\n\
                     agy --model gemini-3.8-flash-high %*\r\n"
                    .to_string());
            }
            return Err("配置文件不存在".to_string());
        }
        fs::read_to_string(&file.path).map_err(|e| format!("读取配置文件失败: {}", e))
    })
    .await
    .unwrap_or_else(|_| Err("Task panicked".to_string()))
}

#[derive(Debug, Clone, Copy)]
pub struct CoreModel {
    pub id: &'static str,
    pub context_window: i64,
}

/// 导出网关全量核心模型（仅保留官方最新支持的旗舰模型及后缀派生裸模型）
pub fn get_core_gateway_models() -> &'static [CoreModel] {
    &[
        // Claude 5.5 系列 (1M 上下文)
        CoreModel {
            id: "claude-opus-5-5",
            context_window: 1_000_000,
        },
        CoreModel {
            id: "claude-opus-5-5-high",
            context_window: 1_000_000,
        },
        CoreModel {
            id: "claude-opus-5-5-medium",
            context_window: 1_000_000,
        },
        CoreModel {
            id: "claude-opus-5-5-low",
            context_window: 1_000_000,
        },
        CoreModel {
            id: "claude-sonnet-5-5",
            context_window: 1_000_000,
        },
        CoreModel {
            id: "claude-sonnet-5-5-high",
            context_window: 1_000_000,
        },
        CoreModel {
            id: "claude-sonnet-5-5-medium",
            context_window: 1_000_000,
        },
        CoreModel {
            id: "claude-sonnet-5-5-low",
            context_window: 1_000_000,
        },
        // Claude 4.6 系列 (256k 上下文)
        CoreModel {
            id: "claude-opus-4-6",
            context_window: 256_000,
        },
        CoreModel {
            id: "claude-opus-4-6-thinking",
            context_window: 256_000,
        },
        CoreModel {
            id: "claude-sonnet-4-6",
            context_window: 256_000,
        },
        // Gemini 4 Argon 系列 (Google 旗舰推理模型，1M 上下文)
        CoreModel {
            id: "gemini-4-argon",
            context_window: 1_048_576,
        },
        CoreModel {
            id: "gemini-4-argon-high",
            context_window: 1_048_576,
        },
        CoreModel {
            id: "gemini-4-argon-medium",
            context_window: 1_048_576,
        },
        CoreModel {
            id: "gemini-4-argon-low",
            context_window: 1_048_576,
        },
        // Gemini 3.8 Flash 系列 (1M 上下文)
        CoreModel {
            id: "gemini-3.8-flash",
            context_window: 1_048_576,
        },
        CoreModel {
            id: "gemini-3.8-flash-tiered",
            context_window: 1_048_576,
        },
        CoreModel {
            id: "gemini-3.8-flash-high",
            context_window: 1_048_576,
        },
        CoreModel {
            id: "gemini-3.8-flash-medium",
            context_window: 1_048_576,
        },
        CoreModel {
            id: "gemini-3.8-flash-low",
            context_window: 1_048_576,
        },
        // Gemini 3.1 Pro & 专用模型
        CoreModel {
            id: "gemini-3.1-pro",
            context_window: 1_048_576,
        },
        CoreModel {
            id: "gemini-3.1-pro-high",
            context_window: 1_048_576,
        },
        CoreModel {
            id: "gemini-3.1-pro-low",
            context_window: 1_048_576,
        },
        CoreModel {
            id: "gemini-3.5-flash-lite",
            context_window: 1_048_576,
        },
        CoreModel {
            id: "gemini-3.1-flash-lite",
            context_window: 1_048_576,
        },
        CoreModel {
            id: "gemini-3.1-flash-image",
            context_window: 65_536,
        },
        // GPT-OSS 官方支持系列
        CoreModel {
            id: "gpt-oss-120b",
            context_window: 131_072,
        },
        CoreModel {
            id: "gpt-oss-120b-medium",
            context_window: 131_072,
        },
    ]
}

/// 核心逻辑：为 JeikCode 生成/更新 config.toml 内容
pub fn sync_jeikcode_toml_content(
    content: &str,
    proxy_url: &str,
    api_key: &str,
    model: Option<&str>,
) -> Result<String, String> {
    use toml_edit::{value, Array, DocumentMut, Item, Table};
    let mut doc = content
        .parse::<DocumentMut>()
        .unwrap_or_else(|_| DocumentMut::new());

    let target_provider_id = "antigravity-manager";
    let normalized_proxy_url = {
        let trimmed = proxy_url.trim().trim_end_matches('/');
        if trimmed.ends_with("/v1") {
            trimmed.to_string()
        } else {
            format!("{}/v1", trimmed)
        }
    };
    let raw_base_url = proxy_url
        .trim()
        .trim_end_matches('/')
        .trim_end_matches("/v1")
        .trim_end_matches('/');

    // 1. 查找并清理已经存在的相同提供商 URL 的账号 (以 anthropic 协议最兼容接入)
    let mut accounts_to_remove = Vec::new();
    if let Some(accounts_table) = doc.get("provider_accounts").and_then(|i| i.as_table()) {
        for (acc_name, acc_item) in accounts_table.iter() {
            if acc_name == target_provider_id {
                continue;
            }
            if let Some(b_url) = acc_item.get("base_url").and_then(|v| v.as_str()) {
                let norm = b_url
                    .trim_end_matches('/')
                    .trim_end_matches("/v1")
                    .trim_end_matches('/');
                if norm == raw_base_url {
                    accounts_to_remove.push(acc_name.to_string());
                }
            }
        }
    }

    // 从 provider_accounts 移除旧同 URL 账号
    if let Some(accounts_table) = doc
        .get_mut("provider_accounts")
        .and_then(|i| i.as_table_mut())
    {
        for acc in &accounts_to_remove {
            accounts_table.remove(acc);
        }
    }

    // 检查旧版 providers 表
    if let Some(providers_table) = doc.get_mut("providers").and_then(|i| i.as_table_mut()) {
        let mut old_p_remove = Vec::new();
        for (p_name, p_item) in providers_table.iter() {
            if let Some(b_url) = p_item.get("base_url").and_then(|v| v.as_str()) {
                let norm = b_url
                    .trim_end_matches('/')
                    .trim_end_matches("/v1")
                    .trim_end_matches('/');
                if norm == raw_base_url {
                    old_p_remove.push(p_name.to_string());
                }
            }
        }
        for p in old_p_remove {
            providers_table.remove(&p);
        }
    }

    // 2. 清理属于被删除账号的模型，以及之前属于 antigravity-manager 的旧模型（以便完全重新生成）
    if let Some(models_table) = doc.get_mut("models").and_then(|i| i.as_table_mut()) {
        let mut models_to_remove = Vec::new();
        for (m_key, m_item) in models_table.iter() {
            if let Some(acc) = m_item.get("account").and_then(|v| v.as_str()) {
                if accounts_to_remove.iter().any(|a| a == acc) || acc == target_provider_id {
                    models_to_remove.push(m_key.to_string());
                }
            }
        }
        for m in models_to_remove {
            models_table.remove(&m);
        }
    }

    // 3. 添加 / 更新 [provider_accounts.antigravity-manager]，以 anthropic 协议接入
    let accounts = doc
        .entry("provider_accounts")
        .or_insert(Item::Table(Table::new()));
    if let Some(acc_table) = accounts.as_table_mut() {
        let ag = acc_table
            .entry(target_provider_id)
            .or_insert(Item::Table(Table::new()));
        if let Some(ag_table) = ag.as_table_mut() {
            ag_table.insert("provider", value("anthropic"));
            ag_table.insert("base_url", value(&normalized_proxy_url));
            ag_table.insert("api_key", value(api_key));
        }
    }

    // 4. 添加最核心的所有 Claude 和 Gemini 模型
    let core_models = get_core_gateway_models();

    let models_entry = doc.entry("models").or_insert(Item::Table(Table::new()));
    if let Some(m_table) = models_entry.as_table_mut() {
        for m in core_models {
            let mut model_table = Table::new();
            model_table.insert("account", value(target_provider_id));
            model_table.insert("model", value(m.id));
            model_table.insert("context_window", value(m.context_window));
            model_table.insert("image_input", value(true));
            model_table.insert("reasoning_model", value(true));
            model_table.insert("reasoning_history", value("exclude"));
            model_table.insert("reasoning_effort", value("high"));

            let mut levels = Array::new();
            levels.push("low");
            levels.push("medium");
            levels.push("high");
            model_table.insert(
                "reasoning_levels",
                Item::Value(toml_edit::Value::Array(levels)),
            );

            m_table.insert(m.id, Item::Table(model_table));
        }
    }

    // 5. 设置默认模型
    let existing_default_model = doc
        .get("default_model")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let chosen_model = match model {
        Some(m) if !m.is_empty() => m.to_string(),
        _ => {
            if let Some(existing) = existing_default_model {
                if core_models.iter().any(|cm| cm.id == existing) {
                    existing
                } else {
                    "gemini-3.8-flash-high".to_string()
                }
            } else {
                "gemini-3.8-flash-high".to_string()
            }
        }
    };
    doc.insert("default_model", value(&chosen_model));
    doc.insert("default_provider", value(&chosen_model));

    Ok(doc.to_string())
}

/// 同步 DeepSeek Harness 的 .credentials.yaml 密钥文件
fn sync_dsh_credentials_content(content: &str, api_key: &str) -> String {
    if content.trim().is_empty() {
        return format!("version: 1\nrefs:\n  ANTIGRAVITY_API_KEY: {}\n", api_key);
    }

    let re_key = regex::Regex::new(r#"(?m)^(\s*ANTIGRAVITY_API_KEY:\s*).*$"#).unwrap();
    if re_key.is_match(content) {
        return re_key
            .replace(content, regex::NoExpand(&format!("${{1}}{}", api_key)))
            .to_string();
    }

    let re_inline_refs = regex::Regex::new(r#"(?s)(refs:\s*\{[\s\S]*?)(\})"#).unwrap();
    if let Some(caps) = re_inline_refs.captures(content) {
        let p1 = caps.get(1).unwrap().as_str().trim_end();
        let needs_comma = !p1.ends_with('{') && !p1.ends_with(',');
        let comma = if needs_comma { "," } else { "" };
        let replacement = format!(
            "{}{}\n    ANTIGRAVITY_API_KEY: {}\n  }}",
            p1, comma, api_key
        );
        return re_inline_refs
            .replace(content, regex::NoExpand(&replacement))
            .to_string();
    }

    let re_block_refs = regex::Regex::new(r#"(?m)^(refs:\s*)$"#).unwrap();
    if re_block_refs.is_match(content) {
        return re_block_refs
            .replace(
                content,
                regex::NoExpand(&format!("refs:\n  ANTIGRAVITY_API_KEY: {}", api_key)),
            )
            .to_string();
    }

    format!(
        "{}\nrefs:\n  ANTIGRAVITY_API_KEY: {}\n",
        content.trim_end(),
        api_key
    )
}

fn build_dsh_models_yaml(selected_model: &str) -> String {
    let core_models = get_core_gateway_models();
    let mut out = String::new();

    if !core_models.iter().any(|m| m.id == selected_model) {
        out.push_str(&format!(
            "          - id: {}\n            name: {}\n            contextWindow: 1048576\n            input:\n              - text\n              - image\n            maxTokens: 128000\n",
            selected_model, selected_model
        ));
    }

    for m in core_models {
        out.push_str(&format!(
            "          - id: {}\n            name: {}\n            contextWindow: {}\n            input:\n              - text\n              - image\n            maxTokens: 128000\n",
            m.id, m.id, m.context_window
        ));
        let has_reasoning = m.id.contains("high")
            || m.id.contains("thinking")
            || m.id.contains("medium")
            || m.id.contains("low")
            || m.id.starts_with("gemini");
        if has_reasoning {
            out.push_str("            reasoningEfforts:\n              off: null\n              low: low\n              medium: medium\n              high: high\n");
        }
    }
    out
}

fn build_dsh_provider_block(proxy_url: &str, selected_model: &str) -> String {
    let models_yaml = build_dsh_models_yaml(selected_model);
    format!(
        "      antigravity:\n        displayName: antigravity\n        apiKeyEnv: ANTIGRAVITY_API_KEY\n        api: openai-completions\n        baseURL: {}\n        models:\n{}",
        proxy_url, models_yaml
    )
}

struct DshPatchEntry {
    id: String,
    lines: Vec<String>,
}

fn parse_dsh_patch_entries(content: &str) -> (Vec<String>, Vec<DshPatchEntry>) {
    let mut header = Vec::new();
    let mut entries = Vec::new();
    let mut current_id: Option<String> = None;
    let mut current_lines: Vec<String> = Vec::new();
    let mut in_header = true;

    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("- id:") {
            in_header = false;
            if let Some(id) = current_id.take() {
                entries.push(DshPatchEntry {
                    id,
                    lines: current_lines.clone(),
                });
                current_lines.clear();
            }
            let id_val = trimmed["- id:".len()..]
                .trim()
                .trim_matches(|c| c == '\'' || c == '"');
            current_id = Some(id_val.to_string());
        }

        if in_header {
            header.push(line.to_string());
        } else {
            current_lines.push(line.to_string());
        }
    }

    if let Some(id) = current_id {
        entries.push(DshPatchEntry {
            id,
            lines: current_lines,
        });
    }

    (header, entries)
}

/// 同步 DeepSeek Harness 的 cordis.patch.yml 配置（同时适用于 Desktop/CLI 与 Web GUI Profile）
fn sync_dsh_cordis_patch_content(content: &str, proxy_url: &str, selected_model: &str) -> String {
    let default_model_block = format!(
        "- id: agent-default-model\n  name: \"@deepseek-ai/dsh-agent-default-model\"\n  config:\n    provider: antigravity\n    model: {}\n",
        selected_model
    );
    let provider_block = build_dsh_provider_block(proxy_url, selected_model);

    let trimmed = content.trim();
    if trimmed.is_empty() || trimmed == "[]" || !trimmed.contains("- id:") {
        let comment = "# Your patch layer for this dsh profile, applied after every bundle layer:\n# a top-level YAML array of loader patch entries (id-targeted config\n# overrides, disables, and insert lists; `!!js` expressions allowed).\n";
        return format!(
            "{}{}- id: llm-pi-ai\n  name: \"@deepseek-ai/dsh-llm-pi-ai\"\n  config:\n    providers:\n{}\n",
            comment, default_model_block, provider_block
        );
    }

    let (header, mut entries) = parse_dsh_patch_entries(content);

    // 1. 更新或插入 agent-default-model
    if let Some(entry) = entries.iter_mut().find(|e| e.id == "agent-default-model") {
        let mut new_lines = Vec::new();
        let mut has_config = false;
        let mut has_provider = false;
        let mut has_model = false;

        for line in &entry.lines {
            let t = line.trim();
            if t.starts_with("config:") {
                has_config = true;
                new_lines.push(line.clone());
            } else if t.starts_with("provider:") {
                has_provider = true;
                let indent = line.len() - line.trim_start().len();
                new_lines.push(format!(
                    "{:indent$}provider: antigravity",
                    "",
                    indent = indent
                ));
            } else if t.starts_with("model:") {
                has_model = true;
                let indent = line.len() - line.trim_start().len();
                new_lines.push(format!(
                    "{:indent$}model: {}",
                    "",
                    selected_model,
                    indent = indent
                ));
            } else {
                new_lines.push(line.clone());
            }
        }

        if !has_config {
            new_lines.push("  config:".to_string());
        }
        if !has_provider {
            new_lines.push("    provider: antigravity".to_string());
        }
        if !has_model {
            new_lines.push(format!("    model: {}", selected_model));
        }

        entry.lines = new_lines;
    } else {
        entries.insert(
            0,
            DshPatchEntry {
                id: "agent-default-model".to_string(),
                lines: default_model_block.lines().map(|s| s.to_string()).collect(),
            },
        );
    }

    // 2. 更新或插入 llm-pi-ai
    if let Some(entry) = entries.iter_mut().find(|e| e.id == "llm-pi-ai") {
        let mut new_lines = Vec::new();
        let mut in_providers = false;
        let mut in_antigravity = false;
        let mut provider_replaced = false;
        let mut has_providers_header = false;

        let provider_block_lines: Vec<String> =
            provider_block.lines().map(|s| s.to_string()).collect();

        for line in &entry.lines {
            let trimmed = line.trim_start();
            let indent = line.len() - trimmed.len();

            if trimmed.starts_with("providers:") {
                in_providers = true;
                has_providers_header = true;
                new_lines.push(line.clone());
                continue;
            }

            if in_providers {
                if indent == 6 && trimmed.ends_with(':') {
                    if trimmed.starts_with("antigravity:") {
                        in_antigravity = true;
                        if !provider_replaced {
                            new_lines.extend(provider_block_lines.clone());
                            provider_replaced = true;
                        }
                        continue;
                    } else {
                        in_antigravity = false;
                    }
                } else if indent < 6 && !trimmed.is_empty() {
                    in_providers = false;
                    in_antigravity = false;
                }

                if in_antigravity {
                    continue;
                }
            }

            new_lines.push(line.clone());
        }

        if !has_providers_header {
            if !new_lines.iter().any(|l| l.trim().starts_with("config:")) {
                new_lines.push("  config:".to_string());
            }
            new_lines.push("    providers:".to_string());
            new_lines.extend(provider_block_lines);
        } else if !provider_replaced {
            let mut final_lines = Vec::new();
            for l in new_lines {
                let is_prov = l.trim().starts_with("providers:");
                final_lines.push(l);
                if is_prov {
                    final_lines.extend(provider_block_lines.clone());
                }
            }
            new_lines = final_lines;
        }

        entry.lines = new_lines;
    } else {
        let new_llm_block = format!(
            "- id: llm-pi-ai\n  name: \"@deepseek-ai/dsh-llm-pi-ai\"\n  config:\n    providers:\n{}\n",
            provider_block
        );
        entries.push(DshPatchEntry {
            id: "llm-pi-ai".to_string(),
            lines: new_llm_block.lines().map(|s| s.to_string()).collect(),
        });
    }

    // 3. 组装输出
    let mut out = String::new();
    if !header.is_empty() {
        for l in header {
            out.push_str(&l);
            out.push('\n');
        }
    }
    for e in entries {
        for l in e.lines {
            out.push_str(&l);
            out.push('\n');
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_jeikcode_toml_content() {
        let initial_toml = r#"default_model = "old-model"
default_provider = "old-model"
language = "zh-CN"

[provider_accounts.local-gemini]
api_key = "sk-old1"
base_url = "http://127.0.0.1:8046/v1"
provider = "openai"

[provider_accounts."local_antigravity测试"]
api_key = "sk-old2"
base_url = "http://127.0.0.1:8046"
provider = "anthropic"

[provider_accounts.other-provider]
api_key = "sk-other"
base_url = "https://other.com/v1"
provider = "openai"

[models."gemini-3.7"]
account = "local-gemini"
model = "gemini-3.7-flash-high"
reasoning_model = false

[models."other-model"]
account = "other-provider"
model = "deepseek-chat"
"#;

        let res = sync_jeikcode_toml_content(
            initial_toml,
            "http://127.0.0.1:8046",
            "sk-newkey123",
            Some("gemini-3.8-flash-high"),
        )
        .expect("sync should succeed");

        use toml_edit::DocumentMut;
        let doc: DocumentMut = res.parse().expect("result should be valid toml");

        // 验证 antigravity-manager 存在且正确
        let accounts = doc.get("provider_accounts").unwrap().as_table().unwrap();
        let ag = accounts.get("antigravity-manager").unwrap();
        assert_eq!(ag.get("provider").unwrap().as_str().unwrap(), "anthropic");
        assert_eq!(
            ag.get("base_url").unwrap().as_str().unwrap(),
            "http://127.0.0.1:8046/v1"
        );
        assert_eq!(ag.get("api_key").unwrap().as_str().unwrap(), "sk-newkey123");

        // 验证同 URL 旧账号被清除
        assert!(!accounts.contains_key("local-gemini"));
        assert!(!accounts.contains_key("local_antigravity测试"));
        // 验证不同 URL 账号被保留
        assert!(accounts.contains_key("other-provider"));

        // 验证核心模型已添加
        let models = doc.get("models").unwrap().as_table().unwrap();
        assert!(models.contains_key("claude-opus-5-5"));
        assert!(models.contains_key("claude-opus-5-5-high"));
        assert!(models.contains_key("claude-sonnet-5-5"));
        assert!(models.contains_key("claude-sonnet-5-5-high"));
        assert!(models.contains_key("claude-sonnet-4-6"));
        assert!(models.contains_key("claude-opus-4-6"));
        assert!(models.contains_key("gemini-4-argon"));
        assert!(models.contains_key("gemini-4-argon-high"));
        assert!(models.contains_key("gemini-3.8-flash"));
        assert!(models.contains_key("gemini-3.8-flash-tiered"));
        assert!(models.contains_key("gemini-3.8-flash-high"));
        assert!(models.contains_key("gemini-3.1-pro"));

        // 验证旧关联模型已被清除，且保留其他账号模型
        assert!(!models.contains_key("gemini-3.7"));
        assert!(models.contains_key("other-model"));

        // 验证模型属性：开启思考模式和思考不回传，以及上下文大小（Gemini 1M，Claude 256k）
        let m = models.get("gemini-3.8-flash-high").unwrap();
        assert_eq!(
            m.get("account").unwrap().as_str().unwrap(),
            "antigravity-manager"
        );
        assert_eq!(
            m.get("context_window").unwrap().as_integer().unwrap(),
            1_048_576
        );
        assert_eq!(m.get("reasoning_model").unwrap().as_bool().unwrap(), true);
        assert_eq!(
            m.get("reasoning_history").unwrap().as_str().unwrap(),
            "exclude"
        );
        assert_eq!(m.get("image_input").unwrap().as_bool().unwrap(), true);

        let m_claude = models.get("claude-sonnet-4-6").unwrap();
        assert_eq!(
            m_claude.get("account").unwrap().as_str().unwrap(),
            "antigravity-manager"
        );
        assert_eq!(
            m_claude
                .get("context_window")
                .unwrap()
                .as_integer()
                .unwrap(),
            256_000
        );
        assert_eq!(
            m_claude.get("reasoning_model").unwrap().as_bool().unwrap(),
            true
        );
        assert_eq!(
            m_claude.get("reasoning_history").unwrap().as_str().unwrap(),
            "exclude"
        );

        // 验证默认模型
        assert_eq!(
            doc.get("default_model").unwrap().as_str().unwrap(),
            "gemini-3.8-flash-high"
        );
    }

    #[test]
    fn test_sync_dsh_credentials_content() {
        let empty = "";
        let res1 = sync_dsh_credentials_content(empty, "sk-test1");
        assert!(res1.contains("ANTIGRAVITY_API_KEY: sk-test1"));

        let existing = r#"version: 1
records:
  browser-session:
    kind: grant
refs:
  {
    KIMI_KEY: sk-kimi,
    ANTIGRAVITY_API_KEY: sk-old
  }
"#;
        let res2 = sync_dsh_credentials_content(existing, "sk-new");
        assert!(res2.contains("sk-new"), "新 key 必须写入: {res2}");
        assert!(res2.contains("sk-kimi"), "其他 key 必须保留: {res2}");
    }

    #[test]
    fn test_sync_dsh_cordis_patch_content() {
        let empty = "[]";
        let res1 = sync_dsh_cordis_patch_content(
            empty,
            "http://127.0.0.1:8045/v1",
            "gemini-3.8-flash-high",
        );
        assert!(res1.contains("provider: antigravity"));
        assert!(res1.contains("model: gemini-3.8-flash-high"));
        assert!(res1.contains("baseURL: http://127.0.0.1:8045/v1"));
        assert!(res1.contains("apiKeyEnv: ANTIGRAVITY_API_KEY"));

        let existing = r#"- id: ui-settings
  config:
    enabled: true
- id: agent-default-model
  config:
    provider: deepseek-official
    model: deepseek-chat
- id: llm-pi-ai
  config:
    providers:
      kimi-k3:
        displayName: kimi-k3
        baseURL: https://hubway.cc
"#;
        let res2 = sync_dsh_cordis_patch_content(
            existing,
            "http://127.0.0.1:8045/v1",
            "claude-sonnet-4-6",
        );
        assert!(res2.contains("provider: antigravity"));
        assert!(res2.contains("model: claude-sonnet-4-6"));
        assert!(res2.contains("baseURL: http://127.0.0.1:8045/v1"));
        assert!(res2.contains("kimi-k3:"));
        assert!(res2.contains("ui-settings"));

        // 二次同步覆盖（更新模型与代理端口）
        let res3 =
            sync_dsh_cordis_patch_content(&res2, "http://127.0.0.1:9000/v1", "gpt-oss-120b-medium");
        assert!(res3.contains("model: gpt-oss-120b-medium"));
        assert!(res3.contains("baseURL: http://127.0.0.1:9000/v1"));
        assert!(!res3.contains("baseURL: http://127.0.0.1:8045/v1"));
        assert!(res3.contains("kimi-k3:"));
    }
}
