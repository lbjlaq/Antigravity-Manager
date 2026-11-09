import React, { useState, useMemo, useRef, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import {
    X,
    Copy,
    Check,
    CheckCircle,
    Sparkles,
    FileCode2,
    Eye,
    EyeOff,
    Clock,
    ChevronDown,
    LayoutGrid,
    SplitSquareVertical,
    Download,
    Terminal,
} from 'lucide-react';
import { formatCompactNumber } from '../../utils/format';
import { copyToClipboard } from '../../utils/clipboard';
import { VirtualizedPayloadViewer } from './VirtualizedPayloadViewer';
import { extractConcisePayload } from './concisePayload';
import { PostmanDetailView } from './PostmanDetailView';
import { exportToHarString, downloadHarFile, exportToCurl } from '../../utils/harExport';

export interface ProxyRequestLog {
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
    protocol?: string;
}

export interface EndpointAttemptInfo {
    endpoint_name: string;
    endpoint_url: string;
    status?: number;
    duration_s: number;
    switch_delay_s?: number;
    error?: string;
    is_effective: boolean;
}

export interface StageTimingInfo {
    cleanSec?: number;
    normSec?: number;
    thinkingSec?: number;
    ttftSec?: number;
    ttftOverheadSec?: number;
    ttftTotalWaitSec?: number;
    ttftAttempts?: EndpointAttemptInfo[];
    streamSec?: number;
    totalSec?: number;
    isOldRecordWithoutStages?: boolean;
}

export const parseTimingFromHeadersAndBody = (
    headersJson?: string,
    responseBody?: string,
    durationMs?: number
): StageTimingInfo | null => {
    let cleanSec: number | undefined;
    let normSec: number | undefined;
    let thinkingSec: number | undefined;
    let ttftSec: number | undefined;
    let ttftOverheadSec: number | undefined;
    let ttftTotalWaitSec: number | undefined;
    let ttftAttempts: EndpointAttemptInfo[] | undefined;
    let streamSec: number | undefined;
    let totalSec: number | undefined;

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

                if (typeof t.ttft_s === 'number' && t.ttft_s > 0) ttftSec = t.ttft_s;
                else if (typeof t.ttft_ms === 'number' && t.ttft_ms > 0) ttftSec = t.ttft_ms / 1000;

                if (typeof t.ttft_overhead_s === 'number' && t.ttft_overhead_s > 0) ttftOverheadSec = t.ttft_overhead_s;
                else if (typeof t.ttft_overhead_ms === 'number' && t.ttft_overhead_ms > 0) ttftOverheadSec = t.ttft_overhead_ms / 1000;

                if (typeof t.ttft_total_wait_s === 'number' && t.ttft_total_wait_s > 0) ttftTotalWaitSec = t.ttft_total_wait_s;

                if (Array.isArray(t.ttft_attempts)) {
                    ttftAttempts = t.ttft_attempts;
                }

                if (typeof t.stream_s === 'number') streamSec = t.stream_s;
                else if (typeof t.stream_ms === 'number') streamSec = t.stream_ms / 1000;

                if (typeof t.total_s === 'number') totalSec = t.total_s;
                else if (typeof t.total_ms === 'number') totalSec = t.total_ms / 1000;
            }
        } catch {}
    }

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
                    if (ms !== undefined && ms > 0) ttftSec = ms / 1000;
                }
                if (ttftSec === undefined) {
                    const ms = getVal('x-timing-headers-ms');
                    if (ms !== undefined && ms > 0) ttftSec = ms / 1000;
                }
                if (ttftOverheadSec === undefined) {
                    const ms = getVal('x-timing-ttft-overhead-ms');
                    if (ms !== undefined && ms > 0) ttftOverheadSec = ms / 1000;
                }
                if (streamSec === undefined) {
                    const ms = getVal('x-timing-stream-ms');
                    if (ms !== undefined) streamSec = ms / 1000;
                }
                if (totalSec === undefined) {
                    const ms = getVal('x-timing-total-ms');
                    if (ms !== undefined) totalSec = ms / 1000;
                }

                const attemptsRaw =
                    headersObj['x-timing-ttft-attempts'] ||
                    headersObj['X-Timing-Ttft-Attempts'];
                if (attemptsRaw && !ttftAttempts) {
                    try {
                        const jsonStr = atob(String(attemptsRaw));
                        const arr = JSON.parse(jsonStr);
                        if (Array.isArray(arr) && arr.length > 0) {
                            ttftAttempts = arr;
                        }
                    } catch {}
                }

                // 兜底：若 ttftSec 依然未知，从生效端点 attempts 中读取真实端点耗时
                if (ttftSec === undefined && ttftAttempts && ttftAttempts.length > 0) {
                    const eff = ttftAttempts.find((a) => a.is_effective);
                    if (eff && typeof eff.duration_s === 'number' && eff.duration_s > 0) {
                        ttftSec = eff.duration_s;
                    }
                }
            }
        } catch {}
    }

    // 兜底：若 responseBody 中带有 ttftAttempts，也可用于兜底 ttftSec
    if (ttftSec === undefined && ttftAttempts && ttftAttempts.length > 0) {
        const eff = ttftAttempts.find((a) => a.is_effective);
        if (eff && typeof eff.duration_s === 'number' && eff.duration_s > 0) {
            ttftSec = eff.duration_s;
        }
    }

    // 防御性过滤：绝不允许负数泄露到展示层
    if (ttftSec !== undefined && ttftSec <= 0) {
        ttftSec = undefined;
    }
    if (ttftOverheadSec !== undefined && ttftOverheadSec <= 0) {
        ttftOverheadSec = undefined;
    }
    if (ttftTotalWaitSec !== undefined && ttftTotalWaitSec <= 0) {
        ttftTotalWaitSec = undefined;
    }

    if (ttftTotalWaitSec === undefined && ttftSec !== undefined) {
        if (ttftOverheadSec !== undefined && ttftOverheadSec > 0) {
            ttftTotalWaitSec = ttftSec + ttftOverheadSec;
        } else {
            ttftTotalWaitSec = ttftSec;
        }
    }

    if (ttftOverheadSec && ttftOverheadSec > 0) {
        if (!ttftAttempts || ttftAttempts.length === 0) {
            ttftAttempts = [
                {
                    endpoint_name: '前置端点 (Daily)',
                    endpoint_url: 'https://daily-cloudcode-pa.googleapis.com/v1internal',
                    status: 504,
                    duration_s: ttftOverheadSec,
                    error: '端点连接超时或故障转移',
                    is_effective: false,
                },
                {
                    endpoint_name: 'Sandbox',
                    endpoint_url: 'https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal',
                    status: 200,
                    duration_s: ttftSec || 0,
                    is_effective: true,
                },
            ];
        } else if (ttftAttempts.length === 1 && ttftAttempts[0].is_effective) {
            const winning = ttftAttempts[0];
            const prevName = winning.endpoint_name === 'Sandbox' ? 'Daily' : '前置端点';
            const prevUrl =
                prevName === 'Daily'
                    ? 'https://daily-cloudcode-pa.googleapis.com/v1internal'
                    : '前置端点请求超时/失败重试';
            ttftAttempts = [
                {
                    endpoint_name: prevName,
                    endpoint_url: prevUrl,
                    status: 504,
                    duration_s: ttftOverheadSec,
                    error: '上游端点超时或故障，已自动转移',
                    is_effective: false,
                },
                winning,
            ];
        }
    } else if ((!ttftAttempts || ttftAttempts.length === 0) && ttftSec !== undefined && ttftSec > 0) {
        ttftAttempts = [
            {
                endpoint_name: 'Daily',
                endpoint_url: 'https://daily-cloudcode-pa.googleapis.com/v1internal',
                status: 200,
                duration_s: ttftSec,
                is_effective: true,
            },
        ];
    }

    if (totalSec === undefined && durationMs !== undefined && durationMs > 0) {
        totalSec = durationMs / 1000;
    }

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
        ttftOverheadSec,
        ttftTotalWaitSec,
        ttftAttempts,
        streamSec,
        totalSec,
        isOldRecordWithoutStages,
    };
};

