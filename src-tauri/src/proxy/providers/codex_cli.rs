use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter, Lines};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio::time::{sleep, timeout, Duration, Instant};

use crate::proxy::{CodexConfig, CodexModelCatalogMode};

const CLIENT_NAME: &str = "antigravity-manager";
const INIT_OPTOUT_NOTIFICATIONS: &[&str] = &["mcpServer/startupStatus/updated"];

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CodexUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CodexProviderStatus {
    pub enabled: bool,
    pub command: String,
    pub executable_path: Option<String>,
    pub version: Option<String>,
    pub login_status: String,
    pub login_message: Option<String>,
    pub desired_workers: usize,
    pub healthy_workers: usize,
    pub busy_workers: usize,
    pub models: Vec<String>,
    pub last_error: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CodexApiRequest {
    pub model: String,
    pub developer_instructions: Option<String>,
    pub prompt_text: String,
    pub images: Vec<String>,
    pub stream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CodexRunResult {
    pub model: String,
    pub text: String,
    pub usage: Option<CodexUsage>,
}

#[derive(Debug, Clone)]
pub enum CodexStreamEvent {
    TextDelta(String),
    Completed(CodexRunResult),
    Error(String),
}

struct CodexWorkerProcess {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: Lines<BufReader<ChildStdout>>,
    next_id: u64,
}

struct CodexWorker {
    slot: Mutex<()>,
    inner: Mutex<CodexWorkerProcess>,
    last_error: Arc<RwLock<Option<String>>>,
    warnings: Arc<RwLock<Vec<String>>>,
}

#[derive(Clone)]
pub struct CodexRuntimeManager {
    config: Arc<RwLock<CodexConfig>>,
    workers: Arc<RwLock<Vec<Arc<CodexWorker>>>>,
    cached_models: Arc<RwLock<Vec<String>>>,
    status: Arc<RwLock<CodexProviderStatus>>,
    next_worker: Arc<AtomicUsize>,
    runtime_signature: Arc<RwLock<Option<String>>>,
}

impl CodexRuntimeManager {
    pub fn new() -> Self {
        Self {
            config: Arc::new(RwLock::new(CodexConfig::default())),
            workers: Arc::new(RwLock::new(Vec::new())),
            cached_models: Arc::new(RwLock::new(Vec::new())),
            status: Arc::new(RwLock::new(CodexProviderStatus {
                login_status: "unknown".to_string(),
                ..CodexProviderStatus::default()
            })),
            next_worker: Arc::new(AtomicUsize::new(0)),
            runtime_signature: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn apply_config(&self, config: CodexConfig) -> Result<(), String> {
        {
            let mut current = self.config.write().await;
            *current = config.clone();
        }

        if !config.enabled {
            self.shutdown().await;
            let mut status = self.status.write().await;
            status.enabled = false;
            status.command = config.command;
            status.desired_workers = config.worker_count;
            return Ok(());
        }

        let new_signature = runtime_signature(&config);
        let previous_signature = self.runtime_signature.read().await.clone();
        let needs_restart = previous_signature.as_deref() != Some(new_signature.as_str())
            || self.workers.read().await.len() != config.worker_count.max(1);

        if needs_restart {
            self.shutdown().await;
            self.start_workers(config.clone()).await?;
            *self.runtime_signature.write().await = Some(new_signature);
        }

        self.refresh_status().await?;
        Ok(())
    }

    pub async fn refresh_status(&self) -> Result<CodexProviderStatus, String> {
        let config = self.config.read().await.clone();
        let executable_path = resolve_command_path(&config.command).await.ok();
        let version = run_command_capture(&config.command, &["--version"])
            .await
            .ok()
            .and_then(|out| out.lines().next().map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty());
        let (login_status, login_message) = detect_login_status(&config.command).await;

        let workers = self.workers.read().await;
        let mut warnings = Vec::new();
        let mut last_error = None;
        let mut busy_workers = 0;
        for worker in workers.iter() {
            if worker.is_busy() {
                busy_workers += 1;
            }
            if let Some(err) = worker.last_error.read().await.clone() {
                last_error = Some(err);
            }
            warnings.extend(worker.warnings.read().await.clone());
        }
        warnings.sort();
        warnings.dedup();
        drop(workers);

        let models = self.cached_models.read().await.clone();
        let status = CodexProviderStatus {
            enabled: config.enabled,
            command: config.command,
            executable_path,
            version,
            login_status,
            login_message,
            desired_workers: config.worker_count.max(1),
            healthy_workers: self.workers.read().await.len(),
            busy_workers,
            models,
            last_error,
            warnings,
        };

        *self.status.write().await = status.clone();
        Ok(status)
    }

    pub async fn get_status(&self) -> CodexProviderStatus {
        self.status.read().await.clone()
    }

    pub async fn get_config(&self) -> CodexConfig {
        self.config.read().await.clone()
    }

    pub async fn refresh_models(&self) -> Result<Vec<String>, String> {
        let config = self.config.read().await.clone();
        if matches!(config.model_catalog_mode, CodexModelCatalogMode::Static) {
            let mut models = config.models.clone();
            models.sort();
            models.dedup();
            *self.cached_models.write().await = models.clone();
            self.refresh_status().await?;
            return Ok(models);
        }

        self.ensure_workers_ready().await?;
        let worker = self.select_worker().await?;
        let timeout_ms = self.config.read().await.request_timeout_ms.max(1_000);
        let _slot = worker.slot.lock().await;
        let mut inner = worker.inner.lock().await;
        let response = timeout(
            Duration::from_millis(timeout_ms),
            call_method_locked(
                &mut inner,
                worker.warnings.clone(),
                worker.last_error.clone(),
                "model/list",
                json!({ "limit": 100, "includeHidden": false }),
            ),
        )
        .await
        .map_err(|_| "Timed out while refreshing Codex models".to_string())??;

        let mut models = response
            .get("data")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.get("id").and_then(|v| v.as_str()).map(str::to_string))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        models.sort();
        models.dedup();
        *self.cached_models.write().await = models.clone();
        drop(inner);
        self.refresh_status().await?;
        Ok(models)
    }

    pub async fn run_request(&self, request: CodexApiRequest) -> Result<CodexRunResult, String> {
        self.ensure_workers_ready().await?;
        let worker = self.select_worker().await?;
        let timeout_ms = self.config.read().await.request_timeout_ms.max(1_000);
        let _slot = worker.slot.lock().await;
        let mut inner = worker.inner.lock().await;
        let result = timeout(
            Duration::from_millis(timeout_ms),
            run_request_locked(
                &mut inner,
                worker.warnings.clone(),
                worker.last_error.clone(),
                request,
                None,
            ),
        )
        .await
        .map_err(|_| "Timed out while waiting for Codex response".to_string())??;
        drop(inner);
        Ok(result)
    }

    pub async fn start_stream(
        &self,
        request: CodexApiRequest,
    ) -> Result<mpsc::Receiver<CodexStreamEvent>, String> {
        self.ensure_workers_ready().await?;
        let worker = self.select_worker().await?;
        let timeout_ms = self.config.read().await.request_timeout_ms.max(1_000);
        let (tx, rx) = mpsc::channel(64);
        tokio::spawn(async move {
            let _slot = worker.slot.lock().await;
            let mut inner = worker.inner.lock().await;
            let run_result = timeout(
                Duration::from_millis(timeout_ms),
                run_request_locked(
                    &mut inner,
                    worker.warnings.clone(),
                    worker.last_error.clone(),
                    request,
                    Some(tx.clone()),
                ),
            )
            .await
            .map_err(|_| "Timed out while waiting for Codex response".to_string())
            .and_then(|result| result);

            match run_result {
                Ok(result) => {
                    let _ = tx.send(CodexStreamEvent::Completed(result)).await;
                }
                Err(error) => {
                    let _ = tx.send(CodexStreamEvent::Error(error)).await;
                }
            }
        });
        Ok(rx)
    }

    pub async fn shutdown(&self) {
        let workers = {
            let mut guard = self.workers.write().await;
            std::mem::take(&mut *guard)
        };

        for worker in workers {
            worker.shutdown().await;
        }

        *self.runtime_signature.write().await = None;
    }

    async fn ensure_workers_ready(&self) -> Result<(), String> {
        let config = self.config.read().await.clone();
        if !config.enabled {
            return Err("Codex provider is disabled".to_string());
        }
        if self.workers.read().await.is_empty() {
            self.start_workers(config.clone()).await?;
            *self.runtime_signature.write().await = Some(runtime_signature(&config));
        }
        Ok(())
    }

    async fn start_workers(&self, config: CodexConfig) -> Result<(), String> {
        let worker_count = config.worker_count.max(1);
        let mut new_workers = Vec::with_capacity(worker_count);
        for worker_id in 0..worker_count {
            match CodexWorker::spawn(worker_id, &config).await {
                Ok(worker) => new_workers.push(Arc::new(worker)),
                Err(error) => {
                    for worker in new_workers {
                        worker.shutdown().await;
                    }
                    *self.status.write().await = CodexProviderStatus {
                        enabled: config.enabled,
                        command: config.command.clone(),
                        desired_workers: worker_count,
                        last_error: Some(error.clone()),
                        login_status: "unknown".to_string(),
                        ..CodexProviderStatus::default()
                    };
                    return Err(error);
                }
            }
        }

        *self.workers.write().await = new_workers;
        Ok(())
    }

    async fn select_worker(&self) -> Result<Arc<CodexWorker>, String> {
        let config = self.config.read().await.clone();
        let deadline = Instant::now() + Duration::from_millis(config.queue_timeout_ms.max(100));
        loop {
            let workers = self.workers.read().await.clone();
            if workers.is_empty() {
                return Err("Codex worker pool is unavailable".to_string());
            }
            let offset = self.next_worker.fetch_add(1, Ordering::Relaxed);
            for step in 0..workers.len() {
                let worker = workers[(offset + step) % workers.len()].clone();
                if !worker.is_busy() {
                    return Ok(worker);
                }
            }
            if Instant::now() >= deadline {
                return Err("Codex worker pool is busy".to_string());
            }
            sleep(Duration::from_millis(100)).await;
        }
    }
}

impl CodexWorker {
    async fn spawn(id: usize, config: &CodexConfig) -> Result<Self, String> {
        let mut command = Command::new(&config.command);
        command
            .arg("app-server")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        for (key, value) in &config.env {
            command.env(key, value);
        }

        let mut child = command
            .spawn()
            .map_err(|e| format!("Failed to start Codex app-server: {}", e))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "Codex app-server stdin is unavailable".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Codex app-server stdout is unavailable".to_string())?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "Codex app-server stderr is unavailable".to_string())?;

        let last_error = Arc::new(RwLock::new(None));
        let warnings = Arc::new(RwLock::new(Vec::new()));
        spawn_stderr_task(id, stderr, last_error.clone(), warnings.clone());

        let worker = Self {
            slot: Mutex::new(()),
            inner: Mutex::new(CodexWorkerProcess {
                child,
                stdin: BufWriter::new(stdin),
                stdout: BufReader::new(stdout).lines(),
                next_id: 1,
            }),
            last_error,
            warnings,
        };

        {
            let mut inner = worker.inner.lock().await;
            call_method_locked(
                &mut inner,
                worker.warnings.clone(),
                worker.last_error.clone(),
                "initialize",
                json!({
                    "clientInfo": { "name": CLIENT_NAME, "version": env!("CARGO_PKG_VERSION") },
                    "capabilities": { "experimentalApi": false, "optOutNotificationMethods": INIT_OPTOUT_NOTIFICATIONS }
                }),
            )
            .await?;
        }

        Ok(worker)
    }

    fn is_busy(&self) -> bool {
        self.slot.try_lock().is_err()
    }

    async fn shutdown(&self) {
        let mut inner = self.inner.lock().await;
        let _ = inner.stdin.shutdown().await;
        let _ = inner.child.kill().await;
    }
}

fn runtime_signature(config: &CodexConfig) -> String {
    let mut env_pairs = config
        .env
        .iter()
        .map(|(key, value)| format!("{}={}", key, value))
        .collect::<Vec<_>>();
    env_pairs.sort();
    format!(
        "{}|{}|{}|{}|{}",
        config.command,
        config.worker_count,
        config.request_timeout_ms,
        config.queue_timeout_ms,
        env_pairs.join(";")
    )
}

fn spawn_stderr_task(
    worker_id: usize,
    stderr: ChildStderr,
    last_error: Arc<RwLock<Option<String>>>,
    warnings: Arc<RwLock<Vec<String>>>,
) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if line.trim().is_empty() {
                continue;
            }
            if line.contains("ERROR") || line.contains("failed") {
                *last_error.write().await = Some(line.clone());
            }
            let mut guard = warnings.write().await;
            guard.push(format!("worker-{}: {}", worker_id, line));
            if guard.len() > 20 {
                let drain = guard.len() - 20;
                guard.drain(0..drain);
            }
        }
    });
}

