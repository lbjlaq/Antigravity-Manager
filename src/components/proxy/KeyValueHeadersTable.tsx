import React, { useState, useMemo, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { Search, Copy, CheckCircle, Eye, EyeOff, Table, Code2 } from 'lucide-react';
import { copyToClipboard } from '../../utils/clipboard';

export interface KeyValuePair {
    name: string;
    value: string;
}

export interface KeyValueHeadersTableProps {
    title?: string;
    searchPlaceholder?: string;
    headersJson?: string;
    pairs?: KeyValuePair[];
    emptyPlaceholder?: string;
    isQueryParams?: boolean;
    onCopyText?: (text: string) => void;
}

const SENSITIVE_KEY_REGEX = /(?:authorization|auth|token|secret|password|api[-_]?key|cookie|session|credential)/i;

export const KeyValueHeadersTable: React.FC<KeyValueHeadersTableProps> = ({
    title,
    searchPlaceholder,
    headersJson,
    pairs: initialPairs,
    emptyPlaceholder,
    isQueryParams = false,
    onCopyText,
}) => {
    const { t } = useTranslation();
    const [searchQuery, setSearchQuery] = useState('');
    const [viewMode, setViewMode] = useState<'table' | 'raw'>('table');
    const [unmaskedKeys, setUnmaskedKeys] = useState<Set<string>>(new Set());
    const [copiedKey, setCopiedKey] = useState<string | null>(null);
    const [isAllCopied, setIsAllCopied] = useState(false);

    // 解析出键值对列表
    const parsedPairs = useMemo<KeyValuePair[]>(() => {
        if (initialPairs && initialPairs.length > 0) {
            return initialPairs;
        }
        if (!headersJson) return [];

        try {
            const parsed = JSON.parse(headersJson);
            if (Array.isArray(parsed)) {
                return parsed
                    .filter((item) => item && typeof item.name === 'string')
                    .map((item) => ({
                        name: String(item.name),
                        value: String(item.value ?? ''),
                    }));
            }
            if (typeof parsed === 'object' && parsed !== null) {
                return Object.entries(parsed).map(([name, val]) => ({
                    name,
                    value: typeof val === 'string' ? val : JSON.stringify(val),
                }));
            }
        } catch {
            const lines = headersJson.split('\n');
            const list: KeyValuePair[] = [];
            for (const line of lines) {
                const idx = line.indexOf(':');
                if (idx > 0) {
                    list.push({
                        name: line.slice(0, idx).trim(),
                        value: line.slice(idx + 1).trim(),
                    });
                }
            }
            return list;
        }
        return [];
    }, [headersJson, initialPairs]);

    // 格式化 Raw 文本
    const rawFormattedText = useMemo(() => {
        if (headersJson) {
            try {
                return JSON.stringify(JSON.parse(headersJson), null, 2);
            } catch {
                return headersJson;
            }
        }
        if (parsedPairs.length > 0) {
            return parsedPairs.map((p) => `${p.name}: ${p.value}`).join('\n');
        }
        return '';
    }, [headersJson, parsedPairs]);

    // 过滤列表
    const filteredPairs = useMemo(() => {
        const query = searchQuery.trim().toLowerCase();
        if (!query) return parsedPairs;
        return parsedPairs.filter(
            (p) =>
                p.name.toLowerCase().includes(query) ||
                p.value.toLowerCase().includes(query)
        );
    }, [parsedPairs, searchQuery]);

    const handleCopy = useCallback(
        async (text: string, identifier: string) => {
            if (onCopyText) {
                onCopyText(text);
                setCopiedKey(identifier);
                setTimeout(() => setCopiedKey(null), 1800);
            } else {
                const success = await copyToClipboard(text);
                if (success) {
                    setCopiedKey(identifier);
                    setTimeout(() => setCopiedKey(null), 1800);
                }
            }
        },
        [onCopyText]
    );

    const handleCopyAll = useCallback(async () => {
        const textToCopy =
            viewMode === 'raw'
                ? rawFormattedText
                : parsedPairs.map((p) => `${p.name}: ${p.value}`).join('\n');
        if (!textToCopy) return;

        if (onCopyText) {
            onCopyText(textToCopy);
            setIsAllCopied(true);
            setTimeout(() => setIsAllCopied(false), 2000);
        } else {
            const success = await copyToClipboard(textToCopy);
            if (success) {
                setIsAllCopied(true);
                setTimeout(() => setIsAllCopied(false), 2000);
            }
        }
    }, [onCopyText, viewMode, rawFormattedText, parsedPairs]);

    const toggleMask = (keyName: string) => {
        setUnmaskedKeys((prev) => {
            const next = new Set(prev);
            if (next.has(keyName)) {
                next.delete(keyName);
            } else {
                next.add(keyName);
            }
            return next;
        });
    };

    const maskValue = (key: string, val: string): string => {
        if (!SENSITIVE_KEY_REGEX.test(key)) return val;
        if (unmaskedKeys.has(key)) return val;

        if (val.length <= 12) {
            return '••••••••';
        }
        const prefix = val.slice(0, 7);
        const suffix = val.slice(-4);
        return `${prefix}••••••••${suffix}`;
    };

    if (parsedPairs.length === 0) {
        return (
            <div className="h-full flex flex-col items-center justify-center p-8 text-center text-gray-400 dark:text-gray-500 select-none">
                <span className="text-xs italic">
                    {emptyPlaceholder || (isQueryParams ? '无 Query 请求参数' : '无 Headers 数据')}
                </span>
            </div>
        );
    }

    return (
        <div className="h-full flex flex-col min-h-0 bg-white dark:bg-base-100 font-sans">
            {/* Toolbar: Search, Mode Switch, Copy All */}
            <div className="px-3 py-2 bg-gray-50/80 dark:bg-base-200/50 border-b border-gray-200 dark:border-base-300 flex items-center justify-between gap-2 shrink-0">
                <div className="flex items-center gap-2 flex-1 min-w-0">
                    {title && (
                        <span className="text-xs font-bold text-gray-700 dark:text-gray-300 shrink-0 hidden sm:inline-block">
                            {title}
                        </span>
                    )}
                    <div className="relative flex-1 max-w-xs">
                        <Search
                            size={12}
                            className="absolute left-2.5 top-1/2 -translate-y-1/2 text-gray-400"
                        />
                        <input
                            type="text"
                            value={searchQuery}
                            onChange={(e) => setSearchQuery(e.target.value)}
                            placeholder={
                                searchPlaceholder ||
                                (isQueryParams
                                    ? t('monitor.details.filter_params', '搜索参数名称或值...')
                                    : t('monitor.details.filter_headers', '搜索 Header 键或值...'))
                            }
                            className="input input-xs pl-7 pr-2 w-full bg-white dark:bg-base-100 border-gray-200 dark:border-base-300 text-xs rounded-md focus:border-blue-500 focus:outline-hidden"
                        />
                    </div>
                </div>

                <div className="flex items-center gap-1.5 shrink-0">
                    <div className="inline-flex items-center p-0.5 bg-gray-200/70 dark:bg-base-300 rounded-lg border border-gray-300/60 dark:border-base-300 text-[10px]">
                        <button
                            type="button"
                            onClick={() => setViewMode('table')}
                            className={`flex items-center gap-1 px-2 py-0.5 rounded-md font-semibold transition-all ${
                                viewMode === 'table'
                                    ? 'bg-white dark:bg-base-100 text-blue-600 dark:text-blue-400 shadow-2xs'
                                    : 'text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white'
                            }`}
                            title={t('monitor.details.view_table_title', '表格视图 (Key-Value)')}
                        >
                            <Table size={11} />
                            <span>{t('monitor.details.view_table', '表格')}</span>
                        </button>
                        <button
                            type="button"
                            onClick={() => setViewMode('raw')}
                            className={`flex items-center gap-1 px-2 py-0.5 rounded-md font-semibold transition-all ${
                                viewMode === 'raw'
                                    ? 'bg-white dark:bg-base-100 text-blue-600 dark:text-blue-400 shadow-2xs'
                                    : 'text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white'
                            }`}
                            title={t('monitor.details.view_raw_title', '原始 JSON / 纯文本')}
                        >
                            <Code2 size={11} />
                            <span>{t('monitor.details.view_raw', '原始')}</span>
                        </button>
                    </div>

                    <button
                        type="button"
                        onClick={handleCopyAll}
                        className="btn btn-ghost btn-xs h-6 px-2 gap-1 text-[11px] text-gray-600 hover:text-blue-600 dark:text-gray-300 dark:hover:text-blue-400 font-normal hover:bg-white dark:hover:bg-base-200 border border-transparent hover:border-gray-200 dark:hover:border-base-300 rounded-md shadow-2xs"
                        title={isAllCopied ? t('common.copied', '已复制') : t('common.copy_all', '复制全部')}
                    >
                        {isAllCopied ? (
                            <CheckCircle size={12} className="text-green-500" />
                        ) : (
                            <Copy size={12} />
                        )}
                        <span>{isAllCopied ? (t('common.copied') || '已复制') : (t('common.copy_all') || '复制全部')}</span>
                    </button>
                </div>
            </div>

            {/* Table or Raw View Area */}
            <div className="flex-1 min-h-0 overflow-auto">
                {viewMode === 'raw' ? (
                    <div className="p-3">
                        <pre className="font-mono text-xs leading-relaxed text-gray-700 dark:text-gray-200 whitespace-pre-wrap select-text m-0">
                            {rawFormattedText}
                        </pre>
                    </div>
                ) : (
                    <table className="w-full text-left border-collapse text-xs">
                        <thead>
                            <tr className="bg-gray-100/70 dark:bg-base-200/80 text-[10px] uppercase font-bold text-gray-500 dark:text-gray-400 border-b border-gray-200 dark:border-base-300 tracking-wider sticky top-0 z-10 backdrop-blur-xs">
                                <th className="py-1.5 px-3 w-1/3">
                                    {isQueryParams ? t('monitor.details.param_key', 'Param Key') : t('monitor.details.header_key', 'Header Key')}
                                </th>
                                <th className="py-1.5 px-3">
                                    {isQueryParams ? t('monitor.details.param_value', 'Param Value') : t('monitor.details.header_value', 'Header Value')}
                                </th>
                                <th className="py-1.5 px-3 w-20 text-right">{t('common.actions', '操作')}</th>
                            </tr>
                        </thead>
                        <tbody className="divide-y divide-gray-100 dark:divide-base-300/60 font-mono text-[11px]">
                            {filteredPairs.length === 0 ? (
                                <tr>
                                    <td
                                        colSpan={3}
                                        className="py-8 text-center text-gray-400 dark:text-gray-500 italic"
                                    >
                                        {t('monitor.details.no_matches', '未找到匹配项')}
                                    </td>
                                </tr>
                            ) : (
                                filteredPairs.map((pair, idx) => {
                                    const isSensitive = SENSITIVE_KEY_REGEX.test(pair.name);
                                    const isUnmasked = unmaskedKeys.has(pair.name);
                                    const displayVal = maskValue(pair.name, pair.value);
                                    const rowKey = `${pair.name}-${idx}`;

                                    return (
                                        <tr
                                            key={rowKey}
                                            className="hover:bg-blue-50/40 dark:hover:bg-base-200/60 transition-colors group"
                                        >
                                            {/* Key Name */}
                                            <td className="py-1.5 px-3 font-semibold text-gray-800 dark:text-gray-200 select-text align-top break-all">
                                                <div className="flex items-center gap-1.5">
                                                    <span>{pair.name}</span>
                                                    {isSensitive && (
                                                        <span className="text-[9px] px-1 py-0.2 rounded font-sans font-medium bg-amber-100 text-amber-800 dark:bg-amber-950/60 dark:text-amber-300 border border-amber-200 dark:border-amber-800/40 shrink-0">
                                                            {t('monitor.details.sensitive', '敏感')}
                                                        </span>
                                                    )}
                                                </div>
                                            </td>

                                            {/* Value */}
                                            <td className="py-1.5 px-3 text-gray-600 dark:text-gray-300 select-text align-top break-all font-mono leading-relaxed">
                                                <div className="flex items-start justify-between gap-2">
                                                    <span className={isSensitive && !isUnmasked ? 'tracking-wider text-gray-400 select-none' : ''}>
                                                        {displayVal}
                                                    </span>
                                                    {isSensitive && (
                                                        <button
                                                            type="button"
                                                            onClick={() => toggleMask(pair.name)}
                                                            className="text-gray-400 hover:text-blue-500 p-0.5 rounded shrink-0 transition-colors"
                                                            title={isUnmasked ? t('monitor.details.mask_sensitive', '遮罩隐藏明文') : t('monitor.details.unmask_sensitive', '查看完整明文')}
                                                        >
                                                            {isUnmasked ? (
                                                                <EyeOff size={12} />
                                                            ) : (
                                                                <Eye size={12} />
                                                            )}
                                                        </button>
                                                    )}
                                                </div>
                                            </td>

                                            {/* Action Copy */}
                                            <td className="py-1.5 px-3 text-right align-top shrink-0">
                                                <button
                                                    type="button"
                                                    onClick={() => handleCopy(pair.value, rowKey)}
                                                    className="opacity-0 group-hover:opacity-100 text-gray-400 hover:text-blue-600 p-1 rounded hover:bg-gray-200/70 dark:hover:bg-base-300 transition-all inline-flex items-center gap-1 text-[10px]"
                                                    title={t('monitor.details.copy_value', '复制此项值')}
                                                >
                                                    {copiedKey === rowKey ? (
                                                        <CheckCircle size={12} className="text-green-500" />
                                                    ) : (
                                                        <Copy size={12} />
                                                    )}
                                                </button>
                                            </td>
                                        </tr>
                                    );
                                })
                            )}
                        </tbody>
                    </table>
                )}
            </div>
        </div>
    );
};
