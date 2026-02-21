//! Ultra Priority Tests for High-End Models (Opus 4.6/4.5)
//!
//! 这些测试验证高端模型（如 Claude Opus 4.6/4.5）优先使用 Ultra 账号的逻辑。
//!
//! ## 背景
//! 用户的账号池包含大量 Gemini Pro 账号和少量 Ultra 账号。当请求 Claude Opus 4.6 模型时，
//! 系统按配额优先的策略可能会选择 Pro 账号，但 Pro 账号无法访问 Opus 4.6，导致 API 返回错误。
//!
//! ## 解决方案
//! 当用户请求高端模型时，优先选择 Ultra 账号；只有 Ultra 账号都不可用时才降级到 Pro/Free 账号。
//!
//! ## 测试覆盖
//! - `test_is_ultra_required_model`: 验证模型识别逻辑
//! - `test_ultra_priority_for_high_end_models`: 验证 Ultra 优先于 Pro（即使 Pro 配额更高）
//! - `test_ultra_accounts_sorted_by_quota`: 验证同为 Ultra 时按配额排序
//! - `test_full_sorting_mixed_accounts`: 验证混合账号池的完整排序

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::proxy::token_manager::ProxyToken;

/// 创建测试用的 ProxyToken
fn create_test_token(
    email: &str,
    tier: Option<&str>,
    health_score: f32,
    reset_time: Option<i64>,
    remaining_quota: Option<i32>,
    supported_models: Vec<&str>,
) -> ProxyToken {
    let mut model_quotas = HashMap::new();
    // 模拟配额：所有支持的模型都给予相同的剩余配额
    for m in supported_models {
        model_quotas.insert(m.to_string(), remaining_quota.unwrap_or(100));
    }

    ProxyToken {
        account_id: email.to_string(),
        access_token: "test_token".to_string(),
        refresh_token: "test_refresh".to_string(),
        expires_in: 3600,
        timestamp: chrono::Utc::now().timestamp() + 3600,
        email: email.to_string(),
        account_path: PathBuf::from("/tmp/test"),
        project_id: None,
        subscription_tier: tier.map(|s| s.to_string()),
        remaining_quota,
        protected_models: HashSet::new(),
        health_score,
        reset_time,
        validation_blocked: false,
        validation_blocked_until: 0,
        model_quotas,
    }
}

/// 需要 Ultra 账号的高端模型列表
const ULTRA_REQUIRED_MODELS: &[&str] = &[
    "claude-opus-4-6",
    "claude-opus-4-5",
    "opus", // 通配匹配
];

/// 检查模型是否需要 Ultra 账号
fn is_ultra_required_model(model: &str) -> bool {
    let lower = model.to_lowercase();
    ULTRA_REQUIRED_MODELS.iter().any(|m| lower.contains(m))
}

/// 测试 is_ultra_required_model 辅助函数
#[test]
fn test_is_ultra_required_model() {
    // 应该识别为高端模型
    assert!(is_ultra_required_model("claude-opus-4-6"));
    assert!(is_ultra_required_model("claude-opus-4-5"));
    assert!(is_ultra_required_model("Claude-Opus-4-6")); // 大小写不敏感
    assert!(is_ultra_required_model("CLAUDE-OPUS-4-5")); // 大小写不敏感
    assert!(is_ultra_required_model("opus")); // 通配匹配
    assert!(is_ultra_required_model("opus-4-6-latest"));
    assert!(is_ultra_required_model("models/claude-opus-4-6"));

    // 应该识别为普通模型
    assert!(!is_ultra_required_model("claude-sonnet-4-6"));
    assert!(!is_ultra_required_model("claude-sonnet"));
    assert!(!is_ultra_required_model("gemini-1.5-flash"));
    assert!(!is_ultra_required_model("gemini-2.0-pro"));
    assert!(!is_ultra_required_model("claude-haiku"));
}

