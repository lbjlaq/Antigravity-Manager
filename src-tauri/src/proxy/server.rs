use crate::proxy::TokenManager;
use axum::{
    extract::DefaultBodyLimit,
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::{any, get, post},
    Router,
};
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use tokio::sync::oneshot;
use tokio::sync::RwLock;
use tower_http::trace::TraceLayer;
use tracing::{debug, error};

/// Axum 应用状态
#[derive(Clone)]
pub struct AppState {
    pub token_manager: Arc<TokenManager>,
    pub custom_mapping: Arc<tokio::sync::RwLock<std::collections::HashMap<String, String>>>,
    #[allow(dead_code)]
    pub request_timeout: u64, // API 请求超时(秒)
    #[allow(dead_code)]
    pub thought_signature_map: Arc<tokio::sync::Mutex<std::collections::HashMap<String, String>>>, // 思维链签名映射 (ID -> Signature)
    #[allow(dead_code)]
    pub upstream_proxy: Arc<tokio::sync::RwLock<crate::proxy::config::UpstreamProxyConfig>>,
    pub upstream: Arc<crate::proxy::upstream::client::UpstreamClient>,
    pub zai: Arc<RwLock<crate::proxy::ZaiConfig>>,
    pub provider_rr: Arc<AtomicUsize>,
    pub zai_vision_mcp: Arc<crate::proxy::zai_vision_mcp::ZaiVisionMcpState>,
    pub monitor: Arc<crate::proxy::monitor::ProxyMonitor>,
    pub experimental: Arc<RwLock<crate::proxy::config::ExperimentalConfig>>,
    /// Story-007-02: Gemini Image Generation Safety Settings
    pub safety_threshold: Arc<RwLock<Option<String>>>,
    /// Story-007-04: Image generation response cache
    pub image_cache: Option<Arc<dyn crate::proxy::cache::CacheBackend>>,
    /// Epic-008 Story-012-02: Budget optimizer with pattern learning
    pub budget_optimizer: Arc<crate::proxy::budget_optimizer::BudgetOptimizer>,
    /// Story-013-05: Response caching for thinking API responses
    pub response_cache: Option<Arc<crate::proxy::response_cache::ResponseCache>>,
}

/// Axum 服务器实例
pub struct AxumServer {
    shutdown_tx: Option<oneshot::Sender<()>>,
    custom_mapping: Arc<tokio::sync::RwLock<std::collections::HashMap<String, String>>>,
    proxy_state: Arc<tokio::sync::RwLock<crate::proxy::config::UpstreamProxyConfig>>,
    security_state: Arc<RwLock<crate::proxy::ProxySecurityConfig>>,
    zai_state: Arc<RwLock<crate::proxy::ZaiConfig>>,
}

impl AxumServer {
    /// Initialize response cache from config (Story-013-05)
    ///
    /// Creates LRU cache for thinking API responses based on configuration.
    /// Returns None if caching is disabled.
    async fn initialize_response_cache(
        experimental_config: &Arc<RwLock<crate::proxy::config::ExperimentalConfig>>,
    ) -> Option<Arc<crate::proxy::response_cache::ResponseCache>> {
        let config = experimental_config.read().await;
        let cache_config = &config.response_cache;

        if !cache_config.enabled {
            tracing::info!("[ResponseCache] Cache disabled via config");
            return None;
        }

        let cache = Arc::new(crate::proxy::response_cache::ResponseCache::new(
            cache_config.capacity,
            cache_config.ttl_seconds,
        ));

        tracing::info!(
            "[ResponseCache] ✓ Response cache enabled: capacity={}, ttl={}s",
            cache_config.capacity,
            cache_config.ttl_seconds
        );

        Some(cache)
    }

