//! 猜灯谜 37603 / 37604 / 37605 / 37607 的接线。
//!
//! 纯逻辑在 [`crate::lantern_riddles`]，本文件只做解码 → 调纯函数 → 改状态 → 编码回包。
//!
//! ## 四条消息的骨架
//! | cmd | 方法 | 前置 |
//! |---|---|---|
//! | 37603 | `lanternRiddlesGetSubject` | `getActivityDbTemplate(50, actId).isOpen()` |
//! | 37604 | `lanternRiddlesChangeSubject` | 同上 + `ActivitySet.getActivity(actId)` |
//! | 37605 | `lanternRiddlesExcludeOption` | 同上 |
//! | 37607 | `lanternRiddlesGetRwd` | 同上 |
//!
//! 四条在 handler 层的第一段完全一样：
//! ```java
//! ActivityDbTemplate dbTemplate =
//!     ActivityDbTemplateCache.getInstance().getActivityDbTemplate(actType, actId);
//! if (!dbTemplate.isOpen()) throw new HandlerException(LanguageUtils.COMMON_SERVER_BUSY);
//! ```
//!
//! ## 失败出口
//! 全部是 `throws HandlerException` → **`ErrorResp_1`（帧号 1）**：
//! | cmd | 场景 | 码 |
//! |---|---|---|
//! | 全部 | 模板不存在 / 不在开放时间 | `2` `CommonServerBusy` |
//! | 37604 | `id ∉ [1,4]` | `5401` `LanternRiddleDateError` |
//! | 37604/37605 | 等级 < `identityRestriction` | `33` `PlayerLvNo` |
//! | 37604/37605 | `rounds() == 0` | `5400` `LanternRiddleNotOpen` |
//! | 37604 | `lanternId` 不对 | `5402` `LanternRiddleIsReturn` |
//! | 37604 | 该题已 `pick` | `5405` `LanternRiddlePack` |
//! | 37605 | `outOfNum >= 上限` 或 `oneOutOfNum >= 上限` | `5403` `LanternRiddleTroubleshootingMax` |
//! | 37605 | `lanternId` 不对 | `5406` `LanternRiddleExcludeOptionError` |
//! | 37605 | 道具 6181 不够 | `6` `ResourcesNo` |
//! | 37607 | `rounds() != 0`（**还没结束**） | `5404` `LanternRiddleIsNotOver` |
//! | 37607 | `startDay==0 && rounds2==0` | `5400` |
//! | 37607 | 该 lun 已领过 | `5407` `LanternRiddleRwdExpire` |
//!
//! ## 本轮未接的部分（AUDIT A23）
//! - **37606 跨服排名 RPC**（`GetRankCsRemoteReq_37606`）—— 需要 `CsServer`。
//! - `LanternRiddlesDbTemplateCache` 的**跨服拉题**（`ActivityUtils.sendCross`）。
//!   题目表 / 选项表在本进程内注入，查不到就当「没这题」。
//! - `LanternRiddlesActivityCommon`（全服 `state` / `lunPlayerIds` / `playIntrgral`）。
//! - `EventLogLanternRiddle.*`（四条事件日志）。
//! - `sendMails()`（活动结束时按排名补发邮件）。
//! - `ActivityLanternRiddlesModule.sendPlayerData()`。
//!
//! ## 为什么线上永远回 `2`
//! `data/sql/platform.sql` 的 `bf_activity` 里没有 `typeId = 50` 的行，
//! 所以 [`crate::install_activity::template`] 恒 `None` → 四条消息都回 `2`。
//! 这与原版「`getActivityDbTemplate` 返回 null → `null.isOpen()` NPE → 框架兜底」
//! **结果一致**。见 AUDIT A23。

use std::sync::{Arc, OnceLock};

