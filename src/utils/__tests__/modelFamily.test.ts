/**
 * Unit tests for Model Family normalization and grouping utilities.
 * Run: npx tsx src/utils/__tests__/modelFamily.test.ts
 */
import {
    getModelFamilyId,
    getTierLabel,
    groupModelTokenStatsByFamily,
    resolvePricingRule,
    aggregateModelTrendsByFamily,
    type ModelTrendPoint,
} from '../modelFamily';

let passed = 0;
let failed = 0;

async function test(description: string, fn: () => void | Promise<void>): Promise<void> {
    try {
        await fn();
        passed++;
        console.log(`  PASS: ${description}`);
    } catch (err) {
        failed++;
        console.error(`  FAIL: ${description} — ${(err as Error).message}`);
    }
}

function assertEqual<T>(actual: T, expected: T, msg?: string): void {
    if (actual !== expected) {
        throw new Error(`${msg ? msg + ': ' : ''}expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
    }
}

async function runTests() {
    console.log('--- Starting Model Family Unit Tests ---');

    await test('1. Strips thinking tier suffixes to extract canonical family ID', () => {
        assertEqual(getModelFamilyId('gemini-3.8-flash-high'), 'gemini-3.8-flash');
        assertEqual(getModelFamilyId('gemini-3.8-flash-medium'), 'gemini-3.8-flash');
        assertEqual(getModelFamilyId('gemini-3.8-flash-low'), 'gemini-3.8-flash');
        assertEqual(getModelFamilyId('gemini-3.8-flash-tiered'), 'gemini-3.8-flash');
        assertEqual(getModelFamilyId('gemini-4-argon-high'), 'gemini-4-argon');
        assertEqual(getModelFamilyId('gemini-4-argon-low'), 'gemini-4-argon');
        assertEqual(getModelFamilyId('claude-opus-5-5-medium'), 'claude-opus-5-5');
        assertEqual(getModelFamilyId('claude-opus-5-5-xhigh'), 'claude-opus-5-5');
        assertEqual(getModelFamilyId('gemini-2.5-flash-thinking'), 'gemini-2.5-flash');
        assertEqual(getModelFamilyId('gemini-3.5-flash-extra-low'), 'gemini-3.5-flash');
        // Untouched base models
        assertEqual(getModelFamilyId('gemini-3.8-flash'), 'gemini-3.8-flash');
        assertEqual(getModelFamilyId('claude-sonnet-4-6'), 'claude-sonnet-4-6');
        assertEqual(getModelFamilyId('gpt-4o'), 'gpt-4o');
    });

    await test('2. Accurately labels tier variants', () => {
        const fam = 'gemini-3.8-flash';
        assertEqual(getTierLabel('gemini-3.8-flash', fam), 'Default (无后缀)');
        assertEqual(getTierLabel('gemini-3.8-flash-high', fam), 'High');
        assertEqual(getTierLabel('gemini-3.8-flash-medium', fam), 'Medium');
        assertEqual(getTierLabel('gemini-3.8-flash-low', fam), 'Low');
        assertEqual(getTierLabel('gemini-3.8-flash-extra-low', fam), 'Extra-Low');
        assertEqual(getTierLabel('gemini-3.8-flash-xhigh', fam), 'X-High');
        assertEqual(getTierLabel('gemini-3.8-flash-tiered', fam), 'Tiered');
        assertEqual(getTierLabel('gemini-2.5-flash-thinking', 'gemini-2.5-flash'), 'Thinking');
    });

    await test('3. Groups model variants into unified families and aggregates metrics', () => {
        const mockRawStats = [
            {
                model: 'gemini-3.8-flash-high',
                total_input_tokens: 1000,
                total_output_tokens: 500,
                total_cached_tokens: 200,
                total_reasoning_tokens: 300,
                total_tokens: 1500,
                request_count: 10,
                avg_ttft_ms: 120,
            },
            {
                model: 'gemini-3.8-flash-medium',
                total_input_tokens: 2000,
                total_output_tokens: 1000,
                total_cached_tokens: 400,
                total_reasoning_tokens: 200,
                total_tokens: 3000,
                request_count: 20,
                avg_ttft_ms: 180,
            },
            {
                model: 'gemini-3.8-flash',
                total_input_tokens: 500,
                total_output_tokens: 200,
                total_cached_tokens: 100,
                total_reasoning_tokens: 0,
                total_tokens: 700,
                request_count: 5,
                avg_ttft_ms: 100,
            },
            {
                model: 'claude-opus-5-5-medium',
                total_input_tokens: 4000,
                total_output_tokens: 2000,
                total_cached_tokens: 1000,
                total_reasoning_tokens: 800,
                total_tokens: 6000,
                request_count: 15,
                avg_ttft_ms: 300,
            },
        ];

        const mockCostCalc = (m: any) => (m.total_tokens * 2) / 1_000_000;

        const families = groupModelTokenStatsByFamily(mockRawStats, mockCostCalc);

        // Should produce 2 distinct families instead of 4 fragmented rows
        assertEqual(families.length, 2, 'Should aggregate 4 records into 2 families');

        // Check first family: claude-opus-5-5 (sorted by total_tokens DESC)
        const claudeFam = families[0];
        assertEqual(claudeFam.familyId, 'claude-opus-5-5');
        assertEqual(claudeFam.total_tokens, 6000);
        assertEqual(claudeFam.request_count, 15);
        assertEqual(claudeFam.variants.length, 1);

        // Check second family: gemini-3.8-flash
        const geminiFam = families[1];
        assertEqual(geminiFam.familyId, 'gemini-3.8-flash');
        assertEqual(geminiFam.total_input_tokens, 3500); // 1000 + 2000 + 500
        assertEqual(geminiFam.total_output_tokens, 1700); // 500 + 1000 + 200
        assertEqual(geminiFam.total_cached_tokens, 700); // 200 + 400 + 100
        assertEqual(geminiFam.total_reasoning_tokens, 500); // 300 + 200 + 0
        assertEqual(geminiFam.total_tokens, 5200); // 1500 + 3000 + 700
        assertEqual(geminiFam.request_count, 35); // 10 + 20 + 5
        assertEqual(geminiFam.variants.length, 3, 'Gemini family should have 3 variants');

        // Variants priority sorting: base (0) -> high (2) -> medium (3)
        assertEqual(geminiFam.variants[0].model, 'gemini-3.8-flash');
        assertEqual(geminiFam.variants[0].isBase, true);
        assertEqual(geminiFam.variants[1].model, 'gemini-3.8-flash-high');
        assertEqual(geminiFam.variants[1].tierLabel, 'High');
        assertEqual(geminiFam.variants[2].model, 'gemini-3.8-flash-medium');
        assertEqual(geminiFam.variants[2].tierLabel, 'Medium');
    });

    await test('4. Intelligent pricing rule resolution with family inheritance', () => {
        const pricingTable = {
            'gemini-flash': { input: 0.75, output: 3.75, cached: 0.075 },
            'gemini-pro': { input: 2.00, output: 12.00, cached: 0.20 },
            'gemini-2.0-flash': { input: 0.10, output: 0.40, cached: 0.025 },
            'gemini-4-argon': { input: 2.00, output: 12.00, cached: 0.20 },
            'claude-sonnet': { input: 3.00, output: 15.00, cached: 0.30 },
            'claude-opus': { input: 5.00, output: 25.00, cached: 0.50 },
            'claude-haiku': { input: 0.80, output: 4.00, cached: 0.08 },
            'default': { input: 1.00, output: 5.00, cached: 0.10 },
        };

        // Exact match
        assertEqual(resolvePricingRule('gemini-2.0-flash', pricingTable).input, 0.10);

        // Stripped tier match: gemini-4-argon-high -> gemini-4-argon
        assertEqual(resolvePricingRule('gemini-4-argon-high', pricingTable).input, 2.00);

        // Family prefix match: gemini-3.8-flash-high -> gemini-flash
        assertEqual(resolvePricingRule('gemini-3.8-flash-high', pricingTable).input, 0.75);
        assertEqual(resolvePricingRule('gemini-3.8-flash-medium', pricingTable).output, 3.75);

        // Claude family match: claude-opus-5-5-medium -> claude-opus
        assertEqual(resolvePricingRule('claude-opus-5-5-medium', pricingTable).input, 5.00);

        // Claude sonnet match: claude-sonnet-5.5 -> claude-sonnet
        assertEqual(resolvePricingRule('claude-sonnet-5.5', pricingTable).input, 3.00);

        // Unknown model fallback to default
        assertEqual(resolvePricingRule('unknown-custom-model', pricingTable).input, 1.00);
    });

    await test('5. Aggregates trend time points into unified model families', () => {
        const mockTrend: ModelTrendPoint[] = [
            {
                period: '10:00',
                model_data: {
                    'gemini-3.8-flash-high': 150,
                    'gemini-3.8-flash-medium': 350,
                    'claude-opus-5-5-medium': 500,
                    'gpt-4o': 100,
                },
            },
            {
                period: '11:00',
                model_data: {
                    'gemini-3.8-flash-low': 200,
                    'gemini-3.8-flash': 100,
                    'claude-opus-5-5': 600,
                },
            },
        ];

        const { transformedTrend, displayModelKeys } = aggregateModelTrendsByFamily(mockTrend, 5, 'Other');

        // Check display keys: should contain canonical families, not variants
        assertEqual(displayModelKeys.includes('gemini-3.8-flash'), true);
        assertEqual(displayModelKeys.includes('claude-opus-5-5'), true);
        assertEqual(displayModelKeys.includes('gemini-3.8-flash-high'), false);

        // Check row 0
        const row0 = transformedTrend[0];
        assertEqual(row0.period, '10:00');
        assertEqual(row0['gemini-3.8-flash'], 500); // 150 + 350
        assertEqual(row0['claude-opus-5-5'], 500);
        assertEqual(row0['gpt-4o'], 100);

        // Check row 1
        const row1 = transformedTrend[1];
        assertEqual(row1.period, '11:00');
        assertEqual(row1['gemini-3.8-flash'], 300); // 200 + 100
        assertEqual(row1['claude-opus-5-5'], 600);
    });

    console.log(`\nModel Family Tests Summary: ${passed} passed, ${failed} failed.\n`);
    if (failed > 0) {
        throw new Error(`Model Family tests failed: ${failed}`);
    }
}

runTests();
