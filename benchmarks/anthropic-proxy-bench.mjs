#!/usr/bin/env node
import { writeFileSync } from 'node:fs';
import { performance } from 'node:perf_hooks';

function parseArgs(argv) {
  const args = {
    baseUrl: 'http://127.0.0.1:8045',
    apiKey: null,
    model: 'claude-3-5-sonnet-20241022',
    profile: 'text',
    runs: 50,
    warmup: 5,
    concurrency: 1,
    out: null,
    timeoutMs: 180_000,
    temperature: 0,
    maxTokens: 1024,
  };

  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--help' || a === '-h') return { ...args, help: true };
    if (a === '--base-url') args.baseUrl = argv[++i];
    else if (a === '--api-key') args.apiKey = argv[++i];
    else if (a === '--model') args.model = argv[++i];
    else if (a === '--profile') args.profile = argv[++i];
    else if (a === '--runs') args.runs = Number(argv[++i]);
    else if (a === '--warmup') args.warmup = Number(argv[++i]);
    else if (a === '--concurrency') args.concurrency = Number(argv[++i]);
    else if (a === '--out') args.out = argv[++i];
    else if (a === '--timeout-ms') args.timeoutMs = Number(argv[++i]);
    else if (a === '--temperature') args.temperature = Number(argv[++i]);
    else if (a === '--max-tokens') args.maxTokens = Number(argv[++i]);
    else throw new Error(`Unknown arg: ${a}`);
  }

  if (args.apiKey === null && process.env.API_KEY) {
    args.apiKey = process.env.API_KEY;
  }

  if (!Number.isFinite(args.runs) || args.runs <= 0) {
    throw new Error('--runs must be a positive number');
  }
  if (!Number.isFinite(args.warmup) || args.warmup < 0) {
    throw new Error('--warmup must be a non-negative number');
  }
  if (!Number.isFinite(args.concurrency) || args.concurrency <= 0) {
    throw new Error('--concurrency must be a positive number');
  }
  if (!Number.isFinite(args.temperature) || args.temperature < 0) {
    throw new Error('--temperature must be a non-negative number');
  }
  if (!Number.isFinite(args.maxTokens) || args.maxTokens <= 0) {
    throw new Error('--max-tokens must be a positive number');
  }

  return args;
}

function printHelp() {
  console.log(`
Usage:
  node benchmarks/anthropic-proxy-bench.mjs [options]

Options:
  --base-url <url>       Default: http://127.0.0.1:8045
  --api-key <key>        Sent as Authorization: Bearer <key> (or set API_KEY env)
  --model <id>           Default: claude-3-5-sonnet-20241022
  --temperature <n>      Default: 0
  --max-tokens <n>       Default: 1024
  --profile <name>       text | caching | tool   (Default: text)
  --runs <n>             Default: 50
  --warmup <n>           Default: 5 (excluded from stats)
  --concurrency <n>      Default: 1
  --out <file.json>      Write raw results JSON
  --timeout-ms <n>       Default: 180000
  --help                 Show this help
`.trim());
}

function buildHeaders(apiKey) {
  const headers = new Headers();
  headers.set('content-type', 'application/json');
  headers.set('anthropic-version', '2023-06-01');
  headers.set('anthropic-beta', 'interleaved-thinking-2025-05-14');
  if (apiKey) {
    headers.set('authorization', `Bearer ${apiKey}`);
  }
  return headers;
}

function sseParseChunk(rawEvent) {
  const lines = rawEvent.split('\n');
  const eventLine = lines.find((l) => l.startsWith('event:'));
  const dataLines = lines.filter((l) => l.startsWith('data:'));
  if (!eventLine || dataLines.length === 0) return null;

  const type = eventLine.slice('event:'.length).trim();
  const dataText = dataLines.map((l) => l.slice('data:'.length).trim()).join('\n');
  if (!dataText) return null;

  try {
    const data = JSON.parse(dataText);
    return { type, data };
  } catch {
    return { type, data: null, dataText };
  }
}

