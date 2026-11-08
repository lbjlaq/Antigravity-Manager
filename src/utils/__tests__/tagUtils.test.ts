import { describe, it } from 'node:test';
import assert from 'node:assert';
import { parseTags, formatTags, reorderTags } from '../tagUtils';

describe('tagUtils', () => {
    it('parses empty or null label to empty array', () => {
        assert.deepStrictEqual(parseTags(undefined), []);
        assert.deepStrictEqual(parseTags(null), []);
        assert.deepStrictEqual(parseTags(''), []);
        assert.deepStrictEqual(parseTags('   '), []);
    });

    it('parses comma-separated labels into trimmed tags', () => {
        assert.deepStrictEqual(parseTags('主力, 备用, 开发者'), ['主力', '备用', '开发者']);
        assert.deepStrictEqual(parseTags('TagA，TagB, TagC'), ['TagA', 'TagB', 'TagC']); // 中英文逗号兼容
    });

    it('formats tag array back to clean comma-separated string', () => {
        assert.strictEqual(formatTags(['主力', '备用']), '主力, 备用');
        assert.strictEqual(formatTags([]), '');
        assert.strictEqual(formatTags([' 单标签 ']), '单标签');
    });

    it('reorders tags safely within bounds', () => {
        const original = ['A', 'B', 'C'];
        assert.deepStrictEqual(reorderTags(original, 0, 2), ['B', 'C', 'A']);
        assert.deepStrictEqual(reorderTags(original, 2, 0), ['C', 'A', 'B']);
        assert.deepStrictEqual(reorderTags(original, -1, 5), ['A', 'B', 'C']);
    });
});
