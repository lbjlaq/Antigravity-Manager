//! CLI Handler Module
//! Handles CLI subcommands (switch, list, current) for frictionless terminal workflows.

use crate::modules::account;
use crate::modules::integration::CliIntegration;
use std::process::Command;

/// Attempts to handle CLI arguments. Returns Ok(true) if a CLI command was handled, Ok(false) otherwise.
pub async fn try_handle_cli(args: &[String]) -> Result<bool, String> {
    if args.len() < 2 {
        return Ok(false);
    }

    let subcmd = args[1].as_str();

    // Skip known app flags so they are passed to GUI / Headless runtimes
    if subcmd == "--headless" || subcmd.starts_with("--") {
        if subcmd == "--help" || subcmd == "-h" {
            print_cli_help();
            return Ok(true);
        }
        return Ok(false);
    }

    match subcmd {
        "switch" | "s" => {
            handle_switch_command(&args[2..]).await?;
            Ok(true)
        }
        "import" | "im" => {
            handle_import_command(&args[2..]).await?;
            Ok(true)
        }
        "export" | "ex" => {
            handle_export_command(&args[2..]).await?;
            Ok(true)
        }
        "list" | "ls" | "accounts" => {
            handle_list_command().await?;
            Ok(true)
        }
        "current" | "whoami" => {
            handle_current_command().await?;
            Ok(true)
        }
        "sync" => {
            handle_sync_command(&args[2..]).await?;
            Ok(true)
        }
        "help" => {
            print_cli_help();
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn print_cli_help() {
    println!(
        "\x1b[1mAntigravity Tools CLI\x1b[0m\n\n\
        Usage: antigravity-tools <command> [options]\n\n\
        Commands:\n  \
          switch [account]   Switch current account (supports email, index, name, or auto round-robin)\n  \
          import <file>      High-performance batch import accounts from JSON (with auto-backup)\n  \
          export [file]      Fast export all accounts to JSON (default: accounts_export.json)\n  \
          list, ls           List all accounts with active status and quota\n  \
          current, whoami    Show currently active accounts across clients (IDE / agy CLI)\n  \
          sync [app]         One-click sync proxy URL and API key to Agent tools (dsh, claude, codex, etc.)\n  \
          help               Show this help message\n\n\
        Flags:\n  \
          --headless         Run as a background headless proxy daemon (no GUI / Wayland display required)\n\n\
        Options for `switch`:\n  \
          --target <target>  Target client: 'agy' (default), 'acp' ('plugin'), 'ide', or 'classic'\n  \
          -c, --continue     Automatically resume agy conversation via `agy --continue` after switch\n\n\
        Options for `sync`:\n  \
          [app]              Target Agent tool: 'dsh' / 'deepseek-harness' (default), 'claude', 'codex', 'gemini', 'opencode', 'jeikcode', 'grok', or 'all'\n  \
          --model <model>    Default model to sync (default: gemini-3.8-flash-high)\n\n\
        Examples:\n  \
          antigravity-tools switch                     # Auto round-robin to next normal account\n  \
          antigravity-tools switch 1                   # Switch to account #1\n  \
          antigravity-tools switch work@gmail.com      # Switch by email or prefix\n  \
          antigravity-tools switch 2 --continue        # Switch to #2 and resume conversation\n  \
          antigravity-tools sync dsh                   # Sync to DeepSeek Harness (Web GUI & CLI)\n  \
          antigravity-tools sync dsh --model gemini-3.8-flash\n  \
          antigravity-tools sync all                   # Sync to all detected Agent tools\n  \
          antigravity-tools --headless                 # Start headless proxy server (systemd friendly)\n"
    );
}

async fn handle_switch_command(args: &[String]) -> Result<(), String> {
    let mut query: Option<String> = None;
    let mut target_ide: String = "agy".to_string();
    let mut auto_continue = false;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "-c" || arg == "--continue" {
            auto_continue = true;
        } else if arg == "--target" || arg == "--ide" {
            if i + 1 < args.len() {
                target_ide = args[i + 1].clone();
                i += 1;
            }
        } else if !arg.starts_with('-') && query.is_none() {
            query = Some(arg.clone());
        }
        i += 1;
    }

    println!("\x1b[34m[Antigravity CLI]\x1b[0m 正在检索目标账号...");

    // 智能解析目标账号
    let target_summary = account::resolve_account_summary(query.as_deref(), Some(&target_ide))?;

    // 优先尝试通过本地运行的 Manager API 切换（同步刷新内存与全量状态）
    let switched_via_api = try_switch_via_local_api(&target_summary.id, &target_ide).await;

    if !switched_via_api {
        // 离线降级方案：直接调用本地核心与 CliIntegration 写系统凭据
        let integration = CliIntegration;
        account::switch_account(&target_summary.id, Some(&target_ide), &integration).await?;
    }

    let name_str = target_summary
        .name
        .map(|n| format!(" ({})", n))
        .unwrap_or_default();
    println!(
        "\x1b[32m✔ 成功切换账号!\x1b[0m 目标: \x1b[1m{}\x1b[0m | 邮箱: \x1b[36m{}{}\x1b[0m",
        target_ide, target_summary.email, name_str
    );

    if auto_continue {
        println!("\x1b[34m[Antigravity CLI]\x1b[0m 正在无缝接续会话 (`agy --continue`)...\n");
        execute_agy_continue();
    }

    Ok(())
}

async fn handle_list_command() -> Result<(), String> {
    let index = account::load_account_index()?;
    let status = account::detect_active_clients_status().unwrap_or_default();

    println!("\n\x1b[1m当前激活账号状态:\x1b[0m");
    println!(
        "  • agy CLI (Keychain):  \x1b[36m{}\x1b[0m",
        status.agy_email.as_deref().unwrap_or("未设置")
    );
    println!(
        "  • IDE Plugin (ACP):    \x1b[36m{}\x1b[0m",
        status.acp_email.as_deref().unwrap_or("未设置")
    );
    println!(
        "  • Antigravity IDE:     \x1b[36m{}\x1b[0m",
        status.ide_email.as_deref().unwrap_or("未设置")
    );
    println!(
        "  • Antigravity Classic: \x1b[36m{}\x1b[0m\n",
        status.classic_email.as_deref().unwrap_or("未设置")
    );

    println!("序号 | 激活 | 邮箱                         | 别名         | 状态");
    println!("-----+------+------------------------------+--------------+---------");

    for (i, acc) in index.accounts.iter().enumerate() {
        let is_active = if status.agy_email.as_deref() == Some(&acc.email) {
            "  👉 "
        } else {
            "     "
        };
        let email = format!("{:<28}", truncate_str(&acc.email, 28));
        let name = format!(
            "{:<12}",
            truncate_str(acc.name.as_deref().unwrap_or("-"), 12)
        );
        let state = if acc.disabled {
            "\x1b[31m已禁用\x1b[0m"
        } else if acc.proxy_disabled {
            "\x1b[33m代理停用\x1b[0m"
        } else {
            "\x1b[32m正常\x1b[0m"
        };
        println!(
            "{:<4} |{}| {} | {} | {}",
            i + 1,
            is_active,
            email,
            name,
            state
        );
    }

    println!("\n提示: 运行 `antigravity-tools switch` 自动顺延，或 `antigravity-tools switch <序号/邮箱>`。\n");
    Ok(())
}

async fn handle_current_command() -> Result<(), String> {
    let status = account::detect_active_clients_status().unwrap_or_default();
    println!(
        "agy: {}\nacp: {}\nide: {}\nclassic: {}",
        status.agy_email.as_deref().unwrap_or("none"),
        status.acp_email.as_deref().unwrap_or("none"),
        status.ide_email.as_deref().unwrap_or("none"),
        status.classic_email.as_deref().unwrap_or("none")
    );
    Ok(())
}

fn truncate_str(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let mut res: String = s.chars().take(max_len.saturating_sub(3)).collect();
        res.push_str("...");
        res
    }
}

fn read_proxy_port_and_key() -> (u16, Option<String>) {
    if let Ok(data_dir) = account::get_data_dir() {
        let cfg_path = data_dir.join("gui_config.json");
        if let Ok(content) = std::fs::read_to_string(cfg_path) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                let p = val
                    .get("proxy")
                    .and_then(|p| p.get("port"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(8045) as u16;
                let k = val
                    .get("proxy")
                    .and_then(|p| p.get("api_key"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                return (p, k);
            }
        }
    }
    (8045, None)
}

/// 一键同步 Agent 工具配置命令（CLI 与 Headless 环境）
async fn handle_sync_command(args: &[String]) -> Result<(), String> {
    let mut target_app = "dsh".to_string();
    let mut model: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--model" || arg == "-m" {
            if i + 1 < args.len() {
                model = Some(args[i + 1].clone());
                i += 1;
            }
        } else if !arg.starts_with('-') {
            target_app = arg.to_lowercase();
        }
        i += 1;
    }

    let (port, api_key_opt) = read_proxy_port_and_key();
    let api_key = api_key_opt.unwrap_or_default();
    let proxy_url = format!("http://127.0.0.1:{}", port);

    println!(
        "\x1b[34m[Antigravity CLI]\x1b[0m 正在一键同步 Agent 工具配置 (目标: \x1b[1m{}\x1b[0m)...",
        target_app
    );

    use crate::proxy::cli_sync::{sync_config, CliApp};

    let target_model = model.unwrap_or_else(|| "gemini-3.8-flash-high".to_string());

    match target_app.as_str() {
        "dsh" | "deepseek" | "deepseek-harness" | "deepseek_harness" => {
            sync_config(
                &CliApp::DeepSeekHarness,
                &proxy_url,
                &api_key,
                Some(&target_model),
            )?;
            println!("\x1b[32m✔ 成功同步配置到 DeepSeek Harness!\x1b[0m");
            println!("  • 适配模式: \x1b[36mWeb GUI (dsh web) & CLI (dsh)\x1b[0m");
            println!("  • 接口地址 (Base URL): \x1b[36m{}/v1\x1b[0m", proxy_url);
            println!("  • 默认模型: \x1b[36m{}\x1b[0m", target_model);
            println!("  • 配置文件:");
            println!("    - ~/.dsh/profiles/desktop/cordis.patch.yml (CLI & Desktop)");
            println!("    - ~/.dsh/profiles/web/cordis.patch.yml (Web GUI)");
            println!("    - ~/.dsh/.credentials.yaml");
        }
        "claude" | "claude-code" | "claudecode" => {
            sync_config(&CliApp::Claude, &proxy_url, &api_key, Some(&target_model))?;
            println!(
                "\x1b[32m✔ 成功同步配置到 Claude Code!\x1b[0m (模型: {})",
                target_model
            );
        }
        "codex" => {
            let codex_url = format!("{}/v1", proxy_url);
            sync_config(&CliApp::Codex, &codex_url, &api_key, Some(&target_model))?;
            println!(
                "\x1b[32m✔ 成功同步配置到 Codex AI!\x1b[0m (模型: {})",
                target_model
            );
        }
        "gemini" | "agy" => {
            sync_config(&CliApp::Gemini, &proxy_url, &api_key, Some(&target_model))?;
            println!(
                "\x1b[32m✔ 成功同步配置到 Antigravity CLI (agy)!\x1b[0m (模型: {})",
                target_model
            );
        }
        "opencode" => {
            let opencode_url = format!("{}/v1", proxy_url);
            sync_config(
                &CliApp::OpenCode,
                &opencode_url,
                &api_key,
                Some(&target_model),
            )?;
            println!("\x1b[32m✔ 成功同步配置到 OpenCode!\x1b[0m");
        }
        "jeikcode" => {
            let jeik_url = format!("{}/v1", proxy_url);
            sync_config(&CliApp::JeikCode, &jeik_url, &api_key, Some(&target_model))?;
            println!(
                "\x1b[32m✔ 成功同步配置到 JeikCode!\x1b[0m (模型: {})",
                target_model
            );
        }
        "grok" | "grokbuild" => {
            let grok_url = format!("{}/v1", proxy_url);
            sync_config(&CliApp::GrokBuild, &grok_url, &api_key, Some(&target_model))?;
            println!(
                "\x1b[32m✔ 成功同步配置到 Grok Build!\x1b[0m (模型: {})",
                target_model
            );
        }
        "all" => {
            let apps = [
                (
                    CliApp::DeepSeekHarness,
                    "DeepSeek Harness (Web GUI & CLI)",
                    true,
                ),
                (CliApp::Claude, "Claude Code", false),
                (CliApp::Codex, "Codex AI", true),
                (CliApp::Gemini, "Antigravity CLI (agy)", false),
                (CliApp::OpenCode, "OpenCode", true),
                (CliApp::JeikCode, "JeikCode", true),
                (CliApp::GrokBuild, "Grok Build", true),
            ];
            for (app, name, needs_v1) in apps {
                let url = if needs_v1 {
                    format!("{}/v1", proxy_url)
                } else {
                    proxy_url.clone()
                };
                if let Err(e) = sync_config(&app, &url, &api_key, Some(&target_model)) {
                    eprintln!("  \x1b[31m✘\x1b[0m 同步 {} 失败: {}", name, e);
                } else {
                    println!("  \x1b[32m✔\x1b[0m 已同步 {}", name);
                }
            }
            println!("\x1b[32m🎉 全部 Agent 工具配置同步完成!\x1b[0m");
        }
        other => {
            return Err(format!(
                "未知的 Agent 工具: '{}'。支持的工具: dsh, claude, codex, gemini, opencode, jeikcode, grok, all",
                other
            ));
        }
    }

    Ok(())
}

async fn try_switch_via_local_api(account_id: &str, target_ide: &str) -> bool {
    // 读取本地配置中的端口与密钥
    let (port, api_key) = read_proxy_port_and_key();

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(1500))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };

    let url = format!("http://127.0.0.1:{}/api/accounts/switch", port);
    let mut req = client.post(&url).json(&serde_json::json!({
        "accountId": account_id,
        "targetIde": target_ide,
    }));

    if let Some(key) = api_key {
        if !key.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", key));
        }
    }

    match req.send().await {
        Ok(res) => res.status().is_success(),
        Err(_) => false,
    }
}

