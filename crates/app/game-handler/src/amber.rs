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
    AmberVipResp17095, AmberWelfareDataResp17091, MsgAmberWelfareData,
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
