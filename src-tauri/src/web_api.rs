//! Web API 层 - 将 Tauri 命令封装为 HTTP REST API
//! 
//! 此模块提供独立运行的 Web 服务端 API，复用现有业务逻辑。

use axum::{
    extract::{Path, Query, State, rejection::JsonRejection, FromRequest, Request},
    http::StatusCode,
    response::{IntoResponse, Response, Json, Sse},
    routing::{delete, get, post, put},
    Router,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::sync::Arc;
use tokio::sync::RwLock;
use futures::stream::Stream;
use std::convert::Infallible;
use std::time::Duration;


use crate::models::{Account, AppConfig, QuotaData};
use crate::modules;
use crate::proxy::{ProxyConfig, TokenManager};
use crate::proxy::monitor::{ProxyMonitor, ProxyRequestLog, ProxyStats};

// ============================================================================
// 共享状态
// ============================================================================

/// Web API 共享状态
pub struct WebApiState {
    /// 反代服务实例
    pub proxy_instance: Arc<RwLock<Option<ProxyServiceInstance>>>,
    /// 监控器
    pub monitor: Arc<RwLock<Option<Arc<ProxyMonitor>>>>,
    /// SSE 广播通道
    pub sse_tx: tokio::sync::broadcast::Sender<SseEvent>,
}

/// 反代服务实例 (复用自 commands/proxy.rs)
pub struct ProxyServiceInstance {
    pub config: ProxyConfig,
    pub token_manager: Arc<TokenManager>,
    pub axum_server: crate::proxy::AxumServer,
    pub server_handle: tokio::task::JoinHandle<()>,
}

/// SSE 事件类型
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum SseEvent {
    ProxyRequest(ProxyRequestLog),
    ConfigUpdated,
    AccountSwitched,
}

impl WebApiState {
    pub fn new() -> Self {
        let (sse_tx, _) = tokio::sync::broadcast::channel(256);
        Self {
            proxy_instance: Arc::new(RwLock::new(None)),
            monitor: Arc::new(RwLock::new(None)),
            sse_tx,
        }
    }
}

// ============================================================================
// API 响应类型
// ============================================================================

#[derive(Serialize)]
struct ApiResponse<T> {
    success: bool,
    data: Option<T>,
    error: Option<String>,
}

impl<T: Serialize> ApiResponse<T> {
    fn ok(data: T) -> Json<Self> {
        Json(Self {
            success: true,
            data: Some(data),
            error: None,
        })
    }

    fn err(error: impl ToString) -> Json<Self> {
        Json(Self {
            success: false,
            data: None,
            error: Some(error.to_string()),
        })
    }
}

// ============================================================================
// 自定义 JSON 提取器 (返回 JSON 格式错误而非纯文本)
// ============================================================================


/// 自定义 JSON 提取器，确保反序列化错误也返回 JSON 格式
pub struct AppJson<T>(pub T);

#[axum::async_trait]
impl<S, T> FromRequest<S> for AppJson<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match axum::extract::Json::<T>::from_request(req, state).await {
            Ok(Json(value)) => Ok(AppJson(value)),
            Err(rejection) => {
                let error_message = match &rejection {
                    JsonRejection::JsonDataError(e) => format!("JSON 解析错误: {}", e),
                    JsonRejection::JsonSyntaxError(e) => format!("JSON 语法错误: {}", e),
                    JsonRejection::MissingJsonContentType(e) => format!("缺少 Content-Type: {}", e),
                    _ => format!("请求体错误: {}", rejection),
                };
                
                let body = serde_json::json!({
                    "success": false,
                    "data": null,
                    "error": error_message
                });
                
                Err((
                    StatusCode::BAD_REQUEST,
                    Json(body),
                ).into_response())
            }
        }
    }
}

// ============================================================================
// 路由构建
// ============================================================================


