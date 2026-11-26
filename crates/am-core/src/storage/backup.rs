use crate::storage::StorageManager;
use crate::types::Account;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// 主分支通用单个导出项格式 (兼容主分支 `AccountExportItem`)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountExportItem {
    pub email: String,
    pub refresh_token: String,
}

/// 主分支标准导出响应对象格式 (`{"accounts": [...]}`)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountExportResponse {
    pub accounts: Vec<AccountExportItem>,
}

/// 兼容导入输入格式 (同时兼容主分支的裸数组 `[{"email": "...", "refresh_token": "..."}]` 或包裹对象)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportAccountEntry {
    #[serde(default)]
    pub email: Option<String>,
    pub refresh_token: String,
}

/// 账号数据便携备份包 (全量快照)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountsBackupBundle {
    pub version: String,
    pub exported_at: String,
    pub total_accounts: usize,
    pub accounts: Vec<Account>,
}

pub struct BackupManager;

impl BackupManager {
    /// 导出所有账号至指定备份 JSON 文件 (默认输出主分支标准的 `{"accounts": [{"email": "...", "refresh_token": "..."}]}`)
    pub fn export_to_file(storage: &StorageManager, export_path: &Path) -> Result<usize> {
        let accounts = storage.list_full_accounts()?;
        let items: Vec<AccountExportItem> = accounts
            .into_iter()
            .map(|a| AccountExportItem {
                email: a.email,
                refresh_token: a.token.refresh_token,
            })
            .collect();
        let resp = AccountExportResponse { accounts: items };

        if let Some(parent) = export_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let json = serde_json::to_string_pretty(&resp)?;
        crate::storage::atomic_write_secure(export_path, &json)?;
        Ok(resp.accounts.len())
    }

    /// 从备份 JSON 文件导入账号 (全面兼容主分支格式)
    /// 兼容形态 1: 主分支导出包裹对象 `{"accounts": [{"email": "...", "refresh_token": "..."}]}`
    /// 兼容形态 2: 主分支导出纯数组 `[{"email": "...", "refresh_token": "..."}]`
    /// 兼容形态 3: 全量 Account 实体快照包 (`AccountsBackupBundle` 或 `Vec<Account>`)
    pub fn import_from_file(
        storage: &StorageManager,
        import_path: &Path,
        overwrite: bool,
    ) -> Result<ImportReport> {
        let content = fs::read_to_string(import_path)
            .with_context(|| format!("Failed to read backup file at {:?}", import_path))?;

        Self::import_from_str(storage, &content, overwrite)
    }

