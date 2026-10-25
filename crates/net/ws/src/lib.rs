//! # ddj-ws — WebSocket 长连
//!
//! 对齐 Java 侧 `jery.ngsp.network.websocket`（`WebSocketServerPipelineInitializer`）。
//!
//! ## 为什么要单独一个 crate
//! 原版 `GameServer` 的 10544 端口**只讲 WebSocket**：用裸 TCP 连上去发我们的帧，
//! 会先收到 HTTP 响应（`bad magic 0x4854` = `"HT"`）。差分测试时踩过这个坑，
//! 所以传输层必须按 WS 实现，不能沿用 `ddj-tcp`。
//!
//! ## 与 `ddj-tcp` 的关系
//! 两者共用同一套「帧 → 守卫 → 处理器 → 回包」管线（[`pipeline`] 由 `ddj-tcp` 提供），
//! 差异只在承载方式与空闲检测。

pub mod codec;
pub mod server;

pub use codec::MessageCodec;
pub use server::WsServer;
