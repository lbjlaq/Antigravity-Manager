import React, { useState } from 'react';
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

    if (!isOpen) return null;

    return (
        <div className="fixed inset-0 bg-black/70 backdrop-blur-md z-50 flex items-center justify-center p-4">
            <div className="bg-[#16181d] text-white rounded-2xl max-w-2xl w-full p-6 shadow-2xl border border-white/[0.08] flex flex-col max-h-[85vh]">
                {/* 标题 */}
                <div className="flex items-center justify-between pb-4 border-b border-white/[0.08]">
                    <div className="flex items-center gap-2">
                        <div className="p-1.5 rounded-lg bg-emerald-500/10 border border-emerald-500/20">
                            <DollarSign className="w-4 h-4 text-emerald-400" />
                        </div>
                        <h3 className="text-base font-semibold text-white">
                            {t('token_stats.pricing_settings', '模型单价配置 ($/1M Tokens)')}
                        </h3>
                    </div>
                    <button
                        onClick={onClose}
                        className="p-1 rounded-lg text-white/50 hover:text-white hover:bg-white/[0.06] transition-colors"
                    >
                        <X className="w-5 h-5" />
                    </button>
                </div>

                {/* 规则列表 */}
                <div className="overflow-y-auto flex-1 my-4 space-y-2 pr-2 scrollbar-thin scrollbar-thumb-white/10">
                    <div className="grid grid-cols-12 gap-2 text-[11px] font-semibold text-white/50 px-2 uppercase tracking-wider">
                        <div className="col-span-4">模型匹配标识</div>
                        <div className="col-span-2 text-right">输入 ($/1M)</div>
                        <div className="col-span-2 text-right">输出 ($/1M)</div>
                        <div className="col-span-2 text-right">缓存 ($/1M)</div>
                        <div className="col-span-2 text-center">操作</div>
                    </div>

                    {Object.entries(tempPricing).map(([mName, rule]) => (
                        <div
                            key={mName}
                            className="grid grid-cols-12 gap-2 items-center p-2 rounded-xl bg-white/[0.03] border border-white/[0.04] text-xs hover:border-white/[0.1] transition-colors"
                        >
                            <div className="col-span-4 font-mono text-[11px] font-medium text-white/90 truncate" title={mName}>
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
                                    className="w-full text-right px-2 py-1 bg-black/40 border border-white/10 rounded-lg text-xs font-mono text-white focus:outline-none focus:border-emerald-500"
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
                                    className="w-full text-right px-2 py-1 bg-black/40 border border-white/10 rounded-lg text-xs font-mono text-white focus:outline-none focus:border-emerald-500"
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
                                    className="w-full text-right px-2 py-1 bg-black/40 border border-white/10 rounded-lg text-xs font-mono text-white focus:outline-none focus:border-emerald-500"
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
                                        className="text-red-400/80 hover:text-red-400 p-1 rounded hover:bg-red-500/10 transition-colors"
                                        title="删除"
                                    >
                                        <Trash2 className="w-3.5 h-3.5" />
                                    </button>
                                )}
                            </div>
                        </div>
                    ))}

                    {/* 添加新规则 */}
                    <div className="p-3 rounded-xl border border-dashed border-white/10 bg-white/[0.01] mt-3 space-y-2">
                        <div className="text-[11px] font-medium text-white/50">添加自定义模型规则</div>
                        <div className="grid grid-cols-12 gap-2 items-center">
                            <input
                                type="text"
                                placeholder="模型标识 (如 claude-5)"
                                value={newModelName}
                                onChange={(e) => setNewModelName(e.target.value)}
                                className="col-span-4 px-2.5 py-1.5 bg-black/40 border border-white/10 rounded-lg text-xs font-mono text-white focus:outline-none focus:border-blue-500"
                            />
                            <input
                                type="number"
                                step="0.01"
                                placeholder="输入"
                                value={newInputPrice}
                                onChange={(e) => setNewInputPrice(e.target.value)}
                                className="col-span-2 text-right px-2 py-1.5 bg-black/40 border border-white/10 rounded-lg text-xs font-mono text-white focus:outline-none focus:border-blue-500"
                            />
                            <input
                                type="number"
                                step="0.01"
                                placeholder="输出"
                                value={newOutputPrice}
                                onChange={(e) => setNewOutputPrice(e.target.value)}
                                className="col-span-2 text-right px-2 py-1.5 bg-black/40 border border-white/10 rounded-lg text-xs font-mono text-white focus:outline-none focus:border-blue-500"
                            />
                            <input
                                type="number"
                                step="0.001"
                                placeholder="缓存"
                                value={newCachePrice}
                                onChange={(e) => setNewCachePrice(e.target.value)}
                                className="col-span-2 text-right px-2 py-1.5 bg-black/40 border border-white/10 rounded-lg text-xs font-mono text-white focus:outline-none focus:border-blue-500"
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
                                <Plus className="w-3.5 h-3.5" /> 添加
                            </button>
                        </div>
                    </div>
                </div>

                {/* 底部按钮栏 */}
                <div className="flex items-center justify-between pt-4 border-t border-white/[0.08]">
                    <button
                        onClick={() => setTempPricing(DEFAULT_PRICING)}
                        className="px-3 py-1.5 text-xs text-white/60 hover:text-white flex items-center gap-1.5 border border-white/10 rounded-xl hover:bg-white/[0.06] transition-colors"
                    >
                        <RotateCcw className="w-3.5 h-3.5" />
                        {t('token_stats.reset_pricing', '恢复默认')}
                    </button>
                    <div className="flex gap-2">
                        <button
                            onClick={onClose}
                            className="px-4 py-1.5 text-xs font-medium text-white/60 hover:text-white hover:bg-white/[0.06] rounded-xl transition-colors"
                        >
                            取消
                        </button>
                        <button
                            onClick={() => onSave(tempPricing)}
                            className="px-4 py-1.5 text-xs font-medium text-white bg-emerald-600 hover:bg-emerald-500 rounded-xl shadow-lg shadow-emerald-900/30 transition-colors"
                        >
                            {t('token_stats.save_pricing', '保存单价')}
                        </button>
                    </div>
                </div>
            </div>
        </div>
    );
};
