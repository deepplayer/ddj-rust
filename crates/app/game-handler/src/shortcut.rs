//! # 保存桌面（`ShortcutModel`，17301 / 17302 / 17303）
//!
//! 对应 Java：
//! - `bf/game/server/message/handler/SysActivityMessageHandler.java:231-254`
//!   （`queryShortcutModel` / `setShortcutInstall` / `gainShortcutReward`）
//! - `bf/game/server/module/sysActivity/model/ShortcutModel.java`（126 行）
//! - `bf/game/server/module/platform/PlatformServerModule.isEligibleShortcut`
//!   （`:712-715`）+ `parseConfig59`（`:1315-1336`）
//!
//! ## 三者的回包差异
//! | cmd | 成功 | 失败 |
//! |---|---|---|
//! | 17301 | `ShortcutDataResp_17301{ret=0, shortcutData}` | 无失败分支 |
//! | 17302 | `ShortcutInstallResp_17302{ret=0, shortcutData}` | `throw` → **`ErrorResp_1`** |
//! | 17303 | `ShortcutRewardResp_17303{ret=0, shortcutData, rewards}` | `throw` → **`ErrorResp_1`** |
//!
//! 17301 的方法签名**没有** `throws HandlerException`，且方法体无 `throw`，
//! 所以它只有成功分支。
//!
//! ## 状态机（`TaskRewardState`）
//! ```text
//! 0 NO_REWARD     未保存到桌面，无奖励
//! 1 NOT_RECEIVE   已保存，奖励待领
//! 2 RECEIVED      已领取
//! ```
//! 17302：`state != 0` → `COMMON_SERVER_STAGE_ERROR(30)`。
//! 17303：`state == 0` → `SHORTCUT_NOT_INSTALL(4732)`；
//!        `state != 1` → `VIVO_WELFARE_SIGN_ERROR(4722)`。
//!
//! ## ⚠ 渠道判定的顺序
//! 17302 与 17303 **都先判渠道**：
//! ```java
//! boolean eligible = PlatformServerModule.isEligibleShortcut(channelId, cid);
//! if (!eligible) throw new HandlerException(SHORTCUT_CID_ERROR);   // 4731
//! ```
//! 所以渠道不匹配时**永远先看到 4731**，看不到后面的 30/4732/4722。
//!
//! ## `isEligibleShortcut` 的真实语义
//! ```java
//! ShortcutConfig cfg = shortcutConfigMap.get(channelId);
//! return cfg != null && cfg.getCidSet().contains(cid);
//! ```
//! `shortcutConfigMap` 由 `parseConfig59` 从 `bf_config.configId=59` 解析：
//! `"channelId=cid1,cid2;channelId2=cid3"`。
//!
//! ⚠ **实测该配置为空串**（`data/sql/platform.sql:562`），
//! 所以 `shortcutConfigMap` 为空 → `isEligibleShortcut` **恒 false**
//! → 17302/17303 恒回 4731。这是原版线上数据的真实状态，照抄。
//!
//! ## `toMsgShortcutData` 的四个字段
//! ```java
//! builder.setCid(cid);
//! builder.setChannelId(channelId);
//! builder.setState(state);
//! builder.setEligible(isEligibleShortcut(channelId, cid));
//! ```

use ddj_pb::RetCode;

/// `TaskRewardState` 的三个值（`bf/common/enums/TaskRewardState.java`）。
pub mod state {
    /// `NO_REWARD`：未保存到桌面。
    pub const NO_REWARD: i32 = 0;
    /// `NOT_RECEIVE`：已保存，奖励待领。
    pub const NOT_RECEIVE: i32 = 1;
    /// `RECEIVED`：已领取。
    pub const RECEIVED: i32 = 2;
}

/// `PlatformServerModule.shortcutConfigMap`：`channelId → cid 白名单`。
///
/// 由 `bf_config.configId=59` 的 `parseConfig59` 解析：
/// `"channelId=cid1,cid2;channelId2=cid3"`。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShortcutConfig {
    /// `channelId → cid 集合`。
    pub by_channel: std::collections::HashMap<i32, Vec<String>>,
}

