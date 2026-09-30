import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { request as invoke } from '../../utils/request';
import { Search, AlertTriangle } from 'lucide-react';

interface IpAccessLog {
    id: string;
    client_ip: string;
    timestamp: number;
    method?: string;
    path?: string;
    user_agent?: string;
    status?: number;
    duration?: number;
    api_key_hash?: string;
    blocked: boolean;
    block_reason?: string;
    username?: string;
}

interface IpAccessLogResponse {
    logs: IpAccessLog[];
    total: number;
}

interface Props {
    refreshKey?: number;
}

export const IpAccessLogs: React.FC<Props> = ({ refreshKey }) => {
    const { t } = useTranslation();
    const [logs, setLogs] = useState<IpAccessLog[]>([]);
    const [total, setTotal] = useState(0);
    const [loading, setLoading] = useState(false);
    const [page, setPage] = useState(1);
    const [pageSize, setPageSize] = useState(50);
    const [search, setSearch] = useState('');
    const [blockedOnly, setBlockedOnly] = useState(false);

    const loadLogs = async () => {
        setLoading(true);
        try {
            const res = await invoke<IpAccessLogResponse>('get_ip_access_logs', {
                page,
                pageSize: pageSize,
                search: search || undefined,
                blockedOnly: blockedOnly,
            });
            setLogs(res.logs);
            setTotal(res.total);
        } catch (e) {
            console.error('Failed to load logs', e);
        } finally {
            setLoading(false);
        }
    };

    useEffect(() => {
        loadLogs();
    }, [page, pageSize, blockedOnly, refreshKey]);

    // Handle search on enter or blur
    const handleSearch = () => {
        setPage(1);
        loadLogs();
    };

    return (
        <div className="flex flex-col h-full bg-white dark:bg-base-100 rounded-xl">
            {/* Toolbar */}
            <div className="p-5 border-b border-gray-100 dark:border-base-200 flex flex-wrap items-center gap-6">
                <div className="relative flex-1 min-w-[200px] max-w-md">
                    <Search className="absolute left-3 top-2.5 text-gray-400" size={16} />
                    <input
                        type="text"
                        placeholder={t('security.logs.search_placeholder')}
                        className="input input-sm input-bordered w-full pl-9"
                        value={search}
                        onChange={(e) => setSearch(e.target.value)}
                        onKeyDown={(e) => e.key === 'Enter' && handleSearch()}
                        onBlur={handleSearch}
                    />
                </div>

                <label className="label cursor-pointer gap-2 shrink-0">
                    <span className="label-text text-xs font-bold text-gray-500 uppercase">{t('security.logs.show_blocked_only')}</span>
                    <input
                        type="checkbox"
                        className="toggle toggle-sm toggle-error"
                        checked={blockedOnly}
                        onChange={(e) => setBlockedOnly(e.target.checked)}
                    />
                </label>

                <div className="flex-1"></div>

                <div className="flex items-center gap-3 shrink-0">
                    <select
                        className="select select-sm select-bordered min-w-[100px]"
                        value={pageSize}
                        onChange={(e) => setPageSize(Number(e.target.value))}
                    >
                        <option value="20">20{t('security.logs.per_page_suffix')}</option>
                        <option value="50">50{t('security.logs.per_page_suffix')}</option>
                        <option value="100">100{t('security.logs.per_page_suffix')}</option>
                    </select>
                </div>
            </div>

            {/* Table */}
            <div className="flex-1 overflow-auto">
                <table className="table table-xs w-full">
                    <thead className="sticky top-0 bg-gray-100 dark:bg-base-200 z-10 shadow-sm text-gray-600 dark:text-gray-400">
                        <tr>
                            <th className="w-20">{t('security.logs.status')}</th>
                            <th className="w-32">{t('security.logs.ip_address')}</th>
                            <th className="w-24">{t('security.logs.username', 'User')}</th>
                            <th className="w-20">{t('security.logs.method')}</th>
                            <th className="">{t('security.logs.path')}</th>
                            <th className="w-24 text-right">{t('security.logs.duration')}</th>
                            <th className="w-32 text-right">{t('security.logs.time')}</th>
                            <th className="w-40">{t('security.logs.reason')}</th>
                        </tr>
                    </thead>
                    <tbody>
                        {logs.map((log) => (
                            <tr key={log.id} className="hover:bg-gray-50 dark:hover:bg-base-200">
                                <td>
                                    {log.blocked ? (
                                        <span className="badge badge-xs bg-rose-600 text-white font-bold border-none shadow-xs gap-1">
                                            <AlertTriangle size={10} /> {t('security.logs.blocked')}
                                        </span>
                                    ) : (
                                        <span className={`badge badge-xs text-white font-bold border-none shadow-xs ${log.status && log.status >= 200 && log.status < 400 ? 'bg-emerald-600' : 'bg-amber-600'}`}>
                                            {log.status || '-'}
                                        </span>
                                    )}
                                </td>
                                <td className="font-mono font-medium">{log.client_ip}</td>
                                <td className="font-medium text-blue-600 dark:text-blue-400">{log.username || '-'}</td>
                                <td className="font-bold text-xs">{log.method || '-'}</td>
                                <td className="max-w-xs truncate text-gray-600 dark:text-gray-400" title={log.path}>{log.path || '-'}</td>
                                <td className="text-right font-mono">{log.duration ? `${log.duration}ms` : '-'}</td>
                                <td className="text-right text-xs text-gray-500">{new Date(log.timestamp * 1000).toLocaleString()}</td>
                                <td className="text-xs text-red-500 truncate" title={log.block_reason}>{log.block_reason}</td>
                            </tr>
                        ))}
                        {!loading && logs.length === 0 && (
                            <tr>
                                <td colSpan={8} className="text-center py-10 text-gray-400">
                                    {t('security.logs.no_logs')}
                                </td>
                            </tr>
                        )}
                    </tbody>
                </table>
            </div>

            {/* Pagination */}
            <div className="p-3 border-t border-gray-100 dark:border-base-200 flex items-center justify-between text-xs text-gray-500 bg-gray-50 dark:bg-base-200">
                <span>{t('security.logs.total_records', { total })}</span>
                <div className="flex gap-2">
                    <button
                        className="btn btn-xs"
                        disabled={page <= 1}
                        onClick={() => setPage(p => p - 1)}
                    >
                        {t('security.logs.prev_page')}
                    </button>
                    <button className="btn btn-xs btn-active">{t('security.logs.page_num', { page })}</button>
                    <button
                        className="btn btn-xs"
                        disabled={logs.length < pageSize}
                        onClick={() => setPage(p => p + 1)}
                    >
                        {t('security.logs.next_page')}
                    </button>
                </div>
            </div>
        </div>
    );
};
