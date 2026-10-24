//! 登录握手回执：cmd 16 / 17 / 18。
//!
//! ## 这三个包到底是什么
//! `PlayerMessageHandler` 里的 `PlayerGetItemReq`(16)、`PlayerGetPetReq`(17)、
//! `PlayerGetMissionReq`(18) 三个方法**都是 48 字节、逐指令同构**，且都返回 `void`：
//!
//! ```text
//! ChannelPlayer cp = ChannelPlayerCache.getChannelPlayerByPlayerId(p.getId());
//! cp.setClientLoginvar(req.getItemId());          // getItemId 被复用成「密码」
//! log.debug("客户端发过来的密码：" + req.getItemId());
//! ```
//!
//! 名字叫「取道具/取宠物/取任务」，实际是客户端把 [`crate::loginvar`] 算出来的
//! 握手密码回传上来 —— 一个字段三种叫法，三种入口同一份实现。
//! 服务端留底的那份存在 `ChannelPlayer.loginvar`，唯一的消费者是活动反作弊
//! （`AbsPurposePlayPlayer.getCheatReturnData` / `addIntegral`）。
//!
//! ## Rust 侧怎么建模
//! 原版把种子和密码都挂在 `ChannelPlayer` 上（随会话生命周期）。这里用
//! [`HandshakeStore`] 存 `player_id → (挑战种子, 客户端回传密码)`，
//! 由 `game-core` 注入实现，`game-handler` 保持零 IO。

use std::collections::HashMap;
use std::sync::Mutex;

use ddj_core::PlayerId;

use crate::loginvar::derive_password;

/// `PlayerGetItemReq_16`。
pub const CMD_GET_ITEM: i32 = 16;
/// `PlayerGetPetReq_17`。
pub const CMD_GET_PET: i32 = 17;
/// `PlayerGetMissionReq_18`。
pub const CMD_GET_MISSION: i32 = 18;

/// 三个握手回执消息号。
pub const HANDSHAKE_CMDS: [i32; 3] = [CMD_GET_ITEM, CMD_GET_PET, CMD_GET_MISSION];

/// 一次握手的检查结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandshakeVerdict {
    /// 回传值与服务端算出来的一致。
    Match,
    /// 不一致 —— 原版不报错，只靠后续活动反作弊兜底。
    Mismatch,
    /// 服务端没记下挑战种子（比如重启过），无从比对。
    NoChallenge,
}

/// 握手状态。
#[derive(Debug, Clone, Copy, Default)]
pub struct Handshake {
    /// 登录时下发的挑战种子（`LoginResp_4.loginvar`）。
    pub challenge: Option<i32>,
    /// 客户端通过 16/17/18 回传的密码。
    pub client_loginvar: Option<[u8; 16]>,
}

/// 握手状态存取。
///
/// 故意做成同步接口：原版 `ChannelPlayer` 就是内存对象，不该为它引入异步。
pub trait HandshakeStore: Send + Sync {
    /// 登录成功时登记挑战种子。
    fn set_challenge(&self, player: PlayerId, challenge: i32);
    /// 客户端回传密码。
    fn set_client_password(&self, player: PlayerId, md5_hex: &str);
    /// 取当前状态。
    fn get(&self, player: PlayerId) -> Handshake;
}

/// 进程内实现（等价于原版 `ChannelPlayerCache` 的内存行为）。
#[derive(Debug, Default)]
pub struct MemoryHandshakeStore {
    rows: Mutex<HashMap<i64, Handshake>>,
}

impl MemoryHandshakeStore {
    /// 构造。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl HandshakeStore for MemoryHandshakeStore {
    fn set_challenge(&self, player: PlayerId, challenge: i32) {
        if let Ok(mut m) = self.rows.lock() {
            m.entry(player.get()).or_default().challenge = Some(challenge);
        }
    }

    fn set_client_password(&self, player: PlayerId, md5_hex: &str) {
        if let Ok(mut m) = self.rows.lock() {
            m.entry(player.get()).or_default().client_loginvar = parse_md5(md5_hex);
        }
    }

    fn get(&self, player: PlayerId) -> Handshake {
        self.rows
            .lock()
            .ok()
            .and_then(|m| m.get(&player.get()).copied())
            .unwrap_or_default()
    }
}

fn parse_md5(s: &str) -> Option<[u8; 16]> {
    if s.len() != 32 {
        return None;
    }
    let mut out = [0_u8; 16];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(s.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

/// 比对客户端回传的密码与服务端留底。
///
/// 对应原版「先存下来，等活动反作弊时再用」。这里多做一步比对，只为了在
/// 日志里能立刻看出握手有没有对上 —— 原版是静默的。
#[must_use]
pub fn verify(store: &dyn HandshakeStore, player: PlayerId) -> HandshakeVerdict {
    let st = store.get(player);
    let Some(challenge) = st.challenge else {
        return HandshakeVerdict::NoChallenge;
    };
    let Some(got) = st.client_loginvar else {
        return HandshakeVerdict::Mismatch;
    };
    let want = derive_password(challenge);
    if parse_md5(&want) == Some(got) {
        HandshakeVerdict::Match
    } else {
        HandshakeVerdict::Mismatch
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_numbers_match_proto() {
        assert_eq!(HANDSHAKE_CMDS, [16, 17, 18]);
    }

    #[test]
    fn no_challenge_yet() {
        let s = MemoryHandshakeStore::new();
        assert_eq!(verify(&s, PlayerId::new(1)), HandshakeVerdict::NoChallenge);
    }

    #[test]
    fn correct_password_matches() {
        let s = MemoryHandshakeStore::new();
        let p = PlayerId::new(7);
        s.set_challenge(p, 7796);
        // 与 loginvar.rs 的独立向量同源：seed=7796 → v=2461 → md5("11988")
        s.set_client_password(p, &derive_password(7796));
        assert_eq!(verify(&s, p), HandshakeVerdict::Match);
    }

    #[test]
    fn wrong_password_mismatches() {
        let s = MemoryHandshakeStore::new();
        let p = PlayerId::new(7);
        s.set_challenge(p, 7796);
        s.set_client_password(p, &derive_password(7823));
        assert_eq!(verify(&s, p), HandshakeVerdict::Mismatch);
    }

    #[test]
    fn missing_password_mismatches() {
        let s = MemoryHandshakeStore::new();
        let p = PlayerId::new(7);
        s.set_challenge(p, 1);
        assert_eq!(verify(&s, p), HandshakeVerdict::Mismatch);
    }

    #[test]
    fn bad_hex_is_ignored() {
        let s = MemoryHandshakeStore::new();
        let p = PlayerId::new(7);
        s.set_client_password(p, "not-a-md5");
        assert_eq!(s.get(p).client_loginvar, None);
    }

    #[test]
    fn all_three_cmds_share_one_state() {
        let s = MemoryHandshakeStore::new();
        let p = PlayerId::new(9);
        s.set_challenge(p, 9999);
        // 无论从 16 / 17 / 18 哪个入口进来，写的都是同一份状态
        for _cmd in HANDSHAKE_CMDS {
            s.set_client_password(p, &derive_password(9999));
        }
        assert_eq!(verify(&s, p), HandshakeVerdict::Match);
    }
}
