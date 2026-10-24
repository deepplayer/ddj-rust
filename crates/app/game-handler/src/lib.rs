//! # ddj-game-handler — 消息处理器
//!
//! 对应 Java：`bf.game.server.message.handler.*`（103 个类 / 1705 个方法）。
//!
//! ## 目录布局（按原包结构一一对应）
//! ```text
//! src/
//!   login.rs         ← LoginMessageHandler       (msgId 3/4/5/6/7/98/11)
//!   item.rs          ← ItemMessageHandler        (11002 ItemUseReq …)
//!   player.rs        ← PlayerMessageHandler
//!   mail.rs          ← MailMessageHandler
//!   guild.rs         ← GuildMessageHandler
//!   rank.rs          ← RankMessageHandler
//!   activity.rs      ← ActivityMessageHandler
//!   battle.rs        ← BattleMessageHandler
//!   ...
//! ```
//!
//! ## 注册方式
//! ```ignore
//! #[handler(11002)]
//! async fn item_use(ctx: &HandlerCtx, req: ItemUseReq11002) -> Result<ItemUseResp11002> {
//!     ctx.player().items().use_item(req).await
//! }
//! ```
//! `#[handler]` 宏展开为 `inventory::submit!`，`HandlerTable` 在启动时收集，
//! cmdId 冲突直接编译失败。
//!
//! ## 返回值约定（复刻原框架）
//! - 返回 `Message` → 框架自动回包
//! - 返回 `()` → handler 自己发包（推送场景）

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod macros;

// 以下模块按原包结构 1:1 对应，逐个填充
pub mod account;
pub mod activity;
pub mod battle;
pub mod chat;
pub mod friend;
pub mod gm;
pub mod guild;
pub mod item;
pub mod login;
pub mod mail;
pub mod player;
pub mod push;
pub mod rank;
pub mod shop;
pub mod sign;
pub mod task;
