/**
 * Standalone test script for tagUtils.
 * Run: npx tsx src/utils/__tests__/tagUtils.test.ts
 */
import { parseTags, formatTags, reorderTags } from '../tagUtils';

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

test('parses empty or null label to empty array', () => {
    assertDeepEqual(parseTags(undefined), []);
    assertDeepEqual(parseTags(null), []);
    assertDeepEqual(parseTags(''), []);
    assertDeepEqual(parseTags('   '), []);
});

test('parses comma-separated labels into trimmed tags', () => {
    assertDeepEqual(parseTags('主力, 备用, 开发者'), ['主力', '备用', '开发者']);
    assertDeepEqual(parseTags('TagA，TagB, TagC'), ['TagA', 'TagB', 'TagC']); // 中英文逗号兼容
});

test('formats tag array back to clean comma-separated string', () => {
    assertEqual(formatTags(['主力', '备用']), '主力, 备用');
    assertEqual(formatTags([]), '');
    assertEqual(formatTags([' 单标签 ']), '单标签');
});

test('reorders tags safely within bounds', () => {
    const original = ['A', 'B', 'C'];
    assertDeepEqual(reorderTags(original, 0, 2), ['B', 'C', 'A']);
    assertDeepEqual(reorderTags(original, 2, 0), ['C', 'A', 'B']);
    assertDeepEqual(reorderTags(original, -1, 5), ['A', 'B', 'C']);
});

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) {
    throw new Error(`${failed} test(s) failed`);
}
