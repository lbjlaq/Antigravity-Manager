import { create } from 'zustand';
import { Account, getAccountTier } from '../types/account';
import { PedalCandidate, PedalModeState, PedalTargetModel } from '../types/pedal';
import { getGroupQuotaSummaries } from '../utils/quotaDisplay';
import { showToast } from '../components/common/ToastContainer';

const STORAGE_KEY = 'antigravity_pedal_mode_state';
const SUPPRESS_KEY = 'antigravity_pedal_mode_suppress';
let dismissedUntil = 0;

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
        toggleProxy: (accountId: string, enable: boolean, reason?: string) => Promise<void>
    ) => Promise<void>;
    checkAndAutoRecover: (
        accounts: Account[],
        toggleProxy: (accountId: string, enable: boolean, reason?: string) => Promise<void>
    ) => Promise<void>;
    manualExit: (
        toggleProxy: (accountId: string, enable: boolean, reason?: string) => Promise<void>
    ) => Promise<void>;
    dismissModal: () => void;
    suppressModel: (model: PedalTargetModel) => void;
    clearSuppress: () => void;
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
        return {
            accountId: account.id,
            accountName: account.name || account.email,
            email: account.email,
            model,
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
        snapshotDisabledIds: initialState.snapshotDisabledIds || [],
        temporarilyDisabledIds: initialState.temporarilyDisabledIds || [],
        suppressNotice: initialSuppress,
        pendingModal: null,

        checkCandidates: (accounts: Account[]) => {
            const state = get();
            if (state.active) {
                // 如果已激活，退出检查交给外部或定期轮询 checkAndAutoRecover
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

        activatePedalMode: async (model, targetIds, accounts, toggleProxy) => {
            const targetSet = new Set(targetIds);
            const originallyDisabled: string[] = [];
            const willDisable: string[] = [];

            for (const acc of accounts) {
                if (!targetSet.has(acc.id)) {
                    if (acc.proxy_disabled) {
                        originallyDisabled.push(acc.id);
                    } else {
                        willDisable.push(acc.id);
                    }
                }
            }

            // 批量将非目标账号临时禁用
            for (const id of willDisable) {
                try {
                    await toggleProxy(id, false, 'pedal_mode_locked');
                } catch (e) {
                    console.error('[PedalMode] Failed to disable account:', id, e);
                }
            }

            const newState: PedalModeState = {
                active: true,
                targetModel: model,
                targetAccountIds: targetIds,
                snapshotDisabledIds: originallyDisabled,
                temporarilyDisabledIds: willDisable,
                suppressNotice: get().suppressNotice,
            };

            try {
                localStorage.setItem(STORAGE_KEY, JSON.stringify(newState));
            } catch (_) {}

            set({
                ...newState,
                pendingModal: null,
            });

            showToast(`🚴 已启动“蹬额度模式”(${model.toUpperCase()})，临时挂起 ${willDisable.length} 个账号`);
        },

        checkAndAutoRecover: async (accounts, toggleProxy) => {
            const state = get();
            if (!state.active || !state.targetModel || state.targetAccountIds.length === 0) {
                return;
            }

            const targetAccounts = accounts.filter(a => state.targetAccountIds.includes(a.id));
            if (targetAccounts.length === 0) {
                // 异常保底：若所有目标账号已被用户删除或移除，立即退出并恢复挂起的正常账号，防死锁
                for (const id of state.temporarilyDisabledIds) {
                    try {
                        await toggleProxy(id, true);
                    } catch (e) {
                        console.error('[PedalMode] Failed to restore account on empty targets:', id, e);
                    }
                }
                const clearedState = {
                    active: false,
                    targetModel: null,
                    targetAccountIds: [],
                    snapshotDisabledIds: [],
                    temporarilyDisabledIds: [],
                };
                try {
                    localStorage.removeItem(STORAGE_KEY);
                } catch (_) {}
                set(clearedState);
                showToast('目标待蹬账号已不存在，已自动退出蹬额度模式并恢复所有账号');
                return;
            }

            // 检查所有待蹬账号是否全部用完 (0%) 或已到达重置时间 (已刷新)
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
                const diffHours = !isNaN(resetTimestamp) ? (resetTimestamp - Date.now()) / (3600 * 1000) : 999;

                // 若任何一个账号配额仍 > 0，则未全部用完
                if (pct > 0) {
                    allDrained = false;
                }

                // 临期等待判定：仅当处于倒计时窗口内 (0 < diffHours <= 24) 且配额尚未恢复 (< 90%) 时，
                // 才说明该账号既未到达重置时间、也未刷新新周期。
                // 若 diffHours <= 0 (到达官方重置时间) 或 pct >= 90 (配额已满血) 或 diffHours > 24 (时间戳已进入下周)，
                // 均表明该账号已到达重置时间或已成功重置。
                if (diffHours > 0 && diffHours <= 24 && pct < 90) {
                    allReset = false;
                }
            }

            if (allDrained || allReset) {
                const reason = allDrained ? '周配额已全部用完' : '已到达官方重置时间';
                // 恢复此前临时禁用的账号
                for (const id of state.temporarilyDisabledIds) {
                    try {
                        await toggleProxy(id, true);
                    } catch (e) {
                        console.error('[PedalMode] Failed to restore account:', id, e);
                    }
                }

                const clearedState = {
                    active: false,
                    targetModel: null,
                    targetAccountIds: [],
                    snapshotDisabledIds: [],
                    temporarilyDisabledIds: [],
                };

                try {
                    localStorage.removeItem(STORAGE_KEY);
                } catch (_) {}

                set(clearedState);
                showToast(`🎉 待蹬账号${reason}，已自动退出蹬额度模式并恢复所有账号！`);
            }
        },

        manualExit: async toggleProxy => {
            const state = get();
            if (!state.active) return;

            for (const id of state.temporarilyDisabledIds) {
                try {
                    await toggleProxy(id, true);
                } catch (e) {
                    console.error('[PedalMode] Failed to restore account manually:', id, e);
                }
            }

            const clearedState = {
                active: false,
                targetModel: null,
                targetAccountIds: [],
                snapshotDisabledIds: [],
                temporarilyDisabledIds: [],
            };

            try {
                localStorage.removeItem(STORAGE_KEY);
            } catch (_) {}

            set(clearedState);
            showToast('已退出“蹬额度模式”，账号已恢复原状');
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
