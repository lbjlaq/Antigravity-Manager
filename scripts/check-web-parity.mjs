#!/usr/bin/env node
/**
 * Web 对齐度防回归检查（阶段 0，见 docs/WEBUI_NO_WEBVIEW2_PLAN.md）
 *
 * 交叉对照四方数据，保证"前端实际用到的每个 Tauri 命令在 Web 模式都可用"：
 *   1. src-tauri/src/lib.rs        —— invoke_handler 注册的命令全集
 *   2. src/**                      —— 前端实际以字符串字面量调用的命令
 *   3. src/utils/request.ts        —— COMMAND_MAPPING（命令 → HTTP 映射）
 *   4. src-tauri/src/proxy/server.rs —— 后端 /api 路由全集
 *
 * 失败条件（exit 1）：
 *   A. 前端实际调用的命令既无 COMMAND_MAPPING 又不在桌面专属白名单（Web 模式必挂）
 *   B. COMMAND_MAPPING 指向的 URL 在 server.rs 中不存在对应路由（Web 模式必挂）
 * 警告条件（exit 0，需人工关注）：
 *   C. 前端调用但 Rust 侧未注册的幽灵命令
 */
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';

const ROOT = join(import.meta.dirname, '..');

// 桌面专属命令：依赖窗口/本地文件系统/系统更新器，永远不要求 Web 映射
const DESKTOP_ONLY = new Set([
  'greet',
  'show_main_window',
  'set_window_theme',
  'save_text_file',
  'read_text_file',
  'check_appimage_installation',
  'check_native_update',
  'check_homebrew_installation',
  'brew_upgrade_cask',
  'patch_agy_binary',
  'migrate_data_dir',
  'set_data_dir',
  'get_data_dir_path',
  'cloudflared_check',
  // 本地 CLI 可执行文件探测（Settings 高级项），Web 端无本地文件系统语义
  'get_antigravity_cli_path',
]);

// Web-only 端点：只有 HTTP 实现、没有对应 Tauri 命令的合法命令名（不参与幽灵命令告警）
const WEB_ONLY_ENDPOINTS = new Set([
  'validate_path',
]);

function read(path) {
  return readFileSync(join(ROOT, path), 'utf8');
}

function walk(dir, exts, acc = []) {
  for (const name of readdirSync(dir)) {
    const full = join(dir, name);
    if (name === 'node_modules' || name === 'dist') continue;
    const st = statSync(full);
    if (st.isDirectory()) walk(full, exts, acc);
    else if (exts.some((e) => name.endsWith(e))) acc.push(full);
  }
  return acc;
}

// 1. lib.rs invoke_handler 注册名单（条目形如 `commands::proxy::get_proxy_stats` 或裸 `greet`）
const libRs = read('src-tauri/src/lib.rs');
const handlerBlock = libRs.match(/invoke_handler\(tauri::generate_handler!\[([\s\S]*?)\]\)/);
if (!handlerBlock) {
  console.error('FATAL: cannot locate invoke_handler block in src-tauri/src/lib.rs');
  process.exit(2);
}
const registered = new Set(
  handlerBlock[1]
    .replace(/\/\/[^\n]*/g, '')
    .split(',')
    .map((s) => s.trim().split('::').pop())
    .filter(Boolean)
);

// 2. 前端实际调用（命令名以字符串字面量出现于 src/**）
const srcFiles = walk(join(ROOT, 'src'), ['.ts', '.tsx']);
const usedByFrontend = new Set();
const CALL_SITE = /(?:\brequest|\binvoke)\s*(?:<[^>()]*>)?\(\s*['"`]([a-zA-Z0-9_]+)['"`]/g;
for (const file of srcFiles) {
  const text = readFileSync(file, 'utf8');
  for (const m of text.matchAll(CALL_SITE)) usedByFrontend.add(m[1]);
}

// 3. request.ts COMMAND_MAPPING（命令 → URL）
const requestTs = read('src/utils/request.ts');
const mapBlock = requestTs.match(/const COMMAND_MAPPING[^=]*=\s*\{([\s\S]*?)\n\};/);
if (!mapBlock) {
  console.error('FATAL: cannot locate COMMAND_MAPPING in src/utils/request.ts');
  process.exit(2);
}
const commandMapping = new Map();
for (const m of mapBlock[1].matchAll(/'([^']+)':\s*\{\s*url:\s*'([^']+)'/g)) {
  commandMapping.set(m[1], m[2]);
}

// 4. server.rs 全部 .route("...")（admin 路由在 .nest("/api", ...) 前，无 /api 前缀）
const serverRs = read('src-tauri/src/proxy/server.rs');
const apiRoutes = new Set();
for (const m of serverRs.matchAll(/\.route\(\s*"([^"]+)"/g)) {
  apiRoutes.add(m[1].replace(/:[^/"]+/g, ':*'));
}

const toBackendPath = (url) => {
  const path = url.startsWith('/api/') ? url.slice(4) : url;
  return path.replace(/:[^/]+/g, ':*');
};

const failures = [];
const warnings = [];
const desktopOnlyHits = [];

for (const cmd of [...usedByFrontend].sort()) {
  const mapped = commandMapping.has(cmd);
  const inRust = registered.has(cmd);
  if (mapped && !inRust && !WEB_ONLY_ENDPOINTS.has(cmd)) warnings.push(`ghost command: frontend calls "${cmd}" but it is NOT registered in lib.rs invoke_handler`);
  if (!inRust && !mapped) continue; // 纯前端内部字符串（误报容忍）
  if (DESKTOP_ONLY.has(cmd)) {
    desktopOnlyHits.push(cmd);
    continue;
  }
  if (!mapped) failures.push(`UNMAPPED: "${cmd}" is invoked by the frontend but has no COMMAND_MAPPING entry (web mode throws "not supported in Web mode")`);
}

for (const [cmd, url] of [...commandMapping].sort()) {
  if (url.startsWith('/api/') && !apiRoutes.has(toBackendPath(url))) {
    failures.push(`DEAD ROUTE: "${cmd}" maps to ${url} but no matching route exists in proxy/server.rs`);
  }
}

// 汇总输出
console.log(`Registered Rust commands : ${registered.size}`);
console.log(`Frontend-invoked commands: ${usedByFrontend.size}`);
console.log(`Web mappings             : ${commandMapping.size}`);
console.log(`Backend /api routes      : ${apiRoutes.size}`);
console.log(`Desktop-only (whitelist) : ${desktopOnlyHits.length}`);
console.log('');

const unregistered = [...registered].filter((c) => !commandMapping.has(c) && !usedByFrontend.has(c) && !DESKTOP_ONLY.has(c));
if (unregistered.length) {
  console.log(`[info] ${unregistered.length} registered command(s) unused by frontend and unmapped (desktop/internal/legacy):`);
  for (const c of unregistered) console.log(`  - ${c}`);
  console.log('');
}

for (const w of warnings) console.log(`[warn] ${w}`);
if (warnings.length) console.log('');

if (failures.length) {
  console.error(`[FAIL] ${failures.length} web-parity problem(s):`);
  for (const f of failures) console.error(`  - ${f}`);
  console.error('\nSee docs/WEBUI_NO_WEBVIEW2_PLAN.md (阶段 0) for the mapping rules.');
  process.exit(1);
}

console.log('[ok] web parity check passed.');