/// 模拟 token_manager.rs 中的排序逻辑 (更新后：始终 Tier 优先)
fn compare_tokens_for_model(a: &ProxyToken, b: &ProxyToken, _target_model: &str) -> Ordering {
    let tier_priority = |tier: &Option<String>| {
        let t = tier.as_deref().unwrap_or("").to_lowercase();
        if t.contains("ultra") { 0 }
        else if t.contains("pro") { 1 }
        else if t.contains("free") { 2 }
        else { 3 }
    };

    // Priority 0: 始终优先订阅等级 (Ultra > Pro > Free)
    let tier_cmp = tier_priority(&a.subscription_tier)
        .cmp(&tier_priority(&b.subscription_tier));
    if tier_cmp != Ordering::Equal {
        return tier_cmp;
    }

    // Priority 1: Quota (higher is better)
    // 注意：这里简化了，直接取 remaining_quota，实际上生产代码取的是 model_quotas.get(target)
    let quota_a = a.remaining_quota.unwrap_or(0);
    let quota_b = b.remaining_quota.unwrap_or(0);
    let quota_cmp = quota_b.cmp(&quota_a);
    if quota_cmp != Ordering::Equal {
        return quota_cmp;
    }

    // Priority 2: Health score
    let health_cmp = b.health_score.partial_cmp(&a.health_score)
        .unwrap_or(Ordering::Equal);
    if health_cmp != Ordering::Equal {
        return health_cmp;
    }

    Ordering::Equal
}

/// 模拟过滤逻辑
fn filter_tokens_by_capability(tokens: Vec<ProxyToken>, target_model: &str) -> Vec<ProxyToken> {
    tokens.into_iter()
        .filter(|t| t.model_quotas.contains_key(target_model))
        .collect()
}

/// 测试高端模型排序：Ultra 账号优先于 Pro 账号（即使 Pro 配额更高）
#[test]
fn test_ultra_priority_for_high_end_models() {
    // 创建测试账号：Ultra 低配额 vs Pro 高配额
    // Ultra 账号支持 Opus 4.6
    let ultra_low_quota = create_test_token("ultra@test.com", Some("ULTRA"), 1.0, None, Some(20), vec!["claude-opus-4-6", "claude-sonnet-4-6"]);
    // Pro 账号不支持 Opus 4.6 (假设)
    let pro_high_quota = create_test_token("pro@test.com", Some("PRO"), 1.0, None, Some(80), vec!["claude-sonnet-4-6"]);

    // 1. 验证过滤逻辑
    let tokens = vec![ultra_low_quota.clone(), pro_high_quota.clone()];
    let filtered = filter_tokens_by_capability(tokens, "claude-opus-4-6");
    assert_eq!(filtered.len(), 1, "Pro account should be filtered out for Opus 4.6");
    assert_eq!(filtered[0].email, "ultra@test.com");

    // 2. 验证排序逻辑 (针对 Sonnet，两者都支持)
    // 即使 Pro 配额更高，由于新策略是 "Ultra First"，Ultra 仍然排在前面
    assert_eq!(
        compare_tokens_for_model(&ultra_low_quota, &pro_high_quota, "claude-sonnet-4-6"),
        Ordering::Less, // Ultra 排在前面
        "Sonnet should now prefer Ultra account over Pro (Strict Tier Policy)"
    );
}

#[test]
fn test_capability_filtering() {
    // Ultra 账号：有 Opus 4.6
    let ultra = create_test_token("ultra@test.com", Some("ULTRA"), 1.0, None, Some(100), vec!["claude-opus-4-6"]);
    // Pro 账号：无 Opus 4.6
    let pro = create_test_token("pro@test.com", Some("PRO"), 1.0, None, Some(100), vec!["claude-sonnet-3-5"]);
    
    // Future Pro 账号：有 Opus 4.6 (模拟未来可能开放)
    let future_pro = create_test_token("future_pro@test.com", Some("PRO"), 1.0, None, Some(50), vec!["claude-opus-4-6"]);

    let pool = vec![ultra.clone(), pro.clone(), future_pro.clone()];

    // 1. 请求 Opus 4.6
    let filtered_opus = filter_tokens_by_capability(pool.clone(), "claude-opus-4-6");
    assert_eq!(filtered_opus.len(), 2, "Should retain Ultra and Future Pro");
    // 验证 Pro 被移除
    assert!(!filtered_opus.iter().any(|t| t.email == "pro@test.com"));

    // 2. 排序 filtered_opus: Ultra 应该排在 Future Pro 前面 (Tier Priority)
    let mut sorted_opus = filtered_opus.clone();
    sorted_opus.sort_by(|a, b| compare_tokens_for_model(a, b, "claude-opus-4-6"));
    assert_eq!(sorted_opus[0].email, "ultra@test.com", "Ultra should be prioritized over Pro even if Pro has capability");
    assert_eq!(sorted_opus[1].email, "future_pro@test.com");
}

