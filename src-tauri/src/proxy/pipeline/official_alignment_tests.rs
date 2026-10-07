// Minimal test verifying Claude and Gemini align_official_envelope behavior
#[cfg(test)]
mod tests {
    use crate::proxy::pipeline::InboundThinkingPipeline;
    use serde_json::json;

    #[test]
    fn test_official_claude_alignment() {
        let mut body = json!({
            "_session_thinking_id": "agent/a76e1573-c906-435a-89d2-18b6683313b1/1790530802878/0181971f-961c-4ba9-a320-2ae44361bc1d/1",
            "model": "claude-sonnet-4-6",
            "request": {
                "contents": [
                    { "role": "user", "parts": [{ "text": "hello" }] }
                ],
                "tools": [
                    {
                        "functionDeclarations": [
                            {
                                "name": "view_file",
                                "description": "This tool supports text files and following binary files: image, pdf, video, audio.",
                                "parameters": { "type": "object" }
                            }
                        ]
                    }
                ],
                "generationConfig": {
                    "thinkingConfig": {
                        "includeThoughts": true,
                        "thinkingBudget": 1024
                    }
                }
            }
        });

        InboundThinkingPipeline::align_official_envelope(&mut body);

        // 1. Envelope keys: project -> requestId -> request -> model -> userAgent -> requestType
        let keys: Vec<&str> = body
            .as_object()
            .unwrap()
            .keys()
            .map(|s| s.as_str())
            .collect();
        assert_eq!(
            keys,
            vec![
                "project",
                "requestId",
                "request",
                "model",
                "userAgent",
                "requestType"
            ]
        );

        let req = body.get("request").unwrap();

        // 2. Labels aligned for Claude
        let labels = req.get("labels").unwrap();
        assert_eq!(labels["model_enum"], "MODEL_PLACEHOLDER_M35");
        assert_eq!(labels["used_claude"], "true");
        assert_eq!(labels["used_claude_conservative"], "true");
        assert_eq!(labels["used_non_gemini_model"], "true");
        assert_eq!(
            labels["trajectory_id"],
            "0181971f-961c-4ba9-a320-2ae44361bc1d"
        );
        assert_eq!(
            labels["request_id"],
            "0181971f-961c-4ba9-a320-2ae44361bc1d-0"
        );

        // 3. GenerationConfig maxOutputTokens = 64000 for Claude, thinkingBudget kept untouched
        let gc = req.get("generationConfig").unwrap();
        assert_eq!(gc["maxOutputTokens"], 64000);
        assert_eq!(gc["thinkingConfig"]["thinkingBudget"], 1024);

        // 4. Tools description kept intact as provided by client (客户端传啥就是啥)
        let tools = req.get("tools").unwrap().as_array().unwrap();
        let view_file_desc = tools[0]["functionDeclarations"][0]["description"]
            .as_str()
            .unwrap();
        assert_eq!(
            view_file_desc,
            "This tool supports text files and following binary files: image, pdf, video, audio."
        );

        // Windows 原生客户端不发 toolConfig
        assert!(req.get("toolConfig").is_none());
    }

    #[test]
    fn test_tool_config_preserved_when_explicitly_provided() {
        let mut inner_req = json!({
            "contents": [
                { "role": "user", "parts": [{ "text": "Call lookup" }] }
            ],
            "tools": [
                {
                    "functionDeclarations": [
                        { "name": "lookup", "parameters": { "type": "object" } }
                    ]
                }
            ],
            "toolConfig": {
                "functionCallingConfig": {
                    "mode": "ANY",
                    "allowedFunctionNames": ["lookup"]
                }
            }
        });

        InboundThinkingPipeline::align_google_request_prefix_topology_with_model(
            &mut inner_req,
            "gemini-2.5-flash",
            None,
        );

        // 验证客户端显式指定的 toolConfig 得到完整保留，且键序严格位于 tools 之后、labels 之前
        let tc = inner_req
            .get("toolConfig")
            .expect("toolConfig must be preserved");
        assert_eq!(tc["functionCallingConfig"]["mode"], "ANY");
        assert_eq!(
            tc["functionCallingConfig"]["allowedFunctionNames"][0],
            "lookup"
        );

        // 检查键序: tools -> toolConfig -> labels
        let keys: Vec<&str> = inner_req
            .as_object()
            .unwrap()
            .keys()
            .map(|s| s.as_str())
            .collect();
        let tools_idx = keys.iter().position(|&k| k == "tools").unwrap();
        let tc_idx = keys.iter().position(|&k| k == "toolConfig").unwrap();
        let labels_idx = keys.iter().position(|&k| k == "labels").unwrap();
        assert!(tools_idx < tc_idx);
        assert!(tc_idx < labels_idx);
    }

