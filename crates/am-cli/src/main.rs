use am_core::types::TargetHost;
use am_core::{
    AccountSelectionFilter, AccountSortOrder, AntigravityManager, ModelPoolPreference,
    SwitchStrategy, DEFAULT_QUOTA_TTL_SECS,
};
use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand};
use colored::*;
use comfy_table::modifiers::UTF8_ROUND_CORNERS;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{Cell, Color, Row, Table};

#[derive(Parser)]
#[command(name = "am")]
#[command(author = "Antigravity Team")]
#[command(version = "0.1.0")]
#[command(
    about = "🚀 Lightweight Google AI Account Manager for Antigravity (App, IDE, Plugin, CLI)",
    long_about = None
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// 📋 List all accounts and their quota pools (alias: ls)
    #[command(alias = "ls")]
    List {
        /// Filter by specific tag
        #[arg(short, long)]
        tag: Option<String>,

        /// Show healthy accounts only
        #[arg(long)]
        healthy: bool,

        /// Sort order: quota, priority, last-used, recent, created, email
        #[arg(short, long)]
        sort: Option<String>,

        /// Output in JSON format
        #[arg(long)]
        json: bool,
    },

    /// 👤 Show currently active accounts across all host targets (alias: whoami)
    #[command(alias = "whoami")]
    Current,

    /// ⚡ Switch active account for a SPECIFIC target host (alias: use)
    #[command(alias = "use")]
    Switch {
        /// Target account ID, email, or prefix (optional if --auto is specified)
        account: Option<String>,

        /// Target host (MANDATORY): ide, app, cli, plugin, or all
        #[arg(short = 't', long = "target")]
        target: String,

        /// Automatically pick the optimal account based on strategy
        #[arg(short, long)]
        auto: bool,

        /// Smart strategy: max-quota (default), least-used, priority, random
        #[arg(short, long, default_value = "max-quota")]
        strategy: String,

        /// Model quota preference: balanced (default), claude, gemini
        #[arg(short, long, default_value = "balanced")]
        prefer: String,

        /// Scope auto-selection to accounts having this tag
        #[arg(long)]
        tag: Option<String>,

        /// Force restart the host application instead of hot reload
        #[arg(short, long)]
        restart: bool,
    },

    /// 🏷 Manage account tags (add, remove, set, ls)
    Tag {
        /// Target account ID, email, or prefix
        account: String,

        #[command(subcommand)]
        action: TagAction,
    },

    /// 🔄 Refresh quota for a single account or all accounts (alias: sync)
    #[command(alias = "sync")]
    Refresh {
        /// Target account ID or email (omit to refresh ALL accounts)
        account: Option<String>,
    },

    /// 🔐 Authenticate and add account via official Google OAuth (dynamic ephemeral port)
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

    /// 🚫 Manually disable an account
    Disable {
        account: String,
        #[arg(short, long)]
        reason: Option<String>,
    },

    /// 🟢 Manually enable a disabled account
    Enable {
        account: String,
    },

    /// 🗑 Delete one or multiple accounts
    #[command(alias = "rm")]
    Delete {
        /// Account ID(s) or Email(s)
        accounts: Vec<String>,
    },

    /// 🔍 Inspect active Hub client fingerprint and local/remote alignment
    Fingerprint,
}

#[derive(Subcommand)]
enum TagAction {
    /// Add one or more tags to the account
    Add {
        #[arg(required = true)]
        tags: Vec<String>,
    },
    /// Remove one or more tags from the account
    #[command(alias = "rm")]
    Remove {
        #[arg(required = true)]
        tags: Vec<String>,
    },
    /// Overwrite all tags for the account
    Set {
        #[arg(required = true)]
        tags: Vec<String>,
    },
    /// List tags of the account
    #[command(alias = "ls")]
    List,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let manager = AntigravityManager::new()?;