impl ShortcutConfig {
    /// `parseConfig59(String)`（`PlatformServerModule.java:1315-1336`）。
    ///
    /// ```java
    /// for (String s : str.split(";")) {
    ///     String[] kv = s.split("=");
    ///     int channelId = Integer.parseInt(kv[0]);
    ///     for (String cid : kv[1].split(",")) cfg.addCidSet(cid);
    /// }
    /// ```
    ///
    /// ⚠ `str == null` 或空串时原版直接返回（`catch` 也没机会触发），
    /// `shortcutConfigMap` 保持空。
    #[must_use]
    pub fn parse(s: &str) -> Self {
        let mut by_channel = std::collections::HashMap::new();
        if s.is_empty() {
            return Self { by_channel };
        }
        for seg in s.split(';') {
            let Some((ch, cids)) = seg.split_once('=') else {
                continue;
            };
            let Ok(channel_id) = ch.trim().parse::<i32>() else {
                continue;
            };
            let list: Vec<String> = cids
                .split(',')
                .map(|c| c.trim().to_owned())
                .filter(|c| !c.is_empty())
                .collect();
            by_channel
                .entry(channel_id)
                .or_insert_with(Vec::new)
                .extend(list);
        }
        Self { by_channel }
    }

    /// `isEligibleShortcut(channelId, cid)`（`PlatformServerModule.java:712-715`）。
    #[must_use]
    pub fn is_eligible(&self, channel_id: i32, cid: &str) -> bool {
        self.by_channel
            .get(&channel_id)
            .is_some_and(|list| list.iter().any(|c| c == cid))
    }
}

/// `MsgShortcutData` 的四个字段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutData {
    /// `channelId`。
    pub channel_id: i32,
    /// `cid`。
    pub cid: String,
    /// `state`。
    pub state: i32,
    /// `eligible`。
    pub eligible: bool,
}

/// `ShortcutModel.toMsgShortcutData()`（`:110-124`）。
#[must_use]
pub fn to_msg(channel_id: i32, cid: &str, state: i32, cfg: &ShortcutConfig) -> ShortcutData {
    ShortcutData {
        channel_id,
        cid: cid.to_owned(),
        state,
        eligible: cfg.is_eligible(channel_id, cid),
    }
}

/// 17302 `setShortcutInstallDone` 的结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallOutcome {
    /// `throw new HandlerException(code)` → `ErrorResp_1`。
    Reject(RetCode),
    /// 成功；返回新状态（恒为 `NOT_RECEIVE`）。
    Accept {
        /// 写入后的 `state`。
        state: i32,
    },
}

/// `ShortcutModel.setShortcutInstallDone(Player)`（`:70-93`）的纯逻辑。
///
/// 分支序（不能调换）：
/// ```text
/// !isEligibleShortcut(channelId, cid) → SHORTCUT_CID_ERROR(4731)
/// state != NO_REWARD                  → COMMON_SERVER_STAGE_ERROR(30)
/// 否则                                 → state = NOT_RECEIVE，回包
/// ```
#[must_use]
pub fn install_verdict(channel_id: i32, cid: &str, state: i32, cfg: &ShortcutConfig) -> InstallOutcome {
    if !cfg.is_eligible(channel_id, cid) {
        return InstallOutcome::Reject(RetCode::ShortcutCidError);
    }
    if state != state::NO_REWARD {
        return InstallOutcome::Reject(RetCode::StageError);
    }
    InstallOutcome::Accept {
        state: state::NOT_RECEIVE,
    }
}

/// 17303 `gainShortcutReward` 的结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RewardOutcome {
    /// `throw new HandlerException(code)` → `ErrorResp_1`。
    Reject(RetCode),
    /// 成功；返回写入后的状态（恒为 `RECEIVED`）。
    Accept {
        /// 写入后的 `state`。
        state: i32,
    },
}

