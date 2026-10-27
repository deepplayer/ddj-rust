//! # 出站宣言（`PlayerMessageHandler.MsgSetMammonDeclareReq_1018` / `MsgGetPlayerDeclareReq_1019`）
//!
//! 对应 Java：`bf/game/server/message/handler/PlayerMessageHandler.java`
//! - `MsgSetMammonDeclareReq_1018`（行 319-354）
//! - `MsgGetPlayerDeclareReq_1019`（行 355-378）
//!
//! 数据落在 `Player.manifesto`（`Player.java:287-288`，`length = BATTLE_NOTICE_SIZE`），
//! 读写走 `Player.getManifesto()` / `setManifesto()`（`Player.java:778-790`）。
//!
//! ## 1018 的分支序（不可调换）
//! ```text
//! 1. `FashionModel.isActivateFashionByType(3)` 为假
//!      → `MsgSetMammonDeclareResp_1018{ret = FORTUNE_ERROR_NO_JURISDICTION}`
//!      → **return**（这个分支不在 try 里，所以不会走到 catch）
//! 2. try 内：
//!    a. `isRelieveFraudSwitch(player)`
//!         真 → checkDirtyWord(dec, type_1, type_3)
//!         假 → checkDirtyWord(dec, type_0, type_1, type_3)
//!    b. 命中敏感词 → `DECLARE_ERROR_STRING_ILLEGAL`
//!    c. `dec.length() < MammonDeclarationSmallLimit
//!        || dec.length() > MammonDeclarationMaxLimit` → `DECLARE_ERROR_LONG_ERROR`
//!    d. 否则 `player.setManifesto(dec)` → `ret = 0`
//! 3. catch(Exception) → **`UpPlayerLvResp_1003{ret = COMMON_SERVER_BUSY}`**
//!    ⚠ 这是原版的复制粘贴 bug：1018 的 catch 回的是 1003 的包。
//! ```
//!
//! ## 1019 的结构（有一个容易漏的点）
//! ```text
//! serverId = getTargetGsServerIdOfPlayer(playerId)
//! if (serverId == 本服) {
//!     dec = toplayer.getManifesto()
//!     ret = 0
//!     player.sendMessage(builder.build())      // ← 第一次发
//! } else {
//!     try { 跨服取 dec } catch { e.printStackTrace() }   // ← 异常只打印，不设 ret
//! }
//! builder.setRet(0)                             // ← 无条件覆盖
//! player.sendMessage(builder.build())           // ← 第二次发（本服分支会收到两个包）
//! ```
//! ⚠ 本服分支**一条请求发两个包**，这是原版行为。跨服异常分支因为后面无条件
//! `setRet(0)`，最终仍然回 `ret=0`（`dec` 保持默认空串）。

use ddj_core::Result;
use ddj_pb::RetCode;

/// `ConfigDBCache.MammonDeclarationSmallLimit`（宣言最短长度）。
///
/// 来自 `ConfigDB` 表，实测由 `ConfigDBCache` 静态字段装载。
/// 过渡期先用原版默认值，接表后从 `ConfigDBCache` 读。
pub const MAMMON_DECLARATION_SMALL_LIMIT: usize = 1;

/// `ConfigDBCache.MammonDeclarationMaxLimit`（宣言最长长度）。
///
/// 真值来自 `ConfigDB` 表；`Player.manifesto` 列宽是
/// `ColiseumServerModule.BATTLE_NOTICE_SIZE`，所以这里取同一个上限。
pub const MAMMON_DECLARATION_MAX_LIMIT: usize = 60;

/// 1018 的判定输入（与请求无关的服务端状态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeclareCtx {
    /// `FashionModel.isActivateFashionByType(3)`：是否解锁「出站宣言」时装。
    pub fashion_unlocked: bool,
    /// `WxMsgSecCheck.checkDirtyWord(...)` 的结论，`true` 表示命中敏感词。
    pub dirty_word: bool,
    /// `ConfigDBCache.MammonDeclarationSmallLimit`。
    pub min_len: usize,
    /// `ConfigDBCache.MammonDeclarationMaxLimit`。
    pub max_len: usize,
}

