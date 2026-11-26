pub mod backup;

use crate::types::{Account, AccountSummary};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

fn default_index_version() -> String {
    "2.0".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AccountIndexFile {
    #[serde(default = "default_index_version")]
    version: String,
    #[serde(default)]
    accounts: Vec<AccountSummary>,
    #[serde(default)]
    current_account_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    current_target_ide: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    current_classic_account_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    current_ide_account_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    current_agy_account_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    current_acp_account_id: Option<String>,
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}

impl Default for AccountIndexFile {
    fn default() -> Self {
        Self {
            version: default_index_version(),
            accounts: Vec::new(),
            current_account_id: None,
            current_target_ide: None,
            current_classic_account_id: None,
            current_ide_account_id: None,
            current_agy_account_id: None,
            current_acp_account_id: None,
            extra: serde_json::Map::new(),
        }
    }
}

pub struct StorageManager {
    base_dir: PathBuf,
    write_lock: std::sync::Mutex<()>,
}

/// 校验账号 ID 是否为安全的单层文件名（严禁路径分隔符、`..`、空字符或控制字符，阻断目录穿越）
pub(crate) fn is_safe_account_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id != "."
        && id != ".."
        && !id.contains("..")
        && !id.contains('/')
        && !id.contains('\\')
        && !id.chars().any(|c| c.is_control())
}

