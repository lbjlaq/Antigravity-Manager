use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::RwLock;
use tracing::{debug, info};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;
#[cfg(target_os = "windows")]
const DETACHED_PROCESS: u32 = 0x00000008;
#[cfg(target_os = "windows")]
const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;

/// Cloudflared隧道模式
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TunnelMode {
    /// 快速隧道(临时URL)
    Quick,
    /// 认证隧道(使用Token)
    Auth,
}

impl Default for TunnelMode {
    fn default() -> Self {
        Self::Quick
    }
}

/// Cloudflared配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudflaredConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub mode: TunnelMode,
    /// 代理的本地端口
    pub port: u16,
    /// 认证模式的Token
    #[serde(default)]
    pub token: Option<String>,
    /// 使用http2协议(更兼容)
    #[serde(default)]
    pub use_http2: bool,
}

impl Default for CloudflaredConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: TunnelMode::Quick,
            port: 8045,
            token: None,
            use_http2: true, // 默认启用http2，更稳定
        }
    }
}

/// Cloudflared状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudflaredStatus {
    pub installed: bool,
    pub version: Option<String>,
    pub running: bool,
    pub url: Option<String>,
    pub error: Option<String>,
}

impl Default for CloudflaredStatus {
    fn default() -> Self {
        Self {
            installed: false,
            version: None,
            running: false,
            url: None,
            error: None,
        }
    }
}

/// Cloudflared管理器状态
pub struct CloudflaredManager {
    process: Arc<RwLock<Option<Child>>>,
    status: Arc<RwLock<CloudflaredStatus>>,
    bin_path: PathBuf,
    /// 用于通知进程监控任务停止
    shutdown_tx: RwLock<Option<tokio::sync::oneshot::Sender<()>>>,
}

impl CloudflaredManager {
    pub fn new(data_dir: &PathBuf) -> Self {
        let bin_name = if cfg!(target_os = "windows") {
            "cloudflared.exe"
        } else {
            "cloudflared"
        };
        let bin_path = data_dir.join("bin").join(bin_name);

        Self {
            process: Arc::new(RwLock::new(None)),
            status: Arc::new(RwLock::new(CloudflaredStatus::default())),
            bin_path,
            shutdown_tx: RwLock::new(None),
        }
    }

    /// 检查是否已安装
    pub async fn check_installed(&self) -> (bool, Option<String>) {
        if !self.bin_path.exists() {
            return (false, None);
        }

        let mut cmd = Command::new(&self.bin_path);
        cmd.arg("--version");
        #[cfg(target_os = "windows")]
        cmd.creation_flags(CREATE_NO_WINDOW);

        match cmd.output().await {
            Ok(output) => {
                if output.status.success() {
                    let version = String::from_utf8_lossy(&output.stdout)
                        .lines()
                        .next()
                        .map(|s| s.trim().to_string());
                    (true, version)
                } else {
                    (false, None)
                }
            }
            Err(_) => (false, None),
        }
    }

    /// 获取当前状态
    pub async fn get_status(&self) -> CloudflaredStatus {
        self.status.read().await.clone()
    }

    /// 更新状态
    async fn update_status(&self, f: impl FnOnce(&mut CloudflaredStatus)) {
        let mut status = self.status.write().await;
        f(&mut status);
    }

    /// 安装cloudflared
    pub async fn install(&self) -> Result<CloudflaredStatus, String> {
        let bin_dir = self.bin_path.parent().unwrap();
        if !bin_dir.exists() {
            std::fs::create_dir_all(bin_dir)
                .map_err(|e| format!("Failed to create bin directory: {}", e))?;
        }

        let download_url = get_download_url()?;
        info!("[cloudflared] Downloading from: {}", download_url);

        let response = reqwest::get(&download_url)
            .await
            .map_err(|e| format!("Download failed: {}", e))?;

        if !response.status().is_success() {
            return Err(format!(
                "Download failed with status: {}",
                response.status()
            ));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| format!("Failed to read response: {}", e))?;

        let is_archive = download_url.ends_with(".tgz");
        if is_archive {
            let archive_path = self.bin_path.with_extension("tgz");
            std::fs::write(&archive_path, &bytes)
                .map_err(|e| format!("Failed to write archive: {}", e))?;

            let status = Command::new("tar")
                .arg("-xzf")
                .arg(&archive_path)
                .arg("-C")
                .arg(bin_dir)
                .status()
                .await
                .map_err(|e| format!("Failed to extract archive: {}", e))?;

            if !status.success() {
                return Err("Failed to extract cloudflared archive".to_string());
            }

            let _ = std::fs::remove_file(&archive_path);
        } else {
            std::fs::write(&self.bin_path, &bytes)
                .map_err(|e| format!("Failed to write binary: {}", e))?;
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.bin_path, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| format!("Failed to set permissions: {}", e))?;
        }

