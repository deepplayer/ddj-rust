//! 流式解码器（`tokio_util::codec::Decoder`）
//!
//! 处理 TCP 粘包 / 半包。逻辑与 Netty 的 `LengthFieldBasedFrameDecoder` 等价，
//! 但长度字段是**包含头部**的绝对值，且魔数需要校验。

use crate::frame::{Frame, FrameError, HEADER_LEN, MAGIC};
use bytes::BytesMut;
use tokio_util::codec::Decoder;

/// 帧解码器。
#[derive(Debug, Default)]
pub struct FrameDecoder {
    /// 单帧允许的最大字节数。
    max_body: usize,
}

impl FrameDecoder {
    /// 用默认上限构造。
    #[must_use]
    pub fn new() -> Self {
        Self {
            max_body: crate::MAX_BODY,
        }
    }

    /// 指定上限。
    #[must_use]
    pub fn with_max_body(max_body: usize) -> Self {
        Self { max_body }
    }
}

impl Decoder for FrameDecoder {
    type Item = Frame;
    type Error = FrameError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.len() < HEADER_LEN {
            src.reserve(HEADER_LEN - src.len());
            return Ok(None);
        }

        // 先窥探头部，绝不 advance —— 半包时数据必须留在缓冲区。
        let magic = u16::from_be_bytes([src[0], src[1]]);
        if magic != MAGIC {
            return Err(FrameError::BadMagic(magic));
        }
        let total = i32::from_be_bytes([src[2], src[3], src[4], src[5]]);

        if total < crate::frame::HEADER_LEN_I32 {
            return Err(FrameError::BadLength(total));
        }
        let need = usize::try_from(total).map_err(|_| FrameError::BadLength(total))?;
        if need - HEADER_LEN > self.max_body {
            return Err(FrameError::BadLength(total));
        }
        if src.len() < need {
            src.reserve(need - src.len());
            return Ok(None);
        }

        let mut slice = src.split_to(need);
        Frame::decode(&mut slice).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REAL: &[u8] = &[
        0x71, 0xAB, 0x00, 0x00, 0x00, 0x15, 0x00, 0x00, 0x2A, 0xFA, 0x08, 0x81, 0xB5, 0x18, 0x10,
        0x03, 0x1A, 0x03, 0x08, 0xB9, 0x60,
    ];

    #[test]
    fn one_shot() {
        let mut d = FrameDecoder::new();
        let mut buf = BytesMut::from(REAL);
        let f = d.decode(&mut buf).unwrap().expect("frame");
        assert_eq!(f.cmd_id, 11002);
        assert!(buf.is_empty());
    }

    #[test]
    fn partial_then_complete() {
        let mut d = FrameDecoder::new();
        let mut buf = BytesMut::new();

        // 逐字节喂入，前 20 次都应返回 None
        for &b in &REAL[..20] {
            buf.extend_from_slice(&[b]);
            assert_eq!(d.decode(&mut buf).unwrap(), None, "不应提前出帧");
        }
        buf.extend_from_slice(&REAL[20..]);
        let f = d.decode(&mut buf).unwrap().expect("frame");
        assert_eq!(f.cmd_id, 11002);
        assert!(buf.is_empty());
    }

    #[test]
    fn sticky_packets() {
        let mut d = FrameDecoder::new();
        let mut buf = BytesMut::new();
        buf.extend_from_slice(REAL);
        buf.extend_from_slice(REAL);

        let f1 = d.decode(&mut buf).unwrap().expect("f1");
        let f2 = d.decode(&mut buf).unwrap().expect("f2");
        assert_eq!(f1.cmd_id, 11002);
        assert_eq!(f2.cmd_id, 11002);
        assert!(buf.is_empty(), "两个包应被完整切分");
    }

    #[test]
    fn rejects_bad_magic() {
        let mut d = FrameDecoder::new();
        let mut buf = BytesMut::from(REAL);
        buf[1] = 0x00;
        assert!(d.decode(&mut buf).is_err());
    }
}