function extractUsage(events) {
  const usage = {
    input_tokens: 0,
    output_tokens: 0,
    cache_read_input_tokens: 0,
    cache_creation_input_tokens: 0,
  };

  const messageStart = events.find((e) => e.type === 'message_start');
  if (messageStart?.data?.message?.usage) {
    const u = messageStart.data.message.usage;
    usage.input_tokens = u.input_tokens || 0;
    usage.cache_read_input_tokens = u.cache_read_input_tokens || 0;
    usage.cache_creation_input_tokens = u.cache_creation_input_tokens || 0;
  }

  const messageDelta = events.find((e) => e.type === 'message_delta');
  if (messageDelta?.data?.usage) {
    const u = messageDelta.data.usage;
    usage.output_tokens = u.output_tokens || 0;
    if (u.cache_read_input_tokens !== undefined) usage.cache_read_input_tokens = u.cache_read_input_tokens;
    if (u.cache_creation_input_tokens !== undefined) usage.cache_creation_input_tokens = u.cache_creation_input_tokens;
  }

  return usage;
}

function countEvents(events) {
  const counts = Object.create(null);
  for (const e of events) counts[e.type] = (counts[e.type] || 0) + 1;
  return counts;
}

function largeSystemPrompt() {
  return 'You are an expert software engineer. Here is important context:\n' +
    '// Large codebase file content line\n'.repeat(1000);
}

function buildProfileBody({ profile, model, temperature, maxTokens }) {
  if (profile === 'text') {
    return {
      model,
      stream: true,
      temperature,
      max_tokens: maxTokens,
      thinking: { type: 'enabled', budget_tokens: maxTokens },
      messages: [{ role: 'user', content: 'Say hello in one sentence.' }],
    };
  }

  if (profile === 'caching') {
    return {
      model,
      stream: true,
      temperature,
      max_tokens: maxTokens,
      thinking: { type: 'enabled', budget_tokens: maxTokens },
      system: largeSystemPrompt(),
      messages: [
        { role: 'user', content: 'Turn 1: briefly explain what a hash function is.' },
        { role: 'assistant', content: 'A hash function maps input data to a fixed-size digest in a deterministic way.' },
        { role: 'user', content: 'Turn 2: now explain what a collision is in one sentence.' },
      ],
    };
  }

  if (profile === 'tool') {
    return {
      model,
      stream: true,
      temperature,
      max_tokens: maxTokens,
      thinking: { type: 'enabled', budget_tokens: maxTokens },
      tools: [
        {
          name: 'get_weather',
          description: 'Get the current weather for a location',
          input_schema: {
            type: 'object',
            properties: { location: { type: 'string', description: 'City name' } },
            required: ['location'],
          },
        },
      ],
      messages: [
        { role: 'user', content: 'Call get_weather for location "Istanbul" and then summarize the result.' },
      ],
    };
  }

  throw new Error(`Unknown profile: ${profile}`);
}

async function benchOnce({ baseUrl, apiKey, body, timeoutMs }) {
  const url = new URL('/v1/messages', baseUrl).toString();

  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), timeoutMs);

  const startedAt = performance.now();
  let firstEventAt = null;
  let firstDeltaAt = null;
  let endedAt = null;

  const events = [];
  let httpStatus = 0;
  let httpOk = false;
  let error = null;

  try {
    const res = await fetch(url, {
      method: 'POST',
      headers: buildHeaders(apiKey),
      body: JSON.stringify(body),
      signal: controller.signal,
    });
    httpStatus = res.status;
    httpOk = res.ok;

    if (!res.body) {
      throw new Error('Missing response body');
    }

    const reader = res.body.getReader();
    const decoder = new TextDecoder();
    let buffer = '';

    while (true) {
      const { done, value } = await reader.read();
      if (done) break;

      buffer += decoder.decode(value, { stream: true });
      let idx;
      while ((idx = buffer.indexOf('\n\n')) >= 0) {
        const rawEvent = buffer.slice(0, idx);
        buffer = buffer.slice(idx + 2);

        const parsed = sseParseChunk(rawEvent);
        if (!parsed) continue;

        const now = performance.now();
        if (firstEventAt === null) firstEventAt = now;
        if (firstDeltaAt === null && parsed.type === 'content_block_delta') firstDeltaAt = now;

        events.push(parsed);
        if (parsed.type === 'message_stop') {
          endedAt = performance.now();
          clearTimeout(timeout);
          return {
            httpStatus,
            httpOk,
            timings: {
              ttft_ms: Math.round(((firstDeltaAt ?? firstEventAt ?? now) - startedAt)),
              total_ms: Math.round((endedAt - startedAt)),
            },
            usage: extractUsage(events),
            eventCounts: countEvents(events),
          };
        }
      }
    }

    endedAt = performance.now();
  } catch (e) {
    endedAt = performance.now();
    error = e?.name === 'AbortError' ? `Timeout after ${timeoutMs}ms` : String(e?.message || e);
  } finally {
    clearTimeout(timeout);
  }

  return {
    httpStatus,
    httpOk,
    timings: {
      ttft_ms: firstDeltaAt || firstEventAt ? Math.round(((firstDeltaAt ?? firstEventAt) - startedAt)) : null,
      total_ms: endedAt ? Math.round((endedAt - startedAt)) : null,
    },
    usage: extractUsage(events),
    eventCounts: countEvents(events),
    error,
  };
}

