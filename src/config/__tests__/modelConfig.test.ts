/**
 * Standalone assertion script for categorizeModel and getModelProtectionKey.
 *
 * Imports from the standalone utils module (no @lobehub/icons dependency,
 * runs under plain Node.js via npx tsx).
 *
 * Run: npx tsx src/config/__tests__/modelConfig.test.ts
 */
import {
    categorizeModel,
    getModelProtectionKey,
    getModelDisplayName,
    findQuotaModel,
    findImageQuotaModel,
    ensurePinnedImageSelector,
    DEFAULT_IMAGE_PIN_SELECTOR,
    resolveQuotaModels,
    type ModelCategory,
} from '../../utils/modelCategory';
import { getModelQuotaDisplay } from '../../utils/quotaDisplay';
import {
    sortModels,
    compareModelsDesc,
    extractModelVersion,
    QUOTA_BUCKET_ORDER,
} from '../../utils/modelSort';
// Compile-time guard: if findImageQuotaModel or sortModels is removed from the config re-export,
// the type alias test below fails with TS2724 on `pnpm tsc --noEmit`.
function __noop<T>(): void { const _x: T[] = []; void _x; }
type _ConfigFindImageQuotaModel = typeof import('../../config/modelConfig').findImageQuotaModel;
type _ConfigSortModels = typeof import('../../config/modelConfig').sortModels;
__noop<_ConfigFindImageQuotaModel>();
__noop<_ConfigSortModels>();

let passed = 0;
let failed = 0;

function test(description: string, fn: () => void): void {
    try {
        fn();
        passed++;
    } catch (e: unknown) {
        failed++;
        const msg = e instanceof Error ? e.message : String(e);
        console.error(`  FAIL: ${description}  — ${msg}`);
    }
}

function assertEqual<T>(actual: T, expected: T): void {
    if (actual !== expected) {
        throw new Error(`expected "${expected}", got "${actual}"`);
    }
}

const categorizeCases: Array<[string, ModelCategory]> = [
    // canonical
    ['gemini-3.5-flash', 'gemini-flash'],
    ['gemini-3.1-pro', 'gemini-pro'],
    // physical / variants
    ['gemini-3-flash-agent', 'gemini-flash'],
    ['gemini-3.5-flash-low', 'gemini-flash'],
    ['gemini-3.5-flash-extra-low', 'gemini-flash'],
    ['gemini-pro-agent', 'gemini-pro'],
    ['gemini-3.1-pro-low', 'gemini-pro'],
    // legacy
    ['gemini-3-flash', 'gemini-flash'],
    ['gemini-3-pro-high', 'gemini-pro'],
    ['gemini-3.1-pro-high', 'gemini-pro'],
    ['gemini-3-pro-low', 'gemini-pro'],
    // image split
    ['gemini-3.1-flash-image', 'gemini-flash-image'],
    ['gemini-3-pro-image', 'gemini-pro-image'],
    ['imagen-3.0', 'gemini-pro-image'],
    // claude
    ['claude-sonnet-4-6', 'claude'],
    ['claude-opus-4-6-thinking', 'claude'],
    // edge / other providers
    ['gpt-4o', 'other'],
    ['gpt-oss-120b-medium', 'other'],
];

for (const [name, expected] of categorizeCases) {
    test(`categorizeModel("${name}")`, () => {
        assertEqual(categorizeModel(name), expected);
    });
}

const protectionCases: Array<[string, string | null]> = [
    ['gemini-3.5-flash', 'gemini-3-flash'],
    ['gemini-3.1-pro', 'gemini-3-pro-high'],
    ['gemini-3.1-flash-image', 'gemini-3.1-flash-image'],
    ['gemini-3-pro-image', 'gemini-3-pro-image'],
    ['claude-sonnet-4-6', 'claude'],
    ['gpt-4o', null],
];

for (const [name, expected] of protectionCases) {
    test(`getModelProtectionKey("${name}")`, () => {
        assertEqual(getModelProtectionKey(name), expected);
    });
}

// ── getModelDisplayName ──────────────────────────────────────────────────────

type ModelInput = { name: string; display_name?: string } | null | undefined;

