//! CLI handlers for the WeChat channel: login, list, delete.

use anyhow::Result;

use crate::WechatCommands;

#[cfg(feature = "channel-wechat")]
pub async fn handle(cmd: WechatCommands) -> Result<()> {
    use zeroclaw_channels::wechat::{accounts, auth};

    match cmd {
        WechatCommands::Login => {
            let account_id = auth::login(auth::LoginOptions::default()).await?;
            println!("✅ WeChat login successful: account_id = {account_id}");
            println!(
                "Now configure ZeroClaw with:\n  zeroclaw config set channels.wechat.account_id {account_id}\n  zeroclaw config set channels.wechat.enabled true"
            );
            Ok(())
        }
        WechatCommands::List => {
            let accounts = accounts::list_accounts();
            if accounts.is_empty() {
                println!("No WeChat accounts saved.");
                println!("Run `zeroclaw channel wechat login` to add one.");
                return Ok(());
            }
            println!("Saved WeChat accounts:");
            for a in accounts {
                let token_marker = if a.has_token { "✅" } else { "❌" };
                println!(
                    "  {token_marker} {id}  user={user}  base_url={base}",
                    id = a.account_id,
                    user = a.user_id.as_deref().unwrap_or("-"),
                    base = a.base_url.as_deref().unwrap_or("-"),
                );
            }
            Ok(())
        }
        WechatCommands::Delete { account_id } => {
            accounts::delete_account(&account_id)?;
            println!("✅ Deleted WeChat account: {account_id}");
            Ok(())
        }
    }
}

#[cfg(not(feature = "channel-wechat"))]
pub async fn handle(_cmd: WechatCommands) -> Result<()> {
    anyhow::bail!(
        "WeChat channel support is not built into this binary. Rebuild with `--features channel-wechat`."
    )
}
