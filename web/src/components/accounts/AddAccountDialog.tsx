import { useState, useEffect } from 'react';
import { createPortal } from 'react-dom';
import { Plus, Loader2, CheckCircle2, XCircle } from 'lucide-react';
import { useTranslation } from 'react-i18next';

interface AddAccountDialogProps {
    onAdd: (email: string, refreshToken: string) => Promise<void>;
}

type Status = 'idle' | 'loading' | 'success' | 'error';

function AddAccountDialog({ onAdd }: AddAccountDialogProps) {
    const { t } = useTranslation();
    const [isOpen, setIsOpen] = useState(false);
    const [refreshToken, setRefreshToken] = useState('');

    // UI State
    const [status, setStatus] = useState<Status>('idle');
    const [message, setMessage] = useState('');

    // Reset state when dialog opens
    useEffect(() => {
        if (isOpen) {
            resetState();
        }
    }, [isOpen]);

    const resetState = () => {
        setStatus('idle');
        setMessage('');
        setRefreshToken('');
    };

    const handleSubmit = async () => {
        if (!refreshToken) {
            setStatus('error');
            setMessage(t('accounts.add.token.error_token'));
            return;
        }

        setStatus('loading');

        // 1. 尝试解析输入
        let tokens: string[] = [];
        const input = refreshToken.trim();

        try {
            // 尝试解析为 JSON
            if (input.startsWith('[') && input.endsWith(']')) {
                const parsed = JSON.parse(input);
                if (Array.isArray(parsed)) {
                    tokens = parsed
                        .map((item: any) => item.refresh_token)
                        .filter((t: any) => typeof t === 'string' && t.startsWith('1//'));
                }
            }
        } catch (e) {
            // JSON 解析失败,忽略
            console.debug('JSON parse failed, falling back to regex', e);
        }

        // 2. 如果 JSON 解析没有结果,尝试正则提取 (或者输入不是 JSON)
        if (tokens.length === 0) {
            const regex = /1\/\/[a-zA-Z0-9_\-]+/g;
            const matches = input.match(regex);
            if (matches) {
                tokens = matches;
            }
        }

        // 去重
        tokens = [...new Set(tokens)];

        if (tokens.length === 0) {
            setStatus('error');
            setMessage(t('accounts.add.token.error_token')); // 或者提示"未找到有效 Token"
            return;
        }

        // 3. 批量添加
        let successCount = 0;
        let failCount = 0;

        for (let i = 0; i < tokens.length; i++) {
            const currentToken = tokens[i];
            setMessage(t('accounts.add.token.batch_progress', { current: i + 1, total: tokens.length }));

            try {
                // 后端 add_account 接口需要 email 和 refresh_token
                // 这里 email 传空字符串，由后端通过 token 获取或以后端逻辑为准
                await onAdd("", currentToken);
                successCount++;
            } catch (error) {
                console.error(`Failed to add token ${i + 1}:`, error);
                failCount++;
            }
            // 稍微延迟一下,避免太快
            await new Promise(r => setTimeout(r, 100));
        }

        // 4. 结果反馈
        if (successCount === tokens.length) {
            setStatus('success');
            setMessage(t('accounts.add.token.batch_success', { count: successCount }));
            setTimeout(() => {
                setIsOpen(false);
                resetState();
            }, 1500);
        } else if (successCount > 0) {
            // 部分成功
            setStatus('success'); // 还是用绿色,但提示部分失败
            setMessage(t('accounts.add.token.batch_partial', { success: successCount, fail: failCount }));
            // 不自动关闭,让用户看到结果
        } else {
            // 全部失败
            setStatus('error');
            setMessage(t('accounts.add.token.batch_fail'));
        }
    };

    // 状态提示组件
    const StatusAlert = () => {
        if (status === 'idle' || !message) return null;

        const styles = {
            loading: 'alert-info',
            success: 'alert-success',
            error: 'alert-error'
        };

        const icons = {
            loading: <Loader2 className="w-5 h-5 animate-spin" />,
            success: <CheckCircle2 className="w-5 h-5" />,
            error: <XCircle className="w-5 h-5" />
        };

        return (
            <div className={`alert ${styles[status]} mb-4 text-sm py-2 shadow-sm`}>
                {icons[status]}
                <span>{message}</span>
            </div>
        );
    };

    return (
        <>
            <button
                className="px-4 py-2 bg-white dark:bg-base-100 text-gray-700 dark:text-gray-300 text-sm font-medium rounded-lg hover:bg-gray-50 dark:hover:bg-base-200 transition-colors flex items-center gap-2 shadow-sm border border-gray-200/50 dark:border-base-300"
                onClick={() => setIsOpen(true)}
            >
                <Plus className="w-4 h-4" />
                {t('accounts.add_account')}
            </button>

            {isOpen && createPortal(
                <dialog className="modal modal-open z-[100]">
                    <div className="modal-box bg-white dark:bg-base-100 text-gray-900 dark:text-base-content">
                        <h3 className="font-bold text-lg mb-4">{t('accounts.add.title')}</h3>

                        {/* 仅保留 Refresh Token 选项 */}
                        <div className="bg-gray-100 dark:bg-base-200 p-1 rounded-xl mb-6 flex justify-center">
                            <div className="py-2 px-6 rounded-lg text-sm font-semibold bg-white dark:bg-base-100 shadow-sm text-blue-600 dark:text-blue-400 w-full text-center">
                                {t('accounts.add.tabs.token')}
                            </div>
                        </div>

                        {/* 状态提示区 */}
                        <StatusAlert />

                        <div className="min-h-[200px]">
                            {/* Refresh Token */}
                            <div className="space-y-4 py-2">
                                <div className="bg-gray-50 dark:bg-base-200 p-4 rounded-lg border border-gray-200 dark:border-base-300">
                                    <div className="flex justify-between items-center mb-2">
                                        <span className="text-sm font-medium text-gray-500 dark:text-gray-400">{t('accounts.add.token.label')}</span>
                                    </div>
                                    <textarea
                                        className="textarea textarea-bordered w-full h-32 font-mono text-xs leading-relaxed focus:outline-none focus:border-blue-500 transition-colors bg-white dark:bg-base-100 text-gray-900 dark:text-base-content border-gray-300 dark:border-base-300 placeholder:text-gray-400"
                                        placeholder={t('accounts.add.token.placeholder')}
                                        value={refreshToken}
                                        onChange={(e) => setRefreshToken(e.target.value)}
                                        disabled={status === 'loading' || status === 'success'}
                                    />
                                    <p className="text-[10px] text-gray-400 mt-2">
                                        {t('accounts.add.token.hint')}
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div className="flex gap-3 w-full mt-6">
                            <button
                                className="flex-1 px-4 py-2.5 bg-gray-100 dark:bg-base-200 text-gray-700 dark:text-gray-300 font-medium rounded-xl hover:bg-gray-200 dark:hover:bg-base-300 transition-colors focus:outline-none focus:ring-2 focus:ring-200 dark:focus:ring-base-300"
                                onClick={() => {
                                    setIsOpen(false);
                                }}
                                disabled={status === 'success'}
                            >
                                {t('accounts.add.btn_cancel')}
                            </button>
                            <button
                                className="flex-1 px-4 py-2.5 text-white font-medium rounded-xl shadow-md transition-all focus:outline-none focus:ring-2 focus:ring-offset-2 bg-blue-500 hover:bg-blue-600 focus:ring-blue-500 shadow-blue-100 dark:shadow-blue-900/30 flex justify-center items-center gap-2"
                                onClick={handleSubmit}
                                disabled={status === 'loading' || status === 'success'}
                            >
                                {status === 'loading' ? <Loader2 className="w-4 h-4 animate-spin" /> : null}
                                {t('accounts.add.btn_confirm')}
                            </button>
                        </div>
                    </div>
                    <div className="modal-backdrop bg-black/40 backdrop-blur-sm fixed inset-0 z-[-1]" onClick={() => setIsOpen(false)}></div>
                </dialog>,
                document.body
            )}
        </>
    );
}

export default AddAccountDialog;
