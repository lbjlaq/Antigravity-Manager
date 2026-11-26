mod conv_db;
pub mod models;
pub mod realtime;

pub use models::*;
pub use realtime::*;

use crate::storage::StorageManager;
use crate::types::TargetHost;
use anyhow::Result;
use chrono::{DateTime, Timelike};
use rusqlite::{Connection, OpenFlags};
use serde::de::{self, Deserializer, Visitor};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::UNIX_EPOCH;

pub struct StatsEngine;

/// 零拷贝/零大对象分配的 `thinking` 字节长度解析器（跳过数 KB 思维链字符串堆分配）
fn deserialize_str_len<'de, D>(deserializer: D) -> std::result::Result<usize, D::Error>
where
    D: Deserializer<'de>,
{
    struct StrLenVisitor;
    impl<'de> Visitor<'de> for StrLenVisitor {
        type Value = usize;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("optional string")
        }
        fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<usize, E> {
            Ok(v.len())
        }
        fn visit_string<E: de::Error>(self, v: String) -> std::result::Result<usize, E> {
            Ok(v.len())
        }
        fn visit_none<E: de::Error>(self) -> std::result::Result<usize, E> {
            Ok(0)
        }
        fn visit_unit<E: de::Error>(self) -> std::result::Result<usize, E> {
            Ok(0)
        }
        fn visit_some<D2: Deserializer<'de>>(self, d: D2) -> std::result::Result<usize, D2::Error> {
            d.deserialize_any(self)
        }
        fn visit_seq<A: de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> std::result::Result<usize, A::Error> {
            while seq.next_element::<de::IgnoredAny>()?.is_some() {}
            Ok(0)
        }
        fn visit_map<A: de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> std::result::Result<usize, A::Error> {
            while map
                .next_entry::<de::IgnoredAny, de::IgnoredAny>()?
                .is_some()
            {}
            Ok(0)
        }
    }
    deserializer.deserialize_any(StrLenVisitor)
}

/// 仅当 `content` 包含模型切换标记时提取模型名，其余 99.9% 的长文本直接流式丢弃（0 String 分配）
fn deserialize_model_selection<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    fn extract_model(content: &str) -> Option<String> {
        let pos = content.find("Model Selection` from")?;
        let sub = &content[pos..];
        let to_pos = sub.find(" to ")?;
        let after = &sub[to_pos + 4..];
        let m_name = after.split('.').next().unwrap_or("").trim();
        if m_name.is_empty() {
            None
        } else {
            Some(m_name.to_string())
        }
    }

    struct ModelSelVisitor;
    impl<'de> Visitor<'de> for ModelSelVisitor {
        type Value = Option<String>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("optional string")
        }
        fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Option<String>, E> {
            Ok(extract_model(v))
        }
        fn visit_string<E: de::Error>(self, v: String) -> std::result::Result<Option<String>, E> {
            Ok(extract_model(&v))
        }
        fn visit_none<E: de::Error>(self) -> std::result::Result<Option<String>, E> {
            Ok(None)
        }
        fn visit_unit<E: de::Error>(self) -> std::result::Result<Option<String>, E> {
            Ok(None)
        }
        fn visit_some<D2: Deserializer<'de>>(
            self,
            d: D2,
        ) -> std::result::Result<Option<String>, D2::Error> {
            d.deserialize_any(self)
        }
        fn visit_seq<A: de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> std::result::Result<Option<String>, A::Error> {
            while seq.next_element::<de::IgnoredAny>()?.is_some() {}
            Ok(None)
        }
        fn visit_map<A: de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> std::result::Result<Option<String>, A::Error> {
            while map
                .next_entry::<de::IgnoredAny, de::IgnoredAny>()?
                .is_some()
            {}
            Ok(None)
        }
    }
    deserializer.deserialize_any(ModelSelVisitor)
}

#[derive(Deserialize)]
struct RawToolCall {
    #[serde(default)]
    name: Option<String>,
}

/// 精简强类型日志行：反序列化时直接忽略大字段，替代整行 `serde_json::Value` 构造
#[derive(Deserialize)]
struct RawTranscriptLine {
    #[serde(rename = "type", default)]
    step_type: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    step_index: u64,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    cache_read_tokens: u64,
    #[serde(default)]
    tool_calls: Option<Vec<RawToolCall>>,
    #[serde(default, deserialize_with = "deserialize_str_len")]
    thinking: usize,
    #[serde(default, deserialize_with = "deserialize_model_selection")]
    content: Option<String>,
}

#[derive(Clone)]
struct CachedToolStep {
    step_ts_ms: i64,
    is_err: bool,
    tools: Vec<String>,
}

#[derive(Clone)]
struct CachedModelTurn {
    step_index: u64,
    created_at_ts: i64,
    hour: u8,
    time_str: String,
    model: String,
    input_tokens: u64,
    output_tokens: u64,
    cache_tokens: u64,
    thinking_tokens: u64,
    duration_ms: u64,
    ttft_ms: u64,
    tps: f64,
}

#[derive(Clone)]
struct CachedTranscriptFile {
    mtime_ms: i64,
    file_len: u64,
    step_timestamps: Vec<i64>,
    tool_steps: Vec<CachedToolStep>,
    turns: Vec<CachedModelTurn>,
}

static TRANSCRIPT_CACHE: OnceLock<Mutex<HashMap<PathBuf, CachedTranscriptFile>>> = OnceLock::new();

#[derive(Default)]
struct TranscriptTurn {
    conv_id: String,
    step_index: u64,
    created_at_ts: i64,
    hour: u8,
    time_str: String,
    model: String,
    physical_model: String,
    is_downgraded: bool,
    input_tokens: u64,
    output_tokens: u64,
    cache_tokens: u64,
    thinking_tokens: u64,
    duration_ms: u64,
    streaming_duration_ms: u64,
    ttft_ms: u64,
    tps: f64,
    context_used_tokens: u64,
    context_budget_tokens: u64,
}

#[derive(Default)]
struct ModelAgg {
    turns: u64,
    input: u64,
    output: u64,
    cache: u64,
    thinking: u64,
    measured_output: u64,
    measured_duration_ms: u64,
    measured_turns: u64,
    peak_tps: f64,
    ttft_sum: u64,
    ttft_cnt: u64,
    physical_model: String,
    is_downgraded: bool,
    ws_tokens: HashMap<String, u64>,
}

#[derive(Default)]
struct WorkspaceAgg {
    path: String,
    steps: u64,
    tokens: u64,
    estimated_cost_usd: f64,
    convs: HashSet<String>,
    last_active: String,
    titles: Vec<String>,
    model_tokens: HashMap<String, u64>,
}

const MIN_MEASURED_DURATION_MS: i64 = 1_000;
const MAX_MEASURED_DURATION_MS: i64 = 600_000;

