//! 编码器（`tokio_util::codec::Encoder`）

use crate::frame::{Frame, FrameError};
use bytes::BytesMut;
use tokio_util::codec::Encoder;

/// 帧编码器。
#[derive(Debug, Default)]
pub struct FrameEncoder;

impl Encoder<Frame> for FrameEncoder {
    type Error = FrameError;

    fn encode(&mut self, item: Frame, dst: &mut BytesMut) -> Result<(), Self::Error> {
        item.encode(dst);
        Ok(())
    }
}

impl Encoder<()> for FrameEncoder {
    type Error = FrameError;

    fn encode(&mut self, _item: (), _dst: &mut BytesMut) -> Result<(), Self::Error> {
        Ok(())
    }
}
