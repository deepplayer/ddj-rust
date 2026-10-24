//! # ddj-runtime — 模块容器与生命周期
//!
//! 对应 Java：`serverModuleListConfig.xml` 驱动的模块装配 + `jery.ngsp` 的
//! `init/start/stop` 钩子。
//!
//! ## 启动阶段（严格顺序）
//! `PreInit` → `Init` → `Start` → `Ready`  （`Stop` 反序）
//!
//! 任一模块 `Init` 失败则**整进程不进入 Start**，避免半启动状态。
//!
//! ## 子模块
//! - [`module`]   `Module` trait + `Phase`
//! - [`registry`] 静态模块注册表（编译期 inventory，无需 XML）
//! - [`lifecycle`] 编排器：拓扑排序 + 超时 + 停机钩子
//! - [`context`]  模块间共享的 `ServiceContext`

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod context;
pub mod lifecycle;
pub mod module;
pub mod registry;

pub use context::ServiceContext;
pub use lifecycle::Lifecycle;
pub use module::{Module, Phase};
