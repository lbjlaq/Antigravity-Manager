// File: src/pages/api-proxy/ui/LoadingState.tsx
// Loading and error states for API Proxy page

import { useTranslation } from 'react-i18next';
import { RefreshCw, Settings } from 'lucide-react';

export function LoadingSpinner() {
    const { t } = useTranslation();
    
    return (
        <div className="flex items-center justify-center py-20">
            <div className="flex flex-col items-center gap-4">
                <RefreshCw size={32} className="animate-spin text-blue-500" />
                <span className="text-sm text-gray-500 dark:text-gray-400">
                    {t('common.loading') || 'Loading...'}
                </span>
            </div>
        </div>
    );
}

interface ErrorStateProps {
    error: string;
    onRetry: () => void;
}

export function ErrorState({ error, onRetry }: ErrorStateProps) {
    const { t } = useTranslation();
    
    return (
        <div className="flex items-center justify-center py-20">
            <div className="flex flex-col items-center gap-4 text-center">
                <div className="w-16 h-16 rounded-full bg-red-100 dark:bg-red-900/30 flex items-center justify-center">
                    <Settings size={32} className="text-red-500" />
                </div>
                <div className="space-y-2">
                    <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100">
                        {t('proxy.error.load_failed') || 'Failed to load configuration'}
                    </h3>
                    <p className="text-sm text-gray-500 dark:text-gray-400 max-w-md">
                        {error}
                    </p>
                </div>
                <button
                    onClick={onRetry}
                    className="px-4 py-2 bg-blue-600 hover:bg-blue-700 text-white rounded-lg text-sm font-medium flex items-center gap-2 transition-colors"
                >
                    <RefreshCw size={16} />
                    {t('common.retry') || 'Retry'}
                </button>
            </div>
        </div>
    );
}
