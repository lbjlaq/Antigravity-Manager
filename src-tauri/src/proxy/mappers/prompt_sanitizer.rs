//! Prompt Sanitizer Pipeline Module
//!
//! 提供中转报文生成后、发往上游出站前的提示词专用清洗流水线节点。
//! 严格且精准地剥离第三方客户端（Claude Code CLI / VS Code CC / Cherry Studio 等）
//! 在 system / user 块中注入的计费元数据伪 Header（如 `x-anthropic-billing-header` 与
//! `cc_version` / `cc_entrypoint` / `cch` 签名）及 Claude Agent SDK 前导指纹声明，
//! 防止触发 Google Cloud Code 上游 WAF 的特征拦截（虚假 429 RESOURCE_EXHAUSTED）
//! 进而导致全局账号池 503 级联封锁。
//!
//! 核心设计与防误杀铁律：
//! 1. 深度保护代码块：自动提取并保护所有围栏代码块（```）与行内代码（`），代码内容 100% 豁免；
//! 2. 严禁碰触会话唯一性（Zero Session Touch）：绝不清洗或过滤任何 session 相关字段（包括
//!    `x-jeikcode-session-id`, `x-atomcode-session-id`, `x-session-id` 等）；会话唯一性由
//!    `SessionScope` 按照严格优先级链条独立处理，本模块对所有 session 标识保持 100% 原样透传；
//! 3. 严禁碰触用户自定义分割符与标签（Zero Delimiter Touch）：绝不删除 `=== ... ===`、`--- ... ---`
//!    以及 XML 标签（如 `<environment>`, `<workflow_and_execution_discipline>` 等），保证如
//!    JeikCode / AtomCode 等 AI 编码框架的用户与系统指令结构完好无损；
//! 4. 严禁裁剪或假定 IDE 反刍文本：不预设删除任何用户自然语言与业务指令；
//! 5. 全链路审计一致：中转报文生成并填充完思考块后统一在流水线层介入，保证协议无关且写进数据库与转出的报文真实一致；
//! 6. 思考块首位铁律（Thinking Block at Index 0）：清洗完后必须确保思考块在 parts 中严格位于首位，
//!    且思考块文本严禁执行任何破坏性修改，保全数字签名与哈希一致性；
//! 7. 物理剔除空 Part 与空 systemInstruction：清洗后产生的空 Part 物理移出数组，若 systemInstruction
//!    为空则物理注销，杜绝 Google API 格式校验异常。
//! 8. 两轨分治（Two-Track Separation）：节点内部严格区分「Header 轨」与「System 轨」，杜绝规则互相污染：
//!    · Header 轨（`clean_text`）：剥离文本内嵌的计费伪 Header 与高危客户端指纹声明，对**全部文本**
//!      （systemInstruction / contents / user）一致生效；
//!    · System 轨（`normalize_system_identity`）：仅在**系统提示词头部窗口**（每个系统提示词块的
//!      前 `IDENTITY_SCAN_MAX_SENTENCES` 句）内，把「身份归属声明句」归一化为中性身份，
//!      **绝不触碰** user / tool 文本，也绝不触碰管道自身的系统提示词。
//!    该分治使四协议共享同一份身份归一化规则（AGENTS.md「Pipeline First」：适配层不做协议专用清洗特例）。

use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value;

/// 代码块保护正则：隔离多行围栏代码块 ```...``` 与行内反引号 `...`
static RE_CODE_BLOCK: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?ms)(```[\s\S]*?```|`[^`\r\n]+`)").unwrap());

/// 触发上游 Google WAF 拦截的高危客户端伪 Header 与特征声明正则（仅在非代码区域生效）：
/// 1) 通用泛化匹配各类客户端注入的 `*-billing*` 伪 Header 行（如 `x-anthropic-billing-header:`, `x-billing:`, `x-client-billing:`, `anthropic-billing-header:` 等）
/// 2) 匹配任何以 `x-` 开头且携带 Claude Code CLI 计费签名特征（`cc_version`, `cc_entrypoint`, `cch=`）的伪 Header
/// 3) 匹配 mid-paragraph（嵌入在段落中间）的 `x-anthropic-billing-header:` 及其后续声明
/// 4) 匹配 Claude Agent SDK 专属前导指纹声明 (`You are a Claude agent, built on Anthropic's Claude Agent SDK.`)
/// 严格排除任何 `session` 关键字，确保用户提问与会话跟踪完全免受干扰。
static RE_WAF_TRIGGER_HEADERS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(concat!(
        r"(?im)^\s*(?:x-[a-z0-9_-]*billing[a-z0-9_-]*|[a-z0-9_-]+-billing-(?:header|metadata|token|info)):\s*[^\r\n]*(\r?\n)?",
        r"|^\s*x-[a-z0-9_-]+:\s*[^\r\n]*(?:cc_version|cc_entrypoint|cch=)[^\r\n]*(\r?\n)?",
        r"|(?i)x-anthropic-billing-header:\s*[^\r\n]*",
        r"|(?i)You are a Claude agent, built on Anthropic's Claude Agent SDK\.(\r?\n)?"
    ))
    .unwrap()
});

/// 连续多余空行收拢正则（剥离元数据后若产生 3 个及以上连续换行，收拢为 2 个换行以保持自然段落）
static RE_MULTI_NEWLINE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\n{3,}").unwrap());