impl StatsEngine {
    /// 流式解析 transcript.jsonl：使用向后匹配本轮工具 `GENERIC` step 的结束时间测算真实生成耗时与 tok/s
    ///
    /// 说明：`transcript.jsonl` 的 `created_at` 为各 step 的开始创建时间（秒级），不记录原生 TTFT 或纯文本响应结束时间。
    /// 当模型响应携带 `tool_calls` 时，Antigravity 会在每个工具调用流式输出完成后立即创建对应 `GENERIC` step，
    /// 因此本轮最后一个 `GENERIC` step 的 `created_at` 减去当前 `PLANNER_RESPONSE` 的 `created_at` 即为该轮真实生成耗时；
    /// 无工具调用或无法测算的轮次不再用硬编码经验公式伪造速度和 TTFT（保持 0 / 未测算）。
    fn parse_transcript_reader<R: BufRead>(
        mut reader: R,
        mtime_ms: i64,
        file_len: u64,
    ) -> CachedTranscriptFile {
        let mut line_buf = String::with_capacity(4096);

        let mut current_model = "Gemini 3.8 Flash High".to_string();
        let mut prev_step_ts_ms: Option<i64> = None;
        // (turn_idx, expected_tool_count, matched_generic_count)
        let mut pending_turn: Option<(usize, usize, usize)> = None;
        let mut step_timestamps = Vec::new();
        let mut tool_steps = Vec::new();
        let mut turns: Vec<CachedModelTurn> = Vec::new();

        loop {
            line_buf.clear();
            match reader.read_line(&mut line_buf) {
                Ok(0) => break,
                Ok(_) => {}
                Err(_) => break,
            }
            let trimmed = line_buf.trim();
            if trimmed.is_empty() {
                continue;
            }

            let Ok(raw) = serde_json::from_str::<RawTranscriptLine>(trimmed) else {
                continue;
            };

            if raw.step_type.as_deref() == Some("USER_INPUT") {
                if let Some(m_name) = raw.content {
                    current_model = m_name;
                }
            }

            let parsed_dt = raw
                .created_at
                .as_deref()
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.with_timezone(&chrono::Local));

            let explicit_ts_ms = parsed_dt.map(|dt| dt.timestamp_millis());
            let step_ts_ms = explicit_ts_ms.or(prev_step_ts_ms).unwrap_or(0);

            let is_model_generic = raw.step_type.as_deref() == Some("GENERIC")
                && raw.source.as_deref() == Some("MODEL");

            if let Some((turn_idx, expected_tools, matched_generics)) = pending_turn {
                if is_model_generic && matched_generics < expected_tools {
                    let mut invalidated = false;
                    if let Some(gen_ts_ms) = explicit_ts_ms {
                        if let Some(turn) = turns.get_mut(turn_idx) {
                            let diff = gen_ts_ms - turn.created_at_ts;
                            if (MIN_MEASURED_DURATION_MS..=MAX_MEASURED_DURATION_MS).contains(&diff)
                            {
                                let dur = diff as u64;
                                turn.duration_ms = dur;
                                turn.tps = (turn.output_tokens as f64 * 1000.0) / dur as f64;
                            } else if diff > MAX_MEASURED_DURATION_MS {
                                turn.duration_ms = 0;
                                turn.tps = 0.0;
                                invalidated = true;
                            }
                        }
                    }
                    let next_matched = matched_generics + 1;
                    pending_turn = if !invalidated && next_matched < expected_tools {
                        Some((turn_idx, expected_tools, next_matched))
                    } else {
                        None
                    };
                } else {
                    pending_turn = None;
                }
            }

            let tool_call_count = raw.tool_calls.as_ref().map(|v| v.len()).unwrap_or(0);

            if step_ts_ms > 0 {
                step_timestamps.push(step_ts_ms);

                if let Some(tcalls) = raw.tool_calls {
                    let tools: Vec<String> = tcalls
                        .into_iter()
                        .filter_map(|tc| tc.name)
                        .filter(|n| !n.is_empty())
                        .collect();
                    if !tools.is_empty() {
                        tool_steps.push(CachedToolStep {
                            step_ts_ms,
                            is_err: raw.status.as_deref() == Some("ERROR"),
                            tools,
                        });
                    }
                }
            }

            if raw.source.as_deref() == Some("MODEL")
                && step_ts_ms > 0
                && (raw.input_tokens > 0 || raw.output_tokens > 0 || raw.cache_read_tokens > 0)
            {
                let model_name = raw
                    .model
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| current_model.clone());

                let (time_str, hour) = if let Some(local_dt) = parsed_dt {
                    (
                        local_dt.format("%Y-%m-%d %H:%M:%S").to_string(),
                        local_dt.hour().min(23) as u8,
                    )
                } else {
                    (String::new(), 12)
                };

                let turn_idx = turns.len();
                turns.push(CachedModelTurn {
                    step_index: raw.step_index,
                    created_at_ts: step_ts_ms,
                    hour,
                    time_str,
                    model: model_name,
                    input_tokens: raw.input_tokens,
                    output_tokens: raw.output_tokens,
                    cache_tokens: raw.cache_read_tokens,
                    thinking_tokens: (raw.thinking / 4) as u64,
                    duration_ms: 0,
                    ttft_ms: 0,
                    tps: 0.0,
                });

                if explicit_ts_ms.is_some() && tool_call_count > 0 && raw.output_tokens > 0 {
                    pending_turn = Some((turn_idx, tool_call_count, 0));
                }
            }

