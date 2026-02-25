use serde::{Deserialize, Serialize};

/// 模型配额信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelQuota {
    pub name: String,
    pub percentage: i32,  // 剩余百分比 0-100
    pub reset_time: String,
    // 动态高级属性
    #[serde(default)]
    pub max_output_tokens: Option<u32>,
    #[serde(default)]
    pub thinking_budget: Option<u32>,
    #[serde(default)]
    pub supports_thinking: Option<bool>,
}

/// 配额数据结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaData {
    pub models: Vec<ModelQuota>,
    pub last_updated: i64,
    #[serde(default)]
    pub is_forbidden: bool,
    /// 禁止访问的原因 (403 详细信息)
    #[serde(default)]
    pub forbidden_reason: Option<String>,
    /// 订阅等级 (FREE/PRO/ULTRA)
    #[serde(default)]
    pub subscription_tier: Option<String>,
    /// 模型重定向路由表 (deprecatedModelIds 映射)
    #[serde(default)]
    pub model_aliases: std::collections::HashMap<String, String>,
}

impl QuotaData {
    pub fn new() -> Self {
        Self {
            models: Vec::new(),
            last_updated: chrono::Utc::now().timestamp(),
            is_forbidden: false,
            forbidden_reason: None,
            subscription_tier: None,
            model_aliases: std::collections::HashMap::new(),
        }
    }

    pub fn add_model(
        &mut self, 
        name: String, 
        percentage: i32, 
        reset_time: String,
        max_output_tokens: Option<u32>,
        thinking_budget: Option<u32>,
        supports_thinking: Option<bool>,
    ) {
        self.models.push(ModelQuota {
            name,
            percentage,
            reset_time,
            max_output_tokens,
            thinking_budget,
            supports_thinking,
        });
    }
}

impl Default for QuotaData {
    fn default() -> Self {
        Self::new()
    }
}
