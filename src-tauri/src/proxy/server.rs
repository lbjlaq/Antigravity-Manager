use axum::{
    extract::{DefaultBodyLimit, State},
    response::{IntoResponse, Json, Response},
    routing::{any, get, post},
    Router,
    http::StatusCode,
};
use std::sync::Arc;
use tokio::sync::oneshot;
use tower_http::trace::TraceLayer;
use tracing::{debug, error};

use crate::proxy::config::{ProxyConfig, UpstreamProxyConfig};
use crate::proxy::token_manager::TokenManager;
use crate::proxy::handlers;

/// App State
#[derive(Clone)]
pub struct AppState {
    pub upstream: Arc<crate::proxy::upstream::client::UpstreamClient>,
    pub token_manager: Arc<TokenManager>,
    pub request_timeout: u64,
    pub custom_mapping: Arc<tokio::sync::RwLock<std::collections::HashMap<String, String>>>,
    pub upstream_proxy: Arc<tokio::sync::RwLock<UpstreamProxyConfig>>,
    pub zai: Arc<tokio::sync::RwLock<crate::proxy::config::ZaiConfig>>,
    pub zai_vision_mcp: crate::proxy::zai_vision_mcp::ZaiVisionMcpState,
    pub experimental: Arc<tokio::sync::RwLock<crate::proxy::config::ExperimentalConfig>>,
    pub monitor: Arc<crate::proxy::monitor::ProxyMonitor>,
    // [NEW] Round-Robin counter for z.ai pooled dispatch
    pub provider_rr: Arc<std::sync::atomic::AtomicUsize>,
}

/// Reverse Proxy Server
pub struct AxumServer {
    shutdown_tx: Option<oneshot::Sender<()>>,
    pub custom_mapping: Arc<tokio::sync::RwLock<std::collections::HashMap<String, String>>>,
    pub proxy_state: Arc<tokio::sync::RwLock<UpstreamProxyConfig>>,
    pub security_state: Arc<crate::proxy::security::SecurityState>,
    pub zai_state: Arc<tokio::sync::RwLock<crate::proxy::config::ZaiConfig>>,
}

