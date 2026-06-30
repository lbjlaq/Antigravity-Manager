//! apply_patch **pre-flight 自动修复**:在把 V4A patch 发给 Codex apply 之前,读目标文件比对,
//! 自动对齐**安全**的上下文失配(尾随空格 / 首尾空白差异),消灭 V4A 头号失败
//! `apply_patch verification failed: Failed to find expected lines`。
//!
//! ## 为什么需要
//! 弱一点的 chat 模型(非 OpenAI)在大文件上常无法逐字节复刻 `Update File` 的 context/删除行
//! (尾随空格、缩进、记忆偏差)→ Codex 找不到锚点 → apply 失败 → 模型整文件重写,浪费时间和 token。
//! 实测真机报错(rollout 地面真相)正是这类。
//!
//! ## 安全边界(绝不损坏文件 —— 对齐用户「不做破坏性降级」硬规则)
//! - **只动锚点**:`Update File` 里的 context(空格前缀)/ 删除(`-`)行。`+新增` 行**绝不改动**。
//! - **只在唯一匹配时修**:锚点块在文件里按「忽略尾随空格 / 首尾空白」找候选,**恰好一个**位置才对齐;
//!   0 个(模型真改错内容)或 ≥2 个(歧义)一律**原样放行**,交给 Codex parse_patch 暴露真坏,绝不靠猜。
//! - **Add File / Delete File 不碰**(无锚点,不涉及匹配)。读不到文件 / 无 cwd → 原样放行。
//! - 每条修复 / 放行都记进 apply-patch 诊断页,可审计。

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde_json::{json, Value};

/// 一条 pre-flight 处理记录(给诊断页 / 日志)。
#[derive(Debug, Clone, PartialEq)]
pub struct Repair {
    /// patch 里的文件路径(相对,原样)。
    pub file: String,
    /// `repaired`(对齐了锚点)/ `clean`(本就精确匹配,未改)/ `skipped:<原因>`(放行未修)。
    pub kind: String,
    /// 人类可读详情(改了几行 / 为何放行)。
    pub detail: String,
}

impl Repair {
    fn to_value(&self) -> Value {
        json!({"file": self.file, "kind": self.kind, "detail": self.detail})
    }
}

/// 把一组 [`Repair`] 转成诊断 `Value` 数组(给 ApplyPatchTrace 的 `repairs` 字段)。
pub fn repairs_to_value(repairs: &[Repair]) -> Value {
    Value::Array(repairs.iter().map(Repair::to_value).collect())
}

/// [MOC-194/MOC-263] 进程级「最近见过的 cwd」候选历史(most-recent-first,去重,容量上限)。
///
/// **为什么从单槽改成候选列表(MOC-263 P1)**:Codex 只在 turn-start 请求发 `<cwd>`,apply_patch
/// 工具循环后续请求不带 cwd → 靠跨请求记忆。旧实现是**进程级单槽**,多个 Codex 会话并发时(真机
/// 常态:同时开 N 个对话改不同项目)单槽被**别的会话**的 turn-start cwd 持续覆盖 → apply_patch
/// 请求回退到的是**别项目的 stale cwd** → Tier B 读盘规则解析到错目录 → 全程 `skipped:unreadable`
/// (实测 phase-1:5/5 段兜底全废)。改成**最近 N 个不同 cwd 的候选列表**:读盘时对每个候选试
/// `cwd/相对路径` 是否存在,选**第一个存在**的(真项目 cwd 才有该文件,stale cwd 没有 → 自动选对)。
/// 命中错 cwd 的同名文件最坏让后续锚点匹配失败 → 安全 skip,绝不误改(保持「不猜不丢」)。
const CWD_CANDIDATES_CAP: usize = 12;
static CWD_HISTORY: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();

fn cwd_history() -> &'static Mutex<VecDeque<String>> {
    CWD_HISTORY.get_or_init(|| Mutex::new(VecDeque::new()))
}

/// 记一个最近见到的 cwd(去重置顶,超 [`CWD_CANDIDATES_CAP`] 淘汰最旧)。空串忽略。
fn remember_cwd(cwd: &str) {
    if cwd.is_empty() {
        return;
    }
    if let Ok(mut q) = cwd_history().lock() {
        if let Some(pos) = q.iter().position(|c| c == cwd) {
            q.remove(pos);
        }
        q.push_front(cwd.to_owned());
        while q.len() > CWD_CANDIDATES_CAP {
            q.pop_back();
        }
    }
}

/// 最近见过的 cwd 候选(most-recent-first)。
fn recall_cwd_candidates() -> Vec<String> {
    cwd_history()
        .lock()
        .map(|q| q.iter().cloned().collect())
        .unwrap_or_default()
}

/// 是否有任何可用 cwd(当前请求的 `primary` 或历史候选)。byte-exact 规则据此短路。
fn has_cwd_candidate(primary: Option<&str>) -> bool {
    primary.map(|c| !c.is_empty()).unwrap_or(false) || !recall_cwd_candidates().is_empty()
}

/// patch section 的「锚点 probe」,每项 `(is_header, 文本)`:
/// - context(` `)/ 删除(`-`)行去前缀 → `(false, 行内容)`,在候选文件里按**整行 exact**(trim)比对;
/// - `@@ <header>` 的 header 文本 → `(true, header)`,按**子串**比对(残缺头是真实整行的子串,如
///   `系统架构建议` ⊂ `## 6. 系统架构建议`)。两类分开评分:exact 头若被 stale 的同名整行命中会误选,
///   故 header 不进 exact(chatgpt-codex-connector review)。供 [`read_patch_file`] 在同名候选间挑目标文件。
fn anchor_probe<'a>(body: &[&'a str]) -> Vec<(bool, &'a str)> {
    let mut probe = Vec::new();
    for l in body {
        match l.chars().next() {
            Some(' ') | Some('-') => probe.push((false, &l[1..])),
            Some('+') => {} // 新增行 —— 不在目标文件,不作 probe
            _ => {
                if let Some(h) = l.strip_prefix("@@ ") {
                    let h = h.trim();
                    if !h.is_empty() {
                        probe.push((true, h));
                    }
                } else if !l.is_empty() && !l.starts_with("@@") && !l.starts_with("*** ") {
                    // 无前缀行(模型漏写前缀,fix_unprefixed_lines 要按文件整行 exact 匹配来修)→ 整行作
                    // exact probe,使空-probe 路径(漏前缀是唯一锚点时)也能在同名候选间挑对文件
                    // (chatgpt-codex-connector review)。
                    probe.push((false, l));
                }
            }
        }
    }
    probe
}

/// 按候选 cwd 解析并读取 patch 目标文件(MOC-263 P1 + P2)。`primary`(当前请求 cwd,apply_patch
/// 请求通常 None)优先,再按最近 cwd 历史逐个试。`probe` = patch 的 context/删除锚点行内容
/// ([`anchor_probe`]):多个候选 cwd 都存在同名相对文件时(并发会话共享 `README.md`/`package.json`
/// 等),**选内容里命中最多 probe 锚点行的候选**(= patch 真正针对的文件),而非取第一个可读的
/// (chatgpt-codex-connector review P2:取第一个会对错文件对齐)。所有候选 probe 命中均为 0 → 没有
/// 候选是目标 → 返回 None(skip,安全)。`probe` 为空(纯新增 patch 无锚点)→ 退回第一个可读
/// (无从判别、最好努力)。绝对路径直接读。
fn read_patch_file(
    relpath: &str,
    primary: Option<&str>,
    probe: &[(bool, &str)],
) -> Option<(PathBuf, String)> {
    let p = Path::new(relpath);
    if p.is_absolute() {
        return std::fs::read_to_string(p)
            .ok()
            .map(|c| (p.to_path_buf(), c));
    }
    // ① fresh primary 权威:当前请求自带 cwd 且文件可读 → 直接用,交下游决定匹配(含 align_at_headers
    //    的 partial `@@` 子串修复)。**probe 只在多个同名候选间做 tie-breaker,绝不当 gate** —— 否则
    //    残缺 `@@` 头 / 单一候选会因 probe 0 命中被误判 unreadable(chatgpt-codex-connector review P2 二轮)。
    if let Some(c) = primary {
        if !c.is_empty() {
            let abs = Path::new(c).join(p);
            if let Ok(content) = std::fs::read_to_string(&abs) {
                return Some((abs, content));
            }
        }
    }
    // ② 否则用最近 cwd 候选历史(most-recent-first),读出所有存在的同名文件。
    let mut readable: Vec<(PathBuf, String)> = Vec::new();
    for c in recall_cwd_candidates() {
        let abs = Path::new(&c).join(p);
        if let Ok(content) = std::fs::read_to_string(&abs) {
            readable.push((abs, content));
        }
    }
    match readable.len() {
        0 => return None,
        // 单候选 → 直接用(下游决定匹配,partial header 子串修复才有机会);不因 probe 0 命中而 skip。
        1 => return readable.into_iter().next(),
        _ => {}
    }
    // ③ 多个同名候选(并发会话共享 README.md/package.json 等)→ 按锚点 probe 挑 patch 真正针对的文件。
    //    评分:context/删除行(非 header)按**整行 exact**(trim)命中;`@@` 头(header)按**子串**命中
    //    真实整行(残缺头是整行子串,如 `系统架构建议` ⊂ `## 6. 系统架构建议`)。两类合并计分,**唯一
    //    最高分**才选;并列 / 全 0 → None(歧义不猜,违反"不猜不丢"则会对 stale 文件对齐)。
    //    header 不进 exact:否则 stale 的同名整行(恰=残缺头)会以 exact 胜过 real 的子串(review)。
    let probe: Vec<(bool, &str)> = probe
        .iter()
        .map(|&(h, t)| (h, t.trim()))
        .filter(|(_, t)| !t.is_empty())
        .collect();
    if probe.is_empty() {
        return readable.into_iter().next(); // 无锚点(纯新增 patch)→ 最 recent(无需对齐,下游 no-op)
    }
    let scores: Vec<usize> = readable
        .iter()
        .map(|(_, c)| {
            let fl: Vec<&str> = c.lines().map(str::trim).collect();
            probe
                .iter()
                .filter(|&&(is_header, t)| {
                    if is_header {
                        fl.iter().any(|line| !line.is_empty() && line.contains(t))
                    } else {
                        fl.iter().any(|line| *line == t)
                    }
                })
                .count()
        })
        .collect();
    let max = scores.iter().copied().max().unwrap_or(0);
    if max == 0 {
        return None; // 没有候选含任何锚点 → 都不是目标 → skip(安全)
    }
    if scores.iter().filter(|s| **s == max).count() != 1 {
        return None; // 并列最高 = 歧义 → 不猜
    }
    let best_idx = scores.iter().position(|s| *s == max).unwrap();
    Some(readable.swap_remove(best_idx))
}

/// 从 Codex Responses 请求里抽 `<cwd>...</cwd>`(Codex 注入的 environment_context 块,
/// 形如 `<environment_context>\n  <cwd>/abs/path</cwd>\n  <shell>zsh</shell>...`)。
///
/// **遍历 Value 树**找含 `<cwd>` 的字符串节点(其值已是 serde 反转义后的原文)再抽取 —— **不能**
/// 先 `serde_json::to_string(整个请求)` 再搜:那会把字符串值**重新 JSON 转义**,Windows 路径
/// `C:\Users\...` 的反斜杠被翻倍成 `C:\\Users\\...`,resolve_path 拿到错路径(codex-connector #435 P2)。
/// 不依赖 `<cwd>` 落在 instructions 还是某条 input message(任意层级的 string 节点都扫)。
pub fn extract_cwd(request: Option<&Value>) -> Option<String> {
    fn find_in_value(v: &Value) -> Option<String> {
        match v {
            Value::String(s) => extract_cwd_from_str(s),
            Value::Array(a) => a.iter().find_map(find_in_value),
            Value::Object(o) => o.values().find_map(find_in_value),
            _ => None,
        }
    }
    find_in_value(request?)
}

/// 从单个(已反转义的)字符串里抽 `<cwd>...</cwd>`。
fn extract_cwd_from_str(s: &str) -> Option<String> {
    let start = s.find("<cwd>")? + "<cwd>".len();
    let rest = &s[start..];
    let end = rest.find("</cwd>")?;
    let cwd = rest[..end].trim();
    if cwd.is_empty() {
        None
    } else {
        Some(cwd.to_owned())
    }
}

/// [MOC-194 关键] 把请求里的 `<cwd>` 记入进程级缓存。**必须对每个请求调用**(不止 apply_patch):
/// 带 `<cwd>` 的是 **turn-start 请求**(不产生 apply_patch、不调 [`optimize_patch`]),而 apply_patch
/// 出现在**不带 cwd 的工具循环后续请求**里。只在 `optimize_patch` 里记忆 → 永远学不到 cwd(实测:
/// `LAST_CWD` 一直 None、所有 Tier B 读盘规则全程 no-op)。故记忆点必须在每请求都经过的地方
/// (转换器 `with_original_request`),turn-start 的 cwd 才能被后续 apply_patch 请求回退到。
pub fn remember_cwd_from_request(request: Option<&Value>) {
    if let Some(cwd) = extract_cwd(request) {
        remember_cwd(&cwd);
    }
}

