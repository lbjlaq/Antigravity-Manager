use crate::storage::StorageManager;
use crate::types::TargetHost;
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// 单次请求的实时性能与指标快照
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RealtimeRequestSnapshot {
    pub timestamp: String,
    pub step_index: u64,
    pub host: TargetHost,
    pub account_email: Option<String>,
    pub model: String,
    pub input_tokens: u64,
    pub cache_read_tokens: u64,
    pub output_tokens: u64,
    pub duration_seconds: f64,
    pub tps: f64,
    pub first_token_latency_seconds: f64,
}

#[derive(Debug, Deserialize)]
struct TranscriptTurn {
    #[serde(default)]
    step_index: u64,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    cache_read_tokens: Option<u64>,
    #[serde(default)]
    output_tokens: Option<u64>,
    #[serde(default)]
    created_at: Option<String>,
}

pub struct RealtimeMonitor {
    _storage: StorageManager,
}

impl RealtimeMonitor {
    pub fn new() -> Result<Self> {
        Ok(Self {
            _storage: StorageManager::new()?,
        })
    }

    /// 探测最新活跃的会话日志路径
    pub fn locate_latest_transcript() -> Option<PathBuf> {
        let home = dirs::home_dir()?;
        let brain_dir = home.join(".gemini").join("antigravity").join("brain");
        if !brain_dir.exists() {
            return None;
        }

        let mut latest_file: Option<(PathBuf, std::time::SystemTime)> = None;

        if let Ok(entries) = std::fs::read_dir(brain_dir) {
            for entry in entries.flatten() {
                let candidate = entry
                    .path()
                    .join(".system_generated")
                    .join("logs")
                    .join("transcript.jsonl");

                if candidate.exists() {
                    if let Ok(meta) = candidate.metadata() {
                        if let Ok(mtime) = meta.modified() {
                            if latest_file.as_ref().map_or(true, |(_, t)| mtime > *t) {
                                latest_file = Some((candidate, mtime));
                            }
                        }
                    }
                }
            }
        }

        latest_file.map(|(p, _)| p)
    }

    /// 提取指定 transcript.jsonl 中最后一次请求的完整指标与 TPS
    pub fn extract_latest_snapshot(&self, path: &Path) -> Result<Option<RealtimeRequestSnapshot>> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);

        // 读取末尾最后 60 行
        let mut lines = Vec::new();
        for line in reader.lines().flatten() {
            if !line.trim().is_empty() {
                lines.push(line);
                if lines.len() > 60 {
                    lines.remove(0);
                }
            }
        }

        if lines.is_empty() {
            return Ok(None);
        }

        let mut parsed_turns: Vec<TranscriptTurn> = Vec::new();
        for l in &lines {
            if let Ok(t) = serde_json::from_str::<TranscriptTurn>(l) {
                parsed_turns.push(t);
            }
        }

        // 倒序寻找具有 output_tokens 的模型响应
        for i in (1..parsed_turns.len()).rev() {
            let curr = &parsed_turns[i];
            let prev = &parsed_turns[i - 1];

            if let Some(out_tokens) = curr.output_tokens {
                if out_tokens > 0 {
                    let model = curr.model.clone().unwrap_or_else(|| "gemini-3.8-flash".to_string());
                    let in_tokens = curr.input_tokens.unwrap_or(0);
                    let cache_tokens = curr.cache_read_tokens.unwrap_or(0);

                    // 计算耗时与 TPS
                    let t_curr = curr
                        .created_at
                        .as_deref()
                        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(Utc::now);

                    let t_prev = prev
                        .created_at
                        .as_deref()
                        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|| t_curr - chrono::Duration::seconds(1));

                    let duration_ms = (t_curr - t_prev).num_milliseconds().max(500) as f64;
                    let duration_sec = duration_ms / 1000.0;
                    let tps = (out_tokens as f64) / duration_sec;

                    // 首字延迟粗算 (小输出约等于响应起点)
                    let first_token_latency = (duration_sec * 0.25).max(0.8);

                    // 探测当前活跃账号与宿主
                    let active_hosts = crate::switcher::SwitchEngine::detect_active_hosts();
                    let account_email = active_hosts.app_account.or(active_hosts.ide_account);

                    return Ok(Some(RealtimeRequestSnapshot {
                        timestamp: t_curr.format("%H:%M:%S").to_string(),
                        step_index: curr.step_index,
                        host: TargetHost::App,
                        account_email,
                        model,
                        input_tokens: in_tokens,
                        cache_read_tokens: cache_tokens,
                        output_tokens: out_tokens,
                        duration_seconds: duration_sec,
                        tps,
                        first_token_latency_seconds: first_token_latency,
                    }));
                }
            }
        }

        Ok(None)
    }
}
