use chrono::{DateTime, Local, Timelike};
use rusqlite::{Connection, OpenFlags};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, UNIX_EPOCH};

const MIN_MEASURED_DURATION_MS: u64 = 100;
const MAX_MEASURED_DURATION_MS: u64 = 600_000;

/// 单个会话 `.db` 中解析出的工具调用记录
#[derive(Clone, Debug)]
pub(crate) struct DbToolStep {
    pub step_ts_ms: i64,
    pub is_err: bool,
    pub tools: Vec<String>,
}

/// 单个会话 `.db` 中解析出的纳秒级精确模型交互记录
#[derive(Clone, Debug)]
pub(crate) struct DbModelTurn {
    pub step_index: u64,
    pub created_at_ts: i64,
    pub hour: u8,
    pub time_str: String,
    pub requested_model: String,
    pub physical_model: String,
    pub is_downgraded: bool,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_tokens: u64,
    pub thinking_tokens: u64,
    pub ttft_ms: u64,
    pub streaming_duration_ms: u64,
    pub duration_ms: u64,
    pub tps: f64,
    pub context_used_tokens: u64,
    pub context_budget_tokens: u64,
}

/// 单个会话 `.db` 中解析出的原生 API 错误/限流事件（429 / 403 / 400 / 网络超时）
#[derive(Clone, Debug)]
pub(crate) struct DbApiError {
    pub step_ts_ms: i64,
    pub time_str: String,
    pub code: String,
    pub model: String,
    pub message: String,
}

#[derive(Clone)]
pub(crate) struct CachedConversationDb {
    db_mtime_ms: i64,
    db_len: u64,
    wal_mtime_ms: i64,
    wal_len: u64,
    pub step_timestamps: Vec<i64>,
    pub user_prompt_timestamps: Vec<i64>,
    pub checkpoint_timestamps: Vec<i64>,
    pub tool_exec_durations: Vec<(i64, u64)>,
    pub tool_steps: Vec<DbToolStep>,
    pub api_errors: Vec<DbApiError>,
    pub turns: Vec<DbModelTurn>,
}

static CONV_DB_CACHE: OnceLock<Mutex<HashMap<PathBuf, CachedConversationDb>>> = OnceLock::new();

#[derive(Default, Clone, Debug)]
struct ProtoUsage {
    model_id: u64,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    thinking_tokens: u64,
}

#[derive(Default, Clone, Debug)]
struct GenMetaRecord {
    model_id: u64,
    usage: ProtoUsage,
    ttft_ms: u64,
    streaming_ms: u64,
    physical_model: String,
    display_model: String,
    context_used_tokens: u64,
    context_budget_tokens: u64,
    error_message: Option<String>,
    last_step_index: Option<i64>,
}

#[derive(Default, Debug)]
struct StepMetaRecord {
    created_at_ms: i64,
    first_token_at_ms: i64,
    gen_completed_at_ms: i64,
    completed_at_ms: i64,
    tool_name: Option<String>,
    has_usage: bool,
    usage: ProtoUsage,
    has_gen_ref: bool,
    gen_meta_idx: u64,
}

#[derive(Clone, Copy)]
enum WireValue<'a> {
    Varint(u64),
    Bytes(&'a [u8]),
    Fixed64,
    Fixed32,
}

/// 读取标准 Protobuf base-128 varint
fn read_varint(buf: &[u8], pos: &mut usize) -> Option<u64> {
    let mut val = 0u64;
    let mut shift = 0u32;
    while *pos < buf.len() {
        let b = buf[*pos];
        *pos += 1;
        if shift < 64 {
            val |= ((b & 0x7f) as u64) << shift;
        }
        if (b & 0x80) == 0 {
            return Some(val);
        }
        shift += 7;
        if shift > 70 {
            return None;
        }
    }
    None
}

