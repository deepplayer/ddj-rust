//! # ddj-telemetry — 可观测性三件套
//!
//! 对应 Java：`log4j` + `slf4j`（**只有日志**）。
//! Rust 侧一次到位：日志 + 指标 + 追踪 + 审计。

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod audit;
pub mod logging;
pub mod metrics;
pub mod sink;
pub mod trace;

pub use logging::{init, LogFormat};
pub use sink::{set_sink, ConsoleSink};
