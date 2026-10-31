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
    // Gemini 4 系列 (Argon)
    'gemini-4-argon': { id: 'gemini-4-argon', name: 'gemini-4-argon', group: 'Gemini 4' },
    'gemini-4-argon-high': { id: 'gemini-4-argon-high', name: 'gemini-4-argon-high', group: 'Gemini 4' },
    'gemini-4-argon-medium': { id: 'gemini-4-argon-medium', name: 'gemini-4-argon-medium', group: 'Gemini 4' },
    'gemini-4-argon-low': { id: 'gemini-4-argon-low', name: 'gemini-4-argon-low', group: 'Gemini 4' },

    // Gemini 3.8
    'gemini-3.8-flash': { id: 'gemini-3.8-flash', name: 'gemini-3.8-flash', group: 'Gemini 3' },
    'gemini-3.8-flash-tiered': { id: 'gemini-3.8-flash-tiered', name: 'gemini-3.8-flash-tiered', group: 'Gemini 3' },
    'gemini-3.8-flash-high': { id: 'gemini-3.8-flash-high', name: 'gemini-3.8-flash-high', group: 'Gemini 3' },
    'gemini-3.8-flash-medium': { id: 'gemini-3.8-flash-medium', name: 'gemini-3.8-flash-medium', group: 'Gemini 3' },
    'gemini-3.8-flash-low': { id: 'gemini-3.8-flash-low', name: 'gemini-3.8-flash-low', group: 'Gemini 3' },

    // Gemini 3.1 / 3.5 Lite & Pro & Image
    'gemini-3.1-pro': { id: 'gemini-3.1-pro', name: 'gemini-3.1-pro', group: 'Gemini 3' },
    'gemini-3.1-pro-high': { id: 'gemini-3.1-pro-high', name: 'gemini-3.1-pro-high', group: 'Gemini 3' },
    'gemini-3.1-pro-low': { id: 'gemini-3.1-pro-low', name: 'gemini-3.1-pro-low', group: 'Gemini 3' },
    'gemini-3.5-flash-lite': { id: 'gemini-3.5-flash-lite', name: 'gemini-3.5-flash-lite', group: 'Gemini 3' },
    'gemini-3.1-flash-lite': { id: 'gemini-3.1-flash-lite', name: 'gemini-3.1-flash-lite', group: 'Gemini 3' },
    'gemini-flash-lite': { id: 'gemini-3.1-flash-lite', name: 'gemini-3.1-flash-lite', group: 'Gemini 3' },
    'gemini-3.1-flash-image': { id: 'gemini-3.1-flash-image', name: 'gemini-3.1-flash-image', group: 'Gemini 3' },
    'gemini-3-pro-image': { id: 'gemini-3-pro-image', name: 'gemini-3-pro-image', group: 'Gemini 3' },

    // Claude 5.5 & 4.6 (基准线 >= 4.6)
    'claude-opus-5-5': { id: 'claude-opus-5-5', name: 'claude-opus-5-5', group: 'Claude' },
    'claude-opus-5-5-high': { id: 'claude-opus-5-5-high', name: 'claude-opus-5-5-high', group: 'Claude' },
    'claude-opus-5-5-medium': { id: 'claude-opus-5-5-medium', name: 'claude-opus-5-5-medium', group: 'Claude' },
    'claude-opus-5-5-low': { id: 'claude-opus-5-5-low', name: 'claude-opus-5-5-low', group: 'Claude' },
    'claude-sonnet-5-5': { id: 'claude-sonnet-5-5', name: 'claude-sonnet-5-5', group: 'Claude' },
    'claude-sonnet-5-5-high': { id: 'claude-sonnet-5-5-high', name: 'claude-sonnet-5-5-high', group: 'Claude' },
    'claude-sonnet-5-5-medium': { id: 'claude-sonnet-5-5-medium', name: 'claude-sonnet-5-5-medium', group: 'Claude' },
    'claude-sonnet-5-5-low': { id: 'claude-sonnet-5-5-low', name: 'claude-sonnet-5-5-low', group: 'Claude' },
    'claude-sonnet-4-6': { id: 'claude-sonnet-4-6', name: 'claude-sonnet-4-6', group: 'Claude' },
    'claude-opus-4-6': { id: 'claude-opus-4-6', name: 'claude-opus-4-6', group: 'Claude' },
    'claude-opus-4-6-thinking': { id: 'claude-opus-4-6-thinking', name: 'claude-opus-4-6-thinking', group: 'Claude' },

    // OpenAI (以官方为准)
    'gpt-oss-120b': { id: 'gpt-oss-120b', name: 'gpt-oss-120b', group: 'Other' },
    'gpt-oss-120b-medium': { id: 'gpt-oss-120b-medium', name: 'gpt-oss-120b-medium', group: 'Other' },
};

