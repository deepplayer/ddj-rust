//! # ddj-tcp — 传输层
//!
//! 对应 Java：`netty-all-4.1.17`。
//!
//! ## tokio 映射
//! | Netty | tokio |
//! |---|---|
//! | `ChannelPipeline` | `Decoder` + `Encoder` + 任务 |
//! | `IdleStateHandler` | `tokio::time::timeout` |
//! | `ChannelGroup` | `SessionManager` |
//! | `WriteBufferWaterMark` | `mpsc` 有界通道（背压） |
//!
//! ## 子模块
//! - [`listener`]  `TcpListener` + accept 循环 + 连接计数
//! - [`conn`]      单连接读写驱动
//! - [`pipeline`]  `Frame` → `TypedMessage` → handler → 回包

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod conn;
pub mod listener;
pub mod pipeline;

pub use listener::TcpServer;
