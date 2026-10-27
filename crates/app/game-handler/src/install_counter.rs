//! 计数器恢复 8001 / 8002 的接线。
//!
//! 对应 Java：
//! - `CounterRecoveryMessageHandler.counterTimeGetReq`（`:17-27`）→ 8001
//! - `CounterRecoveryMessageHandler.counterGetReq`（`:29-39`）→ 8002
//! - `CounterRecoverySet.counterTimeGetReq`（`:124-132`）
//! - `CounterRecoverySet.counterGetReq`（`:134-147`）
//!
//! ## 两条都自己 catch，错误码走 Resp 而不是 `ErrorResp_1`
//! ```java
//! try { ... } catch (Exception e) {
//!     LoggerService.getLogicLog().error(e.getMessage(), e);
//!     MessageUtil.sendToPlayerId(Resp.newBuilder().setRet(COMMON_SERVER_BUSY).build(), ...);
//! }
//! ```
//! 所以**帧号恒为 8001 / 8002**，`ret = 2`（`COMMON_SERVER_BUSY`）。
//!
//! ## ⚠ 8002 有两个「不发包」分支
//! ```java
//! if (allCounterRecovery.containsKey(templateId)) {
//!     if (counterRecoveryMap.containsKey(param1)) { ... send ... }
//!     // ← 没有 else：param1 不存在 → 静默
//! }
//! // ← 没有 else：templateId 不存在 → 静默
//! ```
//! 客户端会超时。这是原版写法，照抄（与 17152 同类）。
//!
//! ## `counterGetReq` 的变长参数
//! ```java
//! if (args.length == 1) templateId = (int) args[0];
//! else if (args.length == 2) { templateId = (int) args[0]; param1 = args[1]; }
//! ```
//! handler 侧只传 `req.getTemplateId()`，所以 `args.length == 1`，
//! **`param1` 恒为 0** —— 意味着只有 `param1 == 0` 的记录查得到（子嗣的查不到）。

use std::sync::{Arc, OnceLock};

use ddj_core::Clock;
use ddj_pb::gen::counter_recovery_message::{
    CounterGetReq8002, CounterGetResp8002, CounterTimeGetReq8001, CounterTimeGetResp8001,
    MsgCounterData,
};
use ddj_pb::RetCode;
use ddj_registry::HandlerTable;
use prost::Message;

use crate::counter::{self, CounterState, CounterTemplate};
use crate::runtime::{decode_err, FnHandler, MemWorld};

/// 东八区偏移（秒）。与原版部署环境 `Asia/Shanghai` 一致。
const TZ_OFFSET_SECS: i32 = 8 * 3600;

/// 计数器模板表（`CounterRecoveryDBCache`）。
pub trait CounterTemplates: Send + Sync {
    /// `CounterRecoveryDBCache.getCounterRecoveryDB(id)`。
    fn get(&self, id: i32) -> Option<CounterTemplate>;
    /// 全部模板 id（`dbInit` 遍历用）。
    fn all_ids(&self) -> Vec<i32>;
    /// `VipDBCache.getVipDB(vipLv).getCounterMax(templateId)`。
    fn counter_max(&self, vip_lv: i32, template_id: i32) -> i64;
}

static TEMPLATES: OnceLock<Arc<dyn CounterTemplates>> = OnceLock::new();

/// 注入计数器模板（只第一次生效）。
pub fn install_templates(t: Arc<dyn CounterTemplates>) {
    let _ = TEMPLATES.set(t);
}

fn templates() -> Option<&'static Arc<dyn CounterTemplates>> {
    TEMPLATES.get()
}

fn tpl_of(id: i32) -> Option<CounterTemplate> {
    templates().and_then(|t| t.get(id))
}

fn max_of(vip_lv: i32, template_id: i32) -> i64 {
    templates().map_or(0, |t| t.counter_max(vip_lv, template_id))
}

/// `CounterRecoverySet.dbInit` 的纯逻辑部分：给缺失的模板建「满值 + 停表」记录。
///
/// ```java
/// for (CounterRecoveryDB db : allCounterRecoveryDBs) {
///     int templateId = db.getId();
///     // ⚠ 跳过 templateId == 5（子嗣），它由 add(templateId, param1) 单独创建
///     if (templateId != 5 && (!containsKey(templateId) || get(templateId).size() <= 0)) {
///         new CounterRecovery(playerId, templateId, 0L, vipDB.getCounterMax(templateId), -1L);
///     }
/// }
/// ```
///
/// ⚠ `new CounterRecovery(pid, tpl, param1, num, time)` —— 第 4 个参数是 `num`，
/// 所以新建的计数器是**满的**，`time = -1`（停表）。
#[must_use]
pub fn init_missing(
    existing: &[(i32, i64)],
    all_ids: &[i32],
    max_of: impl Fn(i32) -> i64,
) -> Vec<(i32, i64, CounterState)> {
    let mut out = Vec::new();
    for &id in all_ids {
        if id == counter::CHILD_ENERGY_TEMPLATE_ID {
            continue;
        }
        if existing.iter().any(|(t, _)| *t == id) {
            continue;
        }
        out.push((id, 0, CounterState::new_full(max_of(id))));
    }
    out
}

/// 把集合状态刷一遍（`CounterRecoverySet.refresh`）。
///
/// 返回需要回写的内容；调用方负责落库。
fn refresh_all(
    set_time: i64,
    counters: &[(i32, CounterState)],
    vip_lv: i32,
    now: i64,
) -> counter::SetRefresh {
    counter::refresh_set(
        set_time,
        counters,
        tpl_of,
        |id| max_of(vip_lv, id),
        now,
        TZ_OFFSET_SECS,
    )
}

