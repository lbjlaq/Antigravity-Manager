import React, { useState, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import {
    FileText,
    ListFilter,
    Sliders,
    ArrowRightLeft,
    Clock,
    Send,
    DownloadCloud,
    LayoutList,
    Code2
} from 'lucide-react';
import { ProxyRequestLog, StageTimingInfo } from './RequestDetailModal';
import { VirtualizedPayloadViewer } from './VirtualizedPayloadViewer';
import { KeyValueHeadersTable } from './KeyValueHeadersTable';
import { StructuredPayloadView } from './StructuredPayloadView';
import { parseQueryStringFromUrl, parseHeadersToHarList } from '../../utils/harExport';
import { isStrictProxySuccessStatus } from '../../utils/httpStatus';

export interface PostmanDetailViewProps {
    log: ProxyRequestLog;
    payloadViewMode: 'concise' | 'full';
    conciseRequestBody: string;
    conciseUpstreamBody: string;
    conciseResponseBody: string;
    timingInfo: StageTimingInfo | null;
    timingNode?: React.ReactNode;
    onCopyText: (text: string) => Promise<boolean | void> | void;
}

type MainTabType = 'request' | 'forwarded' | 'response' | 'timing';
type SubTabType = 'body' | 'headers' | 'params';

export const PostmanDetailView: React.FC<PostmanDetailViewProps> = ({
    log,
    payloadViewMode,
    conciseRequestBody,
    conciseUpstreamBody,
    conciseResponseBody,
    timingNode,
    onCopyText,
}) => {
    const { t } = useTranslation();

    // 一级主 Tab：请求 / 转出 / 响应 / 耗时诊断
    const [mainTab, setMainTab] = useState<MainTabType>('request');

    // 各主 Tab 内部的二级子 Tab
    const [reqSubTab, setReqSubTab] = useState<SubTabType>('body');
    const [forwardedSubTab, setForwardedSubTab] = useState<'body' | 'headers'>('body');
    const [respSubTab, setRespSubTab] = useState<'body' | 'headers'>('body');

    // 正文展现模式：结构化拆分 vs 原始代码
    const [bodyFormat, setBodyFormat] = useState<'structured' | 'raw'>(() => {
        try {
            return (localStorage.getItem('at_proxy_body_format') as 'structured' | 'raw') || 'structured';
        } catch {
            return 'structured';
        }
    });

    const handleBodyFormatChange = (fmt: 'structured' | 'raw') => {
        setBodyFormat(fmt);
        try {
            localStorage.setItem('at_proxy_body_format', fmt);
        } catch {}
    };

    // 统计项
    const reqHeadersCount = useMemo(
        () => parseHeadersToHarList(log.request_headers).length,
        [log.request_headers]
    );
    const upstreamHeadersCount = useMemo(
        () => parseHeadersToHarList(log.upstream_request_headers).length,
        [log.upstream_request_headers]
    );
    const queryParams = useMemo(
        () => parseQueryStringFromUrl(log.url),
        [log.url]
    );
    const respHeadersCount = useMemo(
        () => parseHeadersToHarList(log.response_headers).length,
        [log.response_headers]
    );

    const hasUpstream = Boolean(log.upstream_request_body || log.upstream_request_headers);
    const isSuccess = isStrictProxySuccessStatus(log.status);

    // 当前是否正停留在某个 Body 选项卡
    const isViewingBody =
        (mainTab === 'request' && reqSubTab === 'body') ||
        (mainTab === 'forwarded' && forwardedSubTab === 'body') ||
        (mainTab === 'response' && respSubTab === 'body');

    return (
        <div className="flex-1 min-h-0 flex flex-col bg-white dark:bg-base-200 rounded-xl border border-gray-200 dark:border-base-300 shadow-xs overflow-hidden">
            {/* 顶层主标签栏 (Main Tabs Header) */}
            <div className="px-3 pt-2 pb-0 bg-gray-50/90 dark:bg-base-300/60 border-b border-gray-200 dark:border-base-300 flex items-center justify-between gap-2 shrink-0 select-none overflow-x-auto">
                <div className="flex items-center gap-1.5 min-w-0">
                    {/* Tab 1: 客户端请求 (Request) */}
                    <button
                        type="button"
                        onClick={() => setMainTab('request')}
                        className={`flex items-center gap-2 px-3.5 py-2 rounded-t-lg text-xs font-bold transition-all relative border-t border-x ${
                            mainTab === 'request'
                                ? 'bg-white dark:bg-base-200 text-blue-600 dark:text-blue-400 border-gray-200 dark:border-base-300 -mb-[1px] shadow-xs z-10'
                                : 'border-transparent text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white hover:bg-gray-100/70 dark:hover:bg-base-200/50'
                        }`}
                    >
                        <Send size={13} className={mainTab === 'request' ? 'text-blue-600 dark:text-blue-400' : 'text-gray-400'} />
                        <span>{t('monitor.details.request_tab', '请求报文 (Request)')}</span>
                        {reqHeadersCount > 0 && (
                            <span className="text-[10px] px-1.5 py-0.2 rounded-full bg-blue-100 dark:bg-blue-950/60 text-blue-700 dark:text-blue-300 font-mono font-medium">
                                {reqHeadersCount}
                            </span>
                        )}
                    </button>

                    {/* Tab 2: 转出上游报文 (Forwarded) */}
                    <button
                        type="button"
                        onClick={() => setMainTab('forwarded')}
                        className={`flex items-center gap-2 px-3.5 py-2 rounded-t-lg text-xs font-bold transition-all relative border-t border-x ${
                            mainTab === 'forwarded'
                                ? 'bg-white dark:bg-base-200 text-amber-700 dark:text-amber-300 border-gray-200 dark:border-base-300 -mb-[1px] shadow-xs z-10'
                                : 'border-transparent text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white hover:bg-gray-100/70 dark:hover:bg-base-200/50'
                        }`}
                    >
                        <ArrowRightLeft size={13} className={mainTab === 'forwarded' ? 'text-amber-600 dark:text-amber-400' : 'text-gray-400'} />
                        <span>{t('monitor.details.forwarded_tab', '转出报文 (Forwarded)')}</span>
                        {hasUpstream ? (
                            <span className="w-2 h-2 rounded-full bg-amber-500" title={t('monitor.details.forwarded_tooltip', '已转出发往上游')} />
                        ) : (
                            <span className="text-[10px] text-gray-400">({t('monitor.details.no_forwarded_short', '未转出')})</span>
                        )}
                    </button>

                    {/* Tab 3: 响应报文 (Response) */}
                    <button
                        type="button"
                        onClick={() => setMainTab('response')}
                        className={`flex items-center gap-2 px-3.5 py-2 rounded-t-lg text-xs font-bold transition-all relative border-t border-x ${
                            mainTab === 'response'
                                ? 'bg-white dark:bg-base-200 text-emerald-700 dark:text-emerald-300 border-gray-200 dark:border-base-300 -mb-[1px] shadow-xs z-10'
                                : 'border-transparent text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white hover:bg-gray-100/70 dark:hover:bg-base-200/50'
                        }`}
                    >
                        <DownloadCloud size={13} className={mainTab === 'response' ? 'text-emerald-600 dark:text-emerald-400' : 'text-gray-400'} />
                        <span>{t('monitor.details.response_tab', '响应报文 (Response)')}</span>
                        <span className={`text-[10px] px-1.5 py-0.2 rounded-full font-mono font-bold ${
                            isSuccess
                                ? 'bg-emerald-100 dark:bg-emerald-950/60 text-emerald-700 dark:text-emerald-300'
                                : 'bg-rose-100 dark:bg-rose-950/60 text-rose-700 dark:text-rose-300'
                        }`}>
                            {log.status}
                        </span>
                    </button>

                    {/* Tab 4: 耗时诊断 (Timing) */}
                    <button
                        type="button"
                        onClick={() => setMainTab('timing')}
                        className={`flex items-center gap-2 px-3.5 py-2 rounded-t-lg text-xs font-bold transition-all relative border-t border-x ${
                            mainTab === 'timing'
                                ? 'bg-white dark:bg-base-200 text-purple-700 dark:text-purple-300 border-gray-200 dark:border-base-300 -mb-[1px] shadow-xs z-10'
                                : 'border-transparent text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white hover:bg-gray-100/70 dark:hover:bg-base-200/50'
                        }`}
                    >
                        <Clock size={13} className={mainTab === 'timing' ? 'text-purple-600 dark:text-purple-400' : 'text-gray-400'} />
                        <span>{t('monitor.details.timing_tab', '耗时诊断 (Timing)')}</span>
                        {log.duration > 0 && (
                            <span className="text-[10px] px-1.5 py-0.2 rounded-full bg-purple-100 dark:bg-purple-950/60 text-purple-700 dark:text-purple-300 font-mono">
                                {log.duration}ms
                            </span>
                        )}
                    </button>
                </div>
            </div>

            {/* 二级子导航栏 (Sub-Tabs / Content Partition Header) */}
            <div className="px-3 py-1.5 bg-gray-50/50 dark:bg-base-200/80 border-b border-gray-200 dark:border-base-300 flex items-center justify-between gap-2 shrink-0 select-none">
                <div className="flex items-center gap-1.5 flex-wrap">
                    {/* 请求报文专属子项 */}
                    {mainTab === 'request' && (
                        <>
                            <button
                                type="button"
                                onClick={() => setReqSubTab('body')}
                                className={`flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-xs font-semibold transition-all ${
                                    reqSubTab === 'body'
                                        ? 'bg-blue-50 text-blue-700 dark:bg-blue-900/40 dark:text-blue-300 border border-blue-200 dark:border-blue-800/60 shadow-2xs'
                                        : 'text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white hover:bg-gray-100 dark:hover:bg-base-300'
                                }`}
                            >
                                <FileText size={12} />
                                <span>{t('monitor.details.body_content', '正文内容 (Body)')}</span>
                            </button>

                            <button
                                type="button"
                                onClick={() => setReqSubTab('headers')}
                                className={`flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-xs font-semibold transition-all ${
                                    reqSubTab === 'headers'
                                        ? 'bg-blue-50 text-blue-700 dark:bg-blue-900/40 dark:text-blue-300 border border-blue-200 dark:border-blue-800/60 shadow-2xs'
                                        : 'text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white hover:bg-gray-100 dark:hover:bg-base-300'
                                }`}
                            >
                                <ListFilter size={12} />
                                <span>{t('monitor.details.request_headers', '请求头 (Headers)')}</span>
                                {reqHeadersCount > 0 && (
                                    <span className="text-[10px] px-1.5 py-0.2 rounded-full bg-blue-100 dark:bg-blue-950/60 text-blue-800 dark:text-blue-200 font-mono">
                                        {reqHeadersCount}
                                    </span>
                                )}
                            </button>

                            <button
                                type="button"
                                onClick={() => setReqSubTab('params')}
                                className={`flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-xs font-semibold transition-all ${
                                    reqSubTab === 'params'
                                        ? 'bg-blue-50 text-blue-700 dark:bg-blue-900/40 dark:text-blue-300 border border-blue-200 dark:border-blue-800/60 shadow-2xs'
                                        : 'text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white hover:bg-gray-100 dark:hover:bg-base-300'
                                }`}
                            >
                                <Sliders size={12} />
                                <span>{t('monitor.details.url_params', 'URL 参数 (Params)')}</span>
                                {queryParams.length > 0 && (
                                    <span className="text-[10px] px-1.5 py-0.2 rounded-full bg-emerald-100 dark:bg-emerald-950/60 text-emerald-800 dark:text-emerald-200 font-mono font-bold">
                                        {queryParams.length}
                                    </span>
                                )}
                            </button>
                        </>
                    )}

                    {/* 转出报文专属子项 */}
                    {mainTab === 'forwarded' && (
                        <>
                            <button
                                type="button"
                                onClick={() => setForwardedSubTab('body')}
                                className={`flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-xs font-semibold transition-all ${
                                    forwardedSubTab === 'body'
                                        ? 'bg-amber-100/80 text-amber-900 dark:bg-amber-950/60 dark:text-amber-200 border border-amber-300 dark:border-amber-800/60 shadow-2xs'
                                        : 'text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white hover:bg-gray-100 dark:hover:bg-base-300'
                                }`}
                            >
                                <FileText size={12} />
                                <span>{t('monitor.details.forwarded_body', '转出正文 (Forwarded Body)')}</span>
                            </button>

                            <button
                                type="button"
                                onClick={() => setForwardedSubTab('headers')}
                                className={`flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-xs font-semibold transition-all ${
                                    forwardedSubTab === 'headers'
                                        ? 'bg-amber-100/80 text-amber-900 dark:bg-amber-950/60 dark:text-amber-200 border border-amber-300 dark:border-amber-800/60 shadow-2xs'
                                        : 'text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white hover:bg-gray-100 dark:hover:bg-base-300'
                                }`}
                            >
                                <ListFilter size={12} />
                                <span>{t('monitor.details.forwarded_headers', '转出请求头 (Headers)')}</span>
                                {upstreamHeadersCount > 0 && (
                                    <span className="text-[10px] px-1.5 py-0.2 rounded-full bg-amber-200/80 dark:bg-amber-900/60 text-amber-900 dark:text-amber-200 font-mono">
                                        {upstreamHeadersCount}
                                    </span>
                                )}
                            </button>
                        </>
                    )}

                    {/* 响应报文专属子项 */}
                    {mainTab === 'response' && (
                        <>
                            <button
                                type="button"
                                onClick={() => setRespSubTab('body')}
                                className={`flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-xs font-semibold transition-all ${
                                    respSubTab === 'body'
                                        ? 'bg-emerald-50 text-emerald-800 dark:bg-emerald-950/60 dark:text-emerald-200 border border-emerald-300 dark:border-emerald-800/60 shadow-2xs'
                                        : 'text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white hover:bg-gray-100 dark:hover:bg-base-300'
                                }`}
                            >
                                <FileText size={12} />
                                <span>{t('monitor.details.response_body', '响应正文 (Body)')}</span>
                            </button>

                            <button
                                type="button"
                                onClick={() => setRespSubTab('headers')}
                                className={`flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-xs font-semibold transition-all ${
                                    respSubTab === 'headers'
                                        ? 'bg-emerald-50 text-emerald-800 dark:bg-emerald-950/60 dark:text-emerald-200 border border-emerald-300 dark:border-emerald-800/60 shadow-2xs'
                                        : 'text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white hover:bg-gray-100 dark:hover:bg-base-300'
                                }`}
                            >
                                <ListFilter size={12} />
                                <span>{t('monitor.details.response_headers', '响应头 (Headers)')}</span>
                                {respHeadersCount > 0 && (
                                    <span className="text-[10px] px-1.5 py-0.2 rounded-full bg-emerald-100 dark:bg-emerald-950/60 text-emerald-800 dark:text-emerald-200 font-mono">
                                        {respHeadersCount}
                                    </span>
                                )}
                            </button>
                        </>
                    )}

                    {/* 耗时诊断标签提示 */}
                    {mainTab === 'timing' && (
                        <div className="text-[11px] text-gray-500 dark:text-gray-400 flex items-center gap-1">
                            <Clock size={12} className="text-purple-500" />
                            <span>{t('monitor.details.timing_full_chain', '全链路耗时细分诊断与多端点重试链路')}</span>
                        </div>
                    )}
                </div>

                {/* 右侧：结构化拆分 vs 原始代码 切换胶囊 */}
                <div className="flex items-center gap-2">
                    {isViewingBody && (
                        <div className="inline-flex items-center p-0.5 bg-gray-200/70 dark:bg-base-300 rounded-lg border border-gray-300/60 dark:border-base-300 text-[10px]">
                            <button
                                type="button"
                                onClick={() => handleBodyFormatChange('structured')}
                                className={`flex items-center gap-1 px-2 py-0.5 rounded-md font-semibold transition-all cursor-pointer ${
                                    bodyFormat === 'structured'
                                        ? 'bg-white dark:bg-base-100 text-blue-600 dark:text-blue-400 shadow-2xs'
                                        : 'text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white'
                                }`}
                                title={t('monitor.details.format_structured_title', '结构化分块展示（系统提示词、多轮对话、工具调用与参数）')}
                            >
                                <LayoutList size={11} />
                                <span>{t('monitor.details.format_structured', '结构化拆分')}</span>
                            </button>
                            <button
                                type="button"
                                onClick={() => handleBodyFormatChange('raw')}
                                className={`flex items-center gap-1 px-2 py-0.5 rounded-md font-semibold transition-all cursor-pointer ${
                                    bodyFormat === 'raw'
                                        ? 'bg-white dark:bg-base-100 text-blue-600 dark:text-blue-400 shadow-2xs'
                                        : 'text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white'
                                }`}
                                title={t('monitor.details.format_raw_title', '原始 JSON 代码高亮视图')}
                            >
                                <Code2 size={11} />
                                <span>{t('monitor.details.format_raw', '原始代码')}</span>
                            </button>
                        </div>
                    )}

                    {mainTab === 'response' && !isViewingBody && (
                        <span className="text-[11px] font-mono text-gray-500 dark:text-gray-400">
                            {t('monitor.details.duration', '耗时')}: {log.duration}ms
                        </span>
                    )}
                </div>
            </div>

            {/* 主内容区域 (Main Full-Height Viewport) */}
            <div className="flex-1 min-h-0 relative overflow-hidden bg-white dark:bg-base-200">
                {/* 1. 请求报文面板 */}
                {mainTab === 'request' && (
                    <>
                        {reqSubTab === 'body' && (
                            bodyFormat === 'structured' ? (
                                <StructuredPayloadView
                                    rawPayload={log.request_body}
                                    isResponse={false}
                                    onCopyText={onCopyText}
                                />
                            ) : (
                                <VirtualizedPayloadViewer
                                    cardId="req"
                                    title={t('monitor.details.request_payload', '请求报文 (Request)')}
                                    badge="REQUEST"
                                    badgeStyle="bg-blue-50 text-blue-700"
                                    rawPayload={log.request_body}
                                    concisePayload={conciseRequestBody}
                                    viewMode={payloadViewMode}
                                    emptyPlaceholder={t('monitor.details.payload_empty', '无请求报文')}
                                    hideHeadersSection={true}
                                    hideCardHeader={true}
                                    onCopy={async (text) => {
                                        await onCopyText(text);
                                    }}
                                    isCopied={false}
                                />
                            )
                        )}

                        {reqSubTab === 'headers' && (
                            <KeyValueHeadersTable
                                headersJson={log.request_headers}
                                title={t('monitor.details.request_headers_title', '客户端请求头 (Client Request Headers)')}
                                searchPlaceholder={t('monitor.details.search_headers', '搜索请求头...')}
                                emptyPlaceholder={t('monitor.details.no_headers', '无请求头数据')}
                                onCopyText={onCopyText}
                            />
                        )}

                        {reqSubTab === 'params' && (
                            <KeyValueHeadersTable
                                pairs={queryParams}
                                isQueryParams={true}
                                title={t('monitor.details.url_params_title', 'URL Query 查询参数')}
                                searchPlaceholder={t('monitor.details.search_params', '搜索 URL 参数...')}
                                emptyPlaceholder={t('monitor.details.no_params', 'URL 中无 Query 参数')}
                                onCopyText={onCopyText}
                            />
                        )}
                    </>
                )}

                {/* 2. 转出报文面板 */}
                {mainTab === 'forwarded' && (
                    <>
                        {forwardedSubTab === 'body' && (
                            bodyFormat === 'structured' ? (
                                <StructuredPayloadView
                                    rawPayload={log.upstream_request_body || log.request_body}
                                    isResponse={false}
                                    onCopyText={onCopyText}
                                />
                            ) : (
                                <VirtualizedPayloadViewer
                                    cardId="upstream"
                                    title={t('monitor.details.upstream_request_payload', '转出报文 (Forwarded)')}
                                    badge="FORWARDED"
                                    badgeStyle="bg-amber-50 text-amber-700"
                                    rawPayload={log.upstream_request_body || log.request_body}
                                    concisePayload={conciseUpstreamBody || conciseRequestBody}
                                    viewMode={payloadViewMode}
                                    emptyPlaceholder={t('monitor.details.no_upstream_payload', '无转出报文 (直接转发或未记录)')}
                                    hideHeadersSection={true}
                                    hideCardHeader={true}
                                    onCopy={async (text) => {
                                        await onCopyText(text);
                                    }}
                                    isCopied={false}
                                />
                            )
                        )}

                        {forwardedSubTab === 'headers' && (
                            <KeyValueHeadersTable
                                headersJson={log.upstream_request_headers || log.request_headers}
                                title={t('monitor.details.upstream_headers_title', '网关转出请求头 (Forwarded Headers)')}
                                searchPlaceholder={t('monitor.details.search_forwarded_headers', '搜索转出请求头...')}
                                emptyPlaceholder={t('monitor.details.no_upstream_headers', '无转出请求头')}
                                onCopyText={onCopyText}
                            />
                        )}
                    </>
                )}

                {/* 3. 响应报文面板 */}
                {mainTab === 'response' && (
                    <>
                        {respSubTab === 'body' && (
                            bodyFormat === 'structured' ? (
                                <StructuredPayloadView
                                    rawPayload={log.response_body}
                                    isResponse={true}
                                    onCopyText={onCopyText}
                                />
                            ) : (
                                <VirtualizedPayloadViewer
                                    cardId="resp"
                                    title={t('monitor.details.response_payload', '响应报文 (Response)')}
                                    badge="RESPONSE"
                                    badgeStyle="bg-emerald-50 text-emerald-700"
                                    rawPayload={log.response_body}
                                    concisePayload={conciseResponseBody}
                                    viewMode={payloadViewMode}
                                    emptyPlaceholder={t('monitor.details.payload_empty', '无响应报文')}
                                    hideHeadersSection={true}
                                    hideCardHeader={true}
                                    onCopy={async (text) => {
                                        await onCopyText(text);
                                    }}
                                    isCopied={false}
                                />
                            )
                        )}

                        {respSubTab === 'headers' && (
                            <KeyValueHeadersTable
                                headersJson={log.response_headers}
                                title={t('monitor.details.response_headers_title', '服务响应头 (Response Headers)')}
                                searchPlaceholder={t('monitor.details.search_response_headers', '搜索响应头...')}
                                emptyPlaceholder={t('monitor.details.no_headers', '无响应头数据')}
                                onCopyText={onCopyText}
                            />
                        )}
                    </>
                )}

                {/* 4. 耗时诊断面板 */}
                {mainTab === 'timing' && (
                    <div className="h-full overflow-y-auto p-4 bg-gray-50/50 dark:bg-base-100">
                        {timingNode || (
                            <div className="text-center text-gray-400 text-xs italic py-12">
                                {t('monitor.details.no_timing_data', '暂无阶段耗时诊断数据')}
                            </div>
                        )}
                    </div>
                )}
            </div>
        </div>
    );
};
