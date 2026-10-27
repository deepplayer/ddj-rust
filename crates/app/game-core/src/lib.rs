//! # ddj-game-core — 应用装配层
//!
//! 这一层把下层全部接起来，**6 个可执行服务共用**，每个服务只写自己的 `main.rs`
//! 选择要启动哪些模块。
//!
//! ## 对应关系
//! | 本 crate 模块 | Java 侧 |
//! |---|---|
//! | [`bootstrap`] | `GameServer` / `PlatformServer` / `CrossServer` 的 `main` |
//! | [`modules`]   | `serverModuleListConfig.xml` 列出的模块 |
//! | [`wiring`]    | 手写 DI（原系统靠 XML + 反射） |
//!
//! ## 为什么不用 DI 框架
//! Rust 的 trait object + 显式构造足够清晰，且编译期就能发现缺失依赖。
//! 大型 DI 容器在 Rust 里收益远小于成本。

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod account;
pub mod view_repo;
pub mod bootstrap;
pub mod create_player_repo;
pub mod csv_amber;
pub mod csv_anticheat;
pub mod csv_counter;
pub mod csv_names;
pub mod csv_sign;
pub mod csv_templates;
pub mod gm_backend;
pub mod lifecycle_modules;
pub mod modules;
pub mod player_stores;
pub mod wiring;

pub use ddj_game_handler::divergence;
pub use bootstrap::{Bootstrap, Running, ServiceKind, Stats, StatsSource};
