//! # ddj-codec — 帧编解码
//!
//! 严格复刻 `lets.WebPacket`：
//!
//! ```text
//! ┌────────┬──────────────┬──────────────┬──────────────────┐
//! │ int16  │    int32     │    int32     │                  │
//! │ 0x71AB │ total_length │    cmd_id    │   protobuf body  │
//! │  2 B   │    4 B       │    4 B       │   len = total-10 │
//! └────────┴──────────────┴──────────────┴──────────────────┘
//!            大端 (ByteBuffer.DEFAULT_ENDIAN = BIG_ENDIAN)
//! ```
//!
//! `total_length` **包含** 10 字节头，与原客户端 `bf_writeHead_bf` 一致。
//!
//! ## 限制（与原服务端 `NettyServerHandler` 对齐）
//! - `total_length` > [`MAX_BODY`] → 丢弃并断开
//! - magic 不匹配 → 立即 `close()`
//! - 单连接累计消息 > [`MAX_QUEUE`] → `Channel.close()`

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod decoder;
pub mod encoder;
pub mod frame;
pub mod limits;

pub use decoder::FrameDecoder;
pub use encoder::FrameEncoder;
pub use frame::{Frame, FrameError, HEADER_LEN, MAGIC};

/// 一帧一帧解析整块字节（测试 / 一次性解析用）。
///
/// # Errors
/// 帧格式错误，或尾部有残缺数据。
pub fn parse(mut src: bytes::BytesMut) -> Result<Frame, FrameError> {
    Frame::decode(&mut src)
}

/// 单帧最大字节数（原服务端硬编码 100000）。
pub const MAX_BODY: usize = 100_000;
/// 单连接积压消息上限（原服务端硬编码 100）。
pub const MAX_QUEUE: usize = 100;
