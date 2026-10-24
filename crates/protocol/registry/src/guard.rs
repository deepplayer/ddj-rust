//! 前置守卫
//!
//! 原服务端把「免登录白名单」写死在 `LoginMessageHandler` 里：
//! `msgId ∈ {3,4,0,5,6,7,98,11}` 才放行，其余一律丢弃并打日志
//! 「这条消息需要先登录账号」。
//!
//! 这里改成可组合的守卫链：登录态 / 封禁 / 限速 / 白名单各自独立，
//! 顺序固定，任一拒绝即短路。

use crate::handler::HandlerCtx;
use ddj_core::Result;

/// 守卫决策。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardOutcome {
    /// 放行。
    Pass,
    /// 拒绝，但不断开连接（回错误码）。
    Reject,
    /// 拒绝并立即断开（协议级攻击）。
    Close,
}

/// 一个守卫。
pub trait Guard: Send + Sync {
    /// 守卫名。
    fn name(&self) -> &'static str;
    /// 判定。
    fn check(&self, ctx: &HandlerCtx) -> GuardOutcome;
}

/// 守卫链。
#[derive(Default)]
pub struct GuardChain {
    guards: Vec<Box<dyn Guard>>,
}

impl GuardChain {
    /// 空链。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 追加守卫。
    #[must_use]
    pub fn with(mut self, g: impl Guard + 'static) -> Self {
        self.guards.push(Box::new(g));
        self
    }

    /// 依次执行，短路返回。
    ///
    /// # Errors
    /// 任一守卫拒绝。
    pub fn run(&self, ctx: &HandlerCtx) -> Result<()> {
        for g in &self.guards {
            match g.check(ctx) {
                GuardOutcome::Pass => {}
                GuardOutcome::Reject => {
                    tracing::debug!(guard = g.name(), cmd_id = ctx.cmd_id, "守卫拒绝");
                    return Err(ddj_core::AppError::Unauthorized);
                }
                GuardOutcome::Close => {
                    tracing::warn!(guard = g.name(), cmd_id = ctx.cmd_id, "守卫要求断开");
                    return Err(ddj_core::AppError::BadRequest("协议违规".into()));
                }
            }
        }
        Ok(())
    }
}

/// 登录态守卫。
pub struct LoginGuard;

impl Guard for LoginGuard {
    fn name(&self) -> &'static str {
        "login"
    }
    fn check(&self, ctx: &HandlerCtx) -> GuardOutcome {
        if ctx.player_id.is_some() || crate::FREE_LOGIN_CMDS.contains(&ctx.cmd_id) {
            GuardOutcome::Pass
        } else {
            GuardOutcome::Reject
        }
    }
}