const displayNameCases: Array<[ModelInput, string | undefined, string]> = [
    [{ name: 'gemini-3-pro-high', display_name: 'Gemini 3.1 Pro High' }, undefined, 'Gemini 3.1 Pro High'],
    [{ name: 'gemini-3-flash' }, undefined, 'Gemini 3 Flash'],
    [{ name: 'gemini-3.1-flash-image', display_name: undefined }, undefined, 'Gemini 3.1 Flash Image'],
    [undefined, 'Claude 系列', 'Claude 系列'],
    [null, undefined, ''],
    [{ name: 'claude-opus-4-6-thinking', display_name: 'Claude Opus 4.6 TK' }, undefined, 'Claude Opus 4.6 TK'],
];

for (const [model, fallback, expected] of displayNameCases) {
    const label = model === null
        ? 'getModelDisplayName(null)'
        : model === undefined
            ? `getModelDisplayName(undefined, '${fallback}')`
            : `getModelDisplayName({name:'${model.name}'${model.display_name ? `, display_name:'${model.display_name}'` : ''}})`;
    test(label, () => {
        assertEqual(getModelDisplayName(model, fallback), expected);
    });
}

// ── findQuotaModel ──────────────────────────────────────────────────────────

const findCases: Array<[Array<{ name: string }>, ModelCategory, string | null]> = [
    // Pro: preferred chain
    [[{ name: 'gemini-pro-agent' }, { name: 'gemini-3.1-pro-low' }], 'gemini-pro', 'gemini-pro-agent'],
    [[{ name: 'gemini-2.5-pro' }], 'gemini-pro', 'gemini-2.5-pro'],
    // Flash: preferred chain
    [[{ name: 'gemini-3-flash-agent' }, { name: 'gemini-3.5-flash-low' }], 'gemini-flash', 'gemini-3-flash-agent'],
    // Claude: preferred chain
    [[{ name: 'claude-sonnet-4-6' }, { name: 'claude-opus-4-6-thinking' }], 'claude', 'claude-sonnet-4-6'],
    [[{ name: 'claude-opus-4-6-thinking' }], 'claude', 'claude-opus-4-6-thinking'],
    // Empty
    [[], 'gemini-pro', null],
    // Fallback to categorizeModel
    [[{ name: 'gemini-3.5-flash-extra-low' }], 'gemini-flash', 'gemini-3.5-flash-extra-low'],
];

for (const [models, category, expected] of findCases) {
    test(`findQuotaModel(${category}, ${models.length} models)`, () => {
        const result = findQuotaModel(models, category);
        assertEqual(result?.name ?? null, expected);
    });
}

// ── resolveQuotaModels image-selector regression ──────────────────────────────

const imageModels = [{ name: 'gemini-3.1-flash-image', percentage: 80 }];

test('resolveQuotaModels: legacy gemini-3-pro-image resolves flash-image + category:gemini-image', () => {
    const results = resolveQuotaModels(imageModels, ['gemini-3-pro-image']);
    assertEqual(results.length, 1);
    assertEqual(results[0].selectionKey, 'category:gemini-image');
    assertEqual(results[0].model?.name, 'gemini-3.1-flash-image');
});

test('resolveQuotaModels: current gemini-3.1-flash-image resolves same model + key', () => {
    const results = resolveQuotaModels(imageModels, ['gemini-3.1-flash-image']);
    assertEqual(results.length, 1);
    assertEqual(results[0].selectionKey, 'category:gemini-image');
    assertEqual(results[0].model?.name, 'gemini-3.1-flash-image');
});

test('resolveQuotaModels: both image selectors dedupe to one selection', () => {
    const results = resolveQuotaModels(imageModels, ['gemini-3-pro-image', 'gemini-3.1-flash-image']);
    assertEqual(results.length, 1);
    assertEqual(results[0].selectionKey, 'category:gemini-image');
    assertEqual(results[0].model?.name, 'gemini-3.1-flash-image');
});

test('resolveQuotaModels: legacy image selector with no image API model returns unresolved', () => {
    const results = resolveQuotaModels([{ name: 'gemini-3.5-flash', percentage: 50 }], ['gemini-3-pro-image']);
    assertEqual(results.length, 1);
    assertEqual(results[0].selectionKey, 'category:gemini-image');
    assertEqual(results[0].model, undefined);
});

