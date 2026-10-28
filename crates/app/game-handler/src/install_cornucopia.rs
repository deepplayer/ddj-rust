//! # 聚宝盆（Cornucopia）记录查询：38303 / 38304
//!
//! 对应 Java：
//! - `bf/game/server/message/handler/CornucopiaMessageHandler.java`（69 行）
//! - `bf/game/server/module/activity/fun/cornucopia/CornucopiaActPlayer.java`（`:258-269`）
//! - `bf/game/server/module/activity/fun/cornucopia/CornucopiaActCommon.java`（`:40-53`）
//!
//! ## 本轮只做两条「只读回显」
//! | cmd | 原版 | 依赖 |
//! |---|---|---|
//! | 38301 开始寻宝 | `cornucopiaActPlayer.startMap(player)` | `CornucopiaActPlayer` 完整实体 + 掉落 |
//! | 38302 抽奖 | `cornucopiaActPlayer.drawItem(player, pos)` | 同上 + `WeightRandomModelUtils` |
//! | **38303 自己的记录** | `querySelfRecord()` | 只需 `records` 队列 |
//! | **38304 全服最近记录** | `common.queryRecentRecords()` | 只需 `recentRecords` 队列 |
//!
//! 38303 / 38304 的**逻辑是零**：把内存队列逐条 `toMsgCornucopiaLuckRecord()` 后回包。
//! 所以先接这两条，把「活动模板门禁」的真实链路打通；
//! 38301 / 38302 等 `CornucopiaActPlayer` 的 `cells` / `rate` / 掉落全部就位后再做。
//!
//! ## ⚠ 两条的失败出口不同
//! ```java
//! // 38303：getCornucopiaActPlayer(..., false)  ← checkOpen = false！
//! CornucopiaActPlayer p = getCornucopiaActPlayer(playerId, actId, false);
//! return p.querySelfRecord();
//!
//! // 38304：直接查模板
//! ActivityDbTemplate template = ActivityDbTemplateCache...getActivityDbTemplate(actType, actId);
//! if (template == null) throw new HandlerException(ACTIVITY_NO_EXITS);
//! CornucopiaActCommon common = ...getAbsActivityCommon(template);
//! return common.queryRecentRecords();
//! ```
//! **38303 不检查 `isOpen()`** —— 活动结束后仍然能查自己的历史记录。
//! 38304 也不检查 `isOpen()`，只检查模板是否存在。
//! 两条都不走 `ErrorResp_1` 之外的出口：失败都是 `HandlerException` → 帧号 1。
//!
//! 实测 `bf_activity` 里**没有 `typeId = 60` 的行**，所以两条在当前数据下
//! 都走 `ACTIVITY_NO_EXITS(2751)`。

use std::sync::Arc;

use ddj_core::Clock;
use ddj_pb::gen::cornucopia_act_message::{
    CornucopiaRecordReq38304, CornucopiaRecordResp38304, CornucopiaSelfRecordReq38303,
    CornucopiaSelfRecordResp38303, MsgCornucopiaLuckRecord,
};
use ddj_pb::gen::common_msg_message::MsgResources;
use ddj_pb::RetCode;
use ddj_registry::{HandlerTable, Reply};
use prost::Message;

use crate::runtime::{decode_err, FnHandler, MemWorld};

/// `TypeActivity.Cornucopia`（`TypeActivity.java:2949`）。
pub const TYPE_CORNUCOPIA: i32 = 60;

/// `ErrorResp_1` 自己的消息号。
const ERROR_RESP_CMD: i32 = 1;

/// 一条抽奖记录（`CornucopiaRecord` 的可见部分）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LuckRecord {
    /// 玩家名。
    pub player_name: String,
    /// 抽中的道具 id。
    pub item_id: i32,
    /// 数量。
    pub num: i64,
}

impl LuckRecord {
    /// `CornucopiaRecord.toMsgCornucopiaLuckRecord()`。
    #[must_use]
    pub fn to_msg(&self) -> MsgCornucopiaLuckRecord {
        MsgCornucopiaLuckRecord {
            player_name: Some(self.player_name.clone()),
            resource: Some(MsgResources {
                id: Some(self.item_id),
                num: Some(self.num.to_string()),
                multiple: None,
            }),
        }
    }
}

