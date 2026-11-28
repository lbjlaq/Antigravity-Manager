import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFile } from 'node:child_process';
import { Response, SidecarApp } from 'sidecar_sdk';
import {
  listRecentSessions,
  analyzeTranscript,
  buildUsageAnalytics,
  queryAmAccounts,
  switchAmAccount,
} from '../../telemetry.mjs';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const UI_HTML_PATH = path.resolve(__dirname, '../../ui.html');

const app = new SidecarApp();

function jsonRes(data, status = 200) {
  return new Response(JSON.stringify(data), {
    contentType: 'application/json; charset=utf-8',
    status,
    headers: {
      'Cache-Control': 'no-store',
    },
  });
}

// 1. Serve UI page
app.page('/', () => {
  const html = fs.readFileSync(UI_HTML_PATH, 'utf8');
  return new Response(html, {
    contentType: 'text/html; charset=utf-8',
    status: 200,
    headers: {
      'Cache-Control': 'no-store',
    },
  });
});

// 2. Snapshot API (Current session tokens, context, cache hit rate, TPS, heaviest steps)
app.api('/api/snapshot', async (data) => {
  const sessionId = data.session || '';
  const viewMode = data.view === 'full' ? 'full' : 'window';
  const limit = Number(data.limit || 1_000_000);
  const sessions = listRecentSessions(20);
  const active = sessions.find((s) => s.session_id === sessionId) || sessions[0] || null;
  const report = analyzeTranscript(active?.path, { viewMode, targetLimit: limit });
  return jsonRes({
    host: 'antigravity',
    active_session_id: active?.session_id || null,
    active_session_title: active?.title || 'No active session',
    sessions: sessions.map((s) => ({
      session_id: s.session_id,
      title: s.title,
      updated_at_ms: s.updated_at_ms,
    })),
    report,
  });
}, 'GET');

// 3. Analytics & Accounts API (Macro summary, heatmap, accounts quota)
app.api('/api/analytics', async (data) => {
  const range = data.range || 'all';
  const sessions = listRecentSessions(40);
  const analytics = buildUsageAnalytics(sessions, range);
  const am = await queryAmAccounts('antigravity');
  return jsonRes({ analytics, am });
}, 'GET');

// 4. Hot-switch account API
app.api('/api/switch', async (data) => {
  if (!data.account) return jsonRes({ ok: false, error: 'Missing account' }, 400);
  const result = await switchAmAccount(String(data.account), 'antigravity');
  return jsonRes(result);
}, 'POST');

// 5. Wake Antigravity Manager main application API
app.api('/api/wake-am', async () => {
  if (process.platform === 'darwin') {
    execFile('open', ['-a', 'Antigravity Manager'], () => {});
  }
  return jsonRes({ ok: true });
}, 'POST');

// Start sidecar HTTP server
app.run();
