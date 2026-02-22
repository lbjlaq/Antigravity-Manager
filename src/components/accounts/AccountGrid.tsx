import { useTranslation } from 'react-i18next';
import { Account } from '../../types/account';
import AccountCard from './AccountCard';

interface AccountGridProps {
    accounts: Account[];
    selectedIds: Set<string>;
    refreshingIds: Set<string>;
    onToggleSelect: (id: string) => void;
    currentAccountId: string | null;
    switchingAccountId: string | null;
    onSwitch: (accountId: string) => void;
    onRefresh: (accountId: string) => void;
    onViewDevice: (accountId: string) => void;
    onViewDetails: (accountId: string) => void;
    onCopy: (accountId: string) => void;
    onDelete: (accountId: string) => void;
    onToggleProxy: (accountId: string) => void;
    onWarmup?: (accountId: string) => void;
    onUpdateLabel?: (accountId: string, label: string) => void;
    onViewError: (accountId: string) => void;
}


function AccountGrid({ accounts, selectedIds, refreshingIds, onToggleSelect, currentAccountId, switchingAccountId, onSwitch, onRefresh, onViewDetails, onCopy, onDelete, onToggleProxy, onViewDevice, onWarmup, onUpdateLabel, onViewError }: AccountGridProps) {
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
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
            {accounts.map((account) => (
                <AccountCard
                    key={account.id}
                    account={account}
                    selected={selectedIds.has(account.id)}
                    isRefreshing={refreshingIds.has(account.id)}
                    onSelect={() => onToggleSelect(account.id)}
                    isCurrent={account.id === currentAccountId}
                    isSwitching={account.id === switchingAccountId}
                    onSwitch={() => onSwitch(account.id)}
                    onRefresh={() => onRefresh(account.id)}
                    onViewDevice={() => onViewDevice(account.id)}
                    onViewDetails={() => onViewDetails(account.id)}
                    onCopy={() => onCopy(account.id)}
                    onDelete={() => onDelete(account.id)}
                    onToggleProxy={() => onToggleProxy(account.id)}
                    onWarmup={onWarmup ? () => onWarmup(account.id) : undefined}
                    onUpdateLabel={onUpdateLabel ? (label: string) => onUpdateLabel(account.id, label) : undefined}
                    onViewError={() => onViewError(account.id)}
                />
            ))}
        </div>
    );
}

export default AccountGrid;
