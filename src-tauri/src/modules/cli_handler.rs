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
        "help" => {
            print_cli_help();
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn print_cli_help() {
    println!(
        "\x1b[1mAntigravity Manager CLI\x1b[0m\n\n\
        Usage: antigravity-manager <command> [options]\n\n\
        Commands:\n  \
          switch [account]   Switch current account (supports email, index, name, or auto round-robin)\n  \
          import <file>      High-performance batch import accounts from JSON (with auto-backup)\n  \
          export [file]      Fast export all accounts to JSON (default: accounts_export.json)\n  \
          list, ls           List all accounts with active status and quota\n  \
          current, whoami    Show currently active accounts across clients (IDE / agy CLI)\n  \
          help               Show this help message\n\n\
        Options for `switch`:\n  \
          --target <target>  Target client: 'agy' (default), 'acp' ('plugin'), 'ide', or 'classic'\n  \
          -c, --continue     Automatically resume agy conversation via `agy --continue` after switch\n\n\
        Examples:\n  \
          antigravity-manager switch                     # Auto round-robin to next normal account\n  \
          antigravity-manager switch 1                   # Switch to account #1\n  \
          antigravity-manager switch work@gmail.com      # Switch by email or prefix\n  \
          antigravity-manager switch 2 --continue        # Switch to #2 and resume conversation\n"
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

    println!("\n提示: 运行 `antigravity-manager switch` 自动顺延，或 `antigravity-manager switch <序号/邮箱>`。\n");
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

async fn try_switch_via_local_api(_account_id: &str, _target_ide: &str) -> bool {
    false
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
        return Err("用法: antigravity-manager import <file.json>".to_string());
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
        let args = vec!["antigravity-manager".to_string()];
        assert_eq!(try_handle_cli(&args).await.unwrap(), false);

        let args = vec!["antigravity-manager".to_string(), "--headless".to_string()];
        assert_eq!(try_handle_cli(&args).await.unwrap(), false);

        let args = vec![
            "antigravity-manager".to_string(),
            "--unknown-flag".to_string(),
        ];
        assert_eq!(try_handle_cli(&args).await.unwrap(), false);
    }
}
