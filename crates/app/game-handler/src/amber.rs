//! # 琥珀会员（`AmberWelfareModule`：17091-17097）
//!
//! 对应 Java：`bf/game/server/module/sysActivity/module/AmberWelfareModule.class`
//! 与 `SysActivityMessage$AmberWelfare*`。
//!
//! ## 这一层只是门禁
//! `AmberWelfareMessageHandler` 的 7 个方法全部 9-13 B，只做转发；真正的门禁在
//! `AmberWelfareModule`，而门禁只有一个条件：**玩家 `cid` 必须在
//! `AmberWelfareDBCache.getOppoCids()` 里**。
//!
//! ```text
//! getAmberWelfareModel(pid):
//!     p = PlayerServerModule.getPlayerById(pid)      // 原版不判空 → NPE
//!     cid = p.getCid()
//!     if (AmberWelfareDBCache.getCache().getOppoCids().contains(cid))
//!         return SysActivityModule.getSysSysActivitySet(pid)
//!                    .getSysActivity(TYPE_AMBER_WELFARE, AmberWelfareModel.class)
//!     return null
//! ```
//!
//! 拿不到 model 时 `query` 不会报错，而是回一份
//! `createNotAmberCidVipData`：带上 `cid`，`amberVip=false`、`heyTapVip=false`，
//! 其余字段全是 proto 默认值（0 / 空）。
//!
//! ## 三个动作共用一个前置判定
//! `isNotAmberVip(model) = model == null || !model.isAmberVipOrHeyTapVip()`，
//! 为真就 `throw HandlerException(AMBER_WELFARE_IS_NOT_VIP = 4701)`。
//! 命中后走框架出口，客户端收到的是
//! `ErrorResp_1{msgId = 17092/17093/17094, errorCode = 4701}`，
//! **不是**对应的业务 `Resp`（见 [`crate::sign::error_resp`]）。
//!
//! ## 17095 是纯异步
//! `syncAmberVip` 把真正的同步丢进 `WorkTaskPoolManager.submitAndExecute`，
//! 自己立刻回 `AmberVipResp_17095{ret=0}`；同步结果靠后续推送回填。
//! 所以这个接口**永远成功**，无论 VIP 校验过不过 —— 与 17092/17093/17094 不同。

use ddj_pb::gen::sys_activity_message::{
    AmberVipResp17095, AmberWelfareChargeGiftResp17094, AmberWelfareDataResp17091,
    AmberWelfareExclusiveResp17093, AmberWelfareSignResp17092, MsgAmberWelfareData,
};
use ddj_pb::RetCode;

/// 17091 查询琥珀福利数据。
pub const CMD_QUERY: i32 = 17091;
/// 17092 琥珀签到。
pub const CMD_SIGN: i32 = 17092;
/// 17093 领专属奖励。
pub const CMD_EXCLUSIVE: i32 = 17093;
/// 17094 领充值礼包。
pub const CMD_CHARGE_GIFT: i32 = 17094;
/// 17095 同步琥珀会员状态（异步）。
pub const CMD_SYNC_VIP: i32 = 17095;

/// `AmberWelfareModel` 下发给客户端的那份快照。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AmberData {
    /// 渠道 id，进门禁用的就是它。
    pub cid: String,
    /// 本轮签到进度。
    pub sign_in_progress: i32,
    /// 签到周期天数。
    pub sign_in_cycle_day: i32,
    /// 签到奖励状态。
    pub sign_reward_status: i32,
    /// 专属奖励状态。
    pub exclusive_reward_status: i32,
    /// 累计充值档位。
    pub charge_num: i32,
    /// 已领的充值礼包 id。
    pub charge_reward_ids: Vec<i32>,
    /// 是否琥珀会员。
    pub amber_vip: bool,
    /// 是否 HeyTap 会员。
    pub hey_tap_vip: bool,
}

impl AmberData {
    /// `AmberWelfareModel.isAmberVipOrHeyTapVip()`：两者任一即视为 VIP。
    #[must_use]
    pub const fn is_amber_vip_or_hey_tap_vip(&self) -> bool {
        self.amber_vip || self.hey_tap_vip
    }

    /// `AmberWelfareModel.toMsgAmberWelfareData()`。
    #[must_use]
    pub fn to_msg(&self) -> MsgAmberWelfareData {
        MsgAmberWelfareData {
            cid: Some(self.cid.clone()),
            sign_in_progress: Some(self.sign_in_progress),
            sign_in_cycle_day: Some(self.sign_in_cycle_day),
            sign_reward_status: Some(self.sign_reward_status),
            exclusive_reward_status: Some(self.exclusive_reward_status),
            charge_num: Some(self.charge_num),
            charge_reward_ids: self.charge_reward_ids.clone(),
            amber_vip: Some(self.amber_vip),
            hey_tap_vip: Some(self.hey_tap_vip),
        }
    }
}

/// `AmberWelfareDBCache.getOppoCids().contains(cid)`。
#[must_use]
pub fn is_amber_cid(cid: &str, oppo_cids: &[String]) -> bool {
    oppo_cids.iter().any(|c| c == cid)
}