impl Default for DeclareCtx {
    fn default() -> Self {
        Self {
            fashion_unlocked: false,
            dirty_word: false,
            min_len: MAMMON_DECLARATION_SMALL_LIMIT,
            max_len: MAMMON_DECLARATION_MAX_LIMIT,
        }
    }
}

/// 1018 的判定结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclareOutcome {
    /// 回 `MsgSetMammonDeclareResp_1018{ret}`。
    Resp(RetCode),
    /// 命中原版 catch，回的是 **`UpPlayerLvResp_1003`**（复制粘贴 bug）。
    UpPlayerLvBusy,
    /// 通过，需要把 `dec` 写进 `Player.manifesto`。
    Accept,
}

/// `PlayerMessageHandler.MsgSetMammonDeclareReq_1018` 的纯逻辑部分。
///
/// 长度按 **Java `String.length()`** 计算，即 UTF-16 code unit 数，
/// 不是 Rust 的 `chars().count()`（两者对 BMP 外字符不同）。
#[must_use]
pub fn set_declare_verdict(dec: &str, ctx: &DeclareCtx) -> DeclareOutcome {
    // 1. 时装未解锁 → 无权限（不在 try 内，不经过 catch）
    if !ctx.fashion_unlocked {
        return DeclareOutcome::Resp(RetCode::FortuneErrorNoJurisdiction);
    }
    // 2a/2b. 敏感词
    if ctx.dirty_word {
        return DeclareOutcome::Resp(RetCode::DeclareErrorStringIllegal);
    }
    // 2c. 长度（Java String.length() = UTF-16 code unit 数）
    let len = dec.encode_utf16().count();
    if len < ctx.min_len || len > ctx.max_len {
        return DeclareOutcome::Resp(RetCode::DeclareErrorLongError);
    }
    // 2d. 通过
    DeclareOutcome::Accept
}

/// 1019 的结论：回包内容 + 是否需要额外发一次（复刻本服分支的双发）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetDeclareOutcome {
    /// 回包里的 `dec`（`Player.getManifesto()`，`null` 归一成空串）。
    pub dec: String,
    /// 是否需要发两次（本服分支为 `true`）。
    pub send_twice: bool,
}

/// `PlayerMessageHandler.MsgGetPlayerDeclareReq_1019` 的纯逻辑部分。
///
/// - `same_server` = `serverId == BaseServer.getServerId()`
/// - `manifesto` = `Player.getManifesto()`（跨服异常时为 `None`）
///
/// 原版跨服分支 catch 里只有 `e.printStackTrace()`，不设 `ret`、不设 `dec`，
/// 然后被后面的 `setRet(0)` 覆盖，所以最终 `ret=0` + `dec` 为空。
#[must_use]
pub fn get_declare_outcome(same_server: bool, manifesto: Option<&str>) -> GetDeclareOutcome {
    if same_server {
        GetDeclareOutcome {
            // `getManifesto()` 把 null 归一成 ""
            dec: manifesto.unwrap_or("").to_owned(),
            // 本服分支先 sendMessage 一次，末尾又 sendMessage 一次
            send_twice: true,
        }
    } else {
        GetDeclareOutcome {
            dec: manifesto.unwrap_or("").to_owned(),
            send_twice: false,
        }
    }
}

