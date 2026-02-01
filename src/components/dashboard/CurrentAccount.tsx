// File: src/components/dashboard/CurrentAccount.tsx
// Premium Current Account Card - Clean & Functional

import { memo, useState } from 'react';
import { Mail, Zap, Copy, Check, ArrowRight, Gem, Diamond, Circle, Bot, Image } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Account } from '@/entities/account';
import { cn } from '@/shared/lib';
import { Badge, Button } from '@/shared/ui';

interface CurrentAccountProps {
    account: Account | null | undefined;
    onSwitch?: () => void;
}

const CurrentAccount = function CurrentAccount({ account, onSwitch }: CurrentAccountProps) {
    const { t } = useTranslation();
    const [copied, setCopied] = useState(false);

    if (!account) {
        return (
            <div className="p-8 rounded-2xl border border-dashed border-zinc-300 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-900/50">
                <div className="text-center space-y-3">
                    <div className="w-12 h-12 mx-auto rounded-xl bg-zinc-200 dark:bg-zinc-800 flex items-center justify-center">
                        <Mail className="w-6 h-6 text-zinc-400" />
                    </div>
                    <div>
                        <p className="text-sm font-medium text-zinc-600 dark:text-zinc-400">
                            {t('dashboard.no_active_account')}
                        </p>
                        <p className="text-xs text-zinc-400 dark:text-zinc-500 mt-1">
                            {t('dashboard.add_account_hint', 'Add an account to get started')}
                        </p>
                    </div>
                </div>
            </div>
        );
    }

    const models = [
        { 
            key: 'gemini-pro',
            name: 'Gemini Pro', 
            icon: Zap,
            model: account.quota?.models.find(m => m.name === 'gemini-3-pro-high'),
            color: 'emerald'
        },
        { 
            key: 'claude',
            name: 'Claude 4.5', 
            icon: Bot,
            model: account.quota?.models.find(m => m.name === 'claude-sonnet-4-5-thinking'),
            color: 'cyan'
        },
        { 
            key: 'gemini-flash',
            name: 'Gemini Flash', 
            icon: Zap,
            model: account.quota?.models.find(m => m.name === 'gemini-3-flash'),
            color: 'amber'
        },
        { 
            key: 'gemini-image',
            name: 'Gemini Image', 
            icon: Image,
            model: account.quota?.models.find(m => m.name === 'gemini-3-pro-image'),
            color: 'purple'
        },
    ].filter(m => m.model);

    const getTierBadge = () => {
        const tier = (account.quota?.subscription_tier || '').toLowerCase();
        if (tier.includes('ultra')) {
            return (
                <Badge className="bg-purple-500/10 text-purple-400 border-purple-500/20 gap-1">
                    <Gem className="w-3 h-3" /> ULTRA
                </Badge>
            );
        } else if (tier.includes('pro')) {
            return (
                <Badge className="bg-blue-500/10 text-blue-400 border-blue-500/20 gap-1">
                    <Diamond className="w-3 h-3" /> PRO
                </Badge>
            );
        }
        return (
            <Badge variant="outline" className="border-zinc-600 text-zinc-400 bg-zinc-800/50 gap-1">
                <Circle className="w-3 h-3" /> FREE
            </Badge>
        );
    };

    const copyEmail = async () => {
        try {
            await navigator.clipboard.writeText(account.email);
            setCopied(true);
            setTimeout(() => setCopied(false), 2000);
        } catch (err) {
            console.error('Failed to copy', err);
        }
    };

    const getColorClasses = (color: string, percentage: number) => {
        const isLow = percentage < 30;
        const colors: Record<string, { bar: string; text: string }> = {
            emerald: { 
                bar: isLow ? 'from-amber-500 to-amber-400' : 'from-emerald-500 to-emerald-400',
                text: isLow ? 'text-amber-400' : 'text-emerald-400'
            },
            cyan: { 
                bar: isLow ? 'from-amber-500 to-amber-400' : 'from-cyan-500 to-cyan-400',
                text: isLow ? 'text-amber-400' : 'text-cyan-400'
            },
            amber: { 
                bar: isLow ? 'from-red-500 to-red-400' : 'from-amber-500 to-amber-400',
                text: isLow ? 'text-red-400' : 'text-amber-400'
            },
            purple: { 
                bar: isLow ? 'from-amber-500 to-amber-400' : 'from-purple-500 to-purple-400',
                text: isLow ? 'text-amber-400' : 'text-purple-400'
            },
        };
        return colors[color] || colors.emerald;
    };

    return (
        <div className="rounded-2xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 overflow-hidden">
            {/* Header */}
            <div className="px-6 py-4 border-b border-zinc-100 dark:border-zinc-800 flex items-center justify-between">
                <div className="flex items-center gap-3">
                    <div className="p-2 rounded-lg bg-amber-500/10">
                        <Zap className="w-4 h-4 text-amber-500" />
                    </div>
                    <div>
                        <h3 className="text-xs font-semibold text-zinc-400 uppercase tracking-wide">
                            {t('dashboard.current_account')}
                        </h3>
                    </div>
                </div>
                {getTierBadge()}
            </div>

            {/* Content */}
            <div className="p-6 space-y-6">
                {/* Email Row */}
                <div className="flex items-center gap-4 group">
                    <div className="p-3 rounded-xl bg-zinc-100 dark:bg-zinc-800 border border-zinc-200 dark:border-zinc-700">
                        <Mail className="w-5 h-5 text-zinc-500" />
                    </div>
                    <div className="flex-1 min-w-0">
                        <p className="text-[10px] font-bold text-zinc-400 uppercase tracking-wider mb-0.5">
                            Account ID
                        </p>
                        <div className="flex items-center gap-2">
                            <p className="text-base font-bold text-zinc-900 dark:text-white font-mono truncate">
                                {account.email}
                            </p>
                            <button
                                onClick={copyEmail}
                                className="p-1.5 rounded-md text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-200 hover:bg-zinc-100 dark:hover:bg-zinc-800 transition-colors opacity-0 group-hover:opacity-100"
                            >
                                {copied ? <Check className="w-3.5 h-3.5 text-green-500" /> : <Copy className="w-3.5 h-3.5" />}
                            </button>
                        </div>
                    </div>
                </div>

                {/* Quota Bars */}
                <div className="space-y-4">
                    {models.map(({ key, name, icon: Icon, model, color }) => {
                        if (!model) return null;
                        const colorClasses = getColorClasses(color, model.percentage);
                        
                        return (
                            <div key={key} className="space-y-2">
                                <div className="flex items-center justify-between">
                                    <div className="flex items-center gap-2">
                                        <Icon className="w-3.5 h-3.5 text-zinc-400" />
                                        <span className="text-xs font-medium text-zinc-600 dark:text-zinc-400">
                                            {name}
                                        </span>
                                    </div>
                                    <span className={cn("text-sm font-bold tabular-nums", colorClasses.text)}>
                                        {model.percentage}%
                                    </span>
                                </div>
                                <div className="h-2 bg-zinc-100 dark:bg-zinc-800 rounded-full overflow-hidden">
                                    <div
                                        className={cn("h-full rounded-full bg-gradient-to-r transition-all duration-500", colorClasses.bar)}
                                        style={{ width: `${model.percentage}%` }}
                                    />
                                </div>
                            </div>
                        );
                    })}
                </div>
            </div>

            {/* Footer */}
            {onSwitch && (
                <div className="px-6 py-4 bg-zinc-50 dark:bg-zinc-800/50 border-t border-zinc-100 dark:border-zinc-800">
                    <Button
                        variant="outline"
                        className="w-full h-10 font-medium border-zinc-300 dark:border-zinc-700 hover:bg-zinc-100 dark:hover:bg-zinc-800 gap-2"
                        onClick={onSwitch}
                    >
                        {t('dashboard.switch_account')}
                        <ArrowRight className="w-4 h-4" />
                    </Button>
                </div>
            )}
        </div>
    );
};

export default memo(CurrentAccount);