    /// Initialize image cache from environment variables (Story-007-04)
    ///
    /// Environment variables:
    /// - CACHE_BACKEND=none|filesystem|redis (default: none)
    /// - CACHE_TTL_SECONDS=3600 (default: 1 hour)
    /// - CACHE_MAX_SIZE_MB=100 (default: 100MB)
    /// - CACHE_DIR={data_dir}/image_cache/ (filesystem only)
    fn initialize_cache() -> Option<Arc<dyn crate::proxy::cache::CacheBackend>> {
        use crate::proxy::cache::{FilesystemCache, NoOpCache};

        let backend = std::env::var("CACHE_BACKEND").unwrap_or_else(|_| "none".to_string());

        match backend.as_str() {
            "filesystem" => {
                let cache_dir = std::env::var("CACHE_DIR")
                    .ok()
                    .and_then(|d| {
                        if d.is_empty() {
                            None
                        } else {
                            Some(std::path::PathBuf::from(d))
                        }
                    })
                    .unwrap_or_else(|| {
                        // Default: {data_dir}/image_cache/

                        dirs::data_local_dir()
                            .unwrap_or_else(|| std::path::PathBuf::from("."))
                            .join("com.lbjlaq.antigravity-tools")
                            .join("image_cache")
                    });

                let max_size_mb: u64 = std::env::var("CACHE_MAX_SIZE_MB")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(100);

                let ttl_secs: u64 = std::env::var("CACHE_TTL_SECONDS")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(3600);

                match FilesystemCache::new(
                    cache_dir.clone(),
                    max_size_mb,
                    std::time::Duration::from_secs(ttl_secs),
                ) {
                    Ok(cache) => {
                        tracing::info!(
                            "[Cache] ✓ Filesystem cache enabled: dir={:?}, max_size={}MB, ttl={}s",
                            cache_dir,
                            max_size_mb,
                            ttl_secs
                        );
                        Some(Arc::new(cache) as Arc<dyn crate::proxy::cache::CacheBackend>)
                    }
                    Err(e) => {
                        tracing::error!("[Cache] Failed to initialize filesystem cache: {}", e);
                        tracing::warn!("[Cache] Falling back to NoOp cache (disabled)");
                        Some(Arc::new(NoOpCache::new())
                            as Arc<dyn crate::proxy::cache::CacheBackend>)
                    }
                }
            }
            "redis" => {
                // TODO: Implement RedisCache (optional, Story-007-04)
                tracing::warn!("[Cache] Redis backend not yet implemented, falling back to NoOp");
                Some(Arc::new(NoOpCache::new()) as Arc<dyn crate::proxy::cache::CacheBackend>)
            }
            "none" => {
                tracing::debug!("[Cache] Cache disabled (backend=none)");
                None
            }
            other => {
                tracing::warn!("[Cache] Unknown backend '{}', cache disabled", other);
                None
            }
        }
    }

    pub async fn update_mapping(&self, config: &crate::proxy::config::ProxyConfig) {
        {
            let mut m = self.custom_mapping.write().await;
            *m = config.custom_mapping.clone();
        }
        tracing::debug!("模型映射 (Custom) 已全量热更新");
    }

    /// 更新代理配置
    pub async fn update_proxy(&self, new_config: crate::proxy::config::UpstreamProxyConfig) {
        let mut proxy = self.proxy_state.write().await;
        *proxy = new_config;
        tracing::info!("上游代理配置已热更新");
    }

    pub async fn update_security(&self, config: &crate::proxy::config::ProxyConfig) {
        let mut sec = self.security_state.write().await;
        *sec = crate::proxy::ProxySecurityConfig::from_proxy_config(config);
        tracing::info!("反代服务安全配置已热更新");
    }

