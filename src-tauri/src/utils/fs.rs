use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Platform-specific atomic file replacement
#[cfg(target_os = "windows")]
fn atomic_replace_file(src: &Path, dst: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;

    type Bool = i32;
    type Dword = u32;

    #[link(name = "Kernel32")]
    extern "system" {
        fn MoveFileExW(
            lp_existing_file_name: *const u16,
            lp_new_file_name: *const u16,
            dw_flags: Dword,
        ) -> Bool;
    }

    let src_wide: Vec<u16> = src
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let dst_wide: Vec<u16> = dst
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // MOVEFILE_REPLACE_EXISTING = 0x1
    // MOVEFILE_WRITE_THROUGH = 0x8
    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
    let flags = MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH;

    // Retry loop for Windows transient file locks (e.g., antivirus scanning, indexers, or rapid concurrent read/write handles).
    // Error code 5 = ERROR_ACCESS_DENIED, code 32 = ERROR_SHARING_VIOLATION.
    const MAX_RETRIES: usize = 5;
    let mut last_err = None;

    for attempt in 0..=MAX_RETRIES {
        let result = unsafe { MoveFileExW(src_wide.as_ptr(), dst_wide.as_ptr(), flags) };
        if result != 0 {
            return Ok(());
        }

        let err = std::io::Error::last_os_error();
        let code = err.raw_os_error().unwrap_or(0);

        if (code == 5 || code == 32) && attempt < MAX_RETRIES {
            std::thread::sleep(std::time::Duration::from_millis(5 * (1 << attempt)));
            last_err = Some(err);
            continue;
        }

        let _ = std::fs::remove_file(src);
        return Err(format!(
            "MoveFileExW failed after {} attempts: {}",
            attempt + 1,
            err
        ));
    }

    let _ = std::fs::remove_file(src);
    Err(format!(
        "MoveFileExW failed: {}",
        last_err
            .map(|e| e.to_string())
            .unwrap_or_else(|| "unknown error".into())
    ))
}

/// Non-Windows: use standard atomic rename
#[cfg(not(target_os = "windows"))]
fn atomic_replace_file(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::rename(src, dst).map_err(|e| format!("rename failed: {}", e))
}

/// Atomically replace `dst` with `src`, using `MoveFileExW` retries on Windows.
pub fn replace_file_atomic<P: AsRef<Path>, Q: AsRef<Path>>(src: P, dst: Q) -> Result<(), String> {
    atomic_replace_file(src.as_ref(), dst.as_ref())
}

fn write_atomic_inner(
    target: &Path,
    content: &[u8],
    #[cfg_attr(not(unix), allow(unused_variables))] unix_mode: Option<u32>,
) -> Result<(), String> {
    let parent_dir = target
        .parent()
        .ok_or_else(|| "Target path has no parent directory".to_string())?;

    if !parent_dir.as_os_str().is_empty() {
        std::fs::create_dir_all(parent_dir)
            .map_err(|e| format!("Failed to create directory {:?}: {}", parent_dir, e))?;
    }

    let file_name = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");
    let temp_filename = format!("{}.tmp.{}", file_name, Uuid::new_v4());
    let temp_path: PathBuf = parent_dir.join(temp_filename);

    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if let Some(mode) = unix_mode {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }

    let mut file = options
        .open(&temp_path)
        .map_err(|e| format!("Failed to create temporary file {:?}: {}", temp_path, e))?;

    let write_res = (|| -> Result<(), String> {
        #[cfg(unix)]
        if let Some(mode) = unix_mode {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(mode))
                .map_err(|e| {
                    format!(
                        "Failed to set permissions on temporary file {:?}: {}",
                        temp_path, e
                    )
                })?;
        }
        file.write_all(content)
            .map_err(|e| format!("Failed to write to temporary file {:?}: {}", temp_path, e))?;
        file.sync_all()
            .map_err(|e| format!("Failed to fsync temporary file {:?}: {}", temp_path, e))
    })();

    // Explicitly drop file handle before rename to release Windows lock
    drop(file);

    if let Err(e) = write_res {
        let _ = std::fs::remove_file(&temp_path);
        return Err(e);
    }

    if let Err(e) = atomic_replace_file(&temp_path, target) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(format!("Failed to atomically replace {:?}: {}", target, e));
    }

    Ok(())
}

