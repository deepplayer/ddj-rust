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
pub mod activity_template;
pub mod amber;
pub mod anticheat;
pub mod battle;
pub mod bind_reward;
pub mod chat;
pub mod friend;
pub mod gm;
pub mod guild;
pub mod counter;
pub mod declare;
pub mod divergence;
pub mod drop;
pub mod engine;
pub mod favourable;
pub mod handlers;
pub mod handshake;
pub mod install_activity;
pub mod install_amber;
pub mod install_anticheat;
pub mod install_bind;
pub mod install_cornucopia;
pub mod install_counter;
pub mod install_declare;
pub mod install_engine;
pub mod install_favourable;
pub mod install_item;
pub mod install_mail;
pub mod install_log;
pub mod install_misc;
pub mod install_pet;
pub mod install_platform;
pub mod install_pray;
pub mod install_player;
pub mod install_second_psw;
pub mod install_shortcut;
pub mod install_sign;
pub mod install_special_msg;
pub mod install_state;
pub mod install_system_open;
pub mod install_lantern_riddles;
pub mod install_travel;
pub mod install_view;
pub mod install_vip;
pub mod item;
pub mod login;
pub mod lantern_riddles;
pub mod loginvar;
pub mod mail;
pub mod platform_info;
pub mod pet;
pub mod pray;
pub mod player;
pub mod player_misc;
pub mod player_state;
pub mod push;
pub mod queue;
pub mod rank;
pub mod runtime;
pub mod second_psw;
pub mod shop;
pub mod shortcut;
pub mod sign;
pub mod special_msg;
pub mod system_open;
pub mod task;
pub mod travel;
pub mod vip;
