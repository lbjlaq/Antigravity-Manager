// File: src/features/security/ui/AddIpDialog.tsx
// Dialog for adding IP to blacklist/whitelist with glassmorphism style

import React, { useState } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { X, Shield, ShieldCheck, Clock, AlertTriangle } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type { IpListType } from '@/entities/security';

interface AddIpDialogProps {
  isOpen: boolean;
  type: IpListType;
  onClose: () => void;
  onSubmit: (data: {
    ipPattern: string;
    reason?: string;
    description?: string;
    expiresInSeconds?: number;
  }) => void;
  isSubmitting?: boolean;
}

const DURATION_OPTIONS = [
  { label: '10 minutes', value: 600 },
  { label: '1 hour', value: 3600 },
  { label: '24 hours', value: 86400 },
  { label: '7 days', value: 604800 },
  { label: '30 days', value: 2592000 },
  { label: 'Permanent', value: 0 },
];

export const AddIpDialog: React.FC<AddIpDialogProps> = ({
  isOpen,
  type,
  onClose,
  onSubmit,
  isSubmitting = false,
}) => {
  const { t } = useTranslation();
  const isBlacklist = type === 'blacklist';

  const [ipPattern, setIpPattern] = useState('');
  const [reason, setReason] = useState('');
  const [description, setDescription] = useState('');
  const [duration, setDuration] = useState(0); // 0 = permanent
  const [ipError, setIpError] = useState('');

  const validateIp = (value: string): boolean => {
    if (!value.trim()) {
      setIpError(t('security.ip_required', 'IP address is required'));
      return false;
    }

    // Basic IPv4 pattern (with optional CIDR)
    const ipv4Pattern = /^(\d{1,3}\.){3}\d{1,3}(\/\d{1,2})?$/;
    if (!ipv4Pattern.test(value.trim())) {
      setIpError(t('security.invalid_ip', 'Invalid IP format (e.g., 192.168.1.1 or 10.0.0.0/24)'));
      return false;
    }

    // Validate octets
    const parts = value.split('/')[0].split('.');
    for (const part of parts) {
      const num = parseInt(part, 10);
      if (num < 0 || num > 255) {
        setIpError(t('security.invalid_octet', 'Each octet must be 0-255'));
        return false;
      }
    }

    // Validate CIDR prefix if present
    if (value.includes('/')) {
      const prefix = parseInt(value.split('/')[1], 10);
      if (prefix < 0 || prefix > 32) {
        setIpError(t('security.invalid_cidr', 'CIDR prefix must be 0-32'));
        return false;
      }
    }

    setIpError('');
    return true;
  };

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!validateIp(ipPattern)) return;

    onSubmit({
      ipPattern: ipPattern.trim(),
      reason: isBlacklist ? reason : undefined,
      description: !isBlacklist ? description : undefined,
      expiresInSeconds: isBlacklist && duration > 0 ? duration : undefined,
    });

    // Reset form
    setIpPattern('');
    setReason('');
    setDescription('');
    setDuration(0);
  };

  const handleClose = () => {
    setIpPattern('');
    setReason('');
    setDescription('');
    setDuration(0);
    setIpError('');
    onClose();
  };

  return (
    <AnimatePresence>
      {isOpen && (
        <>
          {/* Backdrop */}
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            onClick={handleClose}
            className="fixed inset-0 bg-black/60 backdrop-blur-sm z-50"
          />

          {/* Dialog */}
          <motion.div
            initial={{ opacity: 0, scale: 0.95, y: 20 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            exit={{ opacity: 0, scale: 0.95, y: 20 }}
            transition={{ type: 'spring', damping: 25, stiffness: 300 }}
            className="fixed left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 z-50 w-full max-w-md"
          >
            <div className="relative bg-zinc-900/95 backdrop-blur-xl border border-white/10 rounded-2xl shadow-2xl overflow-hidden">
              {/* Glow effects */}
              <div
                className={`absolute -top-20 -right-20 w-40 h-40 rounded-full blur-3xl pointer-events-none ${
                  isBlacklist ? 'bg-red-500/20' : 'bg-emerald-500/20'
                }`}
              />
              <div
                className={`absolute -bottom-20 -left-20 w-40 h-40 rounded-full blur-3xl pointer-events-none ${
                  isBlacklist ? 'bg-orange-500/10' : 'bg-teal-500/10'
                }`}
              />

              {/* Header */}
              <div className="relative z-10 flex items-center justify-between p-5 border-b border-white/5">
                <div className="flex items-center gap-3">
                  <div
                    className={`p-2.5 rounded-xl shadow-lg ${
                      isBlacklist
                        ? 'bg-gradient-to-br from-red-500 to-orange-500 shadow-red-500/25'
                        : 'bg-gradient-to-br from-emerald-500 to-teal-500 shadow-emerald-500/25'
                    }`}
                  >
                    {isBlacklist ? (
                      <Shield className="w-5 h-5 text-white" />
                    ) : (
                      <ShieldCheck className="w-5 h-5 text-white" />
                    )}
                  </div>
                  <div>
                    <h3 className="font-bold text-white text-base">
                      {isBlacklist
                        ? t('security.add_to_blacklist', 'Add to Blacklist')
                        : t('security.add_to_whitelist', 'Add to Whitelist')}
                    </h3>
                    <p className="text-xs text-zinc-500">
                      {isBlacklist
                        ? t('security.block_ip_desc', 'Block IP from accessing the proxy')
                        : t('security.allow_ip_desc', 'Allow IP access in strict mode')}
                    </p>
                  </div>
                </div>
                <button
                  onClick={handleClose}
                  className="p-2 rounded-lg text-zinc-500 hover:text-white hover:bg-white/5 transition-colors"
                >
                  <X className="w-5 h-5" />
                </button>
              </div>

              {/* Form */}
              <form onSubmit={handleSubmit} className="relative z-10 p-5 space-y-4">
                {/* IP Pattern */}
                <div>
                  <label className="block text-sm font-medium text-zinc-300 mb-2">
                    {t('security.ip_pattern', 'IP Address or CIDR')}
                  </label>
                  <input
                    type="text"
                    value={ipPattern}
                    onChange={(e) => {
                      setIpPattern(e.target.value);
                      if (ipError) validateIp(e.target.value);
                    }}
                    onBlur={() => validateIp(ipPattern)}
                    placeholder="192.168.1.100 or 10.0.0.0/24"
                    className={`w-full px-4 py-3 bg-zinc-800/50 border rounded-xl text-white placeholder-zinc-600 focus:outline-none focus:ring-2 transition-all font-mono ${
                      ipError
                        ? 'border-red-500/50 focus:ring-red-500/30'
                        : 'border-white/10 focus:ring-blue-500/30 focus:border-blue-500/50'
                    }`}
                  />
                  {ipError && (
                    <div className="flex items-center gap-1.5 mt-2 text-xs text-red-400">
                      <AlertTriangle className="w-3.5 h-3.5" />
                      {ipError}
                    </div>
                  )}
                </div>

                {/* Reason (blacklist) or Description (whitelist) */}
                <div>
                  <label className="block text-sm font-medium text-zinc-300 mb-2">
                    {isBlacklist
                      ? t('security.reason', 'Reason')
                      : t('security.description', 'Description')}
                    <span className="text-zinc-600 ml-1">
                      ({t('common.optional', 'optional')})
                    </span>
                  </label>
                  <input
                    type="text"
                    value={isBlacklist ? reason : description}
                    onChange={(e) =>
                      isBlacklist ? setReason(e.target.value) : setDescription(e.target.value)
                    }
                    placeholder={
                      isBlacklist
                        ? t('security.reason_placeholder', 'e.g., Suspicious activity')
                        : t('security.description_placeholder', 'e.g., Office network')
                    }
                    className="w-full px-4 py-3 bg-zinc-800/50 border border-white/10 rounded-xl text-white placeholder-zinc-600 focus:outline-none focus:ring-2 focus:ring-blue-500/30 focus:border-blue-500/50 transition-all"
                  />
                </div>

                {/* Duration (blacklist only) */}
                {isBlacklist && (
                  <div>
                    <label className="block text-sm font-medium text-zinc-300 mb-2">
                      <Clock className="w-4 h-4 inline mr-1.5" />
                      {t('security.block_duration', 'Block Duration')}
                    </label>
                    <div className="grid grid-cols-3 gap-2">
                      {DURATION_OPTIONS.map((option) => (
                        <button
                          key={option.value}
                          type="button"
                          onClick={() => setDuration(option.value)}
                          className={`px-3 py-2 text-sm rounded-lg border transition-all ${
                            duration === option.value
                              ? 'bg-red-500/20 border-red-500/50 text-red-300'
                              : 'bg-zinc-800/30 border-white/5 text-zinc-400 hover:border-white/20'
                          }`}
                        >
                          {option.label}
                        </button>
                      ))}
                    </div>
                  </div>
                )}

                {/* Actions */}
                <div className="flex gap-3 pt-2">
                  <button
                    type="button"
                    onClick={handleClose}
                    className="flex-1 px-4 py-3 bg-zinc-800 text-zinc-300 rounded-xl hover:bg-zinc-700 transition-colors"
                  >
                    {t('common.cancel', 'Cancel')}
                  </button>
                  <button
                    type="submit"
                    disabled={isSubmitting || !ipPattern.trim()}
                    className={`flex-1 px-4 py-3 font-bold rounded-xl shadow-lg transition-all disabled:opacity-50 disabled:cursor-not-allowed ${
                      isBlacklist
                        ? 'bg-gradient-to-r from-red-500 to-orange-500 text-white shadow-red-500/25 hover:shadow-red-500/40'
                        : 'bg-gradient-to-r from-emerald-500 to-teal-500 text-white shadow-emerald-500/25 hover:shadow-emerald-500/40'
                    }`}
                  >
                    {isSubmitting ? (
                      <span className="flex items-center justify-center gap-2">
                        <div className="w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin" />
                        {t('common.adding', 'Adding...')}
                      </span>
                    ) : isBlacklist ? (
                      t('security.block_ip', 'Block IP')
                    ) : (
                      t('security.allow_ip', 'Allow IP')
                    )}
                  </button>
                </div>
              </form>
            </div>
          </motion.div>
        </>
      )}
    </AnimatePresence>
  );
};
