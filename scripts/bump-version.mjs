#!/usr/bin/env node

/**
 * Antigravity Manager - Atomic Multi-Manifest Version Bump Script
 *
 * Usage:
 *   node scripts/bump-version.mjs patch              # 0.0.1 -> 0.0.2
 *   node scripts/bump-version.mjs minor              # 0.0.1 -> 0.1.0
 *   node scripts/bump-version.mjs major              # 0.0.1 -> 1.0.0
 *   node scripts/bump-version.mjs beta               # 0.0.1 -> 0.0.2-beta.1 (or increment beta.N)
 *   node scripts/bump-version.mjs 0.1.0              # Set explicit SemVer
 *   node scripts/bump-version.mjs 0.1.0 --dry-run    # Preview without modifying files
 *   node scripts/bump-version.mjs 0.1.0 --commit     # Auto git commit
 */

import fs from 'node:fs';
import path from 'node:path';
import { execFileSync, execSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const ROOT_DIR = path.resolve(__dirname, '..');

const TAURI_CONF = path.join(ROOT_DIR, 'crates/am-gui/src-tauri/tauri.conf.json');
const UI_PKG = path.join(ROOT_DIR, 'crates/am-gui/ui/package.json');
const UI_LOCK = path.join(ROOT_DIR, 'crates/am-gui/ui/package-lock.json');
const CARGO_TOMLS = [
  'crates/am-core/Cargo.toml',
  'crates/am-cli/Cargo.toml',
  'crates/am-gui/src-tauri/Cargo.toml',
];
const BREW_CASK = path.join(ROOT_DIR, 'Casks/antigravity-manager.rb');
const CHANGELOG = path.join(ROOT_DIR, 'CHANGELOG.md');

const SEMVER_RE = /^v?(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?$/;

function parseSemVer(raw) {
  const m = String(raw || '').trim().match(SEMVER_RE);
  if (!m) return null;
  return {
    major: Number(m[1]),
    minor: Number(m[2]),
    patch: Number(m[3]),
    prerelease: m[4] || null,
  };
}

function formatSemVer(v) {
  return `${v.major}.${v.minor}.${v.patch}${v.prerelease ? `-${v.prerelease}` : ''}`;
}

function resolveNextVersion(current, targetArg) {
  const arg = targetArg.toLowerCase();
  if (arg === 'patch') {
    return current.prerelease
      ? { major: current.major, minor: current.minor, patch: current.patch, prerelease: null }
      : { major: current.major, minor: current.minor, patch: current.patch + 1, prerelease: null };
  }
  if (arg === 'minor') {
    return { major: current.major, minor: current.minor + 1, patch: 0, prerelease: null };
  }
  if (arg === 'major') {
    return { major: current.major + 1, minor: 0, patch: 0, prerelease: null };
  }
  if (arg === 'beta') {
    if (current.prerelease) {
      const m = current.prerelease.match(/^beta\.(\d+)$/);
      if (m) {
        return { ...current, prerelease: `beta.${Number(m[1]) + 1}` };
      }
      return { ...current, prerelease: 'beta.1' };
    }
    return { major: current.major, minor: current.minor, patch: current.patch + 1, prerelease: 'beta.1' };
  }
  const explicit = parseSemVer(targetArg);
  if (!explicit) {
    console.error(`Invalid version argument: "${targetArg}"`);
    process.exit(1);
  }
  return explicit;
}

function updateJsonVersion(filePath, newVersion, isDryRun) {
  if (!fs.existsSync(filePath)) return false;
  const raw = fs.readFileSync(filePath, 'utf8');
  const json = JSON.parse(raw);
  let changed = false;
  if (json.version !== newVersion) {
    json.version = newVersion;
    changed = true;
  }
  if (json.packages && json.packages[''] && json.packages[''].version !== newVersion) {
    json.packages[''].version = newVersion;
    changed = true;
  }
  if (changed && !isDryRun) {
    fs.writeFileSync(filePath, `${JSON.stringify(json, null, 2)}\n`, 'utf8');
  }
  return changed;
}

function updateCargoTomlVersion(relPath, newVersion, isDryRun) {
  const fullPath = path.join(ROOT_DIR, relPath);
  if (!fs.existsSync(fullPath)) return false;
  const raw = fs.readFileSync(fullPath, 'utf8');
  const updated = raw.replace(
    /(\[package\][\s\S]*?^version\s*=\s*")([^"]+)(")/m,
    `$1${newVersion}$3`
  );
  if (updated === raw) return false;
  if (!isDryRun) {
    fs.writeFileSync(fullPath, updated, 'utf8');
  }
  return true;
}

function updateBrewCaskVersion(filePath, newVersion, isDryRun) {
  if (!fs.existsSync(filePath)) return false;
  const raw = fs.readFileSync(filePath, 'utf8');
  const updated = raw.replace(
    /^(\s*version\s+)(?:"[^"]+"|:latest)\s*$/m,
    `$1"${newVersion}"`
  );
  if (updated === raw) return false;
  if (!isDryRun) {
    fs.writeFileSync(filePath, updated, 'utf8');
  }
  return true;
}

function ensureChangelogSkeleton(newVersion, isDryRun) {
  if (!fs.existsSync(CHANGELOG)) return false;
  const raw = fs.readFileSync(CHANGELOG, 'utf8');
  const heading = `## v${newVersion}`;
  if (raw.includes(`${heading}\n`) || raw.includes(`${heading} (`) || raw.includes(`${heading} `)) {
    return false;
  }
  const today = new Date().toISOString().slice(0, 10);
  const block = `${heading} (${today})\n\n### Highlights\n- TODO: Summarize release highlights\n\n`;
  const idx = raw.indexOf('\n## ');
  const updated = idx >= 0
    ? `${raw.slice(0, idx + 1)}${block}${raw.slice(idx + 1)}`
    : `${raw.trimEnd()}\n\n${block}`;
  if (!isDryRun) {
    fs.writeFileSync(CHANGELOG, updated, 'utf8');
  }
  return true;
}

function main() {
  const args = process.argv.slice(2);
  const isDryRun = args.includes('--dry-run');
  const autoCommit = args.includes('--commit');
  const targetArg = args.find((a) => !a.startsWith('--'));

  const tauriConf = JSON.parse(fs.readFileSync(TAURI_CONF, 'utf8'));
  const current = parseSemVer(tauriConf.version);
  if (!current) {
    console.error(`Invalid current version in tauri.conf.json: "${tauriConf.version}"`);
    process.exit(1);
  }

  if (!targetArg) {
    console.log(`Current version: ${tauriConf.version}`);
    console.log('Usage: node scripts/bump-version.mjs <patch|minor|major|beta|X.Y.Z> [--dry-run] [--commit]');
    process.exit(0);
  }

  const next = resolveNextVersion(current, targetArg);
  const newVersion = formatSemVer(next);
  console.log(`[bump-version] ${tauriConf.version} -> ${newVersion}${isDryRun ? ' (dry-run)' : ''}`);

  let updatedCount = 0;
  for (const [label, changed] of [
    ['crates/am-gui/src-tauri/tauri.conf.json', updateJsonVersion(TAURI_CONF, newVersion, isDryRun)],
    ['crates/am-gui/ui/package.json', updateJsonVersion(UI_PKG, newVersion, isDryRun)],
    ['crates/am-gui/ui/package-lock.json', updateJsonVersion(UI_LOCK, newVersion, isDryRun)],
    ...CARGO_TOMLS.map((p) => [p, updateCargoTomlVersion(p, newVersion, isDryRun)]),
    ['Casks/antigravity-manager.rb', updateBrewCaskVersion(BREW_CASK, newVersion, isDryRun)],
    ['CHANGELOG.md', ensureChangelogSkeleton(newVersion, isDryRun)],
  ]) {
    if (changed) {
      updatedCount += 1;
      console.log(`  ✓ ${label}`);
    } else {
      console.log(`  - ${label} (unchanged)`);
    }
  }

  if (!isDryRun && updatedCount > 0) {
    try {
      execSync('cargo metadata --format-version 1 > /dev/null', { cwd: ROOT_DIR, stdio: 'ignore' });
      console.log('  ✓ Cargo.lock synced');
    } catch {
      console.warn('  ! Skipped Cargo.lock sync (cargo unavailable)');
    }
  }

  if (!isDryRun && autoCommit) {
    execFileSync('git', ['add', '-A'], { cwd: ROOT_DIR, stdio: 'ignore' });
    execFileSync('git', ['commit', '-m', `chore(release): bump version to v${newVersion}`], {
      cwd: ROOT_DIR,
      stdio: 'inherit',
    });
  }

  console.log(`\nNext steps for Antigravity Manager channel:`);
  console.log(`  1. Fill release notes under "## v${newVersion}" in CHANGELOG.md`);
  console.log(`  2. Push branch: git push origin antigravity-manager`);
  console.log(`  3. Tag & publish archived + rolling release:`);
  console.log(`     git tag am-v${newVersion} && git push origin am-v${newVersion}`);
}

main();