async fn call_method_locked(
    inner: &mut CodexWorkerProcess,
    warnings: Arc<RwLock<Vec<String>>>,
    last_error: Arc<RwLock<Option<String>>>,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    let id = inner.next_id;
    inner.next_id += 1;
    send_json_line(
        &mut inner.stdin,
        &json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }),
    )
    .await?;

    loop {
        let message = read_json_line(&mut inner.stdout).await?;
        if let Some(message_id) = message.get("id").and_then(|v| v.as_u64()) {
            if message_id == id {
                if let Some(error) = message.get("error") {
                    let msg = error
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Codex app-server request failed")
                        .to_string();
                    *last_error.write().await = Some(msg.clone());
                    return Err(msg);
                }
                return Ok(message.get("result").cloned().unwrap_or(Value::Null));
            }
            continue;
        }
        capture_notification(&message, warnings.clone(), last_error.clone()).await;
    }
}

async fn run_request_locked(
    inner: &mut CodexWorkerProcess,
    warnings: Arc<RwLock<Vec<String>>>,
    last_error: Arc<RwLock<Option<String>>>,
    request: CodexApiRequest,
    stream_tx: Option<mpsc::Sender<CodexStreamEvent>>,
) -> Result<CodexRunResult, String> {
    let cwd = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .to_string_lossy()
        .to_string();
    let model = request.model.clone();
    let developer_instructions = request.developer_instructions.clone();

    let thread_result = call_method_locked(
        inner,
        warnings.clone(),
        last_error.clone(),
        "thread/start",
        json!({
            "cwd": cwd,
            "model": &model,
            "approvalPolicy": "never",
            "sandbox": "read-only",
            "experimentalRawEvents": false,
            "persistExtendedHistory": false,
            "ephemeral": true,
            "serviceName": "Antigravity Manager",
            "developerInstructions": developer_instructions
        }),
    )
    .await?;

    let thread_id = thread_result
        .get("thread")
        .and_then(|v| v.get("id"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Codex app-server did not return a thread id".to_string())?
        .to_string();

    let input_items = build_turn_input(&request);
    let turn_id = inner.next_id;
    inner.next_id += 1;
    send_json_line(
        &mut inner.stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": turn_id,
            "method": "turn/start",
            "params": { "threadId": thread_id, "input": input_items, "model": &model }
        }),
    )
    .await?;

    let mut output_text = String::new();
    let mut usage = None;

    loop {
        let message = read_json_line(&mut inner.stdout).await?;
        if message.get("id").and_then(|v| v.as_u64()) == Some(turn_id) {
            if let Some(error) = message.get("error") {
                let msg = error
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Codex turn failed")
                    .to_string();
                *last_error.write().await = Some(msg.clone());
                return Err(msg);
            }
            continue;
        }

        if let Some(method) = message.get("method").and_then(|v| v.as_str()) {
            match method {
                "item/agentMessage/delta" => {
                    let delta = message
                        .get("params")
                        .and_then(|v| v.get("delta"))
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    output_text.push_str(&delta);
                    if let Some(tx) = stream_tx.as_ref() {
                        let _ = tx.send(CodexStreamEvent::TextDelta(delta)).await;
                    }
                }
                "item/completed" => {
                    if let Some(item) = message.get("params").and_then(|v| v.get("item")) {
                        if item.get("type").and_then(|v| v.as_str()) == Some("agentMessage")
                            && output_text.is_empty()
                        {
                            if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                                output_text = text.to_string();
                            }
                        }
                    }
                }
                "thread/tokenUsage/updated" => {
                    usage = parse_usage_from_notification(&message);
                }
                "turn/completed" => {
                    break;
                }
                _ => {
                    capture_notification(&message, warnings.clone(), last_error.clone()).await;
                }
            }
        }
    }

    Ok(CodexRunResult {
        model,
        text: output_text,
        usage,
    })
}

