import React, { useEffect, useState, useRef, useMemo, useCallback } from 'react';
import { listen } from '@tauri-apps/api/event';
import ModalDialog from '../common/ModalDialog';
import { useTranslation } from 'react-i18next';
import { request as invoke } from '../../utils/request';
import { Trash2, Search, X, Copy, CheckCircle, ChevronLeft, ChevronRight, ChevronDown, RefreshCw, User, Sparkles, FileCode2, Eye, EyeOff, Clock, Settings, HardDrive, Database, Check } from 'lucide-react';

import { AppConfig, ExperimentalConfig } from '../../types/config';
import { formatCompactNumber } from '../../utils/format';
import { useAccountStore } from '../../stores/useAccountStore';
import { isTauri } from '../../utils/env';
import { copyToClipboard } from '../../utils/clipboard';
import { VirtualizedPayloadViewer } from './VirtualizedPayloadViewer';


interface ProxyRequestLog {
    id: string;
    timestamp: number;
    method: string;
    url: string;
    status: number;
    duration: number;
    model?: string;
    mapped_model?: string;
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
    method: number;
    model: number;
    protocol: number;
    account: number;
    path: number;
    usage: number;
    duration: number;
    time: number;
}

const DEFAULT_COL_WIDTHS: ColumnWidths = {
    status: 65,
    method: 65,
    model: 240,
    protocol: 80,
    account: 150,
    path: 180,
    usage: 125,
    duration: 85,
    time: 85,
};