fn reject(cmd: i32, code: RetCode) -> Reply {
    Reply::with_cmd(
        ERROR_RESP_CMD,
        crate::sign::error_resp(cmd, code).encode_to_vec(),
    )
}

/// 查询模板是否可达（38303 / 38304 共用）。
///
/// 原版 38303 走 `ActivitySet.getActivity(actId)`，38304 走
/// `ActivityDbTemplateCache.getActivityDbTemplate(actType, actId)`；
/// 两者查不到时都是 `ACTIVITY_NO_EXITS`。
fn template_of(act_id: i32) -> Option<&'static crate::activity_template::ActivityTemplate> {
    crate::install_activity::template(TYPE_CORNUCOPIA, act_id)
}

/// 注册 38303 / 38304，返回注册条数。
pub fn install(table: &mut HandlerTable, world: Arc<MemWorld>, clock: Arc<dyn Clock>) -> usize {
    let mut n = 0;

    // 38303 自己的记录。
    {
        let w = world.clone();
        table.insert(Box::new(FnHandler::new(38303, move |ctx, body| {
            let req = CornucopiaSelfRecordReq38303::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let act_id = req.activity_id.unwrap_or(0);

            // ⚠ 原版 `getCornucopiaActPlayer(..., false)` —— **不检查 isOpen**
            if template_of(act_id).is_none() {
                return Ok(Some(reject(38303, RetCode::ActivityNoExits)));
            }
            let records = w.peek(p, |b| b.cornucopia_records.clone());
            let resp = CornucopiaSelfRecordResp38303 {
                ret: 0,
                records: records.iter().map(LuckRecord::to_msg).collect(),
            };
            Ok(Some(Reply::same(resp.encode_to_vec())))
        })));
        n += 1;
    }

    // 38304 全服最近记录。
    {
        let _ = &clock;
        table.insert(Box::new(FnHandler::new(38304, move |ctx, body| {
            let req = CornucopiaRecordReq38304::decode(body).map_err(decode_err)?;
            ctx.require_player()?;
            let act_id = req.activity_id.unwrap_or(0);

            // ⚠ 原版只查模板是否存在，**不检查 isOpen**
            // 也没有 `activity` 实例的判空 —— `getAbsActivityCommon(template)`
            // 在 common 未注册时返回 null → 后续 NPE → 被外层 catch 成 2751。
            // 这里等价成 `ACTIVITY_NO_EXITS`。
            if template_of(act_id).is_none() {
                return Ok(Some(reject(38304, RetCode::ActivityNoExits)));
            }
            // 全服记录是 `CornucopiaActCommon.recentRecords`，跨玩家共享。
            // 过渡期没有公共模块 → 空列表（原版未开始活动时也是空）。
            let resp = CornucopiaRecordResp38304 {
                ret: 0,
                records: Vec::new(),
            };
            Ok(Some(Reply::same(resp.encode_to_vec())))
        })));
        n += 1;
    }

    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_id_matches_java() {
        // `TypeActivity.java:2949` → `Cornucopia(60, "聚宝盆", ...)`
        assert_eq!(TYPE_CORNUCOPIA, 60);
    }

    #[test]
    fn luck_record_maps_fields() {
        let r = LuckRecord {
            player_name: "张三".to_owned(),
            item_id: 203,
            num: 5,
        };
        let m = r.to_msg();
        assert_eq!(m.player_name.as_deref(), Some("张三"));
        let res = m.resource.expect("resource");
        assert_eq!(res.id, Some(203));
        assert_eq!(res.num.as_deref(), Some("5"), "num 是 string 类型");
    }

    #[test]
    fn empty_cache_is_unreachable() {
        // 未注入 → 空表 → 38303/38304 都走 ACTIVITY_NO_EXITS
        assert!(template_of(999).is_none());
    }
}
