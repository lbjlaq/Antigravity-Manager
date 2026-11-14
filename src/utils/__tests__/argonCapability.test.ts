/**
 * Standalone verification for Argon eligibility across Ultra, Pro, and Free tiers.
 * Run: npx tsx src/utils/__tests__/argonCapability.test.ts
 */
import { hasAccountArgonCapability, checkArgonEligibility, type QuotaData } from '../../types/account';
import { getGroupQuotaSummaries, getModelQuotaDisplay, isSupportedDisplayModel } from '../quotaDisplay';
import { inferModelGroup } from '../../config/modelConfig';
import { extractGemini4Codename } from '../modelCategory';

let passed = 0;
let failed = 0;

function test(description: string, fn: () => void): void {
    try {
        fn();
        passed++;
    } catch (err) {
        failed++;
        console.error(`  FAIL: ${description}  — ${(err as Error).message}`);
    }
}

function assertEqual<T>(actual: T, expected: T, msg?: string): void {
    if (actual !== expected) {
        throw new Error(`${msg ? msg + ': ' : ''}expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
    }
}

// 1. Ultra Tier Tests: Without dedicated Argon quota group
test('Ultra tier without dedicated argon group has Argon capability', () => {
    const ultraAccount = {
        quota: {
            last_updated: Date.now(),
            subscription_tier: 'ULTRA',
            quota_groups: [
                {
                    display_name: 'Gemini Models',
                    buckets: [
                        { bucket_id: 'gemini-5h', window: '5h', remaining_fraction: 0.93, reset_time: '2026-10-27T08:00:00Z' },
                        { bucket_id: 'gemini-weekly', window: 'weekly', remaining_fraction: 0.84, reset_time: '2026-10-31T12:00:00Z' },
                    ]
                },
                {
                    display_name: 'Claude and GPT models',
                    buckets: [
                        { bucket_id: 'claude-5h', window: '5h', remaining_fraction: 1.0, reset_time: '2026-10-27T08:00:00Z' },
                        { bucket_id: 'claude-weekly', window: 'weekly', remaining_fraction: 0.01, reset_time: '2026-10-30T00:00:00Z' },
                    ]
                }
            ],
            models: [
                { name: 'gemini-4-argon', percentage: 84, reset_time: '2026-10-31T12:00:00Z' },
                { name: 'gemini-4-argon-high', percentage: 84, reset_time: '2026-10-31T12:00:00Z' },
            ]
        } as QuotaData
    };

    assertEqual(hasAccountArgonCapability(ultraAccount), true, 'Ultra hasAccountArgonCapability');
    assertEqual(checkArgonEligibility(ultraAccount.quota.quota_groups, ultraAccount.quota.models, 'ultra'), true, 'Ultra checkArgonEligibility');
    const summaries = getGroupQuotaSummaries(ultraAccount.quota.quota_groups, ultraAccount.quota.models, 'ultra');
    const argonSummary = summaries.find(s => s.key === 'argon');
    assertEqual(Boolean(argonSummary), true, 'Argon summary present for Ultra');
    assertEqual(argonSummary?.weekly?.percentage, 84, 'Argon inherits weekly quota from Gemini pool');
});

// 2. Pro Tier Tests: With dedicated Argon quota group
test('Pro tier with dedicated Argon quota group has Argon capability', () => {
    const proAccount = {
        quota: {
            last_updated: Date.now(),
            subscription_tier: 'PRO',
            quota_groups: [
                {
                    display_name: 'Gemini Models',
                    buckets: [
                        { bucket_id: 'gemini-5h', window: '5h', remaining_fraction: 1.0, reset_time: '2026-10-27T08:00:00Z' },
                        { bucket_id: 'gemini-weekly', window: 'weekly', remaining_fraction: 0.96, reset_time: '2026-10-31T12:00:00Z' },
                    ]
                },
                {
                    display_name: 'Gemini 4 Argon',
                    buckets: [
                        { bucket_id: 'gemini-argon-wtu-weekly', window: 'weekly', remaining_fraction: 1.0, reset_time: '2026-10-31T12:00:00Z' },
                    ]
                }
            ],
            models: [
                { name: 'gemini-4-argon', percentage: 100, reset_time: '2026-10-31T12:00:00Z' },
            ]
        } as QuotaData
    };

    assertEqual(hasAccountArgonCapability(proAccount), true, 'Pro hasAccountArgonCapability');
    assertEqual(checkArgonEligibility(proAccount.quota.quota_groups, proAccount.quota.models, 'pro'), true, 'Pro checkArgonEligibility');
    const summaries = getGroupQuotaSummaries(proAccount.quota.quota_groups, proAccount.quota.models, 'pro');
    const argonSummary = summaries.find(s => s.key === 'argon');
    assertEqual(Boolean(argonSummary), true, 'Argon summary present for Pro');
    assertEqual(argonSummary?.weekly?.percentage, 100, 'Argon uses its own weekly bucket');
});

// 3. Free Tier Tests: Without Argon quota group
test('Free tier without Argon group strictly does NOT have Argon capability', () => {
    const freeAccount = {
        quota: {
            last_updated: Date.now(),
            subscription_tier: 'FREE',
            quota_groups: [
                {
                    display_name: 'Gemini Models',
                    buckets: [
                        { bucket_id: 'gemini-weekly', window: 'weekly', remaining_fraction: 1.0, reset_time: '2026-10-31T12:00:00Z' },
                    ]
                },
                {
                    display_name: 'Claude and GPT models',
                    buckets: [
                        { bucket_id: 'claude-weekly', window: 'weekly', remaining_fraction: 1.0, reset_time: '2026-10-31T12:00:00Z' },
                    ]
                }
            ],
            models: [
                { name: 'gemini-3.8-flash', percentage: 100, reset_time: '2026-10-31T12:00:00Z' }
            ]
        } as QuotaData
    };

    assertEqual(hasAccountArgonCapability(freeAccount), false, 'Free hasAccountArgonCapability');
    assertEqual(checkArgonEligibility(freeAccount.quota.quota_groups, freeAccount.quota.models, 'free'), false, 'Free checkArgonEligibility');
    const summaries = getGroupQuotaSummaries(freeAccount.quota.quota_groups, freeAccount.quota.models, 'free');
    const argonSummary = summaries.find(s => s.key === 'argon');
    assertEqual(Boolean(argonSummary), false, 'Argon summary must be absent for Free');
});

// 4. getModelQuotaDisplay tests
test('getModelQuotaDisplay correctly preserves Ultra Argon percentage and zeros out unauthorized', () => {
    const ultraModel = { name: 'gemini-4-argon', percentage: 84, reset_time: '2026-10-31T12:00:00Z' };
    const groups = [{ display_name: 'Gemini Models', buckets: [{ bucket_id: 'gemini-weekly', window: 'weekly', remaining_fraction: 0.84, reset_time: '2026-10-31T12:00:00Z' }] }];
    const display = getModelQuotaDisplay(ultraModel.name, ultraModel, groups);
    assertEqual(display.percentage, 84, 'Ultra Argon percentage preserved from model');

    // Free account without Argon percentage
    const displayZero = getModelQuotaDisplay('gemini-4-argon', undefined, groups);
    assertEqual(displayZero.percentage, 0, 'Unauthorized Argon percentage is 0');
});

// 5. Orchestrator (Argon) & internal worker isolation tests
test('gemini-pro-orchestrator-g1-pro maps to Argon group and Gemini 4, while gemini-flash-worker-agy is excluded', () => {
    assertEqual(extractGemini4Codename('gemini-pro-orchestrator-g1-pro'), 'argon', 'Orchestrator maps to argon codename');
    assertEqual(inferModelGroup('gemini-pro-orchestrator-g1-pro'), 'Gemini 4', 'Orchestrator belongs to Gemini 4 group');
    assertEqual(isSupportedDisplayModel('gemini-pro-orchestrator-g1-pro'), true, 'Orchestrator is supported display model');
    assertEqual(isSupportedDisplayModel('gemini-flash-worker-agy'), false, 'Internal worker is excluded from display');

    const proAccount = {
        quota: {
            last_updated: Date.now(),
            subscription_tier: 'PRO',
            quota_groups: [
                {
                    display_name: 'Gemini Models',
                    buckets: [
                        { bucket_id: 'gemini-5h', window: '5h', remaining_fraction: 0.9, reset_time: '2026-10-27T08:00:00Z' },
                    ]
                },
                {
                    display_name: 'Gemini 4 Argon',
                    buckets: [
                        { bucket_id: 'gemini-argon-wtu-weekly', window: 'weekly', remaining_fraction: 0.77, reset_time: '2026-10-31T12:00:00Z' },
                    ]
                }
            ],
            models: [
                { name: 'gemini-pro-orchestrator-g1-pro', percentage: 77, reset_time: '2026-10-31T12:00:00Z' },
            ]
        } as QuotaData
    };

    assertEqual(hasAccountArgonCapability(proAccount), true, 'Pro with orchestrator has Argon capability');
    const orchDisplay = getModelQuotaDisplay('gemini-pro-orchestrator-g1-pro', proAccount.quota.models[0], proAccount.quota.quota_groups);
    assertEqual(orchDisplay.percentage, 77, 'Orchestrator displays Argon quota');
});

console.log(`Passed: ${passed}, Failed: ${failed}`);
if (failed > 0) {
    throw new Error(`Argon capability tests failed: ${failed}`);
}
