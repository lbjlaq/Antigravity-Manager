use serde_json::{json, Value};
use tokio::time::Duration;

use crate::proxy::config::{UpstreamProxyConfig, ZaiWebReaderUrlNormalizationMode};
use crate::proxy::ZaiConfig;

const ZAI_WEB_SEARCH_GENERAL_URL: &str = "https://api.z.ai/api/paas/v4/web_search";
const ZAI_WEB_SEARCH_CODING_URL: &str = "https://api.z.ai/api/coding/paas/v4/web_search";

const ZAI_WEB_READER_GENERAL_URL: &str = "https://api.z.ai/api/paas/v4/reader";
const ZAI_WEB_READER_CODING_URL: &str = "https://api.z.ai/api/coding/paas/v4/reader";

fn build_client(
    upstream_proxy: UpstreamProxyConfig,
    timeout_secs: u64,
) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder().timeout(Duration::from_secs(timeout_secs.max(5)));

    if upstream_proxy.enabled && !upstream_proxy.url.is_empty() {
        let proxy = reqwest::Proxy::all(&upstream_proxy.url)
            .map_err(|e| format!("Invalid upstream proxy url: {}", e))?;
        builder = builder.proxy(proxy);
    }

    builder
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))
}

fn should_strip_tracking_param(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    k.starts_with("utm_")
        || k.starts_with("hsa_")
        || matches!(
            k.as_str(),
            "gclid" | "fbclid" | "gbraid" | "wbraid" | "msclkid"
        )
}

pub(crate) fn normalize_web_reader_url(
    url_str: &str,
    mode: ZaiWebReaderUrlNormalizationMode,
) -> Option<String> {
    use ZaiWebReaderUrlNormalizationMode as Mode;

    if matches!(mode, Mode::Off) {
        return None;
    }

    let mut url = url::Url::parse(url_str).ok()?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return None;
    }

    match mode {
        Mode::Off => None,
        Mode::StripQuery => {
            if url.query().is_none() {
                return None;
            }
            url.set_query(None);
            Some(url.to_string())
        }
        Mode::StripTrackingQuery => {
            let Some(q) = url.query() else {
                return None;
            };

            let original_len = url::form_urlencoded::parse(q.as_bytes()).count();
            let pairs: Vec<(String, String)> = url::form_urlencoded::parse(q.as_bytes())
                .into_owned()
                .filter(|(k, _)| !should_strip_tracking_param(k))
                .collect();

            if pairs.len() == original_len {
                return None;
            }

            if pairs.is_empty() {
                url.set_query(None);
                return Some(url.to_string());
            }

            let mut ser = url::form_urlencoded::Serializer::new(String::new());
            for (k, v) in pairs {
                ser.append_pair(&k, &v);
            }
            let new_q = ser.finish();
            url.set_query(Some(&new_q));
            Some(url.to_string())
        }
    }
}

pub fn web_search_tool_specs() -> Vec<Value> {
    vec![json!({
        "name": "webSearchPrime",
        "description": "Search web information and return results including titles, URLs, and summaries.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "search_query": { "type": "string", "description": "The content to be searched." },
                "count": { "type": "integer", "minimum": 1, "maximum": 50, "description": "Number of results to return (1-50). Defaults to 10." },
                "search_domain_filter": { "type": "string", "description": "Whitelist domain filter (e.g. www.example.com)." },
                "search_recency_filter": { "type": "string", "enum": ["oneDay","oneWeek","oneMonth","oneYear","noLimit"], "description": "Recency filter (default: noLimit)." }
            },
            "required": ["search_query"]
        }
    })]
}

