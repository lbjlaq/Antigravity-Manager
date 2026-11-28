use crate::AntigravityManager;
use anyhow::{anyhow, Result};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

/// `agy` CLI 配置文件路径：`~/.gemini/antigravity-cli/settings.json`
pub fn agy_settings_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    Some(
        home.join(".gemini")
            .join("antigravity-cli")
            .join("settings.json"),
    )
}

/// 备份用户原有 `statusLine` 的路径：`~/.antigravity_tools/agy_statusline_backup.json`
pub fn statusline_backup_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    Some(
        home.join(".antigravity_tools")
            .join("agy_statusline_backup.json"),
    )
}

/// 解析本机可用的 `am` / `am-cli` 可执行路径
pub fn resolve_am_cli_command(override_bin: Option<&str>) -> String {
    if let Some(bin) = override_bin.map(str::trim).filter(|s| !s.is_empty()) {
        return format_statusline_command(bin);
    }

    // 1. 检查当前可执行文件本身或同目录下的 am / am-cli sidecar
    if let Ok(exe) = std::env::current_exe() {
        let fname = exe
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if fname == "am" || fname == "am.exe" || fname == "am-cli" || fname == "am-cli.exe" {
            return format_statusline_command(&exe.to_string_lossy());
        }
        if let Some(dir) = exe.parent() {
            for candidate in ["am", "am.exe", "am-cli", "am-cli.exe"] {
                let p = dir.join(candidate);
                if p.exists() {
                    return format_statusline_command(&p.to_string_lossy());
                }
            }
        }
    }

    // 2. 常见安装路径
    if let Some(home) = dirs::home_dir() {
        let mut candidates = vec![
            home.join(".local/bin/am"),
            home.join(".local/bin/am-cli"),
            home.join(".cargo/bin/am"),
        ];
        #[cfg(target_os = "macos")]
        {
            candidates.push(PathBuf::from("/opt/homebrew/bin/am"));
            candidates.push(PathBuf::from("/usr/local/bin/am"));
        }
        #[cfg(target_os = "linux")]
        {
            candidates.push(PathBuf::from("/usr/local/bin/am"));
            candidates.push(PathBuf::from("/usr/bin/am"));
        }
        for p in candidates {
            if p.exists() {
                return format_statusline_command(&p.to_string_lossy());
            }
        }
    }

    format_statusline_command("am")
}

fn format_statusline_command(bin: &str) -> String {
    if bin.contains(' ') {
        format!("\"{}\" statusline", bin)
    } else {
        format!("{} statusline", bin)
    }
}

fn is_our_statusline_command(cmd: &str) -> bool {
    let trimmed = cmd.trim();
    trimmed.ends_with(" statusline")
        && (trimmed.contains("am ")
            || trimmed.contains("am\"")
            || trimmed.contains("am-cli")
            || trimmed.contains("am.exe"))
}

/// 检查 `agy` CLI 状态栏插件是否已安装
pub fn is_cli_statusline_installed() -> bool {
    let Some(path) = agy_settings_path() else {
        return false;
    };
    let Ok(raw) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(val) = serde_json::from_str::<Value>(&raw) else {
        return false;
    };
    let Some(sl) = val.get("statusLine").and_then(|v| v.as_object()) else {
        return false;
    };
    let enabled = sl.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
    let cmd = sl.get("command").and_then(|v| v.as_str()).unwrap_or("");
    enabled && is_our_statusline_command(cmd)
}

