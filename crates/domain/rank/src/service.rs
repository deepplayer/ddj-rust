//! 排行领域服务

use crate::event::RankEvent;
use crate::model::{Entry, RankKind};
use crate::rule::RankError;

/// 事件发布。
pub trait EventSink: Send + Sync {
    /// 发布。
    fn publish(&self, ev: RankEvent);
}

/// 只保留分数最高的 `n` 名，并按分数降序填充名次。
///
/// 纯函数，便于测试；真实实现把结果写回 Redis ZSET。
#[must_use]
pub fn top_n(mut all: Vec<(ddj_core::PlayerId, i64)>, n: usize) -> Vec<Entry> {
    all.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.get().cmp(&b.0.get())));
    all.into_iter()
        .take(n)
        .enumerate()
        .map(|(i, (player, score))| Entry {
            rank: (i + 1) as u32,
            player,
            score,
        })
        .collect()
}

/// 校验并发布分数变动。
///
/// # Errors
/// 负分。
pub fn update(
    kind: RankKind,
    player: ddj_core::PlayerId,
    score: i64,
    sink: &dyn EventSink,
) -> Result<(), RankError> {
    if score < 0 {
        return Err(RankError::NegativeScore);
    }
    sink.publish(RankEvent::ScoreChanged {
        kind,
        player,
        score,
    });
    Ok(())
}

/// 静默 sink。
pub struct NoopSink;

impl EventSink for NoopSink {
    fn publish(&self, ev: RankEvent) {
        tracing::debug!(?ev, "事件（无订阅者）");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ddj_core::PlayerId;

    #[test]
    fn sorts_desc_and_numbers() {
        let got = top_n(
            vec![
                (PlayerId::new(3), 10),
                (PlayerId::new(1), 30),
                (PlayerId::new(2), 20),
            ],
            2,
        );
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].rank, 1);
        assert_eq!(got[0].player, PlayerId::new(1));
        assert_eq!(got[1].rank, 2);
        assert_eq!(got[1].player, PlayerId::new(2));
    }

    #[test]
    fn tie_breaks_by_id() {
        let got = top_n(vec![(PlayerId::new(9), 5), (PlayerId::new(2), 5)], 2);
        assert_eq!(got[0].player, PlayerId::new(2));
    }
}