/// 零拷贝流式遍历 Protobuf wire 字段
fn for_each_proto_field<'a, F>(buf: &'a [u8], mut f: F)
where
    F: FnMut(u32, WireValue<'a>),
{
    let mut pos = 0usize;
    while pos < buf.len() {
        let Some(tag) = read_varint(buf, &mut pos) else {
            break;
        };
        let field_num = (tag >> 3) as u32;
        let wire_type = (tag & 0x07) as u8;
        if field_num == 0 {
            break;
        }
        match wire_type {
            0 => {
                let Some(v) = read_varint(buf, &mut pos) else {
                    break;
                };
                f(field_num, WireValue::Varint(v));
            }
            1 => {
                if pos + 8 > buf.len() {
                    break;
                }
                pos += 8;
                f(field_num, WireValue::Fixed64);
            }
            2 => {
                let Some(len) = read_varint(buf, &mut pos) else {
                    break;
                };
                let len = len as usize;
                if pos.checked_add(len).map_or(true, |end| end > buf.len()) {
                    break;
                }
                let slice = &buf[pos..pos + len];
                pos += len;
                f(field_num, WireValue::Bytes(slice));
            }
            5 => {
                if pos + 4 > buf.len() {
                    break;
                }
                pos += 4;
                f(field_num, WireValue::Fixed32);
            }
            _ => break,
        }
    }
}

/// 解析 `google.protobuf.Timestamp` 或 `google.protobuf.Duration` -> 毫秒
fn parse_proto_seconds_nanos_ms(buf: &[u8]) -> i64 {
    let mut secs = 0i64;
    let mut nanos = 0i64;
    for_each_proto_field(buf, |fn_num, val| {
        if let WireValue::Varint(v) = val {
            match fn_num {
                1 => secs = v as i64,
                2 => nanos = v as i64,
                _ => {}
            }
        }
    });
    if secs < 0 || nanos < 0 {
        return 0;
    }
    secs.saturating_mul(1_000).saturating_add(nanos / 1_000_000)
}

fn parse_proto_usage(buf: &[u8]) -> ProtoUsage {
    let mut u = ProtoUsage::default();
    for_each_proto_field(buf, |fn_num, val| {
        if let WireValue::Varint(v) = val {
            match fn_num {
                1 => u.model_id = v,
                2 => u.input_tokens = v,
                3 => u.output_tokens = v,
                5 => u.cache_read_tokens = v,
                9 => u.thinking_tokens = v,
                _ => {}
            }
        }
    });
    u
}

fn parse_gen_metadata_blob(buf: &[u8]) -> Option<GenMetaRecord> {
    let mut rec = GenMetaRecord::default();
    let mut found_inner = false;

    for_each_proto_field(buf, |fn_num, val| {
        if fn_num == 5 {
            if let WireValue::Bytes(ebuf) = val {
                if let Ok(text) = std::str::from_utf8(ebuf) {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        rec.error_message = Some(trimmed.chars().take(180).collect());
                    }
                }
            }
        }
        if fn_num == 1 {
            if let WireValue::Bytes(inner) = val {
                found_inner = true;
                for_each_proto_field(inner, |sfn, sval| match (sfn, sval) {
                    (3, WireValue::Varint(v)) => rec.model_id = v,
                    (4, WireValue::Bytes(ubuf)) => {
                        rec.usage = parse_proto_usage(ubuf);
                        if rec.model_id == 0 && rec.usage.model_id > 0 {
                            rec.model_id = rec.usage.model_id;
                        }
                    }
                    (9, WireValue::Bytes(ctx_outer)) => {
                        for_each_proto_field(ctx_outer, |cfn, cval| {
                            if cfn == 10 {
                                if let WireValue::Bytes(ctx_inner) = cval {
                                    for_each_proto_field(ctx_inner, |kfn, kval| {
                                        if let WireValue::Varint(v) = kval {
                                            match kfn {
                                                1 => rec.context_used_tokens = v,
                                                4 => rec.context_budget_tokens = v,
                                                _ => {}
                                            }
                                        }
                                    });
                                }
                            }
                        });
                    }
                    (11, WireValue::Bytes(dbuf)) => {
                        rec.ttft_ms = parse_proto_seconds_nanos_ms(dbuf).max(0) as u64;
                    }
                    (12, WireValue::Bytes(dbuf)) => {
                        rec.streaming_ms = parse_proto_seconds_nanos_ms(dbuf).max(0) as u64;
                    }
                    (19, WireValue::Bytes(s)) => {
                        if let Ok(text) = std::str::from_utf8(s) {
                            let trimmed = text.trim();
                            if !trimmed.is_empty() {
                                rec.physical_model = trimmed.to_string();
                            }
                        }
                    }
                    (20, WireValue::Bytes(kvbuf)) => {
                        let mut k = "";
                        let mut v = "";
                        for_each_proto_field(kvbuf, |kfn, kval| {
                            if let WireValue::Bytes(bs) = kval {
                                if let Ok(s) = std::str::from_utf8(bs) {
                                    match kfn {
                                        1 => k = s,
                                        2 => v = s,
                                        _ => {}
                                    }
                                }
                            }
                        });
                        if k == "last_step_index" {
                            if let Ok(si) = v.trim().parse::<i64>() {
                                rec.last_step_index = Some(si);
                            }
                        }
                    }
                    (21, WireValue::Bytes(s)) => {
                        if let Ok(text) = std::str::from_utf8(s) {
                            let trimmed = text.trim();
                            if !trimmed.is_empty() {
                                rec.display_model = trimmed.to_string();
                            }
                        }
                    }
                    _ => {}
                });
            }
        }
    });

    if found_inner || rec.error_message.is_some() {
        Some(rec)
    } else {
        None
    }
}

