use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;
use std::fs;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum CliApp {
    Claude,
    Codex,
    Gemini,
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
            CliApp::Codex => vec![
                CliConfigFile {
                    name: "auth.json".to_string(),
                    path: home.join(".codex").join("auth.json"),
                },
                CliConfigFile {
                    name: "config.toml".to_string(),
                    path: home.join(".codex").join("config.toml"),
                },
            ],
            CliApp::Gemini => vec![
                CliConfigFile {
                    name: ".env".to_string(),
                    path: home.join(".gemini").join(".env"),
                },
                CliConfigFile {
                    name: "settings.json".to_string(),
                    path: home.join(".gemini").join("settings.json"),
                },
                CliConfigFile {
                    name: "config.json".to_string(),
                    path: home.join(".gemini").join("config.json"),
                },
            ],
        }
    }

    pub fn default_url(&self) -> &'static str {
        match self {
            CliApp::Claude => "https://api.anthropic.com",
            CliApp::Codex => "https://api.openai.com/v1",
            CliApp::Gemini => "https://generativelanguage.googleapis.com",
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
    pub files: Vec<String>, // 返回关联的文件名列表供前端展示
}

/// 检测 CLI 是否安装并获取版本
pub fn check_cli_installed(app: &CliApp) -> (bool, Option<String>) {
    let cmd = app.as_str();
    // 默认使用命令名，如果 fallback 找到路径则更新为绝对路径
    let mut executable_path = PathBuf::from(cmd);
    
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

    let mut installed = match which_output {
        Ok(out) => out.status.success(),
        Err(_) => false,
    };

    // [FIX #765] macOS 增强检测: 如果 which 失败,显式搜索常用二进制路径
    // 解决 Tauri 进程 PATH 可能不完整导致检测不到已安装 CLI 的问题
    if !installed && !cfg!(target_os = "windows") {
        let home = dirs::home_dir().unwrap_or_default();
        let mut common_paths = vec![
            home.join(".local/bin"),
            home.join(".npm-global/bin"),
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

        for path in common_paths {
            let full_path = path.join(cmd);
            if full_path.exists() {
                tracing::debug!("[CLI-Sync] Detected {} via explicit path: {:?}", cmd, full_path);
                installed = true;
                executable_path = full_path;
                break;
            }
        }
    }

    if !installed {
        return (false, None);
    }

    // 2. 获取版本
    let mut ver_cmd = Command::new(&executable_path);
    ver_cmd.arg("--version");
    #[cfg(target_os = "windows")]
    ver_cmd.creation_flags(CREATE_NO_WINDOW);

    let version_output = ver_cmd.output();
    let version = match version_output {
        Ok(out) if out.status.success() => {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            // 优化：提取最末尾的数字版本号
            // 例如 "claude/2.1.2 (Claude Code)" -> "2.1.2"
            // 例如 "codex-cli 0.86.0" -> "0.86.0"
            let cleaned = s.split(|c: char| !c.is_numeric() && c != '.')
                .filter(|part| !part.is_empty())
                .last()
                .map(|p| p.trim())
                .unwrap_or(&s)
                .to_string();
            Some(cleaned)
        }
        _ => None,
    };

    (true, version)
}

/// 读取当前配置并检测同步状态
pub fn get_sync_status(app: &CliApp, proxy_url: &str) -> (bool, bool, Option<String>) {
    let files = app.config_files();
    if files.is_empty() {
        return (false, false, None);
    }

    let mut all_synced = true;
    let mut has_backup = false;
    let mut current_base_url = None;

    for file in &files {
        // 使用更简单的命名规则: original_name + .antigravity.bak
        let backup_path = file.path.with_file_name(format!("{}.antigravity.bak", file.name));
        
        if backup_path.exists() {
            has_backup = true;
        }

        // 如果物理文件不存在
        // 如果物理文件不存在
        if !file.path.exists() {
            // Gemini 的 settings.json/config.json 只要有一个存在即可，或者都不存在（视为未同步）
            if app == &CliApp::Gemini && (file.name == "settings.json" || file.name == "config.json") {
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
                    let url = json.get("env").and_then(|e| e.get("ANTHROPIC_BASE_URL")).and_then(|v| v.as_str());
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
                    let re = regex::Regex::new(r#"(?m)^\s*base_url\s*=\s*['"]([^'"]+)['"]"#).unwrap();
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
                }
            }
        }
    }

    (all_synced, has_backup, current_base_url)
}

/// 执行同步逻辑
pub fn sync_config(app: &CliApp, proxy_url: &str, api_key: &str) -> Result<(), String> {
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
            let backup_path = file.path.with_file_name(format!("{}.antigravity.bak", file.name));
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
                    let mut json: Value = serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}));
                    if let Some(obj) = json.as_object_mut() {
                        obj.insert("hasCompletedOnboarding".to_string(), Value::Bool(true));
                    }
                    content = serde_json::to_string_pretty(&json).unwrap();
                } else if file.name == "settings.json" {
                    let mut json: serde_json::Value = serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}));
                    if json.as_object().is_none() { json = serde_json::json!({}); }
                    let env = json.as_object_mut().unwrap().entry("env").or_insert(serde_json::json!({}));
                    if let Some(env_obj) = env.as_object_mut() {
                        env_obj.insert("ANTHROPIC_BASE_URL".to_string(), Value::String(proxy_url.to_string()));
                        
                        if !api_key.is_empty() {
                            env_obj.insert("ANTHROPIC_API_KEY".to_string(), Value::String(api_key.to_string()));
                            // [FIX] Avoid conflict: remove ANTHROPIC_AUTH_TOKEN if it exists
                            env_obj.remove("ANTHROPIC_AUTH_TOKEN");
                            
                            // [FIX] Cleanup potential model overrides from other providers (e.g. MiniMax)
                            // This ensures Claude CLI uses its default models or ones compatible with our proxy
                            env_obj.remove("ANTHROPIC_MODEL");
                            env_obj.remove("ANTHROPIC_DEFAULT_HAIKU_MODEL");
                            env_obj.remove("ANTHROPIC_DEFAULT_OPUS_MODEL");
                            env_obj.remove("ANTHROPIC_DEFAULT_SONNET_MODEL");
                        } else {
                            // If API Key is empty (e.g. restore/reset), remove it to avoid empty string value
                            env_obj.remove("ANTHROPIC_API_KEY");
                        }
                    }
                    content = serde_json::to_string_pretty(&json).unwrap();
                }
            }
            CliApp::Codex => {
                if file.name == "auth.json" {
                    let mut json: Value = serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}));
                    if let Some(obj) = json.as_object_mut() {
                        obj.insert("OPENAI_API_KEY".to_string(), Value::String(api_key.to_string()));
                        // Codex 的 auth.json 似乎也支持 OPENAI_BASE_URL，但 ccs 没写，我们也同步写一下
                        obj.insert("OPENAI_BASE_URL".to_string(), Value::String(proxy_url.to_string()));
                    }
                    content = serde_json::to_string_pretty(&json).unwrap();
                } else if file.name == "config.toml" {
                    use toml_edit::{DocumentMut, value};
                    let mut doc = content.parse::<DocumentMut>().unwrap_or_else(|_| DocumentMut::new());
                    
                    // 设置层级 [model_providers.custom]
                    let providers = doc.entry("model_providers").or_insert(toml_edit::Item::Table(toml_edit::Table::new()));
                    if let Some(p_table) = providers.as_table_mut() {
                        let custom = p_table.entry("custom").or_insert(toml_edit::Item::Table(toml_edit::Table::new()));
                        if let Some(c_table) = custom.as_table_mut() {
                            c_table.insert("name", value("custom"));
                            c_table.insert("wire_api", value("responses"));
                            c_table.insert("requires_openai_auth", value(true));
                            c_table.insert("base_url", value(proxy_url));
                        }
                    }
                    doc.insert("model_provider", value("custom"));
                    content = doc.to_string();
                }
            }
            CliApp::Gemini => {
                if file.name == ".env" {
                    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
                    let mut found_url = false;
                    let mut found_key = false;
                    for line in lines.iter_mut() {
                        if line.starts_with("GOOGLE_GEMINI_BASE_URL=") {
                            *line = format!("GOOGLE_GEMINI_BASE_URL={}", proxy_url);
                            found_url = true;
                        } else if line.trim().starts_with("GEMINI_API_KEY=") {
                            *line = format!("GEMINI_API_KEY={}", api_key);
                            found_key = true;
                        }
                    }
                    if !found_url { lines.push(format!("GOOGLE_GEMINI_BASE_URL={}", proxy_url)); }
                    if !found_key { lines.push(format!("GEMINI_API_KEY={}", api_key)); }
                    content = lines.join("\n");
                } else if file.name == "settings.json" || file.name == "config.json" {
                    let mut json: Value = serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}));
                    if json.as_object().is_none() { json = serde_json::json!({}); }
                    let sec = json.as_object_mut().unwrap().entry("security").or_insert(serde_json::json!({}));
                    let auth = sec.as_object_mut().unwrap().entry("auth").or_insert(serde_json::json!({}));
                    if let Some(auth_obj) = auth.as_object_mut() {
                        auth_obj.insert("selectedType".to_string(), Value::String("gemini-api-key".to_string()));
                    }
                    content = serde_json::to_string_pretty(&json).unwrap();
                }
            }
        }

        // 使用临时文件原子写入
        let tmp_path = file.path.with_extension("tmp");
        fs::write(&tmp_path, &content).map_err(|e| format!("写入临时文件失败: {}", e))?;
        fs::rename(&tmp_path, &file.path).map_err(|e| format!("重命名配置文件失败: {}", e))?;
    }

    Ok(())
}

