//! Process lifecycle management for Antigravity application.
//!
//! This module provides functions to start and stop the Antigravity application
//! across different platforms with graceful shutdown support.

use std::process::Command;
use std::thread;
use std::time::Duration;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

use super::detection::{get_antigravity_pids, is_antigravity_running};
use super::paths::get_antigravity_executable_path;

#[cfg(target_os = "macos")]
use super::helpers::is_helper_by_name_macos;

#[cfg(any(target_os = "macos", target_os = "linux"))]
use super::helpers::load_manual_path;

#[cfg(any(target_os = "macos", target_os = "linux"))]
use sysinfo::System;

/// Close Antigravity processes.
pub fn close_antigravity(#[allow(unused_variables)] timeout_secs: u64) -> Result<(), String> {
    crate::modules::logger::log_info("Closing Antigravity...");

    #[cfg(target_os = "windows")]
    {
        close_antigravity_windows()?;
    }

    #[cfg(target_os = "macos")]
    {
        close_antigravity_macos(timeout_secs)?;
    }

    #[cfg(target_os = "linux")]
    {
        close_antigravity_linux(timeout_secs)?;
    }

    // Final check
    if is_antigravity_running() {
        return Err(
            "Unable to close Antigravity process, please close manually and retry".to_string(),
        );
    }

    crate::modules::logger::log_info("Antigravity closed successfully");
    Ok(())
}

