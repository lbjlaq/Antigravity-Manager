# 📝 Changelog

> Complete version history for Antigravity Tools. Return to project home at [README_EN.md](README_EN.md).

*   **Version History**:
    *   **Unreleased**:
        -   **[Linux] Black window on niri / Hyprland (Issue #3388)**:
            -   **Do not force `GDK_BACKEND=x11` just because `DISPLAY` is set**: niri / Hyprland / sway / river / labwc / wayfire always expose Xwayland, and forcing X11 makes the WebKitGTK UI go black. Those compositors keep native Wayland; the historical GNOME / KDE X11 fallback is unchanged.
            -   **Disable WebKit DMA-BUF only when needed**: on Wayland, if the compositor is in that set or a proprietary NVIDIA driver is loaded, and the user has not set it, set `WEBKIT_DISABLE_DMABUF_RENDERER=1`. GNOME + AMD/Intel keep the fast path. Existing env vars are never overridden.
    *   **v4.6.7 (2026-09-04)**:
        -   **[Core Fix] Fix Multi-Turn Agent Context Explosion & Session History Duplication BUG (Issue #3382)**:
            -   **Unblock Thinking Compression on Historical Assistant Messages with Tool Calls**: Fixed an issue where the strict `!has_tool_calls` check prevented compressing historical assistant `reasoning_content` in agent environments (e.g. OpenClaw) where almost every assistant turn contains tool calls. Allows pruning long thoughts down to placeholder `...` while fully preserving tool calls and their valid `thoughtSignature` tokens, eliminating token bloat and preventing upstream Google API 400 signature errors.
            -   **Normalized Thought Placeholder for Older Turns**: Updated `openai/request.rs` to generate a minimal placeholder thought block `{ "text": "...", "thought": true }` for historical assistant thinking in older turns outside the recent window, fulfilling upstream thinking schema requirements while discarding thousands of redundant thinking tokens.
            -   **Semantic History Matching & Duplication Prevention in Session Store**: Enhanced `prepare_session_input` with semantic prefix matching (ignoring client ID format differences) and sliding suffix boundary identification. Added fallback protection: when client sends full history and matches fail, uses client's full sequence instead of appending client history onto server history, eliminating exponential history compounding (2x/4x).
        -   **[Feature] Support Video Multimodal Inputs (`video_url` / `inlineData`) in OpenAI-Compatible API (Issue #3381)**:
            -   **OpenAI Content Block Support for `video_url`**: Extended `OpenAIContentBlock` to natively deserialize and process `video_url` blocks, resolving `Invalid request: data did not match any variant of untagged enum OpenAIContent` when clients send video inputs.
            -   **Format Detection & Native Gemini Multimodal Alignment**: Implemented a dedicated video processor (`proxy/video`) supporting base64 data URLs (`data:video/mp4;base64,...`), remote video URLs (`fileData`), local files (`file://` or filesystem paths automatically encoded to inlineData), and raw base64. Covers MP4, WebM, MOV, AVI, WMV, MKV format normalization, oversize advisory warnings, and token estimation.
    *   **v4.6.6 (2026-09-03)**:
        -   **[Core Fix] Fix Gemini 3.7 Tool Call Text Leakage Causing Silent Agent Interruption in Long Contexts (Issue #3379)**:
            -   **call:default_api Leakage Detection & Controlled Fail-Closed Recovery Bridge**: Resolved an issue where Gemini 3.7 Flash, following long-context compression, occasionally leaks internal pseudocode tool invocations (`call:default_api:ToolName{...}`) into plain text deltas instead of structured `functionCall` blocks, silently breaking Claude Desktop / Claude Code agent loops. Enforced a 7-point strict fail-closed guard sequence (registered tools required, no native tool use in current turn, strict prefix match, registered tool whitelist alignment, no surrounding prose, valid JSON args, no prior text deltas emitted) to securely recover leaked calls into standard `tool_use` blocks without prompt injection risks.
            -   **Symmetric Streaming/Non-Streaming Parity & Diagnostics**: Symmetrically aligned recovery logic across SSE streaming (`streaming.rs`) and non-streaming responses (`response.rs`), added diagnostic warning logs (`tracing::warn`) when text patterns are detected, and reinforced stability with 9 unit test scenarios.
    *   **v4.6.5 (2026-09-02)**:
        -   **[Core Fix] Fix Gemini JSON Schema Validation 400 Errors for Nested Arrays Missing Items (PR #3375)**:
            -   **Array Items Fallback Injection**: Resolved upstream Gemini 400 schema validation errors triggered by clients such as Claude Code when emitting itemless array schemas (e.g. `query.where: { type: "array", items: { type: "array" } }`). The recursive JSON schema sanitization now injects a Gemini-compatible `{"type": "string"}` fallback for itemless `array` nodes, backed by unit regression tests.
        -   **[Streaming Proxy & Protocol Compliance] Standardize Claude Upstream Stream Interruption Error Events (PR #3371, PR #3373)**:
            -   **Anthropic Standard SSE Error Output**: Fixed non-standard error payload formatting during upstream stream breaks or exceptions. Standardized on the Anthropic-compliant `type: "error"` structure with `error: { "type": "overloaded_error", "message": ... }` emitted via `state.emit("error", ...)`, ensuring Claude clients reliably catch and handle stream aborts.
        -   **[OpenCode & Model Support] Add Thinking Variant Support to Base Claude Opus 4.5/4.6 Models (PR #3371, PR #3373)**:
            -   **Opus Base Model Thinking Variants**: Added `VariantType::ClaudeThinking` support for `claude-opus-4-5` and `claude-opus-4-6` base model definitions. This resolves an issue where selecting base model IDs yielded no thinking tiers in dropdown selectors. Also aligned `Gemini3Pro` variant ordering.
        -   **[Linux & AppImage] Fix GUI Popup on Version Detection & Child Process Environment Leaks under AppImage (Issue #3370)**:
            -   **Static Package.json Priority Version Reading**: Optimized Linux Antigravity version detection to prioritize reading `resources/app/package.json` directly from the installation directory, preventing executing `--version` from inadvertently launching the Chromium/Electron GUI window.
            -   **Child Process AppImage Environment Sanitization**: When spawning Antigravity on Linux, AppImage runtime environment variables (`APPIMAGE`, `APPDIR`, `ARGV0`, `LD_LIBRARY_PATH`, `GTK_PATH`, etc.) are stripped, and `/tmp/.mount_*` paths are filtered from `XDG_DATA_DIRS` to avoid inheriting conflicting runtime libraries.
            -   **Hardened Process Detection & Self-Exclusion**: Enhanced Linux running process scanning and path resolution to ensure AppImage mount paths and parent/ancestor processes are not misidentified as target Antigravity instances.
        -   **[System & Tray] Prevent Window-State Plugin from Restoring Window Visibility on Startup (PR #3373)**:
            -   **Window Visibility State Filter**: Updated `tauri-plugin-window-state` configuration to exclude `StateFlags::VISIBLE`. This prevents the window from popping up on autostart or tray background launches when configured with `visible: false`.
    *   **v4.6.4 (2026-08-30)**:
        -   **[Core Fix] Fix Token Acquisition Timeout (5s) / Deadlock Error Under High Concurrency & Tokio Runtime Starvation (Issue #3348)**:
            -   **Async Disk I/O & Blocking Thread Pool Isolation**: Refactored `update_account_json` to an async function that dispatches synchronous disk I/O and global account locks to Tokio's blocking thread pool (`spawn_blocking`). This prevents disk serialization contention from blocking Tokio worker threads and causing runtime starvation under high concurrency.
            -   **Fire-and-Forget Disk Persistence on Token Acquisition Hot Path**: On the primary `get_token` scheduling path, file persistence after OAuth token refreshes and `project_id` resolution is now offloaded to background tasks after updating memory caches immediately, preventing disk write overhead from consuming the 5-second timeout window.
        -   **[Core Fix] Fix Indefinite Hang on Minimal/Single-Dot Prompts Causing Claude Desktop Gateway Health Check Timeout (Issue #3359)**:
            -   **Empty SSE Event Fallback**: Added defensive fallback logic in the Claude SSE streaming conversion layer. When upstream models terminate empty responses on minimal/punctuation-only prompts without yielding content or thinking, the stream synthesizer automatically emits valid `message_start`, fallback text ContentBlock, and `message_stop` events, eliminating peek loop timeouts.
            -   **Non-Streaming Collector Schema Guard**: Enforced that `collect_stream_to_json` always returns at least one valid text ContentBlock when parsing empty upstream streams, complying strictly with Anthropic client non-empty content constraints.
        -   **[Streaming & Session Management] Upstream SSE Cancellation on Client Disconnect & Session Branching Graph (PR #3367, PR #3366)**:
            -   **Proactive Upstream SSE Teardown**: Automatically stops polling and consuming upstream SSE streams as soon as the client disconnects or the downstream response body is dropped, preventing incomplete requests from saving invalid sessions.
            -   **Parent-Linked Session Graph**: Fixed streaming HTTP Responses session addressing and replaced full-history deep copies with a persistent parent-linked session tree graph for efficient branch support.
        -   **[Image Proxy & Account Scheduling] Account-Aware Image Scheduling & Rate Limit Lifecycle Hardening (PR #3364, PR #3363, PR #3362)**:
            -   **Account-Aware Image Concurrency Scheduler**: Introduced a shared concurrency scheduler for OpenAI and Gemini image generation and edit requests, enforcing per-account concurrency caps and seamless queueing when all accounts are busy.
            -   **Rate Limit Lifecycle & Grace Retry Optimization**: Corrected short 429 delay parsing, capped same-account retries to at most once, and preserved only explicit long-lived image quota deadlines.
            -   **Image Request Semantics Hardening**: Fully hardened OpenAI-compatible image requests with robust model alias resolution, size mapping, ordering, and input boundary validation.
        -   **[Network & Multimodal Improvements] Tool Image Retention, Bounded Debug SSE Capture & Quota Header Cleanup (PR #3365, PR #3361, PR #3360)**:
            -   **Tool Image Retention & Inline Media Bounding**: Retains images returned from tool outputs as multimodal model inputs, bounds inline image memory, and strips historical inline media before replay or caching.
            -   **Bounded Debug SSE Capture**: Implemented 256 KiB head + 256 KiB rolling tail buffer for debug response logging to eliminate memory bloat on large streams while preserving full downstream streaming.
            -   **Quota Header Cleanup**: Omits redundant `x-goog-user-project` headers on content requests to ensure proper upstream PA service authentication.
    *   **v4.6.3 (2026-08-30)**:
        -   **[Core Fix] Account JSON Storage Self-Healing & Concurrent File Write Lock (Issue #3345)**:
            -   **Self-Healing Parser on Load**: Added streaming deserializer fallback when reading account files. If an account file has trailing characters or extra closing braces (e.g. `trailing characters at line ...`), the parser automatically recovers the valid full `Account` data and atomically rewrites a clean file back to disk, completely preventing accounts from silently disappearing from the UI and causing cascading 429 rate limit outages.
            -   **Per-Account Concurrency Write Lock**: Introduced a global per-account mutex lock mechanism (`ACCOUNT_FILE_LOCKS`) to ensure strict serialized thread safety across concurrent quota refreshes, 429 rate-limit event writes, and `last_used` touch operations.
        -   **[Core Fix] Fix Discrete Model Chip Rendering for Pinned Gemini 3.7 Flash Models (Issue #3344)**:
            -   **Exact Match Priority**: Introduced exact-model matching in `resolveQuotaModels`. When a pinned selector matches a real quota model name (such as `gemini-3.7-flash-low`, `gemini-3.7-flash-high`, etc.), it renders as an independent discrete chip (`model:${id}`) instead of being collapsed and deduplicated into a single legacy `category:gemini-flash` slot that was hardcoded to older models.
            -   **Backward Compatibility**: Unmatched legacy category selectors and image selectors continue to use category-based resolution, preserving backward compatibility.
        -   **[Feature Optimization] Dashboard Best Accounts Recommendation with 5h & Weekly Quota Evaluation (Issue #3343)**:
            -   **Dual-Window Bottleneck Constraint**: Evaluates both the 5-hour rolling window and the 7-day weekly quota constraint ($\min(5h, weekly)$), avoiding recommending accounts that have a full 5h quota but have exhausted their weekly allowance.
            -   **Free Tier Single-Bucket Support**: Automatically detects single/dual-bucket account structures. Accounts with only a weekly quota (Free Tier) smoothly use their weekly quota percentage for evaluation, ensuring fair ranking without false zeroing.
            -   **Exhaustion Circuit Breaker**: Accounts with weekly quota $\le 5\%$ are disqualified from recommendation to prevent switching to unusable accounts.
        -   **[Core Fix] Gemini 3.7 / 3.x Thought-Signature Invalidation & Multi-Turn Variant Compatibility (PR #3342)**:
            -   **Case-Insensitive Thought Signature Error Matching**: Used `to_lowercase()` matching in Claude protocol and common handlers to capture all Google thought signature error variants (`Invalid thought signature.`, `thought_signature`, `thoughtsignature`), reliably triggering automatic retry and signature stripping.
            -   **Gemini 3.x Model Compatibility Rules**: Added explicit compatibility rules for `gemini-3.x` (Flash / Pro families) and `gemini-3.7` in `is_model_compatible`, ensuring thought signatures persist correctly across laddered variant turns.
        -   **[i18n] 100% Full Localization Across Multiple Languages (PR #3338, PR #3339, PR #3340, PR #3341)**:
            -   **Japanese (ja.json, PR #3338)**: Complete translations for quota protection, smart warmup, adaptive circuit breaker, context compression, model routing, and Homebrew updater; cleared residual strings.
            -   **Spanish (es.json, PR #3339)**: Complete translations for Proxy Pool, Debug Console, Network Monitor, IP Security Whitelist/Blacklist, OpenCode sync, and APIKEY.FUN relay.
            -   **Russian (ru.json, PR #3340)**: Complete translations for HTTP API server settings, Debug console, Homebrew upgrade workflow, Context Compression (Caveman/L1-L3), and streaming error prompts.
            -   **Korean (ko.json, PR #3341)**: Complete translations for proxy pool, debug console, model routing presets, 403 quick fix guide, and Homebrew update notifications.
    *   **v4.6.2 (2026-08-28)**:
        -   **[Core Fix] Proxy Startup Diagnostics for Silent Failure & Unreachable Ports (PR #3330)**:
            -   **Startup Failure Logging**: Added explicit `error!` logs when `load_app_config()` fails in `lib.rs`, converting silent exits into actionable error logs and clarifying that services were not started.
            -   **Cleaned Up Dummy Server Handle**: Removed unneeded placeholder `tokio::spawn(async {})` handles from `ProxyServiceInstance`, leaving unified lifecycle management to `AdminServerInstance`.
        -   **[i18n] Brazilian Portuguese (pt-BR) 100% Key Alignment with en.json (PR #3334)**:
            -   **1224+ Translation Keys Completed**: Translated all missing keys and removed residual Chinese strings (0 missing, 0 mismatched).
            -   **Placeholder Synchronization**: Aligned `{{name}}`, `{{error}}` interpolation parameters to avoid runtime UI render issues.
            -   **Component Direct References**: Added missing keys directly referenced by frontend TSX components.
        -   **[Enhancement] Model Catalog Update, Official Icons & OpenCode Sync Optimization (PR #3335)**:
            -   **New Model Support**: Added `gemini-3.7-flash`, `gemini-3.1-flash-lite`, `claude-opus-4-6`, `gpt-oss-120b-medium` with `@lobehub/icons` official brand icons.
            -   **Model List Deduplication**: Normalized alias mappings in `useProxyModels` to eliminate duplicate model entries caused by sub-tier suffixes.
            -   **OpenCode Sync Adjustments**: Enabled `ClaudeThinking` reasoning variants for Claude models and disabled unsupported `max` variants for Gemini 3 series.
        -   **[Platform Fix] Eliminate Windows Background Process Console Flashing (PR #3336)**:
            -   **Unified CREATE_NO_WINDOW Flags**: Replaced/supplemented `DETACHED_PROCESS` with `CREATE_NO_WINDOW` (0x08000000) across Cloudflared, tar decompression, and manual executable calls to eliminate console windows popping up.
            -   **Sync/Async Unified Handling**: Applied no-window flags consistently across `std::process::Command` and `tokio::process::Command` extensions.
        -   **[Core Fix] Fix Gemini 3.x 400 Bad Request on Thinking Block Compression (PR #3337)**:
            -   **Root Cause**: When `ContextManager` compressed thinking content to `"..."`, it preserved the original `thoughtSignature`, causing Google API to fail validation with `400 INVALID_ARGUMENT: Invalid thought signature`.
            -   **Fix**: Cleared the corresponding signature field when compressing thinking content to maintain signature chain integrity.
        -   **[Install Script Fix] Fix Linux Install Script 404 on Version Parsing (Issue #3328)**:
            -   **Validation & Direct Redirection**: Added `_is_valid_version()` semantic version format validation and switched Method 2 to `curl -w '%{url_effective}'` to avoid header parsing whitespace issues.
    *   **v4.6.1 (2026-08-25)**:
        -   **[Core Fix] Prevent 1M Token Overflow on Long Multi-Turn Thinking & Local Token Estimation Fallback (Issue #3325)**:
            -   **Historical Thinking Pruning**: When converting to Gemini contents, only the most recent window of assistant thinking text is preserved; older turns retain only `thoughtSignature` placeholders to prevent context from exceeding the 1M token ceiling.
            -   **Fallback Token Estimation**: If upstream Google returns an error without `usageMetadata`, middleware uses the local token estimation engine to calculate `input_tokens`, preventing blank token stats in monitor logs.
        -   **[Core Fix] JSON Schema `const` Keyword Normalization for Computer Use MCP (Issue #3327)**:
            -   **Schema Sanitization**: Automatically converts `{"const": "value"}` into standard `{"type": "...", "enum": ["value"]}` compatible with Gemini/Vertex Schema Proto.
            -   **Nested & Union Types Support**: Full support for `anyOf`/`oneOf` unions and deeply nested objects containing `const` fields.
    *   **v4.6.0 (2026-08-24)**:
        -   **[Core Feature] OpenAI Endpoint Supports `response_format.json_schema` Structured Outputs (PR #3324)**:
            -   **JSON Schema Support**: Full support for `response_format: { type: "json_schema", json_schema: { ... } }`.
            -   **Recursive Schema Unfolding**: Automatically extracts and sanitizes `$ref`/`$defs` definitions, converting schemas into Gemini `generationConfig.responseSchema` standards with `responseMimeType: "application/json"`.
        -   **[Core Fix] Proxy Pool Health Check 407 & URL Inline Auth Parsing Fix (Issue #3323)**:
            -   **HTTPS 204 Health Check**: Upgraded default health check endpoint to `https://cp.cloudflare.com/generate_204` via standard HTTPS `CONNECT` tunnels, eliminating false `407 Proxy Authentication Required` errors.
            -   **Inline Credentials Parsing**: Safely extracts `username` and `password` from `http(s)://user:pass@ip:port` proxy URLs and injects HTTP Basic Auth.
        -   **[Core Fix] Gemini 3.7 / 3.6 Flash Variant Mapping & 429 Fix (Issue #3322)**:
            -   **Registered 3.7 Variants**: Full registration for `gemini-3.7-flash`, `gemini-3.7-flash-low`, `gemini-3.7-flash-medium`, `gemini-3.7-flash-high`, and `gemini-3.7-flash-tiered`.
            -   **Eliminated False 429 Outages**: Fixed account quota scheduler falsely intercepting requests with "All accounts limited" on unregistered 3.7 variants.
    *   **v4.5.9 (2026-08-23)**:
        -   **[Core Feature] OpenAI Compatible Endpoint Multimodal Audio Input Support (PR #3321)**:
            -   **Standard Audio Formats**: Supports OpenAI official `input_audio` (Base64 + format) and `audio_url`, converting seamlessly to Gemini `inlineData`/`fileData`.
            -   **Normalization**: Normalizes `wav`, `mp3`, `m4a`, `ogg`, `flac`, `aiff` from Data URLs, remote HTTP links, local files, and raw Base64.
        -   **[Core Fix] OAuth Token Refresh Resilience & Backoff (PR #3321)**:
            -   **Proactive Buffer (5 Min)**: Increased token refresh window from 90s to 300s ahead of expiry.
            -   **Backoff Retry & Consecutive Failure Gate**: Retries after 500ms backoff on `invalid_grant` and disables accounts only after 2+ consecutive confirmed failures.
        -   **[Core Fix] 403 / VALIDATION_REQUIRED Detection & URL Parsing**:
            -   **Validation URL Extraction**: Parses `validation_url` / `appeal_url` from Google RPC responses and flags accounts in the UI with a quick-action verification button.
