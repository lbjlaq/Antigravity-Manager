//! Axum Server Module
//!
//! This module provides the HTTP server implementation for the proxy service.
//! It has been refactored from a monolithic 2000+ line file into organized submodules.

pub mod admin;
pub mod oauth;
pub mod routes;
pub mod types;

// Re-export main types for external use
pub use types::AppState;

use crate::proxy::TokenManager;
use dashmap::DashSet;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::sync::OnceLock;
use tokio::sync::oneshot;
use tokio::sync::RwLock;
use tracing::{debug, error};

// =============================================================================
// [FIX] Global queue for pending account reloads
// When update_account_quota updates protected_models, account ID is added here
// TokenManager checks and processes these accounts in get_token
// 
// [PERF] Using DashSet instead of std::sync::RwLock to avoid blocking tokio workers
// =============================================================================

static PENDING_RELOAD_ACCOUNTS: OnceLock<DashSet<String>> = OnceLock::new();

fn get_pending_reload_accounts() -> &'static DashSet<String> {
    PENDING_RELOAD_ACCOUNTS.get_or_init(DashSet::new)
}

/// Trigger account reload signal (called by update_account_quota)
pub fn trigger_account_reload(account_id: &str) {
    let pending = get_pending_reload_accounts();
    pending.insert(account_id.to_string());
    tracing::debug!(
        "[Quota] Queued account {} for TokenManager reload",
        account_id
    );
}

/// Get and clear pending reload accounts (called by TokenManager)
pub fn take_pending_reload_accounts() -> Vec<String> {
    let pending = get_pending_reload_accounts();
    let accounts: Vec<String> = pending.iter().map(|r| r.clone()).collect();
    if !accounts.is_empty() {
        pending.clear();
        tracing::debug!(
            "[Quota] Taking {} pending accounts for reload",
            accounts.len()
        );
    }
    accounts
}

/// Axum server instance
#[derive(Clone)]
pub struct AxumServer {
    shutdown_tx: Arc<tokio::sync::Mutex<Option<oneshot::Sender<()>>>>,
    custom_mapping: Arc<tokio::sync::RwLock<std::collections::HashMap<String, String>>>,
    proxy_state: Arc<tokio::sync::RwLock<crate::proxy::config::UpstreamProxyConfig>>,
    security_state: Arc<RwLock<crate::proxy::ProxySecurityConfig>>,
    pub security_monitor_state: Arc<RwLock<crate::proxy::config::SecurityMonitorConfig>>,
    zai_state: Arc<RwLock<crate::proxy::ZaiConfig>>,
    experimental: Arc<RwLock<crate::proxy::config::ExperimentalConfig>>,
    debug_logging: Arc<RwLock<crate::proxy::config::DebugLoggingConfig>>,
    pub cloudflared_state: Arc<crate::commands::cloudflared::CloudflaredState>,
    pub is_running: Arc<RwLock<bool>>,
    pub upstream: Arc<crate::proxy::upstream::client::UpstreamClient>,
    /// [FIX] Exposed TokenManager for proxy service reuse
    pub token_manager: Arc<TokenManager>,
}

impl AxumServer {
    /// Update model mapping (hot-reload)
    pub async fn update_mapping(&self, config: &crate::proxy::config::ProxyConfig) {
        {
            let mut m = self.custom_mapping.write().await;
            *m = config.custom_mapping.clone();
        }
        tracing::debug!("Model mapping (Custom) hot-reloaded");
    }

    /// Update upstream proxy configuration
    pub async fn update_proxy(&self, new_config: crate::proxy::config::UpstreamProxyConfig) {
        let mut proxy = self.proxy_state.write().await;
        *proxy = new_config.clone();

        // [FIX] Also update underlying reqwest Client
        self.upstream.rebuild_client(Some(new_config)).await;

        tracing::info!("Upstream proxy config hot-reloaded (including HTTP Client)");
    }

