const vscode = require('vscode');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { pathToFileURL } = require('url');

let statusBarItem;
let panel;
let telemetryModPromise;
let amCache = { ts: 0, data: null };
const AM_CACHE_TTL_MS = 15000;

async function getCachedAmAccounts(t, force = false) {
  const now = Date.now();
  if (!force && amCache.data && now - amCache.ts < AM_CACHE_TTL_MS) {
    return amCache.data;
  }
  const data = await t.queryAmAccounts('ide');
  amCache = { ts: now, data };
  return data;
}

function getTelemetry() {
  if (!telemetryModPromise) {
    telemetryModPromise = import(pathToFileURL(path.join(__dirname, 'telemetry.mjs')).href);
  }
  return telemetryModPromise;
}

function fmtK(n) {
  n = Number(n || 0);
  if (n >= 1000000) return (n / 1000000).toFixed(1) + 'M';
  if (n >= 1000) return (n / 1000).toFixed(1) + 'k';
  return String(n);
}

function makeBar(pct) {
  const filled = Math.round(Math.min(100, Math.max(0, pct)) / 10);
  return '█'.repeat(filled) + '░'.repeat(10 - filled);
}

async function updateStatusBar(context) {
  try {
    const t = await getTelemetry();
    const sessions = t.listRecentSessions(1);
    const limit = context.globalState.get('ctxLimit', 1000000);
    const rep = t.analyzeTranscript(sessions[0]?.path, { viewMode: 'window', targetLimit: limit });
    const pct = Math.min(100, (rep.active_context_tokens / limit) * 100);
    const pillItems = context.globalState.get('pillItems', ['context', 'cache']);
    const parts = [];
    if (pillItems.includes('context')) parts.push(`${fmtK(rep.active_context_tokens)} · ${Math.round(pct)}%`);
    if (pillItems.includes('cache')) parts.push(`Cache ${Math.round(rep.avg_cache_hit_rate * 100)}%`);
    if (pillItems.includes('tps') && rep.avg_tps > 0) parts.push(`${rep.avg_tps} t/s`);
    if (pillItems.includes('cost')) parts.push(`$${rep.estimated_cost_usd.toFixed(2)}`);
    if (pillItems.includes('turns')) parts.push(`${rep.turns.length}T`);
    const amInfo = await getCachedAmAccounts(t);
    const curAcc = amInfo.current_email ? amInfo.accounts.find(a => String(a.email || '').toLowerCase() === amInfo.current_email) : null;
    if (pillItems.includes('account') && amInfo.current_email) {
      parts.push(amInfo.current_email.split('@')[0]);
    }
    if (pillItems.includes('quota') && curAcc?.quota) {
      const gp = Math.round(Number(curAcc.quota.gemini_percentage ?? 0));
      const cp = Math.round(Number(curAcc.quota.claude_percentage ?? 0));
      parts.push(`G${gp}% C${cp}%`);
    }
    statusBarItem.text = `$(pulse) ${parts.join(' | ') || 'AM Monitor'}`;

    const md = new vscode.MarkdownString('', true);
    md.isTrusted = true;
    const title = sessions[0]?.title || '当前会话';
    md.appendMarkdown(`**AM Monitor** · \`${rep.model || 'Antigravity'}\`\n\n`);
    if (amInfo.current_email) {
      const tier = String(curAcc?.quota?.subscription_tier || 'FREE').toUpperCase();
      const gp = Math.round(Number(curAcc?.quota?.gemini_percentage ?? 0));
      const cp = Math.round(Number(curAcc?.quota?.claude_percentage ?? 0));
      md.appendMarkdown(`👤 **${amInfo.current_email}** \`${tier}\` · G **${gp}%** · C **${cp}%**\n\n`);
    }
    md.appendMarkdown(`💬 ${title}\n\n`);
    md.appendMarkdown(`\`${makeBar(pct)}\` **${pct.toFixed(1)}%** (${fmtK(rep.active_context_tokens)} / ${fmtK(limit)})\n\n`);
    md.appendMarkdown(`| 指标 | 数值 |\n| :--- | ---: |\n`);
    md.appendMarkdown(`| 缓存命中 | **${(rep.avg_cache_hit_rate * 100).toFixed(1)}%** (骤降 ${rep.cache_drop_count}) |\n`);
    md.appendMarkdown(`| 生成速度 | **${rep.avg_tps > 0 ? rep.avg_tps + ' tok/s' : '-'}** (峰值 ${rep.peak_tps || '-'}) |\n`);
    md.appendMarkdown(`| API 价值 | **$${rep.estimated_cost_usd.toFixed(2)}** (${rep.roi_multiplier}x 回本) |\n`);
    md.appendMarkdown(`| 轮次 / 压缩 | **${rep.turns.length} 轮** · 压缩 ${rep.compaction_count} 次 |\n`);
    if (rep.error_count > 0) {
      md.appendMarkdown(`| 异常步骤 | **${rep.error_count} 条 ERROR** |\n`);
    }
    md.appendMarkdown(`\n---\n*点击状态栏胶囊直接打开 AM Monitor 面板*`);
    statusBarItem.tooltip = md;
  } catch {}
}

