//! # ddj-cache — 缓存与锁
//!
//! 对应 Java：`jedis-2.9.0` + `DistributedLock`（`GameReadWriteLockConfig.xml`）。
//!
//! ## 子模块
//! - [`conn`]   `ConnectionManager` 自动重连
//! - [`kv`]     带 TTL 的读写 + 序列化策略
//! - [`lock`]   `SET NX PX` + Lua 释放；带 fencing token
//! - [`rank`]   ZSet 排行榜原语（对应原系统 rank 模块）
//!
//! > 原系统 `GameReadWriteLockConfig.xml` 只有 `lockIsWrite` / `defaultWriteTime`
//! > 两项，说明全局读写锁是"一把大锁 + 超时"。Rust 侧改为**按键分段锁**，
//! > 不同玩家不再互相阻塞。

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod conn;
pub mod kv;
pub mod lock;
pub mod rank;

pub use conn::RedisPool;
pub use lock::DistributedLock;
