import { QuotaData } from '../types/account';

/**
 * Extracts and returns a summary of remaining quota percentages for Gemini-related buckets.
 * Specifically, it looks for:
 * - Weekly quota: identified by bucket ID or window naming 'gemini-weekly', or 'weekly' window in a Gemini-named group.
 * - 5-Hour quota: identified by bucket ID or window naming 'gemini-5h', or '5h' window in a Gemini-named group.
 * 
 * If multiple buckets match the criteria for a window, it selects the minimum remaining percentage
 * to provide a conservative estimate of the remaining quota.
 * 
 * @param quota The quota data structure containing groups and buckets.
 * @returns An object with `weeklyPct` and `fiveHourPct` percentages (or null if not found), or null if no relevant quotas are present.
 */
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
            if (typeof fraction !== 'number' || !Number.isFinite(fraction)) continue;

            const pct = Math.min(100, Math.max(0, Math.round(fraction * 100)));
            
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