/// 复刻 `getAmberWelfareModel`：cid 不在白名单就返回 `None`，在名单里再看活
/// 动集合有没有这个 model。
///
/// 调用方必须先确认玩家存在 —— 原版这里直接 `getPlayerById(pid).getCid()`，
/// 玩家不在线就是 NPE，由框架兜成 `ErrorResp_1`。
#[must_use]
pub fn model_of(cid: &str, oppo_cids: &[String], model: Option<AmberData>) -> Option<AmberData> {
    if !is_amber_cid(cid, oppo_cids) {
        return None;
    }
    model
}

/// `createNotAmberCidVipData`：非白名单渠道的兜底数据。
#[must_use]
pub fn not_amber_vip_data(cid: &str) -> MsgAmberWelfareData {
    MsgAmberWelfareData {
        cid: Some(cid.to_owned()),
        sign_in_progress: Some(0),
        sign_in_cycle_day: Some(0),
        sign_reward_status: Some(0),
        exclusive_reward_status: Some(0),
        charge_num: Some(0),
        charge_reward_ids: Vec::new(),
        amber_vip: Some(false),
        hey_tap_vip: Some(false),
    }
}

/// 17091 的响应：`ret` 恒 0，`data` 二选一。
#[must_use]
pub fn query_resp(cid: &str, model: Option<&AmberData>) -> AmberWelfareDataResp17091 {
    let data = model.map_or_else(|| not_amber_vip_data(cid), AmberData::to_msg);
    AmberWelfareDataResp17091 {
        ret: 0,
        data: Some(data),
    }
}

/// `isNotAmberVip(model)` 的取反：只有拿到 model 且确实是 VIP 才放行。
#[must_use]
pub const fn is_amber_vip(model: Option<&AmberData>) -> bool {
    match model {
        Some(m) => m.is_amber_vip_or_hey_tap_vip(),
        None => false,
    }
}

/// 17092/17093/17094 共用的门禁。
///
/// 原版 `throw HandlerException(AMBER_WELFARE_IS_NOT_VIP)`，由框架转成
/// `ErrorResp_1{msgId, errorCode=4701}`。
pub fn check_vip(model: Option<&AmberData>) -> Result<(), RetCode> {
    if is_amber_vip(model) {
        Ok(())
    } else {
        Err(RetCode::AmberWelfareIsNotVip)
    }
}

