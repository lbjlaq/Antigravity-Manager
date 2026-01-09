# Versioning

Current version: 3.3.23

Files to update on every bump:
- `package.json` (npm package version; used by `scripts/package_dmg.sh`)
- `src-tauri/tauri.conf.json` (Tauri app bundle version)
- `src-tauri/Cargo.toml` (Rust crate/CLI version; used by `env!("CARGO_PKG_VERSION")`)
- `package-lock.json` (top-level `version` and `packages[""].version`)
- `src-tauri/Cargo.lock` (package entry `name = "antigravity_tools"`)

Release tag:
- `git tag vX.Y.Z` should match the current version above
