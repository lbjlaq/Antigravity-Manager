// Gemini v1internal 包装/解包
use serde_json::{json, Value};
use tracing::{debug, info};

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

    // [PIPELINE] 统一清洗提示词与风控伪 Header（兼容第三方聚合器如 New API 以 Gemini 原生协议转入时的特征残留）
    crate::proxy::mappers::prompt_sanitizer::PromptSanitizer::sanitize_gemini_payload(
        &mut inner_request,
    );

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

    let lower_model = final_model_name.to_lowercase();
    let is_under_v3 = crate::proxy::model_specs::is_gemini_under_v3(final_model_name)
        || crate::proxy::model_specs::is_gemini_under_v3(original_model);
    let force_server_thinking = !is_under_v3
        && crate::proxy::thinking_store::any_model_forces_server_thinking(&[
            final_model_name,
            original_model,
        ]);
    let is_preview = lower_model.contains("preview");
    let should_inject = !is_under_v3
        && (force_server_thinking
            || lower_model.contains("thinking")
            || (crate::proxy::model_specs::is_gemini_v3_or_above(final_model_name) && !is_preview));

    let has_explicit_thinking = inner_request
        .get("generationConfig")
        .and_then(|gc| gc.get("thinkingConfig"))
        .and_then(|tc| tc.get("thinkingBudget"))
        .and_then(|b| b.as_i64())
        .map(|b| b > 0)
        .unwrap_or(false);
    let is_thinking_active = should_inject || has_explicit_thinking;

    if let Some(contents) = inner_request
        .get_mut("contents")
        .and_then(|c| c.as_array_mut())
    {
        let is_google_cloud = final_model_name.starts_with("projects/");
        let can_use_sentinel = !is_google_cloud
            && (should_inject
                || crate::proxy::mappers::common_utils::model_keeps_thinking_without_signature(
                    final_model_name,
                ));

        for (i, content) in contents.iter_mut().enumerate() {
            let role = content.get("role").and_then(|r| r.as_str()).unwrap_or("");
            let is_assistant = role == "model" || role == "assistant";

            let mut name_counters: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();

            if let Some(parts) = content.get_mut("parts").and_then(|p| p.as_array_mut()) {
                // 1. 如果是 assistant/model 轮次，预先扫描提取 turn_signature (对齐 Anthropic)
                let mut turn_signature: Option<String> = None;
                if is_assistant {
                    for part in parts.iter() {
                        if let Some(obj) = part.as_object() {
                            let is_thought = obj
                                .get("thought")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false)
                                || (obj.get("thoughtSignature").is_some()
                                    && !obj.contains_key("functionCall")
                                    && !obj.contains_key("functionResponse"));
                            if is_thought {
                                if let Some(s) = obj
                                    .get("thoughtSignature")
                                    .or(obj.get("thought_signature"))
                                    .and_then(|s| s.as_str())
                                {
                                    if s == crate::proxy::thinking_store::SENTINEL_SIGNATURE
                                        || s.len() >= 50
                                    {
                                        turn_signature = Some(s.to_string());
                                        break;
                                    }
                                }
                            } else if let Some(fc) = obj.get("functionCall") {
                                if let Some(s) = obj
                                    .get("thoughtSignature")
                                    .or(obj.get("thought_signature"))
                                    .and_then(|s| s.as_str())
                                {
                                    if s == crate::proxy::thinking_store::SENTINEL_SIGNATURE
                                        || s.len() >= 50
                                    {
                                        turn_signature = Some(s.to_string());
                                        break;
                                    }
                                }
                                if let Some(call_id) = fc.get("id").and_then(|v| v.as_str()) {
                                    if let Some(s) = crate::proxy::SignatureCache::global()
                                        .get_tool_signature(call_id)
                                    {
                                        turn_signature = Some(s);
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    if turn_signature.is_none() {
                        if let Some(s_id) = session_id {
                            if let Some(s) = crate::proxy::SignatureCache::global()
                                .get_session_signature_at(s_id, i)
                            {
                                turn_signature = Some(s);
                            } else if let Some(s) =
                                crate::proxy::SignatureCache::global().get_session_signature(s_id)
                            {
                                turn_signature = Some(s);
                            }
                        }
                    }
                }

                let mut new_parts = Vec::with_capacity(parts.len());
                let mut saw_non_thinking = false;

                for mut part in parts.drain(..) {
                    let is_thought = part
                        .get("thought")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                        || (part.get("thoughtSignature").is_some()
                            && part.get("functionCall").is_none()
                            && part.get("functionResponse").is_none());

                    if is_assistant && is_thought {
                        let text = part.get("text").and_then(|v| v.as_str()).unwrap_or("");
                        let incoming_sig = part
                            .get("thoughtSignature")
                            .or_else(|| part.get("thought_signature"))
                            .and_then(|s| s.as_str())
                            .map(str::to_string);

                        // 空思考块处理
                        let text = if text.is_empty() { "..." } else { text };

                        // 占位思考规整 (对齐 Anthropic)
                        let is_placeholder =
                            crate::proxy::thinking_store::is_placeholder_thought(text);
                        let final_thought_text = if is_placeholder { "..." } else { text.trim() };

                        // 位置检查：思考块必须是首位部件，若之前已有非思考内容则降级为文本
                        if saw_non_thinking || !new_parts.is_empty() {
                            tracing::warn!("[Gemini-Wrap] Thinking part found at non-zero index. Downgrading to text.");
                            if !final_thought_text.is_empty() {
                                new_parts.push(json!({ "text": final_thought_text }));
                                saw_non_thinking = true;
                            }
                            continue;
                        }

                        // 思考关闭检查 (对齐 Anthropic 降级为普通文本)
                        if !is_thinking_active {
                            tracing::warn!("[Gemini-Wrap] Thinking disabled. Downgrading thinking part to text.");
                            if !final_thought_text.is_empty() {
                                new_parts.push(json!({ "text": final_thought_text }));
                                saw_non_thinking = true;
                            }
                            continue;
                        }

                        // 签名有效性与模型兼容性校验 (对齐 Anthropic)
                        let mut effective_sig = None;
                        if let Some(ref sig) = incoming_sig {
                            if !sig.is_empty() {
                                let cached_family = crate::proxy::SignatureCache::global()
                                    .get_signature_family(sig);
                                match cached_family {
                                    Some(family) => {
                                        if crate::proxy::mappers::common_utils::is_model_compatible(
                                            &family,
                                            final_model_name,
                                        ) {
                                            effective_sig = Some(sig.clone());
                                        } else {
                                            tracing::warn!(
                                                "[Gemini-Wrap] Incompatible thinking signature (Family: {}, Target: {}).",
                                                family, final_model_name
                                            );
                                        }
                                    }
                                    None => {
                                        effective_sig = Some(sig.clone());
                                    }
                                }
                            }
                        }

                        if effective_sig.is_none() {
                            effective_sig = turn_signature.clone();
                        }
                        if effective_sig.is_none() {
                            if let Some(s_id) = session_id {
                                effective_sig = crate::proxy::SignatureCache::global()
                                    .get_session_signature(s_id);
                            }
                        }
                        if effective_sig.is_none() && can_use_sentinel {
                            effective_sig =
                                Some(crate::proxy::thinking_store::SENTINEL_SIGNATURE.to_string());
                        }

                        if let Some(sig) = effective_sig {
                            new_parts.push(json!({
                                "text": final_thought_text,
                                "thought": true,
                                "thoughtSignature": sig,
                            }));
                        } else {
                            new_parts.push(json!({ "text": final_thought_text }));
                            saw_non_thinking = true;
                        }
                    } else {
                        // 处理普通部件及 functionCall / functionResponse
                        if let Some(obj) = part.as_object_mut() {
                            // 1. 处理 functionCall (Assistant 请求调用工具)
                            if let Some(fc) = obj.get_mut("functionCall") {
                                if fc.get("id").is_none() && is_target_claude {
                                    let name = fc
                                        .get("name")
                                        .and_then(|n| n.as_str())
                                        .unwrap_or("unknown");
                                    let count = name_counters.entry(name.to_string()).or_insert(0);
                                    let call_id = format!("call_{}_{}", name, count);
                                    *count += 1;

                                    fc.as_object_mut()
                                        .unwrap()
                                        .insert("id".to_string(), json!(call_id));
                                    tracing::debug!("[Gemini-Wrap] Request stage: Injected missing call_id '{}' for Claude model", call_id);
                                }

                                // 处理签名校验与兼容性 (对齐 Anthropic)
                                let call_id =
                                    fc.get("id").and_then(|v| v.as_str()).map(str::to_string);
                                let incoming_fc_sig = obj
                                    .get("thoughtSignature")
                                    .or_else(|| obj.get("thought_signature"))
                                    .and_then(|s| s.as_str())
                                    .map(str::to_string);

                                let mut effective_fc_sig = None;
                                if let Some(ref sig) = incoming_fc_sig {
                                    if !sig.is_empty() {
                                        let cached_family = crate::proxy::SignatureCache::global()
                                            .get_signature_family(sig);
                                        match cached_family {
                                            Some(family) => {
                                                if crate::proxy::mappers::common_utils::is_model_compatible(&family, final_model_name) {
                                                    effective_fc_sig = Some(sig.clone());
                                                }
                                            }
                                            None => {
                                                effective_fc_sig = Some(sig.clone());
                                            }
                                        }
                                    }
                                }

                                if effective_fc_sig.is_none() {
                                    if let Some(ref id) = call_id {
                                        effective_fc_sig = crate::proxy::SignatureCache::global()
                                            .get_tool_signature(id);
                                    }
                                }
                                if effective_fc_sig.is_none() {
                                    effective_fc_sig = turn_signature.clone();
                                }
                                if effective_fc_sig.is_none()
                                    && (crate::proxy::thinking_store::model_forces_server_thinking(
                                        &final_model_name,
                                    ) || should_inject)
                                {
                                    effective_fc_sig = Some(
                                        crate::proxy::thinking_store::SENTINEL_SIGNATURE
                                            .to_string(),
                                    );
                                }

                                // 单轮单真签名原则：
                                // 首个工具调用挂载真实签名 (若有)，后续并行工具调用统一打上 32 字节哨兵占位 (满足 Google AST 校验且绝不复制 500KB)
                                let has_preceding_fc =
                                    new_parts.iter().any(|p| p.get("functionCall").is_some());
                                if !has_preceding_fc {
                                    if let Some(sig) = effective_fc_sig {
                                        obj.insert("thoughtSignature".to_string(), json!(sig));
                                    } else {
                                        obj.insert(
                                            "thoughtSignature".to_string(),
                                            json!(crate::proxy::thinking_store::SENTINEL_SIGNATURE),
                                        );
                                    }
                                } else {
                                    obj.insert(
                                        "thoughtSignature".to_string(),
                                        json!(crate::proxy::thinking_store::SENTINEL_SIGNATURE),
                                    );
                                }
                                obj.remove("thought_signature");
                            }

                            // 2. 处理 functionResponse (User 回复工具结果)
                            if obj.contains_key("functionResponse") {
                                obj.remove("thoughtSignature");
                                obj.remove("thought_signature");
                            }
                            if let Some(fr) = obj.get_mut("functionResponse") {
                                if fr.get("id").is_none() && is_target_claude {
                                    let name = fr
                                        .get("name")
                                        .and_then(|n| n.as_str())
                                        .unwrap_or("unknown");
                                    let count = name_counters.entry(name.to_string()).or_insert(0);
                                    let call_id = format!("call_{}_{}", name, count);
                                    *count += 1;

                                    fr.as_object_mut()
                                        .unwrap()
                                        .insert("id".to_string(), json!(call_id));
                                    tracing::debug!("[Gemini-Wrap] Request stage: Injected synced response_id '{}' for Claude model", call_id);
                                }
                            }
                        }
                        saw_non_thinking = true;
                        new_parts.push(part);
                    }
                }
                *parts = new_parts;
            }
        }
        crate::proxy::pipeline::InboundThinkingPipeline::process_contents(
            contents,
            crate::proxy::pipeline::ProxyProtocol::GeminiNative,
            &final_model_name,
            should_inject,
            session_id,
            false,
        );
    }

    // [FIX Issue #1355] Gemini Flash thinking budget capping
    // [CONFIGURABLE] 现在改为遵循全局 Thinking Budget 配置
    // [FIX #1557] Also apply to Pro/Thinking models to ensure budget processing
    // [FIX #1557] Auto-inject thinkingConfig if missing for these models
    if force_server_thinking
        || lower_model.contains("flash")
        || lower_model.contains("pro")
        || lower_model.contains("thinking")
        || lower_model.contains("agent")
        || lower_model.contains("gemini")
    {
        // [NEW] Extract OpenAI-style max_tokens before mutably borrowing gen_config
        let req_max_tokens = inner_request.get("max_tokens").and_then(|v| v.as_u64());

        // Determine model family and capability beforehand to avoid borrow checker conflicts
        let is_claude = lower_model.contains("claude");

        if should_inject {
            // Scope for borrowing inner_request/gen_config
            let has_thinking = if is_claude {
                inner_request.get("thinking").is_some()
            } else {
                inner_request
                    .get("generationConfig")
                    .and_then(|v| v.as_object())
                    .map_or(false, |gc| gc.get("thinkingConfig").is_some())
            };

            let default_budget =
                crate::proxy::model_specs::get_thinking_budget(final_model_name, token);

            let is_explicit_tier =
                crate::proxy::model_specs::is_explicit_heuristic_tier_model(final_model_name);

            // [ANTI-POLLUTION] 对齐 Anthropic 与 OpenAI：对于未设置思考配置或显式档位模型，设定权威 default_budget；对于裸模型保留客户端配置供后续 resolve_authoritative_thinking_budget 仲裁
            let should_override_budget = !has_thinking || is_explicit_tier;

            if should_override_budget {
                tracing::debug!(
                    "[Gemini-Wrap] Enforcing authoritative thinking budget {} for {}",
                    default_budget,
                    final_model_name
                );

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

        if is_under_v3 {
            gen_config.remove("thinkingConfig");
        }

        // [ADDED v4.1.24] Inject topK=40 and topP=1.0 if not present to match official client
        if !gen_config.contains_key("topK") {
            gen_config.insert("topK".to_string(), json!(40));
        }
        if !gen_config.contains_key("topP") {
            gen_config.insert("topP".to_string(), json!(1.0));
        }

        if force_server_thinking {
            let default_budget =
                crate::proxy::model_specs::get_thinking_budget(final_model_name, token);
            let thinking_config = gen_config
                .entry("thinkingConfig".to_string())
                .or_insert(json!({}))
                .as_object_mut()
                .unwrap();
            thinking_config.insert("includeThoughts".to_string(), json!(true));
            if !thinking_config.contains_key("thinkingBudget")
                && !thinking_config.contains_key("thinkingLevel")
            {
                thinking_config.insert("thinkingBudget".to_string(), json!(default_budget));
            }
            tracing::debug!(
                "[Gemini-Wrap] Forced includeThoughts=true for keyword model {}",
                final_model_name
            );
        }

        // [AUTHORITATIVE RESOLUTION] 全协议统一由进站流水线节点解析思考预算
        let has_thinking_config = gen_config.contains_key("thinkingConfig");
        let client_level = gen_config
            .get("thinkingConfig")
            .and_then(|t| t.get("thinkingLevel"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let client_budget = gen_config
            .get("thinkingConfig")
            .and_then(|t| t.get("thinkingBudget"))
            .and_then(|v| v.as_i64());

        let tb_config = crate::proxy::config::get_thinking_budget_config();
        let budget_opt = if has_thinking_config || force_server_thinking {
            let mut gc_val = serde_json::Value::Object(std::mem::take(gen_config));
            let resolved =
                crate::proxy::pipeline::InboundThinkingPipeline::configure_inbound_thinking(
                    final_model_name,
                    &mut gc_val,
                    client_level.as_deref(),
                    client_budget.filter(|b| *b > 0).map(|b| b as u64),
                    token,
                );
            if let serde_json::Value::Object(map) = gc_val {
                *gen_config = map;
            }
            resolved
        } else {
            None
        };

        if tb_config.control_source == crate::proxy::config::ThinkingControlSource::Client {
            if let Some(thinking_config) = gen_config.get_mut("thinkingConfig") {
                if let Some(ref lvl) = client_level {
                    if let Some(norm_lvl) =
                        crate::proxy::model_specs::normalize_client_thinking_level(lvl)
                    {
                        let final_lvl = if final_model_name.to_lowercase().contains("pro")
                            && norm_lvl == "MEDIUM"
                        {
                            "HIGH"
                        } else {
                            norm_lvl
                        };
                        thinking_config["thinkingLevel"] = json!(final_lvl);
                    }
                }
                if let Some(b) = client_budget {
                    if b > 0 {
                        thinking_config["thinkingBudget"] = json!(b);
                    } else if b == -1 {
                        if let Some(tc) = thinking_config.as_object_mut() {
                            tc.remove("thinkingBudget");
                        }
                    }
                }
            }
        } else if let Some(tc) = gen_config
            .get_mut("thinkingConfig")
            .and_then(|v| v.as_object_mut())
        {
            tracing::info!(
                "[Gemini-Wrap] Pipeline thinking budget {:?} for {} (client_level={:?})",
                budget_opt,
                final_model_name,
                client_level
            );
            tc.remove("thinkingLevel");
        }

        // [FIX #1747] Ensure max_tokens (maxOutputTokens) is greater than thinking_budget
        // Google v1internal requires maxOutputTokens > thinkingBudget.
        // [FIX #1825] Handle adaptive fallback (incl. -1 and thinkingLevel)
        let thinking_config_opt = gen_config.get("thinkingConfig");
        let is_adaptive = thinking_config_opt.map_or(false, |t| {
            t.get("thinkingLevel").is_some()
                || t.get("thinkingBudget").is_none()
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
        if is_under_v3 {
            gen_config.remove("thinkingConfig");
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
                        // 清洗 Schema: 如果存在 parametersJsonSchema，将其标准化为 parameters
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

    // Inject googleSearch tool if needed (stacking alongside existing tools)
    if config.inject_google_search {
        if config.request_type == "web_search" {
            if let Some(obj) = inner_request.as_object_mut() {
                let tools_entry = obj.entry("tools").or_insert_with(|| json!([]));
                if let Some(tools_arr) = tools_entry.as_array_mut() {
                    let has_functions = tools_arr.iter().any(|t| {
                        t.as_object().map_or(false, |o| {
                            o.contains_key("functionDeclarations")
                                || o.contains_key("function_declarations")
                        })
                    });
                    if !has_functions {
                        // 清理已存在的 googleSearch
                        tools_arr.retain(|t| {
                            if let Some(o) = t.as_object() {
                                !(o.contains_key("googleSearch")
                                    || o.contains_key("google_search")
                                    || o.contains_key("googleSearchRetrieval"))
                            } else {
                                true
                            }
                        });
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
        // [FIX] 彻底移除 web_search 等任何预置搜索 Bot 提示词注入，保持客户端原始 prompt 完全纯净透传。
        // 仅在配置了用户自定义全局系统提示词时进行追加注入。
        let global_prompt_config = crate::proxy::config::get_global_system_prompt();

        // 检查是否已有 systemInstruction
        if let Some(system_instruction) = inner_request.get_mut("systemInstruction") {
            // 补全 role: user
            if let Some(obj) = system_instruction.as_object_mut() {
                if !obj.contains_key("role") {
                    obj.insert("role".to_string(), json!("user"));
                }
            }

            if let Some(parts) = system_instruction.get_mut("parts") {
                if let Some(parts_array) = parts.as_array_mut() {
                    // 注入全局系统提示词（去重 + 换行隔离，不夹官方身份）
                    if global_prompt_config.enabled
                        && !global_prompt_config.content.trim().is_empty()
                    {
                        let prompt_content = global_prompt_config.content.trim();
                        let already_has_global = parts_array.iter().any(|p| {
                            p.get("text")
                                .and_then(|t| t.as_str())
                                .map(|s| s.contains(prompt_content))
                                .unwrap_or(false)
                        });

                        if !already_has_global {
                            let formatted = format!("{}\n\n", prompt_content);
                            parts_array.push(json!({"text": formatted}));
                        }
                    }
                }
            }
        } else {
            // 没有 systemInstruction，仅在启用全局提示词时创建
            if global_prompt_config.enabled && !global_prompt_config.content.trim().is_empty() {
                inner_request["systemInstruction"] = json!({
                    "role": "user",
                    "parts": [{"text": format!("{}\n\n", global_prompt_config.content.trim())}]
                });
            }
        }
    }

    // [ADDED v4.1.24] 扩展 toolConfig 到 VALIDATED 模式并开启 includeServerSideToolInvocations (同时支持 camelCase 与 snake_case)
    if inner_request.get("tools").is_some() {
        // 1. camelCase
        if let Some(tool_config) = inner_request.get_mut("toolConfig") {
            if let Some(obj) = tool_config.as_object_mut() {
                obj.insert("includeServerSideToolInvocations".to_string(), json!(true));
            }
        } else {
            inner_request["toolConfig"] = json!({
                "functionCallingConfig": { "mode": "VALIDATED" },
                "includeServerSideToolInvocations": true
            });
        }
        // 2. snake_case
        if let Some(tool_config_snake) = inner_request.get_mut("tool_config") {
            if let Some(obj) = tool_config_snake.as_object_mut() {
                obj.insert(
                    "include_server_side_tool_invocations".to_string(),
                    json!(true),
                );
            }
        } else {
            inner_request["tool_config"] = json!({
                "function_calling_config": { "mode": "VALIDATED" },
                "include_server_side_tool_invocations": true
            });
        }
    }

    // [ADDED v4.1.24] 注入基于账号的稳定 sessionId
    // [FIX session-1M] 混入对话指纹与代数,不同对话隔离服务端会话,1M 累计报错后 bump 自愈
    if let Some(account_id_str) = account_id {
        let fingerprint = session_id.unwrap_or("default");
        let generation = crate::proxy::common::session::current_bump(account_id_str, fingerprint);
        inner_request["sessionId"] = json!(crate::proxy::common::session::derive_session_scoped(
            account_id_str,
            fingerprint,
            generation
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

    // [NEW] 3. 动态判断是否需要 agent requestType 与 enabledCreditTypes
    // 对齐官方语言服务原生设计：只有存在工具定义 (tools) 或包含工具调用上下文时才进入 agent 模式。
    // 普通问答、纯文本补全不注入 requestType: "agent"，避开 Google 后端针对 Agent 资源池的过载限流。
    let has_tools = inner_request
        .get("tools")
        .and_then(|t| t.as_array())
        .map(|arr| !arr.is_empty())
        .unwrap_or(false);
    let has_tool_interactions = inner_request
        .get("contents")
        .map(crate::proxy::mappers::common_utils::contents_has_tool_interactions)
        .unwrap_or(false);

    let is_agent_request =
        config.request_type != "image_gen" && (has_tools || has_tool_interactions);

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
    // 3. toolConfig & tool_config (稳定，与 tools 共生)
    if let Some(tc) = inner_request.get("toolConfig") {
        reordered_inner["toolConfig"] = tc.clone();
    }
    if let Some(tc_snake) = inner_request.get("tool_config") {
        reordered_inner["tool_config"] = tc_snake.clone();
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
        // [CACHE] requestId 移到末尾避免动态值破坏前缀字节一致性
        "requestId": official_request_id,
    });

    if config.request_type == "image_gen" {
        final_request_obj["requestType"] = json!("image_gen");
    } else if is_agent_request {
        final_request_obj["requestType"] = json!("agent");
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
        assert!(
            injected_sig == signature
                || injected_sig == crate::proxy::thinking_store::SENTINEL_SIGNATURE
        );
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
        assert!(
            part.get("thought_signature").is_none(),
            "Snake case 'thought_signature' must be stripped when sending to Google"
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

        // 验证没有多余注入的 systemInstruction
        assert!(result
            .get("request")
            .unwrap()
            .get("systemInstruction")
            .is_none());
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
        // Pro models without suffix now default to 10001 in wrap_request logic
        assert_eq!(budget_pro, 10001);
    }

    #[test]
    fn test_image_thinking_mode_disabled() {
        let _test_lock = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let _config_lock = crate::proxy::config::TEST_CONFIG_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        // 1. Set global mode to disabled
        crate::proxy::config::update_image_thinking_mode(Some("disabled".to_string()));
        struct ImageResetGuard;
        impl Drop for ImageResetGuard {
            fn drop(&mut self) {
                crate::proxy::config::update_image_thinking_mode(Some("enabled".to_string()));
            }
        }
        let _guard = ImageResetGuard;

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

        // User custom prompt is preserved without injecting unwanted Antigravity identity
        assert_eq!(parts.len(), 1);
        assert_eq!(
            parts[0].get("text").unwrap().as_str().unwrap(),
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
        let _test_lock = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let _config_lock = crate::proxy::config::TEST_CONFIG_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // Update global config to Custom mode to verify logic execution
        use crate::proxy::config::{
            update_thinking_budget_config, ThinkingBudgetConfig, ThinkingBudgetMode,
        };

        update_thinking_budget_config(ThinkingBudgetConfig {
            mode: ThinkingBudgetMode::Custom,
            custom_value: 1024, // Distinct value
            effort: None,
            ..Default::default()
        });
        struct GeminiCustomResetGuard;
        impl Drop for GeminiCustomResetGuard {
            fn drop(&mut self) {
                update_thinking_budget_config(ThinkingBudgetConfig::default());
            }
        }
        let _guard = GeminiCustomResetGuard;

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
    }

    #[cfg(test)]
    mod test_v4_fixes {
        use super::*;
        use serde_json::json;

        #[test]
        fn test_claude_no_root_thinking_injection() {
            let _test_lock = super::TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
            let _config_lock = crate::proxy::config::TEST_CONFIG_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            // 验证 Claude 模型不会在根目录注入 thinking，而是注入到 generationConfig.thinkingConfig
            // 并且 budget 默认为 16000

            // 使用 Auto 模式避免干扰
            crate::proxy::config::update_thinking_budget_config(
                crate::proxy::config::ThinkingBudgetConfig {
                    mode: crate::proxy::config::ThinkingBudgetMode::Auto,
                    custom_value: 0,
                    effort: None,
                    ..Default::default()
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

            // 3. 验证 Claude 默认预算为 16384
            let budget = thinking_config["thinkingBudget"]
                .as_u64()
                .expect("thinkingBudget should be a number");
            assert_eq!(
                budget, 16384,
                "Claude default thinking budget should be 16384"
            );
        }

        #[test]
        fn test_gemini_thinking_injection_default() {
            let _test_lock = super::TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
            let _config_lock = crate::proxy::config::TEST_CONFIG_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());
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
        let _test_lock = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let _config_lock = crate::proxy::config::TEST_CONFIG_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // Reset thinking budget to auto mode at the start to avoid interference from parallel tests
        crate::proxy::config::update_thinking_budget_config(
            crate::proxy::config::ThinkingBudgetConfig {
                mode: crate::proxy::config::ThinkingBudgetMode::Auto,
                custom_value: 24576,
                effort: None,
                ..Default::default()
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
            "Should NOT contain googleSearch due to functionDeclarations presence (preventing client tool dispatch conflicts)"
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

    #[test]
    fn test_gemini_anthropic_alignment_thinking_and_signatures() {
        let valid_sig = "A".repeat(60); // 60 chars valid signature
        let body = json!({
            "contents": [
                {
                    "role": "model",
                    "parts": [
                        {
                            "thought": true,
                            "text": "·", // placeholder thought
                            "thoughtSignature": valid_sig
                        },
                        {
                            "thought": true,
                            "text": "second thought block that should be downgraded",
                            "thoughtSignature": "short"
                        },
                        {
                            "text": "Regular model response"
                        }
                    ]
                }
            ],
            "generationConfig": {
                "thinkingConfig": {
                    "thinkingBudget": 2048
                }
            }
        });

        let wrapped = wrap_request(&body, "test-proj", "gemini-3-pro", None, None, None);
        let contents = wrapped["request"]["contents"].as_array().unwrap();
        let model_msg = &contents[0];
        let parts = model_msg["parts"].as_array().unwrap();

        // Part 0 should be normalized from "·" to "..."
        assert_eq!(parts[0]["thought"], true);
        assert_eq!(parts[0]["text"], "...");
        // Part 0 signature should be preserved because it's valid length and compatible
        assert_eq!(parts[0]["thoughtSignature"], valid_sig);

        // Part 1 should be downgraded to text without thought: true
        assert!(parts[1].get("thought").is_none());
        assert_eq!(
            parts[1]["text"],
            "second thought block that should be downgraded"
        );
        // Downgraded part should not carry thoughtSignature
        assert!(parts[1].get("thoughtSignature").is_none());

        // Part 2 remains regular text
        assert_eq!(parts[2]["text"], "Regular model response");
    }

    #[test]
    fn test_gemini_thinking_level_authority_resolution() {
        let _lock = crate::proxy::config::TEST_CONFIG_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::proxy::config::update_thinking_budget_config(
            crate::proxy::config::ThinkingBudgetConfig::default(),
        );
        // 1. 启发式模型忽略客户端 thinkingLevel
        let req_high = json!({
            "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
            "generationConfig": {
                "thinkingConfig": { "thinkingLevel": "LOW" }
            }
        });
        let wrapped = wrap_request(
            &req_high,
            "test-p",
            "gemini-3.7-flash-high",
            None,
            None,
            None,
        );
        let tc = &wrapped["request"]["generationConfig"]["thinkingConfig"];
        assert_eq!(tc["thinkingBudget"], 16384);
        assert!(tc.get("thinkingLevel").is_none());

        // 2. 裸模型 Flash 接管客户端 thinkingLevel
        let req_flash_high = json!({
            "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
            "generationConfig": {
                "thinkingConfig": { "thinkingLevel": "HIGH" }
            }
        });
        let wrapped = wrap_request(
            &req_flash_high,
            "test-p",
            "gemini-3-flash",
            None,
            None,
            None,
        );
        let tc = &wrapped["request"]["generationConfig"]["thinkingConfig"];
        assert_eq!(tc["thinkingBudget"], 16384);
        assert!(tc.get("thinkingLevel").is_none());

        let req_flash_low = json!({
            "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
            "generationConfig": {
                "thinkingConfig": { "thinkingLevel": "LOW" }
            }
        });
        let wrapped = wrap_request(&req_flash_low, "test-p", "gemini-3-flash", None, None, None);
        let tc = &wrapped["request"]["generationConfig"]["thinkingConfig"];
        assert_eq!(tc["thinkingBudget"], 1024);
        assert!(tc.get("thinkingLevel").is_none());

        // 3. 裸模型 Flash 客户端传 NONE 或未传：绝不关闭思考，强制回填 -medium (4096)
        let req_flash_none = json!({
            "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
            "generationConfig": {
                "thinkingConfig": { "thinkingLevel": "NONE" }
            }
        });
        let wrapped = wrap_request(
            &req_flash_none,
            "test-p",
            "gemini-3-flash",
            None,
            None,
            None,
        );
        let tc = &wrapped["request"]["generationConfig"]["thinkingConfig"];
        assert_eq!(tc["thinkingBudget"], 4096);
        assert!(tc.get("thinkingLevel").is_none());

        let req_flash_empty = json!({
            "contents": [{"role": "user", "parts": [{"text": "hi"}]}]
        });
        let wrapped = wrap_request(
            &req_flash_empty,
            "test-p",
            "gemini-3-flash",
            None,
            None,
            None,
        );
        let tc = &wrapped["request"]["generationConfig"]["thinkingConfig"];
        assert_eq!(tc["thinkingBudget"], 4096);

        // 4. 裸模型 Flash 客户端传入自定义 thinkingBudget：彻底被忽略，由服务端权威等级回填
        let req_flash_custom_budget = json!({
            "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
            "generationConfig": {
                "thinkingConfig": { "thinkingBudget": 12345 }
            }
        });
        let wrapped = wrap_request(
            &req_flash_custom_budget,
            "test-p",
            "gemini-3-flash",
            None,
            None,
            None,
        );
        let tc = &wrapped["request"]["generationConfig"]["thinkingConfig"];
        assert_eq!(tc["thinkingBudget"], 4096);

        let req_flash_high_custom_budget = json!({
            "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
            "generationConfig": {
                "thinkingConfig": { "thinkingLevel": "HIGH", "thinkingBudget": 1234 }
            }
        });
        let wrapped = wrap_request(
            &req_flash_high_custom_budget,
            "test-p",
            "gemini-3-flash",
            None,
            None,
            None,
        );
        let tc = &wrapped["request"]["generationConfig"]["thinkingConfig"];
        assert_eq!(tc["thinkingBudget"], 16384);
    }
}