fn build_turn_input(request: &CodexApiRequest) -> Vec<Value> {
    let mut items = vec![json!({
        "type": "text",
        "text": if request.prompt_text.trim().is_empty() { " " } else { request.prompt_text.as_str() },
        "text_elements": []
    })];
    for image in &request.images {
        items.push(json!({ "type": "image", "url": image }));
    }
    items
}

async fn capture_notification(
    message: &Value,
    warnings: Arc<RwLock<Vec<String>>>,
    last_error: Arc<RwLock<Option<String>>>,
) {
    if let Some(method) = message.get("method").and_then(|v| v.as_str()) {
        match method {
            "configWarning" => {
                if let Some(summary) = message
                    .get("params")
                    .and_then(|v| v.get("summary"))
                    .and_then(|v| v.as_str())
                {
                    let mut guard = warnings.write().await;
                    guard.push(summary.to_string());
                    if guard.len() > 20 {
                        let drain = guard.len() - 20;
                        guard.drain(0..drain);
                    }
                }
            }
            "error" => {
                if let Some(summary) = message
                    .get("params")
                    .and_then(|v| v.get("summary"))
                    .and_then(|v| v.as_str())
                {
                    *last_error.write().await = Some(summary.to_string());
                }
            }
            _ => {}
        }
    }
}