/// 17095 的响应：提交后台任务后立刻返回，恒 `ret=0`。
#[must_use]
pub const fn sync_vip_resp() -> AmberVipResp17095 {
    AmberVipResp17095 { ret: 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oppo() -> Vec<String> {
        vec!["oppo001".to_owned(), "oppo002".to_owned()]
    }

    fn vip() -> AmberData {
        AmberData {
            cid: "oppo001".to_owned(),
            sign_in_progress: 3,
            sign_in_cycle_day: 7,
            amber_vip: true,
            ..AmberData::default()
        }
    }

    #[test]
    fn oppo_whitelist_matches_exactly() {
        assert!(is_amber_cid("oppo001", &oppo()));
        assert!(!is_amber_cid("oppo003", &oppo()));
        assert!(!is_amber_cid("", &oppo()));
    }

    #[test]
    fn model_is_none_when_cid_not_whitelisted() {
        let m = model_of("other", &oppo(), Some(vip()));
        assert!(m.is_none(), "非白名单渠道连 model 都拿不到");
    }

    #[test]
    fn model_passes_through_when_cid_whitelisted() {
        let m = model_of("oppo001", &oppo(), Some(vip()));
        assert_eq!(m, Some(vip()));
    }

    #[test]
    fn whitelisted_but_missing_activity_yields_none() {
        assert!(model_of("oppo001", &oppo(), None).is_none());
    }

    #[test]
    fn query_without_model_returns_zeroed_payload_with_cid() {
        let resp = query_resp("other", None);
        assert_eq!(resp.ret, 0);
        let data = resp.data.unwrap();
        assert_eq!(data.cid.as_deref(), Some("other"));
        assert_eq!(data.amber_vip, Some(false));
        assert_eq!(data.hey_tap_vip, Some(false));
        assert_eq!(data.sign_in_progress, Some(0));
        assert!(data.charge_reward_ids.is_empty());
    }

    #[test]
    fn query_with_model_copies_all_fields() {
        let m = AmberData {
            cid: "oppo001".to_owned(),
            sign_in_progress: 4,
            sign_in_cycle_day: 7,
            sign_reward_status: 1,
            exclusive_reward_status: 2,
            charge_num: 6,
            charge_reward_ids: vec![1, 2],
            amber_vip: true,
            hey_tap_vip: false,
        };
        let data = query_resp("oppo001", Some(&m)).data.unwrap();
        assert_eq!(data.sign_in_progress, Some(4));
        assert_eq!(data.sign_reward_status, Some(1));
        assert_eq!(data.exclusive_reward_status, Some(2));
        assert_eq!(data.charge_num, Some(6));
        assert_eq!(data.charge_reward_ids, vec![1, 2]);
        assert_eq!(data.amber_vip, Some(true));
        assert_eq!(data.hey_tap_vip, Some(false));
    }

    #[test]
    fn missing_model_is_not_vip() {
        assert!(!is_amber_vip(None));
        assert_eq!(check_vip(None), Err(RetCode::AmberWelfareIsNotVip));
    }

    #[test]
    fn model_present_but_not_vip_is_rejected() {
        let plain = AmberData {
            cid: "oppo001".to_owned(),
            ..AmberData::default()
        };
        assert!(!is_amber_vip(Some(&plain)));
        assert_eq!(check_vip(Some(&plain)), Err(RetCode::AmberWelfareIsNotVip));
    }

    #[test]
    fn amber_vip_alone_passes() {
        assert!(check_vip(Some(&vip())).is_ok());
    }

    #[test]
    fn hey_tap_vip_alone_passes() {
        let hey = AmberData {
            cid: "oppo001".to_owned(),
            hey_tap_vip: true,
            ..AmberData::default()
        };
        assert!(check_vip(Some(&hey)).is_ok());
    }

    #[test]
    fn sync_vip_always_returns_zero() {
        assert_eq!(sync_vip_resp().ret, 0);
    }

    #[test]
    fn error_codes_match_languageutils() {
        assert_eq!(RetCode::AmberWelfareIsNotVip.as_i32(), 4701);
        assert_eq!(RetCode::AmberWelfareSignError.as_i32(), 4702);
        assert_eq!(RetCode::AmberWelfareIsNotExclusiveDay.as_i32(), 4703);
        assert_eq!(RetCode::AmberWelfareExclusiveError.as_i32(), 4704);
        assert_eq!(RetCode::AmberWelfareChargeGiftError.as_i32(), 4705);
        assert_eq!(RetCode::AmberWelfareChargeGiftError2.as_i32(), 4706);
    }
}

// ===========================================================================
// 业务层：签到 / 专属奖励 / 充值豪礼
// ===========================================================================
//
// 对应 Java `AmberWelfareModel`（`bf/game/server/module/sysActivity/model/`）：
// - `signUp(Player)`              223 B → 17092
// - `revExclusiveReward(Player)`  155 B → 17093
// - `revChargeGift(Player, int)`  215 B → 17094
//
// 三者的骨架一致：`lockWrite` → 校验 → 改状态 → `save()` → 发奖 → 组装 Resp
// → 埋点 → `unlockWrite`；`catch` 里只有 `unlockWrite` + 重新抛。
// Rust 侧把「状态」显式入参出厂，「发奖 / save / 埋点」抽象成 trait，
// 因此判定可以脱离框架单测，而副作用顺序仍与原版逐条对齐。

use crate::sign::Reward;

/// `TaskRewardState` 的状态值（`TaskRewardState.<clinit>` 实测）。
pub mod reward_state {
    /// 不可领。
    pub const NO_REWARD: i32 = 0;
    /// 可领未领。
    pub const NOT_RECEIVE: i32 = 1;
    /// 已领取。
    pub const RECEIVED: i32 = 2;
}

/// `TypeModule` 里 17092-17094 用到的三个来源常量。
pub mod amber_source {
    /// 琥珀签到奖励。
    pub const SIGN_REWARD: i32 = 0;
    /// 专属礼包。
    pub const EXCLUSIVE_REWARD: i32 = 0;
    /// 充值豪礼。
    pub const CHARGE_GIFT_REWARD: i32 = 0;
}

/// `AmberWelfareDB` 的一行（`id/cid/type/day/payValue/reward/rewardJson/oppoCids`）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AmberTemplate {
    /// `AmberWelfareDB.id`。
    pub id: i32,
    /// 类型。只有 `type == 3` 的行能当充值豪礼领。
    pub template_type: i32,
    /// 签到第几天（埋点文案用）。
    pub day: i32,
    /// 充值豪礼的门槛（累计充值额）。
    pub pay_value: i32,
    /// `rewardJson`：已解析好的奖励列表。
    pub reward_json: Vec<Reward>,
}

/// 充值豪礼的 `type` 取值（`revChargeGift` 里 `getType() != 3` 直接 BUSY）。
pub const AMBER_TYPE_CHARGE_GIFT: i32 = 3;

/// `AmberWelfareModel` 的持久化状态。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AmberModelState {
    /// 已签到天数。
    pub sign_in_progress: i32,
    /// 上次签到时间（毫秒）。
    pub sign_in_time: i64,
    /// 签到奖励状态（[`reward_state`]）。
    pub sign_reward_status: i32,
    /// 专属奖励状态（[`reward_state`]）。
    pub exclusive_reward_status: i32,
    /// 专属奖励领取时间。
    pub exclusive_time: i64,
    /// 活动期间累计充值额。
    pub charge_num: i32,
    /// 已领取的充值豪礼 id（原版是 `ConcurrentHashSet<Integer>`）。
    pub charge_reward_ids: Vec<i32>,
    /// `AmberWelfareModel.heyTapVip`：17097 校验通过/失败后由 `onBecomeHeyTapVip()`
    /// 或 `onExpireHeyTapVip()` 改写，`toMsgAmberWelfareData()` 原样下发。
    pub hey_tap_vip: bool,
    /// `AmberWelfareModel.amberVip`：**持久化字段，默认 false**。
    ///
    /// 原版只有两条路会置 true：`checkAmberVip()`（查平台 HTTP 的 `vipLevel > 0`）
    /// 与 `onBecomeAmberVip()`。所以新建角色第一次登录时 `amberVip == false`，
    /// 17092/17093/17094 必然走 `AMBER_WELFARE_IS_NOT_VIP(4701)` —— 这是原版行为。
    pub amber_vip: bool,
}