/// 创建 Web API 路由
pub fn create_api_router(state: Arc<WebApiState>) -> Router {
    Router::new()
        // 账号管理
        .route("/api/accounts", get(list_accounts))
        .route("/api/accounts", post(add_account))
        .route("/api/accounts/current", get(get_current_account))
        .route("/api/accounts/:id", delete(delete_account))
        .route("/api/accounts/batch-delete", post(delete_accounts))
        .route("/api/accounts/:id/switch", post(switch_account))
        .route("/api/accounts/:id/quota", post(fetch_account_quota))
        .route("/api/accounts/refresh-all", post(refresh_all_quotas))
        .route("/api/accounts/reorder", post(reorder_accounts))
        .route("/api/accounts/:id/proxy-status", post(toggle_proxy_status))
        // 配置
        .route("/api/config", get(load_config))
        .route("/api/config", put(save_config))
        // 反代服务
        .route("/api/proxy/start", post(start_proxy_service))
        .route("/api/proxy/stop", post(stop_proxy_service))
        .route("/api/proxy/status", get(get_proxy_status))
        .route("/api/proxy/stats", get(get_proxy_stats))
        .route("/api/proxy/logs", get(get_proxy_logs))
        .route("/api/proxy/logs", delete(clear_proxy_logs))
        .route("/api/proxy/monitor", post(set_proxy_monitor_enabled))
        .route("/api/proxy/reload-accounts", post(reload_proxy_accounts))
        .route("/api/proxy/model-mapping", put(update_model_mapping))
        .route("/api/proxy/scheduling", get(get_proxy_scheduling_config))
        .route("/api/proxy/scheduling", put(update_proxy_scheduling_config))
        .route("/api/proxy/sessions", delete(clear_proxy_session_bindings))
        .route("/api/proxy/zai-models", post(fetch_zai_models))
        .route("/api/proxy/generate-api-key", post(generate_api_key))
        // OAuth (Web 模式简化版)
        .route("/api/oauth/prepare-url", post(prepare_oauth_url))
        .route("/api/oauth/process-callback", post(process_oauth_callback))
        // 导入

        .route("/api/import/v1", post(import_v1_accounts))
        .route("/api/import/db", post(import_from_db))
        .route("/api/import/custom-db", post(import_custom_db))
        .route("/api/sync/db", post(sync_account_from_db))
        // 系统
        .route("/api/system/data-dir", get(get_data_dir_path))
        .route("/api/system/check-updates", get(check_for_updates))
        .route("/api/system/clear-logs", post(clear_log_cache))
        // SSE 事件流
        .route("/api/events", get(sse_handler))
        // 健康检查
        .route("/api/health", get(health_check))
        .with_state(state)
}

// ============================================================================
// 账号管理 API
// ============================================================================

async fn list_accounts(
    State(_state): State<Arc<WebApiState>>,
) -> impl IntoResponse {
    match modules::list_accounts() {
        Ok(accounts) => ApiResponse::ok(accounts),
        Err(e) => ApiResponse::<Vec<Account>>::err(e),
    }
}

#[derive(Deserialize)]
struct AddAccountRequest {
    email: String,
    refresh_token: String,
}

async fn add_account(
    State(state): State<Arc<WebApiState>>,
    AppJson(req): AppJson<AddAccountRequest>,
) -> impl IntoResponse {
    // 复用 commands/mod.rs 中的逻辑
    let result = async {
        // 1. 使用 refresh_token 获取 access_token
        let token_res = modules::oauth::refresh_access_token(&req.refresh_token).await?;

        // 2. 获取用户信息
        let user_info = modules::oauth::get_user_info(&token_res.access_token).await?;

        // 3. 构造 TokenData
        let token = crate::models::TokenData::new(
            token_res.access_token,
            req.refresh_token,
            token_res.expires_in,
            Some(user_info.email.clone()),
            None,
            None,
        );

        // 4. 添加或更新账号
        let account = modules::upsert_account(
            user_info.email.clone(),
            user_info.get_display_name(),
            token,
        )?;

        modules::logger::log_info(&format!("添加账号成功: {}", account.email));

        // 5. 如果反代服务正在运行，重新加载账号池
        reload_proxy_accounts_internal(&state).await;

        Ok::<_, String>(account)
    }
    .await;

    match result {
        Ok(account) => ApiResponse::ok(account),
        Err(e) => ApiResponse::<Account>::err(e),
    }
}

async fn get_current_account(
    State(_state): State<Arc<WebApiState>>,
) -> impl IntoResponse {
    let result = || -> Result<Option<Account>, String> {
        let account_id = modules::get_current_account_id()?;
        if let Some(id) = account_id {
            modules::load_account(&id).map(Some)
        } else {
            Ok(None)
        }
    };

    match result() {
        Ok(account) => ApiResponse::ok(account),
        Err(e) => ApiResponse::<Option<Account>>::err(e),
    }
}

async fn delete_account(
    State(state): State<Arc<WebApiState>>,
    Path(account_id): Path<String>,
) -> impl IntoResponse {
    match modules::delete_account(&account_id) {
        Ok(()) => {
            reload_proxy_accounts_internal(&state).await;
            ApiResponse::ok(())
        }
        Err(e) => ApiResponse::<()>::err(e),
    }
}

#[derive(Deserialize)]
struct DeleteAccountsRequest {
    account_ids: Vec<String>,
}