        let (installed, version) = self.check_installed().await;
        self.update_status(|s| {
            s.installed = installed;
            s.version = version.clone();
        })
        .await;

        info!(
            "[cloudflared] Installed successfully, version: {:?}",
            version
        );
        Ok(self.get_status().await)
    }

    /// 启动隧道
    pub async fn start(&self, config: CloudflaredConfig) -> Result<CloudflaredStatus, String> {
        // 检查是否已在运行
        {
            let proc = self.process.read().await;
            if proc.is_some() {
                return Ok(self.get_status().await);
            }
        }

        // 停止之前的监控任务
        if let Some(tx) = self.shutdown_tx.write().await.take() {
            let _ = tx.send(());
        }

        let (installed, version) = self.check_installed().await;
        if !installed {
            return Err("Cloudflared not installed".to_string());
        }

        let local_url = format!("http://localhost:{}", config.port);
        info!("[cloudflared] Starting tunnel to: {}", local_url);

        let mut cmd = Command::new(&self.bin_path);

        // 设置工作目录
        // 设置工作目录
        if let Some(bin_dir) = self.bin_path.parent() {
            cmd.current_dir(bin_dir);
            debug!("[cloudflared] Working directory: {:?}", bin_dir);
        }

        match config.mode {
            TunnelMode::Quick => {
                cmd.arg("tunnel").arg("--url").arg(&local_url);

                // 注意：--no-autoupdate 参数在较新版本的 cloudflared 中已不被支持，会导致进程立即退出
                // cmd.arg("--no-autoupdate");

                if config.use_http2 {
                    cmd.arg("--protocol").arg("http2");
                }

                // 注意：--loglevel 参数在此上下文中也会导致 Incorrect Usage 错误，故移除以使用默认值
                // cmd.arg("--loglevel").arg("info");

                info!("[cloudflared] Command args: tunnel --url {} ...", local_url);
            }
            TunnelMode::Auth => {
                if let Some(token) = &config.token {
                    cmd.arg("tunnel").arg("run").arg("--token").arg(token);

                    // 注意：--no-autoupdate 参数不被支持
                    // cmd.arg("--no-autoupdate");

                    if config.use_http2 {
                        cmd.arg("--protocol").arg("http2");
                    }

                    // 注意：--loglevel 参数不被支持
                    // cmd.arg("--loglevel").arg("info");

                    info!("[cloudflared] Command args: tunnel run --token [HIDDEN] ...");
                } else {
                    return Err("Token required for auth mode".to_string());
                }
            }
        }

        // 恢复管道
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        // 使用 DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP 隐藏窗口
        #[cfg(target_os = "windows")]
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);

        let mut child = cmd.spawn().map_err(|e| format!("Failed to spawn: {}", e))?;

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let status_clone = self.status.clone();
        if let Some(stdout) = stdout {
            spawn_log_reader(stdout, status_clone.clone());
        }

        if let Some(stderr) = stderr {
            spawn_log_reader(stderr, status_clone.clone());
        }

        *self.process.write().await = Some(child);
        self.update_status(|s| {
            s.installed = installed.clone();
            s.version = version.clone();
            s.running = true;
            s.error = None;
        })
        .await;

        // 启动进程监控任务
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        *self.shutdown_tx.write().await = Some(shutdown_tx);

        let process_ref = self.process.clone();
        let status_ref = self.status.clone();

        tokio::spawn(async move {
            tokio::select! {
                _ = shutdown_rx => {
                    debug!("[cloudflared] Process monitor shutdown");
                }
                _ = async {
                    loop {
                        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

                        let mut proc_lock = process_ref.write().await;
                        if let Some(ref mut child) = *proc_lock {
                            match child.try_wait() {
                                Ok(Some(exit_status)) => {
                                    // 进程已退出
                                    info!("[cloudflared] Process exited with status: {:?}", exit_status);
                                    *proc_lock = None;
                                    drop(proc_lock);

                                    let mut s = status_ref.write().await;
                                    s.running = false;
                                    s.error = Some(format!("Tunnel process exited (status: {:?})", exit_status));
                                    break;
                                }
                                Ok(None) => {
                                    // 进程仍在运行
                                }
                                Err(e) => {
                                    info!("[cloudflared] Error checking process: {}", e);
                                    *proc_lock = None;
                                    drop(proc_lock);

                                    let mut s = status_ref.write().await;
                                    s.running = false;
                                    s.error = Some(format!("Error checking tunnel: {}", e));
                                    break;
                                }
                            }
                        } else {
                            // 进程不存在
                            drop(proc_lock);
                            let mut s = status_ref.write().await;
                            if s.running {
                                s.running = false;
                                s.error = Some("Tunnel process not found".to_string());
                            }
                            break;
                        }
                    }
                } => {}
            }
        });

        Ok(self.get_status().await)
    }

    /// 停止隧道
    pub async fn stop(&self) -> Result<CloudflaredStatus, String> {
        let mut proc_lock = self.process.write().await;
        if let Some(mut child) = proc_lock.take() {
            let _ = child.kill().await;
            info!("[cloudflared] Tunnel stopped");
        }

        self.update_status(|s| {
            s.running = false;
            s.url = None;
            s.error = None;
        })
        .await;

        Ok(self.get_status().await)
    }
}