/// 安装 `agy` CLI 状态栏钩子到 `~/.gemini/antigravity-cli/settings.json`（自动备份原有配置）
pub fn install_cli_statusline(override_bin: Option<&str>) -> Result<PathBuf> {
    let settings_path =
        agy_settings_path().ok_or_else(|| anyhow!("Cannot resolve home directory"))?;
    if let Some(parent) = settings_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut obj: Map<String, Value> = if settings_path.exists() {
        let raw = std::fs::read_to_string(&settings_path).unwrap_or_default();
        serde_json::from_str(&raw).unwrap_or_default()
    } else {
        Map::new()
    };

    let command = resolve_am_cli_command(override_bin);

    // 若原有 statusLine 不是我们自己的，先做无损备份
    let existing_is_ours = obj
        .get("statusLine")
        .and_then(|v| v.get("command"))
        .and_then(|v| v.as_str())
        .map(is_our_statusline_command)
        .unwrap_or(false);

    if !existing_is_ours {
        if let Some(backup_path) = statusline_backup_path() {
            if let Some(parent) = backup_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let backup = json!({
                "hadStatusLine": obj.contains_key("statusLine"),
                "statusLine": obj.get("statusLine").cloned().unwrap_or(Value::Null),
                "installedCommand": command,
            });
            let _ = std::fs::write(
                backup_path,
                serde_json::to_string_pretty(&backup).unwrap_or_default(),
            );
        }
    }

    obj.insert(
        "statusLine".into(),
        json!({
            "type": "command",
            "command": command,
            "enabled": true,
            "stack_with_default": false,
        }),
    );

    let formatted = serde_json::to_string_pretty(&Value::Object(obj))?;
    std::fs::write(&settings_path, format!("{}\n", formatted))?;
    Ok(settings_path)
}

/// 卸载 `agy` CLI 状态栏钩子（如存在备份则无损恢复）
pub fn uninstall_cli_statusline() -> Result<bool> {
    let settings_path =
        agy_settings_path().ok_or_else(|| anyhow!("Cannot resolve home directory"))?;
    if !settings_path.exists() {
        return Ok(false);
    }

    let raw = std::fs::read_to_string(&settings_path)?;
    let mut obj: Map<String, Value> = serde_json::from_str(&raw).unwrap_or_default();

    let is_ours = obj
        .get("statusLine")
        .and_then(|v| v.get("command"))
        .and_then(|v| v.as_str())
        .map(is_our_statusline_command)
        .unwrap_or(false);

    if !is_ours {
        return Ok(false);
    }

    let mut restored = false;
    if let Some(backup_path) = statusline_backup_path() {
        if let Ok(backup_raw) = std::fs::read_to_string(&backup_path) {
            if let Ok(backup_val) = serde_json::from_str::<Value>(&backup_raw) {
                if backup_val
                    .get("hadStatusLine")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    if let Some(prev) = backup_val.get("statusLine") {
                        if !prev.is_null() {
                            obj.insert("statusLine".into(), prev.clone());
                            restored = true;
                        }
                    }
                }
            }
            let _ = std::fs::remove_file(&backup_path);
        }
    }

    if !restored {
        obj.remove("statusLine");
    }

    let formatted = serde_json::to_string_pretty(&Value::Object(obj))?;
    std::fs::write(&settings_path, format!("{}\n", formatted))?;
    Ok(true)
}