/// apply_patch **中间层总入口**:按白名单规则**逐条恢复已知格式错误**,使模型不遵循 prompt 时
/// 产出的畸形 patch 仍能被 Codex 正确 apply。**只动确定的已知坑;未知一律原样放行(不猜不丢)。**
///
/// 两层结构(对齐 [[MOC-194]] 方案):
///
/// **Tier A 语法规整**(镜像 Codex 给 GPT 的 lark 语法,纯字符串、不读盘 —— 把 GPT 靠语法约束生成
/// 保证的合法性,在第三方 chat 路径事后保证):
/// - [`strip_trailing_at`] — 双边 `@@ … @@` → 单边(grammar `change_context: "@@" | "@@ " /(.+)/`;实测 18×)。
/// - [`ensure_add_file_plus`] — Add File 内容行漏 `+` → 补全(grammar `add_line: "+" /(.*)/`,Add File 无歧义)。
/// - [`ensure_v4a_envelope`] — 缺 `*** Begin/End Patch` → 补全(grammar `start: begin_patch hunk+ end_patch`;
///   gotcha #6 + 真机 seq230)。**仅 `json_complete`(非流式截断)时做**,且**放最后**以包裹 Tier B 产物。
///
/// **Tier B 语义恢复**(grammar 管不到的文件状态/内容层,需 `cwd` 读盘):
/// - [`recover_update_empty_file`] — Update 空文件 → Delete+Add(实测 50×,无损)。
/// - [`align_at_headers`] — `@@ <header>` 残缺锚点 → 对齐文件真实整行(`Failed to find context`)。
/// - [`fix_unprefixed_lines`] — Update 内无前缀行 → 按文件判定补 context 空格 / 删重复废行(seq235)。
/// - [`recover_empty_move`] — 空 Update+Move(rename-only)→ Delete+Add 复制原内容(实测 76×)。
/// - [`preflight_repair`] — Update 上下文 byte-exact 失配 → 读盘对齐(实测 134×)。
///
/// 未覆盖的错点:**原样透过**,交 Codex applier 报错(不猜不丢)。
/// `json_complete`:调用方传 `detect_json_truncation(args).is_none()`(chat);gemini args 一次性完整传 `true`。
pub fn optimize_patch(v4a: &str, cwd: Option<&str>, json_complete: bool) -> (String, Vec<Repair>) {
    // [MOC-194/MOC-263] **两类 cwd,分流使用**:
    // - `fresh_cwd` = 当前请求自带的 `<cwd>`(apply_patch 请求通常 None)。**判定文件 == Codex 应用
    //   文件**,可信。
    // - 候选历史 = 跨请求记忆的最近 N 个不同 cwd([`recall_cwd_candidates`])。Codex 只在 turn-start
    //   请求发 `<cwd>`,apply_patch 工具循环后续请求不带 → 靠它回退。MOC-263:从单槽改候选列表,
    //   并发多会话不再被别项目 stale cwd 覆盖(读盘按候选逐个试、选第一个存在的)。
    //
    // **状态改写规则**(`recover_update_empty_file` / `recover_empty_move`:把 Update 转成 Delete+Add)
    // 的判定文件与应用文件(Codex 用 patch 相对路径在真实 cwd 应用)**可能不是同一个** → 错 cwd 下会
    // 删错项目的同名文件(破坏性)。故这两条**只用 fresh_cwd**(判定==应用才安全),**不查候选历史**;
    // apply_patch 请求无 fresh cwd → 自动跳过透过(安全)。
    // **byte-exact 对齐规则**(align/preflight/fix_unprefixed)传 `fresh_cwd` 作 primary,内部经
    // [`read_patch_file`] 再查候选历史:最坏命中错文件也只是「不唯一匹配 / byte 不符」→ 安全 no-op。
    let fresh_cwd = cwd;
    // 当前请求若带 cwd,记入候选历史(turn-start 的 cwd 主要由转换器 `remember_cwd_from_request`
    // 在每请求记入;这里兜底:万一 apply_patch 请求自带 cwd 也纳入)。
    if let Some(c) = cwd {
        remember_cwd(c);
    }
    let mut repairs = Vec::new();
    let mut s = v4a.to_owned();

    // ── Tier A 语法规整(纯字符串)──
    let (s1, r1) = strip_trailing_at(&s);
    s = s1;
    repairs.extend(r1);

    let (s_g, r_g) = ensure_add_file_plus(&s);
    s = s_g;
    repairs.extend(r_g);

    // ── Tier B 语义恢复 ──
    // 注:`Add File 已存在 → Delete+Add 覆盖` 规则**已撤销**(2026-06-09)。它会覆盖已有文件、
    // 可能丢失 Add 内容里没有的现存内容(破坏性降级);且会抢走模型收到 `already exists` 后
    // 自纠为**针对性 Update**(无损)的机会。改为原样透过、交 Codex 报 `already exists` 让模型自纠。
    //
    // 状态改写规则 → **fresh_cwd**(防 stale 删错文件,见上)。
    let (s_f, r_f) = recover_update_empty_file(&s, fresh_cwd);
    s = s_f;
    repairs.extend(r_f);

    let (s3, r3) = recover_empty_move(&s, fresh_cwd);
    s = s3;
    repairs.extend(r3);

    // byte-exact 对齐规则 → 传 fresh_cwd 作 primary,内部 read_patch_file 再查候选历史(最坏安全 no-op)。
    let (s_h, r_h) = align_at_headers(&s, fresh_cwd);
    s = s_h;
    repairs.extend(r_h);

    let (s_u, r_u) = fix_unprefixed_lines(&s, fresh_cwd);
    s = s_u;
    repairs.extend(r_u);

    let (s2, r2) = preflight_repair(&s, fresh_cwd);
    s = s2;
    repairs.extend(r2);

    // ── 信封补全放最后:包裹 Tier B 可能新增的 Delete+Add 等结构 ──
    if json_complete {
        let (s4, r4) = ensure_v4a_envelope(&s);
        s = s4;
        if let Some(r) = r4 {
            repairs.push(r);
        }
    }
    (s, repairs)
}

/// **规则:双边 `@@ … @@` → 单边 `@@ …`**(prompt gotcha #1 / chat-path #1)。V4A 的 `@@` 是
/// **单边** anchor(`@@ <header>`);模型常写成双边 `@@ <header> @@`,Codex 把尾部 `@@` 当字面文本
/// → `Failed to find context '... @@'`。仅处理**列 0 的 `@@` 头行**(正文行有 `+`/`-`/空格 前缀,不碰),
/// 去掉尾部 `@@` 及其前导空白;**裸 `@@`(section 分隔)不动**。
fn strip_trailing_at(v4a: &str) -> (String, Vec<Repair>) {
    let mut changed = 0usize;
    let out: Vec<String> = v4a
        .lines()
        .map(|l| {
            if l.starts_with("@@") {
                let t = l.trim_end();
                // 裸 `@@`(len==2)是合法 section 分隔,跳过;`@@ x @@` 才去尾。
                if t.len() > 2 && t.ends_with("@@") {
                    let body = t[..t.len() - 2].trim_end();
                    if !body.is_empty() && body != "@@" {
                        changed += 1;
                        return body.to_owned();
                    }
                }
            }
            l.to_owned()
        })
        .collect();
    let mut joined = out.join("\n");
    if v4a.ends_with('\n') {
        joined.push('\n');
    }
    let repairs = if changed > 0 {
        vec![Repair {
            file: "(@@ header)".to_owned(),
            kind: "repaired".to_owned(),
            detail: format!("双边 @@ → 单边: {changed} 行(prompt gotcha #1)"),
        }]
    } else {
        Vec::new()
    };
    (joined, repairs)
}

/// **规则 G:Add File 内容行漏 `+` 前缀 → 补全**(grammar `add_hunk: … add_line+`、
/// `add_line: "+" /(.*)/`)。Add File 语义 = 后续每行都是新文件的**字面内容**、必须 `+` 前缀;
/// 模型偶尔漏写 `+` → Codex 不认作内容。Add File section 内**无歧义**(全是新增),给非 `+` 行
/// 统一补 `+`(空行 → 裸 `+`);已是 `+` 的不动(不重复成 `++`)。纯字符串、不读盘。
fn ensure_add_file_plus(v4a: &str) -> (String, Vec<Repair>) {
    if !v4a.contains("*** Add File:") {
        return (v4a.to_owned(), Vec::new());
    }
    let lines: Vec<&str> = v4a.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut repairs = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if let Some(path) = lines[i].strip_prefix("*** Add File: ") {
            out.push(lines[i].to_owned()); // header
            i += 1;
            let mut fixed = 0usize;
            // body 到下一个 `*** ` 控制行 / EOF;Add File body 全是 `+` 内容行。
            while i < lines.len() && !lines[i].starts_with("*** ") {
                if lines[i].starts_with('+') {
                    out.push(lines[i].to_owned());
                } else {
                    out.push(format!("+{}", lines[i]));
                    fixed += 1;
                }
                i += 1;
            }
            if fixed > 0 {
                repairs.push(Repair {
                    file: path.trim().to_owned(),
                    kind: "repaired".to_owned(),
                    detail: format!("Add File {fixed} 行漏 `+` 前缀 → 补全(lark add_line)"),
                });
            }
        } else {
            out.push(lines[i].to_owned());
            i += 1;
        }
    }
    let mut joined = out.join("\n");
    if v4a.ends_with('\n') {
        joined.push('\n');
    }
    (joined, repairs)
}

/// **规则:`@@ <header>` 锚点对齐文件真实行**(真机 seq181:`Failed to find context 'X'`)。
/// V4A 的 `@@ <header>` 是单边锚点,Codex 按**精确整行**匹配文件里的 section 行;模型常写**残缺**
/// 头(如 `@@ 系统架构建议`,而文件真实行是 `## 6. 系统架构建议`)→ 找不到锚点。当 `<header>` 不是
/// 文件里任何**整行**、但**恰好唯一包含于**某一文件行时,把 `@@ <header>` 对齐成 `@@ <该文件整行>`;
/// 0 个 / 多个包含 → 歧义,原样放行(不猜)。裸 `@@`(无 header)不动。需 `cwd`。
fn align_at_headers(v4a: &str, cwd: Option<&str>) -> (String, Vec<Repair>) {
    if !has_cwd_candidate(cwd) {
        return (v4a.to_owned(), Vec::new());
    }
    if !v4a.contains("*** Update File:") {
        return (v4a.to_owned(), Vec::new());
    }
    let lines: Vec<&str> = v4a.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut repairs = Vec::new();
    let mut file_lines: Vec<String> = Vec::new();
    let mut have_file = false;
    let mut fixed = 0usize;
    let mut i = 0;
    while i < lines.len() {
        if let Some(path) = lines[i].strip_prefix("*** Update File: ") {
            // 切到新 Update File section → 按候选 cwd + 锚点 probe 解析目标文件(MOC-263 P1/P2)
            let mut se = i + 1;
            while se < lines.len() && !lines[se].starts_with("*** ") {
                se += 1;
            }
            let probe = anchor_probe(&lines[i + 1..se]);
            file_lines = read_patch_file(path.trim(), cwd, &probe)
                .map(|(_, c)| c.lines().map(str::to_owned).collect())
                .unwrap_or_default();
            have_file = !file_lines.is_empty();
            out.push(lines[i].to_owned());
            i += 1;
            continue;
        }
        // `@@ <header>` 锚点(非裸 `@@`),且文件已载入
        if have_file {
            if let Some(header) = lines[i].strip_prefix("@@ ") {
                let h = header.trim();
                if !h.is_empty() && !file_lines.iter().any(|fl| fl == h) {
                    let hits: Vec<&String> =
                        file_lines.iter().filter(|fl| fl.contains(h)).collect();
                    if hits.len() == 1 {
                        out.push(format!("@@ {}", hits[0]));
                        fixed += 1;
                        i += 1;
                        continue;
                    }
                }
            }
        }
        out.push(lines[i].to_owned());
        i += 1;
    }
    if fixed > 0 {
        repairs.push(Repair {
            file: "(@@ anchor)".to_owned(),
            kind: "repaired".to_owned(),
            detail: format!("@@ 锚点残缺 → 对齐文件真实整行: {fixed} 处(Failed to find context)"),
        });
    }
    let mut joined = out.join("\n");
    if v4a.ends_with('\n') {
        joined.push('\n');
    }
    (joined, repairs)
}