use ddj_core::Clock;
use ddj_pb::gen::lantern_riddles_activity_message::{
    LanternRiddleChangeSubjectReq37604, LanternRiddleChangeSubjectResp37604,
    LanternRiddleExcludeOptionReq37605, LanternRiddleExcludeOptionResp37605,
    LanternRiddleGetRwdReq37607, LanternRiddleGetRwdResp37607, LanternRiddleGetSubjectReq37603,
    LanternRiddleGetSubjectResp37603, MsgLanternRiddleDate, MsgOptionDate,
};
use ddj_pb::RetCode;
use ddj_registry::{HandlerTable, Reply};
use prost::Message;

use crate::lantern_riddles::{
    self, RiddleCfg, RiddleReward, RiddleRewards, RiddleRows, RiddleState, SubjectRow,
    ACT_TYPE_LANTERN_RIDDLES, EXCLUDE_ITEM_ID,
};
use crate::runtime::{decode_err, FnHandler, MemWorld};

/// `ErrorResp_1` 自己的消息号。
const ERROR_RESP_CMD: i32 = 1;

/// 每条消息在 `LanternRiddlesMessageHandler` 里都先取一次的活动类型。
const ACT_TYPE: i32 = ACT_TYPE_LANTERN_RIDDLES;

/// `ConfigDBCache.LanternRiddleLimitNum.get(0)/(1)`。
///
/// ⚠ 线上 `bf_config` 没有 `LanternRiddleLimitNum` 这一行，
/// 反射赋值失败被 `catch (Exception)` 吞掉 → 原版这里 `null.get(0)` 会 **NPE**。
/// 过渡期按「上限为 0」处理（= 一次都不许排错），见 AUDIT A23。
static LIMIT_LANTERN: OnceLock<(i32, i32)> = OnceLock::new();

/// `ConfigDBCache.LanternRiddleLLayTime`。
///
/// ⚠ 同上，线上不存在 → 原版保持默认 `0`。
static LAY_TIME: OnceLock<i32> = OnceLock::new();

/// 题目表 / 选项表注入点。
static ROWS: OnceLock<Box<dyn RiddleRows + Send + Sync>> = OnceLock::new();

/// 奖励表注入点。
static REWARDS: OnceLock<Box<dyn RiddleRewards + Send + Sync>> = OnceLock::new();

/// 活动扩展配置（`(actId, cfg)` 列表，取第一个匹配）。
static CFGS: OnceLock<Vec<(i32, RiddleCfg)>> = OnceLock::new();

/// 注入 `LanternRiddleLimitNum`：`(单题上限, 单轮上限)`。
pub fn install_limit(one: i32, lun: i32) {
    let _ = LIMIT_LANTERN.set((one, lun));
}

/// 注入 `LanternRiddleLLayTime`。
pub fn install_lay_time(t: i32) {
    let _ = LAY_TIME.set(t);
}

/// 注入题目表。
pub fn install_rows(r: Box<dyn RiddleRows + Send + Sync>) {
    let _ = ROWS.set(r);
}

/// 注入奖励表。
pub fn install_rewards(r: Box<dyn RiddleRewards + Send + Sync>) {
    let _ = REWARDS.set(r);
}

/// 注入活动扩展配置。
pub fn install_cfgs(c: Vec<(i32, RiddleCfg)>) {
    let _ = CFGS.set(c);
}

fn limits() -> (i32, i32) {
    // 默认 (0, 0)：原版 `null` 会 NPE，这里取「一次都不许」这个最接近的结果。
    LIMIT_LANTERN.get().copied().unwrap_or((0, 0))
}

fn lay_time() -> i32 {
    LAY_TIME.get().copied().unwrap_or(0)
}

fn cfg_of(_act_id: i32) -> RiddleCfg {
    // 线上没有 typeId=50 的活动，`ActivityLanternRiddlesExtendCA` 也从未构造过。
    // 真接上了就从注入表里取；取不到用默认（全 0）。
    let _ = lay_time();
    CFGS.get()
        .and_then(|v| v.iter().find(|(act, _)| *act == _act_id))
        .map_or_else(RiddleCfg::default, |(_, c)| c.clone())
}

