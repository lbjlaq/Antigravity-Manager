function isDeepEqual(a: unknown, b: unknown): boolean {
    if (Object.is(a, b)) return true;
    if (typeof a !== typeof b || a === null || b === null) return false;
    if (Array.isArray(a) || Array.isArray(b)) {
        if (!Array.isArray(a) || !Array.isArray(b) || a.length !== b.length) return false;
        return a.every((item, idx) => isDeepEqual(item, b[idx]));
    }
    if (typeof a === 'object' && typeof b === 'object') {
        const objA = a as Record<string, unknown>;
        const objB = b as Record<string, unknown>;
        const keysA = Object.keys(objA).filter(k => objA[k] !== undefined);
        const keysB = Object.keys(objB).filter(k => objB[k] !== undefined);
        if (keysA.length !== keysB.length) return false;
        return keysA.every(k => Object.prototype.hasOwnProperty.call(objB, k) && isDeepEqual(objA[k], objB[k]));
    }
    return false;
}

const assert = {
    equal<T>(actual: T, expected: T, msg?: string): void {
        if (!Object.is(actual, expected)) {
            throw new Error(`${msg ? `${msg}: ` : ''}expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
        }
    },
    deepEqual<T>(actual: T, expected: T, msg?: string): void {
        if (!isDeepEqual(actual, expected)) {
            throw new Error(`${msg ? `${msg}: ` : ''}expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
        }
    },
    throws(fn: () => unknown, pattern: RegExp): void {
        let threw = false;
        try {
            fn();
        } catch (err) {
            threw = true;
            const message = err instanceof Error ? err.message : String(err);
            if (!pattern.test(message)) {
                throw new Error(`Expected error matching ${pattern}, got: ${message}`);
            }
        }
        if (!threw) {
            throw new Error(`Expected function to throw matching ${pattern}, but it did not throw`);
        }
    },
};

import {
    buildModelPriorityPickerOptions,
    buildPedalBoostedModelPriorities,
    formatAccountPriorityBadge,
    formatAccountPriorityTooltip,
    getCanonicalModelSeriesId,
    getSpecificModelPriorityEntries,
    hasAnyCustomModelPriorities,
    isValidModelPriorityKey,
    normalizeModelPrioritiesPayload,
    resolveEffectiveModelPriorityDetail,
    stripAllTierSuffixes,
} from '../modelPriority';

let passed = 0;
let failed = 0;

function test(name: string, fn: () => void) {
    try {
        fn();
        console.log(`  PASS: ${name}`);
        passed++;
    } catch (error) {
        console.error(`  FAIL: ${name}`);
        console.error(error);
        failed++;
    }
}

console.log('Running modelPriority unit tests...');

test('validates and normalizes model priority keys', () => {
    assert.equal(isValidModelPriorityKey('gemini'), true);
    assert.equal(isValidModelPriorityKey('claude'), true);
    assert.equal(isValidModelPriorityKey('Gemini-4-Argon'), true);
    assert.equal(isValidModelPriorityKey('claude-opus-5-5-high'), true);
    assert.equal(isValidModelPriorityKey('gpt-oss-120b-medium'), true);
    assert.equal(isValidModelPriorityKey('anthropic/claude-sonnet-4.6'), true);
    assert.equal(isValidModelPriorityKey(''), false);
    assert.equal(isValidModelPriorityKey('   '), false);
    assert.equal(isValidModelPriorityKey('bad model'), false);
    assert.equal(isValidModelPriorityKey('gemini-*'), false);
    assert.equal(isValidModelPriorityKey('../etc'), false);
    assert.equal(isValidModelPriorityKey('-leading'), false);
    assert.equal(isValidModelPriorityKey('a'.repeat(129)), false);
});

test('strips single and double tier suffixes to canonical series ID', () => {
    assert.deepEqual(stripAllTierSuffixes('gemini-4-argon-high'), ['gemini-4-argon']);
    assert.deepEqual(stripAllTierSuffixes('gemini-4-hydrogen-tiered-xhigh'), [
        'gemini-4-hydrogen-tiered',
        'gemini-4-hydrogen',
    ]);
    assert.equal(getCanonicalModelSeriesId('gemini-4-hydrogen-tiered-xhigh'), 'gemini-4-hydrogen');
    assert.equal(getCanonicalModelSeriesId('claude-opus-5-5-medium'), 'claude-opus-5-5');
    assert.equal(getCanonicalModelSeriesId('gemini-3.1-flash-image-4k'), 'gemini-3.1-flash-image');
});

test('resolves hierarchical priority precedence (exact > series > family > default)', () => {
    const account = {
        priority: 50,
        model_priorities: {
            gemini: 80,
            claude: 70,
            'gemini-4-argon': 20,
            'gemini-4-argon-high': 5,
            'gemini-4-hydrogen': 15,
            'gemini-3.1-flash-image': 30,
            'claude-opus-5-5': 8,
            'gpt-oss-120b': 90,
        },
    };

    assert.deepEqual(resolveEffectiveModelPriorityDetail(account, 'gemini-4-argon-high'), {
        priority: 5,
        source: 'exact',
        matchedKey: 'gemini-4-argon-high',
    });
    assert.deepEqual(resolveEffectiveModelPriorityDetail(account, 'gemini-4-argon-low'), {
        priority: 20,
        source: 'series',
        matchedKey: 'gemini-4-argon',
    });
    assert.deepEqual(resolveEffectiveModelPriorityDetail(account, 'gemini-4-hydrogen-tiered-high'), {
        priority: 15,
        source: 'series',
        matchedKey: 'gemini-4-hydrogen',
    });
    assert.deepEqual(resolveEffectiveModelPriorityDetail(account, 'gemini-3.1-flash-image-4k'), {
        priority: 30,
        source: 'series',
        matchedKey: 'gemini-3.1-flash-image',
    });
    assert.deepEqual(resolveEffectiveModelPriorityDetail(account, 'gemini-3.8-flash-high'), {
        priority: 80,
        source: 'family',
        matchedKey: 'gemini',
    });
    assert.deepEqual(resolveEffectiveModelPriorityDetail(account, 'claude-opus-5-5-medium'), {
        priority: 8,
        source: 'series',
        matchedKey: 'claude-opus-5-5',
    });
    assert.deepEqual(resolveEffectiveModelPriorityDetail(account, 'claude-sonnet-5-5-high'), {
        priority: 70,
        source: 'family',
        matchedKey: 'claude',
    });
    assert.deepEqual(resolveEffectiveModelPriorityDetail(account, 'gpt-oss-120b-medium'), {
        priority: 90,
        source: 'series',
        matchedKey: 'gpt-oss-120b',
    });
    assert.deepEqual(resolveEffectiveModelPriorityDetail({ priority: 42 }, 'gemini-3.8-flash-high'), {
        priority: 42,
        source: 'default',
    });
});

test('formats account card/table priority badge and tooltip cleanly', () => {
    const plain = { priority: 50 };
    assert.equal(hasAnyCustomModelPriorities(plain), false);
    assert.equal(formatAccountPriorityBadge(plain, 'Priority'), 'Priority: 50');

    const familyOnly = { priority: 50, model_priorities: { gemini: 80, claude: 1 } };
    assert.equal(hasAnyCustomModelPriorities(familyOnly), true);
    assert.equal(formatAccountPriorityBadge(familyOnly, 'Priority'), 'Priority: G:80 · C:1');

    const specificOnly = {
        priority: 50,
        model_priorities: { 'gemini-4-argon': 1, 'claude-opus-5-5': 5 },
    };
    assert.equal(hasAnyCustomModelPriorities(specificOnly), true);
    assert.equal(formatAccountPriorityBadge(specificOnly, 'Priority'), 'Priority: 50 · +2');
    assert.equal(
        formatAccountPriorityTooltip(specificOnly, { priority: 'Priority', defaultHint: 'hint' }),
        'Gemini: 50 | Claude: 50 (Priority: 50) | claude-opus-5-5: 5, gemini-4-argon: 1'
    );

    const mixed = {
        priority: 50,
        model_priorities: { gemini: 80, 'gemini-4-argon': 1 },
    };
    assert.equal(formatAccountPriorityBadge(mixed, 'Priority'), 'Priority: G:80 · C:50 · +1');
    assert.deepEqual(getSpecificModelPriorityEntries(mixed.model_priorities), [['gemini-4-argon', 1]]);
});

test('builds deduplicated series and exact picker options excluding configured keys', () => {
    const { seriesOptions, exactOptions } = buildModelPriorityPickerOptions(
        [
            { name: 'gemini-3.8-flash-high', display_name: 'Gemini 3.8 Flash (High)' },
            { name: 'gemini-3.8-flash-medium', display_name: 'Gemini 3.8 Flash (Medium)' },
            { name: 'gemini-3.1-flash-image', display_name: 'Gemini 3.1 Flash Image' },
            { name: 'claude-opus-5-5-high', display_name: 'Claude Opus 5.5 (High)' },
        ],
        new Set(['gemini-3.8-flash'])
    );

    assert.deepEqual(
        seriesOptions.map(o => o.key),
        ['claude-opus-5-5', 'gemini-3.1-flash-image']
    );
    assert.deepEqual(
        exactOptions.map(o => o.key),
        ['claude-opus-5-5-high', 'gemini-3.8-flash-high', 'gemini-3.8-flash-medium']
    );
    assert.equal(seriesOptions[0].label, 'Claude Opus 5.5');
});

test('Pedal Mode boost clears shadowing target-family overrides while preserving other family overrides', () => {
    const boosted = buildPedalBoostedModelPriorities(
        50,
        {
            gemini: 80,
            'gemini-3.8-flash': 90,
            'gemini-4-argon': 40,
            claude: 75,
            'claude-opus-5-5': 5,
            'gpt-oss-120b': 85,
        },
        'gemini'
    );

    assert.deepEqual(boosted, {
        gemini: 1,
        claude: 75,
        'claude-opus-5-5': 5,
        'gpt-oss-120b': 85,
    });
});

test('normalizeModelPrioritiesPayload rejects invalid keys, duplicate normalized keys, and invalid values', () => {
    assert.deepEqual(
        normalizeModelPrioritiesPayload(
            { 'Gemini-4-Argon': 10, claude: 80 },
            'invalid priority',
            k => `invalid key ${k}`
        ),
        { 'gemini-4-argon': 10, claude: 80 }
    );
    assert.throws(
        () =>
            normalizeModelPrioritiesPayload(
                { Gemini: 10, gemini: 20 },
                'invalid priority',
                k => `invalid key ${k}`
            ),
        /invalid key/
    );
    assert.throws(
        () =>
            normalizeModelPrioritiesPayload(
                { 'bad key': 10 },
                'invalid priority',
                k => `invalid key ${k}`
            ),
        /invalid key/
    );
    assert.throws(
        () =>
            normalizeModelPrioritiesPayload(
                { 'gemini-4-argon': 101 },
                'invalid priority',
                k => `invalid key ${k}`
            ),
        /invalid priority/
    );
});

console.log(`\nResults: ${passed} passed, ${failed} failed`);
if (failed > 0) {
    throw new Error(`modelPriority unit tests failed: ${failed}`);
}
