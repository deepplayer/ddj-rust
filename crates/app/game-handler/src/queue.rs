//! # 排队信息（`GetQueueUpInfoReq_7`）
//!
//! 对应 Java：
//! - `EngineMessageHandler.getQueueUpInfoReq`（行 69-72，只转发）
//! - `QueueUpManager.getQueueUpInfoReq(Channel)`（行 36-55）
//!
//! ## 原版全文
//! ```java
//! lock.lockRead();
//! try {
//!     index    = QUEUEUP_PLAYER_LIST.indexOf(new QueueUpPlayer(channel, null));
//!     maxIndex = QUEUEUP_PLAYER_LIST.size();
//!     index2   = index < 0 ? 0 : index;
//!     maxIndex2= maxIndex <= 0 ? 1 : maxIndex;
//!     builder.setRet(0);
//!     builder.setNowIndex(index2 + 1);   // 1-based
//!     builder.setMaxIndex(maxIndex2);
//! } catch (Exception e) {
//!     builder.setRet(COMMON_SERVER_BUSY);
//! } finally { lock.unlockRead(); }
//! ```
//!
//! ## 三个必须照抄的细节
//! 1. **`nowIndex` 是 1-based**：`index2 + 1`，所以不在队列里时返回 1。
//! 2. **`maxIndex` 至少是 1**：`maxIndex <= 0 ? 1 : maxIndex`，队列空时返回 1 而不是 0。
//! 3. **不在队列里时 `index2 = 0`**（不是 -1），因为 `index < 0 ? 0 : index`。
//!
//! 也就是说：**队列空 + 玩家不在队列 → `nowIndex=1, maxIndex=1`**。
//! 客户端据此显示「第 1 位 / 共 1 位」，看起来像排到了。

use ddj_pb::gen::engine_message::GetQueueUpInfoResp7;

/// 排队位置快照。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueSnapshot {
    /// `QUEUEUP_PLAYER_LIST.size()`。
    pub len: usize,
    /// `QUEUEUP_PLAYER_LIST.indexOf(this channel)`，`None` 表示不在队列里。
    pub index: Option<usize>,
}

/// `QueueUpManager.getQueueUpInfoReq` 的纯逻辑。
///
/// 返回 `(nowIndex, maxIndex)`，两者都已按原版规则归一化。
#[must_use]
pub fn queue_position(snap: QueueSnapshot) -> (i32, i32) {
    // `index < 0 ? 0 : index` —— 不在队列里当成 0
    let index2 = snap.index.unwrap_or(0);
    // `maxIndex <= 0 ? 1 : maxIndex` —— 空队列当成 1
    let max_index2 = if snap.len == 0 { 1 } else { snap.len };
    // `setNowIndex(index2 + 1)` —— 1-based
    (
        i32::try_from(index2).unwrap_or(i32::MAX).saturating_add(1),
        i32::try_from(max_index2).unwrap_or(i32::MAX),
    )
}

/// 构造 `GetQueueUpInfoResp_7{ret=0, nowIndex, maxIndex}`。
#[must_use]
pub fn queue_resp(snap: QueueSnapshot) -> GetQueueUpInfoResp7 {
    let (now, max) = queue_position(snap);
    GetQueueUpInfoResp7 {
        ret: 0,
        now_index: Some(now),
        max_index: Some(max),
    }
}

/// 异常分支的回包（原版 `catch` 里只设 `ret`，不设位置）。
#[must_use]
pub fn queue_busy_resp() -> GetQueueUpInfoResp7 {
    GetQueueUpInfoResp7 {
        ret: ddj_pb::cmd::RetCode::ServerBusy.as_i32(),
        now_index: None,
        max_index: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_queue_reports_one_of_one() {
        // 队列空 + 不在队列 → nowIndex=1, maxIndex=1（原版两处归一化叠加）
        let r = queue_resp(QueueSnapshot {
            len: 0,
            index: None,
        });
        assert_eq!(r.ret, 0);
        assert_eq!(r.now_index, Some(1));
        assert_eq!(r.max_index, Some(1));
    }

    #[test]
    fn not_in_queue_but_others_are() {
        // 队列有 5 人，自己不在 → nowIndex = 0+1 = 1，maxIndex = 5
        let (now, max) = queue_position(QueueSnapshot {
            len: 5,
            index: None,
        });
        assert_eq!((now, max), (1, 5));
    }

    #[test]
    fn first_in_queue_is_one_based() {
        let (now, max) = queue_position(QueueSnapshot {
            len: 5,
            index: Some(0),
        });
        assert_eq!((now, max), (1, 5), "index 0 → nowIndex 1");
    }

    #[test]
    fn third_in_queue() {
        let (now, max) = queue_position(QueueSnapshot {
            len: 10,
            index: Some(2),
        });
        assert_eq!((now, max), (3, 10));
    }

    #[test]
    fn last_in_queue() {
        let (now, max) = queue_position(QueueSnapshot {
            len: 7,
            index: Some(6),
        });
        assert_eq!((now, max), (7, 7));
    }

    #[test]
    fn single_element_queue() {
        let (now, max) = queue_position(QueueSnapshot {
            len: 1,
            index: Some(0),
        });
        assert_eq!((now, max), (1, 1));
    }

    #[test]
    fn busy_resp_has_no_positions() {
        let r = queue_busy_resp();
        assert_eq!(r.ret, ddj_pb::cmd::RetCode::ServerBusy.as_i32());
        assert_eq!(r.now_index, None, "原版 catch 不设位置");
        assert_eq!(r.max_index, None);
    }
}