fn parse_usage_from_notification(message: &Value) -> Option<CodexUsage> {
    let usage = message
        .get("params")
        .and_then(|v| v.get("tokenUsage"))
        .and_then(|v| v.get("last").or_else(|| v.get("total")))?;

    Some(CodexUsage {
        input_tokens: usage.get("inputTokens").and_then(|v| v.as_u64()),
        output_tokens: usage.get("outputTokens").and_then(|v| v.as_u64()),
        total_tokens: usage.get("totalTokens").and_then(|v| v.as_u64()),
    })
}

async fn send_json_line(writer: &mut BufWriter<ChildStdin>, value: &Value) -> Result<(), String> {
    let line = serde_json::to_string(value).map_err(|e| e.to_string())?;
    writer
        .write_all(line.as_bytes())
        .await
        .map_err(|e| format!("Failed to write to Codex app-server: {}", e))?;
    writer
        .write_all(b"\n")
        .await
        .map_err(|e| format!("Failed to write newline to Codex app-server: {}", e))?;
    writer
        .flush()
        .await
        .map_err(|e| format!("Failed to flush Codex app-server request: {}", e))
}

async fn read_json_line(reader: &mut Lines<BufReader<ChildStdout>>) -> Result<Value, String> {
    loop {
        let line = reader
            .next_line()
            .await
            .map_err(|e| format!("Failed to read from Codex app-server: {}", e))?
            .ok_or_else(|| "Codex app-server closed the connection".to_string())?;

        if line.trim().is_empty() {
            continue;
        }

        return serde_json::from_str(&line)
            .map_err(|e| format!("Failed to parse Codex app-server JSON: {}", e));
    }
}

