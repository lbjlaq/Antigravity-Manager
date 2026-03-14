use serde::{Deserialize, Serialize};

fn default_is_gcp_tos() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenData {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
    pub expiry_timestamp: i64,
    pub token_type: String,
    pub email: Option<String>,
    /// Google Cloud 项目ID，用于 API 请求标识
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    /// OAuth client key used to obtain/refresh this token
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth_client_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>, // 新增：Antigravity sessionId
    #[serde(default = "default_is_gcp_tos")]
    pub is_gcp_tos: bool,
}

impl TokenData {
    pub fn new(
        access_token: String,
        refresh_token: String,
        expires_in: i64,
        email: Option<String>,
        project_id: Option<String>,
        session_id: Option<String>,
        is_gcp_tos: bool,
    ) -> Self {
        let expiry_timestamp = chrono::Utc::now().timestamp() + expires_in;
        Self {
            access_token,
            refresh_token,
            expires_in,
            expiry_timestamp,
            token_type: "Bearer".to_string(),
            email,
            project_id,
            oauth_client_key: None,
            session_id,
            is_gcp_tos,
        }
    }

    pub fn with_oauth_client_key(mut self, oauth_client_key: Option<String>) -> Self {
        self.oauth_client_key = oauth_client_key;
        self
    }
}
