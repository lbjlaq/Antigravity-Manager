#![cfg(target_os = "windows")]

use std::process::Command as StdCommand;
use tokio::process::Command as TokioCommand;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

const CREATE_NO_WINDOW: u32 = 0x08000000;

pub trait CommandExtWrapper {
    /// 在 Windows 下为命令添加 CREATE_NO_WINDOW 标志，隐藏黑框
    fn creation_flags_windows(&mut self) -> &mut Self;
}

impl CommandExtWrapper for StdCommand {
    fn creation_flags_windows(&mut self) -> &mut Self {
        #[cfg(target_os = "windows")]
        self.creation_flags(CREATE_NO_WINDOW);

        self
    }
}

impl CommandExtWrapper for TokioCommand {
    fn creation_flags_windows(&mut self) -> &mut Self {
        #[cfg(target_os = "windows")]
        self.creation_flags(CREATE_NO_WINDOW);

        self
    }
}
