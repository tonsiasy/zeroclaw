//! Persistent storage for WeChat iLink Bot credentials.
//!
//! Layout under `<state_dir>/wechat/`:
//! - `accounts/<account_id>.json` — per-account credentials
//! - `accounts.json` — ordered index of known account IDs
//! - `sync_buf/<account_id>` — long-poll cursor for `getupdates`
//!
//! `state_dir` resolves to `$ZEROCLAW_STATE_DIR` if set, otherwise `~/.zeroclaw`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const DEFAULT_BASE_URL: &str = "https://ilinkai.weixin.qq.com";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AccountData {
    pub token: Option<String>,
    pub base_url: Option<String>,
    pub user_id: Option<String>,
    pub saved_at: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct StoredAccount {
    pub account_id: String,
    pub has_token: bool,
    pub base_url: Option<String>,
    pub user_id: Option<String>,
    pub saved_at: Option<u64>,
}

fn state_dir() -> PathBuf {
    if let Some(v) = std::env::var("ZEROCLAW_STATE_DIR")
        .ok()
        .filter(|s| !s.is_empty())
    {
        return PathBuf::from(v);
    }
    directories::BaseDirs::new()
        .map(|d| d.home_dir().join(".zeroclaw"))
        .unwrap_or_else(|| PathBuf::from(".zeroclaw"))
}

fn wechat_dir() -> PathBuf {
    state_dir().join("wechat")
}

fn accounts_dir() -> PathBuf {
    wechat_dir().join("accounts")
}

fn account_path(account_id: &str) -> PathBuf {
    accounts_dir().join(format!("{account_id}.json"))
}

fn index_path() -> PathBuf {
    wechat_dir().join("accounts.json")
}

pub fn sync_buf_path(account_id: &str) -> PathBuf {
    wechat_dir().join("sync_buf").join(account_id)
}

/// Normalise a raw iLink user ID into a filesystem-safe account identifier.
pub fn normalize_account_id(raw: &str) -> String {
    raw.trim().to_lowercase().replace(['@', '.'], "-")
}

pub fn list_account_ids() -> Vec<String> {
    let raw = match std::fs::read_to_string(index_path()) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    serde_json::from_str::<Vec<String>>(&raw).unwrap_or_default()
}

pub fn list_accounts() -> Vec<StoredAccount> {
    list_account_ids()
        .into_iter()
        .filter_map(|account_id| {
            let data = load_account(&account_id)?;
            Some(StoredAccount {
                account_id,
                has_token: data
                    .token
                    .as_ref()
                    .map(|v| !v.trim().is_empty())
                    .unwrap_or(false),
                base_url: data.base_url,
                user_id: data.user_id,
                saved_at: data.saved_at,
            })
        })
        .collect()
}

pub fn load_account(account_id: &str) -> Option<AccountData> {
    let raw = std::fs::read_to_string(account_path(account_id)).ok()?;
    serde_json::from_str(&raw).ok()
}

pub fn save_account(account_id: &str, mut data: AccountData) -> std::io::Result<()> {
    std::fs::create_dir_all(accounts_dir())?;
    if data.saved_at.is_none() {
        data.saved_at = Some(now_unix_secs());
    }
    std::fs::write(
        account_path(account_id),
        serde_json::to_vec_pretty(&data).unwrap_or_default(),
    )?;
    register_account_id(account_id)
}

pub fn delete_account(account_id: &str) -> std::io::Result<()> {
    let normalized = normalize_account_id(account_id);
    let path = account_path(&normalized);
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    let mut ids = list_account_ids();
    let original_len = ids.len();
    ids.retain(|v| v != &normalized);
    if ids.len() != original_len {
        std::fs::write(
            index_path(),
            serde_json::to_vec_pretty(&ids).unwrap_or_default(),
        )?;
    }
    // best-effort sync_buf cleanup
    let _ = std::fs::remove_file(sync_buf_path(&normalized));
    Ok(())
}

fn register_account_id(account_id: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(wechat_dir())?;
    let mut ids = list_account_ids();
    ids.retain(|v| v != account_id);
    ids.push(account_id.to_string());
    std::fs::write(
        index_path(),
        serde_json::to_vec_pretty(&ids).unwrap_or_default(),
    )
}

fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Resolve credentials for a specific account ID.
///
/// Returns `Err` with an actionable message if the account is missing or has no token.
pub fn resolve_account(account_id: &str) -> anyhow::Result<(String, String)> {
    let data = load_account(account_id).ok_or_else(|| {
        anyhow::anyhow!(
            "WeChat credentials not found at {}. Run `zeroclaw channel wechat login` first.",
            account_path(account_id).display()
        )
    })?;
    let token = data
        .token
        .filter(|t| !t.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("WeChat credentials missing token"))?;
    let base_url = data
        .base_url
        .filter(|u| !u.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
    Ok((token, base_url))
}

pub fn load_sync_buf(account_id: &str) -> String {
    std::fs::read_to_string(sync_buf_path(account_id)).unwrap_or_default()
}

pub fn save_sync_buf(account_id: &str, buf: &str) {
    let path = sync_buf_path(account_id);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, buf);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_account_id() {
        assert_eq!(normalize_account_id("Tony@Wang.123"), "tony-wang-123");
        assert_eq!(normalize_account_id("  ABC  "), "abc");
    }

    #[test]
    fn test_state_dir_env_override() {
        // Setting the env var produces a deterministic path
        let prev = std::env::var("ZEROCLAW_STATE_DIR").ok();
        // Use a unique value so the test doesn't conflict with parallel runs
        unsafe { std::env::set_var("ZEROCLAW_STATE_DIR", "/tmp/zeroclaw-test-state") };
        assert_eq!(state_dir(), PathBuf::from("/tmp/zeroclaw-test-state"));
        match prev {
            Some(v) => unsafe { std::env::set_var("ZEROCLAW_STATE_DIR", v) },
            None => unsafe { std::env::remove_var("ZEROCLAW_STATE_DIR") },
        }
    }
}
