// File: src/components/security/IpListTable.tsx
// Universal table component for IP blacklist/whitelist

import React from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { Trash2, Shield, ShieldCheck, Clock, Hash, AlertCircle, RefreshCw } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { IpBlacklistEntry, IpWhitelistEntry, IpListType } from '../../types/security';
import { formatDistanceToNow } from 'date-fns';

interface IpListTableProps {
  type: IpListType;
  data: (IpBlacklistEntry | IpWhitelistEntry)[];
  isLoading: boolean;
  isError: boolean;
  error: Error | null;
  onRemove: (id: number) => void;
  onRefetch: () => void;
}

export const IpListTable: React.FC<IpListTableProps> = ({
  type,
  data,
  isLoading,
  isError,
  error,
  onRemove,
  onRefetch,
}) => {
  const { t } = useTranslation();
  const isBlacklist = type === 'blacklist';

  const formatTimestamp = (timestamp: number): string => {
    try {
      return formatDistanceToNow(new Date(timestamp * 1000), { addSuffix: true });
    } catch {
      return 'Unknown';
    }
  };

  const formatExpiresAt = (expiresAt: number | null): string => {
    if (!expiresAt) return t('security.permanent', 'Permanent');
    const now = Date.now() / 1000;
    if (expiresAt < now) return t('security.expired', 'Expired');
    try {
      return formatDistanceToNow(new Date(expiresAt * 1000), { addSuffix: true });
    } catch {
      return 'Unknown';
    }
  };

  // Loading state
  if (isLoading) {
    return (
      <div className="space-y-3">
        {[...Array(3)].map((_, i) => (
          <div key={i} className="h-16 bg-zinc-800/50 rounded-xl animate-pulse" />
        ))}
      </div>
    );
  }

  // Error state
  if (isError) {
    return (
      <div className="flex flex-col items-center justify-center py-12 text-zinc-500">
        <AlertCircle className="w-12 h-12 mb-4 text-red-400" />
        <p className="text-red-400 mb-4">{error?.message || 'Failed to load data'}</p>
        <button
          onClick={onRefetch}
          className="flex items-center gap-2 px-4 py-2 bg-zinc-800 hover:bg-zinc-700 rounded-lg transition-colors"
        >
          <RefreshCw className="w-4 h-4" />
          {t('common.retry', 'Retry')}
        </button>
      </div>
    );
  }

  // Empty state
  if (data.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center py-16 text-zinc-500">
        {isBlacklist ? (
          <Shield className="w-16 h-16 mb-4 opacity-30" />
        ) : (
          <ShieldCheck className="w-16 h-16 mb-4 opacity-30" />
        )}
        <p className="text-lg">
          {isBlacklist
            ? t('security.no_blacklist', 'No blocked IPs')
            : t('security.no_whitelist', 'No whitelisted IPs')}
        </p>
        <p className="text-sm text-zinc-600 mt-2">
          {isBlacklist
            ? t('security.no_blacklist_desc', 'Add IPs to block access to the proxy')
            : t('security.no_whitelist_desc', 'Add IPs to allow access in strict mode')}
        </p>
      </div>
    );
  }

  return (
    <div className="space-y-2">
      <AnimatePresence mode="popLayout">
        {data.map((entry, index) => {
          const isBlacklistEntry = 'reason' in entry;
          const blacklistEntry = isBlacklistEntry ? (entry as IpBlacklistEntry) : null;
          const whitelistEntry = !isBlacklistEntry ? (entry as IpWhitelistEntry) : null;

          return (
            <motion.div
              key={entry.id}
              initial={{ opacity: 0, y: 10 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, x: -20 }}
              transition={{ delay: index * 0.03 }}
              className={`group relative p-4 rounded-xl border backdrop-blur-sm transition-all duration-200 hover:scale-[1.01] ${
                isBlacklist
                  ? 'bg-red-500/5 border-red-500/20 hover:border-red-500/40'
                  : 'bg-emerald-500/5 border-emerald-500/20 hover:border-emerald-500/40'
              }`}
            >
              {/* Main content */}
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-4">
                  {/* Icon */}
                  <div
                    className={`p-2.5 rounded-lg ${
                      isBlacklist ? 'bg-red-500/10' : 'bg-emerald-500/10'
                    }`}
                  >
                    {isBlacklist ? (
                      <Shield className="w-5 h-5 text-red-400" />
                    ) : (
                      <ShieldCheck className="w-5 h-5 text-emerald-400" />
                    )}
                  </div>

                  {/* IP Pattern */}
                  <div>
                    <div className="flex items-center gap-2">
                      <span className="font-mono text-base font-semibold text-zinc-100">
                        {entry.ipPattern}
                      </span>
                      {entry.ipPattern.includes('/') && (
                        <span className="px-1.5 py-0.5 text-[10px] font-bold uppercase rounded bg-blue-500/20 text-blue-400">
                          CIDR
                        </span>
                      )}
                    </div>
                    <div className="flex items-center gap-3 mt-1 text-xs text-zinc-500">
                      {/* Reason/Description */}
                      {blacklistEntry?.reason && (
                        <span className="text-zinc-400">{blacklistEntry.reason}</span>
                      )}
                      {whitelistEntry?.description && (
                        <span className="text-zinc-400">{whitelistEntry.description}</span>
                      )}
                    </div>
                  </div>
                </div>

                {/* Meta info */}
                <div className="flex items-center gap-6">
                  {/* Hit count (blacklist only) */}
                  {blacklistEntry && (
                    <div className="flex items-center gap-1.5 text-xs text-zinc-500">
                      <Hash className="w-3.5 h-3.5" />
                      <span>{blacklistEntry.hitCount} hits</span>
                    </div>
                  )}

                  {/* Expires (blacklist only) */}
                  {blacklistEntry && (
                    <div className="flex items-center gap-1.5 text-xs text-zinc-500">
                      <Clock className="w-3.5 h-3.5" />
                      <span>{formatExpiresAt(blacklistEntry.expiresAt)}</span>
                    </div>
                  )}

                  {/* Created at */}
                  <div className="text-xs text-zinc-600 min-w-[80px] text-right">
                    {formatTimestamp(entry.createdAt)}
                  </div>

                  {/* Remove button */}
                  <button
                    onClick={() => onRemove(entry.id)}
                    className="p-2 rounded-lg text-zinc-500 hover:text-red-400 hover:bg-red-500/10 opacity-0 group-hover:opacity-100 transition-all"
                    title={t('common.remove', 'Remove')}
                  >
                    <Trash2 className="w-4 h-4" />
                  </button>
                </div>
              </div>
            </motion.div>
          );
        })}
      </AnimatePresence>
    </div>
  );
};