/// `Player.setManifesto(str)` 的副作用清单（供接线层执行）。
///
/// 原版 `Player.java:785-790` 除了赋值还做三件事：
/// 1. `updatePlayerManifesto()` —— 刷新离线数据缓存；
/// 2. `PlayerOfflineDataServerModule.getPlayerOfflineData(id).setManifesto(str)`；
/// 3. `save()`。
///
/// 过渡期 `MemWorld` 只有一个 `manifesto` 字段，三处副作用等价于一次赋值。
///
/// # Errors
/// 目前不会失败；保留 `Result` 便于接库后传播。
pub fn apply_set_manifesto(dec: &str) -> Result<String> {
    Ok(dec.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> DeclareCtx {
        DeclareCtx {
            fashion_unlocked: true,
            dirty_word: false,
            ..DeclareCtx::default()
        }
    }

    #[test]
    fn fashion_locked_is_no_jurisdiction() {
        let c = DeclareCtx {
            fashion_unlocked: false,
            ..ctx()
        };
        assert_eq!(
            set_declare_verdict("你好", &c),
            DeclareOutcome::Resp(RetCode::FortuneErrorNoJurisdiction)
        );
    }

    #[test]
    fn fashion_lock_wins_over_dirty_word() {
        // 原版这个分支在 try 之前，所以哪怕有敏感词也先回无权限
        let c = DeclareCtx {
            fashion_unlocked: false,
            dirty_word: true,
            ..ctx()
        };
        assert_eq!(
            set_declare_verdict("脏话", &c),
            DeclareOutcome::Resp(RetCode::FortuneErrorNoJurisdiction)
        );
    }

    #[test]
    fn dirty_word_is_string_illegal() {
        let c = DeclareCtx {
            dirty_word: true,
            ..ctx()
        };
        assert_eq!(
            set_declare_verdict("脏话", &c),
            DeclareOutcome::Resp(RetCode::DeclareErrorStringIllegal)
        );
    }

    #[test]
    fn too_short_is_long_error() {
        let c = DeclareCtx {
            min_len: 2,
            ..ctx()
        };
        assert_eq!(
            set_declare_verdict("a", &c),
            DeclareOutcome::Resp(RetCode::DeclareErrorLongError)
        );
    }

    #[test]
    fn too_long_is_long_error() {
        let c = DeclareCtx {
            max_len: 3,
            ..ctx()
        };
        assert_eq!(
            set_declare_verdict("abcd", &c),
            DeclareOutcome::Resp(RetCode::DeclareErrorLongError)
        );
    }

    #[test]
    fn boundary_lengths_are_accepted() {
        let c = DeclareCtx {
            min_len: 2,
            max_len: 4,
            ..ctx()
        };
        assert_eq!(set_declare_verdict("ab", &c), DeclareOutcome::Accept);
        assert_eq!(set_declare_verdict("abcd", &c), DeclareOutcome::Accept);
    }

    #[test]
    fn length_uses_utf16_code_units_like_java() {
        // Java `String.length()` 对 BMP 外字符返回 2（代理对）。
        // 「𝄞」U+1D11E 在 Java 里 length()==2，Rust chars().count()==1。
        let c = DeclareCtx {
            min_len: 2,
            max_len: 2,
            ..ctx()
        };
        assert_eq!(set_declare_verdict("𝄞", &c), DeclareOutcome::Accept);
        // 如果用 chars().count() 会得到 1 < min_len → 误判为长度错误
    }

    #[test]
    fn get_declare_same_server_sends_twice() {
        let out = get_declare_outcome(true, Some("我的宣言"));
        assert_eq!(out.dec, "我的宣言");
        assert!(out.send_twice, "本服分支原版发两次");
    }

    #[test]
    fn get_declare_cross_server_sends_once() {
        let out = get_declare_outcome(false, Some("对面的宣言"));
        assert_eq!(out.dec, "对面的宣言");
        assert!(!out.send_twice);
    }

    #[test]
    fn get_declare_null_manifesto_becomes_empty() {
        assert_eq!(get_declare_outcome(true, None).dec, "");
        assert_eq!(get_declare_outcome(false, None).dec, "");
    }

    #[test]
    fn cross_server_failure_still_returns_ok() {
        // 原版跨服 catch 只 printStackTrace，末尾 setRet(0) 覆盖 → ret=0 + dec=""
        let out = get_declare_outcome(false, None);
        assert_eq!(out.dec, "");
        assert!(!out.send_twice);
    }
}