fn classify_api_error_code(msg: &str) -> String {
    let upper = msg.to_uppercase();
    if upper.contains("429")
        || upper.contains("RESOURCE_EXHAUSTED")
        || upper.contains("RESOURCEEXHAUSTED")
    {
        "429".to_string()
    } else if upper.contains("403")
        || upper.contains("PERMISSION_DENIED")
        || upper.contains("PERMISSIONDENIED")
    {
        "403".to_string()
    } else if upper.contains("400") || upper.contains("INVALIDARGUMENT") {
        "400".to_string()
    } else if upper.contains("404") || upper.contains("NOT_FOUND") {
        "404".to_string()
    } else if upper.contains("TIMEOUT") || upper.contains("DEADLINE") {
        "TIMEOUT".to_string()
    } else if upper.contains("CANCELED") || upper.contains("CANCELLED") {
        "CANCEL".to_string()
    } else {
        "ERR".to_string()
    }
}

fn parse_step_metadata_blob(buf: &[u8]) -> StepMetaRecord {
    let mut rec = StepMetaRecord::default();
    for_each_proto_field(buf, |fn_num, val| match (fn_num, val) {
        (1, WireValue::Bytes(tbuf)) => rec.created_at_ms = parse_proto_seconds_nanos_ms(tbuf),
        (4, WireValue::Bytes(tbuf)) => {
            for_each_proto_field(tbuf, |tfn, tval| {
                if tfn == 2 {
                    if let WireValue::Bytes(name_bytes) = tval {
                        if let Ok(name) = std::str::from_utf8(name_bytes) {
                            let trimmed = name.trim();
                            if !trimmed.is_empty() {
                                rec.tool_name = Some(trimmed.to_string());
                            }
                        }
                    }
                }
            });
        }
        (6, WireValue::Bytes(tbuf)) => rec.first_token_at_ms = parse_proto_seconds_nanos_ms(tbuf),
        (7, WireValue::Bytes(tbuf)) => rec.gen_completed_at_ms = parse_proto_seconds_nanos_ms(tbuf),
        (8, WireValue::Bytes(tbuf)) => rec.completed_at_ms = parse_proto_seconds_nanos_ms(tbuf),
        (9, WireValue::Bytes(ubuf)) => {
            rec.has_usage = true;
            rec.usage = parse_proto_usage(ubuf);
        }
        (20, WireValue::Bytes(rbuf)) => {
            rec.has_gen_ref = true;
            // Protobuf3 默认值为 0，wire 上省略 field 3 即代表 gen_metadata idx = 0
            rec.gen_meta_idx = 0;
            for_each_proto_field(rbuf, |rfn, rval| {
                if rfn == 3 {
                    if let WireValue::Varint(v) = rval {
                        rec.gen_meta_idx = v;
                    }
                }
            });
        }
        _ => {}
    });
    rec
}

fn encode_sqlite_uri_path(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let normalized = raw.replace('\\', "/");
    let mut out = String::with_capacity(normalized.len() + 16);
    if !normalized.starts_with('/') {
        out.push('/');
    }
    for ch in normalized.chars() {
        match ch {
            '%' => out.push_str("%25"),
            '?' => out.push_str("%3F"),
            '#' => out.push_str("%23"),
            ' ' => out.push_str("%20"),
            _ => out.push(ch),
        }
    }
    out
}