/// 获取下载URL
fn get_download_url() -> Result<String, String> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    let (os_str, arch_str, ext) = match (os, arch) {
        ("macos", "aarch64") => ("darwin", "arm64", ".tgz"),
        ("macos", "x86_64") => ("darwin", "amd64", ".tgz"),
        ("linux", "x86_64") => ("linux", "amd64", ""),
        ("linux", "aarch64") => ("linux", "arm64", ""),
        ("windows", "x86_64") => ("windows", "amd64", ".exe"),
        _ => return Err(format!("Unsupported platform: {}-{}", os, arch)),
    };

    Ok(format!(
        "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-{}-{}{}",
        os_str, arch_str, ext
    ))
}

fn spawn_log_reader<R>(stream: R, status_ref: Arc<RwLock<CloudflaredStatus>>)
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let reader = BufReader::new(stream);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            // 恢复日志级别为 debug，避免污染生产环境日志
            debug!("[cloudflared output] {}", line);
            if let Some(url) = extract_tunnel_url(&line) {
                info!("[cloudflared] Tunnel URL: {}", url);
                let mut s = status_ref.write().await;
                s.url = Some(url);
            }
        }
    });
}

/// 从日志行提取隧道URL
/// 支持两种模式：
/// 1. 快速隧道：直接提取 .trycloudflare.com URL
/// 2. 命名隧道：从 ingress 配置中解析 hostname
fn extract_tunnel_url(line: &str) -> Option<String> {
    // 快速隧道模式：直接查找 trycloudflare.com URL
    if let Some(url) = line
        .split_whitespace()
        .find(|s| s.starts_with("https://") && s.contains(".trycloudflare.com"))
    {
        return Some(url.to_string());
    }

    // 命名隧道模式：从 "Updated to new configuration" 日志中解析 hostname
    // 日志格式示例：Updated to new configuration config="{\"ingress\":[{\"hostname\":\"api.example.com\", ...}]}"
    if line.contains("Updated to new configuration") && line.contains("ingress") {
        // 查找 hostname 字段
        if let Some(start) = line.find("\\\"hostname\\\":\\\"") {
            let after_key = &line[start + 15..]; // 跳过 \"hostname\":\" (共15字符)
            if let Some(end) = after_key.find("\\\"") {
                let hostname = &after_key[..end];
                if !hostname.is_empty() {
                    return Some(format!("https://{}", hostname));
                }
            }
        }
    }

    None
}
