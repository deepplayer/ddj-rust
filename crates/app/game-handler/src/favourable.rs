//! # 渠道好评（`FavourableMessageHandler` / `FavourableModel`，17101 / 17102 / 17103）
//!
//! 对应 Java：
//! - `bf/game/server/message/handler/FavourableMessageHandler.java:29-83`
//! - `bf/game/server/module/sysActivity/model/FavourableModel.java`（110 行）
//! - `PlatformServerModule.FAVOURABLE_CHANNELS` / `CAFE_URL` / `ONE_STORE_URL`
//!   （`:88-90` 定义，`:357-377` 从 `bf_config.configId` 19/20/21 读）
//!
//! ## 三条的失败出口完全不同
//! | cmd | 方法签名 | 失败出口 | 帧号 |
//! |---|---|---|---|
//! | 17101 | `public Message getFavourable(...)` **无 `throws`** | 无失败分支 | 17101 |
//! | 17102 | 无 `throws` | `CloseFavourableWindowResp_17102{ret}` | 17102 |
//! | 17103 | 无 `throws` | `FeedBackResp_17103{ret}` | 17103 |
//!
//! **三条都不走 `ErrorResp_1`** —— 都没有 `throws HandlerException`，
//! 且方法体内没有 `throw`。这是本模块与 17301-17303 最大的区别。
//!
//! ## ⚠ 17102 的两个「提前返回」
//! ```java
//! if (!FAVOURABLE_CHANNELS.contains(channelId)) return builder.setRet(0).build();
//! //                                                                  ↑ ret=0，不是错误
//! CommentDB commentDB = CommentDBCache.getCache().get(id);
//! if (commentDB == null) return builder.setRet(ACTIVITY_PARAM_ERROR(2754)).build();
//! ```
//! 渠道不在白名单时**回 `ret=0` 且不带任何数据**（客户端会当成"没发生过"）。
//!
//! ## ⚠ 17103 的失败码顺序
//! ```java
//! if (!FAVOURABLE_CHANNELS.contains(channelId)) return setRet(FAVOURABLE_ERROR_1(3941));
//! if (model.isFeedBack())                       return setRet(FAVOURABLE_ERROR_2(3942));
//! // 发奖 + model.feedBack()
//! ```
//! 渠道判定优先于「已评价」。
//!
//! ## ⚠ `openCnt` 的取模翻转（`FavourableModel.closeWindow`）
//! ```java
//! if (!openCnt.containsKey(id)) {
//!     openCnt.put(id, 1);
//! } else {
//!     int maxTimes = commentDB.getMaxTimes();
//!     if (maxTimes > 0) {
//!         if (maxTimes != openCnt.get(id)) isRefresh = false;   // ⚠ 注意是 !=
//!         openCnt.put(id, (openCnt.get(id) + 1) % maxTimes);
//!     }
//! }
//! if (isRefresh) lastCloseTime = now;
//! ```
//! 三处反直觉：
//! 1. `maxTimes == 0` 时**什么都不做**（只把 `isRefresh` 留为 true），
//!    计数永远停在旧值、`lastCloseTime` 每次都刷新。
//! 2. `maxTimes != 当前值` 时 `isRefresh = false` —— 也就是只有「计数正好等于
//!    `maxTimes`」时才刷新时间。因为后面 `% maxTimes` 会把计数拉回 `[0, maxTimes)`，
//!    等于 `maxTimes` 只可能出现在 `maxTimes == 1` 且计数为 1 的情况。
//! 3. `(count + 1) % maxTimes` 让计数在 `0..maxTimes-1` 循环。
//!
//! 这三条都照抄。

use std::collections::HashMap;

use ddj_pb::RetCode;

/// `CommentDB`（`bf/template/db/CommentDB.java`）：`id` + `maxTimes`。
///
/// ⚠ 打包的 445 张 CSV 里**没有这张表**（`CommentDB` 实体名搜不到），
/// 所以线上真实数据下 `CommentDBCache.get(id)` 恒返回 `null`
/// → 17102 恒回 `ACTIVITY_PARAM_ERROR(2754)`。照抄。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CommentTemplate {
    /// `id`。
    pub id: i32,
    /// `maxTimes`：弹窗最多开几次。
    pub max_times: i32,
}