/// 零写入只读打开会话 SQLite：
/// 1. 优先 `mode=ro`（活跃 WAL 会话可实时读到 `-wal` 最新帧且零写入）
/// 2. 若已关闭的 WAL 库缺少 `-shm` 导致 `mode=ro` 失败，自动回退 `immutable=1`
pub(crate) fn open_conversation_db_readonly(path: &Path) -> Option<Connection> {
    let encoded = encode_sqlite_uri_path(path);
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
        | OpenFlags::SQLITE_OPEN_URI
        | OpenFlags::SQLITE_OPEN_NO_MUTEX;

    let ro_uri = format!("file:{}?mode=ro", encoded);
    if let Ok(conn) = Connection::open_with_flags(&ro_uri, flags) {
        let _ = conn.busy_timeout(Duration::from_millis(200));
        if conn
            .query_row("SELECT 1 FROM sqlite_master LIMIT 1", [], |_| Ok(()))
            .is_ok()
        {
            return Some(conn);
        }
    }

    let imm_uri = format!("file:{}?immutable=1", encoded);
    if let Ok(conn) = Connection::open_with_flags(&imm_uri, flags) {
        if conn
            .query_row("SELECT 1 FROM sqlite_master LIMIT 1", [], |_| Ok(()))
            .is_ok()
        {
            return Some(conn);
        }
    }

    None
}

pub(crate) fn file_mtime_and_len(path: &Path) -> (i64, u64) {
    let Ok(meta) = path.metadata() else {
        return (0, 0);
    };
    let mtime_ms = meta
        .modified()
        .ok()
        .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    (mtime_ms, meta.len())
}

pub(crate) fn wal_path_for(db_path: &Path) -> PathBuf {
    let mut os = db_path.as_os_str().to_os_string();
    os.push("-wal");
    PathBuf::from(os)
}

fn detect_real_downgrade(requested: &str, physical: &str) -> bool {
    let req_lower = requested.to_lowercase();
    let phy_lower = physical.to_lowercase();
    if phy_lower.contains("conservative") {
        return true;
    }
    if req_lower.contains("claude") && !phy_lower.is_empty() && !phy_lower.contains("claude") {
        return true;
    }
    false
}

fn format_local_time_and_hour(ts_ms: i64) -> (String, u8) {
    if let Some(utc) = DateTime::from_timestamp_millis(ts_ms) {
        let local = utc.with_timezone(&Local);
        (
            local.format("%Y-%m-%d %H:%M:%S").to_string(),
            local.hour().min(23) as u8,
        )
    } else {
        (String::new(), 12)
    }
}

