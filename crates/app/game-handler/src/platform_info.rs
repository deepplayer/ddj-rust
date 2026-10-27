//! # 平台侧信息（1022 / 1107 / 1110）
//!
//! 对应 Java：`bf/game/server/message/handler/PlayerMessageHandler.java`
//! - `MsgQueryVipWordConfigReq_1022`（行 379-382）
//! - `getWechatOfficialAccountInfoReq_1107`（行 395-411）
//! - `getSwitchPaymentReq_1110`（行 457-470）
//!
//! 三条都只读平台侧配置，**不改玩家状态**，所以纯函数化很自然：
//! 输入是「平台配置快照」，输出是回包。
//!
//! ## 1022
//! ```text
//! return MsgQueryVipWordConfigResp_1022{ret=0, info=PlatformServerModule.getWordConfig().toMsg()}
//! ```
//! 无分支。`GsWordConfig.toMsg()`（`GsWordConfig.java`）逐字段映射，
//! 其中 **`wx` / `qq` 为 null 时不 set**（proto2 下表现为字段缺失），
//! 其余字段无条件 set（null 会变成空串）。
//!
//! ## 1107
//! ```text
//! 1. !isOfficialAccount(channelId, cid)  → COMMON_SERVER_PARAM_ERROR(29)
//! 2. wechatId = getWechatId(channelId)
//! 3. getGsWechatOfficialAccount(wechatId) == null → 29
//! 4. title = wechat, content = content, ret = 0
//! ```
//! `isOfficialAccount`（`PlatformServerModule.java:702-710`）：
//! 渠道号不在表里 → false；表里该渠道的 `cid` 集合为空 **或** 包含玩家 cid → true。
//!
//! ## 1110
//! ```text
//! ret = 0
//! if (switchPayment.size() == 2
//!     && lv >= switchPayment.get(0)
//!     && checkpointId >= switchPayment.get(1))  → state = 1
//! else                                          → state = 0
//! ```
//! 注意 `size() == 2` 是**精确等于**，多一个元素就永远走 else。

use ddj_pb::gen::player_message::{GetWechatOfficialAccountInfoResp1107, MsgVipWordConfig};

/// `GsWordConfig` 的字段快照（对应 `bf_word_config` 表）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WordConfig {
    /// `title`：界面标题。
    pub title: String,
    /// `content`：文本内容。
    pub content: String,
    /// `tips`：文本提示。
    pub tips: String,
    /// `wechat_name`：微信客服名称。
    pub wechat_name: String,
    /// `wechat`：微信号。`None` 表示数据库里是 null（不下发该字段）。
    pub wechat: Option<String>,
    /// `qq_name`：QQ 客服名称。
    pub qq_name: String,
    /// `qq`：QQ 号。`None` 表示数据库里是 null（不下发该字段）。
    pub qq: Option<String>,
    /// `reward_title`：奖励标题。
    pub reward_title: String,
}

/// `GsWordConfig.toMsg()`：逐字段映射。
///
/// 三个字段无条件 set（`title`/`content`/`tips`/`wxTitile`/`qqTitile`/`reward`），
/// 只有 `wx` / `qq` 判空 —— 这是原版写法，照抄。
#[must_use]
pub fn word_config_to_msg(cfg: &WordConfig) -> MsgVipWordConfig {
    MsgVipWordConfig {
        title: Some(cfg.title.clone()),
        content: Some(cfg.content.clone()),
        tips: Some(cfg.tips.clone()),
        wx_titile: Some(cfg.wechat_name.clone()),
        wx: cfg.wechat.clone(),
        qq_titile: Some(cfg.qq_name.clone()),
        qq: cfg.qq.clone(),
        reward: Some(cfg.reward_title.clone()),
    }
}