test('resolveQuotaModels: exact match keeps discrete pinned models instead of collapsing into category', () => {
    const models = [
        { name: 'gemini-3.7-flash-low', percentage: 90 },
        { name: 'gemini-3.7-flash-high', percentage: 95 },
        { name: 'gemini-3-flash-agent', percentage: 80 },
    ];
    const results = resolveQuotaModels(models, ['gemini-3.7-flash-low', 'gemini-3.7-flash-high']);
    assertEqual(results.length, 2);
    assertEqual(results[0].selectionKey, 'model:gemini-3.7-flash-low');
    assertEqual(results[0].model?.name, 'gemini-3.7-flash-low');
    assertEqual(results[1].selectionKey, 'model:gemini-3.7-flash-high');
    assertEqual(results[1].model?.name, 'gemini-3.7-flash-high');
});

// ── findImageQuotaModel ───────────────────────────────────────────────────

const imageNameCases: Array<[Array<{ name: string }>, string | null]> = [
    [[{ name: 'gemini-3.1-flash-image' }, { name: 'gemini-3-pro-image' }], 'gemini-3.1-flash-image'],
    [[{ name: 'gemini-3-pro-image' }], 'gemini-3-pro-image'],
    [[{ name: 'gemini-3.5-flash' }], null],
];

for (const [models, expected] of imageNameCases) {
    test(`findImageQuotaModel(${models.length} models)`, () => {
        const result = findImageQuotaModel(models);
        assertEqual(result?.name ?? null, expected);
    });
}

// ── ensurePinnedImageSelector (account management pin gap) ─────────────────

test('ensurePinnedImageSelector: empty list gains default image pin', () => {
    const result = ensurePinnedImageSelector([]);
    assertEqual(result.length, 1);
    assertEqual(result[0], DEFAULT_IMAGE_PIN_SELECTOR);
});

test('ensurePinnedImageSelector: live user pin list without image gains flash-image', () => {
    const livePins = [
        'gemini-3-pro-high',
        'gemini-3-flash',
        'claude-sonnet-4-5-thinking',
        'gemini-3.1-pro-high',
        'gemini-3.1-pro-low',
        'claude-sonnet-4-6',
    ];
    const result = ensurePinnedImageSelector(livePins);
    assertEqual(result.includes(DEFAULT_IMAGE_PIN_SELECTOR), true);
    assertEqual(result.length, livePins.length + 1);
});

test('ensurePinnedImageSelector: existing flash-image pin is unchanged', () => {
    const pins = ['gemini-pro-agent', 'gemini-3.1-flash-image'];
    const result = ensurePinnedImageSelector(pins);
    assertEqual(result.length, 2);
    assertEqual(result.join(','), pins.join(','));
});

test('ensurePinnedImageSelector: existing pro-image pin is unchanged', () => {
    const pins = ['gemini-3-pro-image', 'gemini-3-flash'];
    const result = ensurePinnedImageSelector(pins);
    assertEqual(result.length, 2);
    assertEqual(result.includes(DEFAULT_IMAGE_PIN_SELECTOR), false);
});

test('resolveQuotaModels + ensurePinnedImageSelector: live pin gap resolves Gemini 3.1 Flash Image', () => {
    const livePins = [
        'gemini-3-pro-high',
        'gemini-3-flash',
        'claude-sonnet-4-5-thinking',
        'gemini-3.1-pro-high',
        'gemini-3.1-pro-low',
        'claude-sonnet-4-6',
    ];
    const apiModels = [
        { name: 'gemini-pro-agent', percentage: 80 },
        { name: 'gemini-3-flash-agent', percentage: 70 },
        { name: 'gemini-3.1-flash-image', percentage: 92, display_name: 'Gemini 3.1 Flash Image' },
        { name: 'claude-sonnet-4-6', percentage: 50 },
    ];
    const results = resolveQuotaModels(apiModels, ensurePinnedImageSelector(livePins));
    const image = results.find(r => r.selectionKey === 'category:gemini-image');
    assertEqual(image?.model?.name, 'gemini-3.1-flash-image');
    assertEqual(image?.model?.display_name, 'Gemini 3.1 Flash Image');
});

