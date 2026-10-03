import { useMemo, useEffect, useState } from 'react';
import { MODEL_CONFIG, compareModelsDesc, inferModelGroup } from '../config/modelConfig';
import { useAccountStore } from '../stores/useAccountStore';
import { Bot, Sparkles } from 'lucide-react';
import { request } from '../utils/request';

export interface CanonicalFamilyDto {
    canonical_id: string;
    display_name: string;
    match_ids: string[];
}

const ALIAS_TO_CANONICAL: Record<string, { id: string; name: string; group: string }> = {
    // Gemini 3.8
    'gemini-3.8-flash': { id: 'gemini-3.8-flash', name: 'gemini-3.8-flash', group: 'Gemini 3' },
    'gemini-3.8-flash-tiered': { id: 'gemini-3.8-flash-tiered', name: 'gemini-3.8-flash-tiered', group: 'Gemini 3' },
    'gemini-3.8-flash-high': { id: 'gemini-3.8-flash-high', name: 'gemini-3.8-flash-high', group: 'Gemini 3' },
    'gemini-3.8-flash-medium': { id: 'gemini-3.8-flash-medium', name: 'gemini-3.8-flash-medium', group: 'Gemini 3' },
    'gemini-3.8-flash-low': { id: 'gemini-3.8-flash-low', name: 'gemini-3.8-flash-low', group: 'Gemini 3' },

    // Gemini 3.7
    'gemini-3.7-flash': { id: 'gemini-3.7-flash', name: 'gemini-3.7-flash', group: 'Gemini 3' },
    'gemini-3.7-flash-high': { id: 'gemini-3.7-flash-high', name: 'gemini-3.7-flash-high', group: 'Gemini 3' },
    'gemini-3.7-flash-medium': { id: 'gemini-3.7-flash-medium', name: 'gemini-3.7-flash-medium', group: 'Gemini 3' },
    'gemini-3.7-flash-low': { id: 'gemini-3.7-flash-low', name: 'gemini-3.7-flash-low', group: 'Gemini 3' },
    'gemini-3.7-flash-tiered': { id: 'gemini-3.7-flash-tiered', name: 'gemini-3.7-flash-tiered', group: 'Gemini 3' },

    // Gemini 3.6
    'gemini-3.6-flash': { id: 'gemini-3.6-flash', name: 'gemini-3.6-flash', group: 'Gemini 3' },
    'gemini-3.6-flash-high': { id: 'gemini-3.6-flash-high', name: 'gemini-3.6-flash-high', group: 'Gemini 3' },
    'gemini-3.6-flash-medium': { id: 'gemini-3.6-flash-medium', name: 'gemini-3.6-flash-medium', group: 'Gemini 3' },
    'gemini-3.6-flash-low': { id: 'gemini-3.6-flash-low', name: 'gemini-3.6-flash-low', group: 'Gemini 3' },
    'gemini-3.6-flash-tiered': { id: 'gemini-3.6-flash-tiered', name: 'gemini-3.6-flash-tiered', group: 'Gemini 3' },

    // Gemini 3.5 & Pro & Image & Lite
    'gemini-3.5-flash': { id: 'gemini-3.5-flash', name: 'gemini-3.5-flash', group: 'Gemini 3' },
    'gemini-3.5-flash-low': { id: 'gemini-3.5-flash-low', name: 'gemini-3.5-flash-low', group: 'Gemini 3' },
    'gemini-3.5-flash-extra-low': { id: 'gemini-3.5-flash-extra-low', name: 'gemini-3.5-flash-extra-low', group: 'Gemini 3' },
    'gemini-3.1-pro-high': { id: 'gemini-3.1-pro-high', name: 'gemini-3.1-pro-high', group: 'Gemini 3' },
    'gemini-3.1-pro-low': { id: 'gemini-3.1-pro-low', name: 'gemini-3.1-pro-low', group: 'Gemini 3' },
    'gemini-3.1-flash-lite': { id: 'gemini-3.1-flash-lite', name: 'gemini-3.1-flash-lite', group: 'Gemini 3' },
    'gemini-flash-lite': { id: 'gemini-3.1-flash-lite', name: 'gemini-3.1-flash-lite', group: 'Gemini 3' },
    'gemini-3.1-flash-image': { id: 'gemini-3.1-flash-image', name: 'gemini-3.1-flash-image', group: 'Gemini 3' },
    'gemini-3-pro-image': { id: 'gemini-3-pro-image', name: 'gemini-3-pro-image', group: 'Gemini 3' },

    // Claude (基准线 >= 4.6)
    'claude-sonnet-4-6': { id: 'claude-sonnet-4-6', name: 'claude-sonnet-4-6', group: 'Claude' },
    'claude-sonnet-4-6-thinking': { id: 'claude-sonnet-4-6-thinking', name: 'claude-sonnet-4-6-thinking', group: 'Claude' },
    'claude-opus-4-6': { id: 'claude-opus-4-6', name: 'claude-opus-4-6', group: 'Claude' },
    'claude-opus-4-6-thinking': { id: 'claude-opus-4-6-thinking', name: 'claude-opus-4-6-thinking', group: 'Claude' },

    // OpenAI (以官方为准)
    'gpt-oss-120b-medium': { id: 'gpt-oss-120b-medium', name: 'gpt-oss-120b-medium', group: 'Other' },
};