// ============================ System 轨：身份声明归一化 ============================
//
// 触发上游 WAF 伪限流（虚假 429 RESOURCE_EXHAUSTED）的是**身份归属声明**，而非模型能力或请求体量：
// 上游对"第三方 Agent 框架 + 工具调用能力"的组合施加更严规则集，一旦识别出客户端自我介绍中的
// 厂商 / 产品 / 竞品模型指纹即拒绝该请求，网关若原样透传则每个账号都会被拒，并被误记为账号限流
// 进而波及整池（详见 issue #3507 / #3506，以及 Codex `based on GPT-x` 的 #3444 / #3489）。
//
// Agent 客户端数以千计且会持续出现，无法穷举，因此归一化必须**广谱**：只要一句自我介绍同时具备
// 「身份开场白 + 身份名词 + 归属/版本声明」三要素，就整体归一化为中性身份，而非只处理已知厂商。

/// 身份归一化扫描窗口：仅扫描每个系统提示词块的**前 4 句**。
/// 身份声明必然出现在提示词开头；限制窗口可确保正文（文档、代码注释、用户需求）中对
/// "created by …" 的正常引用 100% 不被误伤。
const IDENTITY_SCAN_MAX_SENTENCES: usize = 4;

/// 归一化后的中性身份声明（统一口径，抹除一切厂商 / 产品 / 竞品模型指纹）
const NEUTRAL_IDENTITY: &str = "You are an AI Agent.";

/// 身份声明句归一化正则（广谱匹配，仅在系统提示词头部窗口内生效）。
///
/// 形态 = A 身份开场白 + B 身份名词短语 + C 归属/版本声明 + D 归属对象 + E 句读：
/// ```text
/// You are Hermes Agent, an intelligent AI assistant created by Nous Research.   → You are an AI Agent.
/// You are Codex, an advanced coding agent based on GPT-6.                       → You are an AI Agent.
/// You are an AI agent created by Example Corp.                                  → You are an AI Agent.
/// You are Antigravity, a powerful agentic AI coding assistant designed by the Google Deepmind team … → You are an AI Agent.
/// ```
/// 防误杀设计（四条硬约束，缺一不可）：
/// 1. **必须同时具备身份名词与归属声明** —— `You are a helpful assistant.`、`You are Claude Code, Anthropic's
///    official CLI for Claude.`、`You are JeikCode AI coding Agent by Jeik.`（无归属动词）一律原样保留；
/// 2. **归属对象取到句读边界（`.` `,` `。` `，` 换行）即止，且禁止跨越 XML 标签** —— 从结构上根除
///    "跨行吞噬后续规则"的缺陷（`[^.]` 在 Rust regex 中可匹配换行，PR #3508 即因此会把身份行之后的
///    多条规则一并删除），同时满足 Zero Delimiter Touch；
/// 3. **只替换声明句本身** —— 归属声明之后的内容（含逗号续写的功能指令）逐字保留；
/// 4. **过渡窗口 ≤ 8 字符**：身份名词与归属动词之间的桥接文本必须极短，防止跨子句误删
///    （如 `… agent, and the config was created by X` 这类正文句式不得被整段归一化）。
static RE_IDENTITY_DECLARATION: Lazy<Regex> = Lazy::new(|| {
    Regex::new(concat!(
        // A. 身份开场白（英文与中文；要求词边界，避免误命中 "…the assistant you are talking to…"）
        r"(?i:\b(?:you\s+are|you're|i\s+am|i'm|你是|我是)\b[\s,，:：]*)",
        // B. 身份名词短语：可选冠词 + ≤8 个修饰词 + 可选 AI + 身份名词
        r"(?i:(?:an?\s+)?(?:[\w'\-]+[\s,，]+){0,8}(?:ai[\s,，]+)?(?:agent\b|assistant\b|ai\b|助手|模型|助理))",
        // C. 归属 / 版本声明（动词 + 介词，或 based on）
        //    过渡窗口允许逗号（覆盖 `You are a Claude agent, built on Anthropic's …` 这类逗号引导形态），
        //    但严格禁止跨越句末标点、换行与 XML 标签，且长度压在 8 字符内 —— 归属声明必然紧邻身份名词，
        //    收窄窗口可防止把 `… agent, and the config was created by X` 这类跨子句文本整段误删。
        r"(?i:[^.!?。！？\r\n<>]{0,8}?\b(?:created|built|made|developed|designed|trained|powered|maintained|published|released|provided)\s+(?:by|on|upon|at)\s+",
        r"|[^.!?。！？\r\n<>]{0,8}?\bbased\s+on\s+)",
        // D. 归属对象：一路取到句读边界，杜绝残留孤立词片段
        r"[^.,，。\r\n<>]{1,80}",
        // E. 紧随其后的句读，用于决定替换后的标点（`,` 续写 → 保留逗号）
        r"([.,，。]?)"
    ))
    .unwrap()
});

/// 身份声明归一化的快速前置特征（命中任一才进入正则流程，规避 33k 级提示词的全量正则开销）
const IDENTITY_FAST_MARKERS: [&str; 13] = [
    "created by",
    "built by",
    "built on",
    "made by",
    "developed by",
    "designed by",
    "trained by",
    "powered by",
    "maintained by",
    "published by",
    "released by",
    "provided by",
    "based on",
];

/// Claude Agent SDK 注入的独立身份块（整串等值匹配）
const CLAUDE_AGENT_SDK_IDENTITY: &str =
    "You are a Claude agent, built on Anthropic's Claude Agent SDK.";
/// 归一化目标：Claude Code CLI 身份。刻意保留「映射成已知客户端身份」而非抹成中性句 ——
/// 上游对该身份更稳定（原适配层语义，收敛时必须原样保留）。
const CLAUDE_CODE_CLI_IDENTITY: &str = "You are Claude Code, Anthropic's official CLI for Claude.";

/// Claude Desktop 注入的单行计费元数据前缀（issue #3452：与大量工具组合会触发上游 429）。
/// 判定**不区分 model** —— 任何客户端注入的风险元数据都必须清洗。
const BILLING_METADATA_PREFIX: &str = "x-anthropic-billing-header:";

