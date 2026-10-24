//! # ddj-core — 核心基础类型
//!
//! 对应 Java：`bf.util.*` + `jery.ngsp.utility.*`
//!
//! 这一层不含任何游戏业务，也不做 IO。所有上层 crate 都允许依赖它。

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod clock;
pub mod error;
pub mod id;
pub mod result;
pub mod rng;

pub use clock::{Clock, SystemClock, TestClock};
pub use error::{AppError, DomainError, ErrorKind, ServiceError};
pub use id::{GuildId, PlayerId, ServerId, TemplateId};
pub use result::Result;
pub use rng::{DeterministicRng, RngSource};