    pub async fn update_zai(&self, config: &crate::proxy::config::ProxyConfig) {
        let mut zai = self.zai_state.write().await;
        *zai = config.zai.clone();
        tracing::info!("z.ai 配置已热更新");
    }
    /// 启动 Axum 服务器
    #[allow(clippy::too_many_arguments)]
    pub async fn start(
        host: String,
        port: u16,
        token_manager: Arc<TokenManager>,
        custom_mapping: std::collections::HashMap<String, String>,
        _request_timeout: u64,
        upstream_proxy: crate::proxy::config::UpstreamProxyConfig,
        security_config: crate::proxy::ProxySecurityConfig,
        zai_config: crate::proxy::ZaiConfig,
        monitor: Arc<crate::proxy::monitor::ProxyMonitor>,
        experimental_config: crate::proxy::config::ExperimentalConfig,
    ) -> Result<(Self, tokio::task::JoinHandle<()>), String> {
        let custom_mapping_state = Arc::new(tokio::sync::RwLock::new(custom_mapping));
        let proxy_state = Arc::new(tokio::sync::RwLock::new(upstream_proxy.clone()));
        let security_state = Arc::new(RwLock::new(security_config));
        let zai_state = Arc::new(RwLock::new(zai_config));
        let provider_rr = Arc::new(AtomicUsize::new(0));
        let zai_vision_mcp_state = Arc::new(crate::proxy::zai_vision_mcp::ZaiVisionMcpState::new());
        let experimental_state = Arc::new(RwLock::new(experimental_config));

        // Epic-008 Story-012-02: Initialize budget optimizer with pattern loading
        let budget_optimizer = Arc::new(crate::proxy::budget_optimizer::BudgetOptimizer::new());

        // Load saved patterns from database (graceful degradation on failure)
        match crate::modules::proxy_db::load_budget_patterns() {
            Ok(patterns) if !patterns.is_empty() => {
                match budget_optimizer.get_pattern_store().write() {
                    Ok(mut store) => {
                        store.load_from_db(patterns.clone());
                        tracing::info!(
                            "[Epic-008] ✓ Loaded {} budget patterns from database",
                            patterns.len()
                        );
                    }
                    Err(e) => tracing::warn!(
                        "[Epic-008] Failed to acquire pattern store lock: {}. Using defaults.",
                        e
                    ),
                }
            }
            Ok(_) => {
                tracing::info!("[Epic-008] No budget patterns in database (first run or empty)")
            }
            Err(e) => {
                tracing::warn!(
                    "[Epic-008] Failed to load budget patterns: {}. Using defaults.",
                    e
                );
            }
        }

        // Story-013-05: Initialize response cache from config before moving experimental_state
        let response_cache_opt = Self::initialize_response_cache(&experimental_state).await;

        let state = AppState {
            token_manager: token_manager.clone(),
            custom_mapping: custom_mapping_state.clone(),
            request_timeout: 300, // 5分钟超时
            thought_signature_map: Arc::new(tokio::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            upstream_proxy: proxy_state.clone(),
            upstream: Arc::new(crate::proxy::upstream::client::UpstreamClient::new(Some(
                upstream_proxy.clone(),
            ))),
            zai: zai_state.clone(),
            provider_rr: provider_rr.clone(),
            zai_vision_mcp: zai_vision_mcp_state,
            monitor: monitor.clone(),
            experimental: experimental_state,
            // Story-007-02: Initialize safety_threshold from environment variable
            // GEMINI_IMAGE_SAFETY_THRESHOLD can be set to OFF|LOW|MEDIUM|HIGH
            safety_threshold: Arc::new(RwLock::new(
                std::env::var("GEMINI_IMAGE_SAFETY_THRESHOLD").ok(),
            )),
            // Story-007-04: Initialize image cache from environment variables
            // CACHE_BACKEND=none|filesystem|redis (default: none)
            // CACHE_TTL_SECONDS=3600 (default: 1 hour)
            // CACHE_MAX_SIZE_MB=100 (default: 100MB)
            image_cache: Self::initialize_cache(),
            // Epic-008 Story-012-02: Budget optimizer with loaded patterns
            budget_optimizer: budget_optimizer.clone(),
            // Story-013-05: Response cache initialized above
            response_cache: response_cache_opt,
        };

        // 构建路由 - 使用新架构的 handlers！
        use crate::proxy::handlers;
        // 构建路由
        let app = Router::new()
            // OpenAI Protocol
            .route("/v1/models", get(handlers::openai::handle_list_models))
            .route(
                "/v1/chat/completions",
                post(handlers::openai::handle_chat_completions),
            )
            .route(
                "/v1/completions",
                post(handlers::openai::handle_completions),
            )
            .route("/v1/responses", post(handlers::openai::handle_completions)) // 兼容 Codex CLI
            .route(
                "/v1/images/generations",
                post(handlers::openai::handle_images_generations),
            ) // 图像生成 API
            .route(
                "/v1/images/edits",
                post(handlers::openai::handle_images_edits),
            ) // 图像编辑 API
            .route(
                "/v1/audio/transcriptions",
                post(handlers::audio::handle_audio_transcription),
            ) // 音频转录 API (PR #311)
            // Claude Protocol
            .route("/v1/messages", post(handlers::claude::handle_messages))
            .route(
                "/v1/messages/count_tokens",
                post(handlers::claude::handle_count_tokens),
            )
            .route(
                "/v1/models/claude",
                get(handlers::claude::handle_list_models),
            )
            // z.ai MCP (optional reverse-proxy)
            .route(
                "/mcp/web_search_prime/mcp",
                any(handlers::mcp::handle_web_search_prime),
            )
            .route("/mcp/web_reader/mcp", any(handlers::mcp::handle_web_reader))
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
            .route(
                "/v1/models/detect",
                post(handlers::common::handle_detect_model),
            )
            .route("/v1/api/event_logging/batch", post(silent_ok_handler))
            .route("/v1/api/event_logging", post(silent_ok_handler))
            .route("/healthz", get(health_check_handler))
            .layer(DefaultBodyLimit::max(100 * 1024 * 1024))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::proxy::middleware::monitor::monitor_middleware,
            ))
            .layer(TraceLayer::new_for_http())
            .layer(axum::middleware::from_fn_with_state(
                security_state.clone(),
                crate::proxy::middleware::auth_middleware,
            ))
            .layer(crate::proxy::middleware::cors_layer())
            .with_state(state);

