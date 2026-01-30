import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const rootDir = path.resolve(__dirname, '..');

const version = process.argv[2];

if (!version) {
    console.error('Usage: node set-version.js <version>');
    process.exit(1);
}

if (!/^\d+\.\d+\.\d+/.test(version)) {
    console.error('Error: Version must be in format x.y.z');
    process.exit(1);
}

console.log(`🚀 Updating project version to ${version}...`);

// 1. package.json
const packageJsonPath = path.join(rootDir, 'package.json');
try {
    const pkg = JSON.parse(fs.readFileSync(packageJsonPath, 'utf-8'));
    const oldVer = pkg.version;
    pkg.version = version;
    fs.writeFileSync(packageJsonPath, JSON.stringify(pkg, null, 2) + '\n');
    console.log(`✅ Updated package.json (${oldVer} -> ${version})`);
} catch (e) {
    console.error('❌ Failed to update package.json:', e);
}

// 2. src-tauri/tauri.conf.json
const tauriConfPath = path.join(rootDir, 'src-tauri', 'tauri.conf.json');
try {
    const conf = JSON.parse(fs.readFileSync(tauriConfPath, 'utf-8'));
    const oldVer = conf.version;
    conf.version = version;
    fs.writeFileSync(tauriConfPath, JSON.stringify(conf, null, 2));
    console.log(`✅ Updated tauri.conf.json (${oldVer} -> ${version})`);
} catch (e) {
    console.error('❌ Failed to update tauri.conf.json:', e);
}

// 3. src-tauri/Cargo.toml (logic preserved...)
const cargoTomlPath = path.join(rootDir, 'src-tauri', 'Cargo.toml');
try {
    let cargo = fs.readFileSync(cargoTomlPath, 'utf-8');
    if (cargo.includes(`version = "`)) {
        const newCargo = cargo.replace(/version = "[^"]+"/, `version = "${version}"`);
        if (newCargo !== cargo) {
             fs.writeFileSync(cargoTomlPath, newCargo);
             console.log(`✅ Updated Cargo.toml version`);
        } else {
             console.log(`⚠️ Cargo.toml version unchanged (regex mismatch?)`);
        }
    }
} catch (e) {
     console.error('❌ Failed to update Cargo.toml:', e);
}

// 4. src/pages/Settings.tsx (UI Version)
const settingsPath = path.join(rootDir, 'src', 'pages', 'Settings.tsx');
try {
    let settings = fs.readFileSync(settingsPath, 'utf-8');
    // Regex to match <span ...>vX.Y.Z</span>
    // We look for >v\d+\.\d+\.\d+< to be safe
    const versionRegex = />v\d+\.\d+\.\d+</;
    if (versionRegex.test(settings)) {
        const newSettings = settings.replace(versionRegex, `>v${version}<`);
        fs.writeFileSync(settingsPath, newSettings);
        console.log(`✅ Updated Settings.tsx UI version to v${version}`);
    } else {
        console.log(`⚠️ Settings.tsx version string not found (expected format >vX.Y.Z<)`);
    }
} catch (e) {
    console.error('❌ Failed to update Settings.tsx:', e);
}

console.log(`\n✨ Version bump complete!`);
