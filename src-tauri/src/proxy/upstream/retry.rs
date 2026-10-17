// 429 重试策略
// Duration 解析

use once_cell::sync::Lazy;
use regex::Regex;

static DURATION_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)([\d.]+)\s*(milliseconds?|ms|seconds?|secs?|s|minutes?|mins?|m|hours?|hrs?|h)")
        .unwrap()
});

static RE_TEXT_DELAY_PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| {
    vec![
        Regex::new(r"(?i)quota will reset after ([^.,;\]\n]+)").unwrap(),
        Regex::new(r"(?i)quota will reset in ([^.,;\]\n]+)").unwrap(),
        Regex::new(r"(?i)resets in ([^.,;\]\n]+)").unwrap(),
        Regex::new(r"(?i)retry after ([^.,;\]\n]+)").unwrap(),
        Regex::new(r"(?i)reset after ([^.,;\]\n]+)").unwrap(),
        Regex::new(r"(?i)try again in ([^.,;\]\n]+)").unwrap(),
        Regex::new(r"(?i)backoff for ([^.,;\]\n]+)").unwrap(),
        Regex::new(r"(?i)(?:^|[\s(])wait\s+([^,;\]\n)]+)").unwrap(),
    ]
});

static RE_LEGACY_DELAY_PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| {
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
        let unit = cap[2].to_ascii_lowercase();

        match unit.as_str() {
            unit if unit == "ms" || unit.starts_with("millisecond") => total_ms += value,
            unit if unit == "s" || unit.starts_with("sec") || unit.starts_with("second") => {
                total_ms += value * 1000.0
            }
            unit if unit == "m" || unit.starts_with("min") || unit.starts_with("minute") => {
                total_ms += value * 60.0 * 1000.0
            }
            unit if unit == "h" || unit.starts_with("hr") || unit.starts_with("hour") => {
                total_ms += value * 60.0 * 60.0 * 1000.0
            }
            _ => {}
        }
    }

    if !matched {
        return None;
    }

    Some(total_ms.round() as u64)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryDelaySource {
    Structured,
    ResponseText,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedRetryDelay {
    pub raw_ms: u64,
    pub source: RetryDelaySource,
}

impl ParsedRetryDelay {
    pub fn actual_wait_ms(self) -> u64 {
        let buffer_ms = match self.source {
            RetryDelaySource::Structured => 200,
            RetryDelaySource::ResponseText => 1000,
        };
        self.raw_ms.saturating_add(buffer_ms)
    }
}

/// 从 Retry-After 或 429 错误中提取原始 retry delay (深度递归解析)
pub fn parse_retry_delay(error_text: &str, retry_after: Option<&str>) -> Option<u64> {
    parse_retry_delay_with_source(error_text, retry_after).map(|delay| delay.raw_ms)
}

pub fn parse_retry_delay_with_source(
    error_text: &str,
    retry_after: Option<&str>,
) -> Option<ParsedRetryDelay> {
    // 1. Retry-After delta-seconds，也兼容已有的 duration 字符串来源
    let header_delay =
        if let Some(value) = retry_after.map(str::trim).filter(|value| !value.is_empty()) {
            if let Ok(seconds) = value.parse::<u64>() {
                seconds.checked_mul(1000).map(|raw_ms| ParsedRetryDelay {
                    raw_ms,
                    source: RetryDelaySource::Structured,
                })
            } else {
                parse_duration_ms(value).map(|delay| ParsedRetryDelay {
                    raw_ms: delay,
                    source: RetryDelaySource::Structured,
                })
            }
        } else {
            None
        };

    // 2. 结构化 JSON 字段优先，避免把 JSON 中的 retryDelay 当作响应文字。
    let body_structured_delay = if let Ok(json) = serde_json::from_str(error_text) {
        extract_structured_delay_recursive(&json, 0, false).map(|raw_ms| ParsedRetryDelay {
            raw_ms,
            source: RetryDelaySource::Structured,
        })
    } else {
        None
    };

    // 3. 从响应文字中提取自然语言时长。
    let body_text_delay = if body_structured_delay.is_none() {
        RE_TEXT_DELAY_PATTERNS.iter().find_map(|re| {
            re.captures(error_text).and_then(|cap| {
                parse_duration_ms(&cap[1]).map(|delay| ParsedRetryDelay {
                    raw_ms: delay,
                    source: RetryDelaySource::ResponseText,
                })
            })
        })
    } else {
        None
    };

    let body_delay = body_structured_delay.or(body_text_delay);

    match (header_delay, body_delay) {
        (Some(h), Some(b)) => {
            // [FIX #3697] 当上游既返回 Header (如防 DDoS 默认 Retry-After: 8) 又在 Body 中返回较长的配额重置时长 (如 resets in 1h19m 或 quotaResetDelay) 时，
            // 必须选取二者较大者 (max)，防止 8s 假阳性提前解锁导致反复 429 失败。
            if b.raw_ms >= h.raw_ms {
                Some(b)
            } else {
                Some(h)
            }
        }
        (Some(h), None) => Some(h),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

/// Preserve the pre-state-machine delay parsing used by the Claude handler.
pub(crate) fn parse_legacy_retry_delay(error_text: &str) -> Option<u64> {
    for re in RE_LEGACY_DELAY_PATTERNS.iter() {
        if let Some(cap) = re.captures(error_text) {
            if let Some(delay) = parse_duration_ms(&cap[1]) {
                return Some(delay);
            }
        }
    }

    let delay = if let Ok(json) = serde_json::from_str(error_text) {
        extract_structured_delay_recursive(&json, 0, true)
    } else {
        None
    };

    delay.map(|delay_ms| delay_ms.saturating_add(1500))
}

/// 递归提取结构化延迟
fn extract_structured_delay_recursive(
    value: &serde_json::Value,
    depth: usize,
    parse_unkeyed_strings: bool,
) -> Option<u64> {
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
                let normalized_key = key.to_lowercase().replace(['-', '_'], "");
                if RETRY_HINT_KEYS.contains(normalized_key.as_str()) {
                    // 如果命中了 Hint Key，直接尝试解析其内容
                    if let Some(d) = parse_structured_duration_value(val) {
                        return Some(d);
                    }
                }
                // 继续深度搜索
                if let Some(d) =
                    extract_structured_delay_recursive(val, depth + 1, parse_unkeyed_strings)
                {
                    return Some(d);
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for val in arr {
                if let Some(d) =
                    extract_structured_delay_recursive(val, depth + 1, parse_unkeyed_strings)
                {
                    return Some(d);
                }
            }
        }
        serde_json::Value::String(s) if parse_unkeyed_strings => return parse_duration_ms(s),
        serde_json::Value::String(_) => {}
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
/// 当 429 报错提示的重置时间在 5s 内，则原地重试比切换账号更有利。
pub fn should_grace_retry(duration_ms: u64) -> bool {
    duration_ms > 0 && duration_ms <= 5000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_duration_ms() {
        assert_eq!(parse_duration_ms("1.5s"), Some(1500));
        assert_eq!(parse_duration_ms("200ms"), Some(200));
        assert_eq!(parse_duration_ms("1h16m0.667s"), Some(4560667));
        assert_eq!(parse_duration_ms("58h24m53s"), Some(210_293_000));
        assert_eq!(parse_duration_ms("72h"), Some(259_200_000));
        assert_eq!(parse_duration_ms("invalid"), None);
    }

    #[test]
    fn task_parses_google_retry_info_and_retry_after() {
        let retry_info_1s = r#"{
            "error": {
                "details": [{
                    "@type": "type.googleapis.com/google.rpc.RetryInfo",
                    "retryDelay": "1s"
                }]
            }
        }"#;
        let retry_info_3s = r#"{
            "error": {
                "details": [{
                    "@type": "type.googleapis.com/google.rpc.RetryInfo",
                    "retryDelay": "3s"
                }]
            }
        }"#;

        let structured_1s = parse_retry_delay_with_source(retry_info_1s, None).unwrap();
        assert_eq!(structured_1s.raw_ms, 1000);
        assert_eq!(structured_1s.source, RetryDelaySource::Structured);
        assert_eq!(structured_1s.actual_wait_ms(), 1200);

        let structured_3s = parse_retry_delay_with_source(retry_info_3s, None).unwrap();
        assert_eq!(structured_3s.actual_wait_ms(), 3200);

        let quota_reset = parse_retry_delay_with_source(
            r#"{"error":{"details":[{"metadata":{"quotaResetDelay":"5s"}}]}}"#,
            None,
        )
        .unwrap();
        assert_eq!(quota_reset.actual_wait_ms(), 5200);
        assert!(should_grace_retry(quota_reset.raw_ms));

        let retry_after = parse_retry_delay_with_source("", Some("3")).unwrap();
        assert_eq!(retry_after.actual_wait_ms(), 3200);

        let response_text = parse_retry_delay_with_source("quota reset after 3s", None).unwrap();
        assert_eq!(response_text.source, RetryDelaySource::ResponseText);
        assert_eq!(response_text.actual_wait_ms(), 4000);
    }

    #[test]
    fn test_individual_quota_resets_in_and_header_overrides() {
        let err_body = "Individual quota reached. Resets in 1h19m6s.";
        let res = parse_retry_delay_with_source(err_body, None);
        assert!(res.is_some(), "Should parse 'Resets in 1h19m6s.'");
        assert_eq!(res.unwrap().raw_ms, 4_746_000);

        // When front-door Anti-DDoS header (Retry-After: 8) is present with long quota reset in body,
        // it must prioritize the larger quota reset delay (4746s), NOT 8s!
        let res_with_header = parse_retry_delay_with_source(err_body, Some("8"));
        assert!(res_with_header.is_some());
        assert_eq!(
            res_with_header.unwrap().raw_ms,
            4_746_000,
            "Must choose max(8s, 4746s) = 4746s, not premature 8s"
        );

        let json_err = r#"{"error":{"details":[{"metadata":{"quotaResetDelay":"4746s"}}]}}"#;
        let res_json_header = parse_retry_delay_with_source(json_err, Some("8"));
        assert!(res_json_header.is_some());
        assert_eq!(
            res_json_header.unwrap().raw_ms,
            4_746_000,
            "Structured quotaResetDelay must override 8s front-door header"
        );
    }
}