impl AxumServer {
    /// Start the server
    pub async fn start(
        host: String,
        port: u16,
        token_manager: Arc<TokenManager>,
        custom_mapping: std::collections::HashMap<String, String>,
        request_timeout: u64,
        upstream_proxy: UpstreamProxyConfig,
        security_config: crate::proxy::security::ProxySecurityConfig,
        zai_config: crate::proxy::config::ZaiConfig,
        monitor: Arc<crate::proxy::monitor::ProxyMonitor>,
        experimental_config: crate::proxy::config::ExperimentalConfig,
    ) -> Result<(Self, tokio::task::JoinHandle<()>), String> {

        let custom_mapping_state = Arc::new(tokio::sync::RwLock::new(custom_mapping));
        let proxy_state = Arc::new(tokio::sync::RwLock::new(upstream_proxy.clone()));
        let zai_state = Arc::new(tokio::sync::RwLock::new(zai_config));
        let experimental_state = Arc::new(tokio::sync::RwLock::new(experimental_config));

        // Create HTTP client via UpstreamClient
        // Note: UpstreamClient uses internal reqwest client which is not updated dynamically currently.
        let upstream = Arc::new(crate::proxy::upstream::client::UpstreamClient::new(Some(upstream_proxy.clone())));

        let state = AppState {
            upstream,
            token_manager,
            request_timeout,
            custom_mapping: custom_mapping_state.clone(),
            upstream_proxy: proxy_state.clone(),
            zai: zai_state.clone(),
            zai_vision_mcp: crate::proxy::zai_vision_mcp::ZaiVisionMcpState::new(),
            experimental: experimental_state.clone(),
            monitor,
            provider_rr: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        };

        // Initialize Security State
        let security_state = Arc::new(crate::proxy::security::SecurityState::new(
            None,
            security_config,
        ));

        // Build Router
        let app = Router::new()
            // OpenAI Protocol (Chat)
            .route("/v1/chat/completions", post(handlers::openai::handle_chat_completions))
            // OpenAI Protocol (Models)
            .route("/v1/models", get(handlers::openai::handle_list_models))
            // OpenAI Images API
            .route("/v1/images/generations", post(handlers::openai::handle_images_generations))
            .route("/v1/images/edits", post(handlers::openai::handle_images_edits))
            // OpenAI Audio API (Whisper)
            .route("/v1/audio/transcriptions", post(handlers::audio::handle_audio_transcription))
            // Legacy Completions (Copilot/Codex)
            .route("/v1/completions", post(handlers::openai::handle_completions))
            .route("/v1/engines/copilot-codex/completions", post(handlers::openai::handle_completions))
            // Anthropic Protocol (Messages)
            .route("/v1/messages", post(handlers::claude::handle_messages))
            .route("/v1/messages/count_tokens", post(handlers::claude::handle_count_tokens))
            // Anthropic Protocol (MCP) - Web Search & Reader
            .route("/mcp/web_search_prime/mcp", any(handlers::mcp::handle_web_search_prime))
            .route("/mcp/web_reader/mcp", any(handlers::mcp::handle_web_reader))
            // Anthropic Protocol (MCP) - z.ai Vision
            .route(
                "/mcp/zai-mcp-server/mcp",
                any(handlers::mcp::handle_zai_mcp_server),
            )
            // Gemini Protocol (Native)
            .route("/v1beta/models", get(handlers::gemini::handle_list_models))
            // Handle both GET (get info) and POST (generateContent with colon) at the same route
            .route(
                "/v1beta/models/:model",
                get(handlers::gemini::handle_get_model).post(handlers::gemini::handle_generate),
            )
            .route(
                "/v1beta/models/:model/countTokens",
                post(handlers::gemini::handle_count_tokens),
            ) // Specific route priority
            .route("/v1/models/detect", post(handlers::common::handle_detect_model))
            .route("/internal/warmup", post(handlers::warmup::handle_warmup)) // Internal warmup endpoint
            .route("/v1/api/event_logging/batch", post(silent_ok_handler))
            .route("/v1/api/event_logging", post(silent_ok_handler))
            .route("/healthz", get(health_check_handler))
            .layer(DefaultBodyLimit::max(100 * 1024 * 1024))
            .layer(axum::middleware::from_fn_with_state(state.clone(), crate::proxy::middleware::monitor::monitor_middleware))
            .layer(TraceLayer::new_for_http())
            .layer(axum::middleware::from_fn_with_state(
                security_state.clone(),
                crate::proxy::middleware::auth_middleware,
            ))
            .layer(crate::proxy::middleware::cors_layer())
            .with_state(state);

        // Bind address
        let addr = format!("{}:{}", host, port);
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .map_err(|e| format!("Address {} bind failed: {}", addr, e))?;

        tracing::info!("Reverse proxy server started at http://{}", addr);

        // Create shutdown channel
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();

        let server_instance = Self {
            shutdown_tx: Some(shutdown_tx),
            custom_mapping: custom_mapping_state.clone(),
            proxy_state,
            security_state,
            zai_state,
        };

        // Start server in new task
        let handle = tokio::spawn(async move {
            use hyper::server::conn::http1;
            use hyper_util::rt::TokioIo;
            use hyper_util::service::TowerToHyperService;

            loop {
                tokio::select! {
                    res = listener.accept() => {
                        match res {
                            Ok((stream, _)) => {
                                let io = TokioIo::new(stream);
                                let service = TowerToHyperService::new(app.clone());

                                tokio::task::spawn(async move {
                                    if let Err(err) = http1::Builder::new()
                                        .serve_connection(io, service)
                                        .with_upgrades() // Support WebSocket (if needed later)
                                        .await
                                    {
                                        debug!("Connection handled end or error: {:?}", err);
                                    }
                                });
                            }
                            Err(e) => {
                                error!("Accept connection failed: {:?}", e);
                            }
                        }
                    }
                    _ = &mut shutdown_rx => {
                        tracing::info!("Reverse proxy server stop listening");
                        break;
                    }
                }
            }
        });

        Ok((server_instance, handle))
    }

    /// Stop server
    pub fn stop(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }

    pub async fn update_mapping(&self, config: &ProxyConfig) {
        let mut mapping = self.custom_mapping.write().await;
        *mapping = config.custom_mapping.clone();
    }

    pub async fn update_proxy(&self, config: UpstreamProxyConfig) {
        let mut proxy = self.proxy_state.write().await;
        *proxy = config;
    }

    pub async fn update_security(&self, config: &ProxyConfig) {
         let mut security = self.security_state.config.write().await;
         *security = crate::proxy::security::ProxySecurityConfig::from_proxy_config(config);
    }

    pub async fn update_zai(&self, config: &ProxyConfig) {
        let mut zai = self.zai_state.write().await;
        *zai = config.zai.clone();
    }
}

// ===== API Handlers (Old code removed, taken over by src/proxy/handlers/*) =====

/// Health check handler
async fn health_check_handler() -> Response {
    Json(serde_json::json!({
        "status": "ok"
    }))
    .into_response()
}

/// Silent success handler (for intercepting telemetry logs, etc.)
async fn silent_ok_handler() -> Response {
    StatusCode::OK.into_response()
}