fn parse_conversation_connection(
    conn: &Connection,
    db_mtime_ms: i64,
    db_len: u64,
    wal_mtime_ms: i64,
    wal_len: u64,
) -> Option<CachedConversationDb> {
    let mut gm_map: HashMap<u64, GenMetaRecord> = HashMap::new();
    let mut model_id_map: HashMap<u64, (String, String)> = HashMap::new();

    if let Ok(mut stmt) = conn.prepare("SELECT idx, data FROM gen_metadata") {
        if let Ok(rows) = stmt.query_map([], |row| {
            let idx: i64 = row.get(0)?;
            let data: Vec<u8> = row.get(1)?;
            Ok((idx, data))
        }) {
            for (idx, data) in rows.flatten() {
                if idx < 0 {
                    continue;
                }
                if let Some(rec) = parse_gen_metadata_blob(&data) {
                    if rec.model_id > 0
                        && (!rec.display_model.is_empty() || !rec.physical_model.is_empty())
                    {
                        model_id_map.insert(
                            rec.model_id,
                            (rec.display_model.clone(), rec.physical_model.clone()),
                        );
                    }
                    gm_map.insert(idx as u64, rec);
                }
            }
        }
    }

    let mut step_timestamps = Vec::new();
    let mut user_prompt_timestamps = Vec::new();
    let mut checkpoint_timestamps = Vec::new();
    let mut tool_exec_durations = Vec::new();
    let mut tool_steps = Vec::new();
    let mut turns = Vec::new();
    let mut step_ts_by_idx: HashMap<i64, i64> = HashMap::new();

    let mut stmt = conn
        .prepare(
            "SELECT idx, step_type, status, metadata, \
             (error_details IS NOT NULL AND length(error_details) > 0) \
             FROM steps ORDER BY idx ASC",
        )
        .ok()?;

    let rows = stmt
        .query_map([], |row| {
            let idx: i64 = row.get(0)?;
            let step_type: i64 = row.get(1)?;
            let status: i64 = row.get(2)?;
            let meta: Option<Vec<u8>> = row.get(3)?;
            let has_err: i64 = row.get(4)?;
            Ok((idx, step_type, status, meta, has_err != 0))
        })
        .ok()?;

    let mut prev_ts_ms = 0i64;
    for (idx, step_type, status, meta_opt, has_err) in rows.flatten() {
        let Some(meta_bytes) = meta_opt else {
            continue;
        };
        if meta_bytes.is_empty() {
            continue;
        }
        let sm = parse_step_metadata_blob(&meta_bytes);
        let step_ts_ms = if sm.created_at_ms > 0 {
            sm.created_at_ms
        } else {
            prev_ts_ms
        };
        if step_ts_ms > 0 {
            prev_ts_ms = step_ts_ms;
            step_timestamps.push(step_ts_ms);
            step_ts_by_idx.insert(idx, step_ts_ms);
            if step_type == 14 {
                user_prompt_timestamps.push(step_ts_ms);
            } else if step_type == 23 {
                checkpoint_timestamps.push(step_ts_ms);
            }
        }

        if let Some(tool_name) = sm.tool_name {
            if step_ts_ms > 0 {
                // 跳过定时器/用户交互等待类工具，并用 t7(真正开始执行) 到 t8(完成) 避免把流式参数生成时间重复计入工具耗时
                let is_wait_tool = matches!(
                    tool_name.as_str(),
                    "schedule" | "ask_question" | "notify_user"
                );
                if !is_wait_tool {
                    let tool_start_ms = if sm.gen_completed_at_ms > 0 {
                        sm.gen_completed_at_ms
                    } else if sm.first_token_at_ms > 0 {
                        sm.first_token_at_ms
                    } else {
                        sm.created_at_ms
                    };
                    if tool_start_ms > 0 && sm.completed_at_ms > tool_start_ms {
                        let exec_ms = (sm.completed_at_ms - tool_start_ms) as u64;
                        if exec_ms <= MAX_MEASURED_DURATION_MS {
                            tool_exec_durations.push((step_ts_ms, exec_ms));
                        }
                    }
                }
                tool_steps.push(DbToolStep {
                    step_ts_ms,
                    is_err: has_err || status == 7 || status == 4,
                    tools: vec![tool_name],
                });
            }
        }

        if !sm.has_usage || step_ts_ms <= 0 {
            continue;
        }

        let is_checkpoint = step_type == 23;
        let gm_opt = if !is_checkpoint && sm.has_gen_ref {
            gm_map.get(&sm.gen_meta_idx)
        } else {
            None
        };

        let (input_tokens, output_tokens, cache_tokens, thinking_tokens) = if let Some(gm) = gm_opt
        {
            (
                if sm.usage.input_tokens > 0 {
                    sm.usage.input_tokens
                } else {
                    gm.usage.input_tokens
                },
                if sm.usage.output_tokens > 0 {
                    sm.usage.output_tokens
                } else {
                    gm.usage.output_tokens
                },
                if sm.usage.cache_read_tokens > 0 {
                    sm.usage.cache_read_tokens
                } else {
                    gm.usage.cache_read_tokens
                },
                if sm.usage.thinking_tokens > 0 {
                    sm.usage.thinking_tokens
                } else {
                    gm.usage.thinking_tokens
                },
            )
        } else {
            (
                sm.usage.input_tokens,
                sm.usage.output_tokens,
                sm.usage.cache_read_tokens,
                sm.usage.thinking_tokens,
            )
        };

        if input_tokens == 0 && output_tokens == 0 && cache_tokens == 0 {
            continue;
        }

        let (mut requested_model, mut physical_model) = if let Some(gm) = gm_opt {
            (gm.display_model.clone(), gm.physical_model.clone())
        } else if let Some((disp, phy)) = model_id_map.get(&sm.usage.model_id) {
            (disp.clone(), phy.clone())
        } else {
            (String::new(), String::new())
        };

        if requested_model.is_empty() && !physical_model.is_empty() {
            requested_model = physical_model.clone();
        }
        if requested_model.is_empty() {
            requested_model = "Unknown Model".to_string();
        }
        if physical_model.is_empty() {
            physical_model = super::StatsEngine::map_physical_model(&requested_model);
        }

        let (ttft_ms, streaming_duration_ms, raw_total_ms) = if is_checkpoint {
            let wall = if sm.completed_at_ms > sm.created_at_ms && sm.created_at_ms > 0 {
                (sm.completed_at_ms - sm.created_at_ms) as u64
            } else {
                0
            };
            (0u64, 0u64, wall)
        } else if let Some(gm) = gm_opt {
            let ttft = gm.ttft_ms;
            let stream = gm.streaming_ms;
            let mut total = ttft.saturating_add(stream);
            if total == 0 && sm.gen_completed_at_ms > sm.created_at_ms && sm.created_at_ms > 0 {
                total = (sm.gen_completed_at_ms - sm.created_at_ms) as u64;
            }
            (ttft, stream, total)
        } else {
            let ttft = if sm.first_token_at_ms > sm.created_at_ms && sm.created_at_ms > 0 {
                (sm.first_token_at_ms - sm.created_at_ms) as u64
            } else {
                0
            };
            let end_ms = if sm.gen_completed_at_ms > sm.created_at_ms {
                sm.gen_completed_at_ms
            } else if sm.completed_at_ms > sm.created_at_ms {
                sm.completed_at_ms
            } else {
                0
            };
            let stream = if end_ms > sm.first_token_at_ms && sm.first_token_at_ms > 0 {
                (end_ms - sm.first_token_at_ms) as u64
            } else {
                0
            };
            let total = if end_ms > sm.created_at_ms && sm.created_at_ms > 0 {
                (end_ms - sm.created_at_ms) as u64
            } else {
                ttft.saturating_add(stream)
            };
            (ttft, stream, total)
        };

        let (duration_ms, tps) = if output_tokens > 0
            && (MIN_MEASURED_DURATION_MS..=MAX_MEASURED_DURATION_MS).contains(&raw_total_ms)
        {
            (
                raw_total_ms,
                (output_tokens as f64 * 1000.0) / raw_total_ms as f64,
            )
        } else {
            (0, 0.0)
        };

        let (context_used_tokens, context_budget_tokens) = gm_opt
            .map(|gm| (gm.context_used_tokens, gm.context_budget_tokens))
            .unwrap_or((0, 0));

        let (time_str, hour) = format_local_time_and_hour(step_ts_ms);
        let is_downgraded = detect_real_downgrade(&requested_model, &physical_model);

        turns.push(DbModelTurn {
            step_index: idx.max(0) as u64,
            created_at_ts: step_ts_ms,
            hour,
            time_str,
            requested_model,
            physical_model,
            is_downgraded,
            input_tokens,
            output_tokens,
            cache_tokens,
            thinking_tokens,
            ttft_ms: if duration_ms > 0 { ttft_ms } else { 0 },
            streaming_duration_ms: if duration_ms > 0 {
                streaming_duration_ms
            } else {
                0
            },
            duration_ms,
            tps,
            context_used_tokens,
            context_budget_tokens,
        });
    }

    let mut api_errors = Vec::new();
    for gm in gm_map.values() {
        if let Some(ref err_msg) = gm.error_message {
            let err_ts = gm
                .last_step_index
                .and_then(|si| step_ts_by_idx.get(&si).copied())
                .unwrap_or(prev_ts_ms);
            if err_ts > 0 {
                let (time_str, _) = format_local_time_and_hour(err_ts);
                let model = if !gm.display_model.is_empty() {
                    gm.display_model.clone()
                } else if !gm.physical_model.is_empty() {
                    gm.physical_model.clone()
                } else {
                    "Unknown Model".to_string()
                };
                api_errors.push(DbApiError {
                    step_ts_ms: err_ts,
                    time_str,
                    code: classify_api_error_code(err_msg),
                    model,
                    message: err_msg.clone(),
                });
            }
        }
    }
    api_errors.sort_by(|a, b| b.step_ts_ms.cmp(&a.step_ts_ms));

    Some(CachedConversationDb {
        db_mtime_ms,
        db_len,
        wal_mtime_ms,
        wal_len,
        step_timestamps,
        user_prompt_timestamps,
        checkpoint_timestamps,
        tool_exec_durations,
        tool_steps,
        api_errors,
        turns,
    })
}

