use serde_json::Value;

// loadCodeAssist 端点（fallback order: Daily → Sandbox → Prod）
//
// [FIX Issue #3525] Daily 优先，与官方 Antigravity language_server 的出站端点一致。
// Sandbox 在部分地区会对合规账号返回终止性 400 `User location is not supported for the API use.`；
// 官方从不访问 sandbox 端点，保留它仅作为可用性兜底。
const LOAD_PROJECT_ENDPOINTS: [&str; 3] = [
    "https://daily-cloudcode-pa.googleapis.com/v1internal:loadCodeAssist",
    "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:loadCodeAssist",
    "https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist",
];

/// 使用 Antigravity 的 loadCodeAssist API 获取 project_id
/// 这是获取 cloudaicompanionProject 的正确方式
pub async fn fetch_project_id(access_token: &str) -> Result<String, String> {
    let request_body = serde_json::json!({
        "metadata": {
            "ideType": "ANTIGRAVITY"
        }
    });

    let client = crate::utils::http::get_client();
    let health_tracker = crate::proxy::upstream::client::get_global_endpoint_health();
    let ordered_endpoints = health_tracker.get_ordered_endpoints(&LOAD_PROJECT_ENDPOINTS);
    let mut last_error = String::new();

    for (idx, url) in ordered_endpoints.iter().enumerate() {
        let response = client
            .post(*url)
            .timeout(std::time::Duration::from_millis(3000))
            .bearer_auth(access_token)
            .header("User-Agent", crate::constants::USER_AGENT.as_str())
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await;

        match response {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    health_tracker.mark_success(url);
                    let data: Value = resp
                        .json()
                        .await
                        .map_err(|e| format!("解析响应失败: {}", e))?;

                    if let Some(project_id) =
                        data.get("cloudaicompanionProject").and_then(|v| v.as_str())
                    {
                        if idx > 0 {
                            tracing::info!(
                                "loadCodeAssist fetch_project_id fallback succeeded at endpoint #{} ({})",
                                idx + 1,
                                url
                            );
                        }
                        return Ok(project_id.to_string());
                    } else {
                        return Err("账号无资格获取官方 cloudaicompanionProject".to_string());
                    }
                } else {
                    if crate::proxy::upstream::client::should_try_next_endpoint(status) {
                        health_tracker.mark_failure(url);
                    }
                    let body = resp.text().await.unwrap_or_default();
                    last_error = format!("loadCodeAssist [{}] 返回错误 {}: {}", url, status, body);
                    continue;
                }
            }
            Err(e) => {
                health_tracker.mark_failure(url);
                last_error = format!("loadCodeAssist [{}] 请求失败: {}", url, e);
                continue;
            }
        }
    }

    Err(if last_error.is_empty() {
        "loadCodeAssist 所有端点均不可用".to_string()
    } else {
        last_error
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_project_endpoints_dynamic_health_ordering() {
        let tracker = crate::proxy::upstream::client::EndpointHealthTracker::new(
            std::time::Duration::from_secs(60),
        );

        // 默认顺序为 Daily -> Sandbox -> Prod
        assert_eq!(
            tracker.get_ordered_endpoints(&LOAD_PROJECT_ENDPOINTS),
            vec![
                "https://daily-cloudcode-pa.googleapis.com/v1internal:loadCodeAssist",
                "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:loadCodeAssist",
                "https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist",
            ]
        );

        // 模拟 Daily 挂掉（无论是主聊天超时还是 loadCodeAssist 发现）
        tracker.mark_failure(
            "https://daily-cloudcode-pa.googleapis.com/v1internal:streamGenerateContent",
        );
        let ordered = tracker.get_ordered_endpoints(&LOAD_PROJECT_ENDPOINTS);
        assert_eq!(
            ordered[0],
            "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:loadCodeAssist"
        );

        // 模拟 Sandbox 也故障
        tracker.mark_failure(
            "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:loadCodeAssist",
        );
        let ordered = tracker.get_ordered_endpoints(&LOAD_PROJECT_ENDPOINTS);
        assert_eq!(
            ordered[0],
            "https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist"
        );
    }
}
