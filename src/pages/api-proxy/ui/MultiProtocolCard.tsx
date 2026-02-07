// File: src/pages/api-proxy/ui/MultiProtocolCard.tsx
// Multi-protocol support information card

import { useTranslation } from 'react-i18next';
import { Code, Copy, CheckCircle } from 'lucide-react';
import type { ProtocolType, ProxyStatus } from '../lib/constants';
import type { AppConfig } from '@/entities/config';

interface MultiProtocolCardProps {
    appConfig: AppConfig;
    status: ProxyStatus;
    selectedProtocol: ProtocolType;
    copied: string | null;
    onSelectProtocol: (protocol: ProtocolType) => void;
    onCopy: (text: string, label: string) => void;
}

export function MultiProtocolCard({
    appConfig,
    status,
    selectedProtocol,
    copied,
    onSelectProtocol,
    onCopy,
}: MultiProtocolCardProps) {
    const { t } = useTranslation();
    const baseUrl = status.running ? status.base_url : `http://127.0.0.1:${appConfig.proxy.port || 8045}`;

    return (
        <div className="bg-white dark:bg-base-100 rounded-xl shadow-sm border border-gray-100 dark:border-base-200 overflow-hidden">
            <div className="p-3">
                <div className="flex items-center gap-3 mb-3">
                    <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-blue-500 to-purple-600 flex items-center justify-center shadow-md">
                        <Code size={16} className="text-white" />
                    </div>
                    <div>
                        <h3 className="text-base font-bold text-gray-900 dark:text-base-content">
                            {t('proxy.multi_protocol.title')}
                        </h3>
                        <p className="text-[10px] text-gray-500 dark:text-gray-400">
                            {t('proxy.multi_protocol.subtitle')}
                        </p>
                    </div>
                </div>

                <p className="text-xs text-gray-700 dark:text-gray-300 mb-4 leading-relaxed">
                    {t('proxy.multi_protocol.description')}
                </p>

                <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
                    {/* OpenAI Card */}
                    <div
                        className={`p-3 rounded-xl border-2 transition-all cursor-pointer ${selectedProtocol === 'openai' ? 'border-blue-500 bg-blue-50/30 dark:bg-blue-900/10' : 'border-gray-100 dark:border-base-200 hover:border-blue-200'}`}
                        onClick={() => onSelectProtocol('openai')}
                    >
                        <div className="flex items-center justify-between mb-2">
                            <span className="text-xs font-bold text-blue-600">{t('proxy.multi_protocol.openai_label')}</span>
                            <button onClick={(e) => { e.stopPropagation(); onCopy(`${baseUrl}/v1`, 'openai'); }} className="btn btn-ghost btn-xs">
                                {copied === 'openai' ? <CheckCircle size={14} /> : <div className="flex items-center gap-1 text-[10px] uppercase font-bold tracking-tighter"><Copy size={12} /> {t('proxy.multi_protocol.copy_base', { defaultValue: 'Base' })}</div>}
                            </button>
                        </div>
                        <div className="space-y-1">
                            <div className="flex items-center justify-between hover:bg-black/5 dark:hover:bg-white/5 rounded p-0.5 group">
                                <code className="text-[10px] opacity-70">/v1/chat/completions</code>
                                <button onClick={(e) => { e.stopPropagation(); onCopy(`${baseUrl}/v1/chat/completions`, 'openai-chat'); }} className="opacity-0 group-hover:opacity-100 transition-opacity">
                                    {copied === 'openai-chat' ? <CheckCircle size={10} className="text-green-500" /> : <Copy size={10} />}
                                </button>
                            </div>
                            <div className="flex items-center justify-between hover:bg-black/5 dark:hover:bg-white/5 rounded p-0.5 group">
                                <code className="text-[10px] opacity-70">/v1/completions</code>
                                <button onClick={(e) => { e.stopPropagation(); onCopy(`${baseUrl}/v1/completions`, 'openai-compl'); }} className="opacity-0 group-hover:opacity-100 transition-opacity">
                                    {copied === 'openai-compl' ? <CheckCircle size={10} className="text-green-500" /> : <Copy size={10} />}
                                </button>
                            </div>
                            <div className="flex items-center justify-between hover:bg-black/5 dark:hover:bg-white/5 rounded p-0.5 group">
                                <code className="text-[10px] opacity-70 font-bold text-blue-500">/v1/responses (Codex)</code>
                                <button onClick={(e) => { e.stopPropagation(); onCopy(`${baseUrl}/v1/responses`, 'openai-resp'); }} className="opacity-0 group-hover:opacity-100 transition-opacity">
                                    {copied === 'openai-resp' ? <CheckCircle size={10} className="text-green-500" /> : <Copy size={10} />}
                                </button>
                            </div>
                        </div>
                    </div>

                    {/* Anthropic Card */}
                    <div
                        className={`p-3 rounded-xl border-2 transition-all cursor-pointer ${selectedProtocol === 'anthropic' ? 'border-purple-500 bg-purple-50/30 dark:bg-purple-900/10' : 'border-gray-100 dark:border-base-200 hover:border-purple-200'}`}
                        onClick={() => onSelectProtocol('anthropic')}
                    >
                        <div className="flex items-center justify-between mb-2">
                            <span className="text-xs font-bold text-purple-600">{t('proxy.multi_protocol.anthropic_label')}</span>
                            <button onClick={(e) => { e.stopPropagation(); onCopy(`${baseUrl}/v1/messages`, 'anthropic'); }} className="btn btn-ghost btn-xs">
                                {copied === 'anthropic' ? <CheckCircle size={14} /> : <Copy size={14} />}
                            </button>
                        </div>
                        <code className="text-[10px] block truncate bg-black/5 dark:bg-white/5 p-1 rounded">/v1/messages</code>
                    </div>

                    {/* Gemini Card */}
                    <div
                        className={`p-3 rounded-xl border-2 transition-all cursor-pointer ${selectedProtocol === 'gemini' ? 'border-green-500 bg-green-50/30 dark:bg-green-900/10' : 'border-gray-100 dark:border-base-200 hover:border-green-200'}`}
                        onClick={() => onSelectProtocol('gemini')}
                    >
                        <div className="flex items-center justify-between mb-2">
                            <span className="text-xs font-bold text-green-600">{t('proxy.multi_protocol.gemini_label')}</span>
                            <button onClick={(e) => { e.stopPropagation(); onCopy(`${baseUrl}/v1beta/models`, 'gemini'); }} className="btn btn-ghost btn-xs">
                                {copied === 'gemini' ? <CheckCircle size={14} /> : <Copy size={14} />}
                            </button>
                        </div>
                        <code className="text-[10px] block truncate bg-black/5 dark:bg-white/5 p-1 rounded">/v1beta/models/...</code>
                    </div>
                </div>
            </div>
        </div>
    );
}