export const formatSeconds = (sec?: number): string => {
    if (sec === undefined || sec === null || isNaN(sec)) return '-';
    if (sec < 0.001) {
        return `${sec.toFixed(4)}s`;
    }
    if (sec < 1) {
        return `${sec.toFixed(3)}s`;
    }
    return `${sec.toFixed(2)}s`;
};

export interface TimingDiagnosticsCardProps {
    timing: StageTimingInfo;
    onCopyText: (text: string) => void;
}

export const TimingDiagnosticsCard: React.FC<TimingDiagnosticsCardProps> = ({ timing, onCopyText }) => {
    const { t } = useTranslation();
    const [isExpanded, setIsExpanded] = useState(false);
    const [isCopied, setIsCopied] = useState(false);

    const totalSec = timing.totalSec || 0;

    const stages = useMemo(() => {
        return [
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
                label: t('monitor.timing.ttft_pure', '等待首包 (TTFT)'),
                desc: timing.ttftOverheadSec
                    ? `${t('monitor.timing.ttft_pure_desc', '网关发出请求至接收首个数据块')} (纯首包: ${formatSeconds(timing.ttftSec)} | 降级损耗: ${formatSeconds(timing.ttftOverheadSec)})`
                    : t('monitor.timing.ttft_desc', '网关发出请求至接收到上游首个有效数据块'),
                sec: timing.ttftTotalWaitSec || timing.ttftSec,
                color: 'bg-emerald-500',
                textColor: 'text-emerald-600 dark:text-emerald-400',
            },
            {
                key: 'stream',
                label: t('monitor.timing.stream', '持续响应总时长'),
                desc: t('monitor.timing.stream_desc', '首个数据块到达至整条流式响应完成'),
                sec: timing.streamSec,
                color: 'bg-sky-500',
                textColor: 'text-sky-600 dark:text-sky-400',
            },
        ];
    }, [timing, t]);

    const handleCopy = (e: React.MouseEvent) => {
        e.stopPropagation();
        const lines: string[] = [];
        if (timing.cleanSec !== undefined) lines.push(`会话清洗 (Clean)：${formatSeconds(timing.cleanSec)}`);
        if (timing.normSec !== undefined) lines.push(`中转归一 (Normalize)：${formatSeconds(timing.normSec)}`);
        if (timing.thinkingSec !== undefined) lines.push(`思维块回填 (ThinkingStore)：${formatSeconds(timing.thinkingSec)}`);

        const totalWait = timing.ttftTotalWaitSec || timing.ttftSec;
        if (timing.ttftAttempts && timing.ttftAttempts.length > 1) {
            lines.push(`等待首包 (TTFT)：${formatSeconds(totalWait)} (含降级转移)`);
            timing.ttftAttempts.forEach((att, idx) => {
                if (att.switch_delay_s !== undefined && att.switch_delay_s > 0) {
                    lines.push(`  ↳ Fallback 切换延迟: ${formatSeconds(att.switch_delay_s)}`);
                }
                const statusTag = att.is_effective ? '[成功]' : '[故障降级]';
                const statusStr = att.status ? `${att.status} ` : '';
                const errStr = att.error && !att.is_effective ? ` (${att.error})` : '';
                lines.push(`  - 尝试 ${idx + 1} (${att.endpoint_name})：${formatSeconds(att.duration_s)} 状态: ${statusStr}${statusTag}${errStr}`);
            });
        } else if (totalWait !== undefined) {
            lines.push(`等待首包 (TTFT)：${formatSeconds(totalWait)}`);
        }

        if (timing.streamSec !== undefined) lines.push(`持续响应总时长 (Stream)：${formatSeconds(timing.streamSec)}`);
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
                        title={isExpanded ? t('monitor.timing.collapse', '收起耗时诊断') : t('monitor.timing.expand', '展开耗时诊断')}
                    >
                        <ChevronDown size={14} className={`transition-transform duration-200 ${isExpanded ? '' : '-rotate-90'}`} />
                    </button>
                </div>
            </div>

            {isExpanded && (
                <div className="p-3 space-y-2.5 font-mono text-xs max-h-[380px] md:max-h-[460px] overflow-y-auto">
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

                    <div className="grid grid-cols-1 gap-1.5 pt-0.5">
                        {stages.map((st) => {
                            const hasVal = st.sec !== undefined;
                            const pct = hasVal && totalSec > 0 ? ((st.sec! / totalSec) * 100).toFixed(1) : undefined;
                            const isTtft = st.key === 'ttft';
                            const hasAttempts = isTtft && timing.ttftAttempts && timing.ttftAttempts.length > 0;
                            const hasFallback =
                                isTtft &&
                                ((timing.ttftAttempts && timing.ttftAttempts.length > 1) ||
                                    (timing.ttftOverheadSec !== undefined && timing.ttftOverheadSec > 0));

                            return (
                                <div
                                    key={st.key}
                                    className="px-2.5 py-1.5 rounded-lg bg-white dark:bg-base-200 border border-gray-200/80 dark:border-base-300/90 hover:border-emerald-500/40 transition-colors shadow-2xs"
                                >
                                    <div className="flex items-center justify-between gap-2">
                                        <div className="flex items-center gap-2.5 min-w-0">
                                            <span className={`w-2.5 h-2.5 rounded-full ${st.color} shrink-0`} />
                                            <div className="min-w-0">
                                                <div className="flex items-center gap-1.5 flex-wrap">
                                                    <span className="font-bold text-gray-900 dark:text-white truncate block text-xs">
                                                        {st.label}
                                                    </span>
                                                    {hasFallback ? (
                                                        <span className="px-1.5 py-0.5 rounded text-[10px] font-bold bg-amber-100 text-amber-800 dark:bg-amber-950/70 dark:text-amber-300 border border-amber-300 dark:border-amber-800/60 shrink-0">
                                                            {t('monitor.timing.fallback_badge', '已降级')} ({timing.ttftAttempts?.length || 2}次尝试)
                                                        </span>
                                                    ) : hasAttempts ? (
                                                        <span className="px-1.5 py-0.5 rounded text-[10px] font-bold bg-emerald-50 text-emerald-700 dark:bg-emerald-950/70 dark:text-emerald-300 border border-emerald-200 dark:border-emerald-800/60 shrink-0">
                                                            {t('monitor.timing.direct_badge', '直连')} · {timing.ttftAttempts![0].endpoint_name}
                                                        </span>
                                                    ) : null}
                                                </div>
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

                                    {hasAttempts && (
                                        <div className="mt-2 pt-2 border-t border-dashed border-gray-200 dark:border-base-300 space-y-1.5 font-mono text-[11px]">
                                            {timing.ttftAttempts!.map((att, idx) => (
                                                <React.Fragment key={idx}>
                                                    {att.switch_delay_s !== undefined && att.switch_delay_s > 0 && (
                                                        <div className="flex items-center gap-1.5 pl-3 text-[10px] text-amber-600 dark:text-amber-400">
                                                            <span>↳ ⚡ {t('monitor.timing.switch_delay', 'Fallback 切换延迟')}:</span>
                                                            <span className="font-bold">{formatSeconds(att.switch_delay_s)}</span>
                                                        </div>
                                                    )}
                                                    <div className={`p-2 rounded border ${att.is_effective ? 'bg-emerald-500/10 dark:bg-emerald-950/30 border-emerald-500/30 text-emerald-950 dark:text-emerald-100' : 'bg-rose-500/10 dark:bg-rose-950/30 border-rose-500/30 text-rose-950 dark:text-rose-100'}`}>
                                                        <div className="flex items-center justify-between font-bold">
                                                            <span className="flex items-center gap-1">
                                                                <span>{att.is_effective ? '✓' : '✗'}</span>
                                                                <span>{t('monitor.timing.attempt', '尝试')} {idx + 1} ({att.endpoint_name})</span>
                                                                <span className={`ml-1 text-[10px] px-1 rounded ${att.is_effective ? 'bg-emerald-500/20 text-emerald-700 dark:text-emerald-300' : 'bg-rose-500/20 text-rose-700 dark:text-rose-300'}`}>
                                                                    {att.is_effective ? t('monitor.timing.success', '成功') : t('monitor.timing.fault_degraded', '故障降级')}
                                                                </span>
                                                            </span>
                                                            <span className="font-black">{formatSeconds(att.duration_s)}</span>
                                                        </div>
                                                        <div className="text-[10px] text-gray-500 dark:text-gray-400 truncate mt-0.5" title={att.endpoint_url}>
                                                            {t('monitor.timing.endpoint_label', '端点')}: {att.endpoint_url}
                                                        </div>
                                                        <div className="text-[10px] mt-1 text-gray-600 dark:text-gray-300 space-y-1">
                                                            <div>
                                                                <span className="font-medium">{t('monitor.timing.status_label', '状态')}: </span>
                                                                <span className="font-bold">
                                                                    {att.status ? `${att.status}` : t('monitor.timing.network_timeout_or_error', '网络超时/异常')}
                                                                    {att.is_effective ? ' OK' : ''}
                                                                    {!att.is_effective && (
                                                                        <span className="font-normal opacity-85 ml-1">
                                                                            ({t('monitor.timing.timeout_or_failover', '端点连接超时或故障转移')})
                                                                        </span>
                                                                    )}
                                                                </span>
                                                            </div>
                                                            {!att.is_effective && att.error && (
                                                                <div className="text-[10px] pt-1 border-t border-rose-500/20 text-rose-700 dark:text-rose-300 break-all select-text">
                                                                    <span className="font-semibold text-rose-900 dark:text-rose-200">
                                                                        {t('monitor.timing.raw_error_payload', '报文')}: 
                                                                    </span>
                                                                    <span className="font-mono bg-rose-500/10 px-1 py-0.5 rounded ml-1 leading-relaxed max-h-36 overflow-y-auto block mt-0.5">
                                                                        {att.error}
                                                                    </span>
                                                                </div>
                                                            )}
                                                        </div>
                                                    </div>
                                                </React.Fragment>
                                            ))}
                                        </div>
                                    )}
                                </div>
                            );
                        })}
                    </div>
                </div>
            )}
        </div>
    );
};

