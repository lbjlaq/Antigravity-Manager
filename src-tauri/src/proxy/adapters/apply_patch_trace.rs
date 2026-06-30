//! apply_patch 转换决策埋点(诊断流量查看器「apply-patch」页的数据源)。
//!
//! ## 为什么需要它(与 forward-trace 的区别)
//!
//! forward-trace 抓的是 **raw 协议体**(Codex 原始请求 / 转换后发上游 / 上游回包),
//! 看不到 adapter 内部把上游 `apply_patch` 工具调用重打包成 Codex `custom_tool_call`
//! wire 时的**中间决策**:原始 function args 长啥样、提取出的 V4A 文本、信封修复改了啥、
//! JSON/V4A 截断检测结果、V4A 后验语法校验 verdict、最终 completed/incomplete 决策。
//! 这些恰是精修 apply_patch 模块(extract / repair / validate 反复迭代)最需要盯的环节,
//! 故单列一个 [`crate::responses`] / [`crate::gemini_native`] 共用的埋点出口。
//!
//! ## 为什么用 sink 注入而非直接调 trace_store
//!
//! `trace_store` 在 `crates/proxy`,而本 crate(`adapters`)被 proxy 依赖 —— 反向 `use`
//! 会造成**循环依赖**。故这里只定义一个进程级 sink hook:proxy 启动时(`build_router`)
//! 注册一个闭包,把本模块构造的诊断 `Value` 补 `seq`/`captured_at` 后 push 进
//! `trace_store`(`TraceKind::ApplyPatch`)。沿用 cat-webfetch 子进程 `POST /api/ingest`
//! 的「外层补 seq」思路,只是这里是进程内闭包、无需跨进程。
//!
//! ## 开销 / 默认关
//!
//! gate 指向 `proxy::diagnostics::forward_trace_enabled`(env `CAS_DIAG_TRACE` 或 app 内
//! 「诊断模式」开关,默认关)。未注册 / 关时 [`emit`] 是一次 `OnceLock` load + 一次原子读,
//! **不构造任何 Value**(闭包 `build` 仅在开启时调用),与 forward-trace 同「关时零开销」契约。
//! 与 forward-trace 同定位:开发者本地诊断,patch 正文(代码)按原文记录、不脱敏,仅 loopback、
//! 默认关,绝不随 release 给终端用户开。

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

use serde_json::{json, Value};

/// 单条 args/input 文本落盘上限(防个别巨型 patch 把一条诊断撑爆)。超出截断 + 标注 `truncated_bytes`。
const MAX_FIELD_BYTES: usize = 256 * 1024;

/// 「已发出、等结果回灌」的 apply_patch call_id 上限(防泄漏:若某 call 永远等不到结果,
/// 超额淘汰最旧)。稳态在飞 apply_patch 很少,512 远够。
const PENDING_CAP: usize = 512;

/// 进程级埋点 hook。`gate` 决定是否采集(指向 proxy 的诊断总开关),`sink` 把构造好的
/// 诊断 `Value` 落到 trace_store(由 proxy 注册的闭包补 seq 再 push)。
struct Hook {
    gate: fn() -> bool,
    sink: Box<dyn Fn(Value) + Send + Sync>,
}

static HOOK: OnceLock<Hook> = OnceLock::new();

/// proxy 启动时注册一次(`OnceLock`,二次调用静默忽略 —— 进程级单例)。
/// - `gate`:返回「当前是否采集诊断」,传 `proxy::diagnostics::forward_trace_enabled`。
/// - `sink`:收一条已构造的 apply_patch 诊断 `Value`(尚无 `seq`/`captured_at`),由 proxy
///   补全后 push 进 `trace_store`(`TraceKind::ApplyPatch`)。
pub fn install(gate: fn() -> bool, sink: Box<dyn Fn(Value) + Send + Sync>) {
    let _ = HOOK.set(Hook { gate, sink });
}

/// 当前是否采集 apply_patch 埋点(未注册 → false)。调用方可在构造昂贵字段前先 gate。
pub fn enabled() -> bool {
    HOOK.get().map(|h| (h.gate)()).unwrap_or(false)
}

