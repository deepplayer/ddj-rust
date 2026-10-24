//! 道具处理器
//!
//! 对应 Java：`ItemMessageHandler` —— 本项目的起点就是它里面的 `itemUseReq`。
//!
//! ## 原 Java 的方法清单（节选，共 40+）
//! | cmdId | 方法 | 说明 |
//! |---|---|---|
//! | 11001 | `itemListReq` | 背包列表 |
//! | 11002 | `itemUseReq` | **使用道具**（本次逆向对象） |
//! | 11003 | `itemSellReq` | 出售 |
//! | 11004 | `itemComposeReq` | 合成 |
//! | 11005 | `itemEquipReq` | 装备 |
//! | 11006 | `itemUnEquipReq` | 卸下 |
//!
//! ## 复刻要点（来自 Java 反编译）
//! 校验顺序：`num<=0` → `useLimit` 复查 → 类型非 EFFECTIVE/CHESS_SCORE →
//! `getUseRemoveNum` → `getNum()<removeNum` → 礼包查表 → 代价校验 →
//! `item.use()` → `revmoeResources(TYPE_507)` → `sendItemUpdate` → 回包。

use crate::macros::{Ctx, Res};

/// 使用道具（原 `msgId = 11002`）。
///
/// 正式实现里 `Req`/`Resp` 来自 `ddj-pb` 生成的 `ItemUseReq11002` /
/// `ItemUseResp11002`；骨架阶段先用占位结构以保持编译。
///
/// # Errors
/// 见 [`ddj_item::rule::ItemError`]。
pub fn use_item(ctx: &Ctx, template_id: i32, num: i64) -> Res<Option<UseResp>> {
    let player = ctx.require_player()?;
    if num <= 0 {
        return Ok(Some(UseResp {
            ret: 400,
            left: 0,
            note: "数量非法".into(),
        }));
    }
    tracing::info!(player = %player, template_id, num, "使用道具");
    Ok(Some(UseResp {
        ret: 0,
        left: 0,
        note: "骨架：等待接入 ItemService".into(),
    }))
}

/// 占位响应。
#[derive(Debug, Clone, Default)]
pub struct UseResp {
    /// 0 = 成功，与 Java 的 `ret` 字段一致。
    pub ret: i32,
    /// 使用后剩余数量。
    pub left: i64,
    /// 调试信息。
    pub note: String,
}

/// 背包列表（原 `msgId = 11001`）。
///
/// # Errors
/// 未登录。
pub fn list(ctx: &Ctx) -> Res<Option<ListResp>> {
    let player = ctx.require_player()?;
    tracing::debug!(player = %player, "查询背包");
    Ok(Some(ListResp { items: Vec::new() }))
}

/// 背包项。
#[derive(Debug, Clone)]
pub struct ListItem {
    /// 模板 ID。
    pub template_id: i32,
    /// 数量。
    pub num: i64,
}

/// 列表响应。
#[derive(Debug, Clone, Default)]
pub struct ListResp {
    /// 道具。
    pub items: Vec<ListItem>,
}
