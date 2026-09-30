use serde_json::{json, Value};

/// 客户端思考控制开关（三态枚举）
/// 遵循最高指令：思考开关（一票否决权） > 思考等级 > 思考预算
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientThinkingSwitch {
    /// 显式关闭（最高准则，一票否决：disabled / 0 / none / off）
    Disabled,
    /// 显式开启（enabled / on / 具体档位 / 具体预算）
    Enabled,
    /// 缺省（未传任何思考参数，或值为 default；业务铁律：缺省就是默认开）
    Default,
}

impl ClientThinkingSwitch {
    /// 是否允许开启思考（缺省即开，关则一票否决）
    pub fn is_allowed(self) -> bool {
        matches!(self, Self::Enabled | Self::Default)
    }

    /// 是否显式关闭
    pub fn is_disabled(self) -> bool {
        matches!(self, Self::Disabled)
    }
}

/// 统一归一化提取客户端思考开关状态（适用于 OpenAI / Claude / Gemini / Codex 等所有协议）
pub fn extract_client_thinking_switch(
    thinking_type: Option<&str>,
    budget: Option<u64>,
    effort: Option<&str>,
) -> ClientThinkingSwitch {
    let t_type = thinking_type.map(|s| s.trim().to_lowercase());
    let eff = effort.map(|s| s.trim().to_lowercase());

    // 1. 显式关闭判定（最高优先级，一票否决）
    if matches!(
        t_type.as_deref(),
        Some("disabled") | Some("off") | Some("false") | Some("0")
    ) || budget == Some(0)
        || matches!(
            eff.as_deref(),
            Some("none") | Some("off") | Some("false") | Some("0") | Some("disabled")
        )
    {
        return ClientThinkingSwitch::Disabled;
    }

    // 2. 显式开启判定（只要客户端带了有效开启标记、具体预算，或任何非空/非关闭的思考等级）
    if matches!(
        t_type.as_deref(),
        Some("enabled") | Some("on") | Some("true")
    ) || budget.map_or(false, |b| b > 0)
        || eff.as_deref().map_or(false, |e| {
            !e.is_empty()
                && e != "none"
                && e != "off"
                && e != "false"
                && e != "0"
                && e != "disabled"
                && e != "default"
        })
    {
        return ClientThinkingSwitch::Enabled;
    }

    // 3. 缺省（全未传，或仅为 "default"；铁律：缺省就是默认开）
    ClientThinkingSwitch::Default
}

/// 统一进站思考管线（InboundThinkingPipeline）
/// 接收任何协议转译成的 Google contents 统一报文，单向流转执行：
/// 1. 签名校验只看目标模型与签名本身，四个协议同一规则
/// 2. 思考块首位强制排序与占位符规范化
/// 3. 状态机历史思维链无损复活 (Hydration)
/// 4. 终审脱敏与前缀缓存格式规范化 (Finalize)
pub struct InboundThinkingPipeline;