    #[test]
    fn test_official_gemini_alignment() {
        let mut body = json!({
            "requestId": "agent/43461060-f160-43b5-829a-935ed4f20115/1790527686221/fc1f7a63-4efd-47dc-b800-1edad55edb1d/1",
            "model": "gemini-3.8-flash-high",
            "request": {
                "contents": [
                    { "role": "user", "parts": [{ "text": "hello" }] }
                ],
                "tools": [
                    {
                        "functionDeclarations": [
                            {
                                "name": "view_file",
                                "description": "This tool supports text files and following binary files: image, video.",
                                "parameters": { "type": "object" }
                            }
                        ]
                    }
                ],
                "generationConfig": {
                    "thinkingConfig": {
                        "includeThoughts": true,
                        "thinkingBudget": -1
                    }
                }
            }
        });

        InboundThinkingPipeline::align_official_envelope(&mut body);

        let req = body.get("request").unwrap();

        // 1. Labels aligned for Gemini
        let labels = req.get("labels").unwrap();
        assert_eq!(labels["model_enum"], "MODEL_PLACEHOLDER_M318");
        assert_eq!(labels["used_claude"], "false");
        assert_eq!(labels["used_claude_conservative"], "false");
        assert_eq!(labels["used_non_gemini_model"], "false");
        assert_eq!(
            labels["trajectory_id"],
            "fc1f7a63-4efd-47dc-b800-1edad55edb1d"
        );
        assert_eq!(
            labels["request_id"],
            "fc1f7a63-4efd-47dc-b800-1edad55edb1d-0"
        );

        // 2. GenerationConfig maxOutputTokens = 65536 for Gemini, thinkingBudget kept untouched (-1)
        let gc = req.get("generationConfig").unwrap();
        assert_eq!(gc["maxOutputTokens"], 65536);
        assert_eq!(gc["thinkingConfig"]["thinkingBudget"], -1);

        // 3. Tools description kept intact as provided by client (客户端传啥就是啥)
        let tools = req.get("tools").unwrap().as_array().unwrap();
        let view_file_desc = tools[0]["functionDeclarations"][0]["description"]
            .as_str()
            .unwrap();
        assert_eq!(
            view_file_desc,
            "This tool supports text files and following binary files: image, video."
        );

        assert!(req.get("toolConfig").is_none());
    }

    #[test]
    fn test_dynamic_multi_step_index() {
        let mut body = json!({
            "requestId": "agent/43461060-f160-43b5-829a-935ed4f20115/1790527686221/fc1f7a63-4efd-47dc-b800-1edad55edb1d/3",
            "model": "claude-sonnet-4-6",
            "request": {
                "contents": [
                    { "role": "user", "parts": [{ "text": "turn 1" }] },
                    { "role": "model", "parts": [{ "text": "resp 1" }] },
                    { "role": "user", "parts": [{ "text": "turn 2" }] }
                ]
            }
        });

        InboundThinkingPipeline::align_official_envelope(&mut body);

        let req = body.get("request").unwrap();
        let labels = req.get("labels").unwrap();
        // requestId 结尾是 /3 -> 上一步已完成索引 last_step_index 动态计算为 "2"
        assert_eq!(labels["last_step_index"], "2");
        assert_eq!(
            labels["request_id"],
            "fc1f7a63-4efd-47dc-b800-1edad55edb1d-2"
        );
    }

