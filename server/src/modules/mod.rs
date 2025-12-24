//! 核心功能模块

pub mod account;
pub mod quota;
pub mod config;
pub mod logger;

// Web 版本不需要的模块
// pub mod db;        // 需要 rusqlite
// pub mod process;   // 桌面进程管理
pub mod oauth;
// pub mod oauth_server;
pub mod proxy;
// pub mod migration;
// pub mod tray;      // 系统托盘
// pub mod i18n;

pub use account::AccountManager;
pub use quota::fetch_quota;
pub use config::{load_app_config, save_app_config};