/// **规则:`Update File` 目标是空文件 → `Delete File + Add File`**(prompt gotcha #3,无损)。
/// `*** Update File:` 无法作用于空文件(Codex 报 `cannot operate on a completely empty file`)。
/// 当目标文件存在且**为空**(真正 0 字节,非纯空白)、且 Update body 是**纯 `+` 行**(纯写内容,无 `-`/context 可
/// 匹配)时,转成 `*** Delete File: X` + `*** Add File: X` + 原 `+` body(空文件无内容可丢 → 无损)。
/// body 含 `-`/context(模型在空文件上写了匹配行,本就矛盾)/ 含 Move(交给 empty-move 规则)→ 不动。需 `cwd`。
fn recover_update_empty_file(v4a: &str, cwd: Option<&str>) -> (String, Vec<Repair>) {
    let Some(cwd) = cwd else {
        return (v4a.to_owned(), Vec::new());
    };
    if !v4a.contains("*** Update File:") {
        return (v4a.to_owned(), Vec::new());
    }
    let lines: Vec<&str> = v4a.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut repairs = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if let Some(path) = lines[i].strip_prefix("*** Update File: ") {
            let p = path.trim();
            // 只认**真正 0 字节**(Codex 仅对 `completely empty file` 报错;纯空白文件仍是可读内容、
            // 能正常 Update)。用 `c.trim().is_empty()` 会把纯空白文件也转 Delete+Add → 丢掉那些
            // 空白字节(破坏性,codex-connector #435 P2)。
            let is_empty = std::fs::read_to_string(resolve_path(p, cwd))
                .map(|c| c.is_empty())
                .unwrap_or(false);
            if is_empty {
                let body_start = i + 1;
                let mut j = body_start;
                while j < lines.len() && !lines[j].starts_with("*** ") {
                    j += 1;
                }
                let body = &lines[body_start..j];
                let has_move = body
                    .first()
                    .map(|l| l.starts_with("*** Move to:"))
                    .unwrap_or(false);
                let content: Vec<&&str> = body
                    .iter()
                    .filter(|l| !l.trim().is_empty() && !l.starts_with("@@"))
                    .collect();
                let all_plus = !content.is_empty() && content.iter().all(|l| l.starts_with('+'));
                if !has_move && all_plus {
                    out.push(format!("*** Delete File: {p}"));
                    out.push(format!("*** Add File: {p}"));
                    for b in body {
                        if b.starts_with('+') {
                            out.push((*b).to_owned());
                        }
                    }
                    repairs.push(Repair {
                        file: p.to_owned(),
                        kind: "repaired".to_owned(),
                        detail: "Update 空文件 → Delete+Add 写入(prompt gotcha #3)".to_owned(),
                    });
                    i = j;
                    continue;
                }
            }
        }
        out.push(lines[i].to_owned());
        i += 1;
    }
    let mut joined = out.join("\n");
    if v4a.ends_with('\n') {
        joined.push('\n');
    }
    (joined, repairs)
}

/// **规则:空 `Update File + Move to`(rename-only)→ `Delete File + Add File`**(prompt gotcha #7)。
/// 模型想纯重命名却写 `*** Update File: X` + `*** Move to: Y` 且**无 hunk** → Codex 报
/// `Update file hunk for path 'X' is empty`。按 prompt **自身建议**恢复:读 X 原内容,转成
/// `*** Delete File: X` + `*** Add File: Y` + 逐行 `+` 复制(空行为裸 `+`)。读不到 X → 原样放行。
fn recover_empty_move(v4a: &str, cwd: Option<&str>) -> (String, Vec<Repair>) {
    let Some(cwd) = cwd else {
        return (v4a.to_owned(), Vec::new());
    };
    if !v4a.contains("*** Move to:") {
        return (v4a.to_owned(), Vec::new());
    }
    let lines: Vec<&str> = v4a.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut repairs = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        // 匹配 `*** Update File: X` 紧跟 `*** Move to: Y`,且 Move 后到下一个 `*** ` 控制行之间无 hunk 行。
        if let Some(old) = lines[i].strip_prefix("*** Update File: ") {
            if i + 1 < lines.len() {
                if let Some(new) = lines[i + 1].strip_prefix("*** Move to: ") {
                    // 看 Move 之后、下一个**文件操作**控制行之前有没有 hunk 内容行。
                    // 注:`*** End of File` 是文档化的 **hunk 内标记**(prompt RENAME/MOVE 段),不是
                    // section 边界 —— 不能停在它(否则 rename+EOF 追加会被误判成空 rename、转成丢内容的
                    // Delete+Add,codex-connector #435 P1)。它本身即表示「有 hunk」,继续往后扫。
                    let mut j = i + 2;
                    let mut has_hunk = false;
                    while j < lines.len() {
                        let t = lines[j];
                        if t.trim_end() == "*** End of File" {
                            has_hunk = true;
                            j += 1;
                            continue;
                        }
                        if t.starts_with("*** ") {
                            break; // 真正的下一个文件操作 / End Patch 边界
                        }
                        if t.starts_with('+')
                            || t.starts_with('-')
                            || t.starts_with(' ')
                            || t.starts_with("@@")
                        {
                            has_hunk = true;
                        }
                        j += 1;
                    }
                    if !has_hunk {
                        // 空 rename-only → 读原文件转 Delete+Add。读不到 / 内容为空 → 不转(空 Add File
                        // 体可能被 Codex 拒)→ 原样放行交 Codex 处理。
                        let abs = resolve_path(old.trim(), cwd);
                        match std::fs::read_to_string(&abs) {
                            Ok(content) if !content.is_empty() => {
                                out.push(format!("*** Delete File: {}", old.trim()));
                                out.push(format!("*** Add File: {}", new.trim()));
                                for cl in content.lines() {
                                    out.push(format!("+{cl}"));
                                }
                                repairs.push(Repair {
                                    file: old.trim().to_owned(),
                                    kind: "repaired".to_owned(),
                                    detail: format!(
                                        "空 Update+Move(rename-only)→ Delete+Add 复制原内容 → {}(prompt gotcha #7)",
                                        new.trim()
                                    ),
                                });
                                i = j; // 跳过原 Update/Move(+空体)
                                continue;
                            }
                            _ => {
                                repairs.push(Repair {
                                    file: old.trim().to_owned(),
                                    kind: "skipped:unreadable_or_empty".to_owned(),
                                    detail: "空 Update+Move 但原文件读不到 / 为空 → 原样放行"
                                        .to_owned(),
                                });
                            }
                        }
                    }
                }
            }
        }
        out.push(lines[i].to_owned());
        i += 1;
    }
    let mut joined = out.join("\n");
    if v4a.ends_with('\n') {
        joined.push('\n');
    }
    (joined, repairs)
}

/// 末个 patch 操作是否为「`*** Add File:` + 代码/结构化配置文件目标」。用于 [`ensure_v4a_envelope`] 判定
/// 末行 `+*** End Patch` 可否安全剥成终止符:**仅 Add File**(新建文件,裸 `*** End Patch` 不可能是合法
/// 源码的**末行** → 必是误前缀终止符)才剥;`*** Update File:` 的 `+*** End Patch` 是**新增行**(可能往
/// 字符串 / fixture 里加这串字),剥了=丢新增 → 不剥(chatgpt-codex-connector review:限定 Add File)。
/// 文档 / 文本 / 未知扩展也不剥(可能是正文,留 incomplete 不猜)。allowlist 保守。MOC-268。
fn last_op_is_add_file_code(body: &str) -> bool {
    let last_op = body.lines().rev().find(|l| {
        let t = l.trim_end();
        t.starts_with("*** Add File: ")
            || t.starts_with("*** Update File: ")
            || t.starts_with("*** Delete File: ")
    });
    let Some(path) = last_op.and_then(|l| l.trim_end().strip_prefix("*** Add File: ")) else {
        return false; // 无操作,或末操作是 Update/Delete(非 Add File)→ 不剥
    };
    let ext = std::path::Path::new(path.trim())
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase());
    matches!(
        ext.as_deref(),
        Some(
            "rs" | "ts"
                | "tsx"
                | "js"
                | "jsx"
                | "mjs"
                | "cjs"
                | "py"
                | "go"
                | "java"
                | "kt"
                | "kts"
                | "c"
                | "h"
                | "cc"
                | "cpp"
                | "cxx"
                | "hpp"
                | "hh"
                | "cs"
                | "rb"
                | "php"
                | "swift"
                | "scala"
                | "lua"
                | "sql"
                | "sh"
                | "bash"
                | "zsh"
                | "css"
                | "scss"
                | "sass"
                | "less"
                | "html"
                | "htm"
                | "xml"
                | "vue"
                | "svelte"
                | "json"
                | "toml"
                | "yaml"
                | "yml"
                | "gradle"
                | "cmake"
                | "proto"
                | "graphql"
                | "dart"
                | "r"
        )
    )
}

/// **缺信封自动补全**:模型常只写 `*** Add/Update File:` + 内容,漏掉 `*** Begin Patch` /
/// `*** End Patch` 头尾 → Codex(及本 adapter 的 V4A 校验)判 incomplete → 模型被迫重试。
/// 当 patch 含至少一个 `*** Add/Update/Delete File:` 操作、JSON 已完整(调用方先 gate
/// `detect_json_truncation` 为 None 才调本函数,确保不是流式截断)、但缺 Begin/End 信封时,
/// **纯补标记**(不改一字节内容、不猜),返回 `(补全后, Some(Repair))`;本就完整 / 非 patch 体
/// 返回 `(原样, None)`。
///
/// 安全:缺 Begin 时**仅当首个非空行就是操作行**才在最前补 `*** Begin Patch`(有前导散文则不动,
/// 交给 `repair_v4a_envelope` / Codex);缺 End 时去尾随空白后补 `*** End Patch`。
pub fn ensure_v4a_envelope(input: &str) -> (String, Option<Repair>) {
    let is_op = |l: &str| {
        let t = l.trim_end();
        t.starts_with("*** Add File:")
            || t.starts_with("*** Update File:")
            || t.starts_with("*** Delete File:")
    };
    if !input.lines().any(is_op) {
        return (input.to_owned(), None); // 不是可识别的 patch 体,不碰
    }
    let has_begin = input.lines().any(|l| l.trim_end() == "*** Begin Patch");
    let has_end = input.lines().any(|l| l.trim_end() == "*** End Patch");
    if has_begin && has_end {
        return (input.to_owned(), None);
    }
    let mut body = input.to_owned();
    let mut added: Vec<&str> = Vec::new();
    if !has_begin {
        // 仅当首个非空行就是操作行才安全(无前导散文混入信封内)。
        let first_nonempty = input.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
        if !is_op(first_nonempty) {
            return (input.to_owned(), None);
        }
        body = format!("*** Begin Patch\n{body}");
        added.push("Begin Patch");
    }
    if !has_end {
        let trimmed = body.trim_end();
        let last = trimmed.lines().last().unwrap_or("");
        // [MOC-268] **只有 `+*** End Patch`(Add 行前缀)** 才是「模型给终止符误加前缀」的形态。
        // ` *** End Patch`(context)/ `-*** End Patch`(deletion)是**合法 Update hunk 行**——例如模型
        // 用 Update **删除**文件里之前残留的 `*** End Patch`(`-*** End Patch`),或用它当 context 锚点;
        // 把它们当终止符剥会**静默丢弃删除 / 破坏锚点**(chatgpt-codex-connector review)→ 故 ` `/`-` 一律
        // 走下面正常 append(补真终止符,hunk 行原样保留)。
        // 对 `+*** End Patch` 再**按文件类型消歧**(用户拍板):
        //   · 代码 / 结构化配置文件(裸 `*** End Patch` 不可能是合法源码末行)→ 必是误前缀终止符 → **剥前缀**
        //     (`head` 切到末行起点、保留其前换行;末行 ASCII,边界安全)。
        //   · 文档 / 文本 / 未知(可能是正文末行)→ **不猜**:不剥(免删正文)、不追加(免残留),留 incomplete
        //     交下游判截断、模型按 guidance 规则2 重发。prompt 才是根治,中间层只在确定安全时介入。
        if last == "+*** End Patch" {
            if last_op_is_add_file_code(&body) {
                let head = &trimmed[..trimmed.len() - last.len()];
                body = format!("{head}*** End Patch");
                added.push("End Patch(代码文件·剥误加前缀终止符)");
            } else {
                return (
                    body,
                    Some(Repair {
                        file: "(envelope)".to_owned(),
                        kind: "skipped:ambiguous_prefixed_end".to_owned(),
                        detail:
                            "末行 +*** End Patch 且目标非代码文件(可能是正文)→ 不猜不补全,留 incomplete"
                                .to_owned(),
                    }),
                );
            }
        } else {
            // 含 ` *** End Patch` / `-*** End Patch`(合法 hunk 行)及普通内容末行 → 正常补真终止符。
            body = format!("{trimmed}\n*** End Patch");
            added.push("End Patch");
        }
    }
    (
        body,
        Some(Repair {
            file: "(envelope)".to_owned(),
            kind: "repaired".to_owned(),
            detail: format!("模型漏写信封,自动补全: {}", added.join(" + ")),
        }),
    )
}

