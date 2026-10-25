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
    //! `prost-build` 生成物。
    //!
    //! 默认模式（jar 还原的 102 个 proto）下 `build.rs` 会写一个 `gen.rs`，
    //! 里面是按 `package` 展开的嵌套模块，外加
    //! `pub use self::bf::messages::game::engine_message as engine_message;`
    //! 这类**旧路径别名** —— 所以业务代码继续写
    //! `ddj_pb::gen::engine_message::LoginReq4` 即可，不必感知 `bf::messages::game`。
    //!
    //! `DDJ_PB_STUB=1` 时退化为一个占位常量。
    include!(concat!(env!("OUT_DIR"), "/gen.rs"));
}

pub use cmd::RetCode;
pub use dispatch::{MessageKind, TypedMessage};