/// 一条 apply_patch 转换决策的输入(全引用,仅在采集开启时才序列化成 `Value`)。
pub struct ApplyPatchTrace<'a> {
    /// 转换来源路径:`"chat"`(responses/converter.rs)/ `"gemini_native"`。
    pub source: &'a str,
    /// 上游模型名(converter `self.model` / gemini `self.model`),apply_patch 行为按模型分布。
    pub model: &'a str,
    /// Codex wire 的 `call_id`(关联工具结果回灌)。
    pub call_id: &'a str,
    /// Codex wire 的 item id(`fc_*`)。
    pub fc_id: &'a str,
    /// 上游回的**原始** function arguments(标准形态 `{"input":"*** Begin Patch…"}`,
    /// 也可能是裸 V4A / 别名 key / 截断残片)。
    pub args_raw: &'a str,
    /// `extract_apply_patch_input` 提取 + `repair_v4a_envelope` 修复后、真正发给 Codex 的 V4A 文本。
    pub input: &'a str,
    /// 流是否中断(chat:无 finish_reason 且非 `[DONE]`;gemini 不增量,恒 false)。
    pub interrupted: bool,
    /// JSON 结构截断检测结果(`detect_json_truncation`;gemini 路径不适用,传 None)。
    pub json_truncation: Option<&'a str>,
    /// V4A 信封截断检测结果(`detect_v4a_truncation`;gemini 路径不适用,传 None)。
    pub v4a_truncation: Option<&'a str>,
    /// V4A 后验语法校验失败(`validate_v4a_syntax`):`(行号, 人类可读消息)`。
    pub v4a_validation: Option<(usize, &'a str)>,
    /// 最终决策:`"completed"`(emit input.delta+done,写 cache)或 `"incomplete"`
    /// (emit status=incomplete,跳过 input.done,不写 cache,防破坏性半应用)。
    pub decision: &'a str,
    /// pre-flight 自动修复记录(`apply_patch_preflight::repairs_to_value` 的产物):每个
    /// `Update File` 读盘比对的结果(repaired / clean / skipped)。无修复时传 `None`。
    pub repairs: Option<&'a Value>,
}

/// 采集开启时构造诊断 `Value`(phase=`call`)并经 sink 落库;关时零开销返回。
/// completed 的 call 会**登记 call_id 到 pending**,等下一轮请求回灌结果时由 [`emit_result`]
/// 配对发射(incomplete 的 call Codex 不会执行、不会有结果,不登记)。
pub fn emit(trace: &ApplyPatchTrace) {
    let Some(hook) = HOOK.get() else { return };
    if !(hook.gate)() {
        return;
    }
    (hook.sink)(build_value(trace));
    if trace.decision == "completed" {
        register_pending(trace.call_id);
    }
}