    #[test]
    fn test_official_model_catalog_resolution() {
        use crate::models::OfficialModelCatalog;

        // 1. Claude Opus 4.6 (Thinking)
        let opus = OfficialModelCatalog::get("claude-opus-4-6-thinking").unwrap();
        assert_eq!(opus.model, "MODEL_PLACEHOLDER_M26");
        assert!(opus.is_claude());
        assert!(opus.is_non_gemini());
        assert_eq!(opus.max_output_tokens, Some(64000));
        assert_eq!(opus.thinking_budget, Some(1024));

        // 2. Claude Sonnet 4.6 (Thinking)
        let sonnet = OfficialModelCatalog::get("claude-sonnet-4-6").unwrap();
        assert_eq!(sonnet.model, "MODEL_PLACEHOLDER_M35");
        assert!(sonnet.is_claude());
        assert!(sonnet.is_non_gemini());
        assert_eq!(sonnet.max_output_tokens, Some(64000));

        // 3. Gemini 3.8 Flash High
        let g38 = OfficialModelCatalog::get("gemini-3.8-flash-high").unwrap();
        assert_eq!(g38.model, "MODEL_PLACEHOLDER_M318");
        assert!(!g38.is_claude());
        assert!(!g38.is_non_gemini());
        assert_eq!(g38.max_output_tokens, Some(65536));
        assert_eq!(g38.thinking_budget, Some(-1));

        // 4. Gemini 3.8 Flash Low
        let g38_low = OfficialModelCatalog::get("gemini-3.8-flash-low").unwrap();
        assert_eq!(g38_low.model, "MODEL_PLACEHOLDER_M320");
        assert_eq!(g38_low.thinking_budget, Some(1000));

        // 5. Gemini 3.1 Pro High
        let g31_pro = OfficialModelCatalog::get("gemini-3.1-pro-high").unwrap();
        assert_eq!(g31_pro.model, "MODEL_PLACEHOLDER_M37");
        assert_eq!(g31_pro.thinking_budget, Some(10001));

        // 6. GPT-OSS 120B Medium
        let gpt = OfficialModelCatalog::get("gpt-oss-120b-medium").unwrap();
        assert_eq!(gpt.model, "MODEL_OPENAI_GPT_OSS_120B_MEDIUM");
        assert!(!gpt.is_claude());
        assert!(gpt.is_non_gemini());
        assert_eq!(gpt.max_output_tokens, Some(32768));

        // 6.1 Claude 5.5 系列官方模型权威元数据测试 (Sonnet & Opus 5.5)
        let sonnet55 = OfficialModelCatalog::get("claude-sonnet-5-5-high").unwrap();
        assert_eq!(sonnet55.model, "MODEL_PLACEHOLDER_M405");
        assert!(sonnet55.is_claude());
        assert!(sonnet55.is_non_gemini());
        assert_eq!(sonnet55.max_output_tokens, Some(128000));
        assert_eq!(sonnet55.max_tokens, Some(1000000));
        assert_eq!(sonnet55.thinking_level.as_deref(), Some("3"));

        let opus55 = OfficialModelCatalog::get("claude-opus-5-5-low").unwrap();
        assert_eq!(opus55.model, "MODEL_PLACEHOLDER_M400");
        assert!(opus55.is_claude());
        assert!(opus55.is_non_gemini());
        assert_eq!(opus55.max_output_tokens, Some(128000));
        assert_eq!(opus55.max_tokens, Some(1000000));
        assert_eq!(opus55.thinking_level.as_deref(), Some("1"));

        // 大小写不同的精确名仍然命中同一条
        let g38_case = OfficialModelCatalog::get("Gemini-3.8-Flash-High").unwrap();
        assert_eq!(g38_case.model, "MODEL_PLACEHOLDER_M318");

        // 7. Tab Flash Lite Preview (行内代码补全模型)
        let tab = OfficialModelCatalog::get("tab_flash_lite_preview").unwrap();
        assert_eq!(tab.model, "MODEL_PLACEHOLDER_M19");
        assert_eq!(tab.max_output_tokens, Some(4096));

        // 空名、短名、以及只是包含已知键的近邻名必须未命中，避免随机选中别的结构体
        assert!(OfficialModelCatalog::get("").is_none());
        assert!(OfficialModelCatalog::get("flash").is_none());
        assert!(OfficialModelCatalog::get("gemini-2.5-flash-lite-preview").is_none());
    }

