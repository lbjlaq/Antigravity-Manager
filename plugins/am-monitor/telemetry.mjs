import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { execFile } from 'node:child_process';

const PRICING = {
  inputPerM: 1.25,
  cacheReadPerM: 0.125,
  outputPerM: 10.0,
  monthlyPlanUsd: 20.0,
};

function getCandidateBrainDirs() {
  const home = os.homedir();
  return [
    path.join(home, '.gemini', 'antigravity', 'brain'),
    path.join(home, '.gemini', 'antigravity-ide', 'brain'),
    path.join(home, '.gemini', 'antigravity-cli', 'brain'),
  ].filter((d) => fs.existsSync(d));
}

function cleanUserTitle(raw) {
  if (!raw || typeof raw !== 'string') return '';
  let s = raw
    .replace(/<USER_REQUEST>/gi, '')
    .replace(/<\/USER_REQUEST>/gi, '')
    .replace(/<CONTEXT_SUMMARY>[\s\S]*?<\/CONTEXT_SUMMARY>/gi, '')
    .replace(/<ADDITIONAL_METADATA>[\s\S]*?<\/ADDITIONAL_METADATA>/gi, '')
    .replace(/\s+/g, ' ')
    .trim();
  if (s.length > 48) s = s.slice(0, 48) + '…';
  return s;
}

