// Run after npm install: node scripts/test-opencode-profiles.mjs
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import ts from 'typescript';

const source = readFileSync(new URL('../src/utils/opencodeProfiles.ts', import.meta.url), 'utf8');
const { outputText } = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.ESNext } });
const { getProfileInfo, sha256Hex } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`);

const url = 'https://api.example.com/v1';
const key = 'test-key';
const id = getProfileInfo([], key, url).providerId;
const provider = { id, apiKey: key, baseUrl: url, npm: '@ai-sdk/openai-compatible', models: ['a', 'b'] };

test('SHA-256 agrees with Node crypto across UTF-8 and padding boundaries', () => {
    for (const input of ['', 'abc', 'Ключ 🔑', ...[55, 56, 63, 64, 65, 119, 120, 1000].map(n => 'a'.repeat(n))]) {
        assert.equal(sha256Hex(input), createHash('sha256').update(input).digest('hex'));
    }
});

test('trimmed keys have stable IDs and model sets ignore order and whitespace', () => {
    assert.equal(getProfileInfo([], ` ${key} `, url).providerId, id);
    assert.equal(getProfileInfo([provider], key, `${url}/`, [' b ', 'a', 'a']).status, 'synced');
    assert.equal(getProfileInfo([provider], key, url, ['a']).status, 'partial');
});

test('URL paths remain case sensitive and npm changes require an update', () => {
    assert.equal(getProfileInfo([provider], key, 'https://api.example.com/V1').status, 'partial');
    assert.equal(getProfileInfo([{ ...provider, npm: '@ai-sdk/anthropic' }], key, url).status, 'partial');
});

test('legacy profile is used only for its own key; per-key profile takes precedence', () => {
    const legacy = { ...provider, id: 'apikey-fun' };
    assert.equal(getProfileInfo([legacy], key, url).providerId, 'apikey-fun');
    assert.equal(getProfileInfo([legacy], 'different-key', url).status, 'not_present');
    assert.equal(getProfileInfo([legacy, provider], key, url).providerId, id);
});

test('short hash collision uses full hash without claiming another key', () => {
    const other = { ...provider, apiKey: 'other-key' };
    const result = getProfileInfo([other], key, url);
    assert.equal(result.status, 'not_present');
    assert.equal(result.providerId, `apikey-fun-${sha256Hex(key)}`);
    const fallback = { ...provider, id: result.providerId };
    assert.equal(getProfileInfo([fallback], key, url).providerId, fallback.id);
    assert.equal(getProfileInfo([other, fallback], key, url).status, 'synced');
});

test('missing key and unknown model inventory are handled', () => {
    assert.equal(getProfileInfo([], ' ', url).providerId, '');
    assert.equal(getProfileInfo([provider], key, url).status, 'synced');
    assert.equal(getProfileInfo([{ ...provider, models: [] }], key, url).status, 'partial');
});

test('OpenCode translations have matching keys and interpolation variables', () => {
    const locales = ['en', 'ru', 'zh', 'zh-TW'].map(lang =>
        JSON.parse(readFileSync(new URL(`../src/locales/${lang}.json`, import.meta.url), 'utf8')).apiKeyFun.opencode);
    for (const locale of locales.slice(1)) {
        assert.deepEqual(Object.keys(locale).sort(), Object.keys(locales[0]).sort());
        for (const key of Object.keys(locale)) {
            assert.deepEqual(locale[key].match(/\{\{.*?\}\}/g), locales[0][key].match(/\{\{.*?\}\}/g));
        }
    }
});
