//! # ddj-session — 会话
//!
//! 对应 Java：`ChannelPlayer` + `Player` 的绑定关系，以及 100 条消息积压上限。
//!
//! ## 状态机
//! ```text
//!     Connected ──login──▶ Authenticated ──enterGame──▶ Playing
//!         │                     │                          │
//!         └──timeout────────────┴──kick────────────────────┘
//! ```
//!
//! ## 子模块
//! - [`state`]   `SessionState` 枚举 + 合法迁移表
//! - [`session`] `Session` 结构：id / 连接句柄 / 玩家引用 / 心跳
//! - [`manager`] 全服会话表：在线查询、按玩家踢号、容量限制

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod manager;
pub mod session;
pub mod state;

pub use manager::SessionManager;
pub use session::{Session, SessionClosed};
pub use state::SessionState;
