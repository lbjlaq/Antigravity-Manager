use once_cell::sync::Lazy;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;

/// 对应 Google `v1internal:fetchAvailableModels` 接口返回的单个模型权威数据结构。
/// 结构体中的每一个属性与官方 API 100% 严格对齐。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfficialModelInfo {
    #[serde(default)]
    pub display_name: Option<String>,
    /// 官方模型代号 (如 MODEL_PLACEHOLDER_M35, MODEL_PLACEHOLDER_M318 等)
    #[serde(default)]
    pub model: String,
    /// 模型供应商 (如 MODEL_PROVIDER_ANTHROPIC, MODEL_PROVIDER_GOOGLE, MODEL_PROVIDER_OPENAI)
    #[serde(default)]
    pub model_provider: Option<String>,
    /// API 供应商 (如 API_PROVIDER_ANTHROPIC_VERTEX, API_PROVIDER_GOOGLE_GEMINI, API_PROVIDER_OPENAI_VERTEX)
    #[serde(default)]
    pub api_provider: Option<String>,
    #[serde(default)]
    pub max_output_tokens: Option<i64>,
    #[serde(default)]
    pub max_tokens: Option<i64>,
    #[serde(default)]
    pub supports_thinking: Option<bool>,
    #[serde(default)]
    pub thinking_budget: Option<i64>,
    #[serde(default)]
    pub min_thinking_budget: Option<i64>,
    #[serde(default)]
    pub supports_images: Option<bool>,
    #[serde(default)]
    pub supports_video: Option<bool>,
    #[serde(default)]
    pub recommended: Option<bool>,
    #[serde(default)]
    pub is_internal: Option<bool>,
    #[serde(default)]
    pub tag_title: Option<String>,
    #[serde(default)]
    pub tag_description: Option<String>,
    #[serde(default)]
    pub quota_info: Option<OfficialQuotaInfo>,
    #[serde(default)]
    pub supported_mime_types: Option<HashMap<String, bool>>,
    #[serde(default)]
    pub model_experiments: Option<serde_json::Value>,
    #[serde(default)]
    pub vertex_model_id: Option<String>,
    #[serde(default)]
    pub prompt_templater_type: Option<String>,
    #[serde(default)]
    pub tool_formatter_type: Option<String>,
    /// 官方 `fetchAvailableModels` 里该字段有时是字符串（`"HIGH"`），
    /// 有时是整数档位（如 `3`）。按单一字符串反序列化会让整份目录解析失败。
    #[serde(default, deserialize_with = "de_flexible_string")]
    pub thinking_level: Option<String>,
    #[serde(default)]
    pub requires_lead_in_generation: Option<bool>,
    #[serde(default)]
    pub requires_no_xml_tool_examples: Option<bool>,
    #[serde(default)]
    pub requires_image_output_outside_function_responses: Option<bool>,
    #[serde(default)]
    pub supports_cumulative_context: Option<bool>,
    #[serde(default)]
    pub supports_estimate_token_counter: Option<bool>,
    #[serde(default)]
    pub add_cursor_to_find_replace_target: Option<bool>,
    #[serde(default)]
    pub tab_jump_print_line_range: Option<bool>,
    /// 官方明确下发的禁用标识 (如 disabled: true)
    #[serde(default)]
    pub disabled: Option<bool>,
}

impl OfficialModelInfo {
    /// 判定模型是否被官方明确标记为禁用
    pub fn is_disabled(&self) -> bool {
        self.disabled == Some(true)
    }

    /// 判定是否为 Claude 模型家族
    pub fn is_claude(&self) -> bool {
        self.model_provider.as_deref() == Some("MODEL_PROVIDER_ANTHROPIC")
            || self.api_provider.as_deref() == Some("API_PROVIDER_ANTHROPIC_VERTEX")
            || self.model == "MODEL_PLACEHOLDER_M35"
            || self.model == "MODEL_PLACEHOLDER_M26"
    }