    /// 从 JSON 字符串批量导入账号 (兼容所有主分支与全量快照形态)
    pub fn import_from_str(
        storage: &StorageManager,
        content: &str,
        overwrite: bool,
    ) -> Result<ImportReport> {
        let mut report = ImportReport::default();

        // 获取现有账号的 email/rt 映射以支持覆盖/跳过判定（随循环实时同步，防止同批次内重复创建）
        let mut existing_accounts = storage.list_full_accounts().unwrap_or_default();

        // 1. 尝试按全量 Account 结构快照解析
        if let Ok(bundle) = serde_json::from_str::<AccountsBackupBundle>(content) {
            for acc in bundle.accounts {
                Self::save_or_skip_account(
                    storage,
                    &mut existing_accounts,
                    acc,
                    overwrite,
                    &mut report,
                )?;
            }
            return Ok(report);
        }
        if let Ok(accs) = serde_json::from_str::<Vec<Account>>(content) {
            for acc in accs {
                Self::save_or_skip_account(
                    storage,
                    &mut existing_accounts,
                    acc,
                    overwrite,
                    &mut report,
                )?;
            }
            return Ok(report);
        }

        // 2. 尝试按主分支标准对象或数组提取 entries (`refresh_token` + `email`)
        let entries = Self::extract_import_entries(content)?;
        if entries.is_empty() {
            anyhow::bail!("No valid account credentials or refresh tokens found in JSON");
        }

        for entry in entries {
            let rt = entry.refresh_token.trim().to_string();
            if rt.is_empty() {
                report.skipped += 1;
                continue;
            }

            let email_hint = entry.email.as_deref().unwrap_or("").trim().to_lowercase();
            let matched_pos = existing_accounts.iter().position(|a| {
                (!email_hint.is_empty() && a.email.to_lowercase() == email_hint)
                    || a.token.refresh_token.trim() == rt
            });

            if let Some(pos) = matched_pos {
                if !overwrite {
                    report.skipped += 1;
                    continue;
                }
                // 原地覆盖 Token，同时清空旧 access_token / expiry 以强制下次切号或刷配额重新换取新 AT
                let mut updated = existing_accounts[pos].clone();
                if updated.token.refresh_token.trim() != rt {
                    updated.token.access_token.clear();
                    updated.token.expiry_timestamp = None;
                }
                updated.token.refresh_token = rt;
                storage.save_account(&updated)?;
                existing_accounts[pos] = updated;
                report.overwritten += 1;
            } else {
                // 新增账号：构建基础 Account 实体落盘
                let id = uuid::Uuid::new_v4().to_string();
                let email = if !email_hint.is_empty() {
                    entry.email.unwrap()
                } else {
                    format!("account_{}@imported", &id[..8])
                };

                let new_acc = Account {
                    id,
                    email,
                    name: None,
                    priority: Some(0),
                    token: crate::types::TokenInfo {
                        access_token: String::new(),
                        refresh_token: rt,
                        expires_in: None,
                        token_type: Some("Bearer".to_string()),
                        expiry_timestamp: None,
                        project_id: None,
                        id_token: None,
                        oauth_client_key: None,
                        is_gcp_tos: None,
                    },
                    device_profile: Some(crate::device::generate_device_profile()),
                    quota: None,
                    tags: Vec::new(),
                    disabled: false,
                    disabled_reason: None,
                    disabled_at: None,
                    banned: false,
                    banned_reason: None,
                    banned_at: None,
                    banned_code: None,
                    proxy_disabled: false,
                    proxy_disabled_reason: None,
                    proxy_disabled_at: None,
                    manual_proxy_disabled: false,
                    validation_blocked: false,
                    validation_blocked_until: None,
                    validation_url: None,
                    ban_appeal_url: None,
                    created_at: Some(chrono::Utc::now().timestamp()),
                    last_used: None,
                };
                storage.save_account(&new_acc)?;
                existing_accounts.push(new_acc);
                report.imported += 1;
            }
        }

        Ok(report)
    }

    fn save_or_skip_account(
        storage: &StorageManager,
        existing_accounts: &mut Vec<Account>,
        mut acc: Account,
        overwrite: bool,
        report: &mut ImportReport,
    ) -> Result<()> {
        // 防护外部备份 JSON 中的非法/路径穿越 ID（如 `../../...`），自动重置为安全 UUID
        if !crate::storage::is_safe_account_id(&acc.id) {
            acc.id = uuid::Uuid::new_v4().to_string();
        }
        let email_lower = acc.email.trim().to_lowercase();
        let rt_trimmed = acc.token.refresh_token.trim();
        let matched_pos = existing_accounts.iter().position(|a| {
            a.id == acc.id
                || (!email_lower.is_empty() && a.email.to_lowercase() == email_lower)
                || (!rt_trimmed.is_empty() && a.token.refresh_token.trim() == rt_trimmed)
        });

        if let Some(pos) = matched_pos {
            if !overwrite {
                report.skipped += 1;
                return Ok(());
            }
            // 沿用已有账号 ID 防止产生同邮箱双 ID
            acc.id = existing_accounts[pos].id.clone();
            storage.save_account(&acc)?;
            existing_accounts[pos] = acc;
            report.overwritten += 1;
        } else {
            storage.save_account(&acc)?;
            existing_accounts.push(acc);
            report.imported += 1;
        }
        Ok(())
    }

