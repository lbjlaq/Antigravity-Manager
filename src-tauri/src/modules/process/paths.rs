//! Executable path resolution for Antigravity application.
//!
//! This module provides functions to locate the Antigravity executable
//! across different platforms and installation methods.

use sysinfo::System;

use super::helpers::{get_current_exe_path, is_helper_process};

/// Get Antigravity executable path and startup arguments from running processes.
///
/// This is the most reliable method to find installations and startup args anywhere.
fn get_process_info() -> (Option<std::path::PathBuf>, Option<Vec<String>>) {
    let mut system = System::new_all();
    system.refresh_all();

    let current_exe = get_current_exe_path();
    let current_pid = std::process::id();

    for (pid, process) in system.processes() {
        let pid_u32 = pid.as_u32();
        if pid_u32 == current_pid {
            continue;
        }

        // Exclude manager process itself
        if let (Some(ref my_path), Some(p_exe)) = (&current_exe, process.exe()) {
            if let Ok(p_path) = p_exe.canonicalize() {
                if my_path == &p_path {
                    continue;
                }
            }
        }

        let name = process.name().to_string_lossy().to_lowercase();

        // Get executable path and command line arguments
        if let Some(exe) = process.exe() {
            let mut args = process.cmd().iter();
            let exe_path = args
                .next()
                .map_or(exe.to_string_lossy(), |arg| arg.to_string_lossy())
                .to_lowercase();

            // Extract actual arguments from command line (skipping exe path)
            let args: Vec<String> = args
                .map(|arg| arg.to_string_lossy().to_lowercase())
                .collect();

            let args_str = args.join(" ");

            // Extended helper detection for path resolution
            let is_helper = args_str.contains("--type=")
                || args_str.contains("node-ipc")
                || args_str.contains("nodeipc")
                || args_str.contains("max-old-space-size")
                || args_str.contains("node_modules")
                || is_helper_process(&name, &args_str, &exe_path);

            let path = Some(exe.to_path_buf());
            let args = Some(args);

            #[cfg(target_os = "macos")]
            {
                // macOS: Exclude helper processes, match main app only, and check Frameworks
                if exe_path.contains("antigravity.app")
                    && !is_helper
                    && !exe_path.contains("frameworks")
                {
                    // Try to extract .app path for better open command support
                    if let Some(app_idx) = exe_path.find(".app") {
                        let app_path_str = &exe.to_string_lossy()[..app_idx + 4];
                        let path = Some(std::path::PathBuf::from(app_path_str));
                        return (path, args);
                    }
                    return (path, args);
                }
            }

            #[cfg(target_os = "windows")]
            {
                // Windows: Strictly match process name and exclude helpers
                if name == "antigravity.exe" && !is_helper {
                    return (path, args);
                }
            }

            #[cfg(target_os = "linux")]
            {
                // Linux: Check process name or path for antigravity, excluding helpers and manager
                if (name == "antigravity" || exe_path.contains("/antigravity"))
                    && !name.contains("tools")
                    && !is_helper
                {
                    return (path, args);
                }
            }
        }
    }
    (None, None)
}

/// Get Antigravity executable path from running processes.
///
/// Most reliable method to find installation anywhere.
pub fn get_path_from_running_process() -> Option<std::path::PathBuf> {
    let (path, _) = get_process_info();
    path
}

/// Get Antigravity startup arguments from running processes.
pub fn get_args_from_running_process() -> Option<Vec<String>> {
    let (_, args) = get_process_info();
    args
}

/// Get --user-data-dir argument value (if exists).
pub fn get_user_data_dir_from_process() -> Option<std::path::PathBuf> {
    // Prefer getting startup arguments from config
    if let Ok(config) = crate::modules::config::load_app_config() {
        if let Some(args) = config.antigravity_args {
            if let Some(path) = extract_user_data_dir(&args) {
                return Some(path);
            }
        }
    }

    // If not in config, get arguments from running process
    if let Some(args) = get_args_from_running_process() {
        if let Some(path) = extract_user_data_dir(&args) {
            return Some(path);
        }
    }

    None
}

/// Extract user-data-dir from arguments list.
fn extract_user_data_dir(args: &[String]) -> Option<std::path::PathBuf> {
    for i in 0..args.len() {
        if args[i] == "--user-data-dir" && i + 1 < args.len() {
            // Next argument is the path
            let path = std::path::PathBuf::from(&args[i + 1]);
            if path.exists() {
                return Some(path);
            }
        } else if args[i].starts_with("--user-data-dir=") {
            // Argument and value in same string
            let parts: Vec<&str> = args[i].splitn(2, '=').collect();
            if parts.len() == 2 {
                let path = std::path::PathBuf::from(parts[1]);
                if path.exists() {
                    return Some(path);
                }
            }
        }
    }
    None
}

/// Get Antigravity executable path (cross-platform).
///
/// Search strategy (highest to lowest priority):
/// 1. Get path from running process (most reliable, supports any location)
/// 2. Iterate standard installation locations
/// 3. Return None
pub fn get_antigravity_executable_path() -> Option<std::path::PathBuf> {
    // Strategy 1: Get from running process (supports any location)
    if let Some(path) = get_path_from_running_process() {
        return Some(path);
    }

    // Strategy 2: Check standard installation locations
    check_standard_locations()
}

/// Check standard installation locations.
fn check_standard_locations() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let path = std::path::PathBuf::from("/Applications/Antigravity.app");
        if path.exists() {
            return Some(path);
        }
    }

    #[cfg(target_os = "windows")]
    {
        use std::env;

        // Get environment variables
        let local_appdata = env::var("LOCALAPPDATA").ok();
        let program_files =
            env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".to_string());
        let program_files_x86 =
            env::var("ProgramFiles(x86)").unwrap_or_else(|_| "C:\\Program Files (x86)".to_string());

        let mut possible_paths = Vec::new();

        // User installation location (preferred)
        if let Some(local) = local_appdata {
            possible_paths.push(
                std::path::PathBuf::from(&local)
                    .join("Programs")
                    .join("Antigravity")
                    .join("Antigravity.exe"),
            );
        }

        // System installation location
        possible_paths.push(
            std::path::PathBuf::from(&program_files)
                .join("Antigravity")
                .join("Antigravity.exe"),
        );

        // 32-bit compatibility location
        possible_paths.push(
            std::path::PathBuf::from(&program_files_x86)
                .join("Antigravity")
                .join("Antigravity.exe"),
        );

        // Return the first existing path
        for path in possible_paths {
            if path.exists() {
                return Some(path);
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        let possible_paths = vec![
            std::path::PathBuf::from("/usr/bin/antigravity"),
            std::path::PathBuf::from("/opt/Antigravity/antigravity"),
            std::path::PathBuf::from("/usr/share/antigravity/antigravity"),
        ];

        // User local installation
        if let Some(home) = dirs::home_dir() {
            let user_local = home.join(".local/bin/antigravity");
            if user_local.exists() {
                return Some(user_local);
            }
        }

        for path in possible_paths {
            if path.exists() {
                return Some(path);
            }
        }
    }

    None
}
