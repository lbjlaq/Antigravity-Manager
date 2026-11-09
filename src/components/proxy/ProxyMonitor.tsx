import React, { useEffect, useState, useRef, useMemo, useCallback } from 'react';
import { listen } from '@tauri-apps/api/event';
import ModalDialog from '../common/ModalDialog';
import { useTranslation } from 'react-i18next';
import { request as invoke } from '../../utils/request';
import { Trash2, Search, X, ChevronLeft, ChevronRight, RefreshCw, User, Clock, Settings, HardDrive, Database, Check, FileWarning } from 'lucide-react';

import { AppConfig, ExperimentalConfig } from '../../types/config';
import { formatCompactNumber } from '../../utils/format';
import { useAccountStore } from '../../stores/useAccountStore';
import { isTauri } from '../../utils/env';
import { RequestDetailModal } from './RequestDetailModal';
import { QoSStatusBadge } from '../accounts/QoSStatusBadge';


interface ProxyRequestLog {
    id: string;
    timestamp: number;
    method: string;
    url: string;
    status: number;
    duration: number;
    model?: string;
    mapped_model?: string;
    upstream_model?: string;
    error?: string;
    request_body?: string;
    upstream_request_body?: string;
    response_body?: string;
    request_headers?: string;
    upstream_request_headers?: string;
    response_headers?: string;
    input_tokens?: number;
    output_tokens?: number;
    cached_tokens?: number;
    account_email?: string;
    protocol?: string;  // "openai" | "anthropic" | "gemini"
}

interface ProxyStats {
    total_requests: number;
    success_count: number;
    error_count: number;
}

interface ProxyMonitorProps {
    className?: string;
}

// Log Table Component
interface LogTableProps {
    logs: ProxyRequestLog[];
    loading: boolean;
    onLogClick: (log: ProxyRequestLog) => void;
    t: any;
}

interface ColumnWidths {
    status: number;
    endpoint: number;
    model: number;
    account: number;
    usage: number;
    duration: number;
    time: number;
}

const DEFAULT_COL_WIDTHS: ColumnWidths = {
    status: 70,
    endpoint: 200,
    model: 240,
    account: 150,
    usage: 185,
    duration: 105,
    time: 90,
};

const MIN_COL_WIDTHS: Record<keyof ColumnWidths, number> = {
    status: 55,
    endpoint: 120,
    model: 140,
    account: 100,
    usage: 160,
    duration: 85,
    time: 75,
};

function extractTtft(log: ProxyRequestLog): number | undefined {
    // 1. 优先从 response_headers 中读取
    if (log.response_headers) {
        try {
            const headers = JSON.parse(log.response_headers);
            if (headers && typeof headers === 'object') {
                for (const key of Object.keys(headers)) {
                    const lower = key.toLowerCase();
                    if (lower === 'x-timing-ttft-ms' || lower === 'x-timing-ttft') {
                        const val = parseFloat(String(headers[key]));
                        if (!isNaN(val) && val > 0) return val;
                    }
                }
                // 1.1 兜底：若 x-timing-ttft-ms 为 -1，则尝试从 x-timing-headers-ms 读取响应头到达耗时
                for (const key of Object.keys(headers)) {
                    const lower = key.toLowerCase();
                    if (lower === 'x-timing-headers-ms') {
                        const val = parseFloat(String(headers[key]));
                        if (!isNaN(val) && val > 0) return val;
                    }
                }
                // 1.2 兜底：从 x-timing-ttft-attempts 中读取生效端点的尝试耗时
                const attemptsRaw = headers['x-timing-ttft-attempts'] || headers['X-Timing-Ttft-Attempts'];
                if (attemptsRaw) {
                    try {
                        const arr = JSON.parse(atob(String(attemptsRaw)));
                        if (Array.isArray(arr)) {
                            const eff = arr.find((a: any) => a.is_effective);
                            if (eff && typeof eff.duration_s === 'number' && eff.duration_s > 0) {
                                return eff.duration_s * 1000;
                            }
                        }
                    } catch {}
                }
            }
        } catch {}
    }

    // 2. 兜底从 response_body 的 _timing 中读取
    if (log.response_body) {
        try {
            const body = JSON.parse(log.response_body);
            if (body && typeof body === 'object' && body._timing) {
                const t = body._timing;
                if (typeof t.ttft_ms === 'number' && t.ttft_ms > 0) return t.ttft_ms;
                if (typeof t.ttft_s === 'number' && t.ttft_s > 0) return t.ttft_s * 1000;
                // 兜底 attempts
                if (Array.isArray(t.ttft_attempts)) {
                    const eff = t.ttft_attempts.find((a: any) => a.is_effective);
                    if (eff && typeof eff.duration_s === 'number' && eff.duration_s > 0) {
                        return eff.duration_s * 1000;
                    }
                }
            }
        } catch {}
    }

    return undefined;
}