/// 管道自身的系统提示词分界标记（混入时剥掉，避免污染上游报文）
const SYSTEM_PROMPT_END_MARKERS: [&str; 2] = ["--- [SYSTEM_PROMPT_END] ---", "[SYSTEM_PROMPT_END]"];

/// 管道自身的官方身份文本（全局提示词混入时整段剥掉，避免重复声明身份）
const SELF_IDENTITY_BLOCK: &str = "You are Antigravity, a powerful agentic AI coding assistant designed by the Google Deepmind team working on Advanced Agentic Coding.\nYou are pair programming with a USER to solve their coding task. The task may require creating a new codebase, modifying or debugging an existing codebase, or simply answering a question.\n**Absolute paths only**\n**Proactiveness**";

pub struct PromptSanitizer;

impl PromptSanitizer {
    /// 对单段提示词文本进行通用、高精度的净化：
    /// 1. 快速检查是否包含高危 WAF 拦截触发词（`billing`, `cc_version`, `cc_entrypoint`），未包含直接原样返回；
    /// 2. 提取并保护所有代码块，使其免受任何正则影响；
    /// 3. 仅剥离触发 WAF 的客户端计费伪 Header，绝不修改任何会话 ID、XML 标签或 `=== ... ===` 分隔符；
    /// 4. 完整恢复受保护的代码块；
    /// 5. 规范化换行，保留所有多行正文排版。
    pub fn clean_text(text: &str) -> String {
        // 快速前置检查：若文本不包含高危特征，直接原样返回，零开销
        let lower = text.to_lowercase();
        let has_suspect = lower.contains("billing")
            || lower.contains("cc_version")
            || lower.contains("cc_entrypoint")
            || lower.contains("cch=")
            || lower.contains("claude agent sdk");

        if !has_suspect {
            return text.to_string();
        }

        // 步骤 1：保护代码块
        let mut placeholders: Vec<String> = Vec::new();
        let protected_text = RE_CODE_BLOCK.replace_all(text, |caps: &regex::Captures| {
            let idx = placeholders.len();
            placeholders.push(caps[0].to_string());
            format!("__PROMPT_SANITIZER_CODE_BLOCK_{}__", idx)
        });

        // 步骤 2：对非代码区域仅剥离触发 WAF 的高危伪 Header
        let pass1 = RE_WAF_TRIGGER_HEADERS.replace_all(&protected_text, "");

        // 步骤 3：恢复受保护的代码块
        let mut restored = pass1.into_owned();
        for (idx, original_code) in placeholders.iter().enumerate() {
            let ph = format!("__PROMPT_SANITIZER_CODE_BLOCK_{}__", idx);
            restored = restored.replace(&ph, original_code);
        }

        // 步骤 4：连续空行收拢（若因删除单行产生多余换行，保持正常双换行段落结构）
        let normalized = RE_MULTI_NEWLINE.replace_all(&restored, "\n\n");

        normalized.trim().to_string()
    }

    /// 计算系统提示词的**头部窗口**：返回从开头起、跨越前 `IDENTITY_SCAN_MAX_SENTENCES` 句的切片。
    /// 句界 = `.` `!` `?` `。` `！` `？`，或**空行**（连续两个 `\n`，兼容 `\r\n`）；普通折行不计为句界。
    fn identity_head_region(text: &str) -> &str {
        let mut sentence_count = 0usize;
        let mut prev_newline = false;

        for (idx, ch) in text.char_indices() {
            let is_newline = ch == '\n';
            let is_terminator = matches!(ch, '.' | '!' | '?' | '。' | '！' | '？');

            if is_terminator || (is_newline && prev_newline) {
                sentence_count += 1;
                if sentence_count >= IDENTITY_SCAN_MAX_SENTENCES {
                    return &text[..idx + ch.len_utf8()];
                }
            }

            if is_newline {
                prev_newline = true;
            } else if ch != '\r' {
                prev_newline = false;
            }
        }

        text
    }

    /// System 轨：把系统提示词头部窗口内的「身份归属声明句」归一化为中性身份。
    ///
    /// 归一化规则（广谱、协议无关）：命中 `RE_IDENTITY_DECLARATION` 的声明句整体替换为
    /// `NEUTRAL_IDENTITY`；若原文以逗号续写功能指令，则保留逗号，续写内容逐字不动。
    /// 代码块（``` 与行内 `）内的身份文本 100% 豁免，头部窗口之后的原文逐字保留。
    pub fn normalize_system_identity(text: &str) -> String {
        // 快速前置检查：无任何归属类特征词则零开销返回
        let lower = text.to_lowercase();
        if !IDENTITY_FAST_MARKERS
            .iter()
            .any(|marker| lower.contains(marker))
        {
            return text.to_string();
        }

        // 仅头部窗口参与身份归一化（正文引用一律豁免）
        let head = Self::identity_head_region(text);
        if !RE_IDENTITY_DECLARATION.is_match(head) {
            return text.to_string();
        }

        // 步骤 1：保护头部窗口内的代码块，使其免于身份归一的任何影响
        let mut placeholders: Vec<String> = Vec::new();
        let protected_head = RE_CODE_BLOCK.replace_all(head, |caps: &regex::Captures| {
            let idx = placeholders.len();
            placeholders.push(caps[0].to_string());
            format!("__PROMPT_SANITIZER_CODE_BLOCK_{}__", idx)
        });

        // 步骤 2：归一化身份声明句（逗号续写场景保留逗号，避免出现 ", ," 或 ", and" 断裂）
        let normalized = RE_IDENTITY_DECLARATION
            .replace_all(&protected_head, |caps: &regex::Captures| {
                match caps.get(1).map(|m| m.as_str()).unwrap_or("") {
                    "," | "，" => "You are an AI Agent,".to_string(),
                    _ => NEUTRAL_IDENTITY.to_string(),
                }
            })
            .into_owned();

        // 步骤 3：还原受保护的代码块，并拼回头部窗口之后的原文
        let mut restored = normalized;
        for (idx, original_code) in placeholders.iter().enumerate() {
            let ph = format!("__PROMPT_SANITIZER_CODE_BLOCK_{}__", idx);
            restored = restored.replace(&ph, original_code);
        }

        restored.push_str(&text[head.len()..]);
        restored
    }