    #[test]
    fn test_gateway_mode_negative_one_budget_fallback_to_official_default() {
        use crate::proxy::config::{
            update_thinking_budget_config, ThinkingBudgetConfig, TEST_CONFIG_LOCK,
        };
        use crate::proxy::pipeline::inbound::ClientThinkingSwitch;

        let _lock = TEST_CONFIG_LOCK.lock().unwrap();
        update_thinking_budget_config(ThinkingBudgetConfig::default());

        struct ResetGuard;
        impl Drop for ResetGuard {
            fn drop(&mut self) {
                crate::proxy::config::update_thinking_budget_config(ThinkingBudgetConfig::default());
            }
        }
        let _guard = ResetGuard;

        // 1. Gemini 3.8 Flash High -> official default thinking_budget is -1
        let mut gc_high = json!({});
        InboundThinkingPipeline::configure_inbound_thinking(
            "gemini-3.8-flash-high",
            &mut gc_high,
            ClientThinkingSwitch::Default,
            None,
            None,
            None,
        );
        let tc_high = gc_high.get("thinkingConfig").unwrap();
        assert_eq!(tc_high["includeThoughts"], true);
        assert_eq!(tc_high["thinkingBudget"], -1);

        // 2. Gemini 3.8 Flash Low -> official default thinking_budget is 1000
        let mut gc_low = json!({});
        InboundThinkingPipeline::configure_inbound_thinking(
            "gemini-3.8-flash-low",
            &mut gc_low,
            ClientThinkingSwitch::Default,
            None,
            None,
            None,
        );
        let tc_low = gc_low.get("thinkingConfig").unwrap();
        assert_eq!(tc_low["includeThoughts"], true);
        assert_eq!(tc_low["thinkingBudget"], 1000);

        // 3. Gemini 3.8 Flash Medium -> official default thinking_budget is 4000
        let mut gc_med = json!({});
        InboundThinkingPipeline::configure_inbound_thinking(
            "gemini-3.8-flash-medium",
            &mut gc_med,
            ClientThinkingSwitch::Default,
            None,
            None,
            None,
        );
        let tc_med = gc_med.get("thinkingConfig").unwrap();
        assert_eq!(tc_med["includeThoughts"], true);
        assert_eq!(tc_med["thinkingBudget"], 4000);

        // 4. Claude Sonnet 4.6 -> official default thinking_budget is 1024
        let mut gc_claude = json!({});
        InboundThinkingPipeline::configure_inbound_thinking(
            "claude-sonnet-4-6",
            &mut gc_claude,
            ClientThinkingSwitch::Default,
            None,
            None,
            None,
        );
        let tc_claude = gc_claude.get("thinkingConfig").unwrap();
        assert_eq!(tc_claude["includeThoughts"], true);
        assert_eq!(tc_claude["thinkingBudget"], 1024);

        // 5. Gemini 3.1 Pro High -> official default thinking_budget is 10001
        let mut gc_pro = json!({});
        InboundThinkingPipeline::configure_inbound_thinking(
            "gemini-3.1-pro-high",
            &mut gc_pro,
            ClientThinkingSwitch::Default,
            None,
            None,
            None,
        );
        let tc_pro = gc_pro.get("thinkingConfig").unwrap();
        assert_eq!(tc_pro["includeThoughts"], true);
        assert_eq!(tc_pro["thinkingBudget"], 10001);
    }