fn execute_agy_continue() {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // 查找可用的 agy 执行程序
        let agy_bin = find_agy_executable();
        let err = Command::new(&agy_bin).arg("--continue").exec();
        eprintln!("启动 agy --continue 失败: {}", err);
    }

    #[cfg(windows)]
    {
        let agy_bin = find_agy_executable();
        let _ = Command::new(&agy_bin).arg("--continue").status();
    }
}

fn find_agy_executable() -> String {
    if let Some(home) = dirs::home_dir() {
        let local_agy = home.join(".local/bin/agy");
        if local_agy.exists() {
            return local_agy.to_string_lossy().to_string();
        }
        let local_agy_cli = home.join(".local/bin/agy_cli");
        if local_agy_cli.exists() {
            return local_agy_cli.to_string_lossy().to_string();
        }
    }
    "agy".to_string()
}

/// [NEW #3726] CLI 批量高性能导入命令
async fn handle_import_command(args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        return Err("用法: antigravity-tools import <file.json>".to_string());
    }

    let file_path = std::path::PathBuf::from(&args[0]);
    if !file_path.exists() {
        return Err(format!("文件不存在: {:?}", file_path));
    }

    let content =
        std::fs::read_to_string(&file_path).map_err(|e| format!("读取导入文件失败: {}", e))?;

    // 自动为源文件创建 <filename>-backup 备份文件，确保失败随时可重新导入
    let backup_path = format!("{}-backup", file_path.to_string_lossy());
    if let Err(e) = std::fs::write(&backup_path, &content) {
        eprintln!("\x1b[33m[Warning]\x1b[0m 无法创建源文件备份: {}", e);
    } else {
        println!(
            "\x1b[32m✔\x1b[0m 已自动备份源文件至: \x1b[1m{}\x1b[0m",
            backup_path
        );
    }

    let entries = account::parse_import_entries_from_str(&content);
    if entries.is_empty() {
        return Err("未从文件中解析出有效的 refresh_token 凭据".to_string());
    }

    println!(
        "\x1b[34m[Antigravity CLI]\x1b[0m 发现 {} 个有效账号凭据，正在以流式并发 (6路) 导入...",
        entries.len()
    );

    let start = std::time::Instant::now();
    let result = account::batch_import_accounts_logic(None, entries).await?;
    let duration = start.elapsed();

    println!(
        "\x1b[32m🎉 批量导入完成!\x1b[0m 成功: \x1b[32m{}\x1b[0m | 失败: \x1b[31m{}\x1b[0m | 耗时: {:.2}s",
        result.success,
        result.failed,
        duration.as_secs_f64()
    );

    if !result.errors.is_empty() {
        eprintln!("\n\x1b[33m部分失败明细:\x1b[0m");
        for (i, err) in result.errors.iter().take(5).enumerate() {
            eprintln!("  {}. {}", i + 1, err);
        }
        if result.errors.len() > 5 {
            eprintln!("  ... 其余 {} 条错误已忽略", result.errors.len() - 5);
        }
    }

    Ok(())
}