    /// 清洗 parts 数组中的所有文本节点，并严格捍卫：
    /// 1. 思考块受 thoughtSignature 严格保护，字节级绝对不可变；
    /// 2. 物理剔除清洗后产生的纯空文本 Part（避免上游 400/429 报错）；
    /// 3. 清洗后若存在思考块，强制保序确保思考块严格位于首位 (Index 0)；
    /// 4. `system_scope` 决定是否叠加 System 轨（身份归一化）—— 仅系统提示词为 true。
    /// 客户端身份归一化：已知客户端身份的**整串等值**映射为目标身份。
    /// 与 `normalize_system_identity`（抹除归属声明）语义不同 —— 这里刻意保留"映射成已知客户端身份"。
    pub fn normalize_client_identity(text: &str) -> &str {
        if text == CLAUDE_AGENT_SDK_IDENTITY {
            CLAUDE_CODE_CLI_IDENTITY
        } else {
            text
        }
    }

    /// 是否为客户端注入的单行计费元数据（issue #3452：与大量工具组合会触发上游 429）。
    /// **不区分 model** —— 风险元数据来自客户端注入，任何模型 / 协议下命中即视为风险。
    pub fn is_billing_metadata(text: &str) -> bool {
        let t = text.trim();
        t.starts_with(BILLING_METADATA_PREFIX) && !t.contains('\n') && !t.contains('\r')
    }

    /// 剥离管道自身的提示词分界标记与官方身份文本（原适配层 `clean_system_prompt_text`）。
    /// 注意：不含 `clean_text` —— Header 轨已在 `sanitize_parts_core` 中统一执行，避免重复。
    pub fn strip_pipeline_markers(text: &str) -> String {
        let mut s = text.to_string();
        for marker in SYSTEM_PROMPT_END_MARKERS {
            if s.contains(marker) {
                s = s.replace(marker, "");
            }
        }
        if s.contains(SELF_IDENTITY_BLOCK) {
            s = s.replace(SELF_IDENTITY_BLOCK, "");
        }
        s.trim().to_string()
    }

    fn sanitize_parts_core(parts: &mut Vec<Value>, system_scope: bool) -> usize {
        let mut cleaned_count = 0;

        // 0. 物理剔除客户端注入的计费元数据 Part（issue #3452）
        //    不分 model / 协议：命中即整块移除 —— 风险来自客户端注入，与模型无关。
        let before = parts.len();
        parts.retain(|part| {
            let is_thought = part
                .get("thought")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                || part.get("thoughtSignature").is_some()
                || part.get("thought_signature").is_some();
            if is_thought {
                return true;
            }
            match part.get("text").and_then(Value::as_str) {
                Some(t) => !Self::is_billing_metadata(t),
                None => true,
            }
        });
        cleaned_count += before - parts.len();

        for part in parts.iter_mut() {
            if let Some(obj) = part.as_object_mut() {
                // 思考块受数字签名 (thoughtSignature) 严格保护，其文本必须保持字节级绝对不可变，严禁执行清洗
                if obj.get("thought").and_then(Value::as_bool).unwrap_or(false)
                    || obj.contains_key("thoughtSignature")
                {
                    continue;
                }

                if let Some(text_val) = obj.get("text").and_then(Value::as_str) {
                    // 客户端身份映射（整串等值）—— 仅系统提示词，用户轮次文本原样保留
                    let mapped = if system_scope {
                        Self::normalize_client_identity(text_val)
                    } else {
                        text_val
                    };
                    // Header 轨（全文本生效）：风控伪 Header 与高危客户端指纹声明
                    let cleaned = Self::clean_text(mapped);
                    // System 轨（仅系统提示词）：剥离管道分界标记 / 官方身份 + 身份归属声明归一化
                    let cleaned = if system_scope {
                        Self::normalize_system_identity(&Self::strip_pipeline_markers(&cleaned))
                    } else {
                        cleaned
                    };
                    if cleaned != text_val {
                        obj.insert("text".to_string(), Value::String(cleaned));
                        cleaned_count += 1;
                    }
                }
            }
        }

        // 物理剔除清洗后产生的纯空文本 Part
        // 注意：思考块以及非文本部件（如 inlineData、functionCall 等）必须完好保留
        parts.retain(|part| {
            let is_thought = part
                .get("thought")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                || part.get("thoughtSignature").is_some()
                || part.get("thought_signature").is_some();
            if is_thought {
                return true;
            }
            if let Some(text) = part.get("text").and_then(Value::as_str) {
                !text.trim().is_empty()
            } else {
                true
            }
        });

        // 思考块位于首位铁律：若当前轮次存在思考块，确保其位于 index 0
        Self::ensure_thought_block_first(parts);

        cleaned_count
    }

    /// 内容轨清洗：仅执行 Header 轨（风控伪 Header 与高危客户端指纹声明），**不做**身份归一化。
    /// 用于 `contents`（user / model 轮次）—— 遵循 AGENTS.md「不得剥离用户提问」。
    pub fn sanitize_parts(parts: &mut Vec<Value>) -> usize {
        Self::sanitize_parts_core(parts, false)
    }

