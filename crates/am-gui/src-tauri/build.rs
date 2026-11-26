use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn files_identical(a: &Path, b: &Path) -> bool {
    let Ok(meta_a) = fs::metadata(a) else {
        return false;
    };
    let Ok(meta_b) = fs::metadata(b) else {
        return false;
    };
    if meta_a.len() != meta_b.len() {
        return false;
    }
    match (fs::read(a), fs::read(b)) {
        (Ok(ba), Ok(bb)) => ba == bb,
        _ => false,
    }
}

fn ensure_am_sidecar() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let target = env::var("TARGET").unwrap_or_default();
    let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap_or_default());

    if target.is_empty() {
        return;
    }

    let is_windows = target.contains("windows");
    let exe_ext = if is_windows { ".exe" } else { "" };
    let bin_name = format!("am{}", exe_ext);
    let binaries_dir = manifest_dir.join("binaries");
    let dest = binaries_dir.join(format!("am-{}{}", target, exe_ext));

    let _ = fs::create_dir_all(&binaries_dir);

    let target_profile_dir = out_dir
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .map(Path::to_path_buf);

    let workspace_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .map(Path::to_path_buf);

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(ref dir) = target_profile_dir {
        candidates.push(dir.join(&bin_name));
    }
    if let Some(ref root) = workspace_root {
        candidates.push(
            root.join("target")
                .join(&target)
                .join(&profile)
                .join(&bin_name),
        );
        if profile != "release" {
            candidates.push(
                root.join("target")
                    .join(&target)
                    .join("release")
                    .join(&bin_name),
            );
        }
        let host = env::var("HOST").unwrap_or_default();
        if host == target {
            candidates.push(root.join("target").join(&profile).join(&bin_name));
            if profile != "release" {
                candidates.push(root.join("target").join("release").join(&bin_name));
            }
        }
    }

    let valid_candidate = candidates
        .into_iter()
        .find(|p| p.is_file() && fs::metadata(p).map(|m| m.len() > 0).unwrap_or(false));

    let dest_len = fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
    let should_copy = if let Some(ref src) = valid_candidate {
        if dest_len == 0 {
            true
        } else {
            let src_mtime = fs::metadata(src).and_then(|m| m.modified()).ok();
            let dest_mtime = fs::metadata(&dest).and_then(|m| m.modified()).ok();
            matches!((src_mtime, dest_mtime), (Some(sm), Some(dm)) if sm > dm)
                && !files_identical(src, &dest)
        }
    } else {
        false
    };

    if should_copy {
        if let Some(ref src) = valid_candidate {
            if let Err(e) = fs::copy(src, &dest) {
                panic!(
                    "Failed to copy am sidecar from {} to {}: {}",
                    src.display(),
                    dest.display(),
                    e
                );
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(mut perms) = fs::metadata(&dest).map(|m| m.permissions()) {
                    perms.set_mode(0o755);
                    let _ = fs::set_permissions(&dest, perms);
                }
            }
        }
    }

    let final_len = fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
    if final_len == 0 {
        if profile == "release" {
            panic!(
                "Missing release sidecar binary at {}. Run `node scripts/prepare-sidecar.mjs --target {}` (or `cargo build --release -p am-cli`) before building am-gui in release mode.",
                dest.display(),
                target
            );
        } else {
            if !dest.exists() {
                let _ = fs::write(&dest, b"");
            }
            println!(
                "cargo:warning=Sidecar {} not found; using debug placeholder. Run `node scripts/prepare-sidecar.mjs` or `cargo build --release -p am-cli` for a working bundled CLI.",
                dest.display()
            );
        }
    }
}

fn main() {
    ensure_am_sidecar();
    tauri_build::build()
}
