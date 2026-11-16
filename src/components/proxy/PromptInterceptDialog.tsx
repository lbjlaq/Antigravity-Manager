import React, { useEffect, useState, useRef, useCallback } from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';
import { listen } from '@tauri-apps/api/event';
import { AlertTriangle, Clock, Sparkles, Bookmark, ArrowRight, ShieldCheck } from 'lucide-react';
import { request as invoke } from '../../utils/request';
import { showToast } from '../common/ToastContainer';

export interface PromptInterceptPayload {
    id: string;
    firstSentence: string;
    fullPromptPreview: string;
    officialIdentity: string;
    clientIp?: string;
    model?: string;
    timeoutSecs: number;
}

export const PromptInterceptDialog: React.FC = () => {
    const { t } = useTranslation();
    const [intercept, setIntercept] = useState<PromptInterceptPayload | null>(null);
    const [timeLeft, setTimeLeft] = useState<number>(25);
    const [submitting, setSubmitting] = useState<boolean>(false);
    const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);

    // 监听 Rust 发送的拦截事件
    useEffect(() => {
        const unlistenPromise = listen<PromptInterceptPayload>('proxy://prompt-intercept', (event) => {
            console.log('[PromptInterceptDialog] Received intercept event:', event.payload);
            setIntercept(event.payload);
            setTimeLeft(event.payload.timeoutSecs || 25);
        });

        return () => {
            unlistenPromise.then((unlisten) => unlisten());
            if (timerRef.current) {
                clearInterval(timerRef.current);
            }
        };
    }, []);

    // 倒计时逻辑
    useEffect(() => {
        if (!intercept) {
            if (timerRef.current) {
                clearInterval(timerRef.current);
                timerRef.current = null;
            }
            return;
        }

        timerRef.current = setInterval(() => {
            setTimeLeft((prev) => {
                if (prev <= 1) {
                    if (timerRef.current) {
                        clearInterval(timerRef.current);
                        timerRef.current = null;
                    }
                    // 超时默认自动执行“本次在前面添加”（不写盘保留）
                    handleResolve('prepend_once', true);
                    return 0;
                }
                return prev - 1;
            });
        }, 1000);

        return () => {
            if (timerRef.current) {
                clearInterval(timerRef.current);
                timerRef.current = null;
            }
        };
    }, [intercept]);

    const handleResolve = useCallback(
        async (action: string, isTimeout: boolean = false) => {
            if (!intercept || submitting) return;
            setSubmitting(true);
            try {
                await invoke('resolve_prompt_intercept', {
                    id: intercept.id,
                    action,
                });
                if (isTimeout) {
                    showToast(t('proxy.prompt_intercept.action_prepend_once') + ' (Timeout Default)', 'info');
                }
            } catch (err) {
                console.error('[PromptInterceptDialog] Failed to resolve intercept:', err);
                showToast(String(err), 'error');
            } finally {
                setSubmitting(false);
                setIntercept(null);
            }
        },
        [intercept, submitting, t]
    );

    if (!intercept) {
        return null;
    }

    const modalContent = (
        <div className="fixed inset-0 z-[99999] flex items-center justify-center p-4 bg-black/60 backdrop-blur-sm animate-fade-in">
            <div className="relative w-full max-w-2xl bg-base-100 text-base-content rounded-2xl shadow-2xl border border-warning/30 overflow-hidden flex flex-col max-h-[90vh]">
                {/* 顶部标题栏 */}
                <div className="px-6 py-4 bg-warning/10 border-b border-warning/20 flex items-center justify-between">
                    <div className="flex items-center gap-3">
                        <div className="p-2 rounded-xl bg-warning/20 text-warning">
                            <AlertTriangle className="w-6 h-6 animate-pulse" />
                        </div>
                        <div>
                            <h3 className="text-lg font-bold flex items-center gap-2">
                                {t('proxy.prompt_intercept.title')}
                            </h3>
                            <p className="text-xs text-base-content/70 mt-0.5">
                                {t('proxy.prompt_intercept.subtitle')}
                            </p>
                        </div>
                    </div>
                    {/* 倒计时徽章 */}
                    <div className="flex items-center gap-1.5 px-3 py-1.5 rounded-full bg-base-200 border border-base-300 font-mono text-xs font-semibold text-warning">
                        <Clock className="w-3.5 h-3.5" />
                        <span>{timeLeft}s</span>
                    </div>
                </div>

                {/* 核心内容预览区域 */}
                <div className="p-6 overflow-y-auto space-y-4 text-sm">
                    {/* 元信息卡片 */}
                    <div className="flex flex-wrap gap-4 text-xs text-base-content/60 bg-base-200/50 p-2.5 rounded-lg border border-base-200">
                        {intercept.model && (
                            <div>
                                <span className="font-semibold">{t('proxy.prompt_intercept.target_model')}: </span>
                                <span className="font-mono text-primary font-medium">{intercept.model}</span>
                            </div>
                        )}
                        {intercept.clientIp && (
                            <div>
                                <span className="font-semibold">{t('proxy.prompt_intercept.client_ip')}: </span>
                                <span className="font-mono">{intercept.clientIp}</span>
                            </div>
                        )}
                    </div>

                    {/* 检测到的首句 */}
                    <div className="space-y-1.5">
                        <label className="text-xs font-semibold text-base-content/70 flex items-center gap-1.5">
                            <ArrowRight className="w-3.5 h-3.5 text-warning" />
                            {t('proxy.prompt_intercept.detected_sentence')}
                        </label>
                        <div className="p-3 bg-base-200 rounded-xl font-mono text-xs text-warning border border-warning/20 break-all select-text">
                            {intercept.firstSentence}
                        </div>
                    </div>

                    {/* 官方 Antigravity 身份声明预览 */}
                    <div className="space-y-1.5">
                        <label className="text-xs font-semibold text-base-content/70 flex items-center gap-1.5">
                            <ShieldCheck className="w-3.5 h-3.5 text-success" />
                            {t('proxy.prompt_intercept.official_identity')}
                        </label>
                        <div className="p-3 bg-success/5 rounded-xl font-mono text-xs text-success border border-success/20 select-text">
                            {intercept.officialIdentity}
                        </div>
                    </div>
                </div>

                {/* 六态处置操作按钮栏（前插、替换、放行 对称呈现“本次临时”与“记住持久化”） */}
                <div className="p-5 bg-base-200/40 border-t border-base-200 flex flex-col gap-3">
                    {/* 第一组：在前面添加（推荐方案，保留原提示词首句） */}
                    <div className="space-y-1.5">
                        <div className="flex items-center justify-between text-xs font-semibold text-base-content/75 px-0.5">
                            <span className="flex items-center gap-1.5 text-primary">
                                <Sparkles className="w-3.5 h-3.5" />
                                方案 A：在前面添加官方声明（最稳健 · 推荐）
                            </span>
                            <span className="text-[11px] text-warning font-normal">
                                超时默认选择本次生效
                            </span>
                        </div>
                        <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
                            <button
                                className="btn btn-primary justify-between shadow-sm hover:shadow"
                                onClick={() => handleResolve('prepend_once')}
                                disabled={submitting}
                            >
                                <span className="flex items-center gap-1.5 font-bold">
                                    <Clock className="w-4 h-4" />
                                    {t('proxy.prompt_intercept.action_prepend_once')}
                                </span>
                                <span className="badge badge-sm badge-warning font-normal">
                                    默认 ({timeLeft}s)
                                </span>
                            </button>
                            <button
                                className="btn btn-primary btn-outline justify-between"
                                onClick={() => handleResolve('prepend_and_remember')}
                                disabled={submitting}
                            >
                                <span className="flex items-center gap-1.5 font-semibold">
                                    <Bookmark className="w-3.5 h-3.5" />
                                    {t('proxy.prompt_intercept.action_prepend_and_remember')}
                                </span>
                                <span className="text-[11px] opacity-75">存入规则</span>
                            </button>
                        </div>
                    </div>

                    {/* 第二组：替换首句与直接放行（网格布局） */}
                    <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 pt-1">
                        {/* 替换组 */}
                        <div className="space-y-1.5">
                            <span className="text-[11px] font-medium text-base-content/60 px-0.5 block">
                                方案 B：替换首句为官方身份
                            </span>
                            <div className="grid grid-cols-2 gap-1.5">
                                <button
                                    className="btn btn-ghost btn-sm border border-base-300 hover:bg-base-300/50"
                                    onClick={() => handleResolve('replace_once')}
                                    disabled={submitting}
                                >
                                    {t('proxy.prompt_intercept.action_replace_once')}
                                </button>
                                <button
                                    className="btn btn-secondary btn-outline btn-sm"
                                    onClick={() => handleResolve('replace_and_remember')}
                                    disabled={submitting}
                                >
                                    {t('proxy.prompt_intercept.action_replace_and_remember')}
                                </button>
                            </div>
                        </div>

                        {/* 放行组 */}
                        <div className="space-y-1.5">
                            <span className="text-[11px] font-medium text-base-content/60 px-0.5 block">
                                方案 C：原样放行给上游
                            </span>
                            <div className="grid grid-cols-2 gap-1.5">
                                <button
                                    className="btn btn-ghost btn-sm border border-base-300 hover:bg-base-300/50"
                                    onClick={() => handleResolve('allow_once')}
                                    disabled={submitting}
                                >
                                    {t('proxy.prompt_intercept.action_allow_once')}
                                </button>
                                <button
                                    className="btn btn-accent btn-outline btn-sm"
                                    onClick={() => handleResolve('allow_and_remember')}
                                    disabled={submitting}
                                >
                                    {t('proxy.prompt_intercept.action_allow_and_remember')}
                                </button>
                            </div>
                        </div>
                    </div>
                </div>
            </div>
        </div>
    );

    return createPortal(modalContent, document.body);
};

export default PromptInterceptDialog;
