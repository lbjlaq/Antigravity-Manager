import { useState, useMemo } from 'react';
import {
    CheckCircle,
    Mail,
    Diamond,
    Gem,
    Circle,
    Tag,
    Laptop,
    Code2,
    Terminal,
    Blocks,
    ArrowRight,
    HelpCircle
} from 'lucide-react';
import { Account, getAccountTier, ActiveClientsStatus } from '../../types/account';
import { useTranslation } from 'react-i18next';
import { getGroupQuotaSummaries, GroupQuotaSummary } from '../../utils/quotaDisplay';
import { GroupQuotaCard } from '../accounts/GroupQuotaCard';
import { getLiveLimitForModel } from '../../utils/liveLimit';

interface CurrentAccountProps {
    account?: Account | null;
    accounts?: Account[];
    activeClients?: ActiveClientsStatus;
    quotaView?: string;
    onSwitch?: (targetIde?: string) => void;
}

type ClientTab = 'classic' | 'ide' | 'agy' | 'acp';

function CurrentAccount({
    account,
    accounts = [],
    activeClients,
    onSwitch
}: CurrentAccountProps) {
    const { t } = useTranslation();

    // 默认定位到有活跃账号的 Tab，优先经典款或 IDE
    const [activeTab, setActiveTab] = useState<ClientTab>(() => {
        if (activeClients?.classic_account_id) return 'classic';
        if (activeClients?.ide_account_id) return 'ide';
        if (activeClients?.agy_account_id) return 'agy';
        if (activeClients?.acp_account_id) return 'acp';
        return 'classic';
    });

    const getClientAccountId = (tab: ClientTab) => {
        switch (tab) {
            case 'classic':
                return activeClients?.classic_account_id;
            case 'ide':
                return activeClients?.ide_account_id;
            case 'agy':
                return activeClients?.agy_account_id;
            case 'acp':
                return activeClients?.acp_account_id;
        }
    };

    const clientTabs = [
        {
            key: 'classic' as ClientTab,
            label: t('accounts.client_classic', '经典款'),
            icon: Laptop,
            tooltip: t('accounts.client_classic_tooltip', '当前 Antigravity 经典款客户端登录账号'),
            activeBg: 'bg-blue-600 text-white shadow-xs',
            activeDot: 'bg-blue-400',
        },
        {
            key: 'ide' as ClientTab,
            label: t('accounts.client_ide', 'IDE'),
            icon: Code2,
            tooltip: t('accounts.client_ide_tooltip', '当前 Antigravity IDE 客户端登录账号'),
            activeBg: 'bg-sky-600 text-white shadow-xs',
            activeDot: 'bg-sky-400',
        },
        {
            key: 'agy' as ClientTab,
            label: t('accounts.client_cli', 'CLI'),
            icon: Terminal,
            tooltip: t('accounts.client_cli_tooltip', '当前 Antigravity CLI (agy) 登录账号'),
            activeBg: 'bg-emerald-600 text-white shadow-xs',
            activeDot: 'bg-emerald-400',
        },
        {
            key: 'acp' as ClientTab,
            label: t('accounts.client_acp', 'Plugin'),
            icon: Blocks,
            tooltip: t('accounts.client_acp_tooltip', 'Active account in Antigravity IDE Plugin (ACP)'),
            activeBg: 'bg-violet-600 text-white shadow-xs',
            activeDot: 'bg-violet-400',
        },
    ];

    const currentClientAccountId = getClientAccountId(activeTab);

    // 根据当前选中的 Tab 检索账号实体
    const displayedAccount = useMemo(() => {
        if (currentClientAccountId && accounts.length > 0) {
            const found = accounts.find((a) => a.id === currentClientAccountId);
            if (found) return found;
        }
        // 回退机制：若为经典款且未单独匹配到 ID，则使用原 account 作为容底
        if (activeTab === 'classic' && account) {
            return account;
        }
        return null;
    }, [currentClientAccountId, accounts, activeTab, account]);

    const tier = displayedAccount ? getAccountTier(displayedAccount) : 'free';
    const groupSummaries: GroupQuotaSummary[] = displayedAccount
        ? getGroupQuotaSummaries(
              displayedAccount.quota?.quota_groups,
              displayedAccount.quota?.models,
              tier
          )
        : [];

    const isGroupProtected = (key: 'gemini' | 'claude' | 'argon') => {
        if (!displayedAccount?.protected_models || displayedAccount.protected_models.length === 0)
            return false;
        if (key === 'argon') {
            return displayedAccount.protected_models.some((m) =>
                m.toLowerCase().includes('argon')
            );
        }
        if (key === 'gemini') {
            return displayedAccount.protected_models.some(
                (m) => m.toLowerCase().includes('gemini') && !m.toLowerCase().includes('argon')
            );
        }
        return displayedAccount.protected_models.some(
            (m) => m.toLowerCase().includes('claude') || m.toLowerCase().includes('gpt')
        );
    };

    return (
        <div className="bg-white dark:bg-[#15161b] rounded-2xl p-5 shadow-sm dark:shadow-xl dark:shadow-black/40 border border-gray-100 dark:border-white/[0.10] h-full flex flex-col hover:shadow-md transition-shadow">
            {/* 标题栏与客户端分段 Tab */}
            <div className="flex items-center justify-between mb-3 gap-2 flex-wrap">
                <h2 className="text-base font-semibold text-gray-900 dark:text-base-content flex items-center gap-2">
                    <CheckCircle className="w-4 h-4 text-green-500" />
                    {t('dashboard.current_account', '当前账号')}
                </h2>

                {/* 客户端 Tab 药丸切换器 */}
                <div className="flex items-center bg-gray-100 dark:bg-base-200/90 p-1 rounded-xl border border-gray-200/70 dark:border-white/10 gap-1">
                    {clientTabs.map((tab) => {
                        const Icon = tab.icon;
                        const isActive = activeTab === tab.key;
                        const hasAccount = !!getClientAccountId(tab.key);
                        return (
                            <button
                                key={tab.key}
                                onClick={() => setActiveTab(tab.key)}
                                title={tab.tooltip}
                                className={`flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-xs font-semibold transition-all cursor-pointer ${
                                    isActive
                                        ? tab.activeBg
                                        : 'text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white hover:bg-gray-200/60 dark:hover:bg-white/5'
                                }`}
                            >
                                <Icon className="w-3.5 h-3.5" />
                                <span>{tab.label}</span>
                                {hasAccount && (
                                    <span
                                        className={`w-1.5 h-1.5 rounded-full ${
                                            isActive ? 'bg-white' : 'bg-emerald-500'
                                        }`}
                                    />
                                )}
                            </button>
                        );
                    })}
                </div>
            </div>

            {/* 账号详情或空状态 */}
            {!displayedAccount ? (
                <div className="flex-1 flex flex-col items-center justify-center py-6 text-center">
                    <div className="w-12 h-12 rounded-2xl bg-gray-100 dark:bg-base-200 flex items-center justify-center text-gray-400 mb-2.5">
                        <HelpCircle className="w-6 h-6 text-gray-400/80" />
                    </div>
                    <div className="text-sm font-semibold text-gray-700 dark:text-gray-300">
                        {t('dashboard.no_client_account', '该应用当前暂无登录或绑定的账号')}
                    </div>
                    <div className="text-xs text-gray-400 dark:text-gray-500 mt-1 max-w-[260px]">
                        {clientTabs.find((t) => t.key === activeTab)?.tooltip}
                    </div>
                    {onSwitch && (
                        <button
                            onClick={() => onSwitch(activeTab)}
                            className="mt-4 px-4 py-1.5 text-xs font-semibold text-blue-600 dark:text-blue-400 bg-blue-50 dark:bg-blue-900/20 hover:bg-blue-100 dark:hover:bg-blue-900/30 rounded-lg transition-colors flex items-center gap-1 cursor-pointer"
                        >
                            <span>{t('dashboard.go_switch_account', '前往切换账号')}</span>
                            <ArrowRight className="w-3.5 h-3.5" />
                        </button>
                    )}
                </div>
            ) : (
                <div className="space-y-4 flex-1 flex flex-col justify-between">
                    <div className="flex items-center gap-3 mb-1">
                        <div className="flex items-center gap-2 flex-1 min-w-0">
                            <Mail className="w-3.5 h-3.5 text-gray-400" />
                            <span
                                className="text-sm font-medium text-gray-700 dark:text-gray-300 truncate"
                                title={displayedAccount.email}
                            >
                                {displayedAccount.email}
                            </span>
                        </div>

                        {/* 订阅类型 */}
                        {(() => {
                            if (tier === 'ultra') {
                                return (
                                    <span className="flex items-center gap-1 px-2 py-0.5 rounded-md bg-gradient-to-r from-purple-600 to-pink-600 text-white text-[10px] font-bold shadow-sm shrink-0">
                                        <Gem className="w-2.5 h-2.5 fill-current" />
                                        ULTRA
                                    </span>
                                );
                            } else if (tier === 'pro') {
                                return (
                                    <span className="flex items-center gap-1 px-2 py-0.5 rounded-md bg-gradient-to-r from-blue-600 to-indigo-600 text-white text-[10px] font-bold shadow-sm shrink-0">
                                        <Diamond className="w-2.5 h-2.5 fill-current" />
                                        PRO
                                    </span>
                                );
                            } else {
                                return (
                                    <span className="flex items-center gap-1 px-2 py-0.5 rounded-md bg-gray-100 dark:bg-white/10 text-gray-500 dark:text-gray-400 text-[10px] font-bold shadow-sm border border-gray-200 dark:border-white/10 shrink-0">
                                        <Circle className="w-2.5 h-2.5" />
                                        FREE
                                    </span>
                                );
                            }
                        })()}

                        {/* 自定义标签 */}
                        {displayedAccount.custom_label && (
                            <span className="flex items-center gap-1 px-2 py-0.5 rounded-md bg-orange-100 dark:bg-orange-900/30 text-orange-600 dark:text-orange-400 text-[10px] font-bold shadow-sm shrink-0">
                                <Tag className="w-2.5 h-2.5" />
                                {displayedAccount.custom_label}
                            </span>
                        )}
                    </div>

                    {/* 统一分组展示: Gemini、Claude/第三方以及专属 Gemini 4 Argon */}
                    <div className="flex flex-col gap-2 my-auto">
                        <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
                            {groupSummaries
                                .filter((s) => s.key !== 'argon')
                                .map((summary) => (
                                    <GroupQuotaCard
                                        key={summary.key}
                                        summary={summary}
                                        isProtected={isGroupProtected(summary.key)}
                                        liveLimit={
                                            summary.key === 'gemini'
                                                ? getLiveLimitForModel(displayedAccount, 'gemini-pro')
                                                : getLiveLimitForModel(displayedAccount, 'claude')
                                        }
                                    />
                                ))}
                        </div>
                        {(() => {
                            const argon = groupSummaries.find((s) => s.key === 'argon');
                            if (!argon) return null;
                            return (
                                <div className="w-full">
                                    <GroupQuotaCard
                                        summary={argon}
                                        isProtected={isGroupProtected('argon')}
                                        liveLimit={getLiveLimitForModel(
                                            displayedAccount,
                                            'gemini-4-argon'
                                        )}
                                        fullWidth
                                    />
                                </div>
                            );
                        })()}
                    </div>

                    {onSwitch && (
                        <div className="pt-2">
                            <button
                                className="w-full px-3 py-1.5 text-xs text-gray-700 dark:text-zinc-300 border border-gray-200 dark:border-white/[0.12] rounded-lg hover:bg-gray-50 dark:hover:bg-white/[0.06] active:scale-[0.98] transition-all cursor-pointer"
                                onClick={() => onSwitch(activeTab)}
                            >
                                {t('dashboard.switch_account', '切换账号')}
                            </button>
                        </div>
                    )}
                </div>
            )}
        </div>
    );
}

export default CurrentAccount;

