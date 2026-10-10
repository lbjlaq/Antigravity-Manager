import React, { useState, useEffect, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { Radio, Laptop, Sun, Flame, Clock, Zap, Sparkles, RefreshCw } from 'lucide-react';
import { PhaseSchedulerConfig } from '../../types/config';
import { request as invoke } from '../../utils/request';

interface PhaseSchedulerProps {
    config?: PhaseSchedulerConfig;
    onChange: (config: PhaseSchedulerConfig) => void;
}

export const PhaseScheduler: React.FC<PhaseSchedulerProps> = ({ config, onChange }) => {
    const { t } = useTranslation();

    const defaultConfig: PhaseSchedulerConfig = {
        enabled: false,
        mode: 'steady',
        work_start_time: '09:00',
        work_duration_hours: 12,
        burst_mode_type: 'scheduled',
        burst_start_time: '20:00',
        burst_duration_hours: 3,
        auto_dark_wake: true,
        monitored_models: ['gemini-3-flash', 'claude', 'gemini-3-pro-high'],
    };

    const current = config || defaultConfig;
    const [activeIdeEmail, setActiveIdeEmail] = useState<string | null>(null);
    const [availableCount, setAvailableCount] = useState<number>(5);
    const [detecting, setDetecting] = useState<boolean>(false);

    const checkActiveIde = useCallback(async () => {
        setDetecting(true);
        try {
            const acc = await invoke<any>('get_active_ide_account');
            if (acc && acc.email) {
                setActiveIdeEmail(acc.email);
            } else {
                setActiveIdeEmail(null);
            }
        } catch (e) {
            console.warn('[PhaseScheduler] get_active_ide_account error:', e);
            setActiveIdeEmail(null);
        } finally {
            setDetecting(false);
        }

        try {
            const accounts = await invoke<any[]>('list_accounts');
            if (Array.isArray(accounts)) {
                const validAccounts = accounts.filter(a => !a.disabled && !a.proxy_disabled);
                setAvailableCount(Math.max(1, validAccounts.length - (activeIdeEmail ? 1 : 0)));
            }
        } catch (e) {
            console.warn('[PhaseScheduler] list_accounts error:', e);
        }
    }, [activeIdeEmail]);

    useEffect(() => {
        checkActiveIde();
    }, [checkActiveIde]);

    const handleToggle = (enabled: boolean) => {
        onChange({ ...current, enabled });
    };

    const handleModeChange = (mode: 'steady' | 'burst') => {
        onChange({ ...current, mode });
    };

    const handleStartTimeChange = (e: React.ChangeEvent<HTMLInputElement>) => {
        onChange({ ...current, work_start_time: e.target.value });
    };

    const handleWorkDurationChange = (hours: number) => {
        onChange({ ...current, work_duration_hours: hours });
    };

    const handleBurstDurationChange = (hours: number) => {
        onChange({ ...current, burst_duration_hours: hours });
    };

    const handleAutoDarkWakeToggle = (auto_dark_wake: boolean) => {
        onChange({ ...current, auto_dark_wake });
    };

    const calcTimes = (startTimeStr: string, durationHours: number) => {
        const [hStr, mStr] = (startTimeStr || '20:00').split(':');
        const h = parseInt(hStr || '20', 10);
        const m = parseInt(mStr || '0', 10);

        const endH = (h + durationHours) % 24;
        const endStr = `${String(endH).padStart(2, '0')}:${String(m).padStart(2, '0')}`;

        const preH = (h - 5 + 24) % 24;
        const preStr = `${String(preH).padStart(2, '0')}:${String(m).padStart(2, '0')}`;

        return { endStr, preStr };
    };

    const deltaMins = Math.round(300 / Math.max(1, availableCount));

    return (
        <div className="space-y-4">
            <div className="flex items-center justify-between">
                <div className="flex items-center gap-4">
                    <div className={`w-10 h-10 rounded-xl flex items-center justify-center transition-all duration-300 ${current.enabled
                        ? 'bg-blue-600 text-white shadow-lg shadow-blue-500/20'
                        : 'bg-blue-50 dark:bg-blue-900/20 text-blue-500 group-hover:bg-blue-600 group-hover:text-white'
                        }`}>
                        <Radio size={20} />
                    </div>
                    <div>
                        <div className="font-bold text-gray-900 dark:text-gray-100 flex items-center gap-2">
                            {t('settings.phase_scheduler.title', '多账号相控阵智能错峰调度中心')}
                            {current.enabled && (
                                <span className={`text-[10px] px-2 py-0.5 rounded-full font-semibold ${current.mode === 'burst'
                                    ? 'bg-red-100 dark:bg-red-900/40 text-red-600 dark:text-red-300'
                                    : 'bg-blue-100 dark:bg-blue-900/40 text-blue-600 dark:text-blue-300'
                                    }`}>
                                    {current.mode === 'burst'
                                        ? (current.burst_mode_type === 'immediate'
                                            ? t('settings.phase_scheduler.mode_burst_imm', '狂暴爆发 (即时)')
                                            : t('settings.phase_scheduler.mode_burst_sch', '狂暴爆发 (预约)'))
                                        : t('settings.phase_scheduler.mode_steady', '平稳续航错峰')}
                                </span>
                            )}
                        </div>
                        <p className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
                            {t('settings.phase_scheduler.desc', '自动隔离反重力IDE主号，根据池内账号动态计算Δ错峰步长，结合Mac暗唤醒保障全天Token不断流')}
                        </p>
                    </div>
                </div>
                <label className="relative inline-flex items-center cursor-pointer">
                    <input
                        type="checkbox"
                        className="sr-only peer"
                        checked={current.enabled}
                        onChange={(e) => handleToggle(e.target.checked)}
                    />
                    <div className="w-11 h-6 bg-gray-200 dark:bg-base-300 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-blue-600 shadow-inner"></div>
                </label>
            </div>

            {/* 反重力IDE主号自动保护指示栏 */}
            <div className="flex items-center justify-between px-3.5 py-2.5 bg-gray-50 dark:bg-base-200/50 rounded-lg border border-gray-100 dark:border-base-300 text-xs text-gray-600 dark:text-gray-400">
                <div className="flex items-center gap-2">
                    <Laptop size={15} className="text-blue-500" />
                    <span>反重力IDE主号状态:</span>
                    {activeIdeEmail ? (
                        <span className="font-mono text-blue-600 dark:text-blue-400 font-medium">
                            {activeIdeEmail} (平时最低优先级独占保护；其他账号全无额度时自动兜底出战)
                        </span>
                    ) : (
                        <span className="text-gray-400">未检测到活跃IDE客户端，全账号供API反代调度</span>
                    )}
                    <button
                        type="button"
                        onClick={checkActiveIde}
                        className="btn btn-ghost btn-xs text-gray-400 hover:text-blue-500 p-1"
                        title="重新检测活跃反重力IDE主号"
                    >
                        <RefreshCw size={12} className={detecting ? 'animate-spin' : ''} />
                    </button>
                </div>
                <div className="text-[11px] font-mono text-gray-500">
                    API池可用: {availableCount} 个 (错峰步长: Δ = {deltaMins} 分钟)
                </div>
            </div>

            {current.enabled && (
                <div className="mt-4 pt-4 border-t border-gray-100 dark:border-base-300 animate-in slide-in-from-top-2 duration-300 space-y-4">
                    {/* 模式选择 */}
                    <div>
                        <label className="text-xs font-semibold text-gray-600 dark:text-gray-400 mb-2 block">
                            {t('settings.phase_scheduler.mode_label', '工作调度模式')}
                        </label>
                        <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                            {/* 平稳续航模式 */}
                            <button
                                type="button"
                                onClick={() => handleModeChange('steady')}
                                className={`p-3 rounded-lg border text-left transition-all flex flex-col justify-between ${current.mode === 'steady'
                                    ? 'border-blue-500 bg-blue-50/50 dark:bg-blue-900/20 shadow-sm'
                                    : 'border-gray-200 dark:border-base-300 hover:border-gray-300 dark:hover:border-gray-600'
                                    }`}
                            >
                                <div className="flex items-center gap-2 mb-1">
                                    <Sun size={16} className={current.mode === 'steady' ? 'text-blue-600' : 'text-gray-400'} />
                                    <span className="font-bold text-xs text-gray-800 dark:text-gray-200">
                                        {t('settings.phase_scheduler.steady_title', '平稳续航错峰模式')}
                                    </span>
                                </div>
                                <p className="text-[11px] text-gray-500 dark:text-gray-400">
                                    {t('settings.phase_scheduler.steady_desc', '设置开工时间与时长，系统按 300/N 步长交错排列刷新相位，适合日常稳定编程')}
                                </p>
                            </button>

                            {/* 限定时间狂暴模式 */}
                            <button
                                type="button"
                                onClick={() => handleModeChange('burst')}
                                className={`p-3 rounded-lg border text-left transition-all flex flex-col justify-between ${current.mode === 'burst'
                                    ? 'border-red-500 bg-red-50/50 dark:bg-red-900/20 shadow-sm'
                                    : 'border-gray-200 dark:border-base-300 hover:border-gray-300 dark:hover:border-gray-600'
                                    }`}
                            >
                                <div className="flex items-center gap-2 mb-1">
                                    <Flame size={16} className={current.mode === 'burst' ? 'text-red-600' : 'text-gray-400'} />
                                    <span className="font-bold text-xs text-gray-800 dark:text-gray-200">
                                        {t('settings.phase_scheduler.burst_title', '限定时间狂暴爆发模式')}
                                    </span>
                                </div>
                                <p className="text-[11px] text-gray-500 dark:text-gray-400">
                                    {t('settings.phase_scheduler.burst_desc', '支持预约时间提前5小时自动对齐探活，并在爆发窗口内解锁双倍额度刷新，吞吐量翻倍')}
                                </p>
                            </button>
                        </div>
                    </div>

                    {/* 平稳模式专属设置 */}
                    {current.mode === 'steady' && (
                        <div className="bg-gray-50/50 dark:bg-base-200/30 p-3.5 rounded-lg border border-gray-100 dark:border-base-300 space-y-3">
                            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                                <div>
                                    <label className="text-xs font-semibold text-gray-700 dark:text-gray-300 block mb-1">
                                        开工基准时间 (每天)
                                    </label>
                                    <input
                                        type="time"
                                        className="input input-sm input-bordered w-full"
                                        value={current.work_start_time || '09:00'}
                                        onChange={handleStartTimeChange}
                                    />
                                </div>
                                <div>
                                    <label className="text-xs font-semibold text-gray-700 dark:text-gray-300 block mb-1">
                                        计划连续工作时长
                                    </label>
                                    <div className="flex gap-2">
                                        {[8, 10, 12, 14].map((hrs) => (
                                            <button
                                                key={hrs}
                                                type="button"
                                                onClick={() => handleWorkDurationChange(hrs)}
                                                className={`btn btn-xs flex-1 ${current.work_duration_hours === hrs ? 'btn-primary' : 'btn-ghost border border-gray-200 dark:border-base-300'}`}
                                            >
                                                {hrs} 小时
                                            </button>
                                        ))}
                                    </div>
                                </div>
                            </div>

                            {/* Mac暗唤醒 */}
                            <div className="flex items-center justify-between pt-2 border-t border-gray-100 dark:border-base-300">
                                <div>
                                    <div className="text-xs font-semibold text-gray-700 dark:text-gray-300">
                                        Mac 原生硬件暗唤醒 (RTC DarkWake)
                                    </div>
                                    <p className="text-[10px] text-gray-400">
                                        在凌晨错峰打卡前自动唤醒 Mac 执行零风控探活，无需整夜保持屏幕常亮
                                    </p>
                                </div>
                                <input
                                    type="checkbox"
                                    className="toggle toggle-sm toggle-primary"
                                    checked={current.auto_dark_wake}
                                    onChange={(e) => handleAutoDarkWakeToggle(e.target.checked)}
                                />
                            </div>
                        </div>
                    )}

                    {/* 狂暴模式专属设置 (支持预约时间与提前5小时相位对齐推演) */}
                    {current.mode === 'burst' && (
                        <div className="bg-red-50/30 dark:bg-red-950/20 p-3.5 rounded-lg border border-red-100 dark:border-red-900/30 space-y-4">
                            <div>
                                <label className="text-xs font-semibold text-gray-700 dark:text-gray-300 block mb-2">
                                    狂暴爆发启动机制
                                </label>
                                <div className="grid grid-cols-2 gap-3">
                                    <button
                                        type="button"
                                        onClick={() => onChange({ ...current, burst_mode_type: 'scheduled' })}
                                        className={`p-2.5 rounded-lg border text-left transition-all ${
                                            (current.burst_mode_type || 'scheduled') === 'scheduled'
                                                ? 'border-red-500 bg-red-100/50 dark:bg-red-900/40 text-red-700 dark:text-red-200 shadow-sm'
                                                : 'border-gray-200 dark:border-base-300 hover:border-gray-300 text-gray-600 dark:text-gray-400'
                                        }`}
                                    >
                                        <div className="font-bold text-xs flex items-center gap-1.5 mb-0.5">
                                            <Clock size={14} className="text-red-500" />
                                            预约爆发 (推荐)
                                        </div>
                                        <div className="text-[10px] opacity-80">
                                            指定开工时间，系统提前 5 小时自动打卡，实现双倍额度刷新
                                        </div>
                                    </button>
                                    <button
                                        type="button"
                                        onClick={() => onChange({ ...current, burst_mode_type: 'immediate' })}
                                        className={`p-2.5 rounded-lg border text-left transition-all ${
                                            current.burst_mode_type === 'immediate'
                                                ? 'border-red-500 bg-red-100/50 dark:bg-red-900/40 text-red-700 dark:text-red-200 shadow-sm'
                                                : 'border-gray-200 dark:border-base-300 hover:border-gray-300 text-gray-600 dark:text-gray-400'
                                        }`}
                                    >
                                        <div className="font-bold text-xs flex items-center gap-1.5 mb-0.5">
                                            <Zap size={14} className="text-red-500" />
                                            立即爆发
                                        </div>
                                        <div className="text-[10px] opacity-80">
                                            从当前时刻起全速并发，集中调用当前周余量最高的账号
                                        </div>
                                    </button>
                                </div>
                            </div>

                            {/* 预约爆发专属面板 */}
                            {(current.burst_mode_type || 'scheduled') === 'scheduled' && (
                                <div className="space-y-3 pt-2 border-t border-red-200/50 dark:border-red-800/30">
                                    <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                                        <div>
                                            <label className="text-xs font-semibold text-gray-700 dark:text-gray-300 block mb-1">
                                                预约开工时间 (爆发开始时点)
                                            </label>
                                            <input
                                                type="time"
                                                className="input input-sm input-bordered w-full"
                                                value={current.burst_start_time || '20:00'}
                                                onChange={(e) => onChange({ ...current, burst_start_time: e.target.value })}
                                            />
                                        </div>
                                        <div>
                                            <label className="text-xs font-semibold text-gray-700 dark:text-gray-300 block mb-1">
                                                爆发持续时长
                                            </label>
                                            <div className="flex gap-2">
                                                {[1, 2, 3, 4, 5].map((hrs) => (
                                                    <button
                                                        key={hrs}
                                                        type="button"
                                                        onClick={() => handleBurstDurationChange(hrs)}
                                                        className={`btn btn-xs flex-1 ${
                                                            current.burst_duration_hours === hrs
                                                                ? 'btn-error text-white'
                                                                : 'btn-ghost border border-gray-200 dark:border-base-300'
                                                        }`}
                                                    >
                                                        {hrs} 小时
                                                    </button>
                                                ))}
                                            </div>
                                        </div>
                                    </div>

                                    {/* 相控阵提前5h对齐推演面板 */}
                                    {(() => {
                                        const { endStr, preStr } = calcTimes(current.burst_start_time || '20:00', current.burst_duration_hours || 3);
                                        return (
                                            <div className="p-3 bg-red-100/40 dark:bg-red-950/40 rounded-lg border border-red-200/60 dark:border-red-800/40 text-xs space-y-1.5">
                                                <div className="font-semibold text-red-800 dark:text-red-300 flex items-center gap-1.5">
                                                    <Sparkles size={14} className="text-red-500" />
                                                    相控阵双倍吞吐排班推演
                                                </div>
                                                <div className="grid grid-cols-1 sm:grid-cols-3 gap-2 pt-1 text-[11px]">
                                                    <div className="bg-white/70 dark:bg-base-100/60 p-2 rounded border border-red-100 dark:border-red-900/30">
                                                        <span className="text-gray-500 block">① 提前5h对齐打卡</span>
                                                        <span className="font-bold text-red-600 dark:text-red-400 font-mono text-sm">{preStr}</span>
                                                        <span className="text-[10px] text-gray-400 block">自动探活高周余量账号</span>
                                                    </div>
                                                    <div className="bg-white/70 dark:bg-base-100/60 p-2 rounded border border-red-100 dark:border-red-900/30">
                                                        <span className="text-gray-500 block">② 满血就绪开工</span>
                                                        <span className="font-bold text-blue-600 dark:text-blue-400 font-mono text-sm">{current.burst_start_time || '20:00'}</span>
                                                        <span className="text-[10px] text-gray-400 block">5h额度归零满血重置</span>
                                                    </div>
                                                    <div className="bg-white/70 dark:bg-base-100/60 p-2 rounded border border-red-100 dark:border-red-900/30">
                                                        <span className="text-gray-500 block">③ 爆发结束时点</span>
                                                        <span className="font-bold text-gray-700 dark:text-gray-200 font-mono text-sm">{endStr}</span>
                                                        <span className="text-[10px] text-emerald-600 dark:text-emerald-400 block font-medium">中途解锁二次刷新</span>
                                                    </div>
                                                </div>
                                            </div>
                                        );
                                    })()}
                                </div>
                            )}

                            {/* 即时爆发面板 */}
                            {current.burst_mode_type === 'immediate' && (
                                <div className="space-y-3 pt-2 border-t border-red-200/50 dark:border-red-800/30">
                                    <div>
                                        <label className="text-xs font-semibold text-gray-700 dark:text-gray-300 block mb-1">
                                            即时爆发持续时长
                                        </label>
                                        <div className="flex gap-2">
                                            {[1, 2, 3, 4, 5].map((hrs) => (
                                                <button
                                                    key={hrs}
                                                    type="button"
                                                    onClick={() => handleBurstDurationChange(hrs)}
                                                    className={`btn btn-xs flex-1 ${
                                                        current.burst_duration_hours === hrs
                                                            ? 'btn-error text-white'
                                                            : 'btn-ghost border border-gray-200 dark:border-base-300'
                                                    }`}
                                                >
                                                    {hrs} 小时
                                                </button>
                                            ))}
                                        </div>
                                    </div>
                                    <div className="p-2.5 bg-red-100/40 dark:bg-red-950/40 rounded border border-red-200/60 dark:border-red-800/40 text-[11px] text-red-700 dark:text-red-300">
                                        🔥 即时模式：将立即从池中筛选当前剩余额度最高、周余量充足的账号，以无锁最高并发直接全力供给 API 代理。
                                    </div>
                                </div>
                            )}
                        </div>
                    )}
                </div>
            )}
        </div>
    );
};

export default PhaseScheduler;
