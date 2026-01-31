// File: src/pages/api-proxy/ui/ModelsTableCard.tsx
// Supported models table with code preview

import { useTranslation } from 'react-i18next';
import { Terminal, Copy, CheckCircle } from 'lucide-react';
import type { ProtocolType } from '../lib/constants';

interface ModelInfo {
    id: string;
    name: string;
    icon: React.ReactNode;
    desc: string;
    group?: string;
}

interface ModelsTableCardProps {
    models: ModelInfo[];
    selectedModelId: string;
    selectedProtocol: ProtocolType;
    copied: string | null;
    onSelectModel: (id: string) => void;
    onCopy: (text: string, label: string) => void;
    getPythonExample: (modelId: string) => string;
}

export function ModelsTableCard({
    models,
    selectedModelId,
    selectedProtocol,
    copied,
    onSelectModel,
    onCopy,
    getPythonExample,
}: ModelsTableCardProps) {
    const { t } = useTranslation();

    return (
        <div className="bg-white dark:bg-base-100 rounded-xl shadow-sm border border-gray-100 dark:border-base-200 overflow-hidden mt-4">
            <div className="px-4 py-2.5 border-b border-gray-100 dark:border-base-200">
                <h2 className="text-base font-bold text-gray-900 dark:text-base-content flex items-center gap-2">
                    <Terminal size={18} />
                    {t('proxy.supported_models.title')}
                </h2>
            </div>

            <div className="grid grid-cols-1 lg:grid-cols-3 gap-0 lg:divide-x dark:divide-gray-700">
                {/* Left: Models table */}
                <div className="col-span-2 p-0">
                    <div className="overflow-x-auto">
                        <table className="table w-full">
                            <thead className="bg-gray-50/50 dark:bg-gray-800/50 text-gray-500 dark:text-gray-400">
                                <tr>
                                    <th className="w-10 pl-3"></th>
                                    <th className="text-[11px] font-medium">{t('proxy.supported_models.model_name')}</th>
                                    <th className="text-[11px] font-medium">{t('proxy.supported_models.model_id')}</th>
                                    <th className="text-[11px] hidden sm:table-cell font-medium">{t('proxy.supported_models.description')}</th>
                                    <th className="text-[11px] w-20 text-center font-medium">{t('proxy.supported_models.action')}</th>
                                </tr>
                            </thead>
                            <tbody>
                                {models.map((m) => (
                                    <tr
                                        key={m.id}
                                        className={`hover:bg-blue-50/50 dark:hover:bg-blue-900/10 cursor-pointer transition-colors ${selectedModelId === m.id ? 'bg-blue-50/80 dark:bg-blue-900/20' : ''}`}
                                        onClick={() => onSelectModel(m.id)}
                                    >
                                        <td className="pl-4 text-blue-500">{m.icon}</td>
                                        <td className="font-bold text-xs">{m.name}</td>
                                        <td className="font-mono text-[10px] text-gray-500">{m.id}</td>
                                        <td className="text-[10px] text-gray-400 hidden sm:table-cell">{m.desc}</td>
                                        <td className="text-center">
                                            <button
                                                className="btn btn-ghost btn-xs text-blue-500"
                                                onClick={(e) => { e.stopPropagation(); onCopy(m.id, `model-${m.id}`); }}
                                            >
                                                {copied === `model-${m.id}` ? <CheckCircle size={14} /> : <div className="flex items-center gap-1 text-[10px] font-bold tracking-tight"><Copy size={12} /> {t('common.copy')}</div>}
                                            </button>
                                        </td>
                                    </tr>
                                ))}
                            </tbody>
                        </table>
                    </div>
                </div>

                {/* Right: Code preview */}
                <div className="col-span-1 bg-gray-900 text-blue-100 flex flex-col h-[400px] lg:h-auto">
                    <div className="p-3 border-b border-gray-800 flex items-center justify-between">
                        <span className="text-xs font-bold text-gray-400 uppercase tracking-wider">{t('proxy.multi_protocol.quick_integration')}</span>
                        <div className="flex gap-2">
                            <span className="text-[10px] px-2 py-0.5 rounded bg-blue-500/20 text-blue-400 border border-blue-500/30">
                                {selectedProtocol === 'anthropic' ? 'Python (Anthropic SDK)' : (selectedProtocol === 'gemini' ? 'Python (Google GenAI)' : 'Python (OpenAI SDK)')}
                            </span>
                        </div>
                    </div>
                    <div className="flex-1 relative overflow-hidden group">
                        <div className="absolute inset-0 overflow-auto scrollbar-thin scrollbar-thumb-gray-700 scrollbar-track-transparent">
                            <pre className="p-4 text-[10px] font-mono leading-relaxed">
                                {getPythonExample(selectedModelId)}
                            </pre>
                        </div>
                        <button
                            onClick={() => onCopy(getPythonExample(selectedModelId), 'example-code')}
                            className="absolute top-4 right-4 p-2 bg-white/10 hover:bg-white/20 rounded-lg transition-colors text-white opacity-0 group-hover:opacity-100"
                        >
                            {copied === 'example-code' ? <CheckCircle size={16} /> : <Copy size={16} />}
                        </button>
                    </div>
                    <div className="p-3 bg-gray-800/50 border-t border-gray-800 text-[10px] text-gray-400">
                        {t('proxy.multi_protocol.click_tip')}
                    </div>
                </div>
            </div>
        </div>
    );
}
