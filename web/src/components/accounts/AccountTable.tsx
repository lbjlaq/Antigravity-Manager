import { Account } from '../../types/account';
import AccountRow from './AccountRow';
import { useTranslation } from 'react-i18next';

interface AccountTableProps {
    accounts: Account[];
    selectedIds: Set<string>;
    refreshingIds: Set<string>;
    onToggleSelect: (id: string) => void;
    onToggleAll: () => void;
    currentAccountId: string | null;
    switchingAccountId: string | null;
    onSwitch: (accountId: string) => void;
    onRefresh: (accountId: string) => void;
    onViewDetails: (accountId: string) => void;
    onExport: (accountId: string) => void;
    onDelete: (accountId: string) => void;
}

function AccountTable({ accounts, selectedIds, refreshingIds, onToggleSelect, onToggleAll, currentAccountId, switchingAccountId, onSwitch, onRefresh, onViewDetails, onExport, onDelete }: AccountTableProps) {
    const { t } = useTranslation();

    if (accounts.length === 0) {
        return (
            <div className="bg-white dark:bg-base-100 rounded-2xl p-12 shadow-sm border border-gray-100 dark:border-base-200 text-center">
                <p className="text-gray-400 mb-2">{t('accounts.empty.title')}</p>
                <p className="text-sm text-gray-400">{t('accounts.empty.desc')}</p>
            </div>
        );
    }

    return (
        <div className="overflow-x-auto">
            <table className="w-full">
                <thead>
                    <tr className="border-b border-gray-100 dark:border-base-200 bg-gray-50 dark:bg-base-200">
                        <th className="pl-6 py-2 text-left w-12">
                            <input
                                type="checkbox"
                                className="checkbox checkbox-sm rounded border-2 border-gray-400 dark:border-gray-500 checked:border-blue-600 checked:bg-blue-600 [--chkbg:theme(colors.blue.600)] [--chkfg:white]"
                                checked={accounts.length > 0 && selectedIds.size === accounts.length}
                                onChange={onToggleAll}
                            />
                        </th>
                        <th className="px-6 py-2 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider whitespace-nowrap">{t('accounts.table.email')}</th>
                        <th className="px-6 py-2 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider w-96 whitespace-nowrap">{t('accounts.table.quota')}</th>
                        <th className="px-6 py-2 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider whitespace-nowrap">{t('accounts.table.last_used')}</th>
                        <th className="px-6 py-2 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider whitespace-nowrap">{t('accounts.table.actions')}</th>
                    </tr>
                </thead>
                <tbody className="divide-y divide-gray-100 dark:divide-base-200">
                    {accounts.map((account) => (
                        <AccountRow
                            key={account.id}
                            account={account}
                            selected={selectedIds.has(account.id)}
                            isRefreshing={refreshingIds.has(account.id)}
                            onSelect={() => onToggleSelect(account.id)}
                            isCurrent={account.id === currentAccountId}
                            isSwitching={account.id === switchingAccountId}
                            onSwitch={() => onSwitch(account.id)}
                            onRefresh={() => onRefresh(account.id)}
                            onViewDetails={() => onViewDetails(account.id)}
                            onExport={() => onExport(account.id)}
                            onDelete={() => onDelete(account.id)}
                        />
                    ))}
                </tbody>
            </table>
        </div>
    );
}

export default AccountTable;