/// 采集开启时,为一条 apply_patch **结果回灌**(Codex apply 后塞回模型的 `custom_tool_call_output`)
/// 发射 phase=`result` 诊断。`output` 是回灌原值(string 或 content_items array)。
///
/// **去重 + 精准**:请求侧每轮都重放完整历史(同一 call_id 的结果会在后续每轮请求里再次出现),
/// 故只在 call_id **首次**命中 pending(= 我们发过的 completed apply_patch call)时发射并移除;
/// 历史重放的重复结果、以及非 apply_patch 的 custom 工具结果都不会命中 → 跳过。重试是新 call_id,
/// 各自独立配对。关 / 未注册 sink 时零开销(先 gate 再查 pending)。
pub fn emit_result(call_id: &str, output: &Value) {
    let Some(hook) = HOOK.get() else { return };
    if !(hook.gate)() {
        return;
    }
    if !take_pending(call_id) {
        return;
    }
    let text = match output {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    (hook.sink)(build_result_value(call_id, &text));
}

// ─────────────────────────────────────────────────────────────────────────────
// MOC-263 P3:shell 写盘诊断埋点。模型可能用 `exec_command`(shell)跑 sed -i / cat> / echo> /
// python write / `apply_patch <<EOF` 等**直接改文件**,绕过结构化 apply_patch(因此既不过 preflight
// 双重兜底、也不进上面的 apply_patch 埋点)。这里在转换器处理 exec_command 类工具调用时识别写盘命令
// 并 emit 一条 `trace_kind:"shell_edit"` 诊断(走同一 sink/gate),让诊断页 / jsonl 能看见"多少编辑
// 绕过了 apply_patch",支撑 phase-2 对 shell 改文件行为的深入分析。**纯观测,不拦截、不改命令。**
// ─────────────────────────────────────────────────────────────────────────────

/// 被视作 shell 执行的工具名(其参数里可能含改文件命令)。
pub fn is_shell_exec_tool(name: &str) -> bool {
    matches!(
        name,
        "exec_command" | "shell" | "execute_command" | "local_shell" | "container.exec"
    )
}

/// 从 shell 串里剔除 fd→/dev 噪音重定向(`2>/dev/null` / `>/dev/null` / `2>&1` / `&>/dev/null`),
/// 避免把它们的 `>` 误判成写盘。
fn strip_dev_redirects(cmd: &str) -> String {
    let mut out = cmd.to_owned();
    for pat in [
        "2>/dev/null",
        "1>/dev/null",
        ">/dev/null",
        "&>/dev/null",
        "2>&1",
        "2> /dev/null",
        "> /dev/null",
    ] {
        out = out.replace(pat, " ");
    }
    out
}

/// 把 shell 串按子命令边界(换行 / `;` / `|` / `&&` / `||` / `&`)粗切。分隔符均 ASCII,
/// 按字节切片不会切坏 UTF-8(ASCII 字节不可能是多字节序列的一部分)。
fn split_subcommands(cmd: &str) -> Vec<&str> {
    let b = cmd.as_bytes();
    let mut segs = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < b.len() {
        if b[i] == b'\n' || b[i] == b';' || b[i] == b'|' {
            segs.push(&cmd[start..i]);
            start = i + 1;
            i += 1;
        } else if b[i] == b'&' && b.get(i + 1) == Some(&b'&') {
            segs.push(&cmd[start..i]);
            start = i + 2;
            i += 2;
        } else if b[i] == b'&' {
            segs.push(&cmd[start..i]);
            start = i + 1;
            i += 1;
        } else {
            i += 1;
        }
    }
    segs.push(&cmd[start..]);
    segs
}

/// 子命令里命令词 `w` 是否作为独立 token 出现(粗判,容许前导 `cd`/赋值已被切分)。
fn has_word(s: &str, w: &str) -> bool {
    s.split(|c: char| c.is_whitespace()).any(|t| t == w)
}

/// 是否带「就地编辑」标志(`sed -i` / `perl -pi` / `--in-place`)。
fn has_inplace_flag(s: &str) -> bool {
    s.split(|c: char| c.is_whitespace()).any(|t| {
        t == "--in-place"
            || t.starts_with("--in-place=")
            || (t.starts_with('-') && !t.starts_with("--") && t.len() > 1 && t[1..].contains('i'))
    })
}

/// 子命令是否把输出重定向写进**真实文件**(已剔除 /dev 噪音后仍含 `>`)。
fn redirects_to_file(s: &str) -> bool {
    s.contains('>')
}

/// 首 token 是否只读类(awk/grep/rg/find/diff/sort/jq/`sed -n`)—— 用于 generic 重定向写盘排除。
fn starts_with_reader(s: &str) -> bool {
    let t = s.trim_start();
    let first = t.split(|c: char| c.is_whitespace()).next().unwrap_or("");
    matches!(
        first,
        "awk" | "grep" | "rg" | "find" | "diff" | "comm" | "sort" | "jq"
    ) || (has_word(s, "sed") && t.contains(" -n"))
}

/// 单个重定向目标是否归档/压缩产物(`*.tar.gz`/`.tgz`/`.zip`/`.gz`/`.bz2`/`.xz`/`.tar`)。
fn target_is_artifact(target: &str) -> bool {
    let t = target.trim_matches(|c| c == '"' || c == '\'');
    [".tar.gz", ".tgz", ".zip", ".gz", ".bz2", ".xz", ".tar"]
        .iter()
        .any(|ext| t.ends_with(ext))
}

/// 子命令里**所有**重定向目标是否都是归档/压缩产物(且至少有一个重定向)。用于 `redirect_write` 豁免:
/// 只有「全部写的是下载/打包产物」才豁免;**任一**重定向目标是非归档的真 workspace 文件就**不豁免**
/// (保留审计信号)。**按目标判定、不按命令**:`curl … > x.tar.gz` 豁免,但 `curl … > src/generated.rs`
/// 仍计;且**逐个**重定向都看 —— `tool 2>err.gz > src.rs` 不能因首个 `2>err.gz` 像归档就豁免、漏掉
/// 真 `> src.rs`(chatgpt-codex-connector review:inspect every redirection)。
fn all_redirect_targets_are_artifacts(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0usize;
    let mut saw = false;
    while i < b.len() {
        if b[i] == b'>' {
            let mut j = i + 1;
            if j < b.len() && b[j] == b'>' {
                j += 1; // `>>` 追加
            }
            while j < b.len() && (b[j] == b' ' || b[j] == b'\t') {
                j += 1;
            }
            let start = j;
            while j < b.len() && !b[j].is_ascii_whitespace() {
                j += 1;
            }
            saw = true;
            if !target_is_artifact(&s[start..j]) {
                return false; // 有一个非归档真文件目标 → 不豁免
            }
            i = j;
        } else {
            i += 1;
        }
    }
    saw
}

/// 识别 shell 串里的「写盘 / 改文件」操作(MOC-263 P3 诊断用)。返回命中的种类(可多个);
/// 纯只读(git/ls/grep/cargo/cat 读 等)返回空。宁可少报不滥报:只认明确的写盘形态。
pub fn classify_shell_write(cmd: &str) -> Vec<&'static str> {
    let mut kinds: Vec<&'static str> = Vec::new();
    let push = |k: &'static str, v: &mut Vec<&'static str>| {
        if !v.contains(&k) {
            v.push(k);
        }
    };
    // 整串级:heredoc / -c 形态(子命令切分会破坏 heredoc 体,故看整串)。
    if cmd.contains("apply_patch") && (cmd.contains("<<") || cmd.contains("*** Begin Patch")) {
        push("apply_patch_via_shell", &mut kinds);
    }
    if (has_word(cmd, "python") || has_word(cmd, "python3"))
        && (cmd.contains("<<") || cmd.contains(" -c"))
        && (cmd.contains(".write(")
            || cmd.contains("write_text")
            || cmd.contains("writelines")
            || (cmd.contains("open(")
                && (cmd.contains("'w'")
                    || cmd.contains("\"w\"")
                    || cmd.contains("'a'")
                    || cmd.contains("\"a\"")
                    || cmd.contains("'w+'")
                    || cmd.contains("'x'"))))
    {
        push("python_write", &mut kinds);
    }
    if has_word(cmd, "node")
        && (cmd.contains("writeFileSync")
            || cmd.contains("createWriteStream")
            || cmd.contains("fs.write"))
    {
        push("node_write", &mut kinds);
    }
    // 子命令级:就地编辑 / 重定向写盘。
    let normalized = strip_dev_redirects(cmd);
    for seg in split_subcommands(&normalized) {
        let s = seg.trim();
        if s.is_empty() {
            continue;
        }
        if has_word(s, "sed") && has_inplace_flag(s) {
            push("sed_inplace", &mut kinds);
        } else if has_word(s, "perl") && has_inplace_flag(s) {
            push("perl_inplace", &mut kinds);
        } else if has_word(s, "tee") {
            push("tee_write", &mut kinds);
        } else if has_word(s, "truncate") {
            push("truncate", &mut kinds);
        } else if redirects_to_file(s)
            && !starts_with_reader(s)
            && !all_redirect_targets_are_artifacts(s)
        {
            // echo>/cat>/printf> 及通用 `prog > file`(已排除 awk/grep/sed -n 等只读左侧)。
            // **按目标豁免、且逐个重定向都看**:仅当**所有**重定向目标都是归档产物(`> x.tar.gz`)才豁免;
            // 下载/写到真项目文件(`curl … > src/x.rs`)、或多重定向里夹一个真文件(`tool 2>err.gz > src.rs`)
            // 仍计 —— 真·绕过 apply_patch 改 workspace,审计必须可见(MOC-268,chatgpt-codex-connector review)。
            push("redirect_write", &mut kinds);
        }
    }
    kinds
}

