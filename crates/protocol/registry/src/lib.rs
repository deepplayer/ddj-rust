//! # ddj-registry — 消息路由表
//!
//! 取代 Java 侧的**反射扫描**：原 `MessageHandlerManager.loadHandler()` 扫包、
//! 读注解、从类名后缀 `parseInt` 猜 cmdId。那套机制的问题：cmdId 靠字符串猜、
//! 启动时才暴露冲突、IDE 无法跳转。
//!
//! Rust 侧改为编译期绑定：cmdId 是字面量，冲突 = 编译错误。

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod guard;
pub mod handler;
pub mod table;

pub use guard::{Guard, GuardChain, GuardOutcome, LoginGuard};
pub use handler::{ErasedHandler, HandlerCtx, MessageHandler, Reply, SessionControl};
pub use table::HandlerTable;

/// 免登录 cmdId 白名单 —— 与原服务端完全一致。
pub const FREE_LOGIN_CMDS: &[i32] = &[0, 3, 4, 5, 6, 7, 11, 98];