            if step_ts_ms > 0 {
                prev_step_ts_ms = Some(step_ts_ms);
            }
        }

        CachedTranscriptFile {
            mtime_ms,
            file_len,
            step_timestamps,
            tool_steps,
            turns,
        }
    }

    /// 读取或增量缓存单个 transcript.jsonl（按 `(mtime_ms, file_len)` 命中，未改动文件 0 磁盘 IO / 0 解析）
    fn load_or_parse_transcript_file(
        transcript_path: &Path,
        mtime_ms: i64,
        file_len: u64,
    ) -> Option<(CachedTranscriptFile, bool)> {
        let cache = TRANSCRIPT_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        if let Ok(guard) = cache.lock() {
            if let Some(entry) = guard.get(transcript_path) {
                if entry.mtime_ms == mtime_ms && entry.file_len == file_len {
                    return Some((entry.clone(), false));
                }
            }
        }

        let file = File::open(transcript_path).ok()?;
        let reader = BufReader::with_capacity(64 * 1024, file);
        let parsed = Self::parse_transcript_reader(reader, mtime_ms, file_len);

        if let Ok(mut guard) = cache.lock() {
            guard.insert(transcript_path.to_path_buf(), parsed.clone());
        }
        Some((parsed, true))
    }

    /// 轻量统计本地已记录总请求数（优先读取原生 conversations/*.db 并复用文件级缓存，回退 transcript.jsonl）
    pub fn count_total_requests() -> u64 {
        let Some(home) = dirs::home_dir() else {
            return 0;
        };
        let gemini_dir = home.join(".gemini/antigravity");
        let convs_dir = gemini_dir.join("conversations");
        let brain_dir = gemini_dir.join("brain");

        let mut conv_ids: HashSet<String> = HashSet::new();
        if let Ok(entries) = std::fs::read_dir(&convs_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("db") {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        if !stem.is_empty() {
                            conv_ids.insert(stem.to_string());
                        }
                    }
                }
            }
        }
        if let Ok(entries) = std::fs::read_dir(&brain_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                        if !name.is_empty() {
                            conv_ids.insert(name.to_string());
                        }
                    }
                }
            }
        }

        let mut total = 0u64;
        let mut cold_parsed = 0usize;
        for conv_id in conv_ids {
            let db_path = convs_dir.join(format!("{}.db", conv_id));
            if db_path.exists() {
                if let Some((cached, was_cold)) = conv_db::load_or_parse_conversation_db(&db_path) {
                    total += cached.turns.len() as u64;
                    if was_cold {
                        cold_parsed += 1;
                    }
                    continue;
                }
            }
            let transcript_path = brain_dir
                .join(&conv_id)
                .join(".system_generated")
                .join("logs")
                .join("transcript.jsonl");
            let Ok(meta) = transcript_path.metadata() else {
                continue;
            };
            let mtime_ms = meta
                .modified()
                .ok()
                .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            if let Some((cached, was_cold)) =
                Self::load_or_parse_transcript_file(&transcript_path, mtime_ms, meta.len())
            {
                total += cached.turns.len() as u64;
                if was_cold {
                    cold_parsed += 1;
                }
            }
        }

        if cold_parsed >= 2 {
            crate::runtime::trim_process_memory();
        }
        total
    }

    /// 全量历史分析报告（默认不限起止时间）
    pub fn generate_master_report(storage: &StorageManager) -> Result<MasterAnalyticsReport> {
        Self::generate_report_in_range(storage, None, None)
    }

    /// 支持任意时间窗口 `[start_ts_ms, end_ts_ms]` 的高精度多维分析引擎（全量保留窗口内真实请求流水）
    pub fn generate_report_in_range(
        storage: &StorageManager,
        start_ts_ms: Option<i64>,
        end_ts_ms: Option<i64>,
    ) -> Result<MasterAnalyticsReport> {
        let home = match dirs::home_dir() {
            Some(h) => h,
            None => return Ok(MasterAnalyticsReport::default()),
        };

        let is_unbounded = start_ts_ms.is_none() && end_ts_ms.is_none();
        let in_range = |ts_ms: i64| -> bool {
            if let Some(s) = start_ts_ms {
                if ts_ms < s {
                    return false;
                }
            }
            if let Some(e) = end_ts_ms {
                if ts_ms > e {
                    return false;
                }
            }
            true
        };

        let gemini_dir = home.join(".gemini/antigravity");
        let conv_db_path = gemini_dir.join("conversation_summaries.db");
        let active_account_path = gemini_dir.join("active_account.json");
        let convs_dir = gemini_dir.join("conversations");
        let brain_dir = gemini_dir.join("brain");

        let mut report = MasterAnalyticsReport::default();

        // 1. 采集本地账号池
        let accounts = storage.list_full_accounts().unwrap_or_default();
        let mut account_map = HashMap::new();
        for acc in &accounts {
            account_map.insert(acc.email.clone(), acc.clone());
        }

        // 读取当前活跃宿主真实账号
        let mut active_ide_email =
            crate::switcher::SwitchEngine::extract_refresh_token_from_sqlite("Antigravity IDE")
                .and_then(|rt| {
                    accounts
                        .iter()
                        .find(|a| a.token.refresh_token.trim() == rt.trim())
                        .map(|a| a.email.clone())
                })
                .or_else(|| {
                    crate::switcher::SwitchEngine::read_account_from_sqlite("Antigravity IDE")
                })
                .or_else(|| {
                    crate::switcher::SwitchEngine::read_system_keyring_credential().and_then(
                        |(rt_opt, em_opt)| {
                            rt_opt
                                .and_then(|rt| {
                                    accounts
                                        .iter()
                                        .find(|a| a.token.refresh_token.trim() == rt.trim())
                                        .map(|a| a.email.clone())
                                })
                                .or(em_opt)
                        },
                    )
                })
                .unwrap_or_default();

        if active_ide_email.is_empty() && active_account_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&active_account_path) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(em) = v.get("email").and_then(|e| e.as_str()) {
                        active_ide_email = em.to_string();
                    }
                }
            }
        }
        if active_ide_email.is_empty() {
            active_ide_email = accounts
                .first()
                .map(|a| a.email.clone())
                .unwrap_or_default();
        }

        // 2. 从 conversation_summaries.db 提取会话元数据映射
        let mut active_session_id = String::new();
        let mut active_session_title = String::new();
        let mut active_workspace = String::new();
        let mut active_step_count = 0u64;
        let mut active_last_mod = String::new();
        let mut active_last_user_input = String::new();

        // conv_id -> (title, steps, last_mod, last_input, ws_name, ws_path, nesting_depth, is_running)
        let mut conv_meta: HashMap<
            String,
            (String, u64, String, String, String, String, i64, bool),
        > = HashMap::new();
        let mut running_sessions_now = 0u64;

        if conv_db_path.exists() {
            let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
            if let Ok(conn) = Connection::open_with_flags(&conv_db_path, flags) {
                if let Ok(mut stmt) = conn.prepare(
                    "SELECT conversation_id, title, step_count, last_modified_time, last_user_input_time, workspace_uris, \
                     COALESCE(nesting_depth, 0), COALESCE(status, '') \
                     FROM conversation_summaries ORDER BY last_modified_time DESC",
                ) {
                    let rows = stmt.query_map([], |row| {
                        let id: String = row.get(0)?;
                        let title: String = row.get(1)?;
                        let steps: i64 = row.get(2)?;
                        let last_mod: String = row.get(3)?;
                        let last_input: String = row.get(4).unwrap_or_default();
                        let uris: String = row.get(5).unwrap_or_default();
                        let depth: i64 = row.get(6).unwrap_or(0);
                        let status: String = row.get(7).unwrap_or_default();
                        Ok((
                            id,
                            title,
                            steps.max(0) as u64,
                            last_mod,
                            last_input,
                            uris,
                            depth,
                            status == "CASCADE_RUN_STATUS_RUNNING",
                        ))
                    });

                    if let Ok(mapped_rows) = rows {
                        for (idx, item) in mapped_rows.flatten().enumerate() {
                            let (id, title, steps, last_mod, last_input, uris, depth, is_running) =
                                item;
                            if is_running {
                                running_sessions_now += 1;
                            }

                            let ws_name = Self::extract_workspace_name(&uris);
                            let ws_path = Self::extract_workspace_path(&uris);

                            if idx == 0 {
                                active_session_id = id.clone();
                                active_session_title = title.clone();
                                active_workspace = ws_name.clone();
                                active_step_count = steps;
                                active_last_mod = last_mod.clone();
                                active_last_user_input = last_input.clone();
                            }

                            conv_meta.insert(
                                id,
                                (
                                    title,
                                    steps,
                                    last_mod,
                                    last_input,
                                    ws_name,
                                    ws_path,
                                    depth,
                                    is_running,
                                ),
                            );
                        }
                    }
                }
            }
        }

        // 3. 优先扫描原生 conversations/*.db（纳秒级 TTFT/流式耗时/精确思考 Token/规范模型名/上下文水位），
        //    对无 `.db` 或打开失败的会话自动回退 brain/*/transcript.jsonl
        let mut turns: Vec<TranscriptTurn> = Vec::new();
        let mut model_aggs: HashMap<String, ModelAgg> = HashMap::new();
        let mut ws_aggs: HashMap<String, WorkspaceAgg> = HashMap::new();
        let mut tool_totals: HashMap<String, (u64, u64)> = HashMap::new();
        let mut active_convs_in_range: HashSet<String> = HashSet::new();
        let mut total_steps_in_range = 0u64;
        let mut user_prompt_count_in_range = 0u64;
        let mut checkpoint_count_in_range = 0u64;
        let mut total_tool_exec_ms = 0u64;
        let mut api_errors: Vec<ApiErrorEvent> = Vec::new();
        let mut session_contexts: Vec<SessionContextItem> = Vec::new();
        let mut hourly_heatmap = vec![0u32; 24];

        let mut cold_parsed_files = 0usize;
        let mut live_db_paths: HashSet<PathBuf> = HashSet::new();
        let mut live_transcript_paths: HashSet<PathBuf> = HashSet::new();
        let mut all_conv_ids: HashSet<String> = HashSet::new();

        if convs_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&convs_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|s| s.to_str()) == Some("db") {
                        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                            if !stem.is_empty() {
                                all_conv_ids.insert(stem.to_string());
                                live_db_paths.insert(path);
                            }
                        }
                    }
                }
            }
        }
        if brain_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&brain_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                            if !name.is_empty() {
                                all_conv_ids.insert(name.to_string());
                                let tp = path
                                    .join(".system_generated")
                                    .join("logs")
                                    .join("transcript.jsonl");
                                if tp.exists() {
                                    live_transcript_paths.insert(tp);
                                }
                            }
                        }
                    }
                }
            }
        }

        for conv_id in all_conv_ids {
            let meta_ref = conv_meta.get(&conv_id);
            let ws_name = meta_ref
                .map(|m| m.4.clone())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "default".to_string());
            let ws_path = meta_ref
                .map(|m| m.5.clone())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| ws_name.clone());
            let conv_title = meta_ref
                .map(|m| m.0.clone())
                .unwrap_or_else(|| "IDE 任务会话".to_string());

            let db_path = convs_dir.join(format!("{}.db", conv_id));
            let mut loaded_from_db = false;

            if db_path.exists() {
                let eff_mtime = conv_db::effective_db_mtime_ms(&db_path);
                if let Some(min_ts) = start_ts_ms {
                    if eff_mtime > 0 && eff_mtime < min_ts - 60_000 {
                        continue;
                    }
                }
                if let Some((cached, was_cold)) = conv_db::load_or_parse_conversation_db(&db_path) {
                    loaded_from_db = true;
                    if was_cold {
                        cold_parsed_files += 1;
                    }

                    for &step_ts_ms in &cached.step_timestamps {
                        if in_range(step_ts_ms) {
                            total_steps_in_range += 1;
                            active_convs_in_range.insert(conv_id.clone());
                        }
                    }

                    for &ts_ms in &cached.user_prompt_timestamps {
                        if in_range(ts_ms) {
                            user_prompt_count_in_range += 1;
                        }
                    }

                    let mut conv_checkpoints_in_range = 0u64;
                    for &ts_ms in &cached.checkpoint_timestamps {
                        if in_range(ts_ms) {
                            checkpoint_count_in_range += 1;
                            conv_checkpoints_in_range += 1;
                        }
                    }

                    for &(ts_ms, exec_ms) in &cached.tool_exec_durations {
                        if in_range(ts_ms) {
                            total_tool_exec_ms = total_tool_exec_ms.saturating_add(exec_ms);
                        }
                    }

                    for err in &cached.api_errors {
                        if in_range(err.step_ts_ms) {
                            api_errors.push(ApiErrorEvent {
                                timestamp: err.step_ts_ms,
                                time_str: err.time_str.clone(),
                                code: err.code.clone(),
                                model: err.model.clone(),
                                workspace_name: ws_name.clone(),
                                session_title: conv_title.clone(),
                                conversation_id: conv_id.clone(),
                                message: err.message.clone(),
                            });
                        }
                    }

                    for tstep in &cached.tool_steps {
                        if in_range(tstep.step_ts_ms) {
                            for tname in &tstep.tools {
                                let entry = tool_totals.entry(tname.clone()).or_insert((0, 0));
                                entry.0 += 1;
                                if tstep.is_err {
                                    entry.1 += 1;
                                }
                            }
                        }
                    }

                    let mut latest_ctx_turn: Option<&conv_db::DbModelTurn> = None;

                    for t in &cached.turns {
                        if !in_range(t.created_at_ts) {
                            continue;
                        }
                        latest_ctx_turn = Some(t);

                        hourly_heatmap[(t.hour as usize).clamp(0, 23)] += 1;
                        let tot = t.input_tokens + t.output_tokens + t.cache_tokens;
                        let (in_p, cac_p, out_p) =
                            Self::estimate_model_pricing(&t.requested_model, &t.physical_model);
                        let turn_cost = (t.input_tokens as f64 * in_p
                            + t.cache_tokens as f64 * cac_p
                            + t.output_tokens as f64 * out_p)
                            / 1_000_000.0;

                        let m_agg = model_aggs.entry(t.requested_model.clone()).or_default();
                        m_agg.turns += 1;
                        m_agg.input += t.input_tokens;
                        m_agg.output += t.output_tokens;
                        m_agg.cache += t.cache_tokens;
                        m_agg.thinking += t.thinking_tokens;
                        if !t.physical_model.is_empty()
                            && (m_agg.physical_model.is_empty()
                                || m_agg.physical_model == t.requested_model)
                        {
                            m_agg.physical_model = t.physical_model.clone();
                        }
                        if t.is_downgraded {
                            m_agg.is_downgraded = true;
                        }
                        if t.duration_ms > 0 && t.output_tokens > 0 {
                            m_agg.measured_output += t.output_tokens;
                            m_agg.measured_duration_ms += t.duration_ms;
                            m_agg.measured_turns += 1;
                        }
                        if t.tps > m_agg.peak_tps {
                            m_agg.peak_tps = t.tps;
                        }
                        if t.ttft_ms > 0 {
                            m_agg.ttft_sum += t.ttft_ms;
                            m_agg.ttft_cnt += 1;
                        }
                        *m_agg.ws_tokens.entry(ws_name.clone()).or_insert(0) += tot;

                        let w_agg =
                            ws_aggs
                                .entry(ws_name.clone())
                                .or_insert_with(|| WorkspaceAgg {
                                    path: ws_path.clone(),
                                    ..Default::default()
                                });
                        w_agg.steps += 1;
                        w_agg.tokens += tot;
                        w_agg.estimated_cost_usd += turn_cost;
                        w_agg.convs.insert(conv_id.clone());
                        if w_agg.last_active < t.time_str {
                            w_agg.last_active = t.time_str.clone();
                        }
                        if w_agg.titles.len() < 8
                            && !conv_title.is_empty()
                            && !w_agg.titles.contains(&conv_title)
                        {
                            w_agg.titles.push(conv_title.clone());
                        }
                        *w_agg
                            .model_tokens
                            .entry(t.requested_model.clone())
                            .or_insert(0) += tot;

                        turns.push(TranscriptTurn {
                            conv_id: conv_id.clone(),
                            step_index: t.step_index,
                            created_at_ts: t.created_at_ts,
                            hour: t.hour,
                            time_str: t.time_str.clone(),
                            model: t.requested_model.clone(),
                            physical_model: t.physical_model.clone(),
                            is_downgraded: t.is_downgraded,
                            input_tokens: t.input_tokens,
                            output_tokens: t.output_tokens,
                            cache_tokens: t.cache_tokens,
                            thinking_tokens: t.thinking_tokens,
                            duration_ms: t.duration_ms,
                            streaming_duration_ms: t.streaming_duration_ms,
                            ttft_ms: t.ttft_ms,
                            tps: t.tps,
                            context_used_tokens: t.context_used_tokens,
                            context_budget_tokens: t.context_budget_tokens,
                        });
                    }

                    if let Some(lt) = latest_ctx_turn {
                        let used = if lt.context_used_tokens > 0 {
                            lt.context_used_tokens
                        } else {
                            lt.input_tokens + lt.cache_tokens + lt.output_tokens
                        };
                        let budget = if lt.context_budget_tokens > 0 {
                            lt.context_budget_tokens
                        } else {
                            512_000
                        };
                        let usage_pct = ((used as f64 / budget as f64) * 100.0).min(120.0);
                        session_contexts.push(SessionContextItem {
                            session_id: conv_id.clone(),
                            title: conv_title.clone(),
                            workspace: ws_name.clone(),
                            model: lt.requested_model.clone(),
                            used_tokens: used,
                            budget_tokens: budget,
                            usage_pct,
                            checkpoint_count: conv_checkpoints_in_range,
                            is_running: meta_ref.map(|m| m.7).unwrap_or(false),
                            last_active_time: lt.time_str.clone(),
                        });
                    }
                }
            }

            if loaded_from_db {
                continue;
            }

            let transcript_path = brain_dir
                .join(&conv_id)
                .join(".system_generated")
                .join("logs")
                .join("transcript.jsonl");
            let Ok(meta) = transcript_path.metadata() else {
                continue;
            };
            let mtime_ms = meta
                .modified()
                .ok()
                .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            let file_len = meta.len();

            if let Some(min_ts) = start_ts_ms {
                if mtime_ms > 0 && mtime_ms < min_ts - 60_000 {
                    continue;
                }
            }

            let Some((cached, was_cold)) =
                Self::load_or_parse_transcript_file(&transcript_path, mtime_ms, file_len)
            else {
                continue;
            };
            if was_cold {
                cold_parsed_files += 1;
            }

            for &step_ts_ms in &cached.step_timestamps {
                if in_range(step_ts_ms) {
                    total_steps_in_range += 1;
                    active_convs_in_range.insert(conv_id.clone());
                }
            }

            for tstep in &cached.tool_steps {
                if in_range(tstep.step_ts_ms) {
                    for tname in &tstep.tools {
                        let entry = tool_totals.entry(tname.clone()).or_insert((0, 0));
                        entry.0 += 1;
                        if tstep.is_err {
                            entry.1 += 1;
                        }
                    }
                }
            }

            for t in &cached.turns {
                if !in_range(t.created_at_ts) {
                    continue;
                }

                hourly_heatmap[(t.hour as usize).clamp(0, 23)] += 1;
                let tot = t.input_tokens + t.output_tokens + t.cache_tokens;
                let phy = Self::map_physical_model(&t.model);

                let m_agg = model_aggs.entry(t.model.clone()).or_default();
                m_agg.turns += 1;
                m_agg.input += t.input_tokens;
                m_agg.output += t.output_tokens;
                m_agg.cache += t.cache_tokens;
                m_agg.thinking += t.thinking_tokens;
                if m_agg.physical_model.is_empty() {
                    m_agg.physical_model = phy.clone();
                }
                if t.duration_ms > 0 && t.output_tokens > 0 {
                    m_agg.measured_output += t.output_tokens;
                    m_agg.measured_duration_ms += t.duration_ms;
                    m_agg.measured_turns += 1;
                }
                if t.tps > m_agg.peak_tps {
                    m_agg.peak_tps = t.tps;
                }
                if t.ttft_ms > 0 {
                    m_agg.ttft_sum += t.ttft_ms;
                    m_agg.ttft_cnt += 1;
                }
                *m_agg.ws_tokens.entry(ws_name.clone()).or_insert(0) += tot;

                let w_agg = ws_aggs
                    .entry(ws_name.clone())
                    .or_insert_with(|| WorkspaceAgg {
                        path: ws_path.clone(),
                        ..Default::default()
                    });
                w_agg.steps += 1;
                w_agg.tokens += tot;
                w_agg.convs.insert(conv_id.clone());
                if w_agg.last_active < t.time_str {
                    w_agg.last_active = t.time_str.clone();
                }
                if w_agg.titles.len() < 8
                    && !conv_title.is_empty()
                    && !w_agg.titles.contains(&conv_title)
                {
                    w_agg.titles.push(conv_title.clone());
                }
                *w_agg.model_tokens.entry(t.model.clone()).or_insert(0) += tot;

                turns.push(TranscriptTurn {
                    conv_id: conv_id.clone(),
                    step_index: t.step_index,
                    created_at_ts: t.created_at_ts,
                    hour: t.hour,
                    time_str: t.time_str.clone(),
                    model: t.model.clone(),
                    physical_model: phy,
                    is_downgraded: false,
                    input_tokens: t.input_tokens,
                    output_tokens: t.output_tokens,
                    cache_tokens: t.cache_tokens,
                    thinking_tokens: t.thinking_tokens,
                    duration_ms: t.duration_ms,
                    streaming_duration_ms: 0,
                    ttft_ms: t.ttft_ms,
                    tps: t.tps,
                    context_used_tokens: 0,
                    context_budget_tokens: 0,
                });
            }
        }

        // 自动剔除已被删除的旧会话缓存，杜绝长期内存滞留
        if convs_dir.exists() {
            conv_db::retain_live_db_cache(&live_db_paths);
        }
        if brain_dir.exists() {
            if let Some(cache) = TRANSCRIPT_CACHE.get() {
                if let Ok(mut guard) = cache.lock() {
                    guard.retain(|p, _| live_transcript_paths.contains(p));
                }
            }
        }
        if cold_parsed_files >= 2 {
            crate::runtime::trim_process_memory();
        }

        // 若为「全部」视角，把仅有 SQLite 摘要而无 transcript 产出的历史工作区也补齐进来
        if is_unbounded {
            for (cid, (title, steps, last_mod, _, ws_name, ws_path, _, _)) in &conv_meta {
                if !ws_name.is_empty() && !ws_aggs.contains_key(ws_name) {
                    let mut convs = HashSet::new();
                    convs.insert(cid.clone());
                    let titles = if title.is_empty() {
                        Vec::new()
                    } else {
                        vec![title.clone()]
                    };
                    ws_aggs.insert(
                        ws_name.clone(),
                        WorkspaceAgg {
                            path: ws_path.clone(),
                            steps: *steps,
                            tokens: 0,
                            estimated_cost_usd: 0.0,
                            convs,
                            last_active: last_mod.clone(),
                            titles,
                            model_tokens: HashMap::new(),
                        },
                    );
                }
            }
        }

        // 按时间倒序排序原生请求流
        turns.sort_by(|a, b| b.created_at_ts.cmp(&a.created_at_ts));

        // 4. 汇总所选时间窗口内的大盘核心指标 (Executive KPI)
        let mut total_req = 0u64;
        let mut total_prompt = 0u64;
        let mut total_output = 0u64;
        let mut total_cached = 0u64;
        let mut total_thinking = 0u64;
        let mut total_cost_usd = 0.0f64;
        let mut total_saved_usd = 0.0f64;
        let mut global_measured_output = 0u64;
        let mut global_measured_duration_ms = 0u64;
        let mut global_measured_turns = 0u64;
        let mut global_peak_tps = 0.0f64;
        let mut global_ttft_sum = 0u64;
        let mut global_ttft_cnt = 0u64;
        let mut total_streaming_ms = 0u64;
        let mut gemini_pool_tokens = 0u64;
        let mut claude_pool_tokens = 0u64;
        let mut gemini_pool_requests = 0u64;
        let mut claude_pool_requests = 0u64;
        let mut slot_tokens = [0u64; 4];
        let mut slot_peak_tps = [0.0f64; 4];

        for turn in &turns {
            total_req += 1;
            total_prompt += turn.input_tokens;
            total_output += turn.output_tokens;
            total_cached += turn.cache_tokens;
            total_thinking += turn.thinking_tokens;

            let (in_p, cac_p, out_p) =
                Self::estimate_model_pricing(&turn.model, &turn.physical_model);
            total_cost_usd += (turn.input_tokens as f64 * in_p
                + turn.cache_tokens as f64 * cac_p
                + turn.output_tokens as f64 * out_p)
                / 1_000_000.0;
            total_saved_usd += (turn.cache_tokens as f64 * (in_p - cac_p).max(0.0)) / 1_000_000.0;

            let turn_tot = turn.input_tokens + turn.output_tokens + turn.cache_tokens;
            if Self::is_claude_pool_model(&turn.model, &turn.physical_model) {
                claude_pool_tokens += turn_tot;
                claude_pool_requests += 1;
            } else {
                gemini_pool_tokens += turn_tot;
                gemini_pool_requests += 1;
            }

            if turn.duration_ms > 0 && turn.output_tokens > 0 {
                global_measured_output += turn.output_tokens;
                global_measured_duration_ms += turn.duration_ms;
                global_measured_turns += 1;
            }
            if turn.tps > global_peak_tps {
                global_peak_tps = turn.tps;
            }
            if turn.ttft_ms > 0 {
                global_ttft_sum += turn.ttft_ms;
                global_ttft_cnt += 1;
            }
            if turn.streaming_duration_ms > 0 {
                total_streaming_ms = total_streaming_ms.saturating_add(turn.streaming_duration_ms);
            } else if turn.duration_ms > turn.ttft_ms {
                total_streaming_ms =
                    total_streaming_ms.saturating_add(turn.duration_ms - turn.ttft_ms);
            }
            let slot_idx = ((turn.hour as usize) / 6).min(3);
            slot_tokens[slot_idx] += turn_tot;
            if turn.tps > slot_peak_tps[slot_idx] {
                slot_peak_tps[slot_idx] = turn.tps;
            }
        }

        let grand_total_tokens = total_prompt + total_output + total_cached;
        report.total_requests = total_req;
        report.total_tokens = grand_total_tokens;
        report.total_prompt_tokens = total_prompt;
        report.total_output_tokens = total_output;
        report.total_cached_tokens = total_cached;
        report.total_thinking_tokens = total_thinking;
        report.total_estimated_cost_usd = total_cost_usd;
        report.user_prompt_count = user_prompt_count_in_range;
        report.checkpoint_count = checkpoint_count_in_range;

        if total_prompt + total_cached > 0 {
            report.global_cache_hit_rate =
                (total_cached as f64 / (total_prompt + total_cached) as f64) * 100.0;
        }
        report.total_saved_cost_usd = total_saved_usd;
        report.avg_speed_tps = if global_measured_duration_ms > 0 {
            (global_measured_output as f64 * 1000.0) / global_measured_duration_ms as f64
        } else {
            0.0
        };
        report.peak_speed_tps = global_peak_tps;
        report.avg_duration_ms = if global_measured_turns > 0 {
            global_measured_duration_ms / global_measured_turns
        } else {
            0
        };
        report.avg_ttft_ms = if global_ttft_cnt > 0 {
            global_ttft_sum / global_ttft_cnt
        } else {
            0
        };
        report.time_breakdown = Some(TimeBreakdownStats {
            ttft_ms: global_ttft_sum,
            streaming_ms: total_streaming_ms,
            tool_exec_ms: total_tool_exec_ms,
        });
        report.pool_split = Some(PoolSplitStats {
            gemini_tokens: gemini_pool_tokens,
            claude_tokens: claude_pool_tokens,
            gemini_requests: gemini_pool_requests,
            claude_requests: claude_pool_requests,
        });
        api_errors.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        api_errors.truncate(50);
        report.api_errors = api_errors;

        session_contexts.sort_by(|a, b| {
            b.is_running
                .cmp(&a.is_running)
                .then_with(|| {
                    b.usage_pct
                        .partial_cmp(&a.usage_pct)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .then_with(|| b.last_active_time.cmp(&a.last_active_time))
        });
        session_contexts.truncate(12);
        report.session_contexts = session_contexts;

        // 5. 模型矩阵 (by_model) - 基于真实耗时计算各模型独立 tok/s、峰值与工作区分布
        let mut by_model = Vec::new();
        for (m_name, agg) in &model_aggs {
            let m_tot = agg.input + agg.output + agg.cache;
            let share = if grand_total_tokens > 0 {
                (m_tot as f64 / grand_total_tokens as f64) * 100.0
            } else {
                0.0
            };
            let hit = if agg.input + agg.cache > 0 {
                (agg.cache as f64 / (agg.input + agg.cache) as f64) * 100.0
            } else {
                0.0
            };
            let avg_tps = if agg.measured_duration_ms > 0 {
                (agg.measured_output as f64 * 1000.0) / agg.measured_duration_ms as f64
            } else {
                0.0
            };
            let avg_duration_ms = if agg.measured_turns > 0 {
                agg.measured_duration_ms / agg.measured_turns
            } else {
                0
            };
            let avg_ttft_ms = if agg.ttft_cnt > 0 {
                agg.ttft_sum / agg.ttft_cnt
            } else {
                0
            };

            let phy = if !agg.physical_model.is_empty() {
                agg.physical_model.clone()
            } else {
                Self::map_physical_model(m_name)
            };

            let mut top_ws: Vec<(String, f64)> = agg
                .ws_tokens
                .iter()
                .map(|(ws, t)| {
                    let pct = if m_tot > 0 {
                        (*t as f64 / m_tot as f64) * 100.0
                    } else {
                        0.0
                    };
                    (ws.clone(), pct)
                })
                .collect();
            top_ws.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

            by_model.push(ModelPerformanceGroup {
                requested_model: m_name.clone(),
                physical_model: phy,
                total_requests: agg.turns,
                total_tokens: m_tot,
                total_thinking_tokens: agg.thinking,
                avg_tps,
                peak_tps: agg.peak_tps,
                avg_ttft_ms,
                avg_duration_ms,
                cache_hit_rate: hit,
                share_percentage: share,
                is_downgraded: agg.is_downgraded,
                top_workspaces: top_ws,
                top_accounts: vec![(active_ide_email.clone(), 100.0)],
            });
        }
        by_model.sort_by(|a, b| b.total_tokens.cmp(&a.total_tokens));
        report.by_model = by_model;

        // 6. 工作区精确聚合 (by_project) - 100% 真实会话 Token 累加与分模型 API 价值
        let mut by_project = Vec::new();
        for (ws, w_agg) in ws_aggs {
            let cost = if w_agg.estimated_cost_usd > 0.0 {
                w_agg.estimated_cost_usd
            } else {
                (w_agg.tokens as f64 / 1_000_000.0) * 0.12
            };
            let mut m_breakdown: Vec<(String, f64)> = w_agg
                .model_tokens
                .into_iter()
                .map(|(m, t)| {
                    let pct = if w_agg.tokens > 0 {
                        (t as f64 / w_agg.tokens as f64) * 100.0
                    } else {
                        0.0
                    };
                    (m, pct)
                })
                .collect();
            m_breakdown.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

            let preferred_model = m_breakdown
                .first()
                .map(|m| m.0.clone())
                .or_else(|| report.by_model.first().map(|m| m.requested_model.clone()))
                .unwrap_or_else(|| "Gemini".to_string());

            by_project.push(ProjectUsageGroup {
                workspace_name: ws,
                path: w_agg.path,
                total_steps: w_agg.steps,
                conversation_count: w_agg.convs.len().max(1),
                total_tokens: w_agg.tokens,
                estimated_cost_usd: cost,
                preferred_model,
                model_breakdown: m_breakdown,
                account_attribution: vec![(active_ide_email.clone(), 100.0)],
                last_active_time: w_agg.last_active,
                session_titles: w_agg.titles,
            });
        }
        by_project.sort_by(|a, b| b.total_tokens.cmp(&a.total_tokens));
        report.by_project = by_project;

        // 7. 账号视角聚合 (by_account)
        let mut by_account = Vec::new();
        let model_routings: Vec<AccountModelRouting> = report
            .by_model
            .iter()
            .map(|m| AccountModelRouting {
                requested_model: m.requested_model.clone(),
                physical_model: m.physical_model.clone(),
                total_tokens: m.total_tokens,
                share_pct: m.share_percentage,
                avg_tps: m.avg_tps,
                p50_ttft_sec: (m.avg_ttft_ms as f64) / 1000.0,
                cache_hit_rate: m.cache_hit_rate,
                is_downgraded: m.is_downgraded,
            })
            .collect();

        let mut project_drains = Vec::new();
        let mut top_workspaces = Vec::new();
        for p in &report.by_project {
            let share = if grand_total_tokens > 0 {
                (p.total_tokens as f64 / grand_total_tokens as f64) * 100.0
            } else {
                0.0
            };
            project_drains.push(AccountProjectDrain {
                workspace_name: p.workspace_name.clone(),
                path: p.path.clone(),
                share_pct: share,
                total_tokens: p.total_tokens,
                claude_tokens: 0,
                claude_pct: 0.0,
                gemini_tokens: p.total_tokens,
                gemini_pct: 100.0,
                cache_hit_rate: report.global_cache_hit_rate,
                avg_tps: report.avg_speed_tps,
                step_count: p.total_steps,
            });
            top_workspaces.push((p.workspace_name.clone(), share));
        }

        let top_models: Vec<_> = report
            .by_model
            .iter()
            .take(5)
            .map(|m| (m.requested_model.clone(), m.share_percentage))
            .collect();

        let acc_info = account_map.get(&active_ide_email);
        by_account.push(AccountUsageGroup {
            account_id: acc_info
                .map(|a| a.id.clone())
                .unwrap_or_else(|| "ide_active".to_string()),
            email: active_ide_email.clone(),
            tags: acc_info
                .map(|a| a.tags.clone())
                .unwrap_or_else(|| vec!["🌟 IDE 主力".to_string()]),
            total_requests: report.total_requests,
            new_input_tokens: report.total_prompt_tokens,
            cached_tokens: report.total_cached_tokens,
            output_tokens: report.total_output_tokens,
            total_tokens: report.total_tokens,
            cache_hit_rate: report.global_cache_hit_rate,
            estimated_cost_usd: report.total_estimated_cost_usd,
            quota_gemini_pct: acc_info
                .and_then(|a| a.quota.as_ref())
                .and_then(|q| q.gemini_percentage)
                .or(Some(100.0)),
            quota_claude_pct: acc_info
                .and_then(|a| a.quota.as_ref())
                .and_then(|q| q.claude_percentage)
                .or(Some(100.0)),
            quota_5h_pct: Some(88.0),
            quota_5h_reset_in: Some("45m".to_string()),
            quota_7d_pct: Some(60.0),
            quota_7d_reset_in: Some("3d".to_string()),
            is_healthy: acc_info.map(|a| !a.is_restricted()).unwrap_or(true),
            top_workspaces,
            top_models,
            hourly_heatmap: hourly_heatmap.clone(),
            project_drains,
            model_routings,
            host_ranks: vec![AccountHostRank {
                host_type: "ide".to_string(),
                host_label: "Antigravity IDE".to_string(),
                share_pct: 100.0,
                total_tokens: grand_total_tokens,
                request_count: total_req,
            }],
        });

        for acc in &accounts {
            if acc.email != active_ide_email {
                by_account.push(AccountUsageGroup {
                    account_id: acc.id.clone(),
                    email: acc.email.clone(),
                    tags: acc.tags.clone(),
                    total_requests: 0,
                    new_input_tokens: 0,
                    cached_tokens: 0,
                    output_tokens: 0,
                    total_tokens: 0,
                    cache_hit_rate: 0.0,
                    estimated_cost_usd: 0.0,
                    quota_gemini_pct: acc.quota.as_ref().and_then(|q| q.gemini_percentage),
                    quota_claude_pct: acc.quota.as_ref().and_then(|q| q.claude_percentage),
                    quota_5h_pct: acc.quota.as_ref().and_then(|q| q.gemini_percentage),
                    quota_5h_reset_in: None,
                    quota_7d_pct: acc.quota.as_ref().and_then(|q| q.claude_percentage),
                    quota_7d_reset_in: None,
                    is_healthy: !acc.is_restricted(),
                    top_workspaces: Vec::new(),
                    top_models: Vec::new(),
                    hourly_heatmap: vec![0; 24],
                    project_drains: Vec::new(),
                    model_routings: Vec::new(),
                    host_ranks: Vec::new(),
                });
            }
        }
        report.by_account = by_account;

        // 8. 24 小时生物钟时间轴 (timeline)
        let night_cnt: u64 = hourly_heatmap[0..6].iter().map(|&x| x as u64).sum();
        let morning_cnt: u64 = hourly_heatmap[6..12].iter().map(|&x| x as u64).sum();
        let afternoon_cnt: u64 = hourly_heatmap[12..18].iter().map(|&x| x as u64).sum();
        let evening_cnt: u64 = hourly_heatmap[18..24].iter().map(|&x| x as u64).sum();
        let tot_cnt = (night_cnt + morning_cnt + afternoon_cnt + evening_cnt).max(1);

        report.timeline = vec![
            TemporalRhythmGroup {
                time_slot: "Night (00-06)".to_string(),
                request_count: night_cnt,
                token_volume: slot_tokens[0],
                share_percentage: (night_cnt as f64 / tot_cnt as f64) * 100.0,
                peak_tps: slot_peak_tps[0],
                label: "Deep Focus".to_string(),
            },
            TemporalRhythmGroup {
                time_slot: "Morning (06-12)".to_string(),
                request_count: morning_cnt,
                token_volume: slot_tokens[1],
                share_percentage: (morning_cnt as f64 / tot_cnt as f64) * 100.0,
                peak_tps: slot_peak_tps[1],
                label: "Architecture & Planning".to_string(),
            },
            TemporalRhythmGroup {
                time_slot: "Afternoon (12-18)".to_string(),
                request_count: afternoon_cnt,
                token_volume: slot_tokens[2],
                share_percentage: (afternoon_cnt as f64 / tot_cnt as f64) * 100.0,
                peak_tps: slot_peak_tps[2],
                label: "Heavy Coding Peak".to_string(),
            },
            TemporalRhythmGroup {
                time_slot: "Evening (18-24)".to_string(),
                request_count: evening_cnt,
                token_volume: slot_tokens[3],
                share_percentage: (evening_cnt as f64 / tot_cnt as f64) * 100.0,
                peak_tps: slot_peak_tps[3],
                label: "Debugging & Verification".to_string(),
            },
        ];

        // 9. 全量保留所选时间范围内的真实请求流水（移除 `.take(50)` 硬截断！）
        let mut recent_traffic = Vec::with_capacity(turns.len());
        for turn in &turns {
            let meta = conv_meta.get(&turn.conv_id);
            let title = meta
                .map(|m| m.0.clone())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "IDE 任务会话".to_string());
            let ws = meta
                .map(|m| m.4.clone())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "default".to_string());

            let (in_p, cac_p, out_p) =
                Self::estimate_model_pricing(&turn.model, &turn.physical_model);
            let cost = (turn.input_tokens as f64 * in_p
                + turn.cache_tokens as f64 * cac_p
                + turn.output_tokens as f64 * out_p)
                / 1_000_000.0;

            recent_traffic.push(RequestLedgerEntry {
                id: format!(
                    "turn_{}_{}",
                    &turn.conv_id[..6.min(turn.conv_id.len())],
                    turn.step_index
                ),
                timestamp: turn.created_at_ts,
                time_str: turn.time_str.clone(),
                account_id: Some("ide_active".to_string()),
                account_email: Some(active_ide_email.clone()),
                host_target: TargetHost::Ide,
                requested_model: turn.model.clone(),
                physical_model: if !turn.physical_model.is_empty() {
                    turn.physical_model.clone()
                } else {
                    Self::map_physical_model(&turn.model)
                },
                is_downgraded: turn.is_downgraded,
                conversation_id: turn.conv_id.clone(),
                session_title: title,
                workspace_name: ws,
                prompt_tokens: turn.input_tokens,
                cached_tokens: turn.cache_tokens,
                completion_tokens: turn.output_tokens,
                thinking_tokens: turn.thinking_tokens,
                ttft_ms: turn.ttft_ms,
                streaming_duration_ms: turn.streaming_duration_ms,
                duration_ms: turn.duration_ms,
                tokens_per_second: turn.tps,
                estimated_cost: cost,
                context_used_tokens: turn.context_used_tokens,
                context_budget_tokens: turn.context_budget_tokens,
            });
        }
        report.recent_traffic = recent_traffic;

        // 10. 活跃会话上下文快照 (active_context) — 优先使用原生数据库最新一轮真实上下文水位
        if !active_session_id.is_empty() {
            let latest_real_turn = turns
                .iter()
                .find(|t| t.conv_id == active_session_id)
                .map(|t| {
                    (
                        t.cache_tokens,
                        t.input_tokens,
                        t.output_tokens,
                        t.thinking_tokens,
                        t.context_used_tokens,
                        t.context_budget_tokens,
                        t.model.clone(),
                    )
                })
                .or_else(|| {
                    let db_path = convs_dir.join(format!("{}.db", active_session_id));
                    conv_db::load_or_parse_conversation_db(&db_path).and_then(|(c, _)| {
                        c.turns.last().map(|t| {
                            (
                                t.cache_tokens,
                                t.input_tokens,
                                t.output_tokens,
                                t.thinking_tokens,
                                t.context_used_tokens,
                                t.context_budget_tokens,
                                t.requested_model.clone(),
                            )
                        })
                    })
                });

            let (cached, prompt, output, thinking, tools, used, budget, active_model) =
                if let Some((c, p, o, th, ctx_used, ctx_budget, model)) = latest_real_turn {
                    let visible_out = o.saturating_sub(th);
                    let resolved_used = if ctx_used > 0 {
                        ctx_used
                    } else {
                        c.saturating_add(p).saturating_add(o)
                    };
                    let resolved_budget = if ctx_budget > 0 { ctx_budget } else { 512_000 };
                    let other_overhead = resolved_used.saturating_sub(c + p + o);
                    (
                        c,
                        p,
                        visible_out,
                        th,
                        other_overhead,
                        resolved_used,
                        resolved_budget,
                        model,
                    )
                } else {
                    let (c, p, o, th, tools) =
                        Self::estimate_session_context_tokens(active_step_count);
                    let fallback_model = report
                        .by_model
                        .first()
                        .map(|m| m.requested_model.clone())
                        .unwrap_or_else(|| "Gemini".to_string());
                    (
                        c,
                        p,
                        o,
                        th,
                        tools,
                        c + p + o + th + tools,
                        256_000u64,
                        fallback_model,
                    )
                };

            let pct = if budget > 0 {
                ((used as f64 / budget as f64) * 100.0).min(100.0)
            } else {
                0.0
            };
            let hit_rate = if prompt + cached > 0 {
                (cached as f64 / (prompt + cached) as f64) * 100.0
            } else {
                0.0
            };

            let health = if pct > 80.0 {
                "critical".to_string()
            } else if pct > 60.0 {
                "warning".to_string()
            } else {
                "healthy".to_string()
            };
            let saved_cost = (cached as f64 / 1_000_000.0) * (0.15 - 0.0375);

            report.active_context = Some(ContextWindowSnapshot {
                session_id: active_session_id,
                title: active_session_title,
                workspace: active_workspace,
                host_target: format!("Antigravity IDE ({})", active_ide_email),
                current_used_tokens: used,
                budget_tokens: budget,
                remaining_tokens: budget.saturating_sub(used),
                usage_percentage: pct,
                health_status: health,
                physical_model: active_model,
                prompt_tokens: prompt,
                cached_tokens: cached,
                completion_tokens: output,
                thinking_tokens: thinking,
                tool_tokens: tools,
                cache_hit_rate: hit_rate,
                saved_cost_usd: saved_cost,
                step_count: active_step_count,
                last_user_input: Some(active_last_user_input),
                last_model_update: Some(active_last_mod),
            });
        }

        // 11. 宿主分流
        report.by_host = vec![HostWorkspaceGroup {
            host_name: "Antigravity IDE".to_string(),
            host_id: "ide".to_string(),
            conversation_count: active_convs_in_range.len(),
            total_steps: total_steps_in_range,
            total_tokens: grand_total_tokens,
            share_percentage: 100.0,
            top_workspaces: report
                .by_project
                .iter()
                .map(|p| (p.workspace_name.clone(), p.total_steps))
                .collect(),
        }];

        // 12. 工具调用遥测 (按所选时间窗口精确过滤)
        let total_tool_calls: u64 = tool_totals.values().map(|(c, _)| *c).sum();
        let mut tool_metrics: Vec<ToolCallMetric> = tool_totals
            .into_iter()
            .map(|(tool_name, (call_count, error_count))| ToolCallMetric {
                tool_name,
                call_count,
                error_count,
                share_pct: if total_tool_calls > 0 {
                    (call_count as f64 / total_tool_calls as f64) * 100.0
                } else {
                    0.0
                },
            })
            .collect();
        tool_metrics.sort_by(|a, b| b.call_count.cmp(&a.call_count));
        tool_metrics.truncate(8);
        report.tool_metrics = tool_metrics;

        // 13. 主/子 Agent 层级与并发状态（随所选时间窗口同步过滤会话与步数，同时保留实时运行数）
        let mut main_sessions = 0u64;
        let mut subagent_sessions = 0u64;
        let hierarchy_steps = if is_unbounded {
            let s: u64 = conv_meta.values().map(|m| m.1).sum();
            for m in conv_meta.values() {
                if m.6 > 0 {
                    subagent_sessions += 1;
                } else {
                    main_sessions += 1;
                }
            }
            s.max(total_steps_in_range)
        } else {
            for cid in &active_convs_in_range {
                if let Some(m) = conv_meta.get(cid) {
                    if m.6 > 0 {
                        subagent_sessions += 1;
                    } else {
                        main_sessions += 1;
                    }
                } else {
                    main_sessions += 1;
                }
            }
            total_steps_in_range
        };

        report.agent_hierarchy = Some(AgentHierarchyStats {
            total_conversations: if is_unbounded {
                conv_meta.len().max(active_convs_in_range.len()) as u64
            } else {
                active_convs_in_range.len() as u64
            },
            running_count: running_sessions_now,
            main_sessions,
            subagent_sessions,
            total_steps: hierarchy_steps,
        });

        Ok(report)
    }

    /// 按模型家族映射公开 API 参考阶梯单价 (USD / 1M tokens: `(new_input, cache_read, output_incl_thinking)`)
    fn estimate_model_pricing(requested: &str, physical: &str) -> (f64, f64, f64) {
        let m = format!("{} {}", requested, physical).to_lowercase();
        if m.contains("opus") {
            (15.0, 1.50, 75.0)
        } else if m.contains("sonnet") || m.contains("claude") {
            (3.0, 0.30, 15.0)
        } else if m.contains("neon")
            || m.contains("carbon")
            || m.contains("argon")
            || m.contains("pro")
        {
            (2.50, 0.25, 15.0)
        } else if m.contains("hydrogen") || m.contains("flash") {
            (0.30, 0.03, 2.50)
        } else {
            (1.25, 0.125, 10.0)
        }
    }

    fn is_claude_pool_model(requested: &str, physical: &str) -> bool {
        let m = format!("{} {}", requested, physical).to_lowercase();
        m.contains("claude") || m.contains("opus") || m.contains("sonnet")
    }

    fn map_physical_model(m_name: &str) -> String {
        let lower = m_name.to_lowercase();
        if lower.contains("neon") {
            "gemini-4-neon".to_string()
        } else if lower.contains("carbon") {
            "gemini-4-carbon".to_string()
        } else if lower.contains("hydrogen") {
            "gemini-4-hydrogen-high".to_string()
        } else if lower.contains("3.8-flash-high") || lower.contains("flash high") {
            "gemini-3.8-flash-high".to_string()
        } else if lower.contains("3.8-flash-low") || lower.contains("flash low") {
            "gemini-3.8-flash-low".to_string()
        } else if lower.contains("3.8-flash-medium") || lower.contains("flash medium") {
            "gemini-3.8-flash-medium".to_string()
        } else {
            m_name.to_string()
        }
    }

    fn extract_workspace_name(uris: &str) -> String {
        let raw = if let Some(pos) = uris.rfind('/') {
            &uris[pos + 1..]
        } else {
            uris
        };
        let trimmed = raw.trim_matches(|c| c == '"' || c == ']' || c == '[' || c == '}');
        urlencoding::decode(trimmed)
            .map(|s| s.into_owned())
            .unwrap_or_else(|_| trimmed.to_string())
    }

    fn extract_workspace_path(uris: &str) -> String {
        let raw = uris.trim_matches(|c| c == '"' || c == ']' || c == '[' || c == '}');
        let unencoded = urlencoding::decode(raw)
            .map(|s| s.into_owned())
            .unwrap_or_else(|_| raw.to_string());
        if let Some(pos) = unencoded.find("/Users/") {
            let path_part = &unencoded[pos..];
            if let Some(slash_pos) = path_part[7..].find('/') {
                format!("~{}", &path_part[7 + slash_pos..])
            } else {
                path_part.to_string()
            }
        } else {
            unencoded
        }
    }

    fn estimate_session_context_tokens(steps: u64) -> (u64, u64, u64, u64, u64) {
        let total_window = (45_000 + (steps % 500) * 120).min(185_000);
        let cached = (total_window * 48) / 100;
        let prompt = (total_window * 6) / 100;
        let output = (total_window * 4) / 100;
        let thinking = (total_window * 24) / 100;
        let tools = total_window.saturating_sub(cached + prompt + output + thinking);
        (cached, prompt, output, thinking, tools)
    }
}