async fn delete_accounts(
    State(state): State<Arc<WebApiState>>,
    AppJson(req): AppJson<DeleteAccountsRequest>,
) -> impl IntoResponse {
    match modules::account::delete_accounts(&req.account_ids) {
        Ok(()) => {
            reload_proxy_accounts_internal(&state).await;
            ApiResponse::ok(())
        }
        Err(e) => ApiResponse::<()>::err(e),
    }
}

async fn switch_account(
    State(state): State<Arc<WebApiState>>,
    Path(account_id): Path<String>,
) -> impl IntoResponse {
    match modules::switch_account(&account_id).await {
        Ok(()) => {
            // 广播账号切换事件
            let _ = state.sse_tx.send(SseEvent::AccountSwitched);
            ApiResponse::ok(())
        }
        Err(e) => ApiResponse::<()>::err(e),
    }
}

async fn fetch_account_quota(
    State(_state): State<Arc<WebApiState>>,
    Path(account_id): Path<String>,
) -> impl IntoResponse {
    let result = async {
        let mut account = modules::load_account(&account_id)?;
        let quota = modules::account::fetch_quota_with_retry(&mut account)
            .await
            .map_err(|e| e.to_string())?;
        modules::update_account_quota(&account_id, quota.clone())?;
        Ok::<_, String>(quota)
    }
    .await;

    match result {
        Ok(quota) => ApiResponse::ok(quota),
        Err(e) => ApiResponse::<QuotaData>::err(e),
    }
}

#[derive(Serialize)]
struct RefreshStats {
    total: usize,
    success: usize,
    failed: usize,
    details: Vec<String>,
}

async fn refresh_all_quotas(
    State(_state): State<Arc<WebApiState>>,
) -> impl IntoResponse {
    let result = async {
        let accounts = modules::list_accounts()?;
        let mut success = 0;
        let mut failed = 0;
        let mut details = Vec::new();

        for mut account in accounts {
            if account.disabled {
                continue;
            }
            if let Some(ref q) = account.quota {
                if q.is_forbidden {
                    continue;
                }
            }

            match modules::account::fetch_quota_with_retry(&mut account).await {
                Ok(quota) => {
                    if modules::update_account_quota(&account.id, quota).is_ok() {
                        success += 1;
                    } else {
                        failed += 1;
                    }
                }
                Err(e) => {
                    failed += 1;
                    details.push(format!("{}: {}", account.email, e));
                }
            }
        }

        Ok::<_, String>(RefreshStats {
            total: success + failed,
            success,
            failed,
            details,
        })
    }
    .await;

    match result {
        Ok(stats) => ApiResponse::ok(stats),
        Err(e) => ApiResponse::<RefreshStats>::err(e),
    }
}

#[derive(Deserialize)]
struct ReorderRequest {
    account_ids: Vec<String>,
}

async fn reorder_accounts(
    State(_state): State<Arc<WebApiState>>,
    AppJson(req): AppJson<ReorderRequest>,
) -> impl IntoResponse {
    match modules::account::reorder_accounts(&req.account_ids) {
        Ok(()) => ApiResponse::ok(()),
        Err(e) => ApiResponse::<()>::err(e),
    }
}

#[derive(Deserialize)]
struct ToggleProxyStatusRequest {
    enable: bool,
    reason: Option<String>,
}

async fn toggle_proxy_status(
    State(state): State<Arc<WebApiState>>,
    Path(account_id): Path<String>,
    AppJson(req): AppJson<ToggleProxyStatusRequest>,
) -> impl IntoResponse {
    // 简化版：直接修改账号文件
    let result = || -> Result<(), String> {
        let data_dir = modules::account::get_data_dir()?;
        let account_path = data_dir.join("accounts").join(format!("{}.json", account_id));

        if !account_path.exists() {
            return Err(format!("账号文件不存在: {}", account_id));
        }

        let content = std::fs::read_to_string(&account_path)
            .map_err(|e| format!("读取账号文件失败: {}", e))?;

        let mut account_json: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| format!("解析账号文件失败: {}", e))?;

        if req.enable {
            account_json["proxy_disabled"] = serde_json::Value::Bool(false);
            account_json["proxy_disabled_reason"] = serde_json::Value::Null;
            account_json["proxy_disabled_at"] = serde_json::Value::Null;
        } else {
            let now = chrono::Utc::now().timestamp();
            account_json["proxy_disabled"] = serde_json::Value::Bool(true);
            account_json["proxy_disabled_at"] = serde_json::Value::Number(now.into());
            account_json["proxy_disabled_reason"] = serde_json::Value::String(
                req.reason.unwrap_or_else(|| "用户手动禁用".to_string()),
            );
        }

        std::fs::write(&account_path, serde_json::to_string_pretty(&account_json).unwrap())
            .map_err(|e| format!("写入账号文件失败: {}", e))?;

        Ok(())
    };

    match result() {
        Ok(()) => {
            reload_proxy_accounts_internal(&state).await;
            ApiResponse::ok(())
        }
        Err(e) => ApiResponse::<()>::err(e),
    }
}

