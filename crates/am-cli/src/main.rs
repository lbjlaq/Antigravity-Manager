use am_core::types::TargetHost;
use am_core::AntigravityManager;
use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::*;
use comfy_table::modifiers::UTF8_ROUND_CORNERS;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{Cell, Color, Row, Table};

#[derive(Parser)]
#[command(name = "am")]
#[command(author = "Antigravity Team")]
#[command(version = "0.1.0")]
#[command(about = "🚀 Lightweight Google AI Account Manager for Antigravity (App, IDE, Plugin, CLI)", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// 📋 List all accounts and their quota pools (alias: ls)
    #[command(alias = "ls")]
    List {
        /// Output in JSON format
        #[arg(long)]
        json: bool,
    },

    /// 👤 Show currently active accounts across all host targets (alias: whoami)
    #[command(alias = "whoami")]
    Current,

    /// ⚡ Hot-switch active account across host targets (alias: use)
    #[command(alias = "use")]
    Switch {
        /// Target account ID, email, or prefix
        account: String,

        /// Target host: all, app, ide, plugin, agy (default: all)
        #[arg(short, long, default_value = "all")]
        target: String,

        /// Force restart the host application instead of hot reload
        #[arg(short, long)]
        restart: bool,
    },

    /// 🔄 Refresh quota for a single account or all accounts (alias: sync)
    #[command(alias = "sync")]
    Refresh {
        /// Target account ID or email (omit to refresh ALL accounts)
        account: Option<String>,
    },

    /// 🔐 Authenticate and add account via official Google OAuth
    Login {
        /// Manual mode: print auth URL and paste authorization code manually (for headless/SSH)
        #[arg(short, long)]
        manual: bool,
    },

    /// ➕ Add account directly using an existing Refresh Token
    Add {
        /// Google OAuth Refresh Token
        #[arg(short, long)]
        token: String,

        /// Optional tags (comma separated)
        #[arg(short, long, value_delimiter = ',')]
        tags: Vec<String>,
    },

    /// 🗑 Delete one or multiple accounts
    #[command(alias = "rm")]
    Delete {
        /// Account ID(s) or Email(s)
        accounts: Vec<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let manager = AntigravityManager::new()?;

    match cli.command {
        None => {
            handle_list(&manager, false)?;
        }
        Some(Commands::List { json }) => {
            handle_list(&manager, json)?;
        }
        Some(Commands::Current) => {
            handle_current(&manager)?;
        }
        Some(Commands::Switch {
            account,
            target,
            restart,
        }) => {
            let target_host: TargetHost = target.parse().map_err(|e| anyhow::anyhow!("{}", e))?;
            handle_switch(&manager, &account, target_host, restart)?;
        }
        Some(Commands::Refresh { account }) => {
            handle_refresh(&manager, account.as_deref()).await?;
        }
        Some(Commands::Login { manual }) => {
            handle_login(&manager, manual).await?;
        }
        Some(Commands::Add { token, tags }) => {
            handle_add(&manager, &token, tags).await?;
        }
        Some(Commands::Delete { accounts }) => {
            handle_delete(&manager, accounts)?;
        }
    }

    Ok(())
}

fn handle_list(manager: &AntigravityManager, json: bool) -> Result<()> {
    let summaries = manager.list_accounts()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&summaries)?);
        return Ok(());
    }

    if summaries.is_empty() {
        println!("{}", "No accounts found. Run 'am login' or 'am add --token <token>' to add one.".yellow());
        return Ok(());
    }

    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.apply_modifier(UTF8_ROUND_CORNERS);
    table.set_header(vec![
        Cell::new("ID").fg(Color::Cyan),
        Cell::new("Email").fg(Color::Green),
        Cell::new("Gemini Pool").fg(Color::Yellow),
        Cell::new("Claude Pool").fg(Color::Magenta),
        Cell::new("Tags").fg(Color::Blue),
        Cell::new("Status").fg(Color::White),
    ]);

    for s in summaries {
        let full = manager.get_account(&s.id).ok().flatten();
        let gemini_str = full
            .as_ref()
            .and_then(|a| a.quota.as_ref())
            .and_then(|q| q.gemini_percentage)
            .map(|p| format!("{:.0}%", p))
            .unwrap_or_else(|| "-".to_string());

        let claude_str = full
            .as_ref()
            .and_then(|a| a.quota.as_ref())
            .and_then(|q| q.claude_percentage)
            .map(|p| format!("{:.0}%", p))
            .unwrap_or_else(|| "-".to_string());

        let tags_str = if s.tags.is_empty() {
            "-".to_string()
        } else {
            s.tags.join(", ")
        };

        let status_str = if s.disabled {
            "🔴 Disabled".red().to_string()
        } else {
            "🟢 Healthy".green().to_string()
        };

        let short_id = if s.id.len() > 8 { &s.id[..8] } else { &s.id };

        table.add_row(Row::from(vec![
            Cell::new(short_id),
            Cell::new(&s.email),
            Cell::new(&gemini_str),
            Cell::new(&claude_str),
            Cell::new(&tags_str),
            Cell::new(&status_str),
        ]));
    }

    println!("{table}");
    Ok(())
}

