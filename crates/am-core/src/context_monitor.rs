use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub session_id: String,
    pub updated_at_ms: u64,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnPoint {
    pub step_index: u64,
    pub input_tokens: u64,
    pub cache_read_tokens: u64,
    pub output_tokens: u64,
    pub total_context_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeavyStep {
    pub step_index: u64,
    pub step_type: String,
    pub approx_tokens: u64,
    pub preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextReport {
    pub session_id: Option<String>,
    pub active_context_tokens: u64,
    pub total_input_tokens: u64,
    pub total_cache_read_tokens: u64,
    pub total_output_tokens: u64,
    pub cache_hit_rate: f64,
    pub turns: Vec<TurnPoint>,
    pub heaviest_steps: Vec<HeavyStep>,
    pub recent_sessions: Vec<SessionSummary>,
}

/// 获取跨平台 Antigravity 会话脑图根目录 (`~/.gemini/antigravity/brain`)
pub fn antigravity_brain_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".gemini").join("antigravity").join("brain"))
}

fn transcript_path_for(brain_dir: &Path, session_id: &str) -> PathBuf {
    brain_dir
        .join(session_id)
        .join(".system_generated")
        .join("logs")
        .join("transcript.jsonl")
}

/// 列出最近活跃的会话（按 transcript.jsonl 修改时间倒序）
pub fn list_recent_sessions(limit: usize) -> Vec<SessionSummary> {
    let Some(brain_dir) = antigravity_brain_dir() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&brain_dir) else {
        return Vec::new();
    };

    let mut items = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(session_id) = path
            .file_name()
            .and_then(|s| s.to_str())
            .map(str::to_string)
        else {
            continue;
        };
        let t_path = transcript_path_for(&brain_dir, &session_id);
        let Ok(meta) = std::fs::metadata(&t_path) else {
            continue;
        };
        let updated_at_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let title = extract_session_title(&t_path)
            .unwrap_or_else(|| session_id.chars().take(8).collect::<String>());
        items.push(SessionSummary {
            session_id,
            updated_at_ms,
            title,
        });
    }

    items.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
    items.truncate(limit);
    items
}

fn extract_session_title(path: &Path) -> Option<String> {
    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);
    for line in reader.lines().take(25).flatten() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if v.get("type").and_then(|t| t.as_str()) == Some("USER_INPUT") {
            if let Some(content) = v.get("content").and_then(|c| c.as_str()) {
                let clean = content.trim().replace('\n', " ");
                if !clean.is_empty() {
                    let preview: String = clean.chars().take(42).collect();
                    return Some(preview);
                }
            }
        }
    }
    None
}

/// 分析指定会话（若为 None 则自动选择最近活跃会话）
pub fn analyze_session(session_id: Option<&str>) -> ContextReport {
    let recent_sessions = list_recent_sessions(12);
    let target_id = session_id
        .map(str::to_string)
        .or_else(|| recent_sessions.first().map(|s| s.session_id.clone()));

    let Some(id) = target_id else {
        return ContextReport {
            session_id: None,
            active_context_tokens: 0,
            total_input_tokens: 0,
            total_cache_read_tokens: 0,
            total_output_tokens: 0,
            cache_hit_rate: 0.0,
            turns: Vec::new(),
            heaviest_steps: Vec::new(),
            recent_sessions,
        };
    };

    let Some(brain_dir) = antigravity_brain_dir() else {
        return ContextReport {
            session_id: Some(id),
            active_context_tokens: 0,
            total_input_tokens: 0,
            total_cache_read_tokens: 0,
            total_output_tokens: 0,
            cache_hit_rate: 0.0,
            turns: Vec::new(),
            heaviest_steps: Vec::new(),
            recent_sessions,
        };
    };

    let t_path = transcript_path_for(&brain_dir, &id);
    let Ok(file) = File::open(&t_path) else {
        return ContextReport {
            session_id: Some(id),
            active_context_tokens: 0,
            total_input_tokens: 0,
            total_cache_read_tokens: 0,
            total_output_tokens: 0,
            cache_hit_rate: 0.0,
            turns: Vec::new(),
            heaviest_steps: Vec::new(),
            recent_sessions,
        };
    };

    let reader = BufReader::new(file);
    let mut report = analyze_transcript_lines(reader.lines().map_while(Result::ok));
    report.session_id = Some(id);
    report.recent_sessions = recent_sessions;
    report
}