        // 绑定地址
        let addr = format!("{}:{}", host, port);
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .map_err(|e| format!("地址 {} 绑定失败: {}", addr, e))?;

        tracing::info!("反代服务器启动在 http://{}", addr);

        // 创建关闭通道
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();

        let server_instance = Self {
            shutdown_tx: Some(shutdown_tx),
            custom_mapping: custom_mapping_state.clone(),
            proxy_state,
            security_state,
            zai_state,
        };

        // 在新任务中启动服务器
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
                                        .with_upgrades() // 支持 WebSocket (如果以后需要)
                                        .await
                                    {
                                        debug!("连接处理结束或出错: {:?}", err);
                                    }
                                });
                            }
                            Err(e) => {
                                error!("接收连接失败: {:?}", e);
                            }
                        }
                    }
                    _ = &mut shutdown_rx => {
                        tracing::info!("反代服务器停止监听");
                        break;
                    }
                }
            }
        });

        // Epic-008 Story-012-02: Start periodic pattern persistence task
        Self::start_pattern_persistence_task(budget_optimizer);

        Ok((server_instance, handle))
    }

    /// Epic-008 Story-012-02: Background task for periodic pattern persistence
    ///
    /// Saves budget patterns to database every 5 minutes.
    /// Runs indefinitely in background, with graceful error handling.
    fn start_pattern_persistence_task(
        optimizer: Arc<crate::proxy::budget_optimizer::BudgetOptimizer>,
    ) {
        use std::time::Duration;

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(300)); // 5 minutes

            loop {
                interval.tick().await;

                // Get all patterns from store
                let patterns = match optimizer.get_pattern_store().read() {
                    Ok(store) => store.get_all_patterns(),
                    Err(e) => {
                        tracing::error!(
                            "[Epic-008] Failed to read pattern store: {}. Skipping persistence.",
                            e
                        );
                        continue;
                    }
                };

                if patterns.is_empty() {
                    tracing::debug!("[Epic-008] No patterns to persist (empty store)");
                    continue;
                }

                // Save patterns to database
                let mut saved_count = 0;
                for pattern in patterns {
                    match crate::modules::proxy_db::save_budget_pattern(&pattern) {
                        Ok(_) => saved_count += 1,
                        Err(e) => tracing::warn!(
                            "[Epic-008] Failed to save pattern {}: {}",
                            &pattern.prompt_hash[..8],
                            e
                        ),
                    }
                }

                tracing::info!(
                    "[Epic-008] ✓ Persisted {} budget patterns to database",
                    saved_count
                );
            }
        });
    }

    /// 停止服务器
    pub fn stop(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

// ===== API 处理器 (旧代码已移除，由 src/proxy/handlers/* 接管) =====

/// 健康检查处理器
async fn health_check_handler() -> Response {
    Json(serde_json::json!({
        "status": "ok"
    }))
    .into_response()
}

/// 静默成功处理器 (用于拦截遥测日志等)
async fn silent_ok_handler() -> Response {
    StatusCode::OK.into_response()
}