fn handle_current(manager: &AntigravityManager) -> Result<()> {
    let hosts = manager.get_active_hosts();
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.apply_modifier(UTF8_ROUND_CORNERS);
    table.set_header(vec![
        Cell::new("Target Host").fg(Color::Cyan),
        Cell::new("Running").fg(Color::Yellow),
        Cell::new("Description").fg(Color::White),
    ]);

    table.add_row(Row::from(vec![
        Cell::new("Antigravity App (Classic)"),
        Cell::new(if hosts.app_running { "🟢 Yes" } else { "⚪ No" }),
        Cell::new("Official Desktop App (/Applications/Antigravity.app)"),
    ]));

    table.add_row(Row::from(vec![
        Cell::new("Antigravity IDE"),
        Cell::new(if hosts.ide_running { "🟢 Yes" } else { "⚪ No" }),
        Cell::new("VSCode-based IDE (/Applications/Antigravity IDE.app)"),
    ]));

    table.add_row(Row::from(vec![
        Cell::new("Antigravity Plugin (ACP)"),
        Cell::new(if hosts.plugin_running { "🟢 Yes" } else { "⚪ No" }),
        Cell::new("IDE Plugin Daemon (agy_acp_server)"),
    ]));

    table.add_row(Row::from(vec![
        Cell::new("Antigravity CLI (agy)"),
        Cell::new("🟢 Ready"),
        Cell::new("Standalone Terminal CLI (~/.local/bin/agy)"),
    ]));

    println!("{table}");
    Ok(())
}

fn handle_switch(
    manager: &AntigravityManager,
    account_query: &str,
    target: TargetHost,
    restart: bool,
) -> Result<()> {
    println!("{}", format!("⚡ Switching to account '{}' (Target: {:?})...", account_query, target).cyan());
    let switched = manager.switch_account(account_query, target, restart)?;
    println!(
        "{}",
        format!(
            "✅ Successfully switched to {} ({}) across {:?}!",
            switched.email,
            &switched.id[..8],
            target
        )
        .green()
    );
    Ok(())
}

async fn handle_refresh(manager: &AntigravityManager, account_query: Option<&str>) -> Result<()> {
    if let Some(query) = account_query {
        let acc = manager
            .find_account(query)?
            .ok_or_else(|| anyhow::anyhow!("Account not found: {}", query))?;
        println!("{}", format!("🔄 Refreshing quota for {}...", acc.email).cyan());
        let q = manager.refresh_account_quota(&acc.id).await?;
        println!(
            "{}",
            format!(
                "✅ Refreshed! Gemini: {:.0}%, Claude: {:.0}%",
                q.gemini_percentage.unwrap_or(0.0),
                q.claude_percentage.unwrap_or(0.0)
            )
            .green()
        );
    } else {
        println!("{}", "🔄 Batch refreshing all accounts...".cyan());
        let results = manager.refresh_all_quotas().await?;
        for (email, res) in results {
            match res {
                Ok(q) => {
                    println!(
                        "  {} {}: Gemini {:.0}%, Claude {:.0}%",
                        "✔".green(),
                        email,
                        q.gemini_percentage.unwrap_or(0.0),
                        q.claude_percentage.unwrap_or(0.0)
                    );
                }
                Err(e) => {
                    println!("  {} {}: Failed ({})", "✖".red(), email, e);
                }
            }
        }
        println!("{}", "✅ All quotas refreshed!".green());
    }
    Ok(())
}

