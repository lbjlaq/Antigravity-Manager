import { Ban, Lock, Clock, ExternalLink, Copy, FileText, Terminal, ChevronDown, ChevronRight } from 'lucide-react';
import { Account } from '../../types/account';
import { formatDate } from '../../utils/format';
import { useTranslation, Trans } from 'react-i18next';
import ModalDialog from '../common/ModalDialog';
import { useState } from 'react';
import { showToast } from '../common/ToastContainer';
import { copyToClipboard } from '../../utils/clipboard';

interface AccountErrorDialogProps {
    account: Account | null;
    onClose: () => void;
}

export default function AccountErrorDialog({ account, onClose }: AccountErrorDialogProps) {
    const [showRaw, setShowRaw] = useState(false);
    const [showGuide, setShowGuide] = useState(false);
    const { t } = useTranslation();
    if (!account) return null;

    const isForbidden = !!account.quota?.is_forbidden;
    const isDisabled = Boolean(account.disabled);
    const isProxyDisabled = account.proxy_disabled;
    const isValidationBlocked = account.validation_blocked;
    const isBanned = Boolean(account.banned);

    const rawReason = account.banned_reason || account.validation_blocked_reason || account.disabled_reason || account.quota?.forbidden_reason || account.proxy_disabled_reason || '';

    // 深度解析解析错误消息
    const extractErrorMessage = (raw: string) => {
        const trimmed = raw.trim();
        if (!trimmed) return raw;
        const tryParse = (value: string) => {
            try {
                return JSON.parse(value);
            } catch (_) {
                return null;
            }
        };
        const start = trimmed.indexOf('{');
        const end = trimmed.lastIndexOf('}');
        const parsed = tryParse(trimmed) || (start >= 0 && end > start ? tryParse(trimmed.slice(start, end + 1)) : null);
        if (!parsed) return raw;
        try {
            let innerParsed = null;
            if (typeof parsed?.error === 'string') {
                innerParsed = tryParse(parsed.error);
            }
            // 按照优先级尝试提取消息
            const msg = innerParsed?.error?.message
                || parsed?.error?.message
                || (Array.isArray(parsed?.error?.details) ? parsed.error.details[0]?.message : null)
                || parsed?.message
                || raw;
            return String(msg);
        } catch (_) {
            // 不处理
        }
        return raw;
    };

    const extractActionInfo = (raw: string): { url: string | null, label: string | null } => {
        const isSafeHttpUrl = (urlStr: string) => /^https?:\/\//i.test(urlStr.trim());

        const decodeUrl = (value: string) => value
            .replace(/\\u0026/g, '&')
            .replace(/\\u003d/g, '=')
            .replace(/\\\//g, '/')
            .replace(/&amp;/g, '&')
            .trim();

        const pickFromJson = (parsed: any): { url: string | null, label: string | null } => {
            const details = parsed?.error?.details || parsed?.details;
            const metadataList = Array.isArray(details) ? details.map((item: any) => item?.metadata).filter(Boolean) : [];
            const metadata = metadataList.find((item: any) => item?.appeal_url) || metadataList.find((item: any) => item?.validation_url);
            let url = metadata?.appeal_url || metadata?.validation_url || parsed?.appeal_url || parsed?.validation_url;
            let label = metadata?.appeal_url_link_text || metadata?.validation_url_link_text || parsed?.appeal_url_link_text || parsed?.validation_url_link_text;

            if (!url && typeof parsed?.error === 'string') {
                try {
                    const innerParsed = JSON.parse(parsed.error);
                    const innerDetails = innerParsed?.error?.details;
                    const innerList = Array.isArray(innerDetails) ? innerDetails.map((item: any) => item?.metadata).filter(Boolean) : [];
                    const innerMeta = innerList.find((item: any) => item?.appeal_url) || innerList.find((item: any) => item?.validation_url);
                    url = innerMeta?.appeal_url || innerMeta?.validation_url;
                    label = innerMeta?.appeal_url_link_text || innerMeta?.validation_url_link_text;
                } catch (_) { }
            }

            return url ? { url: decodeUrl(String(url)), label: label ? String(label) : null } : { url: null, label: null };
        };

        const parseRaw = (): { url: string | null, label: string | null } => {
            const trimmed = raw.trim();
            const tryParse = (value: string) => {
                try { return JSON.parse(value); } catch (_) { return null; }
            };
            const start = trimmed.indexOf('{');
            const end = trimmed.lastIndexOf('}');
            const parsed = tryParse(trimmed)
                || (start >= 0 && end > start ? tryParse(trimmed.slice(start, end + 1)) : null);

            if (parsed) {
                const picked = pickFromJson(parsed);
                if (picked.url && isSafeHttpUrl(picked.url)) {
                    return picked;
                }
            }

            // 最后降级到正则匹配：容忍 https:\/\/... 转义，且不能在 & 或反斜杠处截断。
            const appealIndex = raw.indexOf('appeal_url');
            const validationIndex = raw.indexOf('validation_url');
            const source = appealIndex >= 0 ? raw.slice(appealIndex) : (validationIndex >= 0 ? raw.slice(validationIndex) : raw);
            const urlRegex = /https?:\\?\/\\?\/[^\s"']+/g;
            const match = source.match(urlRegex);
            if (match) {
                const extracted = decodeUrl(match[0]).replace(/[,}\])\\.]+$/g, '');
                if (isSafeHttpUrl(extracted)) {
                    return { url: extracted.trim(), label: null };
                }
            }
            return { url: null, label: null };
        };

        // 封禁申诉链接优先（新数据中由后端完整提取并持久化）。
        if (account.ban_appeal_url && isSafeHttpUrl(account.ban_appeal_url)) {
            return { url: account.ban_appeal_url.trim(), label: null };
        }

        const rawResult = parseRaw();
        const storedUrl = account.validation_url && isSafeHttpUrl(account.validation_url)
            ? account.validation_url.trim()
            : null;

        // 历史数据里的 validation_url 可能被旧正则截断；当原始报文能解析出更长、
        // 且以它开头的完整 URL 时，优先使用原始报文中的完整链接。
        if (rawResult.url && storedUrl && rawResult.url.length > storedUrl.length && rawResult.url.startsWith(storedUrl)) {
            return rawResult;
        }
        if (storedUrl) {
            return { url: storedUrl, label: rawResult.label };
        }
        return rawResult;
    };

    const message = extractErrorMessage(rawReason);
    const { url: actionUrl, label: actionLabel } = extractActionInfo(rawReason);

    // 识别错误类型
    const isViolation = isBanned || rawReason.toLowerCase().includes('terms of service') || rawReason.toLowerCase().includes('violation');
    const isVerificationNeeded = !isViolation && (rawReason.toLowerCase().includes('verify your account') || !!account.validation_url);

    // 复制功能：使用带降级方案的 clipboard 工具，成功后才提示。
    const handleCopyUrl = async (url: string) => {
        const success = await copyToClipboard(url);
        if (success) {
            showToast(
                isViolation
                    ? t('accounts.appeal_url_copied', '申诉链接已复制到剪贴板')
                    : t('accounts.validation_url_copied', '验证链接已复制到剪贴板'),
                'success'
            );
        } else {
            showToast(t('accounts.copy_failed', '复制失败，请手动选择链接复制'), 'error');
        }
    };

    const handleCopyText = async (text: string, msg: string) => {
        const success = await copyToClipboard(text);
        if (success) {
            showToast(msg, 'success');
        } else {
            showToast(t('accounts.copy_failed', '复制失败，请手动选择内容复制'), 'error');
        }
    };

    const renderMessageWithLinks = (text: string) => {
        const urlRegex = /(https?:\/\/[^\s]+)/g;
        const parts = text.split(urlRegex);
        return parts.map((part, i) => {
            if (part.match(urlRegex)) {
                return (
                    <a
                        key={i}
                        href={part}
                        target="_blank"
                        rel="noopener noreferrer"
                        className="text-blue-600 dark:text-blue-400 underline hover:text-blue-700 dark:hover:text-blue-300 break-all inline-flex items-center gap-1"
                        onClick={(e) => e.stopPropagation()}
                    >
                        {t('accounts.click_to_verify', '点击去验证')}
                        <ExternalLink className="w-3 h-3" />
                    </a>
                );
            }
            return part;
        });
    };

    return (
        <ModalDialog
            isOpen={true}
            title={t('accounts.error_details')}
            type="error"
            onConfirm={onClose}
            confirmText={t('common.close')}
        >
            <div className="space-y-4 max-h-[75vh] overflow-y-auto scrollbar-thin scrollbar-thumb-gray-200 dark:scrollbar-thumb-gray-700 pr-1 py-1">
                {/* Account Info */}
                <div>
                    <label className="text-[10px] font-bold text-gray-400 dark:text-gray-500 uppercase tracking-wider block mb-1.5 ml-1">
                        {t('accounts.account')}
                    </label>
                    <div className="text-sm font-medium text-gray-700 dark:text-gray-300 bg-gray-50 dark:bg-base-200/50 px-4 py-2.5 rounded-xl border border-gray-100 dark:border-base-200 shadow-sm">
                        {account.email}
                    </div>
                </div>

                {/* Status */}
                <div>
                    <label className="text-[10px] font-bold text-gray-400 dark:text-gray-500 uppercase tracking-wider block mb-1.5 ml-1">
                        {t('accounts.error_status')}
                    </label>
                    <div className="flex flex-wrap gap-2">
                        {isBanned ? (
                            <span className="flex items-center gap-1.5 px-3 py-1 rounded-lg bg-red-100 text-red-700 dark:bg-red-900/30 dark:text-red-400 text-xs font-bold ring-1 ring-red-200/50 dark:ring-red-900/20">
                                <Ban className="w-3 h-3" />
                                {t('accounts.status.banned', '账号已封禁')}
                            </span>
                        ) : isViolation ? (
                            <span className="flex items-center gap-1.5 px-3 py-1 rounded-lg bg-red-100 text-red-700 dark:bg-red-900/30 dark:text-red-400 text-xs font-bold ring-1 ring-red-200/50 dark:ring-red-900/20">
                                <Lock className="w-3 h-3" />
                                {t('accounts.status.violation_blocked', '由于违规被禁用')}
                            </span>
                        ) : null}
                        {isForbidden && !isViolation && !isVerificationNeeded && !isValidationBlocked && (
                            <span className="flex items-center gap-1.5 px-3 py-1 rounded-lg bg-red-100 text-red-700 dark:bg-red-900/30 dark:text-red-400 text-xs font-bold ring-1 ring-red-200/50 dark:ring-red-900/20">
                                <Lock className="w-3 h-3" />
                                {t('accounts.status.forbidden')}
                            </span>
                        )}
                        {isDisabled && (
                            <span className="flex items-center gap-1.5 px-3 py-1 rounded-lg bg-rose-100 text-rose-700 dark:bg-rose-900/30 dark:text-rose-400 text-xs font-bold ring-1 ring-rose-200/50 dark:ring-rose-900/20">
                                <Ban className="w-3 h-3" />
                                {t('accounts.status.disabled')}
                            </span>
                        )}
                        {isProxyDisabled && !isBanned && (
                            <span className="flex items-center gap-1.5 px-3 py-1 rounded-lg bg-orange-100 text-orange-700 dark:bg-orange-900/30 dark:text-orange-400 text-xs font-bold ring-1 ring-orange-200/50 dark:ring-orange-900/20">
                                <Ban className="w-3 h-3" />
                                {t('accounts.status.proxy_disabled')}
                            </span>
                        )}
                        {(isValidationBlocked || isVerificationNeeded) && (
                            <span className="flex items-center gap-1.5 px-3 py-1 rounded-lg bg-amber-100 text-amber-700 dark:bg-amber-900/30 dark:text-amber-400 text-xs font-bold ring-1 ring-amber-200/50 dark:ring-amber-900/20">
                                <Clock className="w-3 h-3" />
                                {t('accounts.status.validation_required', '账号需验证')}
                            </span>
                        )}
                    </div>
                </div>

                {/* Reason */}
                <div>
                    <div className="flex items-center justify-between mb-1.5 ml-1">
                        <label className="text-[10px] font-bold text-gray-400 dark:text-gray-500 uppercase tracking-wider block">
                            {t('common.reason', '原因')}
                        </label>
                        <button
                            onClick={() => setShowRaw(!showRaw)}
                            className="text-[10px] flex items-center gap-1 text-blue-500 hover:text-blue-600 transition-colors font-medium"
                        >
                            <FileText className="w-2.5 h-2.5" />
                            {showRaw ? t('common.show_parsed', '显示解析后') : t('common.show_raw', '显示原始报文')}
                        </button>
                    </div>
                    <div className="text-xs text-red-600 dark:text-red-400 bg-red-50/50 dark:bg-red-900/10 p-4 rounded-xl border border-red-100 dark:border-red-900/20 break-all leading-relaxed font-mono shadow-inner min-h-[80px] max-h-[40vh] overflow-y-auto scrollbar-thin scrollbar-thumb-red-200 dark:scrollbar-thumb-red-800">
                        {showRaw ? (
                            <pre className="whitespace-pre-wrap break-all">{rawReason}</pre>
                        ) : (
                            message ? renderMessageWithLinks(message) : t('common.unknown')
                        )}
                    </div>

                    {/* Action Buttons for Verification / Appeal */}
                    {actionUrl && !showRaw && (
                        <>
                            <div className="mt-3 flex gap-2">
                                <a
                                    href={actionUrl}
                                    target="_blank"
                                    rel="noopener noreferrer"
                                    className="flex-1 flex items-center justify-center gap-2 py-2 text-xs font-bold bg-blue-600 hover:bg-blue-700 text-white rounded-lg transition-all shadow-md shadow-blue-500/20 active:scale-[0.98]"
                                >
                                    <ExternalLink className="w-3 h-3" />
                                    {actionLabel || (isViolation ? t('accounts.go_to_appeal', '前往申诉') : t('accounts.click_to_verify', '点击去验证'))}
                                </a>
                                <button
                                    onClick={() => handleCopyUrl(actionUrl)}
                                    className="flex-1 flex items-center justify-center gap-2 py-2 text-xs font-bold bg-gray-100 dark:bg-base-300 hover:bg-gray-200 dark:hover:bg-base-200 text-gray-700 dark:text-gray-300 rounded-lg transition-all active:scale-[0.98]"
                                    title={actionUrl}
                                >
                                    <Copy className="w-3 h-3" />
                                    {isViolation ? t('accounts.copy_appeal_url', '复制申诉链接') : t('accounts.copy_validation_url', '复制验证链接')}
                                </button>
                            </div>
                            {/* 完整链接明文展示：即使剪贴板受限，也能手动选择完整链接 */}
                            <div className="mt-2 px-3 py-2 rounded-lg bg-gray-50 dark:bg-base-300/50 border border-gray-100 dark:border-base-200 text-[10px] font-mono text-gray-500 dark:text-gray-400 break-all select-all">
                                {actionUrl}
                            </div>
                        </>
                    )}

                    {/* Terminal Fix Guide */}
                    {(isForbidden || isVerificationNeeded) && !isBanned && !showRaw && (
                        <div className="mt-4 border border-blue-100 dark:border-blue-900/40 rounded-xl overflow-hidden shadow-sm">
                            <button
                                onClick={() => setShowGuide(!showGuide)}
                                className="w-full flex items-center justify-between p-3 bg-blue-50/70 dark:bg-blue-900/20 hover:bg-blue-100/70 dark:hover:bg-blue-900/40 transition-colors"
                            >
                                <div className="flex items-center gap-2 text-blue-700 dark:text-blue-400 font-bold text-xs">
                                    <Terminal className="w-4 h-4" />
                                    <span>{t('accounts.fix_guide.title', '终端一键自救指南 (解决部分 403 拦截)')}</span>
                                </div>
                                {showGuide ? <ChevronDown className="w-4 h-4 text-blue-500" /> : <ChevronRight className="w-4 h-4 text-blue-500" />}
                            </button>
                            {showGuide && (
                                <div className="p-4 text-xs space-y-4 bg-white dark:bg-base-200 text-gray-700 dark:text-gray-300 max-h-[35vh] overflow-y-auto scrollbar-thin scrollbar-thumb-blue-200 dark:scrollbar-thumb-blue-800">
                                    <div>
                                        <p className="mb-2 text-[11px] leading-relaxed">
                                            {t('accounts.fix_guide.step1_desc', '打开终端（Terminal），执行以下命令告诉 Google "是我本人"，可解决部分 403 拦截：')}
                                        </p>
                                        <div className="bg-gray-900 dark:bg-[#1e1e1e] text-green-400 p-2.5 rounded-lg font-mono text-[11px] flex justify-between items-center ring-1 ring-inset ring-gray-800">
                                            <code>gcloud auth login --update-adc</code>
                                            <button
                                                onClick={() => handleCopyText('gcloud auth login --update-adc', t('common.copied', '成功复制命令'))}
                                                className="text-gray-400 hover:text-white transition-colors p-1"
                                                title={t('common.copy', '复制')}
                                            >
                                                <Copy className="w-3.5 h-3.5" />
                                            </button>
                                        </div>
                                        <ul className="mt-2 text-[11px] text-gray-500 dark:text-gray-400 list-disc pl-4 marker:text-gray-300 dark:marker:text-gray-600">
                                            <li><Trans i18nKey="accounts.fix_guide.step1_li1" components={{ 1: <code /> }}>按回车执行，提示继续时输入 <code />。</Trans></li>
                                            <li>{t('accounts.fix_guide.step1_li2')}</li>
                                            <li><Trans i18nKey="accounts.fix_guide.step1_li3" components={{ 1: <code /> }}>看到 <code /> 即大功告成！</Trans></li>
                                        </ul>
                                    </div>

                                    <div className="border-t border-gray-100 dark:border-base-300/50 pt-3">
                                        <h4 className="font-bold text-gray-800 dark:text-gray-200 mb-1.5 flex items-center gap-1.5">
                                            {t('accounts.fix_guide.step2_title', '🧹 如果无效（清除缓存重来）')}
                                        </h4>
                                        <ol className="list-decimal pl-4 space-y-2 text-[11px] text-gray-600 dark:text-gray-400 marker:text-gray-400 font-medium">
                                            <li>
                                                {t('accounts.fix_guide.step2_li1_prefix', '先执行清除命令退出旧认证：')}
                                                <div className="bg-gray-100 dark:bg-base-300/50 mt-1 px-2 py-1.5 rounded text-red-600 dark:text-red-400 inline-block font-mono">
                                                    gcloud auth revoke {account.email || 'your-email@gmail.com'}
                                                </div>
                                            </li>
                                            <li>{t('accounts.fix_guide.step2_li2_prefix', '再执行登录：')}<code className="bg-blue-50 dark:bg-blue-900/30 text-blue-600 dark:text-blue-400 px-1 rounded ml-1">gcloud auth login --update-adc</code></li>
                                        </ol>
                                    </div>

                                    <div className="border-t border-gray-100 dark:border-base-300/50 pt-3">
                                        <h4 className="font-bold text-gray-800 dark:text-gray-200 mb-1.5 flex items-center gap-1.5">
                                            {t('accounts.fix_guide.tips_title', '💡 常见建议')}
                                        </h4>
                                        <ul className="list-disc pl-4 space-y-1.5 text-[11px] text-gray-500 dark:text-gray-400 font-medium marker:text-gray-300">
                                            <li><Trans i18nKey="accounts.fix_guide.tip1" components={{ 1: <code /> }}>若仍 403，尝试先在终端执行 <code /> 重置环境变量。</Trans></li>
                                            <li><Trans i18nKey="accounts.fix_guide.tip2" components={{ 1: <strong /> }}>生产环境强烈建议改用 <strong /> 的 JSON 密钥，更稳定且免交互。</Trans></li>
                                            <li><Trans i18nKey="accounts.fix_guide.tip3" components={{ 1: <a href="https://console.cloud.google.com/" target="_blank" rel="noopener noreferrer" className="text-blue-500 hover:text-blue-600 hover:underline" /> }}>若操作失败，请前往 <a /> 中的 Generative Language API 查看是否被冻结权限。若是，说明账号触发了风控，建议让账号冷却 72 小时后再次尝试。</Trans></li>
                                            <li><Trans i18nKey="accounts.fix_guide.tip4" components={{ 1: <code /> }}>你也可以尝试执行 <code />，只要不弹出错误，大概率在软件内删除账号重新授权即可。</Trans></li>
                                        </ul>
                                    </div>
                                </div>
                            )}
                        </div>
                    )}
                </div>

                {/* Time */}
                <div className="flex items-center gap-2 text-[11px] text-gray-400 dark:text-gray-500 pl-1">
                    <Clock size={12} strokeWidth={2.5} />
                    <span>
                        {t('accounts.error_time')}: {account.disabled_at ? formatDate(account.disabled_at) : (account.quota?.last_updated ? formatDate(account.quota.last_updated) : t('common.unknown'))}
                    </span>
                </div>
            </div>
        </ModalDialog>
    );
}
