//! 配额数据模型

use serde::{Deserialize, Serialize};

/// 配额数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaData {
    /// 已使用配额
    #[serde(default)]
    pub used: f64,
    
    /// 总配额
    #[serde(default)]
    pub total: f64,
    
    /// 配额使用百分比
    #[serde(default)]
    pub percent: f64,
    
    /// 图片配额已使用
    #[serde(default)]
    pub image_used: f64,
    
    /// 图片配额总量
    #[serde(default)]
    pub image_total: f64,
    
    /// 图片配额百分比
    #[serde(default)]
    pub image_percent: f64,
    
    /// 是否被禁止 (403)
    #[serde(default)]
    pub is_forbidden: bool,
    
    /// 最后更新时间
    #[serde(default)]
    pub updated_at: i64,
    
    /// 原始数据 (可选，用于调试)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<serde_json::Value>,
}

impl Default for QuotaData {
    fn default() -> Self {
        Self {
            used: 0.0,
            total: 100.0,
            percent: 0.0,
            image_used: 0.0,
            image_total: 50.0,
            image_percent: 0.0,
            is_forbidden: false,
            updated_at: chrono::Utc::now().timestamp(),
            raw: None,
        }
    }
}

impl QuotaData {
    /// 创建一个表示被禁止的配额
    pub fn forbidden() -> Self {
        Self {
            is_forbidden: true,
            updated_at: chrono::Utc::now().timestamp(),
            ..Default::default()
        }
    }

    /// 从原始 API 数据解析
    pub fn from_raw(raw: serde_json::Value) -> Self {
        let mut quota = QuotaData::default();
        quota.raw = Some(raw.clone());
        quota.updated_at = chrono::Utc::now().timestamp();

        // 尝试解析文本配额
        if let Some(text) = raw.get("text") {
            quota.used = text.get("used").and_then(|v| v.as_f64()).unwrap_or(0.0);
            quota.total = text.get("total").and_then(|v| v.as_f64()).unwrap_or(100.0);
            quota.percent = if quota.total > 0.0 {
                (quota.used / quota.total * 100.0).min(100.0)
            } else {
                0.0
            };
        }

        // 尝试解析图片配额
        if let Some(image) = raw.get("image") {
            quota.image_used = image.get("used").and_then(|v| v.as_f64()).unwrap_or(0.0);
            quota.image_total = image.get("total").and_then(|v| v.as_f64()).unwrap_or(50.0);
            quota.image_percent = if quota.image_total > 0.0 {
                (quota.image_used / quota.image_total * 100.0).min(100.0)
            } else {
                0.0
            };
        }

        quota
    }

    /// 检查配额是否充足
    pub fn has_quota(&self) -> bool {
        !self.is_forbidden && self.percent < 100.0
    }

    /// 检查图片配额是否充足
    pub fn has_image_quota(&self) -> bool {
        !self.is_forbidden && self.image_percent < 100.0
    }
}
