//! account 处理器（占位）
//!
//! 对应 Java：`AccountMessageHandler`。
//! 骨架阶段只声明模块，具体 cmdId 与业务待填充。

use crate::macros::{Ctx, Res};

/// 占位：统一的「未实现」响应。
///
/// # Errors
/// 未登录。
pub fn not_implemented(ctx: &Ctx) -> Res<Option<NotImpl>> {
    let _ = ctx.require_player()?;
    Ok(Some(NotImpl {
        ret: 501,
        note: "handler 待实现".into(),
    }))
}

/// 占位响应。
#[derive(Debug, Clone, Default)]
pub struct NotImpl {
    /// 501 = 未实现（沿用原服务端的“无此功能”码）。
    pub ret: i32,
    /// 说明。
    pub note: String,
}
