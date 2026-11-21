use crate::types::{Account, AccountSummary};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct AccountIndexFile {
    #[serde(default)]
    accounts: Vec<AccountSummary>,
    #[serde(default)]
    current_account_id: Option<String>,
    #[serde(default)]
    current_classic_account_id: Option<String>,
    #[serde(default)]
    current_ide_account_id: Option<String>,
    #[serde(default)]
    current_agy_account_id: Option<String>,
    #[serde(default)]
    current_acp_account_id: Option<String>,
}

pub struct StorageManager {
    base_dir: PathBuf,
}

impl StorageManager {
    /// 默认与主分支共用目录: ~/.antigravity_tools
    pub fn new() -> Result<Self> {
        let home = dirs::home_dir().context("Cannot resolve user home directory")?;
        let base_dir = home.join(".antigravity_tools");
        if !base_dir.exists() {
            fs::create_dir_all(&base_dir)?;
        }
        let accounts_dir = base_dir.join("accounts");
        if !accounts_dir.exists() {
            fs::create_dir_all(&accounts_dir)?;
        }
        Ok(Self { base_dir })
    }

    pub fn with_dir(base_dir: PathBuf) -> Result<Self> {
        if !base_dir.exists() {
            fs::create_dir_all(&base_dir)?;
        }
        let accounts_dir = base_dir.join("accounts");
        if !accounts_dir.exists() {
            fs::create_dir_all(&accounts_dir)?;
        }
        Ok(Self { base_dir })
    }

    fn index_path(&self) -> PathBuf {
        self.base_dir.join("accounts.json")
    }

    fn account_file_path(&self, id: &str) -> PathBuf {
        self.base_dir.join("accounts").join(format!("{}.json", id))
    }

    /// 列出所有账号索引
    pub fn list_account_summaries(&self) -> Result<Vec<AccountSummary>> {
        let path = self.index_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let content = fs::read_to_string(path)?;
        let index: AccountIndexFile = serde_json::from_str(&content).unwrap_or_default();
        Ok(index.accounts)
    }

    /// 读取单个账号详细信息 (含 Token 与配额)
    pub fn get_account(&self, id: &str) -> Result<Option<Account>> {
        let path = self.account_file_path(id);
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(path)?;
        let account: Account = serde_json::from_str(&content)?;
        Ok(Some(account))
    }

    /// 通过 ID 或 Email 模糊/精确查找账号
    pub fn find_account(&self, query: &str) -> Result<Option<Account>> {
        // 先按精确 ID
        if let Some(acc) = self.get_account(query)? {
            return Ok(Some(acc));
        }
        // 按 Email 或前缀查找
        let summaries = self.list_account_summaries()?;
        for s in summaries {
            if s.id == query || s.email.eq_ignore_ascii_case(query) || s.id.starts_with(query) {
                return self.get_account(&s.id);
            }
        }
        Ok(None)
    }

    /// 保存单个账号并原子更新 accounts.json 索引
    pub fn save_account(&self, account: &Account) -> Result<()> {
        // 1. 写实体文件
        let account_path = self.account_file_path(&account.id);
        let content = serde_json::to_string_pretty(account)?;
        fs::write(account_path, content)?;

        // 2. 更新 index 文件
        let index_path = self.index_path();
        let mut index: AccountIndexFile = if index_path.exists() {
            let s = fs::read_to_string(&index_path).unwrap_or_default();
            serde_json::from_str(&s).unwrap_or_default()
        } else {
            AccountIndexFile::default()
        };

        let summary = AccountSummary {
            id: account.id.clone(),
            email: account.email.clone(),
            name: account.name.clone(),
            priority: account.priority,
            disabled: account.disabled,
            tags: account.tags.clone(),
            created_at: account.created_at,
            last_used: account.last_used,
        };

        if let Some(pos) = index.accounts.iter().position(|a| a.id == account.id) {
            index.accounts[pos] = summary;
        } else {
            index.accounts.push(summary);
        }

        let index_content = serde_json::to_string_pretty(&index)?;
        fs::write(index_path, index_content)?;
        Ok(())
    }

    /// 删除账号
    pub fn delete_account(&self, id: &str) -> Result<bool> {
        let account_path = self.account_file_path(id);
        let existed = account_path.exists();
        if existed {
            let _ = fs::remove_file(account_path);
        }

        let index_path = self.index_path();
        if index_path.exists() {
            let s = fs::read_to_string(&index_path).unwrap_or_default();
            let mut index: AccountIndexFile = serde_json::from_str(&s).unwrap_or_default();
            let initial_len = index.accounts.len();
            index.accounts.retain(|a| a.id != id);
            if index.accounts.len() != initial_len {
                let _ = fs::write(index_path, serde_json::to_string_pretty(&index)?);
            }
        }
        Ok(existed)
    }

    /// 更新当前激活账号 ID (按目标宿主标记)
    pub fn set_active_account_id(&self, target: &str, id: &str) -> Result<()> {
        let index_path = self.index_path();
        let mut index: AccountIndexFile = if index_path.exists() {
            let s = fs::read_to_string(&index_path).unwrap_or_default();
            serde_json::from_str(&s).unwrap_or_default()
        } else {
            AccountIndexFile::default()
        };

        match target {
            "app" | "classic" => index.current_classic_account_id = Some(id.to_string()),
            "ide" => index.current_ide_account_id = Some(id.to_string()),
            "agy" => index.current_agy_account_id = Some(id.to_string()),
            "acp" | "plugin" => index.current_acp_account_id = Some(id.to_string()),
            _ => {
                index.current_account_id = Some(id.to_string());
                index.current_classic_account_id = Some(id.to_string());
                index.current_ide_account_id = Some(id.to_string());
            }
        }

        let _ = fs::write(index_path, serde_json::to_string_pretty(&index)?);
        Ok(())
    }
}