function stripToolHeader(raw, toolCall = null) {
  if (toolCall && (toolCall.args || toolCall.parameters)) {
    const a = toolCall.args || toolCall.parameters || {};
    const cmd = a.CommandLine || a.command || a.cmd;
    if (cmd && typeof cmd === 'string') return cmd.trim();
    const p = a.Path || a.path || a.file_path;
    if (p && typeof p === 'string') return p.trim();
    const q = a.query || a.queries || a.prompt || a.Prompt;
    if (q) return (typeof q === 'string' ? q : JSON.stringify(q)).trim();
  }
  if (!raw || typeof raw !== 'string') return '';
  const lines = raw.split(/\r?\n/);
  const kept = [];
  for (const line of lines) {
    const s = line.trim();
    if (!s) continue;
    if (/^(Created At:|Completed At:|Command exited with code|Command output:|The tool's output was truncated|Here is a preview)/i.test(s)) {
      continue;
    }
    kept.push(s);
    if (kept.length >= 3) break;
  }
  const joined = kept.join(' ').replace(/\s+/g, ' ').trim();
  return joined || raw.replace(/\s+/g, ' ').trim();
}

function extractTitleFromTranscript(filePath, fallbackId) {
  try {
    const fd = fs.openSync(filePath, 'r');
    const buf = Buffer.alloc(65536);
    const bytes = fs.readSync(fd, buf, 0, buf.length, 0);
    fs.closeSync(fd);
    for (const line of buf.toString('utf8', 0, bytes).split(/\r?\n/)) {
      if (!line.trim()) continue;
      try {
        const v = JSON.parse(line);
        if (v.type === 'USER_INPUT' && typeof v.content === 'string') {
          const title = cleanUserTitle(v.content);
          if (title) return title;
        }
      } catch {}
    }
  } catch {}
  return fallbackId.slice(0, 8) + '…';
}

export function listRecentSessions(limit = 20) {
  const seen = new Map();
  for (const brainDir of getCandidateBrainDirs()) {
    let entries = [];
    try { entries = fs.readdirSync(brainDir); } catch { continue; }
    for (const name of entries) {
      const tPath = path.join(brainDir, name, '.system_generated', 'logs', 'transcript.jsonl');
      try {
        const st = fs.statSync(tPath);
        const prev = seen.get(name);
        if (!prev || st.mtimeMs > prev.updated_at_ms) {
          seen.set(name, { session_id: name, updated_at_ms: st.mtimeMs, size_bytes: st.size, path: tPath });
        }
      } catch {}
    }
  }
  const list = Array.from(seen.values())
    .sort((a, b) => b.updated_at_ms - a.updated_at_ms)
    .slice(0, limit);
  for (const item of list) item.title = extractTitleFromTranscript(item.path, item.session_id);
  return list;
}

function estimateCostUsd(input, cacheRead, output) {
  return (
    (input / 1_000_000) * PRICING.inputPerM +
    (cacheRead / 1_000_000) * PRICING.cacheReadPerM +
    (output / 1_000_000) * PRICING.outputPerM
  );
}

export function analyzeTranscript(filePath, { viewMode = 'window', targetLimit = 1_000_000 } = {}) {
  const empty = {
    active_context_tokens: 0, latest_cache_read_tokens: 0, latest_new_input_tokens: 0,
    total_input_tokens: 0, total_cache_read_tokens: 0, total_output_tokens: 0,
    cumulative_tokens: 0, total_steps: 0, avg_cache_hit_rate: 0, latest_cache_hit_rate: 0,
    cache_drop_count: 0, error_count: 0, recent_errors: [],
    estimated_cost_usd: 0, roi_multiplier: 0,
    avg_tps: 0, peak_tps: 0, latest_tps: 0, model_duration_sec: 0, tool_duration_sec: 0,
    avg_delta_per_turn: 0, turns_remaining_to_80: null, compaction_count: 0,
    model: '', model_counts: [], tool_counts: [], breakdown: [], turns: [], heaviest_steps: [],
  };
  if (!filePath || !fs.existsSync(filePath)) return empty;

  let raw = '';
  try { raw = fs.readFileSync(filePath, 'utf8'); } catch { return empty; }

  const allTurns = [];
  const allSteps = [];
  const toolCounts = new Map();
  const modelStats = new Map();
  const errors = [];
  let lastModel = '';
  let lastToolName = 'tool';
  let lastToolCall = null;
  let lastCtx = 0;
  let lastHit = null;
  let cacheDropCount = 0;
  let lastCompactionIdx = 0;
  let compactionCount = 0;
  let totalSteps = 0;
  let prevTs = null;
  let modelDurationSec = 0;
  let toolDurationSec = 0;
  let msgChars = 0, toolChars = 0, sysChars = 0, thinkChars = 0;

  for (const line of raw.split(/\r?\n/)) {
    if (!line.trim()) continue;
    let v;
    try { v = JSON.parse(line); } catch { continue; }
    totalSteps += 1;

    const stepIndex = Number(v.step_index || totalSteps);
    const stepType = String(v.type || 'STEP');
    if (typeof v.model === 'string' && v.model) {
      lastModel = v.model;
    }

    if (Array.isArray(v.tool_calls) && v.tool_calls.length > 0) {
      for (const tc of v.tool_calls) {
        if (tc && tc.name) {
          lastToolName = tc.name;
          lastToolCall = tc;
          toolCounts.set(tc.name, (toolCounts.get(tc.name) || 0) + 1);
        }
      }
    }

    if (stepType === 'ERROR_MESSAGE') {
      const preview = stripToolHeader(String(v.content || 'Unknown error')).slice(0, 96);
      errors.push({ step_index: stepIndex, preview });
    }

    const ts = v.created_at ? Date.parse(v.created_at) : NaN;
    let stepDurSec = 0;
    if (Number.isFinite(ts) && prevTs !== null) {
      const diff = (ts - prevTs) / 1000;
      if (diff > 0 && diff < 300) {
        stepDurSec = diff;
        if (stepType === 'PLANNER_RESPONSE') modelDurationSec += diff;
        else if (stepType === 'GENERIC') toolDurationSec += diff;
      }
    }
    if (Number.isFinite(ts)) prevTs = ts;

    const contentStr = typeof v.content === 'string' ? v.content : '';
    const contentLen = contentStr.length;
    const thinkLen = typeof v.thinking === 'string' ? v.thinking.length : 0;
    if (stepType === 'USER_INPUT' || stepType === 'PLANNER_RESPONSE') msgChars += contentLen;
    else if (stepType === 'SYSTEM_MESSAGE') sysChars += contentLen;
    else toolChars += contentLen;
    thinkChars += thinkLen;

    const u = v.usage || v.token_usage || v;
    const inp = Number(u.input_tokens || u.prompt_tokens || 0);
    const cache = Number(u.cache_read_tokens || u.cached_content_token_count || 0);
    const out = Number(u.output_tokens || u.candidates_token_count || 0);

    if (inp > 0 || cache > 0 || out > 0) {
      const ctx = inp + cache;
      const isCompactDrop = lastCtx > 40_000 && ctx < lastCtx * 0.6;
      if (isCompactDrop) {
        compactionCount += 1;
        lastCompactionIdx = allTurns.length;
      }
      lastCtx = ctx;
      const hitRate = ctx > 0 ? cache / ctx : 0;
      if (lastHit !== null && lastHit > 0.7 && hitRate < lastHit - 0.25) cacheDropCount += 1;
      lastHit = hitRate;
      const tps = out >= 20 && stepDurSec >= 0.4 ? Math.min(400, out / stepDurSec) : 0;
      const mName = (typeof v.model === 'string' && v.model) ? v.model : (lastModel || 'unknown');
      const ms = modelStats.get(mName) || { turns: 0, input: 0, cache: 0, tpsSum: 0, tpsCnt: 0 };
      ms.turns += 1;
      ms.input += inp;
      ms.cache += cache;
      if (tps > 0) { ms.tpsSum += tps; ms.tpsCnt += 1; }
      modelStats.set(mName, ms);
      allTurns.push({
        step_index: stepIndex, input_tokens: inp, cache_read_tokens: cache,
        output_tokens: out, total: ctx, cache_hit_rate: hitRate,
        tps: Number(tps.toFixed(1)), compacted: isCompactDrop,
      });
    }

    if (contentLen > 60 && stepType !== 'SYSTEM_MESSAGE') {
      const cleaned = stripToolHeader(contentStr, lastToolCall);
      if (cleaned) {
        allSteps.push({
          step_index: stepIndex,
          label: stepType === 'GENERIC' ? lastToolName : stepType,
          approx_tokens: Math.floor(contentLen / 4),
          preview: cleaned.slice(0, 72),
        });
      }
    }
  }

  const scopedTurns = viewMode === 'window' && lastCompactionIdx > 0 ? allTurns.slice(lastCompactionIdx) : allTurns;
  const minStepIndex = scopedTurns.length > 0 ? scopedTurns[0].step_index : 0;
  const scopedSteps = viewMode === 'window' && lastCompactionIdx > 0
    ? allSteps.filter((s) => s.step_index >= minStepIndex)
    : allSteps;

  let totalIn = 0, totalCache = 0, totalOut = 0;
  const validTps = [];
  for (const t of scopedTurns) {
    totalIn += t.input_tokens;
    totalCache += t.cache_read_tokens;
    totalOut += t.output_tokens;
    if (t.tps > 0) validTps.push(t.tps);
  }

  const latestTurn = scopedTurns[scopedTurns.length - 1] || null;
  const activeCtx = latestTurn ? latestTurn.total : 0;
  const denom = totalIn + totalCache;
  const avgCache = denom > 0 ? totalCache / denom : 0;
  const latestCache = latestTurn ? latestTurn.cache_hit_rate : 0;
  const avgTps = validTps.length ? validTps.reduce((a, b) => a + b, 0) / validTps.length : 0;
  const peakTps = validTps.length ? Math.max(...validTps) : 0;
  const latestTps = validTps.length ? validTps[validTps.length - 1] : 0;

  let avgDelta = 0;
  if (scopedTurns.length >= 2) {
    const recent = scopedTurns.slice(-7);
    const deltas = [];
    for (let i = 1; i < recent.length; i++) {
      const d = recent[i].total - recent[i - 1].total;
      if (d > 0) deltas.push(d);
    }
    if (deltas.length > 0) avgDelta = Math.round(deltas.reduce((a, b) => a + b, 0) / deltas.length);
  }

  const warn80 = Math.round(targetLimit * 0.8);
  let turnsRemainingTo80 = null;
  if (activeCtx >= warn80) turnsRemainingTo80 = 0;
  else if (avgDelta > 0) turnsRemainingTo80 = Math.max(1, Math.floor((warn80 - activeCtx) / avgDelta));

  const rawTotal = Math.max(1, msgChars + toolChars + sysChars + thinkChars);
  const msgTok = Math.round((activeCtx * msgChars) / rawTotal);
  const toolTok = Math.round((activeCtx * toolChars) / rawTotal);
  const sysTok = Math.round((activeCtx * sysChars) / rawTotal);
  const thinkTok = Math.max(0, activeCtx - msgTok - toolTok - sysTok);
  const remainingTok = Math.max(0, targetLimit - activeCtx);

  const breakdown = [
    { key: 'messages', label: '消息', color: '#2563eb', tokens: msgTok },
    { key: 'tools', label: '工具输出', color: '#10b981', tokens: toolTok },
    { key: 'system', label: '系统提示', color: '#f59e0b', tokens: sysTok },
    { key: 'thinking', label: '推理与开销', color: '#8b5cf6', tokens: thinkTok },
    { key: 'remaining', label: '剩余空间', color: 'rgba(148,163,184,0.22)', tokens: remainingTok },
  ];

  const costUsd = estimateCostUsd(totalIn, totalCache, totalOut);
  return {
    active_context_tokens: activeCtx,
    latest_cache_read_tokens: latestTurn ? latestTurn.cache_read_tokens : 0,
    latest_new_input_tokens: latestTurn ? latestTurn.input_tokens : 0,
    total_input_tokens: totalIn, total_cache_read_tokens: totalCache, total_output_tokens: totalOut,
    cumulative_tokens: totalIn + totalCache + totalOut, total_steps: totalSteps,
    avg_cache_hit_rate: avgCache, latest_cache_hit_rate: latestCache,
    cache_drop_count: cacheDropCount,
    error_count: errors.length,
    recent_errors: errors.slice(-3).reverse(),
    estimated_cost_usd: Number(costUsd.toFixed(2)),
    roi_multiplier: Number((costUsd / PRICING.monthlyPlanUsd).toFixed(2)),
    avg_tps: Number(avgTps.toFixed(1)), peak_tps: Number(peakTps.toFixed(1)), latest_tps: Number(latestTps.toFixed(1)),
    model_duration_sec: Math.round(modelDurationSec), tool_duration_sec: Math.round(toolDurationSec),
    avg_delta_per_turn: avgDelta, turns_remaining_to_80: turnsRemainingTo80,
    compaction_count: compactionCount, model: lastModel,
    model_counts: Array.from(modelStats.entries())
      .map(([name, s]) => ({
        name,
        turns: s.turns,
        cache_hit_rate: (s.input + s.cache) > 0 ? s.cache / (s.input + s.cache) : 0,
        avg_tps: s.tpsCnt > 0 ? Number((s.tpsSum / s.tpsCnt).toFixed(1)) : 0,
      }))
      .sort((a, b) => b.turns - a.turns)
      .slice(0, 6),
    tool_counts: Array.from(toolCounts.entries()).sort((a, b) => b[1] - a[1]).slice(0, 6),
    breakdown,
    turns: scopedTurns.slice(-80),
    heaviest_steps: scopedSteps.sort((a, b) => b.approx_tokens - a.approx_tokens).slice(0, 6),
  };
}

export function buildUsageAnalytics(sessions, range = 'all') {
  const now = Date.now();
  const cutoff =
    range === 'today' ? now - 86_400_000
    : range === '7d' ? now - 7 * 86_400_000
    : range === '30d' ? now - 30 * 86_400_000
    : 0;

  const dayMap = new Map();
  const modelMap = new Map();
  let totalIn = 0, totalCache = 0, totalOut = 0, tpsSum = 0, tpsCnt = 0;
  const scoped = sessions.filter((s) => !cutoff || s.updated_at_ms >= cutoff).slice(0, 40);

  for (const s of scoped) {
    const rep = analyzeTranscript(s.path, { viewMode: 'full' });
    totalIn += rep.total_input_tokens;
    totalCache += rep.total_cache_read_tokens;
    totalOut += rep.total_output_tokens;
    if (rep.avg_tps > 0) { tpsSum += rep.avg_tps; tpsCnt += 1; }
    for (const m of rep.model_counts || []) {
      modelMap.set(m.name, (modelMap.get(m.name) || 0) + m.turns);
    }
    const dayKey = new Date(s.updated_at_ms).toISOString().slice(0, 10);
    dayMap.set(dayKey, (dayMap.get(dayKey) || 0) + rep.total_input_tokens + rep.total_output_tokens);
  }

  const days = [];
  for (let i = 181; i >= 0; i--) {
    const d = new Date(now - i * 86_400_000).toISOString().slice(0, 10);
    days.push({ date: d, tokens: dayMap.get(d) || 0 });
  }

  const totalCostUsd = estimateCostUsd(totalIn, totalCache, totalOut);
  const denom = totalIn + totalCache;
  const totalModelTurns = Math.max(1, Array.from(modelMap.values()).reduce((a, b) => a + b, 0));
  const topModels = Array.from(modelMap.entries())
    .sort((a, b) => b[1] - a[1])
    .slice(0, 5)
    .map(([name, turns]) => ({ name, turns, pct: Math.round((turns / totalModelTurns) * 100) }));

  return {
    range,
    total_tokens: totalIn + totalCache + totalOut,
    total_input_tokens: totalIn,
    total_cache_read_tokens: totalCache,
    total_output_tokens: totalOut,
    avg_cache_hit_rate: denom > 0 ? totalCache / denom : 0,
    avg_tps: tpsCnt > 0 ? Number((tpsSum / tpsCnt).toFixed(1)) : 0,
    total_cost_usd: Number(totalCostUsd.toFixed(2)),
    roi_multiplier: Number((totalCostUsd / PRICING.monthlyPlanUsd).toFixed(2)),
    session_count: scoped.length,
    top_models: topModels,
    heatmap: days,
  };
}

export function queryAmAccounts(target = 'antigravity') {
  return new Promise((resolve) => {
    const candidates = ['am', path.join(os.homedir(), '.cargo', 'bin', process.platform === 'win32' ? 'am.exe' : 'am')];
    const tryNext = (idx) => {
      if (idx >= candidates.length) return resolve({ available: false, current_email: null, accounts: [] });
      const bin = candidates[idx];
      execFile(bin, ['list', '--json'], { timeout: 2500 }, (err, stdout) => {
        if (!err && stdout) {
          try {
            const parsed = JSON.parse(stdout);
            const accounts = Array.isArray(parsed) ? parsed : parsed.accounts || [];
            execFile(bin, ['current'], { timeout: 2000 }, (_e2, out2) => {
              const rowKey = target === 'ide' ? 'Antigravity IDE' : 'Antigravity App';
              const lines = String(out2 || '').split(/\r?\n/);
              const matchedLine = lines.find((l) => l.includes(rowKey)) || '';
              const m = matchedLine.match(/[\w.+-]+@[\w.-]+\.\w+/);
              resolve({ available: true, bin, current_email: m ? m[0].toLowerCase() : null, accounts });
            });
            return;
          } catch {}
        }
        tryNext(idx + 1);
      });
    };
    tryNext(0);
  });
}

export function switchAmAccount(accountId, target = 'antigravity') {
  return new Promise((resolve) => {
    const candidates = ['am', path.join(os.homedir(), '.cargo', 'bin', process.platform === 'win32' ? 'am.exe' : 'am')];
    const tryNext = (idx) => {
      if (idx >= candidates.length) return resolve({ ok: false, error: 'am CLI not found' });
      execFile(candidates[idx], ['switch', accountId, '--target', target], { timeout: 6000 }, (err, stdout, stderr) => {
        if (!err) return resolve({ ok: true, output: stdout });
        if (err.code === 'ENOENT') return tryNext(idx + 1);
        resolve({ ok: false, error: stderr || err.message });
      });
    };
    tryNext(0);
  });
}