// ============================================================================
// 配置 API
// ============================================================================

async fn load_config(
    State(_state): State<Arc<WebApiState>>,
) -> impl IntoResponse {
    match modules::load_app_config() {
        Ok(config) => ApiResponse::ok(config),
        Err(e) => ApiResponse::<AppConfig>::err(e),
    }
}

async fn save_config(
    State(state): State<Arc<WebApiState>>,
    AppJson(config): AppJson<AppConfig>,
) -> impl IntoResponse {
    match modules::save_app_config(&config) {
        Ok(()) => {
            // 广播配置更新事件
            let _ = state.sse_tx.send(SseEvent::ConfigUpdated);

            // 热更新正在运行的反代服务
            let instance_lock = state.proxy_instance.read().await;
            if let Some(instance) = instance_lock.as_ref() {
                instance.axum_server.update_mapping(&config.proxy).await;
                instance
                    .axum_server
                    .update_proxy(config.proxy.upstream_proxy.clone())
                    .await;
                instance.axum_server.update_security(&config.proxy).await;
                instance.axum_server.update_zai(&config.proxy).await;
            }

            ApiResponse::ok(())
        }
        Err(e) => ApiResponse::<()>::err(e),
    }
}

// ============================================================================
// 反代服务 API
// ============================================================================

#[derive(Serialize)]
struct ProxyStatus {
    running: bool,
    port: u16,
    base_url: String,
    active_accounts: usize,
}

async fn start_proxy_service(
    State(state): State<Arc<WebApiState>>,
    AppJson(config): AppJson<ProxyConfig>,
) -> impl IntoResponse {
    let mut instance_lock = state.proxy_instance.write().await;

    if instance_lock.is_some() {
        return ApiResponse::<ProxyStatus>::err("服务已在运行中");
    }

    // 确保 monitor 存在
    {
        let mut monitor_lock = state.monitor.write().await;
        if monitor_lock.is_none() {
            // Web 模式下创建不带 app_handle 的 monitor
            *monitor_lock = Some(Arc::new(ProxyMonitor::new(1000, None)));
        }
        if let Some(monitor) = monitor_lock.as_ref() {
            monitor.set_enabled(config.enable_logging);
        }
    }

    let monitor = state.monitor.read().await.as_ref().unwrap().clone();

    // 初始化 Token 管理器
    let app_data_dir = match modules::account::get_data_dir() {
        Ok(dir) => dir,
        Err(e) => return ApiResponse::<ProxyStatus>::err(e),
    };
    let _ = modules::account::get_accounts_dir();

    let token_manager = Arc::new(TokenManager::new(app_data_dir.clone()));
    token_manager
        .update_sticky_config(config.scheduling.clone())
        .await;

    // 加载账号
    let active_accounts = match token_manager.load_accounts().await {
        Ok(count) => count,
        Err(e) => return ApiResponse::<ProxyStatus>::err(format!("加载账号失败: {}", e)),
    };

    if active_accounts == 0 {
        let zai_enabled = config.zai.enabled
            && !matches!(
                config.zai.dispatch_mode,
                crate::proxy::ZaiDispatchMode::Off
            );
        if !zai_enabled {
            return ApiResponse::<ProxyStatus>::err("没有可用账号，请先添加账号");
        }
    }

    // 启动 Axum 服务器
    let result = crate::proxy::AxumServer::start(
        config.get_bind_address().to_string(),
        config.port,
        token_manager.clone(),
        config.anthropic_mapping.clone(),
        config.openai_mapping.clone(),
        config.custom_mapping.clone(),
        config.request_timeout,
        config.upstream_proxy.clone(),
        crate::proxy::ProxySecurityConfig::from_proxy_config(&config),
        config.zai.clone(),
        monitor.clone(),
    )
    .await;

    match result {
        Ok((axum_server, server_handle)) => {
            let instance = ProxyServiceInstance {
                config: config.clone(),
                token_manager,
                axum_server,
                server_handle,
            };

            *instance_lock = Some(instance);

            // 保存配置
            if let Ok(mut app_config) = modules::config::load_app_config() {
                app_config.proxy = config.clone();
                let _ = modules::config::save_app_config(&app_config);
            }

            ApiResponse::ok(ProxyStatus {
                running: true,
                port: config.port,
                base_url: format!("http://127.0.0.1:{}", config.port),
                active_accounts,
            })
        }
        Err(e) => ApiResponse::<ProxyStatus>::err(format!("启动服务器失败: {}", e)),
    }
}