/// 三个动作共同的副作用出口。
pub trait AmberStore {
    /// `save()`。
    fn save(&mut self, state: &AmberModelState);
    /// `ResourcesUtils.addResourcesJsons(player, rewards, type, [])`。
    fn add_resources(&mut self, rewards: &[Reward], source: i32);
    /// `EventLogPropertyAmber.amber_receive(player, kind, dbId, chargeNum, desc, rewards)`。
    ///
    /// `kind`：1 = 签到，2 = 专属，3 = 充值豪礼。
    fn event_log(&mut self, kind: i32, db_id: i32, charge_num: i32, desc: &str, rewards: &[Reward]);
}

/// 模板查找（`AmberWelfareDBCache`）。
pub trait AmberLookup {
    /// `getSignInDbs()`。
    fn sign_in_dbs(&self) -> &[AmberTemplate];
    /// `getExclusiveDb()`。
    fn exclusive_db(&self) -> Option<&AmberTemplate>;
    /// `getById(id)`。
    fn by_id(&self, id: i32) -> Option<&AmberTemplate>;
}

/// 17092 的判定结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignUpOutcome {
    /// 命中 `AMBER_WELFARE_SIGN_ERROR(4702)`。
    Reject(RetCode),
    /// 成功：返回新进度与本次奖励。
    Accept {
        /// 签到后的进度。
        sign_in_progress: i32,
        /// 本次获得的奖励。
        rewards: Vec<Reward>,
    },
}

/// 17093 的判定结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExclusiveOutcome {
    /// 命中 `AMBER_WELFARE_EXCLUSIVE_ERROR(4704)`。
    Reject(RetCode),
    /// 成功：返回新状态与奖励。
    Accept {
        /// 领取后的状态。
        exclusive_reward_status: i32,
        /// 本次获得的奖励。
        rewards: Vec<Reward>,
    },
}

/// 17094 的判定结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChargeGiftOutcome {
    /// 命中 `COMMON_SERVER_BUSY(2)` 或 `AMBER_WELFARE_CHARGE_GIFT_ERROR(4705)` /
    /// `AMBER_WELFARE_CHARGE_GIFT_ERROR2(4706)`。
    Reject(RetCode),
    /// 成功：返回礼包 id 与奖励。
    Accept {
        /// 领取的礼包 id。
        id: i32,
        /// 本次获得的奖励。
        rewards: Vec<Reward>,
    },
}

/// `UtilsTools.isSameDay(a, b)`。
///
/// 原版按本地时区算「同一天」。这里显式传时区偏移，便于测试与多区服。
#[must_use]
pub fn is_same_day(a_ms: i64, b_ms: i64, tz_offset_ms: i64) -> bool {
    const DAY: i64 = 86_400_000;
    let da = (a_ms + tz_offset_ms).div_euclid(DAY);
    let db = (b_ms + tz_offset_ms).div_euclid(DAY);
    da == db
}

/// 17092 签到（`AmberWelfareModel.signUp`）。
///
/// 分支序（字节码偏移）：
/// 1. `isSameDay(now, signInTime) && signRewardStatus == RECEIVED`（12-46）→ 4702
/// 2. `prev = signInProgress`；`signInProgress = prev + 1`（47-60）
/// 3. `signInTime = now`；`signRewardStatus = RECEIVED`（63-75）
/// 4. `save()`（78-79）
/// 5. `db = getSignInDbs().get(prev)`（82-102）—— **用旧进度索引**
/// 6. `rewards = db.getRewardJson()`；发奖（104-121）
/// 7. 埋点 `amber_receive(1, db.getId(), chargeNum, "签到第%s天" % db.getDay(), rewards)`（158-191）
pub fn sign_up<S: AmberStore + ?Sized>(
    state: &AmberModelState,
    now: i64,
    tz_offset_ms: i64,
    lookup: &dyn AmberLookup,
    store: &mut S,
) -> SignUpOutcome {
    if is_same_day(now, state.sign_in_time, tz_offset_ms)
        && state.sign_reward_status == reward_state::RECEIVED
    {
        return SignUpOutcome::Reject(RetCode::AmberWelfareSignError);
    }

    let prev = state.sign_in_progress;
    let mut next = state.clone();
    next.sign_in_progress = prev + 1;
    next.sign_in_time = now;
    next.sign_reward_status = reward_state::RECEIVED;
    store.save(&next);

    // 原版 `List.get(prev)`：越界会抛 IndexOutOfBounds，被外层 catch 兜走。
    // 这里把越界收敛成「空奖励」，但保留调用的先后与索引口径。
    let db = lookup
        .sign_in_dbs()
        .get(usize::try_from(prev).unwrap_or(usize::MAX));
    let (db_id, day, rewards) = match db {
        Some(d) => (d.id, d.day, d.reward_json.clone()),
        None => {
            tracing::warn!(prev, "琥珀签到：进度超出签到模板范围");
            (0, 0, Vec::new())
        }
    };

    store.add_resources(&rewards, amber_source::SIGN_REWARD);
    let desc = format!("签到第{day}天");
    store.event_log(1, db_id, state.charge_num, &desc, &rewards);

    SignUpOutcome::Accept {
        sign_in_progress: next.sign_in_progress,
        rewards,
    }
}

