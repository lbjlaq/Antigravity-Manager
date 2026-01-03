#!/usr/bin/env node
import { execSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { extname } from 'node:path';

const args = new Set(process.argv.slice(2));
const scanMode = args.has('--staged') ? 'staged' : 'tracked';
const verbose = args.has('--verbose');

const MAX_BYTES = 2_000_000;
const BINARY_EXTENSIONS = new Set([
    '.png',
    '.jpg',
    '.jpeg',
    '.gif',
    '.webp',
    '.ico',
    '.icns',
    '.pdf',
    '.zip',
    '.gz',
    '.tgz',
    '.bz2',
    '.7z',
    '.dmg',
    '.exe',
    '.dll',
    '.so',
    '.dylib',
    '.wasm',
    '.woff',
    '.woff2',
    '.ttf',
    '.otf',
]);

function runGit(cmd) {
    return execSync(cmd, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();
}

function getFiles() {
    if (scanMode === 'staged') {
        const out = runGit('git diff --cached --name-only --diff-filter=ACMR');
        return out ? out.split('\n').filter(Boolean) : [];
    }
    const out = runGit('git ls-files');
    return out ? out.split('\n').filter(Boolean) : [];
}

function looksBinary(buf) {
    const limit = Math.min(buf.length, 8192);
    for (let i = 0; i < limit; i++) {
        if (buf[i] === 0) return true;
    }
    return false;
}

function maskValue(value) {
    if (!value) return value;
    if (value.length <= 8) return '[REDACTED]';
    return `${value.slice(0, 4)}…${value.slice(-4)}`;
}

function maskEmail(email) {
    const at = email.indexOf('@');
    if (at <= 0) return '[REDACTED_EMAIL]';
    const user = email.slice(0, at);
    const domain = email.slice(at + 1);
    const userMasked = user.length <= 2 ? `${user[0] ?? ''}…` : `${user.slice(0, 1)}…${user.slice(-1)}`;
    const domainMasked =
        domain.length <= 6 ? `${domain.slice(0, 1)}…` : `${domain.slice(0, 2)}…${domain.slice(-2)}`;
    return `${userMasked}@${domainMasked}`;
}

function describeMatch(kind, match) {
    if (kind === 'email') return maskEmail(match);
    return maskValue(match);
}

// Keep this intentionally conservative (low false negatives, acceptable false positives).
const RULES = [
    { id: 'private_key_block', kind: 'key', re: /-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----/ },
    { id: 'anthropic_key', kind: 'key', re: /\bsk-ant-[A-Za-z0-9_-]{16,}\b/ },
    { id: 'openai_key', kind: 'key', re: /\bsk-[A-Za-z0-9]{16,}\b/ },
    { id: 'github_token', kind: 'key', re: /\b(?:ghp|gho|ghs|ghr)_[A-Za-z0-9]{20,}\b/ },
    { id: 'github_pat', kind: 'key', re: /\bgithub_pat_[A-Za-z0-9_]{20,}\b/ },
    { id: 'aws_access_key', kind: 'key', re: /\bAKIA[0-9A-Z]{16}\b/ },
    { id: 'google_oauth', kind: 'key', re: /\bya29\.[0-9A-Za-z_-]{20,}\b/ },
    { id: 'slack_token', kind: 'key', re: /\bxox(?:b|p|a)-[0-9A-Za-z-]{10,}\b/ },
    // PII: real email addresses (allow common placeholder domains).
    {
        id: 'email',
        kind: 'email',
        // Avoid matching common asset suffixes like `128x128@2x.png`.
        re: /\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.(?!png\b|jpe?g\b|gif\b|webp\b|svg\b|ico\b|icns\b)[A-Z]{2,}\b/i,
        allow: /\b[A-Z0-9._%+-]+@(example\.com|example\.org|example\.net|test\.com|invalid)\b/i,
    },
];

function scanText(file, text) {
    const findings = [];
    const lines = text.split(/\r?\n/);
    for (let i = 0; i < lines.length; i++) {
        const line = lines[i];
        for (const rule of RULES) {
            const m = line.match(rule.re);
            if (!m) continue;
            const value = m[0];
            if (rule.allow && rule.allow.test(value)) continue;
            findings.push({
                file,
                line: i + 1,
                rule: rule.id,
                preview: describeMatch(rule.kind, value),
            });
        }
    }
    return findings;
}

function shouldSkipFile(file) {
    const ext = extname(file).toLowerCase();
    if (BINARY_EXTENSIONS.has(ext)) return true;
    return false;
}

function main() {
    const files = getFiles();
    if (verbose) {
        console.log(`[secret-scan] mode=${scanMode} files=${files.length}`);
    }

    const allFindings = [];
    for (const file of files) {
        if (shouldSkipFile(file)) continue;

        let buf;
        try {
            buf = readFileSync(file);
        } catch {
            continue;
        }

        if (buf.length > MAX_BYTES) continue;
        if (looksBinary(buf)) continue;

        const text = buf.toString('utf8');
        allFindings.push(...scanText(file, text));
    }

    if (allFindings.length === 0) {
        console.log('[secret-scan] OK: no secrets/PII patterns found.');
        process.exit(0);
    }

    console.error(`[secret-scan] FAIL: found ${allFindings.length} potential secret/PII match(es):`);
    for (const f of allFindings) {
        console.error(`- ${f.file}:${f.line} [${f.rule}] ${f.preview}`);
    }
    console.error(
        '\nIf this is a false positive, replace the value with a placeholder (e.g. <your_api_key>) or use an example.* domain for emails.',
    );
    process.exit(1);
}

main();
