/**
 * Standalone verification for Argon eligibility across Ultra, Pro, and Free tiers.
 * Run: npx tsx src/utils/__tests__/argonCapability.test.ts
 */
import { hasAccountArgonCapability, checkArgonEligibility, type QuotaData } from '../../types/account';
import { getGroupQuotaSummaries, getModelQuotaDisplay, isSupportedDisplayModel, formatFlagshipBadgeLabel } from '../quotaDisplay';
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
    assertEqual(summaries.length, 2, 'Ultra on 2-group shared pool renders only 2 quota groups');
    assertEqual(Boolean(summaries.find(s => s.key === 'argon')), false, 'No synthetic 3rd argon card when upstream shares Gemini Models');
    const geminiSummary = summaries.find(s => s.key === 'gemini');
    assertEqual(geminiSummary?.weekly?.percentage, 84, 'Shared Gemini group reports 84% weekly');
    assertEqual(Boolean(geminiSummary?.supportedModels?.includes('gemini-4-argon')), true, 'Shared Gemini group includes flagship models');
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

// 5. Orchestrator (Neon / Carbon / Argon) & internal worker isolation tests
test('gemini-pro-orchestrator variants map to exact codenames and Gemini 4, while gemini-flash-worker-agy is excluded', () => {
    assertEqual(extractGemini4Codename('gemini-pro-orchestrator-neon'), 'neon', 'Orchestrator Neon maps to neon codename');
    assertEqual(extractGemini4Codename('gemini-pro-orchestrator'), 'carbon', 'Orchestrator Carbon maps to carbon codename');
    assertEqual(extractGemini4Codename('gemini-pro-orchestrator-argon'), 'argon', 'Orchestrator Argon maps to argon codename');
    assertEqual(extractGemini4Codename('gemini-pro-orchestrator-g1-pro'), 'argon', 'Legacy Orchestrator Argon maps to argon codename');
    assertEqual(extractGemini4Codename('gemini-4-hydrogen-tiered-xhigh'), 'hydrogen', 'Hydrogen tiered-xhigh strips cleanly');
    assertEqual(extractGemini4Codename('gemini-4-hydrogen-sonar'), 'hydrogen', 'Hydrogen sonar strips cleanly');
    assertEqual(inferModelGroup('gemini-pro-orchestrator-neon'), 'Gemini 4', 'Orchestrator Neon belongs to Gemini 4 group');
    assertEqual(inferModelGroup('gemini-pro-orchestrator'), 'Gemini 4', 'Orchestrator Carbon belongs to Gemini 4 group');
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
                    display_name: 'Gemini 4 Neon',
                    buckets: [
                        { bucket_id: 'gemini-neon-wtu-weekly', window: 'weekly', remaining_fraction: 0.77, reset_time: '2026-10-31T12:00:00Z' },
                    ]
                }
            ],
            models: [
                { name: 'gemini-pro-orchestrator-g1-pro', percentage: 77, reset_time: '2026-10-31T12:00:00Z' },
            ]
        } as QuotaData
    };

    assertEqual(hasAccountArgonCapability(proAccount), true, 'Pro with orchestrator has Neon/Argon capability');
    const orchDisplay = getModelQuotaDisplay('gemini-pro-orchestrator-g1-pro', proAccount.quota.models[0], proAccount.quota.quota_groups);
    assertEqual(orchDisplay.percentage, 77, 'Orchestrator displays Neon quota');
});

// 6. Dynamic Gemini 4+ Codename Extraction & Role Token Filter tests
test('extractGemini4Codename correctly extracts future and current codenames, filtering out generic role tokens', () => {
    // Current & future codenames
    assertEqual(extractGemini4Codename('gemini-4-neon'), 'neon', 'Neon extracted');
    assertEqual(extractGemini4Codename('gemini-4-neon-xhigh'), 'neon', 'Neon xhigh stripped');
    assertEqual(extractGemini4Codename('gemini-4-carbon'), 'carbon', 'Carbon extracted');
    assertEqual(extractGemini4Codename('gemini-4-hydrogen-high'), 'hydrogen', 'Hydrogen high stripped');
    assertEqual(extractGemini4Codename('gemini-4-argon'), 'argon', 'Argon extracted');
    assertEqual(extractGemini4Codename('gemini-4.5-oxygen'), 'oxygen', 'Minor version oxygen extracted');

    // Generic role tokens must return null
    assertEqual(extractGemini4Codename('gemini-4-pro'), null, 'Generic role pro filtered');
    assertEqual(extractGemini4Codename('gemini-4-flash'), null, 'Generic role flash filtered');
    assertEqual(extractGemini4Codename('gemini-4-wtu'), null, 'Generic role wtu filtered');
    assertEqual(extractGemini4Codename('gemini-4-weekly'), null, 'Generic role weekly filtered');
    assertEqual(extractGemini4Codename('gemini-4-quota'), null, 'Generic role quota filtered');

    // Disabled models must return null
    assertEqual(extractGemini4Codename('gemini-4-neon-disabled'), null, 'Disabled model filtered');
    assertEqual(extractGemini4Codename('gemini-4-neon_disabled'), null, 'Underscore disabled filtered');
});

