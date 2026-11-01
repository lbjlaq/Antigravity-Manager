/**
 * Verification for Claude & 3P quota sanitization:
 * Accounts without supported Claude or GPT models must have their 3P quota slot left blank
 * (fiveHour: undefined, weekly: undefined) instead of displaying phantom 100% quotas or errors.
 *
 * Run: npx tsx src/utils/__tests__/claudeQuotaSanitize.test.ts
 */
import { getGroupQuotaSummaries } from '../quotaDisplay';
import type { QuotaGroup, ModelQuota } from '../../types/account';

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

const mockGroups: QuotaGroup[] = [
    {
        display_name: 'Gemini Models',
        buckets: [
            { bucket_id: 'gemini-5h', window: '5h', remaining_fraction: 0.85, reset_time: '2026-11-05T00:00:00Z' },
            { bucket_id: 'gemini-weekly', window: 'weekly', remaining_fraction: 0.70, reset_time: '2026-11-08T00:00:00Z' }
        ]
    },
    {
        display_name: 'Claude and GPT models',
        buckets: [
            { bucket_id: '3p-5h', window: '5h', remaining_fraction: 1.0, reset_time: '2026-11-01T20:00:00Z' },
            { bucket_id: '3p-weekly', window: 'weekly', remaining_fraction: 1.0, reset_time: '2026-11-08T15:00:00Z' }
        ]
    }
];

// 1. Account with NO Claude/GPT models (only Gemini) must leave 3P slot blank (undefined)
test('Account with no Claude or GPT models leaves 3P quota slot blank', () => {
    const modelsOnlyGemini: ModelQuota[] = [
        { name: 'gemini-3.8-flash-high', percentage: 85, reset_time: '2026-11-05T00:00:00Z' }
    ];
    const summaries = getGroupQuotaSummaries(mockGroups, modelsOnlyGemini, 'pro');
    const claudeSummary = summaries.find(s => s.key === 'claude');
    if (!claudeSummary) throw new Error('Claude summary must exist');

    assertEqual(claudeSummary.fiveHour, undefined, 'fiveHour must be undefined (blank)');
    assertEqual(claudeSummary.weekly, undefined, 'weekly must be undefined (blank)');
    assertEqual(claudeSummary.supportedModels?.length, 0, 'supportedModels must be empty');
});

// 2. Account with active Claude 5.5 models preserves normal 3P quota
test('Account with active Claude 5.5 models displays 3P quota normally', () => {
    const modelsWithClaude55: ModelQuota[] = [
        { name: 'gemini-3.8-flash-high', percentage: 85, reset_time: '2026-11-05T00:00:00Z' },
        { name: 'claude-opus-5-5-medium', percentage: 100, reset_time: '2026-11-01T20:00:00Z' }
    ];
    const summaries = getGroupQuotaSummaries(mockGroups, modelsWithClaude55, 'pro');
    const claudeSummary = summaries.find(s => s.key === 'claude');
    if (!claudeSummary) throw new Error('Claude summary must exist');

    assertEqual(claudeSummary.fiveHour?.percentage, 100, 'fiveHour percentage must be 100');
    assertEqual(claudeSummary.weekly?.percentage, 100, 'weekly percentage must be 100');
    assertEqual(claudeSummary.supportedModels?.includes('claude-opus-5-5-medium'), true, 'supportedModels has 5.5');
});

// 3. Account with only disabled Claude models leaves 3P quota slot blank
test('Account with only disabled Claude models leaves 3P quota slot blank', () => {
    const modelsDisabledClaude: ModelQuota[] = [
        { name: 'gemini-3.8-flash-high', percentage: 85, reset_time: '2026-11-05T00:00:00Z' },
        { name: 'claude-sonnet-4-6-disabled', percentage: 0, reset_time: '' }
    ];
    const summaries = getGroupQuotaSummaries(mockGroups, modelsDisabledClaude, 'pro');
    const claudeSummary = summaries.find(s => s.key === 'claude');
    if (!claudeSummary) throw new Error('Claude summary must exist');

    assertEqual(claudeSummary.fiveHour, undefined, 'disabled models must leave fiveHour blank');
    assertEqual(claudeSummary.weekly, undefined, 'disabled models must leave weekly blank');
});

console.log(`Passed: ${passed}, Failed: ${failed}`);
if (failed > 0) {
    throw new Error(`Claude quota sanitize tests failed: ${failed}`);
}