/// 17093 领专属奖励（`AmberWelfareModel.revExclusiveReward`）。
///
/// 分支序：
/// 1. `exclusiveRewardStatus != NOT_RECEIVE`（9-31）→ 4704
/// 2. `exclusiveRewardStatus = RECEIVED`；`exclusiveTime = now`（32-46）
/// 3. `save()`（49-50）
/// 4. `db = getExclusiveDb()`；发奖（53-77）
/// 5. 埋点 `amber_receive(2, db.getId(), chargeNum, "专属礼包", rewards)`（110-125）
pub fn rev_exclusive_reward<S: AmberStore + ?Sized>(
    state: &AmberModelState,
    now: i64,
    lookup: &dyn AmberLookup,
    store: &mut S,
) -> ExclusiveOutcome {
    if state.exclusive_reward_status != reward_state::NOT_RECEIVE {
        return ExclusiveOutcome::Reject(RetCode::AmberWelfareExclusiveError);
    }

    let mut next = state.clone();
    next.exclusive_reward_status = reward_state::RECEIVED;
    next.exclusive_time = now;
    store.save(&next);

    let (db_id, rewards) = match lookup.exclusive_db() {
        Some(d) => (d.id, d.reward_json.clone()),
        None => {
            tracing::warn!("琥珀专属奖励模板缺失");
            (0, Vec::new())
        }
    };

    store.add_resources(&rewards, amber_source::EXCLUSIVE_REWARD);
    store.event_log(2, db_id, state.charge_num, "专属礼包", &rewards);

    ExclusiveOutcome::Accept {
        exclusive_reward_status: next.exclusive_reward_status,
        rewards,
    }
}

/// 17094 领充值豪礼（`AmberWelfareModel.revChargeGift`）。
///
/// 分支序（注意前两步在 `lockWrite` **之前**）：
/// 1. `db = getById(id)`；`db == null || db.getType() != 3`（0-30）→ BUSY(2)
/// 2. `chargeNum < db.getPayValue()`（39-60）→ 4705
/// 3. `chargeRewardIds.contains(id)`（61-85）→ 4706
/// 4. `chargeRewardIds.add(id)`；`save()`（86-99）
/// 5. 发奖（102-121）
/// 6. 埋点 `amber_receive(3, db.getId(), chargeNum, "活动期间充值金额累计达到%d元" % payValue, rewards)`
pub fn rev_charge_gift<S: AmberStore + ?Sized>(
    state: &AmberModelState,
    id: i32,
    now: i64,
    lookup: &dyn AmberLookup,
    store: &mut S,
) -> ChargeGiftOutcome {
    let _ = now;

    let Some(db) = lookup.by_id(id) else {
        return ChargeGiftOutcome::Reject(RetCode::ServerBusy);
    };
    if db.template_type != AMBER_TYPE_CHARGE_GIFT {
        return ChargeGiftOutcome::Reject(RetCode::ServerBusy);
    }

    if state.charge_num < db.pay_value {
        return ChargeGiftOutcome::Reject(RetCode::AmberWelfareChargeGiftError);
    }
    if state.charge_reward_ids.contains(&id) {
        return ChargeGiftOutcome::Reject(RetCode::AmberWelfareChargeGiftError2);
    }

    let mut next = state.clone();
    next.charge_reward_ids.push(id);
    store.save(&next);

    let rewards = db.reward_json.clone();
    store.add_resources(&rewards, amber_source::CHARGE_GIFT_REWARD);
    let desc = format!("活动期间充值金额累计达到{}元", db.pay_value);
    store.event_log(3, db.id, state.charge_num, &desc, &rewards);

    ChargeGiftOutcome::Accept { id, rewards }
}

/// 组装 17092 回包。
#[must_use]
pub fn build_sign_resp(outcome: &SignUpOutcome) -> AmberWelfareSignResp17092 {
    let mut resp = AmberWelfareSignResp17092::default();
    match outcome {
        SignUpOutcome::Reject(code) => resp.ret = code.as_i32(),
        SignUpOutcome::Accept {
            sign_in_progress,
            rewards,
        } => {
            resp.ret = 0;
            resp.sign_in_progress = Some(*sign_in_progress);
            resp.rewards = rewards
                .iter()
                .map(|(id, num)| to_msg_resources(*id, *num))
                .collect();
        }
    }
    resp
}

/// 组装 17093 回包。
#[must_use]
pub fn build_exclusive_resp(outcome: &ExclusiveOutcome) -> AmberWelfareExclusiveResp17093 {
    let mut resp = AmberWelfareExclusiveResp17093::default();
    match outcome {
        ExclusiveOutcome::Reject(code) => resp.ret = code.as_i32(),
        ExclusiveOutcome::Accept {
            exclusive_reward_status,
            rewards,
        } => {
            resp.ret = 0;
            resp.exclusive_reward_status = Some(*exclusive_reward_status);
            resp.rewards = rewards
                .iter()
                .map(|(id, num)| to_msg_resources(*id, *num))
                .collect();
        }
    }
    resp
}

