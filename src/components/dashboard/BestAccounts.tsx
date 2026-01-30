import { TrendingUp } from 'lucide-react';
import { Account } from '../../types/account';
import { Card, CardContent, CardHeader, CardTitle } from '../ui/card';
import { Progress } from '../ui/progress';
import { Badge } from '../ui/badge';
import { cn } from '../../lib/utils';
import { formatTimeRemaining } from '../../utils/format';
import { useTranslation } from 'react-i18next';
import { memo } from 'react';

interface BestAccountsProps {
    accounts: Account[];
    currentAccountId?: string;
    onSwitch?: (accountId: string) => void;
}

const BestAccounts = function BestAccounts({ accounts, currentAccountId, onSwitch }: BestAccountsProps) {
    const { t } = useTranslation();

    const candidates = accounts
        .filter(a => a.id !== currentAccountId && !a.disabled)
        .map(a => {
            const getModel = (namePart: string) => a.quota?.models.find(m => m.name.toLowerCase().includes(namePart));
            
            const geminiProModel = a.quota?.models.find(m => m.name.toLowerCase() === 'gemini-3-pro-high');
            const claudeModel = getModel('claude');
            const geminiFlashModel = a.quota?.models.find(m => m.name.toLowerCase() === 'gemini-3-flash');

            const geminiPro = geminiProModel?.percentage || 0;
            const claude = claudeModel?.percentage || 0;
            const geminiFlash = geminiFlashModel?.percentage || 0;
            
            const score = (geminiPro * 0.4) + (claude * 0.4) + (geminiFlash * 0.2);
            
            return {
                ...a,
                score: Math.round(score),
                metrics: { 
                    geminiPro, 
                    geminiProReset: geminiProModel?.reset_time,
                    claude, 
                    claudeReset: claudeModel?.reset_time,
                    geminiFlash,
                    geminiFlashReset: geminiFlashModel?.reset_time
                }
            };
        })
        .filter(a => a.score > 10)
        .sort((a, b) => b.score - a.score)
        .slice(0, 3);

    return (
        <Card className="h-full flex flex-col shadow-xl border-zinc-200 dark:border-white/5 bg-white dark:bg-zinc-900 relative overflow-hidden transition-all hover:border-white/10">
             {/* Background Decoration */}
             <div className="absolute inset-0 bg-gradient-to-b from-white/[0.02] to-transparent pointer-events-none" />
             
            <CardHeader className="pb-4 pt-6 px-6 border-b border-white/5 relative z-10">
                <CardTitle className="text-sm font-medium flex items-center gap-2 text-zinc-400 uppercase tracking-wider">
                    <TrendingUp className="w-4 h-4 text-emerald-500" />
                    <span className="leading-none">{t('dashboard.best_accounts', 'Top Candidates')}</span>
                </CardTitle>
            </CardHeader>
            <CardContent className="flex-1 overflow-y-auto p-6 relative z-10 custom-scrollbar">
                <div className="space-y-4">
                    {candidates.length > 0 ? (
                        candidates.map((account, index) => (
                            <button
                                key={account.id}
                                onClick={() => onSwitch?.(account.id)}
                                className={cn(
                                    "w-full text-left group relative overflow-hidden rounded-xl border transition-all duration-300",
                                    "hover:shadow-lg hover:border-blue-500/30 hover:-translate-y-0.5",
                                    "bg-white/50 dark:bg-black/40 border-zinc-200 dark:border-white/5",
                                    "p-4"
                                )}
                            >
                                <div className="flex items-center justify-between mb-4">
                                    <div className="flex items-center gap-3">
                                        <div className={cn(
                                            "flex items-center justify-center w-7 h-7 rounded-lg text-xs font-bold",
                                            index === 0 ? "bg-amber-500/10 text-amber-500 border border-amber-500/20" : "bg-zinc-800 text-zinc-500 border border-white/5"
                                        )}>
                                            {index + 1}
                                        </div>
                                        <span className="font-bold text-sm truncate max-w-[12rem] text-zinc-700 dark:text-zinc-300 group-hover:text-blue-400 transition-colors">
                                            {account.email.split('@')[0]}
                                        </span>
                                    </div>
                                    <Badge variant="outline" className="border-blue-500/20 text-blue-400 bg-blue-500/5 font-mono text-xs">
                                        {account.score}
                                    </Badge>
                                </div>
                                
                                <div className="grid grid-cols-3 gap-4">
                                    {/* Gemini Pro */}
                                    <div className="space-y-2">
                                        <div className="flex justify-between items-end text-[10px] uppercase font-bold text-zinc-500">
                                            <span>Gemini</span>
                                            <div className="flex items-center gap-1.5 leading-none">
                                                {account.metrics.geminiProReset && (
                                                    <span className="font-mono text-[9px] text-zinc-600 dark:text-zinc-600 normal-case">
                                                        {formatTimeRemaining(account.metrics.geminiProReset)}
                                                    </span>
                                                )}
                                                <span className={account.metrics.geminiPro > 50 ? "text-emerald-400" : "text-zinc-600"}>{account.metrics.geminiPro}%</span>
                                            </div>
                                        </div>
                                        <Progress 
                                            value={account.metrics.geminiPro} 
                                            className="h-2 bg-zinc-800 rounded-full"
                                            indicatorClassName={cn(
                                                "rounded-full bg-gradient-to-r", 
                                                account.metrics.geminiPro > 50 ? "from-emerald-600 to-emerald-400" : "from-zinc-600 to-zinc-400"
                                            )} 
                                        />
                                    </div>

                                    {/* Claude */}
                                    <div className="space-y-2">
                                        <div className="flex justify-between items-end text-[10px] uppercase font-bold text-zinc-500">
                                            <span>Claude</span>
                                            <div className="flex items-center gap-1.5 leading-none">
                                                {account.metrics.claudeReset && (
                                                    <span className="font-mono text-[9px] text-zinc-600 dark:text-zinc-600 normal-case">
                                                        {formatTimeRemaining(account.metrics.claudeReset)}
                                                    </span>
                                                )}
                                                <span className={account.metrics.claude > 50 ? "text-cyan-400" : "text-zinc-600"}>{account.metrics.claude}%</span>
                                            </div>
                                        </div>
                                        <Progress 
                                            value={account.metrics.claude} 
                                            className="h-2 bg-zinc-800 rounded-full"
                                            indicatorClassName={cn(
                                                "rounded-full bg-gradient-to-r", 
                                                account.metrics.claude > 50 ? "from-cyan-600 to-cyan-400" : "from-zinc-600 to-zinc-400"
                                            )} 
                                        />
                                    </div>

                                    {/* Flash */}
                                    <div className="space-y-2">
                                        <div className="flex justify-between items-end text-[10px] uppercase font-bold text-zinc-500">
                                            <span>Flash</span>
                                            <div className="flex items-center gap-1.5 leading-none">
                                                {account.metrics.geminiFlashReset && (
                                                    <span className="font-mono text-[9px] text-zinc-600 dark:text-zinc-600 normal-case">
                                                        {formatTimeRemaining(account.metrics.geminiFlashReset)}
                                                    </span>
                                                )}
                                                <span className={account.metrics.geminiFlash > 50 ? "text-amber-400" : "text-zinc-600"}>{account.metrics.geminiFlash}%</span>
                                            </div>
                                        </div>
                                        <Progress 
                                            value={account.metrics.geminiFlash} 
                                            className="h-2 bg-zinc-800 rounded-full"
                                            indicatorClassName={cn(
                                                "rounded-full bg-gradient-to-r", 
                                                account.metrics.geminiFlash > 50 ? "from-amber-600 to-amber-400" : "from-zinc-600 to-zinc-400"
                                            )} 
                                        />
                                    </div>
                                </div>
                            </button>
                        ))
                    ) : (
                        <div className="h-40 flex flex-col items-center justify-center text-muted-foreground space-y-3 opacity-50">
                            <TrendingUp className="w-10 h-10 opacity-20" />
                            <span className="text-sm font-medium">No candidates found</span>
                        </div>
                    )}
                </div>
            </CardContent>
        </Card>
    );
};

export default memo(BestAccounts);
