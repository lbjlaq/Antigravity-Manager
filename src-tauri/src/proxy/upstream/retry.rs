// 429 重试策略
// Duration 解析

use once_cell::sync::Lazy;
use regex::Regex;

static DURATION_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"([\d.]+)\s*(ms|s|m|h)").unwrap());

static RE_QUOTA_PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| {
    vec![
        Regex::new(r"(?i)quota will reset after ([^.,;\]\n]+)").unwrap(),
        Regex::new(r"(?i)retry after ([^.,;\]\n]+)").unwrap(),
        Regex::new(r#"(?i)quotaResetDelay["'=:\s]+([^\s,"}\]]+)"#).unwrap(),
    ]
});

static RETRY_HINT_KEYS: Lazy<std::collections::HashSet<&'static str>> = Lazy::new(|| {
    [
        "retryafter",
        "retry_after",
        "retrydelay",
        "retry_delay",
        "quotaresetdelay",
        "quota_reset_delay",
        "backofflimit",
        "backoff_limit",
    ]
    .iter()
    .cloned()
    .collect()
});

/// 解析 Duration 字符串 (e.g., "1.5s", "200ms", "1h16m0.667s")
pub fn parse_duration_ms(duration_str: &str) -> Option<u64> {
    let mut total_ms: f64 = 0.0;
    let mut matched = false;

    for cap in DURATION_RE.captures_iter(duration_str) {
        matched = true;
        let value: f64 = cap[1].parse().ok()?;
        let unit = &cap[2];

        match unit {
            "ms" => total_ms += value,
            "s" => total_ms += value * 1000.0,
            "m" => total_ms += value * 60.0 * 1000.0,
            "h" => total_ms += value * 60.0 * 60.0 * 1000.0,
            _ => {}
        }
    }

    if !matched {
        return None;
    }

    Some(total_ms.round() as u64)
}

/// 从 429 错误中提取 retry delay (深度递归解析)
pub fn parse_retry_delay(error_text: &str) -> Option<u64> {
    // Removed unused import: use serde_json::Value;

    // 1. 尝试正则提取 (针对非 JSON 文本或嵌套不深的文本)
    for re in RE_QUOTA_PATTERNS.iter() {
        if let Some(cap) = re.captures(error_text) {
            if let Some(delay) = parse_duration_ms(&cap[1]) {
                return Some(delay);
            }
        }
    }

    // 2. 尝试结构化 JSON 解析 (递归扫描)
    let delay = if let Ok(json) = serde_json::from_str(error_text) {
        extract_structured_delay_recursive(&json, 0)
    } else {
        None
    };

    // [NEW] 引入 1500ms 官方 "Grace Window" 缓冲区
    // 官方实现会在解析出的延迟基础上强制多等 1.5s，以确保 100% 越过 Google 配额重置点。
    delay.map(|d| d + 1500)
}

/// 递归提取结构化延迟
fn extract_structured_delay_recursive(value: &serde_json::Value, depth: usize) -> Option<u64> {
    if depth > 8 {
        return None;
    }

    match value {
        serde_json::Value::Object(map) => {
            // 检查当前对象是否本身就是一个 Duration 对象 (seconds/nanos)
            if let Some(d) = parse_structured_duration_object(value) {
                return Some(d);
            }

            // 递归扫描子字段
            for (key, val) in map {
                // 模糊 Key 匹配 (转小写, 去除分隔符)
                let normalized_key = key.to_lowercase().replace('-', "").replace('_', "");
                if RETRY_HINT_KEYS.contains(normalized_key.as_str()) {
                    // 如果命中了 Hint Key，直接尝试解析其内容
                    if let Some(d) = parse_structured_duration_value(val) {
                        return Some(d);
                    }
                }
                // 继续深度搜索
                if let Some(d) = extract_structured_delay_recursive(val, depth + 1) {
                    return Some(d);
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for val in arr {
                if let Some(d) = extract_structured_delay_recursive(val, depth + 1) {
                    return Some(d);
                }
            }
        }
        serde_json::Value::String(s) => {
            return parse_duration_ms(s);
        }
        _ => {}
    }
    None
}

/// 解析强类型的 Duration 对象 (Google 格式: {seconds: 1, nanos: 0})
fn parse_structured_duration_object(value: &serde_json::Value) -> Option<u64> {
    let obj = value.as_object()?;
    let seconds = obj
        .get("seconds")
        .or_else(|| obj.get("Seconds"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let nanos = obj
        .get("nanos")
        .or_else(|| obj.get("Nanos"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);

    if seconds > 0.0 || nanos > 0.0 {
        let total_ms = (seconds * 1000.0) + (nanos / 1_000_000.0);
        return Some(total_ms.round() as u64);
    }
    None
}

/// 解析各种可能包含时长信息的 Value
fn parse_structured_duration_value(value: &serde_json::Value) -> Option<u64> {
    match value {
        serde_json::Value::String(s) => parse_duration_ms(s),
        serde_json::Value::Number(n) => n.as_f64().map(|f| (f * 1000.0).round() as u64),
        serde_json::Value::Object(_) => parse_structured_duration_object(value),
        _ => None,
    }
}

/// [NEW] 判断是否应当执行 Grace Retry (原地重试)
/// 当 429 报错提示的重置时间在接受范围内（如 2s 内），则原地重试比切换账号更有利。
pub fn should_grace_retry(duration_ms: u64) -> bool {
    // 默认阈值：2000ms
    duration_ms > 0 && duration_ms <= 2000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_duration_ms() {
        assert_eq!(parse_duration_ms("1.5s"), Some(1500));
        assert_eq!(parse_duration_ms("200ms"), Some(200));
        assert_eq!(parse_duration_ms("1h16m0.667s"), Some(4560667));
        assert_eq!(parse_duration_ms("invalid"), None);
    }

    #[test]
    fn test_parse_retry_delay() {
        let error_json = r#"{
            "error": {
                "details": [{
                    "@type": "type.googleapis.com/google.rpc.RetryInfo",
                    "retryDelay": "1.203608125s"
                }]
            }
        }"#;

        assert_eq!(parse_retry_delay(error_json), Some(1204));
    }
}
