// File: src/widgets/dashboard/ui/BestAccounts.tsx
// Best Accounts - Clean cards, max 3

import { memo } from 'react';
import { TrendingUp, Crown, Zap, Bot } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Account } from '@/entities/account';
import { cn } from '@/shared/lib';

interface BestAccountsProps {
    accounts: Account[];
    currentAccountId?: string;
    onSwitch?: (accountId: string) => void;
}

const BestAccounts = function BestAccounts({ accounts, currentAccountId, onSwitch }: BestAccountsProps) {
    const { t } = useTranslation();

    const candidates = accounts
        .filter(a => a.id !== currentAccountId && !a.disabled)
        .map(a => {
            const geminiPro = a.quota?.models.find(m => m.name === 'gemini-3-pro-high')?.percentage || 0;
            const claude = a.quota?.models.find(m => m.name.toLowerCase().includes('claude'))?.percentage || 0;
            const geminiFlash = a.quota?.models.find(m => m.name === 'gemini-3-flash')?.percentage || 0;
            const score = Math.round((geminiPro * 0.4) + (claude * 0.4) + (geminiFlash * 0.2));

            return { ...a, score, metrics: { geminiPro, claude, geminiFlash } };
        })
        .filter(a => a.score > 10)
        .sort((a, b) => b.score - a.score)
        .slice(0, 3);

    return (
        <div className="h-full flex flex-col rounded-2xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 overflow-hidden">
            {/* Header */}
            <div className="px-5 py-4 border-b border-zinc-100 dark:border-zinc-800">
                <div className="flex items-center gap-2">
                    <div className="p-1.5 rounded-lg bg-emerald-500/10">
                        <TrendingUp className="w-4 h-4 text-emerald-500" />
                    </div>
                    <h3 className="text-xs font-semibold text-zinc-500 dark:text-zinc-400 uppercase tracking-wide">
                        {t('dashboard.best_accounts', 'Top Candidates')}
                    </h3>
                </div>
            </div>

            {/* Cards */}
            <div className="flex-1 p-4 space-y-3 overflow-y-auto">
                {candidates.length > 0 ? (
                    candidates.map((account, index) => (
                        <button
                            key={account.id}
                            onClick={() => onSwitch?.(account.id)}
                            className={cn(
                                "w-full p-4 rounded-xl border text-left transition-all duration-200",
                                "bg-zinc-50 dark:bg-zinc-800/50",
                                "border-zinc-200 dark:border-zinc-700",
                                "hover:border-indigo-400 dark:hover:border-indigo-500",
                                "hover:shadow-md",
                                "group"
                            )}
                        >
                            {/* Top Row: Rank + Email + Score */}
                            <div className="flex items-center gap-3 mb-3">
                                {/* Rank Badge */}
                                <div className={cn(
                                    "w-6 h-6 rounded-lg flex items-center justify-center shrink-0",
                                    index === 0
                                        ? "bg-gradient-to-br from-amber-400 to-orange-500 text-white"
                                        : "bg-zinc-200 dark:bg-zinc-700 text-zinc-500 dark:text-zinc-400"
                                )}>
                                    {index === 0 ? <Crown className="w-3 h-3" /> : <span className="text-[10px] font-bold">{index + 1}</span>}
                                </div>

                                {/* Email */}
                                <p className="flex-1 text-sm font-medium text-zinc-700 dark:text-zinc-200 truncate group-hover:text-indigo-600 dark:group-hover:text-indigo-400 transition-colors">
                                    {account.email}
                                </p>

                                {/* Score Badge */}
                                <div className="shrink-0 px-2 py-0.5 rounded-md bg-indigo-100 dark:bg-indigo-500/20">
                                    <span className="text-xs font-bold text-indigo-600 dark:text-indigo-400 tabular-nums">
                                        {account.score}
                                    </span>
                                </div>
                            </div>

                            {/* Quota Bars */}
                            <div className="space-y-2">
                                <QuotaBar icon={Zap} value={account.metrics.geminiPro} color="emerald" />
                                <QuotaBar icon={Bot} value={account.metrics.claude} color="cyan" />
                                <QuotaBar icon={Zap} value={account.metrics.geminiFlash} color="amber" />
                            </div>
                        </button>
                    ))
                ) : (
                    <div className="h-full flex flex-col items-center justify-center py-12 text-center">
                        <div className="w-14 h-14 rounded-2xl bg-zinc-100 dark:bg-zinc-800 flex items-center justify-center mb-3">
                            <TrendingUp className="w-7 h-7 text-zinc-300 dark:text-zinc-600" />
                        </div>
                        <p className="text-sm font-medium text-zinc-500 dark:text-zinc-400">
                            {t('dashboard.no_candidates', 'No candidates yet')}
                        </p>
                        <p className="text-xs text-zinc-400 dark:text-zinc-500 mt-1 max-w-[200px]">
                            {t('dashboard.add_more_accounts', 'Add more accounts to see recommendations')}
                        </p>
                    </div>
                )}
            </div>
        </div>
    );
};

// Quota Bar Component
interface QuotaBarProps {
    icon: React.ElementType;
    value: number;
    color: 'emerald' | 'cyan' | 'amber';
}

function QuotaBar({ icon: Icon, value, color }: QuotaBarProps) {
    const colors = {
        emerald: { bar: 'bg-emerald-500', text: 'text-emerald-600 dark:text-emerald-400' },
        cyan: { bar: 'bg-cyan-500', text: 'text-cyan-600 dark:text-cyan-400' },
        amber: { bar: 'bg-amber-500', text: 'text-amber-600 dark:text-amber-400' },
    };

    const isLow = value < 30;
    const barColor = isLow ? 'bg-zinc-300 dark:bg-zinc-600' : colors[color].bar;
    const textColor = isLow ? 'text-zinc-500' : colors[color].text;

    return (
        <div className="flex items-center gap-2">
            <Icon className="w-3 h-3 text-zinc-400 shrink-0" />
            <div className="flex-1">
                <div className="h-1.5 bg-zinc-200 dark:bg-zinc-700 rounded-full overflow-hidden">
                    <div
                        className={cn("h-full rounded-full transition-all duration-300", barColor)}
                        style={{ width: `${value}%` }}
                    />
                </div>
            </div>
            <span className={cn("text-[10px] font-bold tabular-nums w-8 text-right", textColor)}>
                {value}%
            </span>
        </div>
    );
}

export default memo(BestAccounts);
