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

    /// 同步安全条款/协议确认状态与清理旧会话历史标识，消除切号后弹窗与历史缺失 (Fixes #4026)
    pub fn apply_onboarding_to_db(conn: &Connection, email: &str, is_gcp_tos: bool) -> Result<()> {
        let tos_str = if is_gcp_tos { "true" } else { "false" };

        let _ = conn.execute(
            "INSERT OR REPLACE INTO ItemTable (key, value) VALUES ('agy.onboarding.lastLoginUsername', ?1)",
            [email],
        );
        let _ = conn.execute(
            "INSERT OR REPLACE INTO ItemTable (key, value) VALUES ('agy.onboarding.lastLoginIsGcpTos', ?1)",
            [tos_str],
        );
        let _ = conn.execute(
            "INSERT OR REPLACE INTO ItemTable (key, value) VALUES ('jetski.onboarding.lastLoginUsername', ?1)",
            [email],
        );
        let _ = conn.execute(
            "INSERT OR REPLACE INTO ItemTable (key, value) VALUES ('jetski.onboarding.lastLoginIsGcpTos', ?1)",
            [tos_str],
        );

        // 删除旧格式状态，避免 IDE 读取陈旧 UserID 导致历史记录拉取失败
        let _ = conn.execute(
            "DELETE FROM ItemTable WHERE key = 'jetskiStateSync.agentManagerInitState'",
            [],
        );

        Ok(())
    }
}
