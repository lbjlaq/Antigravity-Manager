use clap::{Parser, Subcommand};
use antigravity_tools_lib::{
    modules::{account, config},
    services::proxy::ProxyService,
};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage the proxy server
    Server {
        #[command(subcommand)]
        action: ServerCommands,
    },
    /// Manage accounts
    Account {
        #[command(subcommand)]
        action: AccountCommands,
    },
    /// Manage configuration
    Config {
        #[command(subcommand)]
        action: ConfigCommands,
    },
}

#[derive(Subcommand)]
enum ServerCommands {
    /// Start the proxy server
    Start {
        /// Optional port override
        #[arg(short, long)]
        port: Option<u16>,
    },
    /// Stop the proxy server (if running via background service - note: CLI usually runs foreground)
    Stop,
}

#[derive(Subcommand)]
enum AccountCommands {
    /// List all accounts
    List,
    /// Switch active account
    Use {
        /// Account ID or partial email
        id: String,
    },
    /// Delete an account
    Delete {
        id: String,
    }
}

#[derive(Subcommand)]
enum ConfigCommands {
    /// Show current configuration
    Show,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging (simple stdout for CLI for now, or reuse modules::logger if adapted)
    antigravity_tools_lib::modules::logger::init_logger(); 
    // tracing_subscriber::fmt::init(); // Use simple stdout

    let cli = Cli::parse();

    match cli.command {
        Commands::Server { action } => match action {
            ServerCommands::Start { port } => {
                println!("Starting server...");
                let mut app_config = config::load_app_config()?;
                
                if let Some(p) = port {
                    app_config.proxy.port = p;
                }
                
                let service = ProxyService::new();
                let status = service.start(app_config.proxy.clone(), None).await?;
                
                println!("Server running at {}", status.base_url);
                println!("Press Ctrl+C to stop");
                
                // Keep running
                tokio::signal::ctrl_c().await?;
                println!("Shutting down...");
                service.stop().await?;
            }
            ServerCommands::Stop => {
                println!("If the server is running as a daemon, use system tools to stop it. CLI 'start' runs in foreground.");
            }
        },
        Commands::Account { action } => match action {
            AccountCommands::List => {
                let accounts = account::list_accounts()?;
                let current_id = account::get_current_account_id()?;
                
                println!("{:<40} {:<30} {:<10} {:<10}", "ID", "Email", "Tier", "Active");
                println!("{}", "-".repeat(95));
                
                for account in accounts {
                    let active = if Some(&account.id) == current_id.as_ref() { "*" } else { "" };
                    let tier = account.quota.as_ref()
                        .and_then(|q| q.subscription_tier.as_ref())
                        .map(|s| s.as_str())
                        .unwrap_or("Free");
                        
                    println!("{:<40} {:<30} {:<10} {:<10}", 
                        account.id, 
                        account.email, 
                        tier,
                        active
                    );
                }
            }
            AccountCommands::Use { id } => {
                // Find account fuzzy
                let accounts = account::list_accounts()?;
                let target = accounts.iter().find(|a| a.id == id || a.email.contains(&id));
                
                if let Some(acc) = target {
                    account::switch_account(&acc.id).await?;
                    println!("Switched to account: {}", acc.email);
                } else {
                    println!("Account not found");
                }
            }
            AccountCommands::Delete { id } => {
                account::delete_account(&id)?;
                println!("Deleted account {}", id);
            }
        },
        Commands::Config { action } => match action {
            ConfigCommands::Show => {
                let config = config::load_app_config()?;
                println!("{:#?}", config);
            }
        }
    }

    Ok(())
}
