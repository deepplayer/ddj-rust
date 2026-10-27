//! # 绑定奖励与连接标记（1115 / 1004）
//!
//! 对应 Java：`bf/game/server/message/handler/PlayerMessageHandler.java`
//! - `MsgGainBindRewardReq_1115`（行 438-455）
//! - `UpdatePlayerVarReq_1004`（行 433-436）
//!
//! ## 1115 绑定奖励
//! ```text
//! value = playerCommon.getKeyValue(TypePlayerCacheKey.TWBindAccount)   // key=2
//! isGain = value != null && value.equals("1")
//! if (isGain) throw HandlerException(APPRAISAL_GAIN_ACHIEVEMENT=4601)
//! playerCommon.setKeyValue(TWBindAccount, "1")
//! rewards = new ArrayList<>(ConfigDBCache.TwBindReward)
//! if (now - player.getCreatedateTime() <= 86400000)
//!     rewards.addAll(ConfigDBCache.TwBindHighReward)
//! Mail sysCustomMail = Mail.createSysCustomMail(playerId, TwBindMail, rewards)
//! MailServerModule.sendMail(playerId, sysCustomMail)
//! return MsgGainBindRewardResp_1115{ret=0}
//! ```
//!
//! 三个容易漏的点：
//! 1. 判定是 `value.equals("1")`，**不是**「非 null 即已领」；
//! 2. 24 小时窗口用的是 `<=`（等于 86400000 仍算新号）；
//! 3. `createSysCustomMail` 在模板缺失时返回 **null**，随后 `sendMail(null)`
//!    会 NPE → 被 handler 的 `throws HandlerException` 之外的路径吞掉。
//!    原版这里没有 try/catch，异常会冒到框架层（`InvocationTargetException`
//!    非 `HandlerException` → `ErrorResp_1{ret=2}`）。
//!
//! ## 1004 设置连接标记
//! ```text
//! channelPlayer.setBor();
//! ```
//! 只有一句。`ChannelPlayer.bor`（`ChannelPlayer.java:15`）默认 `false`，
//! `setBor()` 无条件置 `true`。它被 `AbsPurposePlayPlayer.getCheatReturnData()`
//! （行 206）用来判断「这条连接是不是走过更新包」—— 配合 `clientLoginvar`
//! 与 `loginvar` 比对，决定是否放行作弊检测。
//!
//! **1004 不回包**（原版返回 `void`），且 `req.var` 字段原版根本没读。

use ddj_core::Result;
use ddj_pb::RetCode;

use crate::mail::Reward;

/// `TypePlayerCacheKey.TWBindAccount` 的枚举值（`TypePlayerCacheKey.java:2`）。
pub const KEY_TW_BIND_ACCOUNT: i32 = 2;

/// 24 小时（毫秒）：新号额外奖励的窗口。
pub const NEW_PLAYER_WINDOW_MS: i64 = 86_400_000;

/// 1115 的判定输入。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindRewardCtx {
    /// `playerCommon.getKeyValue(TWBindAccount)`：`None` 表示没有这条记录。
    pub stored_value: Option<String>,
    /// `player.getCreatedateTime()`。
    pub create_time: i64,
    /// `ConfigDBCache.TwBindReward`：基础奖励。
    pub base_rewards: Vec<Reward>,
    /// `ConfigDBCache.TwBindHighReward`：新号额外奖励。
    pub high_rewards: Vec<Reward>,
    /// `ConfigDBCache.TwBindMail`：邮件模板 id。
    pub mail_template_id: i32,
}

/// 1115 的结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindRewardOutcome {
    /// 已经领过 → `HandlerException(APPRAISAL_GAIN_ACHIEVEMENT=4601)`，
    /// 框架出口是 `ErrorResp_1`。
    AlreadyGained,
    /// 模板缺失 → 原版 `createSysCustomMail` 返回 null → NPE → ret=2。
    MailTemplateMissing,
    /// 通过：要把 `TWBindAccount` 置为 `"1"`，并发一封带 `rewards` 的邮件。
    Accept {
        /// 合并后的奖励（基础 + 可能的新号额外）。
        rewards: Vec<Reward>,
        /// 邮件模板 id。
        mail_template_id: i32,
    },
}

impl BindRewardOutcome {
    /// 协议上的 `ret`；`None` 表示成功（`ret=0`）。
    #[must_use]
    pub const fn resolve_ret(&self) -> Option<RetCode> {
        match self {
            Self::AlreadyGained => Some(RetCode::AppraisalGainAchievement),
            Self::MailTemplateMissing => Some(RetCode::ServerBusy),
            Self::Accept { .. } => None,
        }
    }
}

