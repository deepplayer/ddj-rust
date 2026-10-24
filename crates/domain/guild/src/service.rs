//! 公会领域服务

use crate::event::GuildEvent;
use crate::model::{Guild, GuildRank, Member, MAX_MEMBERS};
use crate::rule::GuildError;

/// 事件发布。
pub trait EventSink: Send + Sync {
    /// 发布。
    fn publish(&self, ev: GuildEvent);
}

/// 加入公会。
///
/// # Errors
/// 满员 / 已是成员。
pub fn join(
    g: &mut Guild,
    player: ddj_core::PlayerId,
    now_ms: i64,
    sink: &dyn EventSink,
) -> Result<(), GuildError> {
    if g.member(player).is_some() {
        return Err(GuildError::AlreadyMember);
    }
    if g.members.len() >= MAX_MEMBERS {
        return Err(GuildError::Full);
    }
    g.members.push(Member {
        player,
        rank: GuildRank::Member,
        contribution: 0,
        joined_ms: now_ms,
    });
    sink.publish(GuildEvent::Joined {
        guild: g.id,
        player,
    });
    Ok(())
}

/// 踢人。
///
/// # Errors
/// 权限不足 / 目标不是成员 / 踢会长。
pub fn kick(
    g: &mut Guild,
    by: ddj_core::PlayerId,
    target: ddj_core::PlayerId,
    sink: &dyn EventSink,
) -> Result<(), GuildError> {
    let by_rank = g
        .member(by)
        .map(|m| m.rank)
        .ok_or(GuildError::NoPermission)?;
    if !by_rank.can_kick() {
        return Err(GuildError::NoPermission);
    }
    let tgt_rank = g
        .member(target)
        .map(|m| m.rank)
        .ok_or(GuildError::NotFound)?;
    if tgt_rank >= by_rank {
        return Err(GuildError::NoPermission);
    }
    g.members.retain(|m| m.player != target);
    sink.publish(GuildEvent::Kicked {
        guild: g.id,
        player: target,
        by,
    });
    Ok(())
}

/// 静默 sink。
pub struct NoopSink;

impl EventSink for NoopSink {
    fn publish(&self, ev: GuildEvent) {
        tracing::debug!(?ev, "事件（无订阅者）");
    }
}
