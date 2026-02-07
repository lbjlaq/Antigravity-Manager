#!/usr/bin/env node
// File: scripts/sync-version.js
// Synchronizes version across all project files from a single source (Cargo.toml)

import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const ROOT = path.resolve(__dirname, '..');

// Files to sync
const FILES = {
  cargo: path.join(ROOT, 'src-tauri', 'Cargo.toml'),
  tauri: path.join(ROOT, 'src-tauri', 'tauri.conf.json'),
  package: path.join(ROOT, 'package.json'),
};

/**
 * Extract version from Cargo.toml (single source of truth)
 */
function getCargoVersion() {
  const content = fs.readFileSync(FILES.cargo, 'utf-8');
  const match = content.match(/^\s*version\s*=\s*"([^"]+)"/m);
  if (!match) {
    throw new Error('Could not find version in Cargo.toml');
  }
  return match[1];
}

/**
 * Set version in Cargo.toml
 */
function setCargoVersion(version) {
  let content = fs.readFileSync(FILES.cargo, 'utf-8');
  content = content.replace(
    /^(\s*version\s*=\s*)"[^"]+"/m,
    `$1"${version}"`
  );
  fs.writeFileSync(FILES.cargo, content, 'utf-8');
  console.log(`  ✓ Cargo.toml: ${version}`);
}

/**
 * Sync version to tauri.conf.json
 */
function syncTauriConfig(version) {
  const content = JSON.parse(fs.readFileSync(FILES.tauri, 'utf-8'));
  content.version = version;
  fs.writeFileSync(FILES.tauri, JSON.stringify(content, null, 2) + '\n', 'utf-8');
  console.log(`  ✓ tauri.conf.json: ${version}`);
}

/**
 * Sync version to package.json
 */
function syncPackageJson(version) {
  const content = JSON.parse(fs.readFileSync(FILES.package, 'utf-8'));
  content.version = version;
  fs.writeFileSync(FILES.package, JSON.stringify(content, null, 2) + '\n', 'utf-8');
  console.log(`  ✓ package.json: ${version}`);
}

/**
 * Bump version (major, minor, patch)
 */
function bumpVersion(version, type) {
  const parts = version.split('.').map(Number);
  if (parts.length !== 3 || parts.some(isNaN)) {
    throw new Error(`Invalid semver: ${version}`);
  }
  
  switch (type) {
    case 'major':
      parts[0]++;
      parts[1] = 0;
      parts[2] = 0;
      break;
    case 'minor':
      parts[1]++;
      parts[2] = 0;
      break;
    case 'patch':
      parts[2]++;
      break;
    default:
      throw new Error(`Unknown bump type: ${type}. Use: major, minor, patch`);
  }
  
  return parts.join('.');
}

/**
 * Main entry point
 */
function main() {
  const args = process.argv.slice(2);
  const command = args[0];
  
  console.log('\n🔄 Antigravity Version Sync\n');
  
  if (!command || command === 'sync') {
    // Sync all files to Cargo.toml version
    const version = getCargoVersion();
    console.log(`📦 Syncing all files to version: ${version}\n`);
    syncTauriConfig(version);
    syncPackageJson(version);
    console.log('\n✅ Version sync complete!\n');
    
  } else if (command === 'get') {
    // Just print current version
    const version = getCargoVersion();
    console.log(version);
    
  } else if (command === 'set') {
    // Set specific version
    const newVersion = args[1];
    if (!newVersion) {
      console.error('❌ Usage: sync-version.js set <version>');
      console.error('   Example: sync-version.js set 5.1.0');
      process.exit(1);
    }
    
    console.log(`📦 Setting version to: ${newVersion}\n`);
    setCargoVersion(newVersion);
    syncTauriConfig(newVersion);
    syncPackageJson(newVersion);
    console.log('\n✅ Version set complete!\n');
    
  } else if (['major', 'minor', 'patch'].includes(command)) {
    // Bump version
    const currentVersion = getCargoVersion();
    const newVersion = bumpVersion(currentVersion, command);
    
    console.log(`📦 Bumping ${command}: ${currentVersion} → ${newVersion}\n`);
    setCargoVersion(newVersion);
    syncTauriConfig(newVersion);
    syncPackageJson(newVersion);
    console.log('\n✅ Version bump complete!\n');
    
  } else {
    console.log(`
Usage: node scripts/sync-version.js <command>

Commands:
  sync          Sync all files to Cargo.toml version (default)
  get           Print current version
  set <version> Set specific version (e.g., set 5.1.0)
  major         Bump major version (5.0.0 → 6.0.0)
  minor         Bump minor version (5.0.0 → 5.1.0)
  patch         Bump patch version (5.0.0 → 5.0.1)

Examples:
  npm run version:sync
  npm run version:set 5.1.0
  npm run version:patch
`);
  }
}

main();