/// 组装 17094 回包。
#[must_use]
pub fn build_charge_gift_resp(outcome: &ChargeGiftOutcome) -> AmberWelfareChargeGiftResp17094 {
    let mut resp = AmberWelfareChargeGiftResp17094::default();
    match outcome {
        ChargeGiftOutcome::Reject(code) => resp.ret = code.as_i32(),
        ChargeGiftOutcome::Accept { id, rewards } => {
            resp.ret = 0;
            resp.id = Some(*id);
            resp.rewards = rewards
                .iter()
                .map(|(id, num)| to_msg_resources(*id, *num))
                .collect();
        }
    }
    resp
}

/// `ResourcesUtils.toMsgAwardBuilder` 的单元素版本。
///
/// 原 proto 里资源数量是 **string**（`optional string num = 2;`），不是数字。
#[must_use]
pub fn to_msg_resources(id: i32, num: i64) -> ddj_pb::gen::common_msg_message::MsgResources {
    ddj_pb::gen::common_msg_message::MsgResources {
        id: Some(id),
        num: Some(num.to_string()),
        multiple: None,
    }
}

#[cfg(test)]
mod model_tests {
    use super::*;
    use std::cell::RefCell;

    const TZ: i64 = 8 * 3_600_000;
    const DAY: i64 = 86_400_000;

    /// 记录调用顺序的假 store。
    #[derive(Default)]
    struct FakeStore {
        log: RefCell<Vec<String>>,
        saved: RefCell<Vec<AmberModelState>>,
        resources: RefCell<Vec<(i32, Vec<Reward>)>>,
        events: RefCell<Vec<(i32, i32, i32, String)>>,
    }

    impl AmberStore for FakeStore {
        fn save(&mut self, state: &AmberModelState) {
            self.log.borrow_mut().push("save".into());
            self.saved.borrow_mut().push(state.clone());
        }
        fn add_resources(&mut self, rewards: &[Reward], source: i32) {
            self.log.borrow_mut().push("add".into());
            self.resources.borrow_mut().push((source, rewards.to_vec()));
        }
        fn event_log(&mut self, kind: i32, db_id: i32, charge: i32, desc: &str, _r: &[Reward]) {
            self.log.borrow_mut().push("event".into());
            self.events
                .borrow_mut()
                .push((kind, db_id, charge, desc.to_string()));
        }
    }

    struct FakeLookup {
        sign: Vec<AmberTemplate>,
        exclusive: Option<AmberTemplate>,
        by_id: Vec<AmberTemplate>,
    }

    impl AmberLookup for FakeLookup {
        fn sign_in_dbs(&self) -> &[AmberTemplate] {
            &self.sign
        }
        fn exclusive_db(&self) -> Option<&AmberTemplate> {
            self.exclusive.as_ref()
        }
        fn by_id(&self, id: i32) -> Option<&AmberTemplate> {
            self.by_id.iter().find(|t| t.id == id)
        }
    }

    fn tpl(id: i32, day: i32, pay: i32, rewards: Vec<Reward>) -> AmberTemplate {
        AmberTemplate {
            id,
            template_type: 1,
            day,
            pay_value: pay,
            reward_json: rewards,
        }
    }

    fn lookup() -> FakeLookup {
        FakeLookup {
            sign: vec![
                tpl(11, 1, 0, vec![(1, 100)]),
                tpl(12, 2, 0, vec![(1, 200), (2, 1)]),
                tpl(13, 3, 0, vec![(1, 300)]),
            ],
            exclusive: Some(tpl(90, 0, 0, vec![(3, 5)])),
            by_id: vec![
                AmberTemplate {
                    template_type: 3,
                    ..tpl(31, 0, 100, vec![(4, 1)])
                },
                AmberTemplate {
                    template_type: 2,
                    ..tpl(32, 0, 100, vec![(4, 2)])
                },
            ],
        }
    }

    fn fresh() -> AmberModelState {
        AmberModelState {
            sign_in_progress: 0,
            sign_in_time: 0,
            sign_reward_status: reward_state::NO_REWARD,
            exclusive_reward_status: reward_state::NOT_RECEIVE,
            charge_num: 0,
            ..AmberModelState::default()
        }
    }

    // ---------------- 17092 ----------------

    #[test]
    fn sign_rejects_when_already_signed_today() {
        let mut st = fresh();
        st.sign_in_time = 10 * DAY;
        st.sign_reward_status = reward_state::RECEIVED;
        let mut store = FakeStore::default();
        let out = sign_up(&st, 10 * DAY + 3_600_000, TZ, &lookup(), &mut store);
        assert_eq!(out, SignUpOutcome::Reject(RetCode::AmberWelfareSignError));
        // 被拒时不产生任何副作用
        assert!(store.log.borrow().is_empty());
    }

