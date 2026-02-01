import { useTranslation } from 'react-i18next';
import { memo } from 'react';
import { Account } from '@/entities/account';
import AccountCard from './AccountCard';
import { motion } from 'framer-motion';

interface AccountGridProps {
    accounts: Account[];
    selectedIds: Set<string>;
    refreshingIds: Set<string>;
    proxySelectedAccountIds?: Set<string>;
    onToggleSelect: (id: string) => void;
    currentAccountId: string | null;
    switchingAccountId: string | null;
    onSwitch: (accountId: string) => void;
    onRefresh: (accountId: string) => void;
    onViewDevice: (accountId: string) => void;
    onViewDetails: (accountId: string) => void;
    onExport: (accountId: string) => void;
    onDelete: (accountId: string) => void;
    onToggleProxy: (accountId: string) => void;
    onWarmup?: (accountId: string) => void;
}

const container = {
    hidden: { opacity: 0 },
    show: {
        opacity: 1,
        transition: {
            staggerChildren: 0.05
        }
    }
};

const item = {
    hidden: { opacity: 0, y: 20 },
    show: { opacity: 1, y: 0 }
};

const AccountGrid = memo(({ accounts, selectedIds, refreshingIds, proxySelectedAccountIds, onToggleSelect, currentAccountId, switchingAccountId, onSwitch, onRefresh, onViewDetails, onExport, onDelete, onToggleProxy, onViewDevice, onWarmup }: AccountGridProps) => {
    const { t } = useTranslation();
    if (accounts.length === 0) {
        return (
            <div className="flex flex-col items-center justify-center py-20 bg-zinc-50 dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 rounded-xl">
                <p className="text-zinc-500 dark:text-zinc-500 mb-2">{t('accounts.empty.title')}</p>
                <p className="text-sm text-zinc-400 dark:text-zinc-600">{t('accounts.empty.desc')}</p>
            </div>
        );
    }

    return (
        <motion.div 
            variants={container}
            initial="hidden"
            animate="show"
            className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4 pb-4"
        >
            {accounts.map((account) => (
                <motion.div key={account.id} variants={item}>
                    <AccountCard
                        account={account}
                        selected={selectedIds.has(account.id)}
                        isRefreshing={refreshingIds.has(account.id)}
                        isSelectedForProxy={proxySelectedAccountIds?.has(account.id) || false}
                        onSelect={() => onToggleSelect(account.id)}
                        isCurrent={account.id === currentAccountId}
                        isSwitching={account.id === switchingAccountId}
                        onSwitch={() => onSwitch(account.id)}
                        onRefresh={() => onRefresh(account.id)}
                        onViewDevice={() => onViewDevice(account.id)}
                        onViewDetails={() => onViewDetails(account.id)}
                        onExport={() => onExport(account.id)}
                        onDelete={() => onDelete(account.id)}
                        onToggleProxy={() => onToggleProxy(account.id)}
                        onWarmup={onWarmup ? () => onWarmup(account.id) : undefined}
                    />
                </motion.div>
            ))}
        </motion.div>
    );
});

export default AccountGrid;