fn parse_search_query(args: &Value) -> Option<String> {
    args.get("search_query")
        .or_else(|| args.get("query"))
        .or_else(|| args.get("q"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn parse_search_engine(args: &Value) -> String {
    args.get("search_engine")
        .or_else(|| args.get("engine"))
        .and_then(|v| v.as_str())
        .unwrap_or("search-prime")
        .to_string()
}

fn parse_search_count(args: &Value) -> Option<i64> {
    let raw = args
        .get("count")
        .or_else(|| args.get("limit"))
        .or_else(|| args.get("top_k"))
        .or_else(|| args.get("num_results"));

    match raw {
        Some(Value::Number(n)) => n.as_i64(),
        Some(Value::String(s)) => s.parse::<i64>().ok(),
        _ => None,
    }
}

fn format_web_search_response(resp: &Value) -> String {
    let mut out = String::new();
    let results = resp
        .get("search_result")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if results.is_empty() {
        return "No results.".to_string();
    }

    for (idx, r) in results.iter().enumerate() {
        let title = r.get("title").and_then(|v| v.as_str()).unwrap_or("").trim();
        let link = r.get("link").and_then(|v| v.as_str()).unwrap_or("").trim();
        let media = r.get("media").and_then(|v| v.as_str()).unwrap_or("").trim();
        let content = r
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();

        let title_part = if !title.is_empty() {
            title
        } else {
            "(untitled)"
        };
        if !link.is_empty() {
            out.push_str(&format!("{}. [{}]({})", idx + 1, title_part, link));
        } else {
            out.push_str(&format!("{}. {}", idx + 1, title_part));
        }
        if !media.is_empty() {
            out.push_str(&format!(" — {}", media));
        }
        out.push('\n');
        if !content.is_empty() {
            out.push_str(content);
            out.push('\n');
        }
        out.push('\n');
    }

    out.trim().to_string()
}

pub async fn call_web_search_prime(
    zai: &ZaiConfig,
    upstream_proxy: UpstreamProxyConfig,
    timeout_secs: u64,
    arguments: &Value,
) -> Result<Value, String> {
    if !zai.enabled || zai.api_key.trim().is_empty() {
        return Err("z.ai is not configured".to_string());
    }

    let search_query =
        parse_search_query(arguments).ok_or_else(|| "Missing search_query".to_string())?;
    let search_engine = parse_search_engine(arguments);
    let count = parse_search_count(arguments);

    let mut body = json!({
        "search_engine": search_engine,
        "search_query": search_query
    });

    if let Some(n) = count {
        body["count"] = Value::Number(serde_json::Number::from(n));
    }
    if let Some(v) = arguments
        .get("search_domain_filter")
        .and_then(|v| v.as_str())
    {
        body["search_domain_filter"] = Value::String(v.to_string());
    }
    if let Some(v) = arguments
        .get("search_recency_filter")
        .and_then(|v| v.as_str())
    {
        body["search_recency_filter"] = Value::String(v.to_string());
    }
    if let Some(v) = arguments.get("request_id").and_then(|v| v.as_str()) {
        body["request_id"] = Value::String(v.to_string());
    }
    if let Some(v) = arguments.get("user_id").and_then(|v| v.as_str()) {
        body["user_id"] = Value::String(v.to_string());
    }

    let client = build_client(upstream_proxy, timeout_secs)?;
    let api_key_raw = if !zai.mcp.api_key_override.trim().is_empty() {
        zai.mcp.api_key_override.trim()
    } else {
        zai.api_key.trim()
    };
    let api_key = crate::proxy::zai_auth::normalize_api_key(api_key_raw);

    // Coding Plan keys often require the `/coding/paas/v4` endpoint.
    let candidates = [
        ("coding", ZAI_WEB_SEARCH_CODING_URL),
        ("general", ZAI_WEB_SEARCH_GENERAL_URL),
    ];

    let mut last_err: Option<String> = None;
    for (label, url) in candidates {
        let resp = client
            .post(url)
            .bearer_auth(&api_key)
            .header("X-Title", "Web Search MCP Local")
            .header("Accept-Language", "en-US,en")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Upstream request failed ({}): {}", label, e))?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            let err = format!("HTTP {} ({}): {}", status, label, text);
            last_err = Some(err);
            if label == "coding" && matches!(status, 401 | 403 | 404) {
                continue;
            }
            return Err(last_err.unwrap_or_else(|| "Web search request failed".to_string()));
        }

        let v: Value = resp
            .json()
            .await
            .map_err(|e| format!("Invalid JSON response ({}): {}", label, e))?;

        let text = format_web_search_response(&v);
        return Ok(json!({ "content": [ { "type": "text", "text": text } ] }));
    }

    Err(last_err.unwrap_or_else(|| "Web search request failed".to_string()))
}

pub fn web_reader_tool_specs() -> Vec<Value> {
    vec![json!({
        "name": "webReader",
        "description": "Read and parse webpage content from a URL.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "The URL to retrieve." },
                "timeout": { "type": "integer", "description": "Request timeout in seconds (default: 20)." },
                "no_cache": { "type": "boolean", "description": "Disable caching (default: false)." },
                "return_format": { "type": "string", "description": "Return format, e.g. markdown or text (default: markdown)." },
                "retain_images": { "type": "boolean", "description": "Whether to retain images (default: true)." },
                "no_gfm": { "type": "boolean", "description": "Disable GitHub Flavored Markdown (default: false)." },
                "keep_img_data_url": { "type": "boolean", "description": "Keep image data URLs (default: false)." },
                "with_images_summary": { "type": "boolean", "description": "Include image summary (default: false)." },
                "with_links_summary": { "type": "boolean", "description": "Include links summary (default: false)." }
            },
            "required": ["url"]
        }
    })]
}