test('quota display: weekly exhaustion overrides raw 5h without changing protection quota', () => {
    const reset = new Date(Date.now() + 7200000).toISOString();
    const model = { name: 'gemini-3.1-pro-high', percentage: 1, reset_time: reset };
    const groups = [{ display_name: 'Gemini Models', buckets: [
        { bucket_id: 'gemini-weekly', window: 'weekly', remaining_fraction: 0.01, reset_time: reset },
        { bucket_id: 'gemini-5h', window: '5h', remaining_fraction: 1, reset_time: '2030-01-01T00:00:00Z' },
    ] }];
    const display = getModelQuotaDisplay(model.name, model, groups);
    assertEqual(display.percentage, 100);
    assertEqual(display.resetTime, groups[0].buckets[1].reset_time);
    assertEqual(display.isWeeklyConstrained, false);
    assertEqual(model.percentage < 2, true);
    for (const fraction of [0, 0.0005, 0.001]) {
        groups[0].buckets[0].remaining_fraction = fraction;
        const blocked = getModelQuotaDisplay(model.name, model, groups);
        assertEqual(blocked.percentage, 0);
        assertEqual(blocked.resetTime, reset);
        assertEqual(blocked.isWeeklyConstrained, true);
        assertEqual(blocked.weeklyResetTime, reset);
    }
    for (const fraction of [0.0011, 0.00285638, 0.01223943]) {
        groups[0].buckets[0].remaining_fraction = fraction;
        const protectedDisplay = getModelQuotaDisplay(model.name, model, groups);
        assertEqual(protectedDisplay.isWeeklyConstrained, false);
        assertEqual(protectedDisplay.percentage, 100);
    }
    groups[0].buckets[0].remaining_fraction = 0;
    groups[0].buckets[0].reset_time = new Date(Date.now() - 1000).toISOString();
    assertEqual(getModelQuotaDisplay(model.name, model, groups).isWeeklyConstrained, false);
    assertEqual(getModelQuotaDisplay('claude-sonnet-4-6', undefined, groups).isWeeklyConstrained, false);
    assertEqual(getModelQuotaDisplay(model.name, model).percentage, 1);
});

test('quota display: virtual buckets preserve their own window percentage and reset time', () => {
    const weeklyReset = '2026-10-06T19:27:00Z';
    const fiveHourReset = '2026-10-01T10:28:00Z';
    const groups = [
        {
            display_name: 'Gemini Models',
            buckets: [
                { bucket_id: 'gemini-5h', window: '5h', remaining_fraction: 0.23, reset_time: fiveHourReset },
                { bucket_id: 'gemini-weekly', window: 'weekly', remaining_fraction: 0.82, reset_time: weeklyReset },
            ]
        },
        {
            display_name: 'Claude and GPT models',
            buckets: [
                { bucket_id: '3p-5h', window: '5h', remaining_fraction: 1.0, reset_time: fiveHourReset },
                { bucket_id: '3p-weekly', window: 'weekly', remaining_fraction: 0.67, reset_time: weeklyReset },
            ]
        }
    ];

    // gemini-weekly 必须展示自身的 82% 与 weeklyReset，绝不能被 gemini-5h 的 23% 和 fiveHourReset 覆盖劫持
    const geminiWeekly = getModelQuotaDisplay('gemini-weekly', undefined, groups);
    assertEqual(geminiWeekly.percentage, 82);
    assertEqual(geminiWeekly.resetTime, weeklyReset);

    // gemini-5h 必须展示自身的 23% 与 fiveHourReset
    const gemini5h = getModelQuotaDisplay('gemini-5h', undefined, groups);
    assertEqual(gemini5h.percentage, 23);
    assertEqual(gemini5h.resetTime, fiveHourReset);

    // 3p-weekly 必须展示自身的 67% 与 weeklyReset
    const p3Weekly = getModelQuotaDisplay('3p-weekly', undefined, groups);
    assertEqual(p3Weekly.percentage, 67);
    assertEqual(p3Weekly.resetTime, weeklyReset);

    // 3p-5h 必须展示自身的 100% 与 fiveHourReset
    const p35h = getModelQuotaDisplay('3p-5h', undefined, groups);
    assertEqual(p35h.percentage, 100);
    assertEqual(p35h.resetTime, fiveHourReset);
});