    match cli.command {
        None => {
            handle_list(&manager, None, false, None, false)?;
        }
        Some(Commands::List {
            tag,
            healthy,
            sort,
            json,
        }) => {
            let sort_order = match sort.as_deref() {
                Some(s) => Some(s.parse::<AccountSortOrder>().map_err(|e| anyhow!("{}", e))?),
                None => None,
            };
            handle_list(&manager, tag.as_deref(), healthy, sort_order, json)?;
        }
        Some(Commands::Current) => {
            handle_current(&manager)?;
        }
        Some(Commands::Fingerprint) => {
            handle_fingerprint().await?;
        }
        Some(Commands::Switch {
            account,
            target,
            auto,
            strategy,
            prefer,
            tag,
            restart,
        }) => {
            let target_host: TargetHost = target.parse().map_err(|e| anyhow!("{}", e))?;
            handle_switch_command(
                &manager,
                account.as_deref(),
                auto,
                &strategy,
                &prefer,
                tag.as_deref(),
                target_host,
                restart,
            )
            .await?;
        }
        Some(Commands::Tag { account, action }) => {
            handle_tag_command(&manager, &account, action)?;
        }
        Some(Commands::Disable { account, reason }) => {
            let acc = manager.disable_account(&account, reason.as_deref())?;
            println!(
                "{}",
                format!(
                    "🚫 Account {} ({}) is now DISABLED: {}",
                    acc.email,
                    &acc.id[..8],
                    acc.disabled_reason.as_deref().unwrap_or("No reason")
                )
                .yellow()
            );
        }
        Some(Commands::Enable { account }) => {
            let acc = manager.enable_account(&account)?;
            println!(
                "{}",
                format!("🟢 Account {} ({}) is now ENABLED!", acc.email, &acc.id[..8]).green()
            );
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

fn handle_list(
    manager: &AntigravityManager,
    filter_tag: Option<&str>,
    healthy_only: bool,
    sort_order: Option<AccountSortOrder>,
    json: bool,
) -> Result<()> {
    let accounts = manager.query_accounts(filter_tag, healthy_only, sort_order)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&accounts)?);
        return Ok(());
    }

    if accounts.is_empty() {
        println!(
            "{}",
            "No matching accounts found. Run 'am login' or check your filters.".yellow()
        );
        return Ok(());
    }

    let now = chrono::Utc::now().timestamp();
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.apply_modifier(UTF8_ROUND_CORNERS);
    table.set_header(vec![
        Cell::new("ID").fg(Color::Cyan),
        Cell::new("Email").fg(Color::Green),
        Cell::new("Gemini Pool").fg(Color::Yellow),
        Cell::new("Claude Pool").fg(Color::Magenta),
        Cell::new("Tags").fg(Color::Blue),
        Cell::new("Last Used").fg(Color::Cyan),
        Cell::new("Status").fg(Color::White),
    ]);

