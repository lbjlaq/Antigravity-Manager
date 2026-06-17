import { QuotaData } from '../types/account';

export const getQuotaSummary = (quota?: QuotaData) => {
    if (!quota?.quota_groups || quota.quota_groups.length === 0) return null;
    
    let weeklyPct: number | null = null;
    let fiveHourPct: number | null = null;

    for (const group of quota.quota_groups) {
        for (const bucket of group.buckets) {
            const pct = Math.round(bucket.remaining_fraction * 100);
            if (bucket.window === 'weekly') {
                if (weeklyPct === null || pct < weeklyPct) weeklyPct = pct;
            } else if (bucket.window === '5h') {
                if (fiveHourPct === null || pct < fiveHourPct) fiveHourPct = pct;
            }
        }
    }
    
    if (weeklyPct === null && fiveHourPct === null) return null;
    return { weeklyPct, fiveHourPct };
};
