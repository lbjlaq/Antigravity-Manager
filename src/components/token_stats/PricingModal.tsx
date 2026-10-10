import React, { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { DollarSign, X, RotateCcw, Plus, Trash2 } from 'lucide-react';
import { ModelPricingRule, DEFAULT_PRICING } from '../../pages/TokenStats';

interface PricingModalProps {
    isOpen: boolean;
    onClose: () => void;
    currentPricing: Record<string, ModelPricingRule>;
    onSave: (pricing: Record<string, ModelPricingRule>) => void;
}

export const PricingModal: React.FC<PricingModalProps> = ({
    isOpen,
    onClose,
    currentPricing,
    onSave,
}) => {
    const { t } = useTranslation();
    const [tempPricing, setTempPricing] = useState<Record<string, ModelPricingRule>>(currentPricing);
    const [newModelName, setNewModelName] = useState('');
    const [newInputPrice, setNewInputPrice] = useState('1.00');
    const [newOutputPrice, setNewOutputPrice] = useState('5.00');
    const [newCachePrice, setNewCachePrice] = useState('0.10');

    useEffect(() => {
        if (isOpen) {
            setTempPricing(currentPricing);
        }
    }, [isOpen, currentPricing]);

    if (!isOpen) return null;

    return (
        <div className="fixed inset-0 bg-black/50 dark:bg-black/70 backdrop-blur-md z-50 flex items-center justify-center p-4">
            <div className="bg-white dark:bg-[#16181d] text-gray-900 dark:text-white rounded-2xl max-w-2xl w-full p-6 shadow-2xl border border-gray-200 dark:border-white/[0.08] flex flex-col max-h-[85vh]">
                {/* 标题 */}
                <div className="flex items-center justify-between pb-4 border-b border-gray-200 dark:border-white/[0.08]">
                    <div className="flex items-center gap-2">
                        <div className="p-1.5 rounded-lg bg-emerald-500/10 border border-emerald-500/20">
                            <DollarSign className="w-4 h-4 text-emerald-600 dark:text-emerald-400" />
                        </div>
                        <h3 className="text-base font-semibold text-gray-900 dark:text-white">
                            {t('token_stats.pricing_settings', '模型价格配置 ($/1M Tokens)')}
                        </h3>
                    </div>
                    <button
                        onClick={onClose}
                        className="p-1 rounded-lg text-gray-400 hover:text-gray-700 dark:text-white/50 dark:hover:text-white hover:bg-gray-100 dark:hover:bg-white/[0.06] transition-colors"
                    >
                        <X className="w-5 h-5" />
                    </button>
                </div>

                {/* 规则列表 */}
                <div className="overflow-y-auto flex-1 my-4 space-y-2 pr-2 scrollbar-thin scrollbar-thumb-gray-200 dark:scrollbar-thumb-white/10">
                    <div className="grid grid-cols-12 gap-2 text-[11px] font-semibold text-gray-500 dark:text-white/50 px-2 uppercase tracking-wider">
                        <div className="col-span-4">{t('token_stats.model_pattern_label', '模型匹配标识')}</div>
                        <div className="col-span-2 text-right">{t('token_stats.input_price_label', '输入 ($/1M)')}</div>
                        <div className="col-span-2 text-right">{t('token_stats.output_price_label', '输出 ($/1M)')}</div>
                        <div className="col-span-2 text-right">{t('token_stats.cache_price_label', '缓存 ($/1M)')}</div>
                        <div className="col-span-2 text-center">{t('token_stats.actions', '操作')}</div>
                    </div>

                    {Object.entries(tempPricing).map(([mName, rule]) => (
                        <div
                            key={mName}
                            className="grid grid-cols-12 gap-2 items-center p-2 rounded-xl bg-gray-50/70 dark:bg-white/[0.03] border border-gray-200/80 dark:border-white/[0.04] text-xs hover:border-gray-300 dark:hover:border-white/[0.1] transition-colors"
                        >
                            <div className="col-span-4 font-mono text-[11px] font-medium text-gray-800 dark:text-white/90 truncate" title={mName}>
                                {mName}
                            </div>
                            <div className="col-span-2">
                                <input
                                    type="number"
                                    step="0.01"
                                    value={rule.input}
                                    onChange={(e) => {
                                        const val = parseFloat(e.target.value) || 0;
                                        setTempPricing((prev) => ({
                                            ...prev,
                                            [mName]: { ...prev[mName], input: val },
                                        }));
                                    }}
                                    className="w-full text-right px-2 py-1 bg-white dark:bg-black/40 border border-gray-300 dark:border-white/10 rounded-lg text-xs font-mono text-gray-900 dark:text-white focus:outline-none focus:border-emerald-500"
                                />
                            </div>
                            <div className="col-span-2">
                                <input
                                    type="number"
                                    step="0.01"
                                    value={rule.output}
                                    onChange={(e) => {
                                        const val = parseFloat(e.target.value) || 0;
                                        setTempPricing((prev) => ({
                                            ...prev,
                                            [mName]: { ...prev[mName], output: val },
                                        }));
                                    }}
                                    className="w-full text-right px-2 py-1 bg-white dark:bg-black/40 border border-gray-300 dark:border-white/10 rounded-lg text-xs font-mono text-gray-900 dark:text-white focus:outline-none focus:border-emerald-500"
                                />
                            </div>
                            <div className="col-span-2">
                                <input
                                    type="number"
                                    step="0.001"
                                    value={rule.cached}
                                    onChange={(e) => {
                                        const val = parseFloat(e.target.value) || 0;
                                        setTempPricing((prev) => ({
                                            ...prev,
                                            [mName]: { ...prev[mName], cached: val },
                                        }));
                                    }}
                                    className="w-full text-right px-2 py-1 bg-white dark:bg-black/40 border border-gray-300 dark:border-white/10 rounded-lg text-xs font-mono text-gray-900 dark:text-white focus:outline-none focus:border-emerald-500"
                                />
                            </div>
                            <div className="col-span-2 flex justify-center">
                                {mName !== 'default' && (
                                    <button
                                        onClick={() => {
                                            const next = { ...tempPricing };
                                            delete next[mName];
                                            setTempPricing(next);
                                        }}
                                        className="text-red-500 hover:text-red-700 dark:text-red-400/80 dark:hover:text-red-400 p-1 rounded hover:bg-red-50 dark:hover:bg-red-500/10 transition-colors"
                                        title={t('token_stats.delete', '删除')}
                                    >
                                        <Trash2 className="w-3.5 h-3.5" />
                                    </button>
                                )}
                            </div>
                        </div>
                    ))}

                    {/* 添加新规则 */}
                    <div className="p-3 rounded-xl border border-dashed border-gray-300 dark:border-white/10 bg-gray-50/50 dark:bg-white/[0.01] mt-3 space-y-2">
                        <div className="text-[11px] font-medium text-gray-500 dark:text-white/50">{t('token_stats.add_custom_pricing', '添加自定义模型规则')}</div>
                        <div className="grid grid-cols-12 gap-2 items-center">
                            <input
                                type="text"
                                placeholder={t('token_stats.model_pattern_placeholder', '模型匹配标识 (如 claude-5)')}
                                value={newModelName}
                                onChange={(e) => setNewModelName(e.target.value)}
                                className="col-span-4 px-2.5 py-1.5 bg-white dark:bg-black/40 border border-gray-300 dark:border-white/10 rounded-lg text-xs font-mono text-gray-900 dark:text-white focus:outline-none focus:border-blue-500"
                            />
                            <input
                                type="number"
                                step="0.01"
                                placeholder={t('token_stats.input', '输入')}
                                value={newInputPrice}
                                onChange={(e) => setNewInputPrice(e.target.value)}
                                className="col-span-2 text-right px-2 py-1.5 bg-white dark:bg-black/40 border border-gray-300 dark:border-white/10 rounded-lg text-xs font-mono text-gray-900 dark:text-white focus:outline-none focus:border-blue-500"
                            />
                            <input
                                type="number"
                                step="0.01"
                                placeholder={t('token_stats.output', '输出')}
                                value={newOutputPrice}
                                onChange={(e) => setNewOutputPrice(e.target.value)}
                                className="col-span-2 text-right px-2 py-1.5 bg-white dark:bg-black/40 border border-gray-300 dark:border-white/10 rounded-lg text-xs font-mono text-gray-900 dark:text-white focus:outline-none focus:border-blue-500"
                            />
                            <input
                                type="number"
                                step="0.001"
                                placeholder={t('token_stats.cached', '缓存')}
                                value={newCachePrice}
                                onChange={(e) => setNewCachePrice(e.target.value)}
                                className="col-span-2 text-right px-2 py-1.5 bg-white dark:bg-black/40 border border-gray-300 dark:border-white/10 rounded-lg text-xs font-mono text-gray-900 dark:text-white focus:outline-none focus:border-blue-500"
                            />
                            <button
                                onClick={() => {
                                    if (!newModelName.trim()) return;
                                    const k = newModelName.trim().toLowerCase();
                                    setTempPricing((prev) => ({
                                        ...prev,
                                        [k]: {
                                            input: parseFloat(newInputPrice) || 0,
                                            output: parseFloat(newOutputPrice) || 0,
                                            cached: parseFloat(newCachePrice) || 0,
                                        },
                                    }));
                                    setNewModelName('');
                                }}
                                className="col-span-2 px-2 py-1.5 bg-blue-600 hover:bg-blue-500 text-white rounded-lg text-xs font-medium flex items-center justify-center gap-1 transition-colors"
                            >
                                <Plus className="w-3.5 h-3.5" /> {t('token_stats.add', '添加')}
                            </button>
                        </div>
                    </div>
                </div>

                {/* 底部按钮栏 */}
                <div className="flex items-center justify-between pt-4 border-t border-gray-200 dark:border-white/[0.08]">
                    <button
                        onClick={() => setTempPricing(DEFAULT_PRICING)}
                        className="px-3 py-1.5 text-xs text-gray-600 hover:text-gray-900 dark:text-white/60 dark:hover:text-white flex items-center gap-1.5 border border-gray-300 dark:border-white/10 rounded-xl hover:bg-gray-100 dark:hover:bg-white/[0.06] transition-colors"
                    >
                        <RotateCcw className="w-3.5 h-3.5" />
                        {t('token_stats.reset_pricing', '恢复默认')}
                    </button>
                    <div className="flex gap-2">
                        <button
                            onClick={onClose}
                            className="px-4 py-1.5 text-xs font-medium text-gray-600 hover:text-gray-900 dark:text-white/60 dark:hover:text-white hover:bg-gray-100 dark:hover:bg-white/[0.06] rounded-xl transition-colors"
                        >
                            {t('token_stats.cancel', '取消')}
                        </button>
                        <button
                            onClick={() => onSave(tempPricing)}
                            className="px-4 py-1.5 text-xs font-medium text-white bg-emerald-600 hover:bg-emerald-500 rounded-xl shadow-lg shadow-emerald-900/30 transition-colors"
                        >
                            {t('token_stats.save_pricing', '保存配置')}
                        </button>
                    </div>
                </div>
            </div>
        </div>
    );
};