/// 从 exec_command 工具的 args(`{"cmd":"..."}` / 别名)里抽出 shell 命令文本。
fn extract_shell_cmd(args_raw: &str) -> Option<String> {
    let v: Value = serde_json::from_str(args_raw.trim()).ok()?;
    let obj = v.as_object()?;
    for k in ["cmd", "command", "script", "input"] {
        if let Some(val) = obj.get(k) {
            if let Some(s) = val.as_str() {
                return Some(s.to_owned());
            }
            if let Some(arr) = val.as_array() {
                let joined: Vec<String> = arr
                    .iter()
                    .filter_map(|x| x.as_str().map(str::to_owned))
                    .collect();
                if !joined.is_empty() {
                    return Some(joined.join(" "));
                }
            }
        }
    }
    None
}

/// 采集开启 + 该 exec_command 是写盘命令时,emit 一条 `shell_edit` 诊断(模型用 shell 直接改文件、
/// 绕过结构化 apply_patch)。否则零开销返回(先 gate,再 extract+classify)。`tool` 是工具名、
/// `args_raw` 是原始工具参数。纯观测。
pub fn emit_shell_edit(
    source: &str,
    model: &str,
    call_id: &str,
    fc_id: &str,
    tool: &str,
    args_raw: &str,
) {
    let Some(hook) = HOOK.get() else { return };
    if !(hook.gate)() {
        return;
    }
    let Some(cmd) = extract_shell_cmd(args_raw) else {
        return;
    };
    let kinds = classify_shell_write(&cmd);
    if kinds.is_empty() {
        return;
    }
    (hook.sink)(build_shell_edit_value(
        source, model, call_id, fc_id, tool, &cmd, &kinds,
    ));
}