/// 测试排序：同为 Ultra 时按配额排序
#[test]
fn test_ultra_accounts_sorted_by_quota() {
    let ultra_high = create_test_token("ultra_high@test.com", Some("ULTRA"), 1.0, None, Some(80), vec!["claude-opus-4-6"]);
    let ultra_low = create_test_token("ultra_low@test.com", Some("ULTRA"), 1.0, None, Some(20), vec!["claude-opus-4-6"]);

    // Opus 4.6: 同为 Ultra，高配额优先
    assert_eq!(
        compare_tokens_for_model(&ultra_high, &ultra_low, "claude-opus-4-6"),
        Ordering::Less, // ultra_high 排在前面
        "Among Ultra accounts, higher quota should come first"
    );
}

/// 测试完整排序场景：混合账号池
#[test]
fn test_full_sorting_mixed_accounts() {
    fn sort_tokens_for_model(tokens: &mut Vec<ProxyToken>, target_model: &str) {
        tokens.sort_by(|a, b| compare_tokens_for_model(a, b, target_model));
    }

    // 创建混合账号池 (全部支持所有模型，简化测试)
    let supported = vec!["claude-opus-4-6", "claude-sonnet-4-6"];
    let ultra_high = create_test_token("ultra_high@test.com", Some("ULTRA"), 1.0, None, Some(80), supported.clone());
    let ultra_low = create_test_token("ultra_low@test.com", Some("ULTRA"), 1.0, None, Some(20), supported.clone());
    let pro_high = create_test_token("pro_high@test.com", Some("PRO"), 1.0, None, Some(90), supported.clone());
    let pro_low = create_test_token("pro_low@test.com", Some("PRO"), 1.0, None, Some(30), supported.clone());
    let free = create_test_token("free@test.com", Some("FREE"), 1.0, None, Some(100), supported.clone());

    // 高端模型 (Opus 4.6) 排序
    let mut tokens_opus = vec![pro_high.clone(), free.clone(), ultra_low.clone(), pro_low.clone(), ultra_high.clone()];
    sort_tokens_for_model(&mut tokens_opus, "claude-opus-4-6");

    let emails_opus: Vec<&str> = tokens_opus.iter().map(|t| t.email.as_str()).collect();
    // 期望顺序: Ultra(高配额) > Ultra(低配额) > Pro(高配额) > Pro(低配额) > Free
    assert_eq!(
        emails_opus,
        vec!["ultra_high@test.com", "ultra_low@test.com", "pro_high@test.com", "pro_low@test.com", "free@test.com"],
        "Opus 4.6 should sort Ultra first, then by quota within each tier"
    );

    // 普通模型 (Sonnet) 排序
    let mut tokens_sonnet = vec![pro_high.clone(), free.clone(), ultra_low.clone(), pro_low.clone(), ultra_high.clone()];
    sort_tokens_for_model(&mut tokens_sonnet, "claude-sonnet-4-6");

    let emails_sonnet: Vec<&str> = tokens_sonnet.iter().map(|t| t.email.as_str()).collect();
    // 期望顺序: Ultra > Pro > Free (严格层级)
    // Ultra 内按 quota: high > low
    // Pro 内按 quota: high > low
    assert_eq!(
        emails_sonnet,
        vec!["ultra_high@test.com", "ultra_low@test.com", "pro_high@test.com", "pro_low@test.com", "free@test.com"],
        "Sonnet should now sort Ultra first, then Pro, then Free"
    );
}