// 7. Dynamic Quota Summary Label Resolution (Neon replaces Argon as primary)
test('getGroupQuotaSummaries generates dynamic Gemini 4 Neon label for Neon models', () => {
    const proNeonAccount = {
        quota: {
            last_updated: Date.now(),
            subscription_tier: 'PRO',
            quota_groups: [
                {
                    display_name: 'Gemini Models',
                    buckets: [
                        { bucket_id: 'gemini-5h', window: '5h', remaining_fraction: 1.0, reset_time: '2026-10-27T08:00:00Z' },
                        { bucket_id: 'gemini-weekly', window: 'weekly', remaining_fraction: 0.95, reset_time: '2026-10-31T12:00:00Z' },
                    ]
                },
                {
                    display_name: 'Gemini 4 Neon',
                    buckets: [
                        { bucket_id: 'gemini-neon-wtu-weekly', window: 'weekly', remaining_fraction: 0.88, reset_time: '2026-10-31T12:00:00Z' },
                    ]
                }
            ],
            models: [
                { name: 'gemini-4-neon', percentage: 88, reset_time: '2026-10-31T12:00:00Z' },
                { name: 'gemini-4-neon-high', percentage: 88, reset_time: '2026-10-31T12:00:00Z' },
            ]
        } as QuotaData
    };

    const summaries = getGroupQuotaSummaries(proNeonAccount.quota.quota_groups, proNeonAccount.quota.models, 'pro');
    const neonSummary = summaries.find(s => s.key === 'argon');
    assertEqual(Boolean(neonSummary), true, 'Flagship summary present');
    assertEqual(neonSummary?.dynamicLabel, 'Gemini 4 Neon', 'Dynamic label correctly set to Gemini 4 Neon');
    assertEqual(neonSummary?.weekly?.percentage, 88, 'Weekly percentage matches Neon group');
});

