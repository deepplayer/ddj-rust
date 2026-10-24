//! # ddj-repo — 仓储实现
//!
//! 对应 Java：`bf.database.*`（639 个类）。
//!
//! ## 依赖倒置
//! 领域层只依赖 [`traits`] 里定义的 trait；[`mysql`] 与 [`memory`] 是两个实现。
//! 单元测试用内存实现，跑得比数据库快几个数量级。

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod entity;
pub mod memory;
pub mod mysql;
pub mod traits;

pub use traits::{GuildRepo, ItemRepo, MailRepo, PlayerRepo};