    for acc in accounts {
        let is_stale = acc
            .quota
            .as_ref()
            .and_then(|q| q.last_updated)
            .map(|t| (now - t) > DEFAULT_QUOTA_TTL_SECS)
            .unwrap_or(true);

        let stale_suffix = if is_stale && acc.quota.is_some() {
            " (stale)"
        } else {
            ""
        };

        let gemini_str = acc
            .quota
            .as_ref()
            .and_then(|q| q.gemini_percentage)
            .map(|p| format!("{:.0}%{}", p, stale_suffix))
            .unwrap_or_else(|| "-".to_string());

        let claude_str = acc
            .quota
            .as_ref()
            .and_then(|q| q.claude_percentage)
            .map(|p| format!("{:.0}%{}", p, stale_suffix))
            .unwrap_or_else(|| "-".to_string());

        let tags_str = if acc.tags.is_empty() {
            "-".to_string()
        } else {
            acc.tags.join(", ")
        };

        let last_used_str = acc
            .last_used
            .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
            .map(|dt| dt.format("%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "Never".to_string());

        let status_str = if acc.disabled {
            let reason = acc.disabled_reason.as_deref().unwrap_or("Disabled");
            format!("🔴 {}", reason).red().to_string()
        } else if acc.banned {
            "🚫 Banned".red().bold().to_string()
        } else {
            "🟢 Healthy".green().to_string()
        };

        let short_id = if acc.id.len() > 8 {
            &acc.id[..8]
        } else {
            &acc.id
        };

        table.add_row(Row::from(vec![
            Cell::new(short_id),
            Cell::new(&acc.email),
            Cell::new(&gemini_str),
            Cell::new(&claude_str),
            Cell::new(&tags_str),
            Cell::new(&last_used_str),
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
        Cell::new(if hosts.app_running {
            "🟢 Yes"
        } else {
            "⚪ No"
        }),
        Cell::new("Official Desktop App (Target: app)"),
    ]));

    table.add_row(Row::from(vec![
        Cell::new("Antigravity IDE"),
        Cell::new(if hosts.ide_running {
            "🟢 Yes"
        } else {
            "⚪ No"
        }),
        Cell::new("VSCode-based IDE (Target: ide)"),
    ]));

    table.add_row(Row::from(vec![
        Cell::new("Antigravity Plugin (ACP)"),
        Cell::new(if hosts.plugin_running {
            "🟢 Yes"
        } else {
            "⚪ No"
        }),
        Cell::new("IDE Plugin Daemon (Target: plugin)"),
    ]));

    table.add_row(Row::from(vec![
        Cell::new("Antigravity CLI (agy)"),
        Cell::new("🟢 Ready"),
        Cell::new("Standalone Terminal CLI (Target: cli / agy)"),
    ]));

    println!("{table}");
    Ok(())
}

async fn handle_switch_command(
    manager: &AntigravityManager,
    account_query: Option<&str>,
    auto: bool,
    strategy_str: &str,
    prefer_str: &str,
    tag: Option<&str>,
    target: TargetHost,
    restart: bool,
) -> Result<()> {
    let target_query = if auto || account_query.is_none() {
        let strategy: SwitchStrategy = strategy_str.parse().map_err(|e| anyhow!("{}", e))?;
        let prefer_pool: ModelPoolPreference =
            prefer_str.parse().map_err(|e| anyhow!("{}", e))?;

        println!(
            "{}",
            format!(
                "🤖 Smart Account Selection: strategy='{}', prefer='{}', tag={:?} for target [{:?}]...",
                strategy_str,
                prefer_str,
                tag.unwrap_or("none"),
                target
            )
            .cyan()
        );

        let filter = AccountSelectionFilter {
            strategy: Some(strategy),
            prefer_pool: Some(prefer_pool),
            required_tag: tag.map(String::from),
            healthy_only: true,
            min_quota_percentage: None,
        };

        let best = manager
            .pick_best_account(filter)?
            .ok_or_else(|| anyhow!("No healthy accounts found matching selection criteria!"))?;

        let quota_eval =
            AntigravityManager::calculate_account_effective_quota(&best, prefer_pool);
        println!(
            "{}",
            format!(
                "🎯 Selected optimal account: {} (ID: {}, Quota Score: {:.1}%)",
                best.email,
                &best.id[..8],
                quota_eval
            )
            .green()
        );

        best.id
    } else {
        account_query.unwrap().to_string()
    };

    println!(
        "{}",
        format!("⚡ Switching target [{:?}] to account '{}'...", target, target_query).cyan()
    );
    let switched = manager.switch_account(&target_query, target, restart)?;
    println!(
        "{}",
        format!(
            "✅ Successfully switched [{:?}] to {} ({})!",
            target,
            switched.email,
            &switched.id[..8]
        )
        .green()
    );

    if !restart {
        println!(
            "{}",
            "💡 Language Server hot-reloaded seamlessly. No application restart needed!".dimmed()
        );
    }
    Ok(())
}

fn handle_tag_command(
    manager: &AntigravityManager,
    account_query: &str,
    action: TagAction,
) -> Result<()> {
    match action {
        TagAction::Add { tags } => {
            let updated = manager.add_account_tags(account_query, &tags)?;
            println!(
                "{}",
                format!(
                    "🏷 Added tags {:?} to {} (current: {:?})",
                    tags, updated.email, updated.tags
                )
                .green()
            );
        }
        TagAction::Remove { tags } => {
            let updated = manager.remove_account_tags(account_query, &tags)?;
            println!(
                "{}",
                format!(
                    "🏷 Removed tags {:?} from {} (current: {:?})",
                    tags, updated.email, updated.tags
                )
                .green()
            );
        }
        TagAction::Set { tags } => {
            let updated = manager.set_account_tags(account_query, &tags)?;
            println!(
                "{}",
                format!("🏷 Set tags for {} to {:?}", updated.email, updated.tags).green()
            );
        }
        TagAction::List => {
            let acc = manager
                .find_account(account_query)?
                .ok_or_else(|| anyhow!("Account not found: {}", account_query))?;
            println!(
                "{}",
                format!("🏷 Tags for {}: {:?}", acc.email, acc.tags).cyan()
            );
        }
    }
    Ok(())
}

async fn handle_refresh(manager: &AntigravityManager, account_query: Option<&str>) -> Result<()> {
    match account_query {
        Some(query) => {
            let acc = manager
                .find_account(query)?
                .ok_or_else(|| anyhow!("Account not found matching: {}", query))?;
            println!(
                "{}",
                format!("🔄 Refreshing quota for {}...", acc.email).cyan()
            );
            match manager.refresh_account_quota(&acc.id).await {
                Ok(quota) => print_quota_summary(&acc.email, &quota),
                Err(e) => println!("  {} Failed: {}", "✖".red(), e),
            }
        }
        None => {
            println!(
                "{}",
                "🔄 Refreshing quotas for ALL accounts with rate-limit pacing (250ms)..."
                    .cyan()
            );
            let results = manager.refresh_all_quotas().await?;
            let mut success = 0;
            let mut disabled_count = 0;
            let mut failed = 0;
            for (email, res) in results {
                match res {
                    Ok(q) => {
                        println!("  {} Refreshed {}", "✔".green(), email);
                        print_quota_summary(&email, &q);
                        success += 1;
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        if msg.contains("Disabled") {
                            println!("  {} Account Auto-Disabled {}: {}", "🚫".yellow(), email, msg);
                            disabled_count += 1;
                        } else {
                            println!("  {} Failed {}: {}", "✖".red(), email, e);
                            failed += 1;
                        }
                    }
                }
            }
            println!(
                "\n{}",
                format!(
                    "✨ Sync complete: {} succeeded, {} auto-disabled, {} failed.",
                    success, disabled_count, failed
                )
                .green()
            );
        }
    }
    Ok(())
}

fn print_quota_summary(_email: &str, quota: &am_core::types::QuotaSummary) {
    let gemini = quota
        .gemini_percentage
        .map(|p| format!("{:.0}%", p))
        .unwrap_or_else(|| "-".to_string());
    let claude = quota
        .claude_percentage
        .map(|p| format!("{:.0}%", p))
        .unwrap_or_else(|| "-".to_string());
    println!(
        "     {} Gemini Pool: {} | Claude Pool: {}",
        "•".blue(),
        gemini.yellow(),
        claude.magenta()
    );
}

async fn handle_login(manager: &AntigravityManager, manual: bool) -> Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let oauth = am_core::auth::OAuthClient::new();

    println!(
        "{}",
        "🔐 Starting official Google OAuth authentication...".cyan()
    );

    if manual {
        let redirect_uri = "http://127.0.0.1/callback";
        let state = uuid::Uuid::new_v4().to_string();
        let auth_url = am_core::auth::OAuthClient::build_auth_url(redirect_uri, &state);

        println!("\nOpen this URL in your browser:\n{}", auth_url.bold());
        println!("\nEnter the authorization code or full callback URL:");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        let code = input.trim();
        let token_info = oauth.exchange_code(code, redirect_uri).await?;
        let user_info = oauth.fetch_user_info(&token_info.access_token).await?;
        println!(
            "{}",
            format!("✅ Successfully authenticated as {}!", user_info.email).green()
        );
        let acc = manager
            .add_account_by_token(&token_info.refresh_token, vec![])
            .await?;
        println!("{}", format!("Saved account ID: {}", acc.id).green());
        return Ok(());
    }

    // 绑定系统随机空闲端口 (:0)，彻底避免冲突
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let redirect_uri = format!("http://127.0.0.1:{}/callback", port);
    let state = uuid::Uuid::new_v4().to_string();
    let auth_url = am_core::auth::OAuthClient::build_auth_url(&redirect_uri, &state);

    println!("Opening browser for authentication (Allocated loopback port: {})...", port);
    let _ = open::that(&auth_url);
    println!("Listening on {} for callback...", redirect_uri);

    if let Ok((mut socket, _)) = listener.accept().await {
        let mut buf = [0u8; 2048];
        let n = socket.read(&mut buf).await?;
        let request = String::from_utf8_lossy(&buf[..n]);

        if let Some(code_part) = request
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
        {
            if let Some(code) = code_part
                .split("code=")
                .nth(1)
                .and_then(|s| s.split('&').next())
            {
                let token_info = oauth.exchange_code(code, &redirect_uri).await?;
                let user_info = oauth.fetch_user_info(&token_info.access_token).await?;

                let resp = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n\
                    <html><body style='font-family:sans-serif;text-align:center;padding:50px;'>\
                    <h1 style='color:green;'>✅ Authorization Successful!</h1>\
                    <p>Antigravity Manager has securely linked your Google account. You may close this tab.</p>\
                    </body></html>";
                let _ = socket.write_all(resp.as_bytes()).await;

                println!(
                    "{}",
                    format!("✅ Successfully authenticated as {}!", user_info.email).green()
                );
                let acc = manager
                    .add_account_by_token(&token_info.refresh_token, vec![])
                    .await?;
                println!("{}", format!("Saved account ID: {}", acc.id).green());
                return Ok(());
            }
        }

        println!(
            "{}",
            "❌ Failed to extract authorization code from callback.".red()
        );
    }

    Ok(())
}

async fn handle_add(
    manager: &AntigravityManager,
    token: &str,
    tags: Vec<String>,
) -> Result<()> {
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
        println!(
            "{}",
            "Please specify at least one account ID or email to delete.".yellow()
        );
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

async fn handle_fingerprint() -> Result<()> {
    let fp = am_core::quota::fingerprint::get_active_fingerprint();
    println!(
        "{}",
        "🛡 Active Antigravity Hub Client Fingerprint".cyan().bold()
    );
    println!("  {} Hub Version:  {}", "•".blue(), fp.version.bold());
    println!("  {} Piper CL:     {}", "•".blue(), fp.cl.bold());
    println!(
        "  {} Source:       {}",
        "•".blue(),
        match fp.source {
            am_core::quota::fingerprint::FingerprintSource::Local => {
                "Local Installed App (Dynamic Alignment)".green().bold()
            }
            am_core::quota::fingerprint::FingerprintSource::Baseline => {
                "Calibrated Baseline (Fallback)".yellow().bold()
            }
        }
    );
    if let Some(path) = &fp.local_path {
        println!(
            "  {} Detected At:  {}",
            "•".blue(),
            path.display().to_string().dimmed()
        );
    }
    println!("  {} Outbound UA:  {}", "•".blue(), fp.user_agent.dimmed());

    // 远程源版本感知
    print!("  {} Checking Google Hub releases... ", "•".blue());
    if let Some(remote) = am_core::quota::fingerprint::check_remote_hub_release().await {
        println!("{}", format!("Latest Remote: v{}", remote.version).green());
        if remote.version != fp.version {
            println!(
                "    {} Google released newer Hub v{}. Update your native App to sync signature.",
                "⚠️".yellow(),
                remote.version
            );
        }
    } else {
        println!("{}", "(Remote check offline)".dimmed());
    }

    Ok(())
}