pub async fn resolve_command_path(command: &str) -> Result<String, String> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return Err("Codex command is empty".to_string());
    }
    if Path::new(trimmed).exists() {
        return Ok(trimmed.to_string());
    }

    let locator = if cfg!(target_os = "windows") { "where.exe" } else { "which" };
    let output = Command::new(locator)
        .arg(trimmed)
        .output()
        .await
        .map_err(|e| format!("Failed to resolve Codex command: {}", e))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    String::from_utf8(output.stdout)
        .map_err(|e| e.to_string())?
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(|line| line.trim().to_string())
        .ok_or_else(|| "Unable to resolve Codex command path".to_string())
}

async fn run_command_capture(command: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(command)
        .args(args)
        .output()
        .await
        .map_err(|e| format!("Failed to run `{}`: {}", command, e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if output.status.success() {
        if stdout.is_empty() {
            Ok(stderr)
        } else {
            Ok(stdout)
        }
    } else {
        Err(if stderr.is_empty() { stdout } else { stderr })
    }
}

async fn detect_login_status(command: &str) -> (String, Option<String>) {
    match run_command_capture(command, &["login", "status"]).await {
        Ok(output) => {
            let lowered = output.to_lowercase();
            if lowered.contains("chatgpt") {
                ("chatgpt".to_string(), Some(output))
            } else if lowered.contains("api key") {
                ("api_key".to_string(), Some(output))
            } else if output.trim().is_empty() {
                ("unknown".to_string(), None)
            } else {
                ("ready".to_string(), Some(output))
            }
        }
        Err(error) => ("not_logged_in".to_string(), Some(error)),
    }
}

pub fn is_codex_target(model: &str) -> bool {
    model.to_ascii_lowercase().starts_with("codex:")
}

pub fn strip_codex_target(model: &str) -> Option<String> {
    let trimmed = model.trim();
    if !is_codex_target(trimmed) {
        return None;
    }
    let actual = trimmed[6..].trim();
    if actual.is_empty() {
        None
    } else {
        Some(actual.to_string())
    }
}

pub fn codex_request_from_chat_body(
    body: &Value,
    mapped_model: &str,
) -> Result<CodexApiRequest, String> {
    reject_client_tools(body)?;

    let model = strip_codex_target(mapped_model).unwrap_or_else(|| mapped_model.to_string());
    let stream = body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);
    let messages = body
        .get("messages")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "Codex chat routing requires `messages`".to_string())?;

    let mut system_lines = Vec::new();
    let mut transcript = Vec::new();
    let mut images = Vec::new();

    for message in messages {
        let role = message
            .get("role")
            .and_then(|v| v.as_str())
            .unwrap_or("user");
        let content = flatten_content_value(message.get("content"), &mut images);
        if role == "system" {
            if !content.is_empty() {
                system_lines.push(content);
            }
            continue;
        }

        let mut rendered = String::new();
        if !content.is_empty() {
            rendered.push_str(&format!("{}:\n{}", title_case_role(role), content));
        }
        if let Some(tool_calls) = message.get("tool_calls") {
            if !rendered.is_empty() {
                rendered.push('\n');
            }
            rendered.push_str("Tool Calls:\n");
            rendered.push_str(&tool_calls.to_string());
        }
        if let Some(tool_call_id) = message.get("tool_call_id").and_then(|v| v.as_str()) {
            if !rendered.is_empty() {
                rendered.push('\n');
            }
            rendered.push_str(&format!("Tool Call ID: {}", tool_call_id));
        }
        if !rendered.trim().is_empty() {
            transcript.push(rendered);
        }
    }

    Ok(CodexApiRequest {
        model,
        developer_instructions: join_non_empty(system_lines),
        prompt_text: transcript.join("\n\n"),
        images,
        stream,
    })
}