/// `FavourableModel` 的持久化字段。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FavourableState {
    /// `isFeedBack`：是否已评价过。
    pub is_feedback: bool,
    /// `lastCloseTime`：上次刷新时间的毫秒值。
    pub last_close_time: i64,
    /// `openCnt`：`id → 已开次数`。
    pub open_cnt: HashMap<i32, i32>,
}

/// 17102 `closeWindow` 的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloseOutcome {
    /// 回包里的 `ret`。
    pub ret: RetCode,
    /// 新的 `lastCloseTime`（只在 `ret == Ok` 时有效）。
    pub last_close_time: i64,
    /// 新的 `openCnt`（只在 `ret == Ok` 时有效）。
    pub open_cnt: HashMap<i32, i32>,
    /// 是否需要落库（原版 `finally { save(); }` **无条件**调用）。
    pub dirty: bool,
}

/// `FavourableModel.closeWindow(int id)`（`:57-90`）的纯逻辑。
///
/// `channels` 是 `PlatformServerModule.FAVOURABLE_CHANNELS`；
/// `comment` 是 `CommentDBCache.get(id)` 的结果。
#[must_use]
pub fn close_verdict(
    state: &FavourableState,
    channels: &[i32],
    channel_id: i32,
    comment: Option<CommentTemplate>,
    id: i32,
    now: i64,
) -> CloseOutcome {
    // ⚠ 渠道不在白名单 → ret=0，且不带任何数据（不是错误）
    if !channels.contains(&channel_id) {
        return CloseOutcome {
            ret: RetCode::Ok,
            last_close_time: state.last_close_time,
            open_cnt: state.open_cnt.clone(),
            dirty: false,
        };
    }
    // `CommentDB == null` → ACTIVITY_PARAM_ERROR(2754)
    let Some(tpl) = comment else {
        return CloseOutcome {
            ret: RetCode::ActivityParamError,
            last_close_time: state.last_close_time,
            open_cnt: state.open_cnt.clone(),
            dirty: false,
        };
    };

    let mut open_cnt = state.open_cnt.clone();
    let mut last_close_time = state.last_close_time;
    let mut is_refresh = true;

    match open_cnt.get(&id).copied() {
        None => {
            open_cnt.insert(id, 1);
        }
        Some(cur) => {
            if tpl.max_times > 0 {
                // ⚠ `maxTimes != 当前值` → 不刷新（见模块文档第 2 条）
                if tpl.max_times != cur {
                    is_refresh = false;
                }
                open_cnt.insert(id, (cur + 1) % tpl.max_times);
            }
            // `maxTimes == 0` → 什么都不做
        }
    }
    if is_refresh {
        last_close_time = now;
    }

    CloseOutcome {
        ret: RetCode::Ok,
        last_close_time,
        open_cnt,
        // 原版 `finally { save(); }` 无条件写库
        dirty: true,
    }
}

/// 17103 `feedBack` 的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedbackOutcome {
    /// 回 `FeedBackResp_17103{ret=code}`，不发奖。
    Reject(RetCode),
    /// 回 `ret=0` + 奖励列表，并置 `is_feedback = true`。
    Accept {
        /// 要发的奖励。
        rewards: Vec<(i32, i64)>,
    },
}

/// `FavourableMessageHandler.feedBack`（`:60-83`）的纯逻辑。
///
/// 分支序：
/// ```text
/// !FAVOURABLE_CHANNELS.contains(channelId) → FAVOURABLE_ERROR_1(3941)
/// model.isFeedBack()                        → FAVOURABLE_ERROR_2(3942)
/// 否则                                       → 发 CommentReward + isFeedBack = true
/// ```
#[must_use]
pub fn feedback_verdict(
    state: &FavourableState,
    channels: &[i32],
    channel_id: i32,
    comment_reward: &[(i32, i64)],
) -> FeedbackOutcome {
    if !channels.contains(&channel_id) {
        return FeedbackOutcome::Reject(RetCode::FavourableError1);
    }
    if state.is_feedback {
        return FeedbackOutcome::Reject(RetCode::FavourableError2);
    }
    FeedbackOutcome::Accept {
        rewards: comment_reward.to_vec(),
    }
}

/// `MsgFavourable` 的一行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FavourableMsg {
    /// `id`。
    pub id: i32,
    /// `count`。
    pub count: i32,
}

