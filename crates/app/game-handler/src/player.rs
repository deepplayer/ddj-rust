//! 角色处理器
//!
//! 对应 Java：`PlayerMessageHandler`（等级 / 属性 / 改名 / 头像 / 在线状态）。

use crate::macros::{Ctx, Res};

/// 角色基础信息（原 `msgId = 10001`）。
///
/// # Errors
/// 未登录。
pub fn info(ctx: &Ctx) -> Res<Option<PlayerInfoResp>> {
    let p = ctx.require_player()?;
    Ok(Some(PlayerInfoResp {
        player_id: p.get(),
        level: 1,
        name: String::new(),
        power: 0,
    }))
}

/// 角色信息响应。
#[derive(Debug, Clone, Default)]
pub struct PlayerInfoResp {
    /// ID。
    pub player_id: i64,
    /// 等级。
    pub level: u32,
    /// 昵称。
    pub name: String,
    /// 战力。
    pub power: i64,
}

/// 改名（原 `msgId = 10005`）。
///
/// # Errors
/// 未登录 / 长度非法。
pub fn rename(ctx: &Ctx, new_name: &str) -> Res<Option<RenameResp>> {
    let _p = ctx.require_player()?;
    let n = new_name.chars().count();
    if !(2..=12).contains(&n) {
        return Ok(Some(RenameResp {
            ret: 400,
            note: "名字长度需 2-12 字".into(),
        }));
    }
    Ok(Some(RenameResp {
        ret: 0,
        note: String::new(),
    }))
}

/// 改名响应。
#[derive(Debug, Clone, Default)]
pub struct RenameResp {
    /// 0 = 成功。
    pub ret: i32,
    /// 说明。
    pub note: String,
}