pub fn codex_request_from_responses_body(
    body: &Value,
    mapped_model: &str,
) -> Result<CodexApiRequest, String> {
    reject_client_tools(body)?;

    let model = strip_codex_target(mapped_model).unwrap_or_else(|| mapped_model.to_string());
    let stream = body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);
    let mut images = Vec::new();
    let mut transcript = Vec::new();
    let developer_instructions = body
        .get("instructions")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    match body.get("input") {
        Some(Value::String(text)) => transcript.push(format!("User:\n{}", text)),
        Some(Value::Array(items)) => {
            for item in items {
                let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
                match item_type {
                    "message" => {
                        let role = item.get("role").and_then(|v| v.as_str()).unwrap_or("user");
                        let content = flatten_content_value(item.get("content"), &mut images);
                        if !content.is_empty() {
                            transcript.push(format!("{}:\n{}", title_case_role(role), content));
                        }
                    }
                    "input_text" | "text" => {
                        if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                            transcript.push(format!("User:\n{}", text));
                        }
                    }
                    "function_call" | "local_shell_call" | "web_search_call" => {
                        transcript.push(format!("Assistant Tool Call:\n{}", item));
                    }
                    "function_call_output" | "custom_tool_call_output" => {
                        transcript.push(format!("Tool Output:\n{}", item));
                    }
                    _ => {}
                }
            }
        }
        Some(other) => transcript.push(format!("User:\n{}", other)),
        None => {}
    }

    Ok(CodexApiRequest {
        model,
        developer_instructions,
        prompt_text: transcript.join("\n\n"),
        images,
        stream,
    })
}

