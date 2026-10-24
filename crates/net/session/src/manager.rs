//! 全服会话表
//!
//! 对应 Java：`SessionManager` 静态单例 + `ChannelGroup`。
//!
//! 职责：
//! - 分配 `SessionId`
//! - 按玩家查会话（踢重复登录）
//! - 广播
//! - 定期清理超时会话

use crate::session::{Session, SessionId};
use dashmap::DashMap;
use ddj_core::PlayerId;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// 会话管理器。
#[derive(Debug, Default)]
pub struct SessionManager {
    by_id: DashMap<SessionId, Arc<Session>>,
    by_player: DashMap<PlayerId, SessionId>,
    next_id: AtomicU64,
}

impl SessionManager {
    /// 新建。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册一条新连接。
    pub fn register(
        &self,
        peer: SocketAddr,
        server: ddj_core::ServerId,
        tx: crate::session::Outbound,
    ) -> Arc<Session> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let s = Arc::new(Session::new(id, peer, server, tx));
        self.by_id.insert(id, Arc::clone(&s));
        s
    }

    /// 按 ID 取。
    #[must_use]
    pub fn get(&self, id: SessionId) -> Option<Arc<Session>> {
        self.by_id.get(&id).map(|r| Arc::clone(r.value()))
    }

    /// 按玩家取。
    #[must_use]
    pub fn of_player(&self, player: PlayerId) -> Option<Arc<Session>> {
        self.by_player
            .get(&player)
            .and_then(|sid| self.get(*sid.value()))
    }

    /// 绑定玩家索引（登录成功后调用）。
    pub fn bind_player(&self, player: PlayerId, session: SessionId) {
        self.by_player.insert(player, session);
    }

    /// 移出并对会话做最终清理。
    pub fn remove(&self, id: SessionId) -> Option<Arc<Session>> {
        let (_, s) = self.by_id.remove(&id)?;
        if let Some(p) = s.player() {
            self.by_player.remove(&p);
        }
        s.close();
        Some(s)
    }

    /// 在线数。
    #[must_use]
    pub fn online(&self) -> usize {
        self.by_id.len()
    }

    /// 全服广播。
    pub async fn broadcast(&self, frame: ddj_codec::Frame) {
        let targets: Vec<Arc<Session>> = self.by_id.iter().map(|r| Arc::clone(r.value())).collect();
        for s in targets {
            let _ = s.send(frame.clone()).await;
        }
    }

    /// 清理空闲超过 `timeout` 的会话，返回被清理的 ID。
    pub fn reap_idle(&self, timeout: std::time::Duration) -> Vec<SessionId> {
        let stale: Vec<SessionId> = self
            .by_id
            .iter()
            .filter(|r| r.value().idle() > timeout)
            .map(|r| *r.key())
            .collect();
        for id in &stale {
            self.remove(*id);
        }
        stale
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    fn mgr() -> SessionManager {
        SessionManager::new()
    }

    #[test]
    fn register_and_remove() {
        let m = mgr();
        let (tx, _rx) = mpsc::channel(8);
        let s = m.register("1.2.3.4:5".parse().unwrap(), ddj_core::ServerId::new(1), tx);
        assert_eq!(m.online(), 1);
        let removed = m.remove(s.id).expect("存在");
        assert_eq!(removed.state(), crate::SessionState::Closed);
        assert_eq!(m.online(), 0);
    }
}