function percentile(values, p) {
  if (values.length === 0) return null;
  const sorted = [...values].sort((a, b) => a - b);
  const idx = Math.floor((p / 100) * (sorted.length - 1));
  return sorted[idx];
}

async function runAll({ baseUrl, apiKey, profile, model, temperature, maxTokens, runs, warmup, concurrency, timeoutMs }) {
  const body = buildProfileBody({ profile, model, temperature, maxTokens });
  const warmupResults = [];
  const results = [];

  for (let i = 0; i < warmup; i++) {
    const r = await benchOnce({ baseUrl, apiKey, body, timeoutMs });
    warmupResults.push(r);
    const ok = r.httpOk && !r.error;
    const ttft = r.timings.ttft_ms;
    const total = r.timings.total_ms;
    console.log(`[warmup ${i + 1}/${warmup}] ${ok ? 'OK' : 'FAIL'} status=${r.httpStatus} ttft=${ttft}ms total=${total}ms`);
  }

  let nextIndex = 0;
  async function worker() {
    while (true) {
      const i = nextIndex++;
      if (i >= runs) return;
      const r = await benchOnce({ baseUrl, apiKey, body, timeoutMs });
      results[i] = r;
      const ok = r.httpOk && !r.error;
      const ttft = r.timings.ttft_ms;
      const total = r.timings.total_ms;
      console.log(`[${i + 1}/${runs}] ${ok ? 'OK' : 'FAIL'} status=${r.httpStatus} ttft=${ttft}ms total=${total}ms`);
    }
  }

  const workers = Array.from({ length: Math.min(concurrency, runs) }, () => worker());
  await Promise.all(workers);

  return { profile, model, temperature, maxTokens, baseUrl, warmup, runs, concurrency, warmupResults, results };
}

function summarize(run) {
  const ok = run.results.filter((r) => r.httpOk && !r.error);
  const fail = run.results.length - ok.length;

  const ttft = ok.map((r) => r.timings.ttft_ms).filter((n) => Number.isFinite(n));
  const total = ok.map((r) => r.timings.total_ms).filter((n) => Number.isFinite(n));
  const outTok = ok.map((r) => r.usage?.output_tokens || 0);
  const totSec = total.reduce((a, b) => a + b, 0) / 1000;
  const tokPerSec = totSec > 0 ? (outTok.reduce((a, b) => a + b, 0) / totSec) : null;

  return {
    ok: ok.length,
    fail,
    ttft_ms: { p50: percentile(ttft, 50), p95: percentile(ttft, 95) },
    total_ms: { p50: percentile(total, 50), p95: percentile(total, 95) },
    tokens_per_sec: tokPerSec ? Math.round(tokPerSec * 100) / 100 : null,
  };
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  if (args.help) {
    printHelp();
    return;
  }

  const run = await runAll(args);
  const summary = summarize(run);

  console.log('\nSummary');
  console.log(`  baseUrl:      ${run.baseUrl}`);
  console.log(`  profile:      ${run.profile}`);
  console.log(`  model:        ${run.model}`);
  console.log(`  temperature:  ${run.temperature}`);
  console.log(`  max_tokens:   ${run.maxTokens}`);
  console.log(`  warmup:       ${run.warmup}`);
  console.log(`  runs:         ${run.runs} (measured)`);
  console.log(`  concurrency:  ${run.concurrency}`);
  console.log(`  ok/fail:      ${summary.ok}/${summary.fail}`);
  console.log(`  ttft_ms p50/p95:       ${summary.ttft_ms.p50}/${summary.ttft_ms.p95}`);
  console.log(`  total_ms p50/p95:      ${summary.total_ms.p50}/${summary.total_ms.p95}`);
  console.log(`  tokens/sec (approx):   ${summary.tokens_per_sec}`);

  if (args.out) {
    writeFileSync(args.out, JSON.stringify({ ...run, summary }, null, 2));
    console.log(`\nWrote ${args.out}`);
  }
}

main().catch((e) => {
  console.error(e?.stack || String(e));
  process.exit(1);
});