    #[test]
    fn test_bare_3x_flash_effort_routing_and_tiered_preservation() {
        use crate::proxy::common::model_mapping::resolve_model_route_with_effort;
        use std::collections::HashMap;

        let empty_mapping = HashMap::new();

        // 1. 裸 3.8 Flash 模型依据思考档位路由
        assert_eq!(
            resolve_model_route_with_effort("gemini-3.8-flash", &empty_mapping, Some("high")),
            "gemini-3.8-flash-high"
        );
        assert_eq!(
            resolve_model_route_with_effort("gemini-3.8-flash", &empty_mapping, None),
            "gemini-3.8-flash-tiered"
        );
        assert_eq!(
            resolve_model_route_with_effort("gemini-3.8-flash", &empty_mapping, Some("low")),
            "gemini-3.8-flash-low"
        );
        assert_eq!(
            resolve_model_route_with_effort("gemini-3.8-flash", &empty_mapping, Some("medium")),
            "gemini-3.8-flash-medium"
        );

        // 2. 裸 3.7 Flash 与 3.6 Flash 模型同理
        assert_eq!(
            resolve_model_route_with_effort("gemini-3.7-flash", &empty_mapping, Some("low")),
            "gemini-3.7-flash-low"
        );
        assert_eq!(
            resolve_model_route_with_effort("gemini-3.7-flash", &empty_mapping, None),
            "gemini-3.7-flash-tiered"
        );
        assert_eq!(
            resolve_model_route_with_effort("gemini-3.6-flash", &empty_mapping, Some("medium")),
            "gemini-3.6-flash-medium"
        );

        // 3. Tiered 模型原样保留模型名，绝对不改名！
        assert_eq!(
            resolve_model_route_with_effort(
                "gemini-3.8-flash-tiered",
                &empty_mapping,
                Some("high")
            ),
            "gemini-3.8-flash-tiered"
        );
        assert_eq!(
            resolve_model_route_with_effort("gemini-3.8-flash-tiered", &empty_mapping, Some("low")),
            "gemini-3.8-flash-tiered"
        );
        assert_eq!(
            resolve_model_route_with_effort("gemini-3.8-flash-tiered", &empty_mapping, None),
            "gemini-3.8-flash-tiered"
        );
    }

