//! 把已移植的处理链路注册进 `HandlerTable`。
//!
//! ## 为什么单独一层
//! 各业务模块（[`crate::login`]、[`crate::item`]、[`crate::sign`] …）都只暴露
//! **无 IO 的纯函数**，依赖通过 trait 注入，这样单测能覆盖全部分支。
//! 但真正的服务端需要把这些纯函数挂到 `cmdId → 处理器` 的表上，这一层负责：
//!
//! 1. 持有运行期依赖（[`login::ServerState`] 等）；
//! 2. 实现 [`ErasedHandler`]，把裸字节翻译成业务函数的入参；
//! 3. 统一把 `AppError` 交给管道层处理（原版对应 `HandlerException` 的出口）。
//!
//! ## 登录为什么不用 `MessageHandler`
//! `LoginReq_4` 的 body 是 **XXTEA + Base64** 密文（见 [`crate::login`]），
//! 直接 `prost::decode` 必定失败。所以登录处理器自己解析 body，
//! 走 [`ErasedHandler`] 而不是类型化的 [`ddj_registry::MessageHandler`]。

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use ddj_core::Result;
use ddj_registry::{ErasedHandler, HandlerCtx, HandlerTable};
use prost::Message;

use crate::login::{self, ServerState};

/// 登录消息号（`LoginMessage.LoginReq_4`）。
pub const CMD_LOGIN: i32 = ddj_pb::cmd::login::LOGIN;

/// 登录（`cv = 4`）。
///
/// 原版：`LoginMessageHandler` → `WorkTaskPlayerSignIn`。
/// 本文档见 [`crate::login`] 的分支表。
pub struct LoginHandler {
    state: ServerState,
}

impl LoginHandler {
    /// 用给定的服务器状态构造。
    #[must_use]
    pub fn new(state: ServerState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl ErasedHandler for LoginHandler {
    fn cmd_id(&self) -> i32 {
        CMD_LOGIN
    }

    /// 登录包必须能在未登录状态下处理 —— 它就是用来建立登录态的。
    fn requires_login(&self) -> bool {
        false
    }

    async fn handle_raw(&self, ctx: &HandlerCtx, body: Bytes) -> Result<Option<Bytes>> {
        let resp = login::handle(ctx, &body, &self.state)?;
        Ok(Some(Bytes::from(resp.encode_to_vec())))
    }
}

/// 把全部已移植的处理器挂到表上。
///
/// 返回实际注册的条数，便于启动日志核对「有多少条链路真正可达」。
///
/// # Errors
/// 目前不会失败；保留 `Result` 是为了后续需要读模板表时不改签名。
pub fn register(table: &mut HandlerTable, state: ServerState) -> Result<usize> {
    let mut n = 0;
    table.insert(Box::new(LoginHandler::new(state)));
    n += 1;
    Ok(n)
}

/// 供 `game-core` 用的便捷包装（内部 `Arc`，避免每个处理器各存一份状态）。
#[must_use]
pub fn login_handler(state: ServerState) -> Arc<dyn ErasedHandler> {
    Arc::new(LoginHandler::new(state))
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use super::*;

    fn ctx() -> HandlerCtx {
        HandlerCtx {
            session_id: 1,
            player_id: None,
            peer_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            cmd_id: CMD_LOGIN,
        }
    }

    #[test]
    fn login_handler_reports_cmd_and_free_login() {
        let h = LoginHandler::new(ServerState::healthy(1));
        assert_eq!(h.cmd_id(), CMD_LOGIN);
        assert!(!h.requires_login(), "登录包必须免登录");
    }

    #[test]
    fn register_counts_login() {
        let mut t = HandlerTable::empty();
        let n = register(&mut t, ServerState::healthy(1)).expect("注册");
        assert_eq!(n, 1);
        assert!(t.get(CMD_LOGIN).is_some(), "登录应已入表");
        assert!(t.get(11002).is_none(), "未移植的链路不应凭空出现");
    }

    #[tokio::test]
    async fn login_handler_rejects_garbage_body() {
        let h = LoginHandler::new(ServerState::healthy(1));
        let err = h
            .handle_raw(&ctx(), Bytes::from_static(b"not-a-login-packet"))
            .await;
        assert!(err.is_err(), "非法密文必须报错而不是回默认响应");
    }
}