/// **规则:Update body 内**无前缀行**按文件判定补全**(真机 seq235:单行漏前缀 → validate 拒 →
/// 整份 Update 重写浪费)。grammar `change_line: ("+"|"-"|" ") /(.*)/` 要求每行带前缀;模型偶尔
/// 漏写一行的前缀。**非破坏性**修(只补前缀 / 删可证重复的废行,绝不丢内容):
/// - 无前缀行**与相邻 `+<同内容>` 行重复**(模型写了两遍)→ 删该废行(内容在 `+` 行里,不丢);
/// - 否则无前缀**非空**行**在目标文件里有完全相同的整行** → 它是 context 行漏了空格 → 补 ` `
///   (合法 context 且 byte-exact;最不破坏的解释:行保留。模型若本意是删,顶多没删成、无数据损失);
/// - 其余(不在文件、非重复、空行)→ 原样透过,交 validate 报错让模型自纠(不猜)。
///
/// 仅作用于 `*** Update File:` section(Add File 的漏 `+` 由 [`ensure_add_file_plus`] 管)。需 `cwd`。
fn fix_unprefixed_lines(v4a: &str, cwd: Option<&str>) -> (String, Vec<Repair>) {
    if !has_cwd_candidate(cwd) {
        return (v4a.to_owned(), Vec::new());
    }
    if !v4a.contains("*** Update File:") {
        return (v4a.to_owned(), Vec::new());
    }
    let lines: Vec<&str> = v4a.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut repairs = Vec::new();
    let mut in_update = false;
    let mut file_lines: Vec<String> = Vec::new();
    let mut drop_dups = 0usize;
    let mut add_ctx = 0usize;
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        if let Some(path) = l.strip_prefix("*** Update File: ") {
            in_update = true;
            // 按候选 cwd + 锚点 probe 解析目标文件(MOC-263 P1/P2)。
            let mut se = i + 1;
            while se < lines.len() && !lines[se].starts_with("*** ") {
                se += 1;
            }
            let probe = anchor_probe(&lines[i + 1..se]);
            file_lines = read_patch_file(path.trim(), cwd, &probe)
                .map(|(_, c)| c.lines().map(str::to_owned).collect())
                .unwrap_or_default();
            out.push(l.to_owned());
            i += 1;
            continue;
        }
        if l.starts_with("*** ") {
            in_update = false; // 任何其它控制行结束 Update body
            out.push(l.to_owned());
            i += 1;
            continue;
        }
        let first = l.chars().next();
        let valid = matches!(first, Some('+') | Some('-') | Some(' '))
            || l.starts_with("@@")
            || l.is_empty();
        if in_update && !valid {
            // case1:与相邻 `+<同内容>` 重复的废行 → 删(内容在 + 行里,不丢)
            let plus_dup = format!("+{l}");
            let next_dup = lines.get(i + 1).map(|n| *n == plus_dup).unwrap_or(false);
            let prev_dup = out.last().map(|o| o == &plus_dup).unwrap_or(false);
            if next_dup || prev_dup {
                drop_dups += 1;
                i += 1;
                continue;
            }
            // case2:文件里有完全相同整行 → context 漏空格 → 补 ` `
            if file_lines.iter().any(|fl| fl == l) {
                out.push(format!(" {l}"));
                add_ctx += 1;
                i += 1;
                continue;
            }
            // else:透过(不猜)
        }
        out.push(l.to_owned());
        i += 1;
    }
    if drop_dups + add_ctx > 0 {
        repairs.push(Repair {
            file: "(unprefixed)".to_owned(),
            kind: "repaired".to_owned(),
            detail: format!(
                "Update 无前缀行修复: 补 context 空格 {add_ctx} / 删重复废行 {drop_dups}(lark change_line)"
            ),
        });
    }
    let mut joined = out.join("\n");
    if v4a.ends_with('\n') {
        joined.push('\n');
    }
    (joined, repairs)
}

/// 对 V4A patch 做 pre-flight 修复。`cwd` 用于把 patch 的相对路径解析到真实文件。
/// 返回 `(修复后 V4A, 处理记录)`。无 cwd / 无 `Update File` / 读不到文件时 V4A 原样返回。
pub fn preflight_repair(v4a: &str, cwd: Option<&str>) -> (String, Vec<Repair>) {
    if !has_cwd_candidate(cwd) {
        return (v4a.to_owned(), Vec::new());
    }
    // 没有任何 Update File 直接短路(Add/Delete File 不涉及锚点匹配)。
    if !v4a.contains("*** Update File:") {
        return (v4a.to_owned(), Vec::new());
    }
    let mut repairs = Vec::new();
    let lines: Vec<&str> = v4a.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if let Some(path) = line.strip_prefix("*** Update File: ") {
            out.push(line.to_owned());
            i += 1;
            // 收集本 Update File section 的 body(到下一个 `*** ` 控制行为止)。
            let body_start = i;
            while i < lines.len() && !lines[i].starts_with("*** ") {
                i += 1;
            }
            let body = &lines[body_start..i];
            let (repaired_body, rep) = repair_update_section(path.trim(), body, cwd);
            out.extend(repaired_body);
            repairs.push(rep);
        } else {
            out.push(line.to_owned());
            i += 1;
        }
    }
    // 保留尾随换行语义:lines() 丢掉末尾换行,join 后若原文以 \n 结尾则补上。
    let mut joined = out.join("\n");
    if v4a.ends_with('\n') {
        joined.push('\n');
    }
    (joined, repairs)
}

/// [MOC-263 P0] 在 `file[floor..]` 里找从 `anchors[0]` 起、能**唯一**匹配的最长连续块。
/// 锚点用「忽略尾随空格」比较(段内字节漂移留给后续 repair_hunk 对齐)。返回 `(块长 = 匹配的锚点数,
/// 文件起点)`。最长且唯一 → Some;最长的非空匹配若 >1 处(歧义)→ None(更短只会更歧义);全 0 → None。
fn longest_unique_block(anchors: &[&str], file: &[&str], floor: usize) -> Option<(usize, usize)> {
    if anchors.is_empty() || floor >= file.len() {
        return None;
    }
    // [MOC-263 P1] 段首锚点必须在 `file[floor..]` **全局唯一**,否则该段起点歧义 —— 同一行在别处也出现时,
    // 贪心最长块会靠更长块的"唯一性"选中**无关的更早区域**(file 有旧 `A/B/C` 块 + 真实 `A/B…gap…C/D`
    // 区,body `A/-B/ C/-D` 被切成旧块的 hunk → 从错块删 B),而不切分时本会安全失败。起点不唯一 = 切分
    // 非唯一确定 → bail,原样透过交模型自纠(chatgpt-codex-connector review;不猜不丢)。
    let first = anchors[0].trim_end();
    if file[floor..]
        .iter()
        .filter(|l| l.trim_end() == first)
        .count()
        != 1
    {
        return None;
    }
    let max_len = anchors.len().min(file.len() - floor);
    for len in (1..=max_len).rev() {
        let block = &anchors[..len];
        let mut hits: Vec<usize> = Vec::new();
        let mut start = floor;
        while start + len <= file.len() {
            if (0..len).all(|t| file[start + t].trim_end() == block[t].trim_end()) {
                hits.push(start);
                if hits.len() > 1 {
                    break;
                }
            }
            start += 1;
        }
        match hits.len() {
            1 => return Some((len, hits[0])),
            0 => continue,    // 太长(跨越文件里的跳变)→ 缩短再试
            _ => return None, // 最长非空匹配即歧义 → 放弃(不猜不丢)
        }
    }
    None
}

/// [MOC-263 P0] 把**无 `@@`** 的 Update body 按文件真实位置切成多个 hunk。
///
/// 真机最主要 apply 失败因(phase-1 seg1/seg3):模型把多个**不连续**编辑组拼进一个 Update File 块、
/// 漏写 `@@` 分隔 → applier 把整块当一段连续上下文匹配 → `Failed to find expected lines`。
/// 这里按锚点(context/delete)序列贪心切成**有序、不重叠、各自唯一匹配**的 N 段(每段 = 从上一段
/// 之后起、唯一匹配的最长连续锚点块),`+` 新增行随相邻段保留。仅 **N≥2 且每段都能唯一定位**时返回
/// `Some`;单段 / 任一段歧义或无法定位 → `None`(调用方原样透过,不猜不丢)。调用方用裸 `@@` 串接各段。
fn segment_no_at_body<'a>(body: &[&'a str], file: &[&str]) -> Option<Vec<Vec<&'a str>>> {
    let anchors: Vec<(usize, &str)> = body
        .iter()
        .enumerate()
        .filter_map(|(idx, l)| match l.chars().next() {
            Some(' ') | Some('-') => Some((idx, &l[1..])),
            _ => None,
        })
        .collect();
    if anchors.len() < 2 {
        return None;
    }
    let anchor_contents: Vec<&str> = anchors.iter().map(|(_, c)| *c).collect();

    // 贪心分段:每段 = 从 floor 起唯一匹配的最长连续锚点块。
    // 记 (anchor_start, anchor_end_excl, file_start, file_end_excl);floor 单调推进保证有序不重叠。
    let mut raw: Vec<(usize, usize, usize, usize)> = Vec::new();
    let mut ai = 0usize;
    let mut floor = 0usize;
    while ai < anchors.len() {
        let (len, pos) = longest_unique_block(&anchor_contents[ai..], file, floor)?;
        raw.push((ai, ai + len, pos, pos + len));
        ai += len;
        floor = pos + len;
    }

    // 合并相邻段:段间 file 间隙若**全空行**(模型漏写文件里的空行)→ 同一 hunk,不在此切,
    // 交 repair_hunk 的 EP-1 blank-tolerant 处理(否则会把空行漂移误切成两段,破坏既有行为)。
    // 只在间隙含**非空行**(真·不连续编辑区域)时才保留为独立段。
    let mut groups: Vec<(usize, usize, usize, usize)> = Vec::new();
    for g in raw {
        if let Some(last) = groups.last_mut() {
            let gap = &file[last.3..g.2];
            if gap.iter().all(|l| l.trim().is_empty()) {
                last.1 = g.1;
                last.3 = g.3;
                continue;
            }
        }
        groups.push(g);
    }
    if groups.len() < 2 {
        return None; // 单段(或全因空行间隙合并成单段)→ 没必要切,交回常规路径
    }

    // [MOC-263 P0 安全防护] 段间「浮动 `+` 插入行」落点歧义 → 一律 bail。
    // 若前段末锚点与后段首锚点之间存在**任何** `+` 新增行,该 `+` 的落点都无法从 V4A 唯一确定 ——
    // 它可能是前段末尾的插入,也可能是模型写给后段的「引入行」;段间隔着非空内容时两种落点是文件里
    // 不同位置,猜错就是**静默错误 apply**(违反不猜不丢)。**关键**:即便前段末锚点是 `-` 删除也不安全
    // (chatgpt-codex-connector review 指出的混合 replace+insert:`-return 1`/`+return 42`/`+@memoize`/
    // ` def beta():` —— `+return 42` 是替换、`+@memoize` 却是给后段的引入行,二者无法区分)→ 不做
    // "前段有删除就放行"的豁免,只要 gap 里有 `+` 就放弃整次分段、原样透过(交模型自纠)。
    // 纯删除 / 纯上下文的多区(gap 无 `+`)仍安全切。
    for gi in 0..groups.len() - 1 {
        let last_anchor_line = anchors[groups[gi].1 - 1].0;
        let next_anchor_line = anchors[groups[gi + 1].0].0;
        let gap_has_add = body[last_anchor_line + 1..next_anchor_line]
            .iter()
            .any(|l| l.starts_with('+'));
        if gap_has_add {
            return None;
        }
    }

    // 段 g 的 body 行区间:首段含开头前导行(body[0..首锚点]);其余段从其首锚点起,
    // 到下一段首锚点止 → 段内 / 段后的 `+` 行随**前**段保留。
    let mut subhunks: Vec<Vec<&'a str>> = Vec::new();
    for gi in 0..groups.len() {
        let line_start = if gi == 0 { 0 } else { anchors[groups[gi].0].0 };
        let line_end = if gi + 1 < groups.len() {
            anchors[groups[gi + 1].0].0
        } else {
            body.len()
        };
        subhunks.push(body[line_start..line_end].to_vec());
    }
    Some(subhunks)
}