export interface RequestDetailModalProps {
    log: ProxyRequestLog | null;
    onClose: () => void;
    loadingDetail?: boolean;
}

export const RequestDetailModal: React.FC<RequestDetailModalProps> = ({ log, onClose, loadingDetail }) => {
    const { t } = useTranslation();
    const [payloadViewMode, setPayloadViewMode] = useState<'concise' | 'full'>('concise');
    const [modalLayout, setModalLayout] = useState<'3-column' | 'postman'>(() => {
        try {
            return (localStorage.getItem('at_proxy_modal_layout') as '3-column' | 'postman') || '3-column';
        } catch {
            return '3-column';
        }
    });
    const [isExportMenuOpen, setIsExportMenuOpen] = useState(false);
    const [exportSuccessMsg, setExportSuccessMsg] = useState<string | null>(null);
    const [showMetadata, setShowMetadata] = useState(true);
    const [copiedCard, setCopiedCard] = useState<'req' | 'upstream' | 'resp' | 'timing' | null>(null);
    const exportMenuRef = useRef<HTMLDivElement>(null);

    useEffect(() => {
        const handleClickOutside = (e: MouseEvent) => {
            if (exportMenuRef.current && !exportMenuRef.current.contains(e.target as Node)) {
                setIsExportMenuOpen(false);
            }
        };
        if (isExportMenuOpen) {
            document.addEventListener('mousedown', handleClickOutside);
        }
        return () => {
            document.removeEventListener('mousedown', handleClickOutside);
        };
    }, [isExportMenuOpen]);

    const handleLayoutChange = (layout: '3-column' | 'postman') => {
        setModalLayout(layout);
        try {
            localStorage.setItem('at_proxy_modal_layout', layout);
        } catch {}
    };

    const handleCopyHar = async () => {
        if (!log) return;
        const har = exportToHarString(log);
        const success = await copyToClipboard(har);
        if (success) {
            setExportSuccessMsg(t('monitor.details.copied_har', '已复制 HAR'));
            setTimeout(() => setExportSuccessMsg(null), 2000);
        }
        setIsExportMenuOpen(false);
    };

    const handleDownloadHar = () => {
        if (!log) return;
        downloadHarFile(log);
        setExportSuccessMsg(t('monitor.details.exported_har', '已导出 .har'));
        setTimeout(() => setExportSuccessMsg(null), 2000);
        setIsExportMenuOpen(false);
    };

    const handleCopyCurl = async () => {
        if (!log) return;
        const curl = exportToCurl(log);
        const success = await copyToClipboard(curl);
        if (success) {
            setExportSuccessMsg(t('monitor.details.copied_curl', '已复制 cURL'));
            setTimeout(() => setExportSuccessMsg(null), 2000);
        }
        setIsExportMenuOpen(false);
    };

    const handleCopyFullJson = async () => {
        if (!log) return;
        const full = JSON.stringify(log, null, 2);
        const success = await copyToClipboard(full);
        if (success) {
            setExportSuccessMsg(t('monitor.details.copied_full_json', '已复制全量 JSON'));
            setTimeout(() => setExportSuccessMsg(null), 2000);
        }
        setIsExportMenuOpen(false);
    };

    const conciseRequestBody = useMemo(() => {
        return log?.request_body
            ? extractConcisePayload(log.request_body, 'request', log)
            : '';
    }, [log?.request_body, log?.id]);

    const conciseUpstreamBody = useMemo(() => {
        return log?.upstream_request_body
            ? extractConcisePayload(log.upstream_request_body, 'upstream', log)
            : '';
    }, [log?.upstream_request_body, log?.id]);

    const conciseResponseBody = useMemo(() => {
        return log?.response_body
            ? extractConcisePayload(log.response_body, 'response', log)
            : '';
    }, [log?.response_body, log?.id, log?.input_tokens, log?.output_tokens, log?.cached_tokens]);

    const timingInfo = useMemo(() => {
        return parseTimingFromHeadersAndBody(
            log?.response_headers,
            log?.response_body,
            log?.duration
        );
    }, [log?.response_headers, log?.response_body, log?.duration]);

    const timingNode = timingInfo ? (
        <div className="p-2.5">
            <TimingDiagnosticsCard
                key={log?.id}
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

    if (!log) return null;

    return (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-sm p-2 sm:p-3 md:p-4" onClick={onClose}>
            <div className="bg-white dark:bg-base-100 rounded-2xl shadow-2xl w-full max-w-[98vw] xl:max-w-[1720px] h-[94vh] max-h-[94vh] flex flex-col overflow-hidden border border-gray-200 dark:border-base-200" onClick={e => e.stopPropagation()}>
                {/* Modal Header */}
                <div className="px-4 py-2.5 border-b border-gray-200 dark:border-base-300 flex items-center justify-between bg-gray-50 dark:bg-base-200 shrink-0">
                    <div className="flex items-center gap-3 min-w-0">
                        {loadingDetail && <div className="loading loading-spinner loading-sm shrink-0"></div>}
                        <span className={`badge badge-sm font-bold text-white border-none shrink-0 shadow-xs ${
                            log.status >= 200 && log.status < 400
                                ? 'bg-emerald-600'
                                : 'bg-rose-600'
                        }`}>
                            {log.status}
                        </span>
                        <span className="font-mono font-bold text-gray-900 dark:text-white text-sm shrink-0">{log.method}</span>
                        <span className="text-xs text-gray-500 dark:text-gray-400 font-mono truncate max-w-lg hidden sm:inline" title={log.url}>{log.url}</span>
                    </div>

                    <div className="flex items-center gap-2 shrink-0">
                        <button onClick={onClose} className="btn btn-ghost btn-sm btn-circle text-gray-500 dark:text-gray-400 hover:bg-gray-200 dark:hover:bg-base-200" aria-label="关闭"><X size={18} /></button>
                    </div>
                </div>

                {/* Modal Content */}
                <div className="flex-1 min-h-0 flex flex-col p-3 sm:p-4 space-y-2.5 bg-gray-100/50 dark:bg-base-100 overflow-hidden">
                    {/* Metadata Section */}
                    {showMetadata && (
                        <div className="bg-white dark:bg-base-200 p-3 sm:p-3.5 rounded-xl border border-gray-200 dark:border-base-300 shadow-sm shrink-0 text-xs transition-all duration-200">
                            <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-[1.1fr_0.8fr_auto_0.8fr_1.2fr_1.3fr] gap-3 items-center">
                                <div>
                                    <span className="block text-gray-500 dark:text-gray-400 uppercase font-bold text-[10px] tracking-wider whitespace-nowrap">{t('monitor.details.time')}</span>
                                    <span className="font-mono font-semibold text-gray-900 dark:text-white text-xs truncate block" title={new Date(log.timestamp).toLocaleString()}>{new Date(log.timestamp).toLocaleString()}</span>
                                </div>
                                <div>
                                    <span className="block text-gray-500 dark:text-gray-400 uppercase font-bold text-[10px] tracking-wider whitespace-nowrap">{t('monitor.details.duration')}</span>
                                    <span className="font-mono font-semibold text-gray-900 dark:text-white text-xs">{log.duration}ms</span>
                                </div>
                                <div className="min-w-fit shrink-0">
                                    <span className="block text-gray-500 dark:text-gray-400 uppercase font-bold text-[10px] tracking-wider whitespace-nowrap">{t('monitor.details.tokens')}</span>
                                    <div className="font-mono text-[11px] flex items-center gap-1.5 mt-0.5 whitespace-nowrap flex-nowrap">
                                        {(() => {
                                            const totalIn = (log.cached_tokens && log.cached_tokens > (log.input_tokens ?? 0))
                                                ? (log.input_tokens ?? 0) + log.cached_tokens
                                                : (log.input_tokens ?? 0);
                                            return (
                                                <span className="text-blue-700 dark:text-blue-300 bg-blue-100 dark:bg-blue-900/40 px-1.5 py-0.5 rounded font-bold shrink-0" title={`Total Input Tokens: ${totalIn}`}>
                                                    In: {formatCompactNumber(totalIn)}
                                                </span>
                                            );
                                        })()}
                                        <span className="text-emerald-700 dark:text-emerald-300 bg-emerald-100 dark:bg-emerald-900/40 px-1.5 py-0.5 rounded font-bold shrink-0">Out: {formatCompactNumber(log.output_tokens ?? 0)}</span>
                                        {log.cached_tokens != null && log.cached_tokens > 0 && (() => {
                                            const totalIn = (log.cached_tokens && log.cached_tokens > (log.input_tokens ?? 0))
                                                ? (log.input_tokens ?? 0) + log.cached_tokens
                                                : (log.input_tokens ?? 0);
                                            const hitRate = totalIn > 0 ? Math.min(100, Math.max(0, (log.cached_tokens / totalIn) * 100)) : 0;
                                            const hitRateText = totalIn > 0 ? (hitRate >= 100 ? '100%' : (hitRate % 1 === 0 ? `${hitRate.toFixed(0)}%` : `${hitRate.toFixed(1)}%`)) : '';
                                            return (
                                                <span
                                                    className="text-purple-700 dark:text-purple-300 bg-purple-100 dark:bg-purple-900/40 px-1.5 py-0.5 rounded font-bold shrink-0"
                                                    title={`Cache: ${log.cached_tokens.toLocaleString()}${hitRateText ? ` (${hitRateText})` : ''}`}
                                                >
                                                    Cache: {formatCompactNumber(log.cached_tokens)}{hitRateText ? ` (${hitRateText})` : ''}
                                                </span>
                                            );
                                        })()}
                                    </div>
                                </div>
                                <div>
                                    <span className="block text-gray-500 dark:text-gray-400 uppercase font-bold text-[10px] tracking-wider">{t('monitor.details.protocol')}</span>
                                    <span className={`inline-block px-2 py-0.5 rounded-full font-mono font-bold text-[11px] uppercase mt-0.5 text-white shadow-xs ${
                                        log.protocol === 'openai' ? 'bg-emerald-600' :
                                        log.protocol === 'anthropic' ? 'bg-amber-600' :
                                        log.protocol === 'gemini' ? 'bg-blue-600' :
                                        log.protocol === 'ban_probe' ? 'bg-rose-600' :
                                        log.protocol === 'warmup' ? 'bg-orange-600' :
                                        'bg-gray-600'
                                    }`}>
                                        {log.protocol === 'ban_probe' ? t('monitor.protocols.ban_probe', 'Ban Probe') : log.protocol === 'warmup' ? t('monitor.protocols.warmup', 'Warmup') : log.protocol || '-'}
                                    </span>
                                </div>
                                <div>
                                    <span className="block text-gray-500 dark:text-gray-400 uppercase font-bold text-[10px] tracking-wider">{t('monitor.details.model')}</span>
                                    <span className="font-mono font-bold text-blue-600 dark:text-blue-400 truncate block text-xs" title={log.model}>{log.model || '-'}</span>
                                    {log.mapped_model && log.model !== log.mapped_model && (
                                        <span className="font-mono text-emerald-600 dark:text-emerald-400 truncate block text-[11px]" title={log.mapped_model}>➔ {log.mapped_model}</span>
                                    )}
                                    {log.upstream_model && log.upstream_model !== log.mapped_model && (
                                        <span className="font-mono text-amber-600 dark:text-amber-400 truncate block text-[11px]" title={`${t('monitor.details.upstream_model', 'Upstream Model')}: ${log.upstream_model}`}>⚡ {log.upstream_model}</span>
                                    )}
                                </div>
                                <div>
                                    <span className="block text-gray-500 dark:text-gray-400 uppercase font-bold text-[10px] tracking-wider">{t('monitor.details.account_used')}</span>
                                    <span className="font-mono font-medium text-gray-900 dark:text-white truncate block text-xs" title={log.account_email || '-'}>{log.account_email || '-'}</span>
                                </div>
                            </div>
                        </div>
                    )}

                    {/* Mode & Toolbar Bar */}
                    <div className="flex flex-wrap items-center justify-between gap-2 px-1 shrink-0">
                        <div className="flex items-center gap-2 flex-wrap">
                            {/* Layout Switcher */}
                            <div className="inline-flex items-center p-1 bg-gray-200/70 dark:bg-base-200 rounded-xl border border-gray-300/70 dark:border-base-300 gap-1 shadow-inner">
                                <button
                                    type="button"
                                    onClick={() => handleLayoutChange('3-column')}
                                    className={`flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-xs font-semibold transition-all duration-150 cursor-pointer select-none ${
                                        modalLayout === '3-column'
                                            ? 'bg-white dark:bg-base-100 text-blue-600 dark:text-blue-400 shadow-sm border border-gray-200 dark:border-base-300'
                                            : 'text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white'
                                    }`}
                                    title={t('monitor.details.layout_3col_title', '三栏横向对比视图')}
                                >
                                    <LayoutGrid size={13} className={modalLayout === '3-column' ? 'text-blue-600 dark:text-blue-400' : 'text-gray-400'} />
                                    <span>{t('monitor.details.layout_3col', '三栏对比')}</span>
                                </button>
                                <button
                                    type="button"
                                    onClick={() => handleLayoutChange('postman')}
                                    className={`flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-xs font-semibold transition-all duration-150 cursor-pointer select-none ${
                                        modalLayout === 'postman'
                                            ? 'bg-white dark:bg-base-100 text-blue-600 dark:text-blue-400 shadow-sm border border-gray-200 dark:border-base-300'
                                            : 'text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white'
                                    }`}
                                    title={t('monitor.details.layout_postman_title', 'Postman 风格上下分屏视图')}
                                >
                                    <SplitSquareVertical size={13} className={modalLayout === 'postman' ? 'text-blue-600 dark:text-blue-400' : 'text-gray-400'} />
                                    <span>{t('monitor.details.layout_postman', 'Postman 风格')}</span>
                                </button>
                            </div>

                            {/* Payload Concise / Full Switcher */}
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
                                    ? t('monitor.details.concise_desc', '保留转出报文的原始层级，并将标识、模型、思考配置、系统提示词、对话与工具前置；图片等大体积内容已省略')
                                    : t('monitor.details.full_desc', '显示原始完整未修剪报文')}
                            </span>
                        </div>

                        <div className="flex items-center gap-2">
                            {/* Export Menu Dropdown */}
                            <div className="relative" ref={exportMenuRef}>
                                <button
                                    type="button"
                                    onClick={() => setIsExportMenuOpen((prev) => !prev)}
                                    className={`flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-xs font-semibold transition-all duration-150 cursor-pointer select-none border shadow-2xs ${
                                        isExportMenuOpen
                                            ? 'bg-blue-50 dark:bg-blue-950/40 text-blue-700 dark:text-blue-300 border-blue-300 dark:border-blue-800'
                                            : 'bg-white dark:bg-base-100 text-gray-700 dark:text-gray-200 border-gray-200 dark:border-base-300 hover:bg-gray-50 dark:hover:bg-base-200 hover:text-blue-600 dark:hover:text-blue-400'
                                    }`}
                                    title={t('monitor.details.export_menu_title', '导出 HAR 格式、复制为 cURL 或诊断 JSON')}
                                >
                                    {exportSuccessMsg ? (
                                        <Check size={12} className="text-emerald-500" />
                                    ) : (
                                        <Download size={12} className="text-blue-500" />
                                    )}
                                    <span>{exportSuccessMsg || t('monitor.details.export_menu', '导出 / 复制')}</span>
                                    <ChevronDown size={11} className={`text-gray-400 transition-transform duration-200 ${isExportMenuOpen ? 'rotate-180' : ''}`} />
                                </button>

                                {isExportMenuOpen && (
                                    <div
                                        className="absolute right-0 top-full mt-1.5 w-48 bg-white dark:bg-base-100 rounded-xl shadow-xl border border-gray-200 dark:border-base-300 py-1.5 z-40 font-sans text-xs divide-y divide-gray-100 dark:divide-base-300"
                                        onClick={(e) => e.stopPropagation()}
                                    >
                                        <div className="py-0.5">
                                            <button
                                                type="button"
                                                onClick={handleCopyHar}
                                                className="w-full text-left px-3 py-1.5 hover:bg-blue-50 dark:hover:bg-blue-950/40 flex items-center gap-2 text-gray-700 dark:text-gray-200 hover:text-blue-600 dark:hover:text-blue-400 transition-colors"
                                            >
                                                <Copy size={13} className="text-blue-500" />
                                                <span className="font-medium">{t('monitor.details.copy_har', '复制 HAR 报文')}</span>
                                            </button>
                                            <button
                                                type="button"
                                                onClick={handleDownloadHar}
                                                className="w-full text-left px-3 py-1.5 hover:bg-emerald-50 dark:hover:bg-emerald-950/40 flex items-center gap-2 text-gray-700 dark:text-gray-200 hover:text-emerald-600 dark:hover:text-emerald-400 transition-colors"
                                            >
                                                <Download size={13} className="text-emerald-500" />
                                                <span className="font-medium">{t('monitor.details.download_har', '下载 .har 文件')}</span>
                                            </button>
                                            <button
                                                type="button"
                                                onClick={handleCopyCurl}
                                                className="w-full text-left px-3 py-1.5 hover:bg-purple-50 dark:hover:bg-purple-950/40 flex items-center gap-2 text-gray-700 dark:text-gray-200 hover:text-purple-600 dark:hover:text-purple-400 transition-colors"
                                            >
                                                <Terminal size={13} className="text-purple-500" />
                                                <span className="font-medium">{t('monitor.details.copy_curl', '复制为 cURL 命令')}</span>
                                            </button>
                                        </div>
                                        <div className="pt-1">
                                            <button
                                                type="button"
                                                onClick={handleCopyFullJson}
                                                className="w-full text-left px-3 py-1.5 hover:bg-gray-100 dark:hover:bg-base-200 flex items-center gap-2 text-gray-500 dark:text-gray-400 hover:text-gray-900 transition-colors"
                                            >
                                                <FileCode2 size={13} />
                                                <span>{t('monitor.details.copy_full_json', '复制全量诊断 JSON')}</span>
                                            </button>
                                        </div>
                                    </div>
                                )}
                            </div>

                            <button
                                type="button"
                                onClick={() => setShowMetadata((prev) => !prev)}
                                className="btn btn-xs btn-ghost text-gray-500 dark:text-gray-400 hover:bg-gray-200 dark:hover:bg-base-200 gap-1 text-[11px]"
                                title={showMetadata ? t('monitor.details.collapse_metadata_title', '折叠元数据以增大报文视野') : t('monitor.details.expand_metadata_title', '展开元数据信息')}
                            >
                                {showMetadata ? <EyeOff size={13} /> : <Eye size={13} />}
                                <span>{showMetadata ? t('monitor.details.collapse_metadata', '收起元数据') : t('monitor.details.expand_metadata', '展开元数据')}</span>
                            </button>
                        </div>
                    </div>

                    {/* Content Area: Postman View or 3-Column Grid */}
                    {modalLayout === 'postman' ? (
                        <PostmanDetailView
                            log={log}
                            payloadViewMode={payloadViewMode}
                            conciseRequestBody={conciseRequestBody}
                            conciseUpstreamBody={conciseUpstreamBody}
                            conciseResponseBody={conciseResponseBody}
                            timingInfo={timingInfo}
                            timingNode={timingNode}
                            onCopyText={async (text) => {
                                await copyToClipboard(text);
                            }}
                        />
                    ) : (
                        /* Horizontal 3-Column Grid */
                        <div className="grid grid-cols-1 lg:grid-cols-3 gap-3 flex-1 min-h-0 overflow-hidden">
                            <VirtualizedPayloadViewer
                                cardId="req"
                                title={t('monitor.details.request_payload', '请求报文 (Request)')}
                                badge="REQUEST"
                                badgeStyle="bg-blue-50 text-blue-700 dark:bg-blue-900/30 dark:text-blue-300 border-blue-200 dark:border-blue-800/60"
                                rawPayload={log.request_body}
                                concisePayload={conciseRequestBody}
                                headersJson={log.request_headers}
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
                                rawPayload={log.upstream_request_body}
                                concisePayload={conciseUpstreamBody}
                                headersJson={log.upstream_request_headers}
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
                                rawPayload={log.response_body}
                                concisePayload={conciseResponseBody}
                                headersJson={log.response_headers}
                                viewMode={payloadViewMode}
                                emptyPlaceholder={t('monitor.details.payload_empty', '无响应报文')}
                                duration={log.duration}
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
                    )}
                </div>
            </div>
        </div>
    );
};
