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

    /// 📊 View local token usage & activity statistics across 4 hosts (Zero-Proxy Local DB)
    Stats {
        /// Projection perspective: all (default), account, model, app, or timeline
        #[arg(short, long, default_value = "all")]
        view: String,

        /// Output in JSON format
        #[arg(long)]
        json: bool,
    },

    /// 💬 Inspect active conversation context window, token breakdown & health
    Context,

    /// ⚡ Realtime live telemetry & TPS monitor for active host application
    Watch {
        /// Polling interval in seconds (default: 2)
        #[arg(short, long, default_value = "2")]
        interval: u64,
    },

    /// ⚙️ Manage background daemon, auto-switch, and quota polling configuration
    Config {
        /// Show current configuration
        #[arg(short, long)]
        show: bool,

        /// Toggle auto-switch on quota exhaustion: on / off
        #[arg(long)]
        auto_switch: Option<String>,

        /// Toggle periodic background quota polling: on / off
        #[arg(long)]
        quota_polling: Option<String>,

        /// Polling interval in minutes (5 to 1440)
        #[arg(long)]
        interval_minutes: Option<u64>,
    },

    /// 📦 Export all accounts into a portable backup JSON file
    Export {
        /// Output file path (default: ./antigravity_accounts_backup.json)
        #[arg(short, long, default_value = "antigravity_accounts_backup.json")]
        output: String,
    },

    /// 📥 Import accounts from a backup JSON file
    Import {
        /// Backup file path to import
        input: String,

        /// Overwrite existing accounts with same ID
        #[arg(short, long)]
        overwrite: bool,
    },
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
        Some(Commands::Stats { view, json }) => {
            handle_stats(&manager, &view, json)?;
        }
        Some(Commands::Context) => {
            handle_context(&manager)?;
        }
        Some(Commands::Watch { interval }) => {
            handle_watch(interval).await?;
        }
        Some(Commands::Config {
            show,
            auto_switch,
            quota_polling,
            interval_minutes,
        }) => {
            handle_config(show, auto_switch.as_deref(), quota_polling.as_deref(), interval_minutes)?;
        }
        Some(Commands::Export { output }) => {
            handle_export(&manager, &output)?;
        }
        Some(Commands::Import { input, overwrite }) => {
            handle_import(&manager, &input, overwrite)?;
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

fn handle_stats(manager: &AntigravityManager, view: &str, json: bool) -> Result<()> {
    let report = manager.get_local_usage_stats()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    match view.to_lowercase().as_str() {
        "account" | "acc" => print_account_perspective(&report),
        "model" | "models" => print_model_perspective(&report),
        "app" | "host" => print_app_perspective(&report),
        "timeline" | "time" => print_timeline_perspective(&report),
        _ => {
            // 默认全景总览 (Apple 设计风格: Hero Card + 多维矩阵切片)
            print_hero_overview(&report);
            println!();
            print_account_perspective(&report);
            println!();
            print_model_perspective(&report);
            println!();
            print_app_perspective(&report);
        }
    }
    Ok(())
}

fn print_hero_overview(r: &am_core::stats::MasterAnalyticsReport) {
    println!("{}", "🍎 Antigravity Intelligence & Efficiency Insights".cyan().bold());
    println!("{}", "────────────────────────────────────────────────────────────────────────".dimmed());
    println!(
        "  {} Total Volume:       {} ({} reqs)",
        "⚡".yellow(),
        format_tokens(r.total_tokens).bold(),
        r.total_requests
    );
    println!(
        "  {} Cache Hit Ratio:    {:.1}% (Saved: ${:.2} USD)",
        "✨".green(),
        r.global_cache_hit_rate,
        r.total_saved_cost_usd
    );
    println!(
        "  {} Performance:        {:.1} tok/s · {}ms TTFT",
        "🚀".cyan(),
        r.avg_speed_tps,
        r.avg_ttft_ms
    );
    println!("{}", "────────────────────────────────────────────────────────────────────────".dimmed());
}

fn print_account_perspective(r: &am_core::stats::MasterAnalyticsReport) {
    println!("{}", "👤 Account Attribution Matrix (Token 归属与价值大盘)".cyan().bold());
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.apply_modifier(UTF8_ROUND_CORNERS);
    table.set_header(vec![
        Cell::new("Account Email").fg(Color::Cyan),
        Cell::new("Reqs").fg(Color::Yellow),
        Cell::new("New Input").fg(Color::White),
        Cell::new("Cached (Hit%)").fg(Color::Green),
        Cell::new("Output").fg(Color::Magenta),
        Cell::new("Est. Cost").fg(Color::Yellow),
        Cell::new("Quota (G/C)").fg(Color::Blue),
    ]);

    for a in &r.by_account {
        let quota_str = format!(
            "{}/{}",
            a.quota_gemini_pct.map(|p| format!("{:.0}%", p)).unwrap_or_else(|| "-".into()),
            a.quota_claude_pct.map(|p| format!("{:.0}%", p)).unwrap_or_else(|| "-".into())
        );

        table.add_row(Row::from(vec![
            Cell::new(&a.email),
            Cell::new(a.total_requests.to_string()),
            Cell::new(format_tokens(a.new_input_tokens)),
            Cell::new(format!("{} ({:.0}%)", format_tokens(a.cached_tokens), a.cache_hit_rate)),
            Cell::new(format_tokens(a.output_tokens)),
            Cell::new(format!("${:.3}", a.estimated_cost_usd)),
            Cell::new(quota_str),
        ]));
    }
    println!("{table}");
}

fn print_model_perspective(r: &am_core::stats::MasterAnalyticsReport) {
    println!("{}", "⚡ Model Fidelity & Physical Backend Resolution (真实物理模型与速度)".cyan().bold());
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.apply_modifier(UTF8_ROUND_CORNERS);
    table.set_header(vec![
        Cell::new("Requested Model").fg(Color::Cyan),
        Cell::new("Physical Backend Model").fg(Color::Green),
        Cell::new("Output Speed").fg(Color::Yellow),
        Cell::new("First Byte").fg(Color::Magenta),
        Cell::new("Total Volume").fg(Color::White),
        Cell::new("Share").fg(Color::Blue),
    ]);

    for m in &r.by_model {
        let phy_display = if m.is_downgraded {
            format!("⚠️ {}", m.physical_model).yellow().to_string()
        } else {
            format!("⚡ {}", m.physical_model).green().to_string()
        };

        table.add_row(Row::from(vec![
            Cell::new(&m.requested_model),
            Cell::new(&phy_display),
            Cell::new(format!("{:.1} tok/s", m.avg_tps)),
            Cell::new(format!("{} ms", m.avg_ttft_ms)),
            Cell::new(format_tokens(m.total_tokens)),
            Cell::new(format!("{:.1}%", m.share_percentage)),
        ]));
    }
    println!("{table}");
}

fn print_app_perspective(r: &am_core::stats::MasterAnalyticsReport) {
    println!("{}", "💻 Multi-Host Target & Top Workspaces (4 大宿主与工作区分流)".cyan().bold());
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.apply_modifier(UTF8_ROUND_CORNERS);
    table.set_header(vec![
        Cell::new("Host Target").fg(Color::Cyan),
        Cell::new("Conversations").fg(Color::Yellow),
        Cell::new("Total Steps").fg(Color::Magenta),
        Cell::new("Volume Share").fg(Color::Green),
        Cell::new("Primary Workspace").fg(Color::White),
    ]);

    for h in &r.by_host {
        let primary_ws = h.top_workspaces.first().map(|(w, s)| format!("{} ({} steps)", w, s)).unwrap_or_else(|| "-".into());
        table.add_row(Row::from(vec![
            Cell::new(&h.host_name),
            Cell::new(h.conversation_count.to_string()),
            Cell::new(format_number(h.total_steps)),
            Cell::new(format!("{:.1}%", h.share_percentage)),
            Cell::new(primary_ws),
        ]));
    }
    println!("{table}");
}

fn print_timeline_perspective(r: &am_core::stats::MasterAnalyticsReport) {
    println!("{}", "⏱ 24h Developer Circadian Rhythm (生物钟时段与开发节奏)".cyan().bold());
    for t in &r.timeline {
        let bar_len = (t.share_percentage / 5.0) as usize;
        let bar = "█".repeat(bar_len) + &"░".repeat(20usize.saturating_sub(bar_len));
        println!(
            "  {:<18} [{}] {:>4.1}%  · {} (Peak: {:.0} tok/s)",
            t.time_slot.bold(),
            bar.cyan(),
            t.share_percentage,
            t.label.dimmed(),
            t.peak_tps
        );
    }
}

fn handle_context(manager: &AntigravityManager) -> Result<()> {
    let report = manager.get_local_usage_stats()?;
    let ctx = match report.active_context {
        Some(c) => c,
        None => {
            println!("{}", "No active conversation context detected.".yellow());
            return Ok(());
        }
    };

    println!("{}", "🍏 Active Context Window & Session Inspector (当前会话深度上下文)".cyan().bold());
    println!("{}", "────────────────────────────────────────────────────────────────────────".dimmed());
    println!("  📍 Active Session:    {}", ctx.title.bold());
    println!("  📁 Workspace:         {}", ctx.workspace.green());
    println!("  🖥️  Host Target:       {}", ctx.host_target.blue());
    println!("  🧠 Physical Engine:   {}", ctx.physical_model.magenta());
    println!("{}", "────────────────────────────────────────────────────────────────────────".dimmed());

    let bar_len = (ctx.usage_percentage / 5.0).min(20.0) as usize;
    let bar = "█".repeat(bar_len) + &"░".repeat(20usize.saturating_sub(bar_len));
    let color_bar = if ctx.health_status == "healthy" {
        bar.green()
    } else if ctx.health_status == "warning" {
        bar.yellow()
    } else {
        bar.red()
    };

    println!("\n🌊 Context Window Utilization (用量水位)");
    println!(
        "  Steps:   {} steps  [{}] {:.1}% ({} remaining)",
        ctx.step_count.to_string().bold(),
        color_bar,
        ctx.usage_percentage,
        format_tokens(ctx.remaining_tokens)
    );
    println!(
        "  Used:    {} / {} budget",
        format_tokens(ctx.current_used_tokens).bold(),
        format_tokens(ctx.budget_tokens)
    );

    println!("\n✨ Cache & Token Reservoir Decomposition (分层水库拆解)");
    println!("  • Cached Input Tokens:   {} ({:.1}% Hit Rate)", format_tokens(ctx.cached_tokens).green(), ctx.cache_hit_rate);
    println!("  • Direct Prompt Input:   {}", format_tokens(ctx.prompt_tokens));
    println!("  • Completion Output:     {}", format_tokens(ctx.completion_tokens).cyan());
    println!("  • Thinking / Reasoning:  {} ({:.1}%)", format_tokens(ctx.thinking_tokens).magenta(), (ctx.thinking_tokens as f64 / ctx.current_used_tokens.max(1) as f64) * 100.0);
    println!("  • Tool Execution Stream: {}", format_tokens(ctx.tool_tokens).yellow());
    println!("  • Cost Saved (Cache):    ${:.2} USD", ctx.saved_cost_usd);

    if let Some(t) = ctx.last_model_update {
        println!("\n⏱ Last Interaction: {}", t.dimmed());
    }
    Ok(())
}

fn format_tokens(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.2}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}K", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

