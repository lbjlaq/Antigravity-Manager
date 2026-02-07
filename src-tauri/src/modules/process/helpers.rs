//! Helper process identification utilities.
//!
//! This module provides functions to identify helper/subprocess patterns
//! and manage process family trees.

#[cfg(target_os = "linux")]
use std::collections::{HashMap, HashSet, VecDeque};

#[cfg(target_os = "linux")]
use sysinfo::System;

/// Keywords indicating a helper/subprocess (not the main Antigravity process).
pub const HELPER_KEYWORDS: &[&str] = &[
    "helper",
    "plugin",
    "renderer",
    "gpu",
    "crashpad",
    "utility",
    "audio",
    "sandbox",
];

/// Extended helper keywords for macOS (includes language_server).
pub const HELPER_KEYWORDS_MACOS: &[&str] = &[
    "helper",
    "plugin",
    "renderer",
    "gpu",
    "crashpad",
    "utility",
    "audio",
    "sandbox",
    "language_server",
];

/// Check if process name indicates a helper process.
pub fn is_helper_by_name(name: &str) -> bool {
    let name_lower = name.to_lowercase();
    HELPER_KEYWORDS
        .iter()
        .any(|keyword| name_lower.contains(keyword))
}

/// Check if process name indicates a helper process (macOS extended check).
#[cfg(target_os = "macos")]
pub fn is_helper_by_name_macos(name: &str) -> bool {
    #![allow(dead_code)]
    let name_lower = name.to_lowercase();
    HELPER_KEYWORDS_MACOS
        .iter()
        .any(|keyword| name_lower.contains(keyword))
}

/// Check if command line arguments indicate a helper process.
pub fn is_helper_by_args(args_str: &str) -> bool {
    args_str.contains("--type=")
}

/// Combined helper detection using both name and args.
pub fn is_helper_process(name: &str, args_str: &str, exe_path: &str) -> bool {
    is_helper_by_args(args_str)
        || is_helper_by_name(name)
        || exe_path.contains("crashpad")
}

/// Get normalized path of the current running executable.
pub fn get_current_exe_path() -> Option<std::path::PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.canonicalize().ok())
}

/// Get PID set of current process and all direct relatives (ancestors + descendants).
///
/// This is used on Linux to avoid killing the manager's own process tree.
#[cfg(target_os = "linux")]
pub fn get_self_family_pids(system: &System) -> HashSet<u32> {
    let current_pid = std::process::id();
    let mut family_pids = HashSet::new();
    family_pids.insert(current_pid);

    // 1. Look up all ancestors - prevent killing the launcher
    let mut next_pid = current_pid;
    // Prevent infinite loop, max depth 10
    for _ in 0..10 {
        let pid_val = sysinfo::Pid::from_u32(next_pid);
        if let Some(process) = system.process(pid_val) {
            if let Some(parent) = process.parent() {
                let parent_id = parent.as_u32();
                // Avoid cycles or duplicates
                if !family_pids.insert(parent_id) {
                    break;
                }
                next_pid = parent_id;
            } else {
                break;
            }
        } else {
            break;
        }
    }

    // 2. Look down all descendants
    // Build parent-child relationship map
    let mut adj: HashMap<u32, Vec<u32>> = HashMap::new();
    for (pid, process) in system.processes() {
        if let Some(parent) = process.parent() {
            adj.entry(parent.as_u32()).or_default().push(pid.as_u32());
        }
    }

    // BFS traversal to find all descendants
    let mut queue = VecDeque::new();
    queue.push_back(current_pid);

    while let Some(pid) = queue.pop_front() {
        if let Some(children) = adj.get(&pid) {
            for &child in children {
                if family_pids.insert(child) {
                    queue.push_back(child);
                }
            }
        }
    }

    family_pids
}

/// Load manual configuration path for Antigravity executable.
pub fn load_manual_path() -> Option<std::path::PathBuf> {
    crate::modules::config::load_app_config()
        .ok()
        .and_then(|c| c.antigravity_executable)
        .and_then(|p| std::path::PathBuf::from(p).canonicalize().ok())
}
