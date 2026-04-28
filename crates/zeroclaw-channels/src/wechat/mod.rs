//! WeChat (iLink Bot) channel implementation.
//!
//! Submodules:
//! - [`channel`]: the [`WeChatChannel`] type that implements `Channel`.
//! - [`accounts`]: credential storage under `~/.zeroclaw/wechat/accounts/`.
//! - [`auth`]: QR-code login flow used by the `zeroclaw channel wechat login` CLI.

pub mod accounts;
pub mod auth;
pub mod channel;

pub use channel::WeChatChannel;