/// 增量缓存读取单个 `conversations/<id>.db`（同时监控 `.db` 和 `.db-wal` 变更，未变更 0 磁盘 IO）
pub(crate) fn load_or_parse_conversation_db(
    db_path: &Path,
) -> Option<(CachedConversationDb, bool)> {
    let (db_mtime_ms, db_len) = file_mtime_and_len(db_path);
    if db_len == 0 {
        return None;
    }
    let wal_path = wal_path_for(db_path);
    let (wal_mtime_ms, wal_len) = file_mtime_and_len(&wal_path);

    let cache = CONV_DB_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(guard) = cache.lock() {
        if let Some(entry) = guard.get(db_path) {
            if entry.db_mtime_ms == db_mtime_ms
                && entry.db_len == db_len
                && entry.wal_mtime_ms == wal_mtime_ms
                && entry.wal_len == wal_len
            {
                return Some((entry.clone(), false));
            }
        }
    }

    let conn = open_conversation_db_readonly(db_path)?;
    let parsed = parse_conversation_connection(&conn, db_mtime_ms, db_len, wal_mtime_ms, wal_len)?;

    if let Ok(mut guard) = cache.lock() {
        guard.insert(db_path.to_path_buf(), parsed.clone());
    }
    Some((parsed, true))
}

