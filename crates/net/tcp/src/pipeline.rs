//! 请求管线
//!
//! `Frame` → 解码 protobuf → 守卫 → handler → 编码回包。
//!
//! 对应 Java：Netty `ChannelPipeline` 里串的 `Decoder`/`Encoder`/`Handler`。

use bytes::Bytes;
use ddj_codec::Frame;
use ddj_core::Result;
use ddj_registry::{GuardChain, HandlerCtx, HandlerTable};

/// 处理一帧，返回要写回去的字节（`None` 表示 handler 自己处理了回包）。
///
/// # Errors
/// 守卫拒绝或 handler 失败。
pub async fn dispatch(
    table: &HandlerTable,
    guards: &GuardChain,
    ctx: &HandlerCtx,
    frame: Frame,
) -> Result<Option<Bytes>> {
    guards.run(ctx)?;

    let Some(h) = table.get(frame.cmd_id) else {
        tracing::debug!(cmd_id = frame.cmd_id, "未注册的消息号，忽略");
        return Ok(None);
    };

    h.handle_raw(ctx, frame.body).await
}