/// 空题目表：`one` 恒 `None`。
struct EmptyRows;

impl RiddleRows for EmptyRows {
    fn one(&self, _act_id: i32, _lantern_id: i32) -> Option<SubjectRow> {
        None
    }
}

/// 空奖励表。
struct EmptyRewards;

impl RiddleRewards for EmptyRewards {
    fn one(&self, _act_id: i32, _num: i32, _rtype: i32) -> Option<RiddleReward> {
        None
    }

    fn list(&self, _act_id: i32, _num: i32, _rtype: i32) -> Vec<RiddleReward> {
        Vec::new()
    }
}

fn rows() -> &'static dyn RiddleRows {
    ROWS.get().map_or(&EmptyRows, |b| b.as_ref())
}

fn rewards() -> &'static dyn RiddleRewards {
    REWARDS.get().map_or(&EmptyRewards, |b| b.as_ref())
}

fn reject(cmd: i32, code: RetCode) -> Reply {
    Reply::with_cmd(
        ERROR_RESP_CMD,
        crate::sign::error_resp(cmd, code).encode_to_vec(),
    )
}

/// `LogicUtils.getTodayZeroTimeStamp()`，东八区。
fn today_zero(now: i64) -> i64 {
    lantern_riddles::zero_ms_of(now)
}