/// 修复一个 `Update File` section 的 body。`path` 是 patch 里的(相对)路径。
/// `cwd` 是当前请求 cwd(primary hint),读盘经 [`read_patch_file`] 再按候选历史解析(MOC-263)。
fn repair_update_section(path: &str, body: &[&str], cwd: Option<&str>) -> (Vec<String>, Repair) {
    let probe = anchor_probe(body);
    let Some((_abs, content)) = read_patch_file(path, cwd, &probe) else {
        return (
            body.iter().map(|l| (*l).to_owned()).collect(),
            Repair {
                file: path.to_owned(),
                kind: "skipped:unreadable".to_owned(),
                detail: format!("读不到文件 {path}(候选 cwd 均无)→ 原样放行"),
            },
        );
    };
    let file_lines: Vec<&str> = content.lines().collect();

    // [MOC-263 P0] body 无 `@@` 但含多个不连续编辑组(模型漏写 `@@` 分隔)→ 自动按文件位置切段、
    // 用裸 `@@` 串接,使 applier 把各段当独立 hunk 定位(否则整块当一段连续上下文必失配)。
    // 仅唯一可分段时才动;单段 / 歧义 → 保持原 body 交常规路径。
    let mut split_owned: Vec<&str> = Vec::new();
    // 用**列 0** `@@`(不 trim_start)判断是否已有 hunk 分隔,与下方实际分割器(`l.starts_with("@@")`)
    // 一致 —— 否则 context 行 ` @@ ...`(前导空格、内容以 @@ 开头,如 markdown/diff 文本)会被误当分隔符、
    // 错误禁用自动切分,而分割器又不切它 → 仍失败(chatgpt-codex-connector review)。
    let did_split = if !body.iter().any(|l| l.starts_with("@@")) {
        match segment_no_at_body(body, &file_lines) {
            Some(subhunks) => {
                for (k, sub) in subhunks.iter().enumerate() {
                    if k > 0 {
                        split_owned.push("@@");
                    }
                    split_owned.extend_from_slice(sub);
                }
                true
            }
            None => false,
        }
    } else {
        false
    };
    let effective_body: &[&str] = if did_split { &split_owned } else { body };

    // 把 body 切成 hunk(按 `@@` 行分段;`@@` 行本身保留、不参与锚点匹配)。
    let mut new_body: Vec<String> = Vec::with_capacity(effective_body.len());
    let mut repaired_hunks = 0;
    let mut clean_hunks = 0;
    let mut skipped: Vec<String> = Vec::new();
    let mut hunk: Vec<&str> = Vec::new();
    let flush = |hunk: &mut Vec<&str>,
                 new_body: &mut Vec<String>,
                 repaired_hunks: &mut usize,
                 clean_hunks: &mut usize,
                 skipped: &mut Vec<String>| {
        if hunk.is_empty() {
            return;
        }
        match repair_hunk(hunk, &file_lines) {
            HunkOutcome::Clean => {
                *clean_hunks += 1;
                new_body.extend(hunk.iter().map(|l| (*l).to_owned()));
            }
            HunkOutcome::Repaired(fixed) => {
                *repaired_hunks += 1;
                new_body.extend(fixed);
            }
            HunkOutcome::Skipped(reason) => {
                skipped.push(reason);
                new_body.extend(hunk.iter().map(|l| (*l).to_owned()));
            }
        }
        hunk.clear();
    };

    for &l in effective_body {
        if l.starts_with("@@") {
            flush(
                &mut hunk,
                &mut new_body,
                &mut repaired_hunks,
                &mut clean_hunks,
                &mut skipped,
            );
            new_body.push(l.to_owned());
        } else {
            hunk.push(l);
        }
    }
    flush(
        &mut hunk,
        &mut new_body,
        &mut repaired_hunks,
        &mut clean_hunks,
        &mut skipped,
    );

    let kind = if repaired_hunks > 0 || did_split {
        "repaired"
    } else if skipped.is_empty() {
        "clean"
    } else {
        "skipped:no_unique_match"
    };
    let detail = format!(
        "{}hunk: 修复 {repaired_hunks} / 本就匹配 {clean_hunks} / 放行 {}{}",
        if did_split {
            "多 hunk 无 @@ 分隔 → 自动按文件位置切段插裸 @@; "
        } else {
            ""
        },
        skipped.len(),
        if skipped.is_empty() {
            String::new()
        } else {
            format!(" ({})", skipped.join("; "))
        }
    );
    (
        new_body,
        Repair {
            file: path.to_owned(),
            kind: kind.to_owned(),
            detail,
        },
    )
}

enum HunkOutcome {
    /// 锚点精确匹配文件,无需改。
    Clean,
    /// 锚点对齐成文件真实字节后的整个 hunk(含原样的 `+` 行)。
    Repaired(Vec<String>),
    /// 未修(0 或多个匹配),附原因。
    Skipped(String),
}

/// 修一个 hunk:锚点 = context(空格前缀)+ 删除(`-`)行的**内容**(去前缀),按序应是文件里的
/// 连续块。精确匹配→Clean;否则按「忽略尾随空格 / 首尾空白」找候选,唯一→对齐,否则放行。
fn repair_hunk(hunk: &[&str], file_lines: &[&str]) -> HunkOutcome {
    // 锚点行在 hunk 里的下标 + 内容(去单字符前缀)。
    let anchors: Vec<(usize, &str)> = hunk
        .iter()
        .enumerate()
        .filter_map(|(idx, l)| match l.chars().next() {
            Some(' ') => Some((idx, &l[1..])),
            Some('-') => Some((idx, &l[1..])),
            _ => None, // '+' 新增行 / 空行 / 其它不作锚点
        })
        .collect();
    if anchors.is_empty() {
        return HunkOutcome::Clean; // 纯新增,无锚点
    }
    let anchor_contents: Vec<&str> = anchors.iter().map(|(_, c)| *c).collect();

    // 精确匹配:文件里存在连续块完全等于锚点内容 → 无需修(Codex 自己能找到)。
    if !find_block(file_lines, &anchor_contents, |a, b| a == b).is_empty() {
        return HunkOutcome::Clean;
    }

    // 模糊匹配:逐行「忽略尾随空格」相等;仍 0 个再退「首尾空白都忽略」。
    let mut matches = find_block(file_lines, &anchor_contents, |a, b| {
        a.trim_end() == b.trim_end()
    });
    let mut mode = "尾随空格";
    if matches.is_empty() {
        matches = find_block(file_lines, &anchor_contents, |a, b| a.trim() == b.trim());
        mode = "首尾空白";
    }
    match matches.len() {
        1 => {
            let pos = matches[0];
            // 把锚点行对齐成文件真实字节(保留 hunk 里 +/- /空格 的交错与 `+` 行)。
            let mut fixed: Vec<String> = hunk.iter().map(|l| (*l).to_owned()).collect();
            for (k, (idx, _)) in anchors.iter().enumerate() {
                let prefix = hunk[*idx].chars().next().unwrap(); // ' ' 或 '-'
                let file_line = file_lines[pos + k];
                fixed[*idx] = format!("{prefix}{file_line}");
            }
            HunkOutcome::Repaired(fixed)
        }
        n if n > 1 => HunkOutcome::Skipped(format!("{mode}下 {n} 处匹配(歧义)")),
        // 0 连续匹配 → 试「忽略空行差异」(EP-1:模型漏/多写空行致整块失配)。锚点**非空行**序列
        // 在文件里唯一定位(允许文件该区间含模型漏写的空行),命中则用文件真实区间(含空行 + 字节)
        // 重建锚点,`+` 插入行保持原位。0/多处仍放行(不猜)。
        _ => {
            // blank-tolerant 重建会丢弃空白锚点行、改用文件空行 → 无法忠实表达「删除一个空行」的 `-`
            // (会被静默转成 context = 该删没删)。若 hunk 含空白行删除,放弃 blank-tolerant、透过(不猜)。
            let has_blank_deletion = hunk
                .iter()
                .any(|l| l.starts_with('-') && l[1..].trim().is_empty());
            if has_blank_deletion {
                return HunkOutcome::Skipped(
                    "含空白行删除,blank-tolerant 不安全 → 放行".to_owned(),
                );
            }
            let regions = find_regions_blank_tolerant(file_lines, &anchor_contents);
            match regions.len() {
                1 => {
                    let (s, e) = regions[0];
                    HunkOutcome::Repaired(rebuild_hunk_with_region(hunk, &file_lines[s..e]))
                }
                0 => HunkOutcome::Skipped("锚点在文件中 0 匹配(疑模型改错内容)".to_owned()),
                n => HunkOutcome::Skipped(format!("忽略空行下 {n} 处匹配(歧义)")),
            }
        }
    }
}

/// EP-1 辅助:在 `file_lines` 里找锚点**非空行**序列能唯一定位的区间(允许文件区间内含模型漏写的
/// 空行,但不允许有额外的非空行)。返回所有匹配区间 `[start, end)`(end 为最后一个匹配非空行的下一位)。
fn find_regions_blank_tolerant(
    file_lines: &[&str],
    anchor_contents: &[&str],
) -> Vec<(usize, usize)> {
    let nb: Vec<&str> = anchor_contents
        .iter()
        .map(|c| c.trim_end())
        .filter(|c| !c.trim().is_empty())
        .collect();
    if nb.is_empty() {
        return Vec::new();
    }
    let mut regions = Vec::new();
    for start in 0..file_lines.len() {
        if file_lines[start].trim().is_empty() || file_lines[start].trim_end() != nb[0] {
            continue;
        }
        let mut fi = start;
        let mut ai = 0;
        let mut ok = true;
        while ai < nb.len() {
            if fi >= file_lines.len() {
                ok = false;
                break;
            }
            let fl = file_lines[fi];
            if fl.trim().is_empty() {
                fi += 1; // 跳过文件空行(模型可能漏写)
                continue;
            }
            if fl.trim_end() == nb[ai] {
                ai += 1;
                fi += 1;
            } else {
                ok = false; // 出现额外非空行 → 此 start 不匹配
                break;
            }
        }
        if ok && ai == nb.len() {
            regions.push((start, fi));
        }
    }
    regions
}

/// EP-1 辅助:用文件真实区间 `region`(含空行)重建 hunk —— 锚点(context/`-`)对齐成文件字节、
/// 补回模型漏写的文件空行(作 context),`+` 插入行按 hunk 原序保持。模型自带的空白锚点行丢弃
/// (改用文件的空行,避免重复)。
fn rebuild_hunk_with_region(hunk: &[&str], region: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    let mut fi = 0usize; // region 游标
    for &hl in hunk {
        match hl.chars().next() {
            Some('+') => out.push(hl.to_owned()), // 插入行原样保位
            Some(' ') | Some('-') => {
                let prefix = hl.chars().next().unwrap();
                let content = &hl[1..];
                if content.trim().is_empty() {
                    continue; // 模型的空锚点行丢弃,用文件空行
                }
                // 先补回文件里模型漏写的空行(作 context)
                while fi < region.len() && region[fi].trim().is_empty() {
                    out.push(format!(" {}", region[fi]));
                    fi += 1;
                }
                if fi < region.len() {
                    out.push(format!("{prefix}{}", region[fi]));
                    fi += 1;
                } else {
                    out.push(hl.to_owned());
                }
            }
            _ => {} // 无前缀空行等丢弃,用文件空行
        }
    }
    out
}

/// 在 `file_lines` 里找所有起点 `i`,使 `file_lines[i..i+anchor.len()]` 与 `anchor` 逐行 `eq` 为真。
/// 返回所有匹配起点。
fn find_block<F: Fn(&str, &str) -> bool>(
    file_lines: &[&str],
    anchor: &[&str],
    eq: F,
) -> Vec<usize> {
    if anchor.is_empty() || anchor.len() > file_lines.len() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for i in 0..=(file_lines.len() - anchor.len()) {
        if (0..anchor.len()).all(|k| eq(file_lines[i + k], anchor[k])) {
            hits.push(i);
        }
    }
    hits
}

