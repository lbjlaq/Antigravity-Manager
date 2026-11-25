import React, { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { AlertTriangle, Check, Globe, Search, ShieldCheck, X } from 'lucide-react';
import { Account, getAccountTier } from '../../types/account';

interface AccountScopeSelectorProps {
    accounts: Account[];
    selectedIds: string[];
    onChange: (ids: string[]) => void;
}

const getTierBadgeClass = (tier: ReturnType<typeof getAccountTier>) => {
    switch (tier) {
        case 'ultra':
            return 'bg-purple-50 dark:bg-purple-900/20 text-purple-600 dark:text-purple-400 border-purple-200 dark:border-purple-800/40';
        case 'pro':
            return 'bg-blue-50 dark:bg-blue-900/20 text-blue-600 dark:text-blue-400 border-blue-200 dark:border-blue-800/40';
        case 'enterprise':
            return 'bg-amber-50 dark:bg-amber-900/20 text-amber-600 dark:text-amber-400 border-amber-200 dark:border-amber-800/40';
        default:
            return 'bg-gray-100 dark:bg-base-300 text-gray-500 dark:text-gray-400 border-gray-200 dark:border-base-300';
    }
};

export const AccountScopeSelector: React.FC<AccountScopeSelectorProps> = ({
    accounts,
    selectedIds,
    onChange,
}) => {
    const { t } = useTranslation();
    const [searchQuery, setSearchQuery] = useState('');
    const [isSpecificMode, setIsSpecificMode] = useState(() => selectedIds.length > 0);

    useEffect(() => {
        if (selectedIds.length > 0) {
            setIsSpecificMode(true);
        }
    }, [selectedIds.length]);

    const accountMap = useMemo(
        () => new Map(accounts.map(acc => [acc.id, acc])),
        [accounts]
    );

    const selectedSet = useMemo(() => new Set(selectedIds), [selectedIds]);

    const missingIds = useMemo(
        () => selectedIds.filter(id => !accountMap.has(id)),
        [selectedIds, accountMap]
    );

    const filteredAccounts = useMemo(() => {
        const q = searchQuery.trim().toLowerCase();
        if (!q) return accounts;
        return accounts.filter(acc => {
            const email = acc.email.toLowerCase();
            const name = (acc.name || '').toLowerCase();
            const label = (acc.custom_label || '').toLowerCase();
            return email.includes(q) || name.includes(q) || label.includes(q);
        });
    }, [accounts, searchQuery]);

    const handleSelectAllMode = () => {
        setIsSpecificMode(false);
        setSearchQuery('');
        if (selectedIds.length > 0) {
            onChange([]);
        }
    };

    const handleSelectSpecificMode = () => {
        setIsSpecificMode(true);
    };

    const toggleAccount = (id: string) => {
        if (selectedSet.has(id)) {
            onChange(selectedIds.filter(item => item !== id));
        } else {
            onChange([...selectedIds, id]);
        }
    };

    const removeMissingId = (id: string) => {
        onChange(selectedIds.filter(item => item !== id));
    };

    return (
        <div className="form-control w-full mb-3">
            <label className="label">
                <span className="label-text">
                    {t('user_token.account_scope', { defaultValue: 'Account Scope' })}
                </span>
                {selectedIds.length > 0 && (
                    <span className="label-text-alt text-blue-500 font-medium">
                        {t('user_token.bound_accounts_count', {
                            count: selectedIds.length,
                            defaultValue: '{{count}} Accounts',
                        })}
                    </span>
                )}
            </label>

            {/* Mode Toggle Pills */}
            <div className="grid grid-cols-2 gap-2 p-1 bg-gray-100 dark:bg-base-200 rounded-xl mb-2">
                <button
                    type="button"
                    onClick={handleSelectAllMode}
                    className={`flex items-center justify-center gap-1.5 py-1.5 px-3 rounded-lg text-xs font-medium transition-all ${
                        !isSpecificMode
                            ? 'bg-white dark:bg-base-100 text-gray-900 dark:text-white shadow-sm'
                            : 'text-gray-500 hover:text-gray-700 dark:hover:text-gray-300'
                    }`}
                >
                    <Globe size={13} />
                    <span>
                        {t('user_token.all_accounts_unlimited', {
                            defaultValue: 'All Accounts (Unlimited)',
                        })}
                    </span>
                </button>
                <button
                    type="button"
                    onClick={handleSelectSpecificMode}
                    className={`flex items-center justify-center gap-1.5 py-1.5 px-3 rounded-lg text-xs font-medium transition-all ${
                        isSpecificMode
                            ? 'bg-white dark:bg-base-100 text-blue-600 dark:text-blue-400 shadow-sm'
                            : 'text-gray-500 hover:text-gray-700 dark:hover:text-gray-300'
                    }`}
                >
                    <ShieldCheck size={13} />
                    <span>
                        {t('user_token.specific_accounts', {
                            defaultValue: 'Specific Accounts',
                        })}
                    </span>
                </button>
            </div>

            {!isSpecificMode ? (
                <div className="text-xs text-gray-500 dark:text-gray-400 bg-gray-50 dark:bg-base-200/60 border border-gray-100 dark:border-base-300 rounded-xl px-3 py-2.5">
                    {t('user_token.all_accounts_hint', {
                        defaultValue: 'This token can access all available accounts in the pool.',
                    })}
                </div>
            ) : (
                <div className="border border-gray-200 dark:border-base-300 rounded-xl overflow-hidden bg-white dark:bg-base-100">
                    {/* Search Input */}
                    <div className="relative border-b border-gray-100 dark:border-base-300">
                        <Search
                            size={14}
                            className="absolute left-3 top-1/2 -translate-y-1/2 text-gray-400 pointer-events-none"
                        />
                        <input
                            type="text"
                            value={searchQuery}
                            onChange={e => setSearchQuery(e.target.value)}
                            placeholder={t('user_token.search_accounts_placeholder', {
                                defaultValue: 'Search by email, name, or label...',
                            })}
                            className="w-full pl-8 pr-8 py-2 text-xs bg-transparent focus:outline-none text-gray-800 dark:text-gray-200 placeholder:text-gray-400"
                        />
                        {searchQuery && (
                            <button
                                type="button"
                                onClick={() => setSearchQuery('')}
                                className="absolute right-2.5 top-1/2 -translate-y-1/2 text-gray-400 hover:text-gray-600 dark:hover:text-gray-200"
                            >
                                <X size={13} />
                            </button>
                        )}
                    </div>

                    {/* Stale / Missing Account IDs */}
                    {missingIds.length > 0 && (
                        <div className="bg-amber-50/70 dark:bg-amber-900/10 border-b border-amber-200/70 dark:border-amber-800/30 p-2 space-y-1.5">
                            <div className="flex items-center gap-1.5 text-[11px] font-medium text-amber-700 dark:text-amber-400 px-1">
                                <AlertTriangle size={12} />
                                <span>
                                    {t('user_token.missing_accounts_warning', {
                                        defaultValue:
                                            'Some bound accounts no longer exist. You can remove them below.',
                                    })}
                                </span>
                            </div>
                            {missingIds.map(id => (
                                <div
                                    key={id}
                                    className="flex items-center justify-between gap-2 px-2 py-1.5 rounded-lg bg-white/80 dark:bg-base-200/80 border border-amber-200/60 dark:border-amber-800/30"
                                >
                                    <div className="flex items-center gap-2 min-w-0">
                                        <code className="text-[11px] font-mono text-gray-600 dark:text-gray-300 truncate">
                                            {id}
                                        </code>
                                        <span className="shrink-0 text-[10px] px-1.5 py-0.5 rounded bg-red-50 dark:bg-red-900/20 text-red-600 dark:text-red-400 border border-red-200 dark:border-red-800/40 font-medium">
                                            {t('user_token.account_missing', {
                                                defaultValue: 'Missing / Deleted',
                                            })}
                                        </span>
                                    </div>
                                    <button
                                        type="button"
                                        onClick={() => removeMissingId(id)}
                                        className="p-1 rounded hover:bg-red-50 dark:hover:bg-red-900/20 text-gray-400 hover:text-red-500 transition-colors shrink-0"
                                        title={t('common.delete', { defaultValue: 'Remove' })}
                                    >
                                        <X size={13} />
                                    </button>
                                </div>
                            ))}
                        </div>
                    )}

                    {/* Account List */}
                    <div className="max-h-48 overflow-y-auto divide-y divide-gray-50 dark:divide-base-200">
                        {filteredAccounts.length === 0 ? (
                            <div className="py-6 text-center text-xs text-gray-400">
                                {t('user_token.no_accounts_found', {
                                    defaultValue: 'No matching accounts found',
                                })}
                            </div>
                        ) : (
                            filteredAccounts.map(acc => {
                                const isSelected = selectedSet.has(acc.id);
                                const tier = acc.quota?.subscription_tier
                                    ? getAccountTier(acc)
                                    : null;
                                const isDisabled = Boolean(acc.disabled);
                                const isProxyDisabled = Boolean(acc.proxy_disabled);

                                return (
                                    <button
                                        key={acc.id}
                                        type="button"
                                        onClick={() => toggleAccount(acc.id)}
                                        className={`w-full flex items-center justify-between gap-2 px-3 py-2 text-left transition-colors ${
                                            isSelected
                                                ? 'bg-blue-50/70 dark:bg-blue-900/15'
                                                : 'hover:bg-gray-50 dark:hover:bg-base-200/60'
                                        }`}
                                    >
                                        <div className="flex items-center gap-2.5 min-w-0">
                                            <div
                                                className={`w-4 h-4 rounded flex items-center justify-center border transition-colors shrink-0 ${
                                                    isSelected
                                                        ? 'bg-blue-500 border-blue-500 text-white'
                                                        : 'border-gray-300 dark:border-base-300 bg-white dark:bg-base-100'
                                                }`}
                                            >
                                                {isSelected && <Check size={11} strokeWidth={3} />}
                                            </div>
                                            <div className="min-w-0">
                                                <div className="text-xs font-medium text-gray-800 dark:text-gray-200 truncate">
                                                    {acc.email}
                                                </div>
                                                {(acc.custom_label || acc.name) && (
                                                    <div className="text-[10px] text-gray-400 truncate">
                                                        {[acc.custom_label, acc.name]
                                                            .filter(Boolean)
                                                            .join(' · ')}
                                                    </div>
                                                )}
                                            </div>
                                        </div>

                                        <div className="flex items-center gap-1.5 shrink-0">
                                            {tier && (
                                                <span
                                                    className={`text-[10px] px-1.5 py-0.5 rounded border font-semibold uppercase ${getTierBadgeClass(
                                                        tier
                                                    )}`}
                                                >
                                                    {tier}
                                                </span>
                                            )}
                                            {isDisabled && (
                                                <span className="text-[10px] px-1.5 py-0.5 rounded bg-red-50 dark:bg-red-900/20 text-red-600 dark:text-red-400 border border-red-200 dark:border-red-800/40">
                                                    {t('user_token.account_disabled', {
                                                        defaultValue: 'Disabled',
                                                    })}
                                                </span>
                                            )}
                                            {!isDisabled && isProxyDisabled && (
                                                <span className="text-[10px] px-1.5 py-0.5 rounded bg-orange-50 dark:bg-orange-900/20 text-orange-600 dark:text-orange-400 border border-orange-200 dark:border-orange-800/40">
                                                    {t('user_token.account_proxy_disabled', {
                                                        defaultValue: 'Proxy Disabled',
                                                    })}
                                                </span>
                                            )}
                                        </div>
                                    </button>
                                );
                            })
                        )}
                    </div>
                </div>
            )}

            <label className="label">
                <span className="label-text-alt text-gray-500">
                    {t('user_token.hint_account_scope', {
                        defaultValue:
                            'Leave as All Accounts to allow routing through any available account.',
                    })}
                </span>
            </label>
        </div>
    );
};