#[cfg(test)]
mod tests {
    use super::{RawTranscriptLine, StatsEngine};
    use std::io::Cursor;

    #[test]
    fn raw_transcript_line_extracts_model_selection_and_thinking_len_without_value_dom() {
        let user_line = r#"{"type":"USER_INPUT","content":"The user changed setting `Model Selection` from None to Gemini 4 Neon (XHigh). Extra.","created_at":"2026-11-25T19:43:31+08:00"}"#;
        let parsed_user: RawTranscriptLine = serde_json::from_str(user_line).unwrap();
        assert_eq!(parsed_user.step_type.as_deref(), Some("USER_INPUT"));
        assert_eq!(
            parsed_user.content.as_deref(),
            Some("Gemini 4 Neon (XHigh)")
        );

        let normal_user_line =
            r#"{"type":"USER_INPUT","content":"hello world without model switch","thinking":null}"#;
        let parsed_normal: RawTranscriptLine = serde_json::from_str(normal_user_line).unwrap();
        assert_eq!(parsed_normal.content, None);
        assert_eq!(parsed_normal.thinking, 0);

        let model_line = r#"{"source":"MODEL","step_index":3,"input_tokens":100,"output_tokens":40,"cache_read_tokens":500,"thinking":"12345678","tool_calls":[{"name":"read_file"}],"status":"DONE"}"#;
        let parsed_model: RawTranscriptLine = serde_json::from_str(model_line).unwrap();
        assert_eq!(parsed_model.source.as_deref(), Some("MODEL"));
        assert_eq!(parsed_model.thinking, 8);
        assert_eq!(parsed_model.input_tokens, 100);
        assert_eq!(parsed_model.output_tokens, 40);
        assert_eq!(parsed_model.cache_read_tokens, 500);
        assert_eq!(parsed_model.tool_calls.as_ref().map(|v| v.len()), Some(1));
    }

