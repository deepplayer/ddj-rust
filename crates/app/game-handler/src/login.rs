//! 登录处理器
//!
//! 对应 Java：`LoginMessageHandler`。
//!
//! ## 原白名单（免登录可发的 cmdId）
//! `3, 4, 0, 5, 6, 7, 98, 11`
//!
//! ## 加密
//! **只有 `msgId == 4`（登录）走加密**：`XXeta.decrypt(body, SIGN_KEY)` 后
//! 再 `Base64.decode`。`SIGN_KEY` 是写死在服务端与客户端的同一串：
//! `{48,53,58,38,40,41,94,38,42,97,98,100,100,122,77,81}`。

use crate::macros::{Ctx, Res};

/// 登录（`msgId = 4`，**唯一加密消息**）。
///
/// # Errors
/// 解密失败 / 账号密码错误。
pub fn login(ctx: &Ctx, account: &str, _password: &str) -> Res<Option<LoginResp>> {
    // 密码不落日志：只记账号与来源 IP。
    tracing::info!(peer = %ctx.peer_ip, account, "登录请求（明文已解密）");
    Ok(Some(LoginResp {
        ret: 0,
        player_id: 0,
        note: "骨架：等待接入 PlatformService".into(),
    }))
}

/// 登录响应。
#[derive(Debug, Clone, Default)]
pub struct LoginResp {
    /// 0 = 成功。
    pub ret: i32,
    /// 角色 ID。
    pub player_id: i64,
    /// 调试信息。
    pub note: String,
}

/// 心跳（`msgId = 5`）。
///
/// 原服务端只回一个时间戳，不校验频率 —— 这是断线检测的唯一依据。
#[must_use]
pub fn heartbeat(now_ms: i64) -> HeartbeatResp {
    HeartbeatResp { server_ms: now_ms }
}

/// 心跳响应。
#[derive(Debug, Clone, Copy)]
pub struct HeartbeatResp {
    /// 服务器毫秒时间戳。
    pub server_ms: i64,
}

/// 区服列表（`msgId = 3`）。
#[must_use]
pub fn server_list() -> ServerListResp {
    ServerListResp {
        servers: Vec::new(),
    }
}

/// 区服条目。
#[derive(Debug, Clone)]
pub struct ServerEntry {
    /// 区服 ID。
    pub id: i32,
    /// 名称。
    pub name: String,
    /// 状态（0 维护 / 1 流畅 / 2 繁忙 / 3 爆满）。
    pub state: i32,
}

/// 区服列表响应。
#[derive(Debug, Clone, Default)]
pub struct ServerListResp {
    /// 区服。
    pub servers: Vec<ServerEntry>,
}