function openFullPanel(context) {
  if (panel) {
    panel.reveal(vscode.ViewColumn.Beside);
    return;
  }
  panel = vscode.window.createWebviewPanel(
    'amContextMonitor',
    'AM Monitor',
    vscode.ViewColumn.Beside,
    { enableScripts: true, retainContextWhenHidden: true }
  );
  panel.onDidDispose(() => { panel = undefined; });
  panel.webview.html = fs.readFileSync(path.join(__dirname, 'ui.html'), 'utf8');

  panel.webview.onDidReceiveMessage(async (msg) => {
    const t = await getTelemetry();
    if (msg.type === 'snapshot') {
      const sessions = t.listRecentSessions(20);
      const active = sessions.find(s => s.session_id === msg.session) || sessions[0] || null;
      const report = t.analyzeTranscript(active?.path, {
        viewMode: msg.view || 'window',
        targetLimit: Number(msg.limit || 1000000),
      });
      panel.webview.postMessage({
        reqId: msg.reqId,
        data: {
          host: 'ide',
          active_session_id: active?.session_id || null,
          active_session_title: active?.title || 'No active session',
          sessions: sessions.map(s => ({ session_id: s.session_id, title: s.title, updated_at_ms: s.updated_at_ms })),
          report,
        },
      });
    } else if (msg.type === 'analytics') {
      const sessions = t.listRecentSessions(40);
      const analytics = t.buildUsageAnalytics(sessions, msg.range || 'all');
      const am = await t.queryAmAccounts('ide');
      panel.webview.postMessage({ reqId: msg.reqId, data: { analytics, am } });
    } else if (msg.type === 'switch') {
      const res = await t.switchAmAccount(String(msg.account || ''), 'ide');
      amCache = { ts: 0, data: null };
      panel.webview.postMessage({ reqId: msg.reqId, data: res });
    } else if (msg.type === 'copyCompact') {
      await vscode.env.clipboard.writeText('/compact');
      vscode.window.showInformationMessage('已复制 /compact 到剪贴板');
    } else if (msg.type === 'wakeAm') {
      t.wakeAntigravityManager();
      panel.webview.postMessage({ reqId: msg.reqId, data: { ok: true } });
    } else if (msg.type === 'setLimit') {
      await context.globalState.update('ctxLimit', Number(msg.limit || 1000000));
      updateStatusBar(context);
    } else if (msg.type === 'setPill') {
      await context.globalState.update('pillItems', Array.isArray(msg.items) ? msg.items : ['context', 'cache']);
      updateStatusBar(context);
    } else if (msg.type === 'savePosterPng' && typeof msg.dataUrl === 'string') {
      const base64 = msg.dataUrl.replace(/^data:image\/png;base64,/, '');
      const out = path.join(os.homedir(), 'Desktop', `antigravity-poster-${Date.now()}.png`);
      fs.writeFileSync(out, Buffer.from(base64, 'base64'));
      vscode.window.showInformationMessage(`海报已保存至 ${out}`);
    }
  });
}

async function openQuickMenu(context) {
  const picked = await vscode.window.showQuickPick(
    [
      { id: 'compact', label: '$(zap) 复制 /compact', description: '一键复制上下文压缩指令' },
      { id: 'switch', label: '$(account) 快捷热切换账号', description: '调用 am CLI 热切换 Antigravity IDE 账号' },
      { id: 'wake', label: '$(rocket) 唤醒 Antigravity Manager', description: '打开主应用管理配额与工具箱' },
      { id: 'pill', label: '$(settings-gear) 自定义状态栏胶囊', description: '勾选状态栏显示的快捷指标' },
      { id: 'panel', label: '$(graph) 打开完整监控面板', description: '查看 4 大板块详情、26 周热力图与导出分享海报' },
    ],
    { placeHolder: 'AM Context Monitor · 快捷操作' }
  );
  if (!picked) return;

  if (picked.id === 'compact') {
    await vscode.env.clipboard.writeText('/compact');
    vscode.window.showInformationMessage('已复制 /compact 到剪贴板');
  } else if (picked.id === 'wake') {
    const t = await getTelemetry();
    t.wakeAntigravityManager();
  } else if (picked.id === 'panel') {
    openFullPanel(context);
  } else if (picked.id === 'pill') {
    const cur = context.globalState.get('pillItems', ['context', 'cache']);
    const opts = [
      { key: 'context', label: '上下文 %', picked: cur.includes('context') },
      { key: 'cache', label: '缓存命中 %', picked: cur.includes('cache') },
      { key: 'cost', label: 'API 参考费用 $', picked: cur.includes('cost') },
      { key: 'turns', label: '轮次', picked: cur.includes('turns') },
    ];
    const sel = await vscode.window.showQuickPick(opts, { canPickMany: true, placeHolder: '勾选要在底部状态栏显示的指标' });
    if (sel) {
      await context.globalState.update('pillItems', sel.map(x => x.key));
      updateStatusBar(context);
    }
  } else if (picked.id === 'switch') {
    const t = await getTelemetry();
    const am = await t.queryAmAccounts();
    if (!am.available || !am.accounts.length) {
      vscode.window.showWarningMessage('未检测到 am CLI 或可用账号，请先安装 Antigravity Manager');
      return;
    }
    const acc = await vscode.window.showQuickPick(
      am.accounts.map(a => ({ id: a.id, label: a.email || a.id, description: a.tier || '' })),
      { placeHolder: '选择要热切换的账号' }
    );
    if (acc) {
      const res = await t.switchAmAccount(acc.id, 'ide');
      amCache = { ts: 0, data: null };
      if (res.ok) vscode.window.showInformationMessage(`已热切换至 ${acc.label}`);
      else vscode.window.showErrorMessage(`切换失败: ${res.error}`);
    }
  }
}

function activate(context) {
  statusBarItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 100);
  statusBarItem.command = 'amMonitor.openPanel';
  statusBarItem.show();
  updateStatusBar(context);

  const timer = setInterval(() => updateStatusBar(context), 3500);
  context.subscriptions.push(statusBarItem, { dispose: () => clearInterval(timer) });

  context.subscriptions.push(
    vscode.commands.registerCommand('amMonitor.openPanel', () => openFullPanel(context))
  );
}

function deactivate() {}

module.exports = { activate, deactivate };
