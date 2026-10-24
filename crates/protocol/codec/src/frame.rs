//! 帧结构
//!
//! 复刻 `lets.WebPacket`：
//!
//! ```text
//! offset  size  field
//!   0      2    magic = 0x71AB (29099)
//!   2      4    total_length（包含这 10 字节头）
//!   6      4    cmd_id
//!  10      N    protobuf body，N = total_length - 10
//! ```
//!
//! 全部**大端**（`ByteBuffer.DEFAULT_ENDIAN = BIG_ENDIAN`）。

use bytes::{Buf, BufMut, Bytes, BytesMut};

/// 帧魔数。
pub const MAGIC: u16 = 0x71AB;
/// 头长度。
pub const HEADER_LEN: usize = 10;
/// 头长度（`usize` → `i32`，协议里用 i32 表达）。
#[allow(clippy::cast_possible_truncation)]
pub const HEADER_LEN_I32: i32 = HEADER_LEN as i32;

/// 一个完整帧。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// 消息号。
    pub cmd_id: i32,
    /// protobuf 载荷。
    pub body: Bytes,
}

impl Frame {
    /// 构造。
    #[must_use]
    pub fn new(cmd_id: i32, body: impl Into<Bytes>) -> Self {
        Self {
            cmd_id,
            body: body.into(),
        }
    }

    /// 整帧字节数。
    #[must_use]
    pub fn total_len(&self) -> i32 {
        #[allow(clippy::cast_possible_truncation)]
        let n = self.body.len() as i32;
        HEADER_LEN_I32 + n
    }

    /// 写入 `dst`（大端）。
    pub fn encode(&self, dst: &mut BytesMut) {
        let total = self.total_len();
        dst.reserve(HEADER_LEN + self.body.len());
        dst.put_u16(MAGIC);
        dst.put_i32(total);
        dst.put_i32(self.cmd_id);
        dst.put_slice(&self.body);
    }

    /// 从已确认完整的 `src` 解析（`src` 长度必须正好等于 `total_length`）。
    ///
    /// # Errors
    /// magic 错、长度不自洽、`src` 字节数不足都会返回错误。
    pub fn decode(src: &mut BytesMut) -> Result<Self, FrameError> {
        if src.len() < HEADER_LEN {
            return Err(FrameError::Incomplete);
        }
        let mut head = &src[..HEADER_LEN];
        let magic = head.get_u16();
        if magic != MAGIC {
            return Err(FrameError::BadMagic(magic));
        }
        let total = head.get_i32();
        let cmd_id = head.get_i32();

        if total < HEADER_LEN_I32 {
            return Err(FrameError::BadLength(total));
        }
        let need = usize::try_from(total).map_err(|_| FrameError::BadLength(total))?;
        if src.len() < need {
            return Err(FrameError::Incomplete);
        }
        src.advance(HEADER_LEN);
        let body = src.split_to(need - HEADER_LEN).freeze();
        Ok(Self { cmd_id, body })
    }
}

/// 帧解析错误。
///
/// 因为 `tokio_util::codec::{Decoder, Encoder}` 要求 `Error: From<std::io::Error>`，
/// 这里带一个 `Io` 变体做兜底（实测只在底层 socket 出错时才会出现）。
#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    /// 数据还没收全。
    #[error("帧数据不完整")]
    Incomplete,
    /// 魔数不匹配 —— 原服务端此时直接 `Channel.close()`。
    #[error("魔数错误: 0x{0:04X}")]
    BadMagic(u16),
    /// 长度字段非法。
    #[error("长度字段非法: {0}")]
    BadLength(i32),
    /// 底层 IO 错误（`tokio_util::codec` 的 trait 约束所需）。
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
}

impl PartialEq for FrameError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Incomplete, Self::Incomplete) => true,
            (Self::BadMagic(a), Self::BadMagic(b)) => a == b,
            (Self::BadLength(a), Self::BadLength(b)) => a == b,
            (Self::Io(a), Self::Io(b)) => a.kind() == b.kind(),
            _ => false,
        }
    }
}

impl Eq for FrameError {}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真实抓包：使用道具 11002
    /// `71ab 00000015 00002afa 08 81b518 10 03 1a 03 08 b960`
    const REAL: &[u8] = &[
        0x71, 0xAB, // magic
        0x00, 0x00, 0x00, 0x15, // total = 21
        0x00, 0x00, 0x2A, 0xFA, // cmd_id = 11002
        0x08, 0x81, 0xB5, 0x18, // field 1 (template_id) = 400001
        0x10, 0x03, // field 2 (num) = 3
        0x1A, 0x03, 0x08, 0xB9, 0x60, // field 3 (use_param) = { talent_id: 12345 }
    ];

    #[test]
    fn decodes_real_capture() {
        let mut buf = BytesMut::from(REAL);
        let f = Frame::decode(&mut buf).expect("decode");
        assert_eq!(f.cmd_id, 11002);
        assert_eq!(f.body.len(), 11);
        assert_eq!(&f.body[..], &REAL[10..]);
        assert!(buf.is_empty(), "整帧应被消耗完");
    }

    #[test]
    fn roundtrip_is_byte_identical() {
        let mut buf = BytesMut::from(REAL);
        let f = Frame::decode(&mut buf).expect("decode");
        let mut out = BytesMut::new();
        f.encode(&mut out);
        assert_eq!(&out[..], REAL, "编码回去必须逐字节一致");
    }

    #[test]
    fn rejects_bad_magic() {
        let mut bad = BytesMut::from(REAL);
        bad[0] = 0x00;
        let mut b = bad;
        assert_eq!(Frame::decode(&mut b), Err(FrameError::BadMagic(0x00AB)));
    }

    #[test]
    fn incomplete_waits() {
        let mut part = BytesMut::from(&REAL[..7]);
        assert_eq!(Frame::decode(&mut part), Err(FrameError::Incomplete));
    }

    #[test]
    fn total_len_matches_header() {
        let f = Frame::new(11002, &REAL[10..]);
        assert_eq!(f.total_len(), 21);
        assert_eq!(
            u32::try_from(REAL.len()).unwrap(),
            u32::try_from(f.total_len()).unwrap()
        );
    }
}
