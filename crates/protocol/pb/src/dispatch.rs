//! 类型化消息
//!
//! 原 Java 侧靠 `Message.parseFrom(body)` 反射实例化，异常靠 catch。
//! 这里用关联类型把「cmdId → 具体类型」变成编译期事实。

use prost::Message;

/// 一条已解码的消息。
pub struct TypedMessage<M> {
    /// 消息号。
    pub cmd_id: i32,
    /// 解码后的消息体。
    pub msg: M,
}

impl<M: Message + Default> TypedMessage<M> {
    /// 从裸字节解码。
    ///
    /// # Errors
    /// protobuf 格式错误。
    pub fn decode(cmd_id: i32, body: &[u8]) -> Result<Self, prost::DecodeError> {
        Ok(Self {
            cmd_id,
            msg: M::decode(body)?,
        })
    }

    /// 编码成字节。
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        self.msg.encode_to_vec()
    }
}

/// 消息的编解码能力。
pub trait MessageKind: Message + Default + Sized {
    /// 消息号。
    const CMD_ID: i32;
}
