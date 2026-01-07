//! Antigravity Manager - 独立 Web 服务端入口
//!
//! 用法:
//!   antigravity-server [OPTIONS]
//!
//! OPTIONS:
//!   --port <PORT>           API 服务端口 (默认: 8765)
//!   --static-dir <PATH>     前端静态文件目录 (默认: ./dist)
//!   --data-dir <PATH>       数据目录 (默认: ~/.antigravity)
//!   --host <HOST>           绑定地址 (默认: 0.0.0.0)

use axum::{
    http::{header, Method, StatusCode},
    response::IntoResponse,
    Router,
};

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::{
    cors::{Any, CorsLayer},
    services::ServeDir,
    trace::TraceLayer,
};
use tracing::{info, error};

// 导入库中的模块
use antigravity_tools_lib::modules::logger;
use antigravity_tools_lib::web_api::{create_api_router, WebApiState};

/// 命令行参数
struct Args {
    port: u16,
    host: String,
    static_dir: PathBuf,
    data_dir: Option<PathBuf>,
}

impl Args {
    fn parse() -> Self {
        let mut args = std::env::args().skip(1);
        let mut port = 8765u16;
        let mut host = "0.0.0.0".to_string();
        let mut static_dir = PathBuf::from("./dist");
        let mut data_dir: Option<PathBuf> = None;

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--port" | "-p" => {
                    if let Some(val) = args.next() {
                        port = val.parse().unwrap_or(8765);
                    }
                }
                "--host" | "-h" => {
                    if let Some(val) = args.next() {
                        host = val;
                    }
                }
                "--static-dir" | "-s" => {
                    if let Some(val) = args.next() {
                        static_dir = PathBuf::from(val);
                    }
                }
                "--data-dir" | "-d" => {
                    if let Some(val) = args.next() {
                        data_dir = Some(PathBuf::from(val));
                    }
                }
                "--help" => {
                    print_help();
                    std::process::exit(0);
                }
                _ => {}
            }
        }

        Self {
            port,
            host,
            static_dir,
            data_dir,
        }
    }
}

fn print_help() {
    println!(
        r#"Antigravity Manager - Web Server Mode

用法:
  antigravity-server [OPTIONS]

OPTIONS:
  -p, --port <PORT>         API 服务端口 (默认: 8765)
  -h, --host <HOST>         绑定地址 (默认: 0.0.0.0)
  -s, --static-dir <PATH>   前端静态文件目录 (默认: ./dist)
  -d, --data-dir <PATH>     数据目录 (默认: ~/.antigravity)
      --help                显示帮助信息

示例:
  antigravity-server --port 8080 --static-dir ./web
  antigravity-server -p 9000 -d /data/antigravity
"#
    );
}

#[tokio::main]
async fn main() {
    // 解析命令行参数
    let args = Args::parse();

    // 设置数据目录环境变量 (如果指定)
    if let Some(ref data_dir) = args.data_dir {
        std::env::set_var("ANTIGRAVITY_DATA_DIR", data_dir);
    }

    // 初始化日志
    logger::init_logger();

    info!("Antigravity Manager Web Server starting...");
    info!("  Port: {}", args.port);
    info!("  Host: {}", args.host);
    info!("  Static dir: {:?}", args.static_dir);
    if let Some(ref data_dir) = args.data_dir {
        info!("  Data dir: {:?}", data_dir);
    }

    // 创建共享状态
    let state = Arc::new(WebApiState::new());

    // 创建 API 路由
    let api_router = create_api_router(state.clone());

    // 创建 CORS 配置
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION]);

    // 创建 fallback 用于 SPA 路由
    let static_dir_clone = args.static_dir.clone();
    let index_path = args.static_dir.join("index.html");
    let fallback = move || {
        let index_path = index_path.clone();
        async move {
            match tokio::fs::read_to_string(&index_path).await {
                Ok(content) => axum::response::Html(content).into_response(),
                Err(_) => StatusCode::NOT_FOUND.into_response(),
            }
        }
    };

    // 组合路由
    let app = Router::new()
        .merge(api_router)
        .fallback_service(
            ServeDir::new(&static_dir_clone)
                .append_index_html_on_directories(true)
                .fallback(axum::routing::get(fallback)),
        )
        .layer(cors)
        .layer(TraceLayer::new_for_http());


    // 启动服务器
    let addr: SocketAddr = format!("{}:{}", args.host, args.port)
        .parse()
        .expect("Invalid address");

    info!("Server listening on http://{}", addr);
    info!("Open http://localhost:{} in your browser", args.port);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    
    axum::serve(listener, app)
        .await
        .expect("Server error");
}