    /// 系统轨清洗：Header 轨 + System 轨（身份归属声明归一化），仅用于 `systemInstruction`。
    pub fn sanitize_system_parts(parts: &mut Vec<Value>) -> usize {
        Self::sanitize_parts_core(parts, true)
    }

    /// 核心前缀保序：确保思考块严格位于 parts 数组的首位（Index 0）
    pub fn ensure_thought_block_first(parts: &mut Vec<Value>) {
        if parts.len() <= 1 {
            return;
        }

        // 铁律：只认 thought: true。若沿用"有签名即思考块"的启发式，会把带签名的正文
        // 误判为思考块并前移 —— 既改写 part 顺序（= 签名锚点语义），也让真正的前缀错位。
        let thought_idx = parts
            .iter()
            .position(|p| crate::proxy::thinking_store::is_thought_part(p));

        if let Some(idx) = thought_idx {
            if idx != 0 {
                let thought_part = parts.remove(idx);
                parts.insert(0, thought_part);
            }
        }
    }

    /// 核心流水线节点：清洗统一中转报文中的系统提示词（systemInstruction）与对话流（contents）
    /// 支持顶层 Gemini Body 以及包裹在 `request` 字段下的 Body。
    pub fn sanitize_gemini_payload(body: &mut Value) -> usize {
        let mut total_cleaned = 0;

        // 0. 深度清理 "[undefined]" 占位串（Cherry Studio 等客户端常见注入）
        //    统一在此执行，不分协议 —— 任何客户端注入的脏数据都必须清洗。
        crate::proxy::mappers::common_utils::deep_clean_undefined(body, 0);

        // 兼容处理：若存在包装层 "request"，清洗包装内部
        if let Some(inner) = body.get_mut("request").and_then(Value::as_object_mut) {
            let mut inner_val = Value::Object(inner.clone());
            let count = Self::sanitize_gemini_payload_inner(&mut inner_val);
            if let Value::Object(new_inner) = inner_val {
                *inner = new_inner;
            }
            total_cleaned += count;
        }

        // 清洗当前层
        total_cleaned += Self::sanitize_gemini_payload_inner(body);
        total_cleaned
    }

    fn sanitize_gemini_payload_inner(body: &mut Value) -> usize {
        let mut cleaned_count = 0;

        // 1. 清洗系统提示词 (systemInstruction)
        //    走 System 轨：Header 轨之外叠加身份声明归一化（协议无关，四协议共用）
        if let Some(sys) = body
            .get_mut("systemInstruction")
            .and_then(Value::as_object_mut)
        {
            if let Some(parts) = sys.get_mut("parts").and_then(Value::as_array_mut) {
                cleaned_count += Self::sanitize_system_parts(parts);
            }

            // 若清洗后 parts 为空，物理移除整个 systemInstruction，避免向 Google 发送空的系统提示词结构
            let is_parts_empty = sys
                .get("parts")
                .and_then(Value::as_array)
                .map(|p| p.is_empty())
                .unwrap_or(true);

            if is_parts_empty {
                body.as_object_mut().map(|b| b.remove("systemInstruction"));
            }
        }

        // 2. 清洗所有对话轮次 (contents，包括 user 块与 model 块)
        if let Some(contents) = body.get_mut("contents").and_then(Value::as_array_mut) {
            for turn in contents.iter_mut() {
                if let Some(parts) = turn.get_mut("parts").and_then(Value::as_array_mut) {
                    cleaned_count += Self::sanitize_parts(parts);
                }
            }

            // 对话轮次保护：移除 parts 被完全清空的轮次（若有），防止空 content 破坏 Google 协议
            contents.retain(|turn| {
                turn.get("parts")
                    .and_then(Value::as_array)
                    .map(|p| !p.is_empty())
                    .unwrap_or(true)
            });
        }

        cleaned_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_client_identity_mapping_is_preserved_not_neutralized() {
        // 整串等值映射：必须变成 Claude Code CLI 身份，不能抹成中性句
        assert_eq!(
            PromptSanitizer::normalize_client_identity(CLAUDE_AGENT_SDK_IDENTITY),
            CLAUDE_CODE_CLI_IDENTITY
        );
        assert_eq!(
            PromptSanitizer::normalize_client_identity("You are an expert coder."),
            "You are an expert coder."
        );
    }

    #[test]
    fn test_strip_pipeline_markers_and_self_identity() {
        assert_eq!(
            PromptSanitizer::strip_pipeline_markers("Rule A\n--- [SYSTEM_PROMPT_END] ---\nRule B"),
            "Rule A\n\nRule B"
        );
        assert_eq!(
            PromptSanitizer::strip_pipeline_markers(&format!(
                "Intro\n{SELF_IDENTITY_BLOCK}\nOutro"
            )),
            "Intro\n\nOutro"
        );
    }

    #[test]
    fn test_billing_metadata_detection_is_model_agnostic() {
        assert!(PromptSanitizer::is_billing_metadata(
            "x-anthropic-billing-header: cc_version=2.1.270.ffc;"
        ));
        // 多行（真实提示词）不得判定为风险
        assert!(!PromptSanitizer::is_billing_metadata(
            "x-anthropic-billing-header: a;\nSecond line prompt instruction"
        ));
        assert!(!PromptSanitizer::is_billing_metadata(
            "You are an expert coder."
        ));
    }

    #[test]
    fn test_billing_metadata_part_is_dropped_and_undefined_cleaned() {
        let mut payload = json!({
            "systemInstruction": { "parts": [
                { "text": "x-anthropic-billing-header: cc_version=2.1.270.ffc;" },
                { "text": "You are an expert coder." }
            ]},
            "generationConfig": { "foo": "[undefined]" }
        });
        PromptSanitizer::sanitize_gemini_payload(&mut payload);

        let parts = payload["systemInstruction"]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 1, "计费元数据 part 必须被剔除: {payload}");
        assert_eq!(parts[0]["text"], "You are an expert coder.");
        assert!(
            payload["generationConfig"].get("foo").is_none(),
            "[undefined] 占位串必须被清理: {payload}"
        );
    }