/// `ShortcutModel.gainShortcutReward(Player)`（`:95-131`）的纯逻辑。
///
/// 分支序（不能调换）：
/// ```text
/// !isEligibleShortcut(channelId, cid) → SHORTCUT_CID_ERROR(4731)
/// state == NO_REWARD                  → SHORTCUT_NOT_INSTALL(4732)
/// state != NOT_RECEIVE                → VIVO_WELFARE_SIGN_ERROR(4722)
/// 否则                                 → state = RECEIVED，发奖
/// ```
#[must_use]
pub fn reward_verdict(
    channel_id: i32,
    cid: &str,
    state: i32,
    cfg: &ShortcutConfig,
) -> RewardOutcome {
    if !cfg.is_eligible(channel_id, cid) {
        return RewardOutcome::Reject(RetCode::ShortcutCidError);
    }
    if state == state::NO_REWARD {
        return RewardOutcome::Reject(RetCode::ShortcutNotInstall);
    }
    if state != state::NOT_RECEIVE {
        return RewardOutcome::Reject(RetCode::VivoWelfareSignError);
    }
    RewardOutcome::Accept {
        state: state::RECEIVED,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> ShortcutConfig {
        ShortcutConfig::parse("100=abc,def;200=xyz")
    }

    // ── 配置解析 ──

    #[test]
    fn parse_multi_channel() {
        let c = cfg();
        assert_eq!(c.by_channel.len(), 2);
        assert_eq!(c.by_channel.get(&100).map(Vec::len), Some(2));
        assert_eq!(c.by_channel.get(&200), Some(&vec!["xyz".to_owned()]));
    }

    #[test]
    fn parse_empty_string_yields_empty_map() {
        // ⚠ 实测 bf_config.configId=59 就是空串
        assert!(ShortcutConfig::parse("").by_channel.is_empty());
    }

    #[test]
    fn parse_skips_malformed_segments() {
        let c = ShortcutConfig::parse("bad;100=a;;200=b");
        assert_eq!(c.by_channel.len(), 2, "bad 与空段被跳过");
    }

    #[test]
    fn empty_config_is_never_eligible() {
        let empty = ShortcutConfig::default();
        assert!(!empty.is_eligible(100, "abc"));
        assert!(!empty.is_eligible(0, ""));
    }

    #[test]
    fn eligible_requires_both_channel_and_cid() {
        let c = cfg();
        assert!(c.is_eligible(100, "abc"));
        assert!(c.is_eligible(100, "def"));
        assert!(!c.is_eligible(100, "xyz"), "cid 不在该渠道");
        assert!(!c.is_eligible(999, "abc"), "渠道不在表里");
    }

    // ── toMsg ──

    #[test]
    fn to_msg_carries_four_fields() {
        let m = to_msg(100, "abc", state::NOT_RECEIVE, &cfg());
        assert_eq!(m.channel_id, 100);
        assert_eq!(m.cid, "abc");
        assert_eq!(m.state, 1);
        assert!(m.eligible);
    }

    #[test]
    fn to_msg_eligible_false_when_not_configured() {
        let m = to_msg(0, "", state::NO_REWARD, &ShortcutConfig::default());
        assert!(!m.eligible, "空配置 → eligible=false（原版线上状态）");
        assert_eq!(m.state, 0);
    }

    // ── 17302 ──

    #[test]
    fn install_rejects_ineligible_first() {
        // 渠道判定优先于状态判定
        let out = install_verdict(999, "abc", state::RECEIVED, &cfg());
        assert_eq!(out, InstallOutcome::Reject(RetCode::ShortcutCidError));
    }

    #[test]
    fn install_rejects_when_already_installed() {
        let out = install_verdict(100, "abc", state::NOT_RECEIVE, &cfg());
        assert_eq!(out, InstallOutcome::Reject(RetCode::StageError));
    }

    #[test]
    fn install_rejects_when_already_received() {
        let out = install_verdict(100, "abc", state::RECEIVED, &cfg());
        assert_eq!(out, InstallOutcome::Reject(RetCode::StageError));
    }

    #[test]
    fn install_accepts_from_no_reward() {
        let out = install_verdict(100, "abc", state::NO_REWARD, &cfg());
        assert_eq!(out, InstallOutcome::Accept { state: 1 });
    }

    #[test]
    fn install_with_empty_config_always_4731() {
        let out = install_verdict(100, "abc", state::NO_REWARD, &ShortcutConfig::default());
        assert_eq!(out, InstallOutcome::Reject(RetCode::ShortcutCidError));
    }

    // ── 17303 ──

    #[test]
    fn reward_rejects_ineligible_first() {
        let out = reward_verdict(999, "abc", state::NOT_RECEIVE, &cfg());
        assert_eq!(out, RewardOutcome::Reject(RetCode::ShortcutCidError));
    }

    #[test]
    fn reward_rejects_when_not_installed() {
        let out = reward_verdict(100, "abc", state::NO_REWARD, &cfg());
        assert_eq!(out, RewardOutcome::Reject(RetCode::ShortcutNotInstall));
    }

    #[test]
    fn reward_rejects_when_already_received() {
        let out = reward_verdict(100, "abc", state::RECEIVED, &cfg());
        assert_eq!(out, RewardOutcome::Reject(RetCode::VivoWelfareSignError));
    }

    #[test]
    fn reward_accepts_from_not_receive() {
        let out = reward_verdict(100, "abc", state::NOT_RECEIVE, &cfg());
        assert_eq!(out, RewardOutcome::Accept { state: 2 });
    }

    #[test]
    fn reward_branch_order_cid_before_not_install() {
        // 渠道不匹配 + state=0 → 4731（不是 4732）
        let out = reward_verdict(999, "abc", state::NO_REWARD, &cfg());
        assert_eq!(out, RewardOutcome::Reject(RetCode::ShortcutCidError));
    }
}