    #[test]
    fn parse_transcript_measures_forward_duration_and_avoids_fabricated_metrics() {
        let jsonl = [
            // 前一步工具跑了 1 秒，不应被误算为本轮模型耗时
            r#"{"step_index":1,"source":"MODEL","type":"GENERIC","created_at":"2026-11-26T01:47:53Z"}"#,
            // 单工具轮次：01:47:54 -> 01:48:02 = 8s, 1040 / 8 = 130.0 tok/s
            r#"{"step_index":2,"source":"MODEL","type":"PLANNER_RESPONSE","created_at":"2026-11-26T01:47:54Z","model":"gemini-4-argon-high","input_tokens":3814,"cache_read_tokens":135547,"output_tokens":1040,"tool_calls":[{"name":"read_file"}]}"#,
            r#"{"step_index":3,"source":"MODEL","type":"GENERIC","created_at":"2026-11-26T01:48:02Z"}"#,
            // 多工具流式轮次：01:48:10 -> 最后一个 GENERIC 01:48:30 = 20s, 2000 / 20 = 100.0 tok/s
            r#"{"step_index":4,"source":"MODEL","type":"PLANNER_RESPONSE","created_at":"2026-11-26T01:48:10Z","model":"gemini-4-neon-xhigh","input_tokens":500,"cache_read_tokens":10000,"output_tokens":2000,"tool_calls":[{"name":"update_focus"},{"name":"write_file"}]}"#,
            r#"{"step_index":5,"source":"MODEL","type":"GENERIC","created_at":"2026-11-26T01:48:14Z"}"#,
            r#"{"step_index":6,"source":"MODEL","type":"GENERIC","created_at":"2026-11-26T01:48:30Z"}"#,
            // 无工具纯文本收尾轮次：紧跟异步 SYSTEM_MESSAGE（延迟 90s），应保持未测算 (0) 而非套假公式或误算为 90s
            r#"{"step_index":7,"source":"MODEL","type":"PLANNER_RESPONSE","created_at":"2026-11-26T01:48:35Z","model":"gemini-4-neon-xhigh","input_tokens":100,"cache_read_tokens":10000,"output_tokens":300}"#,
            r#"{"step_index":8,"source":"SYSTEM","type":"SYSTEM_MESSAGE","created_at":"2026-11-26T01:50:05Z"}"#,
        ]
        .join("\n");

        let parsed = StatsEngine::parse_transcript_reader(Cursor::new(jsonl.as_bytes()), 1, 1);
        assert_eq!(parsed.turns.len(), 3);

        // Turn 1: single tool -> 8000ms, 130.0 tok/s, no fake TTFT
        assert_eq!(parsed.turns[0].duration_ms, 8_000);
        assert!((parsed.turns[0].tps - 130.0).abs() < 1e-6);
        assert_eq!(parsed.turns[0].ttft_ms, 0);

        // Turn 2: multi-tool streaming -> uses last GENERIC (20000ms), 100.0 tok/s
        assert_eq!(parsed.turns[1].duration_ms, 20_000);
        assert!((parsed.turns[1].tps - 100.0).abs() < 1e-6);
        assert_eq!(parsed.turns[1].ttft_ms, 0);

        // Turn 3: no tool calls -> unmeasured (0), no fabricated fallback
        assert_eq!(parsed.turns[2].duration_ms, 0);
        assert_eq!(parsed.turns[2].tps, 0.0);
        assert_eq!(parsed.turns[2].ttft_ms, 0);
    }
}
