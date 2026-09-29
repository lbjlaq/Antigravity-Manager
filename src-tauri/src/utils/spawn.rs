//! 运行时感知的异步任务派发（阶段 3，双 runtime 隐患修复）
//!
//! 背景：headless 模式在 lib.rs 手建 tokio runtime 并 block_on，而
//! `tauri::async_runtime::spawn` 会懒初始化 tauri 自己的全局 runtime，
//! 形成"两套 runtime 并存"。本助手按上下文选择正确的派发目标：
//! - 已处于 tokio runtime 上下文（headless 手建 runtime / 任意 tokio 任务内）→ `tokio::spawn`；
//! - 否则（GUI 主线程 setup 等无 ambient runtime 的场景）→ `tauri::async_runtime::spawn`。

/// 派发一个可跨 await 的后台任务（见模块注释）
pub fn spawn_task<F>(future: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    if tokio::runtime::Handle::try_current().is_ok() {
        tokio::spawn(future);
    } else {
        tauri::async_runtime::spawn(future);
    }
}
