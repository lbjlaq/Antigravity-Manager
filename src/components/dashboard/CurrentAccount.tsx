import { memo, useState } from 'react';
import { Card, CardContent, CardHeader, CardTitle, CardFooter } from '../ui/card';
import { Progress } from '../ui/progress';
import { Badge } from '../ui/badge';
import { Button } from '../ui/button';
import { Mail, Zap, CheckCircle, Gem, Diamond, Circle, Copy, Check } from 'lucide-react';
import { Account } from '../../types/account';
import { useTranslation } from 'react-i18next';
import { cn } from '../../lib/utils';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '../ui/tooltip';

interface CurrentAccountProps {
    account: Account | null | undefined;
    onSwitch?: () => void;
}

const CurrentAccount = function CurrentAccount({ account, onSwitch }: CurrentAccountProps) {
    const { t } = useTranslation();
    const [copied, setCopied] = useState(false);

    if (!account) {
        return (
            <Card className="bg-zinc-900 border-white/10">
                <CardHeader>
                    <CardTitle className="text-base flex items-center gap-2 text-zinc-100">
                         <CheckCircle className="w-4 h-4 text-green-500" />
                         {t('dashboard.current_account')}
                    </CardTitle>
                </CardHeader>
                <CardContent className="text-center py-6 text-sm text-zinc-500">
                    {t('dashboard.no_active_account')}
                </CardContent>
            </Card>
        );
    }

    const geminiProModel = account.quota?.models.find(m => m.name === 'gemini-3-pro-high');
    const geminiFlashModel = account.quota?.models.find(m => m.name === 'gemini-3-flash');
    const claudeModel = account.quota?.models.find(m => m.name === 'claude-sonnet-4-5-thinking');

    const getTierBadge = () => {
        const tier = (account.quota?.subscription_tier || '').toLowerCase();
        if (tier.includes('ultra')) {
            return (
                <Badge className="bg-purple-500/10 text-purple-400 border-purple-500/20 hover:bg-purple-500/20 flex items-center gap-1">
                    <Gem className="w-3 h-3 fill-current" /> ULTRA
                </Badge>
            );
        } else if (tier.includes('pro')) {
            return (
                <Badge className="bg-blue-500/10 text-blue-400 border-blue-500/20 hover:bg-blue-500/20 flex items-center gap-1">
                    <Diamond className="w-3 h-3 fill-current" /> PRO
                </Badge>
            );
        }
        // Always show FREE badge for free/unknown tier
        return (
            <Badge variant="outline" className="border-zinc-600 text-zinc-400 bg-zinc-800/50 flex items-center gap-1">
                <Circle className="w-3 h-3" /> FREE
            </Badge>
        );
    };

    const copyToClipboard = async () => {
        try {
            await navigator.clipboard.writeText(account.email);
            setCopied(true);
            setTimeout(() => setCopied(false), 2000);
        } catch (err) {
            console.error('Failed to copy', err);
        }
    };

    // Helper to truncate email: donald20...@...shop
    const truncateEmail = (email: string) => {
        if (email.length <= 25) return email;
        const [name, domain] = email.split('@');
        const halfName = name.slice(0, Math.min(8, name.length));
        const shortDomain = domain.length > 10 ? '...' + domain.slice(-8) : domain;
        return `${halfName}...@${shortDomain}`;
    };

    return (
        <TooltipProvider>
            <Card className="h-full flex flex-col relative overflow-hidden shadow-xl border-white/5 bg-zinc-900 transition-all duration-300 hover:border-white/10 group">
                 {/* Background Decoration */}
                 <div className="absolute inset-0 bg-gradient-to-b from-white/[0.02] to-transparent pointer-events-none" />
                 
                <CardHeader className="relative z-10 pb-2 pt-6 px-6 border-b border-white/5">
                    <div className="flex flex-row items-center justify-between">
                         <CardTitle className="text-sm font-medium flex items-center gap-2 text-zinc-400 uppercase tracking-wider">
                            <Zap className="w-4 h-4 text-amber-500" />
                            {t('dashboard.current_account')}
                         </CardTitle>
                         <div className="flex-shrink-0">
                            {getTierBadge()}
                         </div>
                    </div>
                </CardHeader>

                <CardContent className="relative z-10 space-y-6 flex-1 pt-6 px-6">
                    {/* User Info Block */}
                    <div className="flex items-center gap-4 group/email">
                        <div className="p-3 bg-white/5 rounded-xl border border-white/5 group-hover/email:border-white/10 transition-colors">
                            <Mail className="w-5 h-5 text-zinc-400" />
                        </div>
                        <div className="flex-1 min-w-0">
                            <div className="text-[10px] font-bold text-zinc-500 uppercase tracking-wider mb-0.5">Account ID</div>
                            <div className="flex items-center gap-2">
                                <Tooltip>
                                    <TooltipTrigger asChild>
                                        <div className="text-lg font-bold text-zinc-100 truncate font-mono tracking-tight">
                                            {truncateEmail(account.email)}
                                        </div>
                                    </TooltipTrigger>
                                    <TooltipContent side="top" className="bg-zinc-950 border-zinc-800 text-zinc-300">
                                        <p>{account.email}</p>
                                    </TooltipContent>
                                </Tooltip>
                                
                                <Button 
                                    variant="ghost" 
                                    size="icon" 
                                    className="h-6 w-6 text-zinc-500 hover:text-zinc-200 hover:bg-white/5 rounded-md transition-all opacity-0 group-hover/email:opacity-100"
                                    onClick={copyToClipboard}
                                >
                                    {copied ? <Check className="w-3 h-3 text-green-500" /> : <Copy className="w-3 h-3" />}
                                </Button>
                            </div>
                        </div>
                    </div>

                    <div className="space-y-5">
                        {/* Compact Quota Grid */}
                        {geminiProModel && (
                            <div className="space-y-2">
                                 <div className="flex justify-between items-end">
                                    <span className="text-xs font-medium text-zinc-400">Gemini 3 Pro</span>
                                    <span className={cn("text-sm font-bold", geminiProModel.percentage >= 50 ? "text-emerald-400" : "text-amber-500")}>
                                        {geminiProModel.percentage}%
                                    </span>
                                </div>
                                <Progress 
                                    value={geminiProModel.percentage} 
                                    className="h-2 bg-zinc-800 rounded-full" 
                                    indicatorClassName={cn(
                                        "rounded-full bg-gradient-to-r", 
                                        geminiProModel.percentage >= 50 ? "from-emerald-600 to-emerald-400" : "from-orange-600 to-orange-400"
                                    )} 
                                />
                            </div>
                        )}

                        {claudeModel && (
                            <div className="space-y-2">
                                 <div className="flex justify-between items-end">
                                    <span className="text-xs font-medium text-zinc-400">Claude 4.5</span>
                                    <span className={cn("text-sm font-bold", claudeModel.percentage >= 50 ? "text-cyan-400" : "text-amber-500")}>
                                        {claudeModel.percentage}%
                                    </span>
                                </div>
                                <Progress 
                                    value={claudeModel.percentage} 
                                    className="h-2 bg-zinc-800 rounded-full" 
                                    indicatorClassName={cn(
                                        "rounded-full bg-gradient-to-r",
                                        claudeModel.percentage >= 50 ? "from-cyan-600 to-cyan-400" : "from-orange-600 to-orange-400"
                                    )}
                                />
                            </div>
                        )}

                        {geminiFlashModel && (
                             <div className="space-y-2">
                                 <div className="flex justify-between items-end">
                                    <span className="text-xs font-medium text-zinc-400">Gemini 3 Flash</span>
                                    <span className={cn("text-sm font-bold", geminiFlashModel.percentage >= 50 ? "text-amber-400" : "text-amber-500")}>
                                        {geminiFlashModel.percentage}%
                                    </span>
                                </div>
                                <Progress 
                                    value={geminiFlashModel.percentage} 
                                    className="h-2 bg-zinc-800 rounded-full" 
                                    indicatorClassName={cn(
                                        "rounded-full bg-gradient-to-r",
                                        geminiFlashModel.percentage >= 50 ? "from-amber-600 to-amber-400" : "from-orange-600 to-orange-400"
                                    )}
                                />
                            </div>
                        )}
                    </div>
                </CardContent>

                {onSwitch && (
                    <CardFooter className="pt-4 pb-6 px-6 bg-white/[0.02] border-t border-white/5">
                        <Button 
                            variant="outline" 
                            size="default"
                            className="w-full h-10 font-semibold border-zinc-700 bg-transparent hover:bg-zinc-800 text-zinc-300 hover:text-white transition-all active:scale-[0.98]" 
                            onClick={onSwitch}
                        >
                            {t('dashboard.switch_account')}
                        </Button>
                    </CardFooter>
                )}
            </Card>
        </TooltipProvider>
    );
};

export default memo(CurrentAccount);
