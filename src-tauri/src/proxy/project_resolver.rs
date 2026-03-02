use crate::models::AccountType;
use serde_json::Value;

/// GeminiCLI 使用的 User-Agent
const GEMINI_CLI_USER_AGENT: &str = "GeminiCLI/0.1.5 (Windows; AMD64)";

/// Antigravity sandbox 端点
const ANTIGRAVITY_BASE_URL: &str = "https://daily-cloudcode-pa.sandbox.googleapis.com";
/// GeminiCLI prod 端点
const GEMINI_CLI_BASE_URL: &str = "https://cloudcode-pa.googleapis.com";

fn extract_project_id(data: &Value) -> Option<String> {
    if let Some(project) = data.get("cloudaicompanionProject") {
        if let Some(pid) = project.as_str() {
            return Some(pid.to_string());
        }
        if let Some(obj) = project.as_object() {
            if let Some(pid) = obj.get("id").and_then(|v| v.as_str()) {
                return Some(pid.to_string());
            }
            if let Some(pid) = obj.get("projectId").and_then(|v| v.as_str()) {
                return Some(pid.to_string());
            }
            if let Some(name) = obj.get("name").and_then(|v| v.as_str()) {
                return Some(name.to_string());
            }
        }
    }
    None
}

fn extract_tier_id(data: &Value) -> Option<String> {
    let get_tier = |tier: &Value| -> Option<String> {
        tier.get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                tier.get("name")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
    };

    if let Some(current_tier) = data.get("currentTier") {
        if let Some(tier) = get_tier(current_tier) {
            return Some(tier);
        }
    }

    if let Some(allowed_tiers) = data.get("allowedTiers").and_then(|v| v.as_array()) {
        if let Some(default_tier) = allowed_tiers
            .iter()
            .find(|t| t.get("isDefault").and_then(|v| v.as_bool()) == Some(true))
        {
            if let Some(tier) = get_tier(default_tier) {
                return Some(tier);
            }
        }
        if let Some(first_tier) = allowed_tiers.first() {
            if let Some(tier) = get_tier(first_tier) {
                return Some(tier);
            }
        }
    }

    if let Some(paid_tier) = data.get("paidTier") {
        if let Some(tier) = get_tier(paid_tier) {
            return Some(tier);
        }
    }

    None
}

/// 使用 loadCodeAssist API 获取 project_id
/// 根据 account_type 选择不同的端点和 User-Agent
pub async fn fetch_project_id(
    access_token: &str,
    account_type: AccountType,
) -> Result<String, String> {
    let (base_url, user_agent) = match account_type {
        AccountType::GeminiCli => (GEMINI_CLI_BASE_URL, GEMINI_CLI_USER_AGENT.to_string()),
        AccountType::Antigravity => (
            ANTIGRAVITY_BASE_URL,
            crate::constants::USER_AGENT.to_string(),
        ),
    };

    let url = format!("{}/v1internal:loadCodeAssist", base_url);

    let request_body = serde_json::json!({
        "metadata": {
            "ideType": "ANTIGRAVITY",
            "platform": "PLATFORM_UNSPECIFIED",
            "pluginType": "GEMINI"
        }
    });

    let client = crate::utils::http::get_client();
    let response = client
        .post(&url)
        .bearer_auth(access_token)
        .header("User-Agent", &user_agent)
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

    let data: Value = response
        .json()
        .await
        .map_err(|e| format!("解析响应失败: {}", e))?;

    // 提取 cloudaicompanionProject（支持 string/object 两种结构）
    if let Some(project_id) = extract_project_id(&data) {
        return Ok(project_id);
    }

    // 如果 loadCodeAssist 没有返回 project_id，尝试 onboardUser 回退
    match try_onboard_user(access_token, base_url, &user_agent, Some(&data)).await {
        Ok(pid) => return Ok(pid),
        Err(e) => {
            crate::modules::logger::log_warn(&format!("onboardUser fallback also failed: {}", e));
        }
    }

    Err("账号无资格获取官方 cloudaicompanionProject".to_string())
}

/// onboardUser 回退：当 loadCodeAssist 未返回 project_id 时调用
/// 参考 gcli2api/src/google_oauth_api.py:645-731
async fn try_onboard_user(
    access_token: &str,
    base_url: &str,
    user_agent: &str,
    load_code_assist_resp: Option<&Value>,
) -> Result<String, String> {
    let url = format!("{}/v1internal:onboardUser", base_url);

    let mut request_body = serde_json::json!({
        "metadata": {
            "ideType": "ANTIGRAVITY",
            "platform": "PLATFORM_UNSPECIFIED",
            "pluginType": "GEMINI"
        }
    });

    // 与 gcli2api 对齐：如果能提取 tierId，优先带上 tierId
    if let Some(data) = load_code_assist_resp {
        if let Some(tier_id) = extract_tier_id(data) {
            request_body["tierId"] = serde_json::json!(tier_id);
        }
    }

    let client = crate::utils::http::get_client();

    // 与 gcli2api 对齐：最多轮询 5 次，每次间隔 2 秒
    for attempt in 1..=5 {
        let response = client
            .post(&url)
            .bearer_auth(access_token)
            .header("User-Agent", user_agent)
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| format!("onboardUser 请求失败: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("onboardUser 返回错误 {}: {}", status, body));
        }

        let data: Value = response
            .json()
            .await
            .map_err(|e| format!("解析 onboardUser 响应失败: {}", e))?;

        // 兼容两种响应：
        // 1) 立即返回 project_id
        // 2) 长任务 done=true 且 project_id 在 response.cloudaicompanionProject
        if let Some(project_id) = extract_project_id(&data) {
            crate::modules::logger::log_info(&format!(
                "onboardUser succeeded on attempt {}: project_id={}",
                attempt, project_id
            ));
            return Ok(project_id);
        }
        if let Some(resp) = data.get("response") {
            if let Some(project_id) = extract_project_id(resp) {
                crate::modules::logger::log_info(&format!(
                    "onboardUser succeeded on attempt {}: project_id={}",
                    attempt, project_id
                ));
                return Ok(project_id);
            }
        }

        if data.get("done").and_then(|v| v.as_bool()) == Some(true) {
            return Err("onboardUser done=true but no project_id found".to_string());
        }

        if attempt < 5 {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    }

    Err("onboardUser 未能返回 project_id".to_string())
}