    #[test]
    fn sign_allows_when_status_not_received_even_same_day() {
        let mut st = fresh();
        st.sign_in_time = 10 * DAY;
        st.sign_reward_status = reward_state::NOT_RECEIVE;
        let mut store = FakeStore::default();
        let out = sign_up(&st, 10 * DAY + 1000, TZ, &lookup(), &mut store);
        assert!(matches!(out, SignUpOutcome::Accept { .. }));
    }

    #[test]
    fn sign_allows_on_new_day_even_if_received() {
        let mut st = fresh();
        st.sign_in_time = 10 * DAY;
        st.sign_reward_status = reward_state::RECEIVED;
        let mut store = FakeStore::default();
        let out = sign_up(&st, 11 * DAY + 1000, TZ, &lookup(), &mut store);
        assert!(matches!(out, SignUpOutcome::Accept { .. }));
    }

    #[test]
    fn sign_increments_progress_and_stamps_time() {
        let mut st = fresh();
        st.sign_in_progress = 1;
        let now = 5 * DAY + 500;
        let mut store = FakeStore::default();
        let _ = sign_up(&st, now, TZ, &lookup(), &mut store);
        let saved = &store.saved.borrow()[0];
        assert_eq!(saved.sign_in_progress, 2);
        assert_eq!(saved.sign_in_time, now);
        assert_eq!(saved.sign_reward_status, reward_state::RECEIVED);
    }

    #[test]
    fn sign_uses_previous_progress_as_template_index() {
        // progress=1 → 取 sign_in_dbs[1]（id=12, 奖励 1x200 + 2x1）
        let mut st = fresh();
        st.sign_in_progress = 1;
        let mut store = FakeStore::default();
        let out = sign_up(&st, 9 * DAY, TZ, &lookup(), &mut store);
        let SignUpOutcome::Accept {
            sign_in_progress,
            rewards,
        } = out
        else {
            panic!("应成功");
        };
        assert_eq!(sign_in_progress, 2);
        assert_eq!(rewards, vec![(1, 200), (2, 1)]);
    }

    #[test]
    fn sign_writes_save_before_granting_rewards() {
        let st = fresh();
        let mut store = FakeStore::default();
        let _ = sign_up(&st, 9 * DAY, TZ, &lookup(), &mut store);
        // 原版顺序：save() → addResourcesJsons → 埋点
        assert_eq!(*store.log.borrow(), vec!["save", "add", "event"]);
    }

    #[test]
    fn sign_event_log_carries_kind_db_id_and_day_text() {
        let mut st = fresh();
        st.sign_in_progress = 2;
        st.charge_num = 66;
        let mut store = FakeStore::default();
        let _ = sign_up(&st, 9 * DAY, TZ, &lookup(), &mut store);
        let ev = &store.events.borrow()[0];
        assert_eq!(ev.0, 1, "kind 固定为 1");
        assert_eq!(ev.1, 13, "dbId 取的是签到模板 id");
        assert_eq!(ev.2, 66, "chargeNum 原样透传");
        assert_eq!(ev.3, "签到第3天");
    }

    #[test]
    fn sign_out_of_range_template_yields_empty_rewards() {
        let mut st = fresh();
        st.sign_in_progress = 99;
        let mut store = FakeStore::default();
        let out = sign_up(&st, 9 * DAY, TZ, &lookup(), &mut store);
        let SignUpOutcome::Accept { rewards, .. } = out else {
            panic!("原版此处抛 IndexOutOfBounds，被外层 catch 兜成 ret=2；Rust 侧收敛为空奖励但仍记录进度");
        };
        assert!(rewards.is_empty());
    }

    #[test]
    fn sign_resp_maps_ret_and_progress() {
        let mut st = fresh();
        st.sign_in_progress = 0;
        let mut store = FakeStore::default();
        let out = sign_up(&st, 9 * DAY, TZ, &lookup(), &mut store);
        let resp = build_sign_resp(&out);
        assert_eq!(resp.ret, 0);
        assert_eq!(resp.sign_in_progress, Some(1));
        assert_eq!(resp.rewards.len(), 1);

        let rejected = build_sign_resp(&SignUpOutcome::Reject(RetCode::AmberWelfareSignError));
        assert_eq!(rejected.ret, 4702);
        assert!(rejected.rewards.is_empty());
    }

    // ---------------- 17093 ----------------

    #[test]
    fn exclusive_rejects_when_not_in_receivable_state() {
        for status in [reward_state::NO_REWARD, reward_state::RECEIVED] {
            let mut st = fresh();
            st.exclusive_reward_status = status;
            let mut store = FakeStore::default();
            let out = rev_exclusive_reward(&st, 1, &lookup(), &mut store);
            assert_eq!(
                out,
                ExclusiveOutcome::Reject(RetCode::AmberWelfareExclusiveError)
            );
            assert!(store.log.borrow().is_empty());
        }
    }

