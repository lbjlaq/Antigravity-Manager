use serde_json::Value;

/// 使用 Antigravity 的 loadCodeAssist API 获取 project_id
/// 这是获取 cloudaicompanionProject 的正确方式
pub async fn fetch_project_id(access_token: &str) -> Result<String, String> {
    const ENDPOINTS: [(&str, &str); 2] = [
        (
            "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:loadCodeAssist",
            "daily-cloudcode-pa.sandbox.googleapis.com",
        ),
        (
            "https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist",
            "cloudcode-pa.googleapis.com",
        ),
    ];
    
    let request_body = serde_json::json!({
        "metadata": {
            "ideType": "ANTIGRAVITY"
        }
    });
    
    let client = crate::utils::http::create_client(30);

    let mut last_err: Option<String> = None;
    let mut last_status: Option<reqwest::StatusCode> = None;

    for (idx, (url, host)) in ENDPOINTS.iter().enumerate() {
        let response = client
            .post(*url)
            .bearer_auth(access_token)
            .header("Host", *host)
            .header("User-Agent", "antigravity/1.11.9 windows/amd64")
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await;

        let response = match response {
            Ok(r) => r,
            Err(e) => {
                let msg = format!("loadCodeAssist 请求失败 ({}): {}", host, e);
                tracing::warn!("{}", msg);
                last_err = Some(msg);
                if idx + 1 < ENDPOINTS.len() {
                    continue;
                }
                return Err(last_err.unwrap_or_else(|| "loadCodeAssist 请求失败".to_string()));
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            last_status = Some(status);
            last_err = Some(format!("loadCodeAssist 返回错误 {} ({}): {}", status, host, body));

            // Try the next endpoint on 429/5xx, otherwise surface immediately.
            if (status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error())
                && idx + 1 < ENDPOINTS.len()
            {
                tracing::warn!(
                    "loadCodeAssist failed at {} (status={}); trying next endpoint",
                    host,
                    status
                );
                continue;
            }

            return Err(last_err.unwrap());
        }

        if idx > 0 {
            tracing::info!("loadCodeAssist endpoint fallback succeeded: {}", host);
        }

        let data: Value = response
            .json()
            .await
            .map_err(|e| format!("解析响应失败: {}", e))?;

        // 提取 cloudaicompanionProject
        if let Some(project_id) = data.get("cloudaicompanionProject")
            .and_then(|v| v.as_str()) {
            return Ok(project_id.to_string());
        }

        // If the response is successful but has no project id, don't retry other endpoints.
        last_status = Some(reqwest::StatusCode::OK);
        last_err = Some("loadCodeAssist succeeded but missing cloudaicompanionProject".to_string());
        break;
    }
    
    // 提取 cloudaicompanionProject
    // 如果没有返回 project_id，说明账号无资格，使用内置随机生成逻辑作为兜底
    let mock_id = generate_mock_project_id();
    tracing::warn!(
        "账号无资格获取官方 cloudaicompanionProject，将使用随机生成的 Project ID 作为兜底: {} (last_status={:?}, last_err={:?})",
        mock_id,
        last_status,
        last_err
    );
    Ok(mock_id)
}

/// 生成随机 project_id（当无法从 API 获取时使用）
/// 格式：{形容词}-{名词}-{5位随机字符}
pub fn generate_mock_project_id() -> String {
    use rand::Rng;
    
    let adjectives = ["useful", "bright", "swift", "calm", "bold"];
    let nouns = ["fuze", "wave", "spark", "flow", "core"];
    
    let mut rng = rand::thread_rng();
    let adj = adjectives[rng.gen_range(0..adjectives.len())];
    let noun = nouns[rng.gen_range(0..nouns.len())];
    
    // 生成5位随机字符（base36）
    let random_num: String = (0..5)
        .map(|_| {
            let chars = "abcdefghijklmnopqrstuvwxyz0123456789";
            let idx = rng.gen_range(0..chars.len());
            chars.chars().nth(idx).unwrap()
        })
        .collect();
    
    format!("{}-{}-{}", adj, noun, random_num)
}
