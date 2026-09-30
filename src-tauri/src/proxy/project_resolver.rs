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
    let mut last_error = String::new();

    for url in LOAD_PROJECT_ENDPOINTS.iter() {
        let response = client
            .post(*url)
            .bearer_auth(access_token)
            .header("User-Agent", crate::constants::USER_AGENT.as_str())
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await;

        match response {
            Ok(resp) => {
                if resp.status().is_success() {
                    let data: Value = resp
                        .json()
                        .await
                        .map_err(|e| format!("解析响应失败: {}", e))?;

                    if let Some(project_id) =
                        data.get("cloudaicompanionProject").and_then(|v| v.as_str())
                    {
                        return Ok(project_id.to_string());
                    } else {
                        return Err("账号无资格获取官方 cloudaicompanionProject".to_string());
                    }
                } else {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    last_error = format!("loadCodeAssist [{}] 返回错误 {}: {}", url, status, body);
                    continue;
                }
            }
            Err(e) => {
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