async fn stop_proxy_service(
    State(state): State<Arc<WebApiState>>,
) -> impl IntoResponse {
    let mut instance_lock = state.proxy_instance.write().await;

    if instance_lock.is_none() {
        return ApiResponse::<()>::err("服务未运行");
    }

    if let Some(instance) = instance_lock.take() {
        instance.axum_server.stop();
        instance.server_handle.await.ok();
    }

    ApiResponse::ok(())
}

async fn get_proxy_status(
    State(state): State<Arc<WebApiState>>,
) -> impl IntoResponse {
    let instance_lock = state.proxy_instance.read().await;

    match instance_lock.as_ref() {
        Some(instance) => ApiResponse::ok(ProxyStatus {
            running: true,
            port: instance.config.port,
            base_url: format!("http://127.0.0.1:{}", instance.config.port),
            active_accounts: instance.token_manager.len(),
        }),
        None => ApiResponse::ok(ProxyStatus {
            running: false,
            port: 0,
            base_url: String::new(),
            active_accounts: 0,
        }),
    }
}

async fn get_proxy_stats(
    State(state): State<Arc<WebApiState>>,
) -> impl IntoResponse {
    let monitor_lock = state.monitor.read().await;
    if let Some(monitor) = monitor_lock.as_ref() {
        ApiResponse::ok(monitor.get_stats().await)
    } else {
        ApiResponse::ok(ProxyStats::default())
    }
}

#[derive(Deserialize)]
struct LogsQuery {
    limit: Option<usize>,
}

async fn get_proxy_logs(
    State(state): State<Arc<WebApiState>>,
    Query(query): Query<LogsQuery>,
) -> impl IntoResponse {
    let monitor_lock = state.monitor.read().await;
    if let Some(monitor) = monitor_lock.as_ref() {
        ApiResponse::ok(monitor.get_logs(query.limit.unwrap_or(100)).await)
    } else {
        ApiResponse::ok(Vec::<ProxyRequestLog>::new())
    }
}

async fn clear_proxy_logs(
    State(state): State<Arc<WebApiState>>,
) -> impl IntoResponse {
    let monitor_lock = state.monitor.read().await;
    if let Some(monitor) = monitor_lock.as_ref() {
        monitor.clear().await;
    }
    ApiResponse::ok(())
}

#[derive(Deserialize)]
struct SetMonitorRequest {
    enabled: bool,
}

async fn set_proxy_monitor_enabled(
    State(state): State<Arc<WebApiState>>,
    AppJson(req): AppJson<SetMonitorRequest>,
) -> impl IntoResponse {
    let monitor_lock = state.monitor.read().await;
    if let Some(monitor) = monitor_lock.as_ref() {
        monitor.set_enabled(req.enabled);
    }
    ApiResponse::ok(())
}

async fn reload_proxy_accounts(
    State(state): State<Arc<WebApiState>>,
) -> impl IntoResponse {
    let instance_lock = state.proxy_instance.read().await;

    if let Some(instance) = instance_lock.as_ref() {
        match instance.token_manager.load_accounts().await {
            Ok(count) => ApiResponse::ok(count),
            Err(e) => ApiResponse::<usize>::err(format!("重新加载账号失败: {}", e)),
        }
    } else {
        ApiResponse::<usize>::err("服务未运行")
    }
}

/// 内部辅助函数：重新加载账号池
async fn reload_proxy_accounts_internal(state: &WebApiState) {
    let instance_lock = state.proxy_instance.read().await;
    if let Some(instance) = instance_lock.as_ref() {
        let _ = instance.token_manager.load_accounts().await;
    }
}

async fn update_model_mapping(
    State(state): State<Arc<WebApiState>>,
    AppJson(config): AppJson<ProxyConfig>,
) -> impl IntoResponse {
    let instance_lock = state.proxy_instance.read().await;
    if let Some(instance) = instance_lock.as_ref() {
        instance.axum_server.update_mapping(&config).await;
    }

    // 保存到配置
    if let Ok(mut app_config) = modules::config::load_app_config() {
        app_config.proxy.anthropic_mapping = config.anthropic_mapping;
        app_config.proxy.openai_mapping = config.openai_mapping;
        app_config.proxy.custom_mapping = config.custom_mapping;
        let _ = modules::config::save_app_config(&app_config);
    }

    ApiResponse::ok(())
}