function isCompliantModel(
    key: string,
    deprecatedSet?: Set<string>,
    activeFlashMinorFloor?: number
): boolean {
    const m = key.toLowerCase().trim();
    if (
        m.includes('-disabled') ||
        m.endsWith('_disabled') ||
        m.startsWith('chat_') ||
        m.startsWith('tab_') ||
        m.includes('internal') ||
        m.includes('-exp') ||
        m.endsWith('-agent') ||
        m === 'gemini-3-flash' ||
        // 未在官方可用集合里的字典残留（已从后端可用集合移除）
        m === 'gemini-3.1-flash-lite'
    ) {
        return false;
    }

    // 1. 严格检查上游接口返回的官方废弃表 (deprecatedModelIds)
    // 注意：lite 独立轻量模型若上游仍在可用列表中返回，绝不可误杀
    if (deprecatedSet && deprecatedSet.has(m) && !m.includes('lite')) {
        return false;
    }

    // 2. 特许放行的特定官方核心与专用模型 (Lite 专用系列 / 绘图系列)
    if (
        m === 'gemini-3.5-flash-lite' ||
        m === 'gemini-flash-lite' ||
        m === 'gemini-3.1-flash-image' ||
        m === 'gemini-3-pro-image'
    ) {
        return true;
    }

    // 3. 淘汰老旧 2.5、3.5(非lite)、3.6、3.7 及以下系列
    if (
        m.includes('2.5') ||
        m.includes('2-5') ||
        m.includes('gemini-1.') ||
        m.includes('gemini-2.') ||
        m.includes('3.5-flash') ||
        m.includes('3.6-flash') ||
        m.includes('3.7-flash')
    ) {
        return false;
    }

    // 4. Claude 系列：以 4.6 为基准线（支持 4.6 与 5.5+，支持 Haiku >= 4.5 与 Fable >= 5.0），淘汰非法 thinking 组合
    if (m.includes('claude')) {
        if ((m.startsWith('claude-sonnet-') || m.includes('-5-5')) && m.includes('thinking')) return false;

        const tokens = m.split(/[-_.]/);
        let detectedMajor: number | null = null;
        let detectedMinor = 0;
        let i = 0;
        while (i < tokens.length) {
            const tok = tokens[i];
            if (tok.length === 8 && (tok.startsWith('202') || tok.startsWith('201'))) {
                i++;
                continue;
            }
            const major = parseInt(tok, 10);
            if (!isNaN(major) && major < 1000) {
                detectedMajor = major;
                if (i + 1 < tokens.length) {
                    const nextTok = tokens[i + 1];
                    if (!(nextTok.length === 8 && (nextTok.startsWith('202') || nextTok.startsWith('201')))) {
                        const minor = parseInt(nextTok, 10);
                        if (!isNaN(minor) && minor < 1000) {
                            detectedMinor = minor;
                            i++;
                        }
                    }
                }
                break;
            }
            i++;
        }

        if (detectedMajor !== null) {
            if (m.includes('fable') || m.includes('mythos')) {
                return detectedMajor >= 5;
            }
            return detectedMajor > 4 || (detectedMajor === 4 && detectedMinor >= 6);
        }
        return false;
    }

    // 5. 针对 Gemini 3 Flash 系列思考/推理模型剪枝：
    if (m.includes('lite') || m.includes('image')) {
        return true;
    }

    if (m.startsWith('gemini-3.') && m.includes('flash') && activeFlashMinorFloor !== undefined) {
        const match = m.match(/gemini-3\.(\d+)/);
        if (match) {
            const minor = parseInt(match[1], 10);
            if (minor < activeFlashMinorFloor) {
                return false;
            }
        }
    }

    return true;
}

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

        // 0. 从所有已登录账号中自动汇聚官方接口下发的废弃模型映射表 (deprecatedModelIds / model_forwarding_rules)
        const deprecatedSet = new Set<string>();
        for (const account of accounts) {
            if (account.quota?.model_forwarding_rules) {
                for (const oldId of Object.keys(account.quota.model_forwarding_rules)) {
                    deprecatedSet.add(oldId.toLowerCase().trim());
                }
            }
        }

        // 0.1 探测当前账号池中 Gemini 3 Flash 家族最高活跃小版本 (如 3.8)，以便自动收敛淘汰已被替代的过渡小版本
        let maxGemini3FlashMinor = 8; // 默认以当前 3.8 为基准
        for (const account of accounts) {
            for (const m of account.quota?.models ?? []) {
                const nameLower = m.name.toLowerCase().trim();
                const match = nameLower.match(/gemini-3\.(\d+)-flash/);
                if (match) {
                    const minor = parseInt(match[1], 10);
                    if (minor > maxGemini3FlashMinor) {
                        maxGemini3FlashMinor = minor;
                    }
                }
            }
        }

        // 1. 以真实账号上报的动态模型为第一权威真理源 (Single Source of Truth)
        for (const account of accounts) {
            for (const m of account.quota?.models ?? []) {
                const rawKey = m.name.toLowerCase().trim();
                if (!isCompliantModel(rawKey, deprecatedSet, maxGemini3FlashMinor)) {
                    continue;
                }

                // Map sub-tier and legacy aliases to the primary canonical model
                const mapped = ALIAS_TO_CANONICAL[rawKey];
                const canonicalId = mapped ? mapped.id : (m.name || rawKey);
                const canonicalName = canonicalId;

                // 泛化推导分组，全面兼容未知/未来新模型 (如 Gemini 4 Argon 等)
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

        // 1.5 动态档位后缀剥离与裸模型派生（与后端算法完全对齐，纯通用数据驱动）
        const derivedBares: string[] = [];
        for (const id of uniqueModelsMap.keys()) {
            for (const suffix of ['-tiered', '-high', '-medium', '-low', '-extra-low', '-thinking']) {
                if (id.endsWith(suffix)) {
                    const base = id.slice(0, -suffix.length);
                    if (base && isCompliantModel(base, deprecatedSet, maxGemini3FlashMinor) && !uniqueModelsMap.has(base)) {
                        derivedBares.push(base);
                    }
                }
            }
        }
        for (const base of derivedBares) {
            if (!uniqueModelsMap.has(base)) {
                const group = inferModelGroup(base);
                const cfgEntry = Object.entries(MODEL_CONFIG).find(
                    ([cfgId, cfg]) => cfgId.toLowerCase() === base.toLowerCase() || cfg.protectedKey?.toLowerCase() === base.toLowerCase()
                );
                const CfgIcon = cfgEntry?.[1].Icon;
                const icon = CfgIcon ? <CfgIcon size={16} /> : (group === 'Claude' ? <Sparkles size={16} className="text-purple-400" /> : <Bot size={16} className="text-blue-400" />);
                uniqueModelsMap.set(base, {
                    id: base,
                    name: base,
                    group,
                    icon,
                });
            }
        }

        // 2. 仅在冷启动（用户完全未添加任何账号）时，才回退补全 MODEL_CONFIG 中的基础核心推荐模型做初始展示；
        // 当已有账号接入时，严格以真实接口返回模型为准，绝不凭空注入任何未在账号中出现的模型！
        if (accounts.length === 0) {
            for (const [id, config] of Object.entries(MODEL_CONFIG)) {
                const rawKey = id.toLowerCase();
                if (!isCompliantModel(rawKey, deprecatedSet, maxGemini3FlashMinor)) {
                    continue;
                }
                const mapped = ALIAS_TO_CANONICAL[rawKey];
                const canonicalId = mapped ? mapped.id : id;
                const canonicalName = canonicalId;
                const group = mapped ? mapped.group : (config.group || inferModelGroup(canonicalId));

                if (!uniqueModelsMap.has(canonicalId)) {
                    uniqueModelsMap.set(canonicalId, {
                        id: canonicalId,
                        name: canonicalName,
                        group,
                        icon: <config.Icon size={16} />,
                    });
                }
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
