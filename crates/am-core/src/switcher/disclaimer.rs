use anyhow::Result;
use rusqlite::Connection;

/// 官方客户端免干扰与二次确认静默配置列表
/// 针对 Antigravity (原生桌面) 与 Antigravity IDE (VSCode定制版)
pub struct DisclaimerBypass;

impl DisclaimerBypass {
    /// 一键将免确认与静默配置注入到指定 state.vscdb
    pub fn apply_to_db(conn: &Connection) -> Result<()> {
        // 1. 跳过新手引导与开屏条款声明
        conn.execute(
            "INSERT OR REPLACE INTO ItemTable (key, value) VALUES ('antigravityOnboarding', 'true')",
            [],
        )?;

        // 2. 授权权限询问标记
        conn.execute(
            "INSERT OR REPLACE INTO ItemTable (key, value) VALUES ('antigravity.notification.didAskUserForPermission', 'true')",
            [],
        )?;

        // 3. 终端配置提示弹窗关闭
        conn.execute(
            "INSERT OR REPLACE INTO ItemTable (key, value) VALUES ('terminal.integrated.showTerminalConfigPrompt', 'false')",
            [],
        )?;

        // 4. 更新说明提示自动静默
        conn.execute(
            "INSERT OR REPLACE INTO ItemTable (key, value) VALUES ('antigravityChangelog/lastVersion', '9.9.9')",
            [],
        )?;

        Ok(())
    }
}