    /// 判定是否为非 Gemini 外部模型 (如 Claude 或 GPT 等)
    pub fn is_non_gemini(&self) -> bool {
        if let Some(ref provider) = self.model_provider {
            provider != "MODEL_PROVIDER_GOOGLE"
        } else {
            self.is_claude()
        }
    }
}

/// 官方配额信息结构
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfficialQuotaInfo {
    pub remaining_fraction: Option<f64>,
    pub reset_time: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfficialModelsFile {
    pub models: HashMap<String, OfficialModelInfo>,
}

const LIVE_CACHE_FILE_NAME: &str = "official_models_live_cache.json";
const LIVE_CACHE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct LiveCatalogCacheFile {
    #[serde(default)]
    schema_version: u32,
    #[serde(default)]
    updated_at: i64,
    #[serde(default)]
    accounts: HashMap<String, Vec<String>>,
    #[serde(default)]
    models: HashMap<String, OfficialModelInfo>,
}

#[derive(Debug, Clone, Default)]
struct LiveCatalogState {
    accounts: HashMap<String, std::collections::HashSet<String>>,
    live_models: HashMap<String, OfficialModelInfo>,
}

static INITIAL_DISK_CACHE: std::sync::OnceLock<Option<LiveCatalogCacheFile>> =
    std::sync::OnceLock::new();

fn initial_disk_cache() -> &'static Option<LiveCatalogCacheFile> {
    INITIAL_DISK_CACHE.get_or_init(|| {
        OfficialModelCatalog::resolve_live_cache_path()
            .and_then(|path| OfficialModelCatalog::load_live_cache_from_path(&path))
    })
}

static DYNAMIC_CATALOG: Lazy<RwLock<HashMap<String, OfficialModelInfo>>> = Lazy::new(|| {
    let json_str = include_str!("../../resources/official_models.json");
    let mut catalog = match serde_json::from_str::<OfficialModelsFile>(json_str) {
        Ok(file) => file.models,
        Err(err) => {
            // 目录解析失败不能 panic：生成请求在选完账号后才会第一次碰到这里，
            // 任务一旦崩掉，连接会在写出响应前被掐断，监控中间件也来不及落日志。
            tracing::error!("official_models.json 解析失败，生成请求将回退到默认模型结构: {err}");
            HashMap::new()
        }
    };
    if let Some(cache) = initial_disk_cache() {
        for (id, info) in &cache.models {
            catalog.insert(id.clone(), info.clone());
        }
    }
    RwLock::new(catalog)
});

/// 接受字符串、数字或 null。数字会被保留成十进制文本，避免官方整数档位把整份目录打崩。
fn de_flexible_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    struct Visitor;

    impl<'de> serde::de::Visitor<'de> for Visitor {
        type Value = Option<String>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a string, number, or null")
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_none<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
            Ok(Some(value.to_string()))
        }

        fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
            Ok(Some(value))
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
            Ok(Some(value.to_string()))
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
            Ok(Some(value.to_string()))
        }

        fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E> {
            Ok(Some(value.to_string()))
        }

        fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
            Ok(Some(value.to_string()))
        }
    }

    deserializer.deserialize_any(Visitor)
}

/// 官方已知的分档后缀标识符
pub const KNOWN_TIER_SUFFIXES: &[&str] = &[
    "tiered",
    "max",
    "xhigh",
    "high",
    "medium",
    "default",
    "low",
    "extra-low",
    "lite",
];

/// 官方模型目录管理器（支持动态解析与运行时增量更新）
pub struct OfficialModelCatalog;

