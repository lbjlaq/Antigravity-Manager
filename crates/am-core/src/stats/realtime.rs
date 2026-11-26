use crate::storage::StorageManager;
use crate::types::TargetHost;
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
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
struct RawToolStub {}

#[derive(Debug, Deserialize)]
struct TranscriptTurn {
    #[serde(default)]
    step_index: u64,
    #[serde(rename = "type", default)]
    step_type: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    cache_read_tokens: Option<u64>,
    #[serde(default)]
    output_tokens: Option<u64>,
    #[serde(default)]
    tool_calls: Option<Vec<RawToolStub>>,
    #[serde(default)]
    created_at: Option<String>,
}

pub struct RealtimeMonitor {
    storage: StorageManager,
}

impl RealtimeMonitor {
    pub fn new() -> Result<Self> {
        Ok(Self {
            storage: StorageManager::new()?,
        })
    }

    /// 探测最新活跃的会话日志路径（优先原生 `conversations/*.db`，兼容回退 `brain/*/transcript.jsonl`）
    pub fn locate_latest_transcript() -> Option<PathBuf> {
        let home = dirs::home_dir()?;
        let gemini_dir = home.join(".gemini").join("antigravity");

        let convs_dir = gemini_dir.join("conversations");
        if convs_dir.exists() {
            let mut latest_db: Option<(PathBuf, i64)> = None;
            if let Ok(entries) = std::fs::read_dir(&convs_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|s| s.to_str()) == Some("db") {
                        let mtime_ms = super::conv_db::effective_db_mtime_ms(&path);
                        if mtime_ms > 0 && latest_db.as_ref().map_or(true, |(_, t)| mtime_ms > *t) {
                            latest_db = Some((path, mtime_ms));
                        }
                    }
                }
            }
            if let Some((p, _)) = latest_db {
                return Some(p);
            }
        }

        let brain_dir = gemini_dir.join("brain");
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

    /// 提取指定会话数据库（`.db`）或 `transcript.jsonl` 中最后一次请求的完整指标与 TPS
    pub fn extract_latest_snapshot(&self, path: &Path) -> Result<Option<RealtimeRequestSnapshot>> {
        if path.extension().and_then(|s| s.to_str()) == Some("db") {
            if let Some((cached, _)) = super::conv_db::load_or_parse_conversation_db(path) {
                if let Some(last) = cached.turns.iter().rev().find(|t| t.output_tokens > 0) {
                    let active_hosts =
                        crate::AntigravityManager::resolve_active_hosts_with_storage(&self.storage);
                    let account_email = active_hosts
                        .ide_account
                        .or(active_hosts.app_account)
                        .or(active_hosts.agy_account)
                        .or(active_hosts.plugin_account);
                    let ts_str = last
                        .time_str
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or(&last.time_str)
                        .to_string();
                    return Ok(Some(RealtimeRequestSnapshot {
                        timestamp: ts_str,
                        step_index: last.step_index,
                        host: TargetHost::App,
                        account_email,
                        model: last.requested_model.clone(),
                        input_tokens: last.input_tokens,
                        cache_read_tokens: last.cache_tokens,
                        output_tokens: last.output_tokens,
                        duration_seconds: last.duration_ms as f64 / 1000.0,
                        tps: last.tps,
                        first_token_latency_seconds: last.ttft_ms as f64 / 1000.0,
                    }));
                }
            }
            return Ok(None);
        }
        let mut file = File::open(path)?;
        let file_len = file.metadata().map(|m| m.len()).unwrap_or(0);
        let tail_window = 128 * 1024u64;
        let seek_offset = file_len.saturating_sub(tail_window);
        if seek_offset > 0 {
            let _ = file.seek(SeekFrom::Start(seek_offset));
        }

        let mut reader = BufReader::new(file);
        if seek_offset > 0 {
            let mut partial = String::new();
            let _ = reader.read_line(&mut partial);
        }

        // O(1) 环形保留末尾最后 60 行
        let mut lines: VecDeque<String> = VecDeque::with_capacity(61);
        for line in reader.lines().map_while(Result::ok) {
            if !line.trim().is_empty() {
                if lines.len() >= 60 {
                    lines.pop_front();
                }
                lines.push_back(line);
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

        // 倒序寻找最近一次具有 output_tokens 的模型响应，并向后匹配同轮工具 GENERIC step 测算真实耗时
        for i in (0..parsed_turns.len()).rev() {
            let curr = &parsed_turns[i];

            if curr.source.as_deref() != Some("MODEL") {
                continue;
            }
            if let Some(out_tokens) = curr.output_tokens {
                if out_tokens > 0 {
                    let model = curr
                        .model
                        .clone()
                        .unwrap_or_else(|| "gemini-3.8-flash".to_string());
                    let in_tokens = curr.input_tokens.unwrap_or(0);
                    let cache_tokens = curr.cache_read_tokens.unwrap_or(0);

                    let t_curr_opt = curr
                        .created_at
                        .as_deref()
                        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                        .map(|dt| dt.with_timezone(&Utc));
                    let t_curr = t_curr_opt.unwrap_or_else(Utc::now);

                    let tool_count = curr.tool_calls.as_ref().map(|v| v.len()).unwrap_or(0);
                    let mut duration_sec = 0.0;
                    let mut tps = 0.0;

                    if let (Some(t0), true) = (t_curr_opt, tool_count > 0) {
                        let mut last_gen_dt: Option<DateTime<Utc>> = None;
                        let mut matched = 0usize;
                        for next_step in &parsed_turns[(i + 1)..] {
                            if matched >= tool_count {
                                break;
                            }
                            if next_step.step_type.as_deref() == Some("GENERIC")
                                && next_step.source.as_deref() == Some("MODEL")
                            {
                                if let Some(dt) = next_step
                                    .created_at
                                    .as_deref()
                                    .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                                    .map(|dt| dt.with_timezone(&Utc))
                                {
                                    last_gen_dt = Some(dt);
                                }
                                matched += 1;
                            } else {
                                break;
                            }
                        }

                        if let Some(t_end) = last_gen_dt {
                            let diff_ms = (t_end - t0).num_milliseconds();
                            if (1_000..=600_000).contains(&diff_ms) {
                                duration_sec = diff_ms as f64 / 1000.0;
                                tps = (out_tokens as f64) / duration_sec;
                            }
                        }
                    }

                    // 探测当前活跃真实绑定账号与宿主
                    let active_hosts =
                        crate::AntigravityManager::resolve_active_hosts_with_storage(&self.storage);
                    let account_email = active_hosts
                        .ide_account
                        .or(active_hosts.app_account)
                        .or(active_hosts.agy_account)
                        .or(active_hosts.plugin_account);

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
                        first_token_latency_seconds: 0.0,
                    }));
                }
            }
        }

        Ok(None)
    }
}
