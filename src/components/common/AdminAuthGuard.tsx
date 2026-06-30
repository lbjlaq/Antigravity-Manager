import React, { useState, useEffect } from 'react';
import { Lock, Key, Globe, AlertCircle, Loader2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { isTauri } from '../../utils/env';

/**
 * AdminAuthGuard
 * 针对 Docker/Web 模式的强制鉴权保护层。
 * 如果检测到没有存储的 API Key 或后端返回 401，将拦截 UI 并要求输入 Key。
 */
export const AdminAuthGuard: React.FC<{ children: React.ReactNode }> = ({ children }) => {
    const { t, i18n } = useTranslation();
    const [isAuthenticated, setIsAuthenticated] = useState(isTauri());
    const [apiKey, setApiKey] = useState('');
    const [showLangMenu, setShowLangMenu] = useState(false);
    const [isLoading, setIsLoading] = useState(false);
    const [error, setError] = useState('');

    useEffect(() => {
        if (isTauri()) return;

        // 检查 Session 存储 (优先)
        const sessionKey = sessionStorage.getItem('abv_admin_api_key');
        if (sessionKey) {
            setIsAuthenticated(true);
            setApiKey(sessionKey);
            return;
        }

        // 检查本地存储 (迁移逻辑)
        const savedKey = localStorage.getItem('abv_admin_api_key');
        if (savedKey) {
            // 迁移到 sessionStorage 并清理 localStorage
            sessionStorage.setItem('abv_admin_api_key', savedKey);
            localStorage.removeItem('abv_admin_api_key');
            setIsAuthenticated(true);
            setApiKey(savedKey);
        }

        // 监听全局 401 事件
        const handleUnauthorized = () => {
            sessionStorage.removeItem('abv_admin_api_key');
            localStorage.removeItem('abv_admin_api_key'); // 双重清理确保万一
            setIsAuthenticated(false);
        };

        window.addEventListener('abv-unauthorized', handleUnauthorized);
        return () => window.removeEventListener('abv-unauthorized', handleUnauthorized);
    }, []);

    const handleLogin = async (e: React.FormEvent) => {
        e.preventDefault();
        const trimmedKey = apiKey.trim();
        if (!trimmedKey) return;

        setIsLoading(true);
        setError('');

        try {
            // 先临时存储 key，用于验证请求
            sessionStorage.setItem('abv_admin_api_key', trimmedKey);

            // 调用一个需要认证的 API 来验证密码是否正确
            const response = await fetch('/api/accounts', {
                method: 'GET',
                headers: {
                    'Content-Type': 'application/json',
                    'Authorization': `Bearer ${trimmedKey}`,
                    'x-api-key': trimmedKey
                }
            });

            if (response.ok || response.status === 204) {
                // 验证成功
                localStorage.removeItem('abv_admin_api_key');
                setIsAuthenticated(true);
                window.location.reload();
            } else if (response.status === 401) {
                // 密码错误
                sessionStorage.removeItem('abv_admin_api_key');
                setError(t('login.error_invalid_key'));
            } else {
                // 其他错误，但可能密码是对的
                setIsAuthenticated(true);
                window.location.reload();
            }
        } catch (err) {
            // 网络错误等
            sessionStorage.removeItem('abv_admin_api_key');
            setError(t('login.error_network'));
        } finally {
            setIsLoading(false);
        }
    };

    const changeLanguage = (lng: string) => {
        i18n.changeLanguage(lng);
        setShowLangMenu(false);
    };

    const languages = [
        { code: 'zh', name: '简体中文' },
        { code: 'zh-TW', name: '繁體中文' },
        { code: 'en', name: 'English' },
        { code: 'ja', name: '日本語' },
        { code: 'ko', name: '한국어' },
        { code: 'ru', name: 'Русский' },
        { code: 'tr', name: 'Türkçe' },
        { code: 'vi', name: 'Tiếng Việt' },
        { code: 'pt', name: 'Português' },
        { code: 'ar', name: 'العربية' },
        { code: 'es', name: 'Español' },
        { code: 'my', name: 'Bahasa Melayu' },
    ];
    const configHintText = t('login.config_hint');
    const configHintCommand = `grep -E '"api_key"|"admin_password"' ~/.antigravity_tools/gui_config.json`;
    const configHintCommandIndex = configHintText.indexOf(configHintCommand);
    const configHintPrefix = configHintCommandIndex >= 0 ? configHintText.slice(0, configHintCommandIndex) : '';
    const configHintSuffix =
        configHintCommandIndex >= 0
            ? configHintText.slice(configHintCommandIndex + configHintCommand.length)
            : '';
    const configHintContent = (
        <span className="group">
            {configHintCommandIndex >= 0 ? (
                <>
                    {configHintPrefix}
                    <span className="rounded px-1 transition-colors duration-150 group-hover:bg-amber-200/80 dark:group-hover:bg-amber-400/20">
                        {configHintCommand}
                    </span>
                    {configHintSuffix}
                </>
            ) : (
                configHintText
            )}
        </span>
    );

    if (isAuthenticated) {
        return <>{children}</>;
    }

    return (
        <div className="min-h-screen bg-slate-50 dark:bg-base-300 flex items-center justify-center p-4 relative">
            {/* 语言切换按钮 */}
            <div className="absolute top-8 right-8">
                <div className="relative">
                    <button
                        onClick={() => setShowLangMenu(!showLangMenu)}
                        className="flex items-center gap-2 px-4 py-2 bg-white dark:bg-base-100 rounded-2xl shadow-sm border border-slate-100 dark:border-white/5 text-slate-600 dark:text-slate-300 hover:bg-slate-50 dark:hover:bg-white/5 transition-all"
                    >
                        <Globe className="w-4 h-4" />
                        <span className="text-sm font-medium uppercase">{i18n.language.split('-')[0]}</span>
                    </button>

                    {showLangMenu && (
                        <div className="absolute right-0 mt-2 w-40 bg-white dark:bg-base-100 rounded-2xl shadow-xl border border-slate-100 dark:border-white/5 py-2 z-50 animate-in fade-in zoom-in duration-200">
                            {languages.map((lang) => (
                                <button
                                    key={lang.code}
                                    onClick={() => changeLanguage(lang.code)}
                                    className={`w-full text-left px-4 py-2 text-sm hover:bg-slate-50 dark:hover:bg-white/5 transition-colors ${i18n.language === lang.code ? 'text-blue-500 font-bold' : 'text-slate-600 dark:text-slate-300'
                                        }`}
                                >
                                    {lang.name}
                                </button>
                            ))}
                        </div>
                    )}
                </div>
            </div>

            <div className="max-w-md w-full bg-white dark:bg-base-100 rounded-3xl shadow-xl overflow-hidden border border-slate-100 dark:border-white/5">
                <div className="p-8">
                    <div className="w-16 h-16 bg-blue-50 dark:bg-blue-900/20 rounded-2xl flex items-center justify-center mb-6 mx-auto">
                        <Lock className="w-8 h-8 text-blue-500" />
                    </div>
                    <h2 className="text-2xl font-bold text-center text-slate-900 dark:text-slate-100 mb-2 font-display">{t('login.title')}</h2>
                    <p className="text-center text-slate-500 dark:text-slate-400 mb-8 text-sm">{t('login.desc')}</p>

                    <form onSubmit={handleLogin} className="space-y-6">
                        <div className="relative">
                            <Key className="absolute left-4 top-1/2 -translate-y-1/2 w-5 h-5 text-slate-400" />
                            <input
                                type="password"
                                placeholder={t('login.placeholder')}
                                className={`w-full pl-12 pr-4 py-4 bg-slate-50 dark:bg-base-200 border-2 rounded-2xl focus:ring-2 focus:ring-blue-500 transition-all outline-none text-slate-900 dark:text-white ${error ? 'border-red-400' : 'border-transparent'}`}
                                value={apiKey}
                                onChange={(e) => { setApiKey(e.target.value); setError(''); }}
                                autoFocus
                                disabled={isLoading}
                            />
                        </div>
                        {error && (
                            <div className="flex items-center gap-2 text-red-500 text-sm">
                                <AlertCircle className="w-4 h-4" />
                                <span>{error}</span>
                            </div>
                        )}
                        <button
                            type="submit"
                            disabled={isLoading || !apiKey.trim()}
                            className="w-full py-4 bg-blue-500 hover:bg-blue-600 disabled:bg-blue-300 disabled:cursor-not-allowed text-white font-bold rounded-2xl shadow-lg shadow-blue-500/30 transition-all active:scale-[0.98] flex items-center justify-center gap-2"
                        >
                            {isLoading ? (
                                <>
                                    <Loader2 className="w-5 h-5 animate-spin" />
                                    {t('login.btn_verifying')}
                                </>
                            ) : (
                                t('login.btn_login')
                            )}
                        </button>
                    </form>

                    <div className="mt-8 pt-6 border-t border-slate-50 dark:border-white/5 text-center">
                        <p className="text-[10px] text-slate-400 leading-relaxed">
                            {t('login.note')}
                            <br />
                            {t('login.lookup_hint')}
                            <br />
                            {configHintContent}
                        </p>
                    </div>
                </div>
            </div>
        </div>
    );
};