pub fn analyze_transcript_lines<I>(lines: I) -> ContextReport
where
    I: IntoIterator<Item = String>,
{
    let mut total_input = 0u64;
    let mut total_cache_read = 0u64;
    let mut total_output = 0u64;
    let mut active_context = 0u64;
    let mut turns = Vec::new();
    let mut steps = Vec::new();

    for line in lines {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let step_index = v.get("step_index").and_then(|x| x.as_u64()).unwrap_or(0);
        let step_type = v
            .get("type")
            .and_then(|x| x.as_str())
            .unwrap_or("UNKNOWN")
            .to_string();

        let usage = v
            .get("usage")
            .or_else(|| v.get("token_usage"))
            .unwrap_or(&v);
        {
            let inp = usage
                .get("input_tokens")
                .or_else(|| usage.get("prompt_tokens"))
                .and_then(|x| x.as_u64())
                .unwrap_or(0);
            let cache = usage
                .get("cache_read_tokens")
                .or_else(|| usage.get("cached_content_token_count"))
                .and_then(|x| x.as_u64())
                .unwrap_or(0);
            let out = usage
                .get("output_tokens")
                .or_else(|| usage.get("candidates_token_count"))
                .and_then(|x| x.as_u64())
                .unwrap_or(0);

            if inp > 0 || cache > 0 || out > 0 {
                total_input += inp;
                total_cache_read += cache;
                total_output += out;
                let ctx = inp + cache;
                active_context = ctx;
                turns.push(TurnPoint {
                    step_index,
                    input_tokens: inp,
                    cache_read_tokens: cache,
                    output_tokens: out,
                    total_context_tokens: ctx,
                });
            }
        }

        if let Some(content) = v.get("content").and_then(|c| c.as_str()) {
            let approx_tokens = (content.chars().count() as u64) / 4;
            if approx_tokens > 0 {
                let preview: String = content.trim().replace('\n', " ").chars().take(60).collect();
                steps.push(HeavyStep {
                    step_index,
                    step_type,
                    approx_tokens,
                    preview,
                });
            }
        }
    }

    steps.sort_by(|a, b| b.approx_tokens.cmp(&a.approx_tokens));
    steps.truncate(3);

    let denom = total_input + total_cache_read;
    let cache_hit_rate = if denom > 0 {
        (total_cache_read as f64) / (denom as f64)
    } else {
        0.0
    };

    ContextReport {
        session_id: None,
        active_context_tokens: active_context,
        total_input_tokens: total_input,
        total_cache_read_tokens: total_cache_read,
        total_output_tokens: total_output,
        cache_hit_rate,
        turns,
        heaviest_steps: steps,
        recent_sessions: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyze_transcript_lines() {
        let lines = vec![
            r#"{"step_index":1,"type":"USER_INPUT","content":"hello world"}"#.to_string(),
            r#"{"step_index":2,"type":"PLANNER_RESPONSE","content":"hi there","usage":{"input_tokens":1000,"cache_read_tokens":9000,"output_tokens":200}}"#.to_string(),
        ];
        let report = analyze_transcript_lines(lines);
        assert_eq!(report.active_context_tokens, 10000);
        assert_eq!(report.total_input_tokens, 1000);
        assert_eq!(report.total_cache_read_tokens, 9000);
        assert_eq!(report.total_output_tokens, 200);
        assert!((report.cache_hit_rate - 0.9).abs() < 1e-6);
        assert_eq!(report.turns.len(), 1);
    }
}