/// `wechatConfig`（`PlatformServerModule` 内部类）：渠道 → 公众号配置。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WechatChannelConfig {
    /// `cid`：允许的渠道号集合。**空集合表示该渠道下所有 cid 都允许**。
    pub cids: Vec<String>,
    /// `wechatId`：指向 `bf_wechat_official_account` 的主键。
    pub wechat_id: i32,
}

/// `GsWechatOfficialAccount`（`bf_wechat_official_account` 表）的推送子集。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WechatOfficialAccount {
    /// `wechat`：公众号。
    pub wechat: String,
    /// `content`：公众号介绍。
    pub content: String,
}

/// `PlatformServerModule.isOfficialAccount(channelId, cid)`（行 702-710）。
///
/// ```java
/// if (!wechatOfficialAccount.containsKey(channelId)) return false;
/// if (cfg.cid.size() <= 0 || cfg.cid.contains(cid)) return true;
/// return false;
/// ```
#[must_use]
pub fn is_official_account(cfg: Option<&WechatChannelConfig>, cid: &str) -> bool {
    match cfg {
        None => false,
        Some(c) => c.cids.is_empty() || c.cids.iter().any(|x| x == cid),
    }
}

/// 1107 的判定结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WechatInfoOutcome {
    /// 回 `getWechatOfficialAccountInfoResp_1107{ret=29}`。
    ParamError,
    /// 回 `{ret=0, title, content}`。
    Accept {
        /// `gsWechatOfficialAccount.getWechat()`。
        title: String,
        /// `gsWechatOfficialAccount.getContent()`。
        content: String,
    },
}

/// `getWechatOfficialAccountInfoReq_1107` 的纯逻辑。
///
/// `channel_cfg` 是 `getWechatId(channelId)` 与 `isOfficialAccount` 共用的那条渠道配置；
/// `account` 是按 `wechatId` 查到的公众号记录。
#[must_use]
pub fn wechat_info_outcome(
    channel_cfg: Option<&WechatChannelConfig>,
    cid: &str,
    account: Option<&WechatOfficialAccount>,
) -> WechatInfoOutcome {
    // 1. isOfficialAccount(channelId, cid) 为假 → 参数错误
    if !is_official_account(channel_cfg, cid) {
        return WechatInfoOutcome::ParamError;
    }
    // 2/3. getGsWechatOfficialAccount(wechatId) 为 null → 参数错误
    let Some(acc) = account else {
        return WechatInfoOutcome::ParamError;
    };
    WechatInfoOutcome::Accept {
        title: acc.wechat.clone(),
        content: acc.content.clone(),
    }
}

/// 把 1107 的结论转成回包。
#[must_use]
pub fn build_wechat_resp(out: &WechatInfoOutcome) -> GetWechatOfficialAccountInfoResp1107 {
    match out {
        WechatInfoOutcome::ParamError => GetWechatOfficialAccountInfoResp1107 {
            ret: ddj_pb::cmd::RetCode::ParamError.as_i32(),
            title: None,
            content: None,
        },
        WechatInfoOutcome::Accept { title, content } => GetWechatOfficialAccountInfoResp1107 {
            ret: 0,
            title: Some(title.clone()),
            content: Some(content.clone()),
        },
    }
}

