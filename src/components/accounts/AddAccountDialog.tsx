import { useState, useEffect, useRef, memo } from 'react';
import { createPortal } from 'react-dom';
import { 
    Plus, Database, Globe, Key, Loader2, CheckCircle2, XCircle, 
    Copy, Check, Link2, Sparkles, Upload, FolderOpen, X
} from 'lucide-react';
import { motion, AnimatePresence } from 'framer-motion';
import { useTranslation } from 'react-i18next';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';

// FSD imports
import { invoke } from '@/shared/api';
import { isTauri, cn, copyToClipboard } from '@/shared/lib';
import { 
    useAccounts,
    useStartOAuthLogin, 
    useCompleteOAuthLogin, 
    useCancelOAuthLogin, 
    useImportFromDb, 
    useImportV1Accounts, 
    useImportFromCustomDb 
} from '@/features/accounts';

interface AddAccountDialogProps {
    onAdd: (email: string, refreshToken: string) => Promise<void>;
    children?: React.ReactNode;
}

type Status = 'idle' | 'loading' | 'success' | 'error';
type TabType = 'oauth' | 'token' | 'import';

// Tab configuration with icons and labels
const TABS: { id: TabType; icon: typeof Globe; labelKey: string; recommended?: boolean }[] = [
    { id: 'oauth', icon: Globe, labelKey: 'accounts.add.tabs.oauth', recommended: true },
    { id: 'token', icon: Key, labelKey: 'accounts.add.tabs.token' },
    { id: 'import', icon: Database, labelKey: 'accounts.add.tabs.import' },
];

// Status Alert Component
const StatusAlert = memo(({ status, message }: { status: Status; message: string }) => {
    if (status === 'idle' || !message) return null;

    const config = {
        loading: { 
            bg: 'bg-blue-500/10 border-blue-500/30', 
            text: 'text-blue-400',
            icon: <Loader2 className="w-4 h-4 animate-spin" />
        },
        success: { 
            bg: 'bg-emerald-500/10 border-emerald-500/30', 
            text: 'text-emerald-400',
            icon: <CheckCircle2 className="w-4 h-4" />
        },
        error: { 
            bg: 'bg-red-500/10 border-red-500/30', 
            text: 'text-red-400',
            icon: <XCircle className="w-4 h-4" />
        },
    };

    const { bg, text, icon } = config[status];

    return (
        <motion.div 
            initial={{ opacity: 0, y: -10 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -10 }}
            className={cn("flex items-center gap-3 p-3 rounded-xl border text-sm", bg, text)}
        >
            {icon}
            <span className="flex-1">{message}</span>
        </motion.div>
    );
});

StatusAlert.displayName = 'StatusAlert';