    /// Update security configuration
    pub async fn update_security(&self, config: &crate::proxy::config::ProxyConfig) {
        let mut sec = self.security_state.write().await;
        *sec = crate::proxy::ProxySecurityConfig::from_proxy_config(config);
        tracing::info!("Proxy security config hot-reloaded");
    }

    /// Update z.ai configuration
    pub async fn update_zai(&self, config: &crate::proxy::config::ProxyConfig) {
        let mut zai = self.zai_state.write().await;
        *zai = config.zai.clone();
        tracing::info!("z.ai config hot-reloaded");
    }

    /// Update experimental configuration
    pub async fn update_experimental(&self, config: &crate::proxy::config::ProxyConfig) {
        let mut exp = self.experimental.write().await;
        *exp = config.experimental.clone();
        tracing::info!("Experimental config hot-reloaded");
    }

    /// Update debug logging configuration
    pub async fn update_debug_logging(&self, config: &crate::proxy::config::ProxyConfig) {
        let mut dbg_cfg = self.debug_logging.write().await;
        *dbg_cfg = config.debug_logging.clone();
        tracing::info!("Debug logging config hot-reloaded");
    }

    /// Update security monitor config (IP blacklist/whitelist)
    pub async fn update_security_monitor(&self, config: &crate::proxy::config::ProxyConfig) {
        let mut sec_mon = self.security_monitor_state.write().await;
        *sec_mon = config.security_monitor.clone();
        tracing::info!("[Security] IP filtering config hot-reloaded");
    }

    /// Update User-Agent configuration (hot-reload)
    pub async fn update_user_agent(&self, config: &crate::proxy::config::ProxyConfig) {
        self.upstream
            .set_user_agent_override(config.user_agent_override.clone())
            .await;
        tracing::info!("User-Agent config hot-reloaded: {:?}", config.user_agent_override);
    }

    /// Set running state
    pub async fn set_running(&self, running: bool) {
        let mut r = self.is_running.write().await;
        *r = running;
        tracing::info!("Proxy service running state updated to: {}", running);
    }