export const useProxyModels = () => {
    const { accounts, fetchAccounts } = useAccountStore();
    const [canonicalFamilies, setCanonicalFamilies] = useState<CanonicalFamilyDto[]>([]);

    useEffect(() => {
        if (accounts.length === 0) {
            fetchAccounts();
        }

        let cancelled = false;
        request<CanonicalFamilyDto[]>('get_canonical_families')
            .then(data => {
                if (!cancelled && data) {
                    setCanonicalFamilies(data);
                }
            })
            .catch(err => console.error('Failed to fetch canonical families:', err));

        return () => { cancelled = true; };
    }, []); // eslint-disable-line react-hooks/exhaustive-deps

    const models = useMemo(() => {
        const uniqueModelsMap = new Map<string, { id: string; name: string; group: string; icon: React.ReactNode }>();

        // 1. Process dynamic models reported by accounts
        for (const account of accounts) {
            for (const m of account.quota?.models ?? []) {
                const rawKey = m.name.toLowerCase();
                
                // Map sub-tier and legacy aliases to the primary canonical model
                const mapped = ALIAS_TO_CANONICAL[rawKey];
                const canonicalId = mapped ? mapped.id : (m.name || rawKey);
                const canonicalName = canonicalId; // 方案 A: 统一采用纯正 Model ID，彻底去除不一致的括号与中文别名
                
                // 泛化推导分组，全面兼容未知/未来模型 (如 Gemini 3.9 / 4 / 5 等)
                const group = mapped ? mapped.group : inferModelGroup(canonicalId || canonicalName);

                const cfgEntry = Object.entries(MODEL_CONFIG).find(
                    ([cfgId, cfg]) => cfgId.toLowerCase() === canonicalId.toLowerCase() || cfg.protectedKey?.toLowerCase() === canonicalId.toLowerCase()
                );
                const CfgIcon = cfgEntry?.[1].Icon;
                const icon = CfgIcon ? <CfgIcon size={16} /> : (group === 'Claude' ? <Sparkles size={16} className="text-purple-400" /> : <Bot size={16} className="text-blue-400" />);

                if (!uniqueModelsMap.has(canonicalId)) {
                    uniqueModelsMap.set(canonicalId, {
                        id: canonicalId,
                        name: canonicalName,
                        group,
                        icon,
                    });
                }
            }
        }

        // 2. Supplement with built-in core models from MODEL_CONFIG
        for (const [id, config] of Object.entries(MODEL_CONFIG)) {
            const rawKey = id.toLowerCase();
            const mapped = ALIAS_TO_CANONICAL[rawKey];
            const canonicalId = mapped ? mapped.id : id;
            const canonicalName = canonicalId; // 方案 A: 统一采用纯正 Model ID
            const group = mapped ? mapped.group : (config.group || inferModelGroup(canonicalId));

            // Skip legacy marketing adjective labels
            if (['Melhor Raciocínio', 'Visualização Flash', 'Geração de Imagem (1:1)', 'Alto Desempenho', '最高推理', '快速响应'].includes(canonicalName)) {
                continue;
            }

            if (!uniqueModelsMap.has(canonicalId)) {
                uniqueModelsMap.set(canonicalId, {
                    id: canonicalId,
                    name: canonicalName,
                    group,
                    icon: <config.Icon size={16} />,
                });
            }
        }

        return Array.from(uniqueModelsMap.values())
            .map(m => ({
                id: m.id,
                name: m.id,
                desc: m.id,
                group: m.group,
                icon: m.icon,
            }))
            .sort(compareModelsDesc);
    }, [accounts, canonicalFamilies]);

    return { models, canonicalFamilies };
};