function AddAccountDialog({ onAdd, children }: AddAccountDialogProps) {
    const { t } = useTranslation();
    
    // FSD hooks
    const { refetch: fetchAccounts } = useAccounts();
    const startOAuthMutation = useStartOAuthLogin();
    const completeOAuthMutation = useCompleteOAuthLogin();
    const cancelOAuthMutation = useCancelOAuthLogin();
    const importFromDbMutation = useImportFromDb();
    const importV1Mutation = useImportV1Accounts();
    const importCustomDbMutation = useImportFromCustomDb();
    
    const [isOpen, setIsOpen] = useState(false);
    const [activeTab, setActiveTab] = useState<TabType>(isTauri() ? 'oauth' : 'token');
    const [refreshToken, setRefreshToken] = useState('');
    const [oauthUrl, setOauthUrl] = useState('');
    const [oauthUrlCopied, setOauthUrlCopied] = useState(false);
    const [manualCode, setManualCode] = useState('');

    // UI State
    const [status, setStatus] = useState<Status>('idle');
    const [message, setMessage] = useState('');

    const oauthUrlRef = useRef(oauthUrl);
    const statusRef = useRef(status);
    const activeTabRef = useRef(activeTab);
    const isOpenRef = useRef(isOpen);

    useEffect(() => {
        oauthUrlRef.current = oauthUrl;
        statusRef.current = status;
        activeTabRef.current = activeTab;
        isOpenRef.current = isOpen;
    }, [oauthUrl, status, activeTab, isOpen]);

    // Reset state when dialog opens or tab changes
    useEffect(() => {
        if (isOpen) {
            resetState();
        }
    }, [isOpen, activeTab]);

    // Listen for OAuth URL
    useEffect(() => {
        if (!isTauri()) return;
        let unlisten: (() => void) | undefined;

        const setupListener = async () => {
            unlisten = await listen('oauth-url-generated', (event) => {
                setOauthUrl(event.payload as string);
            });
        };

        setupListener();
        return () => { if (unlisten) unlisten(); };
    }, []);

    // Listen for OAuth callback completion
    useEffect(() => {
        if (!isTauri()) return;
        let unlisten: (() => void) | undefined;

        const setupListener = async () => {
            unlisten = await listen('oauth-callback-received', async () => {
                if (!isOpenRef.current) return;
                if (activeTabRef.current !== 'oauth') return;
                if (statusRef.current === 'loading' || statusRef.current === 'success') return;
                if (!oauthUrlRef.current) return;

                setStatus('loading');
                setMessage(`${t('accounts.add.tabs.oauth')}...`);

                try {
                    await completeOAuthMutation.mutateAsync();
                    setStatus('success');
                    setMessage(`${t('accounts.add.tabs.oauth')} ${t('common.success')}!`);
                    setTimeout(() => {
                        setIsOpen(false);
                        resetState();
                    }, 1500);
                } catch (error) {
                    setStatus('error');
                    const errorMsg = String(error);
                    if (errorMsg.includes('Refresh Token') || errorMsg.includes('refresh_token')) {
                        setMessage(errorMsg);
                    } else {
                        setMessage(`${t('accounts.add.tabs.oauth')} ${t('common.error')}: ${errorMsg}`);
                    }
                }
            });
        };

        setupListener();
        return () => { if (unlisten) unlisten(); };
    }, [completeOAuthMutation, t]);

    // Pre-generate OAuth URL
    useEffect(() => {
        if (!isOpen || activeTab !== 'oauth' || oauthUrl) return;

        invoke<any>('prepare_oauth_url')
            .then((res) => {
                const url = typeof res === 'string' ? res : res?.url;
                if (url && url.length > 0) setOauthUrl(url);
            })
            .catch(console.error);
    }, [isOpen, activeTab, oauthUrl]);

    // Cancel OAuth when leaving tab
    useEffect(() => {
        if (!isOpen || activeTab === 'oauth' || !oauthUrl) return;
        cancelOAuthMutation.mutate();
        setOauthUrl('');
        setOauthUrlCopied(false);
    }, [isOpen, activeTab, cancelOAuthMutation, oauthUrl]);

    const resetState = () => {
        setStatus('idle');
        setMessage('');
        setRefreshToken('');
        setOauthUrl('');
        setOauthUrlCopied(false);
        setManualCode('');
    };

    const handleAction = async (
        actionName: string,
        actionFn: () => Promise<any>,
        options?: { clearOauthUrl?: boolean }
    ) => {
        setStatus('loading');
        setMessage(`${actionName}...`);
        if (options?.clearOauthUrl !== false) setOauthUrl('');
        
        try {
            await actionFn();
            setStatus('success');
            setMessage(`${actionName} ${t('common.success')}!`);
            setTimeout(() => {
                setIsOpen(false);
                resetState();
            }, 1500);
        } catch (error) {
            setStatus('error');
            const errorMsg = String(error);
            if (errorMsg.includes('Refresh Token') || errorMsg.includes('refresh_token')) {
                setMessage(errorMsg);
            } else {
                setMessage(`${actionName} ${t('common.error')}: ${errorMsg}`);
            }
        }
    };

    const handleSubmit = async () => {
        if (!refreshToken) {
            setStatus('error');
            setMessage(t('accounts.add.token.error_token'));
            return;
        }

        setStatus('loading');
        const input = refreshToken.trim();
        let tokens: string[] = [];

        // Parse JSON or regex
        try {
            if (input.startsWith('[') && input.endsWith(']')) {
                const parsed = JSON.parse(input);
                if (Array.isArray(parsed)) {
                    tokens = parsed
                        .map((item: any) => item.refresh_token)
                        .filter((t: any) => typeof t === 'string' && t.startsWith('1//'));
                }
            }
        } catch (e) {
            console.debug('JSON parse failed, falling back to regex');
        }

        if (tokens.length === 0) {
            const matches = input.match(/1\/\/[a-zA-Z0-9_\-]+/g);
            if (matches) tokens = matches;
        }

        tokens = [...new Set(tokens)];

        if (tokens.length === 0) {
            setStatus('error');
            setMessage(t('accounts.add.token.error_token'));
            return;
        }

        let successCount = 0;
        let failCount = 0;

        for (let i = 0; i < tokens.length; i++) {
            setMessage(t('accounts.add.token.batch_progress', { current: i + 1, total: tokens.length }));
            try {
                await onAdd("", tokens[i]);
                successCount++;
            } catch {
                failCount++;
            }
            await new Promise(r => setTimeout(r, 100));
        }

        if (successCount === tokens.length) {
            setStatus('success');
            setMessage(t('accounts.add.token.batch_success', { count: successCount }));
            setTimeout(() => {
                setIsOpen(false);
                resetState();
            }, 1500);
        } else if (successCount > 0) {
            setStatus('success');
            setMessage(t('accounts.add.token.batch_partial', { success: successCount, fail: failCount }));
        } else {
            setStatus('error');
            setMessage(t('accounts.add.token.batch_fail'));
        }
    };

    const handleOAuthWeb = async () => {
        try {
            setStatus('loading');
            setMessage(t('accounts.add.oauth.btn_start') + '...');

            const res = await invoke<any>('prepare_oauth_url');
            const url = typeof res === 'string' ? res : res.url;
            if (!url) throw new Error('Could not obtain OAuth URL');

            setOauthUrl(url);
            const popup = window.open(url, '_blank');

            if (!popup) {
                setStatus('error');
                setMessage(t('common.error') + ': Popup blocked');
                return;
            }

            const handleMessage = async (event: MessageEvent) => {
                if (event.data?.type === 'oauth-success') {
                    popup.close();
                    window.removeEventListener('message', handleMessage);
                    await fetchAccounts();
                    setStatus('success');
                    setMessage(t('accounts.add.oauth_success') || t('common.success'));
                    setTimeout(() => {
                        setIsOpen(false);
                        resetState();
                    }, 1500);
                }
            };

            window.addEventListener('message', handleMessage);

            const timer = setInterval(() => {
                if (popup.closed) {
                    clearInterval(timer);
                    window.removeEventListener('message', handleMessage);
                    if (statusRef.current === 'loading') {
                        setStatus('idle');
                        setMessage('');
                    }
                }
            }, 1000);
        } catch (error) {
            setStatus('error');
            setMessage(`${t('common.error')}: ${error}`);
        }
    };

    const handleOAuth = () => {
        if (!isTauri()) {
            handleOAuthWeb();
            return;
        }
        handleAction(t('accounts.add.tabs.oauth'), () => startOAuthMutation.mutateAsync(), { clearOauthUrl: false });
    };

    const handleCompleteOAuth = () => {
        handleAction(t('accounts.add.tabs.oauth'), () => completeOAuthMutation.mutateAsync(), { clearOauthUrl: false });
    };

    const handleCopyUrl = async () => {
        if (oauthUrl) {
            const success = await copyToClipboard(oauthUrl);
            if (success) {
                setOauthUrlCopied(true);
                setTimeout(() => setOauthUrlCopied(false), 1500);
            }
        }
    };

    const handleManualSubmit = async () => {
        if (!manualCode.trim()) return;

        setStatus('loading');
        setMessage(t('accounts.add.oauth.manual_submitting', 'Submitting code...'));

        try {
            await invoke('submit_oauth_code', { code: manualCode.trim(), state: null });
            setStatus('success');
            setMessage(t('accounts.add.oauth.manual_submitted', 'Code submitted!'));
            setManualCode('');

            if (!isTauri()) {
                setTimeout(async () => {
                    await fetchAccounts();
                    setIsOpen(false);
                    resetState();
                }, 2000);
            }
        } catch (error) {
            const errStr = String(error);
            if (errStr.includes("No active OAuth flow")) {
                setMessage(t('accounts.add.oauth.error_no_flow'));
            } else {
                setMessage(`${t('common.error')}: ${errStr}`);
            }
            setStatus('error');
        }
    };

    const handleImportDb = () => handleAction(t('accounts.add.tabs.import'), () => importFromDbMutation.mutateAsync());
    const handleImportV1 = () => handleAction(t('accounts.add.import.btn_v1'), () => importV1Mutation.mutateAsync());

    const handleImportCustomDb = async () => {
        if (!isTauri()) {
            alert(t('common.tauri_api_not_loaded') || 'Desktop app required');
            return;
        }
        try {
            const selected = await open({
                multiple: false,
                filters: [
                    { name: 'VSCode DB', extensions: ['vscdb'] },
                    { name: 'All Files', extensions: ['*'] }
                ]
            });
            if (selected && typeof selected === 'string') {
                handleAction(t('accounts.add.import.btn_custom_db') || 'Import Custom DB', () => importCustomDbMutation.mutateAsync(selected));
            }
        } catch (err) {
            console.error('Failed to open dialog:', err);
        }
    };

    const isDisabled = status === 'loading' || status === 'success';

    return (
        <>
            {children ? (
                <div 
                    onClick={(e) => {
                        e.preventDefault();
                        e.stopPropagation();
                        console.log('[AddAccountDialog] children wrapper clicked');
                        setIsOpen(true);
                    }} 
                    className="inline-block cursor-pointer [&>*]:pointer-events-none"
                    role="button"
                    tabIndex={0}
                    onKeyDown={(e) => {
                        if (e.key === 'Enter' || e.key === ' ') {
                            e.preventDefault();
                            setIsOpen(true);
                        }
                    }}
                >
                    {children}
                </div>
            ) : (
                <button
                    type="button"
                    className="px-4 py-2 flex items-center gap-2 rounded-lg border border-zinc-300 dark:border-zinc-700 bg-white dark:bg-zinc-800 text-zinc-600 dark:text-zinc-400 text-sm font-medium hover:bg-zinc-100 dark:hover:bg-zinc-700 hover:text-zinc-900 dark:hover:text-white transition-colors"
                    onClick={(e) => {
                        e.preventDefault();
                        e.stopPropagation();
                        console.log('[AddAccountDialog] button clicked');
                        setIsOpen(true);
                    }}
                >
                    <Plus className="w-4 h-4" />
                    {t('accounts.add_account')}
                </button>
            )}

            {isOpen && createPortal(
                <AnimatePresence>
                    <motion.div
                        key="dialog-overlay"
                        initial={{ opacity: 0 }}
                        animate={{ opacity: 1 }}
                        exit={{ opacity: 0 }}
                        className="fixed inset-0 z-[99999] flex items-center justify-center"
                    >
                        {/* Backdrop */}
                        <motion.div 
                            initial={{ opacity: 0 }}
                            animate={{ opacity: 1 }}
                            exit={{ opacity: 0 }}
                            className="absolute inset-0 bg-black/60 backdrop-blur-sm"
                            onClick={() => !isDisabled && setIsOpen(false)}
                        />

                        {/* Draggable region */}
                        <div data-tauri-drag-region className="fixed top-0 left-0 right-0 h-8 z-[1]" />

                        {/* Dialog */}
                        <motion.div
                            initial={{ opacity: 0, scale: 0.95, y: 20 }}
                            animate={{ opacity: 1, scale: 1, y: 0 }}
                            exit={{ opacity: 0, scale: 0.95, y: 20 }}
                            transition={{ type: 'spring', damping: 25, stiffness: 300 }}
                            className="relative z-10 w-full max-w-lg m-4 max-h-[90vh] overflow-hidden"
                        >
                            <div className="bg-zinc-900/95 backdrop-blur-xl border border-white/10 rounded-2xl shadow-2xl overflow-hidden">
                                {/* Header */}
                                <div className="flex items-center justify-between px-6 py-4 border-b border-white/5">
                                    <div className="flex items-center gap-3">
                                        <div className="p-2 rounded-xl bg-gradient-to-br from-indigo-500 to-purple-500 shadow-lg shadow-indigo-500/25">
                                            <Plus className="w-5 h-5 text-white" />
                                        </div>
                                        <h3 className="text-lg font-bold text-white">{t('accounts.add.title')}</h3>
                                    </div>
                                    <button
                                        onClick={() => !isDisabled && setIsOpen(false)}
                                        className="p-2 rounded-lg text-zinc-500 hover:text-white hover:bg-white/5 transition-colors"
                                    >
                                        <X className="w-5 h-5" />
                                    </button>
                                </div>

                                {/* Content */}
                                <div className="p-6 space-y-5 max-h-[calc(90vh-140px)] overflow-y-auto">
                                    {/* Tab Navigation */}
                                    <div className="bg-zinc-800/50 border border-white/5 p-1 rounded-xl grid grid-cols-3 gap-1">
                                        {TABS.map((tab) => (
                                            <button
                                                key={tab.id}
                                                onClick={() => setActiveTab(tab.id)}
                                                className={cn(
                                                    "relative py-2.5 px-3 rounded-lg text-xs font-bold transition-all flex items-center justify-center gap-2",
                                                    activeTab === tab.id
                                                        ? "text-white"
                                                        : "text-zinc-500 hover:text-zinc-300"
                                                )}
                                            >
                                                {activeTab === tab.id && (
                                                    <motion.div
                                                        layoutId="activeTab"
                                                        className="absolute inset-0 bg-gradient-to-r from-indigo-500/20 to-purple-500/20 border border-indigo-500/30 rounded-lg"
                                                        transition={{ type: 'spring', bounce: 0.2, duration: 0.5 }}
                                                    />
                                                )}
                                                <tab.icon className="w-3.5 h-3.5 relative z-10" />
                                                <span className="relative z-10">{t(tab.labelKey)}</span>
                                                {tab.recommended && activeTab === tab.id && (
                                                    <span className="relative z-10 px-1.5 py-0.5 text-[8px] font-bold bg-emerald-500/20 text-emerald-400 rounded border border-emerald-500/30">
                                                        ★
                                                    </span>
                                                )}
                                            </button>
                                        ))}
                                    </div>

                                    {/* Status Alert */}
                                    <AnimatePresence mode="wait">
                                        <StatusAlert status={status} message={message} />
                                    </AnimatePresence>

                                    {/* Tab Content */}
                                    <AnimatePresence mode="wait">
                                        {/* OAuth Tab */}
                                        {activeTab === 'oauth' && (
                                            <motion.div
                                                key="oauth"
                                                initial={{ opacity: 0, x: -20 }}
                                                animate={{ opacity: 1, x: 0 }}
                                                exit={{ opacity: 0, x: 20 }}
                                                transition={{ duration: 0.2 }}
                                                className="space-y-5"
                                            >
                                                {/* Hero Section */}
                                                <div className="text-center space-y-4 py-4">
                                                    <div className="relative inline-block">
                                                        <div className="absolute inset-0 bg-blue-500/30 rounded-full blur-xl" />
                                                        <div className="relative p-5 rounded-2xl bg-gradient-to-br from-blue-500/20 to-indigo-500/20 border border-blue-500/30">
                                                            <Globe className="w-10 h-10 text-blue-400" />
                                                        </div>
                                                    </div>
                                                    <div className="space-y-1">
                                                        <div className="flex items-center justify-center gap-2">
                                                            <h4 className="font-bold text-white">{t('accounts.add.oauth.recommend')}</h4>
                                                            <span className="px-2 py-0.5 text-[9px] font-bold bg-emerald-500/20 text-emerald-400 rounded-full border border-emerald-500/30 flex items-center gap-1">
                                                                <Sparkles className="w-2.5 h-2.5" />
                                                                {t('accounts.add.oauth.recommended_badge', 'Recommended')}
                                                            </span>
                                                        </div>
                                                        <p className="text-sm text-zinc-400 max-w-xs mx-auto">
                                                            {t('accounts.add.oauth.desc')}
                                                        </p>
                                                    </div>
                                                </div>

                                                {/* Start OAuth Button */}
                                                <motion.button
                                                    whileHover={{ scale: 1.01 }}
                                                    whileTap={{ scale: 0.99 }}
                                                    className="w-full py-3.5 bg-gradient-to-r from-blue-500 to-indigo-500 text-white font-bold rounded-xl shadow-lg shadow-blue-500/25 hover:shadow-blue-500/40 transition-all flex items-center justify-center gap-2 disabled:opacity-50 disabled:cursor-not-allowed"
                                                    onClick={handleOAuth}
                                                    disabled={isDisabled}
                                                >
                                                    {status === 'loading' ? (
                                                        <Loader2 className="w-4 h-4 animate-spin" />
                                                    ) : (
                                                        <Globe className="w-4 h-4" />
                                                    )}
                                                    {status === 'loading' ? t('accounts.add.oauth.btn_waiting') : t('accounts.add.oauth.btn_start')}
                                                </motion.button>

                                                {/* OAuth URL Section */}
                                                {oauthUrl && (
                                                    <motion.div
                                                        initial={{ opacity: 0, y: 10 }}
                                                        animate={{ opacity: 1, y: 0 }}
                                                        className="space-y-3"
                                                    >
                                                        <div className="text-[10px] font-bold text-zinc-500 uppercase tracking-wider">
                                                            {t('accounts.add.oauth.link_label')}
                                                        </div>
                                                        
                                                        <button
                                                            onClick={handleCopyUrl}
                                                            className="w-full p-3 bg-zinc-800/50 border border-white/5 rounded-xl hover:border-white/10 transition-all flex items-center gap-3 group"
                                                        >
                                                            <div className={cn(
                                                                "p-2 rounded-lg transition-colors",
                                                                oauthUrlCopied ? "bg-emerald-500/20 text-emerald-400" : "bg-zinc-700/50 text-zinc-400 group-hover:text-white"
                                                            )}>
                                                                {oauthUrlCopied ? <Check className="w-3.5 h-3.5" /> : <Copy className="w-3.5 h-3.5" />}
                                                            </div>
                                                            <code className="flex-1 text-[10px] font-mono text-zinc-400 truncate text-left">
                                                                {oauthUrl}
                                                            </code>
                                                            <span className={cn(
                                                                "text-[10px] font-bold px-2 py-1 rounded-lg transition-colors",
                                                                oauthUrlCopied ? "bg-emerald-500/20 text-emerald-400" : "bg-zinc-700/50 text-zinc-500"
                                                            )}>
                                                                {oauthUrlCopied ? t('accounts.add.oauth.copied') : t('accounts.add.oauth.copy_link')}
                                                            </span>
                                                        </button>

                                                        <button
                                                            onClick={handleCompleteOAuth}
                                                            disabled={isDisabled}
                                                            className="w-full py-2.5 bg-zinc-800/50 border border-white/5 text-zinc-300 font-medium rounded-xl hover:bg-zinc-700/50 hover:border-white/10 transition-all flex items-center justify-center gap-2 disabled:opacity-50"
                                                        >
                                                            <CheckCircle2 className="w-4 h-4" />
                                                            {t('accounts.add.oauth.btn_finish')}
                                                        </button>
                                                    </motion.div>
                                                )}

                                                {/* Manual Code Entry */}
                                                <div className="pt-4 border-t border-white/5 space-y-3">
                                                    <div className="text-[10px] font-bold text-zinc-500 uppercase tracking-wider">
                                                        {t('accounts.add.oauth.manual_hint')}
                                                    </div>
                                                    <div className="flex gap-2">
                                                        <input
                                                            type="text"
                                                            className="flex-1 px-4 py-2.5 bg-zinc-800/50 border border-white/5 rounded-xl text-sm text-white placeholder:text-zinc-600 focus:outline-none focus:border-indigo-500/50 transition-colors"
                                                            placeholder={t('accounts.add.oauth.manual_placeholder')}
                                                            value={manualCode}
                                                            onChange={(e) => setManualCode(e.target.value)}
                                                        />
                                                        <motion.button
                                                            whileHover={{ scale: 1.02 }}
                                                            whileTap={{ scale: 0.98 }}
                                                            onClick={handleManualSubmit}
                                                            disabled={!manualCode.trim()}
                                                            className="px-4 py-2.5 bg-zinc-700 text-white font-medium rounded-xl hover:bg-zinc-600 transition-colors flex items-center gap-2 disabled:opacity-50 disabled:cursor-not-allowed"
                                                        >
                                                            <Link2 className="w-4 h-4" />
                                                            {t('common.submit')}
                                                        </motion.button>
                                                    </div>
                                                </div>
                                            </motion.div>
                                        )}

                                        {/* Token Tab */}
                                        {activeTab === 'token' && (
                                            <motion.div
                                                key="token"
                                                initial={{ opacity: 0, x: -20 }}
                                                animate={{ opacity: 1, x: 0 }}
                                                exit={{ opacity: 0, x: 20 }}
                                                transition={{ duration: 0.2 }}
                                                className="space-y-4"
                                            >
                                                <div className="p-4 bg-zinc-800/30 border border-white/5 rounded-xl space-y-3">
                                                    <div className="flex items-center justify-between">
                                                        <span className="text-sm font-medium text-zinc-400">{t('accounts.add.token.label')}</span>
                                                        <Key className="w-4 h-4 text-zinc-600" />
                                                    </div>
                                                    <textarea
                                                        className="w-full h-32 px-4 py-3 bg-zinc-900/50 border border-white/5 rounded-xl text-sm font-mono text-white placeholder:text-zinc-600 focus:outline-none focus:border-indigo-500/50 transition-colors resize-none"
                                                        placeholder={t('accounts.add.token.placeholder')}
                                                        value={refreshToken}
                                                        onChange={(e) => setRefreshToken(e.target.value)}
                                                        disabled={isDisabled}
                                                    />
                                                    <p className="text-[10px] text-zinc-500">
                                                        {t('accounts.add.token.hint')}
                                                    </p>
                                                </div>

                                                <motion.button
                                                    whileHover={{ scale: 1.01 }}
                                                    whileTap={{ scale: 0.99 }}
                                                    onClick={handleSubmit}
                                                    disabled={isDisabled || !refreshToken.trim()}
                                                    className="w-full py-3 bg-gradient-to-r from-indigo-500 to-purple-500 text-white font-bold rounded-xl shadow-lg shadow-indigo-500/25 hover:shadow-indigo-500/40 transition-all flex items-center justify-center gap-2 disabled:opacity-50 disabled:cursor-not-allowed"
                                                >
                                                    {status === 'loading' ? (
                                                        <Loader2 className="w-4 h-4 animate-spin" />
                                                    ) : (
                                                        <CheckCircle2 className="w-4 h-4" />
                                                    )}
                                                    {t('accounts.add.btn_confirm')}
                                                </motion.button>
                                            </motion.div>
                                        )}

                                        {/* Import Tab */}
                                        {activeTab === 'import' && (
                                            <motion.div
                                                key="import"
                                                initial={{ opacity: 0, x: -20 }}
                                                animate={{ opacity: 1, x: 0 }}
                                                exit={{ opacity: 0, x: 20 }}
                                                transition={{ duration: 0.2 }}
                                                className="space-y-5"
                                            >
                                                {/* VSCode DB Import */}
                                                <div className="p-4 bg-zinc-800/30 border border-white/5 rounded-xl space-y-3">
                                                    <div className="flex items-center gap-3">
                                                        <div className="p-2 rounded-lg bg-blue-500/10 text-blue-400">
                                                            <Database className="w-4 h-4" />
                                                        </div>
                                                        <div>
                                                            <h4 className="font-bold text-white text-sm">{t('accounts.add.import.scheme_a')}</h4>
                                                            <p className="text-[11px] text-zinc-500">{t('accounts.add.import.scheme_a_desc')}</p>
                                                        </div>
                                                    </div>
                                                    <div className="grid grid-cols-2 gap-2">
                                                        <motion.button
                                                            whileHover={{ scale: 1.02 }}
                                                            whileTap={{ scale: 0.98 }}
                                                            onClick={handleImportDb}
                                                            disabled={isDisabled}
                                                            className="py-2.5 bg-zinc-700/50 border border-white/5 text-zinc-300 font-medium rounded-xl hover:bg-blue-500/10 hover:border-blue-500/30 hover:text-blue-400 transition-all flex items-center justify-center gap-2 disabled:opacity-50"
                                                        >
                                                            <Upload className="w-4 h-4" />
                                                            {t('accounts.add.import.btn_db')}
                                                        </motion.button>
                                                        <motion.button
                                                            whileHover={{ scale: 1.02 }}
                                                            whileTap={{ scale: 0.98 }}
                                                            onClick={handleImportCustomDb}
                                                            disabled={isDisabled}
                                                            className="py-2.5 bg-zinc-700/50 border border-white/5 text-zinc-300 font-medium rounded-xl hover:bg-indigo-500/10 hover:border-indigo-500/30 hover:text-indigo-400 transition-all flex items-center justify-center gap-2 disabled:opacity-50"
                                                        >
                                                            <FolderOpen className="w-4 h-4" />
                                                            {t('accounts.add.import.btn_custom_db', 'Custom DB')}
                                                        </motion.button>
                                                    </div>
                                                </div>

                                                {/* Divider */}
                                                <div className="flex items-center gap-4">
                                                    <div className="flex-1 h-px bg-white/5" />
                                                    <span className="text-[10px] font-bold text-zinc-600 uppercase">{t('accounts.add.import.or')}</span>
                                                    <div className="flex-1 h-px bg-white/5" />
                                                </div>

                                                {/* V1 Import */}
                                                <div className="p-4 bg-zinc-800/30 border border-white/5 rounded-xl space-y-3">
                                                    <div className="flex items-center gap-3">
                                                        <div className="p-2 rounded-lg bg-emerald-500/10 text-emerald-400">
                                                            <Sparkles className="w-4 h-4" />
                                                        </div>
                                                        <div>
                                                            <h4 className="font-bold text-white text-sm">{t('accounts.add.import.scheme_b')}</h4>
                                                            <p className="text-[11px] text-zinc-500">{t('accounts.add.import.scheme_b_desc')}</p>
                                                        </div>
                                                    </div>
                                                    <motion.button
                                                        whileHover={{ scale: 1.02 }}
                                                        whileTap={{ scale: 0.98 }}
                                                        onClick={handleImportV1}
                                                        disabled={isDisabled}
                                                        className="w-full py-2.5 bg-zinc-700/50 border border-white/5 text-zinc-300 font-medium rounded-xl hover:bg-emerald-500/10 hover:border-emerald-500/30 hover:text-emerald-400 transition-all flex items-center justify-center gap-2 disabled:opacity-50"
                                                    >
                                                        <Sparkles className="w-4 h-4" />
                                                        {t('accounts.add.import.btn_v1')}
                                                    </motion.button>
                                                </div>
                                            </motion.div>
                                        )}
                                    </AnimatePresence>
                                </div>

                                {/* Footer */}
                                <div className="px-6 py-4 border-t border-white/5 bg-zinc-900/50">
                                    <button
                                        onClick={async () => {
                                            if (status === 'loading' && activeTab === 'oauth') {
                                                await cancelOAuthMutation.mutateAsync();
                                            }
                                            setIsOpen(false);
                                        }}
                                        disabled={status === 'success'}
                                        className="w-full py-2.5 bg-zinc-800 text-zinc-400 font-medium rounded-xl hover:bg-zinc-700 hover:text-white transition-all disabled:opacity-50"
                                    >
                                        {t('accounts.add.btn_cancel')}
                                    </button>
                                </div>
                            </div>
                        </motion.div>
                    </motion.div>
                </AnimatePresence>,
                document.body
            )}
        </>
    );
}

export default AddAccountDialog;