const LogTable: React.FC<LogTableProps> = ({
    logs,
    loading,
    onLogClick,
    t
}) => {
    const [colWidths, setColWidths] = useState<ColumnWidths>(() => {
        try {
            const saved = localStorage.getItem('proxy_log_col_widths');
            if (saved) {
                return { ...DEFAULT_COL_WIDTHS, ...JSON.parse(saved) };
            }
        } catch {}
        return DEFAULT_COL_WIDTHS;
    });

    // 类似 Excel 的鼠标拖拽调整表头列宽机制
    const handleResizeStart = (colKey: keyof ColumnWidths, e: React.MouseEvent) => {
        e.preventDefault();
        e.stopPropagation();
        const startX = e.clientX;
        const startWidth = colWidths[colKey];

        const originalCursor = document.body.style.cursor;
        const originalUserSelect = document.body.style.userSelect;
        document.body.style.cursor = 'col-resize';
        document.body.style.userSelect = 'none';

        const onMouseMove = (moveEvent: MouseEvent) => {
            const diff = moveEvent.clientX - startX;
            const newWidth = Math.max(45, startWidth + diff);
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
            const finalWidth = Math.max(45, startWidth + finalDiff);
            setColWidths((prev) => {
                const next = { ...prev, [colKey]: finalWidth };
                try {
                    localStorage.setItem('proxy_log_col_widths', JSON.stringify(next));
                } catch {}
                return next;
            });
        };

        window.addEventListener('mousemove', onMouseMove);
        window.addEventListener('mouseup', onMouseUp);
    };

    const totalTableWidth = useMemo(() => {
        return Object.values(colWidths).reduce((a, b) => a + b, 0);
    }, [colWidths]);

    return (
        <div
            className="flex-1 overflow-y-auto overflow-x-auto bg-white dark:bg-base-100 relative scrollbar-thin"
        >
            <table
                className="table table-sm border-separate border-spacing-0"
                style={{ minWidth: `${totalTableWidth}px`, width: `${totalTableWidth}px`, tableLayout: 'fixed' }}
            >
                <thead className="bg-gray-100/90 dark:bg-base-200 text-gray-700 dark:text-gray-200 text-xs font-semibold sticky top-0 z-10 backdrop-blur-sm border-b border-gray-200 dark:border-base-300">
                    <tr>
                        <th style={{ width: `${colWidths.status}px`, minWidth: `${colWidths.status}px`, maxWidth: `${colWidths.status}px` }} className="py-2.5 px-3 relative group select-none">
                            <div className="truncate">{t('monitor.table.status')}</div>
                            <div
                                onMouseDown={(e) => handleResizeStart('status', e)}
                                className="absolute right-0 top-0 bottom-0 w-1.5 cursor-col-resize hover:bg-blue-500 active:bg-blue-600 transition-colors z-20 group-hover:bg-gray-300 dark:group-hover:bg-gray-600"
                                title="拖动调整列宽"
                            />
                        </th>
                        <th style={{ width: `${colWidths.method}px`, minWidth: `${colWidths.method}px`, maxWidth: `${colWidths.method}px` }} className="py-2.5 px-3 relative group select-none">
                            <div className="truncate">{t('monitor.table.method')}</div>
                            <div
                                onMouseDown={(e) => handleResizeStart('method', e)}
                                className="absolute right-0 top-0 bottom-0 w-1.5 cursor-col-resize hover:bg-blue-500 active:bg-blue-600 transition-colors z-20 group-hover:bg-gray-300 dark:group-hover:bg-gray-600"
                                title="拖动调整列宽"
                            />
                        </th>
                        <th style={{ width: `${colWidths.model}px`, minWidth: `${colWidths.model}px`, maxWidth: `${colWidths.model}px` }} className="py-2.5 px-3 relative group select-none">
                            <div className="truncate">{t('monitor.table.model')}</div>
                            <div
                                onMouseDown={(e) => handleResizeStart('model', e)}
                                className="absolute right-0 top-0 bottom-0 w-1.5 cursor-col-resize hover:bg-blue-500 active:bg-blue-600 transition-colors z-20 group-hover:bg-gray-300 dark:group-hover:bg-gray-600"
                                title="拖动调整列宽"
                            />
                        </th>
                        <th style={{ width: `${colWidths.protocol}px`, minWidth: `${colWidths.protocol}px`, maxWidth: `${colWidths.protocol}px` }} className="py-2.5 px-3 relative group select-none">
                            <div className="truncate">{t('monitor.table.protocol')}</div>
                            <div
                                onMouseDown={(e) => handleResizeStart('protocol', e)}
                                className="absolute right-0 top-0 bottom-0 w-1.5 cursor-col-resize hover:bg-blue-500 active:bg-blue-600 transition-colors z-20 group-hover:bg-gray-300 dark:group-hover:bg-gray-600"
                                title="拖动调整列宽"
                            />
                        </th>
                        <th style={{ width: `${colWidths.account}px`, minWidth: `${colWidths.account}px`, maxWidth: `${colWidths.account}px` }} className="py-2.5 px-3 relative group select-none">
                            <div className="truncate">{t('monitor.table.account')}</div>
                            <div
                                onMouseDown={(e) => handleResizeStart('account', e)}
                                className="absolute right-0 top-0 bottom-0 w-1.5 cursor-col-resize hover:bg-blue-500 active:bg-blue-600 transition-colors z-20 group-hover:bg-gray-300 dark:group-hover:bg-gray-600"
                                title="拖动调整列宽"
                            />
                        </th>
                        <th style={{ width: `${colWidths.path}px`, minWidth: `${colWidths.path}px`, maxWidth: `${colWidths.path}px` }} className="py-2.5 px-3 relative group select-none">
                            <div className="truncate">{t('monitor.table.path')}</div>
                            <div
                                onMouseDown={(e) => handleResizeStart('path', e)}
                                className="absolute right-0 top-0 bottom-0 w-1.5 cursor-col-resize hover:bg-blue-500 active:bg-blue-600 transition-colors z-20 group-hover:bg-gray-300 dark:group-hover:bg-gray-600"
                                title="拖动调整列宽"
                            />
                        </th>
                        <th style={{ width: `${colWidths.usage}px`, minWidth: `${colWidths.usage}px`, maxWidth: `${colWidths.usage}px` }} className="text-right py-2.5 px-3 relative group select-none whitespace-nowrap">
                            <div className="truncate">{t('monitor.table.usage')}</div>
                            <div
                                onMouseDown={(e) => handleResizeStart('usage', e)}
                                className="absolute right-0 top-0 bottom-0 w-1.5 cursor-col-resize hover:bg-blue-500 active:bg-blue-600 transition-colors z-20 group-hover:bg-gray-300 dark:group-hover:bg-gray-600"
                                title="拖动调整列宽"
                            />
                        </th>
                        <th style={{ width: `${colWidths.duration}px`, minWidth: `${colWidths.duration}px`, maxWidth: `${colWidths.duration}px` }} className="text-right py-2.5 px-3 relative group select-none">
                            <div className="truncate">{t('monitor.table.duration')}</div>
                            <div
                                onMouseDown={(e) => handleResizeStart('duration', e)}
                                className="absolute right-0 top-0 bottom-0 w-1.5 cursor-col-resize hover:bg-blue-500 active:bg-blue-600 transition-colors z-20 group-hover:bg-gray-300 dark:group-hover:bg-gray-600"
                                title="拖动调整列宽"
                            />
                        </th>
                        <th style={{ width: `${colWidths.time}px`, minWidth: `${colWidths.time}px`, maxWidth: `${colWidths.time}px` }} className="text-right py-2.5 px-3 relative group select-none">
                            <div className="truncate">{t('monitor.table.time')}</div>
                            <div
                                onMouseDown={(e) => handleResizeStart('time', e)}
                                className="absolute right-0 top-0 bottom-0 w-1.5 cursor-col-resize hover:bg-blue-500 active:bg-blue-600 transition-colors z-20 group-hover:bg-gray-300 dark:group-hover:bg-gray-600"
                                title="拖动调整列宽"
                            />
                        </th>
                    </tr>
                </thead>
                <tbody className="font-mono text-gray-800 dark:text-gray-100 text-xs divide-y divide-gray-100 dark:divide-base-200">
                    {logs.map((log) => (
                        <tr
                            key={log.id}
                            className="hover:bg-blue-50/80 dark:hover:bg-base-200/80 cursor-pointer transition-colors"
                            onClick={() => onLogClick(log)}
                        >
                            <td style={{ width: `${colWidths.status}px`, maxWidth: `${colWidths.status}px` }} className="py-2 px-3 truncate">
                                <span className={`badge badge-sm font-bold text-white border-none shadow-xs ${
                                    log.status >= 200 && log.status < 400
                                        ? 'bg-emerald-600 dark:bg-emerald-600'
                                        : 'bg-rose-600 dark:bg-rose-600'
                                }`}>
                                    {log.status}
                                </span>
                            </td>
                            <td className="font-bold text-gray-900 dark:text-white py-2 px-3 truncate" style={{ width: `${colWidths.method}px`, maxWidth: `${colWidths.method}px` }}>{log.method}</td>
                            <td 
                                className="text-sky-600 dark:text-sky-400 font-semibold truncate py-2 px-3" 
                                style={{ width: `${colWidths.model}px`, maxWidth: `${colWidths.model}px` }}
                                title={log.mapped_model && log.model !== log.mapped_model ? `${log.model} => ${log.mapped_model}` : (log.model || '')}
                            >
                                {log.mapped_model && log.model !== log.mapped_model
                                    ? `${log.model} => ${log.mapped_model}`
                                    : (log.model || '-')}
                            </td>
                            <td style={{ width: `${colWidths.protocol}px`, maxWidth: `${colWidths.protocol}px` }} className="py-2 px-3 truncate">
                                {log.protocol && (
                                    <span className={`badge badge-xs px-2 py-0.5 font-bold text-white border-none shadow-xs ${
                                        log.protocol === 'openai' ? 'bg-emerald-600 dark:bg-emerald-600' :
                                            log.protocol === 'anthropic' ? 'bg-amber-600 dark:bg-amber-600' :
                                                log.protocol === 'gemini' ? 'bg-blue-600 dark:bg-blue-600' :
                                                    'bg-gray-600 dark:bg-gray-600'
                                    }`}>
                                        {log.protocol === 'openai' ? 'OpenAI' :
                                            log.protocol === 'anthropic' ? 'Claude' :
                                                log.protocol === 'gemini' ? 'Gemini' : log.protocol}
                                    </span>
                                )}
                            </td>
                            <td className="text-gray-600 dark:text-gray-300 font-sans truncate text-xs py-2 px-3" style={{ width: `${colWidths.account}px`, maxWidth: `${colWidths.account}px` }} title={log.account_email || ''}>
                                {log.account_email ? log.account_email.replace(/(.{3}).*(@.*)/, '$1***$2') : '-'}
                            </td>
                            <td className="text-gray-700 dark:text-gray-300 truncate text-xs py-2 px-3" style={{ width: `${colWidths.path}px`, maxWidth: `${colWidths.path}px` }} title={log.url || ''}>{log.url}</td>
                            <td className="text-right text-xs py-2 px-3 whitespace-nowrap truncate" style={{ width: `${colWidths.usage}px`, maxWidth: `${colWidths.usage}px` }}>
                                {log.input_tokens != null && (() => {
                                    const totalIn = (log.cached_tokens && log.cached_tokens > log.input_tokens)
                                        ? log.input_tokens + log.cached_tokens
                                        : log.input_tokens;
                                    const hitRate = (log.cached_tokens && totalIn > 0)
                                        ? Math.min(100, Math.max(0, (log.cached_tokens / totalIn) * 100))
                                        : 0;
                                    const hitRateText = totalIn > 0 && log.cached_tokens
                                        ? (hitRate >= 100 ? '100%' : (hitRate % 1 === 0 ? `${hitRate.toFixed(0)}%` : `${hitRate.toFixed(1)}%`))
                                        : '';
                                    return (
                                        <div>
                                            <div className="text-gray-700 dark:text-gray-200">{t('monitor.input')}: <span className="font-semibold">{formatCompactNumber(totalIn)}</span></div>
                                            {log.cached_tokens ? (
                                                <div
                                                    className="text-emerald-600 dark:text-emerald-400 font-semibold text-[11px] leading-tight"
                                                    title={`${t('token_stats.cached', 'Cache')}: ${log.cached_tokens.toLocaleString()}${hitRateText ? ` (${hitRateText})` : ''}`}
                                                >
                                                    ({t('monitor.cached', 'Cache')}: {formatCompactNumber(log.cached_tokens)}{hitRateText ? ` ${hitRateText}` : ''})
                                                </div>
                                            ) : null}
                                        </div>
                                    );
                                })()}
                                {log.output_tokens != null && <div className="text-gray-700 dark:text-gray-200">{t('monitor.output')}: <span className="font-semibold">{formatCompactNumber(log.output_tokens)}</span></div>}
                            </td>
                            <td className="text-right text-gray-700 dark:text-gray-300 text-xs font-medium py-2 px-3 truncate" style={{ width: `${colWidths.duration}px`, maxWidth: `${colWidths.duration}px` }}>{log.duration}ms</td>
                            <td className="text-right text-gray-500 dark:text-gray-400 text-xs py-2 px-3 truncate" style={{ width: `${colWidths.time}px`, maxWidth: `${colWidths.time}px` }}>
                                {new Date(log.timestamp).toLocaleTimeString()}
                            </td>
                        </tr>
                    ))}
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


// ==========================================
// 简要模式智能提取与映射算法
// ==========================================
function extractConcisePayload(
    rawStr: string | undefined,
    kind: 'request' | 'upstream' | 'response',
    log?: ProxyRequestLog | null
): string {
    if (!rawStr) return '';
    let obj: any;
    try {
        obj = JSON.parse(rawStr);
    } catch {
        return rawStr;
    }
    if (!obj || typeof obj !== 'object') {
        return rawStr;
    }

    // 工具声明 (完整保留 Schema，方便开发者查看工具拼接与入参定义)
    const simplifyTools = (tools: any): any => {
        if (!Array.isArray(tools)) return undefined;
        return tools;
    };

    // 简化工具调用 (统一规范为: id, type: 'function', function: { name, arguments })
    const simplifyToolCalls = (toolCalls: any): any => {
        if (!Array.isArray(toolCalls)) return undefined;
        return toolCalls.map((tc: any) => {
            if (!tc || typeof tc !== 'object') return tc;
            const res: any = {};
            if (tc.id) res.id = tc.id;
            res.type = tc.type || 'function';
            if (tc.function && typeof tc.function === 'object') {
                res.function = {
                    name: tc.function.name,
                    arguments: tc.function.arguments !== undefined ? tc.function.arguments : {}
                };
            } else {
                const name = tc.name || tc.function?.name || 'unknown';
                const args = tc.arguments !== undefined ? tc.arguments : (tc.args !== undefined ? tc.args : (tc.input !== undefined ? tc.input : {}));
                res.function = {
                    name,
                    arguments: args
                };
            }
            return res;
        });
    };

    // 简化消息内容 (Claude / OpenAI parts)
    const simplifyContent = (content: any): any => {
        if (typeof content === 'string') return content;
        if (Array.isArray(content)) {
            return content.map((item: any) => {
                if (typeof item === 'string') return item;
                if (!item || typeof item !== 'object') return item;
                // Claude tool_use 块
                if (item.type === 'tool_use') {
                    return {
                        type: 'tool_use',
                        id: item.id,
                        name: item.name,
                        input: item.input !== undefined ? item.input : {}
                    };
                }
                // Claude tool_result 块
                if (item.type === 'tool_result') {
                    return {
                        type: 'tool_result',
                        tool_use_id: item.tool_use_id,
                        ...(item.content !== undefined ? { content: item.content } : {}),
                        ...(item.is_error !== undefined ? { is_error: item.is_error } : {})
                    };
                }
                // Claude thinking 块与签名
                if (item.type === 'thinking') {
                    return {
                        type: 'thinking',
                        thinking: item.thinking,
                        ...(item.signature !== undefined ? { signature: item.signature } : {}),
                        ...(item.thought_signature !== undefined ? { thought_signature: item.thought_signature } : {}),
                        ...(item.thoughtSignature !== undefined ? { thoughtSignature: item.thoughtSignature } : {}),
                        ...(item.thinking_signature !== undefined ? { thinking_signature: item.thinking_signature } : {})
                    };
                }
                // Claude redacted_thinking 块
                if (item.type === 'redacted_thinking') {
                    return {
                        type: 'redacted_thinking',
                        data: item.data
                    };
                }
                // 文本块
                if (item.type === 'text') {
                    return item;
                }
                return item;
            });
        }
        return content;
    };

    // 简化消息列表
    const simplifyMessages = (messages: any): any => {
        if (!Array.isArray(messages)) return undefined;
        return messages.map((m: any) => {
            if (!m || typeof m !== 'object') return m;
            const res: any = { role: m.role };
            if (m.content !== undefined) {
                res.content = simplifyContent(m.content);
            }
            if (m.reasoning_content !== undefined) {
                res.reasoning_content = m.reasoning_content;
            }
            if (m.thinking !== undefined) {
                res.thinking = m.thinking;
            }
            if (m.signature !== undefined) {
                res.signature = m.signature;
            }
            if (m.thought_signature !== undefined) {
                res.thought_signature = m.thought_signature;
            }
            if (m.thinking_signature !== undefined) {
                res.thinking_signature = m.thinking_signature;
            }
            if (m.tool_calls) {
                res.tool_calls = simplifyToolCalls(m.tool_calls);
            }
            if (m.tool_call_id) {
                res.tool_call_id = m.tool_call_id;
            }
            if (m.name) {
                res.name = m.name;
            }
            return res;
        });
    };

    // 简化 Gemini 轮次 (contents)
    const simplifyGeminiContents = (contents: any): any => {
        if (!Array.isArray(contents)) return undefined;
        return contents.map((c: any) => {
            if (!c || typeof c !== 'object') return c;
            const res: any = { role: c.role };
            if (Array.isArray(c.parts)) {
                res.parts = c.parts.map((p: any) => {
                    if (!p || typeof p !== 'object') return p;

                    // 1. 优先识别工具调用 (functionCall) 并保留其名称、ID、参数与携带的加密思考签名
                    if (p.functionCall) {
                        const fcPart: any = {
                            functionCall: {
                                name: p.functionCall.name,
                                ...(p.functionCall.id ? { id: p.functionCall.id } : {}),
                                args: p.functionCall.args !== undefined ? p.functionCall.args : {}
                            }
                        };
                        if (p.thought !== undefined) fcPart.thought = p.thought;
                        if (p.thoughtSignature !== undefined) fcPart.thoughtSignature = p.thoughtSignature;
                        if (p.thought_signature !== undefined) fcPart.thought_signature = p.thought_signature;
                        if (p.signature !== undefined) fcPart.signature = p.signature;
                        return fcPart;
                    }

                    // 2. 优先识别工具响应 (functionResponse) 并保留其名称、ID、返回值与携带的签名
                    if (p.functionResponse) {
                        const frPart: any = {
                            functionResponse: {
                                name: p.functionResponse.name,
                                ...(p.functionResponse.id ? { id: p.functionResponse.id } : {}),
                                response: p.functionResponse.response !== undefined ? p.functionResponse.response : {}
                            }
                        };
                        if (p.thought !== undefined) frPart.thought = p.thought;
                        if (p.thoughtSignature !== undefined) frPart.thoughtSignature = p.thoughtSignature;
                        if (p.thought_signature !== undefined) frPart.thought_signature = p.thought_signature;
                        if (p.signature !== undefined) frPart.signature = p.signature;
                        return frPart;
                    }

                    // 3. 独立思考块 (纯思考过程，不带工具调用)
                    if (p.thought !== undefined || p.thought_signature !== undefined || p.thoughtSignature !== undefined || p.signature !== undefined) {
                        const tPart: any = {};
                        if (p.thought !== undefined) tPart.thought = p.thought;
                        if (p.thought_signature !== undefined) tPart.thought_signature = p.thought_signature;
                        if (p.thoughtSignature !== undefined) tPart.thoughtSignature = p.thoughtSignature;
                        if (p.signature !== undefined) tPart.signature = p.signature;
                        if (p.text !== undefined) tPart.text = p.text;
                        return tPart;
                    }

                    // 4. 普通文本块
                    if (p.text !== undefined) {
                        return { text: p.text };
                    }

                    return p;
                });
            }
            return res;
        });
    };

    // 简化系统提示词 (Gemini / Anthropic)
    const simplifySystemInstruction = (sys: any): any => {
        if (!sys || typeof sys !== 'object') return sys;
        if (Array.isArray(sys.parts)) {
            return {
                parts: sys.parts.map((p: any) => {
                    if (typeof p === 'string') return { text: p };
                    if (p && typeof p === 'object' && p.text !== undefined) return { text: p.text };
                    return p;
                })
            };
        }
        return sys;
    };

    // 提取用量与缓存命中率
    const simplifyUsage = (usage: any): any => {
        if (!usage || typeof usage !== 'object') return undefined;
        const res: any = {};
        const rawInput = usage.prompt_tokens ?? usage.input_tokens ?? usage.promptTokenCount;
        const output = usage.completion_tokens ?? usage.output_tokens ?? usage.candidatesTokenCount;

        let cached = usage.cached_tokens ?? usage.cache_read_input_tokens ?? usage.cachedContentTokenCount;
        if (cached == null && usage.prompt_tokens_details?.cached_tokens != null) {
            cached = usage.prompt_tokens_details.cached_tokens;
        }
        if (cached == null && usage.input_tokens_details?.cached_tokens != null) {
            cached = usage.input_tokens_details.cached_tokens;
        }

        // 计算全量上下文输入 Token (Total Context Input)
        // 1. Anthropic 官方协议: input_tokens 仅代表未缓存增量，总上下文 = input_tokens + cache_read_input_tokens
        // 2. 兼容历史日志: 若 cached > rawInput，说明 rawInput 存的是未缓存差值，做自愈加和
        let totalInput = rawInput != null ? Number(rawInput) : undefined;
        if (cached != null && totalInput != null && cached > totalInput) {
            totalInput = totalInput + Number(cached);
        } else if (usage.cache_read_input_tokens != null && usage.prompt_tokens == null && usage.promptTokenCount == null) {
            totalInput = Number(usage.input_tokens || 0) + Number(cached || 0);
        }

        const total = usage.total_tokens ?? usage.totalTokenCount ?? (totalInput != null && output != null ? totalInput + Number(output) : undefined);

        if (totalInput != null) res.input_tokens = totalInput;
        if (output != null) res.output_tokens = Number(output);
        if (total != null) res.total_tokens = Number(total);
        if (cached != null) {
            res.cached_tokens = Number(cached);
            if (totalInput != null && totalInput > 0) {
                const rate = Math.min(100, Math.max(0, (Number(cached) / totalInput) * 100));
                res.cache_hit_rate = `${rate.toFixed(1)}%`;
            }
        }
        if (usage.cache_creation_input_tokens != null) {
            res.cache_creation_input_tokens = usage.cache_creation_input_tokens;
        }
        if (usage.completion_tokens_details?.reasoning_tokens != null) {
            res.reasoning_tokens = usage.completion_tokens_details.reasoning_tokens;
        }
        if (usage.output_tokens_details?.reasoning_tokens != null) {
            res.reasoning_tokens = usage.output_tokens_details.reasoning_tokens;
        }
        return res;
    };

    const concise: any = {};

    // 保留用于标识思考块/会话的单行标识 (支持 requestId, sessionId, trace_id 等)
    const candidateSessionId =
        obj.requestId ||
        obj.request?.sessionId ||
        obj._session_id ||
        obj.session_id ||
        (log?.id ? log.id : undefined);

    if (candidateSessionId) {
        concise._session_thinking_id = candidateSessionId;
    }

    // 模型
    if (obj.model) concise.model = obj.model;

    // 思考模型配置 (开启、预算、effort、summary)
    if (obj.thinking !== undefined) concise.thinking = obj.thinking;
    if (obj.reasoning_effort !== undefined) concise.reasoning_effort = obj.reasoning_effort;
    if (obj.reasoning !== undefined) concise.reasoning = obj.reasoning;
    if (obj.summary !== undefined) concise.summary = obj.summary;
    if (obj.generationConfig?.thinkingConfig !== undefined) {
        concise.thinkingConfig = obj.generationConfig.thinkingConfig;
    } else if (obj.thinkingConfig !== undefined) {
        concise.thinkingConfig = obj.thinkingConfig;
    }

    // 系统提示词
    if (obj.system !== undefined) concise.system = obj.system;
    if (obj.systemInstruction !== undefined) concise.systemInstruction = simplifySystemInstruction(obj.systemInstruction);

    // 对话主体 (OpenAI / Claude)
    if (obj.messages) {
        concise.messages = simplifyMessages(obj.messages);
    }

    // 对话主体 (Gemini)
    if (obj.contents) {
        concise.contents = simplifyGeminiContents(obj.contents);
    }

    // 工具声明
    if (obj.tools) {
        concise.tools = simplifyTools(obj.tools);
    }

    // Antigravity 专用的 request 嵌套包装层 (核心：正确映射原中转报文的嵌套层级)
    if (obj.request && typeof obj.request === 'object') {
        const innerReq: any = {};

        // 单行会话标识
        if (obj.request.sessionId) {
            innerReq.sessionId = obj.request.sessionId;
        }

        // 思考配置 (thinkingConfig / generationConfig)
        if (obj.request.generationConfig?.thinkingConfig !== undefined) {
            innerReq.thinkingConfig = obj.request.generationConfig.thinkingConfig;
        } else if (obj.request.thinkingConfig !== undefined) {
            innerReq.thinkingConfig = obj.request.thinkingConfig;
        }

        // 系统提示词
        if (obj.request.systemInstruction !== undefined) {
            innerReq.systemInstruction = simplifySystemInstruction(obj.request.systemInstruction);
        }

        // 对话主体与思考块 (Gemini contents 或 Claude messages)
        if (obj.request.contents) {
            innerReq.contents = simplifyGeminiContents(obj.request.contents);
        }
        if (obj.request.messages) {
            innerReq.messages = simplifyMessages(obj.request.messages);
        }

        // 工具声明
        if (obj.request.tools) {
            innerReq.tools = simplifyTools(obj.request.tools);
        }

        concise.request = innerReq;
    }

    // 响应：思考块与思考签名 (顶层响应或非流式)
    if (obj.thinking !== undefined) concise.thinking = obj.thinking;
    if (obj.thinking_signature !== undefined) concise.thinking_signature = obj.thinking_signature;
    if (obj.thought_signature !== undefined) concise.thought_signature = obj.thought_signature;
    if (obj.signature !== undefined) concise.signature = obj.signature;
    if (obj.thoughtSignature !== undefined) concise.thoughtSignature = obj.thoughtSignature;
    if (obj._timing !== undefined) concise._timing = obj._timing;

    // 🌟 响应报文规范化提取：若为 response，优先将 choices / candidates / content 数组扁平化提升为顶层统一结构
    if (kind === 'response') {
        if (obj.choices && Array.isArray(obj.choices) && obj.choices.length > 0) {
            const first = obj.choices[0];
            const msg = first?.message || first?.delta;
            if (msg) {
                if (concise.thinking === undefined) {
                    const th = msg.reasoning_content || msg.thinking;
                    if (th) concise.thinking = th;
                }
                if (concise.thinking_signature === undefined) {
                    const sig = msg.thoughtSignature || msg.thought_signature || msg.signature;
                    if (sig) concise.thinking_signature = sig;
                }
                if (concise.content === undefined && msg.content !== undefined) {
                    concise.content = typeof msg.content === 'string' ? msg.content : simplifyContent(msg.content);
                }
                if (concise.tool_calls === undefined && msg.tool_calls) {
                    concise.tool_calls = simplifyToolCalls(msg.tool_calls);
                }
            }
        } else if (obj.candidates && Array.isArray(obj.candidates) && obj.candidates.length > 0) {
            const parts = obj.candidates[0]?.content?.parts;
            if (Array.isArray(parts)) {
                let thText = '';
                let normalText = '';
                let sigText = '';
                const extractedTools: any[] = [];
                for (const p of parts) {
                    if (p.text) {
                        if (p.thought) thText += p.text;
                        else normalText += p.text;
                    }
                    const s = p.thoughtSignature || p.thought_signature || p.signature || p.functionCall?.thoughtSignature || p.functionCall?.thought_signature;
                    if (s && !sigText) sigText = s;
                    if (p.functionCall) {
                        extractedTools.push({
                            id: p.functionCall.id || '',
                            type: 'function',
                            function: {
                                name: p.functionCall.name || 'unknown',
                                arguments: p.functionCall.args !== undefined ? (typeof p.functionCall.args === 'string' ? p.functionCall.args : JSON.stringify(p.functionCall.args)) : '{}'
                            }
                        });
                    }
                }
                if (concise.thinking === undefined && thText) concise.thinking = thText;
                if (concise.thinking_signature === undefined && sigText) concise.thinking_signature = sigText;
                if (concise.content === undefined && normalText) concise.content = normalText;
                if (concise.tool_calls === undefined && extractedTools.length > 0) concise.tool_calls = simplifyToolCalls(extractedTools);
            }
        } else if (Array.isArray(obj.content) && !obj.messages && !obj.choices) {
            let thText = '';
            let sigText = '';
            let normalText = '';
            const extractedTools: any[] = [];
            for (const item of obj.content) {
                if (item && typeof item === 'object') {
                    if (item.type === 'thinking') {
                        if (item.thinking) thText += item.thinking;
                        const s = item.signature || item.thought_signature || item.thoughtSignature;
                        if (s && !sigText) sigText = s;
                    } else if (item.type === 'text' && item.text) {
                        normalText += item.text;
                    } else if (item.type === 'tool_use') {
                        extractedTools.push({
                            id: item.id || '',
                            type: 'function',
                            function: {
                                name: item.name || 'unknown',
                                arguments: item.input !== undefined ? (typeof item.input === 'string' ? item.input : JSON.stringify(item.input)) : '{}'
                            }
                        });
                    }
                }
            }
            if (concise.thinking === undefined && thText) concise.thinking = thText;
            if (concise.thinking_signature === undefined && sigText) concise.thinking_signature = sigText;
            if (concise.content === undefined && normalText) concise.content = normalText;
            if (concise.tool_calls === undefined && extractedTools.length > 0) concise.tool_calls = simplifyToolCalls(extractedTools);
        }
    }

    // 响应：Choices / Candidates / 聚合响应 (若为 request 或未扁平化提取的 response，保留 choices/candidates)
    if (kind !== 'response' || (!concise.content && !concise.tool_calls && !concise.thinking)) {
        if (obj.choices && Array.isArray(obj.choices)) {
            concise.choices = obj.choices.map((c: any) => {
                const choiceRes: any = { index: c.index };
                if (c.finish_reason) choiceRes.finish_reason = c.finish_reason;
                if (c.message) {
                    choiceRes.message = {
                        role: c.message.role,
                        ...(c.message.reasoning_content !== undefined ? { reasoning_content: c.message.reasoning_content } : {}),
                        ...(c.message.thinking !== undefined ? { thinking: c.message.thinking } : {}),
                        ...(c.message.thinking_signature !== undefined ? { thinking_signature: c.message.thinking_signature } : {}),
                        ...(c.message.thought_signature !== undefined ? { thought_signature: c.message.thought_signature } : {}),
                        ...(c.message.signature !== undefined ? { signature: c.message.signature } : {}),
                        ...(c.message.content !== undefined ? { content: c.message.content } : {}),
                        ...(c.message.tool_calls ? { tool_calls: simplifyToolCalls(c.message.tool_calls) } : {})
                    };
                } else if (c.delta) {
                    choiceRes.delta = {
                        role: c.delta.role,
                        ...(c.delta.reasoning_content !== undefined ? { reasoning_content: c.delta.reasoning_content } : {}),
                        ...(c.delta.thinking !== undefined ? { thinking: c.delta.thinking } : {}),
                        ...(c.delta.thinking_signature !== undefined ? { thinking_signature: c.delta.thinking_signature } : {}),
                        ...(c.delta.thought_signature !== undefined ? { thought_signature: c.delta.thought_signature } : {}),
                        ...(c.delta.signature !== undefined ? { signature: c.delta.signature } : {}),
                        ...(c.delta.content !== undefined ? { content: c.delta.content } : {}),
                        ...(c.delta.tool_calls ? { tool_calls: simplifyToolCalls(c.delta.tool_calls) } : {})
                    };
                }
                return choiceRes;
            });
        }

        if (obj.candidates && Array.isArray(obj.candidates)) {
            concise.candidates = obj.candidates.map((cand: any) => {
                const candRes: any = {};
                if (cand.finishReason) candRes.finishReason = cand.finishReason;
                if (cand.content) {
                    candRes.content = simplifyGeminiContents([cand.content])?.[0] || cand.content;
                }
                return candRes;
            });
        }
    }

    if (obj.input !== undefined) {
        concise.input = typeof obj.input === 'string' ? obj.input : (Array.isArray(obj.input) ? simplifyMessages(obj.input) : obj.input);
    }
    if (obj.output !== undefined) {
        concise.output = obj.output;
    }
    if (obj.prompt !== undefined) {
        concise.prompt = obj.prompt;
    }
    if (obj.instructions !== undefined) {
        concise.instructions = obj.instructions;
    }

    if (obj.content !== undefined && !obj.messages && !obj.choices && !obj.request) {
        concise.content = simplifyContent(obj.content);
    }
    if (obj.reasoning_content !== undefined && !obj.messages && !obj.choices) {
        concise.reasoning_content = obj.reasoning_content;
    }
    if (obj.tool_calls && !obj.messages && !obj.choices) {
        concise.tool_calls = simplifyToolCalls(obj.tool_calls);
    }

    // 用量与缓存
    const usage = simplifyUsage(obj.usage || obj.usageMetadata);
    if (usage) {
        concise.usage = usage;
    } else if (kind === 'response' && (log?.input_tokens || log?.output_tokens)) {
        const totalIn = (log.cached_tokens && log.cached_tokens > (log.input_tokens || 0))
            ? (log.input_tokens || 0) + log.cached_tokens
            : (log.input_tokens || 0);
        concise.usage = {
            input_tokens: totalIn,
            output_tokens: log.output_tokens,
            total_tokens: totalIn + (log.output_tokens || 0),
            ...(log.cached_tokens != null ? {
                cached_tokens: log.cached_tokens,
                cache_hit_rate: totalIn > 0 ? `${Math.min(100, Math.max(0, (log.cached_tokens / totalIn) * 100)).toFixed(1)}%` : undefined
            } : {})
        };
    }

    const substantiveKeys = Object.keys(concise).filter(k => k !== '_session_thinking_id');
    if (substantiveKeys.length === 0) {
        return rawStr;
    }

    return JSON.stringify(concise, null, 2);
}

interface StageTimingInfo {
    cleanSec?: number;
    normSec?: number;
    thinkingSec?: number;
    ttftSec?: number;
    streamSec?: number;
    totalSec?: number;
    isOldRecordWithoutStages?: boolean;
}

const parseTimingFromHeadersAndBody = (
    headersJson?: string,
    responseBody?: string,
    durationMs?: number
): StageTimingInfo | null => {
    let cleanSec: number | undefined;
    let normSec: number | undefined;
    let thinkingSec: number | undefined;
    let ttftSec: number | undefined;
    let streamSec: number | undefined;
    let totalSec: number | undefined;

    // 1. Check if responseBody has _timing object
    if (responseBody) {
        try {
            const bodyObj = JSON.parse(responseBody);
            if (bodyObj && typeof bodyObj === 'object' && bodyObj._timing) {
                const t = bodyObj._timing;
                if (typeof t.clean_s === 'number') cleanSec = t.clean_s;
                else if (typeof t.clean_ms === 'number') cleanSec = t.clean_ms / 1000;

                if (typeof t.norm_s === 'number') normSec = t.norm_s;
                else if (typeof t.norm_ms === 'number') normSec = t.norm_ms / 1000;

                if (typeof t.thinking_s === 'number') thinkingSec = t.thinking_s;
                else if (typeof t.thinking_ms === 'number') thinkingSec = t.thinking_ms / 1000;

                if (typeof t.ttft_s === 'number') ttftSec = t.ttft_s;
                else if (typeof t.ttft_ms === 'number') ttftSec = t.ttft_ms / 1000;

                if (typeof t.stream_s === 'number') streamSec = t.stream_s;
                else if (typeof t.stream_ms === 'number') streamSec = t.stream_ms / 1000;

                if (typeof t.total_s === 'number') totalSec = t.total_s;
                else if (typeof t.total_ms === 'number') totalSec = t.total_ms / 1000;
            }
        } catch {}
    }

    // 2. Parse from headersJson if any are still missing
    if (headersJson) {
        try {
            const headersObj = JSON.parse(headersJson);
            if (headersObj && typeof headersObj === 'object') {
                const getVal = (key: string): number | undefined => {
                    const matchKey = Object.keys(headersObj).find(
                        (k) => k.toLowerCase() === key.toLowerCase()
                    );
                    if (!matchKey) return undefined;
                    const v = headersObj[matchKey];
                    if (typeof v === 'number') return v;
                    if (typeof v === 'string') {
                        const parsed = parseFloat(v);
                        return isNaN(parsed) ? undefined : parsed;
                    }
                    if (Array.isArray(v) && v.length > 0) {
                        const parsed = parseFloat(String(v[0]));
                        return isNaN(parsed) ? undefined : parsed;
                    }
                    return undefined;
                };

                if (cleanSec === undefined) {
                    const ms = getVal('x-timing-clean-ms');
                    if (ms !== undefined) cleanSec = ms / 1000;
                }
                if (normSec === undefined) {
                    const ms = getVal('x-timing-norm-ms');
                    if (ms !== undefined) normSec = ms / 1000;
                }
                if (thinkingSec === undefined) {
                    const ms = getVal('x-timing-thinking-ms');
                    if (ms !== undefined) thinkingSec = ms / 1000;
                }
                if (ttftSec === undefined) {
                    const ms = getVal('x-timing-ttft-ms');
                    if (ms !== undefined) ttftSec = ms / 1000;
                }
                if (streamSec === undefined) {
                    const ms = getVal('x-timing-stream-ms');
                    if (ms !== undefined) streamSec = ms / 1000;
                }
                if (totalSec === undefined) {
                    const ms = getVal('x-timing-total-ms');
                    if (ms !== undefined) totalSec = ms / 1000;
                }
            }
        } catch {}
    }

    // 3. Fallback for totalSec if durationMs exists
    if (totalSec === undefined && durationMs !== undefined && durationMs > 0) {
        totalSec = durationMs / 1000;
    }

    // If we have neither totalSec nor any stages, return null
    if (totalSec === undefined && cleanSec === undefined && ttftSec === undefined) {
        return null;
    }

    const isOldRecordWithoutStages =
        cleanSec === undefined &&
        normSec === undefined &&
        thinkingSec === undefined &&
        ttftSec === undefined;

    return {
        cleanSec,
        normSec,
        thinkingSec,
        ttftSec,
        streamSec,
        totalSec,
        isOldRecordWithoutStages,
    };
};

const formatSeconds = (sec?: number): string => {
    if (sec === undefined || sec === null || isNaN(sec)) return '-';
    if (sec < 0.001) {
        return `${sec.toFixed(4)}s`;
    }
    if (sec < 1) {
        return `${sec.toFixed(3)}s`;
    }
    return `${sec.toFixed(2)}s`;
};

interface TimingDiagnosticsCardProps {
    timing: StageTimingInfo;
    onCopyText: (text: string) => void;
}

const TimingDiagnosticsCard: React.FC<TimingDiagnosticsCardProps> = ({ timing, onCopyText }) => {
    const { t } = useTranslation();
    const [isExpanded, setIsExpanded] = useState(false);
    const [isCopied, setIsCopied] = useState(false);

    const totalSec = timing.totalSec || 0;

    const stages = useMemo(() => [
        {
            key: 'clean',
            label: t('monitor.timing.clean', '会话清洗 (Clean)'),
            desc: t('monitor.timing.clean_desc', '清理缓存控制 / 合并同角色 / 历史提纯'),
            sec: timing.cleanSec,
            color: 'bg-indigo-500',
            textColor: 'text-indigo-600 dark:text-indigo-400',
        },
        {
            key: 'norm',
            label: t('monitor.timing.norm', '中转归一 (Normalize)'),
            desc: t('monitor.timing.norm_desc', '模型映射 / 账号调度 / 跨协议转换'),
            sec: timing.normSec,
            color: 'bg-purple-500',
            textColor: 'text-purple-600 dark:text-purple-400',
        },
        {
            key: 'thinking',
            label: t('monitor.timing.thinking', '思维块回填 (ThinkingStore)'),
            desc: t('monitor.timing.thinking_desc', '持久化思维链及补齐商业Agent历史签名'),
            sec: timing.thinkingSec,
            color: 'bg-amber-500',
            textColor: 'text-amber-600 dark:text-amber-400',
        },
        {
            key: 'ttft',
            label: t('monitor.timing.ttft', '等待首包 (TTFT)'),
            desc: t('monitor.timing.ttft_desc', '网关上送至接收首个数据包 (含首Token/思考块)'),
            sec: timing.ttftSec,
            color: 'bg-emerald-500',
            textColor: 'text-emerald-600 dark:text-emerald-400',
        },
        {
            key: 'stream',
            label: t('monitor.timing.stream', '流式传输 (Stream)'),
            desc: t('monitor.timing.stream_desc', '首个数据块到达至整条流式响应完成'),
            sec: timing.streamSec,
            color: 'bg-sky-500',
            textColor: 'text-sky-600 dark:text-sky-400',
        },
    ], [timing, t]);

    const handleCopy = (e: React.MouseEvent) => {
        e.stopPropagation();
        const lines: string[] = [];
        if (timing.cleanSec !== undefined) lines.push(`会话清洗 (Clean)：${formatSeconds(timing.cleanSec)}`);
        if (timing.normSec !== undefined) lines.push(`中转归一 (Normalize)：${formatSeconds(timing.normSec)}`);
        if (timing.thinkingSec !== undefined) lines.push(`思维块回填 (ThinkingStore)：${formatSeconds(timing.thinkingSec)}`);
        if (timing.ttftSec !== undefined) lines.push(`等待首包 (TTFT)：${formatSeconds(timing.ttftSec)}`);
        if (timing.streamSec !== undefined) lines.push(`流式传输 (Stream)：${formatSeconds(timing.streamSec)}`);
        lines.push(`总耗时：${formatSeconds(timing.totalSec)}`);

        onCopyText(lines.join('\n'));
        setIsCopied(true);
        setTimeout(() => setIsCopied(false), 2000);
    };

    if (timing.isOldRecordWithoutStages) {
        return (
            <div className="mb-3 rounded-xl overflow-hidden border border-gray-200 dark:border-base-300 bg-gray-100/50 dark:bg-base-200">
                <div className="px-3 py-2 bg-gray-200/60 dark:bg-base-200 border-b border-gray-200 dark:border-base-300 flex items-center justify-between">
                    <div className="flex items-center gap-2">
                        <Clock size={13} className="text-gray-500 dark:text-gray-400 shrink-0" />
                        <span className="text-xs font-bold tracking-wider text-gray-700 dark:text-gray-200 shrink-0 whitespace-nowrap">
                            {t('monitor.timing.title', '耗时诊断')}
                        </span>
                        <span className="px-2 py-0.5 rounded text-[11px] font-mono font-bold bg-emerald-50 text-emerald-700 dark:bg-emerald-950/70 dark:text-emerald-300 border border-emerald-200 dark:border-emerald-800/60">
                            {t('monitor.timing.total', '总耗时')}: {formatSeconds(timing.totalSec)}
                        </span>
                    </div>
                </div>
            </div>
        );
    }

    return (
        <div className="mb-3 rounded-xl overflow-hidden border border-emerald-500/30 dark:border-emerald-500/25 bg-emerald-50/25 dark:bg-base-100 shadow-sm">
            {/* Card Header */}
            <div
                className={`px-3 py-2 bg-emerald-500/10 dark:bg-emerald-950/30 flex items-center justify-between gap-2 select-none cursor-pointer hover:bg-emerald-500/15 transition-colors ${
                    isExpanded ? 'border-b border-emerald-500/20' : ''
                }`}
                onClick={() => setIsExpanded((prev) => !prev)}
            >
                <div className="flex items-center gap-2 min-w-0">
                    <Clock size={14} className="text-emerald-600 dark:text-emerald-400 shrink-0" />
                    <span className="text-xs font-bold tracking-wider text-emerald-950 dark:text-emerald-100 shrink-0 whitespace-nowrap">
                        {t('monitor.timing.title', '耗时诊断')}
                    </span>
                    {!isExpanded && totalSec > 0 && (
                        <span className="px-2 py-0.5 rounded text-[11px] font-mono font-bold bg-emerald-50 text-emerald-700 dark:bg-emerald-950/70 dark:text-emerald-300 border border-emerald-200 dark:border-emerald-800/60">
                            {t('monitor.timing.total', '总耗时')}: {formatSeconds(timing.totalSec)}
                        </span>
                    )}
                </div>

                <div className="flex items-center gap-1 shrink-0" onClick={(e) => e.stopPropagation()}>
                    <button
                        type="button"
                        onClick={handleCopy}
                        className="btn btn-ghost btn-xs h-6 px-2 text-emerald-800 dark:text-emerald-200 hover:bg-emerald-500/15 text-[11px] font-semibold gap-1"
                        title={isCopied ? t('common.copied', '已复制') : t('common.copy', '复制')}
                    >
                        {isCopied ? <CheckCircle size={12} className="text-emerald-500" /> : <Copy size={12} />}
                        <span>{isCopied ? t('common.copied', '已复制') : t('common.copy', '复制')}</span>
                    </button>
                    <button
                        type="button"
                        onClick={() => setIsExpanded((prev) => !prev)}
                        className="btn btn-ghost btn-xs p-1 h-6 min-h-0 text-emerald-800 dark:text-emerald-300 hover:bg-emerald-500/15"
                        title={isExpanded ? '收起耗时诊断' : '展开耗时诊断'}
                    >
                        <ChevronDown size={14} className={`transition-transform duration-200 ${isExpanded ? '' : '-rotate-90'}`} />
                    </button>
                </div>
            </div>

            {/* Expandable Body */}
            {isExpanded && (
                <div className="p-3 space-y-2.5 font-mono text-xs">
                    {/* Multi-stage Stacked Progress Bar */}
                    {totalSec > 0 && (
                        <div className="space-y-1">
                            <div className="h-2 w-full bg-gray-200/80 dark:bg-base-300 rounded-full flex overflow-hidden shadow-inner">
                                {stages.map((st) => {
                                    if (st.sec === undefined || st.sec <= 0) return null;
                                    const pct = Math.min(100, Math.max(0.5, (st.sec / totalSec) * 100));
                                    return (
                                        <div
                                            key={st.key}
                                            style={{ width: `${pct}%` }}
                                            className={`${st.color} h-full transition-all duration-300 relative group`}
                                            title={`${st.label}: ${formatSeconds(st.sec)} (${((st.sec / totalSec) * 100).toFixed(1)}%)`}
                                        />
                                    );
                                })}
                            </div>
                        </div>
                    )}

                    {/* Stage Metrics Grid */}
                    <div className="grid grid-cols-1 gap-1.5 pt-0.5">
                        {stages.map((st) => {
                            const hasVal = st.sec !== undefined;
                            const pct = hasVal && totalSec > 0 ? ((st.sec! / totalSec) * 100).toFixed(1) : undefined;
                            return (
                                <div
                                    key={st.key}
                                    className="flex items-center justify-between gap-2 px-2.5 py-1.5 rounded-lg bg-white dark:bg-base-200 border border-gray-200/80 dark:border-base-300/90 hover:border-emerald-500/40 transition-colors shadow-2xs"
                                >
                                    <div className="flex items-center gap-2.5 min-w-0">
                                        <span className={`w-2.5 h-2.5 rounded-full ${st.color} shrink-0`} />
                                        <div className="min-w-0">
                                            <span className="font-bold text-gray-900 dark:text-white truncate block text-xs">
                                                {st.label}
                                            </span>
                                            <span className="text-[10px] text-gray-500 dark:text-gray-400 truncate block">
                                                {st.desc}
                                            </span>
                                        </div>
                                    </div>

                                    <div className="flex items-baseline gap-2 shrink-0 text-right font-mono">
                                        <span className={`text-xs font-black ${hasVal ? st.textColor : 'text-gray-400'}`}>
                                            {formatSeconds(st.sec)}
                                        </span>
                                        {pct !== undefined && (
                                            <span className="text-[11px] font-bold text-gray-500 dark:text-gray-400 w-11 text-right">
                                                {pct}%
                                            </span>
                                        )}
                                    </div>
                                </div>
                            );
                        })}

                        {/* Total Duration Row */}
                        <div className="flex items-center justify-between gap-2 px-2.5 py-1.5 rounded-lg bg-emerald-500/15 dark:bg-emerald-950/50 border border-emerald-500/40 font-bold">
                            <div className="flex items-center gap-2 min-w-0">
                                <span className="w-2.5 h-2.5 rounded-full bg-emerald-500 shrink-0" />
                                <span className="text-emerald-950 dark:text-emerald-100 text-xs font-bold">
                                    {t('monitor.timing.total', '总耗时')}
                                </span>
                            </div>
                            <div className="flex items-baseline gap-2 shrink-0 text-right font-mono">
                                <span className="text-sm font-black text-emerald-800 dark:text-emerald-200">
                                    {formatSeconds(timing.totalSec)}
                                </span>
                                <span className="text-[11px] text-emerald-700/80 dark:text-emerald-300/80 w-11 text-right font-bold">
                                    100%
                                </span>
                            </div>
                        </div>
                    </div>
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
    const [isClearConfirmOpen, setIsClearConfirmOpen] = useState(false);
    const [payloadViewMode, setPayloadViewMode] = useState<'concise' | 'full'>('concise');
    const [showMetadata, setShowMetadata] = useState(true);
    const [copiedCard, setCopiedCard] = useState<string | null>(null);

    // 日志存储与维护配置状态
    const [showLogSettings, setShowLogSettings] = useState(false);
    const [appConfig, setAppConfig] = useState<AppConfig | null>(null);
    const [isSavingConfig, setIsSavingConfig] = useState(false);
    const [saveSuccess, setSaveSuccess] = useState(false);
    const [isClearCacheModalOpen, setIsClearCacheModalOpen] = useState(false);
    const [cacheClearedSuccess, setCacheClearedSuccess] = useState(false);
    const [dbDiskSizeBytes, setDbDiskSizeBytes] = useState<number | null>(null);

    const fetchDbDiskSize = useCallback(async () => {
        try {
            const bytes = await invoke<number>('get_proxy_db_disk_size');
            setDbDiskSizeBytes(bytes);
        } catch (e) {
            console.error('Failed to get proxy db disk size', e);
        }
    }, []);

    useEffect(() => {
        if (showLogSettings) {
            fetchDbDiskSize();
        }
    }, [showLogSettings, fetchDbDiskSize]);

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

    const conciseRequestBody = useMemo(() => {
        return selectedLog?.request_body
            ? extractConcisePayload(selectedLog.request_body, 'request', selectedLog)
            : '';
    }, [selectedLog?.request_body, selectedLog?.id]);

    const conciseUpstreamBody = useMemo(() => {
        return selectedLog?.upstream_request_body
            ? extractConcisePayload(selectedLog.upstream_request_body, 'upstream', selectedLog)
            : '';
    }, [selectedLog?.upstream_request_body, selectedLog?.id]);

    const conciseResponseBody = useMemo(() => {
        return selectedLog?.response_body
            ? extractConcisePayload(selectedLog.response_body, 'response', selectedLog)
            : '';
    }, [selectedLog?.response_body, selectedLog?.id, selectedLog?.input_tokens, selectedLog?.output_tokens, selectedLog?.cached_tokens]);

    const timingInfo = useMemo(() => {
        return parseTimingFromHeadersAndBody(
            selectedLog?.response_headers,
            selectedLog?.response_body,
            selectedLog?.duration
        );
    }, [selectedLog?.response_headers, selectedLog?.response_body, selectedLog?.duration]);

    const timingNode = timingInfo ? (
        <div className="p-2.5">
            <TimingDiagnosticsCard
                key={selectedLog?.id}
                timing={timingInfo}
                onCopyText={async (text) => {
                    const success = await copyToClipboard(text);
                    if (success) {
                        setCopiedCard('timing');
                        setTimeout(() => setCopiedCard(null), 2000);
                    }
                }}
            />
        </div>
    ) : undefined;

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

                        // Fetch stats and total count from backend instead of local calculation
                        try {
                            const [currentStats, count] = await Promise.all([
                                invoke<ProxyStats>('get_proxy_stats'),
                                invoke<number>('get_proxy_logs_count_filtered', { filter: '', errorsOnly: false })
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

    useEffect(() => {
        setCopiedCard(null);
    }, [selectedLog?.id]);

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
        const currentRetention = appConfig.proxy?.log_retention || { max_body_age_hours: 24, max_storage_gb: 0.5, max_rows: 100000 };
        const safeVal = field === 'max_storage_gb'
            ? Math.max(0.1, isNaN(value) ? 0.5 : value)
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
                        className={`btn btn-sm gap-2 px-3.5 border font-semibold rounded-lg transition-all ${isLoggingEnabled
                            ? 'bg-rose-600 hover:bg-rose-700 border-rose-600 text-white shadow-xs'
                            : 'bg-white dark:bg-base-200 border-gray-300 dark:border-base-300 text-gray-700 dark:text-gray-200 hover:bg-gray-50 dark:hover:bg-base-300/80 shadow-2xs'
                            }`}
                    >
                        <div className={`w-2.5 h-2.5 rounded-full ${isLoggingEnabled ? 'bg-white' : 'bg-gray-400'}`} />
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
                        title={t('monitor.filters.capture_health_tip', { defaultValue: '默认关闭：过滤全部 GET 成功请求（含 /v1/models 模型列表轮询与 /health 探针）且不入库；失败请求始终记录；开启后才全部记录并落库' })}
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
                                {t('monitor.settings.subtitle', { defaultValue: '统一管理请求日志保留天数、思考块滑动窗口与磁盘空间回收' })}
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

                    {/* 2-Column Balanced Settings Grid */}
                    <div className="grid grid-cols-1 md:grid-cols-2 gap-3.5">
                        {/* 1. 请求日志与报文保留策略 */}
                        <div className="p-3.5 bg-white dark:bg-base-100 rounded-xl border border-gray-200/90 dark:border-base-200 shadow-xs flex flex-col justify-between space-y-3">
                            <div className="space-y-3">
                                <span className="text-xs font-bold text-gray-800 dark:text-gray-200 flex items-center gap-1.5">
                                    <Clock size={13} className="text-indigo-500 dark:text-indigo-400" />
                                    {t('monitor.settings.retention_title', { defaultValue: '请求日志与报文保留策略 (滑动窗口)' })}
                                </span>
                                <div className="space-y-2.5">
                                    {/* 空间上限 */}
                                    <div>
                                        <div className="flex items-center justify-between mb-1">
                                            <label className="text-xs font-medium text-gray-600 dark:text-gray-300">
                                                {t('proxy.config.log_retention_storage_gb', { defaultValue: '日志保留空间上限 (GB)' })}
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
                                            {t('proxy.config.log_retention_storage_gb_desc', { defaultValue: '完全由容量上限滑动窗口托管，保留完整报文不被提前掏空；达到上限自动淘汰最尾部 30% 记录' })}
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

                        {/* 2. 维护与清理操作 */}
                        <div className="p-3.5 bg-white dark:bg-base-100 rounded-xl border border-gray-200/90 dark:border-base-200 shadow-xs flex flex-col justify-between space-y-3">
                            <div>
                                <span className="text-xs font-bold text-gray-800 dark:text-gray-200 flex items-center gap-1.5 mb-1.5">
                                    <HardDrive size={13} className="text-amber-500 dark:text-amber-400" />
                                    {t('monitor.settings.maintenance_title', { defaultValue: '日志维护与空间清理' })}
                                </span>
                                <p className="text-xs text-gray-500 dark:text-gray-400 leading-relaxed">
                                    {t('settings.advanced.logs_desc', { defaultValue: '清理应用产生的日志缓存文件或清空全部历史请求记录，释放磁盘空间。' })}
                                </p>
                            </div>
                            <div className="space-y-2 pt-2">
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
                                    <p className="text-xs text-emerald-600 dark:text-emerald-400 text-center font-medium">
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
                <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-sm p-2 sm:p-3 md:p-4" onClick={() => setSelectedLog(null)}>
                    <div className="bg-white dark:bg-base-100 rounded-2xl shadow-2xl w-full max-w-[98vw] xl:max-w-[1720px] h-[94vh] max-h-[94vh] flex flex-col overflow-hidden border border-gray-200 dark:border-base-200" onClick={e => e.stopPropagation()}>
                        {/* Modal Header */}
                        <div className="px-4 py-2.5 border-b border-gray-200 dark:border-base-300 flex items-center justify-between bg-gray-50 dark:bg-base-200 shrink-0">
                            <div className="flex items-center gap-3 min-w-0">
                                {loadingDetail && <div className="loading loading-spinner loading-sm shrink-0"></div>}
                                <span className={`badge badge-sm font-bold text-white border-none shrink-0 shadow-xs ${
                                    selectedLog.status >= 200 && selectedLog.status < 400
                                        ? 'bg-emerald-600'
                                        : 'bg-rose-600'
                                }`}>
                                    {selectedLog.status}
                                </span>
                                <span className="font-mono font-bold text-gray-900 dark:text-white text-sm shrink-0">{selectedLog.method}</span>
                                <span className="text-xs text-gray-500 dark:text-gray-400 font-mono truncate max-w-lg hidden sm:inline" title={selectedLog.url}>{selectedLog.url}</span>
                            </div>
                            <button onClick={() => setSelectedLog(null)} className="btn btn-ghost btn-sm btn-circle text-gray-500 dark:text-gray-400 hover:bg-gray-200 dark:hover:bg-base-200" aria-label="关闭"><X size={18} /></button>
                        </div>

                        {/* Modal Content */}
                        <div className="flex-1 min-h-0 flex flex-col p-3 sm:p-4 space-y-2.5 bg-gray-100/50 dark:bg-base-100 overflow-hidden">
                            {/* Metadata Section (Collapsible) */}
                            {showMetadata && (
                                <div className="bg-white dark:bg-base-200 p-3 sm:p-3.5 rounded-xl border border-gray-200 dark:border-base-300 shadow-sm shrink-0 text-xs transition-all duration-200">
                                    <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-6 gap-3">
                                        <div>
                                            <span className="block text-gray-500 dark:text-gray-400 uppercase font-bold text-[10px] tracking-wider">{t('monitor.details.time')}</span>
                                            <span className="font-mono font-semibold text-gray-900 dark:text-white text-xs truncate block" title={new Date(selectedLog.timestamp).toLocaleString()}>{new Date(selectedLog.timestamp).toLocaleString()}</span>
                                        </div>
                                        <div>
                                            <span className="block text-gray-500 dark:text-gray-400 uppercase font-bold text-[10px] tracking-wider">{t('monitor.details.duration')}</span>
                                            <span className="font-mono font-semibold text-gray-900 dark:text-white text-xs">{selectedLog.duration}ms</span>
                                        </div>
                                        <div>
                                            <span className="block text-gray-500 dark:text-gray-400 uppercase font-bold text-[10px] tracking-wider">{t('monitor.details.tokens')}</span>
                                            <div className="font-mono text-[11px] flex items-center gap-1.5 mt-0.5">
                                                {(() => {
                                                    const totalIn = (selectedLog.cached_tokens && selectedLog.cached_tokens > (selectedLog.input_tokens ?? 0))
                                                        ? (selectedLog.input_tokens ?? 0) + selectedLog.cached_tokens
                                                        : (selectedLog.input_tokens ?? 0);
                                                    return (
                                                        <span className="text-blue-700 dark:text-blue-300 bg-blue-100 dark:bg-blue-900/40 px-1.5 py-0.5 rounded font-bold" title={`Total Input Tokens: ${totalIn}`}>
                                                            In: {formatCompactNumber(totalIn)}
                                                        </span>
                                                    );
                                                })()}
                                                <span className="text-emerald-700 dark:text-emerald-300 bg-emerald-100 dark:bg-emerald-900/40 px-1.5 py-0.5 rounded font-bold">Out: {formatCompactNumber(selectedLog.output_tokens ?? 0)}</span>
                                                {selectedLog.cached_tokens != null && selectedLog.cached_tokens > 0 && (() => {
                                                    const totalIn = (selectedLog.cached_tokens && selectedLog.cached_tokens > (selectedLog.input_tokens ?? 0))
                                                        ? (selectedLog.input_tokens ?? 0) + selectedLog.cached_tokens
                                                        : (selectedLog.input_tokens ?? 0);
                                                    const hitRate = totalIn > 0 ? Math.min(100, Math.max(0, (selectedLog.cached_tokens / totalIn) * 100)) : 0;
                                                    const hitRateText = totalIn > 0 ? (hitRate >= 100 ? '100%' : (hitRate % 1 === 0 ? `${hitRate.toFixed(0)}%` : `${hitRate.toFixed(1)}%`)) : '';
                                                    return (
                                                        <span
                                                            className="text-purple-700 dark:text-purple-300 bg-purple-100 dark:bg-purple-900/40 px-1.5 py-0.5 rounded font-bold"
                                                            title={`Cache: ${selectedLog.cached_tokens.toLocaleString()}${hitRateText ? ` (${hitRateText})` : ''}`}
                                                        >
                                                            Cache: {formatCompactNumber(selectedLog.cached_tokens)}{hitRateText ? ` (${hitRateText})` : ''}
                                                        </span>
                                                    );
                                                })()}
                                            </div>
                                        </div>
                                        <div>
                                            <span className="block text-gray-500 dark:text-gray-400 uppercase font-bold text-[10px] tracking-wider">{t('monitor.details.protocol')}</span>
                                            <span className={`inline-block px-2 py-0.5 rounded-full font-mono font-bold text-[11px] uppercase mt-0.5 text-white shadow-xs ${
                                                selectedLog.protocol === 'openai' ? 'bg-emerald-600' :
                                                selectedLog.protocol === 'anthropic' ? 'bg-amber-600' :
                                                selectedLog.protocol === 'gemini' ? 'bg-blue-600' :
                                                'bg-gray-600'
                                            }`}>
                                                {selectedLog.protocol || '-'}
                                            </span>
                                        </div>
                                        <div>
                                            <span className="block text-gray-500 dark:text-gray-400 uppercase font-bold text-[10px] tracking-wider">{t('monitor.details.model')}</span>
                                            <span className="font-mono font-bold text-blue-600 dark:text-blue-400 truncate block text-xs" title={selectedLog.model}>{selectedLog.model || '-'}</span>
                                            {selectedLog.mapped_model && selectedLog.model !== selectedLog.mapped_model && (
                                                <span className="font-mono text-emerald-600 dark:text-emerald-400 truncate block text-[11px]" title={selectedLog.mapped_model}>➔ {selectedLog.mapped_model}</span>
                                            )}
                                        </div>
                                        <div>
                                            <span className="block text-gray-500 dark:text-gray-400 uppercase font-bold text-[10px] tracking-wider">{t('monitor.details.account_used')}</span>
                                            <span className="font-mono font-medium text-gray-900 dark:text-white truncate block text-xs" title={selectedLog.account_email || '-'}>{selectedLog.account_email || '-'}</span>
                                        </div>
                                    </div>
                                </div>
                            )}

                            {/* Mode & Toolbar Bar */}
                            <div className="flex flex-wrap items-center justify-between gap-2 px-1 shrink-0">
                                <div className="flex items-center gap-2">
                                    <div className="inline-flex items-center p-1 bg-gray-200/70 dark:bg-base-200 rounded-xl border border-gray-300/70 dark:border-base-300 gap-1 shadow-inner">
                                        <button
                                            type="button"
                                            onClick={() => setPayloadViewMode('concise')}
                                            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-semibold transition-all duration-150 cursor-pointer select-none ${
                                                payloadViewMode === 'concise'
                                                    ? 'bg-white dark:bg-base-100 text-blue-600 dark:text-blue-400 shadow-sm border border-gray-200 dark:border-base-300'
                                                    : 'text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white'
                                            }`}
                                        >
                                            <Sparkles size={13} className={payloadViewMode === 'concise' ? 'text-blue-600 dark:text-blue-400' : 'text-gray-400'} />
                                            <span>{t('monitor.details.concise_mode', '简要模式')}</span>
                                        </button>
                                        <button
                                            type="button"
                                            onClick={() => setPayloadViewMode('full')}
                                            className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-semibold transition-all duration-150 cursor-pointer select-none ${
                                                payloadViewMode === 'full'
                                                    ? 'bg-white dark:bg-base-100 text-blue-600 dark:text-blue-400 shadow-sm border border-gray-200 dark:border-base-300'
                                                    : 'text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white'
                                            }`}
                                        >
                                            <FileCode2 size={13} className={payloadViewMode === 'full' ? 'text-blue-600 dark:text-blue-400' : 'text-gray-400'} />
                                            <span>{t('monitor.details.full_mode', '完整模式')}</span>
                                        </button>
                                    </div>
                                    <span className="hidden sm:inline-block text-[11px] text-gray-500 dark:text-gray-400">
                                        {payloadViewMode === 'concise'
                                            ? t('monitor.details.concise_desc', '已为您精简工具参数与冗余字段，突出思考块、用量与对话主体')
                                            : '显示原始完整未修剪报文'}
                                    </span>
                                </div>

                                <div className="flex items-center gap-2">
                                    <button
                                        type="button"
                                        onClick={() => setShowMetadata((prev) => !prev)}
                                        className="btn btn-xs btn-ghost text-gray-500 dark:text-gray-400 hover:bg-gray-200 dark:hover:bg-base-200 gap-1 text-[11px]"
                                        title={showMetadata ? '折叠元数据以增大报文视野' : '展开元数据信息'}
                                    >
                                        {showMetadata ? <EyeOff size={13} /> : <Eye size={13} />}
                                        <span>{showMetadata ? '收起元数据' : '展开元数据'}</span>
                                    </button>
                                </div>
                            </div>

                            {/* Horizontal 3-Column Grid */}
                            <div className="grid grid-cols-1 lg:grid-cols-3 gap-3 flex-1 min-h-0 overflow-hidden">
                                <VirtualizedPayloadViewer
                                    cardId="req"
                                    title={t('monitor.details.request_payload', '请求报文 (Request)')}
                                    badge="REQUEST"
                                    badgeStyle="bg-blue-50 text-blue-700 dark:bg-blue-900/30 dark:text-blue-300 border-blue-200 dark:border-blue-800/60"
                                    rawPayload={selectedLog.request_body}
                                    concisePayload={conciseRequestBody}
                                    headersJson={selectedLog.request_headers}
                                    viewMode={payloadViewMode}
                                    emptyPlaceholder={t('monitor.details.payload_empty', '无请求报文')}
                                    onCopy={async (text) => {
                                        const success = await copyToClipboard(text);
                                        if (success) {
                                            setCopiedCard('req');
                                            setTimeout(() => setCopiedCard(null), 2000);
                                        }
                                    }}
                                    isCopied={copiedCard === 'req'}
                                />
                                <VirtualizedPayloadViewer
                                    cardId="upstream"
                                    title={t('monitor.details.upstream_request_payload', '中转报文 (Forwarded)')}
                                    badge="FORWARDED"
                                    badgeStyle="bg-amber-50 text-amber-700 dark:bg-amber-900/30 dark:text-amber-300 border-amber-200 dark:border-amber-800/60"
                                    rawPayload={selectedLog.upstream_request_body}
                                    concisePayload={conciseUpstreamBody}
                                    headersJson={selectedLog.upstream_request_headers}
                                    viewMode={payloadViewMode}
                                    emptyPlaceholder={t('monitor.details.no_upstream_payload', '无中转报文 (直接转发或未记录)')}
                                    onCopy={async (text) => {
                                        const success = await copyToClipboard(text);
                                        if (success) {
                                            setCopiedCard('upstream');
                                            setTimeout(() => setCopiedCard(null), 2000);
                                        }
                                    }}
                                    isCopied={copiedCard === 'upstream'}
                                />
                                <VirtualizedPayloadViewer
                                    cardId="resp"
                                    title={t('monitor.details.response_payload', '响应报文 (Response)')}
                                    badge="RESPONSE"
                                    badgeStyle="bg-emerald-50 text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-300 border-emerald-200 dark:border-emerald-800/60"
                                    rawPayload={selectedLog.response_body}
                                    concisePayload={conciseResponseBody}
                                    headersJson={selectedLog.response_headers}
                                    viewMode={payloadViewMode}
                                    emptyPlaceholder={t('monitor.details.payload_empty', '无响应报文')}
                                    duration={selectedLog.duration}
                                    timingNode={timingNode}
                                    onCopy={async (text) => {
                                        const success = await copyToClipboard(text);
                                        if (success) {
                                            setCopiedCard('resp');
                                            setTimeout(() => setCopiedCard(null), 2000);
                                        }
                                    }}
                                    isCopied={copiedCard === 'resp'}
                                />
                            </div>
                        </div>
                    </div>
                </div>
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
