import { useState, memo } from 'react';
import { createPortal } from 'react-dom';
import { 
    Fingerprint, RefreshCw, 
    X, AlertTriangle, Loader2, Info
} from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { invoke } from '@/shared/api';
import { cn } from '@/shared/lib';

// Types (mirrored from backend)
interface DeviceInfo {
    deviceId: string;
    userAgent: string;
    screenWidth: number;
    screenHeight: number;
    platform: string;
    language: string;
    timezone: string;
    hardwareConcurrency: number;
    deviceMemory: number;
    webglVendor: string;
    webglRenderer: string;
    canvasData: string;
    audioData: string;
    fonts: string[];
    plugins: string[];
    batteryLevel?: number;
    isCharging?: boolean;
    creationTime: number; // unix timestamp
    lastUsed: number;     // unix timestamp
    requestsCount: number;
}

interface DeviceFingerprintDialogProps {
    accountId: string;
    accountEmail: string;
    onClose: () => void;
}

export const DeviceFingerprintDialog = memo(({ accountId, accountEmail, onClose }: DeviceFingerprintDialogProps) => {
    const { t } = useTranslation();
    const queryClient = useQueryClient();
    const [status, setStatus] = useState<string>('');

    // Queries & Mutations
    const fingerprintQuery = useQuery({
        queryKey: ['device-fingerprint', accountId],
        queryFn: () => invoke<DeviceInfo>('get_account_fingerprint', { accountId }),
        staleTime: 5 * 60 * 1000,
    });
    
    // For restoration/history (if implemented later, uncomment historyQuery and restoreMutation)
    /*
    const historyQuery = useQuery({
        queryKey: ['device-fingerprint-history', accountId],
        queryFn: () => invoke<DeviceInfo[]>('get_fingerprint_history', { accountId }),
        enabled: false, 
    });
    */

    const generateNewMutation = useMutation({
        mutationFn: () => invoke<DeviceInfo>('rotate_fingerprint', { accountId }),
        onSuccess: (newData) => {
            queryClient.setQueryData(['device-fingerprint', accountId], newData);
            setStatus('generated');
            setTimeout(() => setStatus(''), 2000);
        },
    });

    /*
    const restoreMutation = useMutation({
        mutationFn: (fingerprint: DeviceInfo) => 
            invoke('restore_fingerprint', { accountId, fingerprint }),
        onSuccess: () => {
            queryClient.invalidateQueries({ queryKey: ['device-fingerprint', accountId] });
            setStatus('restored');
            setTimeout(() => setStatus(''), 2000);
        },
    });
    */

    const isLoading = fingerprintQuery.isLoading;
    const data = fingerprintQuery.data;

    // Helper to format values
    const formatValue = (key: string, val: any) => {
        if (key === 'creationTime' || key === 'lastUsed') {
            return new Date(val * 1000).toLocaleString();
        }
        if (typeof val === 'boolean') return val ? 'Yes' : 'No';
        if (Array.isArray(val)) return `${val.length} items`;
        if (key === 'deviceId') return <span className="font-mono text-purple-400">{String(val).substring(0, 16)}...</span>;
        return String(val);
    };

    return createPortal(
        <div className="fixed inset-0 z-[100] flex items-center justify-center p-4">
             {/* Backdrop */}
             <div 
                className="absolute inset-0 bg-black/60 backdrop-blur-sm transition-opacity"
                onClick={onClose}
            />

            {/* Modal */}
            <div className="relative w-full max-w-2xl bg-zinc-900 border border-zinc-800 rounded-2xl shadow-2xl overflow-hidden flex flex-col max-h-[90vh]">
                {/* Header */}
                <div className="flex items-center justify-between px-6 py-4 border-b border-zinc-800 bg-zinc-900/50">
                    <div className="flex items-center gap-3">
                        <div className="p-2 rounded-lg bg-indigo-500/10 text-indigo-400">
                            <Fingerprint className="w-5 h-5" />
                        </div>
                        <div>
                            <h3 className="font-bold text-white text-lg">{t('accounts.device_fingerprint')}</h3>
                            <p className="text-xs text-zinc-500 font-mono">{accountEmail}</p>
                        </div>
                    </div>
                    <button 
                        onClick={onClose}
                        className="p-2 rounded-lg text-zinc-500 hover:text-white hover:bg-zinc-800 transition-colors"
                    >
                        <X className="w-5 h-5" />
                    </button>
                </div>

                {/* Content */}
                <div className="flex-1 overflow-y-auto p-6 scrollbar-thin scrollbar-thumb-zinc-700 scrollbar-track-transparent">
                    {isLoading ? (
                        <div className="flex flex-col items-center justify-center py-20 text-zinc-500">
                            <Loader2 className="w-8 h-8 animate-spin mb-2" />
                            <p>Loading fingerprint data...</p>
                        </div>
                    ) : !data ? (
                        <div className="flex flex-col items-center justify-center py-20 text-zinc-500">
                            <AlertTriangle className="w-10 h-10 mb-3 text-amber-500/50" />
                            <p>No fingerprint data found for this account.</p>
                            <button
                                onClick={() => generateNewMutation.mutate()}
                                className="mt-4 px-4 py-2 bg-indigo-500 text-white rounded-lg hover:bg-indigo-600 transition-colors"
                            >
                                Generate New Fingerprint
                            </button>
                        </div>
                    ) : (
                        <div className="space-y-6">
                            {/* Actions Bar */}
                            <div className="flex items-center justify-between p-4 bg-zinc-800/30 rounded-xl border border-zinc-800">
                                <div className="flex items-center gap-2">
                                    <div className={cn(
                                        "w-2 h-2 rounded-full",
                                        status === 'generated' ? "bg-emerald-500 animate-pulse" : "bg-zinc-600"
                                    )} />
                                    <span className="text-xs font-medium text-zinc-400">
                                        {status === 'generated' ? 'New fingerprint active' : 'Fingerprint active'}
                                    </span>
                                </div>
                                <div className="flex items-center gap-2">
                                    <button
                                        onClick={() => generateNewMutation.mutate()}
                                        disabled={generateNewMutation.isPending}
                                        className="px-3 py-1.5 text-xs font-medium bg-indigo-500/10 text-indigo-400 border border-indigo-500/20 rounded-lg hover:bg-indigo-500/20 transition-colors flex items-center gap-1.5"
                                    >
                                        <RefreshCw className={cn("w-3.5 h-3.5", generateNewMutation.isPending && "animate-spin")} />
                                        Rotation
                                    </button>
                                </div>
                            </div>

                            {/* Main Info Grid */}
                            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                                <InfoCard 
                                    label="Platform" 
                                    value={data.platform} 
                                    sub={data.userAgent.split(') ')[0] + ')'}
                                    icon={<Info className="w-3.5 h-3.5" />}
                                />
                                <InfoCard 
                                    label="Screen" 
                                    value={`${data.screenWidth}x${data.screenHeight}`} 
                                    sub={`WebGL: ${data.webglRenderer.split('DirectX')[0]}`} // Shorten commonly long string for UI
                                />
                                <InfoCard 
                                    label="Hardware" 
                                    value={`Cores: ${data.hardwareConcurrency}`} 
                                    sub={`RAM: ~${data.deviceMemory}GB`} 
                                />
                                <InfoCard 
                                    label="Locale" 
                                    value={data.language} 
                                    sub={data.timezone} 
                                />
                            </div>

                            {/* Detailed JSON View (Collapsed by default logic typically, but showing important bits here) */}
                            <div className="space-y-2">
                                <h4 className="text-xs font-bold text-zinc-500 uppercase tracking-wider">Raw Parameters</h4>
                                <div className="bg-black/30 rounded-xl border border-zinc-800 p-4 font-mono text-xs text-zinc-400 overflow-x-auto">
                                    <table className="w-full text-left">
                                        <tbody>
                                            {Object.entries(data).map(([key, val]) => {
                                                if (['canvasData', 'audioData', 'fonts', 'plugins'].includes(key)) return null; // Skip large data
                                                return (
                                                    <tr key={key} className="border-b border-zinc-800/50 last:border-0 hover:bg-white/5 transition-colors">
                                                        <td className="py-2 pr-4 text-zinc-500">{key}</td>
                                                        <td className="py-2 text-zinc-300">{formatValue(key, val)}</td>
                                                    </tr>
                                                );
                                            })}
                                        </tbody>
                                    </table>
                                </div>
                            </div>

                            {/* History Section (Placeholder / Future) */}
                            {/* <div className="space-y-2 opacity-50">
                                <h4 className="text-xs font-bold text-zinc-500 uppercase tracking-wider flex items-center gap-2">
                                    History <span className="text-[10px] font-normal lowercase">(coming soon)</span>
                                </h4>
                            </div> */}
                        </div>
                    )}
                </div>
            </div>
            
            <div data-tauri-drag-region className="fixed top-0 left-0 right-0 h-8 z-10" />
        </div>,
        document.body
    );
});

function InfoCard({ label, value, sub, icon }: any) {
    return (
        <div className="p-3 bg-zinc-800/30 border border-zinc-800 rounded-xl">
            <div className="flex items-center gap-2 mb-1 text-zinc-500 text-xs font-medium uppercase tracking-wider">
                {icon}
                {label}
            </div>
            <div className="text-sm font-semibold text-zinc-200 truncate" title={String(value)}>{value}</div>
            {sub && <div className="text-xs text-zinc-500 truncate" title={String(sub)}>{sub}</div>}
        </div>
    );
}