/// 注册 2 条消息，返回注册条数。
pub fn install(table: &mut HandlerTable, world: Arc<MemWorld>, clock: Arc<dyn Clock>) -> usize {
    let mut n = 0;

    // 8001 拉全部计数器
    {
        let (w, c) = (world.clone(), clock.clone());
        table.insert(Box::new(FnHandler::new(8001, move |ctx, body| {
            let _req = CounterTimeGetReq8001::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let now = c.now_millis();

            let list = w.with(p, |b| {
                // 1) dbInit：补齐缺失的模板（原版首次访问时做）
                let ids: Vec<i32> = templates().map(|t| t.all_ids()).unwrap_or_default();
                let existing: Vec<(i32, i64)> = b.counter_map.keys().copied().collect();
                let vip_lv = 0; // 过渡期：VIP 等级未落库，按 0 级取上限
                for (id, param1, st) in init_missing(&existing, &ids, |id| max_of(vip_lv, id)) {
                    b.counter_map.entry((id, param1)).or_insert(st);
                }
                if b.counter_set_time == 0 {
                    b.counter_set_time = now;
                }

                // 2) refresh
                let snapshot: Vec<(i32, CounterState)> = b
                    .counter_map
                    .iter()
                    .map(|((t, _), s)| (*t, *s))
                    .collect();
                let out = refresh_all(b.counter_set_time, &snapshot, vip_lv, now);
                if let Some(t) = out.set_time {
                    b.counter_set_time = t;
                }
                for (id, st) in &out.updated {
                    if let Some(slot) = b.counter_map.get_mut(&(*id, 0)) {
                        *slot = *st;
                    }
                }

                // 3) 组装回包（`for (Map counterRecoveryMap : allCounterRecovery.values())`）
                b.counter_map
                    .iter()
                    .filter_map(|((id, _), st)| {
                        let tpl = tpl_of(*id)?;
                        let m = counter::to_counter_data(st, &tpl, max_of(vip_lv, *id));
                        Some(MsgCounterData {
                            template_id: Some(m.template_id),
                            num: Some(m.num),
                            cd_time: Some(m.cd_time),
                        })
                    })
                    .collect::<Vec<_>>()
            });

            Ok(Some(
                CounterTimeGetResp8001 {
                    ret: 0,
                    msg_counter_data: list,
                }
                .encode_to_vec()
                .into(),
            ))
        })));
        n += 1;
    }

    // 8002 查单个计数器
    {
        let (w, c) = (world.clone(), clock.clone());
        table.insert(Box::new(FnHandler::new(8002, move |ctx, body| {
            let req = CounterGetReq8002::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let now = c.now_millis();
            // handler 只传 templateId → 原版 `args.length == 1` → param1 恒 0
            let template_id = req.template_id.unwrap_or(0);
            let param1 = 0i64;

            let found = w.with(p, |b| {
                if !b.counter_map.contains_key(&(template_id, param1)) {
                    return None;
                }
                let vip_lv = 0;
                let snapshot: Vec<(i32, CounterState)> = b
                    .counter_map
                    .iter()
                    .map(|((t, _), s)| (*t, *s))
                    .collect();
                let out = refresh_all(b.counter_set_time, &snapshot, vip_lv, now);
                if let Some(t) = out.set_time {
                    b.counter_set_time = t;
                }
                for (id, st) in &out.updated {
                    if let Some(slot) = b.counter_map.get_mut(&(*id, 0)) {
                        *slot = *st;
                    }
                }
                let st = *b.counter_map.get(&(template_id, param1))?;
                let tpl = tpl_of(template_id)?;
                let m = counter::to_counter_data(&st, &tpl, max_of(vip_lv, template_id));
                Some(MsgCounterData {
                    template_id: Some(m.template_id),
                    num: Some(m.num),
                    cd_time: Some(m.cd_time),
                })
            });

            match found {
                // ⚠ 原版没有 else —— 查不到就**不发包**
                None => Ok(None),
                Some(d) => Ok(Some(
                    CounterGetResp8002 {
                        ret: 0,
                        msg_counter_data: Some(d),
                    }
                    .encode_to_vec()
                    .into(),
                )),
            }
        })));
        n += 1;
    }

    n
}

/// `COMMON_SERVER_BUSY`：catch 分支的 `ret`。
pub const BUSY: RetCode = RetCode::ServerBusy;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_skips_child_template() {
        // templateId == 5 由 add(templateId, param1) 单独创建
        let out = init_missing(&[], &[2, 5, 21], |_| 3);
        assert_eq!(out.len(), 2, "跳过 5");
        assert_eq!(out[0].0, 2);
        assert_eq!(out[1].0, 21);
    }

    #[test]
    fn init_creates_full_and_stopped() {
        let out = init_missing(&[], &[2], |_| 3);
        assert_eq!(out[0].2.num, 3, "新建即满");
        assert_eq!(out[0].2.time, -1, "新建即停表");
        assert_eq!(out[0].1, 0, "param1 = 0");
    }

    #[test]
    fn init_skips_existing() {
        let out = init_missing(&[(2, 0)], &[2, 21], |_| 3);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, 21, "已存在的 2 不再创建");
    }

    #[test]
    fn busy_is_two() {
        assert_eq!(BUSY.as_i32(), 2);
    }
}