#[cfg(target_os = "windows")]
fn close_antigravity_windows() -> Result<(), String> {
    let pids = get_antigravity_pids();
    if !pids.is_empty() {
        crate::modules::logger::log_info(&format!(
            "Precisely closing {} identified processes on Windows...",
            pids.len()
        ));
        for pid in pids {
            let _ = Command::new("taskkill")
                .args(["/F", "/PID", &pid.to_string()])
                .creation_flags(0x08000000) // CREATE_NO_WINDOW
                .output();
        }
        // Give some time for system to clean up PIDs
        thread::sleep(Duration::from_millis(200));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn close_antigravity_macos(timeout_secs: u64) -> Result<(), String> {
    let pids = get_antigravity_pids();
    if pids.is_empty() {
        crate::modules::logger::log_info("Antigravity not running, no need to close");
        return Ok(());
    }

    // Identify main process
    let mut system = System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All);

    let mut main_pid = None;
    let manual_path = load_manual_path();

    crate::modules::logger::log_info("Analyzing process list to identify main process:");
    for pid_u32 in &pids {
        let pid = sysinfo::Pid::from_u32(*pid_u32);
        if let Some(process) = system.process(pid) {
            let name = process.name().to_string_lossy();
            let args = process.cmd();
            let args_str = args
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<String>>()
                .join(" ");

            crate::modules::logger::log_info(&format!(
                " - PID: {} | Name: {} | Args: {}",
                pid_u32, name, args_str
            ));

            // Priority to manual path matching
            if let (Some(ref m_path), Some(p_exe)) = (&manual_path, process.exe()) {
                if let Ok(p_path) = p_exe.canonicalize() {
                    let m_path_str = m_path.to_string_lossy();
                    let p_path_str = p_path.to_string_lossy();
                    if let (Some(m_idx), Some(p_idx)) =
                        (m_path_str.find(".app"), p_path_str.find(".app"))
                    {
                        if m_path_str[..m_idx + 4] == p_path_str[..p_idx + 4] {
                            let is_helper_by_args = args_str.contains("--type=");
                            let is_helper_by_name = is_helper_by_name_macos(&name);

                            if !is_helper_by_args && !is_helper_by_name {
                                main_pid = Some(pid_u32);
                                crate::modules::logger::log_info(
                                    "   => Identified as main process (manual path match)",
                                );
                                break;
                            }
                        }
                    }
                }
            }

            // Feature analysis matching (fallback)
            let is_helper_by_name = is_helper_by_name_macos(&name);
            let is_helper_by_args = args_str.contains("--type=");

            if !is_helper_by_name && !is_helper_by_args {
                if main_pid.is_none() {
                    main_pid = Some(pid_u32);
                    crate::modules::logger::log_info(
                        "   => Identified as main process (Name/Args analysis)",
                    );
                }
            } else {
                crate::modules::logger::log_info(
                    "   => Identified as helper process (Helper/Args)",
                );
            }
        }
    }

    // Phase 1: Graceful exit (SIGTERM)
    if let Some(pid) = main_pid {
        crate::modules::logger::log_info(&format!(
            "Sending SIGTERM to main process PID: {}",
            pid
        ));
        let output = Command::new("kill")
            .args(["-15", &pid.to_string()])
            .output();

        if let Ok(result) = output {
            if !result.status.success() {
                let error = String::from_utf8_lossy(&result.stderr);
                crate::modules::logger::log_warn(&format!(
                    "Main process SIGTERM failed: {}",
                    error
                ));
            }
        }
    } else {
        crate::modules::logger::log_warn(
            "No clear main process identified, attempting SIGTERM for all processes",
        );
        for pid in &pids {
            let _ = Command::new("kill")
                .args(["-15", &pid.to_string()])
                .output();
        }
    }

    // Wait for graceful exit
    let graceful_timeout = (timeout_secs * 7) / 10;
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(graceful_timeout) {
        if !is_antigravity_running() {
            crate::modules::logger::log_info("All Antigravity processes gracefully closed");
            return Ok(());
        }
        thread::sleep(Duration::from_millis(500));
    }

    // Phase 2: Force kill (SIGKILL)
    if is_antigravity_running() {
        let remaining_pids = get_antigravity_pids();
        if !remaining_pids.is_empty() {
            crate::modules::logger::log_warn(&format!(
                "Graceful exit timeout, force killing {} remaining processes (SIGKILL)",
                remaining_pids.len()
            ));
            for pid in &remaining_pids {
                let output = Command::new("kill")
                    .args(["-9", &pid.to_string()])
                    .output();

                if let Ok(result) = output {
                    if !result.status.success() {
                        let error = String::from_utf8_lossy(&result.stderr);
                        if !error.contains("No such process") {
                            crate::modules::logger::log_error(&format!(
                                "SIGKILL process {} failed: {}",
                                pid, error
                            ));
                        }
                    }
                }
            }
            thread::sleep(Duration::from_secs(1));
        }

        if !is_antigravity_running() {
            crate::modules::logger::log_info("All processes exited after forced cleanup");
            return Ok(());
        }
    } else {
        crate::modules::logger::log_info("All processes exited after SIGTERM");
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn close_antigravity_linux(timeout_secs: u64) -> Result<(), String> {
    use super::helpers::is_helper_by_name;

    let pids = get_antigravity_pids();
    if pids.is_empty() {
        crate::modules::logger::log_info(
            "No Antigravity processes found to close (possibly filtered or not running)",
        );
        return Ok(());
    }

    let mut system = System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All);

    let mut main_pid = None;
    let manual_path = load_manual_path();

    crate::modules::logger::log_info("Analyzing Linux process list to identify main process:");
    for pid_u32 in &pids {
        let pid = sysinfo::Pid::from_u32(*pid_u32);
        if let Some(process) = system.process(pid) {
            let name = process.name().to_string_lossy().to_lowercase();
            let args = process.cmd();
            let args_str = args
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<String>>()
                .join(" ");

            crate::modules::logger::log_info(&format!(
                " - PID: {} | Name: {} | Args: {}",
                pid_u32, name, args_str
            ));

            // Priority to manual path matching
            if let (Some(ref m_path), Some(p_exe)) = (&manual_path, process.exe()) {
                if let Ok(p_path) = p_exe.canonicalize() {
                    if &p_path == m_path {
                        let is_helper_by_args = args_str.contains("--type=");
                        let is_helper_by_name = is_helper_by_name(&name);
                        if !is_helper_by_args && !is_helper_by_name {
                            main_pid = Some(pid_u32);
                            crate::modules::logger::log_info(
                                "   => Identified as main process (manual path match)",
                            );
                            break;
                        }
                    }
                }
            }

            // Feature analysis matching
            let is_helper_by_args = args_str.contains("--type=");
            let is_helper_by_name = is_helper_by_name(&name)
                || name.contains("plugin")
                || name.contains("language_server");

            if !is_helper_by_args && !is_helper_by_name {
                if main_pid.is_none() {
                    main_pid = Some(pid_u32);
                    crate::modules::logger::log_info(
                        "   => Identified as main process (Feature analysis)",
                    );
                }
            } else {
                crate::modules::logger::log_info(
                    "   => Identified as helper process (Helper/Args)",
                );
            }
        }
    }

    // Phase 1: Graceful exit (SIGTERM)
    if let Some(pid) = main_pid {
        crate::modules::logger::log_info(&format!(
            "Attempting to gracefully close main process {} (SIGTERM)",
            pid
        ));
        let _ = Command::new("kill")
            .args(["-15", &pid.to_string()])
            .output();
    } else {
        crate::modules::logger::log_warn(
            "No clear Linux main process identified, sending SIGTERM to all associated processes",
        );
        for pid in &pids {
            let _ = Command::new("kill")
                .args(["-15", &pid.to_string()])
                .output();
        }
    }

    // Wait for graceful exit
    let graceful_timeout = (timeout_secs * 7) / 10;
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(graceful_timeout) {
        if !is_antigravity_running() {
            crate::modules::logger::log_info("Antigravity gracefully closed");
            return Ok(());
        }
        thread::sleep(Duration::from_millis(500));
    }

    // Phase 2: Force kill (SIGKILL)
    if is_antigravity_running() {
        let remaining_pids = get_antigravity_pids();
        if !remaining_pids.is_empty() {
            crate::modules::logger::log_warn(&format!(
                "Graceful exit timeout, force killing {} remaining processes (SIGKILL)",
                remaining_pids.len()
            ));
            for pid in &remaining_pids {
                let _ = Command::new("kill")
                    .args(["-9", &pid.to_string()])
                    .output();
            }
            thread::sleep(Duration::from_secs(1));
        }
    }

    Ok(())
}

/// Start Antigravity.
#[allow(unused_mut)]
pub fn start_antigravity() -> Result<(), String> {
    crate::modules::logger::log_info("Starting Antigravity...");

    // Prefer manually specified path and args from configuration
    let config = crate::modules::config::load_app_config().ok();
    let manual_path = config
        .as_ref()
        .and_then(|c| c.antigravity_executable.clone());
    let args = config.and_then(|c| c.antigravity_args.clone());

    if let Some(mut path_str) = manual_path {
        let mut path = std::path::PathBuf::from(&path_str);

        #[cfg(target_os = "macos")]
        {
            // Fault tolerance: auto-correct to .app directory if inside bundle
            if let Some(app_idx) = path_str.find(".app") {
                let corrected_app = &path_str[..app_idx + 4];
                if corrected_app != path_str {
                    crate::modules::logger::log_info(&format!(
                        "Detected macOS path inside .app bundle, auto-correcting to: {}",
                        corrected_app
                    ));
                    path_str = corrected_app.to_string();
                    path = std::path::PathBuf::from(&path_str);
                }
            }
        }

        if path.exists() {
            crate::modules::logger::log_info(&format!(
                "Starting with manual configuration path: {}",
                path_str
            ));
            return start_with_path(&path_str, &args);
        } else {
            crate::modules::logger::log_warn(&format!(
                "Manual configuration path does not exist: {}, falling back to auto-detection",
                path_str
            ));
        }
    }

    // Fall back to platform-specific default startup
    start_default(&args)
}

fn start_with_path(path_str: &str, args: &Option<Vec<String>>) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let path = std::path::PathBuf::from(path_str);
        if path_str.ends_with(".app") || path.is_dir() {
            let mut cmd = Command::new("open");
            cmd.arg("-a").arg(path_str);

            if let Some(ref args) = args {
                for arg in args {
                    cmd.arg(arg);
                }
            }

            cmd.spawn()
                .map_err(|e| format!("Startup failed (open): {}", e))?;
        } else {
            let mut cmd = Command::new(path_str);

            if let Some(ref args) = args {
                for arg in args {
                    cmd.arg(arg);
                }
            }

            cmd.spawn()
                .map_err(|e| format!("Startup failed (direct): {}", e))?;
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let mut cmd = Command::new(path_str);

        if let Some(ref args) = args {
            for arg in args {
                cmd.arg(arg);
            }
        }

        cmd.spawn()
            .map_err(|e| format!("Startup failed: {}", e))?;
    }

    crate::modules::logger::log_info(&format!(
        "Antigravity startup command sent (manual path: {}, args: {:?})",
        path_str, args
    ));
    Ok(())
}

fn start_default(args: &Option<Vec<String>>) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let mut cmd = Command::new("open");
        cmd.args(["-a", "Antigravity"]);

        if let Some(ref args) = args {
            for arg in args {
                cmd.arg(arg);
            }
        }

        let output = cmd
            .output()
            .map_err(|e| format!("Unable to execute open command: {}", e))?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "Startup failed (open exited with {}): {}",
                output.status, error
            ));
        }
    }

    #[cfg(target_os = "windows")]
    {
        let has_args = args.as_ref().map_or(false, |a| !a.is_empty());

        if has_args {
            if let Some(detected_path) = get_antigravity_executable_path() {
                let path_str = detected_path.to_string_lossy().to_string();
                crate::modules::logger::log_info(&format!(
                    "Starting with auto-detected path (has args): {}",
                    path_str
                ));

                let mut cmd = Command::new(&path_str);
                if let Some(ref args) = args {
                    for arg in args {
                        cmd.arg(arg);
                    }
                }

                cmd.spawn()
                    .map_err(|e| format!("Startup failed: {}", e))?;
            } else {
                return Err("Startup arguments configured but cannot find Antigravity executable path. Please set the executable path manually in Settings.".to_string());
            }
        } else {
            let mut cmd = Command::new("cmd");
            cmd.args(["/C", "start", "antigravity://"]);

            let result = cmd.spawn();
            if result.is_err() {
                return Err("Startup failed, please open Antigravity manually".to_string());
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        let mut cmd = Command::new("antigravity");

        if let Some(ref args) = args {
            for arg in args {
                cmd.arg(arg);
            }
        }

        cmd.spawn()
            .map_err(|e| format!("Startup failed: {}", e))?;
    }

    crate::modules::logger::log_info(&format!(
        "Antigravity startup command sent (default detection, args: {:?})",
        args
    ));
    Ok(())
}