    /// 提取输入内容中的所有账号项
    /// 支持：
    /// 1. 主分支标准对象：`{"accounts": [{"email": "...", "refresh_token": "..."}]}`
    /// 2. 主分支裸数组：`[{"email": "...", "refresh_token": "..."}]`
    /// 3. 单纯的 RT 字符串数组：`["1//...", "1//..."]`
    /// 4. 纯文本 / 任意文本中的逐行或正则 RT 字符串 (`1//[a-zA-Z0-9_\-]+`)
    fn extract_import_entries(content: &str) -> Result<Vec<ImportAccountEntry>> {
        let mut results = Vec::new();

        // 1. 尝试以 JSON 解析
        if let Ok(raw) = serde_json::from_str::<serde_json::Value>(content) {
            if let Some(arr) = raw.as_array() {
                for v in arr {
                    if let Some(entry) = Self::parse_entry_value(v) {
                        results.push(entry);
                    }
                }
            } else if let Some(obj) = raw.as_object() {
                if let Some(accounts_val) = obj.get("accounts") {
                    if let Some(arr) = accounts_val.as_array() {
                        for v in arr {
                            if let Some(entry) = Self::parse_entry_value(v) {
                                results.push(entry);
                            }
                        }
                    } else if let Some(sub_obj) = accounts_val.as_object() {
                        for v in sub_obj.values() {
                            if let Some(entry) = Self::parse_entry_value(v) {
                                results.push(entry);
                            }
                        }
                    }
                } else {
                    // 单个对象或者键值对 map
                    if let Some(entry) = Self::parse_entry_value(&raw) {
                        results.push(entry);
                    } else {
                        for v in obj.values() {
                            if let Some(entry) = Self::parse_entry_value(v) {
                                results.push(entry);
                            }
                        }
                    }
                }
            }
        }

        // 2. 如果 JSON 结构化解析未找到结果，或者用户直接粘贴/导入纯 RT 文本 (如每行一个 1//... 或带其他杂质的文本)
        if results.is_empty() {
            let mut seen_tokens = std::collections::HashSet::new();

            for line in content.lines() {
                let trimmed = line.trim();
                // 逐词检查是否以 1// 开头
                for token in
                    trimmed.split(|c: char| c.is_whitespace() || c == ',' || c == '"' || c == '\'')
                {
                    let clean = token.trim();
                    if clean.starts_with("1//") && clean.len() >= 10 {
                        // 过滤合法字符：字母、数字、下划线、减号
                        let rt_candidate: String = clean
                            .chars()
                            .take_while(|c| {
                                c.is_ascii_alphanumeric() || *c == '_' || *c == '-' || *c == '/'
                            })
                            .collect();
                        if rt_candidate.starts_with("1//")
                            && seen_tokens.insert(rt_candidate.clone())
                        {
                            results.push(ImportAccountEntry {
                                email: None,
                                refresh_token: rt_candidate,
                            });
                        }
                    }
                }
            }
        }

        Ok(results)
    }

    fn parse_entry_value(val: &serde_json::Value) -> Option<ImportAccountEntry> {
        // 如果数组元素直接就是字符串 "1//..."
        if let Some(s) = val.as_str() {
            let trimmed = s.trim();
            if trimmed.starts_with("1//") {
                return Some(ImportAccountEntry {
                    email: None,
                    refresh_token: trimmed.to_string(),
                });
            }
        }

        let obj = val.as_object()?;
        let email = obj
            .get("email")
            .or_else(|| obj.get("name"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let rt = if let Some(r) = obj.get("refresh_token").and_then(|v| v.as_str()) {
            r.to_string()
        } else if let Some(token_obj) = obj.get("token").and_then(|v| v.as_object()) {
            token_obj
                .get("refresh_token")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        } else {
            String::new()
        };

        if !rt.is_empty() {
            Some(ImportAccountEntry {
                email,
                refresh_token: rt,
            })
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ImportReport {
    pub imported: usize,
    pub overwritten: usize,
    pub skipped: usize,
}
