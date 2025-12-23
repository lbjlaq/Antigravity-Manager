//! 静态文件服务

use axum::routing::Router;
use tower_http::services::{ServeDir, ServeFile};

/// 创建静态文件服务
pub fn service() -> Router {
    // 默认静态文件目录
    let static_dir = std::env::var("ANTIGRAVITY_STATIC_DIR")
        .unwrap_or_else(|_| "./static".to_string());
    
    let index_path = format!("{}/index.html", static_dir);
    
    // 检查静态目录是否存在
    if !std::path::Path::new(&static_dir).exists() {
        tracing::warn!("静态文件目录不存在: {}", static_dir);
        // 返回空路由
        return Router::new();
    }
    
    // SPA 路由：所有未匹配的请求都返回 index.html
    Router::new()
        .fallback_service(
            ServeDir::new(&static_dir)
                .not_found_service(ServeFile::new(&index_path))
        )
}