pub fn build_chat_completion_response(
    model: &str,
    text: &str,
    usage: Option<&CodexUsage>,
) -> Value {
    let usage_value = usage_to_chat_json(usage);
    json!({
        "id": format!("chatcmpl-{}", uuid::Uuid::new_v4().simple()),
        "object": "chat.completion",
        "created": chrono::Utc::now().timestamp(),
        "model": model,
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": text },
            "finish_reason": "stop"
        }],
        "usage": usage_value
    })
}

pub fn build_responses_response(model: &str, text: &str, usage: Option<&CodexUsage>) -> Value {
    let response_id = format!("resp_{}", uuid::Uuid::new_v4().simple());
    let item_id = format!("msg_{}", uuid::Uuid::new_v4().simple());
    json!({
        "id": response_id,
        "object": "response",
        "created_at": chrono::Utc::now().to_rfc3339(),
        "status": "completed",
        "model": model,
        "output": [{
            "id": item_id,
            "type": "message",
            "role": "assistant",
            "status": "completed",
            "content": [{
                "type": "output_text",
                "text": text
            }]
        }],
        "usage": usage_to_responses_json(usage)
    })
}

fn usage_to_chat_json(usage: Option<&CodexUsage>) -> Value {
    let input = usage.and_then(|u| u.input_tokens).unwrap_or(0);
    let output = usage.and_then(|u| u.output_tokens).unwrap_or(0);
    let total = usage
        .and_then(|u| u.total_tokens)
        .unwrap_or(input.saturating_add(output));
    json!({
        "prompt_tokens": input,
        "completion_tokens": output,
        "total_tokens": total
    })
}

fn usage_to_responses_json(usage: Option<&CodexUsage>) -> Value {
    let input = usage.and_then(|u| u.input_tokens).unwrap_or(0);
    let output = usage.and_then(|u| u.output_tokens).unwrap_or(0);
    let total = usage
        .and_then(|u| u.total_tokens)
        .unwrap_or(input.saturating_add(output));
    json!({
        "input_tokens": input,
        "output_tokens": output,
        "total_tokens": total
    })
}

