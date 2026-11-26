use std::io::Read;
use std::process::{Command as StdCommand, Output, Stdio};
use std::time::{Duration, Instant};
use tokio::process::Command as TokioCommand;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

// CREATE_NO_WINDOW (0x08000000) prevents console window creation on Windows
#[cfg(target_os = "windows")]
const WINDOWS_NO_WINDOW_FLAGS: u32 = 0x08000000;

pub trait CommandExtWrapper {
    /// Add creation flags on Windows to suppress console window flashing
    fn creation_flags_windows(&mut self) -> &mut Self;
}

impl CommandExtWrapper for StdCommand {
    fn creation_flags_windows(&mut self) -> &mut Self {
        #[cfg(target_os = "windows")]
        self.creation_flags(WINDOWS_NO_WINDOW_FLAGS);

        self
    }
}

impl CommandExtWrapper for TokioCommand {
    fn creation_flags_windows(&mut self) -> &mut Self {
        #[cfg(target_os = "windows")]
        self.creation_flags(WINDOWS_NO_WINDOW_FLAGS);

        self
    }
}

/// Run a synchronous `StdCommand` with null stdin, piped stdout/stderr, and a hard timeout.
/// Drains stdout and stderr on background threads so large output never deadlocks on OS pipe buffers.
pub fn output_with_timeout(cmd: &mut StdCommand, timeout: Duration) -> std::io::Result<Output> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn()?;
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();

    let stdout_handle = std::thread::spawn(move || -> Vec<u8> {
        let mut buf = Vec::new();
        if let Some(mut pipe) = stdout_pipe {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });
    let stderr_handle = std::thread::spawn(move || -> Vec<u8> {
        let mut buf = Vec::new();
        if let Some(mut pipe) = stderr_pipe {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait()? {
            Some(status) => break status,
            None => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = stdout_handle.join();
                    let _ = stderr_handle.join();
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "command timed out",
                    ));
                }
                std::thread::sleep(Duration::from_millis(15));
            }
        }
    };

    let stdout = stdout_handle.join().unwrap_or_default();
    let stderr = stderr_handle.join().unwrap_or_default();
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_output_with_timeout_completes_and_kills_hung_process() {
        let mut fast_cmd = if cfg!(target_os = "windows") {
            let mut c = StdCommand::new("cmd.exe");
            c.args(["/C", "echo ok"]);
            c.creation_flags_windows();
            c
        } else {
            let mut c = StdCommand::new("sh");
            c.args(["-c", "echo ok"]);
            c
        };

        let out = output_with_timeout(&mut fast_cmd, Duration::from_secs(5))
            .expect("fast command should succeed");
        assert!(out.status.success());
        assert!(String::from_utf8_lossy(&out.stdout).contains("ok"));

        let mut slow_cmd = if cfg!(target_os = "windows") {
            let mut c = StdCommand::new("ping");
            c.args(["-n", "10", "127.0.0.1"]);
            c.creation_flags_windows();
            c
        } else {
            let mut c = StdCommand::new("sleep");
            c.arg("10");
            c
        };

        let start = Instant::now();
        let err = output_with_timeout(&mut slow_cmd, Duration::from_millis(150))
            .expect_err("slow command should time out");
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
        assert!(start.elapsed() < Duration::from_secs(3));
    }
}
