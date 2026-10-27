//! 客户端特约消息 1109 的接线。
//!
//! 对应 `ClientSendSpecialMsgHandler.java:15-24`：
//! ```java
//! String respData = handlerMsg(req.getMsgId(), req.getData());
//! builder.setRet(0);
//! builder.setMsgId(req.getMsgId());
//! if (respData != null) builder.setData(respData);
//! ```
//!
//! 纯逻辑在 [`crate::special_msg`]（恒返回 `None`），这里只做编解码。
//!
//! ## 回包恒为 `{ret=0, msgId=<回显>}`
//! `data` 字段**永不设置**（`respData` 恒为 `null`）。
//! 不做 `ErrorResp_1` —— 原版方法签名虽然 `throws HandlerException`，
//! 但方法体里没有任何 `throw`。

use std::sync::Arc;

use ddj_core::Clock;
use ddj_pb::gen::player_message::{PlayerSendSpecialMsgReq1109, PlayerSendSpecialMsgResp1109};
use ddj_registry::HandlerTable;
use prost::Message;

use crate::runtime::{decode_err, FnHandler, MemWorld};
use crate::special_msg;

/// 注册 1 条消息，返回注册条数。
pub fn install(table: &mut HandlerTable, _world: Arc<MemWorld>, _clock: Arc<dyn Clock>) -> usize {
    table.insert(Box::new(FnHandler::new(1109, move |ctx, body| {
        let req = PlayerSendSpecialMsgReq1109::decode(body).map_err(decode_err)?;
        let _p = ctx.require_player()?;
        let msg_id = req.msg_id.unwrap_or_default();
        let data = req.data.unwrap_or_default();
        let resp = special_msg::handler_msg(&msg_id, &data);
        Ok(Some(
            PlayerSendSpecialMsgResp1109 {
                ret: 0,
                msg_id: Some(msg_id),
                // `if (respData != null) setData(...)` —— 恒不成立
                data: resp,
            }
            .encode_to_vec()
            .into(),
        ))
    })));
    1
}