pub(crate) fn retain_live_db_cache(live_paths: &HashSet<PathBuf>) {
    if let Some(cache) = CONV_DB_CACHE.get() {
        if let Ok(mut guard) = cache.lock() {
            guard.retain(|p, _| live_paths.contains(p));
        }
    }
}

/// 返回 `.db` 与 `.db-wal` 二者中最新的修改时间（毫秒），用于时间窗口快速跳过旧会话
pub(crate) fn effective_db_mtime_ms(db_path: &Path) -> i64 {
    let (db_m, _) = file_mtime_and_len(db_path);
    let (wal_m, _) = file_mtime_and_len(&wal_path_for(db_path));
    db_m.max(wal_m)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode_varint(mut v: u64, out: &mut Vec<u8>) {
        loop {
            let mut b = (v & 0x7f) as u8;
            v >>= 7;
            if v != 0 {
                b |= 0x80;
            }
            out.push(b);
            if v == 0 {
                break;
            }
        }
    }

    fn encode_field_varint(field_num: u32, v: u64, out: &mut Vec<u8>) {
        encode_varint(((field_num as u64) << 3) | 0, out);
        encode_varint(v, out);
    }

    fn encode_field_bytes(field_num: u32, data: &[u8], out: &mut Vec<u8>) {
        encode_varint(((field_num as u64) << 3) | 2, out);
        encode_varint(data.len() as u64, out);
        out.extend_from_slice(data);
    }

    fn encode_ts_or_dur(secs: u64, nanos: u64) -> Vec<u8> {
        let mut b = Vec::new();
        if secs > 0 {
            encode_field_varint(1, secs, &mut b);
        }
        if nanos > 0 {
            encode_field_varint(2, nanos, &mut b);
        }
        b
    }

    #[test]
    fn parses_conversation_db_gen_metadata_and_steps_with_exact_ttft_thinking_and_checkpoint() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE gen_metadata (idx INTEGER PRIMARY KEY, data BLOB, size INTEGER DEFAULT 0);
             CREATE TABLE steps (
                idx INTEGER PRIMARY KEY,
                step_type INTEGER NOT NULL DEFAULT 0,
                status INTEGER NOT NULL DEFAULT 0,
                metadata BLOB,
                error_details BLOB
             );",
        )
        .unwrap();

        // Build gen_metadata idx=0 (TTFT=7.656s, stream=5.165s -> total=12.821s, out=1040, think=178, ctx=139400/512000)
        let mut usage_b = Vec::new();
        encode_field_varint(1, 1417, &mut usage_b);
        encode_field_varint(2, 3814, &mut usage_b);
        encode_field_varint(3, 1040, &mut usage_b);
        encode_field_varint(5, 135547, &mut usage_b);
        encode_field_varint(9, 178, &mut usage_b);
        encode_field_varint(10, 862, &mut usage_b);

        let mut ctx_inner = Vec::new();
        encode_field_varint(1, 139400, &mut ctx_inner);
        encode_field_varint(4, 512000, &mut ctx_inner);
        let mut ctx_outer = Vec::new();
        encode_field_bytes(10, &ctx_inner, &mut ctx_outer);

        let mut gm_inner = Vec::new();
        encode_field_varint(3, 1417, &mut gm_inner);
        encode_field_bytes(4, &usage_b, &mut gm_inner);
        encode_field_bytes(9, &ctx_outer, &mut gm_inner);
        encode_field_bytes(11, &encode_ts_or_dur(7, 656_000_000), &mut gm_inner);
        encode_field_bytes(12, &encode_ts_or_dur(5, 165_000_000), &mut gm_inner);
        encode_field_bytes(19, b"gemini-4-argon", &mut gm_inner);
        encode_field_bytes(21, b"Gemini 4 Argon (High)", &mut gm_inner);

        let mut gm_outer = Vec::new();
        encode_field_bytes(1, &gm_inner, &mut gm_outer);
        conn.execute(
            "INSERT INTO gen_metadata (idx, data) VALUES (0, ?1)",
            [&gm_outer],
        )
        .unwrap();

        // Step 1: PLANNER_RESPONSE (step_type=15) with protobuf3 default gen_meta_idx=0 omitted on wire
        let mut step1_ref = Vec::new();
        encode_field_bytes(1, b"traj-1", &mut step1_ref);
        let mut step1_meta = Vec::new();
        encode_field_bytes(1, &encode_ts_or_dur(1_795_658_000, 0), &mut step1_meta);
        encode_field_bytes(9, &usage_b, &mut step1_meta);
        encode_field_bytes(20, &step1_ref, &mut step1_meta);
        conn.execute(
            "INSERT INTO steps (idx, step_type, status, metadata, error_details) VALUES (1, 15, 3, ?1, NULL)",
            [&step1_meta],
        )
        .unwrap();

        // Step 2: Tool call (read_file) with error_details
        let mut tool_msg = Vec::new();
        encode_field_bytes(2, b"read_file", &mut tool_msg);
        let mut step2_meta = Vec::new();
        encode_field_bytes(1, &encode_ts_or_dur(1_795_658_013, 0), &mut step2_meta);
        encode_field_bytes(4, &tool_msg, &mut step2_meta);
        conn.execute(
            "INSERT INTO steps (idx, step_type, status, metadata, error_details) VALUES (2, 132, 7, ?1, ?2)",
            rusqlite::params![&step2_meta, b"err".as_slice()],
        )
        .unwrap();

        // Step 3: CHECKPOINT (step_type=23) — must use wall-clock (t8 - t1 = 50s), NOT borrow gm idx=0's 12.8s!
        let mut cp_usage = Vec::new();
        encode_field_varint(1, 1417, &mut cp_usage);
        encode_field_varint(2, 500_000, &mut cp_usage);
        encode_field_varint(3, 5_000, &mut cp_usage);
        encode_field_varint(9, 100, &mut cp_usage);
        let mut step3_meta = Vec::new();
        encode_field_bytes(1, &encode_ts_or_dur(1_795_658_020, 0), &mut step3_meta);
        encode_field_bytes(8, &encode_ts_or_dur(1_795_658_070, 0), &mut step3_meta);
        encode_field_bytes(9, &cp_usage, &mut step3_meta);
        encode_field_bytes(20, &step1_ref, &mut step3_meta);
        conn.execute(
            "INSERT INTO steps (idx, step_type, status, metadata, error_details) VALUES (3, 23, 3, ?1, NULL)",
            [&step3_meta],
        )
        .unwrap();

        let parsed = parse_conversation_connection(&conn, 1, 1, 0, 0).unwrap();
        assert_eq!(parsed.step_timestamps.len(), 3);
        assert_eq!(parsed.tool_steps.len(), 1);
        assert_eq!(parsed.tool_steps[0].tools, vec!["read_file"]);
        assert!(parsed.tool_steps[0].is_err);

        assert_eq!(parsed.turns.len(), 2);
        let t0 = &parsed.turns[0];
        assert_eq!(t0.requested_model, "Gemini 4 Argon (High)");
        assert_eq!(t0.physical_model, "gemini-4-argon");
        assert_eq!(t0.thinking_tokens, 178);
        assert_eq!(t0.ttft_ms, 7_656);
        assert_eq!(t0.streaming_duration_ms, 5_165);
        assert_eq!(t0.duration_ms, 12_821);
        assert!((t0.tps - (1040.0 * 1000.0 / 12_821.0)).abs() < 1e-4);
        assert_eq!(t0.context_used_tokens, 139_400);
        assert_eq!(t0.context_budget_tokens, 512_000);
        assert!(!t0.is_downgraded);

        let t1 = &parsed.turns[1];
        assert_eq!(t1.requested_model, "Gemini 4 Argon (High)");
        assert_eq!(t1.duration_ms, 50_000);
        assert_eq!(t1.ttft_ms, 0);
        assert!((t1.tps - 100.0).abs() < 1e-4);
    }
}