    #[test]
    fn test_clean_text_multiline_system_prompt_with_billing() {
        let raw = concat!(
            "x-anthropic-billing-header: cc_version=2.1.220.04c; cc_entrypoint=sdk-ts;\n",
            "You are Claude Code, Anthropic's official CLI for Claude.\n\n",
            "Please follow these instructions:\n",
            "1. Assist with coding tasks."
        );
        let cleaned = PromptSanitizer::clean_text(raw);
        assert!(!cleaned.contains("x-anthropic-billing-header"));
        assert!(cleaned.starts_with("You are Claude Code"));
        assert!(cleaned.contains("1. Assist with coding tasks."));
    }

    #[test]
    fn test_clean_waf_trigger_cc_entrypoint_header() {
        let raw = concat!(
            "x-custom-billing: cc_version=2.0; cc_entrypoint=cli;\n",
            "Actual user instructions."
        );
        let cleaned = PromptSanitizer::clean_text(raw);
        assert_eq!(cleaned, "Actual user instructions.");
    }

    #[test]
    fn test_clean_generic_wildcard_billing_headers() {
        let raw = concat!(
            "x-billing: enabled\n",
            "x-custom-billing-info: token123\n",
            "x-client-billing: active\n",
            "anthropic-billing-header: cc_version=2.1\n",
            "Actual user instructions."
        );
        let cleaned = PromptSanitizer::clean_text(raw);
        assert_eq!(cleaned, "Actual user instructions.");
    }

    #[test]
    fn test_strictly_preserves_session_headers_and_tokens() {
        // 关键验证：任何形式的 session ID 绝不能被破坏或删除
        let raw = concat!(
            "x-jeikcode-session-id: session-abc-123\n",
            "x-atomcode-session-id: atom-sess-456\n",
            "x-session-id: generic-sess-789\n",
            "x-client-session-id: client-sess-000\n",
            "Please keep my session active."
        );
        let cleaned = PromptSanitizer::clean_text(raw);
        assert_eq!(cleaned, raw);
    }

    #[test]
    fn test_strictly_preserves_user_delimiters_and_xml_tags() {
        // 关键验证：用户的 JeikCode / AtomCode 提示词格式包含 XML 与 === ... === 分割符，100% 原样保留
        let jeik_prompt = concat!(
            "<environment>\n",
            "You are JeikCode AI coding Agent by Jeik.\n\n",
            "## PRECEDENCE:\n",
            "- Content enclosed in XML tags represents current environment.\n",
            "- Rules under headers matching `=== ... (*.md) ===` (such as `AGENTS.md`, `CLAUDE.md`, `=== MEMORY ===`) constitute USER PROVISIONS.\n",
            "</environment>\n\n",
            "=== AGENTS.md ===\n",
            "User custom provisions.\n",
            "=== MEMORY ===\n",
            "Memory block 1."
        );
        let cleaned = PromptSanitizer::clean_text(jeik_prompt);
        assert_eq!(cleaned, jeik_prompt);
    }

    #[test]
    fn test_protects_code_blocks_containing_waf_signatures() {
        let code = concat!(
            "Here is my code:\n",
            "```python\n",
            "headers = {'x-anthropic-billing-header': 'cc_version=1.0'}\n",
            "print(headers)\n",
            "```\n",
            "Does this look right?"
        );
        let cleaned = PromptSanitizer::clean_text(code);
        assert_eq!(cleaned, code);
    }

    #[test]
    fn test_protects_normal_user_prompts_and_http_headers() {
        let normal_text = concat!(
            "How do I set Authorization: Bearer <token> in curl?\n",
            "- Step 1: Add -H flag\n",
            "- Step 2: Test endpoint\n\n",
            "Total steps: 2"
        );
        let cleaned = PromptSanitizer::clean_text(normal_text);
        assert_eq!(cleaned, normal_text);
    }

    #[test]
    fn test_sanitize_gemini_payload_system_and_user_blocks() {
        let mut payload = json!({
            "project": "test-project",
            "model": "gemini-3.8-flash-high",
            "request": {
                "systemInstruction": {
                    "role": "user",
                    "parts": [
                        {
                            "text": "x-anthropic-billing-header: cc_version=2.1;\n<environment>You are JeikCode</environment>\n=== AGENTS.md ==="
                        }
                    ]
                },
                "contents": [
                    {
                        "role": "user",
                        "parts": [
                            {
                                "text": "x-anthropic-billing-header: cc_entrypoint=cli;\nPlease analyze my data:\n=== MEMORY ===\n- Metric A: 10\n- Metric B: 20"
                            }
                        ]
                    },
                    {
                        "role": "model",
                        "parts": [
                            {
                                "text": "Model output response."
                            }
                        ]
                    }
                ]
            }
        });

        let cleaned_count = PromptSanitizer::sanitize_gemini_payload(&mut payload);
        assert_eq!(cleaned_count, 2);

        let sys_text = payload["request"]["systemInstruction"]["parts"][0]["text"]
            .as_str()
            .unwrap();
        // 计费头被精确移除，但 XML 与 === AGENTS.md === 完好无损
        assert!(!sys_text.contains("x-anthropic-billing-header"));
        assert!(sys_text.contains("<environment>You are JeikCode</environment>"));
        assert!(sys_text.contains("=== AGENTS.md ==="));

        let user_text = payload["request"]["contents"][0]["parts"][0]["text"]
            .as_str()
            .unwrap();
        assert!(!user_text.contains("x-anthropic-billing-header"));
        assert!(user_text.contains("=== MEMORY ==="));
        assert!(user_text.contains("- Metric A: 10\n- Metric B: 20"));
    }