    #[test]
    fn exclusive_accepts_and_stamps_state() {
        let st = fresh(); // NOT_RECEIVE
        let mut store = FakeStore::default();
        let out = rev_exclusive_reward(&st, 777, &lookup(), &mut store);
        let ExclusiveOutcome::Accept {
            exclusive_reward_status,
            rewards,
        } = out
        else {
            panic!("应成功");
        };
        assert_eq!(exclusive_reward_status, reward_state::RECEIVED);
        assert_eq!(rewards, vec![(3, 5)]);
        let saved = &store.saved.borrow()[0];
        assert_eq!(saved.exclusive_time, 777);
        assert_eq!(saved.exclusive_reward_status, reward_state::RECEIVED);
        assert_eq!(*store.log.borrow(), vec!["save", "add", "event"]);
    }

    #[test]
    fn exclusive_event_log_uses_fixed_text() {
        let st = fresh();
        let mut store = FakeStore::default();
        let _ = rev_exclusive_reward(&st, 1, &lookup(), &mut store);
        let ev = &store.events.borrow()[0];
        assert_eq!((ev.0, ev.1, ev.3.as_str()), (2, 90, "专属礼包"));
    }

    // ---------------- 17094 ----------------

    #[test]
    fn charge_gift_rejects_unknown_id_as_busy() {
        let st = fresh();
        let mut store = FakeStore::default();
        let out = rev_charge_gift(&st, 999, 0, &lookup(), &mut store);
        assert_eq!(out, ChargeGiftOutcome::Reject(RetCode::ServerBusy));
    }

    #[test]
    fn charge_gift_rejects_wrong_type_as_busy() {
        // id=32 的 type 是 2，不是 3
        let st = fresh();
        let mut store = FakeStore::default();
        let out = rev_charge_gift(&st, 32, 0, &lookup(), &mut store);
        assert_eq!(out, ChargeGiftOutcome::Reject(RetCode::ServerBusy));
    }

    #[test]
    fn charge_gift_rejects_below_threshold() {
        let mut st = fresh();
        st.charge_num = 99; // 门槛 100
        let mut store = FakeStore::default();
        let out = rev_charge_gift(&st, 31, 0, &lookup(), &mut store);
        assert_eq!(
            out,
            ChargeGiftOutcome::Reject(RetCode::AmberWelfareChargeGiftError)
        );
        assert!(store.log.borrow().is_empty());
    }

    #[test]
    fn charge_gift_boundary_equals_threshold_passes() {
        let mut st = fresh();
        st.charge_num = 100; // 恰好等于门槛：if_icmpge 放行
        let mut store = FakeStore::default();
        let out = rev_charge_gift(&st, 31, 0, &lookup(), &mut store);
        assert!(matches!(out, ChargeGiftOutcome::Accept { .. }));
    }

    #[test]
    fn charge_gift_rejects_already_claimed() {
        let mut st = fresh();
        st.charge_num = 500;
        st.charge_reward_ids = vec![31];
        let mut store = FakeStore::default();
        let out = rev_charge_gift(&st, 31, 0, &lookup(), &mut store);
        assert_eq!(
            out,
            ChargeGiftOutcome::Reject(RetCode::AmberWelfareChargeGiftError2)
        );
        assert!(store.log.borrow().is_empty());
    }

    #[test]
    fn charge_gift_accepts_and_appends_id() {
        let mut st = fresh();
        st.charge_num = 250;
        st.charge_reward_ids = vec![7];
        let mut store = FakeStore::default();
        let out = rev_charge_gift(&st, 31, 0, &lookup(), &mut store);
        let ChargeGiftOutcome::Accept { id, rewards } = out else {
            panic!("应成功");
        };
        assert_eq!(id, 31);
        assert_eq!(rewards, vec![(4, 1)]);
        // 追加而非替换，且保留原有顺序
        assert_eq!(store.saved.borrow()[0].charge_reward_ids, vec![7, 31]);
        assert_eq!(*store.log.borrow(), vec!["save", "add", "event"]);
        let ev = &store.events.borrow()[0];
        assert_eq!((ev.0, ev.1, ev.2), (3, 31, 250));
        assert_eq!(ev.3, "活动期间充值金额累计达到100元");
    }

    #[test]
    fn charge_gift_resp_maps_id_and_rewards() {
        let mut st = fresh();
        st.charge_num = 250;
        let mut store = FakeStore::default();
        let out = rev_charge_gift(&st, 31, 0, &lookup(), &mut store);
        let resp = build_charge_gift_resp(&out);
        assert_eq!(resp.ret, 0);
        assert_eq!(resp.id, Some(31));
        assert_eq!(resp.rewards.len(), 1);
        assert_eq!(resp.rewards[0].num.as_deref(), Some("1"));

        let rejected = build_charge_gift_resp(&ChargeGiftOutcome::Reject(RetCode::ServerBusy));
        assert_eq!(rejected.ret, 2);
    }

    #[test]
    fn is_same_day_respects_timezone() {
        // 第 3 天 23:00Z 与第 4 天 01:00Z：
        // - 按 UTC 判定是两天
        // - 按 +08:00 判定都是第 4 天 07:00 / 09:00，同一天
        let a = 3 * DAY + 23 * 3_600_000;
        let b = a + 2 * 3_600_000;
        assert!(!is_same_day(a, b, 0), "UTC 下应跨天");
        assert!(is_same_day(a, b, TZ), "东八区下应同天");
        assert!(is_same_day(a, a, 0));
    }
}
