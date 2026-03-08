// 429 重试策略
// Duration 解析

use once_cell::sync::Lazy;
use regex::Regex;

static DURATION_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"([\d.]+)\s*(ms|s|m|h)").unwrap());

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

/// 从 429 错误中提取 retry delay
pub fn parse_retry_delay(error_text: &str) -> Option<u64> {
    use serde_json::Value;

    let json: Value = serde_json::from_str(error_text).ok()?;
    let details = json.get("error")?.get("details")?.as_array()?;

    // 方式1: RetryInfo.retryDelay
    for detail in details {
        if let Some(type_str) = detail.get("@type").and_then(|v| v.as_str()) {
            if type_str.contains("RetryInfo") {
                if let Some(retry_delay) = detail.get("retryDelay").and_then(|v| v.as_str()) {
                    return parse_duration_ms(retry_delay);
                }
            }
        }
    }

    // 方式2: metadata.quotaResetDelay
    for detail in details {
        if let Some(quota_delay) = detail
            .get("metadata")
            .and_then(|m| m.get("quotaResetDelay"))
            .and_then(|v| v.as_str())
        {
            return parse_duration_ms(quota_delay);
        }
    }

    None
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