    #[test]
    fn test_clean_standalone_billing_header_part_purges_empty_part_and_empty_system_instruction() {
        // [New API / CC Issue] 模拟当 CC 将 billing header 作为独立 Part 上送时：
        // 清洗后空 Part 必须被物理移除；当整个系统提示词仅有该 Header 时，必须物理移除 systemInstruction 节点
        let mut payload = json!({
            "project": "test-project",
            "request": {
                "systemInstruction": {
                    "role": "user",
                    "parts": [
                        {
                            "text": "x-anthropic-billing-header: cc_version=2.1.272.255; cc_entrypoint=claude-vscode;"
                        }
                    ]
                },
                "contents": [
                    {
                        "role": "user",
                        "parts": [
                            { "text": "Hello world" }
                        ]
                    }
                ]
            }
        });

        let cleaned_count = PromptSanitizer::sanitize_gemini_payload(&mut payload);
        assert_eq!(cleaned_count, 1);

        // 验证：systemInstruction 因为 parts 为空被完全注销，不存在 {"text": ""} 畸变
        assert!(payload["request"].get("systemInstruction").is_none());
        assert_eq!(
            payload["request"]["contents"][0]["parts"][0]["text"],
            "Hello world"
        );
    }

    #[test]
    fn test_ensure_thought_block_always_first_after_sanitization() {
        // [铁律验证] 清洗完成后，必须严格确保思考块位于当前轮次 parts 的首位 (Index 0)
        let mut payload = json!({
            "project": "test-project",
            "request": {
                "contents": [
                    {
                        "role": "model",
                        "parts": [
                            {
                                "text": "x-anthropic-billing-header: cc_version=2.1.272.255; cc_entrypoint=cli;\nSome commentary."
                            },
                            {
                                "text": "I am thinking deeply about the problem...",
                                "thought": true,
                                "thoughtSignature": "valid_hmac_signature_123456"
                            },
                            {
                                "text": "Final answer text."
                            }
                        ]
                    }
                ]
            }
        });

        let _ = PromptSanitizer::sanitize_gemini_payload(&mut payload);

        let parts = payload["request"]["contents"][0]["parts"]
            .as_array()
            .expect("parts should be array");

        // 思考块必须被重排置顶到 index 0
        assert_eq!(parts[0]["thought"], true);
        assert_eq!(
            parts[0]["text"],
            "I am thinking deeply about the problem..."
        );
        assert_eq!(parts[0]["thoughtSignature"], "valid_hmac_signature_123456");

        // 其它非思考部件紧随其后且经过了清洗
        assert_eq!(parts[1]["text"], "Some commentary.");
        assert_eq!(parts[2]["text"], "Final answer text.");
    }

    #[test]
    fn test_clean_claude_agent_sdk_preamble() {
        // [WAF 429 规避] 验证 Claude Agent SDK 前导指纹声明被精准剥离
        let raw = concat!(
            "You are a Claude agent, built on Anthropic's Claude Agent SDK.\n\n",
            "You have access to a variety of tools."
        );
        let cleaned = PromptSanitizer::clean_text(raw);
        assert!(!cleaned.contains("Claude Agent SDK"));
        assert_eq!(cleaned, "You have access to a variety of tools.");
    }

    #[test]
    fn test_clean_mid_paragraph_billing_header() {
        // [WAF 429 规避] 验证即使 billing header 嵌入在段落中间也能够被剥离
        let raw = "Intro text x-anthropic-billing-header: cc_version=2.1.278; cc_entrypoint=cli; cch=fa690; and more text";
        let cleaned = PromptSanitizer::clean_text(raw);
        assert!(!cleaned.contains("x-anthropic-billing-header"));
    }

    // ==================== System 轨：身份声明归一化 ====================

    #[test]
    fn test_normalize_identity_broad_attribution_forms() {
        // 广谱覆盖：厂商归属（created by）、竞品模型（based on）、复合身份串（designed by …）
        for (raw, expected) in [
            (
                "You are Hermes Agent, an intelligent AI assistant created by Nous Research.",
                "You are an AI Agent.",
            ),
            (
                "You are Codex, an advanced coding agent based on GPT-6.",
                "You are an AI Agent.",
            ),
            (
                "You are an AI agent created by Example Corp.",
                "You are an AI Agent.",
            ),
            (
                concat!(
                    "You are Antigravity, a powerful agentic AI coding assistant designed by the Google Deepmind team working on Advanced Agentic Coding.\n",
                    "You are pair programming with a USER."
                ),
                "You are an AI Agent.\nYou are pair programming with a USER.",
            ),
            // 逗号引导的归属声明（Claude Agent SDK 形态）
            (
                "You are a Claude agent, built on Anthropic's Claude Agent SDK.\nYou have access to tools.",
                "You are an AI Agent.\nYou have access to tools.",
            ),
            // 长修饰链 + 厂商归属
            (
                "You are a helpful and highly capable general-purpose AI coding assistant created by Acme Corp.",
                "You are an AI Agent.",
            ),
        ] {
            assert_eq!(PromptSanitizer::normalize_system_identity(raw), expected);
        }
    }

