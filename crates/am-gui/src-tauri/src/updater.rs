use crate::tray;
use am_core::config::AppConfig;
use am_core::update::{
    brew_upgrade_cask, check_for_updates, current_unix_secs, detect_install_channel,
    detect_system_proxy, is_version_skipped, should_auto_check, InstallChannel, UpdateInfo,
};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

pub const EVENT_UPDATE_STATE: &str = "updater://state";
pub const EVENT_UPDATE_PROGRESS: &str = "updater://progress";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UpdatePhase {
    Idle,
    Checking,
    UpToDate,
    Available,
    Downloading,
    Installing,
    Ready,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSnapshot {
    pub phase: UpdatePhase,
    pub info: Option<UpdateInfo>,
    pub install_channel: InstallChannel,
    pub progress_pct: u32,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub error: Option<String>,
    pub banner_dismissed: bool,
    pub triggered_by_auto: bool,
    pub updated_via_brew: bool,
}

impl Default for UpdateSnapshot {
    fn default() -> Self {
        Self {
            phase: UpdatePhase::Idle,
            info: None,
            install_channel: detect_install_channel(),
            progress_pct: 0,
            downloaded_bytes: 0,
            total_bytes: 0,
            error: None,
            banner_dismissed: false,
            triggered_by_auto: false,
            updated_via_brew: false,
        }
    }
}

struct StagedNativeUpdate {
    update: Update,
    bytes: Vec<u8>,
}

/// 进程级统一更新状态机：
/// - 独立于前端 WebView 生命周期，轻量模式销毁/重建窗口或切页后状态与下载进度零丢失
/// - 两阶段热更新：`download()` 先下载并校验签名，用户确认重启时再 `install(bytes)` 落盘替换，避免半安装状态
#[derive(Clone)]
pub struct UpdateController {
    inner: Arc<UpdateControllerInner>,
}

struct UpdateControllerInner {
    snapshot: Mutex<UpdateSnapshot>,
    staged_native: tokio::sync::Mutex<Option<StagedNativeUpdate>>,
    op_lock: tokio::sync::Mutex<()>,
}

impl UpdateController {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(UpdateControllerInner {
                snapshot: Mutex::new(UpdateSnapshot::default()),
                staged_native: tokio::sync::Mutex::new(None),
                op_lock: tokio::sync::Mutex::new(()),
            }),
        }
    }

    pub fn snapshot(&self) -> UpdateSnapshot {
        self.inner
            .snapshot
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn mutate_and_emit<F>(&self, app: &AppHandle, mutator: F) -> UpdateSnapshot
    where
        F: FnOnce(&mut UpdateSnapshot),
    {
        let next = {
            let mut guard = self
                .inner
                .snapshot
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            mutator(&mut guard);
            guard.clone()
        };
        let _ = app.emit(EVENT_UPDATE_STATE, &next);
        next
    }

    /// 手动检查更新：始终刷新全局状态，即使曾跳过该版本也会重新展示
    pub async fn check_manual(&self, app: &AppHandle) -> Result<UpdateInfo, String> {
        let _guard = self.inner.op_lock.lock().await;

        let current_phase = self.snapshot().phase;
        if matches!(
            current_phase,
            UpdatePhase::Downloading | UpdatePhase::Installing | UpdatePhase::Ready
        ) {
            if let Some(info) = self.snapshot().info {
                return Ok(info);
            }
        }

        self.mutate_and_emit(app, |s| {
            s.phase = UpdatePhase::Checking;
            s.error = None;
            s.triggered_by_auto = false;
        });

        let current = app.package_info().version.to_string();
        match check_for_updates(&current).await {
            Ok(info) => {
                let _ = AppConfig::update(|cfg| {
                    cfg.update.last_check_time = current_unix_secs();
                });
                let has_update = info.has_update;
                self.mutate_and_emit(app, |s| {
                    s.install_channel = info.install_channel;
                    s.info = Some(info.clone());
                    s.error = None;
                    s.progress_pct = 0;
                    s.downloaded_bytes = 0;
                    s.total_bytes = 0;
                    s.updated_via_brew = false;
                    s.triggered_by_auto = false;
                    s.phase = if has_update {
                        UpdatePhase::Available
                    } else {
                        UpdatePhase::UpToDate
                    };
                });
                Ok(info)
            }
            Err(e) => {
                let msg = e.to_string();
                self.mutate_and_emit(app, |s| {
                    s.phase = UpdatePhase::Error;
                    s.error = Some(msg.clone());
                });
                Err(msg)
            }
        }
    }

    /// 静默自动检查（启动延迟 + 周期轮询）：离线或瞬时失败静默忽略，不打扰用户
    pub async fn check_silent_if_due(&self, app: &AppHandle) {
        let cfg = AppConfig::load();
        if !should_auto_check(&cfg.update) {
            return;
        }

        let Ok(_guard) = self.inner.op_lock.try_lock() else {
            return;
        };

        let phase = self.snapshot().phase;
        if matches!(
            phase,
            UpdatePhase::Downloading | UpdatePhase::Installing | UpdatePhase::Ready
        ) {
            return;
        }

        let current = app.package_info().version.to_string();
        let Ok(info) = check_for_updates(&current).await else {
            return;
        };

        let _ = AppConfig::update(|c| {
            c.update.last_check_time = current_unix_secs();
        });

        if !info.has_update {
            return;
        }

        if is_version_skipped(&cfg.update, &info.latest_version) {
            return;
        }

        self.mutate_and_emit(app, |s| {
            let is_new_ver = s
                .info
                .as_ref()
                .map(|old| old.latest_version != info.latest_version)
                .unwrap_or(true);
            s.install_channel = info.install_channel;
            s.info = Some(info.clone());
            s.error = None;
            s.triggered_by_auto = true;
            if is_new_ver {
                s.banner_dismissed = false;
            }
            s.phase = UpdatePhase::Available;
        });
    }

    /// 第一阶段：下载并验签（或通过 Homebrew Cask 升级），完成后进入 `Ready` 等待重启
    pub async fn download_update(&self, app: &AppHandle, force_native: bool) -> Result<(), String> {
        let _guard = self.inner.op_lock.lock().await;

        let snap = self.snapshot();
        if snap.phase == UpdatePhase::Ready {
            return Ok(());
        }

        let channel = if force_native {
            InstallChannel::Native
        } else {
            snap.install_channel
        };

        match channel {
            InstallChannel::Manual => {
                let msg = "Current Linux package (DEB/RPM) does not support in-app updater; please download manually.".to_string();
                self.mutate_and_emit(app, |s| {
                    s.phase = UpdatePhase::Error;
                    s.error = Some(msg.clone());
                });
                return Err(msg);
            }
            InstallChannel::Homebrew => {
                self.mutate_and_emit(app, |s| {
                    s.phase = UpdatePhase::Downloading;
                    s.progress_pct = 0;
                    s.downloaded_bytes = 0;
                    s.total_bytes = 0;
                    s.error = None;
                    s.updated_via_brew = true;
                });

                match brew_upgrade_cask().await {
                    Ok(_) => {
                        self.mutate_and_emit(app, |s| {
                            s.phase = UpdatePhase::Ready;
                            s.progress_pct = 100;
                            s.error = None;
                            s.updated_via_brew = true;
                            s.banner_dismissed = false;
                        });
                        return Ok(());
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        self.mutate_and_emit(app, |s| {
                            s.phase = UpdatePhase::Error;
                            s.error = Some(msg.clone());
                            s.updated_via_brew = false;
                        });
                        return Err(msg);
                    }
                }
            }
            InstallChannel::Native => {}
        }

        self.mutate_and_emit(app, |s| {
            s.phase = UpdatePhase::Downloading;
            s.progress_pct = 0;
            s.downloaded_bytes = 0;
            s.total_bytes = 0;
            s.error = None;
            s.updated_via_brew = false;
        });

        let mut builder = app.updater_builder();
        if let Some(proxy_str) = detect_system_proxy() {
            if let Ok(proxy_url) = url::Url::parse(&proxy_str) {
                builder = builder.proxy(proxy_url);
            }
        }

        let updater = match builder.build() {
            Ok(u) => u,
            Err(e) => {
                let msg = format!("Updater unavailable: {}", e);
                self.mutate_and_emit(app, |s| {
                    s.phase = UpdatePhase::Error;
                    s.error = Some(msg.clone());
                });
                return Err(msg);
            }
        };

        let update = match updater.check().await {
            Ok(Some(u)) => u,
            Ok(None) => {
                let msg =
                    "No matching native update artifact found for this platform yet".to_string();
                self.mutate_and_emit(app, |s| {
                    s.phase = UpdatePhase::Error;
                    s.error = Some(msg.clone());
                });
                return Err(msg);
            }
            Err(e) => {
                let msg = format!("Failed to fetch update manifest: {}", e);
                self.mutate_and_emit(app, |s| {
                    s.phase = UpdatePhase::Error;
                    s.error = Some(msg.clone());
                });
                return Err(msg);
            }
        };

        let controller_for_cb = self.clone();
        let app_for_cb = app.clone();
        let downloaded_acc = Arc::new(AtomicU64::new(0));
        let last_emitted_pct = Arc::new(AtomicU32::new(u32::MAX));
        let downloaded_clone = downloaded_acc.clone();
        let last_pct_clone = last_emitted_pct.clone();

        let bytes = match update
            .download(
                move |chunk_len, content_len| {
                    let downloaded = downloaded_clone
                        .fetch_add(chunk_len as u64, Ordering::Relaxed)
                        + chunk_len as u64;
                    let total = content_len.unwrap_or(0);
                    let pct = if total > 0 {
                        ((downloaded as f64 / total as f64) * 100.0)
                            .min(100.0)
                            .round() as u32
                    } else {
                        0
                    };

                    let prev_pct = last_pct_clone.swap(pct, Ordering::Relaxed);
                    if pct != prev_pct || prev_pct == u32::MAX || total == 0 {
                        let _ = app_for_cb.emit(
                            EVENT_UPDATE_PROGRESS,
                            serde_json::json!({
                                "downloaded": downloaded,
                                "total": total,
                                "pct": pct,
                            }),
                        );
                        controller_for_cb.mutate_and_emit(&app_for_cb, |s| {
                            s.phase = UpdatePhase::Downloading;
                            s.progress_pct = pct;
                            s.downloaded_bytes = downloaded;
                            s.total_bytes = total;
                        });
                    }
                },
                || {},
            )
            .await
        {
            Ok(b) => b,
            Err(e) => {
                let msg = format!("Update download or signature verification failed: {}", e);
                self.mutate_and_emit(app, |s| {
                    s.phase = UpdatePhase::Error;
                    s.error = Some(msg.clone());
                });
                return Err(msg);
            }
        };

        {
            let mut staged = self.inner.staged_native.lock().await;
            *staged = Some(StagedNativeUpdate { update, bytes });
        }

        self.mutate_and_emit(app, |s| {
            s.phase = UpdatePhase::Ready;
            s.progress_pct = 100;
            s.error = None;
            s.banner_dismissed = false;
        });

        Ok(())
    }

    /// 第二阶段：落盘替换已验签的安装包并安全重启
    pub async fn apply_and_restart(&self, app: &AppHandle) -> Result<(), String> {
        let _guard = self.inner.op_lock.lock().await;

        let snap = self.snapshot();
        if snap.updated_via_brew {
            self.mutate_and_emit(app, |s| {
                s.phase = UpdatePhase::Installing;
            });
            tray::mark_app_exiting();
            app.restart();
        }

        let staged = {
            let mut slot = self.inner.staged_native.lock().await;
            slot.take()
        };

        let Some(StagedNativeUpdate { update, bytes }) = staged else {
            return Err("No downloaded update is staged for installation".to_string());
        };

        self.mutate_and_emit(app, |s| {
            s.phase = UpdatePhase::Installing;
            s.error = None;
        });

        if let Err(e) = update.install(bytes) {
            am_core::trim_process_memory();
            let msg = format!("Update install failed: {}", e);
            self.mutate_and_emit(app, |s| {
                s.phase = UpdatePhase::Error;
                s.error = Some(msg.clone());
            });
            return Err(msg);
        }

        // 必须先放行 ExitRequested 托盘常驻拦截，再触发重启
        tray::mark_app_exiting();
        app.restart();
    }

    pub fn dismiss_banner(&self, app: &AppHandle) -> UpdateSnapshot {
        self.mutate_and_emit(app, |s| {
            s.banner_dismissed = true;
        })
    }

    pub fn skip_version(&self, app: &AppHandle, version: &str) -> Result<UpdateSnapshot, String> {
        let clean = version
            .trim()
            .trim_start_matches("am-")
            .trim_start_matches('v')
            .trim_start_matches('V')
            .to_string();
        if clean.is_empty() {
            return Err("Version cannot be empty".to_string());
        }
        AppConfig::update(|cfg| {
            cfg.update.skipped_version = Some(clean.clone());
        })
        .map_err(|e| e.to_string())?;

        Ok(self.mutate_and_emit(app, |s| {
            s.banner_dismissed = true;
            if s.phase == UpdatePhase::Available {
                s.phase = UpdatePhase::Idle;
            }
        }))
    }
}

/// 启动后台自动检查调度器：启动延迟 3 秒检查一次，之后每 6 小时检查是否到达用户冷却周期（默认 24h）
pub fn spawn_auto_check_scheduler(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(3)).await;
        loop {
            if let Some(ctrl) = app.try_state::<UpdateController>() {
                ctrl.check_silent_if_due(&app).await;
            }
            tokio::time::sleep(Duration::from_secs(6 * 3600)).await;
        }
    });
}