fn reject_client_tools(body: &Value) -> Result<(), String> {
    if body
        .get("tools")
        .and_then(|v| v.as_array())
        .map(|v| !v.is_empty())
        .unwrap_or(false)
    {
        return Err(
            "Codex provider does not support client-supplied tool schemas yet; remove `tools` or route this model to a non-Codex provider".to_string(),
        );
    }
    Ok(())
}

fn join_non_empty(parts: Vec<String>) -> Option<String> {
    let parts = parts
        .into_iter()
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n"))
    }
}

fn title_case_role(role: &str) -> &'static str {
    match role {
        "assistant" => "Assistant",
        "tool" => "Tool",
        _ => "User",
    }
}

fn flatten_content_value(content: Option<&Value>, images: &mut Vec<String>) -> String {
    match content {
        Some(Value::String(text)) => text.to_string(),
        Some(Value::Array(parts)) => {
            let mut text_parts = Vec::new();
            for part in parts {
                if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                    text_parts.push(text.to_string());
                    continue;
                }

                let part_type = part.get("type").and_then(|v| v.as_str()).unwrap_or("");
                if matches!(part_type, "input_image" | "image_url") {
                    if let Some(image_url) = part
                        .get("image_url")
                        .and_then(image_value_to_string)
                        .or_else(|| part.get("url").and_then(|v| v.as_str()).map(str::to_string))
                    {
                        images.push(image_url);
                    }
                }
            }
            text_parts.join("\n")
        }
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

fn image_value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.to_string()),
        Value::Object(map) => map.get("url").and_then(|v| v.as_str()).map(str::to_string),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_codex_target_prefix() {
        assert_eq!(strip_codex_target("codex:gpt-5.4"), Some("gpt-5.4".to_string()));
        assert_eq!(strip_codex_target("CoDeX:codex-mini"), Some("codex-mini".to_string()));
        assert_eq!(strip_codex_target("gpt-5.4"), None);
    }

    #[test]
    fn rejects_client_tools_for_codex_requests() {
        let body = json!({
            "model": "gpt-4o",
            "messages": [{ "role": "user", "content": "hi" }],
            "tools": [{ "type": "function", "function": { "name": "demo" }}]
        });
        let error = codex_request_from_chat_body(&body, "codex:gpt-5.4").unwrap_err();
        assert!(error.contains("client-supplied tool schemas"));
    }

    #[test]
    fn builds_chat_request_transcript_and_images() {
        let body = json!({
            "stream": true,
            "messages": [
                { "role": "system", "content": "You are precise." },
                { "role": "user", "content": [
                    { "type": "text", "text": "Describe this image" },
                    { "type": "input_image", "image_url": "https://example.com/cat.png" }
                ]},
                { "role": "assistant", "content": "Previous reply" }
            ]
        });

        let request = codex_request_from_chat_body(&body, "codex:gpt-5.4-mini").unwrap();
        assert_eq!(request.model, "gpt-5.4-mini");
        assert_eq!(request.developer_instructions.as_deref(), Some("You are precise."));
        assert!(request.stream);
        assert!(request.prompt_text.contains("User:\nDescribe this image"));
        assert!(request.prompt_text.contains("Assistant:\nPrevious reply"));
        assert_eq!(request.images, vec!["https://example.com/cat.png".to_string()]);
    }

    #[test]
    fn builds_responses_request_from_input_items() {
        let body = json!({
            "instructions": "Stay concise",
            "input": [
                {
                    "type": "message",
                    "role": "user",
                    "content": [{ "type": "input_text", "text": "Summarize this" }]
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": { "content": "tool result" }
                }
            ]
        });

        let request = codex_request_from_responses_body(&body, "codex:gpt-5.4").unwrap();
        assert_eq!(request.developer_instructions.as_deref(), Some("Stay concise"));
        assert!(request.prompt_text.contains("User:\nSummarize this"));
        assert!(request.prompt_text.contains("Tool Output:"));
    }
}
