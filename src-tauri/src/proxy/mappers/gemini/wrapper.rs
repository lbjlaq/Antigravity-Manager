// Gemini v1internal 包装/解包
use serde_json::{json, Value};
use tracing::{debug, error, info};

/// 包装请求体为 v1internal 格式
pub fn wrap_request_v2(
    body: &Value,
    project_id: &str,
    mapped_model: &str,
    account_id: Option<&str>,
    session_id: Option<&str>,
    token: Option<&crate::proxy::token_manager::ProxyToken>, // [NEW] 动态规格注入
    token_manager: Option<&std::sync::Arc<crate::proxy::TokenManager>>,
) -> Value {
    // 优先使用传入的 mapped_model，其次尝试从 body 获取
    let original_model = body
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or(mapped_model);

    // 如果 mapped_model 是空的，则使用 original_model
    let final_model_name = if !mapped_model.is_empty() {
        mapped_model
    } else {
        original_model
    };

    // [ADDED v4.1.24] 计算 message_count 供 requestId 使用
    let message_count = body
        .get("contents")
        .and_then(|c| c.as_array())
        .map(|a| a.len())
        .unwrap_or(1);

    // 复制 body 以便修改
    let mut inner_request = body.clone();

    // 深度清理 [undefined] 字符串 (Cherry Studio 等客户端常见注入)
    crate::proxy::mappers::common_utils::deep_clean_undefined(&mut inner_request, 0);

    // [FIX #1522] Inject dummy IDs for Claude models in Gemini protocol
    let is_target_claude = final_model_name.to_lowercase().contains("claude");

    let compression_level = crate::proxy::config::get_global_compression_level();

    let mut compression_applied = false;
    if compression_level == "high" {
        let tm = token_manager;
        let context_limit = if final_model_name.contains("flash") {
            1_000_000
        } else {
            2_000_000
        };

        let raw_estimated =
            crate::proxy::mappers::context_manager::ContextManager::estimate_gemini_token_usage(
                &inner_request,
            );
        let calibrator = crate::proxy::mappers::estimation_calibrator::get_calibrator();
        let mut estimated_usage = calibrator.calibrate(raw_estimated);
        let mut usage_ratio = estimated_usage as f32 / context_limit as f32;

        let threshold_l1 = crate::proxy::config::get_global_threshold_l1();
        let threshold_l2 = crate::proxy::config::get_global_threshold_l2();
        let threshold_l3 = crate::proxy::config::get_global_threshold_l3();

        let trace_id = format!(
            "gemini_req_{}",
            chrono::Utc::now().timestamp_subsec_millis()
        );

        tracing::info!(
            "[{}] [ContextManager] [Gemini] Context pressure: {:.1}% (raw: {}, calibrated: {} / {}), Calibration factor: {:.2}",
            trace_id, usage_ratio * 100.0, raw_estimated, estimated_usage, context_limit, calibrator.get_factor()
        );

        // ===== Layer 1: Tool Message Trimming =====
        if usage_ratio > threshold_l1 && !compression_applied {
            if crate::proxy::mappers::context_manager::ContextManager::trim_gemini_tool_messages(
                &mut inner_request,
                5,
            ) {
                tracing::info!(
                    "[{}] [Layer-1] [Gemini] Tool trimming triggered (usage: {:.1}%, threshold: {:.1}%)",
                    trace_id, usage_ratio * 100.0, threshold_l1 * 100.0
                );
                compression_applied = true;

                let new_raw = crate::proxy::mappers::context_manager::ContextManager::estimate_gemini_token_usage(&inner_request);
                let new_usage = calibrator.calibrate(new_raw);
                let new_ratio = new_usage as f32 / context_limit as f32;

                tracing::info!(
                    "[{}] [Layer-1] [Gemini] Compression result: {:.1}% → {:.1}% (saved {} tokens)",
                    trace_id,
                    usage_ratio * 100.0,
                    new_ratio * 100.0,
                    estimated_usage - new_usage
                );

                if new_ratio < 0.7 {
                    estimated_usage = new_usage;
                    usage_ratio = new_ratio;
                } else {
                    usage_ratio = new_ratio;
                    compression_applied = false;
                }
            }
        }

        // ===== Layer 2: Thinking Content Compression =====
        if usage_ratio > threshold_l2 && !compression_applied {
            tracing::info!(
                "[{}] [Layer-2] [Gemini] Thinking compression triggered (usage: {:.1}%, threshold: {:.1}%)",
                trace_id, usage_ratio * 100.0, threshold_l2 * 100.0
            );

            if crate::proxy::mappers::context_manager::ContextManager::compress_gemini_thinking_preserve_signature(
                &mut inner_request,
                4,
            ) {
                compression_applied = true;

                let new_raw = crate::proxy::mappers::context_manager::ContextManager::estimate_gemini_token_usage(&inner_request);
                let new_usage = calibrator.calibrate(new_raw);
                let new_ratio = new_usage as f32 / context_limit as f32;

                tracing::info!(
                    "[{}] [Layer-2] [Gemini] Compression result: {:.1}% → {:.1}% (saved {} tokens)",
                    trace_id, usage_ratio * 100.0, new_ratio * 100.0, estimated_usage - new_usage
                );

                usage_ratio = new_ratio;
            }
        }

        // ===== Layer 3: Fork Conversation + XML Summary =====
        if usage_ratio > threshold_l3 && !compression_applied {
            tracing::info!(
                "[{}] [Layer-3] [Gemini] Context pressure ({:.1}%) exceeded threshold ({:.1}%), spawning Fork+Summary in background",
                trace_id, usage_ratio * 100.0, threshold_l3 * 100.0
            );

            let tm_opt = tm.cloned();
            let sid_str = session_id.unwrap_or_default().to_string();
            let body_clone = inner_request.clone();
            let trace_id_clone = trace_id.clone();
            let proj_clone = project_id.to_string();
            let acc_clone = account_id.unwrap_or_default().to_string();

            if let Some(tm_arc) = tm_opt {
                tokio::spawn(async move {
                    match try_compress_gemini_with_summary(
                        &body_clone,
                        &trace_id_clone,
                        &tm_arc,
                        &sid_str,
                        &proj_clone,
                        &acc_clone,
                    )
                    .await
                    {
                        Ok(_) => {
                            tracing::info!(
                                "[{}] [Layer-3] [Gemini] Background Fork+Summary completed successfully",
                                trace_id_clone
                            );
                        }
                        Err(e) => {
                            tracing::error!(
                                "[{}] [Layer-3] [Gemini] Background Fork+Summary failed: {}",
                                trace_id_clone,
                                e
                            );
                        }
                    }
                });
            }
        }
    }

    if compression_level != "disabled" {
        if let Some(contents) = inner_request
            .get_mut("contents")
            .and_then(|c| c.as_array_mut())
        {
            let total_turns = contents.len();
            let protected_last_n = 4;
            let start_protection_idx = total_turns.saturating_sub(protected_last_n);

            for (i, content) in contents.iter_mut().enumerate() {
                if let Some(parts) = content.get_mut("parts").and_then(|p| p.as_array_mut()) {
                    for part in parts {
                        if let Some(obj) = part.as_object_mut() {
                            if compression_level == "medium" || compression_level == "high" {
                                if i < start_protection_idx {
                                    if let Some(text_val) =
                                        obj.get_mut("text").and_then(|t| t.as_str())
                                    {
                                        let cleaned = crate::proxy::mappers::caveman_cleaner::CavemanCleaner::clean(text_val);
                                        if cleaned != text_val {
                                            obj.insert("text".to_string(), json!(cleaned));
                                        }
                                    }
                                }
                            }
                            if let Some(fr) = obj.get_mut("functionResponse") {
                                if let Some(resp_obj) =
                                    fr.get_mut("response").and_then(|r| r.as_object_mut())
                                {
                                    for (_key, val) in resp_obj.iter_mut() {
                                        if let Some(s) = val.as_str() {
                                            let cleaned = crate::proxy::mappers::rtk_cleaner::RtkCleaner::clean(s, 48);
                                            if cleaned != s {
                                                *val = json!(cleaned);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if let Some(contents) = inner_request
        .get_mut("contents")
        .and_then(|c| c.as_array_mut())
    {
        for (i, content) in contents.iter_mut().enumerate() {
            let mut name_counters: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();

            if let Some(parts) = content.get_mut("parts").and_then(|p| p.as_array_mut()) {
                for part in parts {
                    if let Some(obj) = part.as_object_mut() {
                        // 1. 处理 functionCall (Assistant 请求调用工具)
                        if let Some(fc) = obj.get_mut("functionCall") {
                            if fc.get("id").is_none() && is_target_claude {
                                let name =
                                    fc.get("name").and_then(|n| n.as_str()).unwrap_or("unknown");
                                let count = name_counters.entry(name.to_string()).or_insert(0);
                                let call_id = format!("call_{}_{}", name, count);
                                *count += 1;

                                fc.as_object_mut()
                                    .unwrap()
                                    .insert("id".to_string(), json!(call_id));
                                tracing::debug!("[Gemini-Wrap] Request stage: Injected missing call_id '{}' for Claude model", call_id);
                            }
                        }

                        // 2. 处理 functionResponse (User 回复工具结果)
                        if let Some(fr) = obj.get_mut("functionResponse") {
                            if fr.get("id").is_none() && is_target_claude {
                                // 启发：如果客户端（如 OpenCode）在响应时没带 ID，说明它收到响应时就没 ID。
                                // 我们在这里生成的 ID 必须与我们在 inject_ids_to_response 中注入响应的 ID 一致。
                                let name =
                                    fr.get("name").and_then(|n| n.as_str()).unwrap_or("unknown");
                                let count = name_counters.entry(name.to_string()).or_insert(0);
                                let call_id = format!("call_{}_{}", name, count);
                                *count += 1;

                                fr.as_object_mut()
                                    .unwrap()
                                    .insert("id".to_string(), json!(call_id));
                                tracing::debug!("[Gemini-Wrap] Request stage: Injected synced response_id '{}' for Claude model", call_id);
                            }
                        }

                        // 3. 处理 thoughtSignature / thought_signature
                        if obj.contains_key("functionCall") {
                            let sig_opt = obj
                                .get("thoughtSignature")
                                .or(obj.get("thought_signature"))
                                .cloned();
                            if let Some(sig) = sig_opt {
                                if obj.get("thoughtSignature").is_none() {
                                    obj.insert("thoughtSignature".to_string(), sig.clone());
                                }
                                if obj.get("thought_signature").is_none() {
                                    obj.insert("thought_signature".to_string(), sig);
                                }
                            } else if let Some(s_id) = session_id {
                                if let Some(sig) = crate::proxy::SignatureCache::global()
                                    .get_session_signature(s_id)
                                {
                                    obj.insert("thoughtSignature".to_string(), json!(sig));
                                    obj.insert("thought_signature".to_string(), json!(sig));
                                    tracing::debug!("[Gemini-Wrap] Injected signature (len: {}) for session: {}", sig.len(), s_id);
                                } else {
                                    // [FIX #2167] Session 缓存为空时对 flash 模型注入哨兵值
                                    // Flash 模型如果不提供任何签名，Gemini API 会拒绝 functionCall
                                    let is_flash =
                                        final_model_name.to_lowercase().contains("gemini-3-flash")
                                            || final_model_name
                                                .to_lowercase()
                                                .contains("gemini-3.1-flash");
                                    if is_flash {
                                        obj.insert(
                                            "thoughtSignature".to_string(),
                                            json!("skip_thought_signature_validator"),
                                        );
                                        obj.insert(
                                            "thought_signature".to_string(),
                                            json!("skip_thought_signature_validator"),
                                        );
                                        tracing::debug!("[Gemini-Wrap] [FIX #2167] Injected sentinel signature for flash model (no session cache)");
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // [FIX Issue #1355] Gemini Flash thinking budget capping
    // [CONFIGURABLE] 现在改为遵循全局 Thinking Budget 配置
    // [FIX #1557] Also apply to Pro/Thinking models to ensure budget processing
    // [FIX #1557] Auto-inject thinkingConfig if missing for these models
    let lower_model = final_model_name.to_lowercase();
    if lower_model.contains("flash")
        || lower_model.contains("pro")
        || lower_model.contains("thinking")
    {
        // [NEW] Extract OpenAI-style max_tokens before mutably borrowing gen_config
        let req_max_tokens = inner_request.get("max_tokens").and_then(|v| v.as_u64());

        // Determine model family and capability beforehand to avoid borrow checker conflicts
        let is_claude = lower_model.contains("claude");
        let is_preview = lower_model.contains("preview");
        let should_inject = lower_model.contains("thinking")
            || (lower_model.contains("gemini-2.0-pro") && !is_preview)
            || (lower_model.contains("gemini-3-pro") && !is_preview)
            || (lower_model.contains("gemini-3.1-pro") && !is_preview);

        if should_inject {
            // Scope for borrowing inner_request/gen_config
            let mut has_thinking = false;
            if is_claude {
                has_thinking = inner_request.get("thinking").is_some();
            } else {
                if let Some(gc) = inner_request
                    .get("generationConfig")
                    .and_then(|v| v.as_object())
                {
                    has_thinking = gc.get("thinkingConfig").is_some();
                }
            }

            if !has_thinking {
                tracing::debug!(
                    "[Gemini-Wrap] Auto-injecting default thinking for {}",
                    final_model_name
                );

                // [FIX] 统一注入到 generationConfig.thinkingConfig
                // 使用动态规格提供的默认预算
                let default_budget =
                    crate::proxy::model_specs::get_thinking_budget(final_model_name, token);

                let gen_config = inner_request
                    .as_object_mut()
                    .unwrap()
                    .entry("generationConfig")
                    .or_insert(json!({}))
                    .as_object_mut()
                    .unwrap();

                gen_config.insert(
                    "thinkingConfig".to_string(),
                    json!({
                        "includeThoughts": true,
                        "thinkingBudget": default_budget
                    }),
                );
            }
        }

        // Re-acquire gen_config to satisfy borrow checker and scope requirements for later logic
        let gen_config = inner_request
            .as_object_mut()
            .unwrap()
            .entry("generationConfig")
            .or_insert(json!({}))
            .as_object_mut()
            .unwrap();

        // [ADDED v4.1.24] Inject topK=40 and topP=1.0 if not present to match official client
        if !gen_config.contains_key("topK") {
            gen_config.insert("topK".to_string(), json!(40));
        }
        if !gen_config.contains_key("topP") {
            gen_config.insert("topP".to_string(), json!(1.0));
        }

        // [FIX] Convert v1beta thinkingLevel (string) to v1internal thinkingBudget (number).
        // Clients (e.g. OpenClaw, Cline) may send thinkingLevel which v1internal does not accept,
        // causing 400 INVALID_ARGUMENT. Convert before any budget processing below.
        if let Some(thinking_config) = gen_config.get_mut("thinkingConfig") {
            if let Some(level) = thinking_config
                .get("thinkingLevel")
                .and_then(|v| v.as_str())
                .map(|s| s.to_uppercase())
            {
                let thinking_budget_cap =
                    crate::proxy::model_specs::get_thinking_budget(final_model_name, token);
                let budget: i64 = match level.as_str() {
                    "NONE" => 0,
                    "LOW" => (thinking_budget_cap / 4).max(4096) as i64,
                    "MEDIUM" => (thinking_budget_cap / 2).max(8192) as i64,
                    "HIGH" => thinking_budget_cap as i64,
                    _ => (thinking_budget_cap / 2).max(8192) as i64, // safe default
                };
                tracing::info!(
                    "[Gemini-Wrap] Converting thinkingLevel '{}' to thinkingBudget {}",
                    level,
                    budget
                );
                if let Some(tc) = thinking_config.as_object_mut() {
                    tc.remove("thinkingLevel");
                    tc.insert("thinkingBudget".to_string(), json!(budget));
                }
            }
        }

        if let Some(thinking_config) = gen_config.get_mut("thinkingConfig") {
            if let Some(budget_val) = thinking_config.get("thinkingBudget") {
                if let Some(budget_i64) = budget_val.as_i64() {
                    // [NEW] -1 indicates native dynamic mode, skip capping
                    if budget_i64 != -1 {
                        let budget = budget_i64 as u64;
                        let thinking_budget_cap =
                            crate::proxy::model_specs::get_thinking_budget(final_model_name, token);
                        let tb_config = crate::proxy::config::get_thinking_budget_config();
                        let final_budget = match tb_config.mode {
                            crate::proxy::config::ThinkingBudgetMode::Passthrough => budget,
                            crate::proxy::config::ThinkingBudgetMode::Custom => {
                                let val = tb_config.custom_value as u64;
                                let is_limited = (final_model_name.contains("gemini")
                                    || final_model_name.contains("thinking"))
                                    && !final_model_name.contains("-image");

                                if is_limited && val > thinking_budget_cap {
                                    thinking_budget_cap
                                } else {
                                    val
                                }
                            }
                            crate::proxy::config::ThinkingBudgetMode::Auto => {
                                let is_limited = (final_model_name.contains("gemini")
                                    || final_model_name.contains("thinking"))
                                    && !final_model_name.contains("-image");

                                if is_limited && budget > thinking_budget_cap {
                                    thinking_budget_cap
                                } else {
                                    budget
                                }
                            }
                            crate::proxy::config::ThinkingBudgetMode::Adaptive => budget,
                        };

                        if final_budget != budget {
                            thinking_config["thinkingBudget"] = json!(final_budget);
                        }
                    }
                }
            }
        }

        // [FIX #1747] Ensure max_tokens (maxOutputTokens) is greater than thinking_budget
        // Google v1internal requires maxOutputTokens > thinkingBudget.
        // [FIX #1825] Handle adaptive fallback (incl. -1 and thinkingLevel)
        let thinking_config_opt = gen_config.get("thinkingConfig");
        let is_adaptive = thinking_config_opt.map_or(false, |t| {
            t.get("thinkingLevel").is_some()
                || t.get("thinkingBudget").and_then(|v| v.as_i64()) == Some(-1)
        }) || (thinking_config_opt
            .and_then(|t| t.get("thinkingBudget").and_then(|v| v.as_u64()))
            == Some(32768)
            && is_claude);

        if let Some(thinking_config) = gen_config.get("thinkingConfig") {
            let budget_opt = thinking_config
                .get("thinkingBudget")
                .and_then(|v| v.as_i64());

            // For adaptive or dynamic mode, we only need to ensure max tokens is large.
            // For fixed budget, we must satisfy maxOutputTokens > thinkingBudget.
            let current_max = gen_config
                .get("maxOutputTokens")
                .and_then(|v| v.as_u64())
                .or(req_max_tokens);

            if is_adaptive {
                if current_max.map_or(true, |m| m < 131072) {
                    gen_config.insert("maxOutputTokens".to_string(), json!(131072));
                }
            } else if let Some(budget_i64) = budget_opt {
                if budget_i64 > 0 {
                    let budget = budget_i64 as u64;
                    let min_required_max = budget + 8192;
                    if current_max.map_or(true, |m| m <= budget) {
                        tracing::info!(
                            "[Gemini-Wrap] Bumping maxOutputTokens from {:?} to {} to satisfy thinkingBudget ({})",
                            current_max, min_required_max, budget
                        );
                        gen_config.insert("maxOutputTokens".to_string(), json!(min_required_max));
                    }
                }
            }
        }

        // [FIX] Cap maxOutputTokens to model-specific limits.
        // gemini-3-flash supports max 65536; gemini-cli sends 131072 which causes 400 INVALID_ARGUMENT.
        if let Some(max_tokens) = gen_config.get("maxOutputTokens").and_then(|v| v.as_u64()) {
            let model_max = if final_model_name.contains("gemini-3-flash") {
                65536u64
            } else if final_model_name.contains("gemini-2.5-flash") {
                65536u64
            } else {
                131072u64 // default reasonable cap
            };
            if max_tokens > model_max {
                tracing::debug!(
                    "[Gemini-Wrap] Capped maxOutputTokens from {} to {} for model {}",
                    max_tokens, model_max, final_model_name
                );
                gen_config.insert("maxOutputTokens".to_string(), json!(model_max));
            }
        }
    }

    // [NEW] 按模型对 maxOutputTokens 进行三层限额 (Dynamic > Static Default > 65535)
    // 修复: gemini-cli 等客户端发送的 131072 超过部分模型支持的上限，导致 v1internal 返回 400 INVALID_ARGUMENT
    {
        let final_cap = crate::proxy::model_specs::get_max_output_tokens(final_model_name, token);
        let gen_config = inner_request
            .as_object_mut()
            .unwrap()
            .entry("generationConfig")
            .or_insert(serde_json::json!({}))
            .as_object_mut()
            .unwrap();
        if let Some(current) = gen_config.get("maxOutputTokens").and_then(|v| v.as_u64()) {
            if current > final_cap {
                tracing::debug!(
                    "[Gemini-Wrap] Capped maxOutputTokens from {} to {} for model {}",
                    current,
                    final_cap,
                    final_model_name
                );
                gen_config.insert("maxOutputTokens".to_string(), serde_json::json!(final_cap));
            }
        }
    }

    // This caused upstream to return empty/invalid responses, leading to 'NoneType' object has no attribute 'strip' in Python clients.
    // relying on upstream defaults or user provided values is safer.

    // 提取 tools 列表以进行联网探测 (Gemini 风格可能是嵌套的)
    let tools_val: Option<Vec<Value>> = inner_request
        .get("tools")
        .and_then(|t| t.as_array())
        .map(|arr| arr.clone());

    // [FIX] Extract OpenAI-compatible image parameters from root (for gemini-3-pro-image)
    let size = body.get("size").and_then(|v| v.as_str());
    let quality = body.get("quality").and_then(|v| v.as_str());
    let image_size = body.get("imageSize").and_then(|v| v.as_str()); // [NEW] Direct imageSize support

    // Use shared grounding/config logic
    let config = crate::proxy::mappers::common_utils::resolve_request_config(
        original_model,
        final_model_name,
        &tools_val,
        size,       // [FIX] Pass size parameter
        quality,    // [FIX] Pass quality parameter
        image_size, // [NEW] Pass direct imageSize parameter
        Some(body), // [NEW] Pass request body for imageConfig parsing
    );

    // Clean tool declarations (remove forbidden Schema fields like multipleOf, and remove redundant search decls)
    if let Some(tools) = inner_request.get_mut("tools") {
        if let Some(tools_arr) = tools.as_array_mut() {
            for tool in tools_arr {
                if let Some(decls) = tool.get_mut("functionDeclarations") {
                    if let Some(decls_arr) = decls.as_array_mut() {
                        // 1. 过滤掉联网关键字函数
                        decls_arr.retain(|decl| {
                            if let Some(name) = decl.get("name").and_then(|v| v.as_str()) {
                                if name == "web_search" || name == "google_search" {
                                    return false;
                                }
                            }
                            true
                        });

                        // 2. 清洗剩余 Schema
                        // [FIX] Gemini CLI 使用 parametersJsonSchema，而标准 Gemini API 使用 parameters
                        // 需要将 parametersJsonSchema 重命名为 parameters
                        for decl in decls_arr {
                            // 检测并转换字段名
                            if let Some(decl_obj) = decl.as_object_mut() {
                                // 如果存在 parametersJsonSchema，将其重命名为 parameters
                                if let Some(params_json_schema) =
                                    decl_obj.remove("parametersJsonSchema")
                                {
                                    let mut params = params_json_schema;
                                    crate::proxy::common::json_schema::clean_json_schema(
                                        &mut params,
                                    );
                                    decl_obj.insert("parameters".to_string(), params);
                                } else if let Some(params) = decl_obj.get_mut("parameters") {
                                    // 标准 parameters 字段
                                    crate::proxy::common::json_schema::clean_json_schema(params);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    tracing::debug!(
        "[Debug] Gemini Wrap: original='{}', mapped='{}', final='{}', type='{}'",
        original_model,
        final_model_name,
        config.final_model,
        config.request_type
    );

    // Inject googleSearch tool if needed
    if config.inject_google_search {
        // [NEW] 阶段 7.3: 如果是 WebSearch 类型，注入官方特定属性 (maxResultCount: 5)
        if config.request_type == "web_search" {
            if let Some(obj) = inner_request.as_object_mut() {
                let tools_entry = obj.entry("tools").or_insert_with(|| json!([]));
                if let Some(tools_arr) = tools_entry.as_array_mut() {
                    tools_arr.push(json!({
                        "googleSearch": {
                            "enhancedContent": {
                                "imageSearch": {
                                    "maxResultCount": 5
                                }
                            }
                        }
                    }));
                }
            }
        } else {
            crate::proxy::mappers::common_utils::inject_google_search_tool(
                &mut inner_request,
                Some(&config.final_model),
            );
        }
    }

    // Inject imageConfig if present (for image generation models)
    if let Some(image_config) = config.image_config {
        if let Some(obj) = inner_request.as_object_mut() {
            // 1. Filter tools: remove tools for image gen
            obj.remove("tools");

            // 2. Remove systemInstruction (image generation does not support system prompts)
            obj.remove("systemInstruction");

            // [FIX] Ensure 'role' field exists for all contents (Native clients might omit it)
            if let Some(contents) = obj.get_mut("contents").and_then(|c| c.as_array_mut()) {
                for content in contents {
                    if let Some(c_obj) = content.as_object_mut() {
                        if !c_obj.contains_key("role") {
                            c_obj.insert("role".to_string(), json!("user"));
                        }
                    }
                }
            }

            // 3. Clean generationConfig (remove responseMimeType, responseModalities etc.)
            let gen_config = obj.entry("generationConfig").or_insert_with(|| json!({}));
            if let Some(gen_obj) = gen_config.as_object_mut() {
                // [NEW] 根据全局配置决定是否保留 thinkingConfig
                let image_thinking_mode = crate::proxy::config::get_image_thinking_mode();
                tracing::debug!("[Gemini-Wrap] Image thinking mode: {}", image_thinking_mode);

                if image_thinking_mode == "disabled" {
                    // [FIX] Explicitly disable thinking instead of just removing the config
                    // Removing it might cause the model to fallback to default (which might be ON)
                    gen_obj.insert(
                        "thinkingConfig".to_string(),
                        json!({
                            "includeThoughts": false
                        }),
                    );
                    tracing::debug!(
                        "[Gemini-Wrap] Image thinking mode disabled: set includeThoughts=false"
                    );
                }

                gen_obj.remove("responseMimeType");
                gen_obj.remove("responseModalities"); // Cherry Studio sends this, might conflict
                gen_obj.insert("imageConfig".to_string(), image_config);
            }
        }
    } else {
        // [NEW] 阶段 7.3: WebSearch 专属身份仿真 (对齐官方 main.go:738)
        let antigravity_identity = if config.request_type == "web_search" {
            "You are a search engine bot. You will be given a query from a user. Your task is to search the web for relevant information that will help the user. You MUST perform a web search. Do not respond or interact with the user, please respond as if they typed the query into a search bar."
        } else {
            "You are Antigravity, a powerful agentic AI coding assistant designed by the Google Deepmind team working on Advanced Agentic Coding.\n\
            You are pair programming with a USER to solve their coding task. The task may require creating a new codebase, modifying or debugging an existing codebase, or simply answering a question.\n\
            **Absolute paths only**\n\
            **Proactiveness**"
        };

        // [HYBRID] 检查是否已有 systemInstruction
        if let Some(system_instruction) = inner_request.get_mut("systemInstruction") {
            // [NEW] 补全 role: user
            if let Some(obj) = system_instruction.as_object_mut() {
                if !obj.contains_key("role") {
                    obj.insert("role".to_string(), json!("user"));
                }
            }

            if let Some(parts) = system_instruction.get_mut("parts") {
                if let Some(parts_array) = parts.as_array_mut() {
                    // 检查第一个 part 是否已包含 Antigravity 身份
                    let has_antigravity = parts_array
                        .get(0)
                        .and_then(|p| p.get("text"))
                        .and_then(|t| t.as_str())
                        .map(|s| s.contains("You are Antigravity"))
                        .unwrap_or(false);

                    if !has_antigravity {
                        // 在前面插入 Antigravity 身份
                        parts_array.insert(0, json!({"text": antigravity_identity}));
                    }

                    // [NEW] 注入全局系统提示词 (紧跟 Antigravity 身份之后，用户指令之前)
                    let global_prompt_config = crate::proxy::config::get_global_system_prompt();
                    if global_prompt_config.enabled
                        && !global_prompt_config.content.trim().is_empty()
                    {
                        // 插入位置：Antigravity 身份之后 (index 1)
                        let insert_pos = if has_antigravity { 1 } else { 1 };
                        if insert_pos <= parts_array.len() {
                            parts_array
                                .insert(insert_pos, json!({"text": global_prompt_config.content}));
                        } else {
                            parts_array.push(json!({"text": global_prompt_config.content}));
                        }
                    }
                }
            }
        } else {
            // 没有 systemInstruction,创建一个新的
            let mut parts = vec![json!({"text": antigravity_identity})];
            // [NEW] 注入全局系统提示词
            let global_prompt_config = crate::proxy::config::get_global_system_prompt();
            if global_prompt_config.enabled && !global_prompt_config.content.trim().is_empty() {
                parts.push(json!({"text": global_prompt_config.content}));
            }
            inner_request["systemInstruction"] = json!({
                "role": "user",
                "parts": parts
            });
        }
    }

    // [ADDED v4.1.24] 扩展 toolConfig 到 VALIDATED 模式
    if inner_request.get("tools").is_some() && !inner_request.get("toolConfig").is_some() {
        inner_request["toolConfig"] = json!({
            "functionCallingConfig": { "mode": "VALIDATED" }
        });
    }

    // [ADDED v4.1.24] 注入基于账号的稳定 sessionId
    if let Some(account_id_str) = account_id {
        inner_request["sessionId"] = json!(crate::proxy::common::session::derive_session_id(
            account_id_str
        ));
    }

    let sid = session_id.unwrap_or("default");

    // [NEW] 1. 深度对齐 requestId 格式 (官方格式: agent/{timestamp_ms}/{random_hex_8bytes})
    // 每次请求生成完全唯一的 ID，避免重试时的幂等性冲突导致 Google 返回旧缓存
    let timestamp_ms = chrono::Utc::now().timestamp_millis();
    let random_hex = &uuid::Uuid::new_v4().simple().to_string()[..8]; // 移除对外部 hex crate 的依赖
    let official_request_id = format!("agent/{}/{}", timestamp_ms, random_hex);

    // [NEW] 2. 动态 userAgent 仿真 (支持 jetski)
    // 根据账号属性或域名判断。Go Worker 中企业/GCP 账号通常使用 jetski 指纹。
    let is_enterprise = if let Some(t) = token {
        !t.email.ends_with("@gmail.com") && !t.email.ends_with("@googlemail.com")
    } else {
        false
    };

    // [NEW] 阶段 7.2: 动态 IDEType 指纹对齐
    let official_ide_type = if is_enterprise {
        "JETSKI"
    } else {
        "ANTIGRAVITY"
    };
    let official_user_agent = if is_enterprise {
        "jetski"
    } else {
        "antigravity"
    };

    // [NEW] 如果是 loadCodeAssist 请求，注入 metadata 字段对齐官方
    if final_model_name == "loadCodeAssist" || inner_request.get("metadata").is_some() {
        let metadata = inner_request
            .as_object_mut()
            .unwrap()
            .entry("metadata")
            .or_insert(json!({}));
        if let Some(m_obj) = metadata.as_object_mut() {
            if m_obj.get("ideType").is_none() {
                m_obj.insert("ideType".to_string(), json!(official_ide_type));
            }
        }
    }

    // [NEW] 3. 条件注入 enabledCreditTypes
    // 这是官方 Worker 极高权重的一个指纹字段。
    // 只有在非图像生成请求（即 agent 类型请求）时注入，避免图像生成场景出现 Credit 判定异常。
    // 特别注意：这是 Google 识别“官方客户端”的重要凭证之一。
    let is_agent_request = config.request_type != "image_gen";

    // [CACHE] 重建 inner_request 字段顺序——稳定前缀在前，动态内容在后
    // 遵循 Google 官方建议："将较大且常见的内容放置在提示的开头"
    // systemInstruction (~稳定的系统提示词) → tools → toolConfig → generationConfig → contents (动态)
    let mut reordered_inner = json!({});
    // 1. systemInstruction (稳定)
    if let Some(si) = inner_request.get("systemInstruction") {
        reordered_inner["systemInstruction"] = si.clone();
    }
    // 2. tools (稳定)
    if let Some(tools) = inner_request.get("tools") {
        reordered_inner["tools"] = tools.clone();
    }
    // 3. toolConfig (稳定，与 tools 共生)
    if let Some(tc) = inner_request.get("toolConfig") {
        reordered_inner["toolConfig"] = tc.clone();
    }
    // 4. generationConfig (稳定)
    if let Some(gc) = inner_request.get("generationConfig") {
        reordered_inner["generationConfig"] = gc.clone();
    }
    // 5. safetySettings (恒定)
    if let Some(ss) = inner_request.get("safetySettings") {
        reordered_inner["safetySettings"] = ss.clone();
    }
    // 6. sessionId (稳定，基于 account hash)
    if let Some(sid) = inner_request.get("sessionId") {
        reordered_inner["sessionId"] = sid.clone();
    }
    // 7. contents (动态 — 对话历史，每次追加，放在最后！)
    reordered_inner["contents"] = inner_request.get("contents").cloned().unwrap_or(json!([]));
    // 8. 其他字段 (metadata, cachedContent 等 — 保持原样但覆盖已有)
    for (k, v) in inner_request.as_object().iter().flat_map(|o| o.iter()) {
        if !reordered_inner
            .as_object()
            .map(|o| o.contains_key(k))
            .unwrap_or(false)
        {
            reordered_inner[k] = v.clone();
        }
    }

    let mut final_request_obj = json!({
        "project": project_id,
        "request": reordered_inner,
        "model": config.final_model,
        "userAgent": official_user_agent,
        "requestType": if is_agent_request { "agent" } else { "image_gen" },
        // [CACHE] requestId 移到末尾避免动态值破坏前缀字节一致性
        "requestId": official_request_id,
    });

    if is_agent_request {
        if let Some(obj) = final_request_obj.as_object_mut() {
            // 强制注入 Google One AI 信用额度支持标号
            obj.insert("enabledCreditTypes".to_string(), json!(["GOOGLE_ONE_AI"]));
        }
    }

    final_request_obj
}

#[cfg(test)]
mod test_fixes {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_wrap_request_with_signature() {
        let session_id = "test-session-sig";
        let signature = "test-signature-must-be-longer-than-fifty-characters-to-be-cached-by-signature-cache-12345"; // > 50 chars
        crate::proxy::SignatureCache::global().cache_session_signature(
            session_id,
            signature.to_string(),
            1,
        );

        let body = json!({
            "model": "gemini-pro",
            "contents": [{
                "role": "user",
                "parts": [{
                    "functionCall": {
                        "name": "get_weather",
                        "args": {"location": "London"}
                    }
                }]
            }]
        });

        let result = wrap_request(&body, "proj", "gemini-pro", None, Some(session_id), None);
        let injected_sig = result["request"]["contents"][0]["parts"][0]["thoughtSignature"]
            .as_str()
            .unwrap();
        assert_eq!(injected_sig, signature);
    }

    #[test]
    fn test_wrap_request_with_snake_case_signature() {
        let body = json!({
            "model": "gemini-pro",
            "contents": [{
                "role": "user",
                "parts": [{
                    "functionCall": {
                        "name": "get_weather",
                        "args": {"location": "London"}
                    },
                    "thought_signature": "client-sent-signature-value-12345"
                }]
            }]
        });

        let result = wrap_request(&body, "proj", "gemini-pro", None, None, None);
        let part = &result["request"]["contents"][0]["parts"][0];
        assert_eq!(
            part["thoughtSignature"].as_str(),
            Some("client-sent-signature-value-12345")
        );
        assert_eq!(
            part["thought_signature"].as_str(),
            Some("client-sent-signature-value-12345")
        );
    }
}

/// 解包响应（提取 response 字段）
pub fn unwrap_response(response: &Value) -> Value {
    response.get("response").unwrap_or(response).clone()
}

/// [NEW v3.3.18] 为 Claude 模型的 Gemini 响应自动注入 Tool ID
///
/// 目点是为了让客户端（如 OpenCode/Vercel AI SDK）能感知到 ID，
/// 并在下一轮对话中原样带回，从而满足 Google v1internal 对 Claude 模型的校验。
pub fn inject_ids_to_response(response: &mut Value, model_name: &str) {
    if !model_name.to_lowercase().contains("claude") {
        return;
    }

    if let Some(candidates) = response
        .get_mut("candidates")
        .and_then(|c| c.as_array_mut())
    {
        for candidate in candidates {
            if let Some(parts) = candidate
                .get_mut("content")
                .and_then(|c| c.get_mut("parts"))
                .and_then(|p| p.as_array_mut())
            {
                let mut name_counters: std::collections::HashMap<String, usize> =
                    std::collections::HashMap::new();
                for part in parts {
                    if let Some(fc) = part.get_mut("functionCall").and_then(|f| f.as_object_mut()) {
                        if fc.get("id").is_none() {
                            let name = fc.get("name").and_then(|n| n.as_str()).unwrap_or("unknown");
                            let count = name_counters.entry(name.to_string()).or_insert(0);
                            let call_id = format!("call_{}_{}", name, count);
                            *count += 1;

                            fc.insert("id".to_string(), json!(call_id));
                            tracing::debug!("[Gemini-Wrap] Response stage: Injected synthetic call_id '{}' for client", call_id);
                        }
                    }
                }
            }
        }
    }
}

const INTERNAL_BACKGROUND_TASK: &str = "gemini-2.5-flash-lite";
const CONTEXT_SUMMARY_PROMPT: &str = r#"You are a context compression specialist. Your task is to create a structured XML snapshot of the conversation history.

This snapshot will become the Agent's ONLY memory of the past. All key details, plans, errors, and user instructions MUST be preserved.

First, think through the entire history in a private <scratchpad>. Review the user's overall goal, the agent's actions, tool outputs, file modifications, and any unresolved issues. Identify every piece of information critical for future actions.

After reasoning, generate the final <state_snapshot> XML object. Information must be extremely dense. Omit any irrelevant conversational filler.

The structure MUST be as follows:

<state_snapshot>
  <overall_goal>
    <!-- Describe the user's high-level goal in one concise sentence -->
  </overall_goal>

  <technical_context>
    <!-- Tech stack: frameworks, languages, toolchain, dependency versions -->
  </technical_context>

  <file_system_state>
    <!-- List files that were created, read, modified, or deleted. Note their status -->
  </file_system_state>

  <code_changes>
    <!-- Key code snippets (preserve function signatures and important logic) -->
  </code_changes>

  <debugging_history>
    <!-- List all errors encountered, with stack traces, and how they were fixed -->
  </debugging_history>

  <current_plan>
    <!-- Step-by-step plan. Mark completed steps -->
  </current_plan>

  <user_preferences>
    <!-- User's work preferences for this project (test commands, code style, etc.) -->
  </user_preferences>

  <key_decisions>
    <!-- Critical architectural decisions and design choices -->
  </key_decisions>

  <latest_thinking_signature>
    <!-- [CRITICAL] Preserve the last valid thinking signature -->
    <!-- Format: base64-encoded signature string -->
    <!-- This MUST be copied exactly as-is, no modifications -->
  </latest_thinking_signature>
</state_snapshot>

**IMPORTANT**:
1. Code snippets must be complete, including function signatures and key logic
2. Error messages must be preserved verbatim, including line numbers and stacks
3. File paths must use absolute paths
4. The thinking signature must be copied exactly, no modifications
"#;

async fn try_compress_gemini_with_summary(
    original_request: &Value,
    trace_id: &str,
    token_manager: &std::sync::Arc<crate::proxy::TokenManager>,
    session_id_str: &str,
    project_id: &str,
    account_id: &str,
) -> Result<Value, String> {
    info!(
        "[{}] [Layer-3] [Gemini] Starting context compression with XML summary",
        trace_id
    );

    let last_signature =
        crate::proxy::mappers::context_manager::ContextManager::extract_last_openai_valid_signature(
            session_id_str,
        );

    let signature_instruction = if let Some(ref sig) = last_signature {
        format!("\n\n**CRITICAL**: The last thinking signature is:\n```\n{}\n```\nYou MUST include this EXACTLY in the <latest_thinking_signature> section.", sig)
    } else {
        "\n\n**Note**: No thinking signature found in history. Leave <latest_thinking_signature> empty.".to_string()
    };

    let mut summary_messages = original_request
        .get("contents")
        .and_then(|c| c.as_array())
        .cloned()
        .unwrap_or_default();

    summary_messages.push(json!({
        "role": "user",
        "parts": [{
            "text": format!("{}{}", CONTEXT_SUMMARY_PROMPT, signature_instruction)
        }]
    }));

    let mut summary_request = original_request.clone();
    if let Some(obj) = summary_request.as_object_mut() {
        obj.insert("contents".to_string(), json!(summary_messages));
        obj.insert("model".to_string(), json!(INTERNAL_BACKGROUND_TASK));
        obj.remove("stream");
    }

    debug!(
        "[{}] [Layer-3] [Gemini] Calling {} for summary generation",
        trace_id, INTERNAL_BACKGROUND_TASK
    );

    let token_obj = token_manager.get_token_by_id(account_id);
    let access_token = token_obj
        .as_ref()
        .map(|t| t.access_token.clone())
        .ok_or_else(|| "No access token available".to_string())?;

    let wrapped_summary_body = wrap_request(
        &summary_request,
        project_id,
        INTERNAL_BACKGROUND_TASK,
        Some(account_id),
        Some(session_id_str),
        token_obj.as_ref(),
    );

    let upstream_url = format!(
        "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal/projects/{}/locations/global/models/{}:generateContent",
        project_id, INTERNAL_BACKGROUND_TASK
    );

    let response = reqwest::Client::new()
        .post(&upstream_url)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Content-Type", "application/json")
        .json(&wrapped_summary_body)
        .send()
        .await
        .map_err(|e| format!("API call failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!(
            "API returned {}: {}",
            response.status(),
            response.text().await.unwrap_or_default()
        ));
    }

    let gemini_response: Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    let xml_summary = gemini_response
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.get(0))
        .and_then(|p| p.get("text"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "Failed to extract text from response".to_string())?;

    info!(
        "[{}] [Layer-3] [Gemini] Generated XML summary (len: {} chars)",
        trace_id,
        xml_summary.len()
    );

    let forked_messages = vec![
        json!({
            "role": "user",
            "parts": [{
                "text": format!("Context has been compressed. Here is the structured summary of our conversation history:\n\n{}", xml_summary)
            }]
        }),
        json!({
            "role": "model",
            "parts": [{
                "text": "I have reviewed the compressed context summary. I understand the current state and will continue from here."
            }]
        }),
    ];

    let mut forked_request = original_request.clone();
    if let Some(obj) = forked_request.as_object_mut() {
        let mut final_msgs = forked_messages;
        if let Some(last_msg) = original_request
            .get("contents")
            .and_then(|c| c.as_array())
            .and_then(|a| a.last())
        {
            if last_msg.get("role").and_then(|r| r.as_str()) == Some("user") {
                let has_summary_inst = last_msg
                    .get("parts")
                    .and_then(|p| p.as_array())
                    .map(|arr| {
                        arr.iter().any(|part| {
                            part.get("text")
                                .and_then(|t| t.as_str())
                                .map(|t| t.contains(CONTEXT_SUMMARY_PROMPT))
                                .unwrap_or(false)
                        })
                    })
                    .unwrap_or(false);
                if !has_summary_inst {
                    final_msgs.push(last_msg.clone());
                }
            }
        }
        obj.insert("contents".to_string(), json!(final_msgs));
    }

    Ok(forked_request)
}

#[allow(dead_code)]
pub fn wrap_request(
    body: &Value,
    project_id: &str,
    mapped_model: &str,
    account_id: Option<&str>,
    session_id: Option<&str>,
    token: Option<&crate::proxy::token_manager::ProxyToken>,
) -> Value {
    wrap_request_v2(
        body,
        project_id,
        mapped_model,
        account_id,
        session_id,
        token,
        None,
    )
}
static TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_wrap_request() {
        let body = json!({
            "model": "gemini-2.5-flash",
            "contents": [{"role": "user", "parts": [{"text": "Hi"}]}]
        });

        let result = wrap_request(&body, "test-project", "gemini-2.5-flash", None, None, None);
        assert_eq!(result["project"], "test-project");
        assert_eq!(result["model"], "gemini-2.5-flash");
        assert!(result["requestId"].as_str().unwrap().starts_with("agent/"));
    }

    #[test]
    fn test_unwrap_response() {
        let wrapped = json!({
            "response": {
                "candidates": [{"content": {"parts": [{"text": "Hello"}]}}]
            }
        });

        let result = unwrap_response(&wrapped);
        assert!(result.get("candidates").is_some());
        assert!(result.get("response").is_none());
    }

    #[test]
    fn test_antigravity_identity_injection_with_role() {
        let body = json!({
            "model": "gemini-pro",
            "messages": []
        });

        let result = wrap_request(&body, "test-proj", "gemini-pro", None, None, None);

        // 验证 systemInstruction
        let sys = result
            .get("request")
            .unwrap()
            .get("systemInstruction")
            .unwrap();
    }

    #[test]
    fn test_gemini_flash_thinking_budget_capping() {
        // Ensure default config (Auto mode)
        crate::proxy::config::update_thinking_budget_config(
            crate::proxy::config::ThinkingBudgetConfig::default(),
        );

        let body = json!({
            "model": "gemini-2.0-flash-thinking-exp",
            "generationConfig": {
                "thinkingConfig": {
                    "includeThoughts": true,
                    "thinkingBudget": 32000
                }
            }
        });

        let _lock = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        crate::proxy::config::update_thinking_budget_config(
            crate::proxy::config::ThinkingBudgetConfig::default(),
        );

        // Test with Flash model
        let result = wrap_request(
            &body,
            "test-proj",
            "gemini-2.0-flash-thinking-exp",
            None,
            None,
            None,
        );
        let req = result.get("request").unwrap();
        let gen_config = req.get("generationConfig").unwrap();
        let budget = gen_config["thinkingConfig"]["thinkingBudget"]
            .as_u64()
            .unwrap();

        // Should be capped at 24576
        assert_eq!(budget, 24576);

        // Test with Pro model (should NOT cap)
        let body_pro = json!({
            "model": "gemini-2.0-pro-exp",
            "generationConfig": {
                "thinkingConfig": {
                    "includeThoughts": true,
                    "thinkingBudget": 32000
                }
            }
        });
        let result_pro = wrap_request(
            &body_pro,
            "test-proj",
            "gemini-2.0-pro-exp",
            None,
            None,
            None,
        );
        let budget_pro = result_pro["request"]["generationConfig"]["thinkingConfig"]
            ["thinkingBudget"]
            .as_u64()
            .unwrap();
        // [FIX #1592] Pro models now also capped to 24576 in wrap_request logic
        assert_eq!(budget_pro, 24576);
    }

    #[test]
    fn test_image_thinking_mode_disabled() {
        // 1. Set global mode to disabled
        crate::proxy::config::update_image_thinking_mode(Some("disabled".to_string()));

        // 2. Create a request for an image model (which triggers the image logic)
        // Note: resolve_request_config needs to return image_config for the logic to trigger
        // So we use a model name that resolves to image_gen
        let body = json!({
            "model": "gemini-3-pro-image-2k",
            "contents": [{"role": "user", "parts": [{"text": "Draw a cat"}]}]
        });

        let result = wrap_request(
            &body,
            "test-proj",
            "gemini-3-pro-image-2k",
            None,
            None,
            None,
        );
        let req = result.get("request").unwrap();
        let gen_config = req.get("generationConfig").unwrap();

        // 3. Verify thinkingConfig has includeThoughts: false
        let thinking_config = gen_config.get("thinkingConfig").unwrap();
        assert_eq!(thinking_config["includeThoughts"], false);

        // 4. Reset global mode
        crate::proxy::config::update_image_thinking_mode(Some("enabled".to_string()));
    }

    #[test]
    fn test_user_instruction_preservation() {
        let body = json!({
            "model": "gemini-pro",
            "systemInstruction": {
                "role": "user",
                "parts": [{"text": "User custom prompt"}]
            }
        });

        let result = wrap_request(&body, "test-proj", "gemini-pro", None, None, None);
        let sys = result
            .get("request")
            .unwrap()
            .get("systemInstruction")
            .unwrap();
        let parts = sys.get("parts").unwrap().as_array().unwrap();

        // Should have 2 parts: Antigravity + User
        assert_eq!(parts.len(), 2);
        assert!(parts[0]
            .get("text")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("You are Antigravity"));
        assert_eq!(
            parts[1].get("text").unwrap().as_str().unwrap(),
            "User custom prompt"
        );
    }

    #[test]
    fn test_duplicate_prevention() {
        let body = json!({
            "model": "gemini-pro",
            "systemInstruction": {
                "parts": [{"text": "You are Antigravity..."}]
            }
        });

        let result = wrap_request(&body, "test-proj", "gemini-pro", None, None, None);
        let sys = result
            .get("request")
            .unwrap()
            .get("systemInstruction")
            .unwrap();
        let parts = sys.get("parts").unwrap().as_array().unwrap();

        // Should NOT inject duplicate, so only 1 part remains
        assert_eq!(parts.len(), 1);
    }

    #[test]
    fn test_image_generation_with_reference_images() {
        // Create 14 reference images + 1 text prompt
        let mut parts = Vec::new();
        parts.push(json!({"text": "Generate a variation"}));

        for _ in 0..14 {
            parts.push(json!({
                "inlineData": {
                    "mimeType": "image/jpeg",
                    "data": "base64data..."
                }
            }));
        }

        let body = json!({
            "model": "gemini-3-pro-image",
            "contents": [{"parts": parts}]
        });

        let result = wrap_request(&body, "test-proj", "gemini-3-pro-image", None, None, None);

        let request = result.get("request").unwrap();
        let contents = request.get("contents").unwrap().as_array().unwrap();
        let result_parts = contents[0].get("parts").unwrap().as_array().unwrap();

        // Verify all 15 parts (1 text + 14 images) are preserved
        assert_eq!(result_parts.len(), 15);
    }

    #[test]
    fn test_gemini_pro_thinking_budget_processing() {
        let _lock = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        // Update global config to Custom mode to verify logic execution
        use crate::proxy::config::{
            update_thinking_budget_config, ThinkingBudgetConfig, ThinkingBudgetMode,
        };

        // Save old config (optional, but good practice if tests ran in parallel, but here it's fine)
        update_thinking_budget_config(ThinkingBudgetConfig {
            mode: ThinkingBudgetMode::Custom,
            custom_value: 1024, // Distinct value
            effort: None,
        });

        let body = json!({
            "model": "gemini-3-pro-preview",
            "generationConfig": {
                "thinkingConfig": {
                    "includeThoughts": true,
                    "thinkingBudget": 32000
                }
            }
        });

        // Test with Pro model
        let result = wrap_request(&body, "test-proj", "gemini-3-pro-preview", None, None, None);
        let req = result.get("request").unwrap();
        let gen_config = req.get("generationConfig").unwrap();

        let budget = gen_config["thinkingConfig"]["thinkingBudget"]
            .as_u64()
            .unwrap();

        // If logic executes, it sees Custom mode and sets 1024
        // If logic skipped, it keeps 32000
        assert_eq!(
            budget, 1024,
            "Budget should be overridden to 1024 by custom config, proving logic execution"
        );

        // Restore default (Auto 24576)
        update_thinking_budget_config(ThinkingBudgetConfig::default());
    }

    #[cfg(test)]
    mod test_v4_fixes {
        use super::*;
        use serde_json::json;

        #[test]
        fn test_claude_no_root_thinking_injection() {
            let _lock = super::TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
            // 验证 Claude 模型不会在根目录注入 thinking，而是注入到 generationConfig.thinkingConfig
            // 并且 budget 默认为 16000

            // 使用 Auto 模式避免干扰
            crate::proxy::config::update_thinking_budget_config(
                crate::proxy::config::ThinkingBudgetConfig {
                    mode: crate::proxy::config::ThinkingBudgetMode::Auto,
                    custom_value: 0,
                    effort: None,
                },
            );

            let body = json!({
                "model": "claude-3-7-sonnet-thinking",
                "messages": [{"role": "user", "content": "hi"}]
            });

            let result = wrap_request(
                &body,
                "proj",
                "claude-3-7-sonnet-thinking",
                None,
                None,
                None,
            );
            let req = result.get("request").unwrap();

            // 1. 确保根目录没有 thinking
            assert!(
                req.get("thinking").is_none(),
                "Root level 'thinking' should NOT be present"
            );

            // 2. 确保 generationConfig.thinkingConfig 存在
            let gen_config = req
                .get("generationConfig")
                .expect("generationConfig should be present");
            let thinking_config = gen_config
                .get("thinkingConfig")
                .expect("thinkingConfig should be injected");

            // 3. 验证 Claude 默认预算为 16000
            let budget = thinking_config["thinkingBudget"]
                .as_u64()
                .expect("thinkingBudget should be a number");
            assert_eq!(
                budget, 16000,
                "Claude default thinking budget should be 16000"
            );
        }

        #[test]
        fn test_gemini_thinking_injection_default() {
            let _lock = super::TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
            crate::proxy::config::update_thinking_budget_config(
                crate::proxy::config::ThinkingBudgetConfig::default(),
            );
            // 验证 Gemini 模型注入默认预算 24576
            let body = json!({
                "model": "gemini-2.0-flash-thinking-exp",
                "contents": [{"role": "user", "parts": [{"text": "hi"}]}]
            });

            let result = wrap_request(
                &body,
                "proj",
                "gemini-2.0-flash-thinking-exp",
                None,
                None,
                None,
            );
            let req = result.get("request").unwrap();
            let gen_config = req.get("generationConfig").unwrap();
            let thinking_config = gen_config.get("thinkingConfig").unwrap();

            let budget = thinking_config["thinkingBudget"].as_u64().unwrap();
            assert_eq!(
                budget, 24576,
                "Gemini default thinking budget should be 24576"
            );
        }
    }

    #[test]
    fn test_gemini_pro_auto_inject_thinking() {
        // Reset thinking budget to auto mode at the start to avoid interference from parallel tests
        crate::proxy::config::update_thinking_budget_config(
            crate::proxy::config::ThinkingBudgetConfig {
                mode: crate::proxy::config::ThinkingBudgetMode::Auto,
                custom_value: 24576,
                effort: None,
            },
        );

        // Request WITHOUT thinkingConfig
        let body = json!({
            "model": "gemini-3-pro-preview",
            // No generationConfig or empty one
            "generationConfig": {}
        });

        // Test with Pro-preview model (should NOT auto-inject to avoid 400)
        let result = wrap_request(&body, "test-proj", "gemini-3-pro-preview", None, None, None);
        let req = result.get("request").unwrap();
        let gen_config = req.get("generationConfig").unwrap();

        // Should NOT have auto-injected thinkingConfig
        assert!(
            gen_config.get("thinkingConfig").is_none(),
            "Should NOT auto-inject thinkingConfig for gemini-3-pro-preview to avoid 400 error"
        );

        // Test with standard gemini-3-pro (non-preview)
        let body_std = json!({
            "model": "gemini-3-pro",
            "generationConfig": {}
        });
        let result_std = wrap_request(&body_std, "test-proj", "gemini-3-pro", None, None, None);
        let gen_config_std = result_std
            .get("request")
            .unwrap()
            .get("generationConfig")
            .unwrap();

        assert!(
            gen_config_std.get("thinkingConfig").is_some(),
            "Should still auto-inject thinkingConfig for standard gemini-3-pro"
        );
    }

    #[test]
    fn test_openai_image_params_support() {
        // Test Case 1: Standard Size + Quality (HD/4K)
        let body_1 = json!({
            "model": "gemini-3-pro-image",
            "size": "1920x1080",
            "quality": "hd",
            "prompt": "Test"
        });

        let result_1 = wrap_request(&body_1, "test-proj", "gemini-3-pro-image", None, None, None);
        let req_1 = result_1.get("request").unwrap();
        let gen_config_1 = req_1.get("generationConfig").unwrap();
        let image_config_1 = gen_config_1.get("imageConfig").unwrap();

        assert_eq!(image_config_1["aspectRatio"], "16:9");
        assert_eq!(image_config_1["imageSize"], "4K");

        // Test Case 2: Aspect Ratio String + Standard Quality
        let body_2 = json!({
            "model": "gemini-3-pro-image",
            "size": "1:1",
            "quality": "standard",
             "prompt": "Test"
        });

        let result_2 = wrap_request(&body_2, "test-proj", "gemini-3-pro-image", None, None, None);
        let req_2 = result_2.get("request").unwrap();
        let image_config_2 = req_2["generationConfig"]["imageConfig"]
            .as_object()
            .unwrap();

        assert_eq!(image_config_2["aspectRatio"], "1:1");
        assert_eq!(image_config_2["imageSize"], "1K");
    }

    #[test]
    fn test_mixed_tools_injection_gemini_native() {
        // 验证 Gemini Native 协议在 Gemini 2.0+ 下支持混合工具
        let body = json!({
            "contents": [{"parts": [{"text": "Hello"}]}],
            "tools": [{"functionDeclarations": [{"name": "get_weather", "parameters": {"type": "OBJECT", "properties": {"location": {"type": "STRING"}}}}]}],
            "generationConfig": {}
        });

        // 模拟 -online 触发的 RequestConfig
        use crate::proxy::mappers::common_utils::resolve_request_config;
        let _config =
            resolve_request_config("-online", "gemini-2.0-flash", &None, None, None, None, None);

        // 实际上 wrap_request 内部会根据 config.inject_google_search 调用 inject_google_search_tool
        // 但 wrap_request 的签名不直接接受 RequestConfig，它内部逻辑如下：
        // if config.inject_google_search { ... }

        // 我们改为直接测试涉及的 wrap_request 逻辑片段。
        // 由于测试 wrap_request 比较复杂（涉及外部 config），
        // 我们可以直接验证 inject_google_search_tool 在 native 格式下的表现。

        let mut inner_request = body.clone();
        crate::proxy::mappers::common_utils::inject_google_search_tool(
            &mut inner_request,
            Some("gemini-2.0-flash"),
        );

        let tools = inner_request["tools"]
            .as_array()
            .expect("Should have tools");
        let has_functions = tools
            .iter()
            .any(|t| t.get("functionDeclarations").is_some());
        let has_google_search = tools.iter().any(|t| t.get("googleSearch").is_some());

        assert!(has_functions, "Should contain functionDeclarations");
        assert!(
            !has_google_search,
            "Should NOT contain googleSearch due to functionDeclarations presence (v1internal limit)"
        );
    }

    #[test]
    fn test_gemini_wrapper_context_compression() {
        crate::proxy::config::update_global_compression_level("high".to_string(), true);
        let body = json!({
            "contents": [
                {
                    "role": "user",
                    "parts": [{"text": "Hello there! Could you please tell me how to fix this?"}]
                },
                {
                    "role": "model",
                    "parts": [{"text": "Basically, it appears to be a bug."}]
                },
                {
                    "role": "user",
                    "parts": [{"text": "Old message 3. I was wondering if you could help."}]
                },
                {
                    "role": "user",
                    "parts": [
                        {
                            "functionResponse": {
                                "name": "run_test",
                                "response": {
                                    "output": "Progress: 10%\nProgress: 20%\nProgress: 30%\nProgress: 40%\nProgress: 50%\nError: compilation failed"
                                }
                            }
                        }
                    ]
                },
                {
                    "role": "user",
                    "parts": [{"text": "Latest message 1. Please keep this."}]
                },
                {
                    "role": "model",
                    "parts": [{"text": "Latest message 2. Of course!"}]
                }
            ],
            "model": "gemini-2.5-pro"
        });

        let wrapped = wrap_request(&body, "test-proj", "gemini-2.5-pro", None, None, None);
        println!(
            "DEBUG: wrapped = {}",
            serde_json::to_string_pretty(&wrapped).unwrap()
        );
        let contents = wrapped["request"]["contents"].as_array().unwrap();

        let text_1 = contents[0]["parts"][0]["text"].as_str().unwrap();
        assert!(!text_1.contains("please"));
        assert!(!text_1.contains("Could you please"));

        let text_2 = contents[1]["parts"][0]["text"].as_str().unwrap();
        assert!(!text_2.contains("Basically"));

        let tool_resp = contents[3]["parts"][0]["functionResponse"]["response"]["output"]
            .as_str()
            .unwrap();
        assert!(tool_resp.contains("Collapsed"));
        assert!(tool_resp.contains("Error: compilation failed"));

        let text_5 = contents[4]["parts"][0]["text"].as_str().unwrap();
        assert!(text_5.contains("Please"));
    }
}
