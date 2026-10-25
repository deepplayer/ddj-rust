//! Handler trait 与请求上下文
//!
//! 对应 Java：`@LabelMessageHandler` / `@LabelMessageMethod` + `AbsMessageHandler`。
//!
//! ## 返回值语义（复刻原框架）
//! - 返回 `Some(resp)` → 框架自动回包
//! - 返回 `None` → handler 自己发包（推送 / 广播场景）

use async_trait::async_trait;
use bytes::Bytes;
use ddj_core::{AppError, Result};
use ddj_pb::dispatch::MessageKind;
use prost::Message;

/// 一次请求的上下文。
/// 会话控制：`protocol` 层不认识 `net/session`，只依赖这个最小接口。
///
/// 登录成功后处理器调 [`SessionControl::bind_player`]，连接任务再去
/// [`HandlerCtx::player_id`] 里读出绑好的角色 —— 无需让协议层反向依赖会话层。
pub trait SessionControl: Send + Sync {
    /// 绑定角色（等价原版 `ChannelPlayer` 建会话）。
    fn bind_player(&self, player: ddj_core::PlayerId);
}

/// 处理上下文。
pub struct HandlerCtx {
    /// 会话 ID。
    pub session_id: u64,
    /// 角色 ID（未登录为 `None`）。
    pub player_id: Option<ddj_core::PlayerId>,
    /// 客户端 IP（用于限流与风控）。
    pub peer_ip: std::net::IpAddr,
    /// 收到的原始 cmdId。
    pub cmd_id: i32,
    /// 会话控制（未接入会话时为 `None`）。
    pub session: Option<std::sync::Arc<dyn SessionControl>>,
}

impl HandlerCtx {
    /// 要求已登录，否则返回 `Unauthorized`。
    ///
    /// # Errors
    /// 未登录。
    pub fn require_player(&self) -> Result<ddj_core::PlayerId> {
        self.player_id.ok_or(AppError::Unauthorized)
    }
}

/// 强类型消息处理器。
///
/// `Req` / `Resp` 都是 protobuf 消息，编解码由框架统一做。
#[async_trait]
pub trait MessageHandler<Req, Resp>: Send + Sync
where
    Req: MessageKind + Send + 'static,
    Resp: Message + Default + Send + 'static,
{
    /// 消息号（必须等于 `Req::CMD_ID`，由 `#[handler]` 宏校验）。
    const CMD_ID: i32 = Req::CMD_ID;

    /// 处理请求。
    async fn handle(&self, ctx: &HandlerCtx, req: Req) -> Result<Option<Resp>>;
}

/// 类型擦除后的处理器（注册表里存这个）。
#[async_trait]
pub trait ErasedHandler: Send + Sync {
    /// 消息号。
    fn cmd_id(&self) -> i32;
    /// 是否要求登录。
    fn requires_login(&self) -> bool {
        true
    }
    /// 处理裸字节，返回裸响应字节。
    async fn handle_raw(&self, ctx: &HandlerCtx, body: Bytes) -> Result<Option<Bytes>>;
}