/// [NEW #3726] CLI 极速导出所有账号命令
async fn handle_export_command(args: &[String]) -> Result<(), String> {
    let out_path = if !args.is_empty() {
        std::path::PathBuf::from(&args[0])
    } else {
        std::path::PathBuf::from("antigravity_accounts_export.json")
    };

    println!("\x1b[34m[Antigravity CLI]\x1b[0m 正在极速导出所有账号...");
    let response = account::export_accounts_by_ids(&[])?;
    let content = serde_json::to_string_pretty(&response.accounts)
        .map_err(|e| format!("序列化失败: {}", e))?;

    std::fs::write(&out_path, content.as_bytes())
        .map_err(|e| format!("写入导出文件失败: {}", e))?;

    println!(
        "\x1b[32m✔ 成功导出 {} 个账号至: \x1b[1m{:?}\x1b[0m",
        response.accounts.len(),
        out_path
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_str() {
        assert_eq!(truncate_str("short", 10), "short");
        assert_eq!(
            truncate_str("verylongemailaddress@gmail.com", 15),
            "verylongemai..."
        );
    }

    #[tokio::test]
    async fn test_try_handle_cli_skips_gui_and_headless() {
        let args = vec!["antigravity-tools".to_string()];
        assert_eq!(try_handle_cli(&args).await.unwrap(), false);

        let args = vec!["antigravity-tools".to_string(), "--headless".to_string()];
        assert_eq!(try_handle_cli(&args).await.unwrap(), false);

        let args = vec![
            "antigravity-tools".to_string(),
            "--unknown-flag".to_string(),
        ];
        assert_eq!(try_handle_cli(&args).await.unwrap(), false);
    }
}
