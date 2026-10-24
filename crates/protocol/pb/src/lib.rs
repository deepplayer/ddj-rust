//! # ddj-pb — protobuf 消息层
//!
//! 源文件 `letsproto.proto`（1.75 MB / 8856 message / 27718 field / proto2）。
//!
//! ## 为什么生成物不签入仓库
//! `include!` 一个 4.4 MB 的单文件会让 rustc 峰值内存到 ~900 MB。
//! 本机 1.9 GB 会 OOM（实测两次）。
//! 因此 `build.rs` 按 `pub mod` 把生成物拆成 117 个文件写进 `OUT_DIR`，
//! 只有 crate 本身编译时才展开。
//!
//! ## 命名映射（prost 规则，需注意）
//! | proto | Rust |
//! |---|---|
//! | `ItemUseReq_11002` | `item_message::ItemUseReq11002` |
//! | `MsgUseParameter` | `item_message::MsgUseParameter` |
//!
//! 下划线被消除、message 按 proto 文件内前缀归入 module。

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod cmd;
pub mod dispatch;

#[allow(missing_docs, clippy::all, clippy::pedantic)]
pub mod gen {
    //! `prost-build` 生成物（扁平导出）。
    // 文件名跟 proto 的 `package` 走（源文件是 `package pb;` → `pb.rs`），
    // 不是跟文件名走。`DDJ_PB_STUB=1` 时 build.rs 也写 `pb.rs`。
    include!(concat!(env!("OUT_DIR"), "/pb.rs"));
}

pub use cmd::RetCode;
pub use dispatch::{MessageKind, TypedMessage};