fn format_web_reader_response(resp: &Value) -> String {
    let title = resp
        .get("reader_result")
        .and_then(|v| v.get("title"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    let url = resp
        .get("reader_result")
        .and_then(|v| v.get("url"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    let desc = resp
        .get("reader_result")
        .and_then(|v| v.get("description"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    let content = resp
        .get("reader_result")
        .and_then(|v| v.get("content"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();

    let mut out = String::new();
    if !title.is_empty() {
        out.push_str("# ");
        out.push_str(title);
        out.push('\n');
        out.push('\n');
    }
    if !url.is_empty() {
        out.push_str("URL: ");
        out.push_str(url);
        out.push('\n');
        out.push('\n');
    }
    if !desc.is_empty() {
        out.push_str(desc);
        out.push('\n');
        out.push('\n');
    }
    out.push_str(content);
    out.trim().to_string()
}

pub async fn call_web_reader(
    zai: &ZaiConfig,
    upstream_proxy: UpstreamProxyConfig,
    timeout_secs: u64,
    url_normalization: ZaiWebReaderUrlNormalizationMode,
    arguments: &Value,
) -> Result<Value, String> {
    let url = arguments
        .get("url")
        .or_else(|| arguments.get("uri"))
        .or_else(|| arguments.get("link"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing url".to_string())?;
    let normalized =
        normalize_web_reader_url(url, url_normalization).unwrap_or_else(|| url.to_string());

    let mut body = json!({ "url": normalized });

    for key in [
        "timeout",
        "no_cache",
        "return_format",
        "retain_images",
        "no_gfm",
        "keep_img_data_url",
        "with_images_summary",
        "with_links_summary",
    ] {
        if let Some(v) = arguments.get(key) {
            body[key] = v.clone();
        }
    }

    let client = build_client(upstream_proxy, timeout_secs)?;
    let api_key_raw = if !zai.mcp.api_key_override.trim().is_empty() {
        zai.mcp.api_key_override.trim()
    } else {
        zai.api_key.trim()
    };
    let api_key = crate::proxy::zai_auth::normalize_api_key(api_key_raw);
    if api_key.is_empty() {
        return Err("z.ai api_key is missing".to_string());
    }

    let candidates = [
        ("coding", ZAI_WEB_READER_CODING_URL),
        ("general", ZAI_WEB_READER_GENERAL_URL),
    ];

    let mut last_err: Option<String> = None;
    for (label, url) in candidates {
        let resp = client
            .post(url)
            .bearer_auth(&api_key)
            .header("X-Title", "Web Reader MCP Local")
            .header("Accept-Language", "en-US,en")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Upstream request failed ({}): {}", label, e))?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            let err = format!("HTTP {} ({}): {}", status, label, text);
            last_err = Some(err);
            if label == "coding" && matches!(status, 401 | 403 | 404) {
                continue;
            }
            return Err(last_err.unwrap_or_else(|| "Web reader request failed".to_string()));
        }

        let v: Value = resp
            .json()
            .await
            .map_err(|e| format!("Invalid JSON response ({}): {}", label, e))?;

        let text = format_web_reader_response(&v);
        if text.is_empty() {
            return Err("Reader response missing content".to_string());
        }

        return Ok(json!({ "content": [ { "type": "text", "text": text } ] }));
    }

    Err(last_err.unwrap_or_else(|| "Web reader request failed".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_off_returns_none() {
        assert_eq!(
            normalize_web_reader_url(
                "https://example.com/path?utm_source=x",
                ZaiWebReaderUrlNormalizationMode::Off
            ),
            None
        );
    }

    #[test]
    fn normalize_strip_query_removes_entire_query() {
        let out = normalize_web_reader_url(
            "https://example.com/path?a=1&utm_source=x",
            ZaiWebReaderUrlNormalizationMode::StripQuery,
        )
        .unwrap();
        assert_eq!(out, "https://example.com/path");
    }

    #[test]
    fn normalize_strip_tracking_query_keeps_non_tracking_params() {
        let out = normalize_web_reader_url(
            "https://example.com/path?a=1&utm_source=x&gclid=zzz&keep=ok",
            ZaiWebReaderUrlNormalizationMode::StripTrackingQuery,
        )
        .unwrap();
        // Order is preserved by the serializer for remaining params.
        assert!(
            out == "https://example.com/path?a=1&keep=ok"
                || out == "https://example.com/path?keep=ok&a=1"
        );
    }

    #[test]
    fn normalize_ignores_non_http_schemes() {
        assert_eq!(
            normalize_web_reader_url(
                "file:///etc/passwd",
                ZaiWebReaderUrlNormalizationMode::StripQuery
            ),
            None
        );
    }
}
