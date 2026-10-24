//! 单条会话
//!
//! 对应 Java：`bf.game.server.ChannelPlayer`。
//!
//! 原实现里 `Channel` / `ChannelPlayer` / `Player` 三者在三个地方持有互相引用，
//! 断线时容易漏清理。这里改为单向持有：`Session` 只存 `player_id`，
//! 真正的 `Player` 聚合在 `ddj-player` 的内存表里。

use crate::state::SessionState;
use ddj_core::{PlayerId, ServerId};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// 当前毫秒时间戳。
///
/// 会话层只需要一个单调递增的墙上时间，不用注入 `Clock`
/// （注入会让每条会话多存一个 `Arc<dyn Clock>`，收益不抵成本）。
fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// 会话唯一 ID（进程内自增）。
pub type SessionId = u64;

/// 发往客户端的下行帧。
/// 出站队列。
///
/// 承载完整的 [`ddj_codec::Frame`] 而不是裸 body —— 服务端主动推送
/// （`ItemUpdateResp_11004` 等）必须自带 cmdId，裸字节无法表达。
pub type Outbound = mpsc::Sender<ddj_codec::Frame>;

/// 下行失败原因：会话已断开。
///
/// 用具名类型而不是 `()`，调用方必须显式处理，
/// 避免把「客户端已断开」和「代码忘了处理」混为一谈。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionClosed;

impl std::fmt::Display for SessionClosed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("会话已断开")
    }
}

impl std::error::Error for SessionClosed {}

/// 一条客户端连接。
#[derive(Debug)]
pub struct Session {
    /// 会话 ID。
    pub id: SessionId,
    /// 对端地址。
    pub peer: SocketAddr,
    /// 所在区服。
    pub server: ServerId,
    /// 绑定的玩家（登录后才有）。
    player: parking_lot::RwLock<Option<PlayerId>>,
    /// 当前状态。
    state: parking_lot::RwLock<SessionState>,
    /// 下行通道。
    tx: Outbound,
    /// 建立时间。
    pub created_at: Instant,
    /// 最后一次收到消息的时间（毫秒时间戳）。
    last_seen_ms: AtomicU64,
    /// 未完成消息计数 —— 原服务端上限 100 条，超限 `Channel.close()`。
    pending: AtomicU32,
}

/// 单连接积压上限，复刻原 `NettyServerHandler` 的硬限制。
pub const MAX_PENDING: u32 = 100;

impl Session {
    /// 新建会话。
    #[must_use]
    pub fn new(id: SessionId, peer: SocketAddr, server: ServerId, tx: Outbound) -> Self {
        Self {
            id,
            peer,
            server,
            player: parking_lot::RwLock::new(None),
            state: parking_lot::RwLock::new(SessionState::Connected),
            tx,
            created_at: Instant::now(),
            last_seen_ms: AtomicU64::new(0),
            pending: AtomicU32::new(0),
        }
    }

    /// 当前状态。
    #[must_use]
    pub fn state(&self) -> SessionState {
        *self.state.read()
    }

    /// 绑定的玩家。
    #[must_use]
    pub fn player(&self) -> Option<PlayerId> {
        *self.player.read()
    }

    /// 标记为已登录。
    ///
    /// # Errors
    /// 状态机不允许该迁移。
    pub fn authenticate(&self, player: PlayerId) -> Result<(), &'static str> {
        let mut st = self.state.write();
        if !st.can_transit_to(SessionState::Authenticated) {
            return Err("当前状态不允许登录");
        }
        *self.player.write() = Some(player);
        *st = SessionState::Authenticated;
        Ok(())
    }

    /// 进入游戏。
    ///
    /// # Errors
    /// 状态机不允许该迁移。
    pub fn enter_game(&self) -> Result<(), &'static str> {
        let mut st = self.state.write();
        if !st.can_transit_to(SessionState::Playing) {
            return Err("当前状态不允许进入游戏");
        }
        *st = SessionState::Playing;
        Ok(())
    }

    /// 关闭。
    pub fn close(&self) {
        let mut st = self.state.write();
        if st.can_transit_to(SessionState::Closed) {
            *st = SessionState::Closed;
        }
    }

    /// 记录一次收包。
    pub fn touch(&self) {
        let now = now_ms();
        self.last_seen_ms.store(now, Ordering::Relaxed);
    }

    /// 空闲时长。
    #[must_use]
    pub fn idle(&self) -> Duration {
        let last = self.last_seen_ms.load(Ordering::Relaxed);
        if last == 0 {
            return self.created_at.elapsed();
        }
        Duration::from_millis(now_ms().saturating_sub(last))
    }

    /// 进入一条待处理消息；超过上限返回 `false`，调用方应断开连接。
    pub fn try_acquire_slot(&self) -> bool {
        self.pending.fetch_add(1, Ordering::Relaxed) < MAX_PENDING
    }

    /// 释放槽位。
    pub fn release_slot(&self) {
        self.pending.fetch_sub(1, Ordering::Relaxed);
    }

    /// 下发一帧。
    ///
    /// # Errors
    /// 下行通道已关闭（客户端已断开）。
    pub async fn send(&self, frame: ddj_codec::Frame) -> Result<(), SessionClosed> {
        self.tx.send(frame).await.map_err(|_| SessionClosed)
    }
}