// 8. Pro Tier Tests: With Carbon/Neon models sharing general Gemini pool (2 groups only)
test('Pro tier with Carbon/Neon models sharing Gemini pool keeps capability without duplicating a 3rd quota card', () => {
    const proCarbonAccount = {
        quota: {
            last_updated: Date.now(),
            subscription_tier: 'PRO',
            quota_groups: [
                {
                    display_name: 'Gemini Models',
                    buckets: [
                        { bucket_id: 'gemini-5h', window: '5h', remaining_fraction: 0.97, reset_time: '2026-10-27T08:00:00Z' },
                        { bucket_id: 'gemini-weekly', window: 'weekly', remaining_fraction: 0.91, reset_time: '2026-10-31T12:00:00Z' },
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
                { name: 'gemini-4-hydrogen-medium', percentage: 91, reset_time: '2026-10-31T12:00:00Z' },
                { name: 'gemini-4-carbon', percentage: 91, reset_time: '2026-10-31T12:00:00Z' },
                { name: 'gemini-4-carbon-high', percentage: 91, reset_time: '2026-10-31T12:00:00Z' },
                { name: 'gemini-4-neon-high', percentage: 91, reset_time: '2026-10-31T12:00:00Z' },
            ]
        } as QuotaData
    };

    assertEqual(hasAccountArgonCapability(proCarbonAccount), true, 'Pro with Carbon hasAccountArgonCapability');
    assertEqual(checkArgonEligibility(proCarbonAccount.quota.quota_groups, proCarbonAccount.quota.models, 'pro'), true, 'Pro with Carbon checkArgonEligibility');
    const summaries = getGroupQuotaSummaries(proCarbonAccount.quota.quota_groups, proCarbonAccount.quota.models, 'pro');
    assertEqual(summaries.length, 2, 'Shared 2-group Pro account renders only 2 quota groups');
    assertEqual(Boolean(summaries.find(s => s.key === 'argon')), false, 'No duplicated 3rd card when no dedicated flagship group exists');
    const geminiSummary = summaries.find(s => s.key === 'gemini');
    assertEqual(geminiSummary?.weekly?.percentage, 91, 'Shared Gemini pool reports 91% weekly');
    assertEqual(Boolean(geminiSummary?.supportedModels?.includes('gemini-4-carbon-high')), true, 'Gemini supportedModels includes shared Carbon');
    assertEqual(Boolean(geminiSummary?.supportedModels?.includes('gemini-4-neon-high')), true, 'Gemini supportedModels includes shared Neon');
    assertEqual(
        formatFlagshipBadgeLabel(geminiSummary?.supportedModels || [], 'pro'),
        'Neon / Carbon',
        'Flagship badge ignores Hydrogen and shows Neon / Carbon'
    );
});

// 9. Pro Tier Tests: With dedicated Carbon quota group
test('Pro tier with dedicated Carbon quota group uses Carbon bucket', () => {
    const proDedicatedCarbonAccount = {
        quota: {
            last_updated: Date.now(),
            subscription_tier: 'PRO',
            quota_groups: [
                {
                    display_name: 'Gemini Models',
                    buckets: [
                        { bucket_id: 'gemini-5h', window: '5h', remaining_fraction: 1.0, reset_time: '2026-10-27T08:00:00Z' },
                        { bucket_id: 'gemini-weekly', window: 'weekly', remaining_fraction: 0.95, reset_time: '2026-10-31T12:00:00Z' },
                    ]
                },
                {
                    display_name: 'Gemini 4 Carbon',
                    buckets: [
                        { bucket_id: 'gemini-carbon-wtu-weekly', window: 'weekly', remaining_fraction: 0.75, reset_time: '2026-10-31T12:00:00Z' },
                    ]
                }
            ],
            models: [
                { name: 'gemini-4-carbon', percentage: 75, reset_time: '2026-10-31T12:00:00Z' },
            ]
        } as QuotaData
    };

    assertEqual(hasAccountArgonCapability(proDedicatedCarbonAccount), true, 'Pro with dedicated Carbon has capability');
    const summaries = getGroupQuotaSummaries(proDedicatedCarbonAccount.quota.quota_groups, proDedicatedCarbonAccount.quota.models, 'pro');
    const carbonSummary = summaries.find(s => s.key === 'argon');
    assertEqual(Boolean(carbonSummary), true, 'Flagship summary present');
    assertEqual(carbonSummary?.dynamicLabel, 'Gemini 4 Carbon', 'Dynamic label is Gemini 4 Carbon');
    assertEqual(carbonSummary?.weekly?.percentage, 75, 'Carbon weekly percentage uses dedicated bucket');
});

// 10. Stale Gemini 4 Argon (0%) must NOT shadow active Gemini 4 Neon / Carbon (Fixes #4048)
test('Stale Gemini 4 Argon 0% group does not shadow active Neon/Carbon models on shared Gemini pool', () => {
    const proAccountWithZombieArgon = {
        quota: {
            last_updated: Date.now(),
            subscription_tier: 'PRO',
            quota_groups: [
                {
                    display_name: 'Gemini Models',
                    buckets: [
                        { bucket_id: 'gemini-5h', window: '5h', remaining_fraction: 0.92, reset_time: '2026-10-27T08:00:00Z' },
                        { bucket_id: 'gemini-weekly', window: 'weekly', remaining_fraction: 0.86, reset_time: '2026-10-31T12:00:00Z' },
                    ]
                },
                {
                    display_name: 'Gemini 4 Argon',
                    buckets: [
                        { bucket_id: 'gemini-argon-wtu-weekly', window: 'weekly', remaining_fraction: 0.0, reset_time: '2026-10-31T12:00:00Z' },
                    ]
                }
            ],
            models: [
                { name: 'gemini-4-neon', percentage: 86, reset_time: '2026-10-31T12:00:00Z' },
                { name: 'gemini-4-carbon', percentage: 86, reset_time: '2026-10-31T12:00:00Z' },
            ]
        } as QuotaData
    };

    const summaries = getGroupQuotaSummaries(
        proAccountWithZombieArgon.quota.quota_groups,
        proAccountWithZombieArgon.quota.models,
        'pro'
    );
    assertEqual(summaries.length, 2, 'Zombie Argon 0% group does not produce a 3rd card');
    assertEqual(Boolean(summaries.find(s => s.key === 'argon')), false, 'No 3rd card when only stale zombie Argon group exists');
    const geminiSummary = summaries.find(s => s.key === 'gemini');
    assertEqual(geminiSummary?.weekly?.percentage, 86, 'Active Neon/Carbon uses shared Gemini pool, not zombie Argon 0%');
    assertEqual(
        formatFlagshipBadgeLabel(geminiSummary?.supportedModels || [], 'pro'),
        'Neon / Carbon',
        'Flagship badge reflects active Neon / Carbon models'
    );

    const neonDisplay = getModelQuotaDisplay(
        'gemini-4-neon',
        proAccountWithZombieArgon.quota.models[0],
        proAccountWithZombieArgon.quota.quota_groups
    );
    assertEqual(neonDisplay.percentage, 86, 'gemini-4-neon not zeroed by zombie Argon group');
});

console.log(`Passed: ${passed}, Failed: ${failed}`);
if (failed > 0) {
    throw new Error(`Argon capability tests failed: ${failed}`);
}