impl OfficialModelCatalog {
    /// 从官方模型目录中动态收集属于指定 base 模型的所有可用档位后缀（如 ["tiered", "high", "medium", "low"]）
    pub fn collect_tiers_for_base(base: &str) -> Vec<String> {
        let base_lower = base.trim().to_lowercase();
        let mut found = Vec::new();
        let mut seen = std::collections::HashSet::new();

        // 不完整的大版本基准（如 claude-opus-5、claude-sonnet-5、claude-3-opus）缺少次版本，不是完整档位基准
        if base_lower.starts_with("claude-") {
            let parts: Vec<&str> = base_lower.split('-').collect();
            if (parts.len() == 3 && parts[2].chars().all(|c| c.is_ascii_digit()))
                || (parts.len() == 3 && parts[1].chars().all(|c| c.is_ascii_digit()))
            {
                return found;
            }
        }

        // 1. 探测已知标准档位标识符（支持别名解析与映射匹配）
        for tier in KNOWN_TIER_SUFFIXES {
            let candidate = format!("{}-{}", base_lower, tier);
            if Self::get(&candidate).is_some() {
                if seen.insert(tier.to_string()) {
                    found.push(tier.to_string());
                }
            }
        }

        // 2. 动态扫描目录中任意以 `{base}-` 开头的新型档位
        if let Ok(lock) = DYNAMIC_CATALOG.read() {
            let prefix = format!("{}-", base_lower);
            for key in lock.keys() {
                let k_lower = key.to_lowercase();
                if let Some(suffix) = k_lower.strip_prefix(&prefix) {
                    let s = suffix.trim();
                    if !s.is_empty()
                        && !s.contains('@')
                        && !s.contains('/')
                        && !s.contains(':')
                        && !s.chars().any(|c| c.is_ascii_digit())
                        && s.chars().all(|c| c.is_ascii_alphabetic() || c == '-')
                        && !s.starts_with('-')
                        && !s.ends_with('-')
                        && s != "thinking"
                        && s != "image"
                        && s != "preview"
                        && s != "exp"
                    {
                        if seen.insert(s.to_string()) {
                            found.push(s.to_string());
                        }
                    }
                }
            }
        }

        found
    }