/// `MsgGainBindRewardReq_1115` 的纯逻辑。
///
/// `mail_template_exists` 对应 `MailTemplateDBCache.getMailTemplate(id) != null`。
#[must_use]
pub fn bind_reward_verdict(
    ctx: &BindRewardCtx,
    now: i64,
    mail_template_exists: bool,
) -> BindRewardOutcome {
    // 1. `value != null && value.equals("1")` → 已领
    if ctx.stored_value.as_deref() == Some("1") {
        return BindRewardOutcome::AlreadyGained;
    }
    // 2. `Mail.createSysCustomMail` 模板缺失 → 返回 null → 后续 NPE
    if !mail_template_exists {
        return BindRewardOutcome::MailTemplateMissing;
    }
    // 3. 奖励 = 基础 +（24h 内再加新号礼包）
    let mut rewards = ctx.base_rewards.clone();
    if now - ctx.create_time <= NEW_PLAYER_WINDOW_MS {
        rewards.extend(ctx.high_rewards.iter().copied());
    }
    BindRewardOutcome::Accept {
        rewards,
        mail_template_id: ctx.mail_template_id,
    }
}

/// `PlayerCommon.setKeyValue(TWBindAccount, "1")` 的落地值。
#[must_use]
pub fn bind_marker_value() -> &'static str {
    "1"
}

/// `UpdatePlayerVarReq_1004` 的纯逻辑：**没有分支**，只是把 `bor` 置 `true`。
///
/// 原版不读 `req.var`、不回包。返回 `()` 是为了让调用点显式表达「无回包」。
///
/// # Errors
/// 目前不会失败。
pub fn apply_set_bor() -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> BindRewardCtx {
        BindRewardCtx {
            stored_value: None,
            create_time: 1_000_000,
            base_rewards: vec![(1, 100)],
            high_rewards: vec![(2, 200)],
            mail_template_id: 900,
        }
    }

    #[test]
    fn already_gained_when_value_is_one() {
        let c = BindRewardCtx {
            stored_value: Some("1".into()),
            ..ctx()
        };
        assert_eq!(
            bind_reward_verdict(&c, 2_000_000, true),
            BindRewardOutcome::AlreadyGained
        );
        assert_eq!(
            BindRewardOutcome::AlreadyGained.resolve_ret(),
            Some(RetCode::AppraisalGainAchievement)
        );
    }

    #[test]
    fn other_values_are_not_treated_as_gained() {
        // 原版是 value.equals("1")，所以 "0" / "" / "true" 都算未领
        for v in ["0", "", "true", "2"] {
            let c = BindRewardCtx {
                stored_value: Some(v.into()),
                ..ctx()
            };
            assert!(
                matches!(
                    bind_reward_verdict(&c, 2_000_000, true),
                    BindRewardOutcome::Accept { .. }
                ),
                "value={v} 应视为未领"
            );
        }
    }

    #[test]
    fn new_player_within_24h_gets_high_reward() {
        let c = ctx();
        // 正好 24h（<= 边界）仍算新号
        let out = bind_reward_verdict(&c, c.create_time + NEW_PLAYER_WINDOW_MS, true);
        match out {
            BindRewardOutcome::Accept { rewards, .. } => {
                assert_eq!(rewards, vec![(1, 100), (2, 200)]);
            }
            other => panic!("应通过，得到 {other:?}"),
        }
    }

    #[test]
    fn old_player_gets_only_base_reward() {
        let c = ctx();
        let out = bind_reward_verdict(&c, c.create_time + NEW_PLAYER_WINDOW_MS + 1, true);
        match out {
            BindRewardOutcome::Accept { rewards, .. } => {
                assert_eq!(rewards, vec![(1, 100)], "超过 24h 不再加新号礼包");
            }
            other => panic!("应通过，得到 {other:?}"),
        }
    }

    #[test]
    fn missing_mail_template_is_server_busy() {
        let out = bind_reward_verdict(&ctx(), 2_000_000, false);
        assert_eq!(out, BindRewardOutcome::MailTemplateMissing);
        assert_eq!(
            out.resolve_ret(),
            Some(RetCode::ServerBusy),
            "原版 NPE 被框架遮罩成 ret=2"
        );
    }

    #[test]
    fn gained_check_wins_over_missing_template() {
        // 原版先判 isGain，再 createSysCustomMail，所以已领优先
        let c = BindRewardCtx {
            stored_value: Some("1".into()),
            ..ctx()
        };
        assert_eq!(
            bind_reward_verdict(&c, 2_000_000, false),
            BindRewardOutcome::AlreadyGained
        );
    }

    #[test]
    fn accept_carries_template_id() {
        match bind_reward_verdict(&ctx(), 2_000_000, true) {
            BindRewardOutcome::Accept {
                mail_template_id, ..
            } => assert_eq!(mail_template_id, 900),
            other => panic!("应通过，得到 {other:?}"),
        }
    }

    #[test]
    fn set_bor_is_infallible() {
        assert!(apply_set_bor().is_ok());
    }
}
