import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { FlaskConical, Play, RotateCcw, Square, X } from 'lucide-react';
import { cn } from '../../utils/cn';
import { copyToClipboard } from '../../utils/clipboard';
import { showToast } from '../common/ToastContainer';
import { PROBE_PROTOCOLS, ProbeInputError, classifyReply, sendProbe, type ProbeProtocol, type ProbeResult } from '../../utils/proxyProbe';

interface RequestProbeModalProps {
    model: string;
    baseUrl: string;
    apiKey: string;
    /** Result of an earlier check to show on open (opened from a status cell); no request is sent. */
    initialResult?: ProbeResult | null;
    onClose: () => void;
}

const PROTOCOL_LABELS: Record<ProbeProtocol, string> = { openai: 'OpenAI', anthropic: 'Anthropic', gemini: 'Gemini' };

const templateText = (protocol: ProbeProtocol, model: string) => JSON.stringify(PROBE_PROTOCOLS[protocol].template(model), null, 2);

function prettyBody(body: string): string {
    try {
        return JSON.stringify(JSON.parse(body), null, 2);
    } catch {
        return body;
    }
}

export function RequestProbeModal({ model, baseUrl, apiKey, initialResult, onClose }: RequestProbeModalProps) {
    const { t } = useTranslation();
    const [protocol, setProtocol] = useState<ProbeProtocol>('openai');
    const [bodyText, setBodyText] = useState(() => templateText('openai', model));
    const [result, setResult] = useState<ProbeResult | null>(initialResult ?? null);
    const [error, setError] = useState<string | null>(null);
    const [sending, setSending] = useState(false);
    const abortRef = useRef<AbortController | null>(null);

    useEffect(() => () => abortRef.current?.abort(), []);

    const switchProtocol = (next: ProbeProtocol) => {
        setProtocol(next);
        setBodyText(templateText(next, model));
    };

    const send = async () => {
        const controller = new AbortController();
        abortRef.current = controller;
        setSending(true);
        setResult(null);
        setError(null);
        try {
            setResult(await sendProbe(baseUrl, apiKey, protocol, model, bodyText, controller.signal));
        } catch (e) {
            if (controller.signal.aborted) return;
            const message = e instanceof Error ? e.message : String(e);
            setError(e instanceof ProbeInputError ? `${t('proxy.probe.invalid_json')}: ${message}` : message);
        } finally {
            if (abortRef.current === controller) abortRef.current = null;
            setSending(false);
        }
    };

    const status = result?.status ?? 0;
    const badgeClass = status >= 200 && status < 300 ? 'badge-success' : status === 429 ? 'badge-warning' : 'badge-error';
    const responseText = result ? prettyBody(result.body) : '';
    const verdict = result ? classifyReply(protocol, result.status, result.body) : null;
    const servedModel = verdict?.servedModel ?? result?.mappedModel ?? null;

    return (
        <div className="fixed inset-0 z-[300] flex items-center justify-center p-4 bg-black/50 backdrop-blur-sm animate-in fade-in duration-200">
            <div className="bg-white dark:bg-base-100 rounded-2xl shadow-2xl border border-gray-200 dark:border-base-300 w-full max-w-2xl max-h-[85vh] overflow-hidden animate-in zoom-in-95 duration-200 flex flex-col">
                {/* Header */}
                <div className="px-5 pt-4 pb-3 shrink-0 flex items-center justify-between">
                    <div className="flex items-center gap-2.5 min-w-0">
                        <div className="p-2 bg-blue-50 dark:bg-blue-900/20 rounded-lg">
                            <FlaskConical size={18} className="text-blue-500" />
                        </div>
                        <h3 className="text-sm font-bold text-gray-900 dark:text-base-content truncate">
                            {t('proxy.probe.title')} — <span className="font-mono">{model}</span>
                        </h3>
                    </div>
                    <button type="button" onClick={onClose} className="p-1.5 rounded-lg hover:bg-gray-100 dark:hover:bg-base-300 transition-colors">
                        <X size={16} className="text-gray-400" />
                    </button>
                </div>

                {/* Request */}
                <div className="px-5 pb-3 shrink-0 space-y-2 border-b border-gray-100 dark:border-base-200">
                    <div className="flex items-center gap-2">
                        <span className="text-[10px] font-bold text-gray-400 uppercase tracking-wider">{t('proxy.probe.protocol')}</span>
                        <div className="flex gap-1">
                            {(Object.keys(PROBE_PROTOCOLS) as ProbeProtocol[]).map((p) => (
                                <button
                                    type="button"
                                    key={p}
                                    onClick={() => switchProtocol(p)}
                                    className={cn(
                                        'px-2.5 py-0.5 rounded-full text-[11px] font-medium border transition-colors',
                                        protocol === p
                                            ? 'bg-blue-500 text-white border-blue-500'
                                            : 'bg-white dark:bg-base-200 text-gray-600 dark:text-gray-300 border-gray-200 dark:border-base-300 hover:border-blue-400',
                                    )}
                                >
                                    {PROTOCOL_LABELS[p]}
                                </button>
                            ))}
                        </div>
                    </div>
                    <div className="font-mono text-[11px] text-gray-500">POST {PROBE_PROTOCOLS[protocol].path(model)}</div>
                    <label className="block">
                        <span className="text-[10px] font-bold text-gray-400 uppercase tracking-wider">{t('proxy.probe.body')}</span>
                        <textarea
                            value={bodyText}
                            onChange={(e) => setBodyText(e.target.value)}
                            spellCheck={false}
                            rows={7}
                            className="mt-1 w-full px-3 py-2 font-mono text-[11px] bg-gray-50 dark:bg-base-200 border border-gray-200 dark:border-base-300 rounded-lg focus:ring-1 focus:ring-blue-500 focus:border-blue-500 outline-none resize-y"
                        />
                    </label>
                    <div className="flex items-center justify-between">
                        <button type="button" onClick={() => setBodyText(templateText(protocol, model))} className="px-2.5 py-1 rounded-lg text-xs font-semibold inline-flex items-center gap-1 border border-gray-200 dark:border-base-300 text-gray-600 dark:text-gray-300 hover:bg-gray-50 dark:hover:bg-base-200 transition-all disabled:opacity-40">
                            <RotateCcw size={12} /> {t('proxy.probe.template')}
                        </button>
                        <div className="flex gap-2">
                            <button type="button" onClick={() => abortRef.current?.abort()} disabled={!sending} className="px-2.5 py-1 rounded-lg text-xs font-semibold inline-flex items-center gap-1 border border-gray-200 dark:border-base-300 text-gray-600 dark:text-gray-300 hover:bg-gray-50 dark:hover:bg-base-200 transition-all disabled:opacity-40">
                                <Square size={12} /> {t('proxy.probe.stop')}
                            </button>
                            <button type="button" onClick={send} disabled={sending} className="px-2.5 py-1 rounded-lg text-xs font-semibold inline-flex items-center gap-1 bg-blue-600 hover:bg-blue-700 text-white shadow-xs transition-all active:scale-95 disabled:opacity-50 disabled:active:scale-100">
                                {sending ? <span className="loading loading-spinner loading-xs" /> : <Play size={12} />} {t('proxy.probe.send')}
                            </button>
                        </div>
                    </div>
                </div>

                {/* Response */}
                <div className="px-5 py-3 overflow-auto flex-1 min-h-0 space-y-2">
                    {error && <div className="text-xs text-red-500 break-all">{error}</div>}
                    {result && (
                        <>
                            {verdict && (
                                <div className={cn(
                                    'p-3 rounded-lg border text-xs space-y-1',
                                    verdict.state === 'ok' ? 'bg-green-50 border-green-200 text-green-800 dark:bg-green-900/20 dark:border-green-800 dark:text-green-300'
                                        : verdict.state === 'limited' ? 'bg-amber-50 border-amber-200 text-amber-800 dark:bg-amber-900/20 dark:border-amber-800 dark:text-amber-300'
                                            : 'bg-red-50 border-red-200 text-red-800 dark:bg-red-900/20 dark:border-red-800 dark:text-red-300',
                                )}>
                                    <div className="font-bold text-sm">
                                        {verdict.state === 'ok' ? '✓' : verdict.state === 'limited' ? '⚠' : '✗'} {t(`proxy.probe.verdict_${verdict.state}`)}
                                    </div>
                                    {servedModel && servedModel !== model && (
                                        <div className="font-mono">{t('proxy.probe.served_by', { requested: model, served: servedModel })}</div>
                                    )}
                                    {verdict.message && <div className="break-words">«{verdict.message}»</div>}
                                </div>
                            )}
                            <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-gray-500">
                                <span className={cn('badge badge-sm font-mono', badgeClass)}>{result.status}</span>
                                <span>{t('proxy.probe.ttft')} {result.ttftMs ?? '—'} ms</span>
                                <span>{t('proxy.probe.total')} {result.totalMs} ms</span>
                            </div>
                            <div className="flex flex-wrap gap-x-3 text-[11px] text-gray-500">
                                <span>{t('proxy.probe.account')} <span className="font-mono">{result.account ?? '—'}</span></span>
                                <span>{t('proxy.probe.mapped_model')} <span className="font-mono">{result.mappedModel ?? '—'}</span></span>
                            </div>
                            {result.headers.length > 0 && (
                                <details className="text-[11px]">
                                    <summary className="cursor-pointer text-[10px] font-bold text-gray-400 uppercase tracking-wider">
                                        {t('proxy.probe.headers')} ({result.headers.length})
                                    </summary>
                                    <div className="mt-1 p-2 bg-gray-50 dark:bg-base-200 rounded-lg font-mono break-all space-y-0.5">
                                        {result.headers.map(([name, value]) => (
                                            <div key={name}><span className="text-gray-500">{name}:</span> {value}</div>
                                        ))}
                                    </div>
                                </details>
                            )}
                            <div className="flex items-center justify-between">
                                <span className="text-[10px] font-bold text-gray-400 uppercase tracking-wider">{t('proxy.probe.response')}</span>
                                <button
                                    type="button"
                                    className="px-2.5 py-1 rounded-lg text-xs font-semibold inline-flex items-center gap-1 border border-gray-200 dark:border-base-300 text-gray-600 dark:text-gray-300 hover:bg-gray-50 dark:hover:bg-base-200 transition-all disabled:opacity-40"
                                    onClick={() => copyToClipboard(responseText).then((ok) => ok && showToast(t('common.copied'), 'success'))}
                                >
                                    {t('common.copy')}
                                </button>
                            </div>
                            <pre className="p-3 bg-gray-900 text-blue-100 rounded-lg text-[11px] font-mono whitespace-pre-wrap break-all">{responseText}</pre>
                        </>
                    )}
                </div>
            </div>
        </div>
    );
}