/// 构造 `shell_edit` 诊断 `Value`(seq/captured_at 由 proxy sink 补)。`pub(crate)` 供测试。
pub(crate) fn build_shell_edit_value(
    source: &str,
    model: &str,
    call_id: &str,
    fc_id: &str,
    tool: &str,
    cmd: &str,
    kinds: &[&str],
) -> Value {
    let (cmd_text, cmd_trunc) = cap_field(cmd);
    json!({
        "trace_kind": "shell_edit",
        "phase": "call",
        "source": source,
        "model": model,
        "call_id": call_id,
        "fc_id": fc_id,
        "tool": tool,
        "bypass": "apply_patch",
        "write_kinds": kinds,
        "cmd": {
            "len": cmd.len(),
            "truncated_bytes": cmd_trunc,
            "text": cmd_text,
        },
    })
}

/// 把一条 [`ApplyPatchTrace`] 构造成诊断 `Value`(viewer / jsonl 用)。`seq`/`captured_at`/
/// `proxy_version` 由 proxy 注册的 sink 补(那里能拿到 `next_seq` + 版本号)。`pub(crate)` 供测试。
pub(crate) fn build_value(t: &ApplyPatchTrace) -> Value {
    let (args_text, args_trunc) = cap_field(t.args_raw);
    let (input_text, input_trunc) = cap_field(t.input);
    let mut reasons: Vec<&str> = Vec::new();
    if t.interrupted {
        reasons.push("interrupted");
    }
    if t.json_truncation.is_some() {
        reasons.push("json_truncated");
    }
    if t.v4a_truncation.is_some() {
        reasons.push("v4a_truncated");
    }
    if t.v4a_validation.is_some() {
        reasons.push("v4a_invalid");
    }
    json!({
        "trace_kind": "apply_patch",
        "phase": "call",
        "source": t.source,
        "model": t.model,
        "call_id": t.call_id,
        "fc_id": t.fc_id,
        "decision": t.decision,
        "extraction": classify_extraction(t.args_raw, t.input),
        "incomplete_reasons": reasons,
        "repairs": t.repairs.cloned().unwrap_or(Value::Null),
        "args": {
            "len": t.args_raw.len(),
            "truncated_bytes": args_trunc,
            "raw": args_text,
        },
        "input": {
            "len": t.input.len(),
            "truncated_bytes": input_trunc,
            "v4a": input_text,
        },
        "checks": {
            "interrupted": t.interrupted,
            "json_truncation": t.json_truncation,
            "v4a_truncation": t.v4a_truncation,
            "v4a_validation": t.v4a_validation.map(|(line, message)| json!({
                "line": line,
                "message": message,
            })),
        },
    })
}

/// 把一条 apply_patch **结果回灌**构造成诊断 `Value`(phase=`result`)。`pub(crate)` 供测试。
pub(crate) fn build_result_value(call_id: &str, output: &str) -> Value {
    let (text, trunc) = cap_field(output);
    json!({
        "trace_kind": "apply_patch",
        "phase": "result",
        "call_id": call_id,
        "is_error": looks_like_error(output),
        "output": {
            "len": output.len(),
            "truncated_bytes": trunc,
            "text": text,
        },
    })
}

