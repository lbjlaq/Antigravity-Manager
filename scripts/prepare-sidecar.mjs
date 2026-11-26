#!/usr/bin/env node
/**
 * Build `am-cli` (`am` / `am.exe`) and stage it into Tauri's `externalBin` directory:
 *   crates/am-gui/src-tauri/binaries/am-<target-triple>[.exe]
 *
 * Usage:
 *   node scripts/prepare-sidecar.mjs
 *   node scripts/prepare-sidecar.mjs --target aarch64-apple-darwin
 *   node scripts/prepare-sidecar.mjs --target universal-apple-darwin
 *   node scripts/prepare-sidecar.mjs --target x86_64-pc-windows-msvc
 *   node scripts/prepare-sidecar.mjs --debug
 */

import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const repoRoot = path.resolve(__dirname, '..');
const binariesDir = path.join(repoRoot, 'crates', 'am-gui', 'src-tauri', 'binaries');

function parseArgs(argv) {
  let target = '';
  let debug = false;
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--target' && argv[i + 1]) {
      target = argv[i + 1];
      i += 1;
    } else if (arg.startsWith('--target=')) {
      target = arg.slice('--target='.length);
    } else if (arg === '--debug') {
      debug = true;
    }
  }
  return { target, debug };
}

function getHostTriple() {
  const out = execFileSync('rustc', ['-vV'], { encoding: 'utf8' });
  const match = out.match(/^host:\s*(\S+)/m);
  if (!match) {
    throw new Error('Failed to detect Rust host target triple from `rustc -vV`.');
  }
  return match[1];
}

function run(cmd, args) {
  console.log(`> ${cmd} ${args.join(' ')}`);
  execFileSync(cmd, args, { cwd: repoRoot, stdio: 'inherit' });
}

function stageSingleTarget(triple, useTargetFlag, debug) {
  const profileDir = debug ? 'debug' : 'release';
  const cargoArgs = ['build', '-p', 'am-cli'];
  if (!debug) {
    cargoArgs.push('--release');
  }
  if (useTargetFlag) {
    cargoArgs.push('--target', triple);
  }

  run('cargo', cargoArgs);

  const isWindows = triple.includes('windows');
  const ext = isWindows ? '.exe' : '';
  const srcPath = useTargetFlag
    ? path.join(repoRoot, 'target', triple, profileDir, `am${ext}`)
    : path.join(repoRoot, 'target', profileDir, `am${ext}`);
  const destPath = path.join(binariesDir, `am-${triple}${ext}`);

  if (!fs.existsSync(srcPath)) {
    throw new Error(`Compiled am binary not found at ${srcPath}`);
  }

  fs.mkdirSync(binariesDir, { recursive: true });
  fs.copyFileSync(srcPath, destPath);
  if (!isWindows) {
    fs.chmodSync(destPath, 0o755);
  }

  const size = fs.statSync(destPath).size;
  if (size <= 0) {
    throw new Error(`Staged sidecar binary is empty: ${destPath}`);
  }
  console.log(`Staged sidecar: ${path.relative(repoRoot, destPath)} (${(size / 1024 / 1024).toFixed(2)} MB)`);
  return destPath;
}

function main() {
  const { target, debug } = parseArgs(process.argv.slice(2));
  const hostTriple = getHostTriple();
  const resolvedTarget = target || hostTriple;

  if (resolvedTarget === 'universal-apple-darwin') {
    const armBin = stageSingleTarget('aarch64-apple-darwin', true, debug);
    const x64Bin = stageSingleTarget('x86_64-apple-darwin', true, debug);
    const universalPath = path.join(binariesDir, 'am-universal-apple-darwin');
    run('lipo', ['-create', '-output', universalPath, armBin, x64Bin]);
    fs.chmodSync(universalPath, 0o755);
    const size = fs.statSync(universalPath).size;
    console.log(
      `Staged universal sidecar: ${path.relative(repoRoot, universalPath)} (${(size / 1024 / 1024).toFixed(2)} MB)`
    );
    return;
  }

  stageSingleTarget(resolvedTarget, Boolean(target), debug);
}

main();