impl InboundThinkingPipeline {
    /// 执行统一进站处理
    pub fn process_contents(
        contents: &mut Vec<Value>,
        target_model: &str,
        is_thinking_enabled: bool,
        session_id: Option<&str>,
        is_retry: bool,
    ) {
        let is_claude = target_model.to_lowercase().contains("claude");

        // 0. 工具调用 ID 统一归一化治理（Pipeline First）：
        // 彻底解决不同客户端在 tool call ID 格式上不一致（如缺少下划线 `call573077` vs `call_573077`）的问题，
        // 将 `functionCall.id` 与 `functionResponse.id` 统一对齐为带 `_` 的标准形态 (`call_...`)。
        Self::normalize_tool_call_ids(contents);

        // 0.1 连续工具调用轮次合并（Pipeline First 统一治理）：
        // 当客户端将并行工具调用拆散为多个连续的 assistant/model 轮次时，
        // 必须将其合并为一个单一的 model content 轮次。
        // 遵循 Google v1internal 规则：一个 model 轮次内仅首个部件需带签名，后续部件可不带签名；
        // 若拆为多个 model 轮次，后续轮次将因缺失独立签名被直接拒收 400。
        Self::merge_consecutive_function_call_turns(contents);

        // 0.2 统一上下文结构对齐（Pipeline First 统一治理）：
        // 将连续的 user 消息直到下一个 model，统一合并为一个 user 轮次的多个 block (parts)，保持严格顺序。
        // 这彻底消除了 Adapter 层各自为政导致的轮次错位，使得四大协议进入流水线后结构 100% 同构！
        // 连续 user 轮**保持独立**（对齐官方形态）。
        //
        // 历史实现会把连续 user 轮的 parts 合并进前一个 content，理由是
        // "Gemini 强制要求 user/model 交替"。但官方 Antigravity 报文里连续 user 轮
        // 是常态（`f81eae5c` 的 contents[11] 用户消息紧接 contents[12] SYSTEM_MESSAGE，
        // 两者独立），v1internal 上游并不要求严格交替。
        //
        // 实测（2026-09-26，`gemini-3.8-flash-tiered` @ daily）：
        //   两个独立 user content     → 200，上下文理解正确
        //   合并为单个 user content   → 200，上下文理解正确
        // 两者都成立，故按官方形态保留独立，以获得跨协议一致的前缀字节。

        // 预先计算每一轮的前置因果锚点 (causal anchor)，以便无 ID 的 Gemini 原生工具调用也能无损合成确定性 ID
        let anchors: Vec<String> = (0..contents.len())
            .map(|i| {
                let preceding = if i > 0 { contents.get(i - 1) } else { None };
                crate::proxy::thinking_store::compute_causal_anchor(preceding)
            })
            .collect();

        // 1. 协议策略清洗与位置规范化
        for (msg_idx, content) in contents.iter_mut().enumerate() {
            let _anchor = &anchors[msg_idx];
            let is_model = matches!(
                content.get("role").and_then(|r| r.as_str()),
                Some("model") | Some("assistant")
            ) || content
                .get("parts")
                .and_then(|p| p.as_array())
                .map_or(false, |parts| {
                    parts.iter().any(|p| p.get("functionCall").is_some())
                });

            if let Some(parts) = content.get_mut("parts").and_then(|p| p.as_array_mut()) {
                if is_model {
                    let mut thinking_part = None;
                    let mut extra_thinking_parts = Vec::new();
                    let mut other_parts = Vec::new();
                    // [2026-09-27] 占位思考块被丢弃时，若其携带真实签名则暂存于此，
                    // 组装阶段转挂到该轮第一个非思考 part（锚点）。
                    let mut placeholder_sig: Option<String> = None;

                    for mut part in parts.drain(..) {
                        // 铁律：只认 thought: true。真机报文的正文 part 同样携带签名，
                        // 绝不能凭"有签名"判定思考块 —— 否则可见回答会被改写成内部思考，
                        // 并静默丢弃该轮唯一的签名。
                        let is_thought = crate::proxy::thinking_store::is_thought_part(&part);

                        if is_thought {
                            let text = part.get("text").and_then(|v| v.as_str()).unwrap_or("");
                            let is_placeholder =
                                crate::proxy::thinking_store::is_placeholder_thought(text);

                            // 校验客户端签名有效性与模型兼容性
                            let mut effective_sig = None;
                            if let Some(sig) = part
                                .get("thoughtSignature")
                                .or_else(|| part.get("thought_signature"))
                                .or_else(|| part.get("signature"))
                                .and_then(|s| s.as_str())
                            {
                                if sig == crate::proxy::thinking_store::SENTINEL_SIGNATURE {
                                    // Claude 模型绝不接受 Gemini 哨兵签名，避免触发 400 Invalid signature
                                    if !is_claude {
                                        effective_sig = Some(sig.to_string());
                                    }
                                } else if sig.len() >= 50 {
                                    let cached_family = crate::proxy::SignatureCache::global()
                                        .get_signature_family(sig);
                                    let compatible = match cached_family {
                                        Some(family) => {
                                            crate::proxy::mappers::common_utils::is_model_compatible(
                                                &family,
                                                target_model,
                                            ) || (is_claude
                                                && family.to_lowercase().contains("claude"))
                                        }
                                        None => {
                                            if target_model.to_lowercase().contains("gemini") {
                                                crate::proxy::thinking_store::is_likely_gemini_signature(sig)
                                            } else if is_claude {
                                                crate::proxy::thinking_store::is_claude_signature(
                                                    sig,
                                                )
                                            } else {
                                                true
                                            }
                                        }
                                    };
                                    if compatible {
                                        let final_sig = if is_claude {
                                            crate::proxy::thinking_store::ensure_google_claude_thought_signature(sig)
                                        } else {
                                            sig.to_string()
                                        };
                                        effective_sig = Some(final_sig);
                                    } else if target_model.to_lowercase().contains("gemini") {
                                        tracing::warn!(
                                            "[InboundPipeline] Stripping foreign signature (len: {}) from thought block for Gemini model {}",
                                            sig.len(), target_model
                                        );
                                        effective_sig = None;
                                    } else if is_claude {
                                        tracing::warn!(
                                            "[InboundPipeline] Stripping foreign signature (len: {}) from thought block for Claude model {}",
                                            sig.len(), target_model
                                        );
                                        effective_sig = None;
                                    }
                                }
                            }

                            // [2026-09-27] 占位思考块直接丢弃（不写思考块），签名照常落锚点。
                            //
                            // 官方样本（baogao.txt 24 轮）：9 轮无思考块但首非思考 part 带签名，
                            // 「无思考块 + 锚点带签名」是官方标准形态。占位思考块（空 /
                            // "." / "..." / 空格 / "·" 等）无信息量，不该出现在出站报文中：
                            // 它曾是网关"防御性占位"的产物（FIX #3382 / Layer-2 压缩已移除）。
                            //
                            // 若占位块本身携带真实签名（客户端把签名误挂在思考块上），
                            // 签名不随思考块丢弃 —— 摘出后挂到第一个非思考 part（锚点），
                            // finalize 的 place_turn_signature 会按锚点规则最终归位。
                            let text_trimmed = text.trim();
                            if is_placeholder || text_trimmed.is_empty() {
                                if let Some(ref sig) = effective_sig {
                                    // 占位思考块携带的签名：转移给该轮锚点（首个非思考 part）
                                    placeholder_sig = Some(sig.clone());
                                    tracing::debug!(
                                        "[InboundPipeline] Placeholder thought block dropped; its signature (len: {}) kept for anchor backfill.",
                                        sig.len(),
                                    );
                                } else {
                                    tracing::debug!(
                                        "[InboundPipeline] Placeholder thought block dropped (text={:?}).",
                                        text_trimmed,
                                    );
                                }
                                continue;
                            }

                            let final_thought_text = text;

                            let thought_obj = json!({
                                "text": final_thought_text,
                                "thought": true,
                            });
                            // 思考块本身不挂签名。签名落到该轮第一个非思考 part。
                            if let Some(sig) = effective_sig {
                                if placeholder_sig.is_none() {
                                    placeholder_sig = Some(sig);
                                }
                            }

                            // Gemini 历史不回传思考正文（Mac IDE 0 次回传，桌面端回传也能通）。
                            // Claude 两边都回传思考正文，保留。
                            if is_claude {
                                if thinking_part.is_none() {
                                    thinking_part = Some(thought_obj);
                                } else if !final_thought_text.is_empty() {
                                    extra_thinking_parts
                                        .push(json!({ "text": final_thought_text }));
                                }
                            }
                        } else {
                            if target_model.to_lowercase().contains("gemini") {
                                // 规范化字段：若客户端携带蛇形 thought_signature 且无驼峰字段，平滑重命名为标准 thoughtSignature 保留
                                if let Some(obj) = part.as_object_mut() {
                                    if let Some(sig) = obj.remove("thought_signature") {
                                        if !obj.contains_key("thoughtSignature") {
                                            obj.insert("thoughtSignature".to_string(), sig);
                                        }
                                    }
                                    // 黄金法则 2.1：根据模型家校验签名合法性，若合法采纳并反向入库，不合法则丢弃由流水线补充
                                    if let Some(sig_str) =
                                        obj.get("thoughtSignature").and_then(|s| s.as_str())
                                    {
                                        if sig_str == crate::proxy::thinking_store::SENTINEL_SIGNATURE {
                                            // 官方跳过验签哨兵 (skip_thought_signature_validator)：客户端自带的合法哨兵，予以保留
                                        } else if crate::proxy::thinking_store::is_real_signature(sig_str)
                                            && crate::proxy::thinking_store::is_likely_gemini_signature(sig_str)
                                        {
                                            if let Some(fc) = obj.get("functionCall") {
                                                if let Some(id) = fc.get("id").and_then(|v| v.as_str()) {
                                                    if let Some(sid) = session_id {
                                                        crate::proxy::SignatureCache::global()
                                                            .cache_tool_signature(
                                                                sid,
                                                                id,
                                                                sig_str.to_string(),
                                                            );
                                                    }
                                                }
                                            }
                                            if let Some(sid) = session_id {
                                                crate::proxy::SignatureCache::global()
                                                    .cache_session_signature(sid, sig_str.to_string(), msg_idx);
                                            }
                                        } else {
                                            obj.remove("thoughtSignature");
                                        }
                                    }
                                }
                            }
                            // 非思考部件：可能是普通正文/过程进度说明（commentary），也可能是 functionCall 等
                            // Claude 桌面端把 thoughtSignature 挂在该轮第一个非思考 part 上，这里不剥。
                            let is_plain_text = part.get("text").is_some()
                                && part.get("functionCall").is_none()
                                && part.get("functionResponse").is_none();

                            if is_plain_text {
                                let raw_text =
                                    part.get("text").and_then(|v| v.as_str()).unwrap_or("");
                                if raw_text.trim().is_empty() {
                                    // 丢弃纯空白文本部件，避免触发 Gemini 400 校验或破坏前缀缓存哈希稳定性
                                    continue;
                                }

                                // 丢弃占位文本部件（如 "..."、"·" 等无意义客户端脏数据）：
                                // 若携带有真实签名，先将签名暂存至 placeholder_sig，防止有效凭证随占位文本丢弃
                                if crate::proxy::thinking_store::is_placeholder_thought(raw_text) {
                                    if placeholder_sig.is_none() {
                                        if let Some(sig) = part
                                            .get("thoughtSignature")
                                            .or_else(|| part.get("thought_signature"))
                                            .and_then(|s| s.as_str())
                                            .filter(|s| {
                                                crate::proxy::thinking_store::is_real_signature(s)
                                                    && crate::proxy::thinking_store::is_likely_gemini_signature(s)
                                            })
                                        {
                                            placeholder_sig = Some(sig.to_string());
                                        }
                                    }
                                    tracing::debug!(
                                        "[InboundPipeline] Dropped placeholder text part (text={:?})",
                                        raw_text
                                    );
                                    continue;
                                }

                                // 跨家族协议自愈：检查是否夹带 <think> 标签包裹的思考内容（如从 Claude 跨切回 Gemini）
                                if let Some((extracted_thought, clean_visible)) =
                                    crate::proxy::thinking_store::extract_think_tags(raw_text)
                                {
                                    if thinking_part.is_none() && is_thinking_enabled {
                                        if crate::proxy::thinking_store::is_meaningful_thought(
                                            &extracted_thought,
                                        ) {
                                            // 铁律 I4：思考块绝不携带签名
                                            // （官方报文里 thought:true 的 part 没有 thoughtSignature）
                                            let t_obj = json!({
                                                "text": extracted_thought.as_str(),
                                                "thought": true,
                                            });
                                            thinking_part = Some(t_obj);
                                        }
                                    }
                                    if !clean_visible.is_empty() {
                                        other_parts.push(json!({ "text": clean_visible }));
                                    }
                                    continue;
                                }

                                // 协议无关自愈：检查是否夹带旧版遗留思考前缀 (如 **Thinking**)
                                if raw_text.trim_start().starts_with("**Thinking**") {
                                    let clean_thought = Self::strip_thinking_prefix(raw_text);
                                    if thinking_part.is_none() && is_thinking_enabled {
                                        if crate::proxy::thinking_store::is_meaningful_thought(
                                            &clean_thought,
                                        ) {
                                            // 铁律 I4：思考块绝不携带签名
                                            let t_obj = json!({
                                                "text": clean_thought,
                                                "thought": true,
                                            });
                                            thinking_part = Some(t_obj);
                                        }
                                    }
                                    // 若已有思考块，该遗留思考块作为陈旧副本剥离，防止二次污染正文
                                    continue;
                                }

                                other_parts.push(part);
                            } else {
                                other_parts.push(part);
                            }
                        }
                    }

                    // 核心前缀保序：首位强制存在且仅存在一个 thinking_part，其余正文与工具调用紧随其后
                    let mut new_parts = Vec::with_capacity(parts.len() + 1);
                    if let Some(tp) = thinking_part {
                        new_parts.push(tp);
                    }
                    new_parts.extend(extra_thinking_parts);
                    new_parts.extend(other_parts);

                    // [2026-09-27] 占位思考块/占位文本被丢弃时的签名转移：
                    // Gemini：有工具调用时落到第一个 functionCall。Claude：落到第一个非思考 part。
                    if let Some(sig) = placeholder_sig.take() {
                        let anchor_idx = crate::proxy::thinking_store::find_turn_anchor_with(
                            &new_parts, !is_claude,
                        );
                        let anchor = anchor_idx.and_then(|idx| new_parts.get_mut(idx));
                        if let Some(anchor) = anchor {
                            if let Some(obj) = anchor.as_object_mut() {
                                obj.insert("thoughtSignature".to_string(), json!(sig));
                            }
                        }
                    }

                    *parts = new_parts;
                }

                // 全协议通用工具回执与多模态解构治理（无论当前外层 role 是 user 还是 model）：
                // 1. 若客户端回传的 functionCall.args 为 JSON 字符串，尝试反序列化为 Object，防上游 400
                // 2. 若客户端回传的 functionResponse.response 为非 Object，自动包装为 {"output": response}
                // 3. 多模态工具响应深度解构：确保全协议 (OpenAI / Claude / Gemini / Responses) 的工具结果中夹带的图片均被提升为独立的 inlineData 视觉感知输入
                let mut extra_inline_parts = Vec::new();
                for part in parts.iter_mut() {
                    if let Some(fc) = part.get_mut("functionCall") {
                        if let Some(args_val) = fc.get_mut("args") {
                            if let Some(args_str) = args_val.as_str() {
                                if let Ok(parsed_obj) = serde_json::from_str::<Value>(args_str) {
                                    if parsed_obj.is_object() {
                                        *args_val = parsed_obj;
                                    }
                                }
                            }
                        }
                    }
                    if let Some(fr) = part.get_mut("functionResponse") {
                        // 不按工具名豁免。名字会随协议改写（local_shell_call → shell）。
                        // 日志里的 data:image 由 extract_multimodal_from_tool_text 按形态拒绝。
                        if let Some(resp) = fr.get_mut("response") {
                            if !resp.is_object() {
                                *resp = json!({ "output": resp.clone() });
                            } else if let Some(resp_obj) = resp.as_object_mut() {
                                // [Pipeline First] 对齐原生 Antigravity 铁律：工具回执必须使用 "output" 字段，消除 "result" 等方言差异
                                if !resp_obj.contains_key("output")
                                    && resp_obj.contains_key("result")
                                {
                                    if let Some(res_val) = resp_obj.remove("result") {
                                        resp_obj.insert("output".to_string(), res_val);
                                    }
                                }
                            }
                            for key in ["result", "output"] {
                                if let Some(v) = resp.get_mut(key) {
                                    if let Some(s) = v.as_str() {
                                        if s.contains("data:image/") {
                                            let clean_s = crate::proxy::mappers::common_utils::extract_multimodal_from_tool_text(s, &mut extra_inline_parts);
                                            *v = json!(clean_s);
                                        }
                                    }
                                }
                            }
                            // [Pipeline First] 空输出防幻觉统一兜底：避免空字符串导致模型幻觉或 Gemini 400 校验异常
                            if let Some(resp_obj) = resp.as_object_mut() {
                                if let Some(v) = resp_obj.get_mut("output") {
                                    if let Some(s) = v.as_str() {
                                        if s.trim().is_empty() {
                                            *v = json!("Command executed successfully.");
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                parts.extend(extra_inline_parts);
            }
        }

        // 2. 状态机无损复活 (Hydration)
        // 开思考时全局复活；关思考时若包含工具调用，亦执行复活以取回历史工具防伪签名
        let has_function_call = contents.iter().any(|c| {
            c.get("parts")
                .and_then(|p| p.as_array())
                .map_or(false, |parts| {
                    parts.iter().any(|p| p.get("functionCall").is_some())
                })
        });
        if (is_thinking_enabled || has_function_call) && !is_retry {
            if let Some(s_id) = session_id {
                crate::proxy::thinking_store::hydrate_gemini_contents_with_model(
                    s_id,
                    contents,
                    Some(target_model),
                );
            }
        }

        // 3. 终审把关与脱敏规范化 (Finalize)
        crate::proxy::thinking_store::finalize_gemini_contents_thinking_with_session(
            contents,
            is_thinking_enabled,
            Some(target_model),
            session_id,
        );

        // 4. 工具回执 role 归一化（对齐官方 Antigravity 形态，Gemini -> model, Claude -> user）。
        //    必须放在**末位**：若前置，回执轮会进入上面的 `is_model` 分支，
        //    从而跳过 `role == "user"` 分支里的多模态解构（图片提升为 inlineData）。
        Self::normalize_function_response_roles(contents, target_model);

        // 5. 全协议多模态保鲜滑动窗口与累积容量治理（Pipeline First）：
        let multimodal_cfg = crate::proxy::config::get_multimodal_config();
        Self::apply_multimodal_sliding_window_and_limits(contents, &multimodal_cfg);
    }

    /// 全协议多模态历史保鲜滑动窗口与累积容量治理（Pipeline First）：
    /// 1. 扫描所有 contents 中的图片 parts（包括 inlineData base64 与 fileData 远程直链）
    /// 2. 若启用了保鲜滑动窗口（enable_sliding_window=true 且 max_fresh_images > 0）：
    ///    - 逆序保鲜最近的 N 张图片（从最新轮次往前计数），保留其完整数据；
    ///    - 对超出保鲜窗口的早期历史图片：
    ///      * inlineData (Base64)：剥离 Base64 并替换为结构化占位符 `[Historical Image #k: omitted to preserve context (mime)]`
    ///      * fileData (远程直链)：若 strip_remote_urls=true，亦替换为 `[Historical Remote Image #k: omitted to preserve context]`；否则完整保留 URL
    /// 3. 内存防爆与累积容量兜底（max_total_image_mb）：
    ///    - 统计剩余所有 inlineData 图片的总字节大小
    ///    - 若依然超出上限，执行 LRU / Recency-First 淘汰：自底向上（从最早的历史轮次）继续将旧图剥离为占位符，
    ///      绝对力保当前轮（最新轮次）的视觉感知输入完好无损！
    pub fn apply_multimodal_sliding_window_and_limits(
        contents: &mut [Value],
        config: &crate::proxy::config::MultimodalConfig,
    ) -> usize {
        // 总开关未开启：100% 原始透传，绝对保真
        if !config.enable_sliding_window {
            return 0;
        }

        let mut image_locations: Vec<(usize, usize, bool, String, usize)> = Vec::new();
        // (content_idx, part_idx, is_remote, mime, approx_byte_len)

        for (c_idx, content) in contents.iter().enumerate() {
            if let Some(parts) = content.get("parts").and_then(Value::as_array) {
                for (p_idx, part) in parts.iter().enumerate() {
                    let inline_obj = part.get("inlineData").or_else(|| part.get("inline_data"));
                    if let Some(obj) = inline_obj {
                        let mime = obj
                            .get("mimeType")
                            .or_else(|| obj.get("mime_type"))
                            .and_then(Value::as_str)
                            .unwrap_or("image/png");
                        if mime.starts_with("image/") || mime.is_empty() {
                            let b64_len = obj
                                .get("data")
                                .and_then(Value::as_str)
                                .map_or(0, |s| s.len());
                            let approx_bytes = b64_len.saturating_mul(3) / 4;
                            image_locations.push((
                                c_idx,
                                p_idx,
                                false,
                                mime.to_string(),
                                approx_bytes,
                            ));
                        }
                    } else {
                        let file_obj = part.get("fileData").or_else(|| part.get("file_data"));
                        if let Some(obj) = file_obj {
                            let uri = obj
                                .get("fileUri")
                                .or_else(|| obj.get("file_uri"))
                                .and_then(Value::as_str)
                                .unwrap_or_default();
                            let mime = obj
                                .get("mimeType")
                                .or_else(|| obj.get("mime_type"))
                                .and_then(Value::as_str)
                                .unwrap_or("image/jpeg");
                            if uri.starts_with("http://") || uri.starts_with("https://") {
                                image_locations.push((
                                    c_idx,
                                    p_idx,
                                    true,
                                    mime.to_string(),
                                    uri.len(),
                                ));
                            }
                        }
                    }
                }
            }
        }

        let total_images = image_locations.len();
        if total_images == 0 {
            return 0;
        }

        let mut pruned_count = 0;

        if config.strategy.eq_ignore_ascii_case("memory") {
            // =========================================================================
            // 策略二：基于累积内存体积滑动保鲜 (Memory-Based Strategy)
            // 逆序从最新轮次往前回溯累加 Base64 字节，超出配额的更早历史图片剥离降级
            // =========================================================================
            let budget_bytes = config.max_total_image_mb.max(1).saturating_mul(1024 * 1024);
            let mut accumulated_bytes = 0usize;
            let mut keep_from_idx = 0usize;

            // 逆序扫描：从最新的一张图片（total_images - 1）往回计算
            for (i, (_, _, is_remote, _, bytes)) in image_locations.iter().enumerate().rev() {
                if !*is_remote {
                    // 最新的一张图片永远保留；其余图片在预算内保留
                    if accumulated_bytes.saturating_add(*bytes) <= budget_bytes
                        || i == total_images - 1
                    {
                        accumulated_bytes = accumulated_bytes.saturating_add(*bytes);
                    } else {
                        keep_from_idx = i + 1;
                        break;
                    }
                }
            }

            // 对超出内存预算的早期历史图片进行剥离
            for i in 0..keep_from_idx {
                let (c_idx, p_idx, is_remote, mime, _) = &image_locations[i];
                let seq = i + 1;
                if *is_remote {
                    if config.strip_remote_urls {
                        if let Some(parts) = contents[*c_idx]
                            .get_mut("parts")
                            .and_then(Value::as_array_mut)
                        {
                            parts[*p_idx] = json!({
                                "text": format!("[Historical Remote Image #{}: omitted due to memory budget]", seq)
                            });
                            pruned_count += 1;
                        }
                    }
                } else if let Some(parts) = contents[*c_idx]
                    .get_mut("parts")
                    .and_then(Value::as_array_mut)
                {
                    parts[*p_idx] = json!({
                        "text": format!("[Historical Image #{}: omitted due to memory budget ({}, {} MB max)]", seq, mime, config.max_total_image_mb)
                    });
                    pruned_count += 1;
                }
            }
        } else {
            // =========================================================================
            // 策略一：基于图片张数滑动保鲜 (Count-Based Strategy)
            // 逆序保鲜最近的 N 张图片，超过 N 张的早期历史图片剥离降级
            // =========================================================================
            if config.max_fresh_images > 0 && total_images > config.max_fresh_images {
                let to_prune = total_images.saturating_sub(config.max_fresh_images);
                for i in 0..to_prune {
                    let (c_idx, p_idx, is_remote, mime, _) = &image_locations[i];
                    let seq = i + 1;
                    if *is_remote {
                        if config.strip_remote_urls {
                            if let Some(parts) = contents[*c_idx]
                                .get_mut("parts")
                                .and_then(Value::as_array_mut)
                            {
                                parts[*p_idx] = json!({
                                    "text": format!("[Historical Remote Image #{}: omitted to preserve context]", seq)
                                });
                                pruned_count += 1;
                            }
                        }
                    } else if let Some(parts) = contents[*c_idx]
                        .get_mut("parts")
                        .and_then(Value::as_array_mut)
                    {
                        parts[*p_idx] = json!({
                            "text": format!("[Historical Image #{}: omitted to preserve context ({})]", seq, mime)
                        });
                        pruned_count += 1;
                    }
                }
            }
        }

        if pruned_count > 0 {
            tracing::info!(
                "[Multimodal-Pipeline] Pruned {} historical image(s) via strategy '{}' (total: {}, fresh_limit: {}, max_mb: {})",
                pruned_count,
                config.strategy,
                total_images,
                config.max_fresh_images,
                config.max_total_image_mb,
            );
        }

        pruned_count
    }

    /// 工具调用 ID 统一规范化治理（Pipeline First）：
    /// 规范化 contents 中所有的 `functionCall` 与 `functionResponse` 的 `id`，
    /// 统一对齐为带下划线的标准形态 (`call_...`)，杜绝客户端格式漂移导致的签名与思考回填脱落。
    ///
    /// 返回被规范化的 ID 数量。
    pub fn normalize_tool_call_ids(contents: &mut [Value]) -> usize {
        let mut normalized_count = 0usize;
        for content in contents.iter_mut() {
            if let Some(parts) = content.get_mut("parts").and_then(|p| p.as_array_mut()) {
                for part in parts.iter_mut() {
                    if let Some(fc) = part.get_mut("functionCall") {
                        if let Some(id_val) = fc.get("id").and_then(|v| v.as_str()) {
                            let norm = crate::proxy::common::utils::normalize_tool_id(id_val);
                            if norm.as_ref() != id_val {
                                fc["id"] = serde_json::json!(norm.as_ref());
                                normalized_count += 1;
                            }
                        }
                    }
                    if let Some(fr) = part.get_mut("functionResponse") {
                        if let Some(id_val) = fr.get("id").and_then(|v| v.as_str()) {
                            let norm = crate::proxy::common::utils::normalize_tool_id(id_val);
                            if norm.as_ref() != id_val {
                                fr["id"] = serde_json::json!(norm.as_ref());
                                normalized_count += 1;
                            }
                        }
                    }
                }
            }
        }
        if normalized_count > 0 {
            tracing::debug!(
                "[InboundPipeline] Normalized {} tool call/response ID(s) to canonical 'call_...' format",
                normalized_count
            );
        }
        normalized_count
    }

    /// 连续工具调用轮次合并（Pipeline First 统一治理）：
    /// 当客户端将并行工具调用拆为多个连续的 assistant/model 轮次时，
    /// 统一合并为一个单一的 model content 轮次。
    /// 遵循 Google v1internal 规则：一个 model 轮次内仅首个部件需带签名，后续部件可不带签名；
    /// 若拆为多个 model 轮次，后续轮次将因缺失独立签名被直接拒收 400。
    pub fn merge_consecutive_function_call_turns(contents: &mut Vec<Value>) -> usize {
        fn is_pure_tool_call_turn(content: &Value) -> bool {
            let role = content.get("role").and_then(|r| r.as_str());
            if !matches!(role, Some("model") | Some("assistant")) {
                return false;
            }
            let parts = match content.get("parts").and_then(|p| p.as_array()) {
                Some(p) if !p.is_empty() => p,
                _ => return false,
            };
            let has_fc = parts.iter().any(|p| p.get("functionCall").is_some());
            let has_plain_text = parts.iter().any(|p| {
                p.get("functionCall").is_none()
                    && p.get("functionResponse").is_none()
                    && !crate::proxy::thinking_store::is_thought_part(p)
                    && p.get("text").and_then(|t| t.as_str()).map_or(false, |s| {
                        !crate::proxy::thinking_store::is_placeholder_thought(s)
                            && !s.trim().is_empty()
                    })
            });
            has_fc && !has_plain_text
        }

        let mut merged: Vec<Value> = Vec::with_capacity(contents.len());
        let mut merged_count = 0;

        for content in contents.drain(..) {
            if is_pure_tool_call_turn(&content) {
                if let Some(prev) = merged.last_mut() {
                    if is_pure_tool_call_turn(prev) {
                        if let (Some(prev_parts), Some(curr_parts)) = (
                            prev.get_mut("parts").and_then(|p| p.as_array_mut()),
                            content.get("parts").and_then(|p| p.as_array()),
                        ) {
                            for part in curr_parts {
                                if !crate::proxy::thinking_store::is_thought_part(part) {
                                    prev_parts.push(part.clone());
                                }
                            }
                            merged_count += 1;
                            continue;
                        }
                    }
                }
            }
            merged.push(content);
        }
        *contents = merged;
        merged_count
    }

    /// 把工具回执（`functionResponse`）轮的 role 归一化为官方 Antigravity 形态，并将连续回执合并打包。
    ///
    /// **官方形态**（实测与 flows(6) 26,695 次工具抓包逐字核对）：
    /// - **Gemini 目标**：`functionResponse` 恒位于 `role: "model"` 的 content 中，紧跟同 role 的 `functionCall` content 之后。
    /// - **Claude 目标**：`functionResponse` 恒位于 `role: "user"` 的 content 中，对齐 Anthropic 协议规范。
    /// - **跨模型多轮切换**：原生客户端会根据当前目标模型，将整个历史对话中的 `functionResponse` 统一转换为该目标模型要求的形态。
    /// 当上一轮为并行工具调用时，所有的工具回执无条件封装在**同一个单一的 target_role content 块中**。
    ///
    /// **核心签名校验机理**：
    /// Google v1internal 强制要求：一个 model 轮次内仅首个部件需带签名，后续部件可不带签名；
    /// 但若分拆为多个独立 content 块，上游会判定每一个独立的 model 轮次都必须携带签名！
    /// 回执部件本身绝不携带签名，故多个分散的回执 content 块必然触发 400 校验终止。
    /// 将连续回执无条件聚合在同一个 content 块内，不仅与官方报文 100% 结构同构，而且彻底消除了签名报错根源！
    ///
    /// 返回被改写与合并的 content 数。
    pub fn normalize_function_response_roles(
        contents: &mut Vec<Value>,
        target_model: &str,
    ) -> usize {
        let is_claude = crate::models::OfficialModelCatalog::get(target_model).map_or_else(
            || target_model.to_lowercase().contains("claude"),
            |m| m.is_claude(),
        );
        let target_role = if is_claude { "user" } else { "model" };

        /// part 是否携带工具回执本体。
        fn has_fr(p: &Value) -> bool {
            p.get("functionResponse").is_some()
        }
        /// part 是否仅为随行媒体（无文本、无回执、无调用）。
        fn is_media(p: &Value) -> bool {
            (p.get("inlineData").is_some() || p.get("inline_data").is_some())
                && p.get("text").is_none()
                && !has_fr(p)
                && p.get("functionCall").is_none()
        }

        let mut rewritten = 0usize;
        let mut out: Vec<Value> = Vec::with_capacity(contents.len());

        for content in contents.drain(..) {
            let role = content
                .get("role")
                .and_then(|r| r.as_str())
                .unwrap_or("user")
                .to_string();

            let parts = match content.get("parts").and_then(|p| p.as_array()) {
                Some(p) if !p.is_empty() => p.clone(),
                _ => {
                    out.push(content);
                    continue;
                }
            };

            // 不含回执本体的轮次原样放行
            if !parts.iter().any(has_fr) {
                out.push(content);
                continue;
            }

            let all_response = parts.iter().all(|p| has_fr(p) || is_media(p));

            // 已经是目标角色且为纯回执轮，无需改写，直接流入聚合阶段
            if role == target_role && all_response {
                out.push(content);
                continue;
            }

            // 纯回执轮：改写 role 为 target_role
            if all_response {
                if target_role == "model" && out.is_empty() {
                    out.push(content);
                    continue;
                }
                let mut c = content;
                c["role"] = json!(target_role);
                out.push(c);
                rewritten += 1;
                continue;
            }

            // 混合轮：按「回执族 / 非回执族」切段，保持 parts 相对顺序
            let mut segments: Vec<(bool, Vec<Value>)> = Vec::new();
            for part in parts {
                let flag = if has_fr(&part) {
                    true
                } else if is_media(&part) {
                    // 随行媒体跟随前驱族群；无前驱则视为普通内容
                    segments.last().map(|(f, _)| *f).unwrap_or(false)
                } else {
                    false
                };
                match segments.last_mut() {
                    Some((f, seg)) if *f == flag => seg.push(part),
                    _ => segments.push((flag, vec![part])),
                }
            }

            if segments.len() <= 1 {
                let mut c = content;
                if role != target_role {
                    c["role"] = json!(target_role);
                    rewritten += 1;
                }
                out.push(c);
                continue;
            }

            // 首段若为回执族且前面没有已产出的 content，且目标为 model 时会破坏「以 user 开头」
            let mut first = true;
            for (flag, seg) in segments {
                let seg_role = if flag {
                    if target_role == "model" && first && out.is_empty() {
                        role.as_str()
                    } else {
                        target_role
                    }
                } else {
                    role.as_str()
                };
                out.push(json!({ "role": seg_role, "parts": seg }));
                first = false;
            }
            rewritten += 1;
        }

        // 连续纯回执轮统一聚合（对齐官方形态 + 消除多 content 独立校验签名报错）：
        // 在官方 Antigravity 报文以及 Google v1internal 规范中，并行调用的所有工具回执必须封装在同一个单一 content 块中（Gemini 为 role:"model"，Claude 为 role:"user"）。
        // 将连续的纯工具回执轮（含随行媒体）全部合并进首个回执轮的 parts 中。
        let mut merged: Vec<Value> = Vec::with_capacity(out.len());
        for content in out {
            let role = content
                .get("role")
                .and_then(|r| r.as_str())
                .unwrap_or("user");
            let is_pure_response = role == target_role
                && content
                    .get("parts")
                    .and_then(|p| p.as_array())
                    .map_or(false, |parts| {
                        parts.iter().any(has_fr) && parts.iter().all(|p| has_fr(p) || is_media(p))
                    });

            if is_pure_response {
                if let Some(prev) = merged.last_mut() {
                    let prev_role = prev.get("role").and_then(|r| r.as_str()).unwrap_or("user");
                    let prev_is_pure_response = prev_role == target_role
                        && prev
                            .get("parts")
                            .and_then(|p| p.as_array())
                            .map_or(false, |parts| {
                                parts.iter().any(has_fr)
                                    && parts.iter().all(|p| has_fr(p) || is_media(p))
                            });

                    if prev_is_pure_response {
                        if let (Some(prev_parts), Some(curr_parts)) = (
                            prev.get_mut("parts").and_then(|p| p.as_array_mut()),
                            content.get("parts").and_then(|p| p.as_array()),
                        ) {
                            prev_parts.extend(curr_parts.clone());
                            rewritten += 1;
                            continue;
                        }
                    }
                }
            }
            merged.push(content);
        }

        // [Pipeline First] 回执轮部件稳定双阶段分区（Stable Partition）：
        // 彻底根除并发多模态（带图）工具回执中媒体插队破坏 functionResponse 连续性的问题 (Fixes #3560, #3094)。
        // 严格遵循上游工具回执状态机规范：同一轮次中，所有的 functionResponse 必须紧凑连续置顶在前，
        // 所有的随行多模态数据（inlineData 等）与辅助说明统一稳定延后追加在末尾。
        for content in merged.iter_mut() {
            let has_any_fr = content
                .get("parts")
                .and_then(|p| p.as_array())
                .map_or(false, |parts| parts.iter().any(has_fr));
            if has_any_fr {
                if let Some(parts) = content.get_mut("parts").and_then(|p| p.as_array_mut()) {
                    let mut fr_parts = Vec::new();
                    let mut other_parts = Vec::new();
                    for part in parts.drain(..) {
                        if has_fr(&part) {
                            fr_parts.push(part);
                        } else {
                            other_parts.push(part);
                        }
                    }
                    parts.extend(fr_parts);
                    parts.extend(other_parts);
                }
            }
        }

        // [zwx-patch] 回执轮随行媒体拆出为紧随其后的 user 轮：
        // 上游不接受以携带 inlineData 的 model 轮结尾（400 "Requests ending with a model turn are not supported."），
        // 典型触发为 Codex 的 view_image 工具回执。无论是否处于末尾都拆分，保证多轮历史形态一致。
        let mut split: Vec<Value> = Vec::with_capacity(merged.len());
        for mut content in merged {
            let is_model_response = content.get("role").and_then(|r| r.as_str()) == Some("model")
                && content
                    .get("parts")
                    .and_then(|p| p.as_array())
                    .map_or(false, |parts| {
                        parts.iter().any(has_fr) && parts.iter().any(is_media)
                    });
            if !is_model_response {
                split.push(content);
                continue;
            }
            let parts = content
                .get_mut("parts")
                .and_then(|p| p.as_array_mut())
                .map(std::mem::take)
                .unwrap_or_default();
            let (media, rest): (Vec<Value>, Vec<Value>) = parts.into_iter().partition(is_media);
            content["parts"] = json!(rest);
            split.push(content);
            split.push(json!({ "role": "user", "parts": media }));
            rewritten += 1;
        }

        *contents = split;
        rewritten
    }

    /// 统一进站思考配置与参数治理（流水线节点）：
    /// 保证四大协议（OpenAI, Claude, Gemini, Codex）的协议无关性。
    /// 1. 自动识别目标模型（包括 Tiered 自适应模型与具名模型）
    /// 2. 忽略客户端数字 budget 防污染，精准捕获客户端无后缀思考参数 (low/medium/high)
    /// 3. 在网关控制模式下，思考参数仅对 Tiered 模型开放操控权，分别映射至网关思考板块的 flash_low, flash_medium, flash_high
    /// 4. 组装并规范化 generationConfig 中的 thinkingConfig 与 maxOutputTokens
    pub fn configure_inbound_thinking(
        target_model: &str,
        generation_config: &mut Value,
        client_switch: ClientThinkingSwitch,
        client_effort: Option<&str>,
        client_budget: Option<u64>,
        token: Option<&crate::proxy::token_manager::ProxyToken>,
    ) -> Option<i64> {
        let is_under_v3 = crate::proxy::model_specs::is_gemini_under_v3(target_model);
        if is_under_v3 {
            // Gemini < 3 非思考模型严禁注入 thinkingConfig
            if let Some(obj) = generation_config.as_object_mut() {
                obj.remove("thinkingConfig");
                obj.remove("thinking_config");
            }
            return None;
        }

        let tb_config = crate::proxy::config::get_thinking_budget_config();

        // ════════════════════════════════════════════════════════════════════
        // 模式分流 1: 客户端自填控制模式（Client Direct Control）
        // 遵循最高指令：思考开关 > 思考等级 > 思考预算，缺省默认开，上游自适应
        // ════════════════════════════════════════════════════════════════════
        if tb_config.control_source == crate::proxy::config::ThinkingControlSource::Client {
            match client_switch {
                ClientThinkingSwitch::Disabled => {
                    // 1. 思考开关显式关闭（一票否决）：彻底不带 thinkingConfig，不填预算
                    if let Some(obj) = generation_config.as_object_mut() {
                        obj.remove("thinkingConfig");
                        obj.remove("thinking_config");
                    }
                    return None;
                }
                ClientThinkingSwitch::Enabled | ClientThinkingSwitch::Default => {
                    // 2. 允许思考（显式开 OR 缺省默认开）
                    // 2.1 预算显式 (> 0)：忠实透传预算数字，绝不脑补等级（防止 Google 400 双字段冲突），必须带上 includeThoughts: true
                    if let Some(budget) = client_budget.filter(|&b| b > 0) {
                        generation_config["thinkingConfig"] = json!({
                            "includeThoughts": true,
                            "thinkingBudget": budget
                        });
                        // 确保 maxOutputTokens 大于 thinkingBudget 避免 400
                        let min_overhead = 8192;
                        let current_max = generation_config
                            .get("maxOutputTokens")
                            .and_then(Value::as_i64)
                            .unwrap_or(65536);
                        if current_max <= budget as i64 {
                            generation_config["maxOutputTokens"] =
                                json!(budget as i64 + min_overhead);
                        }
                        return Some(budget as i64);
                    }

                    // 2.2 预算缺省，但客户端携带了思考等级（包括 low / medium / high 以及任何客户自定义的思考等级）：
                    // 核心铁律：坚决不填预算！忠实透传等级，并且必须带上 includeThoughts: true 核心开关！
                    if let Some(raw_effort) = client_effort.map(str::trim).filter(|s| !s.is_empty())
                    {
                        let lower_effort = raw_effort.to_lowercase();
                        if lower_effort != "default"
                            && lower_effort != "none"
                            && lower_effort != "off"
                            && lower_effort != "disabled"
                        {
                            let final_level = match lower_effort.as_str() {
                                "low" | "extra-low" | "min" | "minimal" => "LOW".to_string(),
                                "medium" | "normal" | "standard" => {
                                    if target_model.to_lowercase().contains("pro") {
                                        "HIGH".to_string()
                                    } else {
                                        "MEDIUM".to_string()
                                    }
                                }
                                "high" | "xhigh" | "max" | "extreme" => "HIGH".to_string(),
                                // 客户带了任何自定义等级，直接忠实透传，绝不硬编码限制！
                                _ => raw_effort.to_uppercase(),
                            };
                            generation_config["thinkingConfig"] = json!({
                                "includeThoughts": true,
                                "thinkingLevel": final_level
                            });
                            return None;
                        }
                    }

                    // 2.3 等级与预算均缺省（或 default）：全部预算不传递，默认上游处理（上游自适应）
                    // ★ 绝对不塞 4000/Medium 预算，仅带 includeThoughts: true
                    generation_config["thinkingConfig"] = json!({
                        "includeThoughts": true
                    });
                    return None;
                }
            }
        }

        // ════════════════════════════════════════════════════════════════════
        // 模式分流 2: 网关权威控制模式（Gateway Authority，99% 用户）
        // 100% 保持原有权威逻辑不变：档位锁死、flash_low/med/high 映射、防 429 注入
        // ════════════════════════════════════════════════════════════════════
        let resolved_budget = crate::proxy::model_specs::resolve_custom_budget(
            target_model,
            client_effort,
            client_budget,
            &tb_config,
            token,
        );

        let is_tiered = crate::proxy::model_specs::is_tiered_flash_model(target_model)
            || target_model.to_lowercase().contains("tiered");

        let mut tc = json!({
            "includeThoughts": true
        });

        // 统一从官方模型结构体中读取权威默认值
        let official_info = crate::models::OfficialModelCatalog::get(target_model);

        // 如果官方模型结构体明确不支持思考（如纯图片生成模型），则不注入 thinkingConfig
        if let Some(ref info) = official_info {
            if info.supports_thinking == Some(false) {
                if let Some(obj) = generation_config.as_object_mut() {
                    obj.remove("thinkingConfig");
                    obj.remove("thinking_config");
                }
                return None;
            }
        }

        // 用户核心要求：
        // "如果我网关模式的思考预算填-1 我的策略是不填模型预算。其实是不对的
        // 应该是如果网关模式都填了-1 应该默认走官方模型结构体的默认值"
        let final_budget = match resolved_budget {
            Some(b) => Some(b),
            None => {
                // 网关模式下未显式配置自定义预算（Default 默认模式）：
                // 默认走官方模型结构体的默认值 (official_model.thinking_budget)；
                // 若官方模型结构体无记录（如非官方目录或旧版别名），Claude 思考模型回落到标准限额
                official_info
                    .as_ref()
                    .and_then(|info| info.thinking_budget)
                    .or_else(|| {
                        if target_model.to_lowercase().contains("claude") {
                            Some(
                                crate::proxy::model_specs::get_thinking_budget(target_model, token)
                                    as i64,
                            )
                        } else {
                            None
                        }
                    })
            }
        };

        if let Some(budget) = final_budget {
            if budget == 0 {
                tc = json!({
                    "thinkingBudget": 0
                });
            } else {
                tc["thinkingBudget"] = json!(budget);

                // 确保 maxOutputTokens 大于 thinkingBudget 避免 400 (仅当 budget > 0 时)
                if budget > 0 {
                    let min_overhead = 8192;
                    let current_max = generation_config
                        .get("maxOutputTokens")
                        .and_then(Value::as_i64)
                        .unwrap_or(65536);
                    if current_max <= budget {
                        generation_config["maxOutputTokens"] = json!(budget + min_overhead);
                    }
                }
            }
            generation_config["thinkingConfig"] = tc;
            return Some(budget);
        } else if is_tiered {
            // Tiered 模型未指定具体数字 budget 且官方结构体无 thinking_budget 时：纯自适应模式
            tc = json!({
                "includeThoughts": true
            });
            generation_config["thinkingConfig"] = tc;
            return None;
        }

        generation_config["thinkingConfig"] = tc;

        // 终审上限保护
        let target_lower = target_model.to_lowercase();
        let safe_limit = if target_lower.contains("claude") {
            64000
        } else if target_lower.contains("pro") {
            65535
        } else {
            65536
        };
        if let Some(val) = generation_config["maxOutputTokens"].as_i64() {
            if val > safe_limit {
                generation_config["maxOutputTokens"] = json!(safe_limit);
            }
        }

        resolved_budget
    }

    /// 统一规范化与对齐四大协议转译后的 Google Request 内部拓扑（Pipeline First 核心归一节点）
    /// 确保 OpenAI Chat, OpenAI Responses, Claude 与 Gemini 在上游呈现 100% 严格对齐官方 Antigravity 的结构：
    /// 1. contents: 上下文历史置于首位
    /// 2. systemInstruction: 统一规整内部 role ("user") -> parts 键序
    /// 3. tools: 清理 Schema，递归转大写 type，绝不拦截任何客户端工具，统一拆分为单函数独立包装形态并按 name 字母序稳定排序
    /// 4. labels: 官方上下文标签元数据
    /// 5. generationConfig: 吸收根节点 thinkingConfig，配齐 maxOutputTokens: 65536，杜绝伪造 topK/topP
    /// 6. sessionId: 会话标识
    /// 7. 严格顺序重组: contents -> systemInstruction -> tools -> labels -> generationConfig -> sessionId -> 其余
    /// 从官方 requestId（形如 agent/<conv_id>/<timestamp>/<trajectory_id>/<step>）提取 trajectory_id，
    /// 若无法提取则生成全新的标准 UUID
    fn extract_or_generate_trajectory_id(
        top_request_id: Option<&str>,
        session_id: Option<&str>,
    ) -> String {
        if let Some(rid) = top_request_id {
            let parts: Vec<&str> = rid.split('/').collect();
            if parts.len() >= 5 && !parts[3].is_empty() {
                return parts[3].to_string();
            }
        }
        if let Some(sid) = session_id {
            if sid.len() >= 32 && sid.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
                return sid.to_string();
            }
        }
        uuid::Uuid::new_v4().to_string()
    }

    /// 统一对齐与规范化 Google Request 内部拓扑结构与稳定前缀（Pipeline First 核心归一）
    pub fn align_google_request_prefix_topology(inner_request: &mut Value) {
        Self::align_google_request_prefix_topology_with_model(inner_request, "", None);
    }

    /// 支持传入 target_model 与 top_request_id 的增强版拓扑对齐:
    /// 对齐官方标准:
    /// 1. contents: 强行复位至最首位
    /// 2. systemInstruction: 统一为 { role: "user", parts: [...] }
    /// 3. tools: 拆解为单函数独立对象 [ { functionDeclarations: [single_decl] } ]，保持客户端原序
    /// 4. labels: 官方模型标签，对齐 Claude 与 Gemini 家族的专属特征 (model_enum, used_claude, used_claude_conservative, used_non_gemini_model)
    /// 5. generationConfig: 吸收根节点 thinkingConfig，按模型设置 maxOutputTokens (Claude: 64000, Gemini: 65536)，保留既有思考预算
    /// 6. sessionId: 会话标识
    /// 7. 严格顺序重组: contents -> systemInstruction -> tools -> labels -> generationConfig -> sessionId -> 其余
    pub fn align_google_request_prefix_topology_with_model(
        inner_request: &mut Value,
        target_model: &str,
        top_request_id: Option<&str>,
    ) {
        let req_obj = match inner_request.as_object_mut() {
            Some(o) => o,
            None => return,
        };

        // 1. contents (官方报文中 contents 置于首位)
        let mut canonical_contents = req_obj.remove("contents").unwrap_or(json!([]));

        // [Gatekeeper 2026-09-29 / 2026-09-30 Pipeline First] Gemini 目标出站终审安全自愈门禁：
        // Google Gemini 3+ 规则：全会话历史与活跃轮次中的所有工具调用 (Function Calling)
        // 均被 Google AST 校验器严格强制检查 thoughtSignature。
        // 若任意历史或活跃轮次的某个 functionCall 缺少签名，出站前统一自动注入官方合法哨兵 SENTINEL_SIGNATURE，彻底杜绝上游 400！
        if target_model.to_lowercase().contains("gemini") {
            if let Some(contents_arr) = canonical_contents.as_array_mut() {
                for content in contents_arr.iter_mut() {
                    if let Some(parts) = content.get_mut("parts").and_then(|p| p.as_array_mut()) {
                        for part in parts.iter_mut() {
                            if part.get("functionCall").is_some() {
                                let has_sig = part
                                    .get("thoughtSignature")
                                    .or_else(|| part.get("thought_signature"))
                                    .and_then(|s| s.as_str())
                                    .map_or(false, |s| !s.trim().is_empty());
                                if !has_sig {
                                    tracing::warn!(
                                        "[Gatekeeper] Found functionCall missing thoughtSignature in turn, auto-injecting sentinel!"
                                    );
                                    part["thoughtSignature"] =
                                        json!(crate::proxy::thinking_store::SENTINEL_SIGNATURE);
                                }
                            }
                        }
                    }
                }
            }
        }

        // 2. systemInstruction (规范化统一键序: role -> parts -> 其余)
        let canonical_si = if let Some(si) = req_obj.remove("systemInstruction") {
            if let Some(si_obj) = si.as_object() {
                let mut c = serde_json::Map::new();
                let role = si_obj.get("role").cloned().unwrap_or_else(|| json!("user"));
                c.insert("role".to_string(), role);
                if let Some(parts) = si_obj.get("parts") {
                    c.insert("parts".to_string(), parts.clone());
                }
                for (k, v) in si_obj {
                    if k != "role" && k != "parts" {
                        c.insert(k.clone(), v.clone());
                    }
                }
                Some(Value::Object(c))
            } else {
                Some(si)
            }
        } else {
            None
        };

        // 3. tools: 规范化 parameters，拆成单函数独立对象，保持客户端声明顺序
        let canonical_tools = if let Some(tools) = req_obj.remove("tools") {
            if let Some(tools_arr) = tools.as_array() {
                let mut expanded_tools: Vec<Value> = Vec::new();
                let mut decls_list: Vec<Value> = Vec::new();

                for tool in tools_arr {
                    if let Some(obj) = tool.as_object() {
                        let decls_opt = obj
                            .get("functionDeclarations")
                            .or_else(|| obj.get("function_declarations"))
                            .and_then(|v| v.as_array());

                        if let Some(decls) = decls_opt {
                            for decl in decls {
                                if let Some(mut decl_obj) = decl.as_object().cloned() {
                                    if let Some(params_json_schema) =
                                        decl_obj.remove("parametersJsonSchema")
                                    {
                                        let mut params = params_json_schema;
                                        crate::proxy::common::json_schema::clean_json_schema(
                                            &mut params,
                                        );
                                        crate::proxy::mappers::openai::request::enforce_uppercase_types(
                                            &mut params,
                                        );
                                        decl_obj.insert("parameters".to_string(), params);
                                    } else if let Some(params) = decl_obj.get_mut("parameters") {
                                        crate::proxy::common::json_schema::clean_json_schema(
                                            params,
                                        );
                                        crate::proxy::mappers::openai::request::enforce_uppercase_types(
                                            params,
                                        );
                                    }

                                    decls_list.push(Value::Object(decl_obj));
                                }
                            }
                        } else {
                            // 非 functionDeclaration 工具（如 googleSearch, codeExecution）
                            expanded_tools.push(tool.clone());
                        }
                    }
                }

                for decl in decls_list {
                    expanded_tools.push(json!({
                        "functionDeclarations": [decl]
                    }));
                }

                if !expanded_tools.is_empty() {
                    Some(Value::Array(expanded_tools))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        // [Pipeline First] 客户端有就传，没有就不传：
        // 提取客户端显式指定的 toolConfig（如由 Claude / OpenAI 适配器根据 tool_choice 翻译而来，或 Gemini 客户端显式指定）。
        // 若客户端未传，绝不主动伪造或注入任何默认配置（与官方 IDE 原生报文保持 100% 干净拓扑同构）；
        // 若客户端显式指定了有效配置（如 mode == ANY 或包含 allowedFunctionNames），必须予以完整保留并发送给上游！
        let canonical_tool_config = req_obj
            .remove("toolConfig")
            .or_else(|| req_obj.remove("tool_config"))
            .filter(|v| {
                if let Some(obj) = v.as_object() {
                    !obj.is_empty()
                } else {
                    false
                }
            });

        // 4. labels 提取保留与官方模型家族严密对齐:
        let mut canonical_labels = req_obj.remove("labels");

        let mut labels_map = canonical_labels
            .as_ref()
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();

        let step_idx_str = labels_map
            .get("last_step_index")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                // 1. 优先从 top_request_id (形如 agent/.../.../<traj>/<step>) 解析 step 编号:
                // last_step_index 即为上一步已完成的索引 (step - 1)
                if let Some(rid) = top_request_id {
                    let parts: Vec<&str> = rid.split('/').collect();
                    if parts.len() >= 5 {
                        if let Ok(step) = parts[4].parse::<u64>() {
                            return step.saturating_sub(1).to_string();
                        }
                    }
                }
                // 2. 备用从 contents 历史对话中计算已完成的模型轮次数 (每轮 model 即为一个已完成 step)
                let model_turns = canonical_contents
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter(|c| c.get("role").and_then(|r| r.as_str()) == Some("model"))
                            .count()
                    })
                    .unwrap_or(0);
                model_turns.to_string()
            });

        let traj_id = labels_map
            .get("trajectory_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                Self::extract_or_generate_trajectory_id(
                    top_request_id,
                    req_obj.get("sessionId").and_then(|v| v.as_str()),
                )
            });

        let req_id_in_labels = labels_map
            .get("request_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("{}-{}", traj_id, step_idx_str));

        labels_map.insert("last_step_index".to_string(), json!(step_idx_str));
        labels_map.insert("request_id".to_string(), json!(req_id_in_labels));
        labels_map.insert("trajectory_id".to_string(), json!(traj_id));

        // 动态根据用户请求的目标模型 ID 从官方模型结构体中提取模型代号与元数据
        let official_model = crate::models::OfficialModelCatalog::get(target_model)
            .unwrap_or_else(crate::models::OfficialModelCatalog::default_model);

        let model_code = official_model.model.as_str();
        let is_claude = official_model.is_claude();
        let is_non_gemini = official_model.is_non_gemini();

        let has_valid_model_enum = labels_map
            .get("model_enum")
            .and_then(|v| v.as_str())
            .map_or(false, |s| !s.is_empty());
        if !has_valid_model_enum {
            labels_map.insert("model_enum".to_string(), json!(model_code));
        }

        labels_map.insert(
            "used_claude".to_string(),
            json!(if is_claude { "true" } else { "false" }),
        );
        // 桌面端 Claude 为 true，Gemini 为 false。不要写死 false。
        labels_map.insert(
            "used_claude_conservative".to_string(),
            json!(if is_claude { "true" } else { "false" }),
        );
        labels_map.insert(
            "used_non_gemini_model".to_string(),
            json!(if is_non_gemini { "true" } else { "false" }),
        );

        // 按照官方标准排布 labels 键序 (字母顺序)
        let mut ordered_labels = serde_json::Map::new();
        for key in &[
            "last_step_index",
            "model_enum",
            "request_id",
            "trajectory_id",
            "used_claude",
            "used_claude_conservative",
            "used_non_gemini_model",
        ] {
            if let Some(val) = labels_map.remove(*key) {
                ordered_labels.insert((*key).to_string(), val);
            }
        }
        for (k, v) in labels_map {
            ordered_labels.insert(k, v);
        }
        canonical_labels = Some(Value::Object(ordered_labels));

        // 5. thinkingConfig 归一吸收与 generationConfig 拓扑对齐:
        // 如果 inner_request 根节点携带 thinkingConfig / thinking_config (如 JeikCode 客户端行为)，
        // 必须将其抽取吸收到 generationConfig.thinkingConfig 中，并从根节点彻底清除。
        let root_tc = req_obj
            .remove("thinkingConfig")
            .or_else(|| req_obj.remove("thinking_config"));

        let mut gc_obj = req_obj
            .remove("generationConfig")
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();

        if let Some(mut tc) = root_tc {
            if !gc_obj.contains_key("thinkingConfig") {
                if let Some(tc_obj) = tc.as_object_mut() {
                    if let Some(tb) = tc_obj.remove("thinking_budget") {
                        if !tc_obj.contains_key("thinkingBudget") {
                            tc_obj.insert("thinkingBudget".to_string(), tb);
                        }
                    }
                }
                gc_obj.insert("thinkingConfig".to_string(), tc);
            }
        }

        // 官方标准：从官方模型结构体中动态读取 maxOutputTokens
        // 用户规则明确指出："思考预算你不用动" (保持既有 thinkingConfig 预算)
        if let Some(official_max_output) = official_model.max_output_tokens {
            if !gc_obj.contains_key("maxOutputTokens")
                || (is_claude
                    && gc_obj.get("maxOutputTokens").and_then(Value::as_i64) == Some(65536))
            {
                gc_obj.insert("maxOutputTokens".to_string(), json!(official_max_output));
            }
        } else if gc_obj.contains_key("thinkingConfig") && !gc_obj.contains_key("maxOutputTokens") {
            gc_obj.insert("maxOutputTokens".to_string(), json!(65536));
        }

        // 规范化 generationConfig 内部键序: candidateCount -> maxOutputTokens -> thinkingConfig -> 其余
        let canonical_gc = if !gc_obj.is_empty() {
            let mut ordered_gc = serde_json::Map::new();
            for key in &[
                "candidateCount",
                "maxOutputTokens",
                "thinkingConfig",
                "temperature",
                "topP",
                "topK",
            ] {
                if let Some(val) = gc_obj.remove(*key) {
                    ordered_gc.insert((*key).to_string(), val);
                }
            }
            for (k, v) in gc_obj {
                ordered_gc.insert(k, v);
            }
            Some(Value::Object(ordered_gc))
        } else {
            None
        };

        // 6. sessionId 提取
        let canonical_sid = req_obj.remove("sessionId");

        // 7. safetySettings (仅当客户端显式传递且非空时保留，官方默认不携带，杜绝伪造 4 项 OFF)
        let canonical_safety = req_obj
            .remove("safetySettings")
            .filter(|v| v.as_array().map_or(false, |a| !a.is_empty()));

        // 8. 严格对齐官方 Topology 键序:
        // contents -> systemInstruction -> tools -> labels -> generationConfig -> sessionId -> safetySettings -> 其余
        let mut reordered = serde_json::Map::new();
        reordered.insert("contents".to_string(), canonical_contents);
        if let Some(si) = canonical_si {
            reordered.insert("systemInstruction".to_string(), si);
        }
        if let Some(tools) = canonical_tools {
            reordered.insert("tools".to_string(), tools);
        }
        if let Some(tc) = canonical_tool_config {
            reordered.insert("toolConfig".to_string(), tc);
        }
        if let Some(labels) = canonical_labels {
            reordered.insert("labels".to_string(), labels);
        }
        if let Some(gc) = canonical_gc {
            reordered.insert("generationConfig".to_string(), gc);
        }
        if let Some(sid) = canonical_sid {
            reordered.insert("sessionId".to_string(), sid);
        }
        if let Some(ss) = canonical_safety {
            reordered.insert("safetySettings".to_string(), ss);
        }

        // 保留其余未知/特定扩展字段在末尾
        for (k, v) in std::mem::take(req_obj) {
            if !reordered.contains_key(&k) {
                reordered.insert(k, v);
            }
        }
        *inner_request = Value::Object(reordered);
    }

    /// 统一规范化出站顶层信封结构（Pipeline First 核心归一出口）
    /// 确保输出至 Google v1internal 接口的报文 100% 完美对齐官方 Antigravity 报文结构:
    /// 1. 消除 _session_thinking_id (若 requestId 缺失则提升为 requestId)
    /// 2. 移除违规计费标号 enabledCreditTypes
    /// 3. 对齐统一 project ("aicode-consumers")、userAgent ("antigravity") 与 requestType ("agent")
    /// 4. 统一处理内部 request 拓扑对齐
    /// 5. 顶层键序严格对齐: project -> requestId -> request -> model -> userAgent -> requestType -> 其余
    pub fn align_official_envelope(body: &mut Value) {
        if !body.is_object() {
            return;
        }

        let body_obj = match body.as_object_mut() {
            Some(obj) => obj,
            None => return,
        };

        // 1. 处理 _session_thinking_id
        let session_thinking_id = body_obj.remove("_session_thinking_id");

        // 2. 移除 enabledCreditTypes
        body_obj.remove("enabledCreditTypes");

        // 3. 处理 requestId
        let mut request_id = body_obj.remove("requestId");
        if request_id
            .as_ref()
            .and_then(|v| v.as_str())
            .map_or(true, |s| s.is_empty())
        {
            if let Some(stid) = session_thinking_id {
                request_id = Some(stid);
            }
        }

        // 4. 处理 project (默认对齐官方 aicode-consumers)
        let project = body_obj
            .remove("project")
            .unwrap_or_else(|| json!("aicode-consumers"));

        // 5. 处理 userAgent (默认对齐官方 antigravity)
        let user_agent = body_obj
            .remove("userAgent")
            .unwrap_or_else(|| json!("antigravity"));

        // 6. 处理 model
        let model = body_obj.remove("model");
        let model_str = model
            .as_ref()
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let top_rid_str = request_id
            .as_ref()
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // 7. 处理 requestType
        let mut request_type = body_obj.remove("requestType");

        // 8. 规范化内部 request
        if let Some(inner_req) = body_obj.get_mut("request") {
            Self::align_google_request_prefix_topology_with_model(
                inner_req,
                &model_str,
                top_rid_str.as_deref(),
            );

            // 若 requestType 缺失，根据 tools 或 contents 判定是否为 agent 请求
            if request_type.is_none() {
                let has_tools = inner_req
                    .get("tools")
                    .and_then(|t| t.as_array())
                    .map_or(false, |a| !a.is_empty());
                let has_tool_interactions = inner_req
                    .get("contents")
                    .map(crate::proxy::mappers::common_utils::contents_has_tool_interactions)
                    .unwrap_or(false);
                if has_tools || has_tool_interactions {
                    request_type = Some(json!("agent"));
                }
            }

            // 若 requestId 仍未生成，根据 sessionId 与 contents 步数构建官方 requestId
            if request_id
                .as_ref()
                .and_then(|v| v.as_str())
                .map_or(true, |s| s.is_empty())
            {
                let sid = inner_req
                    .get("sessionId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("default");
                let step = inner_req
                    .get("contents")
                    .and_then(|c| c.as_array())
                    .map_or(0, |a| a.len() as u64);
                request_id = Some(json!(
                    crate::proxy::mappers::common_utils::build_official_request_id(sid, step)
                ));
            }
        } else {
            // 如果顶层没有 request 包装，直接规范化 body 自身
            Self::align_google_request_prefix_topology_with_model(
                body,
                &model_str,
                top_rid_str.as_deref(),
            );
            return;
        }

        let inner_request = body_obj.remove("request");

        // 9. 顶层严格键序重排:
        // project -> requestId -> request -> model -> userAgent -> requestType -> 其余
        let mut reordered = serde_json::Map::new();
        reordered.insert("project".to_string(), project);
        if let Some(rid) = request_id {
            reordered.insert("requestId".to_string(), rid);
        }
        if let Some(req) = inner_request {
            reordered.insert("request".to_string(), req);
        }
        if let Some(m) = model {
            reordered.insert("model".to_string(), m);
        }
        reordered.insert("userAgent".to_string(), user_agent);
        if let Some(rt) = request_type {
            reordered.insert("requestType".to_string(), rt);
        }

        // 其余未知顶层扩展字段保留在末尾
        for (k, v) in std::mem::take(body_obj) {
            if !reordered.contains_key(&k) {
                reordered.insert(k, v);
            }
        }

        *body = Value::Object(reordered);
    }

    /// 剥离遗留思考块的前缀标记 (**Thinking**)
    fn strip_thinking_prefix(text: &str) -> String {
        let trimmed = text.trim_start();
        if let Some(rest) = trimmed.strip_prefix("**Thinking**") {
            let rest = rest.trim_start_matches(':');
            rest.trim_start_matches(|c| c == '\r' || c == '\n' || c == ' ' || c == '\t')
                .to_string()
        } else {
            text.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preserves_process_commentary_alongside_tool_call() {
        let mut contents = vec![json!({
            "role": "model",
            "parts": [
                { "text": "正在检查网关与后端的连接配置。" },
                {
                    "functionCall": {
                        "name": "inspect_case",
                        "args": { "case": "case_1" }
                    }
                }
            ]
        })];

        InboundThinkingPipeline::process_contents(
            &mut contents,
            "gemini-2.5-pro",
            true,
            None,
            false,
        );

        let parts = contents[0]["parts"].as_array().expect("parts array");
        // 官方 HAR 确认：Gemini model 轮绝不凭空注入占位思考块，
        // 普通进度文本（过程注释）与工具调用均完整保留，顺序不变。
        // thought × functionCall 在官方报文中共现 0 次，不该注入哨兵。
        assert_eq!(
            parts.len(),
            2,
            "commentary + functionCall, no injected thought block"
        );
        assert_eq!(parts[0]["text"], "正在检查网关与后端的连接配置。");
        assert!(parts[1].get("functionCall").is_some());
    }

    #[test]
    fn test_preserves_multiple_plain_text_parts_intact() {
        let mut contents = vec![json!({
            "role": "model",
            "parts": [
                { "text": "第一阶段：检查概览。" },
                { "text": "第二阶段：深入诊断。" },
                {
                    "functionCall": {
                        "name": "run_check",
                        "args": {}
                    }
                }
            ]
        })];

        InboundThinkingPipeline::process_contents(
            &mut contents,
            "gemini-2.5-pro",
            false,
            None,
            false,
        );

        let parts = contents[0]["parts"].as_array().expect("parts array");
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0]["text"], "第一阶段：检查概览。");
        assert_eq!(parts[1]["text"], "第二阶段：深入诊断。");
        assert!(parts[2].get("functionCall").is_some());
    }

    #[test]
    fn test_heals_legacy_thinking_prefix_without_corrupting_prose() {
        let mut contents = vec![json!({
            "role": "model",
            "parts": [
                { "text": "**Thinking**\n\n分析了案例数据，准备调用工具。" },
                { "text": "正在执行检查。" },
                {
                    "functionCall": {
                        "name": "inspect",
                        "args": {}
                    }
                }
            ]
        })];

        InboundThinkingPipeline::process_contents(
            &mut contents,
            "gemini-2.5-pro",
            true,
            None,
            false,
        );

        let parts = contents[0]["parts"].as_array().expect("parts array");
        assert!(parts[0]
            .get("thought")
            .and_then(Value::as_bool)
            .unwrap_or(false));
        assert_eq!(parts[0]["text"], "分析了案例数据，准备调用工具。");
        assert_eq!(parts[1]["text"], "正在执行检查。");
        assert!(parts[2].get("functionCall").is_some());
    }

    #[test]
    fn test_claude_model_packages_signature_for_google_vertex() {
        let raw_claude_sig = "Eu8CCpIBCBIQAhgCKkAtARbmpPNxYxc/Yz+mpbWJOqMo9c9RF4ESxACD0e/d6SZTpwmbrf9gPP/XMGZ9+kBkTMBfdK7ICuVonHJuu1AcMg9jbGF1ZGUtb3B1cy00LTY4AEIIdGhpbmtpbmdaDDg4NDM1NDkxOTA1MnIQLmWKBlED8AVhXRwj5Lb+PogBAagBosG91QawAQISDFXhwclEQyYNjsDteRoMuuu1Y/dUbn7sPe5OIjBoGvxrSlIgU78kwl701wfF0Rj0BCaCpE6a+KRGaB5pO2vL3ox4+yqum5a8o7mQ8+kqiQFDBTvaDITieiRVrkA8EKBUrpV0rLDyEcL7iQnAMsdQOk31ZKDeBddhEVX+Tb7Qs9mNWXNW9cbrs82iea09O+j2IMs0ibbWXPHB20IlkhVc5q9MmKBYgeQSTzKz+8Tgf7EDd78lkYieVk6GHqQaNiWD1Sl+RO0mIDGwURmOON6Fyw6WkCh/WSF+ORgB";
        let expected_google_vertex_sig = "RXU4Q0NwSUJDQklRQWhnQ0trQXRBUmJtcFBOeFl4Yy9ZeittcGJXSk9xTW85YzlSRjRFU3hBQ0QwZS9kNlNaVHB3bWJyZjlnUFAvWE1HWjkra0JrVE1CZmRLN0lDdVZvbkhKdXUxQWNNZzlqYkdGMVpHVXRiM0IxY3kwMExUWTRBRUlJZEdocGJtdHBibWRhRERnNE5ETTFORGt4T1RBMU1uSVFMbVdLQmxFRDhBVmhYUndqNUxiK1BvZ0JBYWdCb3NHOTFRYXdBUUlTREZYaHdjbEVReVlOanNEdGVSb011dXUxWS9kVWJuN3NQZTVPSWpCb0d2eHJTbElnVTc4a3dsNzAxd2ZGMFJqMEJDYUNwRTZhK0tSR2FCNXBPMnZMM294NCt5cXVtNWE4bzdtUTgra3FpUUZEQlR2YURJVGllaVJWcmtBOEVLQlVycFYwckxEeUVjTDdpUW5BTXNkUU9rMzFaS0RlQmRkaEVWWCtUYjdRczltTldYTlc5Y2JyczgyaWVhMDlPK2oySU1zMGliYldYUEhCMjBJbGtoVmM1cTlNbUtCWWdlUVNUekt6KzhUZ2Y3RURkNzhsa1lpZVZrNkdIcVFhTmlXRDFTbCtSTzBtSURHd1VSbU9PTjZGeXc2V2tDaC9XU0YrT1JnQg==";

        let mut contents = vec![json!({
            "role": "model",
            "parts": [
                {
                    "text": "Let me think about this.",
                    "thought": true,
                    "thoughtSignature": raw_claude_sig
                },
                {
                    "text": "Here is the response."
                }
            ]
        })];

        InboundThinkingPipeline::process_contents(
            &mut contents,
            "claude-opus-4-6-thinking",
            true,
            None,
            false,
        );

        let parts = contents[0]["parts"].as_array().expect("parts array");
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0]["thought"], true);
        assert!(parts[0].get("thoughtSignature").is_none());
        assert_eq!(parts[1]["text"], "Here is the response.");
        assert_eq!(parts[1]["thoughtSignature"], expected_google_vertex_sig);
    }

    #[test]
    fn test_claude_model_from_openai_protocol_packages_signature() {
        let raw_claude_sig = "Eu8CCpIBCBIQAhgCKkAtARbmpPNxYxc/Yz+mpbWJOqMo9c9RF4ESxACD0e/d6SZTpwmbrf9gPP/XMGZ9+kBkTMBfdK7ICuVonHJuu1AcMg9jbGF1ZGUtb3B1cy00LTY4AEIIdGhpbmtpbmdaDDg4NDM1NDkxOTA1MnIQLmWKBlED8AVhXRwj5Lb+PogBAagBosG91QawAQISDFXhwclEQyYNjsDteRoMuuu1Y/dUbn7sPe5OIjBoGvxrSlIgU78kwl701wfF0Rj0BCaCpE6a+KRGaB5pO2vL3ox4+yqum5a8o7mQ8+kqiQFDBTvaDITieiRVrkA8EKBUrpV0rLDyEcL7iQnAMsdQOk31ZKDeBddhEVX+Tb7Qs9mNWXNW9cbrs82iea09O+j2IMs0ibbWXPHB20IlkhVc5q9MmKBYgeQSTzKz+8Tgf7EDd78lkYieVk6GHqQaNiWD1Sl+RO0mIDGwURmOON6Fyw6WkCh/WSF+ORgB";
        let expected_google_vertex_sig = "RXU4Q0NwSUJDQklRQWhnQ0trQXRBUmJtcFBOeFl4Yy9ZeittcGJXSk9xTW85YzlSRjRFU3hBQ0QwZS9kNlNaVHB3bWJyZjlnUFAvWE1HWjkra0JrVE1CZmRLN0lDdVZvbkhKdXUxQWNNZzlqYkdGMVpHVXRiM0IxY3kwMExUWTRBRUlJZEdocGJtdHBibWRhRERnNE5ETTFORGt4T1RBMU1uSVFMbVdLQmxFRDhBVmhYUndqNUxiK1BvZ0JBYWdCb3NHOTFRYXdBUUlTREZYaHdjbEVReVlOanNEdGVSb011dXUxWS9kVWJuN3NQZTVPSWpCb0d2eHJTbElnVTc4a3dsNzAxd2ZGMFJqMEJDYUNwRTZhK0tSR2FCNXBPMnZMM294NCt5cXVtNWE4bzdtUTgra3FpUUZEQlR2YURJVGllaVJWcmtBOEVLQlVycFYwckxEeUVjTDdpUW5BTXNkUU9rMzFaS0RlQmRkaEVWWCtUYjdRczltTldYTlc5Y2JyczgyaWVhMDlPK2oySU1zMGliYldYUEhCMjBJbGtoVmM1cTlNbUtCWWdlUVNUekt6KzhUZ2Y3RURkNzhsa1lpZVZrNkdIcVFhTmlXRDFTbCtSTzBtSURHd1VSbU9PTjZGeXc2V2tDaC9XU0YrT1JnQg==";

        let mut contents = vec![json!({
            "role": "model",
            "parts": [
                {
                    "text": "Thinking process",
                    "thought": true,
                    "thoughtSignature": raw_claude_sig
                },
                {
                    "text": "Answer from OpenAI gateway"
                }
            ]
        })];

        InboundThinkingPipeline::process_contents(
            &mut contents,
            "claude-sonnet-4-6",
            true,
            None,
            false,
        );

        let parts = contents[0]["parts"].as_array().expect("parts array");
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0]["thought"], true);
        assert!(parts[0].get("thoughtSignature").is_none());
        assert_eq!(parts[1]["text"], "Answer from OpenAI gateway");
        assert_eq!(parts[1]["thoughtSignature"], expected_google_vertex_sig);
    }

    #[test]
    fn test_gemini_native_signature_preserved_without_double_encoding() {
        let gemini_sig = "EudDCuRDAWkUfRO9pMXsHitwdfey4TDAgCv1WzzMfBXVamvaqJ01BJPawr58";

        let mut contents = vec![json!({
            "role": "model",
            "parts": [
                {
                    "text": "Gemini thinking",
                    "thought": true,
                },
                {
                    "thoughtSignature": gemini_sig,
                    "functionCall": {
                        "name": "bash",
                        "args": { "command": "ls" }
                    }
                }
            ]
        })];

        InboundThinkingPipeline::process_contents(
            &mut contents,
            "gemini-3.8-flash-high",
            true,
            None,
            false,
        );

        let parts = contents[0]["parts"].as_array().expect("parts array");
        assert_eq!(parts.len(), 1);
        assert!(parts[0].get("functionCall").is_some());
        assert!(parts[0].get("thought").is_none());
        assert_eq!(parts[0]["thoughtSignature"], gemini_sig);
    }

    #[test]
    fn test_inbound_pipeline_intercepts_foreign_claude_signature_for_gemini() {
        let foreign_claude_sig = "3mgp11XmVXq9InniGA4VAKd7c97NqFw+dWZt79Uz/w9znho88gSM76jv2bZmir7wI86Ixpha7eWdGuznAot4PNbe3+V9bgMTIEyUarn4MLAiiFVb830ZlM+H5ukQwXdD2Zv8nUSmmZTYinpLPGha8TORZAfpU1FJEvwyECel5+W7kc9kpTWrd8DqRNBTOz5EDtvoatiZgKv5SqInhGXK74SJ+PRIC6fNXvYG082HR6TsVxvVYaerz8A40rloIVTxRNK43h3Ecs1boxY4PZqBT8Yhl2qn/iZ+4Xt7FNkI0DAuS9iK0HYKMC4yw0OqKx/LeU+WFZlyc6hGm1BkzLY6yG97MH7kmJ0OPlBWgWFaTeL/uXuGJX6QkKObXN+phoq+kkF2vdFt/mdJMbdgfmSCVQ9037hGBhOHm0zN50KLkp1SxuAY1oWc+lDcI4ufWoyn";

        let mut contents = vec![json!({
            "role": "model",
            "parts": [
                {
                    "text": "Cross-model thinking from Claude",
                    "thought": true,
                    "thoughtSignature": foreign_claude_sig
                },
                {
                    "thoughtSignature": foreign_claude_sig,
                    "functionCall": {
                        "name": "bash",
                        "args": { "command": "ls" }
                    }
                }
            ]
        })];

        InboundThinkingPipeline::process_contents(
            &mut contents,
            "gemini-3.7-flash-high",
            true,
            None,
            false,
        );

        let parts = contents[0]["parts"].as_array().expect("parts array");
        assert_eq!(parts.len(), 1);
        assert!(parts[0].get("functionCall").is_some());
        assert!(parts[0].get("thought").is_none());
        assert!(
            parts[0].get("thoughtSignature").is_none(),
            "FunctionCall must NOT fall back to rejected sentinel signature"
        );
    }

    #[test]
    fn test_inbound_pipeline_intercepts_foreign_gemini_signature_for_claude() {
        // 模拟 Gemini 原生签名
        let foreign_gemini_sig =
            "Ep4KCpsKAWkUfRMa5ZYMDdlPjxrQTLzVZ6MZeopI88888888888888888888888888888888";

        let mut contents = vec![
            json!({
                "role": "user",
                "parts": [{ "text": "hello" }]
            }),
            json!({
                "role": "model",
                "parts": [
                    {
                        "text": "The input is a Chinese greeting...",
                        "thought": true,
                        "thoughtSignature": foreign_gemini_sig
                    },
                    {
                        "text": "Hello! How can I help you today?"
                    }
                ]
            }),
            json!({
                "role": "user",
                "parts": [{ "text": "continue" }]
            }),
        ];

        InboundThinkingPipeline::process_contents(
            &mut contents,
            "claude-opus-4-6-thinking",
            true,
            None,
            false,
        );

        let model_parts = contents[1]["parts"].as_array().expect("parts array");
        let has_thought_block = model_parts
            .iter()
            .any(|p| p.get("thought").and_then(|v| v.as_bool()) == Some(true));
        assert!(has_thought_block, "Claude history keeps thought text");
        let has_gemini_sig = model_parts.iter().any(|p| {
            p.get("thoughtSignature").and_then(|v| v.as_str()) == Some(foreign_gemini_sig)
                || p.get("thought_signature").and_then(|v| v.as_str()) == Some(foreign_gemini_sig)
        });
        assert!(
            !has_gemini_sig,
            "Foreign Gemini signature must not stay on a Claude turn"
        );
    }

    #[test]
    fn test_inbound_pipeline_lifts_multimodal_images_from_function_response() {
        let fake_b64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
        let mut contents = vec![json!({
            "role": "user",
            "parts": [{
                "functionResponse": {
                    "name": "take_screenshot",
                    "response": {
                        "output": format!("Screenshot result: ![view](data:image/png;base64,{}) done.", fake_b64)
                    }
                }
            }]
        })];

        InboundThinkingPipeline::process_contents(
            &mut contents,
            "gemini-2.5-flash",
            false,
            None,
            false,
        );

        let parts = contents[0]["parts"].as_array().expect("parts array");
        assert_eq!(
            parts.len(),
            2,
            "Should have functionResponse and lifted inlineData"
        );
        assert!(parts[0].get("functionResponse").is_some());
        assert!(parts[1].get("inlineData").is_some());

        assert_eq!(parts[1]["inlineData"]["mimeType"], "image/png");
        assert_eq!(parts[1]["inlineData"]["data"], fake_b64);

        let output_text = parts[0]["functionResponse"]["response"]["output"]
            .as_str()
            .unwrap();
        assert!(!output_text.contains(fake_b64));
        assert!(output_text.contains("[Image: forwarded to visual input (image/png)]"));
    }

    #[test]
    fn test_inbound_pipeline_never_lifts_images_from_terminal_tools() {
        let fake_b64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
        let mut contents = vec![json!({
            "role": "user",
            "parts": [{
                "functionResponse": {
                    "name": "run_command",
                    "response": {
                        "output": format!("image_url prefix: data:image/png;base64,{} len: 100", fake_b64)
                    }
                }
            }]
        })];

        InboundThinkingPipeline::process_contents(
            &mut contents,
            "gemini-2.5-flash",
            false,
            None,
            false,
        );

        let parts = contents[0]["parts"].as_array().expect("parts array");
        assert_eq!(
            parts.len(),
            1,
            "Terminal command output must remain 100% text without lifting inlineData"
        );
        assert!(parts[0].get("functionResponse").is_some());
        let output_text = parts[0]["functionResponse"]["response"]["output"]
            .as_str()
            .unwrap();
        assert!(
            output_text.contains(fake_b64),
            "Terminal command output must be 100% transparent and untainted"
        );
    }

    #[test]
    fn test_inbound_pipeline_empty_tool_result_sanitizes_to_default_output() {
        let mut contents = vec![json!({
            "role": "user",
            "parts": [{
                "functionResponse": {
                    "name": "edit_file",
                    "response": {
                        "output": "   "
                    }
                }
            }]
        })];

        InboundThinkingPipeline::process_contents(
            &mut contents,
            "claude-sonnet-4-6",
            false,
            None,
            false,
        );

        let parts = contents[0]["parts"].as_array().expect("parts array");
        let output_text = parts[0]["functionResponse"]["response"]["output"]
            .as_str()
            .unwrap();
        assert_eq!(output_text, "Command executed successfully.");
    }

    #[test]
    fn test_extract_client_thinking_switch_coverage() {
        // 1. 显式关闭 (一票否决)
        assert_eq!(
            extract_client_thinking_switch(Some("disabled"), None, None),
            ClientThinkingSwitch::Disabled
        );
        assert_eq!(
            extract_client_thinking_switch(Some("off"), None, None),
            ClientThinkingSwitch::Disabled
        );
        assert_eq!(
            extract_client_thinking_switch(None, Some(0), None),
            ClientThinkingSwitch::Disabled
        );
        assert_eq!(
            extract_client_thinking_switch(None, None, Some("none")),
            ClientThinkingSwitch::Disabled
        );
        assert_eq!(
            extract_client_thinking_switch(None, None, Some("off")),
            ClientThinkingSwitch::Disabled
        );

        // 2. 显式开启
        assert_eq!(
            extract_client_thinking_switch(Some("enabled"), None, None),
            ClientThinkingSwitch::Enabled
        );
        assert_eq!(
            extract_client_thinking_switch(None, Some(1024), None),
            ClientThinkingSwitch::Enabled
        );
        assert_eq!(
            extract_client_thinking_switch(None, None, Some("low")),
            ClientThinkingSwitch::Enabled
        );
        assert_eq!(
            extract_client_thinking_switch(None, None, Some("high")),
            ClientThinkingSwitch::Enabled
        );

        // 3. 缺省（开关缺省就是默认开）
        assert_eq!(
            extract_client_thinking_switch(None, None, None),
            ClientThinkingSwitch::Default
        );
        assert_eq!(
            extract_client_thinking_switch(Some("default"), None, None),
            ClientThinkingSwitch::Default
        );
        assert_eq!(
            extract_client_thinking_switch(None, None, Some("default")),
            ClientThinkingSwitch::Default
        );
    }

    #[test]
    fn test_configure_inbound_thinking_client_mode_routing_clean_isolation() {
        use crate::proxy::config::{
            update_thinking_budget_config, ThinkingBudgetConfig, ThinkingControlSource,
        };

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

        // 1. 显式关闭：彻底不带 thinkingConfig
        let mut gc1 = json!({
            "thinkingConfig": { "includeThoughts": true }
        });
        InboundThinkingPipeline::configure_inbound_thinking(
            "gemini-3.8-flash-tiered",
            &mut gc1,
            ClientThinkingSwitch::Disabled,
            None,
            None,
            None,
        );
        assert!(gc1.get("thinkingConfig").is_none());

        // 2. 缺省：includeThoughts=true，绝无 thinkingBudget
        let mut gc2 = json!({});
        InboundThinkingPipeline::configure_inbound_thinking(
            "gemini-3.8-flash-tiered",
            &mut gc2,
            ClientThinkingSwitch::Default,
            None,
            None,
            None,
        );
        let tc2 = gc2.get("thinkingConfig").unwrap().as_object().unwrap();
        assert_eq!(tc2.get("includeThoughts"), Some(&json!(true)));
        assert!(tc2.get("thinkingBudget").is_none());
        assert!(tc2.get("thinkingLevel").is_none());

        // 3. 显式等级：thinkingLevel=LOW / HIGH，绝无 thinkingBudget，必须带上 includeThoughts: true
        let mut gc3 = json!({});
        InboundThinkingPipeline::configure_inbound_thinking(
            "gemini-3.8-flash-tiered",
            &mut gc3,
            ClientThinkingSwitch::Enabled,
            Some("low"),
            None,
            None,
        );
        let tc3 = gc3.get("thinkingConfig").unwrap().as_object().unwrap();
        assert_eq!(tc3.get("includeThoughts"), Some(&json!(true)));
        assert_eq!(tc3.get("thinkingLevel"), Some(&json!("LOW")));
        assert!(tc3.get("thinkingBudget").is_none());

        // 3.1 客户端填了任何自定义等级（非硬编码）且没填预算，忠实透传等级，坚决不填预算
        let mut gc3_custom = json!({});
        let budget_custom_res = InboundThinkingPipeline::configure_inbound_thinking(
            "gemini-3.8-flash-tiered",
            &mut gc3_custom,
            ClientThinkingSwitch::Enabled,
            Some("custom_ultra_level"),
            None,
            None,
        );
        let tc3_custom = gc3_custom
            .get("thinkingConfig")
            .unwrap()
            .as_object()
            .unwrap();
        assert_eq!(tc3_custom.get("includeThoughts"), Some(&json!(true)));
        assert_eq!(
            tc3_custom.get("thinkingLevel"),
            Some(&json!("CUSTOM_ULTRA_LEVEL"))
        );
        assert!(tc3_custom.get("thinkingBudget").is_none(), "When client provides custom effort without budget, thinkingBudget must strictly remain None");
        assert!(budget_custom_res.is_none());

        // 4. 显式预算：thinkingBudget=8192，绝无 thinkingLevel，必须带上 includeThoughts: true
        let mut gc4 = json!({});
        InboundThinkingPipeline::configure_inbound_thinking(
            "gemini-3.8-flash-tiered",
            &mut gc4,
            ClientThinkingSwitch::Enabled,
            None,
            Some(8192),
            None,
        );
        let tc4 = gc4.get("thinkingConfig").unwrap().as_object().unwrap();
        assert_eq!(tc4.get("includeThoughts"), Some(&json!(true)));
        assert_eq!(tc4.get("thinkingBudget"), Some(&json!(8192)));
        assert!(tc4.get("thinkingLevel").is_none());
    }

    #[test]
    fn test_cross_family_think_tag_extraction_and_elevation_for_gemini() {
        let thought_text = "Analyzing user code structure and determining route.";
        let visible_answer = "The issue has been identified and isolated.";
        let wrapped_text = format!("<think>\n{}\n</think>\n\n{}", thought_text, visible_answer);

        let mut contents = vec![json!({
            "role": "model",
            "parts": [
                { "text": wrapped_text }
            ]
        })];

        InboundThinkingPipeline::process_contents(
            &mut contents,
            "gemini-3.8-flash-tiered",
            true,
            None,
            false,
        );

        let parts = contents[0]["parts"].as_array().expect("parts array");
        // 1. 首位成功提升为 thought: true 的思考块
        assert_eq!(parts[0]["thought"], true);
        assert_eq!(parts[0]["text"], thought_text);
        // 铁律 I4：Gemini 目标的思考块**绝不**携带签名。
        // 哨兵（skip_thought_signature_validator）不属于 Antigravity 协议 ——
        // 官方 3 份报文 23 处签名里出现 0 次。
        assert!(
            parts[0].get("thoughtSignature").is_none(),
            "Gemini 目标的思考块不得携带签名（I4）"
        );

        // 2. 正文部件已干净剔除 <think>...</think> 标签与换行，仅保留真实回答
        assert_eq!(parts[1]["text"], visible_answer);
        assert!(parts[1].get("thought").is_none());
    }

    // ============ 工具回执（functionResponse）role 归一化 ============
    //
    // 目标：与官方 Antigravity 形态一比一 —— `functionResponse` 位于 `role: "model"` 的
    // content 中。实测（gemini-3.8-flash-tiered @ daily）表明上游对 user / model 两种
    // role **完全宽容**，故本归一化是「对齐官方形态 + 稳定前缀字节」，
    // 并天然**向下兼容**两种入站形态。

    fn fr_part(id: &str, name: &str) -> Value {
        json!({"functionResponse": {"id": id, "name": name, "response": {"output": "ok"}}})
    }

    fn fc_part(id: &str, name: &str) -> Value {
        json!({"functionCall": {"id": id, "name": name, "args": {}}})
    }

    /// 官方形态（回执已在 `role:"model"`）→ 必须完全幂等。
    #[test]
    fn test_fr_role_official_shape_is_idempotent() {
        let mut contents = vec![
            json!({"role": "user", "parts": [{"text": "go"}]}),
            json!({"role": "model", "parts": [fc_part("c1", "view_file")]}),
            json!({"role": "model", "parts": [fr_part("c1", "view_file")]}),
            json!({"role": "user", "parts": [{"text": "next"}]}),
        ];
        let snapshot = contents.clone();
        let n = InboundThinkingPipeline::normalize_function_response_roles(
            &mut contents,
            "gemini-3.8-flash-low",
        );
        assert_eq!(n, 0, "官方形态应零改动");
        assert_eq!(contents, snapshot);
    }

    /// 入站形态：回执在 `role:"user"` → 归一到官方形态（`role:"model"`）。
    #[test]
    fn test_fr_role_user_response_is_mapped_to_model() {
        let mut contents = vec![
            json!({"role": "user", "parts": [{"text": "go"}]}),
            json!({"role": "model", "parts": [fc_part("c1", "view_file")]}),
            json!({"role": "user", "parts": [fr_part("c1", "view_file")]}),
            json!({"role": "user", "parts": [{"text": "next"}]}),
        ];
        let n = InboundThinkingPipeline::normalize_function_response_roles(
            &mut contents,
            "gemini-3.8-flash-low",
        );
        assert_eq!(n, 1);
        assert_eq!(contents.len(), 4, "纯回执轮只改 role，不增删 content");
        assert_eq!(contents[2]["role"], "model");
        assert_eq!(
            contents[2]["parts"][0]["functionResponse"]["name"],
            "view_file"
        );
        // 相邻轮次不受影响
        assert_eq!(contents[3]["role"], "user");
    }

    /// 并发回执：一轮多个回执整体迁移，**内部顺序原样保持**。
    #[test]
    fn test_fr_role_parallel_responses_kept_in_order() {
        let mut contents = vec![
            json!({"role": "user", "parts": [{"text": "go"}]}),
            json!({"role": "model", "parts": [fc_part("c1", "list_dir"), fc_part("c2", "run_command")]}),
            json!({"role": "user", "parts": [fr_part("c1", "list_dir"), fr_part("c2", "run_command")]}),
        ];
        let n = InboundThinkingPipeline::normalize_function_response_roles(
            &mut contents,
            "gemini-3.8-flash-low",
        );
        assert_eq!(n, 1);
        assert_eq!(contents[2]["role"], "model");
        let parts = contents[2]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0]["functionResponse"]["name"], "list_dir");
        assert_eq!(parts[1]["functionResponse"]["name"], "run_command");
    }

    /// 混合轮：`user [text, fr]` → `user[text]` + `model[fr]`，顺序保持。
    #[test]
    fn test_fr_role_mixed_turn_is_split_preserving_order() {
        let mut contents = vec![
            json!({"role": "user", "parts": [{"text": "go"}]}),
            json!({"role": "model", "parts": [fc_part("c1", "view_file")]}),
            json!({"role": "user", "parts": [{"text": "顺便说明"}, fr_part("c1", "view_file")]}),
        ];
        let n = InboundThinkingPipeline::normalize_function_response_roles(
            &mut contents,
            "gemini-3.8-flash-low",
        );
        assert_eq!(n, 1);
        assert_eq!(contents.len(), 4);
        assert_eq!(contents[2]["role"], "user");
        assert_eq!(contents[2]["parts"][0]["text"], "顺便说明");
        assert_eq!(contents[3]["role"], "model");
        assert!(contents[3]["parts"][0].get("functionResponse").is_some());
    }

    /// 回执附带图片：`user [fr, inlineData]` → `model [fr]` + `user [inlineData]`，
    /// 避免请求以携带 inlineData 的 model 轮结尾（上游 400）。
    #[test]
    fn test_fr_role_response_with_inline_data_splits_media_to_user() {
        let img = json!({"inlineData": {"mimeType": "image/png", "data": "AAA"}});
        let mut contents = vec![
            json!({"role": "user", "parts": [{"text": "go"}]}),
            json!({"role": "model", "parts": [fc_part("c1", "view_image")]}),
            json!({"role": "user", "parts": [fr_part("c1", "view_image"), img.clone()]}),
        ];
        let n = InboundThinkingPipeline::normalize_function_response_roles(
            &mut contents,
            "gemini-3.8-flash-low",
        );
        assert_eq!(n, 2);
        assert_eq!(contents.len(), 4);
        assert_eq!(contents[2]["role"], "model");
        assert_eq!(contents[2]["parts"].as_array().unwrap().len(), 1);
        assert!(contents[2]["parts"][0].get("functionResponse").is_some());
        assert_eq!(contents[3]["role"], "user");
        assert_eq!(contents[3]["parts"], json!([img]));
    }

    /// [FIX #3560, #3094] Claude 目标并发多图回执：
    /// 原始交替穿插 `[fr1, img1, fr2, img2]` 必须归一化重排为稳定的双阶段结构 `[fr1, fr2, img1, img2]`，
    /// 确保所有工具回执绝对连续排在最前面，防止破坏 Claude 逆向转译器状态机导致 400。
    #[test]
    fn test_fr_role_parallel_responses_with_images_topology_claude() {
        let img1 = json!({"inlineData": {"mimeType": "image/png", "data": "AAA"}});
        let img2 = json!({"inlineData": {"mimeType": "image/jpeg", "data": "BBB"}});
        let mut contents = vec![
            json!({"role": "user", "parts": [{"text": "read two pictures"}]}),
            json!({"role": "model", "parts": [fc_part("c1", "read_file"), fc_part("c2", "read_file")]}),
            json!({
                "role": "user",
                "parts": [
                    fr_part("c1", "read_file"),
                    img1.clone(), // 致命插队！
                    fr_part("c2", "read_file"),
                    img2.clone()
                ]
            }),
        ];

        InboundThinkingPipeline::normalize_function_response_roles(
            &mut contents,
            "claude-sonnet-4-6",
        );

        assert_eq!(contents.len(), 3);
        assert_eq!(contents[2]["role"], "user");
        let parts = contents[2]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 4);
        // 关键断言：前两个部件必须连续为 functionResponse，且保持原本相对顺序
        assert!(parts[0].get("functionResponse").is_some());
        assert_eq!(parts[0]["functionResponse"]["id"], "c1");
        assert!(parts[1].get("functionResponse").is_some());
        assert_eq!(parts[1]["functionResponse"]["id"], "c2");
        // 关键断言：后两个部件必须为随行媒体，且保持原本相对顺序
        assert_eq!(parts[2]["inlineData"]["data"], "AAA");
        assert_eq!(parts[3]["inlineData"]["data"], "BBB");
    }

    /// [FIX #3560, #3094] Gemini 目标并发多图回执：
    /// 原始交替穿插 `[fr1, img1, fr2, img2]` 经 Stable Partition 后，
    /// 随行媒体被整洁拆分入紧随其后的 user 轮，model 轮保持连续回执 `[fr1, fr2]`。
    #[test]
    fn test_fr_role_parallel_responses_with_images_topology_gemini() {
        let img1 = json!({"inlineData": {"mimeType": "image/png", "data": "AAA"}});
        let img2 = json!({"inlineData": {"mimeType": "image/jpeg", "data": "BBB"}});
        let mut contents = vec![
            json!({"role": "user", "parts": [{"text": "read two pictures"}]}),
            json!({"role": "model", "parts": [fc_part("c1", "read_file"), fc_part("c2", "read_file")]}),
            json!({
                "role": "user",
                "parts": [
                    fr_part("c1", "read_file"),
                    img1.clone(),
                    fr_part("c2", "read_file"),
                    img2.clone()
                ]
            }),
        ];

        InboundThinkingPipeline::normalize_function_response_roles(
            &mut contents,
            "gemini-3.8-flash-low",
        );

        // 拆分为：user(q) -> model(fc) -> model(fr1, fr2) -> user(img1, img2)
        assert_eq!(contents.len(), 4);
        assert_eq!(contents[2]["role"], "model");
        let model_parts = contents[2]["parts"].as_array().unwrap();
        assert_eq!(model_parts.len(), 2);
        assert_eq!(model_parts[0]["functionResponse"]["id"], "c1");
        assert_eq!(model_parts[1]["functionResponse"]["id"], "c2");

        assert_eq!(contents[3]["role"], "user");
        let user_parts = contents[3]["parts"].as_array().unwrap();
        assert_eq!(user_parts.len(), 2);
        assert_eq!(user_parts[0]["inlineData"]["data"], "AAA");
        assert_eq!(user_parts[1]["inlineData"]["data"], "BBB");
    }

    /// 已是 model 形态的媒体回执（历史轮）同样拆分，且再次处理幂等。
    #[test]
    fn test_fr_role_model_response_with_inline_data_split_is_idempotent() {
        let img = json!({"inlineData": {"mimeType": "image/png", "data": "AAA"}});
        let mut contents = vec![
            json!({"role": "user", "parts": [{"text": "go"}]}),
            json!({"role": "model", "parts": [fc_part("c1", "view_image")]}),
            json!({"role": "model", "parts": [fr_part("c1", "view_image"), img.clone()]}),
            json!({"role": "model", "parts": [{"text": "done"}]}),
            json!({"role": "user", "parts": [{"text": "next"}]}),
        ];
        InboundThinkingPipeline::normalize_function_response_roles(
            &mut contents,
            "gemini-3.8-flash-low",
        );
        assert_eq!(contents[2]["role"], "model");
        assert_eq!(contents[3], json!({"role": "user", "parts": [img]}));
        let snapshot = contents.clone();
        let n = InboundThinkingPipeline::normalize_function_response_roles(
            &mut contents,
            "gemini-3.8-flash-low",
        );
        assert_eq!(n, 0);
        assert_eq!(contents, snapshot);
    }

    /// 用户发图提问 `user [inlineData, text]` **不得**被误判为回执轮。
    #[test]
    fn test_fr_role_user_image_question_is_untouched() {
        let mut contents = vec![
            json!({"role": "user", "parts": [{"text": "go"}]}),
            json!({"role": "model", "parts": [{"text": "ok"}]}),
            json!({"role": "user", "parts": [
                {"inlineData": {"mimeType": "image/png", "data": "AAA"}},
                {"text": "这张图是什么"}
            ]}),
        ];
        let snapshot = contents.clone();
        let n = InboundThinkingPipeline::normalize_function_response_roles(
            &mut contents,
            "gemini-3.8-flash-low",
        );
        assert_eq!(n, 0, "无 functionResponse 的轮次绝不改写");
        assert_eq!(contents, snapshot);
    }

    /// 工具调用轮（`model [fc, fc]`）不受影响。
    #[test]
    fn test_fr_role_function_call_turns_untouched() {
        let mut contents = vec![
            json!({"role": "user", "parts": [{"text": "go"}]}),
            json!({"role": "model", "parts": [fc_part("c1", "a"), fc_part("c2", "b")]}),
        ];
        let snapshot = contents.clone();
        let n = InboundThinkingPipeline::normalize_function_response_roles(
            &mut contents,
            "gemini-3.8-flash-low",
        );
        assert_eq!(n, 0);
        assert_eq!(contents, snapshot);
    }

    /// 防御：首条 content 不得被改成 `model`（Gemini 要求对话以 user 开头）。
    #[test]
    fn test_fr_role_leading_response_stays_user() {
        let mut contents = vec![
            json!({"role": "user", "parts": [fr_part("c1", "view_file")]}),
            json!({"role": "user", "parts": [{"text": "hi"}]}),
        ];
        let n = InboundThinkingPipeline::normalize_function_response_roles(
            &mut contents,
            "gemini-3.8-flash-low",
        );
        assert_eq!(n, 0);
        assert_eq!(contents[0]["role"], "user");
    }

    /// 连续纯回执轮合并：来自 OpenAI 等客户端的分离回执消息统一合并为单一 model content 块。
    #[test]
    fn test_fr_role_consecutive_responses_merged_into_single_model_turn() {
        let mut contents = vec![
            json!({"role": "user", "parts": [{"text": "run both"}]}),
            json!({"role": "model", "parts": [fc_part("c1", "list_dir"), fc_part("c2", "run_command")]}),
            json!({"role": "user", "parts": [fr_part("c1", "list_dir")]}),
            json!({"role": "user", "parts": [fr_part("c2", "run_command")]}),
            json!({"role": "model", "parts": [{"text": "all done"}]}),
        ];
        let _ = InboundThinkingPipeline::normalize_function_response_roles(
            &mut contents,
            "gemini-3.8-flash-low",
        );
        assert_eq!(contents.len(), 4, "连续的 2 个回执应合并为 1 个 model 轮次");
        assert_eq!(contents[2]["role"], "model");
        let parts = contents[2]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0]["functionResponse"]["name"], "list_dir");
        assert_eq!(parts[1]["functionResponse"]["name"], "run_command");
        assert_eq!(contents[3]["role"], "model");
        assert_eq!(contents[3]["parts"][0]["text"], "all done");
    }

    /// 已在 model 轮次内的连续回执亦必须合并为一个 content 块。
    #[test]
    fn test_fr_role_consecutive_model_responses_merged_into_single_turn() {
        let mut contents = vec![
            json!({"role": "user", "parts": [{"text": "go"}]}),
            json!({"role": "model", "parts": [fc_part("c1", "view_file")]}),
            json!({"role": "model", "parts": [fr_part("c1", "view_file")]}),
            json!({"role": "model", "parts": [fr_part("c2", "run_command")]}),
        ];
        let _ = InboundThinkingPipeline::normalize_function_response_roles(
            &mut contents,
            "gemini-3.8-flash-low",
        );
        assert_eq!(contents.len(), 3, "2 个连续 model 回执轮应合并为 1 个");
        assert_eq!(contents[2]["role"], "model");
        let parts = contents[2]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0]["functionResponse"]["name"], "view_file");
        assert_eq!(parts[1]["functionResponse"]["name"], "run_command");
    }

    /// Claude 目标测试：回执必须在 `role:"user"` 中（对齐 Anthropic 官方规范与 flows(6) 846 次实测抓包）
    #[test]
    fn test_fr_role_claude_response_is_mapped_to_user() {
        let mut contents = vec![
            json!({"role": "user", "parts": [{"text": "go"}]}),
            json!({"role": "model", "parts": [fc_part("c1", "view_file")]}),
            json!({"role": "model", "parts": [fr_part("c1", "view_file")]}),
            json!({"role": "user", "parts": [{"text": "next"}]}),
        ];
        let n = InboundThinkingPipeline::normalize_function_response_roles(
            &mut contents,
            "claude-sonnet-4-6",
        );
        assert_eq!(n, 1);
        assert_eq!(contents[2]["role"], "user");
        assert_eq!(
            contents[2]["parts"][0]["functionResponse"]["name"],
            "view_file"
        );
        assert_eq!(contents[3]["role"], "user");
    }

    /// Claude 目标测试：并发工具调用的回执统一封装在单个 `role:"user"` 轮次中
    #[test]
    fn test_fr_role_claude_parallel_responses_merged_into_single_user_turn() {
        let mut contents = vec![
            json!({"role": "user", "parts": [{"text": "run both"}]}),
            json!({"role": "model", "parts": [fc_part("c1", "list_dir"), fc_part("c2", "run_command")]}),
            json!({"role": "user", "parts": [fr_part("c1", "list_dir")]}),
            json!({"role": "user", "parts": [fr_part("c2", "run_command")]}),
            json!({"role": "model", "parts": [{"text": "all done"}]}),
        ];
        let _ = InboundThinkingPipeline::normalize_function_response_roles(
            &mut contents,
            "claude-sonnet-4-6",
        );
        assert_eq!(contents.len(), 4, "连续的 2 个回执应合并为 1 个 user 轮次");
        assert_eq!(contents[2]["role"], "user");
        let parts = contents[2]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0]["functionResponse"]["name"], "list_dir");
        assert_eq!(parts[1]["functionResponse"]["name"], "run_command");
        assert_eq!(contents[3]["role"], "model");
    }

    /// 连续工具调用轮次合并测试：多个独立的 model 工具调用轮合并为单个 model 轮次
    #[test]
    fn test_fc_role_consecutive_tool_call_turns_merged() {
        let mut contents = vec![
            json!({"role": "user", "parts": [{"text": "parallel tools"}]}),
            json!({"role": "model", "parts": [fc_part("c1", "tool_a")]}),
            json!({"role": "model", "parts": [fc_part("c2", "tool_b")]}),
            json!({"role": "user", "parts": [fr_part("c1", "tool_a"), fr_part("c2", "tool_b")]}),
        ];
        let n = InboundThinkingPipeline::merge_consecutive_function_call_turns(&mut contents);
        assert_eq!(n, 1);
        assert_eq!(contents.len(), 3);
        assert_eq!(contents[1]["role"], "model");
        let parts = contents[1]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0]["functionCall"]["name"], "tool_a");
        assert_eq!(parts[1]["functionCall"]["name"], "tool_b");
    }

    /// 测试工具调用与回执 ID 统一归一化为 call_ 规范形态
    #[test]
    fn test_normalize_tool_call_ids() {
        let mut contents = vec![
            json!({"role": "user", "parts": [{"text": "read file"}]}),
            json!({
                "role": "model",
                "parts": [
                    {
                        "functionCall": {
                            "name": "read",
                            "id": "call573077",
                            "args": {"path": "USER.md"}
                        }
                    }
                ]
            }),
            json!({
                "role": "user",
                "parts": [
                    {
                        "functionResponse": {
                            "name": "read",
                            "id": "call573077",
                            "response": {"result": "hello"}
                        }
                    }
                ]
            }),
            json!({
                "role": "model",
                "parts": [
                    {
                        "functionCall": {
                            "name": "exec",
                            "id": "call_1542346", // 已带下划线，保持原样
                            "args": {"command": "ls"}
                        }
                    }
                ]
            }),
        ];

        let count = InboundThinkingPipeline::normalize_tool_call_ids(&mut contents);
        assert_eq!(count, 2, "应归一化 2 个丢失下划线的 tool call/response ID");

        // functionCall 验证
        assert_eq!(contents[1]["parts"][0]["functionCall"]["id"], "call_573077");
        // functionResponse 验证
        assert_eq!(
            contents[2]["parts"][0]["functionResponse"]["id"],
            "call_573077"
        );
        // 原生已带下划线不受影响
        assert_eq!(
            contents[3]["parts"][0]["functionCall"]["id"],
            "call_1542346"
        );
    }

    /// 测试不正常工具函数与回执的自愈清洗：
    /// 1. functionCall.args 为 JSON 字符串时自动反序列化为 Object
    /// 2. functionResponse.response 为非 Object (纯文本/数字/数组) 时自动包装为 {"output": ...}
    #[test]
    fn test_abnormal_tool_call_and_response_sanitization() {
        let mut contents = vec![
            json!({"role": "user", "parts": [{"text": "run"}]}),
            json!({
                "role": "model",
                "parts": [
                    {
                        "functionCall": {
                            "name": "shell",
                            "id": "call_12345",
                            "args": "{\"cmd\": \"echo hi\", \"timeout\": 10}"
                        }
                    }
                ]
            }),
            json!({
                "role": "user",
                "parts": [
                    {
                        "functionResponse": {
                            "name": "shell",
                            "id": "call_12345",
                            "response": "raw string output from shell"
                        }
                    }
                ]
            }),
        ];

        InboundThinkingPipeline::process_contents(
            &mut contents,
            "gemini-3.8-flash-high",
            false,
            None,
            false,
        );

        // 1. functionCall.args 被正确转换为 Object
        let fc = &contents[1]["parts"][0]["functionCall"];
        assert!(fc["args"].is_object());
        assert_eq!(fc["args"]["cmd"], "echo hi");
        assert_eq!(fc["args"]["timeout"], 10);

        // 2. functionResponse.response 被包装为 Object {"output": ...}
        // 同时根据官方形态，回执 role 归一为 model
        assert_eq!(contents[2]["role"], "model");
        let fr = &contents[2]["parts"][0]["functionResponse"];
        assert!(fr["response"].is_object());
        assert_eq!(fr["response"]["output"], "raw string output from shell");
    }

    #[test]
    fn test_multimodal_sliding_window_disabled_by_default() {
        let fake_b64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
        let mut contents = vec![
            json!({
                "role": "user",
                "parts": [{"inlineData": {"mimeType": "image/png", "data": fake_b64}}]
            }),
            json!({
                "role": "model",
                "parts": [{"text": "Seen image 1"}]
            }),
            json!({
                "role": "user",
                "parts": [{"inlineData": {"mimeType": "image/png", "data": fake_b64}}]
            }),
        ];

        let config = crate::proxy::config::MultimodalConfig {
            enable_sliding_window: false,
            strategy: "count".to_string(),
            max_fresh_images: 1,
            strip_remote_urls: false,
            max_total_image_mb: 32,
        };

        let pruned = InboundThinkingPipeline::apply_multimodal_sliding_window_and_limits(
            &mut contents,
            &config,
        );
        assert_eq!(
            pruned, 0,
            "When sliding window is disabled, 0 images should be pruned"
        );
        assert!(contents[0]["parts"][0].get("inlineData").is_some());
        assert!(contents[2]["parts"][0].get("inlineData").is_some());
    }

    #[test]
    fn test_multimodal_sliding_window_prunes_older_images_and_protects_latest() {
        let fake_b64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
        let mut contents = vec![
            json!({
                "role": "user",
                "parts": [{"inlineData": {"mimeType": "image/png", "data": fake_b64}}]
            }),
            json!({
                "role": "user",
                "parts": [{"inlineData": {"mimeType": "image/png", "data": fake_b64}}]
            }),
            json!({
                "role": "user",
                "parts": [{"inlineData": {"mimeType": "image/png", "data": fake_b64}}]
            }),
        ];

        // 保鲜最近 1 张，前 2 张应被修剪
        let config = crate::proxy::config::MultimodalConfig {
            enable_sliding_window: true,
            strategy: "count".to_string(),
            max_fresh_images: 1,
            strip_remote_urls: false,
            max_total_image_mb: 32,
        };

        let pruned = InboundThinkingPipeline::apply_multimodal_sliding_window_and_limits(
            &mut contents,
            &config,
        );
        assert_eq!(pruned, 2);

        // 前 2 张转换为占位符文本
        assert_eq!(
            contents[0]["parts"][0]["text"].as_str().unwrap(),
            "[Historical Image #1: omitted to preserve context (image/png)]"
        );
        assert_eq!(
            contents[1]["parts"][0]["text"].as_str().unwrap(),
            "[Historical Image #2: omitted to preserve context (image/png)]"
        );
        // 最新 1 张完整保留
        assert!(contents[2]["parts"][0].get("inlineData").is_some());
    }

    #[test]
    fn test_multimodal_sliding_window_remote_urls_option() {
        let mut contents = vec![
            json!({
                "role": "user",
                "parts": [{"fileData": {"fileUri": "https://oss.example.com/test1.png", "mimeType": "image/png"}}]
            }),
            json!({
                "role": "user",
                "parts": [{"fileData": {"fileUri": "https://oss.example.com/test2.png", "mimeType": "image/png"}}]
            }),
        ];

        // 1. strip_remote_urls = false: 远程直链保留
        let config_keep = crate::proxy::config::MultimodalConfig {
            enable_sliding_window: true,
            strategy: "count".to_string(),
            max_fresh_images: 1,
            strip_remote_urls: false,
            max_total_image_mb: 32,
        };
        let pruned_keep = InboundThinkingPipeline::apply_multimodal_sliding_window_and_limits(
            &mut contents,
            &config_keep,
        );
        assert_eq!(
            pruned_keep, 0,
            "Remote URL should not be pruned when strip_remote_urls is false"
        );
        assert!(contents[0]["parts"][0].get("fileData").is_some());

        // 2. strip_remote_urls = true: 远程直链被剥离
        let config_strip = crate::proxy::config::MultimodalConfig {
            enable_sliding_window: true,
            strategy: "count".to_string(),
            max_fresh_images: 1,
            strip_remote_urls: true,
            max_total_image_mb: 32,
        };
        let pruned_strip = InboundThinkingPipeline::apply_multimodal_sliding_window_and_limits(
            &mut contents,
            &config_strip,
        );
        assert_eq!(pruned_strip, 1);
        assert_eq!(
            contents[0]["parts"][0]["text"].as_str().unwrap(),
            "[Historical Remote Image #1: omitted to preserve context]"
        );
        assert!(contents[1]["parts"][0].get("fileData").is_some());
    }

    #[test]
    fn test_multimodal_sliding_window_memory_strategy() {
        let fake_b64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
        let mut contents = vec![
            json!({
                "role": "user",
                "parts": [{"inlineData": {"mimeType": "image/png", "data": fake_b64}}]
            }),
            json!({
                "role": "user",
                "parts": [{"inlineData": {"mimeType": "image/png", "data": fake_b64}}]
            }),
        ];

        // 内存策略：设定容量极小 (1MB)，而 2 张图都在 1MB 内，因此全部保留
        let config_fits = crate::proxy::config::MultimodalConfig {
            enable_sliding_window: true,
            strategy: "memory".to_string(),
            max_fresh_images: 10,
            strip_remote_urls: false,
            max_total_image_mb: 1,
        };

        let pruned_fits = InboundThinkingPipeline::apply_multimodal_sliding_window_and_limits(
            &mut contents,
            &config_fits,
        );
        assert_eq!(
            pruned_fits, 0,
            "Images fitting in memory budget should all be preserved"
        );
        assert!(contents[0]["parts"][0].get("inlineData").is_some());
        assert!(contents[1]["parts"][0].get("inlineData").is_some());
    }

    #[test]
    fn test_inbound_preserves_sentinel_signature() {
        let mut contents = vec![
            json!({
                "role": "user",
                "parts": [{"text": "run command"}]
            }),
            json!({
                "role": "model",
                "parts": [{
                    "thoughtSignature": crate::proxy::thinking_store::SENTINEL_SIGNATURE,
                    "functionCall": {
                        "name": "Write",
                        "id": "Write-76",
                        "args": {"path": "test.txt"}
                    }
                }]
            }),
            json!({
                "role": "user",
                "parts": [{
                    "functionResponse": {
                        "name": "Write",
                        "id": "Write-76",
                        "response": {"result": "ok"}
                    }
                }]
            }),
        ];

        InboundThinkingPipeline::process_contents(
            &mut contents,
            "gemini-3.8-flash-high",
            true,
            None,
            false,
        );

        let fc_part = &contents[1]["parts"][0];
        assert_eq!(
            fc_part.get("thoughtSignature").and_then(|v| v.as_str()),
            Some(crate::proxy::thinking_store::SENTINEL_SIGNATURE),
            "客户端自带的官方合法哨兵必须保留，绝不能被进站剥离"
        );
    }

    #[test]
    fn test_gatekeeper_auto_heals_missing_signature_in_active_turn() {
        let mut inner_request = json!({
            "contents": [
                {
                    "role": "user",
                    "parts": [{"text": "historical question"}]
                },
                {
                    "role": "model",
                    "parts": [{"text": "historical answer"}]
                },
                {
                    "role": "user",
                    "parts": [{"text": "current question"}]
                },
                {
                    "role": "model",
                    "parts": [{
                        "functionCall": {
                            "name": "Write",
                            "id": "Write-76",
                            "args": {"file": "output.txt"}
                        }
                    }]
                }
            ]
        });

        InboundThinkingPipeline::align_google_request_prefix_topology_with_model(
            &mut inner_request,
            "gemini-3.8-flash-high",
            Some("agent/test/123/abc/4"),
        );

        let contents = inner_request["contents"]
            .as_array()
            .expect("contents array");
        let active_model_part = &contents[3]["parts"][0];
        assert_eq!(
            active_model_part
                .get("thoughtSignature")
                .and_then(|v| v.as_str()),
            Some(crate::proxy::thinking_store::SENTINEL_SIGNATURE),
            "活跃 Turn 内缺失签名的工具调用出站时必须被安全门禁自动补齐哨兵签名"
        );
    }

    #[test]
    fn test_gatekeeper_auto_heals_missing_signature_in_deep_history() {
        // [Pipeline First Gatekeeper] 验证远古历史轮次（处于最后真实 user 轮次之前）中的工具调用，
        // 若因客户端协议转换或缓存丢失而缺少 thoughtSignature，出站门禁必须全量自愈并补齐哨兵，杜绝 Google 400 校验拦截！
        let mut inner_request = json!({
            "contents": [
                {
                    "role": "user",
                    "parts": [{"text": "first question"}]
                },
                {
                    "role": "model",
                    "parts": [{
                        "functionCall": {
                            "name": "read_file",
                            "id": "call_1104323",
                            "args": {"path": "app.py"}
                        }
                    }]
                },
                {
                    "role": "user",
                    "parts": [{
                        "functionResponse": {
                            "name": "read_file",
                            "id": "call_1104323",
                            "response": {"output": "print('hello')"}
                        }
                    }]
                },
                {
                    "role": "user",
                    "parts": [{"text": "second question (latest user turn)"}]
                },
                {
                    "role": "model",
                    "parts": [{
                        "functionCall": {
                            "name": "edit",
                            "id": "call_684458",
                            "args": {"path": "app.py"}
                        }
                    }]
                }
            ]
        });

        InboundThinkingPipeline::align_google_request_prefix_topology_with_model(
            &mut inner_request,
            "gemini-3.8-flash-high",
            Some("agent/test/456/deep/1"),
        );

        let contents = inner_request["contents"]
            .as_array()
            .expect("contents array");
        // 远古历史中的 tool call (contents[1]) 必须补上哨兵
        let historical_fc = &contents[1]["parts"][0];
        assert_eq!(
            historical_fc
                .get("thoughtSignature")
                .and_then(|v| v.as_str()),
            Some(crate::proxy::thinking_store::SENTINEL_SIGNATURE),
            "历史深处的工具调用若缺少签名，安全门禁必须全量补齐哨兵签名"
        );
        // 当前活跃的 tool call (contents[4]) 同样必须补上哨兵
        let active_fc = &contents[4]["parts"][0];
        assert_eq!(
            active_fc.get("thoughtSignature").and_then(|v| v.as_str()),
            Some(crate::proxy::thinking_store::SENTINEL_SIGNATURE),
            "活跃轮次的工具调用若缺少签名，安全门禁必须补齐哨兵签名"
        );
    }

    #[test]
    fn test_non_native_tool_id_matches_via_synthetic_id_or_falls_back_to_sentinel() {
        use crate::proxy::thinking_store::*;
        // 1. 模拟一个包含非原生 ID 的 contents (如 Write-76)
        let mut contents = vec![
            json!({
                "role": "user",
                "parts": [{"text": "create file"}]
            }),
            json!({
                "role": "model",
                "parts": [{
                    "functionCall": {
                        "name": "Write",
                        "id": "Write-76",
                        "args": {"path": "main.py"}
                    }
                }]
            }),
        ];

        // 2. 先计算出该轮次的前置因果锚点与对应的 ID 无关伪哈希 ID
        let anchor = compute_causal_anchor(contents.get(0));
        let synthetic_id = synthesize_tool_id(
            "Write",
            contents[1]["parts"][0]["functionCall"].get("args"),
            &anchor,
            0,
        );

        // 3. 初始化并存入签名 (以 synthetic_id 为 key，同时写入内存缓存与 SQLite)
        let _ = crate::modules::proxy_db::init_db();
        let real_test_sig = "test-signature-real-valid-mock-length-32-chars-long";
        crate::proxy::SignatureCache::global().cache_tool_signature(
            "test_session_non_native",
            &synthetic_id,
            real_test_sig.to_string(),
        );

        // 4. 执行 finalize
        finalize_gemini_contents_thinking_with_session(
            &mut contents,
            true,
            Some("gemini-3.8-flash-high"),
            Some("test_session_non_native"),
        );

        // 5. 校验：非原生 ID 成功通过伪哈希 ID 命中并回填了真实签名！
        let fc_part = &contents[1]["parts"][0];
        assert_eq!(
            fc_part.get("thoughtSignature").and_then(|v| v.as_str()),
            Some(real_test_sig),
            "非原生 ID 缓存未命中时，必须通过伪哈希 ID 从 SQLite 成功匹配并回填真实签名"
        );

        // 6. 验证：回填不了时，自动带哨兵 (SENTINEL_SIGNATURE)
        let mut contents_not_found = vec![
            json!({
                "role": "user",
                "parts": [{"text": "unknown command"}]
            }),
            json!({
                "role": "model",
                "parts": [{
                    "functionCall": {
                        "name": "CustomTool",
                        "id": "CustomTool-999",
                        "args": {"cmd": "random"}
                    }
                }]
            }),
        ];
        finalize_gemini_contents_thinking_with_session(
            &mut contents_not_found,
            true,
            Some("gemini-3.8-flash-high"),
            Some("test_session_not_found"),
        );
        let fc_part_not_found = &contents_not_found[1]["parts"][0];
        assert_eq!(
            fc_part_not_found
                .get("thoughtSignature")
                .and_then(|v| v.as_str()),
            Some(SENTINEL_SIGNATURE),
            "伪哈希 ID 在 SQLite 仍未命中时，必须兜底回填官方哨兵"
        );
    }
}