// Tauri Commands

#[tauri::command]
pub async fn get_cli_sync_status(app_type: CliApp, proxy_url: String) -> Result<CliStatus, String> {
    let (installed, version) = check_cli_installed(&app_type);
    let (is_synced, has_backup, current_base_url) = if installed {
        get_sync_status(&app_type, &proxy_url)
    } else {
        (false, false, None)
    };

    Ok(CliStatus {
        installed,
        version,
        is_synced,
        has_backup,
        current_base_url,
        files: app_type.config_files().into_iter().map(|f| f.name).collect(),
    })
}

#[tauri::command]
pub async fn execute_cli_sync(app_type: CliApp, proxy_url: String, api_key: String) -> Result<(), String> {
    sync_config(&app_type, &proxy_url, &api_key)
}

#[tauri::command]
pub async fn execute_cli_restore(app_type: CliApp) -> Result<(), String> {
    let files = app_type.config_files();
    let mut restored_count = 0;

    // 尝试从备份恢复
    for file in &files {
        let backup_path = file.path.with_file_name(format!("{}.antigravity.bak", file.name));
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
    sync_config(&app_type, default_url, "")
}

#[tauri::command]
pub async fn get_cli_config_content(app_type: CliApp, file_name: Option<String>) -> Result<String, String> {
    let files = app_type.config_files();
    let file = if let Some(name) = file_name {
        files.into_iter().find(|f| f.name == name).ok_or("找不到指定的文件".to_string())?
    } else {
        files.into_iter().next().ok_or("找不到配置文件".to_string())?
    };

    if !file.path.exists() {
        return Err("配置文件不存在".to_string());
    }
    fs::read_to_string(&file.path).map_err(|e| format!("读取配置文件失败: {}", e))
}