/// `getSwitchPaymentReq_1110` 的纯逻辑：返回 `state`（0 或 1）。
///
/// `switch_payment` 是 `PlatformServerModule.switchPayment`（`List<Integer>`），
/// 元素依次是 `[等级门槛, 关卡门槛]`。
#[must_use]
pub fn switch_payment_state(switch_payment: &[i32], lv: i32, checkpoint_id: i32) -> i32 {
    // 注意是 `size() == 2`，不是 `>= 2`
    if switch_payment.len() == 2 && lv >= switch_payment[0] && checkpoint_id >= switch_payment[1] {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> WordConfig {
        WordConfig {
            title: "客服".into(),
            content: "内容".into(),
            tips: "提示".into(),
            wechat_name: "微信客服".into(),
            wechat: Some("wx123".into()),
            qq_name: "QQ客服".into(),
            qq: None,
            reward_title: "奖励".into(),
        }
    }

    #[test]
    fn word_config_maps_all_fields() {
        let msg = word_config_to_msg(&cfg());
        assert_eq!(msg.title.as_deref(), Some("客服"));
        assert_eq!(msg.wx_titile.as_deref(), Some("微信客服"));
        assert_eq!(msg.wx.as_deref(), Some("wx123"));
        assert_eq!(msg.qq, None, "qq 为 None 时不下发");
        assert_eq!(msg.reward.as_deref(), Some("奖励"));
    }

    #[test]
    fn word_config_null_wx_and_qq_are_omitted() {
        let c = WordConfig {
            wechat: None,
            qq: None,
            ..cfg()
        };
        let msg = word_config_to_msg(&c);
        assert_eq!(msg.wx, None);
        assert_eq!(msg.qq, None);
        // 其余字段即使为空串也要 set
        assert_eq!(msg.title.as_deref(), Some("客服"));
    }

    #[test]
    fn official_account_missing_channel_is_false() {
        assert!(!is_official_account(None, "oppo"));
    }

    #[test]
    fn official_account_empty_cid_list_allows_all() {
        let c = WechatChannelConfig {
            cids: vec![],
            wechat_id: 1,
        };
        assert!(is_official_account(Some(&c), "任意渠道"));
    }

    #[test]
    fn official_account_cid_whitelist() {
        let c = WechatChannelConfig {
            cids: vec!["oppo".into(), "vivo".into()],
            wechat_id: 1,
        };
        assert!(is_official_account(Some(&c), "oppo"));
        assert!(!is_official_account(Some(&c), "huawei"));
    }

    #[test]
    fn wechat_info_param_error_when_not_official_account() {
        let out = wechat_info_outcome(None, "oppo", None);
        assert_eq!(out, WechatInfoOutcome::ParamError);
        assert_eq!(
            build_wechat_resp(&out).ret,
            ddj_pb::cmd::RetCode::ParamError.as_i32()
        );
    }

    #[test]
    fn wechat_info_param_error_when_account_missing() {
        let c = WechatChannelConfig {
            cids: vec![],
            wechat_id: 7,
        };
        // 渠道对，但按 wechatId 查不到公众号 → 仍是参数错误
        assert_eq!(
            wechat_info_outcome(Some(&c), "oppo", None),
            WechatInfoOutcome::ParamError
        );
    }

    #[test]
    fn wechat_info_accept_maps_title_and_content() {
        let c = WechatChannelConfig {
            cids: vec![],
            wechat_id: 7,
        };
        let acc = WechatOfficialAccount {
            wechat: "公众号".into(),
            content: "介绍".into(),
        };
        let out = wechat_info_outcome(Some(&c), "oppo", Some(&acc));
        let resp = build_wechat_resp(&out);
        assert_eq!(resp.ret, 0);
        assert_eq!(resp.title.as_deref(), Some("公众号"));
        assert_eq!(resp.content.as_deref(), Some("介绍"));
    }

    #[test]
    fn switch_payment_needs_exactly_two_elements() {
        // 3 个元素 → 永远 0（原版是 size() == 2）
        assert_eq!(switch_payment_state(&[1, 1, 1], 99, 999), 0);
        // 1 个元素 → 0
        assert_eq!(switch_payment_state(&[1], 99, 999), 0);
        // 空 → 0
        assert_eq!(switch_payment_state(&[], 99, 999), 0);
    }

    #[test]
    fn switch_payment_thresholds() {
        let sp = [10, 100];
        assert_eq!(switch_payment_state(&sp, 10, 100), 1, "都等于门槛即通过");
        assert_eq!(switch_payment_state(&sp, 9, 100), 0, "等级不足");
        assert_eq!(switch_payment_state(&sp, 10, 99), 0, "关卡不足");
        assert_eq!(switch_payment_state(&sp, 50, 500), 1);
    }
}