/// 注册 4 条消息，返回注册条数。
pub fn install(
    table: &mut HandlerTable,
    world: Arc<MemWorld>,
    clock: Arc<dyn Clock>,
) -> usize {
    let mut n = 0;

    // ── 37603 取题 ──
    {
        let (w, c) = (world.clone(), clock.clone());
        table.insert(Box::new(FnHandler::new(37603, move |ctx, body| {
            let req = LanternRiddleGetSubjectReq37603::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let act_id = req.act_id.unwrap_or(0);
            let now = c.now_millis();

            let Some(t) = crate::install_activity::template(ACT_TYPE, act_id) else {
                // 原版 `null.isOpen()` NPE → 框架兜底回 2
                return Ok(Some(reject(37603, RetCode::ServerBusy)));
            };
            if !t.is_open(now) {
                return Ok(Some(reject(37603, RetCode::ServerBusy)));
            }
            let start_ms = t.start.unwrap_or(0);
            let cfg = cfg_of(act_id);
            let zero = today_zero(now);
            let state = w.peek(p, |b| {
                b.lantern
                    .get(&act_id)
                    .map_or_else(RiddleState::default, Clone::clone)
            });
            let input = lantern_riddles::SubjectInput {
                act_id,
                now_ms: now,
                zero_ms: zero,
                start_ms,
                cfg: &cfg,
                rows: rows(),
            };
            let out = lantern_riddles::subject_verdict(&state, &input);

            let mut resp = LanternRiddleGetSubjectResp37603 {
                ret: 0,
                true_id: out.true_ids,
                ..LanternRiddleGetSubjectResp37603::default()
            };
            if let Some(q) = out.question {
                resp.lantern_riddle_date = Some(MsgLanternRiddleDate {
                    act_id: Some(q.act_id),
                    lantern_id: Some(q.lantern_id),
                    question: Some(q.question),
                    option: q
                        .options
                        .into_iter()
                        .map(|(id, answer)| MsgOptionDate {
                            id: Some(id),
                            answer: Some(answer),
                        })
                        .collect(),
                });
                resp.id = Some(q.id);
                resp.value = Some(q.value);
                resp.r#use = Some(q.is_pick);
                resp.lun_exclude_option_num = Some(q.lun_exclude_option_num);
                resp.lun_value = Some(q.lun_value);
            }
            Ok(Some(Reply::same(resp.encode_to_vec())))
        })));
        n += 1;
    }

    // ── 37604 答题 ──
    {
        let (w, c) = (world.clone(), clock.clone());
        table.insert(Box::new(FnHandler::new(37604, move |ctx, body| {
            let req = LanternRiddleChangeSubjectReq37604::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let act_id = req.act_id.unwrap_or(0);
            let now = c.now_millis();

            let Some(t) = crate::install_activity::template(ACT_TYPE, act_id) else {
                return Ok(Some(reject(37604, RetCode::ServerBusy)));
            };
            if !t.is_open(now) {
                return Ok(Some(reject(37604, RetCode::ServerBusy)));
            }
            let start_ms = t.start.unwrap_or(0);
            let cfg = cfg_of(act_id);
            let zero = today_zero(now);
            let lantern_id = req.lantern_id.unwrap_or(0);
            let id = req.id.unwrap_or(0);
            let lv = w.peek(p, |b| b.level);
            let row = rows().one(act_id, lantern_id);
            let true_answer = row.as_ref().map_or(0, SubjectRow::true_answer);

            let mut st = w.peek(p, |b| {
                b.lantern
                    .get(&act_id)
                    .map_or_else(|| RiddleState::new(now), Clone::clone)
            });
            let input = lantern_riddles::ChangeInput {
                act_id,
                id,
                lantern_id,
                player_lv: lv,
                now_ms: now,
                zero_ms: zero,
                start_ms,
                cfg: &cfg,
                true_answer,
            };
            match lantern_riddles::change_verdict(&mut st, &input) {
                Ok(out) => {
                    w.with(p, |b| {
                        b.lantern.insert(act_id, st);
                    });
                    Ok(Some(Reply::same(
                        LanternRiddleChangeSubjectResp37604 {
                            ret: 0,
                            value: Some(out.value),
                        }
                        .encode_to_vec(),
                    )))
                }
                Err(code) => {
                    // 失败时不写状态（原版在 `save()` 之后才抛的两个分支是
                    // `common.state != 0` 和「已 pick」—— 前者本模块未接，
                    // 后者在纯函数里先判，所以这里不回滚是对的）
                    Ok(Some(reject(37604, code_of(code))))
                }
            }
        })));
        n += 1;
    }

    // ── 37605 排错 ──
    {
        let (w, c) = (world.clone(), clock.clone());
        table.insert(Box::new(FnHandler::new(37605, move |ctx, body| {
            let req = LanternRiddleExcludeOptionReq37605::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let act_id = req.act_id.unwrap_or(0);
            let now = c.now_millis();

            let Some(t) = crate::install_activity::template(ACT_TYPE, act_id) else {
                return Ok(Some(reject(37605, RetCode::ServerBusy)));
            };
            if !t.is_open(now) {
                return Ok(Some(reject(37605, RetCode::ServerBusy)));
            }
            let start_ms = t.start.unwrap_or(0);
            let cfg = cfg_of(act_id);
            let zero = today_zero(now);
            let lantern_id = req.lantern_id.unwrap_or(0);
            let lv = w.peek(p, |b| b.level);
            let item_num = w.peek(p, |b| b.bag.get(&EXCLUDE_ITEM_ID).copied().unwrap_or(0));
            let (limit_one, limit_lun) = limits();

            let mut st = w.peek(p, |b| {
                b.lantern
                    .get(&act_id)
                    .map_or_else(|| RiddleState::new(now), Clone::clone)
            });
            let input = lantern_riddles::ExcludeInput {
                player_lv: lv,
                lantern_id,
                now_ms: now,
                zero_ms: zero,
                start_ms,
                cfg: &cfg,
                limit_lun,
                limit_one,
                item_num,
            };
            match lantern_riddles::exclude_verdict(&mut st, &input) {
                Ok(_) => {
                    w.with(p, |b| {
                        // `ItemServerModule.removeItem(player, 6181, 1, ...)`
                        if let Some(v) = b.bag.get_mut(&EXCLUDE_ITEM_ID) {
                            *v = (*v - 1).max(0);
                        }
                        b.lantern.insert(act_id, st);
                    });
                    Ok(Some(Reply::same(
                        LanternRiddleExcludeOptionResp37605 { ret: 0 }.encode_to_vec(),
                    )))
                }
                Err(code) => Ok(Some(reject(37605, code_of(code)))),
            }
        })));
        n += 1;
    }

    // ── 37607 领奖 ──
    {
        let (w, c) = (world.clone(), clock.clone());
        table.insert(Box::new(FnHandler::new(37607, move |ctx, body| {
            let req = LanternRiddleGetRwdReq37607::decode(body).map_err(decode_err)?;
            let p = ctx.require_player()?;
            let act_id = req.act_id.unwrap_or(0);
            let now = c.now_millis();

            let Some(t) = crate::install_activity::template(ACT_TYPE, act_id) else {
                return Ok(Some(reject(37607, RetCode::ServerBusy)));
            };
            if !t.is_open(now) {
                return Ok(Some(reject(37607, RetCode::ServerBusy)));
            }
            let start_ms = t.start.unwrap_or(0);
            let cfg = cfg_of(act_id);
            let zero = today_zero(now);
            let lv = w.peek(p, |b| b.level);

            let mut st = w.peek(p, |b| {
                b.lantern
                    .get(&act_id)
                    .map_or_else(|| RiddleState::new(now), Clone::clone)
            });
            st.refresh(now);
            let input = lantern_riddles::RwdInput {
                act_id,
                player_lv: lv,
                now_ms: now,
                zero_ms: zero,
                start_ms,
                cfg: &cfg,
                rewards: rewards(),
            };
            match lantern_riddles::rwd_verdict(&mut st, &input) {
                Ok(out) => {
                    w.with(p, |b| {
                        for (id, num) in &out.items {
                            if *num == 0 {
                                continue;
                            }
                            *b.bag.entry(*id).or_insert(0) += num;
                        }
                        b.lantern.insert(act_id, st);
                    });
                    Ok(Some(Reply::same(
                        LanternRiddleGetRwdResp37607 {
                            ret: 0,
                            true_num: Some(out.true_num),
                            value: Some(out.value),
                            is_max_value: Some(out.is_max_value),
                            reward_ids: out.reward_ids,
                        }
                        .encode_to_vec(),
                    )))
                }
                Err(code) => Ok(Some(reject(37607, code_of(code)))),
            }
        })));
        n += 1;
    }

    n
}

