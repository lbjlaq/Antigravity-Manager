```
Codex /v1/responses JSON                          Gemini v1internal JSON
═════════════════════════════                      ═════════════════════════

outer body:                                       outer body:
┌─ model                 ────────────────────────→ resolve_model_route() ──→ model
├─ instructions (string) ─┐                       ┌─ request:
│                         ├─ sanitize ────────────→│  ├─ systemInstruction ← ╗
│                         │ + Antigravity identity │  │   {role:"user",      ║
│                         │ + global system prompt │  │    parts:[{text}…]}  ║ ~17.5K tokens
│                         └────────────────────────│  │                      ║ 稳定前缀
│                                                  │  ├─ tools ────── ╗     ║
├─ tools (Codex schema)  ── flatten ── sort ──────→│  │   {functionDeclarations:[…]} ║
│   {type,function,…}     + clean + uppercase      │  │                 ║     ║
│                                                  │  ├─ toolConfig     ║     ║
│                                                  │  ├─ generationConfig ← context params
│                                                  │  ├─ sessionId ← FNV-1a(account_id)
│                                                  │  └─ contents ← ═══════════════╝
│                                                  │      ↕ (按 role 转换)
├─ input[]                                         │
│  ├─ {type:"message", role, content}             │    {role: user/model,
│  │   └─ text / input_image ─────────────────────→│     parts:[{text/inlineData}]}
│  │                                                │
│  ├─ {type:"function_call", name, arguments, id} │    {role: model,
│  │   └─ name → shell/apply_patch/… ────────────→│     parts:[{functionCall:{name,args,id}}]}
│  │                                                │
│  ├─ {type:"function_call_output", call_id, output}│  {role: user,
│  │   └─ output → {result} ──────────────────────→│     parts:[{functionResponse:{name,response,id}}]}
│  │                                                │
│  └─ {type:"local_shell_call" / "web_search_call"}→│  同上，特殊 name 映射
│
├─ temperature           ─────────────────────────→ generationConfig.temperature
├─ max_tokens            ─────────────────────────→ generationConfig.maxOutputTokens
├─ top_p                 ─────────────────────────→ generationConfig.topP
├─ thinking              ─────────────────────────→ generationConfig.thinkingConfig {thinkingBudget}
├─ stream                ── handler 控制 ──────────→ streamGenerateContent / generateContent
│
└─ prompt_cache_key (Codex)── 未使用
                                                    
                                                    outer body (续):
                                                    ├─ project
                                                    ├─ userAgent: "antigravity"  
                                                    └─ requestId ← [末尾]
```

**关键路径三句话：**

| 流向                                                         | 转换                           |
| :----------------------------------------------------------- | :----------------------------- |
| **Codex `instructions`** → developer message → `sanitize()` 清洗动态值 → 加 Antigravity 身份 → `systemInstruction.parts[].text` | 稳定前缀的核心 (~17.5K tokens) |
| **Codex `input[]`** → 逐 item 映射：`message`→`contents[role]`, `function_call`→`functionCall`, `function_call_output`→`functionResponse` | 动态内容 (~1.1M tokens)        |
| **Codex `tools[]`** → `flatten` 展平 namespace → `sort` 按 name 排序 → clean schema → `tools[].functionDeclarations` | 稳定前缀的一部分 (~5K tokens)  |