/// 把 patch 路径解析到绝对路径。绝对路径原样;相对路径对 `cwd` 拼接。
fn resolve_path(path: &str, cwd: &str) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        Path::new(cwd).join(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp_file(name: &str, content: &str) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        (dir, name.to_owned())
    }

    #[test]
    fn extract_cwd_from_env_block() {
        let req = json!({
            "input": [{"type":"message","role":"user","content":"<environment_context>\n  <cwd>/Users/x/proj</cwd>\n  <shell>zsh</shell>\n</environment_context>"}]
        });
        assert_eq!(extract_cwd(Some(&req)).as_deref(), Some("/Users/x/proj"));
        assert_eq!(extract_cwd(None), None);
        assert_eq!(extract_cwd(Some(&json!({"input":[]}))), None);

        // codex-connector #435 P2:Windows 路径反斜杠不能被翻倍(遍历 Value 取反转义原文,
        // 不能先序列化整个请求)。json! 里 "C:\\Users\\me\\repo" = 实际单反斜杠路径。
        let win = json!({
            "input": [{"type":"message","role":"user","content":"<environment_context>\n  <cwd>C:\\Users\\me\\repo</cwd>\n</environment_context>"}]
        });
        assert_eq!(
            extract_cwd(Some(&win)).as_deref(),
            Some(r"C:\Users\me\repo")
        );
    }

    #[test]
    fn trailing_whitespace_anchor_is_repaired_to_file_bytes() {
        // 文件 context 行无尾随空格;patch 的 context 行带尾随空格 → 应被对齐成文件真实字节。
        let (dir, name) = tmp_file("a.txt", "fn main() {\n    let x = 1;\n    let y = 2;\n}\n");
        let cwd = dir.path().to_str().unwrap();
        // patch: 在 `let x = 1;` 后加一行;context 带尾随空格(模型常见错)。
        let v4a = format!(
            "*** Begin Patch\n*** Update File: {name}\n    let x = 1;   \n+    let z = 9;\n    let y = 2;\n*** End Patch\n"
        );
        let (out, reps) = preflight_repair(&v4a, Some(cwd));
        assert!(
            out.contains("    let x = 1;\n"),
            "尾随空格应被对齐掉:\n{out}"
        );
        assert!(out.contains("+    let z = 9;"), "新增行保留");
        assert_eq!(reps[0].kind, "repaired", "{:?}", reps);
    }

    #[test]
    fn exact_match_left_clean() {
        let (dir, name) = tmp_file("b.txt", "alpha\nbeta\ngamma\n");
        let cwd = dir.path().to_str().unwrap();
        let v4a = format!("*** Begin Patch\n*** Update File: {name}\n alpha\n-beta\n+BETA\n gamma\n*** End Patch\n");
        let (out, reps) = preflight_repair(&v4a, Some(cwd));
        assert_eq!(reps[0].kind, "clean");
        assert_eq!(out, v4a, "精确匹配不改一字节");
    }

    #[test]
    fn ambiguous_match_is_skipped_not_guessed() {
        // 锚点 ` x` 在文件里多处出现 → 歧义 → 放行不猜。
        let (dir, name) = tmp_file("c.txt", "x\nx\nx\n");
        let cwd = dir.path().to_str().unwrap();
        let v4a =
            format!("*** Begin Patch\n*** Update File: {name}\n x   \n+added\n*** End Patch\n");
        let (out, reps) = preflight_repair(&v4a, Some(cwd));
        assert!(reps[0].kind.starts_with("skipped"), "{:?}", reps);
        assert_eq!(out, v4a, "歧义不改");
    }

    #[test]
    fn no_match_skipped() {
        let (dir, name) = tmp_file("d.txt", "real content\n");
        let cwd = dir.path().to_str().unwrap();
        let v4a = format!("*** Begin Patch\n*** Update File: {name}\n model hallucinated line\n+x\n*** End Patch\n");
        let (out, reps) = preflight_repair(&v4a, Some(cwd));
        assert!(reps[0].kind.starts_with("skipped"));
        assert_eq!(out, v4a);
    }

    #[test]
    fn unreadable_file_passes_through() {
        let v4a = "*** Begin Patch\n*** Update File: nonexistent_zzz.txt\n a\n+b\n*** End Patch\n";
        let (out, reps) = preflight_repair(v4a, Some("/tmp/no_such_dir_xyz"));
        assert_eq!(out, v4a);
        assert_eq!(reps[0].kind, "skipped:unreadable");
    }

    #[test]
    fn envelope_added_when_model_omits_begin_end() {
        // 真机 seq230 形态:只有 Add File + 内容,无 Begin/End。
        let body = "*** Add File: outputs/x.md\n+# Title\n+body\n";
        let (out, rep) = ensure_v4a_envelope(body);
        assert!(out.starts_with("*** Begin Patch\n"), "{out}");
        assert!(out.trim_end().ends_with("*** End Patch"), "{out}");
        assert!(
            out.contains("+# Title") && out.contains("+body"),
            "内容不丢"
        );
        assert!(rep.is_some());
    }

    #[test]
    fn envelope_only_end_added() {
        let body = "*** Begin Patch\n*** Add File: x\n+a\n";
        let (out, rep) = ensure_v4a_envelope(body);
        assert_eq!(out.matches("*** Begin Patch").count(), 1, "不重复加 Begin");
        assert!(out.trim_end().ends_with("*** End Patch"));
        assert!(rep.unwrap().detail.contains("End Patch"));
    }

    #[test]
    fn envelope_prefixed_end_stripped_for_code_file() {
        // MOC-268(用户拍板「按文件类型剥」):**仅 `+*** End Patch`**(Add 行误前缀)在代码/结构化配置文件
        // 里(裸 `*** End Patch` 不可能是合法源码末行)剥前缀规整成裸终止符,不追加、零残留、零内容丢失。
        for path in ["x.rs", "c.json", "s.toml", "w.vue"] {
            let body = format!("*** Begin Patch\n*** Add File: {path}\n+a\n+b\n+*** End Patch");
            let (out, rep) = ensure_v4a_envelope(&body);
            assert!(
                out.trim_end().ends_with("\n*** End Patch"),
                "代码文件应剥成裸终止符 ({path}):\n{out}"
            );
            assert!(
                !out.contains("+*** End Patch"),
                "不应残留带前缀终止符 ({path}):\n{out}"
            );
            assert_eq!(
                out.matches("*** End Patch").count(),
                1,
                "只一个终止符 ({path}):\n{out}"
            );
            assert!(rep.unwrap().detail.contains("剥误加前缀"), "{path}");
        }
    }

    #[test]
    fn envelope_deletion_or_context_end_line_not_stripped() {
        // MOC-268(chatgpt-codex-connector review):` *** End Patch`(context)/ `-*** End Patch`(deletion)
        // 是**合法 Update hunk 行**(如模型用 Update 删文件里残留的 *** End Patch),**绝不能当终止符剥**
        // (剥 `-` = 静默丢弃删除)。这俩末行 → 正常补真终止符、hunk 行**原样保留**。即便目标是代码文件。
        for last in ["-*** End Patch", " *** End Patch"] {
            let body = format!("*** Begin Patch\n*** Update File: src/foo.rs\n keep\n{last}");
            let (out, rep) = ensure_v4a_envelope(&body);
            assert!(
                out.contains(last),
                "合法 hunk 行 {last:?} 必须原样保留(不剥=不丢删除):\n{out}"
            );
            assert!(
                out.trim_end().ends_with("\n*** End Patch"),
                "应正常补真终止符 ({last:?}):\n{out}"
            );
            let r = rep.unwrap();
            assert!(
                r.detail.contains("End Patch") && !r.detail.contains("剥误加前缀"),
                "走正常 append、非剥 ({last:?}):{}",
                r.detail
            );
        }
    }

    #[test]
    fn envelope_prefixed_end_left_incomplete_for_doc_file() {
        // MOC-268(silent-failure review):文档/文本/未知类型里裸 `*** End Patch` **可能是正文末行**(本仓
        // V4A 文档就有这串字)→ 歧义不猜:既不剥(免删正文)也不追加(免残留)→ 不补全留 incomplete。
        for path in ["notes.md", "readme.txt", "data"] {
            let body = format!(
                "*** Begin Patch\n*** Add File: {path}\n+How to end a patch:\n+*** End Patch"
            );
            let (out, rep) = ensure_v4a_envelope(&body);
            assert_eq!(out, body, "文档文件歧义末行应原样保留 ({path}):\n{out}");
            assert!(
                out.trim_end().ends_with("+*** End Patch"),
                "正文行应保留不删 ({path}):\n{out}"
            );
            assert!(
                !out.trim_end().ends_with("\n*** End Patch"),
                "不应追加裸终止符 ({path}):\n{out}"
            );
            assert_eq!(
                rep.unwrap().kind,
                "skipped:ambiguous_prefixed_end",
                "{path}"
            );
        }
    }

    #[test]
    fn envelope_prefixed_end_not_stripped_for_update_even_code() {
        // MOC-268(chatgpt-codex-connector review):`*** Update File:` 的 `+*** End Patch` 是**新增行**
        // (可能往代码文件的字符串/fixture 里加这串字),不是 Add File 的误前缀终止符 → **即便目标是代码
        // 文件也不剥**(剥了=丢新增),走歧义 → 留 incomplete。剥仅限末操作是 Add File。
        let body =
            "*** Begin Patch\n*** Update File: src/foo.rs\n keep\n+*** End Patch".to_string();
        let (out, rep) = ensure_v4a_envelope(&body);
        assert_eq!(out, body, "Update 的 +*** End Patch 不应被剥/动:\n{out}");
        assert!(
            out.trim_end().ends_with("+*** End Patch"),
            "新增行应保留:\n{out}"
        );
        assert!(
            !out.trim_end().ends_with("\n*** End Patch"),
            "不应追加裸终止符:\n{out}"
        );
        assert_eq!(rep.unwrap().kind, "skipped:ambiguous_prefixed_end");
    }

    #[test]
    fn envelope_complete_untouched() {
        let body = "*** Begin Patch\n*** Add File: x\n+a\n*** End Patch\n";
        let (out, rep) = ensure_v4a_envelope(body);
        assert_eq!(out, body);
        assert!(rep.is_none());
    }

    #[test]
    fn envelope_not_added_to_nonpatch_or_leading_prose() {
        // 非 patch 体不碰
        let (o1, r1) = ensure_v4a_envelope("just some text\nno ops here\n");
        assert_eq!(o1, "just some text\nno ops here\n");
        assert!(r1.is_none());
        // 缺 Begin 且首个非空行不是操作行(有前导散文)→ 不安全,不补 Begin
        let prose = "here is my patch:\n*** Add File: x\n+a\n*** End Patch\n";
        let (o2, _r2) = ensure_v4a_envelope(prose);
        assert!(
            !o2.starts_with("*** Begin Patch"),
            "有前导散文不应贸然补 Begin"
        );
    }

    #[test]
    fn strip_trailing_at_double_sided_to_single() {
        let v4a = "*** Begin Patch\n*** Update File: x\n@@ def f(): @@\n-a\n+b\n*** End Patch\n";
        let (out, reps) = strip_trailing_at(v4a);
        assert!(out.contains("@@ def f():\n"), "应去尾部 @@:\n{out}");
        assert!(!out.contains("@@ def f(): @@"));
        assert_eq!(reps.len(), 1);
    }

    #[test]
    fn strip_trailing_at_keeps_bare_and_single() {
        // 裸 @@(section 分隔)+ 单边 @@ 都不动
        let v4a = "*** Update File: x\n@@\n@@ class Foo\n-a\n+b\n";
        let (out, reps) = strip_trailing_at(v4a);
        assert_eq!(out, v4a);
        assert!(reps.is_empty());
    }

    #[test]
    fn recover_empty_move_to_delete_add() {
        let (dir, name) = tmp_file("old.md", "line1\nline2\n");
        let cwd = dir.path().to_str().unwrap();
        let v4a = format!(
            "*** Begin Patch\n*** Update File: {name}\n*** Move to: new.md\n*** End Patch\n"
        );
        let (out, reps) = recover_empty_move(&v4a, Some(cwd));
        assert!(out.contains(&format!("*** Delete File: {name}")), "{out}");
        assert!(out.contains("*** Add File: new.md"), "{out}");
        assert!(
            out.contains("+line1") && out.contains("+line2"),
            "复制原内容:\n{out}"
        );
        assert!(!out.contains("*** Move to:"), "Move 已被替换");
        assert_eq!(reps[0].kind, "repaired");
    }

    #[test]
    fn recover_empty_move_with_hunk_untouched() {
        // Update+Move 但**有** hunk(rename + 内容改)→ 不碰(prompt 允许)。
        let (dir, name) = tmp_file("old2.md", "a\n");
        let cwd = dir.path().to_str().unwrap();
        let v4a = format!(
            "*** Begin Patch\n*** Update File: {name}\n*** Move to: new2.md\n-a\n+b\n*** End Patch\n"
        );
        let (out, reps) = recover_empty_move(&v4a, Some(cwd));
        assert_eq!(out, v4a, "有 hunk 的 Move 不动");
        assert!(reps.is_empty());
    }

    #[test]
    fn rename_with_eof_marker_hunk_not_treated_as_empty() {
        // codex-connector #435 P1:rename + `*** End of File` 追加 hunk 不能被当空 rename(否则转成
        // 丢内容的 Delete+Add)→ 识别为有 hunk → 透过不转。
        let (dir, name) = tmp_file("eof_old.md", "a\n");
        let cwd = dir.path().to_str().unwrap();
        let v4a = format!(
            "*** Begin Patch\n*** Update File: {name}\n*** Move to: eof_new.md\n*** End of File\n+tail\n*** End Patch\n"
        );
        let (out, reps) = recover_empty_move(&v4a, Some(cwd));
        assert_eq!(out, v4a, "含 EOF hunk 的 rename 应透过不转:\n{out}");
        assert!(reps.is_empty(), "{:?}", reps);
    }

    #[test]
    fn add_on_existing_passes_through_unchanged() {
        // 规则 #2 已撤:Add 已存在文件**不再**转 Delete+Add(避免覆盖丢数据),原样透过让 Codex
        // 报 already exists、模型自纠为针对性 Update。
        let (dir, name) = tmp_file("exists.md", "important old content\n");
        let cwd = dir.path().to_str().unwrap();
        let v4a = format!("*** Begin Patch\n*** Add File: {name}\n+new content\n*** End Patch\n");
        let (out, reps) = optimize_patch(&v4a, Some(cwd), true);
        assert!(
            !out.contains("*** Delete File:"),
            "不应再插 Delete(已撤规则#2):\n{out}"
        );
        assert!(
            out.contains(&format!("*** Add File: {name}")),
            "Add 原样保留"
        );
        assert!(
            !reps.iter().any(|r| r.detail.contains("Delete File 覆盖")),
            "不应有覆盖类修复: {:?}",
            reps
        );
    }

    #[test]
    fn update_empty_file_to_delete_add() {
        let (dir, name) = tmp_file("empty.txt", "");
        let cwd = dir.path().to_str().unwrap();
        let v4a =
            format!("*** Begin Patch\n*** Update File: {name}\n+line1\n+line2\n*** End Patch\n");
        let (out, reps) = recover_update_empty_file(&v4a, Some(cwd));
        assert!(out.contains(&format!("*** Delete File: {name}")), "{out}");
        assert!(out.contains(&format!("*** Add File: {name}")), "{out}");
        assert!(out.contains("+line1") && out.contains("+line2"));
        assert!(!out.contains("*** Update File:"), "Update 已转换");
        assert_eq!(reps[0].kind, "repaired");
    }

    #[test]
    fn update_whitespace_only_file_not_converted() {
        // codex-connector #435 P2:纯空白文件(非 0 字节)不算空 → 不转 Delete+Add(否则丢空白字节)。
        let (dir, name) = tmp_file("ws.txt", "  \n\t\n");
        let cwd = dir.path().to_str().unwrap();
        let v4a = format!("*** Begin Patch\n*** Update File: {name}\n+line1\n*** End Patch\n");
        let (out, reps) = recover_update_empty_file(&v4a, Some(cwd));
        assert_eq!(out, v4a, "纯空白文件 Update 不应转 Delete+Add:\n{out}");
        assert!(reps.is_empty());
    }

    #[test]
    fn update_nonempty_file_not_converted() {
        let (dir, name) = tmp_file("nonempty.txt", "existing\n");
        let cwd = dir.path().to_str().unwrap();
        let v4a = format!(
            "*** Begin Patch\n*** Update File: {name}\n-existing\n+changed\n*** End Patch\n"
        );
        let (out, reps) = recover_update_empty_file(&v4a, Some(cwd));
        assert_eq!(out, v4a, "非空文件 Update 不碰");
        assert!(reps.is_empty());
    }

    #[test]
    fn add_file_missing_plus_prefix_is_filled() {
        // Add File 里有的行漏 `+`(模型常见)、有空行 → 全补 `+`,已有 `+` 的不动。
        let v4a = "*** Begin Patch\n*** Add File: new.md\n+# Title\nplain line no plus\n\n+already plus\n*** End Patch\n";
        let (out, reps) = ensure_add_file_plus(v4a);
        assert!(
            out.contains("\n+plain line no plus\n"),
            "漏 + 的行应补:\n{out}"
        );
        assert!(out.contains("\n+\n+already plus"), "空行 → 裸 +:\n{out}");
        assert!(!out.contains("++already plus"), "已是 + 的不重复");
        assert_eq!(reps[0].kind, "repaired");
        assert!(
            reps[0].detail.contains("2 行"),
            "漏 + 的 plain 行 + 空行 = 2: {:?}",
            reps
        );
    }

    #[test]
    fn add_file_all_plus_untouched_and_update_not_affected() {
        // 全 + 的 Add File 不动;Update section 的非 + 行(context/-)绝不被 G 碰。
        let v4a = "*** Begin Patch\n*** Add File: a\n+x\n+y\n*** Update File: b\n cont\n-old\n+new\n*** End Patch\n";
        let (out, reps) = ensure_add_file_plus(v4a);
        assert_eq!(out, v4a, "Add 全 + + Update 不动:\n{out}");
        assert!(reps.is_empty());
    }

    #[test]
    fn at_header_aligned_to_unique_file_line() {
        // 真机 seq181:@@ 头残缺(漏 `## 6. `),唯一包含于一个文件行 → 对齐。
        let (dir, name) = tmp_file("doc.md", "intro\n## 6. 系统架构建议\n建议分层\n");
        let cwd = dir.path().to_str().unwrap();
        let v4a = format!(
            "*** Begin Patch\n*** Update File: {name}\n@@ 系统架构建议\n 建议分层\n+新增一行\n*** End Patch\n"
        );
        let (out, reps) = align_at_headers(&v4a, Some(cwd));
        assert!(
            out.contains("@@ ## 6. 系统架构建议"),
            "@@ 应对齐成文件真实整行:\n{out}"
        );
        assert_eq!(reps[0].kind, "repaired");
    }

    #[test]
    fn at_header_exact_or_ambiguous_untouched() {
        // 已是文件真实整行 → 不动;多处包含(歧义)→ 不动。
        let (dir, name) = tmp_file("doc2.md", "## A\nx\n## A\n");
        let cwd = dir.path().to_str().unwrap();
        // 精确整行 `## A` 存在,但歧义(两行)→ 不动
        let v4a = format!("*** Update File: {name}\n@@ ## A\n x\n+y\n");
        let (out, reps) = align_at_headers(&v4a, Some(cwd));
        assert_eq!(out, v4a);
        assert!(reps.is_empty());
        // 子串 `A` 在 `## A` 两行里出现 → 歧义不动
        let v4a2 = format!("*** Update File: {name}\n@@ A\n x\n+y\n");
        let (out2, reps2) = align_at_headers(&v4a2, Some(cwd));
        assert_eq!(out2, v4a2);
        assert!(reps2.is_empty());
    }

    #[test]
    fn unprefixed_dup_of_plus_line_dropped() {
        // 真机 seq235:无前缀行 + 紧跟 `+<同内容>` → 删废行(内容在 + 行,不丢)。
        let (dir, name) = tmp_file("u.md", "other\n");
        let cwd = dir.path().to_str().unwrap();
        let v4a = format!("*** Update File: {name}\n*data source*\n+*data source*\n+more\n");
        let (out, reps) = fix_unprefixed_lines(&v4a, Some(cwd));
        assert!(!out.contains("\n*data source*\n"), "无前缀废行应删:\n{out}");
        assert!(out.contains("+*data source*"), "+ 行保留(内容不丢)");
        assert_eq!(reps[0].kind, "repaired");
    }

    #[test]
    fn unprefixed_existing_file_line_gets_context_space() {
        // 无前缀行在文件里有同行 → context 漏空格 → 补 ` `。
        let (dir, name) = tmp_file("u2.md", "alpha\nkeepme\nbeta\n");
        let cwd = dir.path().to_str().unwrap();
        let v4a = format!("*** Update File: {name}\nkeepme\n+added\n");
        let (out, reps) = fix_unprefixed_lines(&v4a, Some(cwd));
        assert!(out.contains("\n keepme\n"), "应补空格成 context:\n{out}");
        assert_eq!(reps[0].kind, "repaired");
    }

    #[test]
    fn unprefixed_unknown_passes_through() {
        // 不在文件、非重复 → 透过(不猜)。
        let (dir, name) = tmp_file("u3.md", "real\n");
        let cwd = dir.path().to_str().unwrap();
        let v4a = format!("*** Update File: {name}\nhallucinated garbage line\n+x\n");
        let (out, reps) = fix_unprefixed_lines(&v4a, Some(cwd));
        assert_eq!(out, v4a, "未知无前缀行原样透过");
        assert!(reps.is_empty());
    }

    #[test]
    fn cwd_candidates_remember_recall_and_resolve() {
        // MOC-263 P1:候选历史(deque)+ read_patch_file 按候选解析(全局态,只做非 flaky 断言)。
        let (dir, name) = tmp_file("cand_moc263.txt", "x\n");
        let real = dir.path().to_str().unwrap().to_owned();
        // 模拟并发污染:先记一个不含该文件的 stale cwd,再记真实 cwd(置顶)。
        remember_cwd("/tmp/stale_zzz_moc263_a");
        remember_cwd(&real);
        assert!(recall_cwd_candidates().iter().any(|c| c == &real));
        assert!(has_cwd_candidate(None), "有候选历史 → true");
        // primary=None(apply_patch 工具循环请求),真实 cwd 在候选里 → 读到文件
        // (stale cwd 无此文件,逐个试时自动跳过)。这是 P1 的核心:不再被 stale 单槽废掉。
        let got = read_patch_file(&name, None, &[(false, "x")]);
        assert!(got.is_some(), "应经候选 cwd 读到文件");
        assert_eq!(got.unwrap().1, "x\n");
        // turn-start 请求抽 cwd 入候选(供后续 apply_patch 回退)。
        let req = json!({"input":[{"type":"message","role":"user","content":"<environment_context>\n  <cwd>/tmp/ts_proj_b3f9</cwd>\n</environment_context>"}]});
        remember_cwd_from_request(Some(&req));
        assert!(recall_cwd_candidates()
            .iter()
            .any(|c| c == "/tmp/ts_proj_b3f9"));
    }

    #[test]
    fn read_patch_file_picks_by_probe_not_first_readable() {
        // MOC-263 P2(chatgpt-codex-connector review):并发会话共享相对路径(README.md 等)、且 stale
        // 会话更新时,按 patch 锚点 probe 选**真正含锚点的候选**,而非取队首(most-recent=stale)可读的。
        let stale = tempfile::tempdir().unwrap();
        let real = tempfile::tempdir().unwrap();
        std::fs::write(stale.path().join("shared_moc263.txt"), "stale_only_line\n").unwrap();
        std::fs::write(
            real.path().join("shared_moc263.txt"),
            "real_anchor_line\nmore\n",
        )
        .unwrap();
        // 真实 cwd 先记、stale 后记 → stale 在候选队首(most-recent),模拟 review 担心的场景。
        remember_cwd(real.path().to_str().unwrap());
        remember_cwd(stale.path().to_str().unwrap());
        // probe 命中 real(含 real_anchor_line)、不命中 stale → 应选 real,不取队首 stale。
        let got = read_patch_file("shared_moc263.txt", None, &[(false, "real_anchor_line")]);
        assert!(got.is_some(), "应选到含锚点的候选");
        assert_eq!(
            got.unwrap().1,
            "real_anchor_line\nmore\n",
            "应选 real(含 probe 锚点)而非队首 stale"
        );
    }

    #[test]
    fn read_patch_file_single_candidate_partial_header_not_skipped() {
        // MOC-263 P2 二轮(chatgpt-codex-connector review):probe 只含残缺 `@@` header(文件里无 exact
        // 行,真实是 `## 6. 系统架构建议`),**单一候选**不应因 probe 0 命中被判 unreadable —— 仍返回
        // 文件,让 align_at_headers 做子串修复。probe 是 tie-breaker 不是 gate。
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("doc_moc263.md"),
            "intro\n## 6. 系统架构建议\n建议分层\n",
        )
        .unwrap();
        remember_cwd(dir.path().to_str().unwrap());
        let got = read_patch_file("doc_moc263.md", None, &[(true, "系统架构建议")]);
        assert!(
            got.is_some(),
            "单候选 + 残缺 header probe 不应被判 unreadable"
        );
        assert!(got.unwrap().1.contains("## 6. 系统架构建议"));
    }

    #[test]
    fn read_patch_file_partial_header_substring_picks_real_over_stale() {
        // MOC-263 P2 三轮(chatgpt-codex-connector review):多候选 + 纯残缺 `@@` 头,exact 全 0 → 退
        // 子串评分,选**子串含该头的 real**,而非盲取队首 stale(否则 align 会对 stale 子串修复)。
        let stale = tempfile::tempdir().unwrap();
        let real = tempfile::tempdir().unwrap();
        std::fs::write(
            stale.path().join("doc2_moc263.md"),
            "stale intro\nunrelated heading\n",
        )
        .unwrap();
        std::fs::write(
            real.path().join("doc2_moc263.md"),
            "intro\n## 6. 系统架构建议\n建议分层\n",
        )
        .unwrap();
        remember_cwd(real.path().to_str().unwrap());
        remember_cwd(stale.path().to_str().unwrap()); // stale 在队首(most-recent)
        let got = read_patch_file("doc2_moc263.md", None, &[(true, "系统架构建议")]);
        assert!(got.is_some());
        assert!(
            got.unwrap().1.contains("## 6. 系统架构建议"),
            "子串评分应选含该头的 real,而非队首 stale"
        );
    }

    #[test]
    fn read_patch_file_tied_score_is_ambiguous_none() {
        // MOC-263 P2 四轮(chatgpt-codex-connector review):两候选 probe 分数并列(共享相同锚点行)→
        // 歧义 → None(不猜),而非取队首 stale。下游 skip,patch 透过交 Codex / 模型自纠。
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        std::fs::write(a.path().join("tie_moc263.txt"), "SHARED_ANCHOR\naaa\n").unwrap();
        std::fs::write(b.path().join("tie_moc263.txt"), "SHARED_ANCHOR\nbbb\n").unwrap();
        remember_cwd(a.path().to_str().unwrap());
        remember_cwd(b.path().to_str().unwrap());
        let got = read_patch_file("tie_moc263.txt", None, &[(false, "SHARED_ANCHOR")]);
        assert!(got.is_none(), "并列分数(歧义)应返回 None 不猜");
    }

    #[test]
    fn read_patch_file_header_probe_not_beaten_by_stale_exact_fragment() {
        // MOC-263 P2 五轮(chatgpt-codex-connector review):probe 只有残缺 `@@` 头;stale 恰有一整行 =
        // 该 fragment,real 是其子串(`## 6. X`)。header 按**子串**评分(不进 exact)→ 两候选都子串命中
        // → 并列 → None,**不会**因 stale 的 exact 整行被误选。
        let stale = tempfile::tempdir().unwrap();
        let real = tempfile::tempdir().unwrap();
        std::fs::write(stale.path().join("h_moc263.md"), "系统架构建议\nx\n").unwrap();
        std::fs::write(real.path().join("h_moc263.md"), "## 6. 系统架构建议\ny\n").unwrap();
        remember_cwd(real.path().to_str().unwrap());
        remember_cwd(stale.path().to_str().unwrap()); // stale 在队首
        let got = read_patch_file("h_moc263.md", None, &[(true, "系统架构建议")]);
        assert!(
            got.is_none(),
            "header 子串并列应 None,不被 stale 的 exact 整行误选"
        );
    }

    #[test]
    fn context_line_starting_with_at_at_does_not_block_split() {
        // MOC-263 P2 五轮(chatgpt-codex-connector review):context 行内容以 @@ 开头(` @@ ...`,列 0 是
        // 空格)不应被当 hunk 分隔符而禁用自动切分(分割器只认列 0 @@)。含此类 context 行的多区纯删除仍应切。
        let content = "@@ banner\nkeep_a\nREMOVE_1\nmid1\nmid2\nmid3\nREMOVE_2\nkeep_b\n";
        let (dir, name) = tmp_file("atat_moc263.txt", content);
        let cwd = dir.path().to_str().unwrap();
        let v4a = format!(
            "*** Begin Patch\n*** Update File: {name}\n @@ banner\n keep_a\n-REMOVE_1\n mid1\n-REMOVE_2\n keep_b\n*** End Patch\n"
        );
        let (out, _reps) = preflight_repair(&v4a, Some(cwd));
        assert!(
            out.contains("\n@@\n"),
            "含 ` @@` context 行的多区删除仍应自动切段(列 0 判定):\n{out}"
        );
    }

    #[test]
    fn anchor_probe_includes_unprefixed_lines() {
        // MOC-263 P2 六轮(chatgpt-codex-connector review):无前缀行(fix_unprefixed_lines 要按文件整行
        // 匹配修)也要进 probe,否则它是唯一锚点时空-probe 路径会在同名候选间选错文件。`+` / `***` 不进。
        let body = vec![
            " ctx",
            "-del",
            "+add",
            "@@ hdr",
            "unprefixed line",
            "*** End Patch",
        ];
        let p = anchor_probe(&body);
        assert!(p.contains(&(false, "ctx")), "context 行进 probe");
        assert!(p.contains(&(false, "del")), "删除行进 probe");
        assert!(p.contains(&(true, "hdr")), "@@ 头进 probe(header)");
        assert!(
            p.contains(&(false, "unprefixed line")),
            "无前缀行应作 exact probe"
        );
        assert!(!p.iter().any(|(_, t)| *t == "add"), "+ 新增行不进 probe");
        assert!(
            !p.iter().any(|(_, t)| t.starts_with("***")),
            "*** 控制行不进 probe"
        );
    }

    #[test]
    fn preflight_aligns_via_candidate_cwd_with_none_primary() {
        // MOC-263 P1 端到端:apply_patch 请求 cwd=None,真实 cwd 仅在候选历史里 → 仍能读盘对齐。
        let (dir, name) = tmp_file("p1_e2e_moc263.txt", "alpha\nbeta\ngamma\n");
        let real = dir.path().to_str().unwrap().to_owned();
        remember_cwd(&real);
        // context 带尾随空格(需读盘对齐);primary=None,靠候选历史找到真实文件。
        let v4a = format!(
            "*** Begin Patch\n*** Update File: {name}\n beta   \n+inserted\n*** End Patch\n"
        );
        let (out, reps) = preflight_repair(&v4a, None);
        assert!(
            out.contains("\n beta\n"),
            "应经候选 cwd 读盘对齐尾随空格:\n{out}"
        );
        assert!(out.contains("+inserted"), "新增行保留");
        assert_eq!(reps[0].kind, "repaired", "{:?}", reps);
    }

    #[test]
    fn multi_hunk_pure_delete_no_at_auto_split() {
        // MOC-263 P0:多个不连续**纯删除/上下文**区拼一个 Update File 块、无 @@ → 安全自动切段插裸 @@。
        // (带插入 `+` 的多区因落点歧义不在此切,见 mixed_replace_insert_gap_passthrough)
        let content = "keep_top\nREMOVE_1\nmiddle\nmiddle2\nmiddle3\nREMOVE_2\nkeep_bottom\n";
        let (dir, name) = tmp_file("multi_del.txt", content);
        let cwd = dir.path().to_str().unwrap();
        let v4a = format!(
            "*** Begin Patch\n*** Update File: {name}\n keep_top\n-REMOVE_1\n middle\n-REMOVE_2\n keep_bottom\n*** End Patch\n"
        );
        let (out, reps) = preflight_repair(&v4a, Some(cwd));
        assert!(out.contains("\n@@\n"), "两个不连续删除区应插裸 @@:\n{out}");
        assert_eq!(reps[0].kind, "repaired", "{:?}", reps);
        assert!(reps[0].detail.contains("自动按文件位置切段"), "{:?}", reps);
        assert!(
            out.contains("-REMOVE_1") && out.contains("-REMOVE_2"),
            "删除行保留:\n{out}"
        );
    }

    #[test]
    fn mixed_replace_insert_gap_passthrough() {
        // MOC-263 P0 安全(chatgpt-codex-connector review 指出):段间「替换 + 额外插入」混合 →
        // `+` 落点歧义(`+return 42` 是替换、`+@memoize` 是给后段 `def beta` 的引入行,无法区分)→
        // 不切、原样透过,避免把 @memoize 静默插到 return 后(错位)。即便前段末锚点是 `-` 删除也不豁免。
        let content = "alpha\nreturn 1\n# gap\ndef beta():\n";
        let (dir, name) = tmp_file("mixed.py", content);
        let cwd = dir.path().to_str().unwrap();
        let v4a = format!(
            "*** Begin Patch\n*** Update File: {name}\n-return 1\n+return 42\n+@memoize\n def beta():\n*** End Patch\n"
        );
        let (out, _reps) = preflight_repair(&v4a, Some(cwd));
        assert!(
            !out.contains("\n@@\n"),
            "混合 replace+insert 落点歧义不应切:\n{out}"
        );
        assert!(
            out.contains("+@memoize") && out.contains("+return 42"),
            "内容不丢"
        );
    }

    #[test]
    fn single_contiguous_hunk_not_split() {
        // 单段连续 hunk → 不切(group<2 → None),走常规对齐。
        let (dir, name) = tmp_file("single.txt", "a\nb\nc\nd\n");
        let cwd = dir.path().to_str().unwrap();
        let v4a =
            format!("*** Begin Patch\n*** Update File: {name}\n a\n b\n+x\n c\n*** End Patch\n");
        let (out, reps) = preflight_repair(&v4a, Some(cwd));
        assert!(!out.contains("\n@@\n"), "单段连续 hunk 不应插 @@:\n{out}");
        assert!(!reps[0].detail.contains("切段"), "{:?}", reps);
    }

    #[test]
    fn ambiguous_multi_region_passthrough() {
        // 锚点内容在文件里重复(歧义)→ longest_unique_block 返回 None → 不切,透过(不猜不丢)。
        let (dir, name) = tmp_file("amb.txt", "x\ny\nx\ny\n");
        let cwd = dir.path().to_str().unwrap();
        let v4a =
            format!("*** Begin Patch\n*** Update File: {name}\n-x\n+X\n-y\n+Y\n*** End Patch\n");
        let (out, _reps) = preflight_repair(&v4a, Some(cwd));
        assert!(!out.contains("\n@@\n"), "歧义不应切:\n{out}");
    }

    #[test]
    fn greedy_split_bails_when_first_anchor_not_globally_unique() {
        // MOC-263 P1(chatgpt-codex-connector review):**块唯一但段首非唯一**的隐蔽歧义 —— file 有旧
        // ALPHA/BETA/GAMMA 块 + 真实 ALPHA/BETA…gap…GAMMA/DELTA 区。body ` ALPHA/-BETA/ GAMMA/-DELTA`
        // 的 [ALPHA,BETA,GAMMA] 作为连续块只在旧块唯一出现,贪心会选中旧块、从**错块**删 BETA;而段首
        // ALPHA 在文件里出现 2 次 = 起点歧义。修复后段首非全局唯一即 bail,不切分、原样透过(不猜不丢)。
        let content = "ALPHA\nBETA\nGAMMA\nmid_x\nmid_y\nALPHA\nBETA\nsep_gap\nGAMMA\nDELTA\n";
        let (dir, name) = tmp_file("greedy_moc263.txt", content);
        let cwd = dir.path().to_str().unwrap();
        let v4a = format!(
            "*** Begin Patch\n*** Update File: {name}\n ALPHA\n-BETA\n GAMMA\n-DELTA\n*** End Patch\n"
        );
        let (out, _reps) = preflight_repair(&v4a, Some(cwd));
        assert!(
            !out.contains("\n@@\n"),
            "段首锚点非全局唯一(起点歧义)时应 bail 不切分,避免从错块删行:\n{out}"
        );
    }

    #[test]
    fn floating_add_after_context_passthrough_not_misplaced() {
        // MOC-263 P0 安全防护:`+` 浮动在两不连续区域之间、前段末锚点是 context(非 `-` 删除)→ 落点
        // 歧义(可能属前段尾插、也可能是后段引入行)→ 不切(否则会把 +@memoize 插到错位置、静默错误
        // apply)。这是 pre-push review 抓到的 BLOCKER 回归点。
        let content =
            "def alpha():\n    return 1\n# --- section break ---\ndef beta():\n    return 2\n";
        let (dir, name) = tmp_file("deco.py", content);
        let cwd = dir.path().to_str().unwrap();
        // 模型以为 `return 1` 与 `def beta():` 相邻、在中间加 +@memoize;实际隔着 section break。
        let v4a = format!(
            "*** Begin Patch\n*** Update File: {name}\n     return 1\n+@memoize\n def beta():\n*** End Patch\n"
        );
        let (out, _reps) = preflight_repair(&v4a, Some(cwd));
        assert!(
            !out.contains("\n@@\n"),
            "浮动 + 落点歧义不应切段(防静默错误 apply):\n{out}"
        );
        assert!(out.contains("+@memoize"), "内容不丢");
    }

    #[test]
    fn blank_line_drift_block_realigned() {
        // EP-1 真机 seq111:模型 context 块漏了文件里的空行 → 整块失配。忽略空行唯一定位 → 重建
        // (补回文件空行 + 对齐字节),`+` 插入保位。
        let (dir, name) = tmp_file(
            "main.py",
            "from a import x\nfrom b import y\n\nfrom c import z\nfrom d import w\n",
        );
        let cwd = dir.path().to_str().unwrap();
        // patch 的 context 漏了 `from b` 与 `from c` 之间的空行,想在 `from d` 后插一行。
        let v4a = format!(
            "*** Begin Patch\n*** Update File: {name}\n from a import x\n from b import y\n from c import z\n from d import w\n+from e import v\n*** End Patch\n"
        );
        let (out, reps) = preflight_repair(&v4a, Some(cwd));
        assert!(out.contains("+from e import v"), "插入行保留:\n{out}");
        // 重建后 context 块应含被补回的空行(裸 ' ')。
        assert!(out.contains("\n \n"), "应补回文件空行作 context:\n{out}");
        assert_eq!(reps[0].kind, "repaired", "{:?}", reps);
    }

    #[test]
    fn blank_tolerant_skips_blank_line_deletion() {
        // 含「删除一个空行」的 `-` → blank-tolerant 重建无法忠实表达 → 放行不改(不静默转 context)。
        let (dir, name) = tmp_file("bd.txt", "x\ny\n");
        let cwd = dir.path().to_str().unwrap();
        let v4a = format!("*** Update File: {name}\n x\n-\n y\n+z\n");
        let (out, reps) = preflight_repair(&v4a, Some(cwd));
        assert_eq!(out, v4a, "含空白行删除应放行不改:\n{out}");
        assert!(reps[0].kind.starts_with("skipped"), "{:?}", reps);
    }

    #[test]
    fn blank_tolerant_ambiguous_passthrough() {
        // 精确失配(文件 p/q 间有空行,patch 没写)但忽略空行后**多处**匹配 → 歧义放行不猜。
        let (dir, name) = tmp_file("dup.txt", "p\n\nq\nX\np\n\nq\n");
        let cwd = dir.path().to_str().unwrap();
        let v4a = format!("*** Update File: {name}\n p\n q\n+r\n");
        let (out, reps) = preflight_repair(&v4a, Some(cwd));
        assert_eq!(out, v4a, "歧义(忽略空行后多处)不改:\n{out}");
        assert!(reps[0].kind.starts_with("skipped"), "{:?}", reps);
    }

    #[test]
    fn optimize_pipeline_fixes_multiple_issues() {
        // 一个 patch 同时:漏信封 + 双边 @@ + 尾随空格上下文 → 全恢复。
        let (dir, name) = tmp_file("multi.txt", "fn main() {\n    let x = 1;\n}\n");
        let cwd = dir.path().to_str().unwrap();
        let v4a = format!(
            "*** Update File: {name}\n@@ fn main() {{ @@\n    let x = 1;   \n+    let y = 2;\n"
        );
        let (out, reps) = optimize_patch(&v4a, Some(cwd), true);
        assert!(out.starts_with("*** Begin Patch\n"), "补信封:\n{out}");
        assert!(out.trim_end().ends_with("*** End Patch"), "补 End:\n{out}");
        assert!(out.contains("@@ fn main() {\n"), "双边 @@ 转单边:\n{out}");
        assert!(out.contains("    let x = 1;\n"), "尾随空格对齐:\n{out}");
        assert!(out.contains("+    let y = 2;"), "新增行保留");
        // 至少 3 类修复都记录
        let kinds: Vec<&str> = reps.iter().map(|r| r.kind.as_str()).collect();
        assert!(
            kinds.iter().filter(|k| **k == "repaired").count() >= 2,
            "{:?}",
            reps
        );
    }

    #[test]
    fn add_file_untouched_no_cwd_noop() {
        let v4a = "*** Begin Patch\n*** Add File: new.txt\n+hello\n*** End Patch\n";
        // 无 Update File → 短路原样返回(即便给 cwd)。
        let (out, reps) = preflight_repair(v4a, Some("/tmp"));
        assert_eq!(out, v4a);
        assert!(reps.is_empty());
        // 无 cwd → 原样
        let (out2, reps2) = preflight_repair(v4a, None);
        assert_eq!(out2, v4a);
        assert!(reps2.is_empty());
    }
}