async fn get_proxy_scheduling_config(
    State(state): State<Arc<WebApiState>>,
) -> impl IntoResponse {
    let instance_lock = state.proxy_instance.read().await;
    if let Some(instance) = instance_lock.as_ref() {
        ApiResponse::ok(instance.token_manager.get_sticky_config().await)
    } else {
        ApiResponse::ok(crate::proxy::sticky_config::StickySessionConfig::default())
    }
}

async fn update_proxy_scheduling_config(
    State(state): State<Arc<WebApiState>>,
    AppJson(config): AppJson<crate::proxy::sticky_config::StickySessionConfig>,
) -> impl IntoResponse {
    let instance_lock = state.proxy_instance.read().await;
    if let Some(instance) = instance_lock.as_ref() {
        instance.token_manager.update_sticky_config(config).await;
        ApiResponse::ok(())
    } else {
        ApiResponse::<()>::err("服务未运行")
    }
}

async fn clear_proxy_session_bindings(
    State(state): State<Arc<WebApiState>>,
) -> impl IntoResponse {
    let instance_lock = state.proxy_instance.read().await;
    if let Some(instance) = instance_lock.as_ref() {
        instance.token_manager.clear_all_sessions();
        ApiResponse::ok(())
    } else {
        ApiResponse::<()>::err("服务未运行")
    }
}

#[derive(Deserialize)]
struct FetchZaiModelsRequest {
    zai: crate::proxy::ZaiConfig,
    upstream_proxy: crate::proxy::config::UpstreamProxyConfig,
    request_timeout: u64,
}

// Helper functions for fetch_zai_models (inlined from commands/proxy.rs)
fn join_base_url(base: &str, path: &str) -> String {
    let base = base.trim_end_matches('/');
    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{}", path)
    };
    format!("{}{}", base, path)
}

fn extract_model_ids(value: &serde_json::Value) -> Vec<String> {
    let mut out = Vec::new();

    fn push_from_item(out: &mut Vec<String>, item: &serde_json::Value) {
        match item {
            serde_json::Value::String(s) => out.push(s.to_string()),
            serde_json::Value::Object(map) => {
                if let Some(id) = map.get("id").and_then(|v| v.as_str()) {
                    out.push(id.to_string());
                } else if let Some(name) = map.get("name").and_then(|v| v.as_str()) {
                    out.push(name.to_string());
                }
            }
            _ => {}
        }
    }

    match value {
        serde_json::Value::Array(arr) => {
            for item in arr {
                push_from_item(&mut out, item);
            }
        }
        serde_json::Value::Object(map) => {
            if let Some(data) = map.get("data") {
                if let serde_json::Value::Array(arr) = data {
                    for item in arr {
                        push_from_item(&mut out, item);
                    }
                }
            }
            if let Some(models) = map.get("models") {
                match models {
                    serde_json::Value::Array(arr) => {
                        for item in arr {
                            push_from_item(&mut out, item);
                        }
                    }
                    other => push_from_item(&mut out, other),
                }
            }
        }
        _ => {}
    }

    out
}

async fn fetch_zai_models(
    State(_state): State<Arc<WebApiState>>,
    AppJson(req): AppJson<FetchZaiModelsRequest>,
) -> impl IntoResponse {
    let result = async {
        if req.zai.base_url.trim().is_empty() {
            return Err("z.ai base_url is empty".to_string());
        }
        if req.zai.api_key.trim().is_empty() {
            return Err("z.ai api_key is not set".to_string());
        }

        let url = join_base_url(&req.zai.base_url, "/v1/models");

        let mut builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(req.request_timeout.max(5)));
        if req.upstream_proxy.enabled && !req.upstream_proxy.url.is_empty() {
            let proxy = reqwest::Proxy::all(&req.upstream_proxy.url)
                .map_err(|e| format!("Invalid upstream proxy url: {}", e))?;
            builder = builder.proxy(proxy);
        }
        let client = builder
            .build()
            .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

        let resp = client
            .get(&url)
            .header("Authorization", format!("Bearer {}", req.zai.api_key))
            .header("x-api-key", &req.zai.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("accept", "application/json")
            .send()
            .await
            .map_err(|e| format!("Upstream request failed: {}", e))?;

        let status = resp.status();
        let text = resp.text().await.map_err(|e| format!("Failed to read response: {}", e))?;

        if !status.is_success() {
            let preview = if text.len() > 4000 { &text[..4000] } else { &text };
            return Err(format!("Upstream returned {}: {}", status, preview));
        }

        let json: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| format!("Invalid JSON response: {}", e))?;
        let mut models = extract_model_ids(&json);
        models.retain(|s| !s.trim().is_empty());
        models.sort();
        models.dedup();
        Ok(models)
    }
    .await;

    match result {
        Ok(models) => ApiResponse::ok(models),
        Err(e) => ApiResponse::<Vec<String>>::err(e),
    }
}