/// 原子写入文件并在创建瞬间锁定 Unix 0600 权限（消除先 0644 落盘再 chmod 的 TOCTOU 窗口）
pub(crate) fn atomic_write_secure(path: &std::path::Path, content: &str) -> Result<()> {
    use std::io::Write;

    let parent = path
        .parent()
        .context("Target file path has no parent directory")?;
    if !parent.exists() {
        fs::create_dir_all(parent)?;
    }
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("tmp");
    let tmp_path = parent.join(format!(
        ".{}.{}.tmp",
        file_name,
        uuid::Uuid::new_v4().simple()
    ));

    let write_res: Result<()> = (|| {
        #[cfg(unix)]
        let mut file = {
            use std::os::unix::fs::OpenOptionsExt;
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&tmp_path)
                .with_context(|| format!("Failed to create secure temp file {:?}", tmp_path))?
        };

        #[cfg(not(unix))]
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)
            .with_context(|| format!("Failed to create temp file {:?}", tmp_path))?;

        file.write_all(content.as_bytes())
            .with_context(|| format!("Failed to write temp file {:?}", tmp_path))?;
        let _ = file.sync_all();
        Ok(())
    })();

    if let Err(e) = write_res {
        let _ = fs::remove_file(&tmp_path);
        return Err(e);
    }

    fs::rename(&tmp_path, path).with_context(|| {
        let _ = fs::remove_file(&tmp_path);
        format!("Failed to atomically rename {:?} -> {:?}", tmp_path, path)
    })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn ensure_private_dir(dir: &std::path::Path) -> Result<()> {
    if !dir.exists() {
        fs::create_dir_all(dir)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

impl StorageManager {
    /// 默认与主分支共用目录: ~/.antigravity_tools
    pub fn new() -> Result<Self> {
        let home = dirs::home_dir().context("Cannot resolve user home directory")?;
        let base_dir = home.join(".antigravity_tools");
        Self::with_dir(base_dir)
    }

    pub fn with_dir(base_dir: PathBuf) -> Result<Self> {
        ensure_private_dir(&base_dir)?;
        let accounts_dir = base_dir.join("accounts");
        ensure_private_dir(&accounts_dir)?;
        Ok(Self {
            base_dir,
            write_lock: std::sync::Mutex::new(()),
        })
    }

    fn index_path(&self) -> PathBuf {
        self.base_dir.join("accounts.json")
    }

    fn read_index(&self) -> Result<AccountIndexFile> {
        let path = self.index_path();
        if !path.exists() {
            return Ok(AccountIndexFile::default());
        }
        let content = fs::read_to_string(&path)?;
        if content.trim().is_empty() {
            return Ok(AccountIndexFile::default());
        }
        match serde_json::from_str::<AccountIndexFile>(&content) {
            Ok(index) => Ok(index),
            Err(e) => {
                let ts = chrono::Utc::now().timestamp();
                let backup_path = self.base_dir.join(format!("accounts.json.corrupt.{}", ts));
                let _ = fs::copy(&path, &backup_path);
                Err(anyhow::anyhow!(
                    "accounts.json is corrupted (backed up to {:?}): {}",
                    backup_path,
                    e
                ))
            }
        }
    }

    fn account_file_path(&self, id: &str) -> PathBuf {
        self.base_dir.join("accounts").join(format!("{}.json", id))
    }

    /// 列出所有账号索引
    pub fn list_account_summaries(&self) -> Result<Vec<AccountSummary>> {
        let index = self.read_index()?;
        Ok(index.accounts)
    }

    /// 列出所有账号完整实体 (严格以 accounts.json 索引为准，与主分支 Antigravity Tools 保持一致)
    pub fn list_full_accounts(&self) -> Result<Vec<Account>> {
        let index_path = self.index_path();
        if index_path.exists() {
            let summaries = self.list_account_summaries()?;
            let mut list = Vec::with_capacity(summaries.len());
            for s in summaries {
                if let Some(acc) = self.get_account(&s.id)? {
                    list.push(acc);
                }
            }
            return Ok(list);
        }

        // 仅当 accounts.json 尚未创建时，扫描 accounts 目录下全量实体文件
        let mut list = Vec::new();
        let accounts_dir = self.base_dir.join("accounts");
        if accounts_dir.exists() {
            if let Ok(entries) = fs::read_dir(accounts_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) == Some("json") {
                        if let Some(file_stem) = path.file_stem().and_then(|s| s.to_str()) {
                            if let Ok(Some(acc)) = self.get_account(file_stem) {
                                list.push(acc);
                            }
                        }
                    }
                }
            }
        }

        Ok(list)
    }

    /// 读取单个账号详细信息 (含 Token 与配额，并自动归一化计算流速/水库额度)
    pub fn get_account(&self, id: &str) -> Result<Option<Account>> {
        if !is_safe_account_id(id) {
            return Ok(None);
        }
        let path = self.account_file_path(id);
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(path)?;
        let mut account: Account = serde_json::from_str(&content)?;

        // 自动计算/回填 Gemini 与 Claude 配额百分比
        if let Some(q) = &mut account.quota {
            if q.gemini_percentage.is_none() {
                q.gemini_percentage = q.calculate_gemini_percentage();
            }
            if q.claude_percentage.is_none() {
                q.claude_percentage = q.calculate_claude_percentage();
            }
        }

        Ok(Some(account))
    }

    /// 通过 ID 或 Email 模糊/精确查找账号
    pub fn find_account(&self, query: &str) -> Result<Option<Account>> {
        // 1. 先按精确 ID
        if let Some(acc) = self.get_account(query)? {
            return Ok(Some(acc));
        }

        // 2. 按索引查找 (ID、Email、前缀)
        let summaries = self.list_account_summaries()?;
        for s in summaries {
            if s.id == query || s.email.eq_ignore_ascii_case(query) || s.id.starts_with(query) {
                if let Ok(Some(acc)) = self.get_account(&s.id) {
                    return Ok(Some(acc));
                }
            }
        }

        // 3. 兜底扫描 accounts 目录下全量实体文件 (兼容未在 index 中的账号)
        let accounts_dir = self.base_dir.join("accounts");
        if accounts_dir.exists() {
            if let Ok(entries) = fs::read_dir(accounts_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) == Some("json") {
                        if let Some(file_stem) = path.file_stem().and_then(|s| s.to_str()) {
                            if file_stem == query || file_stem.starts_with(query) {
                                if let Ok(Some(acc)) = self.get_account(file_stem) {
                                    return Ok(Some(acc));
                                }
                            }
                            if let Ok(Some(acc)) = self.get_account(file_stem) {
                                if acc.email.eq_ignore_ascii_case(query) {
                                    return Ok(Some(acc));
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(None)
    }

    /// 保存单个账号并原子更新 accounts.json 索引（保留主分支扩展字段并确保完全兼容）
    pub fn save_account(&self, account: &Account) -> Result<()> {
        if !is_safe_account_id(&account.id) {
            anyhow::bail!("Unsafe or invalid account ID rejected: {:?}", account.id);
        }
        let _guard = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());
        // 1. 写实体文件（若磁盘已有主分支文件，则合并保留未知扩展字段，避免丢失反代/设备历史等元数据）
        let account_path = self.account_file_path(&account.id);
        let new_val = serde_json::to_value(account)?;
        let merged_val = if account_path.exists() {
            if let Ok(existing_str) = fs::read_to_string(&account_path) {
                if let Ok(mut existing_val) =
                    serde_json::from_str::<serde_json::Value>(&existing_str)
                {
                    if let (Some(existing_obj), Some(new_obj)) =
                        (existing_val.as_object_mut(), new_val.as_object())
                    {
                        for (k, v) in new_obj {
                            if k == "token" {
                                if let (Some(et), Some(nt)) = (
                                    existing_obj
                                        .get_mut("token")
                                        .and_then(|x| x.as_object_mut()),
                                    v.as_object(),
                                ) {
                                    for (tk, tv) in nt {
                                        et.insert(tk.clone(), tv.clone());
                                    }
                                    continue;
                                }
                            }
                            existing_obj.insert(k.clone(), v.clone());
                        }
                        existing_val
                    } else {
                        new_val
                    }
                } else {
                    new_val
                }
            } else {
                new_val
            }
        } else {
            new_val
        };
        let content = serde_json::to_string_pretty(&merged_val)?;
        atomic_write_secure(&account_path, &content)?;

        // 2. 更新 index 文件
        let index_path = self.index_path();
        let mut index = self.read_index()?;
        if index.version.is_empty() {
            index.version = default_index_version();
        }

        let now = chrono::Utc::now().timestamp();
        let summary = AccountSummary {
            id: account.id.clone(),
            email: account.email.clone(),
            name: account.name.clone(),
            priority: account.priority,
            disabled: account.disabled,
            tags: account.tags.clone(),
            created_at: Some(account.created_at.unwrap_or(now)),
            last_used: Some(account.last_used.unwrap_or(now)),
        };

        if let Some(pos) = index.accounts.iter().position(|a| a.id == account.id) {
            index.accounts[pos] = summary;
        } else {
            index.accounts.push(summary);
        }

        let index_content = serde_json::to_string_pretty(&index)?;
        atomic_write_secure(&index_path, &index_content)?;
        Ok(())
    }

    /// 重新调整账号持久化顺序 (供自由拖拽排序使用，对齐主分支)
    pub fn reorder_accounts(&self, ordered_ids: &[String]) -> Result<()> {
        let _guard = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());
        let index_path = self.index_path();
        if !index_path.exists() {
            return Ok(());
        }
        let mut index = self.read_index()?;

        let id_to_summary: std::collections::HashMap<String, AccountSummary> = index
            .accounts
            .into_iter()
            .map(|a| (a.id.clone(), a))
            .collect();

        let mut new_accounts = Vec::with_capacity(id_to_summary.len());
        let mut seen = std::collections::HashSet::new();

        for id in ordered_ids {
            if let Some(summary) = id_to_summary.get(id) {
                new_accounts.push(summary.clone());
                seen.insert(id.clone());
            }
        }

        for (id, summary) in id_to_summary {
            if !seen.contains(&id) {
                new_accounts.push(summary);
            }
        }

        index.accounts = new_accounts;
        let index_content = serde_json::to_string_pretty(&index)?;
        atomic_write_secure(&index_path, &index_content)?;
        Ok(())
    }

    /// 删除账号
    pub fn delete_account(&self, id: &str) -> Result<bool> {
        if !is_safe_account_id(id) {
            anyhow::bail!("Unsafe or invalid account ID rejected: {:?}", id);
        }
        let _guard = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());
        let account_path = self.account_file_path(id);
        let existed = account_path.exists();
        if existed {
            let _ = fs::remove_file(account_path);
        }

        let index_path = self.index_path();
        if index_path.exists() {
            let mut index = self.read_index()?;
            let initial_len = index.accounts.len();
            index.accounts.retain(|a| a.id != id);
            if index.accounts.len() != initial_len {
                atomic_write_secure(&index_path, &serde_json::to_string_pretty(&index)?)?;
            }
        }
        Ok(existed)
    }

    /// 更新当前激活账号 ID (按目标宿主标记，并同步记录 current_target_ide 以对齐主分支状态机)
    pub fn set_active_account_id(&self, target: &str, id: &str) -> Result<()> {
        let _guard = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());
        let index_path = self.index_path();
        let mut index = self.read_index()?;

        index.current_account_id = Some(id.to_string());
        match target {
            "app" | "classic" => {
                index.current_target_ide = Some("classic".to_string());
                index.current_classic_account_id = Some(id.to_string());
            }
            "ide" => {
                index.current_target_ide = Some("ide".to_string());
                index.current_ide_account_id = Some(id.to_string());
            }
            "agy" | "cli" => {
                index.current_target_ide = Some("agy".to_string());
                index.current_agy_account_id = Some(id.to_string());
            }
            "acp" | "plugin" => {
                index.current_target_ide = Some("acp".to_string());
                index.current_acp_account_id = Some(id.to_string());
            }
            _ => {
                index.current_target_ide = Some("all".to_string());
                index.current_classic_account_id = Some(id.to_string());
                index.current_ide_account_id = Some(id.to_string());
            }
        }

        atomic_write_secure(&index_path, &serde_json::to_string_pretty(&index)?)?;
        Ok(())
    }

    /// 获取当前存储中各宿主激活的账号 ID (app, ide, acp, agy)
    pub fn get_active_account_ids(
        &self,
    ) -> Result<(
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    )> {
        let index = self.read_index()?;
        let app = index.current_classic_account_id;
        let ide = index.current_ide_account_id;
        let agy = index.current_agy_account_id;
        let acp = index.current_acp_account_id;
        Ok((app, ide, acp, agy))
    }

    /// 获取完整的索引激活上下文 `(current_account_id, current_target_ide, app, ide, acp, agy)`
    pub fn get_active_index_context(
        &self,
    ) -> Result<(
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    )> {
        let index = self.read_index()?;
        Ok((
            index.current_account_id,
            index.current_target_ide,
            index.current_classic_account_id,
            index.current_ide_account_id,
            index.current_acp_account_id,
            index.current_agy_account_id,
        ))
    }

    /// 解除账号的 403 / 验证锁定状态 (对齐主分支逻辑)
    pub fn clear_account_forbidden(&self, account_id: &str) -> Result<()> {
        if let Some(mut account) = self.get_account(account_id)? {
            // 清除 quota 中的 403 标记
            if let Some(ref mut q) = account.quota {
                q.is_forbidden = Some(false);
                q.forbidden_reason = None;
            }

            // 清除验证锁定
            account.validation_blocked = false;
            account.validation_blocked_until = None;
            account.validation_url = None;

            // 如果 proxy_disabled 是由于 403 触发的，则一并恢复
            if let Some(ref reason) = account.proxy_disabled_reason {
                let lower = reason.to_lowercase();
                if lower.contains("403")
                    || lower.contains("forbidden")
                    || lower.contains("permission_denied")
                {
                    account.proxy_disabled = false;
                    account.proxy_disabled_reason = None;
                }
            }

            self.save_account(&account)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TokenInfo;

    fn sample_account(id: &str, email: &str) -> Account {
        let val = serde_json::json!({
            "id": id,
            "email": email,
            "token": TokenInfo {
                access_token: "ya29.test".to_string(),
                refresh_token: "1//test-rt".to_string(),
                expires_in: Some(3600),
                token_type: Some("Bearer".to_string()),
                expiry_timestamp: None,
                project_id: None,
                id_token: None,
                oauth_client_key: None,
                is_gcp_tos: None,
            }
        });
        serde_json::from_value(val).unwrap()
    }

    #[test]
    fn rejects_path_traversal_ids_and_enforces_secure_permissions() {
        let tmp_dir = std::env::temp_dir().join(format!("am-storage-sec-{}", uuid::Uuid::new_v4()));
        let storage = StorageManager::with_dir(tmp_dir.clone()).unwrap();

        for unsafe_id in [
            "",
            ".",
            "..",
            "../escape",
            "a/b",
            "a\\b",
            "foo..bar",
            "bad\0id",
        ] {
            assert!(!is_safe_account_id(unsafe_id));
            let bad = sample_account(unsafe_id, "evil@example.com");
            assert!(storage.save_account(&bad).is_err());
            assert!(storage.delete_account(unsafe_id).is_err());
            assert!(storage.get_account(unsafe_id).unwrap().is_none());
        }

        // 导入含恶意 ID 的全量快照时应自动净化为安全 UUID 并保存在 accounts/ 目录内
        let malicious_bundle =
            serde_json::json!([sample_account("../../outside", "safe@example.com")]).to_string();
        let report =
            backup::BackupManager::import_from_str(&storage, &malicious_bundle, true).unwrap();
        assert_eq!(report.imported, 1);

        let all = storage.list_full_accounts().unwrap();
        assert_eq!(all.len(), 1);
        assert!(is_safe_account_id(&all[0].id));
        assert_ne!(all[0].id, "../../outside");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let acc_path = tmp_dir.join("accounts").join(format!("{}.json", all[0].id));
            let mode = fs::metadata(&acc_path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
            let dir_mode = fs::metadata(&tmp_dir).unwrap().permissions().mode() & 0o777;
            assert_eq!(dir_mode, 0o700);
        }

        let _ = fs::remove_dir_all(&tmp_dir);
    }
}
