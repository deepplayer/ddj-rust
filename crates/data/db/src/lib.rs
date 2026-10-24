//! # ddj-db — 数据访问
//!
//! 对应 Java：`mysql-connector-java` + `druid` + `proxool` + `jery.ngsp.db.JDBCRepository`。
//!
//! ## 库划分（源自 `data/`）
//! | 库 | 表数 | 用途 |
//! |---|---|---|
//! | `platform` | 277 | 账号 / 区服 / 充值 / CDK |
//! | `game_7010544` | 173 | 角色 / 道具 / 邮件 / 公会 |
//! | `cross_7010106` | — | 跨服 |
//! | `world7010002` | — | 世界 |
//!
//! ## 子模块
//! - [`pool`]       连接池构建（每库一个 `MySqlPool`）
//! - [`tx`]         事务包装 `UnitOfWork`
//! - [`shard`]      分服路由：`ServerId → 库名`
//! - [`migration`]  内嵌 sqlx migrations

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod migration;
pub mod pool;
pub mod shard;
pub mod tx;

pub use pool::DbPools;
pub use shard::ShardRouter;
pub use tx::UnitOfWork;