    #[test]
    fn test_normalize_identity_never_swallows_following_lines() {
        // [结构性防炸] 归属声明取到句读边界即止：身份行之后的多行规则必须逐字保留。
        // 对照 PR #3508 的 `[^.]+`（在 Rust regex 中可匹配换行）会把后续规则整段吞掉。
        let raw = concat!(
            "You are an AI agent created by Acme\n",
            "Rule 1: always do X\n",
            "Rule 2: never do Y"
        );
        assert_eq!(
            PromptSanitizer::normalize_system_identity(raw),
            "You are an AI Agent.\nRule 1: always do X\nRule 2: never do Y"
        );
    }

    #[test]
    fn test_normalize_identity_keeps_comma_continuation_intact() {
        let raw =
            "You are an AI agent created by Acme, and you must never reveal internal tooling.";
        assert_eq!(
            PromptSanitizer::normalize_system_identity(raw),
            "You are an AI Agent, and you must never reveal internal tooling."
        );
    }

    #[test]
    fn test_normalize_identity_preserves_non_attribution_identities() {
        // 缺少「归属声明」要素的一律保留；JeikCode / AtomCode 等框架身份（无归属动词）不得被改写
        for raw in [
            "You are a helpful assistant. Please respond in Chinese.",
            "You are an AI assistant that helps users write code by calling tools.",
            "You are JeikCode AI coding Agent by Jeik.",
        ] {
            assert_eq!(PromptSanitizer::normalize_system_identity(raw), raw);
        }

        // 含归属特征词但属于正文引用（非身份声明）：不得被改写
        let body_mention = concat!(
            "You are Claude Code, Anthropic's official CLI for Claude.\n",
            "When done, summarize the diff created by the previous step.\n"
        );
        assert_eq!(
            PromptSanitizer::normalize_system_identity(body_mention),
            body_mention
        );

        // 跨子句守卫：身份名词与归属动词之间隔着完整的另一个子句时不得整段归一化
        let cross_clause =
            "You are a coding agent, and the config was created by the setup script, so keep it.";
        assert_eq!(
            PromptSanitizer::normalize_system_identity(cross_clause),
            cross_clause
        );
    }

    #[test]
    fn test_normalize_identity_only_scans_system_prompt_head() {
        // 头部窗口（前 4 句）之外的身份类表述一律不动，避免误伤长提示词正文
        let mut raw = String::from("You are a coding assistant.\n\n");
        for i in 0..6 {
            raw.push_str(&format!("Sentence {i} is a normal instruction.\n"));
        }
        raw.push_str("The tool was created by Acme.\n");
        assert_eq!(PromptSanitizer::normalize_system_identity(&raw), raw);
    }

    #[test]
    fn test_normalize_identity_protects_code_blocks_and_xml_delimiters() {
        let code = concat!(
            "You are an AI agent created by Acme.\n\n",
            "Example config:\n",
            "```\n",
            "// generated by Hermes Agent created by Nous Research\n",
            "```"
        );
        let cleaned = PromptSanitizer::normalize_system_identity(code);
        assert!(cleaned.starts_with("You are an AI Agent."));
        assert!(cleaned.contains("// generated by Hermes Agent created by Nous Research"));

        let xml = concat!(
            "<environment>\n",
            "You are Hermes Agent created by Nous Research.\n",
            "</environment>\n\n",
            "=== AGENTS.md ===\nUser provisions."
        );
        assert_eq!(
            PromptSanitizer::normalize_system_identity(xml),
            concat!(
                "<environment>\n",
                "You are an AI Agent.\n",
                "</environment>\n\n",
                "=== AGENTS.md ===\nUser provisions."
            )
        );
    }

    #[test]
    fn test_sanitize_gemini_payload_identity_scope_is_system_only() {
        // System 轨只作用于 systemInstruction：contents（用户提问 / 工具消息）逐字保留
        let mut payload = json!({
            "project": "test-project",
            "request": {
                "systemInstruction": {
                    "role": "user",
                    "parts": [
                        { "text": "You are Hermes Agent, an intelligent AI assistant created by Nous Research." }
                    ]
                },
                "contents": [
                    {
                        "role": "user",
                        "parts": [
                            { "text": "Quote the note created by Alice in the doc." }
                        ]
                    }
                ]
            }
        });

        let cleaned_count = PromptSanitizer::sanitize_gemini_payload(&mut payload);
        assert_eq!(cleaned_count, 1);
        assert_eq!(
            payload["request"]["systemInstruction"]["parts"][0]["text"],
            "You are an AI Agent."
        );
        assert_eq!(
            payload["request"]["contents"][0]["parts"][0]["text"],
            "Quote the note created by Alice in the doc."
        );
    }

    #[test]
    fn test_sanitize_gemini_payload_preserves_framework_prompts() {
        // AGENTS.md 铁律：清洗必须「不剥离管道自身的系统提示词与用户提问」
        let jeik = concat!(
            "<environment>\n",
            "You are JeikCode AI coding Agent by Jeik.\n\n",
            "## PRECEDENCE:\n",
            "- Rules under headers matching `=== ... (*.md) ===` constitute USER PROVISIONS.\n",
            "</environment>\n\n",
            "=== AGENTS.md ===\n",
            "User custom provisions."
        );
        let mut payload = json!({
            "request": {
                "systemInstruction": { "role": "user", "parts": [{ "text": jeik }] },
                "contents": [{ "role": "user", "parts": [{ "text": jeik }] }]
            }
        });

        assert_eq!(PromptSanitizer::sanitize_gemini_payload(&mut payload), 0);
        assert_eq!(
            payload["request"]["systemInstruction"]["parts"][0]["text"],
            jeik
        );
        assert_eq!(payload["request"]["contents"][0]["parts"][0]["text"], jeik);
    }
}