const LogTable: React.FC<LogTableProps> = ({
    logs,
    loading,
    onLogClick,
    t
}) => {
    const { accounts } = useAccountStore();
    const accountQosByEmail = useMemo(() => {
        const map = new Map<string, 'suspicious' | 'jailed' | 'probing' | 'half_open'>();
        accounts.forEach(acc => {
            if (acc.email && acc.qos_status) {
                map.set(acc.email.toLowerCase(), acc.qos_status);
            }
        });
        return map;
    }, [accounts]);
    const containerRef = useRef<HTMLDivElement>(null);
    const [containerWidth, setContainerWidth] = useState<number>(0);
    const isResizingRef = useRef(false);

    const [colWidths, setColWidths] = useState<ColumnWidths>(() => {
        try {
            const saved = localStorage.getItem('proxy_log_col_widths_v8');
            if (saved) {
                return { ...DEFAULT_COL_WIDTHS, ...JSON.parse(saved) };
            }
        } catch {}
        return DEFAULT_COL_WIDTHS;
    });

    // 监听容器宽度，用于支持大屏自适应底宽
    useEffect(() => {
        if (!containerRef.current) return;
        const ro = new ResizeObserver((entries) => {
            for (const entry of entries) {
                const w = Math.floor(entry.contentRect.width);
                if (w > 0) {
                    setContainerWidth(w);
                }
            }
        });
        ro.observe(containerRef.current);
        return () => ro.disconnect();
    }, []);

    // 严格 1:1 独立线性列宽拖拽：以当前列状态设定宽为基准，鼠标移动多少像素该列就变大/变小多少，绝不联动篡改相邻列
    const handleResizeStart = (colKey: keyof ColumnWidths, e: React.MouseEvent) => {
        e.preventDefault();
        e.stopPropagation();
        isResizingRef.current = true;

        const startX = e.clientX;
        const startWidth = colWidths[colKey];
        const minW = MIN_COL_WIDTHS[colKey];

        const originalCursor = document.body.style.cursor;
        const originalUserSelect = document.body.style.userSelect;
        document.body.style.cursor = 'col-resize';
        document.body.style.userSelect = 'none';

        const onMouseMove = (moveEvent: MouseEvent) => {
            const diff = moveEvent.clientX - startX;
            const newWidth = Math.max(minW, startWidth + diff);
            setColWidths((prev) => ({
                ...prev,
                [colKey]: newWidth,
            }));
        };

        const onMouseUp = (upEvent: MouseEvent) => {
            document.body.style.cursor = originalCursor;
            document.body.style.userSelect = originalUserSelect;
            window.removeEventListener('mousemove', onMouseMove);
            window.removeEventListener('mouseup', onMouseUp);

            const finalDiff = upEvent.clientX - startX;
            const finalWidth = Math.max(minW, startWidth + finalDiff);
            setColWidths((prev) => {
                const next = { ...prev, [colKey]: finalWidth };
                try {
                    localStorage.setItem('proxy_log_col_widths_v8', JSON.stringify(next));
                } catch {}
                return next;
            });

            isResizingRef.current = false;
        };

        window.addEventListener('mousemove', onMouseMove);
        window.addEventListener('mouseup', onMouseUp);
    };

    const handleResetColumnWidth = (colKey: keyof ColumnWidths, e: React.MouseEvent) => {
        e.preventDefault();
        e.stopPropagation();
        setColWidths((prev) => {
            const next = { ...prev, [colKey]: DEFAULT_COL_WIDTHS[colKey] };
            try {
                localStorage.setItem('proxy_log_col_widths_v8', JSON.stringify(next));
            } catch {}
            return next;
        });
    };

    const totalTableWidth = useMemo(() => {
        return Object.values(colWidths).reduce((a, b) => a + b, 0);
    }, [colWidths]);

    return (
        <div
            ref={containerRef}
            className="flex-1 overflow-y-auto overflow-x-auto bg-white dark:bg-base-100 relative scrollbar-thin"
        >
            <table
                className="table table-sm border-separate border-spacing-0 w-full"
                style={{
                    minWidth: `${totalTableWidth}px`,
                    width: `${Math.max(containerWidth || 0, totalTableWidth)}px`,
                    tableLayout: 'fixed'
                }}
            >
                <colgroup>
                    <col style={{ width: `${colWidths.status}px`, minWidth: `${MIN_COL_WIDTHS.status}px` }} />
                    <col style={{ width: `${colWidths.endpoint}px`, minWidth: `${MIN_COL_WIDTHS.endpoint}px` }} />
                    <col style={{ width: `${colWidths.model}px`, minWidth: `${MIN_COL_WIDTHS.model}px` }} />
                    <col style={{ width: `${colWidths.account}px`, minWidth: `${MIN_COL_WIDTHS.account}px` }} />
                    <col style={{ width: `${colWidths.usage}px`, minWidth: `${MIN_COL_WIDTHS.usage}px` }} />
                    <col style={{ width: `${colWidths.duration}px`, minWidth: `${MIN_COL_WIDTHS.duration}px` }} />
                    <col style={{ width: `${colWidths.time}px`, minWidth: `${MIN_COL_WIDTHS.time}px` }} />
                </colgroup>
                <thead className="bg-gray-100/90 dark:bg-base-200 text-gray-700 dark:text-gray-200 text-xs font-semibold sticky top-0 z-10 backdrop-blur-sm border-b border-gray-200 dark:border-base-300">
                    <tr>
                        {/* 1. 状态 */}
                        <th style={{ width: `${colWidths.status}px`, maxWidth: `${colWidths.status}px` }} className="py-2.5 px-3 relative select-none overflow-hidden box-border">
                            <div className="truncate font-semibold">{t('monitor.table.status', '状态')}</div>
                            <div
                                onMouseDown={(e) => handleResizeStart('status', e)}
                                onDoubleClick={(e) => handleResetColumnWidth('status', e)}
                                className="absolute right-0 top-0 bottom-0 w-2.5 cursor-col-resize flex items-center justify-end z-20 group/handle"
                                title={t('monitor.table.resize_handle_tooltip', '拖拽调整宽度 / 双击恢复默认')}
                            >
                                <div className="w-[2px] h-full transition-colors group-hover/handle:bg-blue-500 active:bg-blue-600 bg-transparent" />
                            </div>
                        </th>
                        {/* 2. 端点与协议 */}
                        <th style={{ width: `${colWidths.endpoint}px`, maxWidth: `${colWidths.endpoint}px` }} className="py-2.5 px-3 relative select-none overflow-hidden box-border">
                            <div className="truncate font-semibold">{t('monitor.table.endpoint', '端点 / 协议')}</div>
                            <div
                                onMouseDown={(e) => handleResizeStart('endpoint', e)}
                                onDoubleClick={(e) => handleResetColumnWidth('endpoint', e)}
                                className="absolute right-0 top-0 bottom-0 w-2.5 cursor-col-resize flex items-center justify-end z-20 group/handle"
                                title={t('monitor.table.resize_handle_tooltip', '拖拽调整宽度 / 双击恢复默认')}
                            >
                                <div className="w-[2px] h-full transition-colors group-hover/handle:bg-blue-500 active:bg-blue-600 bg-transparent" />
                            </div>
                        </th>
                        {/* 3. 模型 */}
                        <th style={{ width: `${colWidths.model}px`, maxWidth: `${colWidths.model}px` }} className="py-2.5 px-3 relative select-none overflow-hidden box-border">
                            <div className="truncate font-semibold">{t('monitor.table.model_routing', '模型 (请求 / 真实)')}</div>
                            <div
                                onMouseDown={(e) => handleResizeStart('model', e)}
                                onDoubleClick={(e) => handleResetColumnWidth('model', e)}
                                className="absolute right-0 top-0 bottom-0 w-2.5 cursor-col-resize flex items-center justify-end z-20 group/handle"
                                title={t('monitor.table.resize_handle_tooltip', '拖拽调整宽度 / 双击恢复默认')}
                            >
                                <div className="w-[2px] h-full transition-colors group-hover/handle:bg-blue-500 active:bg-blue-600 bg-transparent" />
                            </div>
                        </th>
                        {/* 4. 账号 */}
                        <th style={{ width: `${colWidths.account}px`, maxWidth: `${colWidths.account}px` }} className="py-2.5 px-3 relative select-none overflow-hidden box-border">
                            <div className="truncate font-semibold">{t('monitor.table.account', '服务账号')}</div>
                            <div
                                onMouseDown={(e) => handleResizeStart('account', e)}
                                onDoubleClick={(e) => handleResetColumnWidth('account', e)}
                                className="absolute right-0 top-0 bottom-0 w-2.5 cursor-col-resize flex items-center justify-end z-20 group/handle"
                                title={t('monitor.table.resize_handle_tooltip', '拖拽调整宽度 / 双击恢复默认')}
                            >
                                <div className="w-[2px] h-full transition-colors group-hover/handle:bg-blue-500 active:bg-blue-600 bg-transparent" />
                            </div>
                        </th>
                        {/* 5. Token 消耗 */}
                        <th style={{ width: `${colWidths.usage}px`, maxWidth: `${colWidths.usage}px` }} className="text-right py-2.5 px-3 relative select-none overflow-hidden box-border">
                            <div className="truncate font-semibold">{t('monitor.table.usage', 'Token 消耗')}</div>
                            <div
                                onMouseDown={(e) => handleResizeStart('usage', e)}
                                onDoubleClick={(e) => handleResetColumnWidth('usage', e)}
                                className="absolute right-0 top-0 bottom-0 w-2.5 cursor-col-resize flex items-center justify-end z-20 group/handle"
                                title={t('monitor.table.resize_handle_tooltip', '拖拽调整宽度 / 双击恢复默认')}
                            >
                                <div className="w-[2px] h-full transition-colors group-hover/handle:bg-blue-500 active:bg-blue-600 bg-transparent" />
                            </div>
                        </th>
                        {/* 6. 耗时 / 首字 (独立列) */}
                        <th style={{ width: `${colWidths.duration}px`, maxWidth: `${colWidths.duration}px` }} className="text-right py-2.5 px-3 relative select-none overflow-hidden box-border">
                            <div className="truncate font-semibold">{t('monitor.table.duration_ttft', '耗时 / 首字')}</div>
                            <div
                                onMouseDown={(e) => handleResizeStart('duration', e)}
                                onDoubleClick={(e) => handleResetColumnWidth('duration', e)}
                                className="absolute right-0 top-0 bottom-0 w-2.5 cursor-col-resize flex items-center justify-end z-20 group/handle"
                                title={t('monitor.table.resize_handle_tooltip', '拖拽调整宽度 / 双击恢复默认')}
                            >
                                <div className="w-[2px] h-full transition-colors group-hover/handle:bg-blue-500 active:bg-blue-600 bg-transparent" />
                            </div>
                        </th>
                        {/* 7. 时间 */}
                        <th style={{ width: `${colWidths.time}px`, maxWidth: `${colWidths.time}px` }} className="text-right py-2.5 px-3 relative select-none overflow-hidden box-border">
                            <div className="truncate font-semibold">{t('monitor.table.time', '时间')}</div>
                            <div
                                onMouseDown={(e) => handleResizeStart('time', e)}
                                onDoubleClick={(e) => handleResetColumnWidth('time', e)}
                                className="absolute right-0 top-0 bottom-0 w-2.5 cursor-col-resize flex items-center justify-end z-20 group/handle"
                                title={t('monitor.table.resize_handle_tooltip', '拖拽调整宽度 / 双击恢复默认')}
                            >
                                <div className="w-[2px] h-full transition-colors group-hover/handle:bg-blue-500 active:bg-blue-600 bg-transparent" />
                            </div>
                        </th>
                    </tr>
                </thead>
                <tbody className="divide-y divide-gray-100 dark:divide-base-200">
                    {logs.map((log) => {
                        const isSuccess = log.status >= 200 && log.status < 400;
                        const durationText = log.duration >= 1000 ? `${(log.duration / 1000).toFixed(2)}s` : `${log.duration}ms`;
                        const durationColor = log.duration > 5000 
                            ? 'text-amber-600 dark:text-amber-400 font-medium' 
                            : log.duration > 1500 
                                ? 'text-sky-600 dark:text-sky-400' 
                                : 'text-gray-500 dark:text-gray-400';

                        const protocolLower = (log.protocol || '').toLowerCase();
                        const realModel = log.upstream_model || log.mapped_model || log.model || '-';
                        const isModelChanged = (log.upstream_model && log.upstream_model !== log.model) || (log.mapped_model && log.model !== log.mapped_model);

                        return (
                            <tr
                                key={log.id}
                                className="hover:bg-blue-50/70 dark:hover:bg-base-200/70 cursor-pointer transition-colors"
                                onClick={() => onLogClick(log)}
                            >
                                {/* 1. 状态 */}
                                <td style={{ width: `${colWidths.status}px`, maxWidth: `${colWidths.status}px` }} className="py-2.5 px-3 align-middle overflow-hidden box-border">
                                    <span className={`inline-flex items-center gap-1 px-1.5 py-0.5 rounded text-[11px] font-bold border ${
                                        isSuccess
                                            ? 'bg-emerald-50 text-emerald-700 border-emerald-200/80 dark:bg-emerald-950/40 dark:text-emerald-300 dark:border-emerald-800/60'
                                            : 'bg-rose-50 text-rose-700 border-rose-200/80 dark:bg-rose-950/40 dark:text-rose-300 dark:border-rose-800/60'
                                    }`}>
                                        <span className={`w-1.5 h-1.5 rounded-full ${isSuccess ? 'bg-emerald-500' : 'bg-rose-500'}`} />
                                        {log.status}
                                    </span>
                                </td>

                                {/* 2. 端点与协议 (Method + Protocol + URL) */}
                                <td style={{ width: `${colWidths.endpoint}px`, maxWidth: `${colWidths.endpoint}px` }} className="py-2.5 px-3 align-middle overflow-hidden box-border">
                                    <div className="flex flex-col gap-1 min-w-0 max-w-full overflow-hidden">
                                        <div className="flex items-center gap-1.5 flex-wrap">
                                            {/* HTTP Method */}
                                             <span className="px-1.5 py-0.2 rounded text-[10px] font-bold font-mono tracking-wider bg-gray-100 text-gray-700 dark:bg-base-300 dark:text-gray-300 border border-gray-200/80 dark:border-base-200">
                                                {log.method || 'POST'}
                                            </span>
                                            {/* Protocol Badge */}
                                            {log.protocol ? (
                                                <span className={`px-1.5 py-0.2 rounded text-[10px] font-bold border tracking-tight ${
                                                    protocolLower === 'openai'
                                                        ? 'bg-emerald-50 text-emerald-700 border-emerald-200 dark:bg-emerald-950/50 dark:text-emerald-300 dark:border-emerald-800/80'
                                                        : protocolLower === 'anthropic'
                                                            ? 'bg-amber-50 text-amber-800 border-amber-200 dark:bg-amber-950/50 dark:text-amber-300 dark:border-amber-800/80'
                                                            : protocolLower === 'gemini'
                                                                ? 'bg-blue-50 text-blue-700 border-blue-200 dark:bg-blue-950/50 dark:text-blue-300 dark:border-blue-800/80'
                                                                : protocolLower === 'ban_probe'
                                                                    ? 'bg-rose-50 text-rose-700 border-rose-200 dark:bg-rose-950/50 dark:text-rose-300 dark:border-rose-800/80'
                                                                    : protocolLower === 'warmup'
                                                                        ? 'bg-orange-50 text-orange-700 border-orange-200 dark:bg-orange-950/50 dark:text-orange-300 dark:border-orange-800/80'
                                                                        : 'bg-gray-100 text-gray-700 border-gray-200 dark:bg-base-300 dark:text-gray-300 dark:border-base-200'
                                                }`}>
                                                    {protocolLower === 'openai' ? 'OpenAI' : protocolLower === 'anthropic' ? 'Claude' : protocolLower === 'gemini' ? 'Gemini' : protocolLower === 'ban_probe' ? t('monitor.protocols.ban_probe', 'Ban Probe') : protocolLower === 'warmup' ? t('monitor.protocols.warmup', 'Warmup') : log.protocol}
                                                </span>
                                            ) : null}
                                        </div>
                                        {/* URL Path */}
                                        <div 
                                            className="text-gray-600 dark:text-gray-400 text-xs font-mono truncate max-w-full"
                                            title={log.url || ''}
                                        >
                                            {log.url || '-'}
                                        </div>
                                    </div>
                                </td>

                                {/* 3. 模型 (请求模型与真实使用模型) */}
                                <td style={{ width: `${colWidths.model}px`, maxWidth: `${colWidths.model}px` }} className="py-2.5 px-3 align-middle overflow-hidden box-border">
                                     <div className="flex flex-col gap-0.5 min-w-0 max-w-full overflow-hidden">
                                         <div className="flex items-center gap-1.5 min-w-0 max-w-full" title={`${t('monitor.table.req_model_title', '请求模型')}: ${log.model || '-'}`}>
                                             <span className="text-[10px] font-semibold text-gray-400 dark:text-gray-500 shrink-0 select-none">
                                                 {t('monitor.table.req_prefix', '请求:')}
                                             </span>
                                             <span className="text-gray-900 dark:text-gray-100 font-semibold text-xs font-mono truncate">
                                                 {log.model || '-'}
                                             </span>
                                         </div>
                                         <div className="flex items-center gap-1.5 min-w-0 max-w-full" title={`${t('monitor.table.real_model_title', '真实使用模型')}: ${realModel}`}>
                                             <span className={`text-[10px] font-semibold shrink-0 select-none ${
                                                 isModelChanged ? 'text-sky-500 dark:text-sky-400 font-bold' : 'text-gray-400 dark:text-gray-500'
                                             }`}>
                                                 {t('monitor.table.real_prefix', '真实:')}
                                             </span>
                                             <span className={`text-[11px] font-mono truncate ${
                                                 isModelChanged ? 'text-sky-600 dark:text-sky-400 font-semibold' : 'text-gray-600 dark:text-gray-300'
                                             }`}>
                                                 {realModel}
                                             </span>
                                         </div>
                                     </div>
                                 </td>

                                 {/* 4. 服务账号 */}
                                 <td style={{ width: `${colWidths.account}px`, maxWidth: `${colWidths.account}px` }} className="py-2.5 px-3 align-middle overflow-hidden box-border">
                                     <div className="flex items-center gap-1.5 min-w-0 max-w-full">
                                         {(() => {
                                             const account = log.account_email || '';
                                             // 网关前置拦截哨兵：账号列直接亮出拒答原因（亮色/暗色都保持高对比度）
                                             if (account === 'model not exist' || account === 'no quota accounts') {
                                                 const isNotFound = account === 'model not exist';
                                                 return (
                                                     <span
                                                         className={`inline-flex items-center font-sans font-semibold text-[11px] leading-none px-1.5 py-0.5 rounded whitespace-nowrap ${isNotFound
                                                             ? 'text-rose-600 bg-rose-50 dark:text-rose-300 dark:bg-rose-500/15'
                                                             : 'text-amber-600 bg-amber-50 dark:text-amber-300 dark:bg-amber-500/15'}`}
                                                         title={account}
                                                     >
                                                         {account}
                                                     </span>
                                                 );
                                             }
                                             return (
                                                 <span className="text-gray-700 dark:text-gray-300 font-sans text-xs truncate" title={account}>
                                                     {account ? account.replace(/(.{3}).*(@.*)/, '$1***$2') : '-'}
                                                 </span>
                                             );
                                         })()}
                                         {log.account_email && accountQosByEmail.get(log.account_email.toLowerCase()) && (
                                             <QoSStatusBadge
                                                 status={accountQosByEmail.get(log.account_email.toLowerCase())}
                                                 size="xs"
                                             />
                                         )}
                                     </div>
                                 </td>

                                 {/* 5. Token 消耗 (入 / 出 / 缓) */}
                                 <td style={{ width: `${colWidths.usage}px`, maxWidth: `${colWidths.usage}px` }} className="text-right py-2.5 px-3 align-middle overflow-hidden box-border">
                                     {log.input_tokens != null || log.output_tokens != null ? (() => {
                                        const totalIn = (log.cached_tokens && log.cached_tokens > (log.input_tokens || 0))
                                            ? (log.input_tokens || 0) + log.cached_tokens
                                            : (log.input_tokens || 0);
                                        const hasCache = !!(log.cached_tokens && log.cached_tokens > 0);
                                        const hitRate = (hasCache && totalIn > 0)
                                            ? Math.min(100, Math.max(0, (log.cached_tokens! / totalIn) * 100))
                                            : 0;
                                        const hitRateText = totalIn > 0 && hasCache
                                            ? (hitRate >= 100 ? '100%' : (hitRate % 1 === 0 ? `${hitRate.toFixed(0)}%` : `${hitRate.toFixed(1)}%`))
                                            : '';
                                        return (
                                            <div className="flex flex-col items-end gap-0.5 overflow-hidden max-w-full">
                                                <div className="text-gray-800 dark:text-gray-200 text-xs font-mono font-medium truncate max-w-full">
                                                    <span title={`${t('monitor.table.in_tokens_title', '输入 Token')}: ${totalIn}`}>{t('monitor.table.in_prefix', '入:')} {formatCompactNumber(totalIn)}</span>
                                                    <span className="mx-1 text-gray-300 dark:text-gray-600">|</span>
                                                    <span title={`${t('monitor.table.out_tokens_title', '输出 Token')}: ${log.output_tokens || 0}`}>{t('monitor.table.out_prefix', '出:')} {formatCompactNumber(log.output_tokens || 0)}</span>
                                                </div>
                                                {hasCache ? (
                                                    <div
                                                        className="inline-flex items-center gap-1 text-[11px] font-semibold text-emerald-600 dark:text-emerald-400 font-mono truncate max-w-full"
                                                        title={`${t('token_stats.cached', 'Cache')}: ${log.cached_tokens?.toLocaleString()}${hitRateText ? ` (${hitRateText})` : ''}`}
                                                    >
                                                        <span>⚡ {t('monitor.table.cache_prefix', '缓:')}</span>
                                                        <span>{formatCompactNumber(log.cached_tokens!)}</span>
                                                        {hitRateText && <span className="opacity-80">({hitRateText})</span>}
                                                    </div>
                                                ) : (
                                                    <div className="text-[11px] text-gray-400 dark:text-gray-500 font-mono truncate max-w-full">
                                                        <span>{t('monitor.table.cache_prefix', '缓:')} 0</span>
                                                    </div>
                                                )}
                                            </div>
                                        );
                                    })() : (
                                        <span className="text-gray-400 text-xs">-</span>
                                    )}
                                </td>

                                {/* 6. 耗时与首字 (独立列) */}
                                <td style={{ width: `${colWidths.duration}px`, maxWidth: `${colWidths.duration}px` }} className="text-right py-2.5 px-3 align-middle overflow-hidden box-border">
                                    {(() => {
                                        const ttftMs = extractTtft(log);
                                        const ttftText = ttftMs != null 
                                            ? (ttftMs >= 1000 ? `${(ttftMs / 1000).toFixed(2)}s` : `${Math.round(ttftMs)}ms`)
                                            : null;
                                        return (
                                            <div 
                                                className="flex flex-col items-end gap-0.5 overflow-hidden max-w-full"
                                                title={`${t('monitor.table.total_duration_title', '总耗时')}: ${durationText}${ttftText ? ` | ${t('monitor.table.ttft_title', '首字延迟 (TTFT)')}: ${ttftText}` : ''}`}
                                            >
                                                <span className={`text-xs font-mono font-medium truncate max-w-full ${durationColor}`}>
                                                    {durationText}
                                                </span>
                                                {ttftText ? (
                                                    <span className="text-[11px] font-mono text-sky-600 dark:text-sky-400 truncate max-w-full">
                                                        {t('monitor.table.ttft_prefix', '首字:')} {ttftText}
                                                    </span>
                                                ) : (
                                                    <span className="text-[11px] font-mono text-gray-400 dark:text-gray-500 truncate max-w-full">
                                                        {t('monitor.table.ttft_prefix', '首字:')} -
                                                    </span>
                                                )}
                                            </div>
                                        );
                                    })()}
                                </td>

                                {/* 7. 时间 */}
                                <td style={{ width: `${colWidths.time}px`, maxWidth: `${colWidths.time}px` }} className="text-right py-2.5 px-3 align-middle overflow-hidden box-border">
                                    <div className="text-gray-500 dark:text-gray-400 text-xs font-mono truncate max-w-full">
                                        {new Date(log.timestamp).toLocaleTimeString()}
                                    </div>
                                </td>
                            </tr>
                        );
                    })}
                </tbody>
            </table>

            {/* Loading indicator */}
            {loading && (
                <div className="flex items-center justify-center p-4 bg-white dark:bg-base-100">
                    <div className="loading loading-spinner loading-md text-blue-600"></div>
                    <span className="ml-3 text-sm text-gray-500 dark:text-gray-400">{t('common.loading')}</span>
                </div>
            )}

            {/* Empty state */}
            {!loading && logs.length === 0 && (
                <div className="flex items-center justify-center p-8 text-gray-400 dark:text-gray-500 text-sm">
                    {t('monitor.table.empty') || '暂无请求记录'}
                </div>
            )}
        </div>
    );
};