async fn generate_api_key(
    State(_state): State<Arc<WebApiState>>,
) -> impl IntoResponse {
    ApiResponse::ok(format!("sk-{}", uuid::Uuid::new_v4().simple()))
}

// ============================================================================
// OAuth API (简化版)
// ============================================================================

/// OAuth URL 响应
#[derive(Serialize)]
struct OAuthUrlResponse {
    url: String,
    redirect_uri: String,
}

async fn prepare_oauth_url(
    State(_state): State<Arc<WebApiState>>,
) -> impl IntoResponse {
    // Web 模式下返回 OAuth URL，由用户手动在浏览器中打开
    // 使用固定的 redirect_uri (用户需要手动复制回调 URL)
    let redirect_uri = "http://localhost:9004/callback".to_string();
    let url = modules::oauth::get_auth_url(&redirect_uri);
    
    ApiResponse::ok(OAuthUrlResponse {
        url,
        redirect_uri,
    })
}

/// 处理手动粘贴的 OAuth 回调 URL
#[derive(Deserialize)]
struct ProcessCallbackRequest {
    callback_url: String,
}

async fn process_oauth_callback(
    State(state): State<Arc<WebApiState>>,
    AppJson(req): AppJson<ProcessCallbackRequest>,
) -> impl IntoResponse {
    let result = async {
        // 1. 解析回调 URL 中的 code 参数
        let url = url::Url::parse(&req.callback_url)
            .map_err(|e| format!("无效的回调 URL: {}", e))?;
        
        let code = url.query_pairs()
            .find(|(k, _)| k == "code")
            .map(|(_, v)| v.to_string())
            .ok_or_else(|| "回调 URL 中未找到 code 参数".to_string())?;
        
        // 获取 redirect_uri (从 URL 中提取 scheme://host:port/path)
        let redirect_uri = format!(
            "{}://{}{}",
            url.scheme(),
            url.host_str().unwrap_or("localhost"),
            if let Some(port) = url.port() { format!(":{}", port) } else { String::new() }
        ) + url.path();
        
        // 2. 使用 code 交换 token
        let token_res = modules::oauth::exchange_code(&code, &redirect_uri).await?;
        
        // 3. 检查是否返回了 refresh_token
        let refresh_token = token_res.refresh_token.ok_or_else(|| {
            "OAuth 未返回 Refresh Token。可能原因：\n\
             1. 此 Google 账号之前已授权过此应用\n\
             2. 请访问 https://myaccount.google.com/permissions 撤销授权后重试"
                .to_string()
        })?;
        
        // 4. 获取用户信息
        let user_info = modules::oauth::get_user_info(&token_res.access_token).await?;
        
        // 5. 构造 TokenData 并保存账号
        let token_data = crate::models::TokenData::new(
            token_res.access_token,
            refresh_token,
            token_res.expires_in,
            Some(user_info.email.clone()),  // email: Option<String>
            None,  // project_id
            None,  // session_id
        );
        
        // 6. 创建并保存账号
        let account_id = uuid::Uuid::new_v4().to_string();
        let mut account = crate::models::Account::new(
            account_id,
            user_info.email.clone(),
            token_data,
        );
        account.name = user_info.get_display_name();

        
        modules::account::save_account(&account)?;
        let _ = modules::account::set_current_account_id(&account.id);
        
        // 7. 重新加载反代账号
        reload_proxy_accounts_internal(&state).await;
        
        Ok::<_, String>(account)
    }.await;
    
    match result {
        Ok(account) => ApiResponse::ok(account),
        Err(e) => ApiResponse::<Account>::err(e),
    }
}

// ============================================================================
// 导入 API
// ============================================================================


async fn import_v1_accounts(
    State(_state): State<Arc<WebApiState>>,
) -> impl IntoResponse {
    match modules::migration::import_from_v1().await {
        Ok(accounts) => ApiResponse::ok(accounts),
        Err(e) => ApiResponse::<Vec<Account>>::err(e),
    }
}

async fn import_from_db(
    State(state): State<Arc<WebApiState>>,
) -> impl IntoResponse {
    match modules::migration::import_from_db().await {
        Ok(mut account) => {
            // 设为当前账号
            let _ = modules::account::set_current_account_id(&account.id);
            reload_proxy_accounts_internal(&state).await;
            ApiResponse::ok(account)
        }
        Err(e) => ApiResponse::<Account>::err(e),
    }
}

#[derive(Deserialize)]
struct ImportCustomDbRequest {
    path: String,
}

