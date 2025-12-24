use serde_json::Value;

/// 使用 Antigravity 的 loadCodeAssist API 获取 project_id
/// 这是获取 cloudaicompanionProject 的正确方式
pub async fn fetch_project_id(access_token: &str) -> Result<String, String> {
    let url = "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:loadCodeAssist";
    
    let request_body = serde_json::json!({
        "metadata": {
            "ideType": "ANTIGRAVITY"
        }
    });
    
    let client = crate::utils::http::create_client(30);
    let response = client
        .post(url)
        .bearer_auth(access_token)
        .header("Host", "daily-cloudcode-pa.sandbox.googleapis.com")
        .header("User-Agent", "antigravity/1.11.9 windows/amd64")
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| format!("loadCodeAssist 请求失败: {}", e))?;
    
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("loadCodeAssist 返回错误 {}: {}", status, body));
    }
    
    let data: Value = response.json()
        .await
        .map_err(|e| format!("解析响应失败: {}", e))?;
    
    // 提取 cloudaicompanionProject
    if let Some(project_id) = data.get("cloudaicompanionProject")
        .and_then(|v| v.as_str()) {
        return Ok(project_id.to_string());
    }
    
    // 如果没有返回 project_id，说明账号无资格
    Err("账号无资格获取 cloudaicompanionProject".to_string())
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