/// apply_patch 结果是否像失败(advisory —— viewer 仍展示全文供人判断)。匹配 Codex apply_patch
/// handler / parse_patch 常见失败措辞;成功输出通常是变更文件清单或简短 "Success"。
/// 判断 apply_patch 结果是否失败。**不能**用 `"error"`/`"context"` 等松散子串 —— 会命中
/// 文件名(`ErrorBoundary.tsx`)、代码(`asynccontextmanager`)而误报(MOC-194 真机 seq977:
/// `Exit code: 0 … Success … A …ErrorBoundary.tsx` 被误判 is_error=true)。信号优先级:
/// ① 明确失败短语(apply_patch 校验失败直接报、不带 Exit code 包装)→ ② exec 包装的
/// `Exit code: N`(非 0 = 失败)→ ③ 默认非错。
fn looks_like_error(output: &str) -> bool {
    let l = output.to_ascii_lowercase();
    const FAIL_PHRASES: [&str; 9] = [
        "apply_patch verification failed",
        "failed to find",
        "did not apply",
        "does not match",
        "invalid patch",
        "no such file or directory",
        "is not a valid hunk header",
        "update file hunk for path",
        "cannot operate on a completely empty file",
    ];
    if FAIL_PHRASES.iter().any(|m| l.contains(m)) {
        return true;
    }
    // exec 包装的 `Exit code: N` 是权威信号(成功 = 0)。
    if let Some(code) = parse_exit_code(output) {
        return code != 0;
    }
    false
}

/// 从 exec 包装的 `Exit code: N` 抽退出码(apply_patch 经 shell exec 时带此前缀)。
fn parse_exit_code(output: &str) -> Option<i32> {
    let idx = output.find("Exit code:")?;
    output[idx + "Exit code:".len()..]
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

// ── pending apply_patch call_id 注册表(call ↔ result 配对 + 历史重放去重)──────────
//
// completed 的 apply_patch call 登记 call_id;结果回灌首次命中即发射并移除。只用
// `Mutex<VecDeque<String>>`(每次 apply_patch 才动一次,512 内线性扫可忽略),超额淘汰最旧。

static PENDING: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();

fn pending() -> &'static Mutex<VecDeque<String>> {
    PENDING.get_or_init(|| Mutex::new(VecDeque::new()))
}

/// 登记一个「等结果」的 call_id(超 [`PENDING_CAP`] 淘汰最旧)。空 id 忽略。
fn register_pending(call_id: &str) {
    if call_id.is_empty() {
        return;
    }
    if let Ok(mut q) = pending().lock() {
        // 去重:同 call_id 不重复登记(理论上 call_id 唯一,防御)。
        if q.iter().any(|x| x == call_id) {
            return;
        }
        q.push_back(call_id.to_owned());
        while q.len() > PENDING_CAP {
            q.pop_front();
        }
    }
}

/// 若 call_id 在 pending 中则移除并返回 true(= 这是我们发过的 apply_patch call 的首个结果)。
fn take_pending(call_id: &str) -> bool {
    if let Ok(mut q) = pending().lock() {
        if let Some(pos) = q.iter().position(|x| x == call_id) {
            q.remove(pos);
            return true;
        }
    }
    false
}

/// 截断到 [`MAX_FIELD_BYTES`](按 char 边界,不切坏 UTF-8),返回(文本, 丢弃字节数)。
fn cap_field(s: &str) -> (String, usize) {
    if s.len() <= MAX_FIELD_BYTES {
        return (s.to_owned(), 0);
    }
    // 找 <= cap 的 char 边界,避免切在多字节中间。
    let mut end = MAX_FIELD_BYTES;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    (s[..end].to_owned(), s.len() - end)
}

