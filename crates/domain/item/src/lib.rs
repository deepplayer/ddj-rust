//! # ddj-item — 道具：背包 / 使用 / 消耗 / 合成 / 限次校验
//!
//! 纯领域层：不依赖 tokio 网络、不直接碰 DB（只通过 `ddj-repo` 的 trait）。
//! 所有状态变更通过聚合根方法完成，便于单元测试与不变量校验。
//!
//! ## 子模块
//! - [`model`]     聚合根与值对象
//! - [`service`]   领域服务（跨聚合的编排）
//! - [`event`]     领域事件（用于副作用解耦）
//! - [`rule`]      不变量与业务规则校验

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod event;
pub mod model;
pub mod rule;
pub mod service;