async fn import_custom_db(
    State(state): State<Arc<WebApiState>>,
    AppJson(req): AppJson<ImportCustomDbRequest>,
) -> impl IntoResponse {
    match modules::migration::import_from_custom_db_path(req.path).await {
        Ok(mut account) => {
            let _ = modules::account::set_current_account_id(&account.id);
            reload_proxy_accounts_internal(&state).await;
            ApiResponse::ok(account)
        }
        Err(e) => ApiResponse::<Account>::err(e),
    }
}

async fn sync_account_from_db(
    State(state): State<Arc<WebApiState>>,
) -> impl IntoResponse {
    let result = async {
        let db_refresh_token = modules::migration::get_refresh_token_from_db()?;
        let curr_account = modules::account::get_current_account()?;

        if let Some(acc) = curr_account {
            if acc.token.refresh_token == db_refresh_token {
                return Ok(None);
            }
        }

        let account = modules::migration::import_from_db().await?;
        let _ = modules::account::set_current_account_id(&account.id);
        Ok::<_, String>(Some(account))
    }
    .await;

    match result {
        Ok(account) => {
            if account.is_some() {
                reload_proxy_accounts_internal(&state).await;
            }
            ApiResponse::ok(account)
        }
        Err(e) => ApiResponse::<Option<Account>>::err(e),
    }
}

// ============================================================================
// 系统 API
// ============================================================================

async fn get_data_dir_path(
    State(_state): State<Arc<WebApiState>>,
) -> impl IntoResponse {
    match modules::account::get_data_dir() {
        Ok(path) => ApiResponse::ok(path.to_string_lossy().to_string()),
        Err(e) => ApiResponse::<String>::err(e),
    }
}

#[derive(Serialize)]
struct UpdateInfo {
    has_update: bool,
    latest_version: String,
    current_version: String,
    download_url: String,
}

async fn check_for_updates(
    State(_state): State<Arc<WebApiState>>,
) -> impl IntoResponse {
    const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
    const GITHUB_API_URL: &str =
        "https://api.github.com/repos/lbjlaq/Antigravity-Manager/releases/latest";

    let result = async {
        let client = crate::utils::http::create_client(15);
        let response = client
            .get(GITHUB_API_URL)
            .header("User-Agent", "Antigravity-Tools")
            .send()
            .await
            .map_err(|e| format!("请求失败: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("GitHub API 返回错误: {}", response.status()));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("解析响应失败: {}", e))?;

        let latest_version = json["tag_name"]
            .as_str()
            .ok_or("无法获取版本号")?
            .trim_start_matches('v');

        let download_url = json["html_url"]
            .as_str()
            .unwrap_or("https://github.com/lbjlaq/Antigravity-Manager/releases")
            .to_string();

        let has_update = compare_versions(latest_version, CURRENT_VERSION);

        Ok(UpdateInfo {
            has_update,
            latest_version: format!("v{}", latest_version),
            current_version: format!("v{}", CURRENT_VERSION),
            download_url,
        })
    }
    .await;

    match result {
        Ok(info) => ApiResponse::ok(info),
        Err(e) => ApiResponse::<UpdateInfo>::err(e),
    }
}

fn compare_versions(latest: &str, current: &str) -> bool {
    let parse_version =
        |v: &str| -> Vec<u32> { v.split('.').filter_map(|s| s.parse::<u32>().ok()).collect() };

    let latest_parts = parse_version(latest);
    let current_parts = parse_version(current);

    for i in 0..3 {
        let l = latest_parts.get(i).unwrap_or(&0);
        let c = current_parts.get(i).unwrap_or(&0);
        if l > c {
            return true;
        } else if l < c {
            return false;
        }
    }

    false
}

async fn clear_log_cache(
    State(_state): State<Arc<WebApiState>>,
) -> impl IntoResponse {
    match modules::logger::clear_logs() {
        Ok(()) => ApiResponse::ok(()),
        Err(e) => ApiResponse::<()>::err(e),
    }
}

// ============================================================================
// SSE 事件流
// ============================================================================

async fn sse_handler(
    State(state): State<Arc<WebApiState>>,
) -> Sse<impl Stream<Item = Result<axum::response::sse::Event, Infallible>>> {
    let rx = state.sse_tx.subscribe();

    let stream = async_stream::stream! {
        let mut rx = rx;
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let data = serde_json::to_string(&event).unwrap_or_default();
                    yield Ok(axum::response::sse::Event::default().data(data));
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            }
        }
    };

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(30))
            .text("ping"),
    )
}

// ============================================================================
// 健康检查
// ============================================================================

async fn health_check() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "mode": "web"
    }))
}