    /// Start the Axum server
    pub async fn start(
        host: String,
        port: u16,
        token_manager: Arc<TokenManager>,
        custom_mapping: std::collections::HashMap<String, String>,
        _request_timeout: u64,
        upstream_proxy: crate::proxy::config::UpstreamProxyConfig,
        user_agent_override: Option<String>,
        security_config: crate::proxy::ProxySecurityConfig,
        zai_config: crate::proxy::ZaiConfig,
        monitor: Arc<crate::proxy::monitor::ProxyMonitor>,
        experimental_config: crate::proxy::config::ExperimentalConfig,
        debug_logging: crate::proxy::config::DebugLoggingConfig,
        integration: crate::modules::integration::SystemManager,
        cloudflared_state: Arc<crate::commands::cloudflared::CloudflaredState>,
    ) -> Result<(Self, tokio::task::JoinHandle<()>), String> {
        let custom_mapping_state = Arc::new(tokio::sync::RwLock::new(custom_mapping));
        let proxy_state = Arc::new(tokio::sync::RwLock::new(upstream_proxy.clone()));
        let security_state = Arc::new(RwLock::new(security_config));
        let zai_state = Arc::new(RwLock::new(zai_config));
        let provider_rr = Arc::new(AtomicUsize::new(0));
        let zai_vision_mcp_state =
            Arc::new(crate::proxy::zai_vision_mcp::ZaiVisionMcpState::new());
        let experimental_state = Arc::new(RwLock::new(experimental_config));
        let debug_logging_state = Arc::new(RwLock::new(debug_logging));
        let is_running_state = Arc::new(RwLock::new(true));

        // Create upstream client once and share between AppState and AxumServer
        let upstream_client = Arc::new(crate::proxy::upstream::client::UpstreamClient::new(Some(
            upstream_proxy.clone(),
        )));

        // Initialize User-Agent override if configured
        if user_agent_override.is_some() {
            upstream_client.set_user_agent_override(user_agent_override).await;
        }

        let state = AppState {
            token_manager: token_manager.clone(),
            custom_mapping: custom_mapping_state.clone(),
            request_timeout: 300, // 5 minutes
            thought_signature_map: Arc::new(tokio::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            upstream_proxy: proxy_state.clone(),
            upstream: upstream_client.clone(),
            zai: zai_state.clone(),
            provider_rr: provider_rr.clone(),
            zai_vision_mcp: zai_vision_mcp_state,
            monitor: monitor.clone(),
            experimental: experimental_state.clone(),
            debug_logging: debug_logging_state.clone(),
            switching: Arc::new(RwLock::new(false)),
            integration: integration.clone(),
            account_service: Arc::new(crate::modules::account_service::AccountService::new(
                integration.clone(),
            )),
            security: security_state.clone(),
            cloudflared_state: cloudflared_state.clone(),
            is_running: is_running_state.clone(),
            port,
        };

        // Build routes
        use crate::proxy::middleware::{
            admin_auth_middleware, auth_middleware, cors_layer, ip_filter_middleware,
            monitor_middleware, service_status_middleware, SecurityState,
        };

        // Create security monitor state for IP filtering
        let security_monitor_state: SecurityState = Arc::new(RwLock::new(
            crate::proxy::config::SecurityMonitorConfig::default(),
        ));

        // Initialize security database
        if let Err(e) = crate::modules::security_db::init_db() {
            tracing::warn!("[Security] Failed to initialize security database: {}", e);
        }

        // 1. Build proxy routes (AI endpoints with auth)
        let proxy_routes = routes::build_proxy_routes()
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            ))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                monitor_middleware,
            ));

        // 2. Build admin routes (forced auth)
        let admin_routes = routes::build_admin_routes().layer(
            axum::middleware::from_fn_with_state(state.clone(), admin_auth_middleware),
        );

        // 3. Combine and apply global layers
        let max_body_size: usize = std::env::var("ABV_MAX_BODY_SIZE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(100 * 1024 * 1024); // Default 100MB
        tracing::info!("Request body size limit: {} MB", max_body_size / 1024 / 1024);

        let app = axum::Router::new()
            .nest("/api", admin_routes)
            .merge(proxy_routes)
            // Public routes (no auth)
            .route("/auth/callback", axum::routing::get(oauth::handle_oauth_callback))
            // Health check endpoint (no IP filter)
            .route("/healthz", axum::routing::get(routes::health_check))
            // Apply global monitoring and status layers
            .layer(axum::middleware::from_fn(ip_filter_middleware))
            .layer(axum::Extension(security_monitor_state.clone()))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                service_status_middleware,
            ))
            .layer(cors_layer())
            .layer(axum::extract::DefaultBodyLimit::max(max_body_size))
            .with_state(state.clone());

        // Static file hosting (for Headless/Docker mode)
        let dist_path = std::env::var("ABV_DIST_PATH").unwrap_or_else(|_| "dist".to_string());
        let app = if std::path::Path::new(&dist_path).exists() {
            tracing::info!("Hosting static assets from: {}", dist_path);
            app.fallback_service(
                tower_http::services::ServeDir::new(&dist_path).fallback(
                    tower_http::services::ServeFile::new(format!("{}/index.html", dist_path)),
                ),
            )
        } else {
            app
        };

        // Bind address
        let addr = format!("{}:{}", host, port);
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .map_err(|e| format!("Failed to bind address {}: {}", addr, e))?;

        tracing::info!("Proxy server started on http://{}", addr);

        // Create shutdown channel
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();

        let server_instance = Self {
            shutdown_tx: Arc::new(tokio::sync::Mutex::new(Some(shutdown_tx))),
            custom_mapping: custom_mapping_state.clone(),
            proxy_state,
            security_state,
            security_monitor_state: security_monitor_state.clone(),
            zai_state,
            experimental: experimental_state.clone(),
            debug_logging: debug_logging_state.clone(),
            cloudflared_state,
            is_running: is_running_state,
            upstream: upstream_client,
            token_manager: token_manager.clone(),
        };

        // [PERF] Connection limiter to prevent resource exhaustion under high load
        // Default: 10K concurrent connections (configurable via ABV_MAX_CONNECTIONS env)
        let max_connections: usize = std::env::var("ABV_MAX_CONNECTIONS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(10_000);
        let connection_semaphore = Arc::new(tokio::sync::Semaphore::new(max_connections));
        tracing::info!(
            "Connection limiter initialized: max {} concurrent connections",
            max_connections
        );

        // Start server in a new task
        let handle = tokio::spawn(async move {
            use hyper_util::rt::{TokioExecutor, TokioIo};
            use hyper_util::service::TowerToHyperService;

            // [PERF] Track active connections for monitoring
            let active_connections = Arc::new(AtomicUsize::new(0));

            loop {
                tokio::select! {
                    res = listener.accept() => {
                        match res {
                            Ok((stream, remote_addr)) => {
                                // [PERF] Acquire semaphore permit before spawning
                                let permit = match connection_semaphore.clone().try_acquire_owned() {
                                    Ok(p) => p,
                                    Err(_) => {
                                        // Connection limit reached - reject gracefully
                                        tracing::warn!(
                                            "Connection limit reached ({} active), rejecting new connection from {}",
                                            active_connections.load(std::sync::atomic::Ordering::Relaxed),
                                            remote_addr
                                        );
                                        // Drop stream immediately to reject connection
                                        drop(stream);
                                        continue;
                                    }
                                };

                                let io = TokioIo::new(stream);
                                let active_count = active_connections.clone();
                                active_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                                // [FIX] Inject ConnectInfo for real IP extraction
                                use tower::util::ServiceExt;
                                use hyper::body::Incoming;
                                let app_with_info = app.clone().map_request(move |mut req: axum::http::Request<Incoming>| {
                                    req.extensions_mut().insert(axum::extract::ConnectInfo(remote_addr));
                                    req
                                });

                                let service = TowerToHyperService::new(app_with_info);

                                tokio::task::spawn(async move {
                                    // [PERF] Try HTTP/2 first via auto-detection, fallback to HTTP/1.1
                                    // Using hyper's auto HTTP version detection
                                    let result = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
                                        .http1()
                                        .keep_alive(true)
                                        .http2()
                                        .max_concurrent_streams(250)
                                        .serve_connection_with_upgrades(io, service)
                                        .await;

                                    if let Err(err) = result {
                                        debug!("Connection handler finished or errored: {:?}", err);
                                    }

                                    // [PERF] Release connection count and permit
                                    active_count.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                                    drop(permit);
                                });
                            }
                            Err(e) => {
                                error!("Failed to accept connection: {:?}", e);
                            }
                        }
                    }
                    _ = &mut shutdown_rx => {
                        tracing::info!("Proxy server stopped listening, waiting for active connections to drain...");
                        
                        // [PERF] Graceful shutdown: wait for active connections to complete
                        // Maximum wait time: 30 seconds
                        let drain_start = std::time::Instant::now();
                        let max_drain_time = std::time::Duration::from_secs(30);
                        
                        loop {
                            let active = active_connections.load(std::sync::atomic::Ordering::Relaxed);
                            if active == 0 {
                                tracing::info!("All connections drained successfully");
                                break;
                            }
                            
                            if drain_start.elapsed() > max_drain_time {
                                tracing::warn!(
                                    "Graceful shutdown timeout reached with {} active connections, forcing shutdown",
                                    active
                                );
                                break;
                            }
                            
                            tracing::debug!("Waiting for {} active connections to drain...", active);
                            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                        }
                        
                        break;
                    }
                }
            }
        });

        Ok((server_instance, handle))
    }

    /// Stop the server with graceful connection draining
    pub fn stop(&self) {
        let tx_mutex = self.shutdown_tx.clone();
        tokio::spawn(async move {
            let mut lock = tx_mutex.lock().await;
            if let Some(tx) = lock.take() {
                let _ = tx.send(());
                tracing::info!("Axum server stop signal sent (graceful shutdown initiated)");
            }
        });
    }
}