fn format_number(num: u64) -> String {
    let s = num.to_string();
    let mut result = String::new();
    let mut count = 0;
    for c in s.chars().rev() {
        if count > 0 && count % 3 == 0 {
            result.push(',');
        }
        result.push(c);
        count += 1;
    }
    result.chars().rev().collect()
}

async fn handle_watch(interval_secs: u64) -> Result<()> {
    println!("{}", "👀 Starting Live Antigravity Telemetry Watcher... (Ctrl+C to stop)".cyan().bold());
    let monitor = am_core::RealtimeMonitor::new()?;
    let path = match am_core::RealtimeMonitor::locate_latest_transcript() {
        Some(p) => p,
        None => {
            println!("{}", "No active Antigravity session transcript found.".yellow());
            return Ok(());
        }
    };

    println!("Targeting session log: {}\n", path.display().to_string().dimmed());
    println!("{:<10} {:<15} {:<24} {:<10} {:<10} {:<12} {:<10}", 
        "TIME".bold(), "APP / HOST".bold(), "MODEL".bold(), "OUT TOK".bold(), "LATENCY".bold(), "SPEED (TPS)".bold(), "ACCOUNT".bold());
    println!("{}", "─".repeat(95).dimmed());

    let mut last_step = 0u64;

    loop {
        if let Ok(Some(snap)) = monitor.extract_latest_snapshot(&path) {
            if snap.step_index != last_step {
                last_step = snap.step_index;
                let account = snap.account_email.unwrap_or_else(|| "current".to_string());
                let tps_color = if snap.tps > 150.0 {
                    format!("{:.1} tok/s", snap.tps).green().bold()
                } else if snap.tps > 80.0 {
                    format!("{:.1} tok/s", snap.tps).cyan()
                } else {
                    format!("{:.1} tok/s", snap.tps).yellow()
                };

                println!("{:<10} {:<15} {:<24} {:<10} {:<10} {:<12} {:<10}",
                    snap.timestamp,
                    format!("{:?}", snap.host),
                    snap.model.dimmed(),
                    format_tokens(snap.output_tokens),
                    format!("{:.1}s", snap.first_token_latency_seconds),
                    tps_color,
                    account.cyan()
                );
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(interval_secs.max(1))).await;
    }
}

fn handle_config(
    show: bool,
    auto_switch: Option<&str>,
    quota_polling: Option<&str>,
    interval_minutes: Option<u64>,
) -> Result<()> {
    let mut cfg = am_core::AppConfig::load();
    let mut modified = false;

    if let Some(sw) = auto_switch {
        let val = sw.eq_ignore_ascii_case("on") || sw.eq_ignore_ascii_case("true") || sw == "1";
        cfg.auto_switch.enabled = val;
        modified = true;
        println!("⚙️ Auto-Switch on quota exhaustion: {}", if val { "ENABLED".green().bold() } else { "DISABLED".yellow() });
    }

    if let Some(qp) = quota_polling {
        let val = qp.eq_ignore_ascii_case("on") || qp.eq_ignore_ascii_case("true") || qp == "1";
        cfg.quota_polling.enabled = val;
        modified = true;
        println!("⚙️ Background Quota Polling: {}", if val { "ENABLED".green().bold() } else { "DISABLED".yellow() });
    }

    if let Some(inv) = interval_minutes {
        let val = inv.max(5);
        cfg.quota_polling.interval_minutes = val;
        modified = true;
        println!("⚙️ Polling Interval: {} minutes", val.to_string().cyan());
    }

    if modified {
        cfg.save()?;
        println!("{}", "✅ Configuration saved successfully!".green());
    } else if show || (auto_switch.is_none() && quota_polling.is_none() && interval_minutes.is_none()) {
        println!("{}", "⚙️ Current Antigravity Manager Configuration:".cyan().bold());
        println!("{}", serde_json::to_string_pretty(&cfg)?);
    }

    Ok(())
}

fn handle_export(_manager: &AntigravityManager, output: &str) -> Result<()> {
    let storage = am_core::storage::StorageManager::new()?;
    let path = std::path::Path::new(output);
    let count = am_core::storage::backup::BackupManager::export_to_file(&storage, path)?;
    println!("{}", format!("✅ Exported {} accounts to '{}'", count, output).green().bold());
    Ok(())
}

fn handle_import(_manager: &AntigravityManager, input: &str, overwrite: bool) -> Result<()> {
    let storage = am_core::storage::StorageManager::new()?;
    let path = std::path::Path::new(input);
    let report = am_core::storage::backup::BackupManager::import_from_file(&storage, path, overwrite)?;
    println!("{}", "📦 Import complete:".cyan().bold());
    println!("  • Imported new: {}", report.imported.to_string().green());
    println!("  • Overwritten:  {}", report.overwritten.to_string().yellow());
    println!("  • Skipped:      {}", report.skipped.to_string().dimmed());
    Ok(())
}
