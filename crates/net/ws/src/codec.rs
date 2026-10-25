//! WebSocket 消息 ↔ 协议帧
//!
//! ## 原版行为（`WebSocketMsgDecoder`）
//! 只处理 `BinaryWebSocketFrame`；文本帧原样放行（业务处理器是
//! `SimpleChannelInboundHandler<IoMessage>`，不会收到文本帧）。
//!
//! 一个 WebSocket 二进制消息 = **恰好一个** 协议帧。原版解码器遇到
//! `readableBytes() < msgSize-10` 时直接 `resetReaderIndex()` 丢弃，不跨消息累积。
//! 这里保留一个缓冲区并按 `FrameDecoder` 迭代，比原版更宽容（能吃掉粘连消息），
//! 但不会改变合法输入的结果 —— 见 [`MessageCodec::feed`] 的测试。

use bytes::BytesMut;
use ddj_codec::{Frame, FrameDecoder, FrameError};
use tokio_util::codec::Decoder as _;

/// 把 WebSocket 二进制消息切成协议帧。
pub struct MessageCodec {
    dec: FrameDecoder,
    buf: BytesMut,
}

impl MessageCodec {
    /// 默认上限（100 000 字节，对齐原版） 。
    #[must_use]
    pub fn new() -> Self {
        Self {
            dec: FrameDecoder::new(),
            buf: BytesMut::new(),
        }
    }

    /// 喂入一个二进制消息，把解析出的帧追加到 `out`。
    ///
    /// 返回 `Ok(())` 表示缓冲内已无完整帧（半包会留在缓冲里等下一个消息）。
    ///
    /// # Errors
    /// magic 不对、长度非法、超过上限。
    pub fn feed(&mut self, data: &[u8], out: &mut Vec<Frame>) -> Result<(), FrameError> {
        self.buf.extend_from_slice(data);
        loop {
            match self.dec.decode(&mut self.buf) {
                Ok(Some(f)) => out.push(f),
                Ok(None) => return Ok(()),
                Err(e) => return Err(e),
            }
        }
    }

    /// 缓冲里还有多少字节没成帧（用于日志与断言）。
    #[must_use]
    pub fn pending(&self) -> usize {
        self.buf.len()
    }
}

impl Default for MessageCodec {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod ws_codec_tests {
    use super::*;
    use bytes::BufMut as _;

    /// 构造一个合法帧的字节。
    fn frame_bytes(cmd_id: i32, body: &[u8]) -> Vec<u8> {
        let mut b = BytesMut::new();
        b.put_u16(29099);
        b.put_i32(10 + body.len() as i32);
        b.put_i32(cmd_id);
        b.put_slice(body);
        b.to_vec()
    }

    #[test]
    fn decodes_single_message() {
        let mut c = MessageCodec::new();
        let mut out = Vec::new();
        c.feed(&frame_bytes(11002, &[8, 1]), &mut out).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].cmd_id, 11002);
        assert_eq!(&out[0].body[..], &[8, 1]);
        assert_eq!(c.pending(), 0);
    }

    #[test]
    fn two_sticky_frames_in_one_message() {
        let mut c = MessageCodec::new();
        let mut raw = frame_bytes(4, &[8, 0]);
        raw.extend_from_slice(&frame_bytes(6, &[8, 0]));
        let mut out = Vec::new();
        c.feed(&raw, &mut out).unwrap();
        assert_eq!(out.iter().map(|f| f.cmd_id).collect::<Vec<_>>(), vec![4, 6]);
    }

    #[test]
    fn half_frame_waits_for_next_message() {
        let mut c = MessageCodec::new();
        let full = frame_bytes(4, &[8, 0, 16, 0]);
        let (a, b) = full.split_at(6);
        let mut out = Vec::new();
        c.feed(a, &mut out).unwrap();
        assert!(out.is_empty());
        assert_eq!(c.pending(), 6);
        c.feed(b, &mut out).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(c.pending(), 0);
    }

    #[test]
    fn rejects_bad_magic() {
        let mut c = MessageCodec::new();
        let mut raw = frame_bytes(4, &[]);
        raw[0] = 0x48; // 'H'
        raw[1] = 0x54; // 'T' —— 正是裸 TCP 撞上 HTTP 时的症状
        let mut out = Vec::new();
        assert!(c.feed(&raw, &mut out).is_err());
    }

    #[test]
    fn rejects_oversize_body() {
        let mut c = MessageCodec::new();
        let mut b = BytesMut::new();
        b.put_u16(29099);
        b.put_i32(100_001 + 10);
        b.put_i32(4);
        let mut out = Vec::new();
        assert!(c.feed(&b, &mut out).is_err());
    }
}
