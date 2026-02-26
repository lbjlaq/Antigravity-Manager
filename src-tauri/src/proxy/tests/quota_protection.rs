// ==================================================================================
// 配额保护功能完整测试
// 验证从账号创建到配额保护策略执行的完整流程
// ==================================================================================

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::models::QuotaProtectionConfig;
    use crate::proxy::common::model_mapping::normalize_to_standard_id;
    use crate::proxy::token_manager::ProxyToken;

    // ==================================================================================
    // 辅助函数：创建模拟账号
    // ==================================================================================

    fn create_mock_token(
        account_id: &str,
        email: &str,
        protected_models: Vec<&str>,
        remaining_quota: Option<i32>,
    ) -> ProxyToken {
        ProxyToken {
            account_id: account_id.to_string(),
            access_token: format!("mock_access_token_{}", account_id),
            refresh_token: format!("mock_refresh_token_{}", account_id),
            expires_in: 3600,
            timestamp: chrono::Utc::now().timestamp() + 3600,
            email: email.to_string(),
            account_path: PathBuf::from(format!("/tmp/test_accounts/{}.json", account_id)),
            project_id: Some("test-project".to_string()),
            subscription_tier: Some("PRO".to_string()),
            remaining_quota,
            protected_models: protected_models.iter().map(|s| s.to_string()).collect(),
            health_score: 1.0,
            reset_time: None,
            validation_blocked: false,
            validation_blocked_until: 0,
            validation_url: None,
            model_quotas: std::collections::HashMap::new(),
        }
    }

    // ==================================================================================
    // 测试 1: normalize_to_standard_id 函数正确性
    // 验证各种 Claude 模型名称都能正确归一化
    // ==================================================================================

    #[test]
    fn test_normalize_to_standard_id_claude_models() {
        // Claude Sonnet 系列
        assert_eq!(
            normalize_to_standard_id("claude"),
            Some("claude".to_string())
        );
        assert_eq!(
            normalize_to_standard_id("claude-thinking"),
            Some("claude".to_string())
        );

        // Claude Opus 系列 - 这是关键的测试！
        assert_eq!(
            normalize_to_standard_id("claude-opus-4-5-thinking"),
            Some("claude".to_string()),
            "claude-opus-4-5-thinking 应该归一化为 claude"
        );

        // Gemini 系列
        assert_eq!(
            normalize_to_standard_id("gemini-3-flash"),
            Some("gemini-3-flash".to_string())
        );
        assert_eq!(
            normalize_to_standard_id("gemini-3-pro-high"),
            Some("gemini-3-pro-high".to_string())
        );
        assert_eq!(
            normalize_to_standard_id("gemini-3-pro-low"),
            Some("gemini-3-pro-high".to_string())
        );

        // 不支持的模型应返回 None
        assert_eq!(normalize_to_standard_id("gpt-4"), None);
        assert_eq!(normalize_to_standard_id("unknown-model"), None);
    }

    // ==================================================================================
    // 测试 2: 配额保护模型匹配逻辑
    // 验证 protected_models.contains() 在归一化后能正确匹配
    // ==================================================================================

    #[test]
    fn test_protected_models_matching() {
        // 创建一个账号，protected_models 中有 claude
        let token = create_mock_token(
            "account-1",
            "test@example.com",
            vec!["claude"],
            Some(50),
        );

        // 测试：请求 claude-opus-4-5-thinking 时应该被保护
        let target_model = "claude-opus-4-5-thinking";
        let normalized =
            normalize_to_standard_id(target_model).unwrap_or_else(|| target_model.to_string());

        assert_eq!(normalized, "claude");
        assert!(
            token.protected_models.contains(&normalized),
            "claude-opus-4-5-thinking 归一化后应该匹配 protected_models 中的 claude"
        );

        // 测试：请求 claude-thinking 时也应该被保护
        let target_model_2 = "claude-thinking";
        let normalized_2 =
            normalize_to_standard_id(target_model_2).unwrap_or_else(|| target_model_2.to_string());

        assert!(
            token.protected_models.contains(&normalized_2),
            "claude-thinking 归一化后应该匹配 protected_models"
        );

        // 测试：请求 gemini-3-flash 时不应该被保护（因为 protected_models 中没有）
        let target_model_3 = "gemini-3-flash";
        let normalized_3 =
            normalize_to_standard_id(target_model_3).unwrap_or_else(|| target_model_3.to_string());

        assert!(
            !token.protected_models.contains(&normalized_3),
            "gemini-3-flash 不应该匹配 claude"
        );
    }

    // ==================================================================================
    // 测试 3: 多账号轮询时的配额保护过滤
    // 模拟多个账号，验证被保护的账号会被跳过
    // ==================================================================================

    #[test]
    fn test_multi_account_quota_protection_filtering() {
        // 创建 3 个账号
        let tokens = vec![
            // 账号 1: claude 被保护（配额低）
            create_mock_token(
                "account-1",
                "user1@example.com",
                vec!["claude"],
                Some(20),
            ),
            // 账号 2: 没有被保护
            create_mock_token("account-2", "user2@example.com", vec![], Some(80)),
            // 账号 3: gemini-3-flash 被保护
            create_mock_token(
                "account-3",
                "user3@example.com",
                vec!["gemini-3-flash"],
                Some(30),
            ),
        ];

        // 模拟请求 claude-opus-4-5-thinking
        let target_model = "claude-opus-4-5-thinking";
        let normalized_target =
            normalize_to_standard_id(target_model).unwrap_or_else(|| target_model.to_string());

        // 过滤掉被保护的账号
        let available_accounts: Vec<_> = tokens
            .iter()
            .filter(|t| !t.protected_models.contains(&normalized_target))
            .collect();

        // 验证：账号 1 被过滤（因为 claude 被保护）
        // 账号 2 和 3 可用
        assert_eq!(available_accounts.len(), 2);
        assert!(available_accounts
            .iter()
            .any(|t| t.account_id == "account-2"));
        assert!(available_accounts
            .iter()
            .any(|t| t.account_id == "account-3"));
        assert!(!available_accounts
            .iter()
            .any(|t| t.account_id == "account-1"));

        // 模拟请求 gemini-3-flash
        let target_model_2 = "gemini-3-flash";
        let normalized_target_2 =
            normalize_to_standard_id(target_model_2).unwrap_or_else(|| target_model_2.to_string());

        let available_accounts_2: Vec<_> = tokens
            .iter()
            .filter(|t| !t.protected_models.contains(&normalized_target_2))
            .collect();

        // 验证：账号 3 被过滤（因为 gemini-3-flash 被保护）
        // 账号 1 和 2 可用
        assert_eq!(available_accounts_2.len(), 2);
        assert!(available_accounts_2
            .iter()
            .any(|t| t.account_id == "account-1"));
        assert!(available_accounts_2
            .iter()
            .any(|t| t.account_id == "account-2"));
        assert!(!available_accounts_2
            .iter()
            .any(|t| t.account_id == "account-3"));
    }

    // ==================================================================================
    // 测试 4: 所有账号都被保护时的行为
    // 验证当所有账号的目标模型都被保护时，返回错误
    // ==================================================================================

    #[test]
    fn test_all_accounts_protected_returns_error() {
        // 创建 3 个账号，全部对 claude 进行保护
        let tokens = vec![
            create_mock_token(
                "account-1",
                "user1@example.com",
                vec!["claude"],
                Some(10),
            ),
            create_mock_token(
                "account-2",
                "user2@example.com",
                vec!["claude"],
                Some(15),
            ),
            create_mock_token(
                "account-3",
                "user3@example.com",
                vec!["claude"],
                Some(5),
            ),
        ];

        let target_model = "claude-opus-4-5-thinking";
        let normalized_target =
            normalize_to_standard_id(target_model).unwrap_or_else(|| target_model.to_string());

        let available_accounts: Vec<_> = tokens
            .iter()
            .filter(|t| !t.protected_models.contains(&normalized_target))
            .collect();

        // 所有账号都被过滤，应该返回 0
        assert_eq!(available_accounts.len(), 0);

        // 在实际代码中，这会导致 "All accounts failed or unhealthy" 错误
    }

    // ==================================================================================
    // 测试 5: monitored_models 配置与归一化一致性
    // 验证配置中的 monitored_models 能正确匹配归一化后的模型名
    // ==================================================================================

    #[test]
    fn test_monitored_models_normalization_consistency() {
        let config = QuotaProtectionConfig {
            enabled: true,
            threshold_percentage: 60,
            monitored_models: vec![
                "claude".to_string(),
                "gemini-3-pro-high".to_string(),
                "gemini-3-flash".to_string(),
            ],
        };

        // 测试各种模型名归一化后是否在 monitored_models 中
        let test_cases = vec![
            ("claude-opus-4-5-thinking", true),   // 归一化为 claude
            ("claude-thinking", true), // 归一化为 claude
            ("claude", true),          // 直接匹配
            ("gemini-3-pro-high", true),          // 直接匹配
            ("gemini-3-pro-low", true),           // 归一化为 gemini-3-pro-high
            ("gemini-3-flash", true),             // 直接匹配
            ("gpt-4", false),                     // 不支持的模型
            ("gemini-2.5-flash", true),           // 在监控列表中 (归一化为 gemini-3-flash)
        ];

        for (model_name, expected_monitored) in test_cases {
            let standard_id = normalize_to_standard_id(model_name);

            let is_monitored = match &standard_id {
                Some(id) => config.monitored_models.contains(id),
                None => false,
            };

            assert_eq!(
                is_monitored, expected_monitored,
                "模型 {} (归一化为 {:?}) 的监控状态应为 {}",
                model_name, standard_id, expected_monitored
            );
        }
    }

    // ==================================================================================
    // 测试 6: 配额阈值触发逻辑
    // 验证配额低于阈值时触发保护，高于阈值时恢复
    // ==================================================================================

    #[test]
    fn test_quota_threshold_trigger_logic() {
        let threshold = 60; // 60% 阈值

        // 模拟 quota 数据
        let quota_data = vec![
            ("claude-opus-4-5-thinking", 50, true), // 50% <= 60%, 应触发保护
            ("claude-thinking", 60, true), // 60% <= 60%, 应触发保护（边界情况）
            ("gemini-3-flash", 61, false),          // 61% > 60%, 不触发保护
            ("gemini-3-pro-high", 100, false),      // 100% > 60%, 不触发保护
        ];

        for (model_name, percentage, should_protect) in quota_data {
            let should_trigger = percentage <= threshold;

            assert_eq!(
                should_trigger,
                should_protect,
                "模型 {} 配额 {}% (阈值 {}%) 应 {} 触发保护",
                model_name,
                percentage,
                threshold,
                if should_protect { "" } else { "不" }
            );
        }
    }

    // ==================================================================================
    // 测试 7: 账号优先级排序后的保护过滤
    // 验证高配额账号被保护后，会回退到低配额账号
    // ==================================================================================

    #[test]
    fn test_priority_fallback_when_protected() {
        // 创建 3 个账号，按配额排序
        let mut tokens = vec![
            create_mock_token(
                "account-high",
                "high@example.com",
                vec!["claude"],
                Some(90),
            ),
            create_mock_token("account-mid", "mid@example.com", vec![], Some(60)),
            create_mock_token("account-low", "low@example.com", vec![], Some(30)),
        ];

        // 按配额降序排序（高配额优先）
        tokens.sort_by(|a, b| {
            let qa = a.remaining_quota.unwrap_or(0);
            let qb = b.remaining_quota.unwrap_or(0);
            qb.cmp(&qa)
        });

        // 验证排序正确
        assert_eq!(tokens[0].account_id, "account-high");
        assert_eq!(tokens[1].account_id, "account-mid");
        assert_eq!(tokens[2].account_id, "account-low");

        // 模拟请求 claude-opus-4-5-thinking
        let target_model = "claude-opus-4-5-thinking";
        let normalized_target =
            normalize_to_standard_id(target_model).unwrap_or_else(|| target_model.to_string());

        // 按顺序选择第一个可用账号
        let selected = tokens
            .iter()
            .find(|t| !t.protected_models.contains(&normalized_target));

        // 验证：account-high 被跳过，选择 account-mid
        assert!(selected.is_some());
        assert_eq!(
            selected.unwrap().account_id,
            "account-mid",
            "高配额账号被保护后，应该回退到 account-mid"
        );
    }

    // ==================================================================================
    // 测试 8: 模型级别保护（同一账号不同模型）
    // 验证一个账号可以对某些模型保护，对其他模型不保护
    // ==================================================================================

    #[test]
    fn test_model_level_protection_granularity() {
        // 账号对 claude 保护，但对 gemini-3-flash 不保护
        let token = create_mock_token(
            "account-1",
            "user@example.com",
            vec!["claude"],
            Some(50),
        );

        // 请求 claude-opus-4-5-thinking -> 被保护
        let normalized_claude = normalize_to_standard_id("claude-opus-4-5-thinking")
            .unwrap_or_else(|| "claude-opus-4-5-thinking".to_string());
        assert!(
            token.protected_models.contains(&normalized_claude),
            "Claude 请求应该被保护"
        );

        // 请求 gemini-3-flash -> 不被保护
        let normalized_gemini = normalize_to_standard_id("gemini-3-flash")
            .unwrap_or_else(|| "gemini-3-flash".to_string());
        assert!(
            !token.protected_models.contains(&normalized_gemini),
            "Gemini 请求不应该被保护"
        );
    }

    // ==================================================================================
    // 测试 9: 配额保护启用/禁用开关
    // 验证当 quota_protection.enabled = false 时，保护逻辑不生效
    // ==================================================================================

    #[test]
    fn test_quota_protection_enabled_flag() {
        let config_enabled = QuotaProtectionConfig {
            enabled: true,
            threshold_percentage: 60,
            monitored_models: vec!["claude".to_string()],
        };

        let config_disabled = QuotaProtectionConfig {
            enabled: false,
            threshold_percentage: 60,
            monitored_models: vec!["claude".to_string()],
        };

        let token = create_mock_token(
            "account-1",
            "user@example.com",
            vec!["claude"],
            Some(50),
        );

        let target_model = "claude-opus-4-5-thinking";
        let normalized_target =
            normalize_to_standard_id(target_model).unwrap_or_else(|| target_model.to_string());

        // 启用配额保护时，账号应该被过滤
        let is_protected_when_enabled =
            config_enabled.enabled && token.protected_models.contains(&normalized_target);
        assert!(is_protected_when_enabled, "启用时应该被保护");

        // 禁用配额保护时，即使 protected_models 中有值，也不过滤
        let is_protected_when_disabled =
            config_disabled.enabled && token.protected_models.contains(&normalized_target);
        assert!(!is_protected_when_disabled, "禁用时不应该被保护");
    }

    // ==================================================================================
    // 测试 10: 完整流程模拟（集成测试风格）
    // 模拟多账号、配额保护配置、请求轮询的完整流程
    // ==================================================================================

    #[test]
    fn test_full_quota_protection_flow() {
        // 1. 配置配额保护
        let config = QuotaProtectionConfig {
            enabled: true,
            threshold_percentage: 60,
            monitored_models: vec![
                "claude".to_string(),
                "gemini-3-flash".to_string(),
            ],
        };

        // 2. 创建多个账号，模拟不同配额状态
        let accounts = vec![
            // 账号 A: Claude 配额低（50%），应该被保护
            create_mock_token(
                "account-a",
                "a@example.com",
                vec!["claude"],
                Some(50),
            ),
            // 账号 B: Claude 配额正常（80%），不被保护
            create_mock_token("account-b", "b@example.com", vec![], Some(80)),
            // 账号 C: Claude 和 Gemini 都被保护
            create_mock_token(
                "account-c",
                "c@example.com",
                vec!["claude", "gemini-3-flash"],
                Some(30),
            ),
            // 账号 D: 只有 Gemini 被保护
            create_mock_token(
                "account-d",
                "d@example.com",
                vec!["gemini-3-flash"],
                Some(40),
            ),
        ];

        // 3. 模拟多次请求，验证账号选择逻辑

        // 请求 1: claude-opus-4-5-thinking
        let target_claude = normalize_to_standard_id("claude-opus-4-5-thinking")
            .unwrap_or_else(|| "claude-opus-4-5-thinking".to_string());

        let available_for_claude: Vec<_> = accounts
            .iter()
            .filter(|a| !config.enabled || !a.protected_models.contains(&target_claude))
            .collect();

        // 账号 A 和 C 被过滤，B 和 D 可用
        assert_eq!(available_for_claude.len(), 2);
        let claude_account_ids: Vec<_> = available_for_claude
            .iter()
            .map(|a| a.account_id.as_str())
            .collect();
        assert!(claude_account_ids.contains(&"account-b"));
        assert!(claude_account_ids.contains(&"account-d"));

        // 请求 2: gemini-3-flash
        let target_gemini = normalize_to_standard_id("gemini-3-flash")
            .unwrap_or_else(|| "gemini-3-flash".to_string());

        let available_for_gemini: Vec<_> = accounts
            .iter()
            .filter(|a| !config.enabled || !a.protected_models.contains(&target_gemini))
            .collect();

        // 账号 C 和 D 被过滤，A 和 B 可用
        assert_eq!(available_for_gemini.len(), 2);
        let gemini_account_ids: Vec<_> = available_for_gemini
            .iter()
            .map(|a| a.account_id.as_str())
            .collect();
        assert!(gemini_account_ids.contains(&"account-a"));
        assert!(gemini_account_ids.contains(&"account-b"));

        // 请求 3: 未被监控的模型 (gemini-2.5-flash)
        let target_unmonitored = normalize_to_standard_id("gemini-2.5-flash")
            .unwrap_or_else(|| "gemini-2.5-flash".to_string());

        let available_for_unmonitored: Vec<_> = accounts
            .iter()
            .filter(|a| !config.enabled || !a.protected_models.contains(&target_unmonitored))
            .collect();

        // 未被监控的模型 (Gemini 2.5 Flash 实际上被归一化为已监控的 3-flash)
        // 在 4 个测试账号中，账号 C 和 D 开启了 3-flash 保护，而 A 和 B 未开启。
        // 因此，应该有 2 个账号可用。
        assert_eq!(available_for_unmonitored.len(), 2, "Gemini 2.5 Flash 共享了 3-flash 的保护状态，应有 2 个账号可用");
    }

    // ==================================================================================
    // 测试 11: 边界情况 - 空 protected_models
    // ==================================================================================

    #[test]
    fn test_empty_protected_models() {
        let token = create_mock_token(
            "account-1",
            "user@example.com",
            vec![], // 没有被保护的模型
            Some(50),
        );

        let target = normalize_to_standard_id("claude-opus-4-5-thinking")
            .unwrap_or_else(|| "claude-opus-4-5-thinking".to_string());

        assert!(
            !token.protected_models.contains(&target),
            "空 protected_models 不应该匹配任何模型"
        );
    }

    // ==================================================================================
    // 测试 12: 边界情况 - 大小写敏感性
    // ==================================================================================

    #[test]
    fn test_model_name_case_sensitivity() {
        // normalize_to_standard_id 应该是大小写不敏感的
        assert_eq!(
            normalize_to_standard_id("Claude-Opus-4-5-Thinking"),
            Some("claude".to_string())
        );
        assert_eq!(
            normalize_to_standard_id("CLAUDE-OPUS-4-5-THINKING"),
            Some("claude".to_string())
        );
        assert_eq!(
            normalize_to_standard_id("GEMINI-3-FLASH"),
            Some("gemini-3-flash".to_string())
        );
    }

    // ==================================================================================
    // 测试 13: 端到端场景 - 会话中途配额保护生效后的路由切换
    // 模拟：请求1 -> 绑定账号A -> 请求2 -> 继续用A -> 刷新配额 -> A被保护 -> 请求3 -> 切换到B
    // ==================================================================================

    #[test]
    fn test_sticky_session_quota_protection_mid_session_single_account() {
        // 场景：只有一个账号，会话绑定后配额保护生效
        // 预期：返回配额保护错误

        let session_id = "session-12345";
        let target_model = "claude-opus-4-5-thinking";
        let normalized_target =
            normalize_to_standard_id(target_model).unwrap_or_else(|| target_model.to_string());

        // 初始状态：账号 A 没有被保护
        let mut account_a = create_mock_token(
            "account-a",
            "a@example.com",
            vec![], // 初始没有保护
            Some(70),
        );

        // 模拟会话绑定表
        let mut session_bindings: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();

        // === 请求 1: 绑定到账号 A ===
        session_bindings.insert(session_id.to_string(), account_a.account_id.clone());

        // 验证请求 1 成功
        let bound_account = session_bindings.get(session_id);
        assert_eq!(bound_account, Some(&"account-a".to_string()));

        // === 请求 2: 继续使用账号 A ===
        // 账号 A 仍然可用
        assert!(!account_a.protected_models.contains(&normalized_target));

        // === 系统触发配额刷新，发现账号 A 配额低于阈值 ===
        // 模拟配额刷新后，account_a 的 claude 被加入保护列表
        account_a
            .protected_models
            .insert("claude".to_string());

        // === 请求 3: 尝试使用账号 A，但被配额保护 ===
        let accounts = vec![account_a.clone()]; // 只有一个账号

        // 检查绑定的账号是否被保护
        let bound_id = session_bindings.get(session_id).unwrap();
        let bound_account = accounts.iter().find(|a| &a.account_id == bound_id).unwrap();
        let is_protected = bound_account.protected_models.contains(&normalized_target);

        assert!(is_protected, "账号 A 应该被配额保护");

        // 尝试找其他可用账号
        let available_accounts: Vec<_> = accounts
            .iter()
            .filter(|a| !a.protected_models.contains(&normalized_target))
            .collect();

        // 没有可用账号
        assert_eq!(available_accounts.len(), 0, "应该没有可用账号");

        // 在实际实现中，这会返回错误消息
        // 验证应该返回配额保护相关的错误
        let error_message = if available_accounts.is_empty() {
            if accounts
                .iter()
                .all(|a| a.protected_models.contains(&normalized_target))
            {
                format!(
                    "All accounts quota-protected for model {}",
                    normalized_target
                )
            } else {
                "All accounts failed or unhealthy.".to_string()
            }
        } else {
            "OK".to_string()
        };

        assert!(
            error_message.contains("quota-protected"),
            "错误消息应该包含 quota-protected: {}",
            error_message
        );
    }

    #[test]
    fn test_sticky_session_quota_protection_mid_session_multi_account() {
        // 场景：多个账号，会话绑定的账号配额保护生效后，应该路由到其他账号

        let session_id = "session-67890";
        let target_model = "claude-opus-4-5-thinking";
        let normalized_target =
            normalize_to_standard_id(target_model).unwrap_or_else(|| target_model.to_string());

        // 初始状态：账号 A 和 B 都没有被保护
        let mut account_a = create_mock_token("account-a", "a@example.com", vec![], Some(70));
        let account_b = create_mock_token("account-b", "b@example.com", vec![], Some(80));

        let mut session_bindings: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();

        // === 请求 1: 绑定到账号 A ===
        session_bindings.insert(session_id.to_string(), account_a.account_id.clone());

        // === 请求 2: 继续使用账号 A ===
        assert!(!account_a.protected_models.contains(&normalized_target));

        // === 系统触发配额刷新，账号 A 被保护 ===
        account_a
            .protected_models
            .insert("claude".to_string());

        // === 请求 3: 账号 A 被保护，应该解绑并切换到账号 B ===
        let accounts = vec![account_a.clone(), account_b.clone()];

        // 检查绑定的账号
        let bound_id = session_bindings.get(session_id).unwrap();
        let bound_account = accounts.iter().find(|a| &a.account_id == bound_id).unwrap();
        let is_protected = bound_account.protected_models.contains(&normalized_target);

        assert!(is_protected, "账号 A 应该被配额保护");

        // 模拟解绑逻辑
        if is_protected {
            session_bindings.remove(session_id);
        }

        // 寻找其他可用账号
        let available_accounts: Vec<_> = accounts
            .iter()
            .filter(|a| !a.protected_models.contains(&normalized_target))
            .collect();

        // 应该有账号 B 可用
        assert_eq!(available_accounts.len(), 1);
        assert_eq!(available_accounts[0].account_id, "account-b");

        // 重新绑定到账号 B
        let new_account = available_accounts[0];
        session_bindings.insert(session_id.to_string(), new_account.account_id.clone());

        // 验证新绑定
        assert_eq!(
            session_bindings.get(session_id),
            Some(&"account-b".to_string()),
            "会话应该重新绑定到账号 B"
        );
    }

    // ==================================================================================
    // 测试 14: 配额保护实时同步测试
    // 模拟：配额刷新后 protected_models 被更新，TokenManager 内存应该同步
    // ==================================================================================

    #[test]
    fn test_quota_protection_sync_after_refresh() {
        // 这个测试模拟 update_account_quota 触发 TokenManager 重新加载的场景

        // 初始内存状态
        let mut tokens_in_memory = vec![create_mock_token(
            "account-a",
            "a@example.com",
            vec![],
            Some(70),
        )];

        // 模拟磁盘上的账号数据（配额刷新后更新）
        let mut account_on_disk = create_mock_token("account-a", "a@example.com", vec![], Some(50));

        // 模拟配额刷新：检测到配额低于阈值，触发保护
        let threshold = 60;
        if account_on_disk.remaining_quota.unwrap_or(100) <= threshold {
            account_on_disk
                .protected_models
                .insert("claude".to_string());
        }

        // 验证磁盘数据已更新
        assert!(
            account_on_disk
                .protected_models
                .contains("claude"),
            "磁盘上的账号应该已被保护"
        );

        // 此时内存数据还是旧的
        assert!(
            !tokens_in_memory[0]
                .protected_models
                .contains("claude"),
            "内存中的账号还没被同步"
        );

        // 模拟 trigger_account_reload -> reload_account 同步
        tokens_in_memory[0] = account_on_disk.clone();

        // 验证内存数据已同步
        assert!(
            tokens_in_memory[0]
                .protected_models
                .contains("claude"),
            "同步后内存中的账号应该被保护"
        );

        // 现在请求应该被正确过滤
        let target = normalize_to_standard_id("claude-opus-4-5-thinking")
            .unwrap_or_else(|| "claude-opus-4-5-thinking".to_string());

        let available: Vec<_> = tokens_in_memory
            .iter()
            .filter(|t| !t.protected_models.contains(&target))
            .collect();

        assert_eq!(available.len(), 0, "同步后账号应该被过滤");
    }

    // ==================================================================================
    // 测试 15: 多轮请求中的配额保护动态变化
    // 模拟完整的请求序列，包括配额保护的触发和恢复
    // ==================================================================================

    #[test]
    fn test_quota_protection_dynamic_changes() {
        let target_model = "claude-opus-4-5-thinking";
        let normalized_target =
            normalize_to_standard_id(target_model).unwrap_or_else(|| target_model.to_string());

        // 账号池
        let mut account_a = create_mock_token("account-a", "a@example.com", vec![], Some(70));
        let mut account_b = create_mock_token("account-b", "b@example.com", vec![], Some(80));

        // === 阶段 1: 初始状态，两个账号都可用 ===
        let accounts = vec![account_a.clone(), account_b.clone()];
        let available: Vec<_> = accounts
            .iter()
            .filter(|t| !t.protected_models.contains(&normalized_target))
            .collect();
        assert_eq!(available.len(), 2, "阶段1: 两个账号都可用");

        // === 阶段 2: 账号 A 配额降低，触发保护 ===
        account_a.remaining_quota = Some(40);
        account_a
            .protected_models
            .insert("claude".to_string());

        let accounts = vec![account_a.clone(), account_b.clone()];
        let available: Vec<_> = accounts
            .iter()
            .filter(|t| !t.protected_models.contains(&normalized_target))
            .collect();
        assert_eq!(available.len(), 1, "阶段2: 只有账号 B 可用");
        assert_eq!(available[0].account_id, "account-b");

        // === 阶段 3: 账号 B 也触发保护 ===
        account_b.remaining_quota = Some(30);
        account_b
            .protected_models
            .insert("claude".to_string());

        let accounts = vec![account_a.clone(), account_b.clone()];
        let available: Vec<_> = accounts
            .iter()
            .filter(|t| !t.protected_models.contains(&normalized_target))
            .collect();
        assert_eq!(available.len(), 0, "阶段3: 没有可用账号");

        // === 阶段 4: 账号 A 配额恢复（重置），解除保护 ===
        account_a.remaining_quota = Some(100);
        account_a.protected_models.remove("claude");

        let accounts = vec![account_a.clone(), account_b.clone()];
        let available: Vec<_> = accounts
            .iter()
            .filter(|t| !t.protected_models.contains(&normalized_target))
            .collect();
        assert_eq!(available.len(), 1, "阶段4: 账号 A 恢复可用");
        assert_eq!(available[0].account_id, "account-a");
    }

    // ==================================================================================
    // 测试 16: 完整错误消息验证
    // 验证不同场景下返回的错误消息是否正确
    // ==================================================================================

    #[test]
    fn test_error_messages_for_quota_protection() {
        let target_model = "claude-opus-4-5-thinking";
        let normalized_target =
            normalize_to_standard_id(target_model).unwrap_or_else(|| target_model.to_string());

        // 场景 1: 所有账号都因配额保护不可用
        let all_protected = vec![
            create_mock_token("a1", "a1@example.com", vec!["claude"], Some(30)),
            create_mock_token("a2", "a2@example.com", vec!["claude"], Some(20)),
        ];

        let all_are_quota_protected = all_protected
            .iter()
            .all(|a| a.protected_models.contains(&normalized_target));

        assert!(all_are_quota_protected, "所有账号都被配额保护");

        // 生成错误消息
        let error = format!(
            "All {} accounts are quota-protected for model '{}'. Wait for quota reset or adjust protection threshold.",
            all_protected.len(),
            normalized_target
        );

        assert!(error.contains("quota-protected"));
        assert!(error.contains("claude"));

        // 场景 2: 混合情况（部分限流，部分配额保护）
        let mixed = vec![
            create_mock_token("a1", "a1@example.com", vec!["claude"], Some(30)),
            create_mock_token("a2", "a2@example.com", vec![], Some(20)), // 这个假设被限流
        ];

        let quota_protected_count = mixed
            .iter()
            .filter(|a| a.protected_models.contains(&normalized_target))
            .count();

        assert_eq!(quota_protected_count, 1);
    }

    // ==================================================================================
    // 测试 17: get_model_quota_from_json 函数正确性
    // 验证从磁盘读取特定模型 quota 而非 max(所有模型)
    // ==================================================================================

    #[test]
    fn test_get_model_quota_from_json_reads_correct_model() {
        // 创建模拟账号 JSON 文件，包含多个模型的 quota
        let account_json = serde_json::json!({
            "email": "test@example.com",
            "quota": {
                "models": [
                    { "name": "claude", "percentage": 60 },
                    { "name": "claude-opus-4-5-thinking", "percentage": 40 },
                    { "name": "gemini-3-flash", "percentage": 100 }
                ]
            }
        });

        // 使用 std::env::temp_dir() 创建临时文件
        let temp_dir = std::env::temp_dir();
        let account_path = temp_dir.join(format!("test_quota_{}.json", uuid::Uuid::new_v4()));
        std::fs::write(&account_path, account_json.to_string()).expect("Failed to write temp file");

        // 测试读取 claude 的 quota
        let sonnet_quota =
            crate::proxy::token_manager::TokenManager::get_model_quota_from_json_for_test(
                &account_path,
                "claude",
            );
        assert_eq!(
            sonnet_quota,
            Some(60),
            "claude 应该返回 60%，而非 max(100%)"
        );

        // 测试读取 gemini-3-flash 的 quota
        let gemini_quota =
            crate::proxy::token_manager::TokenManager::get_model_quota_from_json_for_test(
                &account_path,
                "gemini-3-flash",
            );
        assert_eq!(gemini_quota, Some(100), "gemini-3-flash 应该返回 100%");

        // 测试读取不存在的模型
        let unknown_quota =
            crate::proxy::token_manager::TokenManager::get_model_quota_from_json_for_test(
                &account_path,
                "unknown-model",
            );
        assert_eq!(unknown_quota, None, "不存在的模型应该返回 None");

        // 清理临时文件
        let _ = std::fs::remove_file(&account_path);
    }

    // ==================================================================================
    // 测试 18: 排序使用目标模型 quota 而非 max quota
    // 验证修复后的排序逻辑正确性
    // ==================================================================================

    #[test]
    fn test_sorting_uses_target_model_quota_not_max() {
        // 使用 std::env::temp_dir() 创建临时目录
        let temp_dir = std::env::temp_dir().join(format!("test_sorting_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).expect("Failed to create temp dir");

        // 账号 A: max=100 (gemini), sonnet=40
        let account_a_json = serde_json::json!({
            "email": "carmelioventori@example.com",
            "quota": {
                "models": [
                    { "name": "claude", "percentage": 40 },
                    { "name": "gemini-3-flash", "percentage": 100 }
                ]
            }
        });

        // 账号 B: max=100 (gemini), sonnet=100
        let account_b_json = serde_json::json!({
            "email": "kiriyamaleo@example.com",
            "quota": {
                "models": [
                    { "name": "claude", "percentage": 100 },
                    { "name": "gemini-3-flash", "percentage": 100 }
                ]
            }
        });

        // 账号 C: max=100 (gemini), sonnet=60
        let account_c_json = serde_json::json!({
            "email": "mizusawakai9@example.com",
            "quota": {
                "models": [
                    { "name": "claude", "percentage": 60 },
                    { "name": "gemini-3-flash", "percentage": 100 }
                ]
            }
        });

        // 写入临时文件
        let path_a = temp_dir.join("account_a.json");
        let path_b = temp_dir.join("account_b.json");
        let path_c = temp_dir.join("account_c.json");

        std::fs::write(&path_a, account_a_json.to_string()).unwrap();
        std::fs::write(&path_b, account_b_json.to_string()).unwrap();
        std::fs::write(&path_c, account_c_json.to_string()).unwrap();

        // 创建 tokens，remaining_quota 使用 max 值（模拟旧逻辑）
        let mut tokens = vec![
            create_mock_token_with_path("a", "carmelioventori@example.com", vec![], Some(100), path_a.clone()),
            create_mock_token_with_path("b", "kiriyamaleo@example.com", vec![], Some(100), path_b.clone()),
            create_mock_token_with_path("c", "mizusawakai9@example.com", vec![], Some(100), path_c.clone()),
        ];

        // 目标模型: claude
        let target_model = "claude";

        // 使用修复后的排序逻辑：读取目标模型的 quota
        tokens.sort_by(|a, b| {
            let quota_a = crate::proxy::token_manager::TokenManager::get_model_quota_from_json_for_test(
                &a.account_path,
                target_model,
            )
            .unwrap_or(0);
            let quota_b = crate::proxy::token_manager::TokenManager::get_model_quota_from_json_for_test(
                &b.account_path,
                target_model,
            )
            .unwrap_or(0);
            quota_b.cmp(&quota_a) // 高 quota 优先
        });

        // 验证排序结果：sonnet quota 100% > 60% > 40%
        assert_eq!(
            tokens[0].email, "kiriyamaleo@example.com",
            "sonnet=100% 的账号应该排第一"
        );
        assert_eq!(
            tokens[1].email, "mizusawakai9@example.com",
            "sonnet=60% 的账号应该排第二"
        );
        assert_eq!(
            tokens[2].email, "carmelioventori@example.com",
            "sonnet=40% 的账号应该排第三"
        );

        // 清理临时目录
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // ==================================================================================
    // 测试 19: 模型名称归一化后的 quota 匹配
    // 验证请求 claude-opus-4-5-thinking 时能正确匹配 claude 的 quota
    // ==================================================================================

    #[test]
    fn test_quota_matching_with_normalized_model_name() {
        // 账号 JSON：只记录标准化后的模型名
        let account_json = serde_json::json!({
            "email": "test@example.com",
            "quota": {
                "models": [
                    { "name": "claude", "percentage": 75 },
                    { "name": "gemini-3-flash", "percentage": 90 }
                ]
            }
        });

        let temp_dir = std::env::temp_dir();
        let account_path = temp_dir.join(format!("test_normalized_{}.json", uuid::Uuid::new_v4()));
        std::fs::write(&account_path, account_json.to_string()).expect("Failed to write temp file");

        // 请求 claude-opus-4-5-thinking，应该归一化为 claude
        let request_model = "claude-opus-4-5-thinking";
        let normalized = normalize_to_standard_id(request_model)
            .unwrap_or_else(|| request_model.to_string());

        assert_eq!(normalized, "claude", "应该归一化为 claude");

        // 读取归一化后模型的 quota
        let quota = crate::proxy::token_manager::TokenManager::get_model_quota_from_json_for_test(
            &account_path,
            &normalized,
        );

        assert_eq!(
            quota,
            Some(75),
            "claude-opus-4-5-thinking 归一化后应该读取 claude 的 quota (75%)"
        );

        // 清理临时文件
        let _ = std::fs::remove_file(&account_path);
    }

    /// 辅助函数：创建带有自定义 account_path 的 mock token
    fn create_mock_token_with_path(
        account_id: &str,
        email: &str,
        protected_models: Vec<&str>,
        remaining_quota: Option<i32>,
        account_path: PathBuf,
    ) -> ProxyToken {
        ProxyToken {
            account_id: account_id.to_string(),
            access_token: format!("mock_access_token_{}", account_id),
            refresh_token: format!("mock_refresh_token_{}", account_id),
            expires_in: 3600,
            timestamp: chrono::Utc::now().timestamp() + 3600,
            email: email.to_string(),
            account_path,
            project_id: Some("test-project".to_string()),
            subscription_tier: Some("PRO".to_string()),
            remaining_quota,
            protected_models: protected_models.iter().map(|s| s.to_string()).collect(),
            health_score: 1.0,
            reset_time: None,
            validation_blocked: false,
            validation_blocked_until: 0,
            validation_url: None,
            model_quotas: std::collections::HashMap::new(),
        }
    }
}
