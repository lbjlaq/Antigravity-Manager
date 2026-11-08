import { create } from 'zustand';
import { Account, getAccountTier, ModelPriorities } from '../types/account';
import { PedalCandidate, PedalModeState, PedalTargetModel } from '../types/pedal';
import { getGroupQuotaSummaries } from '../utils/quotaDisplay';
import { showToast } from '../components/common/ToastContainer';
import { useAccountStore } from './useAccountStore';
import { request } from '../utils/request';

const STORAGE_KEY = 'antigravity_pedal_mode_state';
const SUPPRESS_KEY = 'antigravity_pedal_mode_suppress';
let dismissedUntil = 0;
let isProcessing = false;

interface PedalPendingModal {
    model: PedalTargetModel;
    modelLabel: string;
    candidates: PedalCandidate[];
}

interface PedalModeStore extends PedalModeState {
    pendingModal: PedalPendingModal | null;

    // Actions
    checkCandidates: (accounts: Account[]) => void;
    activatePedalMode: (
        model: PedalTargetModel,
        targetIds: string[],
        accounts: Account[],
        updatePriority?: (accountId: string, priority: number, modelPriorities?: ModelPriorities) => Promise<void>
    ) => Promise<void>;
    checkAndAutoRecover: (
        accounts: Account[],
        updatePriority?: (accountId: string, priority: number, modelPriorities?: ModelPriorities) => Promise<void>,
        legacyToggle?: (accountId: string, enable: boolean, reason?: string) => Promise<void>
    ) => Promise<void>;
    manualExit: (
        updatePriority?: (accountId: string, priority: number, modelPriorities?: ModelPriorities) => Promise<void>,
        legacyToggle?: (accountId: string, enable: boolean, reason?: string) => Promise<void>
    ) => Promise<void>;
    dismissModal: () => void;
    suppressModel: (model: PedalTargetModel) => void;
    clearSuppress: () => void;
    resetPedalModeState: () => void;
}

const getStoredSuppress = () => {
    try {
        const raw = localStorage.getItem(SUPPRESS_KEY);
        if (raw) return JSON.parse(raw);
    } catch (_) {}
    return { gemini: false, claude: false };
};

const getStoredState = (): Partial<PedalModeState> => {
    try {
        const raw = localStorage.getItem(STORAGE_KEY);
        if (raw) return JSON.parse(raw);
    } catch (_) {}
    return {};
};

async function safeClearSessionBindings() {
    if (typeof window === 'undefined') return;
    try {
        await request('clear_proxy_session_bindings');
    } catch (_) {}
}

function extractCandidate(account: Account, model: PedalTargetModel): PedalCandidate | null {
    // 排除已被用户主动禁用或处于锁定状态的账号，避免坏号被选为唯一可用账号导致全池瘫痪
    if (account.proxy_disabled) {
        return null;
    }

    const tier = getAccountTier(account);
    const summaries = getGroupQuotaSummaries(account.quota?.quota_groups, account.quota?.models, tier);
    const summary = summaries.find(s => s.key === model);
    if (!summary || !summary.weekly || summary.weekly.percentage === undefined || !summary.weekly.resetTime) {
        return null;
    }

    const pct = summary.weekly.percentage;
    const resetTimeStr = summary.weekly.resetTime;
    const resetTimestamp = Date.parse(resetTimeStr);
    if (isNaN(resetTimestamp)) return null;

    const diffHours = (resetTimestamp - Date.now()) / (3600 * 1000);

    // 触发条件：周配额 > 5% 且距离重置小于 24 小时
    if (pct > 5 && diffHours > 0 && diffHours <= 24) {
        const rawBadge = summary.dynamicLabel || summary.defaultLabel;
        const cleanBadge = rawBadge.replace(/\s*\/\s*GPT$/i, '');
        return {
            accountId: account.id,
            accountName: account.custom_label || account.name || account.email,
            email: account.email,
            model,
            modelBadge: cleanBadge,
            supportedModels: summary.supportedModels,
            percentage: pct,
            resetTime: resetTimeStr,
            hoursLeft: diffHours,
        };
    }
    return null;
}

