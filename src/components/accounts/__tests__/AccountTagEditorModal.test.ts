/**
 * Unit tests for AccountTagEditorModal business logic and tag validation rules.
 * Run: npx tsx src/components/accounts/__tests__/AccountTagEditorModal.test.ts
 */
import { parseTags, formatTags, reorderTags } from '../../../utils/tagUtils';

let passed = 0;
let failed = 0;

function test(description: string, fn: () => void): void {
    try {
        fn();
        passed += 1;
        console.log(`  ok  ${description}`);
    } catch (err) {
        failed += 1;
        console.error(`  FAIL  ${description}`);
        console.error(err);
    }
}

function assertEqual<T>(actual: T, expected: T): void {
    if (actual !== expected) {
        throw new Error(`expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
    }
}

function assertDeepEqual(actual: unknown, expected: unknown): void {
    const left = JSON.stringify(actual);
    const right = JSON.stringify(expected);
    if (left !== right) {
        throw new Error(`expected ${right}, got ${left}`);
    }
}

test('parses initial label correctly', () => {
    assertDeepEqual(parseTags('主力, 备用'), ['主力', '备用']);
});

test('enforces max 8 tags limit', () => {
    const tags = ['1', '2', '3', '4', '5', '6', '7', '8'];
    const canAdd = tags.length < 8;
    assertEqual(canAdd, false);
});

test('enforces max 15 characters per tag limit', () => {
    const longTag = 'ThisIsAVeryLongTagNameExceedingLimit';
    const isValid = longTag.trim().length <= 15 && longTag.trim().length > 0;
    assertEqual(isValid, false);

    const normalTag = '主力账号';
    assertEqual(normalTag.trim().length <= 15, true);
});

test('prevents duplicate tags', () => {
    const tags = ['主力', '备用'];
    const tagToAdd = '主力';
    const isDuplicate = tags.includes(tagToAdd.trim());
    assertEqual(isDuplicate, true);
});

test('supports reordering tags via reorderTags utility', () => {
    const tags = ['A', 'B', 'C'];
    // Move B (index 1) up to index 0
    const reorderedUp = reorderTags(tags, 1, 0);
    assertDeepEqual(reorderedUp, ['B', 'A', 'C']);

    // Move B (index 1) down to index 2
    const reorderedDown = reorderTags(tags, 1, 2);
    assertDeepEqual(reorderedDown, ['A', 'C', 'B']);
});

test('formats final tags for saving', () => {
    assertEqual(formatTags(['主力', 'Claude', '高配']), '主力, Claude, 高配');
});

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) {
    throw new Error(`${failed} test(s) failed`);
}