/// 粗分类「V4A 是怎么从原始 args 里抽出来的」(给 viewer 摘要 / 过滤)。轻量 re-derive,
/// 与 `extract_apply_patch_input` 的实际分支对齐但不耦合其内部:
/// - `json_input`:args 是 JSON 且含 `input` 字段(标准形态)。
/// - `json_alt_key`:args 是 JSON、无 `input` 但 input 文本回收自别名 key(patch/diff/…)。
/// - `bare_v4a`:args 本身就是裸 V4A(无 JSON 包裹)。
/// - `raw_fallback`:既非合法 JSON 也不像裸 V4A → 原样透传(多半截断 / schema drift)。
pub(crate) fn classify_extraction(args_raw: &str, _input: &str) -> &'static str {
    let trimmed = args_raw.trim();
    if trimmed.is_empty() {
        return "empty";
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(v) => {
            if v.get("input").and_then(Value::as_str).is_some() {
                "json_input"
            } else if v.is_object() {
                "json_alt_key"
            } else {
                "raw_fallback"
            }
        }
        Err(_) => {
            if trimmed.contains("*** Begin Patch") {
                "bare_v4a"
            } else {
                "raw_fallback"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample<'a>(args: &'a str, input: &'a str) -> ApplyPatchTrace<'a> {
        ApplyPatchTrace {
            source: "chat",
            model: "qwen-test",
            call_id: "call_1",
            fc_id: "fc_1",
            args_raw: args,
            input,
            interrupted: false,
            json_truncation: None,
            v4a_truncation: None,
            v4a_validation: None,
            decision: "completed",
            repairs: None,
        }
    }

    #[test]
    fn classify_covers_the_four_paths() {
        assert_eq!(
            classify_extraction(r#"{"input":"*** Begin Patch\n*** End Patch"}"#, ""),
            "json_input"
        );
        assert_eq!(
            classify_extraction(r#"{"patch":"*** Begin Patch"}"#, ""),
            "json_alt_key"
        );
        assert_eq!(
            classify_extraction("*** Begin Patch\n*** End Patch", ""),
            "bare_v4a"
        );
        assert_eq!(classify_extraction("garbage not json", ""), "raw_fallback");
        assert_eq!(classify_extraction("   ", ""), "empty");
    }

    #[test]
    fn build_value_carries_decision_and_reasons() {
        let mut t = sample(
            r#"{"input":"*** Begin Patch\n*** End Patch"}"#,
            "*** Begin Patch\n*** End Patch",
        );
        t.decision = "incomplete";
        t.interrupted = true;
        t.v4a_validation = Some((3, "expected '*** End Patch'"));
        let v = build_value(&t);
        assert_eq!(v["trace_kind"], "apply_patch");
        assert_eq!(v["decision"], "incomplete");
        assert_eq!(v["extraction"], "json_input");
        assert_eq!(v["checks"]["v4a_validation"]["line"], 3);
        let reasons = v["incomplete_reasons"].as_array().unwrap();
        assert!(reasons.iter().any(|r| r == "interrupted"));
        assert!(reasons.iter().any(|r| r == "v4a_invalid"));
    }

    #[test]
    fn build_result_value_flags_error_and_carries_output() {
        let ok = build_result_value("call_x", "Success. Updated 1 file.");
        assert_eq!(ok["phase"], "result");
        assert_eq!(ok["call_id"], "call_x");
        assert_eq!(ok["is_error"], false);
        assert_eq!(ok["output"]["text"], "Success. Updated 1 file.");

        let err = build_result_value("call_y", "error: context does not match at line 12");
        assert_eq!(err["is_error"], true);

        // 真机 seq977 回归:成功结果含文件名 ErrorBoundary.tsx,不能因 "error" 子串误报。
        let ok2 = build_result_value(
            "call_z",
            "Exit code: 0\nWall time: 0.1 seconds\nOutput:\nSuccess. Updated the following files:\nA frontend/src/components/common/ErrorBoundary.tsx\n",
        );
        assert_eq!(
            ok2["is_error"], false,
            "成功结果含 ErrorBoundary 文件名不应误报"
        );

        // 真实失败短语(不带 Exit code 包装)仍要判 error。
        let err2 = build_result_value(
            "call_w",
            "apply_patch verification failed: Failed to find context 'uploadImage' in foo.ts",
        );
        assert_eq!(err2["is_error"], true);
    }

    #[test]
    fn pending_pairs_once_then_dedupes_replay() {
        // 唯一 call_id 避免与并行测试/转换器 emit 撞车
        let id = "call_pending_test_unique_9af3";
        assert!(!take_pending(id), "未登记时不应命中");
        register_pending(id);
        assert!(take_pending(id), "首次结果应配对成功");
        assert!(!take_pending(id), "历史重放的重复结果应被去重(已移除)");
    }

    #[test]
    fn classify_shell_write_flags_real_writes_only() {
        // MOC-263 P3:写盘命令 → 命中对应种类。
        assert!(classify_shell_write("sed -i '' '199,282d' f.rs").contains(&"sed_inplace"));
        assert!(classify_shell_write("sed -i 's/a/b/' f.rs").contains(&"sed_inplace"));
        assert!(classify_shell_write("perl -pi -e 's/x/y/' f.rs").contains(&"perl_inplace"));
        assert!(classify_shell_write("echo \"}\" >> f.rs").contains(&"redirect_write"));
        assert!(classify_shell_write("cat > new.rs <<'EOF'\nx\nEOF").contains(&"redirect_write"));
        assert!(classify_shell_write("tee -a Cargo.toml").contains(&"tee_write"));
        assert!(
            classify_shell_write("python3 - <<'PY'\nopen('f','w').write(1)\nPY")
                .contains(&"python_write")
        );
        assert!(
            classify_shell_write("apply_patch <<'EOF'\n*** Begin Patch\nEOF")
                .contains(&"apply_patch_via_shell")
        );
        // 只读命令 → 空(不滥报):管道/读取/awk NR>=/cargo/find/跑脚本。
        assert!(classify_shell_write("cd x && git log --oneline 2>/dev/null | head").is_empty());
        assert!(classify_shell_write("cat agent/issues.md 2>/dev/null | head -120").is_empty());
        assert!(classify_shell_write("cargo check 2>&1 | tail").is_empty());
        assert!(classify_shell_write("grep -rn foo src/ > /dev/null").is_empty());
        assert!(classify_shell_write("awk 'NR>=199 && NR<=240' f.rs").is_empty());
        assert!(classify_shell_write("python3 train.py --epochs 3").is_empty());
        assert!(classify_shell_write("ls -la && find . -type f").is_empty());
        assert!(classify_shell_write("cat file.rs").is_empty());
        // [MOC-268] **归档产物目标** → 不计 redirect_write(按目标判定,非按命令;phase-2 误报口径修正)。
        assert!(
            classify_shell_write("cd /tmp && gh api repos/o/r/tarball/main > sda.tar.gz 2>/dev/null && tar xzf sda.tar.gz")
                .is_empty(),
            "下载到 tarball 产物不应计 redirect_write"
        );
        assert!(classify_shell_write("curl -sL https://x/y.zip > y.zip").is_empty());
        assert!(
            classify_shell_write("python3 render.py > /tmp/w/plot.tar.gz").is_empty(),
            "归档产物目标不计(即便左侧非下载)"
        );
        // [MOC-268 review] 下载/抓取到**真项目文件**仍计 —— 那是绕过 apply_patch 改 workspace,审计须可见
        // (chatgpt-codex-connector:不按 gh/curl/wget 命令一刀切豁免)。
        assert!(
            classify_shell_write("curl -sL https://x/gen > src/generated.rs")
                .contains(&"redirect_write"),
            "curl 覆盖真项目文件应计 redirect_write"
        );
        assert!(
            classify_shell_write("gh api repos/o/r/contents/x > fixtures/data.json")
                .contains(&"redirect_write"),
            "gh 写真项目文件应计 redirect_write"
        );
        // 真源码写盘仍命中(回归保护:别因排除矫枉过正漏掉真编辑)。
        assert!(classify_shell_write("echo 'x' > src/real.rs").contains(&"redirect_write"));
        assert!(classify_shell_write("printf 'a' > config.toml").contains(&"redirect_write"));
        // [MOC-268 review] 多重定向:逐个看,早一个像归档(`2>err.gz`)不能豁免真文件目标(`> src.rs`)。
        assert!(
            classify_shell_write("tool 2>err.gz > src/generated.rs").contains(&"redirect_write"),
            "早归档 decoy 不应豁免真文件写"
        );
        // 全部目标都是归档产物才豁免。
        assert!(
            classify_shell_write("gh api repos/o/r/tarball/main > out.tar.gz 2>log.gz").is_empty()
        );
    }

    #[test]
    fn extract_and_build_shell_edit() {
        assert_eq!(
            extract_shell_cmd(r#"{"cmd":"sed -i '' '1d' f.rs"}"#).as_deref(),
            Some("sed -i '' '1d' f.rs")
        );
        let v = build_shell_edit_value(
            "chat",
            "glm-5.2",
            "call_1",
            "fc_1",
            "exec_command",
            "sed -i '' '1d' f.rs",
            &["sed_inplace"],
        );
        assert_eq!(v["trace_kind"], "shell_edit");
        assert_eq!(v["bypass"], "apply_patch");
        assert_eq!(v["tool"], "exec_command");
        assert!(v["write_kinds"]
            .as_array()
            .unwrap()
            .iter()
            .any(|k| k == "sed_inplace"));
        assert!(v["cmd"]["text"].as_str().unwrap().contains("sed -i"));
    }

    #[test]
    fn cap_field_truncates_on_char_boundary() {
        let big = "あ".repeat(MAX_FIELD_BYTES); // 3 bytes each → well over cap
        let (text, trunc) = cap_field(&big);
        assert!(text.len() <= MAX_FIELD_BYTES);
        assert!(trunc > 0);
        // 没切坏 UTF-8:能完整重新解析
        assert!(text.chars().all(|c| c == 'あ'));
    }
}