    #[test]
    fn test_bare_flash_and_tiered_budget_resolution() {
        use crate::proxy::config::{
            ThinkingBudgetConfig, ThinkingBudgetMode, ThinkingControlSource, TEST_CONFIG_LOCK,
        };
        use crate::proxy::model_specs::resolve_custom_budget;
        use crate::proxy::pipeline::inbound::ClientThinkingSwitch;

        let _lock = TEST_CONFIG_LOCK.lock().unwrap();

        let mut tb_default = ThinkingBudgetConfig::default();
        tb_default.control_source = ThinkingControlSource::Gateway;
        tb_default.flash_mode = ThinkingBudgetMode::Custom;
        tb_default.flash_high = -1;
        tb_default.flash_medium = -1;
        tb_default.flash_low = -1;
        tb_default.flash_tiered = -1;

        // 1. Non-tiered Flash High: 默认 -1
        assert_eq!(
            resolve_custom_budget("gemini-3.8-flash-high", None, None, &tb_default, None),
            Some(-1)
        );

        // 2. Non-tiered Flash Low: 默认 1000
        assert_eq!(
            resolve_custom_budget("gemini-3.8-flash-low", None, None, &tb_default, None),
            Some(1000)
        );

        // 3. Non-tiered Flash Medium: 默认 4000
        assert_eq!(
            resolve_custom_budget("gemini-3.8-flash-medium", None, None, &tb_default, None),
            Some(4000)
        );

        // 4. 用户配置自定义预算时的优先级生效
        let mut tb_custom = tb_default.clone();
        tb_custom.flash_high = 16000;
        tb_custom.flash_low = 2000;
        tb_custom.flash_medium = 8000;
        assert_eq!(
            resolve_custom_budget("gemini-3.8-flash-high", None, None, &tb_custom, None),
            Some(16000)
        );
        assert_eq!(
            resolve_custom_budget("gemini-3.8-flash-low", None, None, &tb_custom, None),
            Some(2000)
        );
        assert_eq!(
            resolve_custom_budget("gemini-3.8-flash-medium", None, None, &tb_custom, None),
            Some(8000)
        );

        // 5. Tiered 模型：严格按照 client effort (low/med/high) 填充预算
        assert_eq!(
            resolve_custom_budget(
                "gemini-3.8-flash-tiered",
                Some("low"),
                None,
                &tb_default,
                None
            ),
            Some(1000)
        );
        assert_eq!(
            resolve_custom_budget(
                "gemini-3.8-flash-tiered",
                Some("medium"),
                None,
                &tb_default,
                None
            ),
            Some(4000)
        );
        assert_eq!(
            resolve_custom_budget(
                "gemini-3.8-flash-tiered",
                Some("high"),
                None,
                &tb_default,
                None
            ),
            Some(-1)
        );
        assert_eq!(
            resolve_custom_budget("gemini-3.8-flash-tiered", None, None, &tb_default, None),
            Some(-1)
        );

        // 5.1 具名后缀绝对优先级测试：
        // 即使用户/客户端传入了 client_effort = "high"，具名模型 gemini-3.8-flash-medium 也必须返回 medium (4000) 或用户配的 medium！
        assert_eq!(
            resolve_custom_budget(
                "gemini-3.8-flash-medium",
                Some("high"),
                None,
                &tb_default,
                None
            ),
            Some(4000)
        );
        assert_eq!(
            resolve_custom_budget(
                "gemini-3.8-flash-low",
                Some("high"),
                None,
                &tb_default,
                None
            ),
            Some(1000)
        );

        // 5.2 具名模型的 Default 模式回退测试：
        // 当 flash_mode 为 Default 时，resolve_custom_budget 必须返回 None，以便回落至官方模型目录
        let mut tb_system_default = tb_default.clone();
        tb_system_default.flash_mode = ThinkingBudgetMode::Default;
        assert_eq!(
            resolve_custom_budget(
                "gemini-3.8-flash-medium",
                None,
                None,
                &tb_system_default,
                None
            ),
            None
        );
        assert_eq!(
            resolve_custom_budget("gemini-3.8-flash-low", None, None, &tb_system_default, None),
            None
        );
        assert_eq!(
            resolve_custom_budget(
                "gemini-3.8-flash-high",
                None,
                None,
                &tb_system_default,
                None
            ),
            None
        );

        // 6. Inbound pipeline 配置测试：-1 预算正确注入且 maxOutputTokens 不被截断
        let mut gc = json!({});
        InboundThinkingPipeline::configure_inbound_thinking(
            "gemini-3.8-flash-high",
            &mut gc,
            ClientThinkingSwitch::Default,
            Some("high"),
            None,
            None,
        );
        assert_eq!(gc["thinkingConfig"]["thinkingBudget"], -1);
        assert_eq!(gc["thinkingConfig"]["includeThoughts"], true);
    }