    /// 获取运行时在线推荐模型缓存 (account_key -> recommended model IDs + 模型规格叠加)
    fn live_recommended_store() -> &'static RwLock<Option<LiveCatalogState>> {
        static STORE: Lazy<RwLock<Option<LiveCatalogState>>> = Lazy::new(|| {
            if let Some(cache) = initial_disk_cache() {
                if !cache.accounts.is_empty() {
                    let accounts: HashMap<String, std::collections::HashSet<String>> = cache
                        .accounts
                        .iter()
                        .map(|(k, ids)| (k.clone(), ids.iter().cloned().collect()))
                        .collect();
                    return RwLock::new(Some(LiveCatalogState {
                        accounts,
                        live_models: cache.models.clone(),
                    }));
                }
            }
            RwLock::new(None)
        });
        &STORE
    }

    fn resolve_live_cache_path() -> Option<std::path::PathBuf> {
        crate::modules::account::get_data_dir()
            .ok()
            .map(|dir| dir.join(LIVE_CACHE_FILE_NAME))
    }

    fn compact_live_model_info(info: &OfficialModelInfo) -> OfficialModelInfo {
        let mut compacted = info.clone();
        // quota_info 属于单账号私有配额，model_experiments 体积大且目录不用，落盘前剔除
        compacted.quota_info = None;
        compacted.model_experiments = None;
        compacted
    }

    pub(crate) fn load_live_cache_from_path(
        path: &std::path::Path,
    ) -> Option<LiveCatalogCacheFile> {
        let raw = std::fs::read_to_string(path).ok()?;
        let parsed: LiveCatalogCacheFile = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(err) => {
                tracing::warn!(
                    "[OfficialCatalog] Failed to parse live cache {:?}, ignoring: {}",
                    path,
                    err
                );
                return None;
            }
        };
        if parsed.schema_version != LIVE_CACHE_SCHEMA_VERSION || parsed.accounts.is_empty() {
            return None;
        }
        Some(parsed)
    }

    pub(crate) fn save_live_cache_to_path(
        path: &std::path::Path,
        cache: Option<&LiveCatalogCacheFile>,
    ) {
        let Some(cache) = cache.filter(|c| !c.accounts.is_empty()) else {
            if path.exists() {
                if let Err(err) = std::fs::remove_file(path) {
                    tracing::warn!(
                        "[OfficialCatalog] Failed to remove empty live cache {:?}: {}",
                        path,
                        err
                    );
                }
            }
            return;
        };

        match serde_json::to_string_pretty(cache) {
            Ok(content) => {
                if let Err(err) = crate::utils::fs::write_atomic(path, content.as_bytes()) {
                    tracing::warn!(
                        "[OfficialCatalog] Failed to persist live cache {:?}: {}",
                        path,
                        err
                    );
                }
            }
            Err(err) => {
                tracing::warn!("[OfficialCatalog] Failed to serialize live cache: {}", err);
            }
        }
    }

    fn persist_live_cache(cache: Option<&LiveCatalogCacheFile>) {
        if let Some(path) = Self::resolve_live_cache_path() {
            Self::save_live_cache_to_path(&path, cache);
        }
    }

    fn build_cache_snapshot(state: &LiveCatalogState) -> LiveCatalogCacheFile {
        let accounts = state
            .accounts
            .iter()
            .map(|(k, ids)| {
                let mut sorted: Vec<String> = ids.iter().cloned().collect();
                sorted.sort();
                (k.clone(), sorted)
            })
            .collect();
        LiveCatalogCacheFile {
            schema_version: LIVE_CACHE_SCHEMA_VERSION,
            updated_at: chrono::Utc::now().timestamp(),
            accounts,
            models: state.live_models.clone(),
        }
    }

    /// 计算单个在线 fetchAvailableModels 响应下的权威推荐模型集合
    pub(crate) fn extract_live_recommended_ids(
        models: &HashMap<String, OfficialModelInfo>,
        official_supported_ids: &std::collections::HashSet<String>,
    ) -> std::collections::HashSet<String> {
        let mut recommended = std::collections::HashSet::new();
        for (id, info) in models {
            if info.is_internal == Some(true) || info.is_disabled() {
                continue;
            }
            let lower = id.trim().to_lowercase();
            let is_recommended = if official_supported_ids.is_empty() {
                info.recommended == Some(true) || lower == "gemini-3.1-flash-image"
            } else {
                official_supported_ids.contains(&lower)
            };
            if is_recommended {
                recommended.insert(id.trim().to_string());
            }
        }
        recommended
    }

    /// 计算暴露给端点的官方推荐模型列表（纯函数版本，便于并发安全的单元测试）
    pub(crate) fn resolve_recommended_model_ids(
        live_accounts: Option<&HashMap<String, std::collections::HashSet<String>>>,
        catalog: &HashMap<String, OfficialModelInfo>,
    ) -> Vec<String> {
        let is_active_info =
            |info: &OfficialModelInfo| info.is_internal != Some(true) && !info.is_disabled();

        let mut result = std::collections::BTreeSet::new();
        if let Some(accounts) = live_accounts.filter(|m| !m.is_empty()) {
            for ids in accounts.values() {
                for id in ids {
                    let allow = catalog.get(id.as_str()).map(is_active_info).unwrap_or(true);
                    if allow {
                        result.insert(id.clone());
                    }
                }
            }
            return result.into_iter().collect();
        }

        for (id, info) in catalog {
            if !is_active_info(info) {
                continue;
            }
            if info.recommended == Some(true) || id.eq_ignore_ascii_case("gemini-3.1-flash-image") {
                result.insert(id.clone());
            }
        }
        result.into_iter().collect()
    }

    /// 运行时动态更新官方模型目录 (由 fetchAvailableModels 接口返回数据触发)
    pub fn update(models: HashMap<String, OfficialModelInfo>) {
        if let Ok(mut lock) = DYNAMIC_CATALOG.write() {
            for (k, v) in models {
                lock.insert(k, v);
            }
        }
    }

    /// 使用在线 `fetchAvailableModels` 的官方推荐列表同步动态目录，替换冷启动静态快照的推荐集合并持久化到数据目录
    pub fn sync_live_catalog(
        account_key: Option<&str>,
        models: HashMap<String, OfficialModelInfo>,
        official_supported_ids: &std::collections::HashSet<String>,
    ) {
        let recommended = Self::extract_live_recommended_ids(&models, official_supported_ids);
        let compacted_models: Vec<(String, OfficialModelInfo)> = models
            .iter()
            .filter(|(id, info)| {
                info.is_internal != Some(true) && !info.is_disabled() && !id.trim().is_empty()
            })
            .map(|(id, info)| (id.clone(), Self::compact_live_model_info(info)))
            .collect();
        Self::update(models);

        let key = account_key
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "_global".to_string());

        let snapshot = if let Ok(mut live_lock) = Self::live_recommended_store().write() {
            let state = live_lock.get_or_insert_with(LiveCatalogState::default);
            state.accounts.insert(key, recommended);
            for (id, info) in compacted_models {
                state.live_models.insert(id, info);
            }
            Some(Self::build_cache_snapshot(state))
        } else {
            None
        };

        if let Some(cache) = snapshot {
            Self::persist_live_cache(Some(&cache));
        }
    }

    /// 判断是否已接收到在线上游 `fetchAvailableModels` 同步的权威推荐列表
    pub fn has_live_catalog() -> bool {
        Self::live_recommended_store()
            .read()
            .ok()
            .and_then(|guard| guard.as_ref().map(|s| !s.accounts.is_empty()))
            .unwrap_or(false)
    }

    /// 当账号被永久删除时清理其在线推荐模型快照（同步刷新/清理磁盘缓存）
    pub fn remove_live_account(account_key: &str) {
        let key = account_key.trim().to_lowercase();
        if key.is_empty() {
            return;
        }
        let mut updated_snapshot: Option<Option<LiveCatalogCacheFile>> = None;
        if let Ok(mut live_lock) = Self::live_recommended_store().write() {
            if let Some(state) = live_lock.as_mut() {
                if state.accounts.remove(&key).is_some() {
                    if state.accounts.is_empty() {
                        *live_lock = None;
                        updated_snapshot = Some(None);
                    } else {
                        updated_snapshot = Some(Some(Self::build_cache_snapshot(state)));
                    }
                }
            }
        }
        if let Some(cache_opt) = updated_snapshot {
            Self::persist_live_cache(cache_opt.as_ref());
        }
    }

    /// 获取官方推荐的可用模型 ID 列表（在线同步或重启恢复磁盘缓存后优先使用官方推荐联合集合；仅纯首次冷启动回退随包内嵌快照）
    pub fn recommended_model_ids() -> Vec<String> {
        let live_guard = Self::live_recommended_store().read().ok();
        let live_ref = live_guard
            .as_ref()
            .and_then(|g| g.as_ref())
            .map(|s| &s.accounts);
        if let Ok(catalog) = DYNAMIC_CATALOG.read() {
            Self::resolve_recommended_model_ids(live_ref, &catalog)
        } else {
            Vec::new()
        }
    }

    /// 获取当前官方模型目录中所有权威模型 ID 列表
    pub fn all_model_ids() -> Vec<String> {
        if let Ok(lock) = DYNAMIC_CATALOG.read() {
            lock.keys().cloned().collect()
        } else {
            Vec::new()
        }
    }

    /// 根据用户传入的模型 ID（官方标准 ID、别名、路由结果，以及大小写不敏感的精确名）
    /// 获取对应的官方模型结构体。未命中时返回 None，由调用方使用 `default_model()`。
    pub fn get(model_id: &str) -> Option<OfficialModelInfo> {
        let lock = DYNAMIC_CATALOG.read().ok()?;

        // 1. 精确匹配原始 model_id
        if let Some(info) = lock.get(model_id) {
            return Some(info.clone());
        }

        // 2. 通过项目已有别名表解析归一化 ID 匹配
        let aliased = crate::proxy::model_specs::resolve_alias(model_id);
        if let Some(info) = lock.get(&aliased) {
            return Some(info.clone());
        }

        // 2.5 通过路由映射解析匹配
        let routed = crate::proxy::common::model_mapping::map_claude_model_to_gemini(model_id);
        if let Some(info) = lock.get(&routed) {
            return Some(info.clone());
        }

        // 3. 忽略大小写的精确匹配
        let lower = model_id.to_lowercase();
        for (k, v) in lock.iter() {
            if k.to_lowercase() == lower {
                return Some(v.clone());
            }
        }

        // 4. 若传入的是无后缀派生裸模型（如 claude-opus-5-5, gemini-3.8-flash, gemini-4-argon, gpt-oss-120b），
        // 自动匹配其标准分档变体以继承官方权威的 maxTokens (1048576 / 1000000) 与 maxOutputTokens (65536 / 128000)
        for suffix in ["tiered", "xhigh", "medium", "high", "low", "thinking"] {
            let candidate = format!("{}-{}", lower, suffix);
            if let Some(info) = lock.get(&candidate) {
                return Some(info.clone());
            }
        }

        None
    }

    /// 获取默认官方模型结构体 (gemini-3.8-flash-high)
    pub fn default_model() -> OfficialModelInfo {
        Self::get("gemini-3.8-flash-high").unwrap_or_else(|| OfficialModelInfo {
            display_name: Some("Gemini 3.8 Flash (High)".to_string()),
            model: "MODEL_PLACEHOLDER_M318".to_string(),
            model_provider: Some("MODEL_PROVIDER_GOOGLE".to_string()),
            api_provider: Some("API_PROVIDER_GOOGLE_GEMINI".to_string()),
            max_output_tokens: Some(65536),
            max_tokens: Some(1048576),
            supports_thinking: Some(true),
            thinking_budget: Some(-1),
            min_thinking_budget: Some(32),
            supports_images: Some(true),
            supports_video: Some(true),
            recommended: Some(true),
            is_internal: None,
            tag_title: Some("Fast".to_string()),
            tag_description: Some("Default".to_string()),
            quota_info: None,
            supported_mime_types: None,
            model_experiments: None,
            vertex_model_id: None,
            prompt_templater_type: None,
            tool_formatter_type: None,
            thinking_level: None,
            requires_lead_in_generation: None,
            requires_no_xml_tool_examples: None,
            requires_image_output_outside_function_responses: None,
            supports_cumulative_context: None,
            supports_estimate_token_counter: None,
            add_cursor_to_find_replace_target: None,
            tab_jump_print_line_range: None,
            disabled: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_catalog_accepts_numeric_thinking_level() {
        let flash = OfficialModelCatalog::get("gemini-2.5-flash")
            .expect("gemini-2.5-flash must load; numeric thinkingLevel used to panic the catalog");
        assert_eq!(flash.thinking_level.as_deref(), Some("1"));

        let high = OfficialModelCatalog::get("gemini-3.8-flash-high")
            .expect("gemini-3.8-flash-high must load from the embedded catalog");
        assert_eq!(high.model, "MODEL_PLACEHOLDER_M318");
        assert_eq!(high.thinking_level.as_deref(), Some("3"));
    }

    #[test]
    fn flexible_string_accepts_string_number_and_null() {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Sample {
            #[serde(default, deserialize_with = "de_flexible_string")]
            thinking_level: Option<String>,
        }

        let as_string: Sample =
            serde_json::from_str(r#"{"thinkingLevel":"HIGH"}"#).expect("string");
        assert_eq!(as_string.thinking_level.as_deref(), Some("HIGH"));

        let as_number: Sample = serde_json::from_str(r#"{"thinkingLevel":3}"#).expect("number");
        assert_eq!(as_number.thinking_level.as_deref(), Some("3"));

        let as_null: Sample = serde_json::from_str(r#"{"thinkingLevel":null}"#).expect("null");
        assert!(as_null.thinking_level.is_none());

        let missing: Sample = serde_json::from_str("{}").expect("missing");
        assert!(missing.thinking_level.is_none());
    }

    #[test]
    fn collect_tiers_for_base_rejects_digit_suffixes() {
        let tiers_for_incomplete_base =
            OfficialModelCatalog::collect_tiers_for_base("claude-opus-5");
        // "claude-opus-5" 绝不能将 "claude-opus-5-5-medium" 截取出的 "5-medium" 当成合法档位！
        assert!(
            !tiers_for_incomplete_base.contains(&"5-medium".to_string()),
            "Incomplete base claude-opus-5 must not extract 5-medium as a tier: {:?}",
            tiers_for_incomplete_base
        );
        assert!(
            tiers_for_incomplete_base.is_empty(),
            "claude-opus-5 should have no tiers, got: {:?}",
            tiers_for_incomplete_base
        );

        let tiers_for_full_base = OfficialModelCatalog::collect_tiers_for_base("claude-opus-5-5");
        assert!(
            tiers_for_full_base.contains(&"medium".to_string()),
            "claude-opus-5-5 must contain medium tier"
        );
        assert!(
            tiers_for_full_base.contains(&"low".to_string()),
            "claude-opus-5-5 must contain low tier"
        );
        assert!(
            tiers_for_full_base.contains(&"high".to_string()),
            "claude-opus-5-5 must contain high tier"
        );
    }

    #[test]
    fn recommended_model_ids_cold_start_excludes_retired_and_includes_hydrogen() {
        let catalog = DYNAMIC_CATALOG.read().unwrap();
        let recommended = OfficialModelCatalog::resolve_recommended_model_ids(None, &catalog);

        assert_eq!(
            recommended.len(),
            20,
            "cold-start recommended baseline must match exact 20 active official entries, got: {:?}",
            recommended
        );

        for expected in [
            "gemini-4-hydrogen-high",
            "gemini-4-hydrogen-xhigh",
            "gemini-4-argon-high",
            "gemini-3.8-flash-high",
            "gemini-3.8-flash-tiered",
            "gemini-3.5-flash-lite",
            "gemini-3.1-flash-image",
            "claude-opus-5-5-high",
            "claude-sonnet-5-5-high",
            "gpt-oss-120b-medium",
        ] {
            assert!(
                recommended.iter().any(|id| id == expected),
                "cold-start recommended catalog must include {expected}"
            );
        }

        for retired in [
            "gemini-3.6-flash-low",
            "gemini-3.6-flash-medium",
            "gemini-3.6-flash-high",
            "gemini-3.6-flash-tiered",
            "gemini-3.7-flash-high",
            "gemini-2.5-flash",
            "gemini-2.5-pro",
            "gemini-3.5-flash-low",
            "gemini-3.5-flash-extra-low",
            "gemini-3-flash",
            "gemini-3-flash-agent",
            "gemini-pro-agent",
            "tab_flash_lite_preview",
            "tab_jump_flash_lite_preview",
            "chat_20706",
            "chat_23310",
            "gemini-3.1-flash-lite",
            "claude-sonnet-4-6",
            "claude-opus-4-6-thinking",
            "gemini-3.1-pro-high",
            "gemini-4-carbon-high",
        ] {
            assert!(
                !recommended.iter().any(|id| id == retired),
                "cold-start recommended catalog must exclude retired/non-recommended model {retired}"
            );
        }

        // Historical spec lookup via get() must remain intact for compatibility
        assert!(OfficialModelCatalog::get("gemini-2.5-flash").is_some());
        assert!(OfficialModelCatalog::get("tab_flash_lite_preview").is_some());
        assert!(OfficialModelCatalog::get("claude-sonnet-4-6").is_some());
        assert!(OfficialModelCatalog::get("gemini-3.1-pro-high").is_some());
    }

    #[test]
    fn live_recommended_prunes_stale_snapshot_and_keeps_account_union() {
        let mut embedded = HashMap::new();
        let mut stale_36 = OfficialModelCatalog::default_model();
        stale_36.recommended = Some(true);
        embedded.insert("gemini-3.6-flash-medium".to_string(), stale_36);
        let mut hy = OfficialModelCatalog::default_model();
        hy.recommended = Some(true);
        embedded.insert("gemini-4-hydrogen-high".to_string(), hy.clone());
        let mut flash38 = OfficialModelCatalog::default_model();
        flash38.recommended = Some(true);
        embedded.insert("gemini-3.8-flash-high".to_string(), flash38.clone());
        let mut internal = OfficialModelCatalog::default_model();
        internal.recommended = Some(true);
        internal.is_internal = Some(true);
        embedded.insert("chat_999".to_string(), internal.clone());

        let mut live_acc_a_models = HashMap::new();
        live_acc_a_models.insert("gemini-4-hydrogen-high".to_string(), hy);
        live_acc_a_models.insert("gemini-3.8-flash-high".to_string(), flash38.clone());
        live_acc_a_models.insert("chat_999".to_string(), internal);
        let acc_a_supported: std::collections::HashSet<String> = [
            "gemini-4-hydrogen-high".to_string(),
            "gemini-3.8-flash-high".to_string(),
            "chat_999".to_string(),
        ]
        .into_iter()
        .collect();

        let mut live_acc_b_models = HashMap::new();
        live_acc_b_models.insert("gemini-3.8-flash-high".to_string(), flash38);
        let acc_b_supported: std::collections::HashSet<String> =
            ["gemini-3.8-flash-high".to_string()].into_iter().collect();

        let mut live_map = HashMap::new();
        live_map.insert(
            "acc_a@example.com".to_string(),
            OfficialModelCatalog::extract_live_recommended_ids(
                &live_acc_a_models,
                &acc_a_supported,
            ),
        );
        live_map.insert(
            "acc_b@example.com".to_string(),
            OfficialModelCatalog::extract_live_recommended_ids(
                &live_acc_b_models,
                &acc_b_supported,
            ),
        );

        let recommended =
            OfficialModelCatalog::resolve_recommended_model_ids(Some(&live_map), &embedded);
        assert!(recommended.contains(&"gemini-4-hydrogen-high".to_string()));
        assert!(recommended.contains(&"gemini-3.8-flash-high".to_string()));
        assert!(
            !recommended.contains(&"gemini-3.6-flash-medium".to_string()),
            "live upstream sync must replace stale static snapshot entries"
        );
        assert!(
            !recommended.contains(&"chat_999".to_string()),
            "internal models must never enter recommended list even if listed in sorts"
        );
    }

    #[test]
    fn live_catalog_cache_persists_and_cleans_up_on_disk() {
        let temp_dir =
            std::env::temp_dir().join(format!("abv_live_catalog_cache_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let cache_path = temp_dir.join(LIVE_CACHE_FILE_NAME);

        let mut info = OfficialModelCatalog::default_model();
        info.quota_info = Some(OfficialQuotaInfo {
            remaining_fraction: Some(0.5),
            reset_time: Some("2026-11-14T00:00:00Z".to_string()),
        });
        info.model_experiments = Some(serde_json::json!({"heavy": true}));

        let mut state = LiveCatalogState::default();
        state.accounts.insert(
            "vanskou@outlook.com".to_string(),
            [
                "gemini-4-hydrogen-high".to_string(),
                "gemini-3.5-flash-lite".to_string(),
            ]
            .into_iter()
            .collect(),
        );
        state.live_models.insert(
            "gemini-4-hydrogen-high".to_string(),
            OfficialModelCatalog::compact_live_model_info(&info),
        );

        let snapshot = OfficialModelCatalog::build_cache_snapshot(&state);
        OfficialModelCatalog::save_live_cache_to_path(&cache_path, Some(&snapshot));
        assert!(cache_path.exists());

        let reloaded = OfficialModelCatalog::load_live_cache_from_path(&cache_path)
            .expect("valid persisted live cache must reload cleanly");
        assert_eq!(
            reloaded
                .accounts
                .get("vanskou@outlook.com")
                .cloned()
                .unwrap(),
            vec![
                "gemini-3.5-flash-lite".to_string(),
                "gemini-4-hydrogen-high".to_string()
            ]
        );
        let reloaded_hy = reloaded.models.get("gemini-4-hydrogen-high").unwrap();
        assert!(reloaded_hy.quota_info.is_none());
        assert!(reloaded_hy.model_experiments.is_none());

        // Corrupted or stale-schema cache must gracefully return None
        std::fs::write(
            &cache_path,
            r#"{"schemaVersion":999,"accounts":{"a":["b"]}}"#,
        )
        .unwrap();
        assert!(OfficialModelCatalog::load_live_cache_from_path(&cache_path).is_none());

        // Clearing accounts removes disk cache file
        OfficialModelCatalog::save_live_cache_to_path(&cache_path, None);
        assert!(!cache_path.exists());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
