use crate::storage::StorageManager;
use crate::types::Account;
use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// 账号数据便携备份包
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountsBackupBundle {
    pub version: String,
    pub exported_at: String,
    pub total_accounts: usize,
    pub accounts: Vec<Account>,
}

pub struct BackupManager;

impl BackupManager {
    /// 导出所有账号至指定备份 JSON 文件
    pub fn export_to_file(storage: &StorageManager, export_path: &Path) -> Result<usize> {
        let accounts = storage.list_full_accounts()?;
        let bundle = AccountsBackupBundle {
            version: "1.0".to_string(),
            exported_at: Utc::now().to_rfc3339(),
            total_accounts: accounts.len(),
            accounts,
        };

        if let Some(parent) = export_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let json = serde_json::to_string_pretty(&bundle)?;
        fs::write(export_path, json)?;
        Ok(bundle.total_accounts)
    }

    /// 从备份 JSON 文件导入账号 (支持覆盖或合并)
    pub fn import_from_file(
        storage: &StorageManager,
        import_path: &Path,
        overwrite: bool,
    ) -> Result<ImportReport> {
        let content = fs::read_to_string(import_path)
            .with_context(|| format!("Failed to read backup file at {:?}", import_path))?;

        let bundle: AccountsBackupBundle = serde_json::from_str(&content)
            .or_else(|_| -> Result<AccountsBackupBundle, serde_json::Error> {
                let accs: Vec<Account> = serde_json::from_str(&content)?;
                Ok(AccountsBackupBundle {
                    version: "raw".to_string(),
                    exported_at: Utc::now().to_rfc3339(),
                    total_accounts: accs.len(),
                    accounts: accs,
                })
            })
            .with_context(|| "Failed to parse accounts backup bundle")?;

        let mut report = ImportReport::default();

        for acc in bundle.accounts {
            let existing = storage.get_account(&acc.id)?;
            if existing.is_some() && !overwrite {
                report.skipped += 1;
                continue;
            }

            if existing.is_some() {
                report.overwritten += 1;
            } else {
                report.imported += 1;
            }

            storage.save_account(&acc)?;
        }

        Ok(report)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ImportReport {
    pub imported: usize,
    pub overwritten: usize,
    pub skipped: usize,
}