test('quota display: handles undefined/null buckets and groups safely without throwing', () => {
    const model = { name: 'gemini-3.1-pro-high', percentage: 75, reset_time: '2030-01-01T00:00:00Z' };
    const malformedGroups = [
        { display_name: 'Gemini Models' } as any,
        { display_name: 'Claude and GPT models', buckets: null } as any,
        { display_name: 'Other Models', buckets: undefined } as any,
    ];
    const display1 = getModelQuotaDisplay(model.name, model, malformedGroups);
    assertEqual(display1.percentage, 75);
    assertEqual(display1.isWeeklyConstrained, false);

    const display2 = getModelQuotaDisplay(model.name, model, undefined as any);
    assertEqual(display2.percentage, 75);
});

// ── Quota Buckets & Model Sorting ──────────────────────────────────────────

test('extractModelVersion: quota bucket ids do not match as version numbers', () => {
    const [maj1, min1] = extractModelVersion('gemini-5h');
    assertEqual(maj1, 0);
    assertEqual(min1, 0);

    const [maj2, min2] = extractModelVersion('gemini-weekly');
    assertEqual(maj2, 0);
    assertEqual(min2, 0);

    const [maj3, min3] = extractModelVersion('3p-5h');
    assertEqual(maj3, 0);
    assertEqual(min3, 0);

    const [maj4, min4] = extractModelVersion('3p-weekly');
    assertEqual(maj4, 0);
    assertEqual(min4, 0);
});

test('sortModels: quota buckets ordered Gemini 5h -> Gemini Weekly -> Claude/GPT 5h -> Claude/GPT Weekly', () => {
    // 验证 compareModelsDesc 与 QUOTA_BUCKET_ORDER
    assertEqual(QUOTA_BUCKET_ORDER['gemini-5h'], 1);
    assertEqual(QUOTA_BUCKET_ORDER['gemini-weekly'], 2);
    assertEqual(QUOTA_BUCKET_ORDER['3p-5h'], 3);
    assertEqual(QUOTA_BUCKET_ORDER['3p-weekly'], 4);

    assertEqual(compareModelsDesc({ id: 'gemini-5h' }, { id: 'gemini-weekly' }) < 0, true);
    assertEqual(compareModelsDesc({ id: 'gemini-weekly' }, { id: '3p-5h' }) < 0, true);
    assertEqual(compareModelsDesc({ id: '3p-5h' }, { id: '3p-weekly' }) < 0, true);

    // 任意乱序输入
    const input = [
        { id: '3p-weekly' },
        { id: 'gemini-weekly' },
        { id: '3p-5h' },
        { id: 'gemini-5h' },
    ];
    const sorted = sortModels(input).map(m => m.id);
    assertEqual(sorted[0], 'gemini-5h');
    assertEqual(sorted[1], 'gemini-weekly');
    assertEqual(sorted[2], '3p-5h');
    assertEqual(sorted[3], '3p-weekly');
});

test('sortModels: mixed quota buckets and model list', () => {
    const input = [
        { id: 'claude-sonnet-4-6' },
        { id: 'gemini-3.1-pro-high' },
        { id: '3p-weekly' },
        { id: 'gemini-weekly' },
        { id: '3p-5h' },
        { id: 'gemini-5h' },
    ];
    const sorted = sortModels(input).map(m => m.id);
    assertEqual(sorted[0], 'gemini-5h');
    assertEqual(sorted[1], 'gemini-weekly');
    assertEqual(sorted[2], '3p-5h');
    assertEqual(sorted[3], '3p-weekly');
    assertEqual(sorted[4], 'gemini-3.1-pro-high');
    assertEqual(sorted[5], 'claude-sonnet-4-6');
});

if (failed > 0) {
    throw new Error(`${failed} test(s) failed`);
}