    #[test]
    fn test_thinking_budget_invariant_scaled_down_when_output_capped() {
        use crate::proxy::config::{
            update_thinking_budget_config, ThinkingBudgetConfig, ThinkingControlSource,
            TEST_CONFIG_LOCK,
        };
        use crate::proxy::pipeline::inbound::ClientThinkingSwitch;

        let _lock = TEST_CONFIG_LOCK.lock().unwrap();
        let mut config = ThinkingBudgetConfig::default();
        config.control_source = ThinkingControlSource::Client;
        update_thinking_budget_config(config);

        struct ResetGuard;
        impl Drop for ResetGuard {
            fn drop(&mut self) {
                crate::proxy::config::update_thinking_budget_config(ThinkingBudgetConfig::default());
            }
        }
        let _guard = ResetGuard;

        // 模拟客户端传入超大思考预算（与输出上限相等，如 Claude 64000）
        let mut gc = json!({
            "maxOutputTokens": 64000
        });
        InboundThinkingPipeline::configure_inbound_thinking(
            "claude-sonnet-4-6",
            &mut gc,
            ClientThinkingSwitch::Enabled,
            None,
            Some(64000),
            None,
        );

        let max_tokens = gc["maxOutputTokens"].as_i64().unwrap();
        let budget = gc["thinkingConfig"]["thinkingBudget"].as_i64().unwrap();

        // 严格断言满足 Google v1internal 铁律：maxOutputTokens > thinkingBudget
        assert!(max_tokens > budget);
        assert_eq!(max_tokens, 64000);
        assert_eq!(budget, 62976);

        // 验证 Gemini Wrapper 层在模型限额截断时的联动缩减
        let body = json!({
            "model": "claude-sonnet-4-6",
            "generationConfig": {
                "maxOutputTokens": 70000,
                "thinkingConfig": {
                    "includeThoughts": true,
                    "thinkingBudget": 65000
                }
            }
        });
        let wrapped = crate::proxy::mappers::gemini::wrapper::wrap_request(
            &body,
            "test-proj",
            "claude-sonnet-4-6",
            None,
            None,
            None,
        );
        let wrapped_gc = &wrapped["request"]["generationConfig"];
        let wrapped_max = wrapped_gc["maxOutputTokens"].as_u64().unwrap();
        let wrapped_budget = wrapped_gc["thinkingConfig"]["thinkingBudget"]
            .as_u64()
            .unwrap();
        assert!(wrapped_max > wrapped_budget);
        assert_eq!(wrapped_max, 64000);
        assert_eq!(wrapped_budget, 62976);
    }

    #[test]
    fn test_thinking_budget_preserved_when_official_capacity_allows() {
        use crate::proxy::config::{
            update_thinking_budget_config, ThinkingBudgetConfig, ThinkingControlSource,
            TEST_CONFIG_LOCK,
        };
        use crate::proxy::pipeline::inbound::ClientThinkingSwitch;

        let _lock = TEST_CONFIG_LOCK.lock().unwrap();
        let mut config = ThinkingBudgetConfig::default();
        config.control_source = ThinkingControlSource::Client;
        update_thinking_budget_config(config);

        struct ResetGuard;
        impl Drop for ResetGuard {
            fn drop(&mut self) {
                crate::proxy::config::update_thinking_budget_config(ThinkingBudgetConfig::default());
            }
        }
        let _guard = ResetGuard;

        // 核心场景：客户端传了较小的 maxOutputTokens (如 4096)，但指定了高思考预算 (如 16384)
        // 目标模型是 Claude 4.6 (官方物理容量 64,000，完全可以包住 16,384)
        let mut gc = json!({
            "maxOutputTokens": 4096
        });
        InboundThinkingPipeline::configure_inbound_thinking(
            "claude-sonnet-4-6",
            &mut gc,
            ClientThinkingSwitch::Enabled,
            None,
            Some(16384),
            None,
        );

        let max_tokens = gc["maxOutputTokens"].as_i64().unwrap();
        let budget = gc["thinkingConfig"]["thinkingBudget"].as_i64().unwrap();

        // 验证：用户的思考预算 16384 被 100% 完整保留，没有被粗暴砍掉！
        assert_eq!(budget, 16384);
        // 验证：maxOutputTokens 被智能提升，且满足 Google 协议硬约束：maxOutputTokens > thinkingBudget
        assert!(max_tokens > budget);
        assert!(max_tokens <= 64000);
        assert_eq!(max_tokens, 16384 + 1024);

        // 验证 Gemini Wrapper 层同样保全用户的思考预算
        let body = json!({
            "model": "claude-sonnet-4-6",
            "generationConfig": {
                "maxOutputTokens": 4096,
                "thinkingConfig": {
                    "includeThoughts": true,
                    "thinkingBudget": 16384
                }
            }
        });
        let wrapped = crate::proxy::mappers::gemini::wrapper::wrap_request(
            &body,
            "test-proj",
            "claude-sonnet-4-6",
            None,
            None,
            None,
        );
        let wrapped_gc = &wrapped["request"]["generationConfig"];
        let wrapped_max = wrapped_gc["maxOutputTokens"].as_u64().unwrap();
        let wrapped_budget = wrapped_gc["thinkingConfig"]["thinkingBudget"]
            .as_i64()
            .unwrap();
        assert_eq!(wrapped_budget, 16384);
        assert!(wrapped_max > wrapped_budget as u64);
        assert_eq!(wrapped_max, 16384 + 1024);
    }