export const ProxyMonitor: React.FC<ProxyMonitorProps> = ({ className }) => {
    const { t } = useTranslation();
    const [logs, setLogs] = useState<ProxyRequestLog[]>([]);
    const [stats, setStats] = useState<ProxyStats>({ total_requests: 0, success_count: 0, error_count: 0 });
    const [filter, setFilter] = useState('');
    const [accountFilter, setAccountFilter] = useState('');
    // [FIX] 使用 ref 存储最新的筛选条件，避免 setInterval 闭包问题
    const filterRef = useRef(filter);
    const accountFilterRef = useRef(accountFilter);
    const currentPageRef = useRef(1);
    const globalFilterInputRef = useRef<HTMLInputElement>(null);
    const [selectedLog, setSelectedLog] = useState<ProxyRequestLog | null>(null);
    const [isLoggingEnabled, setIsLoggingEnabled] = useState(false);
    const [captureHealthLogs, setCaptureHealthLogs] = useState(false);
    const [internalErrorLogPath, setInternalErrorLogPath] = useState('');
    const [isClearConfirmOpen, setIsClearConfirmOpen] = useState(false);

    // 日志存储与维护配置状态
    const [showLogSettings, setShowLogSettings] = useState(false);
    const [appConfig, setAppConfig] = useState<AppConfig | null>(null);
    const [isSavingConfig, setIsSavingConfig] = useState(false);
    const [saveSuccess, setSaveSuccess] = useState(false);
    const [isClearCacheModalOpen, setIsClearCacheModalOpen] = useState(false);
    const [cacheClearedSuccess, setCacheClearedSuccess] = useState(false);
    const [dbDiskSizeBytes, setDbDiskSizeBytes] = useState<number | null>(null);
    const [errorLogDiskSizeBytes, setErrorLogDiskSizeBytes] = useState<number | null>(null);

    const fetchDbDiskSize = useCallback(async () => {
        try {
            const bytes = await invoke<number>('get_proxy_db_disk_size');
            setDbDiskSizeBytes(bytes);
        } catch (e) {
            console.error('Failed to get proxy db disk size', e);
        }
    }, []);

    const fetchErrorLogDiskSize = useCallback(async () => {
        try {
            const bytes = await invoke<number>('get_internal_error_log_disk_size');
            setErrorLogDiskSizeBytes(bytes);
        } catch (e) {
            console.error('Failed to get internal error log disk size', e);
        }
    }, []);

    useEffect(() => {
        if (showLogSettings) {
            fetchDbDiskSize();
            fetchErrorLogDiskSize();
        }
    }, [showLogSettings, fetchDbDiskSize, fetchErrorLogDiskSize]);

    useEffect(() => {
        invoke<string>('get_internal_error_log_path')
            .then((path) => setInternalErrorLogPath(path))
            .catch((e) => console.error('Failed to get internal error log path', e));
    }, []);

    const formatBytes = (bytes: number) => {
        if (bytes < 1024) return `${bytes} B`;
        if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
        if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
        return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
    };

    // 全局快捷键 Ctrl+F：当焦点在报文卡片之外时，聚焦主界面的全局过滤搜索框
    useEffect(() => {
        const handleGlobalKeyDown = (e: KeyboardEvent) => {
            if ((e.ctrlKey || e.metaKey) && (e.key === 'f' || e.key === 'F')) {
                const activeEl = document.activeElement;
                if (activeEl && activeEl.closest('.payload-viewer-card')) {
                    return;
                }
                e.preventDefault();
                globalFilterInputRef.current?.focus();
                globalFilterInputRef.current?.select();
            }
        };
        window.addEventListener('keydown', handleGlobalKeyDown);
        return () => window.removeEventListener('keydown', handleGlobalKeyDown);
    }, []);


    const { accounts, fetchAccounts } = useAccountStore();

    // Pagination state
    const PAGE_SIZE_OPTIONS = [50, 100, 200, 500];
    const [pageSize, setPageSize] = useState(100);
    const [currentPage, setCurrentPage] = useState(1);
    const [totalCount, setTotalCount] = useState(0);
    const [loading, setLoading] = useState(false);
    const [loadingDetail, setLoadingDetail] = useState(false);

    const uniqueAccounts = useMemo(() => {
        const emailSet = new Set<string>();
        logs.forEach(log => {
            if (log.account_email) {
                emailSet.add(log.account_email);
            }
        });
        accounts.forEach(acc => {
            emailSet.add(acc.email);
        });
        return Array.from(emailSet).sort();
    }, [logs, accounts]);

    const loadData = async (page = 1, searchFilter = filter, accountEmailFilter = accountFilter) => {
        if (loading) return;
        setLoading(true);

        try {
            // Add timeout control (10 seconds)
            const timeoutPromise = new Promise((_, reject) =>
                setTimeout(() => reject(new Error('Request timeout')), 10000)
            );

            const config = await Promise.race([
                invoke<AppConfig>('load_config'),
                timeoutPromise
            ]) as AppConfig;

            if (config && config.proxy) {
                setAppConfig(config);
                setIsLoggingEnabled(config.proxy.enable_logging);
                const healthLogsEnabled = !!config.proxy.capture_health_logs;
                setCaptureHealthLogs(healthLogsEnabled);
                await invoke('set_proxy_monitor_enabled', { enabled: config.proxy.enable_logging });
                await invoke('set_proxy_capture_health_logs', { enabled: healthLogsEnabled });
            }

            const errorsOnly = searchFilter === '__ERROR__';
            const baseFilter = errorsOnly ? '' : searchFilter;
            const actualFilter = accountEmailFilter
                ? (baseFilter ? `${baseFilter} ${accountEmailFilter}` : accountEmailFilter)
                : baseFilter;

            // Get count with filter
            const count = await Promise.race([
                invoke<number>('get_proxy_logs_count_filtered', {
                    filter: actualFilter,
                    errorsOnly: errorsOnly
                }),
                timeoutPromise
            ]) as number;
            setTotalCount(count);

            // Use filtered paginated query
            const offset = (page - 1) * pageSize;
            const history = await Promise.race([
                invoke<ProxyRequestLog[]>('get_proxy_logs_filtered', {
                    filter: actualFilter,
                    errorsOnly: errorsOnly,
                    limit: pageSize,
                    offset: offset
                }),
                timeoutPromise
            ]) as ProxyRequestLog[];

            if (Array.isArray(history)) {
                setLogs(history);
                // Clear pending logs to avoid duplicates (database data is authoritative)
                pendingLogsRef.current = [];
            }

            const currentStats = await Promise.race([
                invoke<ProxyStats>('get_proxy_stats'),
                timeoutPromise
            ]) as ProxyStats;

            if (currentStats) setStats(currentStats);
            fetchAccounts();
        } catch (e: any) {
            console.error("Failed to load proxy data", e);
            if (e.message === 'Request timeout') {
                // Show timeout error to user
                console.error('Loading monitor data timeout, please try again later');
            }
        } finally {
            setLoading(false);
        }
    };

    const totalPages = Math.ceil(totalCount / pageSize);
    const pageStart = totalCount === 0 ? 0 : (currentPage - 1) * pageSize + 1;
    const pageEnd = totalCount === 0 ? 0 : Math.min(currentPage * pageSize, totalCount);

    const goToPage = (page: number) => {
        if (page >= 1 && page <= totalPages && page !== currentPage) {
            setCurrentPage(page);
            currentPageRef.current = page; // [FIX] 同步 ref
            loadData(page, filter, accountFilter);
        }
    };

    const toggleLogging = async () => {
        const newState = !isLoggingEnabled;
        try {
            const config = await invoke<AppConfig>('load_config');
            if (config && config.proxy) {
                config.proxy.enable_logging = newState;
                await invoke('save_config', { config });
                await invoke('set_proxy_monitor_enabled', { enabled: newState });
                setIsLoggingEnabled(newState);
            }
        } catch (e) {
            console.error("Failed to toggle logging", e);
        }
    };

    const toggleCaptureHealthLogs = async () => {
        const newState = !captureHealthLogs;
        try {
            const config = await invoke<AppConfig>('load_config');
            if (config && config.proxy) {
                config.proxy.capture_health_logs = newState;
                await invoke('save_config', { config });
                await invoke('set_proxy_capture_health_logs', { enabled: newState });
                setCaptureHealthLogs(newState);
                loadData(1, filter, accountFilter);
            }
        } catch (e) {
            console.error("Failed to toggle capture health logs", e);
        }
    };

    const pendingLogsRef = useRef<ProxyRequestLog[]>([]);
    const listenerSetupRef = useRef(false);
    const isMountedRef = useRef(true);

    useEffect(() => {
        isMountedRef.current = true;
        loadData();
        fetchAccounts();

        let unlistenFn: (() => void) | null = null;
        let updateTimeout: number | null = null;

        const setupListener = async () => {
            if (!isTauri()) return;
            // Prevent duplicate listener registration (React 18 StrictMode)
            if (listenerSetupRef.current) {
                console.debug('[ProxyMonitor] Listener already set up, skipping...');
                return;
            }
            listenerSetupRef.current = true;

            console.debug('[ProxyMonitor] Setting up event listener for proxy://request');
            unlistenFn = await listen<ProxyRequestLog>('proxy://request', (event) => {
                if (!isMountedRef.current) return;

                const newLog = event.payload;

                // 移除 body 以减少内存占用
                const logSummary = {
                    ...newLog,
                    request_body: undefined,
                    upstream_request_body: undefined,
                    response_body: undefined
                };

                // Check if this log already exists (deduplicate at event level)
                const alreadyExists = pendingLogsRef.current.some(log => log.id === newLog.id);
                if (alreadyExists) {
                    console.debug('[ProxyMonitor] Duplicate event ignored:', newLog.id);
                    return;
                }

                pendingLogsRef.current.push(logSummary);

                // 防抖:每 500ms 批量更新一次
                if (updateTimeout) clearTimeout(updateTimeout);
                updateTimeout = window.setTimeout(async () => {
                    if (!isMountedRef.current) return;

                    const currentPending = pendingLogsRef.current;
                    if (currentPending.length > 0) {
                        setLogs(prev => {
                            // Deduplicate by id
                            const existingIds = new Set(prev.map(log => log.id));
                            const uniqueNewLogs = currentPending.filter(log => !existingIds.has(log.id));
                            // Merge and sort by timestamp descending (newest first)
                            const merged = [...uniqueNewLogs, ...prev];
                            merged.sort((a, b) => b.timestamp - a.timestamp);
                            return merged.slice(0, 100);
                        });

                        // Fetch stats, total count and latest account QoS states from backend
                        try {
                            const [currentStats, count] = await Promise.all([
                                invoke<ProxyStats>('get_proxy_stats'),
                                invoke<number>('get_proxy_logs_count_filtered', { filter: '', errorsOnly: false }),
                                fetchAccounts()
                            ]);
                            if (isMountedRef.current) {
                                if (currentStats) setStats(currentStats);
                                setTotalCount(count);
                            }
                        } catch (e) {
                            console.error('Failed to fetch stats:', e);
                        }

                        pendingLogsRef.current = [];
                    }
                }, 500);
            });
        };
        setupListener();

        // Web 模式補強：如果不是 Tauri 環境，則啟用定時輪詢
        let pollInterval: number | null = null;
        if (!isTauri()) {
            console.debug('[ProxyMonitor] Web mode detected, starting auto-poll (10s)');
            pollInterval = window.setInterval(() => {
                if (isMountedRef.current && !loading) {
                    // [FIX] 使用 ref.current 获取最新的筛选条件
                    loadData(currentPageRef.current, filterRef.current, accountFilterRef.current);
                }
            }, 10000);
        }

        return () => {
            isMountedRef.current = false;
            listenerSetupRef.current = false;
            if (unlistenFn) unlistenFn();
            if (updateTimeout) clearTimeout(updateTimeout);
            if (pollInterval) clearInterval(pollInterval);
        };
    }, []);

    // Reload when pageSize changes
    useEffect(() => {
        setCurrentPage(1);
        loadData(1, filter, accountFilter);
    }, [pageSize]);

    // Reload when filter changes (search based on all logs)
    useEffect(() => {
        setCurrentPage(1);
        loadData(1, filter, accountFilter);
        // [FIX] 同步 ref 值，供 setInterval 使用
        filterRef.current = filter;
        accountFilterRef.current = accountFilter;
        currentPageRef.current = 1;
    }, [filter, accountFilter]);

    // Logs are already filtered and sorted by backend
    // Apply account filter and noise filter on frontend (与后端 should_skip_request_log 语义保持一致)
    const filteredLogs = useMemo(() => {
        let result = logs;
        if (!captureHealthLogs) {
            result = result.filter(log => {
                const isGetSuccess = log.method?.toUpperCase() === 'GET'
                    && typeof log.status === 'number'
                    && log.status >= 200 && log.status < 300;
                return !isGetSuccess;
            });
        }
        if (accountFilter) {
            result = result.filter(log => log.account_email === accountFilter);
        }
        return result;
    }, [logs, accountFilter, captureHealthLogs]);

    const quickFilters = [
        { label: t('monitor.filters.all'), value: '' },
        { label: 'claude', value: 'claude' },
        { label: 'flash', value: 'flash' },
        { label: 'pro', value: 'pro' },
        { label: 'agent', value: 'agent' },
        { label: t('monitor.filters.error'), value: '__ERROR__' },
        { label: t('monitor.filters.chat'), value: 'completions' },
        { label: t('monitor.filters.gemini'), value: 'gemini' },
        { label: t('monitor.filters.images'), value: 'images' }
    ];

    const clearLogs = () => {
        setIsClearConfirmOpen(true);
    };

    const executeClearLogs = async () => {
        setIsClearConfirmOpen(false);
        try {
            await invoke('clear_proxy_logs');
            setLogs([]);
            setStats({ total_requests: 0, success_count: 0, error_count: 0 });
            setTotalCount(0);
            fetchDbDiskSize();
        } catch (e) {
            console.error("Failed to clear logs", e);
        }
    };

    const updateLogRetentionField = (field: 'max_body_age_hours' | 'max_storage_gb' | 'max_rows', value: number) => {
        if (!appConfig) return;
        const currentRetention = appConfig.proxy?.log_retention || { max_body_age_hours: 24, max_storage_gb: 1.0, max_rows: 100000 };
        const safeVal = field === 'max_storage_gb'
            ? Math.max(0.1, isNaN(value) ? 1.0 : value)
            : Math.max(1, isNaN(value) ? 1 : value);
        const updated = {
            ...currentRetention,
            [field]: safeVal,
        };
        const currentExp: ExperimentalConfig = appConfig.proxy?.experimental || {
            enable_usage_scaling: true,
        };
        const updatedConfig: AppConfig = {
            ...appConfig,
            proxy: {
                ...appConfig.proxy,
                log_retention: updated,
                experimental: {
                    ...currentExp,
                }
            }
        };
        setAppConfig(updatedConfig);
    };

    const updateInternalErrorLogRetention = (value: number) => {
        if (!appConfig) return;
        const safeVal = Math.max(1, isNaN(value) ? 500 : value);
        const updatedConfig: AppConfig = {
            ...appConfig,
            proxy: {
                ...appConfig.proxy,
                internal_error_log_retention: {
                    max_storage_mb: safeVal,
                },
            }
        };
        setAppConfig(updatedConfig);
    };

    const updateExperimentalField = (field: 'payload_storage_mode' | 'thinking_retention_days', value: any) => {
        if (!appConfig) return;
        const currentExp: ExperimentalConfig = appConfig.proxy?.experimental || {
            enable_usage_scaling: true,
        };
        const updatedExp: ExperimentalConfig = {
            ...currentExp,
            [field]: field === 'thinking_retention_days' ? Math.max(1, parseInt(value) || 15) : value,
        };
        const updatedConfig: AppConfig = {
            ...appConfig,
            proxy: {
                ...appConfig.proxy,
                experimental: updatedExp
            }
        };
        setAppConfig(updatedConfig);
    };

    const handleSaveLogSettings = async () => {
        if (!appConfig) return;
        setIsSavingConfig(true);
        try {
            await invoke('save_config', { config: appConfig });
            setSaveSuccess(true);
            fetchDbDiskSize();
            fetchErrorLogDiskSize();
            setTimeout(() => setSaveSuccess(false), 2000);
        } catch (e) {
            console.error('Failed to save log settings', e);
        } finally {
            setIsSavingConfig(false);
        }
    };

    const handleClearCache = async () => {
        setIsClearCacheModalOpen(false);
        try {
            await invoke('clear_log_cache');
            setCacheClearedSuccess(true);
            setTimeout(() => setCacheClearedSuccess(false), 2500);
        } catch (e) {
            console.error('Failed to clear log cache', e);
        }
    };

    return (
        <div className={`flex flex-col bg-white dark:bg-base-100 rounded-xl shadow-xs border border-gray-200/80 dark:border-base-200 overflow-hidden ${className || 'flex-1'}`}>
            <div className="p-3.5 border-b border-gray-200/80 dark:border-base-200 space-y-3 bg-gray-50/80 dark:bg-base-200">
                <div className="flex items-center gap-3">
                    <button
                        onClick={toggleLogging}
                        className={`btn btn-sm gap-2 px-3 border font-semibold rounded-lg transition-all ${isLoggingEnabled
                            ? 'bg-rose-50 dark:bg-rose-950/40 border-rose-300 dark:border-rose-800 text-rose-600 dark:text-rose-400 hover:bg-rose-100 dark:hover:bg-rose-900/40 shadow-xs'
                            : 'bg-white dark:bg-base-200 border-gray-300 dark:border-base-300 text-gray-700 dark:text-gray-200 hover:bg-gray-50 dark:hover:bg-base-300/80 shadow-2xs'
                            }`}
                    >
                        <div className={`w-2 h-2 rounded-full ${isLoggingEnabled ? 'bg-rose-500 animate-pulse' : 'bg-gray-400'}`} />
                        {isLoggingEnabled ? t('monitor.logging_status.active') : t('monitor.logging_status.paused')}
                    </button>

                    <div className="relative flex-1">
                        <Search className="absolute left-2.5 top-2 text-gray-400" size={14} />
                        <input
                            ref={globalFilterInputRef}
                            type="text"
                            placeholder={t('monitor.filters.placeholder')}
                            className="input input-sm input-bordered w-full pl-9 text-xs bg-white dark:bg-base-200 border-gray-300 dark:border-base-300 text-gray-900 dark:text-white focus:border-blue-500 focus:ring-1 focus:ring-blue-500/30"
                            value={filter}
                            onChange={(e) => setFilter(e.target.value)}
                        />
                    </div>

                    <div className="relative">
                        <User className="absolute left-2.5 top-2 text-gray-400 z-10" size={14} />
                        <select
                            className="select select-sm select-bordered pl-8 text-xs min-w-[140px] max-w-[220px] bg-white dark:bg-base-200 border-gray-300 dark:border-base-300 text-gray-900 dark:text-white focus:border-blue-500 focus:ring-1 focus:ring-blue-500/30"
                            value={accountFilter}
                            onChange={(e) => setAccountFilter(e.target.value)}
                            title={t('monitor.filters.by_account')}
                        >
                            <option value="">{t('monitor.filters.all_accounts')}</option>
                            {uniqueAccounts.map(email => (
                                <option key={email} value={email} title={email}>
                                    {email}
                                </option>
                            ))}
                        </select>
                    </div>

                    <div className="hidden lg:flex items-center gap-3 text-xs font-bold font-mono">
                        <span className="text-blue-600 dark:text-blue-400">
                            {formatCompactNumber(stats.total_requests)} <span className="font-sans font-semibold text-[11px] text-gray-500 dark:text-gray-400">{t('monitor.stats.total')}</span>
                        </span>
                        <span className="text-emerald-600 dark:text-emerald-400">
                            {formatCompactNumber(stats.success_count)} <span className="font-sans font-semibold text-[11px] text-gray-500 dark:text-gray-400">{t('monitor.stats.ok')}</span>
                        </span>
                        <span className="text-rose-600 dark:text-rose-400">
                            {formatCompactNumber(stats.error_count)} <span className="font-sans font-semibold text-[11px] text-gray-500 dark:text-gray-400">{t('monitor.stats.err')}</span>
                        </span>
                    </div>

                    <button onClick={() => loadData(currentPage, filter)} className="btn btn-sm btn-ghost text-gray-400 hover:text-gray-600 dark:hover:text-gray-200" title={t('common.refresh')}>
                        <RefreshCw size={16} className={loading ? 'animate-spin' : ''} />
                    </button>
                    <button
                        onClick={() => setShowLogSettings(!showLogSettings)}
                        className={`btn btn-sm btn-ghost ${
                            showLogSettings
                                ? 'text-blue-600 dark:text-blue-400 bg-blue-50 dark:bg-blue-900/30'
                                : 'text-gray-400 hover:text-gray-600 dark:hover:text-gray-200'
                        }`}
                        title={t('common.settings', { defaultValue: '设置' })}
                        aria-label={t('common.settings', { defaultValue: '设置' })}
                    >
                        <Settings size={16} />
                    </button>
                    <button onClick={clearLogs} className="btn btn-sm btn-ghost text-gray-400 hover:text-gray-600 dark:hover:text-gray-200" title={t('monitor.actions.clear_all_requests', { defaultValue: '清空请求日志' })}>
                        <Trash2 size={16} />
                    </button>
                </div>

                <div className="flex flex-wrap items-center gap-2">
                    <span className="text-xs font-bold text-gray-700 dark:text-gray-200 uppercase tracking-wide">{t('monitor.filters.quick_filters')}</span>
                    {quickFilters.map(q => (
                        <button
                            key={q.label}
                            onClick={() => setFilter(q.value)}
                            className={`px-3 py-0.5 rounded-full text-xs font-semibold border transition-all ${
                                filter === q.value
                                    ? 'bg-blue-600 text-white border-blue-600 shadow-xs'
                                    : 'bg-white dark:bg-base-200 text-gray-700 dark:text-gray-200 border-gray-300 dark:border-base-300 hover:bg-gray-100 dark:hover:bg-base-300/80 hover:text-gray-900 dark:hover:text-white shadow-2xs'
                            }`}
                        >
                            {q.label}
                        </button>
                    ))}
                    <button
                        onClick={toggleCaptureHealthLogs}
                        className={`px-3 py-0.5 rounded-full text-xs font-semibold border transition-all flex items-center gap-1.5 ${
                            captureHealthLogs
                                ? 'bg-emerald-600 text-white border-emerald-600 shadow-xs'
                                : 'bg-white dark:bg-base-200 text-gray-700 dark:text-gray-200 border-gray-300 dark:border-base-300 hover:bg-gray-100 dark:hover:bg-base-300/80 hover:text-gray-900 dark:hover:text-white shadow-2xs'
                        }`}
                        title={t('monitor.filters.capture_health_tip', { defaultValue: '默认关闭：过滤全部 GET 成功请求（含 /v1/models 模型列表轮询与 /health 探针）以及三方网关探活探测且不入库；失败请求始终记录；开启后才全部记录并落库' })}
                    >
                        <span className={`w-1.5 h-1.5 rounded-full ${captureHealthLogs ? 'bg-white animate-pulse' : 'bg-gray-400 dark:bg-gray-500'}`} />
                        {t('monitor.filters.capture_health', { defaultValue: '捕获健康检查' })}
                    </button>
                    {(filter || accountFilter) && (
                        <button
                            onClick={() => { setFilter(''); setAccountFilter(''); }}
                            className="text-xs font-semibold text-blue-600 dark:text-blue-400 hover:underline ml-1"
                        >
                            {t('monitor.filters.reset')}
                        </button>
                    )}
                    {internalErrorLogPath && (
                        <span
                            className="ml-auto min-w-0 max-w-full truncate text-[11px] leading-5 text-gray-400 dark:text-gray-500 select-text"
                            title={t('monitor.filters.internal_error_log', { path: internalErrorLogPath, defaultValue: `内部错误记录在 ${internalErrorLogPath}，滑动窗口` })}
                        >
                            {t('monitor.filters.internal_error_log', { path: internalErrorLogPath, defaultValue: `内部错误记录在 ${internalErrorLogPath}，滑动窗口` })}
                        </span>
                    )}
                </div>
            </div>

            {/* 日志存储与维护展开配置面板 */}
            {showLogSettings && appConfig && (
                <div className="bg-gray-50/90 dark:bg-base-200 border-b border-gray-200 dark:border-base-300 p-4 space-y-3.5 shadow-xs">
                    {/* Panel Header */}
                    <div className="flex items-center justify-between border-b border-gray-200/80 dark:border-base-200 pb-2.5">
                        <div className="flex items-center gap-2">
                            <Database size={16} className="text-blue-600 dark:text-blue-400" />
                            <span className="text-sm font-bold text-gray-900 dark:text-white">
                                {t('monitor.settings.title', { defaultValue: '日志存储周期与维护设置' })}
                            </span>
                            <span className="text-xs text-gray-500 dark:text-gray-400 hidden sm:inline">
                                {t('monitor.settings.subtitle', { defaultValue: '统一管理请求日志与内部报错日志的滑动窗口容量' })}
                            </span>
                        </div>
                        <div className="flex items-center gap-2">
                            <button
                                onClick={handleSaveLogSettings}
                                disabled={isSavingConfig}
                                className={`px-3 py-1.5 rounded-lg text-xs font-semibold flex items-center gap-1.5 shadow-sm transition-all text-white active:scale-95 ${
                                    saveSuccess
                                        ? 'bg-emerald-600 hover:bg-emerald-700'
                                        : 'bg-blue-600 hover:bg-blue-700'
                                }`}
                                title="保存全部日志与思考块配置"
                            >
                                <Check size={13} />
                                <span>{saveSuccess ? t('common.saved', { defaultValue: '已生效' }) : (isSavingConfig ? t('common.saving', { defaultValue: '保存中...' }) : t('common.save', { defaultValue: '全部保存并热生效' }))}</span>
                            </button>
                            <button
                                onClick={() => setShowLogSettings(false)}
                                className="btn btn-xs btn-ghost text-gray-400 hover:text-gray-600 dark:hover:text-gray-200"
                            >
                                <X size={15} />
                            </button>
                        </div>
                    </div>

                    {/* 请求日志 / 内部报错日志 / 维护 */}
                    <div className="grid grid-cols-1 md:grid-cols-2 gap-3.5">
                        {/* 1. 请求日志 */}
                        <div className="p-3.5 bg-white dark:bg-base-100 rounded-xl border border-gray-200/90 dark:border-base-200 shadow-xs flex flex-col justify-between space-y-3">
                            <div className="space-y-3">
                                <span className="text-xs font-bold text-gray-800 dark:text-gray-200 flex items-center gap-1.5">
                                    <Clock size={13} className="text-indigo-500 dark:text-indigo-400" />
                                    {t('monitor.settings.retention_title', { defaultValue: '请求日志 (滑动窗口)' })}
                                </span>
                                <div className="space-y-2.5">
                                    {/* 空间上限 */}
                                    <div>
                                        <div className="flex items-center justify-between mb-1">
                                            <label className="text-xs font-medium text-gray-600 dark:text-gray-300">
                                                {t('proxy.config.log_retention_storage_gb', { defaultValue: '请求日志上限 (GB)' })}
                                            </label>
                                            <span className="text-[10px] text-gray-500 dark:text-gray-400">
                                                {t('proxy.config.log_retention_current_usage', { defaultValue: '当前库占用' })}: <strong className="font-mono text-gray-700 dark:text-gray-200">{dbDiskSizeBytes !== null ? formatBytes(dbDiskSizeBytes) : '...'}</strong>
                                            </span>
                                        </div>
                                        <input
                                            type="number"
                                            min={0.1}
                                            max={100}
                                            step={0.1}
                                            value={appConfig.proxy.log_retention?.max_storage_gb ?? 1.0}
                                            onChange={(e) => updateLogRetentionField('max_storage_gb', parseFloat(e.target.value))}
                                            className="input input-xs input-bordered bg-gray-50 dark:bg-base-200 border-gray-300 dark:border-base-300 text-gray-800 dark:text-white w-full font-mono text-xs focus:border-blue-500"
                                        />
                                        <p className="text-[10px] text-gray-500 dark:text-gray-400 mt-0.5 leading-tight">
                                            {t('proxy.config.log_retention_storage_gb_desc', { defaultValue: '默认 1GB；达到上限后自动淘汰最旧 30% 记录并继续写入' })}
                                        </p>
                                    </div>

                                    {/* 最大保留条数与报文模式并排 */}
                                    <div className="grid grid-cols-1 sm:grid-cols-2 gap-2.5 pt-1">
                                        <div>
                                            <label className="block text-xs font-medium text-gray-600 dark:text-gray-300 mb-1">
                                                {t('proxy.config.log_retention_rows', { defaultValue: '最大保留条数' })}
                                            </label>
                                            <input
                                                type="number"
                                                min={100}
                                                step={1000}
                                                value={appConfig.proxy.log_retention?.max_rows ?? 100000}
                                                onChange={(e) => updateLogRetentionField('max_rows', Number(e.target.value))}
                                                className="input input-xs input-bordered bg-gray-50 dark:bg-base-200 border-gray-300 dark:border-base-300 text-gray-800 dark:text-white w-full font-mono text-xs focus:border-blue-500"
                                            />
                                        </div>
                                        <div>
                                            <label className="block text-xs font-medium text-gray-600 dark:text-gray-300 mb-1">
                                                {t('proxy.config.experimental.payload_storage_mode_label', { defaultValue: '监控报文存储模式' })}
                                            </label>
                                            <select
                                                className="select select-xs select-bordered bg-gray-50 dark:bg-base-200 border-gray-300 dark:border-base-300 text-gray-800 dark:text-white w-full text-xs"
                                                value={appConfig.proxy.experimental?.payload_storage_mode || 'simple'}
                                                onChange={(e) => updateExperimentalField('payload_storage_mode', e.target.value)}
                                            >
                                                <option value="simple">{t('proxy.config.experimental.payload_mode_simple', { defaultValue: '简要模式 (推荐)' })}</option>
                                                <option value="full">{t('proxy.config.experimental.payload_mode_full', { defaultValue: '完整原文 (排错)' })}</option>
                                            </select>
                                        </div>
                                    </div>
                                    <p className="text-[10px] text-gray-500 dark:text-gray-400 leading-tight">
                                        {t('proxy.config.experimental.payload_storage_mode_desc', { defaultValue: '简要模式避免工具参数与图片撑爆日志库；排错时可切完整模式。' })}
                                    </p>
                                </div>
                            </div>
                        </div>

                        {/* 2. 内部报错日志 */}
                        <div className="p-3.5 bg-white dark:bg-base-100 rounded-xl border border-gray-200/90 dark:border-base-200 shadow-xs flex flex-col justify-between space-y-3">
                            <div className="space-y-3">
                                <span className="text-xs font-bold text-gray-800 dark:text-gray-200 flex items-center gap-1.5">
                                    <FileWarning size={13} className="text-rose-500 dark:text-rose-400" />
                                    {t('monitor.settings.internal_error_title', { defaultValue: '内部报错日志 (滑动窗口)' })}
                                </span>
                                <div>
                                    <div className="flex items-center justify-between mb-1">
                                        <label className="text-xs font-medium text-gray-600 dark:text-gray-300">
                                            {t('proxy.config.internal_error_log_storage_mb', { defaultValue: '内部报错日志上限 (MB)' })}
                                        </label>
                                        <span className="text-[10px] text-gray-500 dark:text-gray-400">
                                            {t('proxy.config.internal_error_log_current_usage', { defaultValue: '当前占用' })}: <strong className="font-mono text-gray-700 dark:text-gray-200">{errorLogDiskSizeBytes !== null ? formatBytes(errorLogDiskSizeBytes) : '...'}</strong>
                                        </span>
                                    </div>
                                    <input
                                        type="number"
                                        min={1}
                                        max={10240}
                                        step={50}
                                        value={appConfig.proxy.internal_error_log_retention?.max_storage_mb ?? 500}
                                        onChange={(e) => updateInternalErrorLogRetention(Number(e.target.value))}
                                        className="input input-xs input-bordered bg-gray-50 dark:bg-base-200 border-gray-300 dark:border-base-300 text-gray-800 dark:text-white w-full font-mono text-xs focus:border-blue-500"
                                    />
                                    <p className="text-[10px] text-gray-500 dark:text-gray-400 mt-0.5 leading-tight">
                                        {t('proxy.config.internal_error_log_storage_mb_desc', { defaultValue: '默认 500MB；仅记录失败。达到上限后淘汰最旧 30% 并继续追加，不影响账号数据。' })}
                                    </p>
                                </div>
                            </div>
                        </div>

                        {/* 3. 维护与清理操作 */}
                        <div className="p-3.5 bg-white dark:bg-base-100 rounded-xl border border-gray-200/90 dark:border-base-200 shadow-xs flex flex-col justify-between space-y-3 md:col-span-2">
                            <div>
                                <span className="text-xs font-bold text-gray-800 dark:text-gray-200 flex items-center gap-1.5 mb-1.5">
                                    <HardDrive size={13} className="text-amber-500 dark:text-amber-400" />
                                    {t('monitor.settings.maintenance_title', { defaultValue: '日志维护与空间清理' })}
                                </span>
                                <p className="text-xs text-gray-500 dark:text-gray-400 leading-relaxed">
                                    {t('settings.advanced.logs_desc', { defaultValue: '清理应用产生的日志缓存文件或清空全部历史请求记录，释放磁盘空间。' })}
                                </p>
                            </div>
                            <div className="grid grid-cols-1 sm:grid-cols-2 gap-2 pt-2">
                                <button
                                    type="button"
                                    onClick={() => setIsClearCacheModalOpen(true)}
                                    className="btn btn-xs w-full btn-outline btn-warning gap-1.5 text-xs font-semibold"
                                >
                                    <Trash2 size={12} />
                                    {t('settings.advanced.clear_logs', { defaultValue: '清理日志缓存文件' })}
                                </button>
                                <button
                                    type="button"
                                    onClick={clearLogs}
                                    className="btn btn-xs w-full btn-outline btn-error gap-1.5 text-xs font-semibold"
                                >
                                    <Trash2 size={12} />
                                    {t('monitor.actions.clear_all_requests', { defaultValue: '清空全部历史请求' })}
                                </button>
                                {cacheClearedSuccess && (
                                    <p className="text-xs text-emerald-600 dark:text-emerald-400 text-center font-medium sm:col-span-2">
                                        ✓ {t('settings.advanced.logs_cleared', { defaultValue: '日志缓存已清理' })}
                                    </p>
                                )}
                            </div>
                        </div>
                    </div>
                </div>
            )}

            <LogTable
                logs={filteredLogs}
                loading={loading}
                onLogClick={async (log: ProxyRequestLog) => {
                    setLoadingDetail(true);
                    try {
                        const detail = await invoke<ProxyRequestLog>('get_proxy_log_detail', { logId: log.id, log_id: log.id });
                        setSelectedLog(detail || log);
                    } catch (e) {
                        console.error('Failed to load log detail', e);
                        setSelectedLog(log);
                    } finally {
                        setLoadingDetail(false);
                    }
                }}
                t={t}
            />

            {/* Pagination Controls */}
            <div className="flex items-center justify-between px-4 py-3 bg-gray-50 dark:bg-base-200 border-t border-gray-200 dark:border-base-300 text-xs">
                <div className="flex items-center gap-2 whitespace-nowrap">
                    <span className="text-gray-500">{t('common.per_page')}</span>
                    <select
                        value={pageSize}
                        onChange={(e) => setPageSize(Number(e.target.value))}
                        className="select select-xs select-bordered w-16"
                    >
                        {PAGE_SIZE_OPTIONS.map(size => (
                            <option key={size} value={size}>{size}</option>
                        ))}
                    </select>
                </div>

                <div className="flex items-center gap-3">
                    <button
                        onClick={() => goToPage(currentPage - 1)}
                        disabled={currentPage <= 1 || loading}
                        className="btn btn-xs btn-ghost"
                    >
                        <ChevronLeft size={14} />
                    </button>
                    <span className="text-gray-600 dark:text-gray-400 min-w-[80px] text-center">
                        {currentPage} / {totalPages || 1}
                    </span>
                    <button
                        onClick={() => goToPage(currentPage + 1)}
                        disabled={currentPage >= totalPages || loading}
                        className="btn btn-xs btn-ghost"
                    >
                        <ChevronRight size={14} />
                    </button>
                </div>

                <div className="text-gray-500">
                    {t('common.pagination_info', { start: pageStart, end: pageEnd, total: totalCount })}
                </div>
            </div>

            {selectedLog && (
                <RequestDetailModal
                    log={selectedLog}
                    onClose={() => setSelectedLog(null)}
                    loadingDetail={loadingDetail}
                />
            )}

            <ModalDialog
                isOpen={isClearConfirmOpen}
                title={t('monitor.dialog.clear_title')}
                message={t('monitor.dialog.clear_msg')}
                type="confirm"
                confirmText={t('common.delete')}
                isDestructive={true}
                onConfirm={executeClearLogs}
                onCancel={() => setIsClearConfirmOpen(false)}
            />

            <ModalDialog
                isOpen={isClearCacheModalOpen}
                title={t('settings.advanced.clear_logs_title', { defaultValue: '清理日志缓存确认' })}
                message={t('settings.advanced.clear_logs_msg', { defaultValue: '确定要清理所有日志缓存文件吗？这不会影响历史请求记录和账号数据。' })}
                type="confirm"
                confirmText={t('common.clear', { defaultValue: '清理' })}
                cancelText={t('common.cancel', { defaultValue: '取消' })}
                isDestructive={true}
                onConfirm={handleClearCache}
                onCancel={() => setIsClearCacheModalOpen(false)}
            />
        </div>
    );
};
