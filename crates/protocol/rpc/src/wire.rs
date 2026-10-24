//! 跨服线协议
//!
//! 对应 Java：Hessian 二进制（`cross` 4106 / `game` 4544 端口）。
//!
//! 为什么不沿用 Hessian：跨语言无成熟实现、Java 对象序列化语义难维护。
//! 这里改用「原 TCP 帧头（0x71AB + len + cmdId）+ protobuf body」，
//! 复用同一套 codec 与消息定义，跨服消息只需新增 proto 即可。

use bytes::{BufMut, Bytes, BytesMut};
use ddj_codec::{Frame, HEADER_LEN, MAGIC};

/// 跨服消息头标记：与客户端帧同 magic，靠端口区分。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RpcEnvelope {
    /// 源服务。
    pub from: ServiceTag,
    /// 目标服务。
    pub to: ServiceTag,
    /// 消息号。
    pub cmd_id: i32,
}

/// 服务标识。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum ServiceTag {
    /// 平台服。
    Platform = 1,
    /// 游戏服。
    Game = 2,
    /// 跨服。
    Cross = 3,
    /// 大厅。
    Hall = 4,
    /// 战斗。
    Battle = 5,
    /// 世界。
    World = 6,
    /// GM。
    Gm = 7,
}

impl ServiceTag {
    /// 从字节还原。
    #[must_use]
    pub const fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            1 => Self::Platform,
            2 => Self::Game,
            3 => Self::Cross,
            4 => Self::Hall,
            5 => Self::Battle,
            6 => Self::World,
            7 => Self::Gm,
            _ => return None,
        })
    }
}

/// 把跨服消息编码成帧。
///
/// 布局：`[magic:2][len:4][cmd:4][from:1][to:1][body]`
#[must_use]
pub fn encode(env: RpcEnvelope, body: &[u8]) -> Bytes {
    let total = HEADER_LEN + 2 + body.len();
    let mut buf = BytesMut::with_capacity(total);
    buf.put_i16(MAGIC as i16);
    buf.put_i32(total as i32);
    buf.put_i32(env.cmd_id);
    buf.put_u8(env.from as u8);
    buf.put_u8(env.to as u8);
    buf.put_slice(body);
    buf.freeze()
}

/// 从帧还原跨服消息。
///
/// # Errors
/// 帧体不足 2 字节，或服务标识未知。
pub fn decode(frame: &Frame) -> Result<(RpcEnvelope, &[u8]), &'static str> {
    let b = &frame.body;
    if b.len() < 2 {
        return Err("跨服帧体过短");
    }
    let from = ServiceTag::from_u8(b[0]).ok_or("未知源服务")?;
    let to = ServiceTag::from_u8(b[1]).ok_or("未知目标服务")?;
    Ok((
        RpcEnvelope {
            from,
            to,
            cmd_id: frame.cmd_id,
        },
        &b[2..],
    ))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;
    use bytes::BytesMut;

    #[test]
    fn roundtrip() {
        let env = RpcEnvelope {
            from: ServiceTag::Game,
            to: ServiceTag::Cross,
            cmd_id: 21001,
        };
        let raw = encode(env, b"hello");
        let frame = ddj_codec::parse(BytesMut::from(&raw[..])).expect("解析");
        let (got, body) = decode(&frame).expect("解跨服");
        assert_eq!(got, env);
        assert_eq!(body, b"hello");
    }
}