async fn handle_login(manager: &AntigravityManager, manual: bool) -> Result<()> {
    let redirect_uri = "http://localhost:8085/oauth/callback";
    let state = uuid::Uuid::new_v4().to_string();
    let auth_url = am_core::auth::OAuthClient::build_auth_url(redirect_uri, &state);

    println!("{}", "🔐 Google OAuth 2.0 Authorization".bold());
    println!("{}", "Open the following URL in your browser to sign in:".yellow());
    println!("\n  {}\n", auth_url.underline().blue());

    if manual {
        println!("After signing in, paste the authorization code (or the full redirect URL) below:");
        print!("> ");
        use std::io::Write;
        std::io::stdout().flush()?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        let code = input.trim();
        let code = if code.contains("code=") {
            code.split("code=").nth(1).unwrap_or(code).split('&').next().unwrap_or(code)
        } else {
            code
        };

        let oauth = am_core::auth::OAuthClient::new();
        let token_info = oauth.exchange_code(code, redirect_uri).await?;
        let user_info = oauth.fetch_user_info(&token_info.access_token).await?;

        println!("{}", format!("✅ Successfully authenticated as {}!", user_info.email).green());
        let acc = manager.add_account_by_token(&token_info.refresh_token, vec![]).await?;
        println!("{}", format!("Saved account ID: {}", acc.id).green());
    } else {
        println!("{}", "Waiting for local browser callback on port 8085...".cyan());
        // 自动拉起默认浏览器 (Mac: open)
        let _ = std::process::Command::new("open").arg(&auth_url).spawn();

        // 简易本地 HTTP 监听一次性请求
        let listener = tokio::net::TcpListener::bind("127.0.0.1:8085").await?;
        let (mut socket, _) = listener.accept().await?;
        let mut buf = [0u8; 2048];
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let n = socket.read(&mut buf).await?;
        let request = String::from_utf8_lossy(&buf[..n]);

        if let Some(code_part) = request.lines().next().and_then(|l| l.split_whitespace().nth(1)) {
            if let Some(code) = code_part.split("code=").nth(1).and_then(|s| s.split('&').next()) {
                let oauth = am_core::auth::OAuthClient::new();
                let token_info = oauth.exchange_code(code, redirect_uri).await?;
                let user_info = oauth.fetch_user_info(&token_info.access_token).await?;

                let resp = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n\
                    <html><body style='font-family:sans-serif;text-align:center;padding:50px;'>\
                    <h1 style='color:green;'>✅ Authorization Successful!</h1>\
                    <p>You can close this tab and return to the terminal.</p>\
                    </body></html>";
                let _ = socket.write_all(resp.as_bytes()).await;

                println!("{}", format!("✅ Successfully authenticated as {}!", user_info.email).green());
                let acc = manager.add_account_by_token(&token_info.refresh_token, vec![]).await?;
                println!("{}", format!("Saved account ID: {}", acc.id).green());
                return Ok(());
            }
        }

        println!("{}", "❌ Failed to extract authorization code from callback.".red());
    }

    Ok(())
}

async fn handle_add(manager: &AntigravityManager, token: &str, tags: Vec<String>) -> Result<()> {
    println!("{}", "➕ Verifying token and adding account...".cyan());
    let account = manager.add_account_by_token(token, tags).await?;
    println!(
        "{}",
        format!(
            "✅ Successfully added account {} ({})!",
            account.email,
            &account.id[..8]
        )
        .green()
    );
    Ok(())
}

fn handle_delete(manager: &AntigravityManager, accounts: Vec<String>) -> Result<()> {
    if accounts.is_empty() {
        println!("{}", "Please specify at least one account ID or email to delete.".yellow());
        return Ok(());
    }

    for acc in accounts {
        match manager.delete_account(&acc) {
            Ok(_) => println!("  {} Deleted account '{}'", "✔".green(), acc),
            Err(e) => println!("  {} Failed to delete '{}': {}", "✖".red(), acc, e),
        }
    }
    Ok(())
}
