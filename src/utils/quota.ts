import { QuotaData } from '../types/account';

export const getQuotaSummary = (quota?: QuotaData) => {
    if (!quota?.quota_groups || quota.quota_groups.length === 0) return null;
    
    let weeklyPct: number | null = null;
    let fiveHourPct: number | null = null;

    for (const group of quota.quota_groups) {
        if (!group) continue;
        const groupNameLower = group.display_name?.toLowerCase() || '';
        const isGeminiGroup = groupNameLower.includes('gemini');

        for (const bucket of group.buckets || []) {
            if (!bucket) continue;
            const fraction = bucket.remaining_fraction;
            if (typeof fraction !== 'number' || isNaN(fraction)) continue;

            const pct = Math.round(fraction * 100);
            
            const isWeekly = bucket.bucket_id === 'gemini-weekly' || 
                             bucket.window === 'gemini-weekly' || 
                             (isGeminiGroup && bucket.window === 'weekly');
            const isFiveHour = bucket.bucket_id === 'gemini-5h' || 
                               bucket.window === 'gemini-5h' || 
                               (isGeminiGroup && bucket.window === '5h');

            if (isWeekly) {
                if (weeklyPct === null || pct < weeklyPct) weeklyPct = pct;
            } else if (isFiveHour) {
                if (fiveHourPct === null || pct < fiveHourPct) fiveHourPct = pct;
            }
        }
    }
    
    if (weeklyPct === null && fiveHourPct === null) return null;
    return { weeklyPct, fiveHourPct };
};