/// 17101 的 `builder.addMsgFavourable(...)` 部分。
///
/// ⚠ **`openCnt` 是 `HashMap`，遍历顺序不稳定**。原版 `ConcurrentHashMap` 也一样。
/// 客户端按 `id` 索引，所以顺序无意义。
#[must_use]
pub fn to_msg_list(open_cnt: &HashMap<i32, i32>) -> Vec<FavourableMsg> {
    let mut v: Vec<FavourableMsg> = open_cnt
        .iter()
        .map(|(id, count)| FavourableMsg {
            id: *id,
            count: *count,
        })
        .collect();
    // 固定顺序，方便单测与客户端日志比对（原版无保证，等价）
    v.sort_by_key(|m| m.id);
    v
}

/// `PlatformServerModule` 的三个静态配置（`bf_config.configId` 19/20/21）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlatformUrls {
    /// `CAFE_URL`（`configId=19`）。
    pub cafe_url: String,
    /// `ONE_STORE_URL`（`configId=20`）。
    pub one_store_url: String,
    /// `FAVOURABLE_CHANNELS`（`configId=21`，逗号分隔）。
    pub channels: Vec<i32>,
}

impl PlatformUrls {
    /// 从三个配置值构造（`PlatformServerModule.java:357-377`）。
    ///
    /// `configId=21` 用 `split(",")` + `Integer.parseInt`；
    /// 实测三个都是 `null`（`data/sql/platform.sql:517-519`），所以全部为空。
    #[must_use]
    pub fn parse(cafe: Option<&str>, one_store: Option<&str>, channels: Option<&str>) -> Self {
        Self {
            cafe_url: cafe.filter(|s| !s.is_empty()).unwrap_or("").to_owned(),
            one_store_url: one_store.filter(|s| !s.is_empty()).unwrap_or("").to_owned(),
            channels: channels
                .filter(|s| !s.is_empty())
                .map(|s| {
                    s.split(',')
                        .filter_map(|x| x.trim().parse::<i32>().ok())
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    /// `FAVOURABLE_CHANNELS.contains(channelId)`。
    #[must_use]
    pub fn is_favourable_channel(&self, channel_id: i32) -> bool {
        self.channels.contains(&channel_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn urls() -> PlatformUrls {
        PlatformUrls::parse(Some("https://cafe"), Some("https://store"), Some("1,2,3"))
    }

    fn state() -> FavourableState {
        FavourableState::default()
    }

    // ── 平台配置 ──

    #[test]
    fn parse_urls_and_channels() {
        let u = urls();
        assert_eq!(u.cafe_url, "https://cafe");
        assert_eq!(u.one_store_url, "https://store");
        assert_eq!(u.channels, vec![1, 2, 3]);
    }

    #[test]
    fn parse_all_none_yields_empty() {
        // 实测三个配置都是 null
        let u = PlatformUrls::parse(None, None, None);
        assert!(u.cafe_url.is_empty());
        assert!(u.channels.is_empty());
    }

    #[test]
    fn parse_empty_string_treated_as_absent() {
        let u = PlatformUrls::parse(Some(""), Some(""), Some(""));
        assert!(u.cafe_url.is_empty());
        assert!(u.channels.is_empty());
    }

    #[test]
    fn channel_membership() {
        assert!(urls().is_favourable_channel(2));
        assert!(!urls().is_favourable_channel(9));
    }

    // ── 17102 ──

    #[test]
    fn close_not_in_channel_returns_ok() {
        // ⚠ 渠道不符 → ret=0，不是错误
        let o = close_verdict(&state(), &[1, 2], 9, None, 1, 1000);
        assert_eq!(o.ret, RetCode::Ok);
        assert!(!o.dirty, "渠道不符直接返回，不写库");
    }

    #[test]
    fn close_missing_template_is_2754() {
        let o = close_verdict(&state(), &[1, 2], 1, None, 1, 1000);
        assert_eq!(o.ret, RetCode::ActivityParamError);
        assert!(!o.dirty);
    }

    #[test]
    fn close_first_time_sets_count_one_and_refreshes() {
        let tpl = CommentTemplate {
            id: 1,
            max_times: 3,
        };
        let o = close_verdict(&state(), &[1], 1, Some(tpl), 1, 5_000);
        assert_eq!(o.ret, RetCode::Ok);
        assert_eq!(o.open_cnt.get(&1), Some(&1), "首次 → 1");
        assert_eq!(o.last_close_time, 5_000);
        assert!(o.dirty, "成功路径无条件 save()");
    }

    #[test]
    fn close_max_times_zero_freezes_count() {
        // maxTimes == 0 → 什么都不做，isRefresh 保持 true
        let tpl = CommentTemplate {
            id: 1,
            max_times: 0,
        };
        let mut s = state();
        s.open_cnt.insert(1, 7);
        let o = close_verdict(&s, &[1], 1, Some(tpl), 1, 9_000);
        assert_eq!(o.open_cnt.get(&1), Some(&7), "maxTimes=0 计数不变");
        assert_eq!(o.last_close_time, 9_000, "但仍刷新时间");
    }

    #[test]
    fn close_wraps_with_modulo() {
        let tpl = CommentTemplate {
            id: 1,
            max_times: 3,
        };
        let mut s = state();
        s.open_cnt.insert(1, 1);
        let o = close_verdict(&s, &[1], 1, Some(tpl), 1, 9_000);
        assert_eq!(o.open_cnt.get(&1), Some(&2), "(1+1)%3 = 2");
    }

    #[test]
    fn close_wraps_back_to_zero() {
        let tpl = CommentTemplate {
            id: 1,
            max_times: 3,
        };
        let mut s = state();
        s.open_cnt.insert(1, 2);
        let o = close_verdict(&s, &[1], 1, Some(tpl), 1, 9_000);
        assert_eq!(o.open_cnt.get(&1), Some(&0), "(2+1)%3 = 0");
    }

    #[test]
    fn close_refresh_only_when_count_equals_max_times() {
        // ⚠ `maxTimes != 当前值` → 不刷新。maxTimes=3，当前=1 → 不刷新
        let tpl = CommentTemplate {
            id: 1,
            max_times: 3,
        };
        let mut s = state();
        s.open_cnt.insert(1, 1);
        s.last_close_time = 111;
        let o = close_verdict(&s, &[1], 1, Some(tpl), 1, 9_000);
        assert_eq!(o.last_close_time, 111, "1 != 3 → 不刷新（原版写法）");
    }

    #[test]
    fn close_refresh_when_count_equals_max_times() {
        // maxTimes == 当前值 → 刷新。只有 maxTimes=1 且计数=1 时可能
        let tpl = CommentTemplate {
            id: 1,
            max_times: 1,
        };
        let mut s = state();
        s.open_cnt.insert(1, 1);
        s.last_close_time = 111;
        let o = close_verdict(&s, &[1], 1, Some(tpl), 1, 9_000);
        assert_eq!(o.last_close_time, 9_000, "1 == 1 → 刷新");
        assert_eq!(o.open_cnt.get(&1), Some(&0), "(1+1)%1 = 0");
    }

    // ── 17103 ──

    #[test]
    fn feedback_not_in_channel_is_3941() {
        let out = feedback_verdict(&state(), &[1, 2], 9, &[(1, 100)]);
        assert_eq!(out, FeedbackOutcome::Reject(RetCode::FavourableError1));
    }

    #[test]
    fn feedback_already_done_is_3942() {
        let mut s = state();
        s.is_feedback = true;
        let out = feedback_verdict(&s, &[1], 1, &[(1, 100)]);
        assert_eq!(out, FeedbackOutcome::Reject(RetCode::FavourableError2));
    }

    #[test]
    fn feedback_channel_checked_before_already_done() {
        // 渠道不符 + 已评价 → 3941（不是 3942）
        let mut s = state();
        s.is_feedback = true;
        let out = feedback_verdict(&s, &[1], 9, &[(1, 100)]);
        assert_eq!(out, FeedbackOutcome::Reject(RetCode::FavourableError1));
    }

    #[test]
    fn feedback_accepts_with_rewards() {
        let out = feedback_verdict(&state(), &[1], 1, &[(1, 100), (231, 2)]);
        assert_eq!(
            out,
            FeedbackOutcome::Accept {
                rewards: vec![(1, 100), (231, 2)]
            }
        );
    }

    // ── to_msg_list ──

    #[test]
    fn msg_list_sorted_by_id() {
        let mut m = HashMap::new();
        m.insert(3, 1);
        m.insert(1, 2);
        m.insert(2, 3);
        let v = to_msg_list(&m);
        assert_eq!(v[0].id, 1);
        assert_eq!(v[1].id, 2);
        assert_eq!(v[2].id, 3);
    }

    #[test]
    fn msg_list_empty() {
        assert!(to_msg_list(&HashMap::new()).is_empty());
    }
}
