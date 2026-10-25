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

    /// 主动下发一帧（等价 `MessageUtil.sendToPlayerId` / `player.sendMessage`）。
    ///
    /// 原版大量处理器是「返回 void + 自己发包」，例如
    /// `ItemServerModule.sendItemUpdate` → `ItemUpdateResp_11004`。
    /// 这些包与「请求的回包」是**两条独立消息**，不能塞进 `Reply`。
    ///
    /// 默认实现不做事：单测与未接会话的场景不需要真实下行。
    fn push(&self, cmd: i32, body: Bytes) {
        let _ = (cmd, body);
    }
}

/// 处理上下文。
#[derive(Clone)]
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
    /// 处理裸字节，返回待回包（`Reply::cmd` 为 `None` 时沿用请求的消息号）。
    async fn handle_raw(&self, ctx: &HandlerCtx, body: Bytes) -> Result<Option<Reply>>;
}

/// 处理器回包。
///
/// ## 为什么要带 `cmd`
/// 原版框架的 `HandlerException` 出口发的是 **`ErrorResp_1`**，帧头消息号是
/// `ErrorResp_1` 自己的 `1`，body 里的 `msgId` 才是触发错误的请求号。
/// 若沿用请求号组帧，客户端会拿该请求的解析器去读 `msgId`（实测把 16003 当成 ret）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    /// `None` = 沿用请求的消息号。
    pub cmd: Option<i32>,
    /// 响应体。
    pub body: Bytes,
}

impl Reply {
    /// 沿用请求消息号。
    pub fn same(body: impl Into<Bytes>) -> Self {
        Self {
            cmd: None,
            body: body.into(),
        }
    }

    /// 换一个消息号（典型：`ErrorResp_1` 用 1）。
    pub fn with_cmd(cmd: i32, body: impl Into<Bytes>) -> Self {
        Self {
            cmd: Some(cmd),
            body: body.into(),
        }
    }
}

impl From<Bytes> for Reply {
    fn from(body: Bytes) -> Self {
        Self::same(body)
    }
}

impl From<Vec<u8>> for Reply {
    fn from(body: Vec<u8>) -> Self {
        Self::same(body)
    }
}