/// 纯逻辑返回的裸 `i32` 码 → [`RetCode`]。
fn code_of(code: i32) -> RetCode {
    RetCode::from_i32(code).unwrap_or(RetCode::ServerBusy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn act_type_is_fifty() {
        // `TypeActivity.guessLanternRiddles(50, "猜灯谜", ...)`
        assert_eq!(ACT_TYPE, 50);
    }

    #[test]
    fn default_limits_are_zero() {
        // 线上 `LanternRiddleLimitNum` 不存在 → 原版 NPE；这里取 (0, 0)
        assert_eq!(limits(), (0, 0));
    }

    #[test]
    fn code_of_maps_known_codes() {
        assert_eq!(code_of(2), RetCode::ServerBusy);
        assert_eq!(code_of(6), RetCode::ResourcesNo);
        assert_eq!(code_of(33), RetCode::PlayerLvNo);
        assert_eq!(code_of(5400), RetCode::LanternRiddleNotOpen);
        assert_eq!(code_of(5404), RetCode::LanternRiddleIsNotOver);
    }

    #[test]
    fn today_zero_is_midnight_east8() {
        // 2026-01-01T00:00:00+08:00 = 1767196800000
        assert_eq!(today_zero(1_767_196_800_000), 1_767_196_800_000);
        // 同一天 12:00
        assert_eq!(today_zero(1_767_196_800_000 + 43_200_000), 1_767_196_800_000);
        // 前一天 23:59:59.999
        assert_eq!(
            today_zero(1_767_196_800_000 - 1),
            1_767_196_800_000 - 86_400_000
        );
    }

    #[test]
    fn empty_rows_returns_none() {
        assert!(EmptyRows.one(1, 1).is_none());
        assert_eq!(EmptyRows.true_answer(1, 1), 0);
    }
}
