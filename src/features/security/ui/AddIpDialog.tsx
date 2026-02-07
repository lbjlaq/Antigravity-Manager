// File: src/features/security/ui/AddIpDialog.tsx
// Dialog for adding IP to blacklist/whitelist with glassmorphism style

import React, { useState, useEffect, useCallback } from 'react';
import { createPortal } from 'react-dom';
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
  { label: '10m', value: 600 },
  { label: '1h', value: 3600 },
  { label: '24h', value: 86400 },
  { label: '7d', value: 604800 },
  { label: '30d', value: 2592000 },
  { label: '∞', value: 0 },
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

  // Handle Escape key
  const handleKeyDown = useCallback((e: KeyboardEvent) => {
    if (e.key === 'Escape') {
      onClose();
    }
  }, [onClose]);

  useEffect(() => {
    if (isOpen) {
      document.addEventListener('keydown', handleKeyDown);
      // Prevent body scroll when modal is open
      document.body.style.overflow = 'hidden';
      return () => {
        document.removeEventListener('keydown', handleKeyDown);
        document.body.style.overflow = '';
      };
    }
  }, [isOpen, handleKeyDown]);

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

  return createPortal(
    <AnimatePresence>
      {isOpen && (
        <div className="fixed inset-0 z-[100] flex items-center justify-center p-4">
          {/* Backdrop */}
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.15 }}
            onClick={handleClose}
            className="absolute inset-0 bg-black/60 backdrop-blur-sm"
          />

          {/* Tauri drag region */}
          <div data-tauri-drag-region className="fixed top-0 left-0 right-0 h-8 z-[110]" />

          {/* Dialog */}
          <motion.div
            initial={{ opacity: 0, scale: 0.95, y: 20 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            exit={{ opacity: 0, scale: 0.95, y: 20 }}
            transition={{ type: 'spring', damping: 25, stiffness: 300 }}
            className="relative w-full max-w-md"
          >
            <div className="relative bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 rounded-xl shadow-2xl overflow-hidden">
              {/* Subtle glow effects */}
              <div
                className={`absolute -top-20 -right-20 w-40 h-40 rounded-full blur-3xl pointer-events-none opacity-30 ${
                  isBlacklist ? 'bg-rose-500' : 'bg-emerald-500'
                }`}
              />

              {/* Header */}
              <div className="relative z-10 flex items-center justify-between p-4 border-b border-zinc-200 dark:border-zinc-800">
                <div className="flex items-center gap-3">
                  <div
                    className={`p-2 rounded-lg ${
                      isBlacklist
                        ? 'bg-rose-100 dark:bg-rose-900/30 text-rose-600 dark:text-rose-400'
                        : 'bg-emerald-100 dark:bg-emerald-900/30 text-emerald-600 dark:text-emerald-400'
                    }`}
                  >
                    {isBlacklist ? (
                      <Shield className="w-5 h-5" />
                    ) : (
                      <ShieldCheck className="w-5 h-5" />
                    )}
                  </div>
                  <div>
                    <h3 className="font-semibold text-zinc-900 dark:text-white">
                      {isBlacklist
                        ? t('security.add_to_blacklist', 'Add to Blacklist')
                        : t('security.add_to_whitelist', 'Add to Whitelist')}
                    </h3>
                    <p className="text-xs text-zinc-500 dark:text-zinc-400">
                      {isBlacklist
                        ? t('security.block_ip_desc', 'Block IP from accessing the proxy')
                        : t('security.allow_ip_desc', 'Allow IP access in strict mode')}
                    </p>
                  </div>
                </div>
                <button
                  onClick={handleClose}
                  className="p-2 rounded-lg text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-300 hover:bg-zinc-100 dark:hover:bg-zinc-800 transition-colors"
                >
                  <X className="w-5 h-5" />
                </button>
              </div>

              {/* Form */}
              <form onSubmit={handleSubmit} className="relative z-10 p-4 space-y-4">
                {/* IP Pattern */}
                <div>
                  <label className="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1.5">
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
                    className={`w-full px-3 py-2.5 bg-zinc-50 dark:bg-zinc-800 border rounded-lg text-zinc-900 dark:text-white placeholder-zinc-400 dark:placeholder-zinc-500 focus:outline-none focus:ring-2 transition-all font-mono text-sm ${
                      ipError
                        ? 'border-rose-300 dark:border-rose-500/50 focus:ring-rose-500/30'
                        : 'border-zinc-200 dark:border-zinc-700 focus:ring-indigo-500/30 focus:border-indigo-500'
                    }`}
                  />
                  {ipError && (
                    <div className="flex items-center gap-1.5 mt-1.5 text-xs text-rose-600 dark:text-rose-400">
                      <AlertTriangle className="w-3.5 h-3.5" />
                      {ipError}
                    </div>
                  )}
                </div>

                {/* Reason (blacklist) or Description (whitelist) */}
                <div>
                  <label className="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1.5">
                    {isBlacklist
                      ? t('security.reason', 'Reason')
                      : t('security.description', 'Description')}
                    <span className="text-zinc-400 dark:text-zinc-500 font-normal ml-1">
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
                    className="w-full px-3 py-2.5 bg-zinc-50 dark:bg-zinc-800 border border-zinc-200 dark:border-zinc-700 rounded-lg text-zinc-900 dark:text-white placeholder-zinc-400 dark:placeholder-zinc-500 focus:outline-none focus:ring-2 focus:ring-indigo-500/30 focus:border-indigo-500 transition-all text-sm"
                  />
                </div>

                {/* Duration (blacklist only) */}
                {isBlacklist && (
                  <div>
                    <label className="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1.5">
                      <Clock className="w-4 h-4 inline mr-1.5 -mt-0.5" />
                      {t('security.block_duration', 'Block Duration')}
                    </label>
                    <div className="grid grid-cols-6 gap-1.5">
                      {DURATION_OPTIONS.map((option) => (
                        <button
                          key={option.value}
                          type="button"
                          onClick={() => setDuration(option.value)}
                          className={`px-2 py-2 text-xs font-medium rounded-lg border transition-all ${
                            duration === option.value
                              ? 'bg-rose-100 dark:bg-rose-900/30 border-rose-300 dark:border-rose-500/50 text-rose-700 dark:text-rose-300'
                              : 'bg-zinc-50 dark:bg-zinc-800 border-zinc-200 dark:border-zinc-700 text-zinc-600 dark:text-zinc-400 hover:border-zinc-300 dark:hover:border-zinc-600'
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
                    className="flex-1 px-4 py-2.5 bg-zinc-100 dark:bg-zinc-800 text-zinc-700 dark:text-zinc-300 rounded-lg hover:bg-zinc-200 dark:hover:bg-zinc-700 transition-colors font-medium text-sm"
                  >
                    {t('common.cancel', 'Cancel')}
                  </button>
                  <button
                    type="submit"
                    disabled={isSubmitting || !ipPattern.trim()}
                    className={`flex-1 px-4 py-2.5 font-medium text-sm text-white rounded-lg transition-all disabled:opacity-50 disabled:cursor-not-allowed ${
                      isBlacklist
                        ? 'bg-rose-600 hover:bg-rose-500 focus:ring-2 focus:ring-rose-500/50'
                        : 'bg-emerald-600 hover:bg-emerald-500 focus:ring-2 focus:ring-emerald-500/50'
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
        </div>
      )}
    </AnimatePresence>,
    document.body
  );
};
