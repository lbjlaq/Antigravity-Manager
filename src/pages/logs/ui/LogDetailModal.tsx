// File: src/pages/logs/ui/LogDetailModal.tsx
// Log detail modal component

import { memo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { motion, AnimatePresence } from 'framer-motion';
import { X, Copy, Check } from 'lucide-react';

import { formatCompactNumber, copyToClipboard } from '@/shared/lib';
import type { ProxyRequestLog } from '../model';

interface LogDetailModalProps {
  log: ProxyRequestLog | null;
  loading: boolean;
  onClose: () => void;
}

export const LogDetailModal = memo(function LogDetailModal({
  log,
  loading,
  onClose,
}: LogDetailModalProps) {
  const { t } = useTranslation();
  const [copiedField, setCopiedField] = useState<string | null>(null);

  const handleCopy = async (content: string, field: string) => {
    const success = await copyToClipboard(content);
    if (success) {
      setCopiedField(field);
      setTimeout(() => setCopiedField(null), 2000);
    }
  };

  const formatBody = (body?: string) => {
    if (!body) {
      return <span className="text-zinc-400 italic">{t('logs.detail.empty', 'No content')}</span>;
    }
    try {
      const obj = JSON.parse(body);
      return (
        <pre className="text-xs font-mono whitespace-pre-wrap text-zinc-700 dark:text-zinc-300 overflow-x-auto">
          {JSON.stringify(obj, null, 2)}
        </pre>
      );
    } catch {
      return (
        <pre className="text-xs font-mono whitespace-pre-wrap text-zinc-700 dark:text-zinc-300 overflow-x-auto">
          {body}
        </pre>
      );
    }
  };

  const getStatusColor = (status: number) => {
    if (status >= 200 && status < 300) return 'bg-green-500';
    if (status >= 300 && status < 400) return 'bg-yellow-500';
    return 'bg-red-500';
  };

  const getProtocolColor = (protocol?: string) => {
    const colors: Record<string, string> = {
      openai: 'bg-emerald-100 text-emerald-700 dark:bg-emerald-900/40 dark:text-emerald-400 border-emerald-200 dark:border-emerald-800/50',
      anthropic: 'bg-orange-100 text-orange-700 dark:bg-orange-900/40 dark:text-orange-400 border-orange-200 dark:border-orange-800/50',
      gemini: 'bg-blue-100 text-blue-700 dark:bg-blue-900/40 dark:text-blue-400 border-blue-200 dark:border-blue-800/50',
    };
    return colors[protocol || ''] || 'bg-zinc-100 text-zinc-700 dark:bg-zinc-800 dark:text-zinc-400';
  };

  return (
    <AnimatePresence>
      {log && (
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm p-4"
          onClick={onClose}
        >
          <motion.div
            initial={{ scale: 0.95, opacity: 0 }}
            animate={{ scale: 1, opacity: 1 }}
            exit={{ scale: 0.95, opacity: 0 }}
            transition={{ duration: 0.2 }}
            className="bg-white dark:bg-zinc-900 rounded-xl shadow-2xl w-full max-w-4xl max-h-[90vh] flex flex-col overflow-hidden border border-zinc-200 dark:border-zinc-800"
            onClick={(e) => e.stopPropagation()}
          >
            {/* Header */}
            <div className="px-5 py-4 border-b border-zinc-200 dark:border-zinc-800 flex items-center justify-between bg-zinc-50 dark:bg-zinc-800/50">
              <div className="flex items-center gap-3">
                {loading && <div className="w-4 h-4 border-2 border-purple-500 border-t-transparent rounded-full animate-spin" />}
                <span className={`px-2.5 py-1 text-xs font-bold text-white rounded ${getStatusColor(log.status)}`}>
                  {log.status}
                </span>
                <span className="font-mono font-bold text-zinc-900 dark:text-zinc-100">
                  {log.method}
                </span>
                <span className="text-sm text-zinc-500 dark:text-zinc-400 font-mono truncate max-w-md hidden sm:inline">
                  {log.url}
                </span>
              </div>
              <button
                onClick={onClose}
                className="p-2 text-zinc-500 hover:text-zinc-700 dark:hover:text-zinc-300 hover:bg-zinc-100 dark:hover:bg-zinc-800 rounded-lg transition-colors"
              >
                <X className="w-5 h-5" />
              </button>
            </div>

            {/* Content */}
            <div className="flex-1 overflow-y-auto p-5 space-y-6">
              {/* Metadata */}
              <div className="bg-zinc-50 dark:bg-zinc-800/50 p-5 rounded-xl border border-zinc-200 dark:border-zinc-700">
                <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-5">
                  <div className="space-y-1">
                    <span className="block text-zinc-500 dark:text-zinc-400 uppercase font-bold text-[10px] tracking-widest">
                      {t('logs.detail.time', 'Time')}
                    </span>
                    <span className="font-mono font-medium text-zinc-900 dark:text-zinc-100 text-sm">
                      {new Date(log.timestamp).toLocaleString()}
                    </span>
                  </div>
                  <div className="space-y-1">
                    <span className="block text-zinc-500 dark:text-zinc-400 uppercase font-bold text-[10px] tracking-widest">
                      {t('logs.detail.duration', 'Duration')}
                    </span>
                    <span className="font-mono font-medium text-zinc-900 dark:text-zinc-100 text-sm">
                      {log.duration}ms
                    </span>
                  </div>
                  <div className="space-y-1">
                    <span className="block text-zinc-500 dark:text-zinc-400 uppercase font-bold text-[10px] tracking-widest">
                      {t('logs.detail.tokens', 'Tokens')}
                    </span>
                    <div className="flex gap-2 text-xs font-mono">
                      <span className="px-2 py-1 rounded bg-blue-100 dark:bg-blue-900/40 text-blue-700 dark:text-blue-300 border border-blue-200 dark:border-blue-800/50 font-bold">
                        In: {formatCompactNumber(log.input_tokens ?? 0)}
                      </span>
                      <span className="px-2 py-1 rounded bg-green-100 dark:bg-green-900/40 text-green-700 dark:text-green-300 border border-green-200 dark:border-green-800/50 font-bold">
                        Out: {formatCompactNumber(log.output_tokens ?? 0)}
                      </span>
                    </div>
                  </div>
                </div>

                <div className="mt-5 pt-5 border-t border-zinc-200 dark:border-zinc-700">
                  <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-5">
                    {log.protocol && (
                      <div className="space-y-1">
                        <span className="block text-zinc-500 dark:text-zinc-400 uppercase font-bold text-[10px] tracking-widest">
                          {t('logs.detail.protocol', 'Protocol')}
                        </span>
                        <span className={`inline-block px-2.5 py-1 rounded font-mono font-bold text-xs uppercase border ${getProtocolColor(log.protocol)}`}>
                          {log.protocol}
                        </span>
                      </div>
                    )}
                    <div className="space-y-1">
                      <span className="block text-zinc-500 dark:text-zinc-400 uppercase font-bold text-[10px] tracking-widest">
                        {t('logs.detail.model', 'Model')}
                      </span>
                      <span className="font-mono font-bold text-purple-600 dark:text-purple-400 text-sm">
                        {log.model || '-'}
                      </span>
                    </div>
                    {log.mapped_model && log.model !== log.mapped_model && (
                      <div className="space-y-1">
                        <span className="block text-zinc-500 dark:text-zinc-400 uppercase font-bold text-[10px] tracking-widest">
                          {t('logs.detail.mapped_model', 'Mapped To')}
                        </span>
                        <span className="font-mono font-bold text-green-600 dark:text-green-400 text-sm">
                          {log.mapped_model}
                        </span>
                      </div>
                    )}
                  </div>
                </div>

                {log.account_email && (
                  <div className="mt-5 pt-5 border-t border-zinc-200 dark:border-zinc-700">
                    <span className="block text-zinc-500 dark:text-zinc-400 uppercase font-bold text-[10px] tracking-widest mb-2">
                      {t('logs.detail.account', 'Account Used')}
                    </span>
                    <span className="font-mono font-medium text-zinc-900 dark:text-zinc-100 text-sm">
                      {log.account_email}
                    </span>
                  </div>
                )}
              </div>

              {/* Request/Response Payloads */}
              <div className="space-y-4">
                {/* Request */}
                <div>
                  <div className="flex items-center justify-between mb-2">
                    <h3 className="text-xs font-bold uppercase text-zinc-500 dark:text-zinc-400">
                      {t('logs.detail.request', 'Request Payload')}
                    </h3>
                    {log.request_body && (
                      <button
                        onClick={() => handleCopy(log.request_body!, 'request')}
                        className="flex items-center gap-1 px-2 py-1 text-xs text-zinc-500 hover:text-zinc-700 dark:hover:text-zinc-300 hover:bg-zinc-100 dark:hover:bg-zinc-800 rounded transition-colors"
                      >
                        {copiedField === 'request' ? <Check className="w-3 h-3 text-green-500" /> : <Copy className="w-3 h-3" />}
                        {copiedField === 'request' ? t('common.copied', 'Copied') : t('common.copy', 'Copy')}
                      </button>
                    )}
                  </div>
                  <div className="bg-zinc-50 dark:bg-zinc-800/50 rounded-lg p-4 border border-zinc-200 dark:border-zinc-700 max-h-64 overflow-y-auto">
                    {formatBody(log.request_body)}
                  </div>
                </div>

                {/* Response */}
                <div>
                  <div className="flex items-center justify-between mb-2">
                    <h3 className="text-xs font-bold uppercase text-zinc-500 dark:text-zinc-400">
                      {t('logs.detail.response', 'Response Payload')}
                    </h3>
                    {log.response_body && (
                      <button
                        onClick={() => handleCopy(log.response_body!, 'response')}
                        className="flex items-center gap-1 px-2 py-1 text-xs text-zinc-500 hover:text-zinc-700 dark:hover:text-zinc-300 hover:bg-zinc-100 dark:hover:bg-zinc-800 rounded transition-colors"
                      >
                        {copiedField === 'response' ? <Check className="w-3 h-3 text-green-500" /> : <Copy className="w-3 h-3" />}
                        {copiedField === 'response' ? t('common.copied', 'Copied') : t('common.copy', 'Copy')}
                      </button>
                    )}
                  </div>
                  <div className="bg-zinc-50 dark:bg-zinc-800/50 rounded-lg p-4 border border-zinc-200 dark:border-zinc-700 max-h-64 overflow-y-auto">
                    {formatBody(log.response_body)}
                  </div>
                </div>
              </div>
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
});
