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
            "gemini-3.8-flash-high"
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
            "gemini-3.7-flash-high"
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
            ThinkingBudgetConfig, ThinkingBudgetMode, ThinkingControlSource,
        };
        use crate::proxy::model_specs::resolve_custom_budget;
        use crate::proxy::pipeline::inbound::ClientThinkingSwitch;

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
}