    #[test]
    fn test_thinking_budget_fast_intent_6x_ratio_compression() {
        use crate::proxy::config::{
            update_thinking_budget_config, ThinkingBudgetConfig, ThinkingControlSource,
            TEST_CONFIG_LOCK,
        };
        use crate::proxy::pipeline::inbound::ClientThinkingSwitch;

        let _lock = TEST_CONFIG_LOCK.lock().unwrap();
        let mut config = ThinkingBudgetConfig::default();
        config.control_source = ThinkingControlSource::Client;
        update_thinking_budget_config(config);

        struct ResetGuard;
        impl Drop for ResetGuard {
            fn drop(&mut self) {
                crate::proxy::config::update_thinking_budget_config(ThinkingBudgetConfig::default());
            }
        }
        let _guard = ResetGuard;

        // 场景 1: 客户端明确传 m=1024，但思考预算 t=8192 (>= 6*m) -> 极速意图，预算压缩为 0
        let mut gc1 = json!({
            "maxOutputTokens": 1024
        });
        InboundThinkingPipeline::configure_inbound_thinking(
            "claude-sonnet-4-6",
            &mut gc1,
            ClientThinkingSwitch::Enabled,
            None,
            Some(8192),
            None,
        );

        let max_tokens1 = gc1["maxOutputTokens"].as_i64().unwrap();
        let budget1 = gc1["thinkingConfig"]["thinkingBudget"].as_i64().unwrap();
        assert_eq!(max_tokens1, 1024);
        assert_eq!(budget1, 0, "6倍反差且m<=1024时思考预算应压缩为0以秒回");

        // 场景 2: 客户端明确传 m=1500，思考预算 t=9000 (>= 6*m) -> 预算压缩为 1500 - 1024 = 476
        let mut gc2 = json!({
            "maxOutputTokens": 1500
        });
        InboundThinkingPipeline::configure_inbound_thinking(
            "claude-sonnet-4-6",
            &mut gc2,
            ClientThinkingSwitch::Enabled,
            None,
            Some(9000),
            None,
        );

        let max_tokens2 = gc2["maxOutputTokens"].as_i64().unwrap();
        let budget2 = gc2["thinkingConfig"]["thinkingBudget"].as_i64().unwrap();
        assert_eq!(max_tokens2, 1500);
        assert_eq!(budget2, 476, "6倍反差且m>1024时思考预算应压缩为 m - 1024");
        assert!(max_tokens2 > budget2);

        // 场景 3: 验证 Gemini Wrapper 层同样遵守 6 倍极速压缩规则
        let body = json!({
            "model": "gemini-3.8-flash-high",
            "generationConfig": {
                "maxOutputTokens": 1024,
                "thinkingConfig": {
                    "includeThoughts": true,
                    "thinkingBudget": 8192
                }
            }
        });
        let wrapped = crate::proxy::mappers::gemini::wrapper::wrap_request(
            &body,
            "test-proj",
            "gemini-3.8-flash-high",
            None,
            None,
            None,
        );
        let wrapped_gc = &wrapped["request"]["generationConfig"];
        let wrapped_max = wrapped_gc["maxOutputTokens"].as_u64().unwrap();
        let wrapped_budget = wrapped_gc["thinkingConfig"]["thinkingBudget"]
            .as_i64()
            .unwrap();
        assert_eq!(wrapped_max, 1024);
        assert_eq!(wrapped_budget, 0);
    }
}