export const usePedalModeStore = create<PedalModeStore>((set, get) => {
    const initialSuppress = getStoredSuppress();
    const initialState = getStoredState();

    return {
        active: initialState.active || false,
        targetModel: initialState.targetModel || null,
        targetAccountIds: initialState.targetAccountIds || [],
        originalPriorities: initialState.originalPriorities || {},
        originalModelPriorities: initialState.originalModelPriorities || {},
        snapshotDisabledIds: initialState.snapshotDisabledIds || [],
        temporarilyDisabledIds: initialState.temporarilyDisabledIds || [],
        suppressNotice: initialSuppress,
        pendingModal: null,

        checkCandidates: (accounts: Account[]) => {
            const state = get();
            if (state.active || state.pendingModal) {
                // 如果已激活或弹窗已在展示中，不重复触发检查
                return;
            }

            // 用户若在过去 2 小时内点击了“暂不启动”，不重复打扰
            if (Date.now() < dismissedUntil) {
                return;
            }

            // 1. 优先检测 Gemini
            if (!state.suppressNotice.gemini) {
                const geminiCandidates: PedalCandidate[] = [];
                for (const acc of accounts) {
                    const c = extractCandidate(acc, 'gemini');
                    if (c) geminiCandidates.push(c);
                }
                if (geminiCandidates.length > 0) {
                    set({
                        pendingModal: {
                            model: 'gemini',
                            modelLabel: 'Gemini',
                            candidates: geminiCandidates,
                        },
                    });
                    return;
                }
            }

            // 2. 检测 Claude
            if (!state.suppressNotice.claude) {
                const claudeCandidates: PedalCandidate[] = [];
                for (const acc of accounts) {
                    const c = extractCandidate(acc, 'claude');
                    if (c) claudeCandidates.push(c);
                }
                if (claudeCandidates.length > 0) {
                    set({
                        pendingModal: {
                            model: 'claude',
                            modelLabel: 'Claude',
                            candidates: claudeCandidates,
                        },
                    });
                }
            }
        },

        activatePedalMode: async (model, targetIds, accounts, updatePriority) => {
            if (!targetIds || targetIds.length === 0) {
                return;
            }
            const targetSet = new Set(targetIds);
            const originalPriorities: Record<string, number> = {};
            const originalModelPriorities: Record<string, ModelPriorities> = {};
            const otherModel: PedalTargetModel = model === 'gemini' ? 'claude' : 'gemini';

            for (const acc of accounts) {
                if (targetSet.has(acc.id)) {
                    originalPriorities[acc.id] = typeof acc.priority === 'number' ? acc.priority : 50;
                    originalModelPriorities[acc.id] = acc.model_priorities ? { ...acc.model_priorities } : {};
                }
            }

            const priorityUpdater = updatePriority || (async (id: string, p: number, mp?: ModelPriorities) => {
                const { updateAccountPriority } = useAccountStore.getState();
                if (updateAccountPriority) await updateAccountPriority(id, p, mp);
            });

            // 批量将目标账号对应模型族设置为最高调度优先级（priority = 1），保留另一模型族原优先级，绝不挂起其他正常账号
            for (const id of targetIds) {
                try {
                    const origP = originalPriorities[id] ?? 50;
                    const origMp = originalModelPriorities[id] || {};
                    const boostedMp: ModelPriorities = {
                        ...origMp,
                        [model]: 1,
                        [otherModel]: origMp[otherModel] ?? origP,
                    };
                    await priorityUpdater(id, 1, boostedMp);
                } catch (e) {
                    console.error('[PedalMode] Failed to boost account priority:', id, e);
                }
            }

            // 自动清除既有会话粘性映射，使正在进行的旧对话在下一次发消息时立即切换至待蹬账号
            await safeClearSessionBindings();

            const newState: PedalModeState = {
                active: true,
                targetModel: model,
                targetAccountIds: targetIds,
                originalPriorities,
                originalModelPriorities,
                snapshotDisabledIds: [],
                temporarilyDisabledIds: [],
                suppressNotice: get().suppressNotice,
            };

            try {
                localStorage.setItem(STORAGE_KEY, JSON.stringify(newState));
            } catch (_) {}

            set({
                ...newState,
                pendingModal: null,
            });

            showToast(`🚴 已启动“蹬额度模式”(${model.toUpperCase()})，已将 ${targetIds.length} 个临期账号设为最高调度优先级`);
        },

        checkAndAutoRecover: async (accounts, updatePriority, legacyToggle) => {
            if (isProcessing) return;
            const state = get();
            if (!state.active || !state.targetModel || state.targetAccountIds.length === 0) {
                return;
            }

            isProcessing = true;
            try {
                const priorityUpdater = updatePriority || (async (id: string, p: number, mp?: ModelPriorities) => {
                    const { updateAccountPriority } = useAccountStore.getState();
                    if (updateAccountPriority) await updateAccountPriority(id, p, mp);
                });

                const targetAccounts = accounts.filter(a => state.targetAccountIds.includes(a.id));
                if (targetAccounts.length === 0) {
                    const clearedState = {
                        active: false,
                        targetModel: null,
                        targetAccountIds: [],
                        originalPriorities: {},
                        originalModelPriorities: {},
                        snapshotDisabledIds: [],
                        temporarilyDisabledIds: [],
                    };
                    try {
                        localStorage.removeItem(STORAGE_KEY);
                    } catch (_) {}
                    // 立即重置 Store 状态，防止中间任何异步刷新将自身误判为激活态而重入
                    set(clearedState);

                    // 异常保底：若所有目标账号已被用户删除或移除，立即退出并恢复默认优先级
                    for (const id of state.targetAccountIds) {
                        const origP = state.originalPriorities?.[id] ?? 50;
                        const origMp = state.originalModelPriorities?.[id] ?? {};
                        try {
                            await priorityUpdater(id, origP, origMp);
                        } catch (e) {
                            console.error('[PedalMode] Failed to restore account priority on empty targets:', id, e);
                        }
                    }
                    // 容错处理旧版本残留的挂起账号（仅当账号客观处于禁用时才调用，消除重复写盘）
                    if (legacyToggle && state.temporarilyDisabledIds?.length > 0) {
                        for (const id of state.temporarilyDisabledIds) {
                            const acc = accounts.find(a => a.id === id);
                            if (!acc || acc.proxy_disabled) {
                                try { await legacyToggle(id, true); } catch (_) {}
                            }
                        }
                    }
                    await safeClearSessionBindings();
                    showToast('目标待蹬账号已不存在，已自动退出蹬额度模式并恢复调度优先级');
                    return;
                }

                // 检查待蹬账号：单个账号用尽时恢复其调度优先级；当所有目标账号用尽或到达重置时间后整体退出
                let allDrained = true;
                let allReset = true;

                for (const acc of targetAccounts) {
                    const tier = getAccountTier(acc);
                    const summaries = getGroupQuotaSummaries(acc.quota?.quota_groups, acc.quota?.models, tier);
                    const s = summaries.find(sm => sm.key === state.targetModel);

                    // 配额数据未就绪或临时加载失败时，进行保守保护，防止误判为 0% 提前退出
                    if (!s || !s.weekly || s.weekly.percentage === undefined) {
                        allDrained = false;
                        allReset = false;
                        continue;
                    }

                    const pct = s.weekly.percentage;
                    const resetTimeStr = s.weekly.resetTime;
                    const resetTimestamp = resetTimeStr ? Date.parse(resetTimeStr) : NaN;
                    const hasValidResetTime = !isNaN(resetTimestamp);
                    const diffHours = hasValidResetTime ? (resetTimestamp - Date.now()) / (3600 * 1000) : 999;

                    // 若该账号配额已降至 0%，且该账号仍处于优先级 1 状态，则将其动态恢复至原始优先级
                    // 避免单号用尽后仍作为最高优先级截留非待蹬模型的请求
                    if (pct <= 0) {
                        const effectiveModelP = acc.model_priorities?.[state.targetModel];
                        if (acc.priority === 1 || acc.priority === 0 || effectiveModelP === 1) {
                            const origP = state.originalPriorities?.[acc.id] ?? 50;
                            const origMp = state.originalModelPriorities?.[acc.id] ?? {};
                            try {
                                await priorityUpdater(acc.id, origP, origMp);
                            } catch (e) {
                                console.error('[PedalMode] Failed to restore single drained account priority:', acc.id, e);
                            }
                        }
                    } else {
                        allDrained = false;
                    }

                    const isThisAccountReset = pct >= 90 || (hasValidResetTime && diffHours <= 0);
                    if (!isThisAccountReset) {
                        allReset = false;
                    }
                }

                if (allDrained || allReset) {
                    const reason = allDrained ? '周配额已全部用完' : '已到达官方重置时间';
                    const clearedState = {
                        active: false,
                        targetModel: null,
                        targetAccountIds: [],
                        originalPriorities: {},
                        originalModelPriorities: {},
                        snapshotDisabledIds: [],
                        temporarilyDisabledIds: [],
                    };

                    try {
                        localStorage.removeItem(STORAGE_KEY);
                    } catch (_) {}

                    // 立即更新 Store 状态，防止中间任何异步事件并发重入
                    set(clearedState);

                    // 恢复目标账号的原始调度优先级
                    for (const id of state.targetAccountIds) {
                        const origP = state.originalPriorities?.[id] ?? 50;
                        const origMp = state.originalModelPriorities?.[id] ?? {};
                        try {
                            await priorityUpdater(id, origP, origMp);
                        } catch (e) {
                            console.error('[PedalMode] Failed to restore priority:', id, e);
                        }
                    }

                    // 容错处理旧版本残留的挂起账号（仅当账号客观处于禁用时才调用）
                    if (legacyToggle && state.temporarilyDisabledIds?.length > 0) {
                        for (const id of state.temporarilyDisabledIds) {
                            const acc = accounts.find(a => a.id === id);
                            if (!acc || acc.proxy_disabled) {
                                try { await legacyToggle(id, true); } catch (_) {}
                            }
                        }
                    }

                    await safeClearSessionBindings();
                    showToast(`🎉 待蹬账号${reason}，已自动退出蹬额度模式并恢复调度优先级！`);
                }
            } finally {
                isProcessing = false;
            }
        },

        manualExit: async (updatePriority, legacyToggle) => {
            if (isProcessing) return;
            const state = get();
            if (!state.active) return;

            isProcessing = true;
            try {
                const clearedState = {
                    active: false,
                    targetModel: null,
                    targetAccountIds: [],
                    originalPriorities: {},
                    originalModelPriorities: {},
                    snapshotDisabledIds: [],
                    temporarilyDisabledIds: [],
                };

                try {
                    localStorage.removeItem(STORAGE_KEY);
                } catch (_) {}

                // 立即退出，避免退出期间任何外部刷新事件引发竞态
                set(clearedState);

                const currentAccounts = useAccountStore.getState().accounts || [];
                const priorityUpdater = updatePriority || (async (id: string, p: number, mp?: ModelPriorities) => {
                    const { updateAccountPriority } = useAccountStore.getState();
                    if (updateAccountPriority) await updateAccountPriority(id, p, mp);
                });

                for (const id of state.targetAccountIds) {
                    const origP = state.originalPriorities?.[id] ?? 50;
                    const origMp = state.originalModelPriorities?.[id] ?? {};
                    try {
                        await priorityUpdater(id, origP, origMp);
                    } catch (e) {
                        console.error('[PedalMode] Failed to restore account priority manually:', id, e);
                    }
                }

                // 容错处理旧版本残留的挂起账号（仅对客观处于禁用的账号恢复）
                if (legacyToggle && state.temporarilyDisabledIds?.length > 0) {
                    for (const id of state.temporarilyDisabledIds) {
                        const acc = currentAccounts.find(a => a.id === id);
                        if (!acc || acc.proxy_disabled) {
                            try { await legacyToggle(id, true); } catch (_) {}
                        }
                    }
                }

                await safeClearSessionBindings();
                showToast('已退出“蹬额度模式”，账号已恢复原状');
            } finally {
                isProcessing = false;
            }
        },

        resetPedalModeState: () => {
            isProcessing = false;
            try {
                localStorage.removeItem(STORAGE_KEY);
            } catch (_) {}
            set({
                active: false,
                targetModel: null,
                targetAccountIds: [],
                originalPriorities: {},
                originalModelPriorities: {},
                snapshotDisabledIds: [],
                temporarilyDisabledIds: [],
                pendingModal: null,
            });
            showToast('已重置蹬额度模式状态');
        },

        dismissModal: () => {
            // 暂不启动时设置 2 小时静默窗口，避免频繁打扰
            dismissedUntil = Date.now() + 2 * 3600 * 1000;
            set({ pendingModal: null });
        },

        suppressModel: (model: PedalTargetModel) => {
            const current = get().suppressNotice;
            const updated = { ...current, [model]: true };
            try {
                localStorage.setItem(SUPPRESS_KEY, JSON.stringify(updated));
            } catch (_) {}
            set({
                suppressNotice: updated,
                pendingModal: null,
            });
            showToast(`已设置为不再提示 ${model.toUpperCase()} 模型的蹬额度提醒`);
        },

        clearSuppress: () => {
            const reset = { gemini: false, claude: false };
            try {
                localStorage.removeItem(SUPPRESS_KEY);
            } catch (_) {}
            set({ suppressNotice: reset });
            showToast('已重置“不再提示”偏好');
        },
    };
});