/// 渲染双行 ANSI 状态栏（供 `am statusline` 调用，零网络请求，毫秒级返回）
pub fn render_statusline(stdin_json: &str) -> String {
    let payload: Value = serde_json::from_str(stdin_json).unwrap_or(Value::Null);

    // 1. 工作区路径
    let cwd = payload
        .pointer("/workspace/current_dir")
        .or_else(|| payload.get("cwd"))
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));

    let short_dir = format_short_dir(&cwd);
    let git_branch = read_git_branch_fast(&cwd);

    // 2. 免 TUN 代理端口检测
    let proxy_port = detect_env_proxy_port();

    // 3. 模型名称
    let model = payload
        .pointer("/model/display_name")
        .or_else(|| payload.pointer("/model/id"))
        .or_else(|| payload.get("model"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Antigravity");

    // 4. 上下文水位、Token、缓存命中率、吐字速率与预估费用
    let ctx_pct = payload
        .pointer("/context_window/used_percentage")
        .or_else(|| payload.pointer("/context/used_percentage"))
        .and_then(|v| v.as_f64())
        .map(|v| v.clamp(0.0, 100.0));

    let ctx_size = payload
        .pointer("/context_window/context_window_size")
        .and_then(|v| v.as_u64());

    let total_in = payload
        .pointer("/context_window/total_input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let total_out = payload
        .pointer("/context_window/total_output_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let cur_in = payload
        .pointer("/context_window/current_usage/input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let cur_cache_read = payload
        .pointer("/context_window/current_usage/cache_read_input_tokens")
        .or_else(|| payload.pointer("/context_window/total_cache_read_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let cache_hit_pct = if cur_cache_read > 0 && (cur_in + cur_cache_read) > 0 {
        Some((cur_cache_read as f64 / (cur_in + cur_cache_read) as f64) * 100.0)
    } else {
        None
    };

    let api_ms = payload
        .pointer("/cost/total_api_duration_ms")
        .and_then(|v| v.as_f64())
        .filter(|&ms| ms > 0.0);
    let tok_per_sec = match (total_out, api_ms) {
        (out, Some(ms)) if out > 0 => Some(out as f64 / (ms / 1000.0)),
        _ => None,
    };

    let cost_usd = payload
        .pointer("/cost/total_cost_usd")
        .and_then(|v| v.as_f64())
        .filter(|&c| c > 0.0)
        .or_else(|| {
            if total_in + total_out + cur_cache_read > 0 {
                let est = (total_in as f64 * 1.25
                    + cur_cache_read as f64 * 0.125
                    + total_out as f64 * 10.0)
                    / 1_000_000.0;
                (est >= 0.001).then_some(est)
            } else {
                None
            }
        });

    // 5. 本地 am-core 账号与额度缓存（零网络调用）
    let (active_email, claude_pct, gemini_pct) = read_local_account_quota();

    // 第一行：目录 (分支) · PX 端口 │ 模型 │ 速率 · 费用
    let mut line1 = format!("\x1b[38;5;75m{}\x1b[0m", short_dir);
    if let Some(branch) = git_branch {
        line1.push_str(&format!(" \x1b[38;5;141m({})\x1b[0m", branch));
    }
    if let Some(port) = proxy_port {
        line1.push_str(&format!(
            " \x1b[38;5;242m·\x1b[0m \x1b[38;5;114mPX {}\x1b[0m",
            port
        ));
    }
    line1.push_str(&format!(
        " \x1b[38;5;240m│\x1b[0m \x1b[1;38;5;252m{}\x1b[0m",
        model
    ));

    let mut perf_parts: Vec<String> = Vec::new();
    if let Some(tps) = tok_per_sec {
        perf_parts.push(format!("\x1b[38;5;117m{:.0}t/s\x1b[0m", tps));
    }
    if let Some(cost) = cost_usd {
        perf_parts.push(format!("\x1b[38;5;221m~${:.2}\x1b[0m", cost));
    }
    if !perf_parts.is_empty() {
        line1.push_str(&format!(
            " \x1b[38;5;240m│\x1b[0m {}",
            perf_parts.join(" \x1b[38;5;242m·\x1b[0m ")
        ));
    }

    // 第二行：CTX 进度条 │ CACHE 命中率 │ Gemini / Claude 额度 │ 当前账号
    let mut line2_parts: Vec<String> = Vec::new();
    if let Some(pct) = ctx_pct {
        let bar = render_bar(pct, 8, true);
        let tok_hint = ctx_size
            .map(|sz| {
                format!(
                    " ({})",
                    format_compact_tokens(((sz as f64) * pct / 100.0) as u64)
                )
            })
            .unwrap_or_default();
        let warn = if pct >= 85.0 {
            " \x1b[38;5;203m→/compact\x1b[0m"
        } else {
            ""
        };
        line2_parts.push(format!("CTX {} {:.0}%{}{}", bar, pct, tok_hint, warn));
    }
    if let Some(hit) = cache_hit_pct {
        line2_parts.push(format!("CACHE \x1b[38;5;114m{:.0}%\x1b[0m", hit));
    }
    if let Some(pct) = gemini_pct {
        let bar = render_bar(pct as f64, 6, false);
        line2_parts.push(format!("Gemini {} {}%", bar, pct));
    }
    if let Some(pct) = claude_pct {
        let bar = render_bar(pct as f64, 6, false);
        line2_parts.push(format!("Claude {} {}%", bar, pct));
    }
    if let Some(email) = active_email {
        line2_parts.push(format!("\x1b[38;5;245m{}\x1b[0m", email));
    }

    if line2_parts.is_empty() {
        line1
    } else {
        format!(
            "{}\n{}",
            line1,
            line2_parts.join(" \x1b[38;5;240m│\x1b[0m ")
        )
    }
}

fn format_compact_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.0}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn format_short_dir(path: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(rel) = path.strip_prefix(&home) {
            let s = rel.to_string_lossy();
            return if s.is_empty() {
                "~".to_string()
            } else {
                format!("~/{}", s)
            };
        }
    }
    path.to_string_lossy().to_string()
}

/// 直接读取 `.git/HEAD`（零子进程开销）
fn read_git_branch_fast(start: &Path) -> Option<String> {
    let mut cur = Some(start);
    while let Some(dir) = cur {
        let head = dir.join(".git").join("HEAD");
        if head.is_file() {
            let content = std::fs::read_to_string(head).ok()?;
            let trimmed = content.trim();
            if let Some(branch) = trimmed.strip_prefix("ref: refs/heads/") {
                return Some(branch.to_string());
            }
            if trimmed.len() >= 7 {
                return Some(trimmed[..7].to_string());
            }
            return None;
        }
        cur = dir.parent();
    }
    None
}

fn detect_env_proxy_port() -> Option<u16> {
    for key in [
        "HTTPS_PROXY",
        "HTTP_PROXY",
        "ALL_PROXY",
        "https_proxy",
        "http_proxy",
    ] {
        if let Ok(val) = std::env::var(key) {
            if let Ok(node) = am_proxy::ProxyNode::parse_url(&val, None) {
                return Some(node.port);
            }
        }
    }
    None
}

fn read_local_account_quota() -> (Option<String>, Option<i32>, Option<i32>) {
    let Ok(mgr) = AntigravityManager::new() else {
        return (None, None, None);
    };
    let active_id = mgr
        .storage
        .get_active_account_ids()
        .ok()
        .and_then(|(app, ide, _acp, agy)| agy.or(app).or(ide));
    let account_id = active_id.or_else(|| {
        mgr.list_accounts()
            .ok()
            .and_then(|list| list.into_iter().next().map(|a| a.id))
    });

    let Some(id) = account_id else {
        return (None, None, None);
    };
    let Ok(Some(acc)) = mgr.get_account(&id) else {
        return (None, None, None);
    };

    let mut claude_pct: Option<i32> = None;
    let mut gemini_pct: Option<i32> = None;
    if let Some(q) = &acc.quota {
        for m in &q.models {
            let lower = m.name.to_ascii_lowercase();
            let pct = m.percentage.round() as i32;
            if lower.contains("claude") {
                claude_pct = Some(claude_pct.map_or(pct, |cur| cur.max(pct)));
            } else if lower.contains("gemini") {
                gemini_pct = Some(gemini_pct.map_or(pct, |cur| cur.max(pct)));
            }
        }
    }

    (Some(acc.email), claude_pct, gemini_pct)
}

fn render_bar(pct: f64, width: usize, high_is_warn: bool) -> String {
    let filled = ((pct / 100.0) * width as f64).round() as usize;
    let filled = filled.min(width);
    let empty = width - filled;

    let color = if high_is_warn {
        if pct >= 85.0 {
            "\x1b[38;5;203m" // red
        } else if pct >= 65.0 {
            "\x1b[38;5;221m" // yellow
        } else {
            "\x1b[38;5;114m" // green
        }
    } else if pct <= 15.0 {
        "\x1b[38;5;203m"
    } else if pct <= 35.0 {
        "\x1b[38;5;221m"
    } else {
        "\x1b[38;5;114m"
    };

    format!(
        "{}{}\x1b[38;5;240m{}\x1b[0m",
        color,
        "▮".repeat(filled),
        "▯".repeat(empty)
    )
}
