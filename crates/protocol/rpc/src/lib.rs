//! # ddj-rpc — 跨服通信
//!
//! 对应 Java：Hessian（`cross` 4106 / `game` 4544 端口 + `ICrossService` 40+ 方法）。
//!
//! ## 为什么换协议
//! | 方面 | Hessian（原） | 本项目 |
//! |---|---|---|
//! | 跨语言 | 仅 Java 生态 | protobuf + 原帧头 |
//! | 契约 | 反射调用，签名变更不报错 | trait 声明，编译期校验 |
//! | 断线 | 需重启进程 | 连接池自动重连 |
//! | 可观测 | 无 | 每帧带 `from/to` 标签 |
//!
//! ## 子模块
//! - [`wire`]    跨服帧编解码
//! - [`service`] 服务契约 trait
//! - [`client`]  客户端（连接池）
//! - [`server`]  服务端（路由表）

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod client;
pub mod server;
pub mod service;
pub mod wire;

pub use client::RpcClient;
pub use server::RpcServer;
pub use service::{BattleService, CrossService, RemoteService, RpcError, RpcResult};
pub use wire::{RpcEnvelope, ServiceTag};
