use anyhow::Result;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};

/// 宿主会话与步骤统计项
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HostUsageSummary {
    /// 宿主名称
    pub host_name: String,
    /// 标识符号
    pub host_id: String,
    /// 会话数量
    pub conversation_count: usize,
    /// 总步骤数 (Step Count)
    pub total_steps: u64,
    /// 估算的 Token 消耗量 (~按官方平均 step 经验值)
    pub estimated_tokens: u64,
    /// 最新会话时间
    pub latest_activity: Option<String>,
}

/// 聚合多宿主本地统计大盘
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LocalUsageStats {
    /// 各宿主详情
    pub hosts: Vec<HostUsageSummary>,
    /// 总会话数
    pub total_conversations: usize,
    /// 总步骤数
    pub total_steps: u64,
    /// 总估算 Token 消耗
    pub total_estimated_tokens: u64,
}

pub struct StatsEngine;

impl StatsEngine {
    /// 从官方本地数据库获取统计数据 (零代理、零拦截，只读模式安全读取)
    pub fn collect_local_stats() -> Result<LocalUsageStats> {
        let home = match dirs::home_dir() {
            Some(h) => h,
            None => return Ok(LocalUsageStats::default()),
        };

        let db_path = home.join(".gemini/antigravity/conversation_summaries.db");
        if !db_path.exists() {
            return Ok(LocalUsageStats::default());
        }

        // 以只读模式打开 SQLite，确保绝对不加锁或干扰 IDE/CLI 写入
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let conn = Connection::open_with_flags(&db_path, flags)?;

        let mut stmt = conn.prepare(
            "SELECT conversation_id, step_count, last_modified_time, source, app_data_dir 
             FROM conversation_summaries",
        )?;

        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let steps: i64 = row.get(1)?;
            let last_mod: String = row.get(2)?;
            let source: String = row.get(3).unwrap_or_default();
            let app_data: String = row.get(4).unwrap_or_default();
            Ok((id, steps.max(0) as u64, last_mod, source, app_data))
        })?;

        let mut ide_summary = HostUsageSummary {
            host_name: "Antigravity IDE".to_string(),
            host_id: "ide".to_string(),
            ..Default::default()
        };

        let mut app_summary = HostUsageSummary {
            host_name: "Antigravity App (Classic)".to_string(),
            host_id: "app".to_string(),
            ..Default::default()
        };

        let mut cli_summary = HostUsageSummary {
            host_name: "Antigravity CLI (agy)".to_string(),
            host_id: "cli".to_string(),
            ..Default::default()
        };

        let mut plugin_summary = HostUsageSummary {
            host_name: "Plugin (ACP Daemon)".to_string(),
            host_id: "plugin".to_string(),
            ..Default::default()
        };

        let mut total_conversations = 0;
        let mut total_steps = 0;

        for item in rows.flatten() {
            let (_id, steps, last_mod, source, app_data) = item;
            total_conversations += 1;
            total_steps += steps;

            // 识别宿主来源归因
            // 1. source: "cli" / "agy"
            // 2. app_data_dir 或 source 带有特定特征
            let target_host = if source.contains("cli") || source.contains("agy") {
                &mut cli_summary
            } else if source.contains("acp") || source.contains("plugin") {
                &mut plugin_summary
            } else if app_data.contains("Antigravity") && !app_data.contains("IDE") {
                &mut app_summary
            } else {
                // 默认大部分来自 Antigravity IDE
                &mut ide_summary
            };

            target_host.conversation_count += 1;
            target_host.total_steps += steps;
            if target_host.latest_activity.is_none()
                || target_host.latest_activity.as_deref().unwrap_or("") < last_mod.as_str()
            {
                target_host.latest_activity = Some(last_mod);
            }
        }

        // 估算 Token (官方多轮会话每 step 平均输入+输出约为 1,250 tokens 经验模型)
        for host in [
            &mut ide_summary,
            &mut app_summary,
            &mut cli_summary,
            &mut plugin_summary,
        ] {
            host.estimated_tokens = host.total_steps * 1250;
        }

        let total_estimated = total_steps * 1250;

        let hosts = vec![ide_summary, app_summary, cli_summary, plugin_summary];

        Ok(LocalUsageStats {
            hosts,
            total_conversations,
            total_steps,
            total_estimated_tokens: total_estimated,
        })
    }
}