/// Atomically write bytes to a file at `target_path`.
///
/// Steps:
/// 1. Create a unique temporary file in the same directory (`<file>.tmp.<uuid>`) with private 0600 permissions on Unix.
/// 2. Write content and call `sync_all()` to flush buffers to physical disk.
/// 3. Replace target file via atomic rename (`MoveFileExW` with `MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH` on Windows, `fs::rename` on POSIX).
/// 4. If any step fails, remove the temporary file.
pub fn write_atomic<P: AsRef<Path>>(target_path: P, content: &[u8]) -> Result<(), String> {
    write_atomic_inner(target_path.as_ref(), content, Some(0o600))
}

/// Atomically write bytes to `target_path`, enforcing `unix_mode` (e.g. `0o600` or `0o700`) on Unix
/// at file creation time before sensitive contents are written.
pub fn write_atomic_restricted<P: AsRef<Path>>(
    target_path: P,
    content: &[u8],
    unix_mode: u32,
) -> Result<(), String> {
    write_atomic_inner(target_path.as_ref(), content, Some(unix_mode))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_write_atomic_basic() {
        let temp_dir = std::env::temp_dir().join(format!("test_atomic_{}", Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();

        let target_file = temp_dir.join("config.json");
        let initial_data = b"{\"key\":\"initial_value\"}";
        write_atomic(&target_file, initial_data).expect("First write should succeed");

        assert_eq!(fs::read(&target_file).unwrap(), initial_data);

        let updated_data = b"{\"key\":\"updated_value\"}";
        write_atomic(&target_file, updated_data).expect("Overwrite should succeed");

        assert_eq!(fs::read(&target_file).unwrap(), updated_data);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_write_atomic_survives_partial_tmp() {
        let temp_dir = std::env::temp_dir().join(format!("test_atomic_partial_{}", Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();

        let target_file = temp_dir.join("accounts.json");
        let good_data = b"{\"valid\":true}";
        write_atomic(&target_file, good_data).expect("Initial write should succeed");

        // Simulate leftover half-written temp file from a simulated crash / power loss
        let leftover_tmp = temp_dir.join("accounts.json.tmp.aborted");
        fs::write(&leftover_tmp, b"{\"valid\":false, truncated...").unwrap();

        // Target file should remain intact and valid
        assert_eq!(fs::read(&target_file).unwrap(), good_data);

        // Next write should succeed cleanly
        let next_data = b"{\"valid\":true,\"generation\":2}";
        write_atomic(&target_file, next_data).expect("Subsequent write should succeed");
        assert_eq!(fs::read(&target_file).unwrap(), next_data);

        let _ = fs::remove_dir_all(&temp_dir);
    }

<<<<<<< HEAD
    #[test]
    fn test_write_atomic_restricted_creates_parent_and_replaces_content() {
        let temp_dir =
            std::env::temp_dir().join(format!("test_atomic_restricted_{}", Uuid::new_v4()));
        let nested_target = temp_dir.join("nested").join("secret.json");

        fs::create_dir_all(nested_target.parent().unwrap()).unwrap();
        fs::write(&nested_target, b"old-world-readable").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&nested_target, fs::Permissions::from_mode(0o644)).unwrap();
        }

        write_atomic_restricted(&nested_target, b"{\"apiKey\":\"sk-secret\"}", 0o600)
            .expect("restricted write should succeed");
        assert_eq!(
            fs::read(&nested_target).unwrap(),
            b"{\"apiKey\":\"sk-secret\"}"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&nested_target).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }

        let exec_target = temp_dir.join("nested").join("wrapper.sh");
        write_atomic_restricted(&exec_target, b"#!/bin/zsh\necho hi\n", 0o700)
            .expect("restricted executable write should succeed");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&exec_target).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700);
        }

        let entries: Vec<_> = fs::read_dir(nested_target.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(!entries.iter().any(|name| name.contains(".tmp.")));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_replace_file_atomic_overwrites_destination() {
        let temp_dir = std::env::temp_dir().join(format!("test_replace_atomic_{}", Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();

        let src = temp_dir.join("settings.json.antigravity.bak");
        let dst = temp_dir.join("settings.json");
        fs::write(&src, b"backup-data").unwrap();
        fs::write(&dst, b"modified-data").unwrap();

        replace_file_atomic(&src, &dst).expect("atomic replace should succeed");
        assert!(!src.exists());
        assert_eq!(fs::read(&dst).unwrap(), b"backup-data");

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[cfg(unix)]
    #[test]
    fn test_write_atomic_sets_0600_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = std::env::temp_dir().join(format!("test_atomic_perms_{}", Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();

        let target_file = temp_dir.join("secret.json");
        fs::write(&target_file, b"{}").unwrap();
        fs::set_permissions(&target_file, fs::Permissions::from_mode(0o644)).unwrap();

        write_atomic(&target_file, b"{\"refresh_token\":\"secret\"}").unwrap();
        let mode = fs::metadata(&target_file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
