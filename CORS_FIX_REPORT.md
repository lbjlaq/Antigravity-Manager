# 🐛 Bug报告：在浏览器环境中使用API时出现CORS错误

## 问题描述

在基于浏览器的客户端（如Droid）中使用反代API时会报CORS跨域错误，但在Claude Code CLI中使用相同的API则完全正常。

## 环境信息

- **版本**: v3.3.0
- **平台**: macOS (aarch64)
- **问题出现环境**: Droid及其他浏览器环境
- **正常工作环境**: Claude Code CLI

## 重现步骤

1. 启动反代服务
2. 在Droid或其他浏览器环境的客户端中配置API端点
3. 尝试调用任何API接口（如 `/v1/chat/completions`）
4. 浏览器控制台显示CORS错误

## 根本原因分析

浏览器在发送跨域请求前会先发送 **OPTIONS 预检请求** 来检查服务器的CORS策略。问题出在三个方面：

1. **CORS配置不完整** - `cors.rs` 中只使用了 `Any`，没有明确配置所有必要的CORS响应头
2. **中间件顺序错误** - CORS层在最外层，导致其他中间件可能先拦截请求
3. **认证中间件拦截OPTIONS** - auth中间件没有豁免OPTIONS预检请求，导致CORS检查失败

## 解决方案

需要修改以下三个文件：

### 1. `src-tauri/src/proxy/middleware/cors.rs`

#### 修改前：

```rust
// CORS 中间件
use tower_http::cors::{CorsLayer, Any};

/// 创建 CORS layer
pub fn cors_layer() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
}
```

#### 修改后：

```rust
// CORS 中间件
use tower_http::cors::{CorsLayer, Any};
use axum::http::Method;

/// 创建 CORS layer
pub fn cors_layer() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::HEAD,
            Method::OPTIONS,
            Method::PATCH,
        ])
        .allow_headers(Any)
        .allow_credentials(false)
        .max_age(std::time::Duration::from_secs(3600))
}
```

#### 改动说明：

- 明确列出所有允许的HTTP方法（特别是 `OPTIONS`）
- 添加 `allow_credentials(false)` 避免跨域凭证问题
- 设置 `max_age(3600)` 减少预检请求频率

---

### 2. `src-tauri/src/proxy/middleware/auth.rs`

#### 修改前：

```rust
// API Key 认证中间件
use axum::{
    extract::Request,
    http::{header, StatusCode},
    middleware::Next,
    response::Response,
};

/// API Key 认证中间件
pub async fn auth_middleware(request: Request, next: Next) -> Result<Response, StatusCode> {
    // Log the request method and URI
    tracing::info!("Request: {} {}", request.method(), request.uri());

    // 从 header 中提取 API key
    let api_key = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .or_else(|| {
            request
                .headers()
                .get("x-api-key")
                .and_then(|h| h.to_str().ok())
        });

    // TODO: 实际验证 API key
    // 目前暂时允许所有请求通过
    if api_key.is_some() || true {
        Ok(next.run(request).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}
```

#### 修改后：

```rust
// API Key 认证中间件
use axum::{
    extract::Request,
    http::{header, StatusCode, Method},
    middleware::Next,
    response::Response,
};

/// API Key 认证中间件
pub async fn auth_middleware(request: Request, next: Next) -> Result<Response, StatusCode> {
    // Log the request method and URI
    tracing::info!("Request: {} {}", request.method(), request.uri());

    // 允许 OPTIONS 预检请求直接通过(用于CORS)
    if request.method() == Method::OPTIONS {
        return Ok(next.run(request).await);
    }

    // 从 header 中提取 API key
    let api_key = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .or_else(|| {
            request
                .headers()
                .get("x-api-key")
                .and_then(|h| h.to_str().ok())
        });

    // TODO: 实际验证 API key
    // 目前暂时允许所有请求通过
    if api_key.is_some() || true {
        Ok(next.run(request).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}
```

#### 改动说明：

- 添加 `Method` 导入
- 在认证逻辑之前检查是否为OPTIONS请求
- OPTIONS请求直接放行，不进行认证检查

---

### 3. `src-tauri/src/proxy/server.rs` (约第89-114行)

#### 修改前：

```rust
let app = Router::new()
    // ... 路由定义 ...
    .layer(DefaultBodyLimit::max(100 * 1024 * 1024))
    .layer(TraceLayer::new_for_http())
    .layer(axum::middleware::from_fn(crate::proxy::middleware::auth_middleware))
    .layer(crate::proxy::middleware::cors_layer())
    .with_state(state);
```

#### 修改后：

```rust
let app = Router::new()
    // ... 路由定义 ...
    .layer(DefaultBodyLimit::max(100 * 1024 * 1024))
    .layer(crate::proxy::middleware::cors_layer())
    .layer(TraceLayer::new_for_http())
    .layer(axum::middleware::from_fn(crate::proxy::middleware::auth_middleware))
    .with_state(state);
```

#### 改动说明：

- 将 `cors_layer()` 从最外层移到内层
- 调整顺序为: DefaultBodyLimit → **CORS** → TraceLayer → Auth
- 这样CORS能优先处理响应头，确保跨域请求的响应头正确设置

---

## 技术原理

Axum的middleware执行顺序是"洋葱模型"：

- **请求流向**: 外层 → 内层 → 处理器
- **响应流向**: 处理器 → 内层 → 外层

修改后的顺序确保：

1. CORS层能够在响应返回时优先添加必要的响应头
2. OPTIONS预检请求不会被认证中间件拦截
3. 所有CORS相关的HTTP方法都被明确允许

## 测试结果

修复后：

- ✅ 在Droid中可以正常调用API
- ✅ 在Claude Code CLI中继续正常工作
- ✅ 其他浏览器环境的客户端也能正常使用
- ✅ OPTIONS预检请求返回正确的CORS响应头

## 相关文件清单

```
src-tauri/src/proxy/middleware/cors.rs
src-tauri/src/proxy/middleware/auth.rs
src-tauri/src/proxy/server.rs
```

## 建议

建议将这些修复合并到主分支，以便所有用户都能在浏览器环境中正常使用反代服务。

---

**报告生成时间**: 2025-12-31
**修复版本**: v3.3.0 (已测试通过)
